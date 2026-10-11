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
    use std::mem::{size_of, zeroed};
    use std::ptr::{null, null_mut};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::{self, JoinHandle};

    type RawHandle = *mut c_void;
    type RawHpc = *mut c_void;
    type Bool = i32;

    const FALSE: Bool = 0;
    const INVALID_HANDLE_VALUE: RawHandle = -1isize as RawHandle;
    const EXTENDED_STARTUPINFO_PRESENT: u32 = 0x0008_0000;
    const CREATE_UNICODE_ENVIRONMENT: u32 = 0x0000_0400;
    const PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE: usize = 0x0002_0016;
    const WAIT_OBJECT_0: u32 = 0;
    const WAIT_TIMEOUT: u32 = 258;

    #[repr(C)]
    struct Coord {
        x: i16,
        y: i16,
    }

    #[repr(C)]
    struct SecurityAttributes {
        length: u32,
        descriptor: *mut c_void,
        inherit_handle: Bool,
    }

    #[repr(C)]
    struct StartupInfoW {
        cb: u32,
        reserved: *mut u16,
        desktop: *mut u16,
        title: *mut u16,
        x: u32,
        y: u32,
        x_size: u32,
        y_size: u32,
        x_count_chars: u32,
        y_count_chars: u32,
        fill_attribute: u32,
        flags: u32,
        show_window: u16,
        cb_reserved_2: u16,
        reserved_2: *mut u8,
        std_input: RawHandle,
        std_output: RawHandle,
        std_error: RawHandle,
    }

    #[repr(C)]
    struct StartupInfoExW {
        startup_info: StartupInfoW,
        attribute_list: *mut c_void,
    }

    #[repr(C)]
    struct ProcessInformation {
        process: RawHandle,
        thread: RawHandle,
        process_id: u32,
        thread_id: u32,
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
        fn CloseHandle(handle: RawHandle) -> Bool;
        fn GetLastError() -> u32;
        fn CreatePseudoConsole(
            size: Coord,
            input: RawHandle,
            output: RawHandle,
            flags: u32,
            pc: *mut RawHpc,
        ) -> i32;
        fn ClosePseudoConsole(pc: RawHpc);
        fn InitializeProcThreadAttributeList(
            list: *mut c_void,
            count: u32,
            flags: u32,
            size: *mut usize,
        ) -> Bool;
        fn UpdateProcThreadAttribute(
            list: *mut c_void,
            flags: u32,
            attribute: usize,
            value: *const c_void,
            size: usize,
            previous: *mut c_void,
            return_size: *const usize,
        ) -> Bool;
        fn DeleteProcThreadAttributeList(list: *mut c_void);
        fn CreateProcessW(
            application: *const u16,
            command_line: *mut u16,
            process_attrs: *const SecurityAttributes,
            thread_attrs: *const SecurityAttributes,
            inherit: Bool,
            flags: u32,
            environment: *const c_void,
            cwd: *const u16,
            startup_info: *const StartupInfoW,
            process_info: *mut ProcessInformation,
        ) -> Bool;
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
        pc: Option<RawHpc>,
        stop: std::sync::Arc<AtomicBool>,
        writer: Option<JoinHandle<()>>,
        reader: Option<JoinHandle<()>>,
        timeout: Duration,
        closed: bool,
    }

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
            // SAFETY: all output pointers are valid for the duration of the calls.
            if unsafe { CreatePipe(&mut input_read, &mut input_write, null(), 0) } == FALSE {
                return Err(api_error("CreatePipe", unsafe { GetLastError() }));
            }
            if unsafe { CreatePipe(&mut output_read, &mut output_write, null(), 0) } == FALSE {
                unsafe { CloseHandle(input_read) };
                unsafe { CloseHandle(input_write) };
                return Err(api_error("CreatePipe", unsafe { GetLastError() }));
            }

            let mut pc = null_mut();
            let result = unsafe {
                CreatePseudoConsole(
                    Coord {
                        x: config.columns as i16,
                        y: config.rows as i16,
                    },
                    input_read,
                    output_write,
                    0,
                    &mut pc,
                )
            };
            unsafe { CloseHandle(input_read) };
            unsafe { CloseHandle(output_write) };
            if result < 0 || pc.is_null() {
                unsafe { CloseHandle(input_write) };
                unsafe { CloseHandle(output_read) };
                return Err(ConptyError::ApiFailure {
                    operation: "CreatePseudoConsole",
                    code: result as u32,
                });
            }

            let mut attribute_bytes = 0usize;
            // SAFETY: first call intentionally asks Windows for the required size.
            let first = unsafe {
                InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut attribute_bytes)
            };
            if first != FALSE || attribute_bytes == 0 {
                unsafe { ClosePseudoConsole(pc) };
                unsafe { CloseHandle(input_write) };
                unsafe { CloseHandle(output_read) };
                return Err(api_error("InitializeProcThreadAttributeList", unsafe {
                    GetLastError()
                }));
            }
            let words = (attribute_bytes + size_of::<usize>() - 1) / size_of::<usize>();
            let mut attribute_storage = vec![0usize; words];
            let attribute_list = attribute_storage.as_mut_ptr() as *mut c_void;
            if unsafe {
                InitializeProcThreadAttributeList(attribute_list, 1, 0, &mut attribute_bytes)
            } == FALSE
            {
                unsafe { ClosePseudoConsole(pc) };
                unsafe { CloseHandle(input_write) };
                unsafe { CloseHandle(output_read) };
                return Err(api_error("InitializeProcThreadAttributeList", unsafe {
                    GetLastError()
                }));
            }
            let updated = unsafe {
                UpdateProcThreadAttribute(
                    attribute_list,
                    0,
                    PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
                    &pc as *const RawHpc as *const c_void,
                    size_of::<RawHpc>(),
                    null_mut(),
                    null(),
                )
            };
            if updated == FALSE {
                unsafe { DeleteProcThreadAttributeList(attribute_list) };
                unsafe { ClosePseudoConsole(pc) };
                unsafe { CloseHandle(input_write) };
                unsafe { CloseHandle(output_read) };
                return Err(api_error("UpdateProcThreadAttribute", unsafe {
                    GetLastError()
                }));
            }

            let mut command_line = quote_windows_arg(&executable);
            for argument in &config.args {
                command_line.push(' ');
                command_line.push_str(&quote_windows_arg(Path::new(argument)));
            }
            let mut command_line_w = command_line.encode_utf16().collect::<Vec<_>>();
            command_line_w.push(0);
            let executable_w = executable
                .to_string_lossy()
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            let cwd_w = cwd
                .to_string_lossy()
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            let environment_w = environment_block(&config.environment)?;
            let mut startup = StartupInfoExW {
                startup_info: unsafe { zeroed() },
                attribute_list,
            };
            startup.startup_info.cb = size_of::<StartupInfoExW>() as u32;
            let mut process_info: ProcessInformation = unsafe { zeroed() };
            let created = unsafe {
                CreateProcessW(
                    executable_w.as_ptr(),
                    command_line_w.as_mut_ptr(),
                    null(),
                    null(),
                    FALSE,
                    EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
                    environment_w.as_ptr() as *const c_void,
                    cwd_w.as_ptr(),
                    &startup.startup_info as *const StartupInfoW,
                    &mut process_info,
                )
            };
            unsafe { DeleteProcThreadAttributeList(attribute_list) };
            if created == FALSE {
                unsafe { ClosePseudoConsole(pc) };
                unsafe { CloseHandle(input_write) };
                unsafe { CloseHandle(output_read) };
                return Err(api_error("CreateProcessW", unsafe { GetLastError() }));
            }
            unsafe { CloseHandle(process_info.thread) };

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
                process: Some(Handle(process_info.process)),
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
            let _ = self.terminate_child();
            if let Some(writer) = self.writer.take() {
                let _ = writer.join();
            }
            if let Some(reader) = self.reader.take() {
                let _ = reader.join();
            }
            self.process.take();
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
            if ok == FALSE || read == 0 {
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
}
