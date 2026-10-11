//! Tauri 2 composition root.
//!
//! Commands intentionally stay thin: state and mutation rules belong to the
//! shared Rust application service (and, during migration, the TypeScript core
//! contract). No arbitrary shell or filesystem command is exposed here.

use cxszn_menv_core::{application::ApplicationService, default_service, domain::EnvironmentSnapshot};
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
}

#[tauri::command]
fn get_environment_state(
    state: State<'_, AppState>,
) -> Result<Envelope<EnvironmentSnapshot>, String> {
    state
        .service
        .get_environment_state()
        .map(Envelope::success)
        .map_err(|error| error.to_string())
}

/// Keep the command list explicit so adding a new privileged operation requires
/// a reviewed capability and application-service method.
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![get_environment_state])
        .run(tauri::generate_context!())
        .expect("error while running MEnv Tauri application");
}
