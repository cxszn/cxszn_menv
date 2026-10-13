//! Tauri 2 composition root.
//!
//! Commands intentionally stay thin: state and mutation rules belong to the
//! shared Rust application service (and, during migration, the TypeScript core
//! contract). No arbitrary shell or filesystem command is exposed here.

#[cfg(windows)]
mod cli_confirmation;
#[cfg(all(test, windows))]
mod cli_confirmation_tests;
#[cfg(all(test, windows))]
mod native_confirmation_tests;
#[cfg(windows)]
mod php_install_plan;
#[cfg(all(test, windows))]
mod php_install_plan_cancel_tests;
#[cfg(windows)]
use php_install_plan::{
    cancel_php_install_plan_in_store, persist_php_install_plans_in_store,
    prepare_php_install_apply_in_store, read_php_install_plans_from_store,
    reusable_php_install_plan,
};
#[cfg(windows)]
mod php_catalog_inventory;
#[cfg(windows)]
mod php_ini_backup;
#[cfg(windows)]
use php_ini_backup::{apply_confirmed_backup, bind_backup_source, PhpIniBackupInfo};

use cxszn_menv_core::{
    application::{
        ApplicationError, ChangeAction, ChangePlan, ChangeRequest, ConfirmationToken,
        OperationReceipt,
    },
    default_service,
    domain::EnvironmentSnapshot,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsString;
#[cfg(windows)]
use std::io::Write;
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::State;
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    service: Arc<cxszn_menv_core::DefaultService>,
    #[cfg(windows)]
    php_install_plans: Arc<Mutex<HashMap<String, PhpInstallPlanRecord>>>,
    #[cfg(windows)]
    php_register_plans: Arc<Mutex<HashMap<String, PhpRegisterPlanRecord>>>,
    #[cfg(windows)]
    php_install_io_lock: Arc<Mutex<()>>,
    #[cfg(windows)]
    terminal_sessions: Arc<Mutex<HashMap<String, cxszn_menv_core::windows::conpty::ConptySession>>>,
    #[cfg(windows)]
    php_servers: Arc<Mutex<HashMap<String, PhpServerRecord>>>,
}

#[cfg(windows)]
#[derive(Clone, Debug, Deserialize, Serialize)]
struct PhpInstallPlanRecord {
    plan: ChangePlan,
    spec: cxszn_menv_core::windows::install::PhpArtifactInstallSpec,
    result: Option<EnvironmentSnapshot>,
    #[serde(default)]
    status: PhpInstallPlanStatus,
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum PhpInstallPlanStatus {
    #[default]
    Pending,
    Started,
    Cancelled,
}

#[cfg(windows)]
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistedPhpInstallPlans {
    schema_version: String,
    plans: Vec<PhpInstallPlanRecord>,
}

#[cfg(windows)]
#[derive(Clone, Debug, Deserialize, Serialize)]
struct PhpRegisterPlanRecord {
    plan: ChangePlan,
    runtime: cxszn_menv_core::domain::RuntimeRecord,
    result: Option<EnvironmentSnapshot>,
}

#[cfg(windows)]
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistedPhpRegisterPlans {
    schema_version: String,
    plans: Vec<PhpRegisterPlanRecord>,
}

#[cfg(windows)]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PhpRegisterPlanResult {
    plan: ChangePlan,
    runtime: cxszn_menv_core::domain::RuntimeRecord,
}

#[cfg(windows)]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PhpInstallPlanSummary {
    plan: ChangePlan,
    artifact: cxszn_menv_core::windows::PhpArtifactSummary,
}

#[cfg(windows)]
fn summarize_php_install_plans(
    plans: &HashMap<String, PhpInstallPlanRecord>,
    artifacts: &[cxszn_menv_core::windows::PhpArtifactSummary],
    now: u64,
) -> Vec<PhpInstallPlanSummary> {
    let mut summaries = plans
        .values()
        .filter(|record| {
            record.status != PhpInstallPlanStatus::Cancelled
                && record.result.is_none()
                && record.plan.expires_at_epoch_seconds > now
        })
        .filter_map(|record| {
            let artifact = artifacts
                .iter()
                .find(|artifact| artifact.file_name == record.spec.file_name)
                .cloned()?;
            Some(PhpInstallPlanSummary {
                plan: record.plan.clone(),
                artifact,
            })
        })
        .collect::<Vec<_>>();
    summaries.sort_by(|left, right| left.plan.plan_id.cmp(&right.plan.plan_id));
    summaries
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            service: Arc::new(default_service()),
            #[cfg(windows)]
            php_install_plans: Arc::new(Mutex::new(load_php_install_plans())),
            #[cfg(windows)]
            php_register_plans: Arc::new(Mutex::new(load_php_register_plans())),
            #[cfg(windows)]
            php_install_io_lock: Arc::new(Mutex::new(())),
            #[cfg(windows)]
            terminal_sessions: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(windows)]
            php_servers: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[cfg(windows)]
impl Drop for AppState {
    fn drop(&mut self) {
        if let Ok(mut servers) = self.php_servers.lock() {
            for server in servers.values_mut() {
                if server.child.try_wait().ok().flatten().is_none() {
                    let _ = server.child.kill();
                }
                let _ = server.child.wait();
            }
            servers.clear();
        }
    }
}

#[cfg(windows)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TerminalStartRequest {
    cwd: Option<String>,
}

#[cfg(windows)]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminalStartResult {
    session_id: String,
    cwd: String,
}

#[cfg(windows)]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TerminalEventPayload {
    kind: String,
    text: Option<String>,
    code: Option<String>,
}

#[cfg(windows)]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PhpIniResult {
    runtime_id: String,
    path: Option<String>,
    content: Option<String>,
    truncated: bool,
}

#[cfg(windows)]
struct PhpServerRecord {
    child: std::process::Child,
}

#[cfg(windows)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PhpServerStartRequest {
    runtime_id: String,
    docroot: String,
    host: String,
    port: u16,
}

#[cfg(windows)]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PhpServerInfo {
    server_id: String,
    runtime_id: String,
    docroot: String,
    endpoint: String,
    log_path: String,
}

#[cfg(windows)]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebServerDiagnostic {
    server: String,
    detected: bool,
    config_paths: Vec<String>,
    php_references: Vec<String>,
    warnings: Vec<String>,
}

#[cfg(windows)]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstallJournalLine {
    stage: String,
    managed_directory: String,
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
    load_native_environment_state(&state.service)
        .map(Envelope::success)
        .unwrap_or_else(Envelope::failure)
}

/// Desktop and native CLI must use the same recovery and locking boundary.
fn load_native_environment_state(
    service: &cxszn_menv_core::DefaultService,
) -> Result<EnvironmentSnapshot, ApplicationError> {
    #[cfg(windows)]
    let _process_operation_lock = acquire_runtime_operation_lock()?;
    #[cfg(windows)]
    recover_php_install_journals(service)?;
    service.get_environment_state()
}

/// Reconcile a published catalog install after an application crash. Only
/// journal entries under the fixed MEnv managed PHP root are considered; user
/// supplied paths and malformed records are ignored.
#[cfg(windows)]
fn recover_php_install_journals(
    service: &cxszn_menv_core::DefaultService,
) -> Result<(), ApplicationError> {
    let root = current_data_root()?.join("operations").join("php");
    let managed_root = current_data_root()?.join("versions").join("php");
    recover_php_install_journals_at(service, &root, &managed_root)
}

#[cfg(windows)]
fn recover_php_install_journals_at(
    service: &cxszn_menv_core::DefaultService,
    root: &Path,
    managed_root: &Path,
) -> Result<(), ApplicationError> {
    let mut candidates = Vec::new();
    let mut candidate_keys = HashSet::new();
    for path in collect_php_install_journals(root)? {
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(line) = content
            .lines()
            .rev()
            .find_map(|value| serde_json::from_str::<InstallJournalLine>(value).ok())
        else {
            continue;
        };
        if line.stage != "published" && line.stage != "registered" {
            continue;
        }
        let Some(executable) =
            safe_recovery_entrypoint(Path::new(&line.managed_directory), managed_root)
        else {
            continue;
        };
        let key = executable.to_string_lossy().to_ascii_lowercase();
        if candidate_keys.insert(key) {
            candidates.push(executable);
        }
    }
    let snapshot = service.get_environment_state()?;
    candidates.retain(|executable| {
        !snapshot.runtimes.iter().any(|runtime| {
            runtime
                .executable_path
                .eq_ignore_ascii_case(&executable.to_string_lossy())
                && runtime.availability == cxszn_menv_core::domain::Availability::Managed
        })
    });
    if candidates.is_empty() {
        return Ok(());
    }
    let snapshot = cxszn_menv_core::scan_php(service)?;
    for executable in candidates {
        if snapshot.runtimes.iter().any(|runtime| {
            runtime
                .executable_path
                .eq_ignore_ascii_case(&executable.to_string_lossy())
                && runtime.availability == cxszn_menv_core::domain::Availability::Managed
        }) {
            continue;
        }
        if executable.is_file() {
            let _ = service.mark_managed_runtime_disabled(&executable)?;
        }
    }
    Ok(())
}

/// Discover root-level and one-level-per-version PHP journals without
/// following symlinks or Windows reparse points. Catalog installs use
/// `operations\php\<version>\install.jsonl`; older previews may have written a
/// journal directly under `operations\php`, so both layouts remain readable.
#[cfg(windows)]
fn collect_php_install_journals(root: &Path) -> Result<Vec<PathBuf>, ApplicationError> {
    let metadata = match std::fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(ApplicationError::Port(
                cxszn_menv_core::ports::PortError::Platform(format!(
                    "read PHP install journal root: {error}"
                )),
            ));
        }
    };
    if !metadata.is_dir() || !safe_journal_metadata(&metadata) {
        return Err(ApplicationError::Port(
            cxszn_menv_core::ports::PortError::Platform(
                "PHP install journal root is not a regular directory".to_owned(),
            ),
        ));
    }
    let entries = std::fs::read_dir(root).map_err(|error| {
        ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(format!(
            "read PHP install journals: {error}"
        )))
    })?;
    let mut journals = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !safe_journal_metadata(&metadata) {
            continue;
        }
        if metadata.is_file() && is_jsonl_path(&path) {
            journals.push(path);
            continue;
        }
        if !metadata.is_dir() {
            continue;
        }
        let Ok(children) = std::fs::read_dir(&path) else {
            continue;
        };
        for child in children.flatten() {
            let child_path = child.path();
            let Ok(child_metadata) = std::fs::symlink_metadata(&child_path) else {
                continue;
            };
            if child_metadata.is_file()
                && safe_journal_metadata(&child_metadata)
                && is_jsonl_path(&child_path)
            {
                journals.push(child_path);
            }
        }
    }
    journals.sort_by(|left, right| left.to_string_lossy().cmp(&right.to_string_lossy()));
    Ok(journals)
}

#[cfg(windows)]
fn is_jsonl_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("jsonl"))
}

#[cfg(windows)]
fn safe_journal_metadata(metadata: &std::fs::Metadata) -> bool {
    !metadata.file_type().is_symlink() && metadata.file_attributes() & 0x400 == 0
}

/// Return a journal recovery entrypoint only when every path component stays
/// under the app-owned managed PHP root and none of the existing components is
/// a symlink/reparse point. Journal files are append-only application data but
/// remain untrusted after tampering or a partial disk write, so recovery must
/// fail closed instead of following a junction into a user-supplied location.
#[cfg(windows)]
fn safe_recovery_entrypoint(managed_directory: &Path, managed_root: &Path) -> Option<PathBuf> {
    let managed_directory =
        cxszn_menv_core::windows::validate_local_windows_path(managed_directory)
            .ok()
            .map(PathBuf::from)?;
    let managed_root = cxszn_menv_core::windows::validate_local_windows_path(managed_root)
        .ok()
        .map(PathBuf::from)?;
    let root_key = managed_root
        .to_string_lossy()
        .trim_end_matches('\\')
        .to_ascii_lowercase();
    let directory_key = managed_directory.to_string_lossy().to_ascii_lowercase();
    let root_prefix = format!("{root_key}\\");
    if !directory_key.starts_with(&root_prefix) {
        return None;
    }

    let entrypoint = managed_directory.join("php.exe");
    let entrypoint_key = entrypoint.to_string_lossy().to_ascii_lowercase();
    if !entrypoint_key.starts_with(&root_prefix) {
        return None;
    }
    let mut current = entrypoint.as_path();
    loop {
        let metadata = std::fs::symlink_metadata(current).ok()?;
        if metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0 {
            return None;
        }
        if current
            .to_string_lossy()
            .trim_end_matches('\\')
            .eq_ignore_ascii_case(&root_key)
        {
            break;
        }
        current = current.parent()?;
    }
    if !metadata_is_regular_file(&entrypoint) {
        return None;
    }
    Some(entrypoint)
}

