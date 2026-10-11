//! App-owned runtime shims.
//!
//! A shim is represented by a hard link in the fixed user-local `bin`
//! directory.  Hard links keep the selected executable as an absolute,
//! immutable target and avoid generating shell scripts.  Cross-volume targets,
//! symlinks, UNC/device paths, and an unowned existing destination are rejected
//! rather than copied or overwritten.

use crate::path::validate_local_windows_path;
use cxszn_menv_ports::PortError;
use std::fs;
use std::path::{Path, PathBuf};

pub const PHP_SHIM_NAME: &str = "php.exe";
const TARGET_RECORD_NAME: &str = "php.target";
const TARGET_RECORD_VERSION: &str = "1";

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

    /// Validate the target and create an app-owned hard-link shim.  This
    /// method never removes an existing destination unless its target record
    /// proves that MEnv created it.
    pub fn activate_php(&self, target: ShimTarget) -> Result<ShimActivation, PortError> {
        let executable = Self::validate_target(&target)?;
        let target_path = PathBuf::from(&executable);
        let shim_path = self.bin_directory.join(PHP_SHIM_NAME);
        let record_path = self.bin_directory.join(TARGET_RECORD_NAME);

        ensure_regular_local_file(&target_path)?;
        fs::create_dir_all(&self.bin_directory)
            .map_err(|error| PortError::Platform(format!("create app shim directory: {error}")))?;

        if shim_path.exists() || record_path.exists() {
            let previous = read_target_record(&record_path)?;
            if previous.as_deref() != Some(executable.as_str()) {
                return Err(PortError::Platform(
                    "existing php shim is not owned by this MEnv instance".to_owned(),
                ));
            }
            remove_owned_shim(&shim_path, &record_path)?;
        }

        let temp_shim = self
            .bin_directory
            .join(format!(".{PHP_SHIM_NAME}.{}.tmp", std::process::id()));
        let _ = fs::remove_file(&temp_shim);
        fs::hard_link(&target_path, &temp_shim).map_err(|error| {
            PortError::Platform(format!(
                "create PHP shim hard link (target and bin must share a volume): {error}"
            ))
        })?;
        fs::rename(&temp_shim, &shim_path).map_err(|error| {
            let _ = fs::remove_file(&temp_shim);
            PortError::Platform(format!("publish PHP shim atomically: {error}"))
        })?;

        if let Err(error) = write_target_record(&record_path, &executable) {
            let _ = fs::remove_file(&shim_path);
            return Err(error);
        }
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

fn write_target_record(path: &Path, target: &str) -> Result<(), PortError> {
    let temp = path.with_extension(format!("target.{}.tmp", std::process::id()));
    fs::write(&temp, format!("{TARGET_RECORD_VERSION}\n{target}\n"))
        .map_err(|error| PortError::Platform(format!("write PHP shim target record: {error}")))?;
    fs::rename(&temp, path).map_err(|error| {
        let _ = fs::remove_file(&temp);
        PortError::Platform(format!("publish PHP shim target record: {error}"))
    })
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
}
