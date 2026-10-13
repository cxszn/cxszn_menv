//! Windows adapter boundary.
//!
//! The first slice intentionally provides contract-safe placeholders. Real
//! registry/PATH, ConPTY, and process probing code belongs here and must stay
//! behind the `RuntimeProvider`, `UserPathStore`, and `ProcessProbe` ports.

pub mod catalog;
pub mod conpty;
#[cfg(windows)]
pub mod console;
pub mod install;
pub mod lock;
pub mod mysql;
pub mod node;
pub mod path;
pub mod php;
pub mod python;
pub mod shim;
pub mod store;

pub use path::{
    merge_app_owned_entry, path_hash, validate_local_windows_path, WindowsUserPathStore,
};
pub use shim::{run_php_shim_if_invoked, ShimActivation, ShimManager, ShimTarget, PHP_SHIM_NAME};
pub use store::JsonEnvironmentStore;

pub use catalog::{
    create_php_install_spec, embedded_php_catalog, list_php_artifacts, parse_php_catalog,
    CatalogError, PhpArtifactSummary, PhpCatalogArtifact, PhpCatalogDocument,
    EMBEDDED_PHP_CATALOG_JSON,
};
pub use mysql::WindowsMysqlProvider;
pub use node::WindowsNodeProvider;
pub use php::WindowsPhpProvider;
pub use python::WindowsPythonProvider;
#[cfg(windows)]
pub use store::SqliteEnvironmentStore;

#[cfg(test)]
mod tests {
    use super::*;
    use cxszn_menv_domain::{RuntimeFamily, RuntimeRecord};
    use cxszn_menv_ports::{RuntimeProvider, ScanRequest};

    #[test]
    fn provider_is_php_only() {
        let provider = WindowsPhpProvider;
        assert_eq!(provider.family(), RuntimeFamily::Php);
        assert_eq!(
            provider
                .scan(&ScanRequest {
                    family: Some(RuntimeFamily::Node),
                    include_common_locations: false
                })
                .unwrap(),
            Vec::<RuntimeRecord>::new()
        );
    }

    #[test]
    fn node_provider_is_node_only() {
        let provider = WindowsNodeProvider;
        assert_eq!(provider.family(), RuntimeFamily::Node);
        assert_eq!(
            provider
                .scan(&ScanRequest {
                    family: Some(RuntimeFamily::Php),
                    include_common_locations: false
                })
                .unwrap(),
            Vec::<RuntimeRecord>::new()
        );
    }

    #[test]
    fn python_provider_is_python_only() {
        let provider = WindowsPythonProvider;
        assert_eq!(provider.family(), RuntimeFamily::Python);
        assert_eq!(
            provider
                .scan(&ScanRequest {
                    family: Some(RuntimeFamily::Php),
                    include_common_locations: false
                })
                .unwrap(),
            Vec::<RuntimeRecord>::new()
        );
    }

    #[test]
    fn mysql_provider_is_mysql_only() {
        let provider = WindowsMysqlProvider;
        assert_eq!(provider.family(), RuntimeFamily::Mysql);
        assert_eq!(
            provider
                .scan(&ScanRequest {
                    family: Some(RuntimeFamily::Php),
                    include_common_locations: false
                })
                .unwrap(),
            Vec::<RuntimeRecord>::new()
        );
    }
}