#[cfg(windows)]
fn metadata_is_regular_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return false;
    };
    metadata.is_file()
        && !metadata.file_type().is_symlink()
        && metadata.file_attributes() & 0x400 == 0
}

#[tauri::command]
fn scan_installed_runtimes(state: State<'_, AppState>) -> Envelope<EnvironmentSnapshot> {
    #[cfg(windows)]
    let _process_operation_lock = match acquire_runtime_operation_lock() {
        Ok(lock) => lock,
        Err(error) => return Envelope::failure(error),
    };
    cxszn_menv_core::scan_runtimes(&state.service)
        .map(Envelope::success)
        .unwrap_or_else(Envelope::failure)
}

/// Verify an explicit PHP path and create a confirmation-bound manual
/// registration plan. Planning is read-only; the external executable is not
/// copied, changed, or enabled until `apply_php_path_registration` succeeds.
#[cfg(windows)]
#[tauri::command]
fn plan_php_path_registration(
    state: State<'_, AppState>,
    path: String,
    actor: String,
    idempotency_key: String,
) -> Envelope<PhpRegisterPlanResult> {
    let result = (|| {
        if actor.trim().is_empty() {
            return Err(ApplicationError::MissingActor);
        }
        if idempotency_key.trim().is_empty() {
            return Err(ApplicationError::MissingIdempotencyKey);
        }
        let provider = cxszn_menv_core::windows::WindowsPhpProvider;
        let runtime = cxszn_menv_core::ports::RuntimeProvider::verify_path(
            &provider,
            &cxszn_menv_core::ports::VerifyPathRequest {
                family: cxszn_menv_core::domain::RuntimeFamily::Php,
                path: PathBuf::from(path),
            },
        )?;
        let created_at = now_epoch_seconds();
        let snapshot = state.service.get_environment_state()?;
        let mut plan = ChangePlan {
            plan_id: Uuid::new_v4().to_string(),
            plan_hash: String::new(),
            request: ChangeRequest {
                action: ChangeAction::Register,
                family: cxszn_menv_core::domain::RuntimeFamily::Php,
                runtime_id: Some(runtime.id.clone()),
                target_runtime_id: None,
                idempotency_key,
                actor,
            },
            requires_confirmation: true,
            created_at_epoch_seconds: created_at,
            expires_at_epoch_seconds: created_at + 300,
            snapshot_hash: snapshot_hash(&snapshot)?,
            backup_source: None,
        };
        plan.plan_hash = registration_plan_hash(&plan, &runtime)?;
        let _process_operation_lock = acquire_runtime_operation_lock()?;
        let mut plans = state
            .php_register_plans
            .lock()
            .map_err(|_| poisoned_lock("PHP registration plan"))?;
        if let Some(existing) = plans.values().find(|record| {
            record.plan.request.idempotency_key == plan.request.idempotency_key
                && record.plan.expires_at_epoch_seconds > now_epoch_seconds()
        }) {
            return Ok(PhpRegisterPlanResult {
                plan: existing.plan.clone(),
                runtime: existing.runtime.clone(),
            });
        }
        let previous_plans = plans.clone();
        plans.insert(
            plan.plan_id.clone(),
            PhpRegisterPlanRecord {
                plan: plan.clone(),
                runtime: runtime.clone(),
                result: None,
            },
        );
        let current_plans = plans.clone();
        drop(plans);
        if let Err(error) = persist_php_register_plans(&current_plans) {
            *state
                .php_register_plans
                .lock()
                .map_err(|_| poisoned_lock("PHP registration plan"))? = previous_plans;
            return Err(error);
        }
        Ok(PhpRegisterPlanResult { plan, runtime })
    })();
    result
        .map(Envelope::success)
        .unwrap_or_else(Envelope::failure)
}

/// Apply a previously confirmed manual PHP path registration plan.
#[cfg(windows)]
#[tauri::command]
fn apply_php_path_registration(
    state: State<'_, AppState>,
    plan: ChangePlan,
    confirmation_token: Option<ConfirmationToken>,
) -> Envelope<EnvironmentSnapshot> {
    let result = (|| {
        let cached = {
            let plans = state
                .php_register_plans
                .lock()
                .map_err(|_| poisoned_lock("PHP registration plan"))?;
            let record = plans.get(&plan.plan_id).ok_or_else(|| {
                ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(
                    "PHP registration plan was not found".to_owned(),
                ))
            })?;
            if record.plan != plan
                || plan.plan_hash != registration_plan_hash(&plan, &record.runtime)?
            {
                return Err(ApplicationError::PlanTampered);
            }
            validate_php_registration_confirmation(
                &plan,
                confirmation_token.as_ref(),
                &*state.service,
            )?;
            record.result.clone()
        };
        if let Some(snapshot) = cached {
            return Ok(snapshot);
        }
        let _process_operation_lock = acquire_runtime_operation_lock()?;
        if snapshot_hash(&state.service.get_environment_state()?)? != plan.snapshot_hash {
            return Err(ApplicationError::StateChanged);
        }
        let runtime = {
            let plans = state
                .php_register_plans
                .lock()
                .map_err(|_| poisoned_lock("PHP registration plan"))?;
            plans
                .get(&plan.plan_id)
                .ok_or_else(|| {
                    ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(
                        "PHP registration plan was not found".to_owned(),
                    ))
                })?
                .runtime
                .clone()
        };
        let snapshot = apply_confirmed_php_registration(
            &state.service,
            &plan,
            confirmation_token.as_ref(),
            runtime,
        )?;
        let mut plans = state
            .php_register_plans
            .lock()
            .map_err(|_| poisoned_lock("PHP registration plan"))?;
        let record = plans.get_mut(&plan.plan_id).ok_or_else(|| {
            ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(
                "PHP registration plan was not found".to_owned(),
            ))
        })?;
        let (result, current_plans) = if let Some(existing) = record.result.clone() {
            (existing, plans.clone())
        } else {
            record.result = Some(snapshot.clone());
            (snapshot, plans.clone())
        };
        drop(plans);
        persist_php_register_plans(&current_plans)?;
        Ok(result)
    })();
    result
        .map(Envelope::success)
        .unwrap_or_else(Envelope::failure)
}

/// Read the selected PHP configuration without writing or executing it.
#[cfg(windows)]
#[tauri::command]
fn read_php_ini(state: State<'_, AppState>, runtime_id: String) -> Envelope<PhpIniResult> {
    let result = (|| {
        let snapshot = state.service.get_environment_state()?;
        let runtime = snapshot
            .runtimes
            .iter()
            .find(|runtime| runtime.id.as_str() == runtime_id)
            .ok_or(ApplicationError::RuntimeNotFound)?;
        if runtime.family != cxszn_menv_core::domain::RuntimeFamily::Php {
            return Err(ApplicationError::RuntimeFamilyMismatch);
        }
        let Some(path) = runtime.ini.path.as_ref() else {
            return Ok(PhpIniResult {
                runtime_id,
                path: None,
                content: None,
                truncated: false,
            });
        };
        let normalized =
            cxszn_menv_core::windows::validate_local_windows_path(PathBuf::from(path).as_path())
                .map_err(ApplicationError::Port)?;
        let ini_path = PathBuf::from(&normalized);
        let metadata = std::fs::symlink_metadata(&ini_path).map_err(|error| {
            ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(format!(
                "read php.ini metadata: {error}"
            )))
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(ApplicationError::Port(
                cxszn_menv_core::ports::PortError::InvalidPath(
                    "php.ini must be a regular local file".to_owned(),
                ),
            ));
        }
        const MAX_INI_BYTES: u64 = 512 * 1024;
        let bytes = std::fs::read(&ini_path).map_err(|error| {
            ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(format!(
                "read php.ini: {error}"
            )))
        })?;
        let truncated = bytes.len() as u64 > MAX_INI_BYTES;
        let content =
            String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_INI_BYTES as usize)]).into_owned();
        Ok(PhpIniResult {
            runtime_id,
            path: Some(normalized),
            content: Some(content),
            truncated,
        })
    })();
    result
        .map(Envelope::success)
        .unwrap_or_else(Envelope::failure)
}

#[cfg(windows)]
#[tauri::command]
fn apply_php_ini_backup(
    state: State<'_, AppState>,
    plan: ChangePlan,
    confirmation_token: Option<ConfirmationToken>,
) -> Envelope<PhpIniBackupInfo> {
    let result = (|| {
        let _process_operation_lock = acquire_runtime_operation_lock()?;
        apply_confirmed_backup(
            &*state.service,
            &plan,
            confirmation_token.as_ref(),
            &current_data_root()?,
        )
    })();
    result
        .map(Envelope::success)
        .unwrap_or_else(Envelope::failure)
}

/// Start the fixed PHP built-in server on loopback with an absolute docroot.
#[cfg(windows)]
#[tauri::command]
fn php_server_start(
    state: State<'_, AppState>,
    request: PhpServerStartRequest,
) -> Envelope<PhpServerInfo> {
    let result = (|| {
        if request.host != "127.0.0.1" && request.host != "localhost" {
            return Err(ApplicationError::Port(
                cxszn_menv_core::ports::PortError::InvalidPath(
                    "PHP built-in server only accepts loopback host".to_owned(),
                ),
            ));
        }
        if !(1024..=65535).contains(&request.port) {
            return Err(ApplicationError::Port(
                cxszn_menv_core::ports::PortError::InvalidPath(
                    "PHP server port must be between 1024 and 65535".to_owned(),
                ),
            ));
        }
        let snapshot = state.service.get_environment_state()?;
        let runtime = snapshot
            .runtimes
            .iter()
            .find(|runtime| runtime.id.as_str() == request.runtime_id)
            .ok_or(ApplicationError::RuntimeNotFound)?;
        if runtime.family != cxszn_menv_core::domain::RuntimeFamily::Php {
            return Err(ApplicationError::RuntimeFamilyMismatch);
        }
        let executable = PathBuf::from(cxszn_menv_core::windows::validate_local_windows_path(
            PathBuf::from(&runtime.executable_path).as_path(),
        )?);
        let docroot = PathBuf::from(cxszn_menv_core::windows::validate_local_windows_path(
            PathBuf::from(&request.docroot).as_path(),
        )?);
        let metadata = std::fs::symlink_metadata(&docroot).map_err(|error| {
            ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(format!(
                "read PHP docroot: {error}"
            )))
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(ApplicationError::Port(
                cxszn_menv_core::ports::PortError::InvalidPath(
                    "PHP docroot must be a regular local directory".to_owned(),
                ),
            ));
        }
        let server_id = Uuid::new_v4().to_string();
        let log_dir = current_data_root()?.join("logs").join("php-server");
        std::fs::create_dir_all(&log_dir).map_err(|error| {
            ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(format!(
                "create PHP server log directory: {error}"
            )))
        })?;
        let log_path = log_dir.join(format!("{server_id}.log"));
        let log_file = std::fs::File::create(&log_path).map_err(|error| {
            ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(format!(
                "create PHP server log: {error}"
            )))
        })?;
        let endpoint = format!("127.0.0.1:{}", request.port);
        let child = std::process::Command::new(&executable)
            .args(["-n", "-S", &endpoint, "-t", &docroot.to_string_lossy()])
            .current_dir(&docroot)
            .stdin(std::process::Stdio::null())
            .stdout(log_file.try_clone().map_err(|error| {
                ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(format!(
                    "clone PHP server log: {error}"
                )))
            })?)
            .stderr(log_file)
            .spawn()
            .map_err(|error| {
                ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(format!(
                    "start PHP built-in server: {error}"
                )))
            })?;
        let info = PhpServerInfo {
            server_id: server_id.clone(),
            runtime_id: request.runtime_id.clone(),
            docroot: docroot.to_string_lossy().replace('/', "\\"),
            endpoint,
            log_path: log_path.to_string_lossy().replace('/', "\\"),
        };
        state
            .php_servers
            .lock()
            .map_err(|_| poisoned_lock("PHP servers"))?
            .insert(server_id, PhpServerRecord { child });
        Ok(info)
    })();
    result
        .map(Envelope::success)
        .unwrap_or_else(Envelope::failure)
}

