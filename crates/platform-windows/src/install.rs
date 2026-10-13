//! Safe PHP artifact staging and installation boundary.
//!
//! The adapter accepts only the fixed official PHP release host. It downloads
//! into an app-owned staging directory, verifies the exact byte count and
//! SHA-256, rejects unsafe ZIP members, and publishes a new immutable managed
//! directory with one rename. It never replaces an existing version.

use serde::{Deserialize, Serialize};
#[cfg(windows)]
use sha2::{Digest, Sha256};
#[cfg(windows)]
use std::fs::{self, File, OpenOptions};
#[cfg(windows)]
use std::io::{Read, Write};
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

const MAX_ARCHIVE_ENTRIES: usize = 20_000;
const MAX_UNPACKED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_DOWNLOAD_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct PhpArtifactInstallSpec {
    pub url: String,
    pub file_name: String,
    pub expected_size_bytes: u64,
    pub expected_sha256: String,
    pub staging_root: PathBuf,
    pub managed_directory: PathBuf,
    pub entrypoint: String,
    /// Optional append-only journal path owned by the application operation.
    /// `None` keeps the low-level adapter useful for pure contract tests.
    pub journal_path: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallJournalEntry<'a> {
    schema_version: &'static str,
    stage: &'a str,
    updated_at_epoch_seconds: u64,
    file_name: &'a str,
    expected_sha256: &'a str,
    managed_directory: String,
    error: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledPhpArtifact {
    pub managed_directory: PathBuf,
    pub downloaded_size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum InstallError {
    #[error("artifact installation is only available on Windows")]
    UnsupportedPlatform,
    #[error("artifact source is not a fixed official PHP HTTPS URL")]
    SourceNotAllowed,
    #[error("artifact metadata is invalid: {0}")]
    InvalidMetadata(String),
    #[error("download failed: {0}")]
    Download(String),
    #[error("download size does not match the catalog")]
    SizeMismatch,
    #[error("download SHA-256 does not match the catalog")]
    HashMismatch,
    #[error("archive is unsafe: {0}")]
    UnsafeArchive(String),
    #[error("archive entrypoint was not found")]
    EntrypointMissing,
    #[error("managed version already exists")]
    ManagedVersionExists,
    #[error("install filesystem operation failed: {0}")]
    Filesystem(String),
}

/// Validate the immutable metadata before any network or filesystem mutation.
pub fn validate_install_spec(spec: &PhpArtifactInstallSpec) -> Result<(), InstallError> {
    let Some(rest) = spec.url.strip_prefix("https://") else {
        return Err(InstallError::SourceNotAllowed);
    };
    let Some((host, path)) = rest.split_once('/') else {
        return Err(InstallError::SourceNotAllowed);
    };
    if host != "downloads.php.net"
        || path.contains('?')
        || path.contains('#')
        || !path.starts_with("~windows/releases/")
        || path.contains("..")
        || path.ends_with('/')
    {
        return Err(InstallError::SourceNotAllowed);
    }
    if path.rsplit('/').next() != Some(spec.file_name.as_str()) {
        return Err(InstallError::InvalidMetadata(
            "URL basename must match file_name".to_owned(),
        ));
    }
    if spec.file_name.is_empty()
        || spec.file_name.contains(['/', '\\'])
        || spec.file_name.contains("..")
        || !spec.file_name.to_ascii_lowercase().ends_with(".zip")
    {
        return Err(InstallError::InvalidMetadata(
            "file_name must be one ZIP filename".to_owned(),
        ));
    }
    if spec.expected_size_bytes == 0 || spec.expected_size_bytes > MAX_DOWNLOAD_BYTES {
        return Err(InstallError::InvalidMetadata(
            "expected_size_bytes is outside the safe range".to_owned(),
        ));
    }
    if spec.expected_sha256.len() != 64
        || !spec
            .expected_sha256
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(InstallError::InvalidMetadata(
            "expected_sha256 must be 64 hexadecimal characters".to_owned(),
        ));
    }
    if spec.entrypoint != "php.exe" {
        return Err(InstallError::InvalidMetadata(
            "only php.exe is accepted as the PHP entrypoint".to_owned(),
        ));
    }
    if spec.staging_root.as_os_str().is_empty() || spec.managed_directory.as_os_str().is_empty() {
        return Err(InstallError::InvalidMetadata(
            "staging_root and managed_directory are required".to_owned(),
        ));
    }
    Ok(())
}

/// Append one non-secret lifecycle record for an installation operation.
/// JSONL avoids replacing a previous record on Windows and preserves the last
/// durable stage after a process crash.
pub fn append_install_journal(
    spec: &PhpArtifactInstallSpec,
    stage: &str,
    error: Option<&str>,
) -> Result<(), InstallError> {
    let Some(path) = spec.journal_path.as_ref() else {
        return Ok(());
    };
    let parent = path
        .parent()
        .ok_or_else(|| InstallError::InvalidMetadata("journal path has no parent".to_owned()))?;
    #[cfg(windows)]
    fs::create_dir_all(parent)
        .map_err(|value| InstallError::Filesystem(format!("create journal directory: {value}")))?;
    #[cfg(not(windows))]
    let _ = parent;
    let entry = InstallJournalEntry {
        schema_version: "1",
        stage,
        updated_at_epoch_seconds: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_secs())
            .unwrap_or_default(),
        file_name: &spec.file_name,
        expected_sha256: &spec.expected_sha256,
        managed_directory: spec.managed_directory.to_string_lossy().replace('/', "\\"),
        error,
    };
    let mut line = serde_json::to_vec(&entry)
        .map_err(|value| InstallError::Filesystem(format!("serialize install journal: {value}")))?;
    line.push(b'\n');
    #[cfg(windows)]
    {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|value| InstallError::Filesystem(format!("open install journal: {value}")))?;
        file.write_all(&line)
            .map_err(|value| InstallError::Filesystem(format!("write install journal: {value}")))?;
        file.sync_all()
            .map_err(|value| InstallError::Filesystem(format!("flush install journal: {value}")))?;
        if should_mirror_install_journal(path) {
            let store = crate::store::SqliteEnvironmentStore::default_for_current_user();
            store
                .append_install_journal_row(path, &String::from_utf8_lossy(&line))
                .map_err(|value| {
                    InstallError::Filesystem(format!("mirror install journal in SQLite: {value}"))
                })?;
        }
    }
    Ok(())
}

