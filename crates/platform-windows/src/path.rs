//! User-scoped Windows PATH leasing.
//!
//! The adapter only ever opens `HKCU\\Environment`.  It does not know how to
//! write machine PATH, a shell profile, or an arbitrary registry key.  The
//! merge functions are platform-neutral so they can be tested without a live
//! registry; the Windows implementation adds read-after-write and conflict
//! checks around those functions.

use cxszn_menv_ports::{PathLease, PortError, UserPathStore};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::shim::{ShimManager, ShimTarget};

const ENVIRONMENT_KEY: &str = "Environment";
const PATH_VALUE: &str = "PATH";

/// The registry representation is kept so a `REG_EXPAND_SZ` PATH is not
/// silently converted to a different value type during an update.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PathValueType {
    Sz,
    #[default]
    ExpandSz,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserPathSnapshot {
    pub raw: String,
    pub entries: Vec<String>,
    pub hash: String,
    pub value_type: PathValueType,
}

#[derive(Clone, Debug)]
struct LastObserved {
    raw: String,
    hash: String,
}

/// Merge one app-owned shim entry without normalizing or deleting unrelated
/// entries.  The comparison is Windows case-insensitive and slash-insensitive
/// but the original spelling of every unrelated entry is preserved.
pub fn merge_app_owned_entry(
    raw_path: &str,
    app_entry: &Path,
    enabled: bool,
) -> Result<String, PortError> {
    let app_entry = validate_local_windows_path(app_entry)?;
    let app_key = path_key(Path::new(&app_entry));
    let mut entries = split_path(raw_path);
    entries.retain(|entry| path_key(Path::new(entry)) != app_key);
    if enabled {
        entries.push(app_entry);
    }
    Ok(entries.join(";"))
}

pub fn split_path(raw_path: &str) -> Vec<String> {
    raw_path.split(';').map(ToOwned::to_owned).collect()
}

pub fn path_hash(raw_path: &str) -> String {
    let mut hasher = DefaultHasher::new();
    raw_path.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Reject paths that could escape the local user-owned shim directory.  This
/// deliberately accepts Windows drive paths even when unit tests run on a
/// non-Windows host, while rejecting UNC/device namespaces and `..` segments.
pub fn validate_local_windows_path(path: &Path) -> Result<String, PortError> {
    let text = path.to_string_lossy().trim().to_owned();
    if text.is_empty() {
        return Err(PortError::InvalidPath("path is empty".to_owned()));
    }
    let bytes = text.as_bytes();
    let drive_absolute = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/');
    if !drive_absolute
        || text.starts_with("\\\\")
        || text.starts_with("//")
        || text.starts_with("\\\\?\\")
        || text.starts_with("\\\\.\\")
    {
        return Err(PortError::InvalidPath(
            "only an absolute local Windows path is allowed".to_owned(),
        ));
    }
    if text
        .replace('/', "\\")
        .split('\\')
        .any(|component| component == "..")
    {
        return Err(PortError::InvalidPath(
            "path traversal is not allowed".to_owned(),
        ));
    }
    Ok(text.replace('/', "\\"))
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

#[cfg(windows)]
fn read_user_path_snapshot() -> Result<UserPathSnapshot, PortError> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::WIN32_ERROR;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE,
    };

    let key_name = wide(ENVIRONMENT_KEY);
    let value_name = wide(PATH_VALUE);
    let mut key = HKEY::default();
    unsafe {
        let status = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key_name.as_ptr()),
            Some(0),
            KEY_QUERY_VALUE,
            &mut key,
        );
        if status != WIN32_ERROR(0) {
            return Err(PortError::Platform(format!(
                "open HKCU Environment: Win32 error {}",
                status.0
            )));
        }
    }
    let result = unsafe { query_registry_path(key, PCWSTR(value_name.as_ptr())) };
    unsafe {
        let _ = RegCloseKey(key);
    }
    result
}