#[cfg(windows)]
#[tauri::command]
fn php_server_stop(state: State<'_, AppState>, server_id: String) -> Envelope<()> {
    let result = (|| {
        let mut servers = state
            .php_servers
            .lock()
            .map_err(|_| poisoned_lock("PHP servers"))?;
        let mut server = servers.remove(&server_id).ok_or_else(|| {
            ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(
                "PHP server was not found".to_owned(),
            ))
        })?;
        if server
            .child
            .try_wait()
            .map_err(|error| {
                ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(format!(
                    "check PHP server: {error}"
                )))
            })?
            .is_none()
        {
            let _ = server.child.kill();
        }
        let _ = server.child.wait();
        Ok(())
    })();
    result
        .map(Envelope::success)
        .unwrap_or_else(Envelope::failure)
}

#[cfg(windows)]
#[tauri::command]
fn php_server_status(state: State<'_, AppState>, server_id: String) -> Envelope<bool> {
    let result = (|| {
        let mut servers = state
            .php_servers
            .lock()
            .map_err(|_| poisoned_lock("PHP servers"))?;
        let Some(server) = servers.get_mut(&server_id) else {
            return Ok(false);
        };
        if server
            .child
            .try_wait()
            .map_err(|error| {
                ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(format!(
                    "check PHP server: {error}"
                )))
            })?
            .is_some()
        {
            servers.remove(&server_id);
            return Ok(false);
        }
        Ok(true)
    })();
    result
        .map(Envelope::success)
        .unwrap_or_else(Envelope::failure)
}

/// Inspect fixed local IIS/Apache/Nginx configuration locations without
/// changing or restarting any external service.
#[cfg(windows)]
#[tauri::command]
fn diagnose_web_servers() -> Envelope<Vec<WebServerDiagnostic>> {
    let result = (|| {
        let system_root = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let mut candidates = vec![
            (
                "IIS".to_owned(),
                vec![system_root
                    .join("System32")
                    .join("inetsrv")
                    .join("config")
                    .join("applicationHost.config")],
            ),
            (
                "Apache".to_owned(),
                vec![PathBuf::from(r"C:\Apache24\conf\httpd.conf")],
            ),
            (
                "Nginx".to_owned(),
                vec![PathBuf::from(r"C:\nginx\conf\nginx.conf")],
            ),
        ];
        for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(root) = std::env::var_os(variable).map(PathBuf::from) {
                candidates[1]
                    .1
                    .push(root.join("Apache24").join("conf").join("httpd.conf"));
                candidates[2]
                    .1
                    .push(root.join("nginx").join("conf").join("nginx.conf"));
            }
        }
        let mut results = Vec::new();
        for (server, paths) in candidates {
            let existing = paths
                .into_iter()
                .filter(|path| path.is_file())
                .collect::<Vec<_>>();
            let mut references = Vec::new();
            let mut warnings = Vec::new();
            for path in &existing {
                let bytes = std::fs::read(path).map_err(|error| {
                    ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(format!(
                        "read {server} config: {error}"
                    )))
                })?;
                let text = String::from_utf8_lossy(&bytes[..bytes.len().min(512 * 1024)]);
                for line in text.lines().filter(|line| {
                    let lower = line.to_ascii_lowercase();
                    lower.contains("php") || lower.contains("fastcgi")
                }) {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() && !references.iter().any(|item| item == trimmed) {
                        references.push(trimmed.to_owned());
                    }
                }
                if bytes.len() > 512 * 1024 {
                    warnings.push(format!(
                        "{} config exceeds the 512 KiB inspection limit",
                        server
                    ));
                }
            }
            if !existing.is_empty() && references.is_empty() {
                warnings
                    .push("config was found, but no PHP/FastCGI reference was detected".to_owned());
            }
            results.push(WebServerDiagnostic {
                server,
                detected: !existing.is_empty(),
                config_paths: existing
                    .into_iter()
                    .map(|path| path.to_string_lossy().replace('/', "\\"))
                    .collect(),
                php_references: references,
                warnings,
            });
        }
        Ok(results)
    })();
    result
        .map(Envelope::success)
        .unwrap_or_else(Envelope::failure)
}

/// Start a fixed Windows PowerShell session through the shared ConPTY adapter.
/// The caller can choose only the working directory; executable and argv stay
/// application-owned so this command is not an arbitrary shell launcher.
#[cfg(windows)]
#[tauri::command]
fn terminal_start(
    state: State<'_, AppState>,
    request: TerminalStartRequest,
) -> Envelope<TerminalStartResult> {
    let result = (|| {
        let system_root = std::env::var_os("SystemRoot").ok_or_else(|| {
            ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(
                "SystemRoot is not available".to_owned(),
            ))
        })?;
        let executable = PathBuf::from(system_root)
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe");
        let cwd = request
            .cwd
            .map(PathBuf::from)
            .unwrap_or(std::env::current_dir().map_err(|error| {
                ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(format!(
                    "resolve terminal working directory: {error}"
                )))
            })?);
        let base_environment = std::env::vars_os().collect::<BTreeMap<_, _>>();
        let snapshot = state.service.get_environment_state()?;
        let environment = terminal_environment(&base_environment, &snapshot);
        let config = cxszn_menv_core::windows::conpty::ConptyConfig {
            executable,
            args: vec![
                "-NoLogo".into(),
                "-NoProfile".into(),
                "-ExecutionPolicy".into(),
                "Bypass".into(),
            ],
            cwd: cwd.clone(),
            environment,
            columns: 120,
            rows: 32,
            output_limit_bytes: 8 * 1024 * 1024,
            timeout: std::time::Duration::from_secs(30 * 60),
        };
        let session =
            cxszn_menv_core::windows::conpty::ConptySession::start(config).map_err(|error| {
                ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(
                    error.to_string(),
                ))
            })?;
        let session_id = Uuid::new_v4().to_string();
        state
            .terminal_sessions
            .lock()
            .map_err(|_| poisoned_lock("terminal sessions"))?
            .insert(session_id.clone(), session);
        Ok(TerminalStartResult {
            session_id,
            cwd: cwd.to_string_lossy().replace('/', "\\"),
        })
    })();
    result
        .map(Envelope::success)
        .unwrap_or_else(Envelope::failure)
}

#[cfg(windows)]
fn terminal_environment(
    base: &BTreeMap<OsString, OsString>,
    snapshot: &EnvironmentSnapshot,
) -> BTreeMap<OsString, OsString> {
    let mut environment = base.clone();
    if snapshot.path_lease.enabled && !snapshot.path_lease.entries.is_empty() {
        environment.insert(
            OsString::from("PATH"),
            OsString::from(snapshot.path_lease.entries.join(";")),
        );
    }
    environment
}

#[cfg(windows)]
#[tauri::command]
fn terminal_send_input(
    state: State<'_, AppState>,
    session_id: String,
    input: String,
) -> Envelope<()> {
    let result = (|| {
        if session_id.trim().is_empty() {
            return Err(ApplicationError::Port(
                cxszn_menv_core::ports::PortError::InvalidPath(
                    "terminal session id is required".to_owned(),
                ),
            ));
        }
        let sessions = state
            .terminal_sessions
            .lock()
            .map_err(|_| poisoned_lock("terminal sessions"))?;
        let session = sessions.get(&session_id).ok_or_else(|| {
            ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(
                "terminal session was not found".to_owned(),
            ))
        })?;
        session.send_input(input.as_bytes()).map_err(|error| {
            ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(
                error.to_string(),
            ))
        })
    })();
    result
        .map(Envelope::success)
        .unwrap_or_else(Envelope::failure)
}

#[cfg(windows)]
#[tauri::command]
fn terminal_poll(
    state: State<'_, AppState>,
    session_id: String,
) -> Envelope<Vec<TerminalEventPayload>> {
    let result = (|| {
        let mut sessions = state
            .terminal_sessions
            .lock()
            .map_err(|_| poisoned_lock("terminal sessions"))?;
        let session = sessions.get_mut(&session_id).ok_or_else(|| {
            ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(
                "terminal session was not found".to_owned(),
            ))
        })?;
        let mut events = Vec::new();
        loop {
            match session.recv(std::time::Duration::ZERO) {
                Ok(Some(cxszn_menv_core::windows::conpty::ConptyEvent::Output(bytes))) => {
                    events.push(TerminalEventPayload {
                        kind: "output".to_owned(),
                        text: Some(String::from_utf8_lossy(&bytes).into_owned()),
                        code: None,
                    });
                }
                Ok(Some(cxszn_menv_core::windows::conpty::ConptyEvent::Closed)) => {
                    events.push(TerminalEventPayload {
                        kind: "closed".to_owned(),
                        text: None,
                        code: None,
                    });
                    break;
                }
                Ok(Some(cxszn_menv_core::windows::conpty::ConptyEvent::Exited(code))) => {
                    events.push(TerminalEventPayload {
                        kind: "exited".to_owned(),
                        text: None,
                        code: Some(code.to_string()),
                    });
                }
                Ok(Some(cxszn_menv_core::windows::conpty::ConptyEvent::Error(error))) => {
                    return Err(ApplicationError::Port(
                        cxszn_menv_core::ports::PortError::Platform(error.to_string()),
                    ));
                }
                Ok(None) => break,
                Err(error) => {
                    return Err(ApplicationError::Port(
                        cxszn_menv_core::ports::PortError::Platform(error.to_string()),
                    ));
                }
            }
        }
        Ok(events)
    })();
    result
        .map(Envelope::success)
        .unwrap_or_else(Envelope::failure)
}

#[cfg(windows)]
#[tauri::command]
fn terminal_close(state: State<'_, AppState>, session_id: String) -> Envelope<()> {
    let result = (|| {
        let mut sessions = state
            .terminal_sessions
            .lock()
            .map_err(|_| poisoned_lock("terminal sessions"))?;
        let mut session = sessions.remove(&session_id).ok_or_else(|| {
            ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(
                "terminal session was not found".to_owned(),
            ))
        })?;
        session.shutdown().map_err(|error| {
            ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(
                error.to_string(),
            ))
        })
    })();
    result
        .map(Envelope::success)
        .unwrap_or_else(Envelope::failure)
}

/// List the embedded, verified PHP catalog without touching the network or
/// filesystem. Installation state is joined from the shared environment
/// snapshot so a catalog artifact is never presented as installed by default.
#[cfg(windows)]
#[tauri::command]
fn list_php_catalog(
    state: State<'_, AppState>,
) -> Envelope<Vec<cxszn_menv_core::windows::PhpArtifactSummary>> {
    let result = (|| {
        let catalog = cxszn_menv_core::windows::embedded_php_catalog().map_err(|error| {
            ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(
                error.to_string(),
            ))
        })?;
        let snapshot = state.service.get_environment_state()?;
        let mut artifacts =
            cxszn_menv_core::windows::list_php_artifacts(&catalog).map_err(|error| {
                ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(
                    error.to_string(),
                ))
            })?;
        php_catalog_inventory::mark_installed(&mut artifacts, &snapshot.runtimes);
        Ok::<_, ApplicationError>(artifacts)
    })();
    result
        .map(Envelope::success)
        .unwrap_or_else(Envelope::failure)
}

