//! Application orchestration contracts shared by desktop, CLI, and MCP.
//!
//! The implementation here is intentionally small: it validates safe request
//! shapes and delegates platform work to ports. It does not contain a Windows
//! registry or shell implementation.

use cxszn_menv_domain::{
    validate_activation_transition, Activation, EnvironmentChangeAction, EnvironmentSnapshot,
    OperationStatus, RuntimeFamily, RuntimeId,
};
use cxszn_menv_ports::{
    EnvironmentStore, PathLease, PortError, RuntimeProvider, ScanRequest, UserPathStore,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use uuid::Uuid;

const PLAN_TTL_SECONDS: u64 = 300;
const DEFAULT_SHIM_DIRECTORY: &str = r"C:\Users\Current\AppData\Local\cxszn-menv\bin";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChangeAction {
    Register,
    Install,
    Enable,
    Disable,
    Switch,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeRequest {
    pub action: ChangeAction,
    pub family: RuntimeFamily,
    pub runtime_id: Option<RuntimeId>,
    pub target_runtime_id: Option<RuntimeId>,
    pub idempotency_key: String,
    pub actor: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePlan {
    pub plan_id: String,
    pub plan_hash: String,
    pub request: ChangeRequest,
    pub requires_confirmation: bool,
    pub created_at_epoch_seconds: u64,
    pub expires_at_epoch_seconds: u64,
    pub snapshot_hash: String,
}

/// Host-issued confirmation material.  The opaque value is never logged or
/// persisted by this crate; all public fields are bound to the plan before a
/// mutation is attempted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmationToken {
    pub value: String,
    pub plan_id: String,
    pub plan_hash: String,
    pub actor: String,
    pub expires_at_epoch_seconds: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationReceipt {
    pub operation_id: String,
    pub status: OperationStatus,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum ApplicationError {
    #[error("idempotency key is required")]
    MissingIdempotencyKey,
    #[error("actor is required")]
    MissingActor,
    #[error("confirmation token is required for this operation")]
    ConfirmationRequired,
    #[error("confirmation token is invalid or not bound to this plan")]
    ConfirmationInvalid,
    #[error("change plan has expired")]
    PlanExpired,
    #[error("change plan hash is invalid")]
    PlanTampered,
    #[error("environment changed after the plan was created")]
    StateChanged,
    #[error("change action is not implemented by this native service")]
    UnsupportedChangeAction,
    #[error("runtime id is required for this operation")]
    MissingRuntimeId,
    #[error("target runtime id is required for a switch")]
    MissingTargetRuntimeId,
    #[error("runtime was not found")]
    RuntimeNotFound,
    #[error("runtime family does not match the requested family")]
    RuntimeFamilyMismatch,
    #[error("idempotency key was already used for another plan")]
    IdempotencyConflict,
    #[error("invalid activation transition")]
    InvalidActivationTransition,
    #[error("serialization failed: {0}")]
    Serialization(String),
    #[error("port error: {0}")]
    Port(#[from] PortError),
}

pub trait ApplicationService: Send + Sync {
    fn get_environment_state(&self) -> Result<EnvironmentSnapshot, ApplicationError>;
    fn scan_installed(
        &self,
        family: Option<RuntimeFamily>,
    ) -> Result<EnvironmentSnapshot, ApplicationError>;
    fn plan_change(&self, request: ChangeRequest) -> Result<ChangePlan, ApplicationError>;
    fn apply_change(
        &self,
        plan: &ChangePlan,
        confirmation_token: Option<&ConfirmationToken>,
    ) -> Result<OperationReceipt, ApplicationError>;
}

/// Thin orchestrator used by all adapters. Concrete providers and storage are
/// injected by the platform composition root.
pub struct Orchestrator<S, P = FailClosedUserPathStore> {
    store: S,
    path_store: P,
    shim_directory: PathBuf,
}

/// Safe default used when a platform path adapter is unavailable.  Mutating
/// calls fail closed instead of silently claiming a PATH change succeeded.
#[derive(Clone, Copy, Debug, Default)]
pub struct FailClosedUserPathStore;

impl UserPathStore for FailClosedUserPathStore {
    fn read(&self) -> Result<Vec<String>, PortError> {
        Err(PortError::UnsupportedPlatform)
    }

    fn apply_lease(&self, _lease: &PathLease) -> Result<(), PortError> {
        Err(PortError::UnsupportedPlatform)
    }

    fn remove_lease(&self, _lease: &PathLease) -> Result<(), PortError> {
        Err(PortError::UnsupportedPlatform)
    }
}

impl<S> Orchestrator<S, FailClosedUserPathStore> {
    pub fn new(store: S) -> Self {
        Self::with_path_store(store, FailClosedUserPathStore)
    }
}

impl<S, P> Orchestrator<S, P> {
    pub fn with_path_store(store: S, path_store: P) -> Self {
        Self {
            store,
            path_store,
            shim_directory: PathBuf::from(DEFAULT_SHIM_DIRECTORY),
        }
    }

    pub fn with_shim_directory(mut self, shim_directory: PathBuf) -> Self {
        self.shim_directory = shim_directory;
        self
    }
}

impl<S, P> Orchestrator<S, P>
where
    S: EnvironmentStore,
    P: UserPathStore,
{
    pub fn get_environment_state(&self) -> Result<EnvironmentSnapshot, ApplicationError> {
        Ok(self.store.load()?)
    }

    pub fn scan_with_provider<Q: RuntimeProvider>(
        &self,
        provider: &Q,
        request: ScanRequest,
    ) -> Result<EnvironmentSnapshot, ApplicationError> {
        let mut snapshot = self.store.load()?;
        let records = provider.scan(&request)?;
        snapshot
            .runtimes
            .retain(|item| item.family != provider.family());
        snapshot.runtimes.extend(records);
        self.store.save(&snapshot)?;
        Ok(snapshot)
    }

    pub fn plan_change(&self, request: ChangeRequest) -> Result<ChangePlan, ApplicationError> {
        if request.idempotency_key.trim().is_empty() {
            return Err(ApplicationError::MissingIdempotencyKey);
        }
        if request.actor.trim().is_empty() {
            return Err(ApplicationError::MissingActor);
        }
        match request.action {
            ChangeAction::Enable | ChangeAction::Disable => {
                if request.runtime_id.is_none() {
                    return Err(ApplicationError::MissingRuntimeId);
                }
            }
            ChangeAction::Switch => {
                if request.runtime_id.is_none() {
                    return Err(ApplicationError::MissingRuntimeId);
                }
                if request.target_runtime_id.is_none() {
                    return Err(ApplicationError::MissingTargetRuntimeId);
                }
            }
            ChangeAction::Register | ChangeAction::Install => {
                return Err(ApplicationError::UnsupportedChangeAction)
            }
        }
        let snapshot = self.store.load()?;
        if matches!(
            request.action,
            ChangeAction::Enable | ChangeAction::Disable | ChangeAction::Switch
        ) {
            // The Windows adapter records this read as the expected PATH
            // snapshot.  A later apply therefore rejects external edits
            // instead of overwriting them.
            self.path_store.read()?;
        }
        if let Some(runtime_id) = &request.runtime_id {
            let runtime = snapshot
                .runtimes
                .iter()
                .find(|runtime| &runtime.id == runtime_id)
                .ok_or(ApplicationError::RuntimeNotFound)?;
            if runtime.family != request.family {
                return Err(ApplicationError::RuntimeFamilyMismatch);
            }
        }
        if let Some(target_runtime_id) = &request.target_runtime_id {
            let target = snapshot
                .runtimes
                .iter()
                .find(|runtime| &runtime.id == target_runtime_id)
                .ok_or(ApplicationError::RuntimeNotFound)?;
            if target.family != request.family {
                return Err(ApplicationError::RuntimeFamilyMismatch);
            }
        }
        let created_at_epoch_seconds = now_epoch_seconds();
        let expires_at_epoch_seconds = created_at_epoch_seconds + PLAN_TTL_SECONDS;
        let plan_id = Uuid::new_v4().to_string();
        let snapshot_hash = hash_snapshot(&snapshot)?;
        let mut plan = ChangePlan {
            plan_id,
            plan_hash: String::new(),
            request,
            requires_confirmation: true,
            created_at_epoch_seconds,
            expires_at_epoch_seconds,
            snapshot_hash,
        };
        plan.plan_hash = hash_plan(&plan)?;
        Ok(plan)
    }

    pub fn apply_change(
        &self,
        plan: &ChangePlan,
        confirmation_token: Option<&ConfirmationToken>,
    ) -> Result<OperationReceipt, ApplicationError> {
        if plan.plan_hash != hash_plan(plan)? {
            return Err(ApplicationError::PlanTampered);
        }
        let now = now_epoch_seconds();
        if now >= plan.expires_at_epoch_seconds {
            return Err(ApplicationError::PlanExpired);
        }
        if plan.requires_confirmation && confirmation_token.is_none() {
            return Err(ApplicationError::ConfirmationRequired);
        }
        if let Some(token) = confirmation_token {
            if token.value.trim().is_empty()
                || token.plan_id != plan.plan_id
                || token.plan_hash != plan.plan_hash
                || token.actor != plan.request.actor
                || token.expires_at_epoch_seconds != plan.expires_at_epoch_seconds
                || token.expires_at_epoch_seconds <= now
            {
                return Err(ApplicationError::ConfirmationInvalid);
            }
        }

        let mut snapshot = self.store.load()?;
        if let Some(existing) = snapshot
            .operations
            .iter()
            .find(|operation| operation.idempotency_key == plan.request.idempotency_key)
        {
            if existing.plan_id != plan.plan_id {
                return Err(ApplicationError::IdempotencyConflict);
            }
            return Ok(OperationReceipt {
                operation_id: existing.operation_id.clone(),
                status: existing.status.clone(),
            });
        }
        if hash_snapshot(&snapshot)? != plan.snapshot_hash {
            return Err(ApplicationError::StateChanged);
        }
        let operation_action = operation_action(&plan.request.action)?;
        let runtime_id = plan
            .request
            .target_runtime_id
            .clone()
            .or_else(|| plan.request.runtime_id.clone())
            .ok_or(ApplicationError::MissingRuntimeId)?;
        let runtime = snapshot
            .runtimes
            .iter()
            .find(|runtime| runtime.id == runtime_id)
            .ok_or(ApplicationError::RuntimeNotFound)?;
        if runtime.family != plan.request.family {
            return Err(ApplicationError::RuntimeFamilyMismatch);
        }
        let before = snapshot.clone();
        let lease = PathLease {
            lease_id: "menv-user-path".to_owned(),
            shim_directory: self.shim_directory.clone(),
            target_executable: Some(PathBuf::from(&runtime.executable_path)),
        };
        let path_applied = match plan.request.action {
            ChangeAction::Enable | ChangeAction::Switch => {
                self.path_store.apply_lease(&lease)?;
                true
            }
            ChangeAction::Disable => {
                self.path_store.remove_lease(&lease)?;
                true
            }
            ChangeAction::Register | ChangeAction::Install => false,
        };
        let path_entries = if path_applied {
            match self.path_store.read() {
                Ok(entries) => Some(entries),
                Err(error) => {
                    self.rollback_path_lease(&plan.request.action, &lease);
                    return Err(ApplicationError::Port(error));
                }
            }
        } else {
            None
        };
        let operation_id = Uuid::new_v4().to_string();
        let now_iso = unix_epoch_iso(now);
        if let Some(entries) = path_entries {
            snapshot.path_lease.entries = entries;
            snapshot.path_lease.hash = hash_path_entries(&snapshot.path_lease.entries);
            snapshot.path_lease.enabled = matches!(
                plan.request.action,
                ChangeAction::Enable | ChangeAction::Switch
            );
            snapshot.path_lease.path = if snapshot.path_lease.enabled {
                Some(lease.shim_directory.to_string_lossy().to_string())
            } else {
                None
            };
            snapshot.path_lease.runtime_id = if snapshot.path_lease.enabled {
                Some(runtime_id.clone())
            } else {
                None
            };
            snapshot.path_lease.updated_at = Some(now_iso.clone());
        }
        if let Err(error) = update_snapshot_after_change(
            &mut snapshot,
            &plan.request,
            operation_id.clone(),
            operation_action,
            plan.plan_id.clone(),
            now_iso.clone(),
        ) {
            self.rollback_path_lease(&plan.request.action, &lease);
            return Err(error);
        }
        if let Err(error) = self.store.save(&snapshot) {
            if path_applied {
                let _ = match plan.request.action {
                    ChangeAction::Enable | ChangeAction::Switch => {
                        self.path_store.remove_lease(&lease)
                    }
                    ChangeAction::Disable => self.path_store.apply_lease(&lease),
                    ChangeAction::Register | ChangeAction::Install => Ok(()),
                };
            }
            let _ = self.store.save(&before);
            return Err(ApplicationError::Port(error));
        }
        Ok(OperationReceipt {
            operation_id,
            status: OperationStatus::Succeeded,
        })
    }

    fn rollback_path_lease(&self, action: &ChangeAction, lease: &PathLease) {
        let _ = match action {
            ChangeAction::Enable | ChangeAction::Switch => self.path_store.remove_lease(lease),
            ChangeAction::Disable => self.path_store.apply_lease(lease),
            ChangeAction::Register | ChangeAction::Install => Ok(()),
        };
    }
}

impl<S, P> ApplicationService for Orchestrator<S, P>
where
    S: EnvironmentStore,
    P: UserPathStore,
{
    fn get_environment_state(&self) -> Result<EnvironmentSnapshot, ApplicationError> {
        Orchestrator::get_environment_state(self)
    }

    fn scan_installed(
        &self,
        _family: Option<RuntimeFamily>,
    ) -> Result<EnvironmentSnapshot, ApplicationError> {
        // Provider-specific discovery is wired by the platform composition
        // root through `scan_with_provider`; this method remains read-only
        // until a provider is selected.
        Orchestrator::get_environment_state(self)
    }

    fn plan_change(&self, request: ChangeRequest) -> Result<ChangePlan, ApplicationError> {
        Orchestrator::plan_change(self, request)
    }

    fn apply_change(
        &self,
        plan: &ChangePlan,
        confirmation_token: Option<&ConfirmationToken>,
    ) -> Result<OperationReceipt, ApplicationError> {
        Orchestrator::apply_change(self, plan, confirmation_token)
    }
}

fn operation_action(action: &ChangeAction) -> Result<EnvironmentChangeAction, ApplicationError> {
    match action {
        ChangeAction::Enable => Ok(EnvironmentChangeAction::Enable),
        ChangeAction::Disable => Ok(EnvironmentChangeAction::Disable),
        ChangeAction::Switch => Ok(EnvironmentChangeAction::Switch),
        ChangeAction::Register | ChangeAction::Install => {
            Err(ApplicationError::UnsupportedChangeAction)
        }
    }
}

fn update_snapshot_after_change(
    snapshot: &mut EnvironmentSnapshot,
    request: &ChangeRequest,
    operation_id: String,
    action: EnvironmentChangeAction,
    plan_id: String,
    timestamp: String,
) -> Result<(), ApplicationError> {
    let runtime_id = request
        .target_runtime_id
        .clone()
        .or_else(|| request.runtime_id.clone())
        .ok_or(ApplicationError::MissingRuntimeId)?;

    for runtime in &mut snapshot.runtimes {
        let desired = match request.action {
            ChangeAction::Enable => {
                if runtime.id == runtime_id {
                    Some(Activation::Active)
                } else {
                    None
                }
            }
            ChangeAction::Disable => {
                if runtime.id == runtime_id {
                    Some(Activation::Disabled)
                } else {
                    None
                }
            }
            ChangeAction::Switch => {
                if runtime.id == runtime_id {
                    Some(Activation::Active)
                } else if runtime.activation == Activation::Active {
                    Some(Activation::Disabled)
                } else {
                    None
                }
            }
            ChangeAction::Register | ChangeAction::Install => None,
        };
        if let Some(desired) = desired {
            if runtime.activation != desired {
                if runtime.activation == Activation::Failed && desired == Activation::Disabled {
                    validate_activation_transition(runtime.activation.clone(), desired.clone())
                        .map_err(|_| ApplicationError::InvalidActivationTransition)?;
                } else {
                    let intermediate = match desired {
                        Activation::Active => Activation::Enabling,
                        Activation::Disabled => Activation::Disabling,
                        _ => desired.clone(),
                    };
                    if runtime.activation != intermediate {
                        validate_activation_transition(
                            runtime.activation.clone(),
                            intermediate.clone(),
                        )
                        .map_err(|_| ApplicationError::InvalidActivationTransition)?;
                    }
                    if intermediate != desired {
                        validate_activation_transition(intermediate, desired.clone())
                            .map_err(|_| ApplicationError::InvalidActivationTransition)?;
                    }
                }
                runtime.activation = desired;
            }
        }
    }

    snapshot.updated_at = timestamp.clone();
    snapshot
        .operations
        .push(cxszn_menv_domain::OperationRecord {
            operation_id,
            plan_id,
            idempotency_key: request.idempotency_key.clone(),
            action,
            runtime_id,
            status: OperationStatus::Succeeded,
            created_at: timestamp.clone(),
            updated_at: timestamp,
            error: None,
        });
    Ok(())
}

fn now_epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn unix_epoch_iso(seconds: u64) -> String {
    format!("{seconds}Z")
}

fn hash_snapshot(snapshot: &EnvironmentSnapshot) -> Result<String, ApplicationError> {
    let bytes = serde_json::to_vec(snapshot)
        .map_err(|error| ApplicationError::Serialization(error.to_string()))?;
    Ok(hash_bytes(&bytes))
}

fn hash_plan(plan: &ChangePlan) -> Result<String, ApplicationError> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct CanonicalPlan<'a> {
        plan_id: &'a str,
        request: &'a ChangeRequest,
        requires_confirmation: bool,
        created_at_epoch_seconds: u64,
        expires_at_epoch_seconds: u64,
        snapshot_hash: &'a str,
    }
    let canonical = CanonicalPlan {
        plan_id: &plan.plan_id,
        request: &plan.request,
        requires_confirmation: plan.requires_confirmation,
        created_at_epoch_seconds: plan.created_at_epoch_seconds,
        expires_at_epoch_seconds: plan.expires_at_epoch_seconds,
        snapshot_hash: &plan.snapshot_hash,
    };
    let bytes = serde_json::to_vec(&canonical)
        .map_err(|error| ApplicationError::Serialization(error.to_string()))?;
    Ok(hash_bytes(&bytes))
}

fn hash_bytes(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn hash_path_entries(entries: &[String]) -> String {
    hash_bytes(entries.join(";").as_bytes())
}

pub fn validate_activation_change(
    from: Activation,
    to: Activation,
) -> Result<(), ApplicationError> {
    validate_activation_transition(from, to)
        .map_err(|_| ApplicationError::InvalidActivationTransition)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cxszn_menv_domain::{
        Architecture, Health, PhpThreadSafety, RuntimeIniInfo, RuntimeIniSource, RuntimeRecord,
    };
    use cxszn_menv_ports::{EmptyEnvironmentStore, PathLease, PortError, UserPathStore};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct TestPathStore {
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl TestPathStore {
        fn calls(&self) -> Vec<String> {
            self.calls.lock().expect("calls lock").clone()
        }
    }

    impl UserPathStore for TestPathStore {
        fn read(&self) -> Result<Vec<String>, PortError> {
            Ok(Vec::new())
        }

        fn apply_lease(&self, lease: &PathLease) -> Result<(), PortError> {
            self.calls
                .lock()
                .expect("calls lock")
                .push(format!("apply:{}", lease.lease_id));
            Ok(())
        }

        fn remove_lease(&self, lease: &PathLease) -> Result<(), PortError> {
            self.calls
                .lock()
                .expect("calls lock")
                .push(format!("remove:{}", lease.lease_id));
            Ok(())
        }
    }

    fn test_snapshot() -> cxszn_menv_domain::EnvironmentSnapshot {
        let id = RuntimeId::new("php-8.3-x64").expect("valid id");
        let mut snapshot = cxszn_menv_domain::EnvironmentSnapshot::default();
        snapshot.runtimes.push(RuntimeRecord {
            id,
            family: RuntimeFamily::Php,
            version: Some("8.3.0".to_owned()),
            executable_path: r"C:\php\php.exe".to_owned(),
            root_path: r"C:\php".to_owned(),
            architecture: Architecture::X64,
            thread_safety: PhpThreadSafety::Nts,
            ownership: cxszn_menv_domain::Ownership::Manual,
            availability: cxszn_menv_domain::Availability::Manual,
            activation: Activation::Disabled,
            health: Health::Unknown,
            ini: RuntimeIniInfo {
                source: RuntimeIniSource::Unknown,
                path: None,
            },
            legacy: false,
            diagnostics: Vec::new(),
            first_seen_at: "2026-10-12T00:00:00Z".to_owned(),
            last_checked_at: "2026-10-12T00:00:00Z".to_owned(),
        });
        snapshot
    }

    #[derive(Clone)]
    struct TestEnvironmentStore {
        snapshot: Arc<Mutex<cxszn_menv_domain::EnvironmentSnapshot>>,
    }

    impl EnvironmentStore for TestEnvironmentStore {
        fn load(&self) -> Result<cxszn_menv_domain::EnvironmentSnapshot, PortError> {
            Ok(self.snapshot.lock().expect("snapshot lock").clone())
        }

        fn save(&self, snapshot: &cxszn_menv_domain::EnvironmentSnapshot) -> Result<(), PortError> {
            *self.snapshot.lock().expect("snapshot lock") = snapshot.clone();
            Ok(())
        }
    }

    #[test]
    fn write_plan_requires_idempotency_key() {
        let orchestrator = Orchestrator::new(EmptyEnvironmentStore);
        let result = orchestrator.plan_change(ChangeRequest {
            action: ChangeAction::Enable,
            family: RuntimeFamily::Php,
            runtime_id: None,
            target_runtime_id: None,
            idempotency_key: String::new(),
            actor: "test".to_owned(),
        });
        assert_eq!(result, Err(ApplicationError::MissingIdempotencyKey));
    }

    #[test]
    fn apply_requires_confirmation() {
        let orchestrator = Orchestrator::with_path_store(
            TestEnvironmentStore {
                snapshot: Arc::new(Mutex::new(test_snapshot())),
            },
            TestPathStore {
                calls: Arc::new(Mutex::new(Vec::new())),
            },
        );
        let plan = orchestrator
            .plan_change(ChangeRequest {
                action: ChangeAction::Enable,
                family: RuntimeFamily::Php,
                runtime_id: Some(RuntimeId::new("php-8.3-x64").expect("valid id")),
                target_runtime_id: None,
                idempotency_key: "retry-me".to_owned(),
                actor: "test".to_owned(),
            })
            .expect("plan succeeds");
        assert_eq!(
            orchestrator.apply_change(&plan, None),
            Err(ApplicationError::ConfirmationRequired)
        );
    }

    #[test]
    fn plan_hash_is_canonical_and_has_expiry_and_state_binding() {
        let store = TestEnvironmentStore {
            snapshot: Arc::new(Mutex::new(test_snapshot())),
        };
        let path_store = TestPathStore {
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let orchestrator = Orchestrator::with_path_store(store, path_store)
            .with_shim_directory(PathBuf::from(r"C:\Users\demo\AppData\Local\cxszn-menv\bin"));
        let plan = orchestrator
            .plan_change(ChangeRequest {
                action: ChangeAction::Enable,
                family: RuntimeFamily::Php,
                runtime_id: Some(RuntimeId::new("php-8.3-x64").expect("valid id")),
                target_runtime_id: None,
                idempotency_key: "hash-test".to_owned(),
                actor: "user".to_owned(),
            })
            .expect("plan succeeds");

        assert_eq!(plan.plan_hash.len(), 64);
        assert!(plan.expires_at_epoch_seconds > plan.created_at_epoch_seconds);
        assert_eq!(plan.snapshot_hash.len(), 64);
        assert_ne!(plan.plan_hash, format!("plan:{}", plan.plan_id));
    }

    #[test]
    fn apply_requires_a_typed_token_bound_to_the_plan_and_actor() {
        let store = TestEnvironmentStore {
            snapshot: Arc::new(Mutex::new(test_snapshot())),
        };
        let path_store = TestPathStore {
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let orchestrator = Orchestrator::with_path_store(store, path_store);
        let plan = orchestrator
            .plan_change(ChangeRequest {
                action: ChangeAction::Enable,
                family: RuntimeFamily::Php,
                runtime_id: Some(RuntimeId::new("php-8.3-x64").expect("valid id")),
                target_runtime_id: None,
                idempotency_key: "token-test".to_owned(),
                actor: "user".to_owned(),
            })
            .expect("plan succeeds");

        let wrong_actor = ConfirmationToken {
            value: "host-issued".to_owned(),
            plan_id: plan.plan_id.clone(),
            plan_hash: plan.plan_hash.clone(),
            actor: "other".to_owned(),
            expires_at_epoch_seconds: plan.expires_at_epoch_seconds,
        };
        assert_eq!(
            orchestrator.apply_change(&plan, Some(&wrong_actor)),
            Err(ApplicationError::ConfirmationInvalid)
        );
    }

    #[test]
    fn apply_mutates_snapshot_and_path_lease_and_is_idempotent() {
        let state = Arc::new(Mutex::new(test_snapshot()));
        let store = TestEnvironmentStore {
            snapshot: Arc::clone(&state),
        };
        let path_store = TestPathStore {
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let path_calls = path_store.clone();
        let orchestrator = Orchestrator::with_path_store(store, path_store)
            .with_shim_directory(PathBuf::from(r"C:\Users\demo\AppData\Local\cxszn-menv\bin"));
        let plan = orchestrator
            .plan_change(ChangeRequest {
                action: ChangeAction::Enable,
                family: RuntimeFamily::Php,
                runtime_id: Some(RuntimeId::new("php-8.3-x64").expect("valid id")),
                target_runtime_id: None,
                idempotency_key: "apply-test".to_owned(),
                actor: "user".to_owned(),
            })
            .expect("plan succeeds");
        let token = ConfirmationToken {
            value: "host-issued".to_owned(),
            plan_id: plan.plan_id.clone(),
            plan_hash: plan.plan_hash.clone(),
            actor: "user".to_owned(),
            expires_at_epoch_seconds: plan.expires_at_epoch_seconds,
        };

        let receipt = orchestrator
            .apply_change(&plan, Some(&token))
            .expect("apply succeeds");
        assert_eq!(receipt.status, OperationStatus::Succeeded);
        let persisted = state.lock().expect("snapshot lock").clone();
        assert_eq!(persisted.runtimes[0].activation, Activation::Active);
        assert!(persisted.path_lease.enabled);
        assert_eq!(
            persisted
                .path_lease
                .runtime_id
                .as_ref()
                .map(RuntimeId::as_str),
            Some("php-8.3-x64")
        );
        assert_eq!(path_calls.calls(), vec!["apply:menv-user-path".to_owned()]);

        let replay = orchestrator
            .apply_change(&plan, Some(&token))
            .expect("idempotent replay succeeds");
        assert_eq!(replay, receipt);
        assert_eq!(path_calls.calls(), vec!["apply:menv-user-path".to_owned()]);
    }

    #[test]
    fn apply_rejects_tampered_and_expired_plans_before_touching_path() {
        let state = Arc::new(Mutex::new(test_snapshot()));
        let path_store = TestPathStore {
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let path_calls = path_store.clone();
        let orchestrator = Orchestrator::with_path_store(
            TestEnvironmentStore {
                snapshot: Arc::clone(&state),
            },
            path_store,
        );
        let plan = orchestrator
            .plan_change(ChangeRequest {
                action: ChangeAction::Enable,
                family: RuntimeFamily::Php,
                runtime_id: Some(RuntimeId::new("php-8.3-x64").expect("valid id")),
                target_runtime_id: None,
                idempotency_key: "tamper-test".to_owned(),
                actor: "user".to_owned(),
            })
            .expect("plan succeeds");
        let mut tampered = plan.clone();
        tampered.request.actor = "attacker".to_owned();
        let token = ConfirmationToken {
            value: "host-issued".to_owned(),
            plan_id: plan.plan_id.clone(),
            plan_hash: plan.plan_hash.clone(),
            actor: "user".to_owned(),
            expires_at_epoch_seconds: plan.expires_at_epoch_seconds,
        };
        assert_eq!(
            orchestrator.apply_change(&tampered, Some(&token)),
            Err(ApplicationError::PlanTampered)
        );

        let mut expired = plan;
        expired.expires_at_epoch_seconds = expired.created_at_epoch_seconds;
        expired.plan_hash = hash_plan(&expired).expect("rehashed expired plan");
        assert_eq!(
            orchestrator.apply_change(&expired, Some(&token)),
            Err(ApplicationError::PlanExpired)
        );
        assert!(path_calls.calls().is_empty());
        assert_eq!(
            state.lock().expect("snapshot lock").runtimes[0].activation,
            Activation::Disabled
        );
    }

    #[test]
    fn default_path_store_fails_closed_for_real_mutations() {
        let state = Arc::new(Mutex::new(test_snapshot()));
        let orchestrator = Orchestrator::new(TestEnvironmentStore { snapshot: state });
        let result = orchestrator.plan_change(ChangeRequest {
            action: ChangeAction::Enable,
            family: RuntimeFamily::Php,
            runtime_id: Some(RuntimeId::new("php-8.3-x64").expect("valid id")),
            target_runtime_id: None,
            idempotency_key: "fail-closed".to_owned(),
            actor: "user".to_owned(),
        });
        assert_eq!(
            result,
            Err(ApplicationError::Port(PortError::UnsupportedPlatform))
        );
    }

    #[test]
    fn direct_active_transition_is_rejected() {
        assert!(validate_activation_change(Activation::Disabled, Activation::Active).is_err());
    }
}
