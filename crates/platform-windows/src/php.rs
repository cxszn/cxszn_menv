//! Read-only PHP discovery and verification for Windows.
//!
//! This adapter never registers, copies, deletes, or enables a PHP executable.
//! It probes an explicit absolute `php.exe` path with `-n` and `--ini`, keeps
//! every discovered record disabled, and reports unsupported versions instead
//! of pretending that they are installable.

use cxszn_menv_domain::{
    Architecture, Availability, Health, PhpThreadSafety, RuntimeFamily, RuntimeId, RuntimeIniInfo,
    RuntimeIniSource, RuntimeRecord, RuntimeSource,
};
use cxszn_menv_ports::{PortError, RuntimeProvider, ScanRequest, VerifyPathRequest};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_COMMON_SCAN_DEPTH: usize = 2;

#[derive(Default)]
pub struct WindowsPhpProvider;

impl RuntimeProvider for WindowsPhpProvider {
    fn family(&self) -> RuntimeFamily {
        RuntimeFamily::Php
    }

    fn scan(&self, request: &ScanRequest) -> Result<Vec<RuntimeRecord>, PortError> {
        if request.family.is_some() && request.family != Some(RuntimeFamily::Php) {
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
                let path = PathBuf::from(entry).join("php.exe");
                push_candidate(&mut candidates, &mut seen, path, RuntimeSource::SystemPath);
            }
            if request.include_common_locations {
                for (root, source) in common_roots() {
                    collect_php_candidates(&root, 0, source, &mut candidates, &mut seen);
                }
            }
            let mut records = Vec::new();
            for (path, source) in candidates {
                match self.verify_path_with_source(&path, source) {
                    Ok(record) => records.push(record),
                    Err(PortError::InvalidPath(_)) | Err(PortError::ProbeTimeout) => continue,
                    Err(error) => return Err(error),
                }
            }
            Ok(records)
        }
    }

    fn verify_path(&self, request: &VerifyPathRequest) -> Result<RuntimeRecord, PortError> {
        if request.family != RuntimeFamily::Php {
            return Err(PortError::InvalidPath(
                "Windows PHP provider only accepts family=php".to_owned(),
            ));
        }
        #[cfg(not(windows))]
        {
            let _ = request;
            Err(PortError::UnsupportedPlatform)
        }
        #[cfg(windows)]
        {
            let path = normalize_php_path(&request.path)?;
            self.verify_path_with_source(&path, RuntimeSource::Manual)
        }
    }
}

