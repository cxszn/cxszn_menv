//! Windows ConPTY session adapter.
//!
//! The public part of this module is deliberately platform-neutral and
//! fail-closed.  The Windows implementation uses the native synchronous
//! `CreatePseudoConsole` API and keeps each pipe on its own worker thread;
//! this is the arrangement recommended by Microsoft because servicing both
//! channels from one thread can deadlock when a buffer fills.
//!
//! References:
//! - https://learn.microsoft.com/en-us/windows/console/creating-a-pseudoconsole-session
//! - https://learn.microsoft.com/en-us/windows/console/createpseudoconsole

use std::collections::{BTreeMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

/// Hard upper bound for data retained by a terminal session.
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
/// Hard upper bound for one write submitted by the host.
pub const MAX_INPUT_BYTES: usize = 1024 * 1024;
/// The session timeout is bounded so a caller cannot accidentally create an
/// unbounded child process through the terminal adapter.
pub const MAX_SESSION_TIMEOUT: Duration = Duration::from_secs(60 * 60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConptyConfig {
    /// Absolute executable path.  Shell strings and script launchers are not
    /// accepted; callers must pass the executable and argv separately.
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    /// Existing, absolute working directory controlled by the caller.
    pub cwd: PathBuf,
    /// Environment is explicit by design.  The adapter never implicitly
    /// copies the host process environment into a child session.
    pub environment: BTreeMap<OsString, OsString>,
    pub columns: u16,
    pub rows: u16,
    pub output_limit_bytes: usize,
    pub timeout: Duration,
}

impl Default for ConptyConfig {
    fn default() -> Self {
        Self {
            executable: PathBuf::new(),
            args: Vec::new(),
            cwd: PathBuf::new(),
            environment: BTreeMap::new(),
            columns: 120,
            rows: 32,
            output_limit_bytes: 8 * 1024 * 1024,
            timeout: Duration::from_secs(30 * 60),
        }
    }
}

impl ConptyConfig {
    pub fn validate(&self) -> Result<(), ConptyError> {
        validate_absolute_executable(&self.executable)?;
        validate_absolute_directory(&self.cwd)?;

        if self.columns == 0 || self.rows == 0 {
            return Err(ConptyError::InvalidDimensions);
        }
        if self.output_limit_bytes == 0 || self.output_limit_bytes > MAX_OUTPUT_BYTES {
            return Err(ConptyError::InvalidOutputLimit);
        }
        if self.timeout.is_zero() || self.timeout > MAX_SESSION_TIMEOUT {
            return Err(ConptyError::InvalidTimeout);
        }

        for argument in &self.args {
            if contains_nul(argument) {
                return Err(ConptyError::InvalidArgument);
            }
        }

        let mut normalized_keys = HashSet::new();
        for (key, value) in &self.environment {
            let key = key.to_string_lossy();
            // Windows uses exactly `=C:`-style keys for current-drive
            // directories. Preserve those system-generated entries, but
            // reject every other attempt to smuggle an equals sign into a
            // caller-controlled environment key.
            let drive_variable = key.len() == 3
                && key.starts_with('=')
                && key.as_bytes()[1].is_ascii_alphabetic()
                && key.as_bytes()[2] == b':';
            if key.is_empty()
                || (!drive_variable && (key.starts_with('=') || key.contains('=')))
                || !normalized_keys.insert(key.to_ascii_uppercase())
            {
                return Err(ConptyError::InvalidEnvironmentKey);
            }
            if contains_nul(value) {
                return Err(ConptyError::InvalidEnvironmentValue);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConptyEvent {
    Output(Vec<u8>),
    Closed,
    Exited(u32),
    Error(ConptyError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConptyError {
    UnsupportedPlatform,
    InvalidExecutable,
    InvalidWorkingDirectory,
    InvalidArgument,
    InvalidEnvironmentKey,
    InvalidEnvironmentValue,
    InvalidDimensions,
    InvalidOutputLimit,
    InvalidTimeout,
    InputTooLarge,
    OutputLimitExceeded,
    Timeout,
    Closed,
    ApiFailure { operation: &'static str, code: u32 },
    Io(String),
}

impl fmt::Display for ConptyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => f.write_str("ConPTY is only available on Windows"),
            Self::InvalidExecutable => {
                f.write_str("executable must be an existing absolute .exe path")
            }
            Self::InvalidWorkingDirectory => {
                f.write_str("working directory must be an existing absolute directory")
            }
            Self::InvalidArgument => f.write_str("argv contains an invalid NUL character"),
            Self::InvalidEnvironmentKey => f.write_str("environment key is invalid"),
            Self::InvalidEnvironmentValue => {
                f.write_str("environment value contains a NUL character")
            }
            Self::InvalidDimensions => f.write_str("terminal dimensions must be non-zero"),
            Self::InvalidOutputLimit => {
                f.write_str("terminal output limit is outside the safe range")
            }
            Self::InvalidTimeout => f.write_str("terminal timeout is outside the safe range"),
            Self::InputTooLarge => f.write_str("terminal input exceeds the per-write limit"),
            Self::OutputLimitExceeded => {
                f.write_str("terminal output exceeded the configured limit")
            }
            Self::Timeout => f.write_str("terminal process exceeded its timeout"),
            Self::Closed => f.write_str("terminal session is closed"),
            Self::ApiFailure { operation, code } => {
                write!(f, "{operation} failed with Win32 error {code}")
            }
            Self::Io(message) => write!(f, "terminal I/O failed: {message}"),
        }
    }
}

impl std::error::Error for ConptyError {}

/// A running pseudoconsole.  On non-Windows this type can be constructed only
/// to return `UnsupportedPlatform`; that behavior is intentional and prevents
/// a test or a CLI running on another OS from silently using a shell fallback.
pub struct ConptySession {
    #[cfg(windows)]
    inner: windows_impl::Session,
    #[cfg(not(windows))]
    _private: (),
}

impl ConptySession {
    pub fn start(config: ConptyConfig) -> Result<Self, ConptyError> {
        config.validate()?;
        #[cfg(windows)]
        {
            windows_impl::Session::start(config).map(|inner| Self { inner })
        }
        #[cfg(not(windows))]
        {
            let _ = config;
            Err(ConptyError::UnsupportedPlatform)
        }
    }

    pub fn send_input(&self, input: &[u8]) -> Result<(), ConptyError> {
        if input.len() > MAX_INPUT_BYTES {
            return Err(ConptyError::InputTooLarge);
        }
        #[cfg(windows)]
        {
            self.inner.send_input(input)
        }
        #[cfg(not(windows))]
        {
            let _ = input;
            Err(ConptyError::UnsupportedPlatform)
        }
    }

    pub fn recv(&self, timeout: Duration) -> Result<Option<ConptyEvent>, ConptyError> {
        #[cfg(windows)]
        {
            self.inner.recv(timeout)
        }
        #[cfg(not(windows))]
        {
            let _ = timeout;
            Err(ConptyError::UnsupportedPlatform)
        }
    }

    /// Waits for the child.  A timeout terminates the child and closes the
    /// session; it never leaves a detached terminal process behind.
    pub fn wait(&mut self) -> Result<u32, ConptyError> {
        #[cfg(windows)]
        {
            self.inner.wait()
        }
        #[cfg(not(windows))]
        {
            Err(ConptyError::UnsupportedPlatform)
        }
    }

    pub fn shutdown(&mut self) -> Result<(), ConptyError> {
        #[cfg(windows)]
        {
            self.inner.shutdown()
        }
        #[cfg(not(windows))]
        {
            Err(ConptyError::UnsupportedPlatform)
        }
    }
}

impl Drop for ConptySession {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            let _ = self.inner.shutdown();
        }
    }
}

fn contains_nul(value: &OsStr) -> bool {
    value.to_string_lossy().contains('\0')
}

fn is_drive_absolute(path: &Path) -> bool {
    let value = path.to_string_lossy();
    let bytes = value.as_bytes();
    bytes.len() >= 3 && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/')
}

fn is_forbidden_root(path: &Path) -> bool {
    let value = path.to_string_lossy();
    value.starts_with(r"\\") || value.starts_with(r"//")
}

fn validate_absolute_executable(path: &Path) -> Result<(), ConptyError> {
    let value = path.to_string_lossy().to_ascii_lowercase();
    if !is_drive_absolute(path)
        || is_forbidden_root(path)
        || contains_nul(path.as_os_str())
        || !value.ends_with(".exe")
        || value.contains("\\.\\")
        || value.contains("\\?\\")
    {
        return Err(ConptyError::InvalidExecutable);
    }
    Ok(())
}

fn validate_absolute_directory(path: &Path) -> Result<(), ConptyError> {
    if !is_drive_absolute(path)
        || is_forbidden_root(path)
        || contains_nul(path.as_os_str())
        || path.to_string_lossy().contains("\\.\\")
        || path.to_string_lossy().contains("\\?\\")
    {
        return Err(ConptyError::InvalidWorkingDirectory);
    }
    Ok(())
}

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::ptr::null_mut;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::{self, JoinHandle};
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Console::{
        CreatePseudoConsole as WinCreatePseudoConsole, COORD as WinCoord,
    };
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows::Win32::System::Memory::{GetProcessHeap, HeapAlloc, HeapFree, HEAP_FLAGS};
    use windows::Win32::System::Threading::{
        CreateProcessW as WinCreateProcessW,
        DeleteProcThreadAttributeList as WinDeleteProcThreadAttributeList,
        InitializeProcThreadAttributeList as WinInitializeProcThreadAttributeList,
        UpdateProcThreadAttribute as WinUpdateProcThreadAttribute,
        CREATE_UNICODE_ENVIRONMENT as WIN_CREATE_UNICODE_ENVIRONMENT,
        EXTENDED_STARTUPINFO_PRESENT as WIN_EXTENDED_STARTUPINFO_PRESENT,
        LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION as WinProcessInformation,
        PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as WIN_PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
        STARTUPINFOEXW,
    };

    type RawHandle = *mut c_void;
    type RawHpc = isize;
    type Bool = i32;

    const FALSE: Bool = 0;
    const INVALID_HANDLE_VALUE: RawHandle = -1isize as RawHandle;
    const WAIT_OBJECT_0: u32 = 0;
    const WAIT_TIMEOUT: u32 = 258;
    const HANDLE_FLAG_INHERIT: u32 = 0x0000_0001;

    #[repr(C)]
    struct SecurityAttributes {
        length: u32,
        descriptor: *mut c_void,
        inherit_handle: Bool,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreatePipe(
            read: *mut RawHandle,
            write: *mut RawHandle,
            attrs: *const SecurityAttributes,
            size: u32,
        ) -> Bool;
        fn ReadFile(
            file: RawHandle,
            buffer: *mut u8,
            count: u32,
            read: *mut u32,
            overlapped: *mut c_void,
        ) -> Bool;
        fn WriteFile(
            file: RawHandle,
            buffer: *const u8,
            count: u32,
            written: *mut u32,
            overlapped: *mut c_void,
        ) -> Bool;
        fn SetHandleInformation(handle: RawHandle, mask: u32, flags: u32) -> Bool;
        fn CloseHandle(handle: RawHandle) -> Bool;
        fn GetLastError() -> u32;
        fn ClosePseudoConsole(pc: RawHpc);
        fn WaitForSingleObject(handle: RawHandle, timeout_ms: u32) -> u32;
        fn GetExitCodeProcess(handle: RawHandle, code: *mut u32) -> Bool;
        fn TerminateProcess(handle: RawHandle, code: u32) -> Bool;
    }

    struct Handle(RawHandle);

    unsafe impl Send for Handle {}
    unsafe impl Sync for Handle {}

    impl Handle {
        fn is_valid(&self) -> bool {
            !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE
        }
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            if self.is_valid() {
                // SAFETY: this wrapper owns the handle and closes it exactly once.
                unsafe { CloseHandle(self.0) };
            }
        }
    }

    enum InputMessage {
        Bytes(Vec<u8>),
    }

    pub struct Session {
        input: Option<mpsc::Sender<InputMessage>>,
        output: mpsc::Receiver<ConptyEvent>,
        process: Option<Handle>,
        job: Option<Handle>,
        pc: Option<RawHpc>,
        stop: std::sync::Arc<AtomicBool>,
        writer: Option<JoinHandle<()>>,
        reader: Option<JoinHandle<()>>,
        timeout: Duration,
        closed: bool,
    }

    // The session owns the raw handles and worker threads exclusively; moving
    // the owner between Tauri command calls does not share a handle concurrently.
    unsafe impl Send for Session {}

    impl Session {
        pub fn start(config: ConptyConfig) -> Result<Self, ConptyError> {
            let executable = config.executable.clone();
            let resolved_executable =
                std::fs::canonicalize(&executable).map_err(|_| ConptyError::InvalidExecutable)?;
            let cwd = config.cwd.clone();
            let resolved_cwd =
                std::fs::canonicalize(&cwd).map_err(|_| ConptyError::InvalidWorkingDirectory)?;
            if !resolved_executable.is_file()
                || resolved_executable
                    .symlink_metadata()
                    .map_err(|_| ConptyError::InvalidExecutable)?
                    .file_type()
                    .is_symlink()
            {
                return Err(ConptyError::InvalidExecutable);
            }
            if !resolved_cwd.is_dir()
                || resolved_cwd
                    .symlink_metadata()
                    .map_err(|_| ConptyError::InvalidWorkingDirectory)?
                    .file_type()
                    .is_symlink()
            {
                return Err(ConptyError::InvalidWorkingDirectory);
            }

            let mut input_read = null_mut();
            let mut input_write = null_mut();
            let mut output_read = null_mut();
            let mut output_write = null_mut();
            let security_attributes = SecurityAttributes {
                length: size_of::<SecurityAttributes>() as u32,
                descriptor: null_mut(),
                // ConPTY-facing ends must be inheritable; parent-owned ends
                // are marked non-inheritable immediately after creation.
                inherit_handle: 1,
            };
            // SAFETY: output pointers and security attributes remain valid for
            // the duration of both CreatePipe calls.
            if unsafe { CreatePipe(&mut input_read, &mut input_write, &security_attributes, 0) }
                == FALSE
            {
                return Err(api_error("CreatePipe", unsafe { GetLastError() }));
            }
            if unsafe { CreatePipe(&mut output_read, &mut output_write, &security_attributes, 0) }
                == FALSE
            {
                unsafe { CloseHandle(input_read) };
                unsafe { CloseHandle(input_write) };
                return Err(api_error("CreatePipe", unsafe { GetLastError() }));
            }
            if unsafe { SetHandleInformation(input_write, HANDLE_FLAG_INHERIT, 0) } == FALSE {
                let code = unsafe { GetLastError() };
                unsafe {
                    CloseHandle(input_read);
                    CloseHandle(input_write);
                    CloseHandle(output_read);
                    CloseHandle(output_write);
                }
                return Err(api_error("SetHandleInformation", code));
            }
            if unsafe { SetHandleInformation(output_read, HANDLE_FLAG_INHERIT, 0) } == FALSE {
                let code = unsafe { GetLastError() };
                unsafe {
                    CloseHandle(input_read);
                    CloseHandle(input_write);
                    CloseHandle(output_read);
                    CloseHandle(output_write);
                }
                return Err(api_error("SetHandleInformation", code));
            }

            let pc = match unsafe {
                WinCreatePseudoConsole(
                    WinCoord {
                        X: config.columns as i16,
                        Y: config.rows as i16,
                    },
                    HANDLE(input_read),
                    HANDLE(output_write),
                    0,
                )
            } {
                Ok(handle) => handle.0,
                Err(error) => {
                    unsafe { CloseHandle(input_read) };
                    unsafe { CloseHandle(output_write) };
                    unsafe { CloseHandle(input_write) };
                    unsafe { CloseHandle(output_read) };
                    return Err(ConptyError::ApiFailure {
                        operation: "CreatePseudoConsole",
                        code: error.code().0 as u32,
                    });
                }
            };
            if pc == 0 {
                unsafe { CloseHandle(input_read) };
                unsafe { CloseHandle(output_write) };
                unsafe { CloseHandle(input_write) };
                unsafe { CloseHandle(output_read) };
                return Err(ConptyError::ApiFailure {
                    operation: "CreatePseudoConsole",
                    code: 0,
                });
            }

            let mut attribute_bytes = 0usize;
            // SAFETY: first call intentionally asks Windows for the required size.
            let _ = unsafe {
                WinInitializeProcThreadAttributeList(None, 1, None, &mut attribute_bytes)
            };
            if attribute_bytes == 0 {
                unsafe { CloseHandle(input_read) };
                unsafe { CloseHandle(output_write) };
                unsafe { ClosePseudoConsole(pc) };
                unsafe { CloseHandle(input_write) };
                unsafe { CloseHandle(output_read) };
                return Err(api_error("InitializeProcThreadAttributeList", unsafe {
                    GetLastError()
                }));
            }
            let heap = match unsafe { GetProcessHeap() } {
                Ok(heap) => heap,
                Err(error) => {
                    unsafe { CloseHandle(input_read) };
                    unsafe { CloseHandle(output_write) };
                    unsafe { ClosePseudoConsole(pc) };
                    unsafe { CloseHandle(input_write) };
                    unsafe { CloseHandle(output_read) };
                    return Err(ConptyError::ApiFailure {
                        operation: "GetProcessHeap",
                        code: error.code().0 as u32,
                    });
                }
            };
            let attribute_storage = unsafe { HeapAlloc(heap, HEAP_FLAGS(0), attribute_bytes) };
            if attribute_storage.is_null() {
                unsafe { CloseHandle(input_read) };
                unsafe { CloseHandle(output_write) };
                unsafe { ClosePseudoConsole(pc) };
                unsafe { CloseHandle(input_write) };
                unsafe { CloseHandle(output_read) };
                return Err(ConptyError::ApiFailure {
                    operation: "HeapAlloc",
                    code: unsafe { GetLastError() },
                });
            }
            let attribute_list = LPPROC_THREAD_ATTRIBUTE_LIST(attribute_storage);
            if let Err(error) = unsafe {
                WinInitializeProcThreadAttributeList(
                    Some(attribute_list),
                    1,
                    None,
                    &mut attribute_bytes,
                )
            } {
                unsafe { CloseHandle(input_read) };
                unsafe { CloseHandle(output_write) };
                unsafe { ClosePseudoConsole(pc) };
                unsafe { CloseHandle(input_write) };
                unsafe { CloseHandle(output_read) };
                unsafe {
                    let _ = HeapFree(heap, HEAP_FLAGS(0), Some(attribute_storage));
                };
                return Err(ConptyError::ApiFailure {
                    operation: "InitializeProcThreadAttributeList",
                    code: error.code().0 as u32,
                });
            }
            if let Err(error) = unsafe {
                WinUpdateProcThreadAttribute(
                    attribute_list,
                    0,
                    WIN_PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                    Some(pc as *const c_void),
                    size_of::<RawHpc>(),
                    None,
                    None,
                )
            } {
                unsafe { CloseHandle(input_read) };
                unsafe { CloseHandle(output_write) };
                unsafe { WinDeleteProcThreadAttributeList(attribute_list) };
                unsafe {
                    let _ = HeapFree(heap, HEAP_FLAGS(0), Some(attribute_storage));
                };
                unsafe { ClosePseudoConsole(pc) };
                unsafe { CloseHandle(input_write) };
                unsafe { CloseHandle(output_read) };
                return Err(ConptyError::ApiFailure {
                    operation: "UpdateProcThreadAttribute",
                    code: error.code().0 as u32,
                });
            }

            let mut command_line = quote_windows_arg(&executable);
            for argument in &config.args {
                command_line.push(' ');
                command_line.push_str(&quote_windows_arg(Path::new(argument)));
            }
            let mut command_line_w = command_line.encode_utf16().collect::<Vec<_>>();
            command_line_w.push(0);
            let cwd_w = cwd
                .to_string_lossy()
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            let environment_w = environment_block(&config.environment)?;
            let mut startup = STARTUPINFOEXW::default();
            startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
            startup.lpAttributeList = attribute_list;
            let mut process_info = WinProcessInformation::default();
            let created = unsafe {
                WinCreateProcessW(
                    PCWSTR::null(),
                    Some(PWSTR(command_line_w.as_mut_ptr())),
                    None,
                    None,
                    false,
                    WIN_EXTENDED_STARTUPINFO_PRESENT | WIN_CREATE_UNICODE_ENVIRONMENT,
                    Some(environment_w.as_ptr() as *const c_void),
                    PCWSTR(cwd_w.as_ptr()),
                    &startup.StartupInfo,
                    &mut process_info,
                )
            };
            unsafe { WinDeleteProcThreadAttributeList(attribute_list) };
            unsafe {
                let _ = HeapFree(heap, HEAP_FLAGS(0), Some(attribute_storage));
            };
            if let Err(error) = created {
                unsafe { CloseHandle(input_read) };
                unsafe { CloseHandle(output_write) };
                unsafe { ClosePseudoConsole(pc) };
                unsafe { CloseHandle(input_write) };
                unsafe { CloseHandle(output_read) };
                return Err(ConptyError::ApiFailure {
                    operation: "CreateProcessW",
                    code: error.code().0 as u32,
                });
            }
            // The pseudoconsole has consumed the communication endpoints after
            // CreateProcessW; release this process's duplicate references now.
            unsafe { CloseHandle(input_read) };
            unsafe { CloseHandle(output_write) };

            let job_handle =
                unsafe { CreateJobObjectW(None, PCWSTR::null()) }.map_err(|error| {
                    unsafe {
                        TerminateProcess(process_info.hProcess.0, 1);
                        CloseHandle(process_info.hThread.0);
                        CloseHandle(process_info.hProcess.0);
                        ClosePseudoConsole(pc);
                        CloseHandle(input_write);
                        CloseHandle(output_read);
                    }
                    ConptyError::ApiFailure {
                        operation: "CreateJobObjectW",
                        code: error.code().0 as u32,
                    }
                })?;
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if let Err(error) = unsafe {
                SetInformationJobObject(
                    job_handle,
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as *const c_void,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            } {
                unsafe {
                    CloseHandle(job_handle.0);
                    TerminateProcess(process_info.hProcess.0, 1);
                    CloseHandle(process_info.hThread.0);
                    CloseHandle(process_info.hProcess.0);
                    ClosePseudoConsole(pc);
                    CloseHandle(input_write);
                    CloseHandle(output_read);
                }
                return Err(ConptyError::ApiFailure {
                    operation: "SetInformationJobObject",
                    code: error.code().0 as u32,
                });
            }
            if let Err(error) =
                unsafe { AssignProcessToJobObject(job_handle, process_info.hProcess) }
            {
                unsafe {
                    CloseHandle(job_handle.0);
                    TerminateProcess(process_info.hProcess.0, 1);
                    CloseHandle(process_info.hThread.0);
                    CloseHandle(process_info.hProcess.0);
                    ClosePseudoConsole(pc);
                    CloseHandle(input_write);
                    CloseHandle(output_read);
                }
                return Err(ConptyError::ApiFailure {
                    operation: "AssignProcessToJobObject",
                    code: error.code().0 as u32,
                });
            }
            unsafe { CloseHandle(process_info.hThread.0) };

            let stop = std::sync::Arc::new(AtomicBool::new(false));
            let (input_tx, input_rx) = mpsc::channel();
            let (event_tx, event_rx) = mpsc::channel();
            let output_limit = config.output_limit_bytes;
            let timeout = config.timeout;
            let writer_stop = stop.clone();
            let input_handle = Handle(input_write);
            let writer = thread::spawn(move || writer_loop(input_handle, input_rx, writer_stop));
            let reader_stop = stop.clone();
            let output_handle = Handle(output_read);
            let reader = thread::spawn(move || {
                reader_loop(output_handle, event_tx, reader_stop, output_limit)
            });
            Ok(Self {
                input: Some(input_tx),
                output: event_rx,
                process: Some(Handle(process_info.hProcess.0)),
                job: Some(Handle(job_handle.0)),
                pc: Some(pc),
                stop,
                writer: Some(writer),
                reader: Some(reader),
                timeout,
                closed: false,
            })
        }

        pub fn send_input(&self, input: &[u8]) -> Result<(), ConptyError> {
            if self.closed {
                return Err(ConptyError::Closed);
            }
            self.input
                .as_ref()
                .ok_or(ConptyError::Closed)?
                .send(InputMessage::Bytes(input.to_vec()))
                .map_err(|_| ConptyError::Closed)
        }

        pub fn recv(&self, timeout: Duration) -> Result<Option<ConptyEvent>, ConptyError> {
            if self.closed {
                return Err(ConptyError::Closed);
            }
            match self.output.recv_timeout(timeout) {
                Ok(event) => {
                    if let ConptyEvent::Error(error) = &event {
                        // Reader failures (including the output budget) are
                        // terminal for this session. Stop the child before
                        // surfacing the error so a caller that only observes
                        // the failed poll cannot leave a process running.
                        let _ = self.terminate_child();
                        return Err(error.clone());
                    }
                    Ok(Some(event))
                }
                Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
                Err(mpsc::RecvTimeoutError::Disconnected) => Ok(Some(ConptyEvent::Closed)),
            }
        }

        pub fn wait(&mut self) -> Result<u32, ConptyError> {
            let process = self.process.as_ref().ok_or(ConptyError::Closed)?;
            let timeout_ms = self.timeout.as_millis().min(u32::MAX as u128) as u32;
            // SAFETY: process handle is owned by this session and remains open.
            let result = unsafe { WaitForSingleObject(process.0, timeout_ms) };
            if result == WAIT_TIMEOUT {
                let _ = self.terminate_child();
                let _ = self.shutdown();
                return Err(ConptyError::Timeout);
            }
            if result != WAIT_OBJECT_0 {
                return Err(api_error("WaitForSingleObject", unsafe { GetLastError() }));
            }
            let mut code = 0;
            if unsafe { GetExitCodeProcess(process.0, &mut code) } == FALSE {
                return Err(api_error("GetExitCodeProcess", unsafe { GetLastError() }));
            }
            let _ = self.shutdown();
            Ok(code)
        }

        pub fn shutdown(&mut self) -> Result<(), ConptyError> {
            if self.closed {
                return Ok(());
            }
            self.closed = true;
            self.stop.store(true, Ordering::Release);
            self.input.take();
            if let Some(pc) = self.pc.take() {
                // SAFETY: this is the one owner of the pseudoconsole handle.
                unsafe { ClosePseudoConsole(pc) };
            }
            if let Some(job) = self.job.as_ref() {
                // SAFETY: the job handle is owned by this session and contains
                // the child process tree created for this ConPTY session.
                let _ = unsafe { TerminateJobObject(HANDLE(job.0), 1) };
            }
            let _ = self.terminate_child();
            if let Some(writer) = self.writer.take() {
                let _ = writer.join();
            }
            if let Some(reader) = self.reader.take() {
                let _ = reader.join();
            }
            self.process.take();
            self.job.take();
            Ok(())
        }

        fn terminate_child(&self) -> Result<(), ConptyError> {
            if let Some(process) = self.process.as_ref() {
                // SAFETY: process handle is valid while Session owns it.
                if unsafe { TerminateProcess(process.0, 1) } == FALSE {
                    let code = unsafe { GetLastError() };
                    // Already exited is harmless during shutdown; all other
                    // failures are surfaced to the caller.
                    if code != 5 {
                        return Err(api_error("TerminateProcess", code));
                    }
                }
            }
            Ok(())
        }
    }

    fn writer_loop(
        handle: Handle,
        receiver: mpsc::Receiver<InputMessage>,
        stop: std::sync::Arc<AtomicBool>,
    ) {
        while !stop.load(Ordering::Acquire) {
            let message = match receiver.recv() {
                Ok(message) => message,
                Err(_) => break,
            };
            match message {
                InputMessage::Bytes(bytes) => {
                    let mut offset = 0usize;
                    while offset < bytes.len() && !stop.load(Ordering::Acquire) {
                        let remaining = (bytes.len() - offset).min(u32::MAX as usize);
                        let mut written = 0u32;
                        // SAFETY: handle and byte slice are valid for this call;
                        // anonymous pipes intentionally use synchronous I/O.
                        let ok = unsafe {
                            WriteFile(
                                handle.0,
                                bytes[offset..offset + remaining].as_ptr(),
                                remaining as u32,
                                &mut written,
                                null_mut(),
                            )
                        };
                        if ok == FALSE || written == 0 {
                            break;
                        }
                        offset += written as usize;
                    }
                }
            }
        }
    }

    fn reader_loop(
        handle: Handle,
        sender: mpsc::Sender<ConptyEvent>,
        stop: std::sync::Arc<AtomicBool>,
        output_limit: usize,
    ) {
        let mut total = 0usize;
        let mut buffer = vec![0u8; 8192];
        loop {
            if stop.load(Ordering::Acquire) {
                break;
            }
            let mut read = 0u32;
            // SAFETY: handle and buffer remain valid for the synchronous call.
            let ok = unsafe {
                ReadFile(
                    handle.0,
                    buffer.as_mut_ptr(),
                    buffer.len() as u32,
                    &mut read,
                    null_mut(),
                )
            };
            if ok == FALSE {
                let code = unsafe { GetLastError() };
                // ERROR_BROKEN_PIPE and ERROR_INVALID_HANDLE are expected
                // while shutdown closes the pseudoconsole endpoints.
                if code != 109 && code != 6 {
                    let _ = sender.send(ConptyEvent::Error(ConptyError::ApiFailure {
                        operation: "ReadFile",
                        code,
                    }));
                }
                break;
            }
            if read == 0 {
                break;
            }
            total = total.saturating_add(read as usize);
            if total > output_limit {
                let _ = sender.send(ConptyEvent::Error(ConptyError::OutputLimitExceeded));
                break;
            }
            if sender
                .send(ConptyEvent::Output(buffer[..read as usize].to_vec()))
                .is_err()
            {
                break;
            }
        }
        let _ = sender.send(ConptyEvent::Closed);
    }

    fn api_error(operation: &'static str, code: u32) -> ConptyError {
        ConptyError::ApiFailure { operation, code }
    }

    fn environment_block(
        environment: &BTreeMap<OsString, OsString>,
    ) -> Result<Vec<u16>, ConptyError> {
        let mut entries = environment.iter().collect::<Vec<_>>();
        entries.sort_by_key(|(key, _)| key.to_string_lossy().to_ascii_uppercase());
        let mut block = Vec::new();
        for (key, value) in entries {
            block.extend(key.to_string_lossy().encode_utf16());
            block.push('=' as u16);
            block.extend(value.to_string_lossy().encode_utf16());
            block.push(0);
        }
        block.push(0);
        if block.len() == 1 {
            // An empty environment block is represented by two terminators.
            block.push(0);
        }
        Ok(block)
    }

    fn quote_windows_arg(path: &Path) -> String {
        quote_windows_text(&path.to_string_lossy())
    }

    pub(super) fn quote_windows_text(text: &str) -> String {
        if !text.is_empty() && !text.chars().any(|ch| ch.is_whitespace() || ch == '"') {
            return text.to_owned();
        }
        let mut result = String::with_capacity(text.len() + 2);
        result.push('"');
        let mut backslashes = 0usize;
        for character in text.chars() {
            if character == '\\' {
                backslashes += 1;
            } else if character == '"' {
                result.push_str(&"\\".repeat(backslashes * 2 + 1));
                result.push('"');
                backslashes = 0;
            } else {
                if backslashes > 0 {
                    result.push_str(&"\\".repeat(backslashes));
                    backslashes = 0;
                }
                result.push(character);
            }
        }
        if backslashes > 0 {
            result.push_str(&"\\".repeat(backslashes * 2));
        }
        result.push('"');
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_config() -> ConptyConfig {
        ConptyConfig {
            executable: PathBuf::from(r"C:\Windows\System32\cmd.exe"),
            cwd: PathBuf::from(r"C:\Windows"),
            environment: BTreeMap::from([(
                OsString::from("SystemRoot"),
                OsString::from(r"C:\Windows"),
            )]),
            ..ConptyConfig::default()
        }
    }

    #[test]
    fn validates_explicit_absolute_executable_and_cwd() {
        assert!(valid_config().validate().is_ok());
    }

    #[test]
    fn rejects_relative_or_script_executables() {
        let mut config = valid_config();
        config.executable = PathBuf::from("cmd.exe");
        assert_eq!(config.validate(), Err(ConptyError::InvalidExecutable));
        config.executable = PathBuf::from(r"C:\temp\start.cmd");
        assert_eq!(config.validate(), Err(ConptyError::InvalidExecutable));
    }

    #[test]
    fn rejects_unc_device_and_invalid_environment() {
        let mut config = valid_config();
        config.executable = PathBuf::from(r"\\server\share\cmd.exe");
        assert_eq!(config.validate(), Err(ConptyError::InvalidExecutable));
        config.executable = PathBuf::from(r"C:\Windows\System32\cmd.exe");
        config
            .environment
            .insert(OsString::from("A=B"), OsString::from("x"));
        assert_eq!(config.validate(), Err(ConptyError::InvalidEnvironmentKey));
    }

    #[test]
    fn rejects_unbounded_output_and_timeout() {
        let mut config = valid_config();
        config.output_limit_bytes = MAX_OUTPUT_BYTES + 1;
        assert_eq!(config.validate(), Err(ConptyError::InvalidOutputLimit));
        config.output_limit_bytes = 1024;
        config.timeout = MAX_SESSION_TIMEOUT + Duration::from_millis(1);
        assert_eq!(config.validate(), Err(ConptyError::InvalidTimeout));
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_start_fails_closed() {
        let error = match ConptySession::start(valid_config()) {
            Err(error) => error,
            Ok(_) => panic!("non-Windows must not spawn a shell"),
        };
        assert_eq!(error, ConptyError::UnsupportedPlatform);
    }

    #[cfg(windows)]
    #[test]
    fn command_line_quoting_preserves_trailing_backslashes() {
        assert_eq!(
            windows_impl::quote_windows_text(r"C:\Program Files\"),
            r#""C:\Program Files\\""#
        );
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "console-host ConPTY output is covered by the GUI-subsystem smoke; run explicitly when diagnosing the host"]
    fn live_conpty_child_io_and_job_cleanup() {
        let mut config = valid_config();
        // Keep the child alive for a short, deterministic interval so the
        // synchronous reader worker can observe the pseudoconsole output
        // before cmd.exe exits and closes its side of the pipe.  A bare
        // `cmd /C echo` is too short-lived under a console test host and can
        // race the reader even though the GUI-subsystem smoke is healthy.
        config.args = vec![
            "/D".into(),
            "/C".into(),
            "ping -n 2 127.0.0.1 >nul & echo MENV_CONPTY_OK".into(),
        ];
        config.environment = std::env::vars_os().collect();
        config.timeout = Duration::from_secs(10);
        config.output_limit_bytes = 64 * 1024;
        let mut session = ConptySession::start(config).expect("start ConPTY child");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut output = Vec::new();
        while !String::from_utf8_lossy(&output).contains("MENV_CONPTY_OK") {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                panic!("ConPTY child did not produce command output before the test deadline");
            }
            match session
                .recv(remaining.min(Duration::from_millis(250)))
                .expect("receive initial ConPTY event")
            {
                Some(ConptyEvent::Output(bytes)) => output.extend(bytes),
                Some(ConptyEvent::Closed) => panic!("ConPTY output closed before marker"),
                Some(ConptyEvent::Exited(_)) => {}
                Some(ConptyEvent::Error(error)) => panic!("ConPTY error: {error}"),
                None => {}
            }
        }
        assert!(String::from_utf8_lossy(&output).contains("MENV_CONPTY_OK"));
        assert_eq!(session.wait().expect("wait ConPTY child"), 0);
        session.shutdown().expect("shutdown ConPTY session");
    }

    #[cfg(windows)]
    #[test]
    fn live_conpty_output_limit_terminates_the_child() {
        let mut config = valid_config();
        config.args = vec!["/C".into(), "echo MENV_CONPTY_OUTPUT_LIMIT".into()];
        config.environment = std::env::vars_os().collect();
        config.timeout = Duration::from_secs(10);
        config.output_limit_bytes = 8;
        let mut session = ConptySession::start(config).expect("start ConPTY child");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut saw_limit = false;
        while std::time::Instant::now() < deadline {
            match session.recv(Duration::from_millis(250)) {
                Err(ConptyError::OutputLimitExceeded) => {
                    saw_limit = true;
                    break;
                }
                Ok(Some(ConptyEvent::Closed)) => break,
                Ok(Some(ConptyEvent::Output(_))) | Ok(None) => {}
                Ok(Some(ConptyEvent::Exited(_))) => break,
                Ok(Some(ConptyEvent::Error(error))) => panic!("unexpected ConPTY event: {error}"),
                Err(error) => panic!("unexpected ConPTY error: {error}"),
            }
        }
        assert!(saw_limit, "output budget did not terminate the session");
        let _ = session.wait();
        session.shutdown().expect("shutdown limited ConPTY session");
    }

    #[cfg(windows)]
    #[test]
    fn live_conpty_job_terminates_a_spawned_child_tree() {
        let root =
            std::env::temp_dir().join(format!("cxszn-menv-conpty-tree-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create child-tree test directory");
        let started = root.join("started.txt");
        let survived = root.join("survived.txt");
        let child_script = root.join("child.cmd");
        let parent_script = root.join("parent.cmd");
        std::fs::write(
            &child_script,
            format!(
                "@echo off\r\nping 127.0.0.1 -n 3 >NUL\r\necho survived>\"{}\"\r\n",
                survived.display()
            ),
        )
        .expect("write child script");
        std::fs::write(
            &parent_script,
            format!(
                "@echo off\r\necho started>\"{}\"\r\nstart \"\" /B \"%ComSpec%\" /C call \"{}\"\r\nexit /B 0\r\n",
                started.display(),
                child_script.display()
            ),
        )
        .expect("write parent script");

        let mut config = valid_config();
        config.args = vec![
            "/C".into(),
            "call".into(),
            parent_script.as_os_str().to_owned(),
        ];
        config.environment = std::env::vars_os().collect();
        config.timeout = Duration::from_secs(10);
        config.output_limit_bytes = 64 * 1024;
        let mut session = ConptySession::start(config).expect("start ConPTY parent");
        let _parent_exit = session.wait().expect("wait ConPTY parent");

        let started_deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !started.exists() && std::time::Instant::now() < started_deadline {
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(started.is_file(), "spawned child never started");
        std::thread::sleep(Duration::from_secs(3));
        assert!(!survived.exists(), "spawned child outlived the ConPTY job");
        session.shutdown().expect("shutdown child-tree session");
        std::fs::remove_dir_all(&root).expect("remove child-tree test directory");
    }

    #[cfg(windows)]
    #[test]
    fn live_conpty_shutdown_cancels_parent_and_child_tree() {
        let root = std::env::temp_dir().join(format!(
            "cxszn-menv-conpty-cancel-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create cancel test directory");
        let started = root.join("started.txt");
        let survived = root.join("survived.txt");
        let child_script = root.join("child.cmd");
        let parent_script = root.join("parent.cmd");
        std::fs::write(
            &child_script,
            format!(
                "@echo off\r\nping 127.0.0.1 -n 10 >NUL\r\necho survived>\"{}\"\r\n",
                survived.display()
            ),
        )
        .expect("write cancel child script");
        std::fs::write(
            &parent_script,
            format!(
                "@echo off\r\necho started>\"{}\"\r\nstart \"\" /B \"%ComSpec%\" /C call \"{}\"\r\nping 127.0.0.1 -n 30 >NUL\r\n",
                started.display(),
                child_script.display()
            ),
        )
        .expect("write cancel parent script");

        let mut config = valid_config();
        config.args = vec![
            "/C".into(),
            "call".into(),
            parent_script.as_os_str().to_owned(),
        ];
        config.environment = std::env::vars_os().collect();
        config.timeout = Duration::from_secs(30);
        config.output_limit_bytes = 64 * 1024;
        let mut session = ConptySession::start(config).expect("start cancellable parent");
        let started_deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !started.exists() && std::time::Instant::now() < started_deadline {
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(started.is_file(), "cancellable child never started");
        session.shutdown().expect("cancel ConPTY session");
        std::thread::sleep(Duration::from_secs(3));
        assert!(
            !survived.exists(),
            "cancelled child outlived the ConPTY job"
        );
        std::fs::remove_dir_all(&root).expect("remove cancel test directory");
    }
}
