use super::*;
use cxszn_menv_core::windows::SqliteEnvironmentStore;

fn plan_error(code: &str, message: &str) -> ApplicationError {
    ApplicationError::ArtifactInstallFailed {
        code: code.to_owned(),
        message: message.to_owned(),
        retryable: false,
    }
}

pub(super) fn read_php_install_plans_from_store(
    store: &SqliteEnvironmentStore,
) -> Result<HashMap<String, PhpInstallPlanRecord>, ApplicationError> {
    store
        .load_php_install_plan_rows()?
        .into_iter()
        .map(|(id, payload)| {
            let record: PhpInstallPlanRecord = serde_json::from_str(&payload).map_err(|error| {
                ApplicationError::Serialization(format!("parse PHP install plan: {error}"))
            })?;
            if record.plan.plan_id != id {
                return Err(ApplicationError::PlanTampered);
            }
            // Keep terminal and expired records: their idempotency key must never revive a cancelled plan.
            Ok((id, record))
        })
        .collect()
}

pub(super) fn persist_php_install_plans_in_store(
    store: &SqliteEnvironmentStore,
    plans: &HashMap<String, PhpInstallPlanRecord>,
) -> Result<(), ApplicationError> {
    let rows = plans
        .iter()
        .map(|(id, record)| {
            serde_json::to_string(record)
                .map(|payload| (id.clone(), payload))
                .map_err(|error| ApplicationError::Serialization(error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    store
        .save_php_install_plan_rows(&rows)
        .map_err(ApplicationError::Port)
}

pub(super) fn reusable_php_install_plan(
    plans: &HashMap<String, PhpInstallPlanRecord>,
    idempotency_key: &str,
) -> Result<Option<ChangePlan>, ApplicationError> {
    if plans.values().any(|record| {
        record.plan.request.idempotency_key == idempotency_key
            && record.status == PhpInstallPlanStatus::Cancelled
    }) {
        return Err(plan_error(
            "PLAN_CANCELLED",
            "PHP install plan was cancelled",
        ));
    }
    Ok(plans
        .values()
        .find(|record| {
            record.plan.request.idempotency_key == idempotency_key
                && record.plan.expires_at_epoch_seconds > now_epoch_seconds()
        })
        .map(|record| record.plan.clone()))
}

// The caller holds the user-scoped operation mutex through this read/modify/write.
pub(super) fn cancel_php_install_plan_in_store(
    store: &SqliteEnvironmentStore,
    plan_id: &str,
) -> Result<HashMap<String, PhpInstallPlanRecord>, ApplicationError> {
    let mut plans = read_php_install_plans_from_store(store)?;
    let record = plans
        .get_mut(plan_id)
        .ok_or_else(|| plan_error("PLAN_REQUIRED", "PHP install plan was not found"))?;
    if record.result.is_some() || record.status == PhpInstallPlanStatus::Started {
        return Err(ApplicationError::OperationNotCancellable);
    }
    if record.status != PhpInstallPlanStatus::Cancelled {
        record.status = PhpInstallPlanStatus::Cancelled;
        persist_php_install_plans_in_store(store, &plans)?;
    }
    Ok(plans)
}

// Called only after both installation locks. No download can precede this durable boundary.
pub(super) fn prepare_php_install_apply_in_store(
    store: &SqliteEnvironmentStore,
    plan: &ChangePlan,
    token: Option<&ConfirmationToken>,
    service: &dyn cxszn_menv_core::application::ApplicationService,
) -> Result<PhpInstallPlanRecord, ApplicationError> {
    let mut plans = read_php_install_plans_from_store(store)?;
    let record = plans
        .get_mut(&plan.plan_id)
        .ok_or_else(|| plan_error("PLAN_REQUIRED", "PHP install plan was not found"))?;
    if record.status == PhpInstallPlanStatus::Cancelled {
        return Err(plan_error(
            "PLAN_CANCELLED",
            "PHP install plan was cancelled",
        ));
    }
    if record.plan != *plan || install_plan_hash(plan, &record.spec)? != plan.plan_hash {
        return Err(ApplicationError::PlanTampered);
    }
    validate_install_confirmation(plan, token, service)?;
    if record.result.is_none() {
        if snapshot_hash(&service.get_environment_state()?)? != plan.snapshot_hash {
            return Err(ApplicationError::StateChanged);
        }
        record.status = PhpInstallPlanStatus::Started;
        let result = record.clone();
        persist_php_install_plans_in_store(store, &plans)?;
        return Ok(result);
    }
    Ok(record.clone())
}