#[cfg(windows)]
fn should_mirror_install_journal(path: &Path) -> bool {
    let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") else {
        return false;
    };
    let root = PathBuf::from(local_app_data).join("cxszn-menv");
    let Ok(journal) = path.canonicalize() else {
        return path.to_string_lossy().to_ascii_lowercase().starts_with(
            &root
                .to_string_lossy()
                .replace('/', "\\")
                .to_ascii_lowercase(),
        );
    };
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    journal.starts_with(root)
}

/// Download, verify, safely extract, and atomically publish a PHP version.
pub fn install_php_artifact(
    spec: &PhpArtifactInstallSpec,
) -> Result<InstalledPhpArtifact, InstallError> {
    validate_install_spec(spec)?;
    #[cfg(not(windows))]
    {
        let _ = spec;
        Err(InstallError::UnsupportedPlatform)
    }
    #[cfg(windows)]
    {
        install_windows(spec)
    }
}

#[cfg(windows)]
fn install_windows(spec: &PhpArtifactInstallSpec) -> Result<InstalledPhpArtifact, InstallError> {
    validate_windows_install_paths(spec)?;
    append_install_journal(spec, "planned", None)?;
    fs::create_dir_all(&spec.staging_root)
        .map_err(|error| InstallError::Filesystem(format!("create staging root: {error}")))?;
    let staging_root = fs::canonicalize(&spec.staging_root)
        .map_err(|error| InstallError::Filesystem(format!("resolve staging root: {error}")))?;
    if is_reparse(&staging_root)? {
        return Err(InstallError::UnsafeArchive(
            "staging root must not be a reparse point".to_owned(),
        ));
    }
    if spec.managed_directory.exists() {
        return Err(InstallError::ManagedVersionExists);
    }
    let stage = staging_root.join(format!(
        ".php-install-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    if stage.exists() {
        return Err(InstallError::Filesystem(
            "staging operation directory already exists".to_owned(),
        ));
    }
    fs::create_dir_all(&stage)
        .map_err(|error| InstallError::Filesystem(format!("create staging directory: {error}")))?;
    append_install_journal(spec, "staged", None)?;
    let result = install_windows_staged(spec, &stage);
    // The managed directory is atomically published outside staging. Remove
    // the operation directory after both success and failure so a later
    // catalog install in the same process cannot inherit stale staging state.
    let cleanup_result = fs::remove_dir_all(&stage);
    if result.is_ok() {
        if let Err(error) = cleanup_result {
            let _ = append_install_journal(spec, "published", None);
            return Err(InstallError::Filesystem(format!(
                "clean staging operation directory: {error}"
            )));
        }
        if let Err(journal_error) = append_install_journal(spec, "published", None) {
            // The managed directory was created by this operation, but the
            // durable recovery marker could not be written. Remove the new
            // directory before surfacing the failure so a restart cannot see
            // an untracked half-complete installation.
            let rollback = fs::remove_dir_all(&spec.managed_directory);
            let rollback_detail = match rollback {
                Ok(()) => "new managed directory rolled back".to_owned(),
                Err(error) => format!("failed to roll back managed directory: {error}"),
            };
            let message =
                format!("write published install journal: {journal_error}; {rollback_detail}");
            let _ = append_install_journal(spec, "failed", Some(&message));
            return Err(InstallError::Filesystem(message));
        }
    }
    if let Err(error) = &result {
        let _ = append_install_journal(spec, "failed", Some(&error.to_string()));
    }
    result
}

#[cfg(windows)]
fn validate_windows_install_paths(spec: &PhpArtifactInstallSpec) -> Result<(), InstallError> {
    for (label, path) in [
        ("staging_root", spec.staging_root.as_path()),
        ("managed_directory", spec.managed_directory.as_path()),
    ] {
        let value = path.to_string_lossy();
        if value.len() < 3
            || value.as_bytes()[1] != b':'
            || value.as_bytes()[2] != b'\\'
            || value.starts_with(r"\\")
            || value.starts_with(r"\\?\")
            || path
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(InstallError::InvalidMetadata(format!(
                "{label} must be an absolute local drive path"
            )));
        }
        reject_reparse_ancestors(path, label)?;
    }
    let staging_drive = spec.staging_root.to_string_lossy().as_bytes()[0].to_ascii_lowercase();
    let managed_drive = spec.managed_directory.to_string_lossy().as_bytes()[0].to_ascii_lowercase();
    if staging_drive != managed_drive {
        return Err(InstallError::InvalidMetadata(
            "staging_root and managed_directory must be on the same drive".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn reject_reparse_ancestors(path: &Path, label: &str) -> Result<(), InstallError> {
    let mut current = Some(path);
    while let Some(candidate) = current {
        match fs::symlink_metadata(candidate) {
            Ok(metadata) => {
                if metadata_is_reparse(&metadata) {
                    return Err(InstallError::UnsafeArchive(format!(
                        "{label} contains a reparse-point component"
                    )));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(InstallError::Filesystem(format!(
                    "read {label} path component: {error}"
                )));
            }
        }
        current = candidate.parent();
    }
    Ok(())
}

#[cfg(windows)]
fn install_windows_staged(
    spec: &PhpArtifactInstallSpec,
    stage: &Path,
) -> Result<InstalledPhpArtifact, InstallError> {
    append_install_journal(spec, "downloading", None)?;
    let archive_path = stage.join(&spec.file_name);
    let mut archive_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&archive_path)
        .map_err(|error| {
            InstallError::Filesystem(format!("create archive staging file: {error}"))
        })?;
    let client = reqwest::blocking::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| InstallError::Download(format!("create HTTPS client: {error}")))?;
    let mut response = client
        .get(&spec.url)
        .send()
        .map_err(|error| InstallError::Download(error.to_string()))?;
    if !response.status().is_success() {
        return Err(InstallError::Download(format!(
            "HTTP {}",
            response.status()
        )));
    }
    if let Some(content_length) = response.content_length() {
        if content_length != spec.expected_size_bytes {
            return Err(InstallError::SizeMismatch);
        }
    }
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = response
            .read(&mut buffer)
            .map_err(|error| InstallError::Download(error.to_string()))?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        if total > spec.expected_size_bytes {
            return Err(InstallError::SizeMismatch);
        }
        hasher.update(&buffer[..read]);
        archive_file.write_all(&buffer[..read]).map_err(|error| {
            InstallError::Filesystem(format!("write archive staging file: {error}"))
        })?;
    }
    archive_file.sync_all().map_err(|error| {
        InstallError::Filesystem(format!("flush archive staging file: {error}"))
    })?;
    if total != spec.expected_size_bytes {
        return Err(InstallError::SizeMismatch);
    }
    let actual_sha = format!("{:x}", hasher.finalize());
    if actual_sha != spec.expected_sha256.to_ascii_lowercase() {
        return Err(InstallError::HashMismatch);
    }
    append_install_journal(spec, "verifying", None)?;
    drop(archive_file);

    let extracted = stage.join("extracted");
    fs::create_dir(&extracted)
        .map_err(|error| InstallError::Filesystem(format!("create extraction root: {error}")))?;
    extract_zip_safely(&archive_path, &extracted)?;
    let entrypoint = extracted.join(&spec.entrypoint);
    if !entrypoint.is_file() || is_reparse(&entrypoint)? {
        return Err(InstallError::EntrypointMissing);
    }
    let managed_parent = spec.managed_directory.parent().ok_or_else(|| {
        InstallError::InvalidMetadata("managed_directory has no parent".to_owned())
    })?;
    fs::create_dir_all(managed_parent)
        .map_err(|error| InstallError::Filesystem(format!("create managed parent: {error}")))?;
    let managed_parent_resolved = fs::canonicalize(managed_parent)
        .map_err(|error| InstallError::Filesystem(format!("resolve managed parent: {error}")))?;
    if is_reparse(&managed_parent_resolved)? {
        return Err(InstallError::UnsafeArchive(
            "managed parent must not be a reparse point".to_owned(),
        ));
    }
    if spec.managed_directory.exists() {
        return Err(InstallError::ManagedVersionExists);
    }
    fs::rename(&extracted, &spec.managed_directory).map_err(|error| {
        InstallError::Filesystem(format!("atomically publish managed version: {error}"))
    })?;
    let _ = fs::remove_file(&archive_path);
    Ok(InstalledPhpArtifact {
        managed_directory: spec.managed_directory.clone(),
        downloaded_size_bytes: total,
        sha256: actual_sha,
    })
}

#[cfg(windows)]
fn extract_zip_safely(archive_path: &Path, destination: &Path) -> Result<(), InstallError> {
    let file = File::open(archive_path)
        .map_err(|error| InstallError::Filesystem(format!("open staged ZIP: {error}")))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| InstallError::UnsafeArchive(format!("read ZIP: {error}")))?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(InstallError::UnsafeArchive(
            "too many archive entries".to_owned(),
        ));
    }
    let mut total_unpacked = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| InstallError::UnsafeArchive(format!("read ZIP entry: {error}")))?;
        let name = entry.name().to_owned();
        let relative = safe_zip_path(&name)?;
        let target = destination.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|error| {
                InstallError::Filesystem(format!("create ZIP directory: {error}"))
            })?;
            continue;
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(InstallError::UnsafeArchive(
                "symbolic links are not accepted".to_owned(),
            ));
        }
        let expected_entry_size = entry.size();
        total_unpacked = total_unpacked.saturating_add(expected_entry_size);
        if total_unpacked > MAX_UNPACKED_BYTES {
            return Err(InstallError::UnsafeArchive(
                "unpacked size budget exceeded".to_owned(),
            ));
        }
        let parent = target
            .parent()
            .ok_or_else(|| InstallError::UnsafeArchive("ZIP entry has no parent".to_owned()))?;
        fs::create_dir_all(parent)
            .map_err(|error| InstallError::Filesystem(format!("create ZIP parent: {error}")))?;
        if target.exists() {
            return Err(InstallError::UnsafeArchive(
                "duplicate ZIP entry".to_owned(),
            ));
        }
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|error| InstallError::Filesystem(format!("create extracted file: {error}")))?;
        let copied = std::io::copy(&mut entry, &mut output)
            .map_err(|error| InstallError::Filesystem(format!("extract ZIP entry: {error}")))?;
        if copied != expected_entry_size {
            return Err(InstallError::UnsafeArchive(
                "ZIP entry size does not match its payload".to_owned(),
            ));
        }
    }
    Ok(())
}