#[cfg(windows)]
impl WindowsPhpProvider {
    fn verify_path_with_source(
        &self,
        path: &Path,
        source: RuntimeSource,
    ) -> Result<RuntimeRecord, PortError> {
        let executable = normalize_php_path(path)?;
        let shim = crate::shim::ShimManager::default_for_current_user()?;
        if crate::shim::is_same_local_path(&executable, &shim.bin_directory().join("php.exe")) {
            return Err(PortError::InvalidPath(
                "the app-owned PHP shim is not an independent runtime".to_owned(),
            ));
        }
        let checked_at = now_iso();
        let output = run_bounded(&executable, &["-n", "-v"])?;
        let stdout = text(&output.stdout);
        let stderr = text(&output.stderr);
        let combined = format!("{stdout}\n{stderr}");
        let version = parse_version(&stdout);
        let supported = version
            .as_deref()
            .map(is_supported_version)
            .unwrap_or(false);
        let mut diagnostics = Vec::new();
        if !stderr.trim().is_empty() {
            diagnostics.push(stderr.trim().to_owned());
        }
        if output.status.code().unwrap_or(1) != 0 {
            diagnostics.push(format!(
                "PHP probe exited with code {}.",
                output.status.code().unwrap_or(-1)
            ));
        }
        if version.is_none() {
            diagnostics.push("PHP version could not be parsed from probe output.".to_owned());
        } else if !supported {
            diagnostics.push("This PHP version is outside the supported 5.6–8.5 range.".to_owned());
        }

        let ini = if output.status.success() && version.is_some() {
            match run_bounded(&executable, &["--ini"]) {
                Ok(ini_output) => parse_ini(&text(&ini_output.stdout), &text(&ini_output.stderr)),
                Err(error) => {
                    diagnostics.push(format!("PHP ini probe failed: {error}"));
                    RuntimeIniInfo {
                        source: RuntimeIniSource::Unknown,
                        path: None,
                    }
                }
            }
        } else {
            RuntimeIniInfo {
                source: RuntimeIniSource::Unknown,
                path: None,
            }
        };

        let normalized = executable.to_string_lossy().replace('/', "\\");
        let id = RuntimeId::new(format!("php:win:{}", normalized.to_ascii_lowercase()))
            .map_err(|_| PortError::InvalidPath("PHP path is too long".to_owned()))?;
        let mut record = RuntimeRecord::discovered(
            id,
            RuntimeFamily::Php,
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
        record.thread_safety = parse_thread_safety(&combined);
        record.availability = if supported {
            record.availability
        } else {
            Availability::Unsupported
        };
        record.health = if output.status.success() && version.is_some() {
            if supported {
                Health::Healthy
            } else {
                Health::Warning
            }
        } else {
            Health::Error
        };
        record.ini = ini;
        record.legacy = version
            .as_deref()
            .is_some_and(|value| value.starts_with("5.6."));
        record.diagnostics = diagnostics;
        record.first_seen_at = checked_at.clone();
        record.last_checked_at = checked_at;
        Ok(record)
    }
}

#[cfg(windows)]
fn common_roots() -> Vec<(PathBuf, RuntimeSource)> {
    let mut roots = vec![(PathBuf::from(r"C:\php"), RuntimeSource::CommonLocation)];
    for variable in ["LOCALAPPDATA", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(value) = std::env::var_os(variable) {
            let root = PathBuf::from(value);
            let managed_source = if variable == "LOCALAPPDATA" {
                RuntimeSource::ManagedCatalog
            } else {
                RuntimeSource::CommonLocation
            };
            roots.push((
                root.join("cxszn-menv").join("versions").join("php"),
                managed_source,
            ));
            roots.push((root.join("PHP"), RuntimeSource::CommonLocation));
        }
    }
    // When the portable app is installed under a developer tool root such as
    // `<drive>:\App\Code_Tool\cxszn_menv`, also inspect the sibling
    // `<drive>:\App\Env\php` tree. This remains read-only and is derived from
    // the actual executable location instead of hard-coding a machine path.
    if let Ok(executable) = std::env::current_exe() {
        if let Some(install_dir) = executable.parent() {
            roots.push((
                install_dir.join("versions").join("php"),
                RuntimeSource::ManagedCatalog,
            ));
            if let Some(tool_dir) = install_dir.parent() {
                if let Some(app_root) = tool_dir.parent() {
                    roots.push((
                        app_root.join("Env").join("php"),
                        RuntimeSource::CommonLocation,
                    ));
                    roots.push((
                        app_root.join("env").join("php"),
                        RuntimeSource::CommonLocation,
                    ));
                }
            }
        }
    }
    roots
}

#[cfg(windows)]
fn collect_php_candidates(
    root: &Path,
    depth: usize,
    source: RuntimeSource,
    candidates: &mut Vec<(PathBuf, RuntimeSource)>,
    seen: &mut HashSet<String>,
) {
    if depth > MAX_COMMON_SCAN_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("php.exe"))
        {
            push_candidate(candidates, seen, path, source.clone());
        } else if path.is_dir() {
            collect_php_candidates(&path, depth + 1, source.clone(), candidates, seen);
        }
    }
}

#[cfg(windows)]
fn push_candidate(
    candidates: &mut Vec<(PathBuf, RuntimeSource)>,
    seen: &mut HashSet<String>,
    path: PathBuf,
    source: RuntimeSource,
) {
    let Ok(normalized) = normalize_php_path(&path) else {
        return;
    };
    let key = normalized.to_string_lossy().to_ascii_lowercase();
    if seen.insert(key) {
        candidates.push((normalized, source));
    }
}

