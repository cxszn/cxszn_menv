//! Filesystem adapter shared by the GUI and native CLI backup commands.
pub(crate) use cxszn_menv_core::application::PhpIniBackupInfo;
use cxszn_menv_core::application::{ApplicationError, ChangePlan, ConfirmationToken, Orchestrator};
use cxszn_menv_core::ports::{
    EnvironmentStore, IniBackupArtifact, PhpIniBackupStore, PortError, UserPathStore,
};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

const MAX_INI_BYTES: u64 = cxszn_menv_core::application::MAX_INI_BYTES as u64;

struct WindowsPhpIniBackupStore {
    data_root: PathBuf,
}

pub(crate) fn bind_backup_source<S: EnvironmentStore, P: UserPathStore>(
    service: &Orchestrator<S, P>,
    plan: &mut ChangePlan,
    data_root: &Path,
) -> Result<(), ApplicationError> {
    service.bind_php_ini_backup_source(
        plan,
        &WindowsPhpIniBackupStore {
            data_root: data_root.to_owned(),
        },
    )
}

/// Callers serialize native writes with the user-scoped operation mutex.
pub(crate) fn apply_confirmed_backup<S: EnvironmentStore, P: UserPathStore>(
    service: &Orchestrator<S, P>,
    plan: &ChangePlan,
    confirmation: Option<&ConfirmationToken>,
    data_root: &Path,
) -> Result<PhpIniBackupInfo, ApplicationError> {
    service.apply_php_ini_backup(
        plan,
        confirmation,
        &WindowsPhpIniBackupStore {
            data_root: data_root.to_owned(),
        },
    )
}

impl WindowsPhpIniBackupStore {
    fn backup_path(&self, backup_id: &str) -> Result<PathBuf, PortError> {
        uuid::Uuid::parse_str(backup_id)
            .map_err(|_| PortError::InvalidPath("backup id must be a UUID".into()))?;
        let root = cxszn_menv_core::windows::validate_local_windows_path(&self.data_root)?;
        let path = PathBuf::from(root)
            .join("backups")
            .join("php-ini")
            .join(format!("{backup_id}.php.ini.bak"));
        ensure_no_reparse(&path, true)?;
        Ok(path)
    }
}

impl PhpIniBackupStore for WindowsPhpIniBackupStore {
    fn read_source(&self, path: &Path) -> Result<Vec<u8>, PortError> {
        let path = cxszn_menv_core::windows::validate_local_windows_path(path)?;
        read_ini(Path::new(&path))
    }

    fn create_backup(
        &self,
        backup_id: &str,
        contents: &[u8],
    ) -> Result<IniBackupArtifact, PortError> {
        let path = self.backup_path(backup_id)?;
        let parent = path.parent().expect("validated backup path has a parent");
        std::fs::create_dir_all(parent)
            .map_err(|e| PortError::Platform(format!("create php.ini backup directory: {e}")))?;
        ensure_no_reparse(parent, false)?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| PortError::Platform(format!("create php.ini backup: {e}")))?;
        let written = file.write_all(contents).and_then(|_| file.sync_all());
        drop(file);
        if written.is_err() {
            return Err(PortError::BackupRecoveryRequired {
                backup_path: path.to_string_lossy().into_owned(),
            });
        }
        self.get_backup(backup_id)
            .map_err(|_| PortError::BackupRecoveryRequired {
                backup_path: path.to_string_lossy().into_owned(),
            })
    }

    fn get_backup(&self, backup_id: &str) -> Result<IniBackupArtifact, PortError> {
        let path = self.backup_path(backup_id)?;
        let sha256 = ini_hash(&read_ini(&path)?);
        Ok(IniBackupArtifact { path, sha256 })
    }
}

fn read_ini(path: &Path) -> Result<Vec<u8>, PortError> {
    ensure_no_reparse(path, false)?;
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| PortError::Platform(format!("read php.ini backup source: {error}")))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(PortError::InvalidPath(
            "php.ini backup source must be a regular local file".to_owned(),
        ));
    }
    if metadata.len() > MAX_INI_BYTES {
        return Err(PortError::InvalidPath(
            "php.ini exceeds the 4 MiB backup limit".to_owned(),
        ));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|error| PortError::Platform(format!("open php.ini: {error}")))?
        .take(MAX_INI_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| PortError::Platform(format!("read php.ini: {error}")))?;
    if bytes.len() as u64 > MAX_INI_BYTES {
        return Err(PortError::InvalidPath(
            "php.ini exceeds the 4 MiB backup limit".to_owned(),
        ));
    }
    Ok(bytes)
}

fn ini_hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn ensure_no_reparse(path: &Path, allow_missing: bool) -> Result<(), PortError> {
    for component in path.ancestors() {
        match std::fs::symlink_metadata(component) {
            Ok(metadata)
                if metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0 =>
            {
                return Err(PortError::InvalidPath(
                    "backup paths cannot contain symlinks or reparse points".to_owned(),
                ));
            }
            Ok(_) => {}
            Err(error) if allow_missing && error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(PortError::Platform(format!("check backup path: {error}"))),
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "php_ini_backup_tests.rs"]
mod tests;
