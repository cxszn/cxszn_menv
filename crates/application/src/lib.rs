//! Application orchestration contracts shared by desktop, CLI, and MCP.
//!
//! The implementation here is intentionally small: it validates safe request
//! shapes and delegates platform work to ports. It does not contain a Windows
//! registry or shell implementation.

use cxszn_menv_domain::{
    validate_activation_transition, Activation, Availability, EnvironmentChangeAction,
    EnvironmentSnapshot, OperationStatus, Ownership, RuntimeFamily, RuntimeId, RuntimeRecord,
};
use cxszn_menv_ports::{
    ConfirmationStore, EnvironmentStore, PathLease, PortError, RuntimeProvider, ScanRequest,
    UserPathStore,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use uuid::Uuid;

mod confirmation;
mod ini_backup;
pub use ini_backup::{PhpIniBackupInfo, MAX_INI_BYTES};

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
    BackupIni,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeRequest {
    pub action: ChangeAction,
    pub family: RuntimeFamily,
    pub runtime_id: Option<RuntimeId>,
    pub target_runtime_id: Option<RuntimeId>,
    pub idempotency_key: String,
    pub actor: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackupSourceBinding {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangePlan {
    pub plan_id: String,
    pub plan_hash: String,
    pub request: ChangeRequest,
    pub requires_confirmation: bool,
    pub created_at_epoch_seconds: u64,
    pub expires_at_epoch_seconds: u64,
    pub snapshot_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_source: Option<BackupSourceBinding>,
}

/// Host-issued confirmation material.  The opaque value is never logged or
/// persisted by this crate; all public fields are bound to the plan before a
/// mutation is attempted.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
    #[error("another MEnv operation is already running")]
    OperationBusy,
    #[error("artifact installation failed: {message}")]
    ArtifactInstallFailed {
        code: String,
        message: String,
        retryable: bool,
    },
    #[error("operation was not found")]
    OperationNotFound,
    #[error("operation is not cancellable in its current state")]
    OperationNotCancellable,
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
    fn issue_confirmation_token(
        &self,
        plan: &ChangePlan,
    ) -> Result<ConfirmationToken, ApplicationError>;
    fn validate_confirmation_token(
        &self,
        plan: &ChangePlan,
        token: Option<&ConfirmationToken>,
    ) -> Result<(), ApplicationError>;
    fn apply_change(
        &self,
        plan: &ChangePlan,
        confirmation_token: Option<&ConfirmationToken>,
    ) -> Result<OperationReceipt, ApplicationError>;
    fn apply_php_ini_backup(
        &self,
        plan: &ChangePlan,
        confirmation_token: Option<&ConfirmationToken>,
        backup_store: &dyn cxszn_menv_ports::PhpIniBackupStore,
    ) -> Result<PhpIniBackupInfo, ApplicationError>;
}

/// Thin orchestrator used by all adapters. Concrete providers and storage are
/// injected by the platform composition root.
pub struct Orchestrator<S, P = FailClosedUserPathStore> {
    store: S,
    path_store: P,
    shim_directory: PathBuf,
    confirmation_store: Arc<dyn ConfirmationStore>,
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
            confirmation_store: Arc::new(confirmation::MemoryConfirmationStore::default()),
        }
    }

    pub fn with_shim_directory(mut self, shim_directory: PathBuf) -> Self {
        self.shim_directory = shim_directory;
        self
    }

    pub fn with_confirmation_store(mut self, store: impl ConfirmationStore + 'static) -> Self {
        self.confirmation_store = Arc::new(store);
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

    /// Return one persisted operation record without changing runtime state.
    pub fn get_operation_status(
        &self,
        operation_id: &str,
    ) -> Result<cxszn_menv_domain::OperationRecord, ApplicationError> {
        let snapshot = self.store.load()?;
        snapshot
            .operations
            .into_iter()
            .find(|operation| operation.operation_id == operation_id)
            .ok_or(ApplicationError::OperationNotFound)
    }

    /// Mark a queued/running operation cancelled. This is intentionally a
    /// state transition only; synchronous adapters must observe the lock and
    /// operation state before starting destructive work.
    pub fn cancel_operation(
        &self,
        operation_id: &str,
    ) -> Result<EnvironmentSnapshot, ApplicationError> {
        let mut snapshot = self.store.load()?;
        let operation = snapshot
            .operations
            .iter_mut()
            .find(|operation| operation.operation_id == operation_id)
            .ok_or(ApplicationError::OperationNotFound)?;
        if !matches!(
            operation.status,
            OperationStatus::Queued | OperationStatus::Running
        ) {
            return Err(ApplicationError::OperationNotCancellable);
        }
        operation.status = OperationStatus::Cancelled;
        operation.updated_at = unix_epoch_iso(now_epoch_seconds());
        snapshot.updated_at = unix_epoch_iso(now_epoch_seconds());
        self.store.save(&snapshot)?;
        Ok(snapshot)
    }

    pub fn scan_with_provider<Q: RuntimeProvider>(
        &self,
        provider: &Q,
        request: ScanRequest,
    ) -> Result<EnvironmentSnapshot, ApplicationError> {
        let mut snapshot = self.store.load()?;
        let previous = snapshot
            .runtimes
            .iter()
            .filter(|runtime| runtime.family == provider.family())
            .cloned()
            .collect::<Vec<_>>();
        let records = provider
            .scan(&request)?
            .into_iter()
            .filter(|record| !self.is_app_owned_php_shim(record))
            .map(|mut record| {
                // Ownership and activation are independent: explicitly
                // enabled external runtimes still belong to their owner.
                if let Some(existing) = previous.iter().find(|runtime| {
                    runtime.family == record.family
                        && normalize_runtime_path(&runtime.executable_path)
                            == normalize_runtime_path(&record.executable_path)
                }) {
                    if existing.ownership != Ownership::DetectedUnmanaged {
                        record.ownership = existing.ownership.clone();
                        record.availability = existing.availability.clone();
                    }
                    record.activation = existing.activation.clone();
                    record.first_seen_at = existing.first_seen_at.clone();
                }
                record
            })
            .collect::<Vec<_>>();
        let discovered_paths = records
            .iter()
            .map(|record| normalize_runtime_path(&record.executable_path))
            .collect::<HashSet<_>>();
        snapshot.runtimes.retain(|item| {
            if item.family != provider.family() {
                return true;
            }
            if self.is_app_owned_php_shim(item) {
                return false;
            }
            // A scan is allowed to forget stale unmanaged observations, but
            // it must never forget a manual/managed record merely because the
            // executable is temporarily absent from PATH or a common root.
            (item.ownership != Ownership::DetectedUnmanaged
                || item.activation != Activation::Disabled)
                && !discovered_paths.contains(&normalize_runtime_path(&item.executable_path))
        });
        snapshot.runtimes.extend(records);
        snapshot.updated_at = unix_epoch_iso(now_epoch_seconds());
        self.store.save(&snapshot)?;
        Ok(snapshot)
    }

    /// Persist the result of a verified catalog installation without enabling
    /// it.  Discovery and installation are separate operations: callers must
    /// explicitly scan first, then pass the exact executable that was
    /// published by the installer.  This keeps a downloaded runtime disabled
    /// until a later user-confirmed enable plan.
    pub fn mark_managed_runtime_disabled(
        &self,
        executable_path: &Path,
    ) -> Result<EnvironmentSnapshot, ApplicationError> {
        let target = executable_path.to_string_lossy().replace('/', "\\");
        let mut snapshot = self.store.load()?;
        let runtime = snapshot
            .runtimes
            .iter_mut()
            .find(|runtime| runtime.executable_path.eq_ignore_ascii_case(&target))
            .ok_or(ApplicationError::RuntimeNotFound)?;
        runtime.ownership = Ownership::Managed;
        runtime.availability = Availability::Managed;
        runtime.activation = Activation::Disabled;
        snapshot.updated_at = unix_epoch_iso(now_epoch_seconds());
        self.store.save(&snapshot)?;
        Ok(snapshot)
    }

    /// Persist a previously verified external runtime as a manual, disabled
    /// registration. The caller must perform path probing before reaching this
    /// method; this method never copies, overwrites, deletes, or enables the
    /// external executable.
    pub fn register_verified_runtime(
        &self,
        mut verified: RuntimeRecord,
    ) -> Result<EnvironmentSnapshot, ApplicationError> {
        let normalized = normalize_runtime_path(&verified.executable_path);
        let mut snapshot = self.store.load()?;
        verified.ownership = Ownership::Manual;
        verified.availability = Availability::Manual;

        if let Some(existing) = snapshot.runtimes.iter().find(|runtime| {
            runtime.family == verified.family
                && normalize_runtime_path(&runtime.executable_path) == normalized
        }) {
            // Re-registering an already active path must not silently disable
            // it; a new manual registration only changes ownership metadata.
            verified.activation = existing.activation.clone();
            verified.first_seen_at = existing.first_seen_at.clone();
        } else {
            verified.activation = Activation::Disabled;
        }

        if let Some(index) = snapshot.runtimes.iter().position(|runtime| {
            runtime.family == verified.family
                && normalize_runtime_path(&runtime.executable_path) == normalized
        }) {
            snapshot.runtimes[index] = verified;
        } else {
            snapshot.runtimes.push(verified);
        }
        snapshot.updated_at = unix_epoch_iso(now_epoch_seconds());
        self.store.save(&snapshot)?;
        Ok(snapshot)
    }

    /// Register a verified runtime and append a durable, idempotent operation
    /// record for protocol adapters that already have a confirmation-bound
    /// plan. The external executable remains untouched and disabled.
    pub fn register_verified_runtime_with_operation(
        &self,
        mut verified: RuntimeRecord,
        plan_id: &str,
        idempotency_key: &str,
    ) -> Result<EnvironmentSnapshot, ApplicationError> {
        if idempotency_key.trim().is_empty() {
            return Err(ApplicationError::MissingIdempotencyKey);
        }
        if plan_id.trim().is_empty() {
            return Err(ApplicationError::Serialization(
                "registration plan id is required".to_owned(),
            ));
        }
        let normalized = normalize_runtime_path(&verified.executable_path);
        let mut snapshot = self.store.load()?;
        if let Some(existing) = snapshot
            .operations
            .iter()
            .find(|operation| operation.idempotency_key == idempotency_key)
        {
            if existing.plan_id != plan_id {
                return Err(ApplicationError::IdempotencyConflict);
            }
            return Ok(snapshot);
        }
        verified.ownership = Ownership::Manual;
        verified.availability = Availability::Manual;
        if let Some(existing) = snapshot.runtimes.iter().find(|runtime| {
            runtime.family == verified.family
                && normalize_runtime_path(&runtime.executable_path) == normalized
        }) {
            verified.activation = existing.activation.clone();
            verified.first_seen_at = existing.first_seen_at.clone();
        } else {
            verified.activation = Activation::Disabled;
        }
        if let Some(index) = snapshot.runtimes.iter().position(|runtime| {
            runtime.family == verified.family
                && normalize_runtime_path(&runtime.executable_path) == normalized
        }) {
            snapshot.runtimes[index] = verified.clone();
        } else {
            snapshot.runtimes.push(verified.clone());
        }
        let now = unix_epoch_iso(now_epoch_seconds());
        snapshot
            .operations
            .push(cxszn_menv_domain::OperationRecord {
                operation_id: Uuid::new_v4().to_string(),
                plan_id: plan_id.to_owned(),
                plan_hash: None,
                idempotency_key: idempotency_key.to_owned(),
                action: EnvironmentChangeAction::Register,
                runtime_id: verified.id,
                status: OperationStatus::Succeeded,
                created_at: now.clone(),
                updated_at: now,
                error: None,
            });
        snapshot.updated_at = unix_epoch_iso(now_epoch_seconds());
        self.store.save(&snapshot)?;
        Ok(snapshot)
    }

    /// Append a successful catalog-install operation after the installer has
    /// published and registered the verified managed executable.
    pub fn record_install_operation(
        &self,
        executable_path: &Path,
        plan_id: &str,
        idempotency_key: &str,
    ) -> Result<EnvironmentSnapshot, ApplicationError> {
        if idempotency_key.trim().is_empty() {
            return Err(ApplicationError::MissingIdempotencyKey);
        }
        let target = executable_path.to_string_lossy().replace('/', "\\");
        let mut snapshot = self.store.load()?;
        if let Some(existing) = snapshot
            .operations
            .iter()
            .find(|operation| operation.idempotency_key == idempotency_key)
        {
            if existing.plan_id != plan_id {
                return Err(ApplicationError::IdempotencyConflict);
            }
            return Ok(snapshot);
        }
        let runtime_id = snapshot
            .runtimes
            .iter()
            .find(|runtime| runtime.executable_path.eq_ignore_ascii_case(&target))
            .map(|runtime| runtime.id.clone())
            .ok_or(ApplicationError::RuntimeNotFound)?;
        let now = unix_epoch_iso(now_epoch_seconds());
        snapshot
            .operations
            .push(cxszn_menv_domain::OperationRecord {
                operation_id: Uuid::new_v4().to_string(),
                plan_id: plan_id.to_owned(),
                plan_hash: None,
                idempotency_key: idempotency_key.to_owned(),
                action: EnvironmentChangeAction::Install,
                runtime_id,
                status: OperationStatus::Succeeded,
                created_at: now.clone(),
                updated_at: now,
                error: None,
            });
        snapshot.updated_at = unix_epoch_iso(now_epoch_seconds());
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
            ChangeAction::BackupIni => {
                if request.runtime_id.is_none() {
                    return Err(ApplicationError::MissingRuntimeId);
                }
            }
            ChangeAction::Register | ChangeAction::Install => {
                return Err(ApplicationError::UnsupportedChangeAction)
            }
        }
        let snapshot = self.store.load()?;
        if matches!(request.action, ChangeAction::Enable | ChangeAction::Switch) {
            let target_id = request
                .target_runtime_id
                .as_ref()
                .or(request.runtime_id.as_ref());
            if snapshot.runtimes.iter().any(|runtime| {
                Some(&runtime.id) == target_id && self.is_app_owned_php_shim(runtime)
            }) {
                return Err(PortError::InvalidPath(
                    "the app-owned PHP shim is not an independent runtime".to_owned(),
                )
                .into());
            }
        }
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
            backup_source: None,
        };
        plan.plan_hash = hash_plan(&plan)?;
        Ok(plan)
    }

    /// Validate before an adapter performs filesystem side effects. Callers
    /// hold the same operation lock until apply completes.
    pub fn validate_change(
        &self,
        plan: &ChangePlan,
        confirmation_token: Option<&ConfirmationToken>,
    ) -> Result<Option<OperationReceipt>, ApplicationError> {
        Ok(self.prepare_change(plan, confirmation_token)?.1)
    }

    fn prepare_change(
        &self,
        plan: &ChangePlan,
        confirmation_token: Option<&ConfirmationToken>,
    ) -> Result<(EnvironmentSnapshot, Option<OperationReceipt>), ApplicationError> {
        if plan.plan_hash != hash_plan(plan)? {
            return Err(ApplicationError::PlanTampered);
        }
        let now = now_epoch_seconds();
        if now >= plan.expires_at_epoch_seconds {
            return Err(ApplicationError::PlanExpired);
        }
        self.validate_confirmation_token(plan, confirmation_token)?;

        let snapshot = self.store.load()?;
        if let Some(existing) = snapshot
            .operations
            .iter()
            .find(|operation| operation.idempotency_key == plan.request.idempotency_key)
        {
            if existing.plan_id != plan.plan_id {
                return Err(ApplicationError::IdempotencyConflict);
            }
            if existing.plan_hash.as_ref() != Some(&plan.plan_hash) {
                return Err(ApplicationError::IdempotencyConflict);
            }
            let receipt = OperationReceipt {
                operation_id: existing.operation_id.clone(),
                status: existing.status.clone(),
            };
            return Ok((snapshot, Some(receipt)));
        }
        if hash_snapshot(&snapshot)? != plan.snapshot_hash {
            return Err(ApplicationError::StateChanged);
        }
        operation_action(&plan.request.action)?;
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
        if matches!(
            plan.request.action,
            ChangeAction::Enable | ChangeAction::Switch
        ) && self.is_app_owned_php_shim(runtime)
        {
            return Err(PortError::InvalidPath(
                "the app-owned PHP shim is not an independent runtime".to_owned(),
            )
            .into());
        }
        Ok((snapshot, None))
    }

    pub fn apply_change(
        &self,
        plan: &ChangePlan,
        confirmation_token: Option<&ConfirmationToken>,
    ) -> Result<OperationReceipt, ApplicationError> {
        let (mut snapshot, existing) = self.prepare_change(plan, confirmation_token)?;
        if plan.request.action == ChangeAction::BackupIni {
            return Err(ApplicationError::UnsupportedChangeAction);
        }
        if let Some(receipt) = existing {
            return Ok(receipt);
        }
        let now = now_epoch_seconds();
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
            ChangeAction::Register | ChangeAction::Install | ChangeAction::BackupIni => false,
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
            plan.plan_hash.clone(),
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
                    ChangeAction::Register | ChangeAction::Install | ChangeAction::BackupIni => {
                        Ok(())
                    }
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

    fn is_app_owned_php_shim(&self, runtime: &RuntimeRecord) -> bool {
        runtime.family == RuntimeFamily::Php
            && normalize_runtime_path(&runtime.executable_path)
                == normalize_runtime_path(&self.shim_directory.join("php.exe").to_string_lossy())
    }

    fn rollback_path_lease(&self, action: &ChangeAction, lease: &PathLease) {
        let _ = match action {
            ChangeAction::Enable | ChangeAction::Switch => self.path_store.remove_lease(lease),
            ChangeAction::Disable => self.path_store.apply_lease(lease),
            ChangeAction::Register | ChangeAction::Install | ChangeAction::BackupIni => Ok(()),
        };
    }
}

