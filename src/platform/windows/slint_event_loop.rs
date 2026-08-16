use std::collections::VecDeque;
use std::ptr::{null, null_mut};
use std::rc::Rc;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use slint::platform::{
    Clipboard, EventLoopProxy, Platform, PlatformError, WindowAdapter,
    duration_until_next_timer_update, update_timers_and_animations,
};
use windows_sys::Win32::Foundation::{WAIT_FAILED, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_NOREMOVE,
    PM_REMOVE, PeekMessageW, PostThreadMessageW, QS_ALLINPUT, TranslateMessage, WM_APP, WM_QUIT,
};

use super::hotkey::Host as HotkeyHost;

const WAKE_MESSAGE: u32 = WM_APP + 0x31A;

type Event = Box<dyn FnOnce() + Send>;

struct EventLoopState {
    thread_id: u32,
    events: Mutex<VecDeque<Event>>,
    terminated: AtomicBool,
}

impl EventLoopState {
    fn wake(&self) -> Result<(), slint::EventLoopError> {
        if self.terminated.load(Ordering::Acquire) {
            return Err(slint::EventLoopError::EventLoopTerminated);
        }
        let posted = unsafe { PostThreadMessageW(self.thread_id, WAKE_MESSAGE, 0, 0) };
        if posted == 0 {
            return Err(slint::EventLoopError::NoEventLoopProvider);
        }
        Ok(())
    }

    fn drain_events(&self) {
        loop {
            let event = self
                .events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .pop_front();
            let Some(event) = event else { break };
            event();
        }
    }
}

#[derive(Clone)]
struct WindowsEventLoopProxy(Arc<EventLoopState>);

impl EventLoopProxy for WindowsEventLoopProxy {
    fn quit_event_loop(&self) -> Result<(), slint::EventLoopError> {
        self.0.terminated.store(true, Ordering::Release);
        // WM_QUIT also unwinds a native modal/nested GetMessage loop, such as
        // the capture overlay. A private wake message cannot do that.
        let posted = unsafe { PostThreadMessageW(self.0.thread_id, WM_QUIT, 0, 0) };
        if posted == 0 {
            return Err(slint::EventLoopError::NoEventLoopProvider);
        }
        Ok(())
    }

    fn invoke_from_event_loop(&self, event: Event) -> Result<(), slint::EventLoopError> {
        if self.0.terminated.load(Ordering::Acquire) {
            return Err(slint::EventLoopError::EventLoopTerminated);
        }
        self.0
            .events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push_back(event);
        self.0.wake()
    }
}

pub struct WindowsSlintPlatform {
    state: Arc<EventLoopState>,
    hotkeys: Rc<HotkeyHost>,
}

impl WindowsSlintPlatform {
    pub fn new(hotkeys: Rc<HotkeyHost>) -> Self {
        // Creating the thread message queue before publishing the proxy makes
        // PostThreadMessageW reliable even before run_event_loop starts.
        let mut message = MSG::default();
        unsafe {
            PeekMessageW(&mut message, null_mut(), 0, 0, PM_NOREMOVE);
        }
        Self {
            state: Arc::new(EventLoopState {
                thread_id: unsafe { GetCurrentThreadId() },
                events: Mutex::new(VecDeque::new()),
                terminated: AtomicBool::new(false),
            }),
            hotkeys,
        }
    }

    fn wait_timeout_ms() -> u32 {
        duration_until_next_timer_update().map_or(u32::MAX, |duration| {
            let millis = duration.as_nanos().div_ceil(1_000_000);
            millis.min(u128::from(u32::MAX - 1)) as u32
        })
    }
}

impl Platform for WindowsSlintPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        super::slint_window::create()
    }

    fn run_event_loop(&self) -> Result<(), PlatformError> {
        if unsafe { GetCurrentThreadId() } != self.state.thread_id {
            return Err(PlatformError::Other(
                "the Slint event loop must run on the thread that installed the platform".into(),
            ));
        }

        self.state.terminated.store(false, Ordering::Release);
        while !self.state.terminated.load(Ordering::Acquire) {
            update_timers_and_animations();
            self.state.drain_events();
            if self.state.terminated.load(Ordering::Acquire) {
                break;
            }

            let wait_result = unsafe {
                MsgWaitForMultipleObjectsEx(
                    0,
                    null(),
                    Self::wait_timeout_ms(),
                    QS_ALLINPUT,
                    MWMO_INPUTAVAILABLE,
                )
            };
            if wait_result == WAIT_FAILED {
                return Err(PlatformError::Other(
                    "MsgWaitForMultipleObjectsEx failed".into(),
                ));
            }
            if wait_result == WAIT_TIMEOUT {
                continue;
            }

            let mut message = MSG::default();
            while unsafe { PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) } != 0 {
                if message.message == WM_QUIT {
                    self.state.terminated.store(true, Ordering::Release);
                    break;
                }
                if message.message == WAKE_MESSAGE {
                    continue;
                }
                if self.hotkeys.handle_message(message.message, message.wParam) {
                    continue;
                }
                unsafe {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
        }
        self.state.drain_events();
        Ok(())
    }

    fn new_event_loop_proxy(&self) -> Option<Box<dyn EventLoopProxy>> {
        Some(Box::new(WindowsEventLoopProxy(self.state.clone())))
    }

    fn set_clipboard_text(&self, text: &str, clipboard_kind: Clipboard) {
        if clipboard_kind != Clipboard::DefaultClipboard {
            return;
        }
        if let Err(error) = super::clipboard::write_text(text) {
            eprintln!("write Slint text selection to clipboard failed: {error:#}");
        }
    }
}
