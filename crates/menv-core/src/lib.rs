//! Composition-facing facade for Tauri, CLI, and MCP adapters.

pub use cxszn_menv_application as application;
pub use cxszn_menv_domain as domain;
pub use cxszn_menv_ports as ports;

use application::Orchestrator;
use ports::EmptyEnvironmentStore;

/// A safe, read-only default service used by the desktop shell until platform
/// adapters are wired. It exposes no arbitrary command execution.
pub type DefaultService = Orchestrator<EmptyEnvironmentStore>;

pub fn default_service() -> DefaultService {
    Orchestrator::new(EmptyEnvironmentStore)
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
