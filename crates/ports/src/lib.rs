//! Port traits used by the application layer.
//!
//! Implementations live in platform-specific crates.  Keeping these traits
//! free from Tauri and Windows types lets CLI, MCP, and desktop share behavior.

use cxszn_menv_domain::{EnvironmentSnapshot, RuntimeFamily, RuntimeId, RuntimeRecord};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanRequest {
    pub family: Option<RuntimeFamily>,
    pub include_common_locations: bool,
}

impl Default for ScanRequest {
    fn default() -> Self {
        Self {
            family: None,
            include_common_locations: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyPathRequest {
    pub family: RuntimeFamily,
    pub path: PathBuf,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum PortError {
    #[error("operation is not supported on this platform")]
    UnsupportedPlatform,
    #[error("path is invalid: {0}")]
    InvalidPath(String),
    #[error("runtime executable did not respond in time")]
    ProbeTimeout,
    #[error("platform operation failed: {0}")]
    Platform(String),
}

/// Discovers runtimes without mutating PATH or taking ownership of them.
pub trait RuntimeProvider: Send + Sync {
    fn family(&self) -> RuntimeFamily;

    fn scan(&self, request: &ScanRequest) -> Result<Vec<RuntimeRecord>, PortError>;

    fn verify_path(&self, request: &VerifyPathRequest) -> Result<RuntimeRecord, PortError>;
}

/// Read/write boundary for the persisted environment snapshot. Implementations
/// must use a journal for mutations; this trait does not expose arbitrary SQL.
pub trait EnvironmentStore: Send + Sync {
    fn load(&self) -> Result<EnvironmentSnapshot, PortError>;

    fn save(&self, snapshot: &EnvironmentSnapshot) -> Result<(), PortError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathLease {
    pub lease_id: String,
    pub shim_directory: PathBuf,
    /// The already-verified runtime executable that the app-owned shim must
    /// target. `None` is allowed only for contract fakes that manage PATH
    /// without publishing a runtime shim.
    pub target_executable: Option<PathBuf>,
}

/// User-level PATH lease adapter. It must preserve unknown entries and reject
/// concurrent external modifications rather than overwriting them.
pub trait UserPathStore: Send + Sync {
    fn read(&self) -> Result<Vec<String>, PortError>;

    fn apply_lease(&self, lease: &PathLease) -> Result<(), PortError>;

    fn remove_lease(&self, lease: &PathLease) -> Result<(), PortError>;
}

/// Process probing is explicit and bounded. Shell command strings are not part
/// of this interface; adapters receive an executable and argv separately.
pub trait ProcessProbe: Send + Sync {
    fn run_version_probe(&self, executable: &Path, args: &[String]) -> Result<String, PortError>;
}

/// Minimal in-memory store for contract tests and the Tauri shell skeleton.
#[derive(Default)]
pub struct EmptyEnvironmentStore;

impl EnvironmentStore for EmptyEnvironmentStore {
    fn load(&self) -> Result<EnvironmentSnapshot, PortError> {
        Ok(EnvironmentSnapshot::default())
    }

    fn save(&self, _snapshot: &EnvironmentSnapshot) -> Result<(), PortError> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeLookup {
    pub id: RuntimeId,
    pub family: RuntimeFamily,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoopProvider;

    impl RuntimeProvider for NoopProvider {
        fn family(&self) -> RuntimeFamily {
            RuntimeFamily::Php
        }

        fn scan(&self, _request: &ScanRequest) -> Result<Vec<RuntimeRecord>, PortError> {
            Ok(Vec::new())
        }

        fn verify_path(&self, _request: &VerifyPathRequest) -> Result<RuntimeRecord, PortError> {
            Err(PortError::UnsupportedPlatform)
        }
    }

    #[test]
    fn provider_contract_is_read_only() {
        let provider = NoopProvider;
        let records = provider
            .scan(&ScanRequest::default())
            .expect("scan succeeds");
        assert!(records.is_empty());
        assert_eq!(provider.family(), RuntimeFamily::Php);
    }
}