fn normalize_runtime_path(path: &str) -> String {
    path.replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
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

    fn issue_confirmation_token(
        &self,
        plan: &ChangePlan,
    ) -> Result<ConfirmationToken, ApplicationError> {
        Orchestrator::issue_confirmation_token(self, plan)
    }

    fn validate_confirmation_token(
        &self,
        plan: &ChangePlan,
        token: Option<&ConfirmationToken>,
    ) -> Result<(), ApplicationError> {
        Orchestrator::validate_confirmation_token(self, plan, token)
    }

    fn apply_change(
        &self,
        plan: &ChangePlan,
        confirmation_token: Option<&ConfirmationToken>,
    ) -> Result<OperationReceipt, ApplicationError> {
        Orchestrator::apply_change(self, plan, confirmation_token)
    }

    fn apply_php_ini_backup(
        &self,
        plan: &ChangePlan,
        confirmation_token: Option<&ConfirmationToken>,
        backup_store: &dyn cxszn_menv_ports::PhpIniBackupStore,
    ) -> Result<PhpIniBackupInfo, ApplicationError> {
        Orchestrator::apply_php_ini_backup(self, plan, confirmation_token, backup_store)
    }
}

fn operation_action(action: &ChangeAction) -> Result<EnvironmentChangeAction, ApplicationError> {
    match action {
        ChangeAction::Register => Ok(EnvironmentChangeAction::Register),
        ChangeAction::Enable => Ok(EnvironmentChangeAction::Enable),
        ChangeAction::Disable => Ok(EnvironmentChangeAction::Disable),
        ChangeAction::Switch => Ok(EnvironmentChangeAction::Switch),
        ChangeAction::BackupIni => Ok(EnvironmentChangeAction::BackupIni),
        ChangeAction::Install => Err(ApplicationError::UnsupportedChangeAction),
    }
}