#[cfg(windows)]
unsafe fn query_registry_path(
    key: windows::Win32::System::Registry::HKEY,
    value_name: windows::core::PCWSTR,
) -> Result<UserPathSnapshot, PortError> {
    use windows::Win32::Foundation::WIN32_ERROR;
    use windows::Win32::System::Registry::{
        RegQueryValueExW, REG_EXPAND_SZ, REG_SZ, REG_VALUE_TYPE,
    };

    let mut value_type = REG_VALUE_TYPE(0);
    let mut byte_len = 0u32;
    let status = RegQueryValueExW(
        key,
        value_name,
        None,
        Some(&mut value_type),
        None,
        Some(&mut byte_len),
    );
    if status == WIN32_ERROR(2) {
        return Ok(UserPathSnapshot {
            raw: String::new(),
            entries: Vec::new(),
            hash: path_hash(""),
            value_type: PathValueType::default(),
        });
    }
    if status != WIN32_ERROR(0) {
        return Err(PortError::Platform(format!(
            "query user PATH size: Win32 error {}",
            status.0
        )));
    }
    let mut bytes = vec![0u8; byte_len as usize];
    let status = RegQueryValueExW(
        key,
        value_name,
        None,
        Some(&mut value_type),
        Some(bytes.as_mut_ptr()),
        Some(&mut byte_len),
    );
    if status != WIN32_ERROR(0) {
        return Err(PortError::Platform(format!(
            "query user PATH: Win32 error {}",
            status.0
        )));
    }
    bytes.truncate(byte_len as usize);
    if !bytes.len().is_multiple_of(2) {
        return Err(PortError::Platform(
            "user PATH registry value has invalid UTF-16 length".to_owned(),
        ));
    }
    let mut wide_values = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks(2) {
        wide_values.push(u16::from_le_bytes([pair[0], pair[1]]));
    }
    while wide_values.last().copied() == Some(0) {
        wide_values.pop();
    }
    let raw = String::from_utf16(&wide_values)
        .map_err(|_| PortError::Platform("user PATH is not valid UTF-16".to_owned()))?;
    let value_type = if value_type == REG_SZ {
        PathValueType::Sz
    } else if value_type == REG_EXPAND_SZ {
        PathValueType::ExpandSz
    } else {
        return Err(PortError::Platform(
            "user PATH must be REG_SZ or REG_EXPAND_SZ".to_owned(),
        ));
    };
    Ok(UserPathSnapshot {
        entries: split_path(&raw),
        hash: path_hash(&raw),
        raw,
        value_type,
    })
}

#[cfg(windows)]
fn write_user_path_snapshot(snapshot: &UserPathSnapshot) -> Result<(), PortError> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::WIN32_ERROR;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE,
        REG_EXPAND_SZ, REG_SZ,
    };

    let key_name = wide(ENVIRONMENT_KEY);
    let value_name = wide(PATH_VALUE);
    let mut value = wide(&snapshot.raw);
    let byte_len = (value.len() * std::mem::size_of::<u16>()) as u32;
    let value_type = match snapshot.value_type {
        PathValueType::Sz => REG_SZ,
        PathValueType::ExpandSz => REG_EXPAND_SZ,
    };
    let mut key = HKEY::default();
    unsafe {
        let status = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(key_name.as_ptr()),
            Some(0),
            KEY_SET_VALUE,
            &mut key,
        );
        if status != WIN32_ERROR(0) {
            return Err(PortError::Platform(format!(
                "open HKCU Environment for write: Win32 error {}",
                status.0
            )));
        }
        let status = RegSetValueExW(
            key,
            PCWSTR(value_name.as_ptr()),
            Some(0),
            value_type,
            Some(std::slice::from_raw_parts(
                value.as_mut_ptr() as *const u8,
                byte_len as usize,
            )),
        );
        let _ = RegCloseKey(key);
        if status == WIN32_ERROR(0) {
            Ok(())
        } else {
            Err(PortError::Platform(format!(
                "write user PATH: Win32 error {}",
                status.0
            )))
        }
    }
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A concrete HKCU-only PATH store.  It records the last observed raw value
/// and refuses to overwrite it when another process changed the value while a
/// plan was pending.
pub struct WindowsUserPathStore {
    shim_directory: PathBuf,
    observed: Mutex<Option<LastObserved>>,
}

impl WindowsUserPathStore {
    pub fn new(shim_directory: impl Into<PathBuf>) -> Result<Self, PortError> {
        let shim_directory = shim_directory.into();
        let _ = validate_local_windows_path(&shim_directory)?;
        Ok(Self {
            shim_directory,
            observed: Mutex::new(None),
        })
    }