/// Return unexpired, not-yet-completed PHP install plans restored from the
/// durable plan file. Listing is read-only; the plan still requires a fresh
/// UI confirmation token before any download or extraction can start.
#[cfg(windows)]
#[tauri::command]
fn list_php_install_plans() -> Envelope<Vec<PhpInstallPlanSummary>> {
    let result = (|| {
        let catalog = cxszn_menv_core::windows::embedded_php_catalog().map_err(|error| {
            ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(
                error.to_string(),
            ))
        })?;
        let artifacts =
            cxszn_menv_core::windows::list_php_artifacts(&catalog).map_err(|error| {
                ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(
                    error.to_string(),
                ))
            })?;
        let plans = read_php_install_plans_from_store(
            &cxszn_menv_core::windows::SqliteEnvironmentStore::default_for_current_user(),
        )?;
        Ok(summarize_php_install_plans(
            &plans,
            &artifacts,
            now_epoch_seconds(),
        ))
    })();
    result
        .map(Envelope::success)
        .unwrap_or_else(Envelope::failure)
}

/// Build a download/install plan. The plan is stored in memory and no bytes
/// are downloaded until the user confirms it through `apply_php_install`.
#[cfg(windows)]
#[tauri::command]
fn plan_php_install(
    state: State<'_, AppState>,
    artifact_id: String,
    actor: String,
    idempotency_key: String,
) -> Envelope<ChangePlan> {
    let result = (|| {
        if actor.trim().is_empty() {
            return Err(ApplicationError::MissingActor);
        }
        if idempotency_key.trim().is_empty() {
            return Err(ApplicationError::MissingIdempotencyKey);
        }
        let _process_operation_lock = acquire_runtime_operation_lock()?;
        let catalog = cxszn_menv_core::windows::embedded_php_catalog().map_err(|error| {
            ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(
                error.to_string(),
            ))
        })?;
        let data_root = current_data_root()?;
        let spec =
            cxszn_menv_core::windows::create_php_install_spec(&catalog, &artifact_id, &data_root)
                .map_err(|error| {
                ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(
                    error.to_string(),
                ))
            })?;
        let created_at = now_epoch_seconds();
        let mut plan = ChangePlan {
            plan_id: Uuid::new_v4().to_string(),
            plan_hash: String::new(),
            request: ChangeRequest {
                action: ChangeAction::Install,
                family: cxszn_menv_core::domain::RuntimeFamily::Php,
                runtime_id: None,
                target_runtime_id: None,
                idempotency_key,
                actor,
            },
            requires_confirmation: true,
            created_at_epoch_seconds: created_at,
            expires_at_epoch_seconds: created_at + 300,
            snapshot_hash: snapshot_hash(&state.service.get_environment_state()?)?,
            backup_source: None,
        };
        plan.plan_hash = install_plan_hash(&plan, &spec)?;
        let store = cxszn_menv_core::windows::SqliteEnvironmentStore::default_for_current_user();
        let mut plans = read_php_install_plans_from_store(&store)?;
        if let Some(existing) = reusable_php_install_plan(&plans, &plan.request.idempotency_key)? {
            return Ok(existing);
        }
        plans.insert(
            plan.plan_id.clone(),
            PhpInstallPlanRecord {
                plan: plan.clone(),
                spec,
                result: None,
                status: PhpInstallPlanStatus::Pending,
            },
        );
        persist_php_install_plans_in_store(&store, &plans)?;
        *state
            .php_install_plans
            .lock()
            .map_err(|_| poisoned_lock("PHP install plan"))? = plans;
        Ok(plan)
    })();
    result
        .map(Envelope::success)
        .unwrap_or_else(Envelope::failure)
}

/// Cancel only a durable plan whose installer has never started.
#[cfg(windows)]
#[tauri::command]
fn cancel_php_install(state: State<'_, AppState>, plan_id: String) -> Envelope<()> {
    let result = (|| {
        let _process_operation_lock = acquire_runtime_operation_lock()?;
        let store = cxszn_menv_core::windows::SqliteEnvironmentStore::default_for_current_user();
        let plans = cancel_php_install_plan_in_store(&store, &plan_id)?;
        *state
            .php_install_plans
            .lock()
            .map_err(|_| poisoned_lock("PHP install plan"))? = plans;
        Ok(())
    })();
    result
        .map(Envelope::success)
        .unwrap_or_else(Envelope::failure)
}

/// Apply a previously planned PHP download after the UI confirmation token
/// has been issued. The same plan is idempotent within the running app.
#[cfg(windows)]
#[tauri::command]
fn apply_php_install(
    state: State<'_, AppState>,
    plan: ChangePlan,
    confirmation_token: Option<ConfirmationToken>,
) -> Envelope<EnvironmentSnapshot> {
    let result = (|| {
        let _process_operation_lock = acquire_runtime_operation_lock()?;
        // Keep the plan map available to catalog listing and new plan requests
        // while the synchronous network and ZIP work runs. Installation I/O is
        // serialized so two applies cannot publish the same managed version or
        // race the JSON environment store.
        let _install_guard = state
            .php_install_io_lock
            .lock()
            .map_err(|_| poisoned_lock("PHP install I/O"))?;
        let store = cxszn_menv_core::windows::SqliteEnvironmentStore::default_for_current_user();
        // Read authority and check expiry after both locks: waiting cannot extend approval.
        let record = prepare_php_install_apply_in_store(
            &store,
            &plan,
            confirmation_token.as_ref(),
            &*state.service,
        )?;
        if let Some(result) = record.result {
            return Ok(result);
        }
        let spec = record.spec;
        let installed_path = spec.managed_directory.join(&spec.entrypoint);
        match cxszn_menv_core::windows::install::install_php_artifact(&spec) {
            Ok(_) => {}
            Err(cxszn_menv_core::windows::install::InstallError::ManagedVersionExists) => {
                // A process crash can happen after the atomic directory publish
                // and before the plan result is cached. Re-scan and reconcile a
                // complete published runtime instead of downloading again.
                let _ = cxszn_menv_core::scan_php(&state.service)?;
                let snapshot = state.service.get_environment_state()?;
                if !snapshot.runtimes.iter().any(|runtime| {
                    runtime
                        .executable_path
                        .eq_ignore_ascii_case(&installed_path.to_string_lossy())
                }) {
                    return Err(ApplicationError::Port(
                        cxszn_menv_core::ports::PortError::Platform(
                            "managed version exists but its PHP entrypoint was not found"
                                .to_owned(),
                        ),
                    ));
                }
            }
            Err(error) => {
                return Err(map_catalog_install_error(error));
            }
        }
        let _ = cxszn_menv_core::scan_php(&state.service)?;
        // The application service persists the ownership/activation update;
        // mutating only the scan result would make the next command forget
        // that the downloaded version is managed.  Registration deliberately
        // leaves activation disabled until a separate enable plan is applied.
        state
            .service
            .mark_managed_runtime_disabled(&installed_path)?;
        let snapshot = state.service.record_install_operation(
            &installed_path,
            &plan.plan_id,
            &plan.request.idempotency_key,
        )?;
        cxszn_menv_core::windows::install::append_install_journal(&spec, "registered", None)
            .map_err(|error| {
                ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(
                    error.to_string(),
                ))
            })?;
        let (result, current_plans) = {
            let mut plans = read_php_install_plans_from_store(&store)?;
            let record = plans.get_mut(&plan.plan_id).ok_or_else(|| {
                ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(
                    "PHP install plan was not found".to_owned(),
                ))
            })?;
            if let Some(result) = record.result.clone() {
                (result, plans.clone())
            } else {
                record.result = Some(snapshot.clone());
                (snapshot, plans.clone())
            }
        };
        persist_php_install_plans_in_store(&store, &current_plans)?;
        *state
            .php_install_plans
            .lock()
            .map_err(|_| poisoned_lock("PHP install plan"))? = current_plans;
        Ok(result)
    })();
    result
        .map(Envelope::success)
        .unwrap_or_else(Envelope::failure)
}

#[tauri::command]
fn plan_environment_change(
    state: State<'_, AppState>,
    request: ChangeRequest,
) -> Envelope<ChangePlan> {
    let result = (|| {
        #[cfg(windows)]
        let _process_operation_lock = acquire_runtime_operation_lock()?;
        let plan = state.service.plan_change(request)?;
        #[cfg(windows)]
        let plan = {
            let mut plan = plan;
            if plan.request.action == ChangeAction::BackupIni {
                bind_backup_source(&*state.service, &mut plan, &current_data_root()?)?;
            }
            plan
        };
        Ok::<_, ApplicationError>(plan)
    })();
    result
        .map(Envelope::success)
        .unwrap_or_else(Envelope::failure)
}

#[tauri::command]
fn apply_environment_change(
    state: State<'_, AppState>,
    plan: ChangePlan,
    confirmation_token: Option<ConfirmationToken>,
) -> Envelope<OperationReceipt> {
    if plan.request.action == ChangeAction::BackupIni {
        return Envelope::failure(ApplicationError::UnsupportedChangeAction);
    }
    #[cfg(windows)]
    let _process_operation_lock = match acquire_runtime_operation_lock() {
        Ok(lock) => lock,
        Err(error) => return Envelope::failure(error),
    };
    state
        .service
        .apply_change(&plan, confirmation_token.as_ref())
        .map(Envelope::success)
        .unwrap_or_else(Envelope::failure)
}

/// Issue a short-lived host token only after the desktop confirmation dialog
/// has been accepted. The token is bound to the exact plan and actor; it does
/// not mutate PATH or runtime state by itself.
#[tauri::command]
fn issue_confirmation_token(
    state: State<'_, AppState>,
    plan: ChangePlan,
) -> Envelope<ConfirmationToken> {
    let result = (|| {
        #[cfg(windows)]
        let _process_operation_lock = acquire_runtime_operation_lock()?;
        #[cfg(windows)]
        match plan.request.action {
            ChangeAction::Install => {
                let plans = read_php_install_plans_from_store(
                    &cxszn_menv_core::windows::SqliteEnvironmentStore::default_for_current_user(),
                )?;
                let record = plans
                    .get(&plan.plan_id)
                    .ok_or(ApplicationError::PlanTampered)?;
                if record.status == PhpInstallPlanStatus::Cancelled {
                    return Err(ApplicationError::ArtifactInstallFailed {
                        code: "PLAN_CANCELLED".to_owned(),
                        message: "PHP install plan was cancelled".to_owned(),
                        retryable: false,
                    });
                }
                if record.plan != plan || install_plan_hash(&plan, &record.spec)? != plan.plan_hash
                {
                    return Err(ApplicationError::PlanTampered);
                }
            }
            ChangeAction::Register => {
                let plans = state
                    .php_register_plans
                    .lock()
                    .map_err(|_| poisoned_lock("PHP registration plan"))?;
                let record = plans
                    .get(&plan.plan_id)
                    .ok_or(ApplicationError::PlanTampered)?;
                if record.plan != plan {
                    return Err(ApplicationError::PlanTampered);
                }
                validate_native_plan_payload(&plan, Some(&record.runtime))?;
            }
            _ => validate_native_plan_payload(&plan, None)?,
        }
        state.service.issue_confirmation_token(&plan)
    })();
    result
        .map(Envelope::success)
        .unwrap_or_else(Envelope::failure)
}

#[cfg(windows)]
fn current_data_root() -> Result<PathBuf, ApplicationError> {
    let local_app_data = std::env::var_os("LOCALAPPDATA").ok_or_else(|| {
        ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(
            "LOCALAPPDATA is not available".to_owned(),
        ))
    })?;
    Ok(PathBuf::from(local_app_data).join("cxszn-menv"))
}

#[cfg(windows)]
fn php_install_plans_path() -> Result<PathBuf, ApplicationError> {
    Ok(current_data_root()?
        .join("plans")
        .join("php-install-plans.json"))
}

#[cfg(all(windows, test))]
fn encode_php_install_plans(
    plans: &HashMap<String, PhpInstallPlanRecord>,
) -> Result<Vec<u8>, ApplicationError> {
    let mut records = plans.values().cloned().collect::<Vec<_>>();
    records.sort_by(|left, right| left.plan.plan_id.cmp(&right.plan.plan_id));
    serde_json::to_vec_pretty(&PersistedPhpInstallPlans {
        schema_version: "1".to_owned(),
        plans: records,
    })
    .map_err(|error| ApplicationError::Serialization(error.to_string()))
}

