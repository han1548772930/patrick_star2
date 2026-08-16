use std::ptr::null;

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows_sys::Win32::System::Threading::CreateMutexW;

use crate::platform::SingleInstanceGuard;

const INSTANCE_NAME: &str = "Local\\PatrickStar2.Application.5C427E13-946D-48A2-919D-48FFBF5ED31A";

struct Guard(HANDLE);

impl SingleInstanceGuard for Guard {}

impl Drop for Guard {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub fn acquire() -> anyhow::Result<Option<Box<dyn SingleInstanceGuard>>> {
    acquire_named(INSTANCE_NAME)
}

fn acquire_named(name: &str) -> anyhow::Result<Option<Box<dyn SingleInstanceGuard>>> {
    let name = name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let handle = unsafe { CreateMutexW(null(), 0, name.as_ptr()) };
    anyhow::ensure!(!handle.is_null(), "CreateMutexW failed: {}", unsafe {
        GetLastError()
    });
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            CloseHandle(handle);
        }
        return Ok(None);
    }
    Ok(Some(Box::new(Guard(handle))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_lock_is_exclusive_and_released_with_its_guard() {
        let name = format!(
            "Local\\PatrickStar2.Test.{}.{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("single-instance")
        );
        let first = acquire_named(&name).unwrap();
        assert!(first.is_some());
        assert!(acquire_named(&name).unwrap().is_none());
        drop(first);
        assert!(acquire_named(&name).unwrap().is_some());
    }
}
