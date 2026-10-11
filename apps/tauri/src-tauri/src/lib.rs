//! Tauri 2 composition root.
//!
//! Commands intentionally stay thin: state and mutation rules belong to the
//! shared Rust application service (and, during migration, the TypeScript core
//! contract). No arbitrary shell or filesystem command is exposed here.

use cxszn_menv_core::{
    application::{
        ApplicationError, ChangePlan, ChangeRequest, ConfirmationToken, OperationReceipt,
    },
    default_service,
    domain::EnvironmentSnapshot,
};
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

#[derive(Clone)]
pub struct AppState {
    service: Arc<cxszn_menv_core::DefaultService>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            service: Arc::new(default_service()),
        }
    }
}

#[derive(Debug, Serialize)]
struct ErrorPayload {
    code: String,
    message: String,
    retryable: bool,
    next_actions: Vec<String>,
}

#[derive(Debug, Serialize)]
struct Envelope<T> {
    schema_version: String,
    request_id: String,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ErrorPayload>,
}

impl<T> Envelope<T> {
    fn success(result: T) -> Self {
        Self {
            schema_version: "1".to_owned(),
            request_id: uuid::Uuid::new_v4().to_string(),
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    fn failure(error: ApplicationError) -> Self {
        Self {
            schema_version: "1".to_owned(),
            request_id: uuid::Uuid::new_v4().to_string(),
            ok: false,
            result: None,
            error: Some(error_payload(&error)),
        }
    }
}

#[tauri::command]
fn get_environment_state(state: State<'_, AppState>) -> Envelope<EnvironmentSnapshot> {
    state
        .service
        .get_environment_state()
        .map(Envelope::success)
        .unwrap_or_else(Envelope::failure)
}

#[tauri::command]
fn plan_environment_change(
    state: State<'_, AppState>,
    request: ChangeRequest,
) -> Envelope<ChangePlan> {
    state
        .service
        .plan_change(request)
        .map(Envelope::success)
        .unwrap_or_else(Envelope::failure)
}

#[tauri::command]
fn apply_environment_change(
    state: State<'_, AppState>,
    plan: ChangePlan,
    confirmation_token: Option<ConfirmationToken>,
) -> Envelope<OperationReceipt> {
    state
        .service
        .apply_change(&plan, confirmation_token.as_ref())
        .map(Envelope::success)
        .unwrap_or_else(Envelope::failure)
}

fn error_payload(error: &ApplicationError) -> ErrorPayload {
    let code = match error {
        ApplicationError::MissingIdempotencyKey => "IDEMPOTENCY_REQUIRED",
        ApplicationError::MissingActor => "ACTOR_REQUIRED",
        ApplicationError::ConfirmationRequired => "CONFIRMATION_REQUIRED",
        ApplicationError::ConfirmationInvalid => "CONFIRMATION_INVALID",
        ApplicationError::PlanExpired => "PLAN_EXPIRED",
        ApplicationError::PlanTampered => "PLAN_STALE",
        ApplicationError::StateChanged => "PLAN_STALE",
        ApplicationError::UnsupportedChangeAction => "UNSUPPORTED_ACTION",
        ApplicationError::MissingRuntimeId => "RUNTIME_REQUIRED",
        ApplicationError::MissingTargetRuntimeId => "TARGET_RUNTIME_REQUIRED",
        ApplicationError::RuntimeNotFound => "RUNTIME_NOT_FOUND",
        ApplicationError::RuntimeFamilyMismatch => "RUNTIME_FAMILY_MISMATCH",
        ApplicationError::IdempotencyConflict => "IDEMPOTENCY_CONFLICT",
        ApplicationError::InvalidActivationTransition => "INVALID_STATE_TRANSITION",
        ApplicationError::Serialization(_) => "SERIALIZATION_ERROR",
        ApplicationError::Port(_) => "PLATFORM_ERROR",
    }
    .to_owned();
    let retryable = matches!(
        error,
        ApplicationError::PlanExpired | ApplicationError::StateChanged
    );
    let next_actions = match error {
        ApplicationError::ConfirmationRequired => {
            vec!["Confirm the plan, then retry apply.".to_owned()]
        }
        ApplicationError::PlanExpired | ApplicationError::StateChanged => {
            vec!["Create a new plan and retry.".to_owned()]
        }
        _ => Vec::new(),
    };
    ErrorPayload {
        code,
        message: error.to_string(),
        retryable,
        next_actions,
    }
}

/// Keep the command list explicit so adding a new privileged operation requires
/// a reviewed capability and application-service method.
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            get_environment_state,
            plan_environment_change,
            apply_environment_change
        ])
        .run(tauri::generate_context!())
        .expect("error while running MEnv Tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_envelope_is_stable_and_does_not_expose_token_material() {
        let envelope: Envelope<EnvironmentSnapshot> =
            Envelope::failure(ApplicationError::ConfirmationInvalid);
        assert_eq!(envelope.schema_version, "1");
        assert!(!envelope.ok);
        let error = envelope.error.expect("error payload");
        assert_eq!(error.code, "CONFIRMATION_INVALID");
        assert!(!error.message.contains("host-issued"));
    }
}
