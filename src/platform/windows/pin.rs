use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::thread;

use anyhow::{Context, Result, anyhow};
use windows_sys::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT, UpdateWindow};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CS_DBLCLKS, CS_OWNDC, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GWLP_USERDATA, GetMessageW, GetWindowLongPtrW, HTCAPTION, IDC_ARROW, IsWindow, LoadCursorW,
    MSG, PostQuitMessage, RegisterClassExW, SW_SHOW, SetForegroundWindow, SetWindowLongPtrW,
    ShowWindow, TranslateMessage, WM_CLOSE, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_NCHITTEST,
    WM_NCLBUTTONDBLCLK, WM_NCRBUTTONUP, WM_PAINT, WM_SIZE, WNDCLASSEXW, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};

use crate::model::RgbaFrame;
use crate::rendering::PinnedImageRenderer;

use super::wgl;

const CLASS_NAME: &[u16] = &[
    'P' as u16, 'a' as u16, 't' as u16, 'r' as u16, 'i' as u16, 'c' as u16, 'k' as u16, 'S' as u16,
    't' as u16, 'a' as u16, 'r' as u16, '2' as u16, 'P' as u16, 'i' as u16, 'n' as u16, 0,
];

pub fn show(image: RgbaFrame) -> Result<()> {
    let (ready_sender, ready_receiver) = sync_channel(1);
    thread::Builder::new()
        .name("patrick-star-pin".into())
        .spawn(move || {
            let mut ready = Some(ready_sender);
            if let Err(error) = run(image, &mut ready) {
                if let Some(sender) = ready.take() {
                    let _ = sender.send(Err(format!("{error:#}")));
                } else {
                    eprintln!("pinned image window failed: {error:#}");
                }
            }
        })
        .context("spawn pinned image UI thread")?;

    match ready_receiver
        .recv()
        .context("pinned image UI thread stopped during initialization")?
    {
        Ok(()) => Ok(()),
        Err(error) => anyhow::bail!(error),
    }
}

fn run(image: RgbaFrame, ready: &mut Option<SyncSender<Result<(), String>>>) -> Result<()> {
    let instance = unsafe { GetModuleHandleW(null()) };
    anyhow::ensure!(
        !instance.is_null(),
        "GetModuleHandleW failed for pinned image"
    );
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: CS_OWNDC | CS_DBLCLKS,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        lpszClassName: CLASS_NAME.as_ptr(),
        ..Default::default()
    };
    let atom = unsafe { RegisterClassExW(&class) };
    anyhow::ensure!(
        atom != 0 || unsafe { GetLastError() } == ERROR_CLASS_ALREADY_EXISTS,
        "RegisterClassExW failed for pinned image"
    );

    let bounds = image.bounds();
    let width = i32::try_from(bounds.width()).context("pinned image width exceeds i32")?;
    let height = i32::try_from(bounds.height()).context("pinned image height exceeds i32")?;
    let title = wide("Patrick Star Pinned Image");
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            CLASS_NAME.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            bounds.left,
            bounds.top,
            width,
            height,
            null_mut(),
            null_mut(),
            instance,
            null(),
        )
    };
    anyhow::ensure!(!hwnd.is_null(), "CreateWindowExW failed for pinned image");

    let result = run_window(hwnd, &image, ready);
    if unsafe { IsWindow(hwnd) } != 0 {
        unsafe { DestroyWindow(hwnd) };
    }
    result
}

fn run_window(
    hwnd: HWND,
    image: &RgbaFrame,
    ready: &mut Option<SyncSender<Result<(), String>>>,
) -> Result<()> {
    let surface = wgl::Surface::new(hwnd).context("create WGL surface for pinned image")?;
    let renderer = unsafe {
        PinnedImageRenderer::new(image, |name| surface.proc_address(name))
            .context("initialize pinned image renderer")?
    };
    let mut state = Box::new(WindowState {
        renderer,
        surface,
        width: image.width(),
        height: image.height(),
        error: None,
    });
    unsafe {
        SetWindowLongPtrW(
            hwnd,
            GWLP_USERDATA,
            (&mut *state as *mut WindowState) as isize,
        );
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        UpdateWindow(hwnd);
    }

    if let Some(error) = state.error.take() {
        return Err(error);
    }
    ready
        .take()
        .expect("pinned image readiness is reported once")
        .send(Ok(()))
        .map_err(|_| anyhow!("pinned image caller stopped during initialization"))?;

    let loop_result = message_loop();
    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
    let error = state.error.take();
    drop(state);
    loop_result?;
    if let Some(error) = error {
        return Err(error);
    }
    Ok(())
}

fn message_loop() -> Result<()> {
    let mut message = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
        if result == -1 {
            return Err(anyhow!("GetMessageW failed for pinned image"));
        }
        if result == 0 {
            return Ok(());
        }
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

struct WindowState {
    // GPU resources must be dropped while this WGL surface is current.
    renderer: PinnedImageRenderer,
    surface: wgl::Surface,
    width: u32,
    height: u32,
    error: Option<anyhow::Error>,
}

impl WindowState {
    fn render(&mut self) {
        self.renderer.render(self.width.max(1), self.height.max(1));
        if let Err(error) = self.surface.present() {
            self.error = Some(error.context("present pinned image"));
            unsafe { PostQuitMessage(1) };
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut WindowState;
    if state_ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    let state = unsafe { &mut *state_ptr };
    match message {
        WM_PAINT => {
            let mut paint: PAINTSTRUCT = unsafe { zeroed() };
            unsafe { BeginPaint(hwnd, &mut paint) };
            state.render();
            unsafe { EndPaint(hwnd, &paint) };
            0
        }
        WM_ERASEBKGND => 1,
        WM_NCHITTEST => HTCAPTION as LRESULT,
        WM_NCLBUTTONDBLCLK | WM_NCRBUTTONUP | WM_CLOSE => {
            unsafe { PostQuitMessage(0) };
            0
        }
        WM_KEYDOWN if wparam as u16 == VK_ESCAPE => {
            unsafe { PostQuitMessage(0) };
            0
        }
        WM_SIZE => {
            state.width = (lparam as u32 & 0xffff).max(1);
            state.height = ((lparam as u32 >> 16) & 0xffff).max(1);
            0
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