fn safe_zip_path(name: &str) -> Result<PathBuf, InstallError> {
    if name.is_empty() || name.contains('\\') || name.contains('\0') || name.starts_with('/') {
        return Err(InstallError::UnsafeArchive(format!("unsafe path {name:?}")));
    }
    let mut output = PathBuf::new();
    for component in Path::new(name).components() {
        match component {
            Component::Normal(value) => output.push(value),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(InstallError::UnsafeArchive(format!(
                    "path traversal {name:?}"
                )))
            }
        }
    }
    if output.as_os_str().is_empty() {
        return Err(InstallError::UnsafeArchive("empty ZIP path".to_owned()));
    }
    Ok(output)
}

#[cfg(windows)]
fn is_reparse(path: &Path) -> Result<bool, InstallError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| InstallError::Filesystem(format!("read file metadata: {error}")))?;
    Ok(metadata_is_reparse(&metadata))
}

#[cfg(windows)]
fn metadata_is_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> PhpArtifactInstallSpec {
        PhpArtifactInstallSpec {
            url: "https://downloads.php.net/~windows/releases/php-8.5.11-nts-Win32-vs17-x64.zip"
                .to_owned(),
            file_name: "php-8.5.11-nts-Win32-vs17-x64.zip".to_owned(),
            expected_size_bytes: 36_048_897,
            expected_sha256: "0ea96e0d2b9b737a6036f05cf4e95c49313faa6d0f27bd97edb2742503f0c043"
                .to_owned(),
            staging_root: PathBuf::from(r"C:\Users\demo\AppData\Local\cxszn-menv\staging"),
            managed_directory: PathBuf::from(
                r"C:\Users\demo\AppData\Local\cxszn-menv\versions\php\8.5.11\x86_64",
            ),
            entrypoint: "php.exe".to_owned(),
            journal_path: None,
        }
    }

    #[test]
    fn accepts_only_fixed_php_release_sources() {
        assert!(validate_install_spec(&spec()).is_ok());
        let mut hostile = spec();
        hostile.url = "https://example.com/payload.zip".to_owned();
        assert_eq!(
            validate_install_spec(&hostile),
            Err(InstallError::SourceNotAllowed)
        );
        hostile.url = "https://downloads.php.net/~windows/releases/other.zip".to_owned();
        assert!(matches!(
            validate_install_spec(&hostile),
            Err(InstallError::InvalidMetadata(_))
        ));
        hostile.url = "https://downloads.php.net/~windows/releases/../payload.zip".to_owned();
        assert_eq!(
            validate_install_spec(&hostile),
            Err(InstallError::SourceNotAllowed)
        );
    }

    #[test]
    fn rejects_zip_slip_absolute_and_device_members() {
        for path in [
            "../payload.exe",
            "/absolute.exe",
            "C:\\payload.exe",
            "\\\\server\\payload.exe",
            "dir\\payload.exe",
        ] {
            assert!(
                safe_zip_path(path).is_err(),
                "path should be rejected: {path}"
            );
        }
        assert_eq!(
            safe_zip_path("php.exe").expect("safe entry"),
            PathBuf::from("php.exe")
        );
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "downloads the fixed official PHP artifact; run with MENV_RUN_LIVE_WINDOWS_INSTALL=1"]
    fn live_downloads_verifies_and_atomically_installs_php_artifact() {
        if std::env::var_os("MENV_RUN_LIVE_WINDOWS_INSTALL").as_deref() != Some("1".as_ref()) {
            return;
        }
        let root = PathBuf::from(std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA"))
            .join("cxszn-menv")
            .join("acceptance-install-test");
        if root.exists() {
            fs::remove_dir_all(&root).expect("remove prior acceptance directory");
        }
        let spec = PhpArtifactInstallSpec {
            url: "https://downloads.php.net/~windows/releases/php-8.5.11-nts-Win32-vs17-x64.zip"
                .to_owned(),
            file_name: "php-8.5.11-nts-Win32-vs17-x64.zip".to_owned(),
            expected_size_bytes: 36_048_897,
            expected_sha256: "0ea96e0d2b9b737a6036f05cf4e95c49313faa6d0f27bd97edb2742503f0c043"
                .to_owned(),
            staging_root: root.join("staging"),
            managed_directory: root.join("versions").join("php-8.5.11-x86_64"),
            entrypoint: "php.exe".to_owned(),
            journal_path: Some(root.join("operations").join("php-8.5.11.jsonl")),
        };
        let result = install_php_artifact(&spec).expect("official PHP install");
        assert!(result.managed_directory.join("php.exe").is_file());
        let output = std::process::Command::new(result.managed_directory.join("php.exe"))
            .args(["-n", "-v"])
            .output()
            .expect("run installed PHP health probe");
        assert!(output.status.success());
        println!(
            "PHP install acceptance: size={} sha256={} managed={}",
            result.downloaded_size_bytes,
            result.sha256,
            result.managed_directory.display()
        );
        fs::remove_dir_all(&root).expect("remove acceptance directory");
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "downloads every fixed PHP catalog artifact; run with MENV_RUN_LIVE_WINDOWS_INSTALL=1"]
    fn live_downloads_every_supported_catalog_artifact() {
        if std::env::var_os("MENV_RUN_LIVE_WINDOWS_INSTALL").as_deref() != Some("1".as_ref()) {
            return;
        }
        let root = std::env::temp_dir().join(format!(
            "cxszn-menv-catalog-install-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let catalog = crate::catalog::embedded_php_catalog().expect("embedded catalog");
        let artifacts = catalog
            .artifacts
            .iter()
            .filter(|artifact| artifact.platform == "windows" && artifact.arch == "x86_64")
            .collect::<Vec<_>>();
        assert!(
            !artifacts.is_empty(),
            "catalog has no Windows x64 artifacts"
        );

        for artifact in artifacts {
            let spec =
                crate::catalog::create_php_install_spec(&catalog, &artifact.artifact_id, &root)
                    .expect("catalog install spec");
            let result = install_php_artifact(&spec).expect("catalog PHP install");
            let executable = result.managed_directory.join("php.exe");
            let output = std::process::Command::new(&executable)
                .args(["-n", "-v"])
                .output()
                .expect("run catalog PHP health probe");
            assert!(
                output.status.success(),
                "health probe failed for {}",
                artifact.version
            );
            assert!(
                String::from_utf8_lossy(&output.stdout).contains(&artifact.version),
                "probe output did not contain {}",
                artifact.version
            );
            let journal = spec
                .journal_path
                .as_ref()
                .expect("catalog install journal path");
            let journal_text = fs::read_to_string(journal).expect("read catalog install journal");
            assert!(journal_text.contains("\"stage\":\"published\""));
            println!(
                "PHP catalog acceptance: artifact={} size={} sha256={} managed={}",
                artifact.artifact_id,
                result.downloaded_size_bytes,
                result.sha256,
                result.managed_directory.display()
            );
        }
        fs::remove_dir_all(&root).expect("remove catalog acceptance directory");
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "downloads a fixed PHP artifact; run with MENV_RUN_LIVE_WINDOWS_INSTALL=1"]
    fn live_hash_mismatch_cleans_staging_and_records_failed_journal() {
        if std::env::var_os("MENV_RUN_LIVE_WINDOWS_INSTALL").as_deref() != Some("1".as_ref()) {
            return;
        }
        let root = std::env::temp_dir().join(format!(
            "cxszn-menv-catalog-hash-failure-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let catalog = crate::catalog::embedded_php_catalog().expect("embedded catalog");
        let mut spec =
            crate::catalog::create_php_install_spec(&catalog, "php-8.5.11-nts-x64", &root)
                .expect("catalog install spec");
        spec.expected_sha256 = "0".repeat(64);

        assert_eq!(
            install_php_artifact(&spec).expect_err("hash mismatch must fail"),
            InstallError::HashMismatch
        );
        assert!(!spec.managed_directory.exists());
        let staging_entries = fs::read_dir(&spec.staging_root)
            .map(|entries| entries.flatten().collect::<Vec<_>>())
            .unwrap_or_default();
        assert!(
            staging_entries.is_empty(),
            "staging must not retain failed operation directories"
        );
        let journal = spec.journal_path.as_ref().expect("journal path");
        let journal_text = fs::read_to_string(journal).expect("failed journal");
        assert!(journal_text.contains("\"stage\":\"failed\""));
        fs::remove_dir_all(&root).expect("remove hash failure root");
    }
}
