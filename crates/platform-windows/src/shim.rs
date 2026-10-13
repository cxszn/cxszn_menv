//! App-owned runtime shims.
//!
//! A same-volume shim uses a hard link. When a manually installed runtime is
//! on another volume, the fixed user-local `bin` directory receives a copy of
//! the trusted MEnv executable as a native launcher. The launcher reads the
//! app-owned target record and forwards argv to PHP; we never copy PHP itself,
//! which would break DLL discovery.

use crate::path::validate_local_windows_path;
use cxszn_menv_ports::PortError;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const PHP_SHIM_NAME: &str = "php.exe";
const TARGET_RECORD_NAME: &str = "php.target";
const TARGET_RECORD_VERSION: &str = "1";
const MAX_EXECUTABLE_COMPARE_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShimTarget {
    pub runtime_id: String,
    pub executable_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShimActivation {
    pub shim_path: PathBuf,
    pub target: ShimTarget,
}

pub struct ShimManager {
    bin_directory: PathBuf,
}

impl ShimManager {
    pub fn new(bin_directory: impl Into<PathBuf>) -> Result<Self, PortError> {
        let bin_directory = bin_directory.into();
        let _ = validate_local_windows_path(&bin_directory)?;
        Ok(Self { bin_directory })
    }

    pub fn default_for_current_user() -> Result<Self, PortError> {
        let local_app_data = std::env::var_os("LOCALAPPDATA")
            .ok_or_else(|| PortError::Platform("LOCALAPPDATA is not available".to_owned()))?;
        Self::new(PathBuf::from(local_app_data).join("cxszn-menv").join("bin"))
    }

    pub fn bin_directory(&self) -> &Path {
        &self.bin_directory
    }

    pub fn validate_target(target: &ShimTarget) -> Result<String, PortError> {
        if target.runtime_id.trim().is_empty() || target.runtime_id.len() > 256 {
            return Err(PortError::InvalidPath(
                "shim runtime id must be non-empty and at most 256 bytes".to_owned(),
            ));
        }
        let executable = validate_local_windows_path(&target.executable_path)?;
        if !executable
            .rsplit_once('\\')
            .map(|(_, name)| name.eq_ignore_ascii_case(PHP_SHIM_NAME))
            .unwrap_or(false)
        {
            return Err(PortError::InvalidPath(
                "the PHP shim target must be a php.exe file".to_owned(),
            ));
        }
        Ok(executable)
    }

    /// Validate the target and create an app-owned shim. This method never
    /// removes an existing destination unless its target record proves that
    /// MEnv created it.
    pub fn activate_php(&self, target: ShimTarget) -> Result<ShimActivation, PortError> {
        let executable = Self::validate_target(&target)?;
        let target_path = PathBuf::from(&executable);
        let shim_path = self.bin_directory.join(PHP_SHIM_NAME);
        let record_path = self.bin_directory.join(TARGET_RECORD_NAME);

        if is_same_local_path(&target_path, &shim_path) {
            return Err(PortError::InvalidPath(
                "the app-owned PHP shim cannot be its own runtime target".to_owned(),
            ));
        }
        ensure_regular_local_file(&target_path)?;
        let host = std::env::current_exe().map_err(|error| {
            PortError::Platform(format!("resolve MEnv host executable: {error}"))
        })?;
        if executable_content_matches(&target_path, &host)? {
            return Err(PortError::InvalidPath(
                "a PHP runtime cannot alias the MEnv host executable".to_owned(),
            ));
        }
        fs::create_dir_all(&self.bin_directory)
            .map_err(|error| PortError::Platform(format!("create app shim directory: {error}")))?;

        // Read and validate the existing lease before creating any new files.
        // A valid record proves that both the old shim and record are owned by
        // MEnv, so they may be replaced.  We do not remove either file yet:
        // activation must leave the last-known-good target usable if staging
        // or publication fails.
        let previous_record = read_target_record(&record_path)?;
        let previous_shim_exists = shim_path.exists();
        if previous_shim_exists && executable_content_matches(&target_path, &shim_path)? {
            if let Some(previous_target) = &previous_record {
                // Same-volume PHP is itself hard-linked to the shim. A copied
                // launcher, however, differs from the recorded real PHP.
                if is_same_local_path(Path::new(previous_target), &shim_path)
                    || !executable_content_matches(&shim_path, Path::new(previous_target))?
                {
                    return Err(PortError::InvalidPath(
                        "a PHP runtime cannot alias the app-owned launcher".to_owned(),
                    ));
                }
            }
        }
        if previous_record.is_none() && previous_shim_exists {
            // A shim without its app-owned record cannot be attributed safely
            // and must never be overwritten.
            return Err(PortError::Platform(
                "existing php shim is not owned by this MEnv instance".to_owned(),
            ));
        }

        let operation = next_operation_suffix();
        let temp_shim = self
            .bin_directory
            .join(format!(".{PHP_SHIM_NAME}.{operation}.tmp"));
        let temp_record = self
            .bin_directory
            .join(format!(".{TARGET_RECORD_NAME}.{operation}.tmp"));
        let backup_shim = self
            .bin_directory
            .join(format!(".{PHP_SHIM_NAME}.{operation}.bak"));
        let backup_record = self
            .bin_directory
            .join(format!(".{TARGET_RECORD_NAME}.{operation}.bak"));

        for path in [&temp_shim, &temp_record, &backup_shim, &backup_record] {
            if path.exists() {
                return Err(PortError::Platform(format!(
                    "MEnv shim operation path already exists: {}",
                    path.display()
                )));
            }
        }

        // Stage both files while the previous activation is still intact.
        // This is deliberately done before moving any old file so failures in
        // hard-link creation, launcher lookup, or target-record writing are
        // harmless to the active PHP version.
        let stage_result = (|| {
            if same_local_volume(&target_path, &self.bin_directory) {
                match fs::hard_link(&target_path, &temp_shim) {
                    Ok(()) => {}
                    // A drive-letter comparison cannot identify every Windows
                    // volume boundary (for example mounted volumes). Preserve
                    // the hard-link fast path, but fall back to the native
                    // launcher when Windows reports ERROR_NOT_SAME_DEVICE.
                    Err(error) if is_not_same_device(&error) => {
                        create_launcher_shim(&temp_shim)?;
                    }
                    Err(error) => {
                        return Err(PortError::Platform(format!(
                            "create PHP shim hard link: {error}"
                        )));
                    }
                }
            } else {
                create_launcher_shim(&temp_shim)?;
            }
            write_target_record_file(&temp_record, &executable)
        })();
        if let Err(error) = stage_result {
            let _ = fs::remove_file(&temp_shim);
            let _ = fs::remove_file(&temp_record);
            return Err(error);
        }

        // Windows' std::fs::rename does not replace an existing file. Move
        // the owned pair to private backups, publish both staged files, and
        // restore the backups on any error.  This keeps a failed version
        // switch from disabling the previously active PHP version.
        let mut old_shim_backed_up = false;
        let mut old_record_backed_up = false;
        let mut new_shim_published = false;
        let mut new_record_published = false;
        let publish_result = (|| {
            if previous_shim_exists {
                fs::rename(&shim_path, &backup_shim).map_err(|error| {
                    PortError::Platform(format!("backup existing PHP shim: {error}"))
                })?;
                old_shim_backed_up = true;
            }
            if previous_record.is_some() {
                fs::rename(&record_path, &backup_record).map_err(|error| {
                    PortError::Platform(format!("backup existing PHP shim record: {error}"))
                })?;
                old_record_backed_up = true;
            }
            fs::rename(&temp_shim, &shim_path).map_err(|error| {
                PortError::Platform(format!("publish PHP shim atomically: {error}"))
            })?;
            new_shim_published = true;
            fs::rename(&temp_record, &record_path).map_err(|error| {
                PortError::Platform(format!("publish PHP shim record atomically: {error}"))
            })?;
            new_record_published = true;
            Ok::<(), PortError>(())
        })();

        if let Err(error) = publish_result {
            let rollback_error = rollback_shim_activation(
                &shim_path,
                &record_path,
                &temp_shim,
                &temp_record,
                &backup_shim,
                &backup_record,
                old_shim_backed_up,
                old_record_backed_up,
                new_shim_published,
                new_record_published,
            );
            return Err(match rollback_error {
                Some(rollback_error) => PortError::Platform(format!(
                    "{error}; failed to restore previous PHP shim: {rollback_error}"
                )),
                None => error,
            });
        }

        // Backups contain only the previous app-owned shim and record.  A
        // running PHP process may keep one open on Windows, so cleanup is
        // best-effort after the new activation is already safely published.
        let _ = fs::remove_file(&backup_shim);
        let _ = fs::remove_file(&backup_record);
        Ok(ShimActivation { shim_path, target })
    }

    pub fn deactivate_php(&self, expected_target: Option<&Path>) -> Result<(), PortError> {
        let shim_path = self.bin_directory.join(PHP_SHIM_NAME);
        let record_path = self.bin_directory.join(TARGET_RECORD_NAME);
        let recorded = read_target_record(&record_path)?;
        if let Some(expected) = expected_target {
            let expected = validate_local_windows_path(expected)?;
            if recorded.as_deref() != Some(expected.as_str()) {
                return Err(PortError::Platform(
                    "PHP shim target does not match the requested deactivation".to_owned(),
                ));
            }
        }
        if recorded.is_some() {
            remove_owned_shim(&shim_path, &record_path)?;
        }
        Ok(())
    }
}

/// Windows reports ERROR_NOT_SAME_DEVICE (17) when a hard-link source and
/// destination resolve to different volumes.  The raw code is used instead
/// of a Windows-only dependency so the path and shim contract remain testable
/// on non-Windows hosts.
fn is_not_same_device(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(17)
}

/// Resolve aliases without treating legitimate same-volume hard links as
/// self-reference: compare canonical paths, not file identities.
pub(crate) fn is_same_local_path(first: &Path, second: &Path) -> bool {
    let key = |path: &Path| {
        let resolved = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        resolved
            .to_string_lossy()
            .replace('/', "\\")
            .trim_start_matches(r"\\?\")
            .trim_end_matches('\\')
            .to_ascii_lowercase()
    };
    key(first) == key(second)
}

fn executable_content_matches(first: &Path, second: &Path) -> Result<bool, PortError> {
    ensure_regular_local_file(first)?;
    ensure_regular_local_file(second)?;
    let length = |path: &Path| {
        fs::metadata(path)
            .map(|metadata| metadata.len())
            .map_err(|error| PortError::Platform(format!("read launcher image length: {error}")))
    };
    let first_length = length(first)?;
    if first_length != length(second)? {
        return Ok(false);
    }
    if first_length > MAX_EXECUTABLE_COMPARE_BYTES {
        return Err(PortError::InvalidPath(
            "launcher image comparison exceeds its size limit".to_owned(),
        ));
    }
    let hash = |path: &Path| -> Result<_, PortError> {
        let mut file = fs::File::open(path)
            .map_err(|error| PortError::Platform(format!("open launcher image: {error}")))?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 8192];
        let mut total = 0u64;
        loop {
            let count = file
                .read(&mut buffer)
                .map_err(|error| PortError::Platform(format!("read launcher image: {error}")))?;
            if count == 0 {
                break;
            }
            total += count as u64;
            if total > MAX_EXECUTABLE_COMPARE_BYTES {
                return Err(PortError::InvalidPath(
                    "launcher image comparison exceeds its size limit".to_owned(),
                ));
            }
            hasher.update(&buffer[..count]);
        }
        if total != first_length {
            return Err(PortError::InvalidPath(
                "launcher image changed during comparison".to_owned(),
            ));
        }
        Ok(hasher.finalize())
    };
    Ok(hash(first)? == hash(second)?)
}

fn create_launcher_shim(destination: &Path) -> Result<(), PortError> {
    let launcher = trusted_launcher_executable()?;
    fs::copy(&launcher, destination).map_err(|error| {
        PortError::Platform(format!("create cross-volume PHP launcher shim: {error}"))
    })?;
    Ok(())
}

fn next_operation_suffix() -> String {
    static NEXT_OPERATION: AtomicU64 = AtomicU64::new(0);
    format!(
        "{}-{}",
        std::process::id(),
        NEXT_OPERATION.fetch_add(1, Ordering::Relaxed)
    )
}

/// Remove any newly published files and restore the previous app-owned pair.
/// The returned string is only populated when a cleanup/restore operation also
/// fails; callers can then surface both the original publication error and the
/// fact that rollback needs attention.
#[allow(clippy::too_many_arguments)]
fn rollback_shim_activation(
    shim_path: &Path,
    record_path: &Path,
    temp_shim: &Path,
    temp_record: &Path,
    backup_shim: &Path,
    backup_record: &Path,
    old_shim_backed_up: bool,
    old_record_backed_up: bool,
    new_shim_published: bool,
    new_record_published: bool,
) -> Option<String> {
    let mut failures = Vec::new();
    if new_record_published {
        if let Err(error) = fs::remove_file(record_path) {
            failures.push(format!("remove new PHP shim record: {error}"));
        }
    }
    if new_shim_published {
        if let Err(error) = fs::remove_file(shim_path) {
            failures.push(format!("remove new PHP shim: {error}"));
        }
    }
    let _ = fs::remove_file(temp_record);
    let _ = fs::remove_file(temp_shim);
    if old_record_backed_up {
        if let Err(error) = fs::rename(backup_record, record_path) {
            failures.push(format!("restore previous PHP shim record: {error}"));
        }
    }
    if old_shim_backed_up {
        if let Err(error) = fs::rename(backup_shim, shim_path) {
            failures.push(format!("restore previous PHP shim: {error}"));
        }
    }
    if failures.is_empty() {
        None
    } else {
        Some(failures.join("; "))
    }
}

fn same_local_volume(left: &Path, right: &Path) -> bool {
    fn drive(path: &Path) -> Option<u8> {
        let value = path.to_string_lossy();
        let bytes = value.as_bytes();
        if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
            Some(bytes[0].to_ascii_lowercase())
        } else {
            None
        }
    }
    matches!((drive(left), drive(right)), (Some(a), Some(b)) if a == b)
}

