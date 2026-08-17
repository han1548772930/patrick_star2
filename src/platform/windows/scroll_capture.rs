use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicI8, AtomicI32, AtomicPtr, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_ESCAPE, VK_RETURN};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DestroyWindow, DispatchMessageW, GetMessageW, HC_ACTION,
    HHOOK, HWND_MESSAGE, MSG, PostMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_APP, WM_KEYDOWN, WM_MOUSEWHEEL,
    WM_SYSKEYDOWN,
};

use crate::model::{PointI, RectI, ScrollAction};
use crate::platform::{
    ActiveScrollCapture, CapturedScrollFrame, ScrollCaptureEvent, ScrollCaptureIntent,
    ScrollDirection,
};

use super::capture::RegionCapture;

const MESSAGE_FRAME: u32 = WM_APP + 1;
const MESSAGE_FINISH: u32 = WM_APP + 2;
const MESSAGE_CANCEL: u32 = WM_APP + 3;
const FINISH_EDIT: WPARAM = 1;
const FINISH_SAVE: WPARAM = 2;
const FINISH_CLIPBOARD: WPARAM = 3;

static ACTIVE_WINDOW: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static ACTIVE_LEFT: AtomicI32 = AtomicI32::new(0);
static ACTIVE_TOP: AtomicI32 = AtomicI32::new(0);
static ACTIVE_RIGHT: AtomicI32 = AtomicI32::new(0);
static ACTIVE_BOTTOM: AtomicI32 = AtomicI32::new(0);
static WHEEL_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static WHEEL_DIRECTION: AtomicI8 = AtomicI8::new(0);

type FrameResult = std::result::Result<CapturedScrollFrame, String>;

struct LatestFrameSlot {
    frame: Mutex<Option<FrameResult>>,
    closed: AtomicBool,
}

impl LatestFrameSlot {
    fn new() -> Self {
        Self {
            frame: Mutex::new(None),
            closed: AtomicBool::new(false),
        }
    }

    fn replace(&self, frame: FrameResult) -> std::result::Result<(), ()> {
        if self.closed.load(Ordering::Acquire) {
            return Err(());
        }
        let mut slot = self.frame.lock().map_err(|_| ())?;
        *slot = Some(frame);
        Ok(())
    }

    fn take(&self) -> Option<FrameResult> {
        self.frame.lock().ok().and_then(|mut slot| slot.take())
    }

    fn close(&self) {
        self.closed.store(true, Ordering::Release);
        if let Ok(mut slot) = self.frame.lock() {
            *slot = None;
        }
    }
}

