//! Platform-neutral domain contracts for MEnv.
//!
//! This crate deliberately contains no Tauri, Windows registry, process, or UI
//! types.  Adapters translate platform details into these stable contracts.

use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

pub const SCHEMA_VERSION: &str = "1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeFamily {
    Php,
    Node,
    Mysql,
    Python,
}

impl fmt::Display for RuntimeFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Php => "php",
            Self::Node => "node",
            Self::Mysql => "mysql",
            Self::Python => "python",
        };
        f.write_str(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Architecture {
    #[serde(rename = "x86")]
    X86,
    #[serde(rename = "x64")]
    X64,
    #[serde(rename = "arm64")]
    Arm64,
    #[serde(rename = "unknown")]
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Ownership {
    DetectedUnmanaged,
    Manual,
    Managed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Availability {
    NotInstalled,
    DetectedUnmanaged,
    Manual,
    Managed,
    InvalidPath,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Activation {
    Disabled,
    Enabling,
    Active,
    Disabling,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Health {
    Unknown,
    Healthy,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OperationStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeSource {
    SystemPath,
    CommonLocation,
    Manual,
    ManagedCatalog,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PhpThreadSafety {
    Ts,
    Nts,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeIniSource {
    Unknown,
    None,
    Explicit,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeIniInfo {
    pub source: RuntimeIniSource,
    pub path: Option<String>,
}

/// A stable runtime identity used by all protocol adapters.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RuntimeId(pub String);

impl RuntimeId {
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        if value.trim().is_empty() || value.len() > 256 {
            return Err(DomainError::InvalidRuntimeId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRecord {
    pub id: RuntimeId,
    pub family: RuntimeFamily,
    pub version: Option<String>,
    pub executable_path: String,
    pub root_path: String,
    pub architecture: Architecture,
    pub thread_safety: PhpThreadSafety,
    pub ownership: Ownership,
    pub availability: Availability,
    pub activation: Activation,
    pub health: Health,
    pub ini: RuntimeIniInfo,
    pub legacy: bool,
    pub diagnostics: Vec<String>,
    pub first_seen_at: String,
    pub last_checked_at: String,
}

impl RuntimeRecord {
    /// New observations are intentionally not active. Discovery never changes
    /// the user's PATH or starts a runtime.
    pub fn discovered(
        id: RuntimeId,
        family: RuntimeFamily,
        version: impl Into<String>,
        architecture: Architecture,
        path: impl Into<String>,
        source: RuntimeSource,
    ) -> Self {
        let version = version.into();
        let ownership = match &source {
            RuntimeSource::Manual => Ownership::Manual,
            RuntimeSource::ManagedCatalog => Ownership::Managed,
            RuntimeSource::SystemPath | RuntimeSource::CommonLocation => {
                Ownership::DetectedUnmanaged
            }
        };
        let availability = match &source {
            RuntimeSource::Manual => Availability::Manual,
            RuntimeSource::ManagedCatalog => Availability::Managed,
            RuntimeSource::SystemPath | RuntimeSource::CommonLocation => {
                Availability::DetectedUnmanaged
            }
        };
        let legacy = family == RuntimeFamily::Php && version.starts_with("5.");
        Self {
            id,
            family,
            version: Some(version),
            executable_path: path.into(),
            root_path: String::new(),
            architecture,
            thread_safety: PhpThreadSafety::Unknown,
            ownership,
            availability,
            activation: Activation::Disabled,
            health: Health::Unknown,
            ini: RuntimeIniInfo {
                source: RuntimeIniSource::Unknown,
                path: None,
            },
            legacy,
            diagnostics: Vec::new(),
            first_seen_at: String::new(),
            last_checked_at: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathLeaseState {
    pub scope: EnvironmentScope,
    pub enabled: bool,
    pub path: Option<String>,
    pub runtime_id: Option<RuntimeId>,
    pub entries: Vec<String>,
    pub hash: String,
    pub updated_at: Option<String>,
}

impl Default for PathLeaseState {
    fn default() -> Self {
        Self {
            scope: EnvironmentScope::User,
            enabled: false,
            path: None,
            runtime_id: None,
            entries: Vec::new(),
            hash: String::new(),
            updated_at: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnvironmentScope {
    Process,
    User,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnvironmentChangeAction {
    Register,
    Install,
    Enable,
    Disable,
    Switch,
    #[serde(rename = "backup-ini")]
    BackupIni,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationRecord {
    pub operation_id: String,
    pub plan_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_hash: Option<String>,
    pub idempotency_key: String,
    pub action: EnvironmentChangeAction,
    pub runtime_id: RuntimeId,
    pub status: OperationStatus,
    pub created_at: String,
    pub updated_at: String,
    pub error: Option<SerializedCoreError>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedCoreError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
    pub next_actions: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentSnapshot {
    pub schema_version: String,
    pub runtimes: Vec<RuntimeRecord>,
    pub path_lease: PathLeaseState,
    pub operations: Vec<OperationRecord>,
    pub updated_at: String,
}

impl Default for EnvironmentSnapshot {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION.to_owned(),
            runtimes: Vec::new(),
            path_lease: PathLeaseState::default(),
            operations: Vec::new(),
            // A deterministic placeholder keeps the wire shape valid until a
            // platform clock is injected by the composition root.
            updated_at: "1970-01-01T00:00:00.000Z".to_owned(),
        }
    }
}

pub type RuntimeState = EnvironmentSnapshot;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("runtime id must be non-empty and at most 256 characters")]
    InvalidRuntimeId,
    #[error("runtime version must be non-empty")]
    InvalidRuntimeVersion,
    #[error("unsupported state transition: {from:?} -> {to:?}")]
    InvalidActivationTransition { from: Activation, to: Activation },
}

pub fn validate_version(version: &str) -> Result<(), DomainError> {
    if version.trim().is_empty() {
        return Err(DomainError::InvalidRuntimeVersion);
    }
    Ok(())
}

pub fn can_transition_activation(from: &Activation, to: &Activation) -> bool {
    matches!(
        (from, to),
        (Activation::Disabled, Activation::Enabling)
            | (Activation::Enabling, Activation::Active)
            | (Activation::Enabling, Activation::Failed)
            | (Activation::Active, Activation::Disabling)
            | (Activation::Disabling, Activation::Disabled)
            | (Activation::Disabling, Activation::Failed)
            | (Activation::Failed, Activation::Disabled)
            | (Activation::Failed, Activation::Enabling)
    )
}

pub fn validate_activation_transition(from: Activation, to: Activation) -> Result<(), DomainError> {
    if can_transition_activation(&from, &to) {
        Ok(())
    } else {
        Err(DomainError::InvalidActivationTransition { from, to })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newly_discovered_runtime_is_disabled() {
        let id = RuntimeId::new("php-8.3-x64").expect("valid id");
        let record = RuntimeRecord::discovered(
            id,
            RuntimeFamily::Php,
            "8.3.0",
            Architecture::X64,
            r"C:\php\php.exe",
            RuntimeSource::SystemPath,
        );
        assert_eq!(record.activation, Activation::Disabled);
        assert_eq!(record.ownership, Ownership::DetectedUnmanaged);
        assert!(!record.legacy);
    }

    #[test]
    fn activation_transition_rejects_direct_enable() {
        assert!(validate_activation_transition(Activation::Disabled, Activation::Active).is_err());
        assert!(validate_activation_transition(Activation::Disabled, Activation::Enabling).is_ok());
    }

    #[test]
    fn snapshot_uses_shared_schema_version() {
        assert_eq!(EnvironmentSnapshot::default().schema_version, "1");
    }
}
