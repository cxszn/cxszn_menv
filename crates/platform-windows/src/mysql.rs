//! Read-only MySQL/MariaDB client discovery for Windows.
//!
//! This provider probes only the local `mysql.exe --version` executable. It
//! never connects to a server, starts a service, inspects data directories, or
//! changes PATH.

use cxszn_menv_domain::{
    Architecture, Health, PhpThreadSafety, RuntimeFamily, RuntimeId, RuntimeRecord, RuntimeSource,
};
use cxszn_menv_ports::{PortError, RuntimeProvider, ScanRequest, VerifyPathRequest};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Default)]
pub struct WindowsMysqlProvider;

impl RuntimeProvider for WindowsMysqlProvider {
    fn family(&self) -> RuntimeFamily {
        RuntimeFamily::Mysql
    }

    fn scan(&self, request: &ScanRequest) -> Result<Vec<RuntimeRecord>, PortError> {
        if request.family.is_some() && request.family != Some(RuntimeFamily::Mysql) {
            return Ok(Vec::new());
        }
        #[cfg(not(windows))]
        {
            let _ = request;
            return Err(PortError::UnsupportedPlatform);
        }
        #[cfg(windows)]
        {
            let mut candidates = Vec::new();
            let mut seen = HashSet::new();
            for entry in std::env::var_os("PATH")
                .unwrap_or_default()
                .to_string_lossy()
                .split(';')
                .filter(|entry| !entry.trim().is_empty())
            {
                push_candidate(
                    &mut candidates,
                    &mut seen,
                    PathBuf::from(entry).join("mysql.exe"),
                    RuntimeSource::SystemPath,
                );
            }
            if request.include_common_locations {
                for (path, source) in common_candidates() {
                    push_candidate(&mut candidates, &mut seen, path, source);
                }
            }
            let mut records = Vec::new();
            for (path, source) in candidates {
                match verify_mysql_path(&path, source) {
                    Ok(record) => records.push(record),
                    Err(PortError::InvalidPath(_)) | Err(PortError::ProbeTimeout) => continue,
                    Err(error) => return Err(error),
                }
            }
            Ok(records)
        }
    }

    fn verify_path(&self, request: &VerifyPathRequest) -> Result<RuntimeRecord, PortError> {
        if request.family != RuntimeFamily::Mysql {
            return Err(PortError::InvalidPath(
                "Windows MySQL provider only accepts family=mysql".to_owned(),
            ));
        }
        #[cfg(not(windows))]
        {
            let _ = request;
            Err(PortError::UnsupportedPlatform)
        }
        #[cfg(windows)]
        {
            let path = normalize_mysql_path(&request.path)?;
            verify_mysql_path(&path, RuntimeSource::Manual)
        }
    }
}

#[cfg(windows)]
fn common_candidates() -> Vec<(PathBuf, RuntimeSource)> {
    let mut candidates = Vec::new();
    if let Some(home) = std::env::var_os("MYSQL_HOME").map(PathBuf::from) {
        candidates.push((
            home.join("bin").join("mysql.exe"),
            RuntimeSource::CommonLocation,
        ));
        candidates.push((home.join("mysql.exe"), RuntimeSource::CommonLocation));
    }
    for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(root) = std::env::var_os(variable).map(PathBuf::from) {
            for product in ["MySQL", "MariaDB"] {
                let product_root = root.join(product);
                if let Ok(entries) = std::fs::read_dir(&product_root) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            candidates.push((
                                path.join("bin").join("mysql.exe"),
                                RuntimeSource::CommonLocation,
                            ));
                        }
                    }
                }
            }
        }
    }
    candidates
}

#[cfg(windows)]
fn push_candidate(
    candidates: &mut Vec<(PathBuf, RuntimeSource)>,
    seen: &mut HashSet<String>,
    path: PathBuf,
    source: RuntimeSource,
) {
    let Ok(normalized) = normalize_mysql_path(&path) else {
        return;
    };
    let key = normalized.to_string_lossy().to_ascii_lowercase();
    if seen.insert(key) {
        candidates.push((normalized, source));
    }
}

