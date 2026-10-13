//! Read-only Node.js discovery for Windows.
//!
//! This provider deliberately stops at detection and probing. It never
//! changes PATH or takes ownership of nvm-windows, an external Node manager,
//! or an existing installation.

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
pub struct WindowsNodeProvider;

impl RuntimeProvider for WindowsNodeProvider {
    fn family(&self) -> RuntimeFamily {
        RuntimeFamily::Node
    }

    fn scan(&self, request: &ScanRequest) -> Result<Vec<RuntimeRecord>, PortError> {
        if request.family.is_some() && request.family != Some(RuntimeFamily::Node) {
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
                    PathBuf::from(entry).join("node.exe"),
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
                match verify_node_path(&path, source) {
                    Ok(record) => records.push(record),
                    Err(PortError::InvalidPath(_)) | Err(PortError::ProbeTimeout) => continue,
                    Err(error) => return Err(error),
                }
            }
            Ok(records)
        }
    }

    fn verify_path(&self, request: &VerifyPathRequest) -> Result<RuntimeRecord, PortError> {
        if request.family != RuntimeFamily::Node {
            return Err(PortError::InvalidPath(
                "Windows Node provider only accepts family=node".to_owned(),
            ));
        }
        #[cfg(not(windows))]
        {
            let _ = request;
            Err(PortError::UnsupportedPlatform)
        }
        #[cfg(windows)]
        {
            let path = normalize_node_path(&request.path)?;
            verify_node_path(&path, RuntimeSource::Manual)
        }
    }
}

