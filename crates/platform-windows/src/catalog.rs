//! Embedded, fixed-source PHP runtime catalog for the Windows adapter.
//!
//! The desktop shell must not invent download URLs.  This module turns the
//! checked-in catalog metadata into a small, typed list that the application
//! layer can present to a user and into the existing `PhpArtifactInstallSpec`
//! consumed by the safe downloader.  It deliberately does not perform network
//! or filesystem writes while loading or planning an artifact.
//!
//! The catalog is generated and signed outside this adapter.  Both the native
//! loader and the TypeScript catalog verifier validate the Ed25519 trust root
//! before exposing production metadata.  Rust additionally validates every
//! field needed by the installer and rejects a source, path, hash, or lifecycle
//! value outside the fixed policy.

use crate::install::{validate_install_spec, InstallError, PhpArtifactInstallSpec};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use thiserror::Error;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

/// The production catalog is embedded so an installed preview does not depend
/// on a repository checkout being present next to the executable.
pub const EMBEDDED_PHP_CATALOG_JSON: &str =
    include_str!("../../../catalog/manifests/php-windows-official.json");
pub const EMBEDDED_RUNTIME_TRUST_ROOT_JSON: &str =
    include_str!("../../../catalog/trust/runtime-root.json");
const APP_VERSION: &str = "0.0.2";