#[cfg(windows)]
fn verify_mysql_path(path: &Path, source: RuntimeSource) -> Result<RuntimeRecord, PortError> {
    let executable = normalize_mysql_path(path)?;
    let checked_at = now_iso();
    let output = run_bounded(&executable, &["--version"])?;
    let stdout = text(&output.stdout);
    let stderr = text(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");
    let version = parse_mysql_version(&combined);
    let mut diagnostics = Vec::new();
    if !stderr.trim().is_empty() {
        diagnostics.push(stderr.trim().to_owned());
    }
    if output.status.code().unwrap_or(1) != 0 {
        diagnostics.push(format!(
            "MySQL probe exited with code {}.",
            output.status.code().unwrap_or(-1)
        ));
    }
    if version.is_none() {
        diagnostics.push("MySQL version could not be parsed from probe output.".to_owned());
    }
    let normalized = executable.to_string_lossy().replace('/', "\\");
    let id = RuntimeId::new(format!("mysql:win:{}", normalized.to_ascii_lowercase()))
        .map_err(|_| PortError::InvalidPath("MySQL path is too long".to_owned()))?;
    let mut record = RuntimeRecord::discovered(
        id,
        RuntimeFamily::Mysql,
        version.clone().unwrap_or_else(|| "unknown".to_owned()),
        parse_architecture(&combined),
        normalized.clone(),
        source,
    );
    record.root_path = executable
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_string_lossy()
        .replace('/', "\\");
    record.thread_safety = PhpThreadSafety::Unknown;
    record.health = if output.status.success() && version.is_some() {
        Health::Healthy
    } else {
        Health::Error
    };
    record.diagnostics = diagnostics;
    record.first_seen_at = checked_at.clone();
    record.last_checked_at = checked_at;
    Ok(record)
}

#[cfg(windows)]
fn normalize_mysql_path(path: &Path) -> Result<PathBuf, PortError> {
    let candidate = if path.is_dir() {
        path.join("mysql.exe")
    } else {
        path.to_path_buf()
    };
    let text = candidate.to_string_lossy();
    if !is_local_absolute(&candidate)
        || text.starts_with(r"\\")
        || text.starts_with(r"\\?\")
        || !candidate
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("mysql.exe"))
    {
        return Err(PortError::InvalidPath(
            "MySQL path must be an absolute local mysql.exe".to_owned(),
        ));
    }
    let metadata = std::fs::symlink_metadata(&candidate)
        .map_err(|_| PortError::InvalidPath("MySQL executable does not exist".to_owned()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(PortError::InvalidPath(
            "MySQL executable must be a regular non-symlink file".to_owned(),
        ));
    }
    let canonical =
        strip_extended_drive_prefix(std::fs::canonicalize(&candidate).map_err(|_| {
            PortError::InvalidPath("MySQL executable could not be resolved".to_owned())
        })?);
    if !is_local_absolute(&canonical) || canonical.to_string_lossy().starts_with(r"\\") {
        return Err(PortError::InvalidPath(
            "MySQL executable resolved outside a local drive".to_owned(),
        ));
    }
    Ok(canonical)
}

#[cfg(windows)]
fn strip_extended_drive_prefix(path: PathBuf) -> PathBuf {
    let value = path.to_string_lossy();
    if value.len() >= 6
        && value.starts_with(r"\\?\")
        && value.as_bytes()[4].is_ascii_alphabetic()
        && value.as_bytes()[5] == b':'
    {
        return PathBuf::from(&value[4..]);
    }
    path
}

#[cfg(windows)]
fn run_bounded(executable: &Path, args: &[&str]) -> Result<Output, PortError> {
    let mut child = Command::new(executable)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| PortError::Platform(format!("start MySQL probe: {error}")))?;
    wait_bounded(&mut child)?;
    child
        .wait_with_output()
        .map_err(|error| PortError::Platform(format!("read MySQL probe: {error}")))
}

#[cfg(windows)]
fn wait_bounded(child: &mut Child) -> Result<(), PortError> {
    let deadline = std::time::Instant::now() + PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) if std::time::Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(25))
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(PortError::ProbeTimeout);
            }
            Err(error) => {
                return Err(PortError::Platform(format!(
                    "wait for MySQL probe: {error}"
                )))
            }
        }
    }
}

fn parse_mysql_version(output: &str) -> Option<String> {
    // MariaDB prints a client protocol version after `Ver` and the actual
    // distribution version after `Distrib`; the latter is the useful runtime
    // version for the inventory. Standard MySQL has no Distrib marker.
    let (marker, prefix_len) = if let Some(marker) = output.find("Distrib ") {
        (marker, "Distrib ".len())
    } else {
        (output.find("Ver ")?, "Ver ".len())
    };
    let value = &output[marker + prefix_len..];
    let version: String = value
        .chars()
        .take_while(|character| character.is_ascii_digit() || *character == '.')
        .collect();
    if version.split('.').count() >= 2 {
        Some(version)
    } else {
        None
    }
}

#[cfg(windows)]
fn parse_architecture(output: &str) -> Architecture {
    let value = output.to_ascii_lowercase();
    if value.contains("win64") || value.contains("x86_64") || value.contains("amd64") {
        Architecture::X64
    } else if value.contains("win32") || value.contains("x86") {
        Architecture::X86
    } else if value.contains("arm64") || value.contains("aarch64") {
        Architecture::Arm64
    } else {
        Architecture::Unknown
    }
}

#[cfg(windows)]
fn is_local_absolute(path: &Path) -> bool {
    let value = path.to_string_lossy();
    value.len() >= 3 && value.as_bytes()[1] == b':' && matches!(value.as_bytes()[2], b'\\' | b'/')
}

#[cfg(windows)]
fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(windows)]
fn now_iso() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0);
    format!("{seconds}Z")
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_mysql_versions_without_accepting_arbitrary_output() {
        assert_eq!(
            super::parse_mysql_version("mysql  Ver 8.0.36 for Win64"),
            Some("8.0.36".to_owned())
        );
        assert_eq!(
            super::parse_mysql_version("mysql  Ver 15.1 Distrib 10.11.7-MariaDB, for Win64"),
            Some("10.11.7".to_owned())
        );
        assert_eq!(super::parse_mysql_version("mysql client"), None);
    }
}
