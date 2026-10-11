//! Atomic JSON snapshot store used by the Windows Tauri composition root.
//!
//! This is a transitional durable adapter, not the final SQLite journal. It
//! keeps native plan/apply state behind the `EnvironmentStore` port and never
//! accepts an arbitrary path from UI or MCP.

use cxszn_menv_domain::EnvironmentSnapshot;
use cxszn_menv_ports::{EnvironmentStore, PortError};
use std::fs;
use std::path::{Path, PathBuf};

pub struct JsonEnvironmentStore {
    path: PathBuf,
}

impl JsonEnvironmentStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn default_for_current_user() -> Self {
        let root = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Users\Default\AppData\Local"));
        Self::new(root.join("cxszn-menv").join("state-rust.json"))
    }

    fn temporary_path(&self) -> PathBuf {
        self.path
            .with_extension(format!("json.{}.tmp", std::process::id()))
    }

    fn ensure_parent(&self) -> Result<(), PortError> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)
            .map_err(|error| PortError::Platform(format!("create state directory: {error}")))
    }
}

impl EnvironmentStore for JsonEnvironmentStore {
    fn load(&self) -> Result<EnvironmentSnapshot, PortError> {
        match fs::read_to_string(&self.path) {
            Ok(value) => serde_json::from_str(&value)
                .map_err(|error| PortError::Platform(format!("parse environment state: {error}"))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(EnvironmentSnapshot::default())
            }
            Err(error) => Err(PortError::Platform(format!(
                "read environment state: {error}"
            ))),
        }
    }

    fn save(&self, snapshot: &EnvironmentSnapshot) -> Result<(), PortError> {
        self.ensure_parent()?;
        let bytes = serde_json::to_vec_pretty(snapshot).map_err(|error| {
            PortError::Platform(format!("serialize environment state: {error}"))
        })?;
        let temporary = self.temporary_path();
        fs::write(&temporary, bytes).map_err(|error| {
            PortError::Platform(format!("write environment state staging: {error}"))
        })?;
        if let Err(error) = fs::rename(&temporary, &self.path) {
            let _ = fs::remove_file(&temporary);
            return Err(PortError::Platform(format!(
                "publish environment state: {error}"
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn missing_state_loads_default_and_save_roundtrips() {
        let root = std::env::temp_dir().join(format!(
            "cxszn-menv-store-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let store = JsonEnvironmentStore::new(root.join("nested").join("state.json"));
        let initial = store.load().expect("missing state is default");
        store.save(&initial).expect("save state");
        assert_eq!(store.load().expect("reload state"), initial);
        let _ = fs::remove_dir_all(root);
    }
}