fn update_snapshot_after_change(
    snapshot: &mut EnvironmentSnapshot,
    request: &ChangeRequest,
    operation_id: String,
    action: EnvironmentChangeAction,
    plan_id: String,
    timestamp: String,
    plan_hash: String,
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
            ChangeAction::Register | ChangeAction::Install | ChangeAction::BackupIni => None,
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
            plan_hash: Some(plan_hash),
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

/// Compute the canonical hash shared by native adapters that create their own
/// confirmation-bound plans (for example manual PHP registration).
pub fn hash_plan(plan: &ChangePlan) -> Result<String, ApplicationError> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct CanonicalPlan<'a> {
        plan_id: &'a str,
        request: &'a ChangeRequest,
        requires_confirmation: bool,
        created_at_epoch_seconds: u64,
        expires_at_epoch_seconds: u64,
        snapshot_hash: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        backup_source: &'a Option<BackupSourceBinding>,
    }
    let canonical = CanonicalPlan {
        plan_id: &plan.plan_id,
        request: &plan.request,
        requires_confirmation: plan.requires_confirmation,
        created_at_epoch_seconds: plan.created_at_epoch_seconds,
        expires_at_epoch_seconds: plan.expires_at_epoch_seconds,
        snapshot_hash: &plan.snapshot_hash,
        backup_source: &plan.backup_source,
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
    use cxszn_menv_ports::{
        EmptyEnvironmentStore, PathLease, PortError, RuntimeProvider, ScanRequest, UserPathStore,
        VerifyPathRequest,
    };
    use std::path::{Path, PathBuf};
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

    #[test]
    fn operation_status_can_be_read_and_queued_operation_cancelled() {
        let state = Arc::new(Mutex::new(test_snapshot()));
        state
            .lock()
            .expect("snapshot lock")
            .operations
            .push(cxszn_menv_domain::OperationRecord {
                operation_id: "op-queued".to_owned(),
                plan_id: "plan-queued".to_owned(),
                plan_hash: None,
                idempotency_key: "key-queued".to_owned(),
                action: EnvironmentChangeAction::Enable,
                runtime_id: RuntimeId::new("php-8.3-x64").expect("runtime id"),
                status: OperationStatus::Queued,
                created_at: "now".to_owned(),
                updated_at: "now".to_owned(),
                error: None,
            });
        let orchestrator = Orchestrator::with_path_store(
            TestEnvironmentStore {
                snapshot: Arc::clone(&state),
            },
            TestPathStore {
                calls: Arc::new(Mutex::new(Vec::new())),
            },
        );

        assert_eq!(
            orchestrator
                .get_operation_status("op-queued")
                .expect("operation status")
                .status,
            OperationStatus::Queued
        );
        let cancelled = orchestrator
            .cancel_operation("op-queued")
            .expect("cancel operation");
        assert_eq!(cancelled.operations[0].status, OperationStatus::Cancelled);
    }

    #[test]
    fn completed_operation_cannot_be_cancelled() {
        let state = Arc::new(Mutex::new(test_snapshot()));
        state
            .lock()
            .expect("snapshot lock")
            .operations
            .push(cxszn_menv_domain::OperationRecord {
                operation_id: "op-done".to_owned(),
                plan_id: "plan-done".to_owned(),
                plan_hash: None,
                idempotency_key: "key-done".to_owned(),
                action: EnvironmentChangeAction::Enable,
                runtime_id: RuntimeId::new("php-8.3-x64").expect("runtime id"),
                status: OperationStatus::Succeeded,
                created_at: "now".to_owned(),
                updated_at: "now".to_owned(),
                error: None,
            });
        let orchestrator = Orchestrator::new(TestEnvironmentStore { snapshot: state });
        assert_eq!(
            orchestrator.cancel_operation("op-done"),
            Err(ApplicationError::OperationNotCancellable)
        );
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

    struct RescanningProvider;

    impl RuntimeProvider for RescanningProvider {
        fn family(&self) -> RuntimeFamily {
            RuntimeFamily::Php
        }

        fn scan(&self, _request: &ScanRequest) -> Result<Vec<RuntimeRecord>, PortError> {
            let mut record = RuntimeRecord::discovered(
                RuntimeId::new("php:win:c:\\php\\php.exe").expect("valid id"),
                RuntimeFamily::Php,
                "8.3.0",
                Architecture::X64,
                r"C:\php\php.exe",
                cxszn_menv_domain::RuntimeSource::CommonLocation,
            );
            record.health = Health::Healthy;
            Ok(vec![record])
        }

        fn verify_path(&self, _request: &VerifyPathRequest) -> Result<RuntimeRecord, PortError> {
            Err(PortError::UnsupportedPlatform)
        }
    }

    struct EmptyPhpProvider;

    impl RuntimeProvider for EmptyPhpProvider {
        fn family(&self) -> RuntimeFamily {
            RuntimeFamily::Php
        }

        fn scan(&self, _request: &ScanRequest) -> Result<Vec<RuntimeRecord>, PortError> {
            Ok(Vec::new())
        }

        fn verify_path(&self, _request: &VerifyPathRequest) -> Result<RuntimeRecord, PortError> {
            Err(PortError::UnsupportedPlatform)
        }
    }

    #[test]
    fn rescanning_preserves_prior_managed_activation_for_the_same_executable() {
        let state = Arc::new(Mutex::new(test_snapshot()));
        {
            let mut snapshot = state.lock().expect("snapshot lock");
            snapshot.runtimes[0].id = RuntimeId::new("php:win:c:\\php\\php.exe").expect("id");
            snapshot.runtimes[0].ownership = Ownership::Managed;
            snapshot.runtimes[0].availability = Availability::Managed;
            snapshot.runtimes[0].activation = Activation::Active;
            snapshot.runtimes[0].first_seen_at = "first-seen".to_owned();
        }
        let orchestrator = Orchestrator::with_path_store(
            TestEnvironmentStore {
                snapshot: Arc::clone(&state),
            },
            TestPathStore {
                calls: Arc::new(Mutex::new(Vec::new())),
            },
        );

        let snapshot = orchestrator
            .scan_with_provider(&RescanningProvider, ScanRequest::default())
            .expect("scan succeeds");
        assert_eq!(snapshot.runtimes[0].ownership, Ownership::Managed);
        assert_eq!(snapshot.runtimes[0].availability, Availability::Managed);
        assert_eq!(snapshot.runtimes[0].activation, Activation::Active);
        assert_eq!(snapshot.runtimes[0].first_seen_at, "first-seen");
    }

    #[test]
    fn regression_rescan_preserves_explicitly_enabled_unmanaged_php() {
        let mut initial = test_snapshot();
        initial.runtimes[0].id = RuntimeId::new("php:win:c:\\php\\php.exe").expect("valid id");
        initial.runtimes[0].ownership = Ownership::DetectedUnmanaged;
        initial.runtimes[0].availability = Availability::DetectedUnmanaged;
        initial.runtimes[0].first_seen_at = "2026-10-10T08:00:00Z".to_owned();
        let runtime_id = initial.runtimes[0].id.clone();
        let state = Arc::new(Mutex::new(initial));
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
                runtime_id: Some(runtime_id.clone()),
                target_runtime_id: None,
                idempotency_key: "enable-unmanaged-before-rescan".to_owned(),
                actor: "user".to_owned(),
            })
            .expect("external PHP can be explicitly enabled");
        let token = orchestrator
            .issue_confirmation_token(&plan)
            .expect("approved enable");
        let receipt = orchestrator
            .apply_change(&plan, Some(&token))
            .expect("confirmed enable succeeds");
        assert_eq!(receipt.status, OperationStatus::Succeeded);
        assert_eq!(
            orchestrator
                .get_environment_state()
                .expect("enabled state")
                .runtimes[0]
                .activation,
            Activation::Active
        );

        orchestrator
            .scan_with_provider(&RescanningProvider, ScanRequest::default())
            .expect("scan succeeds");
        let persisted = orchestrator
            .get_environment_state()
            .expect("rescanned state");
        assert_eq!(persisted.runtimes.len(), 1);
        let runtime = &persisted.runtimes[0];
        assert_eq!(runtime.id, runtime_id);
        assert_eq!(runtime.activation, Activation::Active);
        assert_eq!(runtime.ownership, Ownership::DetectedUnmanaged);
        assert_eq!(runtime.availability, Availability::DetectedUnmanaged);
        assert_eq!(runtime.first_seen_at, "2026-10-10T08:00:00Z");
        assert_eq!(runtime.health, Health::Healthy);
        assert!(persisted.path_lease.enabled);
        assert_eq!(persisted.path_lease.runtime_id, Some(runtime_id));
        assert_eq!(path_calls.calls(), vec!["apply:menv-user-path".to_owned()]);
    }

    struct FixturePhpProvider {
        records: Vec<RuntimeRecord>,
    }

    impl RuntimeProvider for FixturePhpProvider {
        fn family(&self) -> RuntimeFamily {
            RuntimeFamily::Php
        }

        fn scan(&self, _request: &ScanRequest) -> Result<Vec<RuntimeRecord>, PortError> {
            Ok(self.records.clone())
        }

        fn verify_path(&self, _request: &VerifyPathRequest) -> Result<RuntimeRecord, PortError> {
            Err(PortError::UnsupportedPlatform)
        }
    }

    #[test]
    fn regression_scan_excludes_own_shim_candidates_and_old_shim_record() {
        let shim_directory = r"C:\Users\demo\AppData\Local\cxszn-menv\bin";
        let shim_record = RuntimeRecord::discovered(
            RuntimeId::new("php-own-shim").expect("shim id"),
            RuntimeFamily::Php,
            "8.3.0",
            Architecture::X64,
            r"C:/Users/DEMO/AppData/Local/cxszn-menv/bin/PHP.EXE",
            cxszn_menv_domain::RuntimeSource::SystemPath,
        );
        let mut initial = test_snapshot();
        initial.runtimes.push(shim_record.clone());
        let state = Arc::new(Mutex::new(initial));
        let path_store = TestPathStore {
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let path_calls = path_store.clone();
        let orchestrator = Orchestrator::with_path_store(
            TestEnvironmentStore {
                snapshot: Arc::clone(&state),
            },
            path_store,
        )
        .with_shim_directory(PathBuf::from(shim_directory));
        let external_record = RescanningProvider
            .scan(&ScanRequest::default())
            .expect("external fixture")
            .remove(0);
        let near_shim_record = RuntimeRecord::discovered(
            RuntimeId::new("php-near-shim").expect("external id"),
            RuntimeFamily::Php,
            "8.3.0",
            Architecture::X64,
            r"C:\Users\demo\AppData\Local\cxszn-menv\bin-extra\php.exe",
            cxszn_menv_domain::RuntimeSource::CommonLocation,
        );

        orchestrator
            .scan_with_provider(
                &FixturePhpProvider {
                    records: vec![external_record, shim_record, near_shim_record.clone()],
                },
                ScanRequest::default(),
            )
            .expect("scan succeeds");
        let persisted = orchestrator
            .get_environment_state()
            .expect("rescanned state");
        assert_eq!(persisted.runtimes.len(), 2);
        assert!(persisted.runtimes.iter().all(|runtime| {
            !runtime
                .executable_path
                .replace('/', "\\")
                .eq_ignore_ascii_case(&format!("{shim_directory}\\php.exe"))
        }));
        assert!(persisted
            .runtimes
            .iter()
            .any(|runtime| runtime.id == near_shim_record.id));
        assert!(path_calls.calls().is_empty());
    }

    #[test]
    fn regression_plan_rejects_enabling_own_shim_from_old_state() {
        let mut initial = test_snapshot();
        let shim_id = RuntimeId::new("php-own-shim").expect("shim id");
        initial.runtimes.push(RuntimeRecord::discovered(
            shim_id.clone(),
            RuntimeFamily::Php,
            "8.3.0",
            Architecture::X64,
            r"C:/Users/DEMO/AppData/Local/cxszn-menv/bin/PHP.EXE",
            cxszn_menv_domain::RuntimeSource::SystemPath,
        ));
        let state = Arc::new(Mutex::new(initial.clone()));
        let path_store = TestPathStore {
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let path_calls = path_store.clone();
        let orchestrator = Orchestrator::with_path_store(
            TestEnvironmentStore {
                snapshot: Arc::clone(&state),
            },
            path_store,
        )
        .with_shim_directory(PathBuf::from(r"C:\Users\demo\AppData\Local\cxszn-menv\bin"));

        let plan = orchestrator.plan_change(ChangeRequest {
            action: ChangeAction::Enable,
            family: RuntimeFamily::Php,
            runtime_id: Some(shim_id),
            target_runtime_id: None,
            idempotency_key: "reject-own-shim-enable".to_owned(),
            actor: "user".to_owned(),
        });
        assert!(
            plan.is_err(),
            "own shim must not be offered as an enable target"
        );
        assert_eq!(
            orchestrator.get_environment_state().expect("state"),
            initial
        );
        assert!(path_calls.calls().is_empty());
    }

    #[test]
    fn regression_plan_rejects_switching_to_own_shim_from_old_state() {
        let mut initial = test_snapshot();
        initial.runtimes[0].activation = Activation::Active;
        let current_id = initial.runtimes[0].id.clone();
        let shim_id = RuntimeId::new("php-own-shim").expect("shim id");
        initial.runtimes.push(RuntimeRecord::discovered(
            shim_id.clone(),
            RuntimeFamily::Php,
            "8.3.0",
            Architecture::X64,
            r"C:/Users/DEMO/AppData/Local/cxszn-menv/bin/PHP.EXE",
            cxszn_menv_domain::RuntimeSource::SystemPath,
        ));
        let state = Arc::new(Mutex::new(initial.clone()));
        let path_store = TestPathStore {
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let path_calls = path_store.clone();
        let orchestrator = Orchestrator::with_path_store(
            TestEnvironmentStore {
                snapshot: Arc::clone(&state),
            },
            path_store,
        )
        .with_shim_directory(PathBuf::from(r"C:\Users\demo\AppData\Local\cxszn-menv\bin"));

        let plan = orchestrator.plan_change(ChangeRequest {
            action: ChangeAction::Switch,
            family: RuntimeFamily::Php,
            runtime_id: Some(current_id),
            target_runtime_id: Some(shim_id),
            idempotency_key: "reject-own-shim-switch".to_owned(),
            actor: "user".to_owned(),
        });
        assert!(
            plan.is_err(),
            "own shim must not be offered as a switch target"
        );
        assert_eq!(
            orchestrator.get_environment_state().expect("state"),
            initial
        );
        assert!(path_calls.calls().is_empty());
    }

    #[test]
    fn scan_keeps_managed_runtime_when_provider_temporarily_finds_nothing() {
        let state = Arc::new(Mutex::new(test_snapshot()));
        {
            let mut snapshot = state.lock().expect("snapshot lock");
            snapshot.runtimes[0].ownership = Ownership::Managed;
            snapshot.runtimes[0].availability = Availability::Managed;
            snapshot.runtimes[0].activation = Activation::Active;
        }
        let orchestrator = Orchestrator::with_path_store(
            TestEnvironmentStore {
                snapshot: Arc::clone(&state),
            },
            TestPathStore {
                calls: Arc::new(Mutex::new(Vec::new())),
            },
        );
        let snapshot = orchestrator
            .scan_with_provider(&EmptyPhpProvider, ScanRequest::default())
            .expect("scan succeeds");
        assert_eq!(snapshot.runtimes.len(), 1);
        assert_eq!(snapshot.runtimes[0].ownership, Ownership::Managed);
        assert_eq!(snapshot.runtimes[0].activation, Activation::Active);
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
    fn marking_managed_runtime_persists_disabled_state() {
        let state = Arc::new(Mutex::new(test_snapshot()));
        let orchestrator = Orchestrator::with_path_store(
            TestEnvironmentStore {
                snapshot: Arc::clone(&state),
            },
            TestPathStore {
                calls: Arc::new(Mutex::new(Vec::new())),
            },
        );

        let snapshot = orchestrator
            .mark_managed_runtime_disabled(Path::new(r"C:\php\php.exe"))
            .expect("runtime is persisted as managed");
        assert_eq!(snapshot.runtimes[0].ownership, Ownership::Managed);
        assert_eq!(snapshot.runtimes[0].availability, Availability::Managed);
        assert_eq!(snapshot.runtimes[0].activation, Activation::Disabled);
        let persisted = state.lock().expect("snapshot lock").clone();
        assert_eq!(persisted, snapshot);
    }

    #[test]
    fn registering_verified_runtime_is_manual_and_disabled_without_touching_path() {
        let state = Arc::new(Mutex::new(test_snapshot()));
        let orchestrator = Orchestrator::with_path_store(
            TestEnvironmentStore {
                snapshot: Arc::clone(&state),
            },
            TestPathStore {
                calls: Arc::new(Mutex::new(Vec::new())),
            },
        );
        let verified = RuntimeRecord::discovered(
            RuntimeId::new("php:win:d:\\tools\\php.exe").expect("valid id"),
            RuntimeFamily::Php,
            "8.5.11",
            Architecture::X64,
            r"D:\tools\php.exe",
            cxszn_menv_domain::RuntimeSource::CommonLocation,
        );

        let snapshot = orchestrator
            .register_verified_runtime(verified)
            .expect("manual registration succeeds");
        let registered = snapshot
            .runtimes
            .iter()
            .find(|runtime| {
                runtime
                    .executable_path
                    .eq_ignore_ascii_case(r"D:\tools\php.exe")
            })
            .expect("registered runtime");
        assert_eq!(registered.ownership, Ownership::Manual);
        assert_eq!(registered.availability, Availability::Manual);
        assert_eq!(registered.activation, Activation::Disabled);
        assert!(snapshot.path_lease.entries.is_empty());
    }

    #[test]
    fn manual_registration_appends_an_idempotent_operation_record() {
        let state = Arc::new(Mutex::new(test_snapshot()));
        let orchestrator = Orchestrator::with_path_store(
            TestEnvironmentStore {
                snapshot: Arc::clone(&state),
            },
            TestPathStore {
                calls: Arc::new(Mutex::new(Vec::new())),
            },
        );
        let verified = RuntimeRecord::discovered(
            RuntimeId::new("php:win:e:\\tools\\php.exe").expect("valid id"),
            RuntimeFamily::Php,
            "8.5.11",
            Architecture::X64,
            r"E:\tools\php.exe",
            cxszn_menv_domain::RuntimeSource::CommonLocation,
        );
        let first = orchestrator
            .register_verified_runtime_with_operation(verified.clone(), "plan-1", "register-1")
            .expect("registration succeeds");
        assert_eq!(first.operations.len(), 1);
        assert_eq!(
            first.operations[0].action,
            EnvironmentChangeAction::Register
        );
        assert_eq!(first.operations[0].status, OperationStatus::Succeeded);
        let replay = orchestrator
            .register_verified_runtime_with_operation(verified, "plan-1", "register-1")
            .expect("idempotent replay succeeds");
        assert_eq!(replay.operations.len(), 1);
    }

    #[test]
    fn catalog_install_operation_is_idempotent_after_managed_registration() {
        let state = Arc::new(Mutex::new(test_snapshot()));
        let orchestrator = Orchestrator::with_path_store(
            TestEnvironmentStore {
                snapshot: Arc::clone(&state),
            },
            TestPathStore {
                calls: Arc::new(Mutex::new(Vec::new())),
            },
        );
        orchestrator
            .mark_managed_runtime_disabled(Path::new(r"C:\php\php.exe"))
            .expect("managed registration");
        let snapshot = orchestrator
            .record_install_operation(Path::new(r"C:\php\php.exe"), "install-plan-1", "install-1")
            .expect("install operation");
        assert_eq!(snapshot.operations.len(), 1);
        assert_eq!(
            snapshot.operations[0].action,
            EnvironmentChangeAction::Install
        );
        let replay = orchestrator
            .record_install_operation(Path::new(r"C:\php\php.exe"), "install-plan-1", "install-1")
            .expect("idempotent install replay");
        assert_eq!(replay.operations.len(), 1);
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

        let mut wrong_actor = orchestrator
            .issue_confirmation_token(&plan)
            .expect("approved plan");
        wrong_actor.actor = "other".to_owned();
        assert_eq!(
            orchestrator.apply_change(&plan, Some(&wrong_actor)),
            Err(ApplicationError::ConfirmationInvalid)
        );
    }

    #[test]
    fn unissued_confirmation_is_rejected_by_orchestrator_without_state_or_path_changes() {
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
        let before = orchestrator.get_environment_state().expect("before");
        let plan = orchestrator
            .plan_change(ChangeRequest {
                action: ChangeAction::Enable,
                family: RuntimeFamily::Php,
                runtime_id: Some(before.runtimes[0].id.clone()),
                target_runtime_id: None,
                idempotency_key: "unissued-direct-confirmation".to_owned(),
                actor: "user".to_owned(),
            })
            .expect("valid enable plan");
        let token = ConfirmationToken {
            value: "never-issued-token".to_owned(),
            plan_id: plan.plan_id.clone(),
            plan_hash: plan.plan_hash.clone(),
            actor: plan.request.actor.clone(),
            expires_at_epoch_seconds: plan.expires_at_epoch_seconds,
        };

        assert_eq!(
            orchestrator.apply_change(&plan, Some(&token)),
            Err(ApplicationError::ConfirmationInvalid)
        );
        assert_eq!(orchestrator.get_environment_state().expect("after"), before);
        assert!(path_calls.calls().is_empty());
    }

    #[test]
    fn unissued_confirmation_is_rejected_by_application_service_without_state_or_path_changes() {
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
        let service: &dyn ApplicationService = &orchestrator;
        let before = service.get_environment_state().expect("before");
        let plan = service
            .plan_change(ChangeRequest {
                action: ChangeAction::Enable,
                family: RuntimeFamily::Php,
                runtime_id: Some(before.runtimes[0].id.clone()),
                target_runtime_id: None,
                idempotency_key: "unissued-trait-confirmation".to_owned(),
                actor: "user".to_owned(),
            })
            .expect("valid enable plan");
        let token = ConfirmationToken {
            value: "never-issued-token".to_owned(),
            plan_id: plan.plan_id.clone(),
            plan_hash: plan.plan_hash.clone(),
            actor: plan.request.actor.clone(),
            expires_at_epoch_seconds: plan.expires_at_epoch_seconds,
        };

        assert_eq!(
            service.apply_change(&plan, Some(&token)),
            Err(ApplicationError::ConfirmationInvalid)
        );
        assert_eq!(service.get_environment_state().expect("after"), before);
        assert!(path_calls.calls().is_empty());
    }

    #[derive(Clone, Default)]
    struct RecordingConfirmationStore {
        records: Arc<Mutex<Vec<cxszn_menv_ports::ConfirmationRecord>>>,
    }

    impl ConfirmationStore for RecordingConfirmationStore {
        fn insert(&self, record: &cxszn_menv_ports::ConfirmationRecord) -> Result<(), PortError> {
            let mut records = self.records.lock().expect("confirmation records");
            if records
                .iter()
                .any(|existing| existing.token_hash == record.token_hash)
            {
                return Err(PortError::Platform(
                    "confirmation digest already exists".to_owned(),
                ));
            }
            records.push(record.clone());
            Ok(())
        }

        fn find(
            &self,
            token_hash: &str,
        ) -> Result<Option<cxszn_menv_ports::ConfirmationRecord>, PortError> {
            Ok(self
                .records
                .lock()
                .expect("confirmation records")
                .iter()
                .find(|record| record.token_hash == token_hash)
                .cloned())
        }
    }

    #[test]
    fn issued_confirmation_is_digest_only_and_not_transferable_to_another_authority() {
        let confirmations = RecordingConfirmationStore::default();
        let path_store = TestPathStore {
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let path_calls = path_store.clone();
        let orchestrator = Orchestrator::with_path_store(
            TestEnvironmentStore {
                snapshot: Arc::new(Mutex::new(test_snapshot())),
            },
            path_store,
        )
        .with_confirmation_store(confirmations.clone());
        let before = orchestrator.get_environment_state().expect("before");
        let plan = orchestrator
            .plan_change(ChangeRequest {
                action: ChangeAction::Enable,
                family: RuntimeFamily::Php,
                runtime_id: Some(before.runtimes[0].id.clone()),
                target_runtime_id: None,
                idempotency_key: "issued-confirmation".to_owned(),
                actor: "user".to_owned(),
            })
            .expect("plan");
        let token = orchestrator
            .issue_confirmation_token(&plan)
            .expect("explicit approval");

        assert_eq!(
            orchestrator.validate_confirmation_token(&plan, Some(&token)),
            Ok(())
        );
        assert_eq!(
            orchestrator
                .get_environment_state()
                .expect("after issuance"),
            before
        );
        assert!(path_calls.calls().is_empty());
        let records = confirmations.records.lock().expect("records").clone();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].token_hash, hash_bytes(token.value.as_bytes()));
        assert!(!format!("{:?}", records[0]).contains(&token.value));
        assert!(!format!("{token:?}").contains(&token.value));
        assert!(format!("{token:?}").contains("<redacted>"));
        let other = Orchestrator::new(TestEnvironmentStore {
            snapshot: Arc::new(Mutex::new(before)),
        });
        assert_eq!(
            other.validate_confirmation_token(&plan, Some(&token)),
            Err(ApplicationError::ConfirmationInvalid)
        );
    }

    #[test]
    fn issued_nonce_cannot_be_rebound_to_another_actor_hash_or_expiry() {
        let path_store = TestPathStore {
            calls: Arc::new(Mutex::new(Vec::new())),
        };
        let path_calls = path_store.clone();
        let orchestrator = Orchestrator::with_path_store(
            TestEnvironmentStore {
                snapshot: Arc::new(Mutex::new(test_snapshot())),
            },
            path_store,
        );
        let before = orchestrator.get_environment_state().expect("before");
        let plan = orchestrator
            .plan_change(ChangeRequest {
                action: ChangeAction::Enable,
                family: RuntimeFamily::Php,
                runtime_id: Some(before.runtimes[0].id.clone()),
                target_runtime_id: None,
                idempotency_key: "borrowed-confirmation".to_owned(),
                actor: "user".to_owned(),
            })
            .expect("plan");
        let token = orchestrator
            .issue_confirmation_token(&plan)
            .expect("explicit approval");
        for altered_field in ["actor", "hash", "expiry"] {
            let mut changed_plan = plan.clone();
            match altered_field {
                "actor" => changed_plan.request.actor = "other-user".to_owned(),
                "hash" => changed_plan.snapshot_hash = "another-snapshot".to_owned(),
                "expiry" => changed_plan.expires_at_epoch_seconds -= 1,
                _ => unreachable!(),
            }
            changed_plan.plan_hash = hash_plan(&changed_plan).expect("modified plan hash");
            let mut borrowed = token.clone();
            borrowed.actor = changed_plan.request.actor.clone();
            borrowed.plan_hash = changed_plan.plan_hash.clone();
            borrowed.expires_at_epoch_seconds = changed_plan.expires_at_epoch_seconds;
            assert_eq!(
                orchestrator.apply_change(&changed_plan, Some(&borrowed)),
                Err(ApplicationError::ConfirmationInvalid),
                "borrowed nonce must reject {altered_field}"
            );
        }
        assert_eq!(orchestrator.get_environment_state().expect("after"), before);
        assert!(path_calls.calls().is_empty());
    }

    #[test]
    fn confirmation_issuer_rejects_displayed_action_that_does_not_match_digest() {
        let orchestrator = Orchestrator::with_path_store(
            TestEnvironmentStore {
                snapshot: Arc::new(Mutex::new(test_snapshot())),
            },
            TestPathStore {
                calls: Arc::new(Mutex::new(Vec::new())),
            },
        );
        let mut displayed = orchestrator
            .plan_change(ChangeRequest {
                action: ChangeAction::Enable,
                family: RuntimeFamily::Php,
                runtime_id: Some(RuntimeId::new("php-8.3-x64").expect("runtime id")),
                target_runtime_id: None,
                idempotency_key: "approval-display-binding".to_owned(),
                actor: "user".to_owned(),
            })
            .expect("plan");
        displayed.request.action = ChangeAction::Disable;

        assert_eq!(
            orchestrator.issue_confirmation_token(&displayed),
            Err(ApplicationError::PlanTampered)
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
        let token = orchestrator
            .issue_confirmation_token(&plan)
            .expect("approved plan");

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
    fn native_plan_binding_is_hash_bound_and_unknown_fields_are_rejected() {
        let orchestrator = Orchestrator::new(TestEnvironmentStore {
            snapshot: Arc::new(Mutex::new(test_snapshot())),
        });
        let mut plan = orchestrator
            .plan_change(ChangeRequest {
                action: ChangeAction::BackupIni,
                family: RuntimeFamily::Php,
                runtime_id: Some(RuntimeId::new("php-8.3-x64").expect("id")),
                target_runtime_id: None,
                idempotency_key: "binding-contract".to_owned(),
                actor: "user".to_owned(),
            })
            .expect("plan");
        let legacy = serde_json::to_value(&plan).expect("legacy JSON");
        assert!(legacy.get("backupSource").is_none());
        assert_eq!(
            serde_json::from_value::<ChangePlan>(legacy.clone()).expect("legacy decode"),
            plan
        );
        plan.backup_source = Some(BackupSourceBinding {
            path: r"C:\php\php.ini".to_owned(),
            sha256: "a".repeat(64),
        });
        plan.plan_hash = hash_plan(&plan).expect("bound hash");
        let first = plan.plan_hash.clone();
        plan.backup_source.as_mut().expect("binding").sha256 = "b".repeat(64);
        assert_ne!(hash_plan(&plan).expect("modified hash"), first);
        let mut unknown = legacy;
        unknown["confirm"] = serde_json::json!(true);
        assert!(serde_json::from_value::<ChangePlan>(unknown).is_err());
        assert!(
            serde_json::from_value::<BackupSourceBinding>(serde_json::json!({
                "path": r"C:\php\php.ini", "sha256": "a".repeat(64), "override": true
            }))
            .is_err()
        );
    }

    #[test]
    fn writable_plan_cannot_turn_off_confirmation_by_rehashing_the_flag() {
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
        let before = orchestrator.get_environment_state().expect("before");
        let mut plan = orchestrator
            .plan_change(ChangeRequest {
                action: ChangeAction::Enable,
                family: RuntimeFamily::Php,
                runtime_id: Some(before.runtimes[0].id.clone()),
                target_runtime_id: None,
                idempotency_key: "no-confirmation-flag".to_owned(),
                actor: "user".to_owned(),
            })
            .expect("plan");
        plan.requires_confirmation = false;
        plan.plan_hash = hash_plan(&plan).expect("rehash");
        let result = orchestrator.apply_change(&plan, None);
        assert!(
            result.is_err(),
            "a writable plan must never opt out of confirmation"
        );
        assert!(path_calls.calls().is_empty());
        assert_eq!(orchestrator.get_environment_state().expect("after"), before);
    }

    #[test]
    fn generic_apply_rejects_backup_ini_without_recording_success() {
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
        let before = orchestrator.get_environment_state().expect("before");
        let mut plan = orchestrator
            .plan_change(ChangeRequest {
                action: ChangeAction::BackupIni,
                family: RuntimeFamily::Php,
                runtime_id: Some(RuntimeId::new("php-8.3-x64").expect("valid runtime id")),
                target_runtime_id: None,
                idempotency_key: "backup-ini-test".to_owned(),
                actor: "user".to_owned(),
            })
            .expect("backup plan succeeds");
        plan.backup_source = Some(BackupSourceBinding {
            path: r"C:\php\php.ini".to_owned(),
            sha256: "a".repeat(64),
        });
        plan.plan_hash = hash_plan(&plan).expect("bound backup plan");
        let token = orchestrator
            .issue_confirmation_token(&plan)
            .expect("approved backup");

        assert_eq!(
            orchestrator.apply_change(&plan, Some(&token)),
            Err(ApplicationError::UnsupportedChangeAction)
        );
        assert_eq!(orchestrator.get_environment_state().expect("after"), before);
        assert!(path_calls.calls().is_empty());
    }

    #[test]
    fn generic_application_service_apply_rejects_backup_ini_without_recording_success() {
        let orchestrator = Orchestrator::new(TestEnvironmentStore {
            snapshot: Arc::new(Mutex::new(test_snapshot())),
        });
        let service: &dyn ApplicationService = &orchestrator;
        let before = service.get_environment_state().expect("before");
        let mut plan = service
            .plan_change(ChangeRequest {
                action: ChangeAction::BackupIni,
                family: RuntimeFamily::Php,
                runtime_id: Some(before.runtimes[0].id.clone()),
                target_runtime_id: None,
                idempotency_key: "generic-trait-backup".to_owned(),
                actor: "user".to_owned(),
            })
            .expect("backup plan");
        plan.backup_source = Some(BackupSourceBinding {
            path: r"C:\php\php.ini".to_owned(),
            sha256: "a".repeat(64),
        });
        plan.plan_hash = hash_plan(&plan).expect("bound backup plan");
        let token = service
            .issue_confirmation_token(&plan)
            .expect("approved backup");

        assert_eq!(
            service.apply_change(&plan, Some(&token)),
            Err(ApplicationError::UnsupportedChangeAction)
        );
        assert_eq!(service.get_environment_state().expect("after"), before);
    }

    #[test]
    fn legacy_operation_without_plan_hash_is_readable_but_rejects_changed_payload_replay() {
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
        let mut plan = orchestrator
            .plan_change(ChangeRequest {
                action: ChangeAction::Enable,
                family: RuntimeFamily::Php,
                runtime_id: Some(RuntimeId::new("php-8.3-x64").expect("runtime id")),
                target_runtime_id: None,
                idempotency_key: "legacy-replay".to_owned(),
                actor: "user".to_owned(),
            })
            .expect("enable plan");
        let token = orchestrator
            .issue_confirmation_token(&plan)
            .expect("approved enable");
        let receipt = orchestrator
            .apply_change(&plan, Some(&token))
            .expect("original enable succeeds");
        state.lock().expect("state").operations[0].plan_hash = None;
        let before = orchestrator.get_environment_state().expect("legacy state");
        assert_eq!(
            orchestrator
                .get_operation_status(&receipt.operation_id)
                .expect("legacy operation remains readable"),
            before.operations[0]
        );
        plan.request.actor = "different-user".to_owned();
        plan.plan_hash = hash_plan(&plan).expect("modified plan hash");
        let changed_token = orchestrator
            .issue_confirmation_token(&plan)
            .expect("approve changed plan");

        assert_eq!(
            orchestrator.apply_change(&plan, Some(&changed_token)),
            Err(ApplicationError::IdempotencyConflict)
        );
        assert_eq!(orchestrator.get_environment_state().expect("after"), before);
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
        let token = orchestrator
            .issue_confirmation_token(&plan)
            .expect("approved plan");
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
