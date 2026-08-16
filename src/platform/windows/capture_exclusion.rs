use windows_sys::Win32::Foundation::{GetLastError, HWND};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_MONITOR,
};

/// Keeps application chrome out of desktop capture. Older Windows releases
/// fall back to `WDA_MONITOR`, which masks the window instead of removing it.
pub fn apply(hwnd: HWND) -> anyhow::Result<()> {
    if unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) } != 0 {
        return Ok(());
    }
    let exclude_error = unsafe { GetLastError() };
    if unsafe { SetWindowDisplayAffinity(hwnd, WDA_MONITOR) } != 0 {
        return Ok(());
    }
    anyhow::bail!(
        "SetWindowDisplayAffinity failed (exclude error {exclude_error}, fallback error {})",
        unsafe { GetLastError() }
    )
}