#[cfg(windows)]
fn common_candidates() -> Vec<(PathBuf, RuntimeSource)> {
    let mut candidates = Vec::new();
    for variable in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
        if let Some(root) = std::env::var_os(variable).map(PathBuf::from) {
            candidates.push((
                root.join("nodejs").join("node.exe"),
                RuntimeSource::CommonLocation,
            ));
            candidates.push((
                root.join("Programs").join("nodejs").join("node.exe"),
                RuntimeSource::CommonLocation,
            ));
        }
    }
    for variable in ["NVM_HOME", "APPDATA"] {
        if let Some(root) = std::env::var_os(variable).map(PathBuf::from) {
            let nvm_root = if variable == "APPDATA" {
                root.join("nvm")
            } else {
                root
            };
            if let Ok(entries) = std::fs::read_dir(&nvm_root) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        candidates.push((path.join("node.exe"), RuntimeSource::CommonLocation));
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
    let Ok(normalized) = normalize_node_path(&path) else {
        return;
    };
    let key = normalized.to_string_lossy().to_ascii_lowercase();
    if seen.insert(key) {
        candidates.push((normalized, source));
    }
}

#[cfg(windows)]
fn verify_node_path(path: &Path, source: RuntimeSource) -> Result<RuntimeRecord, PortError> {
    let executable = normalize_node_path(path)?;
    let checked_at = now_iso();
    let version_output = run_bounded(&executable, &["--version"])?;
    let stdout = text(&version_output.stdout);
    let stderr = text(&version_output.stderr);
    let version = parse_node_version(&stdout);
    let mut diagnostics = Vec::new();
    if !stderr.trim().is_empty() {
        diagnostics.push(stderr.trim().to_owned());
    }
    if version_output.status.code().unwrap_or(1) != 0 {
        diagnostics.push(format!(
            "Node probe exited with code {}.",
            version_output.status.code().unwrap_or(-1)
        ));
    }
    if version.is_none() {
        diagnostics.push("Node version could not be parsed from probe output.".to_owned());
    }

    let architecture = match run_bounded(&executable, &["-p", "process.arch"]) {
        Ok(output) => parse_architecture(&text(&output.stdout)),
        Err(error) => {
            diagnostics.push(format!("Node architecture probe failed: {error}"));
            Architecture::Unknown
        }
    };
    let normalized = executable.to_string_lossy().replace('/', "\\");
    let id = RuntimeId::new(format!("node:win:{}", normalized.to_ascii_lowercase()))
        .map_err(|_| PortError::InvalidPath("Node path is too long".to_owned()))?;
    let mut record = RuntimeRecord::discovered(
        id,
        RuntimeFamily::Node,
        version.clone().unwrap_or_else(|| "unknown".to_owned()),
        architecture,
        normalized.clone(),
        source,
    );
    record.root_path = executable
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_string_lossy()
        .replace('/', "\\");
    record.thread_safety = PhpThreadSafety::Unknown;
    record.health = if version_output.status.success() && version.is_some() {
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
fn normalize_node_path(path: &Path) -> Result<PathBuf, PortError> {
    let candidate = if path.is_dir() {
        path.join("node.exe")
    } else {
        path.to_path_buf()
    };
    let text = candidate.to_string_lossy();
    if !is_local_absolute(&candidate)
        || text.starts_with(r"\\")
        || text.starts_with(r"\\?\")
        || !candidate
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("node.exe"))
    {
        return Err(PortError::InvalidPath(
            "Node path must be an absolute local node.exe".to_owned(),
        ));
    }
    let metadata = std::fs::symlink_metadata(&candidate)
        .map_err(|_| PortError::InvalidPath("Node executable does not exist".to_owned()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(PortError::InvalidPath(
            "Node executable must be a regular non-symlink file".to_owned(),
        ));
    }
    let canonical = std::fs::canonicalize(&candidate)
        .map_err(|_| PortError::InvalidPath("Node executable could not be resolved".to_owned()))?;
    let canonical = strip_extended_drive_prefix(canonical);
    if !is_local_absolute(&canonical) || canonical.to_string_lossy().starts_with(r"\\") {
        return Err(PortError::InvalidPath(
            "Node executable resolved outside a local drive".to_owned(),
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
        .map_err(|error| PortError::Platform(format!("start Node probe: {error}")))?;
    wait_bounded(&mut child)?;
    child
        .wait_with_output()
        .map_err(|error| PortError::Platform(format!("read Node probe: {error}")))
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
            Err(error) => return Err(PortError::Platform(format!("wait for Node probe: {error}"))),
        }
    }
}

#[cfg(windows)]
fn parse_architecture(output: &str) -> Architecture {
    match output.trim().to_ascii_lowercase().as_str() {
        "x64" => Architecture::X64,
        "ia32" | "x86" => Architecture::X86,
        "arm64" => Architecture::Arm64,
        _ => Architecture::Unknown,
    }
}

fn parse_node_version(output: &str) -> Option<String> {
    let value = output.trim().strip_prefix('v').unwrap_or(output.trim());
    let parts = value.split('.').collect::<Vec<_>>();
    if parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty() && part.chars().all(|character| character.is_ascii_digit())
        })
    {
        Some(value.to_owned())
    } else {
        None
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
    use super::{parse_node_version, RuntimeSource};
    #[cfg(windows)]
    use cxszn_menv_domain::RuntimeFamily;
    #[cfg(windows)]
    use std::path::PathBuf;

    #[test]
    fn parses_node_versions_without_accepting_arbitrary_output() {
        assert_eq!(
            parse_node_version("v22.15.0\r\n"),
            Some("22.15.0".to_owned())
        );
        assert_eq!(parse_node_version("22.15.0"), Some("22.15.0".to_owned()));
        assert_eq!(parse_node_version("node v22"), None);
        assert_eq!(parse_node_version(""), None);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "probes a user-supplied local Node executable; set MENV_TEST_NODE_PATH=... to run"]
    fn live_verifies_user_supplied_node_executable() {
        let path = std::env::var_os("MENV_TEST_NODE_PATH")
            .map(PathBuf::from)
            .expect("MENV_TEST_NODE_PATH");
        let record =
            super::verify_node_path(&path, RuntimeSource::Manual).expect("verify Node executable");
        assert_eq!(record.family, RuntimeFamily::Node);
        assert!(record
            .version
            .as_deref()
            .is_some_and(|version| { version.split('.').count() == 3 }));
        assert_eq!(record.ownership, cxszn_menv_domain::Ownership::Manual);
        assert_eq!(record.activation, cxszn_menv_domain::Activation::Disabled);
        println!(
            "Node scan acceptance: version={} architecture={:?} path={}",
            record.version.as_deref().unwrap_or("unknown"),
            record.architecture,
            record.executable_path
        );
    }
}