/// Resolve the installed MEnv executable that can safely act as a native
/// launcher. Refuse arbitrary test/host executables so a PATH lease cannot
/// accidentally publish an unrelated process as `php.exe`.
fn trusted_launcher_executable() -> Result<PathBuf, PortError> {
    let executable = std::env::current_exe().map_err(|error| {
        PortError::Platform(format!("resolve MEnv launcher executable: {error}"))
    })?;
    let is_menv = executable
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("cxszn-menv.exe"));
    if !is_menv {
        return Err(PortError::Platform(
            "cross-volume PHP activation requires the cxszn-menv.exe launcher".to_owned(),
        ));
    }
    let metadata = fs::symlink_metadata(&executable)
        .map_err(|error| PortError::Platform(format!("read MEnv launcher metadata: {error}")))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(PortError::Platform(
            "MEnv launcher must be a regular non-symlink file".to_owned(),
        ));
    }
    Ok(executable)
}

/// When the installed executable is invoked through the app-owned `php.exe`
/// copy, forward the original argv to the selected PHP executable. Returning
/// `None` means this is the normal desktop process; `Some(exit_code)` means
/// the caller must exit without starting Tauri.
#[cfg(windows)]
pub fn run_php_shim_if_invoked() -> Option<i32> {
    let current = std::env::current_exe().ok()?;
    if !current
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(PHP_SHIM_NAME))
    {
        return None;
    }

    let record = current.parent()?.join(TARGET_RECORD_NAME);
    let target = match read_target_record(&record) {
        Ok(Some(value)) => PathBuf::from(value),
        Ok(None) => {
            eprintln!("MEnv PHP shim target record is missing.");
            return Some(1);
        }
        Err(error) => {
            eprintln!("MEnv PHP shim target record is invalid: {error}");
            return Some(1);
        }
    };
    // This entrypoint runs only inside the MEnv launcher, not inside PHP.
    // Identical contents here also catch hard-link and copied aliases.
    let matches_launcher = match executable_content_matches(&target, &current) {
        Ok(matches) => matches,
        Err(error) => {
            eprintln!("MEnv PHP shim target could not be validated: {error}");
            return Some(1);
        }
    };
    if is_same_local_path(&target, &current) || matches_launcher {
        eprintln!("MEnv PHP shim target cannot point back to the launcher.");
        return Some(1);
    }
    if let Err(error) = ensure_regular_local_file(&target) {
        eprintln!("MEnv PHP shim target is unavailable: {error}");
        return Some(1);
    }

    let mut command = std::process::Command::new(&target);
    command.args(std::env::args_os().skip(1));
    match command.status() {
        Ok(status) => Some(status.code().unwrap_or(1)),
        Err(error) => {
            eprintln!("MEnv could not start PHP: {error}");
            Some(1)
        }
    }
}

