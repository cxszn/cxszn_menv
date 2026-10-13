use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Console::{
    GetConsoleMode, GetStdHandle, CONSOLE_MODE, STD_ERROR_HANDLE, STD_HANDLE, STD_INPUT_HANDLE,
};

#[derive(Debug)]
pub struct ConfirmationConsoleCapabilities {
    pub stdin_console: bool,
    pub stderr_console: bool,
}

/// Borrow standard handles; never allocate a console or replace redirection.
/// https://learn.microsoft.com/en-us/windows/console/getstdhandle#remarks
pub fn confirmation_console_capabilities() -> ConfirmationConsoleCapabilities {
    ConfirmationConsoleCapabilities {
        stdin_console: standard_handle_is_console(STD_INPUT_HANDLE),
        stderr_console: standard_handle_is_console(STD_ERROR_HANDLE),
    }
}

fn standard_handle_is_console(kind: STD_HANDLE) -> bool {
    unsafe { GetStdHandle(kind) }.is_ok_and(is_console_handle)
}

fn is_console_handle(handle: HANDLE) -> bool {
    if handle.is_invalid() {
        return false;
    }
    let mut mode = CONSOLE_MODE::default();
    // Unlike named-PTY heuristics, GetConsoleMode requires a real console buffer.
    // https://learn.microsoft.com/en-us/windows/console/getconsolemode#parameters
    unsafe { GetConsoleMode(handle, &mut mode) }.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::AsRawHandle;

    #[test]
    fn confirmation_console_check_rejects_missing_and_regular_file_handles() {
        assert!(!is_console_handle(HANDLE::default()));
        let path = std::env::temp_dir().join(format!(
            "menv-console-file-{}.tmp",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("isolated regular file");
        assert!(!is_console_handle(HANDLE(file.as_raw_handle())));
        drop(file);
        std::fs::remove_file(path).expect("remove isolated regular file");
    }
}