    pub fn default_for_current_user() -> Result<Self, PortError> {
        let local_app_data = std::env::var_os("LOCALAPPDATA")
            .ok_or_else(|| PortError::Platform("LOCALAPPDATA is not available".to_owned()))?;
        Self::new(PathBuf::from(local_app_data).join("cxszn-menv").join("bin"))
    }

    pub fn shim_directory(&self) -> &Path {
        &self.shim_directory
    }

    fn validate_lease(&self, lease: &PathLease) -> Result<String, PortError> {
        let requested = validate_local_windows_path(&lease.shim_directory)?;
        if path_key(Path::new(&requested)) != path_key(&self.shim_directory) {
            return Err(PortError::InvalidPath(
                "the PATH lease is not owned by this MEnv shim directory".to_owned(),
            ));
        }
        if lease.lease_id.trim().is_empty() {
            return Err(PortError::InvalidPath("PATH lease id is empty".to_owned()));
        }
        Ok(requested)
    }

    #[cfg(windows)]
    fn read_snapshot(&self) -> Result<UserPathSnapshot, PortError> {
        read_user_path_snapshot()
    }

    #[cfg(not(windows))]
    fn read_snapshot(&self) -> Result<UserPathSnapshot, PortError> {
        Err(PortError::UnsupportedPlatform)
    }

    #[cfg(windows)]
    fn write_snapshot(&self, snapshot: &UserPathSnapshot) -> Result<(), PortError> {
        write_user_path_snapshot(snapshot)
    }

    #[cfg(not(windows))]
    fn write_snapshot(&self, _snapshot: &UserPathSnapshot) -> Result<(), PortError> {
        Err(PortError::UnsupportedPlatform)
    }

    fn remember(&self, snapshot: &UserPathSnapshot) -> Result<(), PortError> {
        let mut observed = self
            .observed
            .lock()
            .map_err(|_| PortError::Platform("PATH lease mutex is poisoned".to_owned()))?;
        *observed = Some(LastObserved {
            raw: snapshot.raw.clone(),
            hash: snapshot.hash.clone(),
        });
        Ok(())
    }

    fn check_expected(&self, snapshot: &UserPathSnapshot) -> Result<(), PortError> {
        let observed = self
            .observed
            .lock()
            .map_err(|_| PortError::Platform("PATH lease mutex is poisoned".to_owned()))?;
        if let Some(last) = &*observed {
            if last.hash != snapshot.hash || last.raw != snapshot.raw {
                return Err(PortError::Platform(
                    "user PATH changed outside MEnv; refusing to overwrite it".to_owned(),
                ));
            }
        }
        Ok(())
    }

    fn mutate(&self, lease: &PathLease, enabled: bool) -> Result<(), PortError> {
        let shim = self.validate_lease(lease)?;
        let shim_manager = ShimManager::new(self.shim_directory.clone())?;
        let target = lease
            .target_executable
            .clone()
            .map(|executable_path| ShimTarget {
                runtime_id: lease.lease_id.clone(),
                executable_path,
            });

        // Publish the shim before PATH points at it. If PATH mutation fails,
        // remove the newly-created shim so a partial activation is not left.
        if enabled {
            if let Some(target) = target.as_ref() {
                shim_manager.activate_php(target.clone())?;
            }
        }

        let path_result = (|| {
            let before = self.read_snapshot()?;
            self.check_expected(&before)?;
            let next_raw = merge_app_owned_entry(&before.raw, Path::new(&shim), enabled)?;
            let next = UserPathSnapshot {
                entries: split_path(&next_raw),
                hash: path_hash(&next_raw),
                raw: next_raw,
                value_type: before.value_type,
            };
            self.write_snapshot(&next)?;
            let after = self.read_snapshot()?;
            if after.raw != next.raw || after.hash != next.hash {
                return Err(PortError::Platform(
                    "user PATH read-back did not match the requested lease".to_owned(),
                ));
            }
            self.remember(&after)?;
            notify_environment_change()
        })();

        if let Err(error) = path_result {
            if enabled && target.is_some() {
                let _ = shim_manager
                    .deactivate_php(target.as_ref().map(|item| item.executable_path.as_path()));
            }
            return Err(error);
        }

        if !enabled {
            if let Some(target) = target.as_ref() {
                shim_manager.deactivate_php(Some(target.executable_path.as_path()))?;
            }
        }
        Ok(())
    }
}

impl UserPathStore for WindowsUserPathStore {
    fn read(&self) -> Result<Vec<String>, PortError> {
        let snapshot = self.read_snapshot()?;
        self.remember(&snapshot)?;
        Ok(snapshot.entries)
    }