const MAX_CATALOG_ARTIFACT_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PhpCatalogDocument {
    pub schema_version: String,
    pub catalog_id: String,
    pub catalog_revision: u64,
    pub generated_at: String,
    pub expires_at: String,
    pub fixture: bool,
    #[serde(default)]
    pub fixture_note: Option<String>,
    pub signature: Option<PhpCatalogSignature>,
    pub policy: PhpCatalogPolicy,
    pub artifacts: Vec<PhpCatalogArtifact>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PhpCatalogSignature {
    pub algorithm: String,
    pub key_id: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RuntimeTrustRoot {
    schema_version: String,
    purpose: String,
    keys: Vec<RuntimeTrustKey>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RuntimeTrustKey {
    key_id: String,
    algorithm: String,
    public_key: String,
    #[serde(default)]
    revoked: bool,
    not_before: String,
    not_after: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PhpCatalogPolicy {
    pub min_app_version: String,
    pub revoked_artifact_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PhpCatalogArtifact {
    pub artifact_id: String,
    pub version: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub sha256: String,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub deprecated_after: Option<String>,
    pub runtime_family: String,
    pub release_channel: String,
    pub lifecycle: String,
    pub platform: String,
    pub arch: String,
    pub thread_safety: String,
    pub package_format: String,
    pub source: PhpCatalogSource,
    pub entrypoint: String,
    pub root_layout: String,
    pub dependencies: PhpCatalogDependencies,
    pub flags: PhpCatalogFlags,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PhpCatalogSource {
    pub kind: String,
    pub provider: String,
    pub host: String,
    pub artifact_path: String,
    #[serde(default)]
    pub redirect_hosts: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PhpCatalogDependencies {
    pub runtime: Vec<String>,
    pub extensions: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PhpCatalogFlags {
    pub legacy: bool,
    pub archival: bool,
    pub public_listener_default: bool,
}

/// Safe metadata suitable for listing in the desktop/CLI catalog view.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PhpArtifactSummary {
    /// Runtime family is repeated in the summary so protocol adapters do not
    /// have to infer it from a command name.  The desktop mapper treats this
    /// as a required native field for every catalog entry.
    pub family: String,
    pub artifact_id: String,
    pub version: String,
    pub architecture: String,
    pub thread_safety: String,
    pub lifecycle: String,
    pub legacy: bool,
    pub archival: bool,
    pub file_name: String,
    pub size_bytes: u64,
    pub installed: bool,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum CatalogError {
    #[error("PHP catalog JSON is invalid: {0}")]
    Parse(String),
    #[error("PHP catalog metadata is invalid: {0}")]
    InvalidMetadata(String),
    #[error("PHP catalog signature is invalid: {0}")]
    SignatureInvalid(String),
    #[error("PHP catalog signing key is not trusted: {0}")]
    KeyNotTrusted(String),
    #[error("PHP catalog is outside its validity window")]
    Expired,
    #[error("PHP catalog is not valid yet")]
    NotYetValid,
    #[error("PHP catalog requires a newer application version")]
    MinimumAppVersion,
    #[error("PHP catalog artifact {0} was not found")]
    ArtifactNotFound(String),
    #[error("PHP catalog artifact {0} is revoked")]
    ArtifactRevoked(String),
    #[error("PHP catalog artifact {0} is not available for Windows x64")]
    UnsupportedArtifact(String),
    #[error("PHP catalog install specification is invalid: {0}")]
    InstallSpec(#[from] InstallError),
}

/// Parse and validate catalog JSON.  This function has no I/O and is useful in
/// tests when a fixture is supplied instead of the embedded production file.
pub fn parse_php_catalog(json: &str) -> Result<PhpCatalogDocument, CatalogError> {
    let raw: Value =
        serde_json::from_str(json).map_err(|error| CatalogError::Parse(error.to_string()))?;
    let catalog: PhpCatalogDocument = serde_json::from_value(raw.clone())
        .map_err(|error| CatalogError::Parse(error.to_string()))?;
    validate_php_catalog(&catalog)?;
    verify_catalog_signature(&raw, &catalog)?;
    Ok(catalog)
}

/// Load the immutable PHP catalog bundled into the Windows adapter.
pub fn embedded_php_catalog() -> Result<PhpCatalogDocument, CatalogError> {
    parse_php_catalog(EMBEDDED_PHP_CATALOG_JSON)
}

/// Return all catalog entries that are safe to offer for this Windows x64
/// build.  Revoked entries are never returned, even if a caller asks for a
/// summary after a catalog update.
pub fn list_php_artifacts(
    catalog: &PhpCatalogDocument,
) -> Result<Vec<PhpArtifactSummary>, CatalogError> {
    validate_php_catalog(catalog)?;
    catalog
        .artifacts
        .iter()
        .filter(|artifact| {
            artifact.arch == "x86_64"
                && artifact.platform == "windows"
                && artifact.lifecycle != "revoked"
                && !catalog
                    .policy
                    .revoked_artifact_ids
                    .iter()
                    .any(|id| id == &artifact.artifact_id)
        })
        .map(|artifact| {
            Ok(PhpArtifactSummary {
                family: "php".to_owned(),
                artifact_id: artifact.artifact_id.clone(),
                version: artifact.version.clone(),
                architecture: artifact.arch.clone(),
                thread_safety: artifact.thread_safety.clone(),
                lifecycle: artifact.lifecycle.clone(),
                legacy: artifact.flags.legacy,
                archival: artifact.flags.archival,
                file_name: artifact.file_name.clone(),
                size_bytes: artifact.size_bytes,
                // Installation state belongs to the application state store;
                // catalog metadata alone cannot claim that a version is local.
                installed: false,
            })
        })
        .collect()
}

/// Create the existing safe downloader specification for one catalog entry.
/// `data_root` is the user's local MEnv data directory, not an arbitrary URL or
/// executable path.  The operation remains a plan; no network or filesystem
/// write occurs here.
pub fn create_php_install_spec(
    catalog: &PhpCatalogDocument,
    artifact_id: &str,
    data_root: &Path,
) -> Result<PhpArtifactInstallSpec, CatalogError> {
    validate_php_catalog(catalog)?;
    let artifact = catalog
        .artifacts
        .iter()
        .find(|candidate| candidate.artifact_id == artifact_id)
        .ok_or_else(|| CatalogError::ArtifactNotFound(artifact_id.to_owned()))?;
    if catalog
        .policy
        .revoked_artifact_ids
        .iter()
        .any(|id| id == &artifact.artifact_id)
        || artifact.lifecycle == "revoked"
    {
        return Err(CatalogError::ArtifactRevoked(artifact.artifact_id.clone()));
    }
    if artifact.platform != "windows" || artifact.arch != "x86_64" {
        return Err(CatalogError::UnsupportedArtifact(
            artifact.artifact_id.clone(),
        ));
    }
    let root = crate::path::validate_local_windows_path(data_root)
        .map_err(|error| CatalogError::InvalidMetadata(error.to_string()))?;
    let root = root.trim_end_matches('\\');
    let staging_root = windows_join(root, &["staging"]);
    let managed_directory = windows_join(
        root,
        &["versions", "php", &artifact.version, &artifact.arch],
    );
    let source = &artifact.source;
    let url = format!("https://{}{}", source.host, source.artifact_path);
    let spec = PhpArtifactInstallSpec {
        url,
        file_name: artifact.file_name.clone(),
        expected_size_bytes: artifact.size_bytes,
        expected_sha256: artifact.sha256.clone(),
        staging_root,
        managed_directory,
        entrypoint: artifact.entrypoint.clone(),
        journal_path: Some(windows_join(
            root,
            &["operations", "php", &artifact.version, "install.jsonl"],
        )),
    };
    validate_install_spec(&spec)?;
    Ok(spec)
}

fn windows_join(root: &str, components: &[&str]) -> PathBuf {
    let mut value = root.to_owned();
    for component in components {
        value.push('\\');
        value.push_str(component);
    }
    PathBuf::from(value)
}

fn validate_php_catalog(catalog: &PhpCatalogDocument) -> Result<(), CatalogError> {
    if catalog.schema_version != "1" {
        return Err(CatalogError::InvalidMetadata(
            "unsupported schema_version".to_owned(),
        ));
    }
    if catalog.catalog_id.trim().is_empty() || catalog.catalog_id.len() > 64 {
        return Err(CatalogError::InvalidMetadata(
            "catalog_id is empty or too long".to_owned(),
        ));
    }
    if catalog.catalog_revision == 0
        || catalog.generated_at.trim().is_empty()
        || catalog.expires_at.trim().is_empty()
    {
        return Err(CatalogError::InvalidMetadata(
            "catalog revision and validity window are required".to_owned(),
        ));
    }
    if catalog.fixture {
        return Err(CatalogError::InvalidMetadata(
            "fixture catalogs are not downloadable".to_owned(),
        ));
    }
    let Some(signature) = catalog.signature.as_ref() else {
        return Err(CatalogError::InvalidMetadata(
            "a signed catalog is required".to_owned(),
        ));
    };
    if signature.algorithm != "ed25519"
        || signature.key_id.trim().is_empty()
        || signature.value.trim().is_empty()
    {
        return Err(CatalogError::InvalidMetadata(
            "catalog signature metadata is invalid".to_owned(),
        ));
    }
    if catalog.policy.min_app_version.trim().is_empty() {
        return Err(CatalogError::InvalidMetadata(
            "policy.min_app_version is required".to_owned(),
        ));
    }
    let mut revoked = HashSet::new();
    for id in &catalog.policy.revoked_artifact_ids {
        if !safe_component(id) || !revoked.insert(id) {
            return Err(CatalogError::InvalidMetadata(
                "policy.revoked_artifact_ids contains an invalid or duplicate id".to_owned(),
            ));
        }
    }
    if catalog.artifacts.is_empty() {
        return Err(CatalogError::InvalidMetadata(
            "catalog must contain at least one artifact".to_owned(),
        ));
    }
    let mut ids = HashSet::new();
    for artifact in &catalog.artifacts {
        validate_php_artifact(artifact)?;
        if !ids.insert(&artifact.artifact_id) {
            return Err(CatalogError::InvalidMetadata(
                "catalog contains duplicate artifact ids".to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_php_artifact(artifact: &PhpCatalogArtifact) -> Result<(), CatalogError> {
    if !safe_component(&artifact.artifact_id)
        || artifact.artifact_id.contains("..")
        || !safe_component(&artifact.version)
        || !safe_version(&artifact.version)
        || !safe_component(&artifact.file_name)
        || artifact.file_name.contains("..")
        || artifact.file_name.contains('\\')
        || artifact.file_name.contains('/')
    {
        return Err(CatalogError::InvalidMetadata(format!(
            "artifact {} has an unsafe identifier or file name",
            artifact.artifact_id
        )));
    }
    if artifact.size_bytes == 0 || artifact.size_bytes > MAX_CATALOG_ARTIFACT_BYTES {
        return Err(CatalogError::InvalidMetadata(format!(
            "artifact {} has an unsafe size",
            artifact.artifact_id
        )));
    }
    if artifact.sha256.len() != 64
        || !artifact
            .sha256
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(CatalogError::InvalidMetadata(format!(
            "artifact {} has an invalid SHA-256",
            artifact.artifact_id
        )));
    }
    if artifact.runtime_family != "php"
        || artifact.platform != "windows"
        || artifact.package_format != "zip"
        || artifact.entrypoint != "php.exe"
        || artifact.root_layout != "archive_root"
    {
        return Err(CatalogError::UnsupportedArtifact(
            artifact.artifact_id.clone(),
        ));
    }
    if !matches!(
        artifact.release_channel.as_str(),
        "stable" | "beta" | "nightly" | "archival"
    ) {
        return Err(CatalogError::InvalidMetadata(format!(
            "artifact {} has an unsupported release channel",
            artifact.artifact_id
        )));
    }
    if !matches!(
        artifact.lifecycle.as_str(),
        "supported" | "eol" | "archival" | "revoked"
    ) {
        return Err(CatalogError::InvalidMetadata(format!(
            "artifact {} has an unsupported lifecycle",
            artifact.artifact_id
        )));
    }
    if (matches!(artifact.lifecycle.as_str(), "eol" | "archival" | "revoked")
        && !artifact.flags.legacy)
        || (matches!(artifact.lifecycle.as_str(), "archival" | "revoked")
            && !artifact.flags.archival)
    {
        return Err(CatalogError::InvalidMetadata(format!(
            "artifact {} has inconsistent lifecycle flags",
            artifact.artifact_id
        )));
    }
    if artifact.source.kind != "official"
        || artifact.source.provider != "php.net"
        || artifact.source.host != "downloads.php.net"
        || !artifact
            .source
            .artifact_path
            .starts_with("/~windows/releases/")
        || artifact.source.artifact_path.contains("..")
        || artifact.source.artifact_path.contains(['\\', '?', '#'])
        || artifact
            .source
            .artifact_path
            .split('/')
            .skip(1)
            .any(|segment| segment.is_empty())
        || artifact.source.artifact_path.ends_with('/')
        || artifact.source.artifact_path.rsplit('/').next() != Some(artifact.file_name.as_str())
        || artifact
            .source
            .redirect_hosts
            .iter()
            .any(|host| host != "downloads.php.net")
    {
        return Err(CatalogError::InvalidMetadata(format!(
            "artifact {} has an unapproved source",
            artifact.artifact_id
        )));
    }
    if artifact.arch != "x86_64" && artifact.arch != "x86" {
        return Err(CatalogError::UnsupportedArtifact(
            artifact.artifact_id.clone(),
        ));
    }
    if artifact.thread_safety != "nts" && artifact.thread_safety != "ts" {
        return Err(CatalogError::InvalidMetadata(format!(
            "artifact {} has an unsupported thread-safety value",
            artifact.artifact_id
        )));
    }
    if artifact.flags.public_listener_default {
        return Err(CatalogError::InvalidMetadata(format!(
            "artifact {} enables a public listener by default",
            artifact.artifact_id
        )));
    }
    if let Some(published_at) = &artifact.published_at {
        parse_rfc3339(published_at, "artifact published_at")?;
    }
    if let Some(deprecated_after) = &artifact.deprecated_after {
        parse_rfc3339(deprecated_after, "artifact deprecated_after")?;
    }
    Ok(())
}

/// Verify the signed catalog against the embedded trust root.  The payload is
/// canonicalized with the same sorted-object-key algorithm used by the shared
/// TypeScript core (`stableStringify`), while retaining omitted optional fields
/// exactly as they appeared in the signed JSON.
fn verify_catalog_signature(raw: &Value, catalog: &PhpCatalogDocument) -> Result<(), CatalogError> {
    let trust_root: RuntimeTrustRoot = serde_json::from_str(EMBEDDED_RUNTIME_TRUST_ROOT_JSON)
        .map_err(|error| CatalogError::SignatureInvalid(error.to_string()))?;
    if trust_root.schema_version != "1" || trust_root.purpose != "runtime-catalog" {
        return Err(CatalogError::SignatureInvalid(
            "embedded trust root metadata is invalid".to_owned(),
        ));
    }
    let signature = catalog
        .signature
        .as_ref()
        .ok_or_else(|| CatalogError::SignatureInvalid("catalog signature is missing".to_owned()))?;
    let key = trust_root
        .keys
        .iter()
        .find(|candidate| candidate.key_id == signature.key_id)
        .ok_or_else(|| CatalogError::KeyNotTrusted(signature.key_id.clone()))?;
    if key.algorithm != "ed25519" || key.revoked {
        return Err(CatalogError::KeyNotTrusted(key.key_id.clone()));
    }
    let now = OffsetDateTime::now_utc();
    let generated = parse_rfc3339(&catalog.generated_at, "generated_at")?;
    let expires = parse_rfc3339(&catalog.expires_at, "expires_at")?;
    if expires <= generated {
        return Err(CatalogError::InvalidMetadata(
            "catalog expires_at must be after generated_at".to_owned(),
        ));
    }
    if now < generated {
        return Err(CatalogError::NotYetValid);
    }
    if now >= expires {
        return Err(CatalogError::Expired);
    }
    let key_not_before = parse_rfc3339(&key.not_before, "trust key not_before")?;
    let key_not_after = parse_rfc3339(&key.not_after, "trust key not_after")?;
    if key_not_after <= key_not_before {
        return Err(CatalogError::SignatureInvalid(
            "trust key validity window is invalid".to_owned(),
        ));
    }
    if now < key_not_before || now >= key_not_after {
        return Err(CatalogError::KeyNotTrusted(format!(
            "signing key {} is outside its validity window",
            key.key_id
        )));
    }
    if compare_versions(APP_VERSION, &catalog.policy.min_app_version)? < 0 {
        return Err(CatalogError::MinimumAppVersion);
    }

    let public_key_bytes = decode_base64url(&key.public_key, "trust root public key")?;
    let public_key_bytes: [u8; 32] = public_key_bytes.try_into().map_err(|_| {
        CatalogError::SignatureInvalid("Ed25519 public key must contain 32 bytes".to_owned())
    })?;
    let verifying_key = VerifyingKey::from_bytes(&public_key_bytes)
        .map_err(|error| CatalogError::SignatureInvalid(error.to_string()))?;
    let signature_bytes = decode_base64url(&signature.value, "catalog signature")?;
    let signature = Signature::from_slice(&signature_bytes)
        .map_err(|error| CatalogError::SignatureInvalid(error.to_string()))?;
    let mut payload = raw.clone();
    let object = payload.as_object_mut().ok_or_else(|| {
        CatalogError::SignatureInvalid("catalog payload must be a JSON object".to_owned())
    })?;
    object.insert("signature".to_owned(), Value::Null);
    let canonical = stable_json(&payload);
    verifying_key
        .verify(canonical.as_bytes(), &signature)
        .map_err(|_| CatalogError::SignatureInvalid("Ed25519 verification failed".to_owned()))
}

fn decode_base64url(value: &str, field: &str) -> Result<Vec<u8>, CatalogError> {
    URL_SAFE_NO_PAD.decode(value).map_err(|error| {
        CatalogError::SignatureInvalid(format!("{field} is not base64url: {error}"))
    })
}

fn parse_rfc3339(value: &str, field: &str) -> Result<OffsetDateTime, CatalogError> {
    OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|error| CatalogError::InvalidMetadata(format!("{field} is not RFC3339: {error}")))
}

fn compare_versions(left: &str, right: &str) -> Result<i8, CatalogError> {
    fn parse(value: &str) -> Option<(u64, u64, u64)> {
        let core = value.split(['-', '+']).next()?;
        if core.split('.').any(|part| {
            part.is_empty()
                || (part.len() > 1 && part.starts_with('0'))
                || !part.chars().all(|character| character.is_ascii_digit())
        }) {
            return None;
        }
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some((major, minor, patch))
    }
    let left = parse(left).ok_or_else(|| {
        CatalogError::InvalidMetadata("application version is not semantic version".to_owned())
    })?;
    let right = parse(right).ok_or_else(|| {
        CatalogError::InvalidMetadata("catalog minimum version is not semantic version".to_owned())
    })?;
    Ok(match left.cmp(&right) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    })
}

fn stable_json(value: &Value) -> String {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            serde_json::to_string(value).expect("JSON primitive serialization cannot fail")
        }
        Value::Array(values) => {
            let values = values.iter().map(stable_json).collect::<Vec<_>>().join(",");
            format!("[{values}]")
        }
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            let values = keys
                .into_iter()
                .map(|key| {
                    let value = object
                        .get(key)
                        .expect("key collected from the same JSON object");
                    format!(
                        "{}:{}",
                        serde_json::to_string(key)
                            .expect("JSON object key serialization cannot fail"),
                        stable_json(value)
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{values}}}")
        }
    }
}

fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || ".-_+".contains(character))
}

fn safe_version(value: &str) -> bool {
    let core = value.split(['-', '+']).next().unwrap_or_default();
    let parts = core.split('.').collect::<Vec<_>>();
    parts.len() == 3
        && parts.iter().all(|part| {
            let normalized = part.trim_start_matches('0');
            !part.is_empty()
                && part.len() <= 10
                && part.chars().all(|character| character.is_ascii_digit())
                && (normalized.is_empty() || !part.starts_with('0'))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_lists_multiple_php_versions() {
        let catalog = embedded_php_catalog().expect("embedded catalog");
        let entries = list_php_artifacts(&catalog).expect("list catalog");
        assert_eq!(entries.len(), 7);
        assert!(entries.iter().any(|entry| entry.version == "8.2.34"));
        assert!(entries.iter().any(|entry| entry.version == "8.5.11"));
        assert!(entries
            .iter()
            .any(|entry| entry.version == "8.1.34" && entry.legacy));
        assert!(entries
            .iter()
            .any(|entry| entry.version == "7.4.33" && entry.legacy));
        assert!(entries.iter().all(|entry| entry.family == "php"));
        assert!(entries.iter().all(|entry| entry.architecture == "x86_64"));
        assert!(entries.iter().all(|entry| !entry.installed));
    }

    #[test]
    fn creates_fixed_plan_for_another_php_version_without_writing() {
        let catalog = embedded_php_catalog().expect("embedded catalog");
        let spec = create_php_install_spec(
            &catalog,
            "php-8.4.26-nts-x64",
            Path::new(r"C:\Users\demo\AppData\Local\cxszn-menv"),
        )
        .expect("download plan");
        assert_eq!(
            spec.url,
            "https://downloads.php.net/~windows/releases/php-8.4.26-nts-Win32-vs17-x64.zip"
        );
        assert!(spec
            .managed_directory
            .to_string_lossy()
            .ends_with(r"versions\php\8.4.26\x86_64"));
        assert!(!spec.managed_directory.exists());
    }

    #[test]
    fn every_x64_catalog_entry_produces_an_install_spec() {
        let catalog = embedded_php_catalog().expect("embedded catalog");
        for entry in list_php_artifacts(&catalog).expect("list catalog") {
            let spec = create_php_install_spec(
                &catalog,
                &entry.artifact_id,
                Path::new(r"C:\Users\demo\AppData\Local\cxszn-menv"),
            )
            .expect("fixed install spec");
            assert_eq!(spec.entrypoint, "php.exe");
            assert_eq!(spec.file_name, entry.file_name);
            assert!(spec
                .url
                .starts_with("https://downloads.php.net/~windows/releases/"));
        }
    }

    #[test]
    fn rejects_unknown_revoked_and_unsafe_artifacts() {
        let catalog = embedded_php_catalog().expect("embedded catalog");
        assert!(matches!(
            create_php_install_spec(
                &catalog,
                "php-9.0.0-nts-x64",
                Path::new(r"C:\Users\demo\AppData\Local\cxszn-menv")
            ),
            Err(CatalogError::ArtifactNotFound(_))
        ));

        let mut revoked = catalog.clone();
        revoked
            .policy
            .revoked_artifact_ids
            .push("php-8.4.26-nts-x64".to_owned());
        assert!(matches!(
            create_php_install_spec(
                &revoked,
                "php-8.4.26-nts-x64",
                Path::new(r"C:\Users\demo\AppData\Local\cxszn-menv")
            ),
            Err(CatalogError::ArtifactRevoked(_))
        ));

        let mut hostile = catalog;
        hostile.artifacts[0].source.artifact_path = "/~windows/releases/../payload.zip".to_owned();
        assert!(matches!(
            parse_php_catalog(&serde_json::to_string(&hostile).expect("serialize hostile")),
            Err(CatalogError::InvalidMetadata(_))
        ));
    }

    #[test]
    fn fixture_catalogs_fail_closed_before_install() {
        let mut catalog = embedded_php_catalog().expect("embedded catalog");
        catalog.fixture = true;
        assert!(matches!(
            list_php_artifacts(&catalog),
            Err(CatalogError::InvalidMetadata(_))
        ));
    }

    #[test]
    fn signed_payload_tamper_and_policy_downgrade_fail_closed() {
        let mut tampered: Value =
            serde_json::from_str(EMBEDDED_PHP_CATALOG_JSON).expect("embedded JSON");
        tampered["artifacts"][0]["size_bytes"] = Value::from(1_u64);
        assert!(matches!(
            parse_php_catalog(&tampered.to_string()),
            Err(CatalogError::SignatureInvalid(_))
        ));

        let mut downgraded: Value =
            serde_json::from_str(EMBEDDED_PHP_CATALOG_JSON).expect("embedded JSON");
        downgraded["policy"]["min_app_version"] = Value::from("9.0.0");
        assert!(matches!(
            parse_php_catalog(&downgraded.to_string()),
            Err(CatalogError::MinimumAppVersion)
        ));

        let mut expired: Value =
            serde_json::from_str(EMBEDDED_PHP_CATALOG_JSON).expect("embedded JSON");
        expired["generated_at"] = Value::from("2019-01-01T00:00:00.000Z");
        expired["expires_at"] = Value::from("2020-01-01T00:00:00.000Z");
        assert!(matches!(
            parse_php_catalog(&expired.to_string()),
            Err(CatalogError::Expired)
        ));

        let mut not_yet_valid: Value =
            serde_json::from_str(EMBEDDED_PHP_CATALOG_JSON).expect("embedded JSON");
        not_yet_valid["generated_at"] = Value::from("2999-01-01T00:00:00.000Z");
        not_yet_valid["expires_at"] = Value::from("3000-01-01T00:00:00.000Z");
        assert!(matches!(
            parse_php_catalog(&not_yet_valid.to_string()),
            Err(CatalogError::NotYetValid)
        ));
    }
}
