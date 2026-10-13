//! Read-only Python discovery for Windows.
//!
//! This provider detects `python.exe` installations and leaves `py.exe`,
//! virtual environments, PATH, and external Python managers untouched.

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
pub struct WindowsPythonProvider;

impl RuntimeProvider for WindowsPythonProvider {
    fn family(&self) -> RuntimeFamily {
        RuntimeFamily::Python
    }

    fn scan(&self, request: &ScanRequest) -> Result<Vec<RuntimeRecord>, PortError> {
        if request.family.is_some() && request.family != Some(RuntimeFamily::Python) {
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
                for executable_name in ["python.exe", "py.exe"] {
                    push_candidate(
                        &mut candidates,
                        &mut seen,
                        PathBuf::from(entry).join(executable_name),
                        RuntimeSource::SystemPath,
                    );
                }
            }
            if request.include_common_locations {
                for (path, source) in common_candidates() {
                    push_candidate(&mut candidates, &mut seen, path, source);
                }
            }
            let mut records = Vec::new();
            for (path, source) in candidates {
                match verify_python_path(&path, source) {
                    Ok(record) => records.push(record),
                    Err(PortError::InvalidPath(_)) | Err(PortError::ProbeTimeout) => continue,
                    Err(error) => return Err(error),
                }
            }
            Ok(records)
        }
    }

    fn verify_path(&self, request: &VerifyPathRequest) -> Result<RuntimeRecord, PortError> {
        if request.family != RuntimeFamily::Python {
            return Err(PortError::InvalidPath(
                "Windows Python provider only accepts family=python".to_owned(),
            ));
        }
        #[cfg(not(windows))]
        {
            let _ = request;
            Err(PortError::UnsupportedPlatform)
        }
        #[cfg(windows)]
        {
            let path = normalize_python_path(&request.path)?;
            verify_python_path(&path, RuntimeSource::Manual)
        }
    }
}

#[cfg(windows)]
fn common_candidates() -> Vec<(PathBuf, RuntimeSource)> {
    let mut candidates = Vec::new();
    if let Some(home) = std::env::var_os("PYTHONHOME").map(PathBuf::from) {
        candidates.push((home.join("python.exe"), RuntimeSource::CommonLocation));
    }
    for variable in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
        if let Some(root) = std::env::var_os(variable).map(PathBuf::from) {
            let python_root = if variable == "LOCALAPPDATA" {
                root.join("Programs").join("Python")
            } else {
                root
            };
            if let Ok(entries) = std::fs::read_dir(&python_root) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        candidates.push((path.join("python.exe"), RuntimeSource::CommonLocation));
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
    let Ok(normalized) = normalize_python_path(&path) else {
        return;
    };
    let key = normalized.to_string_lossy().to_ascii_lowercase();
    if seen.insert(key) {
        candidates.push((normalized, source));
    }
}

#[cfg(windows)]
fn verify_python_path(path: &Path, source: RuntimeSource) -> Result<RuntimeRecord, PortError> {
    let executable = normalize_python_path(path)?;
    let checked_at = now_iso();
    let version_output = run_bounded(&executable, &["--version"])?;
    let stdout = text(&version_output.stdout);
    let stderr = text(&version_output.stderr);
    let combined = format!("{stdout}\n{stderr}");
    let version = parse_python_version(&combined);
    let mut diagnostics = Vec::new();
    if !stderr.trim().is_empty() {
        diagnostics.push(stderr.trim().to_owned());
    }
    if version_output.status.code().unwrap_or(1) != 0 {
        diagnostics.push(format!(
            "Python probe exited with code {}.",
            version_output.status.code().unwrap_or(-1)
        ));
    }
    if version.is_none() {
        diagnostics.push("Python version could not be parsed from probe output.".to_owned());
    }
    if executable
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("py.exe"))
    {
        diagnostics.push("py.exe launcher; selected interpreter may vary.".to_owned());
    }
    let architecture = match run_bounded(
        &executable,
        &["-c", "import platform; print(platform.machine())"],
    ) {
        Ok(output) => parse_architecture(&text(&output.stdout)),
        Err(error) => {
            diagnostics.push(format!("Python architecture probe failed: {error}"));
            Architecture::Unknown
        }
    };
    let normalized = executable.to_string_lossy().replace('/', "\\");
    let id = RuntimeId::new(format!("python:win:{}", normalized.to_ascii_lowercase()))
        .map_err(|_| PortError::InvalidPath("Python path is too long".to_owned()))?;
    let mut record = RuntimeRecord::discovered(
        id,
        RuntimeFamily::Python,
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
fn normalize_python_path(path: &Path) -> Result<PathBuf, PortError> {
    let candidate = if path.is_dir() {
        path.join("python.exe")
    } else {
        path.to_path_buf()
    };
    let text = candidate.to_string_lossy();
    if !is_local_absolute(&candidate)
        || text.starts_with(r"\\")
        || text.starts_with(r"\\?\")
        || !candidate.file_name().is_some_and(is_python_entrypoint)
    {
        return Err(PortError::InvalidPath(
            "Python path must be an absolute local python.exe".to_owned(),
        ));
    }
    let metadata = std::fs::symlink_metadata(&candidate)
        .map_err(|_| PortError::InvalidPath("Python executable does not exist".to_owned()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(PortError::InvalidPath(
            "Python executable must be a regular non-symlink file".to_owned(),
        ));
    }
    let canonical = std::fs::canonicalize(&candidate).map_err(|_| {
        PortError::InvalidPath("Python executable could not be resolved".to_owned())
    })?;
    let canonical = strip_extended_drive_prefix(canonical);
    if !is_local_absolute(&canonical) || canonical.to_string_lossy().starts_with(r"\\") {
        return Err(PortError::InvalidPath(
            "Python executable resolved outside a local drive".to_owned(),
        ));
    }
    Ok(canonical)
}

#[cfg(windows)]
fn is_python_entrypoint(name: &std::ffi::OsStr) -> bool {
    name.eq_ignore_ascii_case("python.exe") || name.eq_ignore_ascii_case("py.exe")
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
        .map_err(|error| PortError::Platform(format!("start Python probe: {error}")))?;
    wait_bounded(&mut child)?;
    child
        .wait_with_output()
        .map_err(|error| PortError::Platform(format!("read Python probe: {error}")))
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
                    "wait for Python probe: {error}"
                )))
            }
        }
    }
}

