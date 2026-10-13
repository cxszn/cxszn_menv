//! Composition-facing facade for Tauri, CLI, and MCP adapters.

pub use cxszn_menv_application as application;
pub use cxszn_menv_domain as domain;
#[cfg(windows)]
pub use cxszn_menv_platform_windows as windows;
pub use cxszn_menv_ports as ports;

use application::ApplicationError;
use application::Orchestrator;
use domain::EnvironmentSnapshot;
#[cfg(not(windows))]
use ports::EmptyEnvironmentStore;
use ports::ScanRequest;

/// A safe, read-only default service used by the desktop shell until platform
/// adapters are wired. It exposes no arbitrary command execution.
#[cfg(not(windows))]
pub type DefaultService = Orchestrator<EmptyEnvironmentStore>;

#[cfg(windows)]
pub type DefaultService = Orchestrator<
    cxszn_menv_platform_windows::SqliteEnvironmentStore,
    cxszn_menv_platform_windows::WindowsUserPathStore,
>;

#[cfg(not(windows))]
pub fn default_service() -> DefaultService {
    Orchestrator::new(EmptyEnvironmentStore)
}

/// Run the platform PHP scanner through the shared application orchestrator.
/// The scan is read-only and never registers, copies, or enables a runtime.
#[cfg(windows)]
pub fn scan_php(service: &DefaultService) -> Result<EnvironmentSnapshot, ApplicationError> {
    let provider = cxszn_menv_platform_windows::WindowsPhpProvider;
    service.scan_with_provider(&provider, ScanRequest::default())
}

/// Scan the runtime families implemented by the Windows read-only providers.
/// Discovery never registers, copies, enables, or changes PATH entries.
#[cfg(windows)]
pub fn scan_runtimes(service: &DefaultService) -> Result<EnvironmentSnapshot, ApplicationError> {
    let php = cxszn_menv_platform_windows::WindowsPhpProvider;
    let node = cxszn_menv_platform_windows::WindowsNodeProvider;
    let python = cxszn_menv_platform_windows::WindowsPythonProvider;
    let mysql = cxszn_menv_platform_windows::WindowsMysqlProvider;
    let _ = service.scan_with_provider(&php, ScanRequest::default())?;
    let _ = service.scan_with_provider(&node, ScanRequest::default())?;
    let _ = service.scan_with_provider(&python, ScanRequest::default())?;
    service.scan_with_provider(&mysql, ScanRequest::default())
}

#[cfg(not(windows))]
pub fn scan_runtimes(service: &DefaultService) -> Result<EnvironmentSnapshot, ApplicationError> {
    service.get_environment_state()
}

#[cfg(not(windows))]
pub fn scan_php(service: &DefaultService) -> Result<EnvironmentSnapshot, ApplicationError> {
    let _ = ScanRequest::default();
    service.get_environment_state()
}

/// Entry-point hook for the app-owned native PHP launcher shim. The copied
/// Tauri executable exits through this path before initializing the desktop
/// runtime, so `php` remains a normal process invocation for the user.
#[cfg(windows)]
pub fn run_php_shim_if_invoked() -> Option<i32> {
    cxszn_menv_platform_windows::run_php_shim_if_invoked()
}

#[cfg(not(windows))]
pub fn run_php_shim_if_invoked() -> Option<i32> {
    None
}

#[cfg(windows)]
pub fn default_service() -> DefaultService {
    let path_store = cxszn_menv_platform_windows::WindowsUserPathStore::default_for_current_user()
        .unwrap_or_else(|_| {
            cxszn_menv_platform_windows::WindowsUserPathStore::new(
                r"C:\Users\Default\AppData\Local\cxszn-menv\bin",
            )
            .expect("fallback MEnv shim directory must be a valid local Windows path")
        });
    let shim_directory = path_store.shim_directory().to_path_buf();
    let store = cxszn_menv_platform_windows::SqliteEnvironmentStore::default_for_current_user();
    Orchestrator::with_path_store(store.clone(), path_store)
        .with_confirmation_store(store)
        .with_shim_directory(shim_directory)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facade_returns_empty_snapshot_without_claiming_runtime_state() {
        // The production Windows composition root intentionally reads the
        // user's durable state.  Tests must not make an assertion about that
        // state because a developer may already have registered a runtime.
        // Inject an isolated store instead, while retaining the same
        // orchestrator behavior under test.
        #[cfg(windows)]
        let (service, root) = {
            use std::time::{SystemTime, UNIX_EPOCH};

            let root = std::env::temp_dir().join(format!(
                "cxszn-menv-core-test-{}",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("clock")
                    .as_nanos()
            ));
            let store = windows::JsonEnvironmentStore::new(root.join("state-rust.json"));
            (application::Orchestrator::new(store), root)
        };

        #[cfg(not(windows))]
        let (service, root) = (default_service(), None::<std::path::PathBuf>);

        let snapshot = service.get_environment_state().expect("empty snapshot");
        assert_eq!(snapshot.schema_version, "1");
        assert!(snapshot.runtimes.is_empty());

        #[cfg(windows)]
        let _ = std::fs::remove_dir_all(root);
    }
}