#[cfg(windows)]
fn decode_php_install_plans(
    bytes: &[u8],
) -> Result<HashMap<String, PhpInstallPlanRecord>, ApplicationError> {
    let payload: PersistedPhpInstallPlans = serde_json::from_slice(bytes).map_err(|error| {
        ApplicationError::Serialization(format!("parse PHP install plans: {error}"))
    })?;
    if payload.schema_version != "1" {
        return Err(ApplicationError::Serialization(
            "unsupported PHP install plan schema".to_owned(),
        ));
    }
    Ok(payload
        .plans
        .into_iter()
        .map(|record| (record.plan.plan_id.clone(), record))
        .collect())
}

#[cfg(windows)]
fn load_php_install_plans() -> HashMap<String, PhpInstallPlanRecord> {
    let Ok(_process_operation_lock) = acquire_runtime_operation_lock() else {
        return HashMap::new();
    };
    let sqlite_store = cxszn_menv_core::windows::SqliteEnvironmentStore::default_for_current_user();
    match read_php_install_plans_from_store(&sqlite_store) {
        Ok(plans) if !plans.is_empty() => return plans,
        Ok(_) => {}
        Err(error) => {
            eprintln!("MEnv could not read SQLite PHP install plans: {error}");
            return HashMap::new();
        }
    }

    // One-time compatibility migration. The legacy file is intentionally left
    // in place so a user can recover it if the preview is rolled back.
    let Ok(path) = php_install_plans_path() else {
        return HashMap::new();
    };
    let candidates = [path.clone(), path.with_extension("json.bak")];
    for candidate in candidates {
        let Ok(bytes) = std::fs::read(&candidate) else {
            continue;
        };
        if let Ok(plans) = decode_php_install_plans(&bytes) {
            let rows = plans
                .iter()
                .filter_map(|(plan_id, record)| {
                    serde_json::to_string(record)
                        .ok()
                        .map(|payload| (plan_id.clone(), payload))
                })
                .collect::<Vec<_>>();
            if let Err(error) = sqlite_store.save_php_install_plan_rows(&rows) {
                eprintln!("MEnv could not migrate PHP install plans to SQLite: {error}");
            }
            return plans;
        }
    }
    HashMap::new()
}

#[cfg(windows)]
fn php_register_plans_path() -> Result<PathBuf, ApplicationError> {
    Ok(current_data_root()?
        .join("plans")
        .join("php-register-plans.json"))
}

#[cfg(windows)]
fn encode_php_register_plans(
    plans: &HashMap<String, PhpRegisterPlanRecord>,
) -> Result<Vec<u8>, ApplicationError> {
    let mut records = plans.values().cloned().collect::<Vec<_>>();
    records.sort_by(|left, right| left.plan.plan_id.cmp(&right.plan.plan_id));
    serde_json::to_vec_pretty(&PersistedPhpRegisterPlans {
        schema_version: "1".to_owned(),
        plans: records,
    })
    .map_err(|error| ApplicationError::Serialization(error.to_string()))
}

#[cfg(windows)]
fn decode_php_register_plans(
    bytes: &[u8],
) -> Result<HashMap<String, PhpRegisterPlanRecord>, ApplicationError> {
    let payload: PersistedPhpRegisterPlans = serde_json::from_slice(bytes).map_err(|error| {
        ApplicationError::Serialization(format!("parse PHP registration plans: {error}"))
    })?;
    if payload.schema_version != "1" {
        return Err(ApplicationError::Serialization(
            "unsupported PHP registration plan schema".to_owned(),
        ));
    }
    Ok(payload
        .plans
        .into_iter()
        .filter(|record| record.plan.expires_at_epoch_seconds > now_epoch_seconds())
        .map(|record| (record.plan.plan_id.clone(), record))
        .collect())
}

#[cfg(windows)]
fn load_php_register_plans() -> HashMap<String, PhpRegisterPlanRecord> {
    let Ok(path) = php_register_plans_path() else {
        return HashMap::new();
    };
    for candidate in [path.clone(), path.with_extension("json.bak")] {
        let Ok(bytes) = std::fs::read(&candidate) else {
            continue;
        };
        if let Ok(plans) = decode_php_register_plans(&bytes) {
            return plans;
        }
    }
    HashMap::new()
}

#[cfg(windows)]
fn persist_php_register_plans(
    plans: &HashMap<String, PhpRegisterPlanRecord>,
) -> Result<(), ApplicationError> {
    let path = php_register_plans_path()?;
    let parent = path.parent().ok_or_else(|| {
        ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(
            "PHP registration plan path has no parent".to_owned(),
        ))
    })?;
    std::fs::create_dir_all(parent).map_err(|error| {
        ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(format!(
            "create PHP registration plan directory: {error}"
        )))
    })?;
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let backup = path.with_extension("json.bak");
    let bytes = encode_php_register_plans(plans)?;
    let mut file = std::fs::File::create(&temporary).map_err(|error| {
        ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(format!(
            "create PHP registration plan: {error}"
        )))
    })?;
    file.write_all(&bytes).map_err(|error| {
        ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(format!(
            "write PHP registration plan: {error}"
        )))
    })?;
    file.sync_all().map_err(|error| {
        ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(format!(
            "flush PHP registration plan: {error}"
        )))
    })?;
    drop(file);
    let had_previous = path.exists();
    if had_previous {
        let _ = std::fs::remove_file(&backup);
        std::fs::rename(&path, &backup).map_err(|error| {
            let _ = std::fs::remove_file(&temporary);
            ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(format!(
                "backup PHP registration plan: {error}"
            )))
        })?;
    }
    if let Err(error) = std::fs::rename(&temporary, &path) {
        if had_previous {
            let _ = std::fs::rename(&backup, &path);
        }
        let _ = std::fs::remove_file(&temporary);
        return Err(ApplicationError::Port(
            cxszn_menv_core::ports::PortError::Platform(format!(
                "publish PHP registration plan: {error}"
            )),
        ));
    }
    let _ = std::fs::remove_file(&backup);
    Ok(())
}

#[cfg(windows)]
fn snapshot_hash(snapshot: &EnvironmentSnapshot) -> Result<String, ApplicationError> {
    let bytes = serde_json::to_vec(snapshot)
        .map_err(|error| ApplicationError::Serialization(error.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[cfg(windows)]
fn install_plan_hash(
    plan: &ChangePlan,
    spec: &cxszn_menv_core::windows::install::PhpArtifactInstallSpec,
) -> Result<String, ApplicationError> {
    #[derive(Serialize)]
    struct InstallPlanHashInput {
        plan: ChangePlan,
        url: String,
        file_name: String,
        expected_size_bytes: u64,
        expected_sha256: String,
        staging_root: String,
        managed_directory: String,
        entrypoint: String,
        journal_path: Option<String>,
    }

    let mut unsigned = plan.clone();
    unsigned.plan_hash.clear();
    let input = InstallPlanHashInput {
        plan: unsigned,
        url: spec.url.clone(),
        file_name: spec.file_name.clone(),
        expected_size_bytes: spec.expected_size_bytes,
        expected_sha256: spec.expected_sha256.clone(),
        staging_root: spec.staging_root.to_string_lossy().replace('/', "\\"),
        managed_directory: spec.managed_directory.to_string_lossy().replace('/', "\\"),
        entrypoint: spec.entrypoint.clone(),
        journal_path: spec
            .journal_path
            .as_ref()
            .map(|path| path.to_string_lossy().replace('/', "\\")),
    };
    let bytes = serde_json::to_vec(&input)
        .map_err(|error| ApplicationError::Serialization(error.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[cfg(windows)]
fn validate_install_confirmation(
    plan: &ChangePlan,
    token: Option<&ConfirmationToken>,
    service: &dyn cxszn_menv_core::application::ApplicationService,
) -> Result<(), ApplicationError> {
    if plan.request.action != ChangeAction::Install {
        return Err(ApplicationError::UnsupportedChangeAction);
    }
    service.validate_confirmation_token(plan, token)
}

#[cfg(windows)]
fn validate_php_registration_confirmation(
    plan: &ChangePlan,
    token: Option<&ConfirmationToken>,
    service: &dyn cxszn_menv_core::application::ApplicationService,
) -> Result<(), ApplicationError> {
    if plan.request.action != ChangeAction::Register {
        return Err(ApplicationError::UnsupportedChangeAction);
    }
    service.validate_confirmation_token(plan, token)
}

#[cfg(windows)]
fn registration_plan_hash(
    plan: &ChangePlan,
    runtime: &cxszn_menv_core::domain::RuntimeRecord,
) -> Result<String, ApplicationError> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct RegistrationBinding<'a> {
        canonical_plan_hash: String,
        runtime: &'a cxszn_menv_core::domain::RuntimeRecord,
    }
    let binding = RegistrationBinding {
        canonical_plan_hash: cxszn_menv_core::application::hash_plan(plan)?,
        runtime,
    };
    let bytes = serde_json::to_vec(&binding)
        .map_err(|error| ApplicationError::Serialization(error.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[cfg(windows)]
fn validate_native_plan_payload(
    plan: &ChangePlan,
    runtime: Option<&cxszn_menv_core::domain::RuntimeRecord>,
) -> Result<(), ApplicationError> {
    let expected_hash = if let Some(runtime) = runtime {
        if plan.request.action != ChangeAction::Register
            || plan.request.runtime_id.as_ref() != Some(&runtime.id)
            || plan.request.family != runtime.family
            || runtime.family != cxszn_menv_core::domain::RuntimeFamily::Php
            || plan.request.target_runtime_id.is_some()
        {
            return Err(ApplicationError::PlanTampered);
        }
        registration_plan_hash(plan, runtime)?
    } else {
        if matches!(
            plan.request.action,
            ChangeAction::Install | ChangeAction::Register
        ) {
            return Err(ApplicationError::UnsupportedChangeAction);
        }
        cxszn_menv_core::application::hash_plan(plan)?
    };
    if plan.plan_hash != expected_hash {
        return Err(ApplicationError::PlanTampered);
    }
    Ok(())
}

#[cfg(windows)]
fn apply_confirmed_php_registration<S, P>(
    service: &cxszn_menv_core::application::Orchestrator<S, P>,
    plan: &ChangePlan,
    token: Option<&ConfirmationToken>,
    runtime: cxszn_menv_core::domain::RuntimeRecord,
) -> Result<EnvironmentSnapshot, ApplicationError>
where
    S: cxszn_menv_core::ports::EnvironmentStore,
    P: cxszn_menv_core::ports::UserPathStore,
{
    // Both callers hold the operation lock here; approval may have expired while waiting.
    validate_native_plan_payload(plan, Some(&runtime))?;
    validate_php_registration_confirmation(plan, token, service)?;
    service.register_verified_runtime_with_operation(
        runtime,
        &plan.plan_id,
        &plan.request.idempotency_key,
    )
}

#[cfg(windows)]
fn poisoned_lock(resource: &str) -> ApplicationError {
    ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(format!(
        "{resource} lock is poisoned"
    )))
}

#[cfg(windows)]
fn acquire_runtime_operation_lock(
) -> Result<cxszn_menv_core::windows::lock::WindowsOperationLock, ApplicationError> {
    cxszn_menv_core::windows::lock::acquire_runtime_operation_lock(std::time::Duration::from_secs(
        30,
    ))
    .map_err(|error| match error {
        cxszn_menv_core::windows::lock::OperationLockError::Timeout => {
            ApplicationError::OperationBusy
        }
        other => ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(
            other.to_string(),
        )),
    })
}

#[cfg(windows)]
fn map_catalog_install_error(
    error: cxszn_menv_core::windows::install::InstallError,
) -> ApplicationError {
    use cxszn_menv_core::windows::install::InstallError;
    let (code, retryable) = match &error {
        InstallError::Download(_) => ("DOWNLOAD_FAILED", true),
        InstallError::SizeMismatch => ("SIZE_MISMATCH", false),
        InstallError::HashMismatch => ("HASH_MISMATCH", false),
        InstallError::UnsafeArchive(_) => ("ARCHIVE_REJECTED", false),
        InstallError::EntrypointMissing => ("ENTRYPOINT_MISSING", false),
        InstallError::ManagedVersionExists => ("VERSION_EXISTS", false),
        InstallError::Filesystem(_) => ("INSTALL_FILESYSTEM_ERROR", false),
        InstallError::SourceNotAllowed
        | InstallError::InvalidMetadata(_)
        | InstallError::UnsupportedPlatform => ("CATALOG_REJECTED", false),
    };
    ApplicationError::ArtifactInstallFailed {
        code: code.to_owned(),
        message: error.to_string(),
        retryable,
    }
}

fn now_epoch_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
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
        ApplicationError::OperationBusy => "OPERATION_BUSY",
        ApplicationError::OperationNotFound => "OPERATION_NOT_FOUND",
        ApplicationError::OperationNotCancellable => "OPERATION_NOT_CANCELLABLE",
        ApplicationError::ArtifactInstallFailed { code, .. } => code.as_str(),
        ApplicationError::InvalidActivationTransition => "INVALID_STATE_TRANSITION",
        ApplicationError::Serialization(_) => "SERIALIZATION_ERROR",
        ApplicationError::Port(cxszn_menv_core::ports::PortError::BackupRecoveryRequired {
            ..
        }) => "BACKUP_RECOVERY_REQUIRED",
        ApplicationError::Port(_) => "PLATFORM_ERROR",
    }
    .to_owned();
    let retryable = matches!(
        error,
        ApplicationError::PlanExpired
            | ApplicationError::StateChanged
            | ApplicationError::OperationBusy
            | ApplicationError::ArtifactInstallFailed {
                retryable: true,
                ..
            }
    );
    let next_actions = match error {
        ApplicationError::ConfirmationRequired => {
            vec!["Confirm the plan, then retry apply.".to_owned()]
        }
        ApplicationError::PlanExpired | ApplicationError::StateChanged => {
            vec!["Create a new plan and retry.".to_owned()]
        }
        ApplicationError::OperationBusy => {
            vec!["Wait for the current operation to finish, then retry.".to_owned()]
        }
        ApplicationError::Port(cxszn_menv_core::ports::PortError::BackupRecoveryRequired {
            ..
        }) => {
            vec!["Inspect the retained backup, then create a new plan; do not overwrite or delete it automatically.".to_owned()]
        }
        ApplicationError::ArtifactInstallFailed {
            retryable: true, ..
        } => {
            vec!["Check the network and retry the installation plan.".to_owned()]
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
#[cfg(windows)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            get_environment_state,
            scan_installed_runtimes,
            plan_php_path_registration,
            apply_php_path_registration,
            read_php_ini,
            php_server_start,
            php_server_stop,
            php_server_status,
            diagnose_web_servers,
            apply_php_ini_backup,
            terminal_start,
            terminal_send_input,
            terminal_poll,
            terminal_close,
            list_php_catalog,
            list_php_install_plans,
            plan_php_install,
            cancel_php_install,
            apply_php_install,
            plan_environment_change,
            apply_environment_change,
            issue_confirmation_token
        ])
        .run(tauri::generate_context!())
        .expect("error while running MEnv Tauri application");
}

#[cfg(not(windows))]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            get_environment_state,
            scan_installed_runtimes,
            plan_environment_change,
            apply_environment_change,
            issue_confirmation_token
        ])
        .run(tauri::generate_context!())
        .expect("error while running MEnv Tauri application");
}