fn parse_python_version(output: &str) -> Option<String> {
    let marker = output.find("Python ")?;
    let value = &output[marker + "Python ".len()..];
    let version: String = value
        .chars()
        .take_while(|character| character.is_ascii_digit() || *character == '.')
        .collect();
    if version.split('.').count() == 3 {
        Some(version)
    } else {
        None
    }
}

#[cfg(windows)]
fn parse_architecture(output: &str) -> Architecture {
    let value = output.trim().to_ascii_lowercase();
    if value.contains("arm64") || value.contains("aarch64") {
        Architecture::Arm64
    } else if value.contains("amd64") || value.contains("x86_64") {
        Architecture::X64
    } else if value.contains("x86") || value.contains("i386") || value.contains("i686") {
        Architecture::X86
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
    #[cfg(windows)]
    use std::path::PathBuf;

    #[test]
    fn parses_python_versions_without_accepting_arbitrary_output() {
        assert_eq!(
            super::parse_python_version("Python 3.12.7\r\n"),
            Some("3.12.7".to_owned())
        );
        assert_eq!(
            super::parse_python_version("warning\nPython 3.11.9"),
            Some("3.11.9".to_owned())
        );
        assert_eq!(super::parse_python_version("Python 3.12"), None);
        assert_eq!(super::parse_python_version(""), None);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "probes a user-supplied local Python executable; set MENV_TEST_PYTHON_PATH=... to run"]
    fn live_verifies_user_supplied_python_executable() {
        let path = std::env::var_os("MENV_TEST_PYTHON_PATH")
            .map(PathBuf::from)
            .expect("MENV_TEST_PYTHON_PATH");
        let record = super::verify_python_path(&path, super::RuntimeSource::Manual)
            .expect("verify Python executable");
        assert_eq!(record.family, cxszn_menv_domain::RuntimeFamily::Python);
        assert!(record
            .version
            .as_deref()
            .is_some_and(|version| { version.split('.').count() == 3 }));
        assert_eq!(record.ownership, cxszn_menv_domain::Ownership::Manual);
        assert_eq!(record.activation, cxszn_menv_domain::Activation::Disabled);
        println!(
            "Python scan acceptance: version={} architecture={:?} path={}",
            record.version.as_deref().unwrap_or("unknown"),
            record.architecture,
            record.executable_path
        );
    }
}
