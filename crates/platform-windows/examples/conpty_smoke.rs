#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    use cxszn_menv_platform_windows::conpty::{ConptyConfig, ConptyEvent, ConptySession};
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    let config = ConptyConfig {
        executable: PathBuf::from(r"C:\Windows\System32\cmd.exe"),
        args: vec![OsString::from("/C"), OsString::from("echo MENV_CONPTY_OK")],
        cwd: PathBuf::from(r"C:\Windows"),
        environment: std::env::vars_os().collect::<BTreeMap<_, _>>(),
        columns: 120,
        rows: 32,
        output_limit_bytes: 64 * 1024,
        timeout: Duration::from_secs(10),
    };
    let mut session = match ConptySession::start(config) {
        Ok(session) => session,
        Err(_) => std::process::exit(10),
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut output = String::new();
    while Instant::now() < deadline && !output.contains("MENV_CONPTY_OK") {
        match session.recv(Duration::from_millis(100)) {
            Ok(Some(ConptyEvent::Output(bytes))) => {
                output.push_str(&String::from_utf8_lossy(&bytes));
            }
            Ok(Some(ConptyEvent::Closed)) | Ok(Some(ConptyEvent::Exited(_))) => break,
            Ok(Some(ConptyEvent::Error(_))) | Err(_) => break,
            Ok(None) => {}
        }
    }
    let exit_code = session.wait().unwrap_or(11);
    let _ = session.shutdown();
    if exit_code == 0 && output.contains("MENV_CONPTY_OK") {
        std::process::exit(0);
    }
    std::process::exit(20);
}

#[cfg(not(windows))]
fn main() {
    std::process::exit(101);
}