/// Entrypoint hook used by the app-owned cross-volume PHP launcher shim.
pub fn run_php_shim_if_invoked() -> Option<i32> {
    cxszn_menv_core::run_php_shim_if_invoked()
}

/// Run the native JSON CLI when the installed desktop executable is
/// invoked with a supported JSON command. Unknown/non-JSON arguments return
/// `None` so the normal Tauri GUI remains the default entrypoint.
#[cfg(windows)]
fn cli_option(args: &[String], name: &str) -> Option<String> {
    let prefix = format!("--{name}=");
    args.iter().enumerate().find_map(|(index, argument)| {
        if let Some(value) = argument.strip_prefix(&prefix) {
            return Some(value.to_owned());
        }
        if argument == &format!("--{name}") {
            return args.get(index + 1).cloned();
        }
        None
    })
}

#[cfg(windows)]
fn cli_required_option(args: &[String], name: &str) -> Result<String, ApplicationError> {
    cli_option(args, name)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ApplicationError::Serialization(format!("missing required CLI option --{name}"))
        })
}

#[cfg(windows)]
fn cli_plan_path() -> Result<PathBuf, ApplicationError> {
    Ok(current_data_root()?.join("cli-change-plan.json"))
}

#[cfg(windows)]
fn cli_register_plan_path() -> Result<PathBuf, ApplicationError> {
    Ok(current_data_root()?.join("cli-register-plan.json"))
}

#[cfg(windows)]
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CliRegisterPlanBundle {
    plan: ChangePlan,
    runtime: cxszn_menv_core::domain::RuntimeRecord,
}

