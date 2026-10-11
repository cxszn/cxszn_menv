//! Composition-facing facade for Tauri, CLI, and MCP adapters.

pub use cxszn_menv_application as application;
pub use cxszn_menv_domain as domain;
pub use cxszn_menv_ports as ports;

use application::Orchestrator;
#[cfg(not(windows))]
use ports::EmptyEnvironmentStore;

/// A safe, read-only default service used by the desktop shell until platform
/// adapters are wired. It exposes no arbitrary command execution.
#[cfg(not(windows))]
pub type DefaultService = Orchestrator<EmptyEnvironmentStore>;

#[cfg(windows)]
pub type DefaultService = Orchestrator<
    cxszn_menv_platform_windows::JsonEnvironmentStore,
    cxszn_menv_platform_windows::WindowsUserPathStore,
>;

#[cfg(not(windows))]
pub fn default_service() -> DefaultService {
    Orchestrator::new(EmptyEnvironmentStore)
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
    Orchestrator::with_path_store(
        cxszn_menv_platform_windows::JsonEnvironmentStore::default_for_current_user(),
        path_store,
    )
    .with_shim_directory(shim_directory)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facade_returns_empty_snapshot_without_claiming_runtime_state() {
        let service = default_service();
        let snapshot = service.get_environment_state().expect("empty snapshot");
        assert_eq!(snapshot.schema_version, "1");
        assert!(snapshot.runtimes.is_empty());
    }
}