pub fn start(bounds: RectI) -> Result<Box<dyn ActiveScrollCapture>> {
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
    set_active_bounds(bounds);
    WHEEL_DIRECTION.store(0, Ordering::Release);

    let module = unsafe { GetModuleHandleW(null()) };
    let mouse_hook = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), module, 0) };
    if mouse_hook.is_null() {
        cleanup_failed_start(hwnd, null_mut(), null_mut());
        anyhow::bail!("install scroll mouse hook failed");
    }
    let keyboard_hook =
        unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), module, 0) };
    if keyboard_hook.is_null() {
        cleanup_failed_start(hwnd, mouse_hook, null_mut());
        anyhow::bail!("install scroll keyboard hook failed");
    }

    let frames = Arc::new(LatestFrameSlot::new());
    let notified = Arc::new(AtomicBool::new(false));
    let stop = Arc::new(AtomicBool::new(false));
    let worker_frames = frames.clone();
    let worker_notified = notified.clone();
    let worker_stop = stop.clone();
    let worker_hwnd = hwnd as usize;
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let grab_thread = match std::thread::Builder::new()
        .name("scroll-capture-grab".to_owned())
        .spawn(move || {
            let mut capture = match RegionCapture::new(bounds) {
                Ok(capture) => capture,
                Err(error) => {
                    let _ = ready_tx.send(Err(error.to_string()));
                    return;
                }
            };
            if ready_tx.send(Ok(())).is_err() {
                return;
            }
            let hwnd = worker_hwnd as HWND;
            while !worker_stop.load(Ordering::Acquire) {
                let captured_at = Instant::now();
                let result = capture.capture_rgba().map(|frame| CapturedScrollFrame {
                    frame,
                    captured_at,
                    direction: current_direction(),
                    wheel_sequence: WHEEL_SEQUENCE.load(Ordering::Acquire),
                    native_scroll_position: None,
                    discontinuity: false,
                });
                if worker_frames
                    .replace(result.map_err(|error| error.to_string()))
                    .is_err()
                {
                    break;
                }
                if !worker_notified.swap(true, Ordering::AcqRel)
                    && unsafe { PostMessageW(hwnd, MESSAGE_FRAME, 0, 0) } == 0
                {
                    break;
                }
            }
        }) {
        Ok(thread) => thread,
        Err(error) => {
            cleanup_failed_start(hwnd, mouse_hook, keyboard_hook);
            return Err(error).context("spawn scroll capture grab worker");
        }
    };
    match ready_rx.recv() {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            stop.store(true, Ordering::Release);
            frames.close();
            let _ = grab_thread.join();
            cleanup_failed_start(hwnd, mouse_hook, keyboard_hook);
            anyhow::bail!(error);
        }
        Err(_) => {
            stop.store(true, Ordering::Release);
            frames.close();
            let _ = grab_thread.join();
            cleanup_failed_start(hwnd, mouse_hook, keyboard_hook);
            anyhow::bail!("scroll capture grab worker stopped during startup");
        }
    }

    Ok(Box::new(WindowsScrollCapture {
        hwnd,
        mouse_hook,
        keyboard_hook,
        frames,
        notified,
        stop,
        grab_thread: Some(grab_thread),
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
    frames: Arc<LatestFrameSlot>,
    notified: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    grab_thread: Option<std::thread::JoinHandle<()>>,
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
                    MESSAGE_FRAME => {
                        self.notified.store(false, Ordering::Release);
                        match self.frames.take() {
                            Some(Ok(frame)) => return Ok(ScrollCaptureEvent::Frame(frame)),
                            Some(Err(error)) => return Err(anyhow!(error)),
                            None => continue,
                        }
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
        self.stop.store(true, Ordering::Release);
        self.frames.close();
        if let Some(thread) = self.grab_thread.take() {
            let _ = thread.join();
        }
        unsafe {
            UnhookWindowsHookEx(self.keyboard_hook);
            UnhookWindowsHookEx(self.mouse_hook);
        }
        if ACTIVE_WINDOW
            .compare_exchange(
                self.hwnd.cast(),
                null_mut(),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            clear_active_bounds();
        }
        unsafe { DestroyWindow(self.hwnd) };
    }
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && wparam as u32 == WM_MOUSEWHEEL {
        let mouse = unsafe {
            &*(lparam as *const windows_sys::Win32::UI::WindowsAndMessaging::MSLLHOOKSTRUCT)
        };
        let point = PointI::new(mouse.pt.x, mouse.pt.y);
        if active_bounds_contain(point) {
            let delta = (mouse.mouseData >> 16) as u16 as i16;
            if delta != 0 {
                WHEEL_DIRECTION.store(if delta < 0 { 1 } else { -1 }, Ordering::Release);
                WHEEL_SEQUENCE.fetch_add(1, Ordering::AcqRel);
            }
        }
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

fn current_direction() -> ScrollDirection {
    match WHEEL_DIRECTION.load(Ordering::Acquire) {
        value if value < 0 => ScrollDirection::Up,
        value if value > 0 => ScrollDirection::Down,
        _ => ScrollDirection::Unknown,
    }
}

fn cleanup_failed_start(hwnd: HWND, mouse_hook: HHOOK, keyboard_hook: HHOOK) {
    unsafe {
        if !keyboard_hook.is_null() {
            UnhookWindowsHookEx(keyboard_hook);
        }
        if !mouse_hook.is_null() {
            UnhookWindowsHookEx(mouse_hook);
        }
    }
    ACTIVE_WINDOW.store(null_mut(), Ordering::Release);
    clear_active_bounds();
    unsafe { DestroyWindow(hwnd) };
}

fn post_active(message: u32, wparam: WPARAM, lparam: LPARAM) {
    let hwnd: HWND = ACTIVE_WINDOW.load(Ordering::Acquire).cast();
    if !hwnd.is_null() {
        unsafe { PostMessageW(hwnd, message, wparam, lparam) };
    }
}

fn set_active_bounds(bounds: RectI) {
    ACTIVE_LEFT.store(bounds.left, Ordering::Relaxed);
    ACTIVE_TOP.store(bounds.top, Ordering::Relaxed);
    ACTIVE_RIGHT.store(bounds.right(), Ordering::Relaxed);
    ACTIVE_BOTTOM.store(bounds.bottom(), Ordering::Release);
}

fn clear_active_bounds() {
    ACTIVE_BOTTOM.store(0, Ordering::Release);
    ACTIVE_RIGHT.store(0, Ordering::Relaxed);
    ACTIVE_TOP.store(0, Ordering::Relaxed);
    ACTIVE_LEFT.store(0, Ordering::Relaxed);
}

fn active_bounds_contain(point: PointI) -> bool {
    let bottom = ACTIVE_BOTTOM.load(Ordering::Acquire);
    let left = ACTIVE_LEFT.load(Ordering::Relaxed);
    let top = ACTIVE_TOP.load(Ordering::Relaxed);
    let right = ACTIVE_RIGHT.load(Ordering::Relaxed);
    point.x >= left && point.x < right && point.y >= top && point.y < bottom
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn captured(seed: u8) -> CapturedScrollFrame {
        CapturedScrollFrame {
            frame: crate::model::RgbaFrame::new(
                RectI::new(0, 0, 1, 1),
                vec![seed, seed, seed, 255],
            )
            .unwrap(),
            captured_at: Instant::now(),
            direction: ScrollDirection::Unknown,
            wheel_sequence: 0,
            native_scroll_position: None,
            discontinuity: false,
        }
    }

    #[test]
    fn latest_frame_slot_overwrites_an_unconsumed_frame() {
        let slot = LatestFrameSlot::new();
        slot.replace(Ok(captured(1))).unwrap();
        slot.replace(Ok(captured(2))).unwrap();
        assert_eq!(slot.take().unwrap().unwrap().frame.pixels()[0], 2);
        assert!(slot.take().is_none());
    }
}