#[cfg(windows)]
fn save_cli_plan(plan: &ChangePlan) -> Result<(), ApplicationError> {
    let path = cli_plan_path()?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|error| {
        ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(format!(
            "create CLI plan directory: {error}"
        )))
    })?;
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let bytes = serde_json::to_vec_pretty(plan)
        .map_err(|error| ApplicationError::Serialization(error.to_string()))?;
    std::fs::write(&temporary, bytes).map_err(|error| {
        ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(format!(
            "write CLI plan: {error}"
        )))
    })?;
    if let Err(error) = std::fs::rename(&temporary, &path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(ApplicationError::Port(
            cxszn_menv_core::ports::PortError::Platform(format!("publish CLI plan: {error}")),
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn load_cli_plan() -> Result<ChangePlan, ApplicationError> {
    let path = cli_plan_path()?;
    let content = std::fs::read_to_string(&path).map_err(|error| {
        ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(format!(
            "read CLI plan: {error}"
        )))
    })?;
    serde_json::from_str(&content)
        .map_err(|error| ApplicationError::Serialization(format!("parse CLI plan: {error}")))
}

#[cfg(windows)]
fn save_cli_register_plan(bundle: &CliRegisterPlanBundle) -> Result<(), ApplicationError> {
    let path = cli_register_plan_path()?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|error| {
        ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(format!(
            "create CLI registration plan directory: {error}"
        )))
    })?;
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let bytes = serde_json::to_vec_pretty(bundle)
        .map_err(|error| ApplicationError::Serialization(error.to_string()))?;
    std::fs::write(&temporary, bytes).map_err(|error| {
        ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(format!(
            "write CLI registration plan: {error}"
        )))
    })?;
    if let Err(error) = std::fs::rename(&temporary, &path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(ApplicationError::Port(
            cxszn_menv_core::ports::PortError::Platform(format!(
                "publish CLI registration plan: {error}"
            )),
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn load_cli_plan_any(
) -> Result<(ChangePlan, Option<cxszn_menv_core::domain::RuntimeRecord>), ApplicationError> {
    let plan_path = cli_plan_path()?;
    if plan_path.is_file() {
        let plan = load_cli_plan()?;
        validate_native_plan_payload(&plan, None)?;
        return Ok((plan, None));
    }
    let register_path = cli_register_plan_path()?;
    let content = std::fs::read_to_string(&register_path).map_err(|error| {
        ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(format!(
            "read CLI registration plan: {error}"
        )))
    })?;
    let bundle: CliRegisterPlanBundle = serde_json::from_str(&content).map_err(|error| {
        ApplicationError::Serialization(format!("parse CLI registration plan: {error}"))
    })?;
    validate_native_plan_payload(&bundle.plan, Some(&bundle.runtime))?;
    Ok((bundle.plan, Some(bundle.runtime)))
}

#[cfg(windows)]
fn cli_snapshot_value(
    snapshot: EnvironmentSnapshot,
) -> Result<serde_json::Value, ApplicationError> {
    let mut snapshot = snapshot;
    snapshot.path_lease.entries = snapshot
        .path_lease
        .entries
        .iter()
        .map(|_| "<redacted>".to_owned())
        .collect();
    serde_json::to_value(snapshot)
        .map_err(|error| ApplicationError::Serialization(error.to_string()))
}

#[cfg(windows)]
fn remove_cli_plans() -> Result<(), ApplicationError> {
    let _ = std::fs::remove_file(cli_plan_path()?);
    let _ = std::fs::remove_file(cli_register_plan_path()?);
    Ok(())
}

#[cfg(windows)]
fn parse_cli_family(
    value: Option<String>,
) -> Result<cxszn_menv_core::domain::RuntimeFamily, ApplicationError> {
    match value.as_deref().unwrap_or("php") {
        "php" => Ok(cxszn_menv_core::domain::RuntimeFamily::Php),
        "node" => Ok(cxszn_menv_core::domain::RuntimeFamily::Node),
        "python" => Ok(cxszn_menv_core::domain::RuntimeFamily::Python),
        "mysql" => Ok(cxszn_menv_core::domain::RuntimeFamily::Mysql),
        other => Err(ApplicationError::Serialization(format!(
            "unsupported runtime family: {other}"
        ))),
    }
}

#[cfg(windows)]
fn make_cli_plan(
    service: &cxszn_menv_core::DefaultService,
    args: &[String],
    action_name: &str,
) -> Result<serde_json::Value, ApplicationError> {
    let action = match action_name {
        "enable" => ChangeAction::Enable,
        "disable" => ChangeAction::Disable,
        "switch" => ChangeAction::Switch,
        "backup-ini" => ChangeAction::BackupIni,
        other => {
            return Err(ApplicationError::Serialization(format!(
                "unsupported change action: {other}"
            )))
        }
    };
    let family = parse_cli_family(cli_option(args, "family"))?;
    if family != cxszn_menv_core::domain::RuntimeFamily::Php {
        return Err(ApplicationError::UnsupportedChangeAction);
    }
    let runtime_id = cli_required_option(args, "runtime-id")?;
    let target_runtime_id = cli_option(args, "target-runtime-id");
    let actor = cli_option(args, "actor").unwrap_or_else(|| "native-cli".to_owned());
    let idempotency_key = cli_option(args, "idempotency-key")
        .unwrap_or_else(|| format!("native-cli-{}", Uuid::new_v4()));
    let mut plan = service.plan_change(ChangeRequest {
        action,
        family,
        runtime_id: Some(
            cxszn_menv_core::domain::RuntimeId::new(runtime_id)
                .map_err(|error| ApplicationError::Serialization(error.to_string()))?,
        ),
        target_runtime_id: target_runtime_id
            .map(cxszn_menv_core::domain::RuntimeId::new)
            .transpose()
            .map_err(|error| ApplicationError::Serialization(error.to_string()))?,
        idempotency_key,
        actor,
    })?;
    if plan.request.action == ChangeAction::BackupIni {
        let _process_operation_lock = acquire_runtime_operation_lock()?;
        bind_backup_source(service, &mut plan, &current_data_root()?)?;
    }
    save_cli_plan(&plan)?;
    serde_json::to_value(plan).map_err(|error| ApplicationError::Serialization(error.to_string()))
}

#[cfg(windows)]
fn make_cli_register_plan(
    service: &cxszn_menv_core::DefaultService,
    args: &[String],
) -> Result<serde_json::Value, ApplicationError> {
    let family = parse_cli_family(cli_option(args, "family"))?;
    if family != cxszn_menv_core::domain::RuntimeFamily::Php {
        return Err(ApplicationError::UnsupportedChangeAction);
    }
    let path = cli_required_option(args, "path")?;
    let actor = cli_option(args, "actor").unwrap_or_else(|| "native-cli".to_owned());
    let idempotency_key = cli_option(args, "idempotency-key")
        .unwrap_or_else(|| format!("native-cli-register-{}", Uuid::new_v4()));
    let provider = cxszn_menv_core::windows::WindowsPhpProvider;
    let runtime = cxszn_menv_core::ports::RuntimeProvider::verify_path(
        &provider,
        &cxszn_menv_core::ports::VerifyPathRequest {
            family,
            path: PathBuf::from(path),
        },
    )?;
    let created_at = now_epoch_seconds();
    let mut plan = ChangePlan {
        plan_id: Uuid::new_v4().to_string(),
        plan_hash: String::new(),
        request: ChangeRequest {
            action: ChangeAction::Register,
            family: cxszn_menv_core::domain::RuntimeFamily::Php,
            runtime_id: Some(runtime.id.clone()),
            target_runtime_id: None,
            idempotency_key,
            actor,
        },
        requires_confirmation: true,
        created_at_epoch_seconds: created_at,
        expires_at_epoch_seconds: created_at + 300,
        snapshot_hash: snapshot_hash(&service.get_environment_state()?)?,
        backup_source: None,
    };
    plan.plan_hash = registration_plan_hash(&plan, &runtime)?;
    let bundle = CliRegisterPlanBundle { plan, runtime };
    save_cli_register_plan(&bundle)?;
    serde_json::to_value(bundle).map_err(|error| ApplicationError::Serialization(error.to_string()))
}

#[cfg(windows)]
pub fn run_native_cli_if_requested() -> Option<i32> {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    let json = args.iter().any(|argument| argument == "--json");
    if !json {
        return None;
    }
    args.retain(|argument| argument != "--json");
    let service = cxszn_menv_core::default_service();
    let result = (|| {
        let value = if args.first().is_some_and(|value| value == "runtime")
            && args.get(1).is_some_and(|value| value == "add")
        {
            make_cli_register_plan(&service, &args)?
        } else if args.first().is_some_and(|value| value == "runtime")
            && args
                .get(1)
                .is_some_and(|value| matches!(value.as_str(), "enable" | "disable"))
        {
            make_cli_plan(&service, &args, args.get(1).expect("action").as_str())?
        } else if args.first().is_some_and(|value| value == "change")
            && args.get(1).is_some_and(|value| value == "plan")
        {
            make_cli_plan(&service, &args, &cli_required_option(&args, "action")?)?
        } else if args.first().is_some_and(|value| value == "change")
            && args.get(1).is_some_and(|value| value == "confirm")
        {
            let (plan, runtime) = load_cli_plan_any()?;
            if let Some(plan_id) = cli_option(&args, "plan-id") {
                if plan.plan_id != plan_id {
                    return Err(ApplicationError::PlanTampered);
                }
            }
            if now_epoch_seconds() >= plan.expires_at_epoch_seconds {
                return Err(ApplicationError::PlanExpired);
            }
            cli_confirmation::read_user_confirmation(&plan, runtime.as_ref())?;
            let token = service.issue_confirmation_token(&plan)?;
            serde_json::to_value(token)
                .map_err(|error| ApplicationError::Serialization(error.to_string()))?
        } else if args.first().is_some_and(|value| value == "change")
            && args.get(1).is_some_and(|value| value == "apply")
        {
            let (plan, runtime) = load_cli_plan_any()?;
            if let Some(plan_id) = cli_option(&args, "plan-id") {
                if plan.plan_id != plan_id {
                    return Err(ApplicationError::PlanTampered);
                }
            }
            let token_value = cli_required_option(&args, "confirmation-token")?;
            let idempotency_key = cli_required_option(&args, "idempotency-key")?;
            if idempotency_key != plan.request.idempotency_key {
                return Err(ApplicationError::IdempotencyConflict);
            }
            let token = ConfirmationToken {
                value: token_value,
                plan_id: plan.plan_id.clone(),
                plan_hash: plan.plan_hash.clone(),
                actor: plan.request.actor.clone(),
                expires_at_epoch_seconds: plan.expires_at_epoch_seconds,
            };
            let _process_operation_lock = acquire_runtime_operation_lock()?;
            let value = if let Some(runtime) = runtime {
                let snapshot =
                    apply_confirmed_php_registration(&service, &plan, Some(&token), runtime)?;
                serde_json::to_value(snapshot)
                    .map_err(|error| ApplicationError::Serialization(error.to_string()))?
            } else if plan.request.action == ChangeAction::BackupIni {
                let info =
                    apply_confirmed_backup(&service, &plan, Some(&token), &current_data_root()?)?;
                serde_json::to_value(info)
                    .map_err(|error| ApplicationError::Serialization(error.to_string()))?
            } else {
                let receipt = service.apply_change(&plan, Some(&token))?;
                serde_json::to_value(receipt)
                    .map_err(|error| ApplicationError::Serialization(error.to_string()))?
            };
            remove_cli_plans()?;
            value
        } else if args.first().is_some_and(|value| value == "operation")
            && args
                .get(1)
                .is_some_and(|value| matches!(value.as_str(), "get" | "cancel"))
        {
            let operation_id = cli_required_option(&args, "id")?;
            let operation = if args.get(1).is_some_and(|value| value == "get") {
                service.get_operation_status(&operation_id)?
            } else {
                let _process_operation_lock = acquire_runtime_operation_lock()?;
                let snapshot = service.cancel_operation(&operation_id)?;
                snapshot
                    .operations
                    .into_iter()
                    .find(|operation| operation.operation_id == operation_id)
                    .ok_or(ApplicationError::OperationNotFound)?
            };
            serde_json::to_value(operation)
                .map_err(|error| ApplicationError::Serialization(error.to_string()))?
        } else {
            match args.as_slice() {
            [command] if command == "doctor" => {
                cli_snapshot_value(load_native_environment_state(&service)?)
            }
            [group, command] if group == "runtimes" && command == "list" => {
                cli_snapshot_value(load_native_environment_state(&service)?)
            }
            [group, command] if group == "runtime" && command == "scan" => {
                let _process_operation_lock = acquire_runtime_operation_lock()?;
                cli_snapshot_value(cxszn_menv_core::scan_runtimes(&service)?)
            }
            [group, command, ..]
                if group == "runtime" && command == "verify" =>
            {
                let family = parse_cli_family(cli_option(&args, "family"))?;
                let path = cli_required_option(&args, "path")?;
                let request = cxszn_menv_core::ports::VerifyPathRequest {
                    family: family.clone(),
                    path: PathBuf::from(path),
                };
                let record = match family {
                    cxszn_menv_core::domain::RuntimeFamily::Php => {
                        let provider = cxszn_menv_core::windows::WindowsPhpProvider;
                        cxszn_menv_core::ports::RuntimeProvider::verify_path(&provider, &request)?
                    }
                    cxszn_menv_core::domain::RuntimeFamily::Node => {
                        let provider = cxszn_menv_core::windows::WindowsNodeProvider;
                        cxszn_menv_core::ports::RuntimeProvider::verify_path(&provider, &request)?
                    }
                    cxszn_menv_core::domain::RuntimeFamily::Python => {
                        let provider = cxszn_menv_core::windows::WindowsPythonProvider;
                        cxszn_menv_core::ports::RuntimeProvider::verify_path(&provider, &request)?
                    }
                    cxszn_menv_core::domain::RuntimeFamily::Mysql => {
                        let provider = cxszn_menv_core::windows::WindowsMysqlProvider;
                        cxszn_menv_core::ports::RuntimeProvider::verify_path(&provider, &request)?
                    }
                };
                serde_json::to_value(record)
                    .map_err(|error| ApplicationError::Serialization(error.to_string()))
            }
            [group, command, kind_flag, kind]
                if group == "catalog"
                    && command == "list"
                    && kind_flag == "--kind"
                    && kind == "php" =>
            {
                let catalog = cxszn_menv_core::windows::embedded_php_catalog().map_err(|error| {
                    ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(
                        error.to_string(),
                    ))
                })?;
                let mut artifacts = cxszn_menv_core::windows::list_php_artifacts(&catalog).map_err(|error| {
                        ApplicationError::Port(cxszn_menv_core::ports::PortError::Platform(
                            error.to_string(),
                        ))
                    })?;
                php_catalog_inventory::mark_installed(&mut artifacts, &service.get_environment_state()?.runtimes);
                serde_json::to_value(artifacts)
                .map_err(|error| ApplicationError::Serialization(error.to_string()))
            }
            _ => {
                return Err(ApplicationError::Serialization(
                    "supported native JSON commands: doctor, runtimes list, runtime scan/verify, catalog list --kind php, runtime add/enable/disable, change plan/confirm/apply (including backup-ini), operation get/cancel --id <id>"
                        .to_owned(),
                ));
            }
            }
            .map_err(|error| ApplicationError::Serialization(error.to_string()))?
        };
        Ok::<_, ApplicationError>(value)
    })();
    let envelope = match result {
        Ok(value) => Envelope::success(value),
        Err(error) => Envelope::<serde_json::Value>::failure(error),
    };
    let ok = envelope.ok;
    println!(
        "{}",
        serde_json::to_string(&envelope).unwrap_or_else(|_| {
            "{\"schema_version\":\"1\",\"request_id\":\"native-cli\",\"ok\":false}".to_owned()
        })
    );
    Some(if ok { 0 } else { 2 })
}

#[cfg(not(windows))]
pub fn run_native_cli_if_requested() -> Option<i32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn registered_install_recovery_does_not_rescan_or_change_a_pending_plan_snapshot() {
        use cxszn_menv_core::domain::{
            Architecture, Availability, Ownership, RuntimeFamily, RuntimeId, RuntimeRecord,
            RuntimeSource,
        };
        use cxszn_menv_core::ports::EnvironmentStore;
        let root = std::env::temp_dir().join(format!("menv-recovery-read-{}", Uuid::new_v4()));
        let journal_root = root.join("operations").join("php");
        let managed_root = root.join("versions").join("php");
        let version_root = managed_root.join("8.3.35").join("x64");
        std::fs::create_dir_all(&version_root).expect("managed fixture");
        std::fs::create_dir_all(&journal_root).expect("journal fixture");
        let executable = version_root.join("php.exe");
        std::fs::write(&executable, b"not executed: already registered").expect("fixture file");
        std::fs::write(
            journal_root.join("install.jsonl"),
            serde_json::json!({
                "stage": "registered", "managedDirectory": version_root.to_string_lossy(),
            })
            .to_string(),
        )
        .expect("fixture journal");
        let mut runtime = RuntimeRecord::discovered(
            RuntimeId::new("registered-php").expect("id"),
            RuntimeFamily::Php,
            "8.3.35",
            Architecture::X64,
            executable.to_string_lossy(),
            RuntimeSource::CommonLocation,
        );
        runtime.ownership = Ownership::Managed;
        runtime.availability = Availability::Managed;
        let before = EnvironmentSnapshot {
            updated_at: "snapshot-before-confirmation".to_owned(),
            runtimes: vec![runtime],
            ..EnvironmentSnapshot::default()
        };
        let store =
            cxszn_menv_core::windows::SqliteEnvironmentStore::new(root.join("state.sqlite3"), None);
        store.save(&before).expect("save fixture");
        let service = cxszn_menv_core::application::Orchestrator::with_path_store(
            store,
            cxszn_menv_core::windows::WindowsUserPathStore::new(root.join("bin"))
                .expect("path store"),
        );
        let result = recover_php_install_journals_at(&service, &journal_root, &managed_root);
        let after = service
            .get_environment_state()
            .expect("read fixture result");
        std::fs::remove_dir_all(&root).expect("remove unique fixture");
        result.expect("recovery read succeeds");
        assert_eq!(
            after, before,
            "reading a completed journal must not invalidate a user's plan"
        );
    }

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

    #[test]
    fn operation_busy_error_is_retryable_and_has_a_stable_code() {
        let envelope: Envelope<EnvironmentSnapshot> =
            Envelope::failure(ApplicationError::OperationBusy);
        let error = envelope.error.expect("busy error");
        assert_eq!(error.code, "OPERATION_BUSY");
        assert!(error.retryable);
        assert!(!error.next_actions.is_empty());
    }

    #[test]
    fn retained_backup_failure_is_actionable_and_not_blindly_retryable() {
        let envelope: Envelope<EnvironmentSnapshot> = Envelope::failure(ApplicationError::Port(
            cxszn_menv_core::ports::PortError::BackupRecoveryRequired {
                backup_path: "fixture.php.ini.bak".to_owned(),
            },
        ));
        assert!(!envelope.ok);
        let error = envelope.error.expect("recovery error");
        assert_eq!(error.code, "BACKUP_RECOVERY_REQUIRED");
        assert!(!error.retryable);
        assert!(!error.next_actions.is_empty());
        assert!(error.message.contains("fixture.php.ini.bak"));
    }

    #[test]
    fn artifact_install_errors_keep_specific_codes_and_retry_hints() {
        let envelope: Envelope<EnvironmentSnapshot> =
            Envelope::failure(ApplicationError::ArtifactInstallFailed {
                code: "HASH_MISMATCH".to_owned(),
                message: "download SHA-256 does not match".to_owned(),
                retryable: false,
            });
        let error = envelope.error.expect("artifact install error");
        assert_eq!(error.code, "HASH_MISMATCH");
        assert!(!error.retryable);
        assert!(error.next_actions.is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn persisted_php_install_plan_payload_roundtrips_without_confirmation_material() {
        let plan = ChangePlan {
            plan_id: "plan-persisted".to_owned(),
            plan_hash: "hash".to_owned(),
            request: ChangeRequest {
                action: ChangeAction::Install,
                family: cxszn_menv_core::domain::RuntimeFamily::Php,
                runtime_id: None,
                target_runtime_id: None,
                idempotency_key: "persisted-key".to_owned(),
                actor: "test".to_owned(),
            },
            requires_confirmation: true,
            created_at_epoch_seconds: now_epoch_seconds(),
            expires_at_epoch_seconds: now_epoch_seconds() + 300,
            snapshot_hash: "snapshot".to_owned(),
            backup_source: None,
        };
        let record = PhpInstallPlanRecord {
            plan,
            spec: cxszn_menv_core::windows::install::PhpArtifactInstallSpec {
                url:
                    "https://downloads.php.net/~windows/releases/php-8.5.11-nts-Win32-vs17-x64.zip"
                        .to_owned(),
                file_name: "php-8.5.11-nts-Win32-vs17-x64.zip".to_owned(),
                expected_size_bytes: 10,
                expected_sha256: "a".repeat(64),
                staging_root: PathBuf::from(r"C:\Users\demo\AppData\Local\cxszn-menv\staging"),
                managed_directory: PathBuf::from(
                    r"C:\Users\demo\AppData\Local\cxszn-menv\versions\php\8.5.11\x86_64",
                ),
                entrypoint: "php.exe".to_owned(),
                journal_path: None,
            },
            result: None,
            status: PhpInstallPlanStatus::Pending,
        };
        let plans = HashMap::from([("plan-persisted".to_owned(), record)]);
        let encoded = encode_php_install_plans(&plans).expect("encode empty plans");
        let decoded = decode_php_install_plans(&encoded).expect("decode empty plans");
        assert_eq!(decoded.len(), 1);
        assert_eq!(
            decoded["plan-persisted"].plan.request.idempotency_key,
            "persisted-key"
        );
        let text = String::from_utf8(encoded).expect("UTF-8 plan payload");
        assert!(!text.contains("confirmation"));
        assert!(!text.contains("token"));
    }

    #[cfg(windows)]
    #[test]
    fn recovered_php_install_plan_summary_filters_expired_completed_and_unknown_plans() {
        let catalog = cxszn_menv_core::windows::embedded_php_catalog().expect("embedded catalog");
        let artifacts =
            cxszn_menv_core::windows::list_php_artifacts(&catalog).expect("catalog summaries");
        let spec = cxszn_menv_core::windows::create_php_install_spec(
            &catalog,
            "php-8.5.11-nts-x64",
            Path::new(r"C:\Users\demo\AppData\Local\cxszn-menv"),
        )
        .expect("install spec");

        let make_plan = |id: &str, expires: u64| ChangePlan {
            plan_id: id.to_owned(),
            plan_hash: format!("hash-{id}"),
            request: ChangeRequest {
                action: ChangeAction::Install,
                family: cxszn_menv_core::domain::RuntimeFamily::Php,
                runtime_id: None,
                target_runtime_id: None,
                idempotency_key: format!("key-{id}"),
                actor: "test".to_owned(),
            },
            requires_confirmation: true,
            created_at_epoch_seconds: 1,
            expires_at_epoch_seconds: expires,
            snapshot_hash: format!("snapshot-{id}"),
            backup_source: None,
        };

        let future = make_plan("future", 200);
        let expired = make_plan("expired", 50);
        let completed = make_plan("completed", 200);
        let unknown = make_plan("unknown", 200);
        let plans = HashMap::from([
            (
                future.plan_id.clone(),
                PhpInstallPlanRecord {
                    plan: future,
                    spec: spec.clone(),
                    result: None,
                    status: PhpInstallPlanStatus::Pending,
                },
            ),
            (
                expired.plan_id.clone(),
                PhpInstallPlanRecord {
                    plan: expired,
                    spec: spec.clone(),
                    result: None,
                    status: PhpInstallPlanStatus::Pending,
                },
            ),
            (
                completed.plan_id.clone(),
                PhpInstallPlanRecord {
                    plan: completed,
                    spec: spec.clone(),
                    result: Some(EnvironmentSnapshot::default()),
                    status: PhpInstallPlanStatus::Started,
                },
            ),
            (
                unknown.plan_id.clone(),
                PhpInstallPlanRecord {
                    plan: unknown,
                    spec: cxszn_menv_core::windows::install::PhpArtifactInstallSpec {
                        file_name: "not-in-catalog.zip".to_owned(),
                        ..spec
                    },
                    result: None,
                    status: PhpInstallPlanStatus::Pending,
                },
            ),
        ]);

        let summaries = summarize_php_install_plans(&plans, &artifacts, 100);
        assert_eq!(
            summaries
                .iter()
                .map(|summary| summary.plan.plan_id.as_str())
                .collect::<Vec<_>>(),
            vec!["future"]
        );
        assert_eq!(summaries[0].artifact.version, "8.5.11");
    }

    #[cfg(windows)]
    #[test]
    fn persisted_php_register_plan_payload_roundtrips_without_confirmation_material() {
        let runtime = cxszn_menv_core::domain::RuntimeRecord::discovered(
            cxszn_menv_core::domain::RuntimeId::new("php:win:c:\\php\\php.exe")
                .expect("runtime id"),
            cxszn_menv_core::domain::RuntimeFamily::Php,
            "8.5.11",
            cxszn_menv_core::domain::Architecture::X64,
            r"C:\php\php.exe",
            cxszn_menv_core::domain::RuntimeSource::Manual,
        );
        let plan = ChangePlan {
            plan_id: "register-persisted".to_owned(),
            plan_hash: "hash".to_owned(),
            request: ChangeRequest {
                action: ChangeAction::Register,
                family: cxszn_menv_core::domain::RuntimeFamily::Php,
                runtime_id: Some(runtime.id.clone()),
                target_runtime_id: None,
                idempotency_key: "register-key".to_owned(),
                actor: "test".to_owned(),
            },
            requires_confirmation: true,
            created_at_epoch_seconds: now_epoch_seconds(),
            expires_at_epoch_seconds: now_epoch_seconds() + 300,
            snapshot_hash: "snapshot".to_owned(),
            backup_source: None,
        };
        let plans = HashMap::from([(
            plan.plan_id.clone(),
            PhpRegisterPlanRecord {
                plan,
                runtime,
                result: None,
            },
        )]);
        let encoded = encode_php_register_plans(&plans).expect("encode register plans");
        let decoded = decode_php_register_plans(&encoded).expect("decode register plans");
        assert_eq!(decoded.len(), 1);
        assert_eq!(
            decoded["register-persisted"].plan.request.idempotency_key,
            "register-key"
        );
        let text = String::from_utf8(encoded).expect("UTF-8 register plan payload");
        assert!(!text.contains("confirmation"));
        assert!(!text.contains("token"));
    }

    #[cfg(windows)]
    #[test]
    fn terminal_environment_uses_the_current_path_lease_snapshot() {
        let base = BTreeMap::from([(
            OsString::from("PATH"),
            OsString::from("C:\\old-shim;C:\\Windows"),
        )]);
        let mut snapshot = EnvironmentSnapshot::default();
        snapshot.path_lease.enabled = true;
        snapshot.path_lease.entries = vec![
            r"C:\Users\demo\AppData\Local\cxszn-menv\bin".to_owned(),
            r"C:\Windows".to_owned(),
        ];
        let environment = terminal_environment(&base, &snapshot);
        assert_eq!(
            environment.get(&OsString::from("PATH")),
            Some(&OsString::from(
                r"C:\Users\demo\AppData\Local\cxszn-menv\bin;C:\Windows"
            ))
        );
    }

    #[cfg(windows)]
    #[test]
    fn journal_recovery_discovers_nested_version_journals_without_following_other_files() {
        let root = std::env::temp_dir().join(format!(
            "cxszn-menv-journal-discovery-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("8.5.11")).expect("create journal directory");
        std::fs::write(root.join("legacy.jsonl"), b"{}\n").expect("write root journal");
        std::fs::write(root.join("8.5.11").join("install.jsonl"), b"{}\n")
            .expect("write nested journal");
        std::fs::write(root.join("8.5.11").join("notes.txt"), b"ignore")
            .expect("write unrelated file");

        let journals = collect_php_install_journals(&root).expect("discover journals");
        assert_eq!(journals.len(), 2);
        assert!(journals.iter().any(|path| path.ends_with("legacy.jsonl")));
        assert!(journals
            .iter()
            .any(|path| path.ends_with(r"8.5.11\install.jsonl")));

        std::fs::remove_dir_all(&root).expect("remove journal directory");
    }

    #[cfg(windows)]
    #[test]
    fn journal_recovery_rejects_paths_outside_managed_root_and_accepts_regular_entrypoints() {
        let root =
            std::env::temp_dir().join(format!("cxszn-menv-recovery-test-{}", std::process::id()));
        let managed_root = root.join("versions").join("php");
        let managed_directory = managed_root.join("8.5.11").join("x86_64");
        std::fs::create_dir_all(&managed_directory).expect("create managed test directory");
        let entrypoint = managed_directory.join("php.exe");
        std::fs::write(&entrypoint, b"test executable").expect("write regular entrypoint");

        assert_eq!(
            safe_recovery_entrypoint(&managed_directory, &managed_root),
            Some(entrypoint.clone())
        );
        assert_eq!(
            safe_recovery_entrypoint(&root.join("outside"), &managed_root),
            None
        );

        std::fs::remove_dir_all(&root).expect("remove recovery test directory");
    }

    #[cfg(windows)]
    #[test]
    fn install_plan_hash_binds_the_fixed_artifact_spec() {
        let plan = ChangePlan {
            plan_id: "plan-1".to_owned(),
            plan_hash: String::new(),
            request: ChangeRequest {
                action: ChangeAction::Install,
                family: cxszn_menv_core::domain::RuntimeFamily::Php,
                runtime_id: None,
                target_runtime_id: None,
                idempotency_key: "idempotency-1".to_owned(),
                actor: "test".to_owned(),
            },
            requires_confirmation: true,
            created_at_epoch_seconds: 1,
            expires_at_epoch_seconds: 301,
            snapshot_hash: "snapshot".to_owned(),
            backup_source: None,
        };
        let mut spec = cxszn_menv_core::windows::install::PhpArtifactInstallSpec {
            url: "https://downloads.php.net/~windows/releases/php-8.5.11-nts-Win32-vs17-x64.zip"
                .to_owned(),
            file_name: "php-8.5.11-nts-Win32-vs17-x64.zip".to_owned(),
            expected_size_bytes: 10,
            expected_sha256: "a".repeat(64),
            staging_root: PathBuf::from(r"C:\Users\demo\AppData\Local\cxszn-menv\staging"),
            managed_directory: PathBuf::from(
                r"C:\Users\demo\AppData\Local\cxszn-menv\versions\php\8.5.11\x86_64",
            ),
            entrypoint: "php.exe".to_owned(),
            journal_path: None,
        };
        let original = install_plan_hash(&plan, &spec).expect("hash spec");
        spec.expected_sha256 = "b".repeat(64);
        let changed = install_plan_hash(&plan, &spec).expect("hash changed spec");
        assert_ne!(original, changed);
    }
}
