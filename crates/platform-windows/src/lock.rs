//! Cross-process user-scoped operation locking for Windows mutations.

use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum OperationLockError {
    #[error("operation locks are only available on Windows")]
    UnsupportedPlatform,
    #[error("operation lock name is invalid")]
    InvalidName,
    #[error("another MEnv operation is already running")]
    Timeout,
    #[error("Windows operation lock API {operation} failed with error {code}")]
    Api { operation: &'static str, code: u32 },
}

#[derive(Debug)]
pub struct WindowsOperationLock {
    #[cfg(windows)]
    handle: *mut std::ffi::c_void,
}

#[cfg(windows)]
unsafe impl Send for WindowsOperationLock {}

pub fn acquire_runtime_operation_lock(
    timeout: Duration,
) -> Result<WindowsOperationLock, OperationLockError> {
    acquire_named("Local\\cxszn-menv-runtime-operation", timeout)
}

pub fn acquire_named(
    name: &str,
    timeout: Duration,
) -> Result<WindowsOperationLock, OperationLockError> {
    #[cfg(not(windows))]
    {
        let _ = (name, timeout);
        return Err(OperationLockError::UnsupportedPlatform);
    }
    #[cfg(windows)]
    {
        windows_impl::acquire(name, timeout)
    }
}

#[cfg(windows)]
impl Drop for WindowsOperationLock {
    fn drop(&mut self) {
        windows_impl::release(self.handle);
    }
}

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use std::ffi::c_void;

    type RawHandle = *mut c_void;
    const WAIT_OBJECT_0: u32 = 0;
    const WAIT_ABANDONED: u32 = 0x0000_0080;
    const WAIT_TIMEOUT: u32 = 0x0000_0102;
    const WAIT_FAILED: u32 = 0xffff_ffff;

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateMutexW(
            attributes: *const c_void,
            initial_owner: i32,
            name: *const u16,
        ) -> RawHandle;
        fn WaitForSingleObject(handle: RawHandle, timeout_ms: u32) -> u32;
        fn ReleaseMutex(handle: RawHandle) -> i32;
        fn CloseHandle(handle: RawHandle) -> i32;
        fn GetLastError() -> u32;
    }

    pub(super) fn acquire(
        name: &str,
        timeout: Duration,
    ) -> Result<WindowsOperationLock, OperationLockError> {
        if name.is_empty() || name.contains('\0') || name.len() > 240 {
            return Err(OperationLockError::InvalidName);
        }
        let name = name
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(OperationLockError::Api {
                operation: "CreateMutexW",
                code: unsafe { GetLastError() },
            });
        }
        let timeout_ms = timeout.as_millis().min(u32::MAX as u128) as u32;
        let wait = unsafe { WaitForSingleObject(handle, timeout_ms) };
        if wait == WAIT_OBJECT_0 || wait == WAIT_ABANDONED {
            return Ok(WindowsOperationLock { handle });
        }
        unsafe { CloseHandle(handle) };
        if wait == WAIT_TIMEOUT {
            return Err(OperationLockError::Timeout);
        }
        Err(OperationLockError::Api {
            operation: "WaitForSingleObject",
            code: if wait == WAIT_FAILED {
                unsafe { GetLastError() }
            } else {
                wait
            },
        })
    }

    pub(super) fn release(handle: RawHandle) {
        if !handle.is_null() {
            unsafe {
                let _ = ReleaseMutex(handle);
                let _ = CloseHandle(handle);
            }
        }
    }
}

#[cfg(windows)]
#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn a_second_process_scope_waits_then_times_out_until_owner_releases() {
        let name = format!("Local\\cxszn-menv-test-{}", std::process::id());
        let owner = acquire_named(&name, Duration::from_millis(100)).expect("owner lock");
        let contender_name = name.clone();
        let contender = thread::spawn(move || {
            acquire_named(&contender_name, Duration::from_millis(50))
                .expect_err("contender should time out")
        });
        assert_eq!(
            contender.join().expect("contender thread"),
            OperationLockError::Timeout
        );
        drop(owner);
        acquire_named(&name, Duration::from_millis(100)).expect("lock after release");
    }
}