#[cfg(windows)]
fn normalize_php_path(path: &Path) -> Result<PathBuf, PortError> {
    let candidate = if path.is_dir() {
        path.join("php.exe")
    } else {
        path.to_path_buf()
    };
    let text = candidate.to_string_lossy();
    if !is_local_absolute(&candidate)
        || text.starts_with(r"\\")
        || text.starts_with(r"\\?\")
        || !candidate
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("php.exe"))
    {
        return Err(PortError::InvalidPath(
            "PHP path must be an absolute local php.exe".to_owned(),
        ));
    }
    let metadata = fs::symlink_metadata(&candidate)
        .map_err(|_| PortError::InvalidPath("PHP executable does not exist".to_owned()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(PortError::InvalidPath(
            "PHP executable must be a regular non-symlink file".to_owned(),
        ));
    }
    let canonical = fs::canonicalize(&candidate)
        .map_err(|_| PortError::InvalidPath("PHP executable could not be resolved".to_owned()))?;
    let canonical = strip_extended_drive_prefix(canonical);
    if !is_local_absolute(&canonical) || canonical.to_string_lossy().starts_with(r"\\") {
        return Err(PortError::InvalidPath(
            "PHP executable resolved outside a local drive".to_owned(),
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
fn is_local_absolute(path: &Path) -> bool {
    let value = path.to_string_lossy();
    value.len() >= 3 && value.as_bytes()[1] == b':' && matches!(value.as_bytes()[2], b'\\' | b'/')
}

#[cfg(windows)]
fn run_bounded(executable: &Path, args: &[&str]) -> Result<Output, PortError> {
    let mut child = Command::new(executable)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| PortError::Platform(format!("start PHP probe: {error}")))?;
    wait_bounded(&mut child)?;
    child
        .wait_with_output()
        .map_err(|error| PortError::Platform(format!("read PHP probe: {error}")))
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
            Err(error) => return Err(PortError::Platform(format!("wait for PHP probe: {error}"))),
        }
    }
}

#[cfg(windows)]
fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn parse_version(output: &str) -> Option<String> {
    let marker = output.find("PHP ")?;
    let value = &output[marker + 4..];
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

fn is_supported_version(version: &str) -> bool {
    let mut parts = version.split('.').map(|part| part.parse::<u32>().ok());
    let (Some(major), Some(minor), Some(_patch)) = (
        parts.next().flatten(),
        parts.next().flatten(),
        parts.next().flatten(),
    ) else {
        return false;
    };
    (major == 5 && minor == 6) || (major == 7 && minor <= 4) || (major == 8 && minor <= 5)
}

fn parse_architecture(output: &str) -> Architecture {
    let lower = output.to_ascii_lowercase();
    if lower.contains("arm64") {
        Architecture::Arm64
    } else if lower.contains("x64") || lower.contains("amd64") {
        Architecture::X64
    } else if lower.contains("x86") || lower.contains("win32") {
        Architecture::X86
    } else {
        Architecture::Unknown
    }
}

fn parse_thread_safety(output: &str) -> PhpThreadSafety {
    let upper = output.to_ascii_uppercase();
    if upper.contains("NTS") {
        PhpThreadSafety::Nts
    } else if upper.contains("TS") {
        PhpThreadSafety::Ts
    } else {
        PhpThreadSafety::Unknown
    }
}

fn parse_ini(stdout: &str, stderr: &str) -> RuntimeIniInfo {
    let combined = format!("{stdout}\n{stderr}");
    let Some(line) = combined.lines().find(|line| {
        line.to_ascii_lowercase()
            .contains("loaded configuration file")
    }) else {
        return RuntimeIniInfo {
            source: RuntimeIniSource::Unknown,
            path: None,
        };
    };
    let Some((_, value)) = line.split_once("=>").or_else(|| line.split_once(':')) else {
        return RuntimeIniInfo {
            source: RuntimeIniSource::Unknown,
            path: None,
        };
    };
    let value = value.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("(none)") {
        RuntimeIniInfo {
            source: RuntimeIniSource::None,
            path: None,
        }
    } else {
        RuntimeIniInfo {
            source: RuntimeIniSource::Explicit,
            path: Some(value.replace('/', "\\")),
        }
    }
}

fn now_iso() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0);
    format!("{seconds}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_php_versions_and_rejects_out_of_range() {
        assert_eq!(parse_version("PHP 8.5.11 (cli)"), Some("8.5.11".to_owned()));
        assert!(is_supported_version("5.6.40"));
        assert!(is_supported_version("8.5.11"));
        assert!(!is_supported_version("8.6.0"));
    }

    #[test]
    fn parses_php_architecture_thread_safety_and_ini() {
        assert_eq!(
            parse_architecture("PHP 8.4.26 (cli) (built: x64 NTS)"),
            Architecture::X64
        );
        assert_eq!(
            parse_thread_safety("PHP 8.4.26 (cli) (built: x64 NTS)"),
            PhpThreadSafety::Nts
        );
        assert_eq!(
            parse_ini("Loaded Configuration File => C:\\PHP\\php.ini", ""),
            RuntimeIniInfo {
                source: RuntimeIniSource::Explicit,
                path: Some("C:\\PHP\\php.ini".to_owned())
            }
        );
    }

    #[test]
    fn parses_actual_php_ini_cli_labels_without_splitting_the_drive_letter() {
        assert_eq!(
            parse_ini(
                "Loaded Configuration File:         R:\\Tools\\PHP\\php.ini\r\n",
                ""
            ),
            RuntimeIniInfo {
                source: RuntimeIniSource::Explicit,
                path: Some(r"R:\Tools\PHP\php.ini".to_owned()),
            }
        );
        assert_eq!(
            parse_ini("Loaded Configuration File:         (none)\r\n", ""),
            RuntimeIniInfo {
                source: RuntimeIniSource::None,
                path: None
            }
        );
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "probes a user-supplied local PHP executable; set MENV_TEST_PHP_PATH=... to run"]
    fn live_verifies_user_supplied_php_executable() {
        let path = std::env::var_os("MENV_TEST_PHP_PATH").expect("MENV_TEST_PHP_PATH");
        let record = WindowsPhpProvider
            .verify_path(&VerifyPathRequest {
                family: RuntimeFamily::Php,
                path: PathBuf::from(path),
            })
            .expect("PHP verification");
        assert_eq!(record.version.as_deref(), Some("8.5.11"));
        assert_eq!(record.activation, cxszn_menv_domain::Activation::Disabled);
        assert_eq!(record.availability, Availability::Manual);
    }
}