#[cfg(not(windows))]
pub fn run_php_shim_if_invoked() -> Option<i32> {
    None
}

fn ensure_regular_local_file(path: &Path) -> Result<(), PortError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| PortError::Platform(format!("read PHP executable metadata: {error}")))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(PortError::InvalidPath(
            "PHP shim target must be a regular non-symlink file".to_owned(),
        ));
    }
    Ok(())
}

fn read_target_record(path: &Path) -> Result<Option<String>, PortError> {
    match fs::read_to_string(path) {
        Ok(value) => {
            let mut lines = value.lines();
            if lines.next() != Some(TARGET_RECORD_VERSION) {
                return Err(PortError::Platform(
                    "PHP shim target record has an unsupported version".to_owned(),
                ));
            }
            let target = lines.next().unwrap_or_default().trim();
            if target.is_empty() {
                return Err(PortError::Platform(
                    "PHP shim target record is empty".to_owned(),
                ));
            }
            let validated = validate_local_windows_path(Path::new(target))?;
            Ok(Some(validated))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(PortError::Platform(format!(
            "read PHP shim target record: {error}"
        ))),
    }
}

fn write_target_record_file(path: &Path, target: &str) -> Result<(), PortError> {
    fs::write(path, format!("{TARGET_RECORD_VERSION}\n{target}\n"))
        .map_err(|error| PortError::Platform(format!("write PHP shim target record: {error}")))
}

fn remove_owned_shim(shim_path: &Path, record_path: &Path) -> Result<(), PortError> {
    if shim_path.exists() {
        fs::remove_file(shim_path)
            .map_err(|error| PortError::Platform(format!("remove owned PHP shim: {error}")))?;
    }
    if record_path.exists() {
        fs::remove_file(record_path).map_err(|error| {
            PortError::Platform(format!("remove owned PHP shim record: {error}"))
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_php_executables_as_targets() {
        let target = ShimTarget {
            runtime_id: "php-8.3-x64".to_owned(),
            executable_path: PathBuf::from(r"C:\php\php.exe"),
        };
        assert_eq!(
            ShimManager::validate_target(&target).expect("valid target"),
            r"C:\php\php.exe"
        );
        let invalid = ShimTarget {
            executable_path: PathBuf::from(r"C:\php\php.cmd"),
            ..target
        };
        assert!(ShimManager::validate_target(&invalid).is_err());
    }

    #[test]
    fn rejects_untrusted_target_paths() {
        for path in [
            r"C:\php\..\php.exe",
            r"\\server\share\php.exe",
            r"relative\php.exe",
            r"C:\php\php.ps1",
        ] {
            let target = ShimTarget {
                runtime_id: "php".to_owned(),
                executable_path: PathBuf::from(path),
            };
            assert!(ShimManager::validate_target(&target).is_err(), "{path}");
        }
    }

    #[test]
    fn cross_volume_targets_use_the_native_launcher_path() {
        assert!(!same_local_volume(
            Path::new(r"R:\App\Env\php\php8.5.11\php.exe"),
            Path::new(r"C:\Users\demo\AppData\Local\cxszn-menv\bin")
        ));
        assert!(same_local_volume(
            Path::new(r"C:\php\php.exe"),
            Path::new(r"C:\Users\demo\AppData\Local\cxszn-menv\bin")
        ));
        assert!(!same_local_volume(
            Path::new(r"relative\php.exe"),
            Path::new(r"C:\Users\demo\AppData\Local\cxszn-menv\bin")
        ));
    }

    #[test]
    fn only_not_same_device_hard_link_errors_trigger_launcher_fallback() {
        assert!(is_not_same_device(&std::io::Error::from_raw_os_error(17)));
        assert!(!is_not_same_device(&std::io::Error::from_raw_os_error(5)));
        assert!(!is_not_same_device(&std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "permission denied",
        )));
    }

    #[test]
    fn rollback_restores_old_shim_when_record_backup_did_not_complete() {
        let root = std::env::temp_dir().join(format!(
            "cxszn-menv-shim-rollback-{}",
            next_operation_suffix()
        ));
        fs::create_dir_all(&root).expect("create rollback fixture");
        let shim = root.join(PHP_SHIM_NAME);
        let record = root.join(TARGET_RECORD_NAME);
        let temp_shim = root.join(".php.exe.next.tmp");
        let temp_record = root.join(".php.target.next.tmp");
        let backup_shim = root.join(".php.exe.next.bak");
        let backup_record = root.join(".php.target.next.bak");

        // Simulate the exact failure window where the old shim was backed up
        // but moving the old record failed: the old record is still at its
        // published path, while the new staged files must be discarded.
        fs::write(&shim, b"old shim").expect("write old shim backup");
        fs::rename(&shim, &backup_shim).expect("move old shim to backup");
        fs::write(&record, b"old record").expect("keep old record published");
        fs::write(&temp_shim, b"new shim").expect("write staged shim");
        fs::write(&temp_record, b"new record").expect("write staged record");

        let rollback_error = rollback_shim_activation(
            &shim,
            &record,
            &temp_shim,
            &temp_record,
            &backup_shim,
            &backup_record,
            true,
            false,
            false,
            false,
        );
        assert!(
            rollback_error.is_none(),
            "rollback failed: {rollback_error:?}"
        );
        assert_eq!(fs::read(&shim).expect("restored shim"), b"old shim");
        assert_eq!(fs::read(&record).expect("preserved record"), b"old record");
        assert!(!temp_shim.exists());
        assert!(!temp_record.exists());
        assert!(!backup_shim.exists());
        assert!(!backup_record.exists());
        fs::remove_dir_all(root).expect("remove rollback fixture");
    }

    #[cfg(windows)]
    #[test]
    fn rejects_alias_of_the_host_on_first_activation_before_creating_bin() {
        let host = std::env::current_exe().expect("test host");
        assert!(fs::metadata(&host).expect("host metadata").len() <= MAX_EXECUTABLE_COMPARE_BYTES);
        for kind in ["hard-link", "copy"] {
            // Keep the fixture on the host volume so the hard-link case is real.
            let root = host.parent().expect("host directory").join(format!(
                "menv-first-launcher-{kind}-{}",
                next_operation_suffix()
            ));
            let alias_directory = root.join("external");
            fs::create_dir_all(&alias_directory).expect("alias directory");
            let alias = alias_directory.join(PHP_SHIM_NAME);
            if kind == "hard-link" {
                fs::hard_link(&host, &alias).expect("host hard link");
            } else {
                fs::copy(&host, &alias).expect("host copy");
            }
            let bin = root.join("bin");
            let result = ShimManager::new(&bin)
                .expect("manager")
                .activate_php(ShimTarget {
                    runtime_id: "host-alias".to_owned(),
                    executable_path: alias,
                });
            let bin_created = bin.exists();
            fs::remove_dir_all(&root).expect("remove unique fixture");
            assert!(
                matches!(result, Err(PortError::InvalidPath(_))),
                "{kind} host accepted: {result:?}"
            );
            assert!(
                !bin_created,
                "rejected host alias must not create a shim directory"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn rejects_external_hard_link_to_launcher_but_keeps_the_owned_pair() {
        let root =
            std::env::temp_dir().join(format!("menv-shim-alias-{}", next_operation_suffix()));
        let bin = root.join("bin");
        let runtime = root.join("php").join(PHP_SHIM_NAME);
        let alias = root.join("alias").join(PHP_SHIM_NAME);
        fs::create_dir_all(&bin).expect("owned directory");
        fs::create_dir_all(runtime.parent().expect("runtime parent")).expect("runtime directory");
        fs::create_dir_all(alias.parent().expect("alias parent")).expect("alias directory");
        fs::write(&runtime, b"real PHP fixture").expect("runtime");
        let shim = bin.join(PHP_SHIM_NAME);
        let record = bin.join(TARGET_RECORD_NAME);
        fs::write(&shim, b"copied MEnv launcher fixture").expect("owned launcher");
        write_target_record_file(&record, runtime.to_string_lossy().as_ref())
            .expect("owned record");
        fs::hard_link(&shim, &alias).expect("external hard link");
        let before_record = fs::read(&record).expect("record");
        let before_shim = fs::read(&shim).expect("shim");

        let result = ShimManager::new(&bin)
            .expect("manager")
            .activate_php(ShimTarget {
                runtime_id: "launcher-alias".to_owned(),
                executable_path: alias.clone(),
            });
        let after_record = fs::read(&record).expect("record after attempt");
        let after_shim = fs::read(&shim).expect("shim after attempt");
        let after_alias = fs::read(&alias).expect("external alias after attempt");
        fs::remove_dir_all(&root).expect("remove unique fixture");
        assert!(
            matches!(result, Err(PortError::InvalidPath(_))),
            "launcher alias accepted: {result:?}"
        );
        assert_eq!(after_record, before_record);
        assert_eq!(after_shim, before_shim);
        assert_eq!(after_alias, before_shim);
    }

    #[cfg(windows)]
    #[test]
    fn refuses_self_referential_shim_without_changing_the_owned_pair() {
        let root = std::env::temp_dir().join(format!(
            "cxszn-menv-shim-self-{}-{}",
            std::process::id(),
            next_operation_suffix()
        ));
        let target = root.join("runtime").join(PHP_SHIM_NAME);
        fs::create_dir_all(target.parent().expect("runtime parent")).expect("runtime directory");
        fs::write(&target, b"existing PHP fixture").expect("runtime fixture");
        let manager = ShimManager::new(root.join("bin")).expect("manager");
        manager
            .activate_php(ShimTarget {
                runtime_id: "external-php".to_owned(),
                executable_path: target,
            })
            .expect("initial activation");
        let shim = manager.bin_directory().join(PHP_SHIM_NAME);
        let record = manager.bin_directory().join(TARGET_RECORD_NAME);
        let before_record = fs::read(&record).expect("original record");
        let before_shim = fs::read(&shim).expect("original shim");

        let result = manager.activate_php(ShimTarget {
            runtime_id: "self-shim".to_owned(),
            executable_path: manager.bin_directory().join(".").join(PHP_SHIM_NAME),
        });
        // Clean only this uniquely created test fixture, including on RED.
        let after_record = fs::read(&record).expect("record after attempt");
        let after_shim = fs::read(&shim).expect("shim after attempt");
        fs::remove_dir_all(&root).expect("remove unique test fixture");
        assert!(
            matches!(result, Err(PortError::InvalidPath(_))),
            "self target result: {result:?}"
        );
        assert_eq!(after_record, before_record);
        assert_eq!(after_shim, before_shim);
    }

    #[cfg(windows)]
    #[test]
    fn replaces_an_existing_owned_shim_when_switching_versions() {
        let root =
            std::env::temp_dir().join(format!("cxszn-menv-shim-switch-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let first = root.join("php-8.4").join(PHP_SHIM_NAME);
        let second = root.join("php-8.5").join(PHP_SHIM_NAME);
        fs::create_dir_all(first.parent().expect("first parent")).expect("first directory");
        fs::create_dir_all(second.parent().expect("second parent")).expect("second directory");
        fs::write(&first, b"php-8.4").expect("first target");
        fs::write(&second, b"php-8.5").expect("second target");

        let manager = ShimManager::new(root.join("bin")).expect("shim manager");
        manager
            .activate_php(ShimTarget {
                runtime_id: "php-8.4".to_owned(),
                executable_path: first.clone(),
            })
            .expect("first activation");
        manager
            .activate_php(ShimTarget {
                runtime_id: "php-8.5".to_owned(),
                executable_path: second.clone(),
            })
            .expect("switch activation");
        manager
            .activate_php(ShimTarget {
                runtime_id: "php-8.5-again".to_owned(),
                executable_path: second.clone(),
            })
            .expect("legitimate same-volume PHP hard link can be re-enabled");

        let shim = root.join("bin").join(PHP_SHIM_NAME);
        assert_eq!(fs::read(&shim).expect("read switched shim"), b"php-8.5");
        assert_eq!(
            read_target_record(&root.join("bin").join(TARGET_RECORD_NAME))
                .expect("read target record")
                .as_deref(),
            Some(second.to_string_lossy().as_ref())
        );
        manager
            .deactivate_php(Some(second.as_path()))
            .expect("deactivate switched shim");
        assert!(!shim.exists());
        let _ = fs::remove_dir_all(&root);
    }
}
