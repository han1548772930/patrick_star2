use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicPtr, Ordering};

use anyhow::{Result, anyhow};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_ESCAPE, VK_RETURN};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DestroyWindow, DispatchMessageW, GetMessageW, HC_ACTION,
    HHOOK, HWND_MESSAGE, KillTimer, MSG, PostMessageW, SetTimer, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_APP, WM_KEYDOWN,
    WM_MOUSEWHEEL, WM_SYSKEYDOWN, WM_TIMER,
};

use crate::model::{PointI, RectI, ScrollAction};
use crate::platform::{ActiveScrollCapture, ScrollCaptureEvent, ScrollCaptureIntent};

use super::capture::RegionCapture;

const MESSAGE_WHEEL: u32 = WM_APP + 1;
const MESSAGE_FINISH: u32 = WM_APP + 2;
const MESSAGE_CANCEL: u32 = WM_APP + 3;
const CAPTURE_TIMER: usize = 1;
const CAPTURE_SETTLE_MS: u32 = 80;
const FINISH_EDIT: WPARAM = 1;
const FINISH_SAVE: WPARAM = 2;
const FINISH_CLIPBOARD: WPARAM = 3;

static ACTIVE_WINDOW: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

pub fn start(bounds: RectI) -> Result<Box<dyn ActiveScrollCapture>> {
    let capture = RegionCapture::new(bounds)?;
    let class_name = wide("STATIC");
    let window_name = wide("Patrick Star Scroll Capture");
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            window_name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        )
    };
    anyhow::ensure!(
        !hwnd.is_null(),
        "create scroll capture message window failed"
    );
    if ACTIVE_WINDOW
        .compare_exchange(null_mut(), hwnd.cast(), Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        unsafe { DestroyWindow(hwnd) };
        anyhow::bail!("a scroll capture session is already active");
    }

    let module = unsafe { GetModuleHandleW(null()) };
    let mouse_hook = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), module, 0) };
    if mouse_hook.is_null() {
        ACTIVE_WINDOW.store(null_mut(), Ordering::Release);
        unsafe { DestroyWindow(hwnd) };
        anyhow::bail!("install scroll mouse hook failed");
    }
    let keyboard_hook =
        unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), module, 0) };
    if keyboard_hook.is_null() {
        unsafe { UnhookWindowsHookEx(mouse_hook) };
        ACTIVE_WINDOW.store(null_mut(), Ordering::Release);
        unsafe { DestroyWindow(hwnd) };
        anyhow::bail!("install scroll keyboard hook failed");
    }

    Ok(Box::new(WindowsScrollCapture {
        hwnd,
        mouse_hook,
        keyboard_hook,
        bounds,
        capture,
    }))
}

pub fn cancel_active() -> Result<()> {
    let hwnd: HWND = ACTIVE_WINDOW.load(Ordering::Acquire).cast();
    if hwnd.is_null() {
        return Ok(());
    }
    anyhow::ensure!(
        unsafe { PostMessageW(hwnd, MESSAGE_CANCEL, 0, 0) } != 0,
        "cancel active scroll capture failed"
    );
    Ok(())
}

pub(super) fn request(action: ScrollAction) {
    match action {
        ScrollAction::Edit => post_active(MESSAGE_FINISH, FINISH_EDIT, 0),
        ScrollAction::Save => post_active(MESSAGE_FINISH, FINISH_SAVE, 0),
        ScrollAction::Cancel => post_active(MESSAGE_CANCEL, 0, 0),
        ScrollAction::Confirm => post_active(MESSAGE_FINISH, FINISH_CLIPBOARD, 0),
    }
}

struct WindowsScrollCapture {
    hwnd: HWND,
    mouse_hook: HHOOK,
    keyboard_hook: HHOOK,
    bounds: RectI,
    capture: RegionCapture,
}

impl ActiveScrollCapture for WindowsScrollCapture {
    fn next_event(&mut self) -> Result<ScrollCaptureEvent> {
        let mut message = MSG::default();
        loop {
            let result = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
            if result == -1 {
                return Err(anyhow!("GetMessageW failed during scroll capture"));
            }
            if result == 0 {
                return Ok(ScrollCaptureEvent::Cancelled);
            }

            if message.hwnd == self.hwnd {
                match message.message {
                    MESSAGE_WHEEL => {
                        let point = PointI::new(message.wParam as i32, message.lParam as i32);
                        if !self.bounds.contains(point) {
                            continue;
                        }
                        unsafe { KillTimer(self.hwnd, CAPTURE_TIMER) };
                        anyhow::ensure!(
                            unsafe { SetTimer(self.hwnd, CAPTURE_TIMER, CAPTURE_SETTLE_MS, None) }
                                != 0,
                            "schedule scroll frame capture failed"
                        );
                        continue;
                    }
                    WM_TIMER if message.wParam == CAPTURE_TIMER => {
                        unsafe { KillTimer(self.hwnd, CAPTURE_TIMER) };
                        return Ok(ScrollCaptureEvent::Frame(self.capture.capture_rgba()?));
                    }
                    MESSAGE_FINISH => {
                        let intent = match message.wParam {
                            FINISH_EDIT => ScrollCaptureIntent::Edit,
                            FINISH_SAVE => ScrollCaptureIntent::Save,
                            FINISH_CLIPBOARD => ScrollCaptureIntent::Clipboard,
                            _ => continue,
                        };
                        return Ok(ScrollCaptureEvent::Finished(intent));
                    }
                    MESSAGE_CANCEL => return Ok(ScrollCaptureEvent::Cancelled),
                    _ => {}
                }
            }

            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
}

impl Drop for WindowsScrollCapture {
    fn drop(&mut self) {
        unsafe {
            KillTimer(self.hwnd, CAPTURE_TIMER);
            UnhookWindowsHookEx(self.keyboard_hook);
            UnhookWindowsHookEx(self.mouse_hook);
        }
        let _ = ACTIVE_WINDOW.compare_exchange(
            self.hwnd.cast(),
            null_mut(),
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        unsafe { DestroyWindow(self.hwnd) };
    }
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && wparam as u32 == WM_MOUSEWHEEL {
        let mouse = unsafe {
            &*(lparam as *const windows_sys::Win32::UI::WindowsAndMessaging::MSLLHOOKSTRUCT)
        };
        post_active(MESSAGE_WHEEL, mouse.pt.x as usize, mouse.pt.y as isize);
    }
    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN) {
        let key = unsafe {
            &*(lparam as *const windows_sys::Win32::UI::WindowsAndMessaging::KBDLLHOOKSTRUCT)
        };
        match key.vkCode as u16 {
            VK_RETURN => {
                post_active(MESSAGE_FINISH, FINISH_EDIT, 0);
                return 1;
            }
            VK_ESCAPE => {
                post_active(MESSAGE_CANCEL, 0, 0);
                return 1;
            }
            _ => {}
        }
    }
    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
}

fn post_active(message: u32, wparam: WPARAM, lparam: LPARAM) {
    let hwnd: HWND = ACTIVE_WINDOW.load(Ordering::Acquire).cast();
    if !hwnd.is_null() {
        unsafe { PostMessageW(hwnd, message, wparam, lparam) };
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
