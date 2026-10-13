use super::*;
use cxszn_menv_ports::ConfirmationRecord;
use std::collections::{hash_map::Entry, HashMap};
use std::sync::Mutex;

#[derive(Default)]
pub(super) struct MemoryConfirmationStore(Mutex<HashMap<String, ConfirmationRecord>>);

impl ConfirmationStore for MemoryConfirmationStore {
    fn insert(&self, record: &ConfirmationRecord) -> Result<(), PortError> {
        let mut records = self
            .0
            .lock()
            .map_err(|_| PortError::Platform("confirmation store lock is poisoned".into()))?;
        match records.entry(record.token_hash.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(record.clone());
                Ok(())
            }
            Entry::Occupied(_) => Err(PortError::Platform(
                "confirmation digest is already registered".into(),
            )),
        }
    }

    fn find(&self, token_hash: &str) -> Result<Option<ConfirmationRecord>, PortError> {
        Ok(self
            .0
            .lock()
            .map_err(|_| PortError::Platform("confirmation store lock is poisoned".into()))?
            .get(token_hash)
            .cloned())
    }
}

impl std::fmt::Debug for ConfirmationToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConfirmationToken")
            .field("value", &"<redacted>")
            .field("plan_id", &self.plan_id)
            .field("plan_hash", &self.plan_hash)
            .field("actor", &self.actor)
            .field("expires_at_epoch_seconds", &self.expires_at_epoch_seconds)
            .finish()
    }
}

impl<S: EnvironmentStore, P: UserPathStore> Orchestrator<S, P> {
    /// The trusted UI/interactive CLI calls this only after explicit approval.
    /// Artifact-bound plans are validated by their composition adapter; this
    /// issuer binds the approved digest rather than recomputing an install hash.
    pub fn issue_confirmation_token(
        &self,
        plan: &ChangePlan,
    ) -> Result<ConfirmationToken, ApplicationError> {
        if !matches!(
            plan.request.action,
            ChangeAction::Install | ChangeAction::Register
        ) && plan.plan_hash != hash_plan(plan)?
        {
            return Err(ApplicationError::PlanTampered);
        }
        let now = now_epoch_seconds();
        if plan.expires_at_epoch_seconds <= now {
            return Err(ApplicationError::PlanExpired);
        }
        if plan.expires_at_epoch_seconds > now.saturating_add(PLAN_TTL_SECONDS)
            || plan.plan_hash.len() != 64
            || !plan.plan_hash.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(ApplicationError::ConfirmationInvalid);
        }
        if plan.request.actor.trim().is_empty() {
            return Err(ApplicationError::MissingActor);
        }
        if plan.request.idempotency_key.trim().is_empty() {
            return Err(ApplicationError::MissingIdempotencyKey);
        }
        Uuid::parse_str(&plan.plan_id).map_err(|_| ApplicationError::PlanTampered)?;
        let token = ConfirmationToken {
            value: Uuid::new_v4().to_string(),
            plan_id: plan.plan_id.clone(),
            plan_hash: plan.plan_hash.clone(),
            actor: plan.request.actor.clone(),
            expires_at_epoch_seconds: plan.expires_at_epoch_seconds,
        };
        self.confirmation_store.insert(&ConfirmationRecord {
            token_hash: hash_bytes(token.value.as_bytes()),
            plan_id: token.plan_id.clone(),
            plan_hash: token.plan_hash.clone(),
            actor: token.actor.clone(),
            expires_at_epoch_seconds: token.expires_at_epoch_seconds,
        })?;
        Ok(token)
    }

    pub fn validate_confirmation_token(
        &self,
        plan: &ChangePlan,
        token: Option<&ConfirmationToken>,
    ) -> Result<(), ApplicationError> {
        let token = token.ok_or(ApplicationError::ConfirmationRequired)?;
        let now = now_epoch_seconds();
        if plan.expires_at_epoch_seconds <= now {
            return Err(ApplicationError::PlanExpired);
        }
        if token.value.trim().is_empty()
            || token.value.len() > 128
            || token.plan_id != plan.plan_id
            || token.plan_hash != plan.plan_hash
            || token.actor != plan.request.actor
            || token.expires_at_epoch_seconds != plan.expires_at_epoch_seconds
        {
            return Err(ApplicationError::ConfirmationInvalid);
        }
        let expected = ConfirmationRecord {
            token_hash: hash_bytes(token.value.as_bytes()),
            plan_id: token.plan_id.clone(),
            plan_hash: token.plan_hash.clone(),
            actor: token.actor.clone(),
            expires_at_epoch_seconds: token.expires_at_epoch_seconds,
        };
        if self.confirmation_store.find(&expected.token_hash)?.as_ref() != Some(&expected) {
            return Err(ApplicationError::ConfirmationInvalid);
        }
        Ok(())
    }
}