    fn apply_lease(&self, lease: &PathLease) -> Result<(), PortError> {
        self.mutate(lease, true)
    }

    fn remove_lease(&self, lease: &PathLease) -> Result<(), PortError> {
        self.mutate(lease, false)
    }
}

#[cfg(windows)]
fn notify_environment_change() -> Result<(), PortError> {
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };

    let environment = wide("Environment");
    unsafe {
        let result = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(environment.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            5_000,
            None,
        );
        if result == LRESULT(0) {
            return Err(PortError::Platform(
                "broadcast WM_SETTINGCHANGE failed or timed out".to_owned(),
            ));
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn notify_environment_change() -> Result<(), PortError> {
    Err(PortError::UnsupportedPlatform)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_lease_preserves_external_entries_and_adds_one_shim() {
        let raw = r"C:\Windows\System32;C:\Tools;";
        let merged = merge_app_owned_entry(
            raw,
            Path::new(r"C:\Users\demo\AppData\Local\cxszn-menv\bin"),
            true,
        )
        .expect("merge succeeds");
        assert!(merged.starts_with(r"C:\Windows\System32;C:\Tools;"));
        assert_eq!(merged.matches("cxszn-menv\\bin").count(), 1);
    }

    #[test]
    fn disabling_removes_only_the_owned_entry() {
        let raw = r"C:\Tools;C:\Users\Demo\AppData\Local\CXszn-menv\bin;C:\Tools";
        let merged = merge_app_owned_entry(
            raw,
            Path::new(r"C:\Users\demo\AppData\Local\cxszn-menv\bin"),
            false,
        )
        .expect("merge succeeds");
        assert_eq!(merged, r"C:\Tools;C:\Tools");
    }

    #[test]
    fn rejects_unc_device_relative_and_traversal_paths() {
        for path in [
            r"relative\bin",
            r"\\server\share\bin",
            r"\\?\C:\Users\demo\bin",
            r"C:\Users\demo\..\bin",
        ] {
            assert!(
                validate_local_windows_path(Path::new(path)).is_err(),
                "{path}"
            );
        }
    }

    #[test]
    fn path_hash_changes_when_external_value_changes() {
        assert_ne!(path_hash(r"C:\one"), path_hash(r"C:\two"));
    }

    /// Opt-in Windows 11 evidence test. It mutates only the current user's
    /// HKCU PATH and restores the exact entry list before returning.
    #[cfg(windows)]
    #[test]
    #[ignore = "mutates HKCU PATH; run only with MENV_RUN_LIVE_WINDOWS_TEST=1"]
    fn live_hkcu_path_and_shim_roundtrip() {
        if std::env::var("MENV_RUN_LIVE_WINDOWS_TEST").ok().as_deref() != Some("1") {
            return;
        }
        let local_app_data = std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA");
        let root = PathBuf::from(local_app_data)
            .join("cxszn-menv")
            .join("acceptance")
            .join(format!("path-{}", std::process::id()));
        let target = root.join("php.exe");
        let bin = root.parent().expect("acceptance root parent").join("bin");
        std::fs::create_dir_all(&root).expect("create acceptance root");
        std::fs::write(&target, b"MEnv acceptance placeholder").expect("create target");

        let store = WindowsUserPathStore::new(&bin).expect("create path store");
        let before = store.read().expect("read initial user PATH");
        let lease = PathLease {
            lease_id: "live-acceptance".to_owned(),
            shim_directory: bin.clone(),
            target_executable: Some(target.clone()),
        };
        let applied = store.apply_lease(&lease);
        if let Err(error) = applied {
            let _ = std::fs::remove_dir_all(&root);
            panic!("apply user PATH lease: {error}");
        }
        let during = store.read().expect("read activated user PATH");
        assert!(during
            .iter()
            .any(|entry| path_key(Path::new(entry)) == path_key(&bin)));
        assert!(bin.join(crate::shim::PHP_SHIM_NAME).is_file());

        store.remove_lease(&lease).expect("remove user PATH lease");
        let after = store.read().expect("read restored user PATH");
        assert_eq!(after, before);
        assert!(!bin.join(crate::shim::PHP_SHIM_NAME).exists());
        std::fs::remove_dir_all(&root).expect("remove acceptance root");
    }
}
