//! Application orchestration contracts shared by desktop, CLI, and MCP.
//!
//! The implementation here is intentionally small: it validates safe request
//! shapes and delegates platform work to ports. It does not contain a Windows
//! registry or shell implementation.

use cxszn_menv_domain::{
    validate_activation_transition, Activation, EnvironmentSnapshot, OperationStatus,
    RuntimeFamily, RuntimeId,
};
use cxszn_menv_ports::{EnvironmentStore, PortError, RuntimeProvider, ScanRequest};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChangeAction {
    Register,
    Install,
    Enable,
    Disable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeRequest {
    pub action: ChangeAction,
    pub family: RuntimeFamily,
    pub runtime_id: Option<RuntimeId>,
    pub idempotency_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePlan {
    pub plan_id: String,
    pub plan_hash: String,
    pub request: ChangeRequest,
    pub requires_confirmation: bool,
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
    #[error("confirmation token is required for this operation")]
    ConfirmationRequired,
    #[error("runtime id is required for this operation")]
    MissingRuntimeId,
    #[error("invalid activation transition")]
    InvalidActivationTransition,
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
        confirmation_token: Option<&str>,
    ) -> Result<OperationReceipt, ApplicationError>;
}

/// Thin orchestrator used by all adapters. Concrete providers and storage are
/// injected by the platform composition root.
pub struct Orchestrator<S> {
    store: S,
}

impl<S> Orchestrator<S>
where
    S: EnvironmentStore,
{
    pub fn new(store: S) -> Self {
        Self { store }
    }

    pub fn get_environment_state(&self) -> Result<EnvironmentSnapshot, ApplicationError> {
        Ok(self.store.load()?)
    }

    pub fn scan_with_provider<P: RuntimeProvider>(
        &self,
        provider: &P,
        request: ScanRequest,
    ) -> Result<EnvironmentSnapshot, ApplicationError> {
        let mut snapshot = self.store.load()?;
        let records = provider.scan(&request)?;
        snapshot.runtimes.retain(|item| item.family != provider.family());
        snapshot.runtimes.extend(records);
        Ok(snapshot)
    }

    pub fn plan_change(&self, request: ChangeRequest) -> Result<ChangePlan, ApplicationError> {
        if request.idempotency_key.trim().is_empty() {
            return Err(ApplicationError::MissingIdempotencyKey);
        }
        if matches!(request.action, ChangeAction::Enable | ChangeAction::Disable)
            && request.runtime_id.is_none()
        {
            return Err(ApplicationError::MissingRuntimeId);
        }
        let plan_id = Uuid::new_v4().to_string();
        // A deterministic, opaque placeholder hash keeps the interface stable;
        // the persistence layer will replace this with a canonical plan hash.
        let plan_hash = format!("plan:{}", plan_id);
        Ok(ChangePlan {
            plan_id,
            plan_hash,
            request,
            requires_confirmation: true,
            expires_at_epoch_seconds: 0,
        })
    }

    pub fn apply_change(
        &self,
        plan: &ChangePlan,
        confirmation_token: Option<&str>,
    ) -> Result<OperationReceipt, ApplicationError> {
        if plan.requires_confirmation && confirmation_token.is_none() {
            return Err(ApplicationError::ConfirmationRequired);
        }
        Ok(OperationReceipt {
            operation_id: Uuid::new_v4().to_string(),
            status: OperationStatus::Queued,
        })
    }
}

impl<S> ApplicationService for Orchestrator<S>
where
    S: EnvironmentStore,
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
        confirmation_token: Option<&str>,
    ) -> Result<OperationReceipt, ApplicationError> {
        Orchestrator::apply_change(self, plan, confirmation_token)
    }
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
    use cxszn_menv_ports::EmptyEnvironmentStore;

    #[test]
    fn write_plan_requires_idempotency_key() {
        let orchestrator = Orchestrator::new(EmptyEnvironmentStore);
        let result = orchestrator.plan_change(ChangeRequest {
            action: ChangeAction::Enable,
            family: RuntimeFamily::Php,
            runtime_id: None,
            idempotency_key: String::new(),
        });
        assert_eq!(result, Err(ApplicationError::MissingIdempotencyKey));
    }

    #[test]
    fn apply_requires_confirmation() {
        let orchestrator = Orchestrator::new(EmptyEnvironmentStore);
        let plan = orchestrator
            .plan_change(ChangeRequest {
                action: ChangeAction::Enable,
                family: RuntimeFamily::Php,
                runtime_id: Some(RuntimeId::new("php-8.3").expect("valid id")),
                idempotency_key: "retry-me".to_owned(),
            })
            .expect("plan succeeds");
        assert_eq!(
            orchestrator.apply_change(&plan, None),
            Err(ApplicationError::ConfirmationRequired)
        );
    }

    #[test]
    fn direct_active_transition_is_rejected() {
        assert!(validate_activation_change(Activation::Disabled, Activation::Active).is_err());
    }
}
