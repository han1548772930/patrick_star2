use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};

use anyhow::{Context, Result};
use windows_sys::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::Graphics::Dwm::DwmFlush;
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, PAINTSTRUCT, RDW_INVALIDATE, RDW_NOERASE, RDW_UPDATENOW, RedrawWindow,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CS_OWNDC, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetSystemMetrics,
    GetWindowLongPtrW, IDC_ARROW, IsWindow, LoadCursorW, RegisterClassExW, SM_CXVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SW_HIDE, SW_SHOWNOACTIVATE, SetWindowLongPtrW, ShowWindow, WM_CLOSE,
    WM_DESTROY, WM_ERASEBKGND, WM_PAINT, WM_SIZE, WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};

use crate::model::{DesktopFrame, RgbaFrame};
use crate::platform::ScrollPreview;
use crate::rendering::ScrollPreviewRenderer;
use crate::scroll::{PreviewPatch, PreviewRegion};

use super::{capture_exclusion, scroll_overlay, wgl};

const CLASS_NAME: &[u16] = &[
    'P' as u16, 'a' as u16, 't' as u16, 'r' as u16, 'i' as u16, 'c' as u16, 'k' as u16, 'S' as u16,
    't' as u16, 'a' as u16, 'r' as u16, '2' as u16, 'S' as u16, 'c' as u16, 'r' as u16, 'o' as u16,
    'l' as u16, 'l' as u16, 'P' as u16, 'r' as u16, 'e' as u16, 'v' as u16, 'i' as u16, 'e' as u16,
    'w' as u16, 0,
];

const PREVIEW_WIDTH: i32 = 280;
const PREVIEW_GAP: i32 = 12;

pub fn open(desktop: &DesktopFrame, initial: &RgbaFrame) -> Result<Box<dyn ScrollPreview>> {
    let overlay = scroll_overlay::open(desktop, initial.bounds())?;
    let right = open_right(initial)?;
    Ok(Box::new(WindowsScrollPreview { overlay, right }))
}

struct WindowsScrollPreview {
    overlay: Box<scroll_overlay::Window>,
    right: Box<RightPreview>,
}

impl ScrollPreview for WindowsScrollPreview {
    fn update(&mut self, patch: PreviewPatch<'_>) -> Result<()> {
        self.right.update(patch)
    }
}

impl Drop for WindowsScrollPreview {
    fn drop(&mut self) {
        unsafe { ReleaseCapture() };
        let overlay_was_visible = self.overlay.hide_for_close();
        let preview_was_visible = self.right.hide_for_close();
        if overlay_was_visible || preview_was_visible {
            unsafe {
                let _ = DwmFlush();
            }
        }
    }
}

fn open_right(initial: &RgbaFrame) -> Result<Box<RightPreview>> {
    register_class()?;
    let bounds = initial.bounds();
    let virtual_left = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let virtual_right = virtual_left + unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) };
    let preferred_right = bounds.right().saturating_add(PREVIEW_GAP);
    let left = if preferred_right.saturating_add(PREVIEW_WIDTH) <= virtual_right {
        preferred_right
    } else {
        bounds.left.saturating_sub(PREVIEW_WIDTH + PREVIEW_GAP)
    }
    .max(virtual_left);
    let height = i32::try_from(bounds.height())
        .context("scroll preview height exceeds i32")?
        .clamp(240, 720);
    let title = wide("Patrick Star Scroll Preview");
    let instance = unsafe { GetModuleHandleW(null()) };
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            CLASS_NAME.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            left,
            bounds.top,
            PREVIEW_WIDTH,
            height,
            null_mut(),
            null_mut(),
            instance,
            null(),
        )
    };
    anyhow::ensure!(!hwnd.is_null(), "create Windows scroll preview failed");
    if let Err(error) = capture_exclusion::apply(hwnd) {
        eprintln!("exclude scroll preview from capture failed: {error:#}");
    }

    let surface = match wgl::Surface::new(hwnd) {
        Ok(surface) => surface,
        Err(error) => {
            unsafe { DestroyWindow(hwnd) };
            return Err(error).context("create WGL surface for scroll preview");
        }
    };
    let renderer = match unsafe {
        ScrollPreviewRenderer::new(initial.width(), |name| surface.proc_address(name))
    } {
        Ok(renderer) => renderer,
        Err(error) => {
            drop(surface);
            unsafe { DestroyWindow(hwnd) };
            return Err(error).context("initialize scroll preview renderer");
        }
    };
    let mut preview = Box::new(RightPreview {
        hwnd,
        renderer: Some(renderer),
        surface: Some(surface),
        width: PREVIEW_WIDTH as u32,
        height: height as u32,
        visible: false,
        error: None,
    });
    unsafe {
        SetWindowLongPtrW(
            hwnd,
            GWLP_USERDATA,
            (&mut *preview as *mut RightPreview) as isize,
        );
    }
    preview.upload(PreviewPatch {
        document_width: initial.width(),
        document_height: initial.height(),
        region: PreviewRegion {
            top: 0,
            height: initial.height(),
        },
        rgba: initial.pixels(),
    })?;
    unsafe {
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    preview.visible = true;
    preview.redraw_now()?;
    unsafe {
        let _ = DwmFlush();
    }
    Ok(preview)
}

fn register_class() -> Result<()> {
    let instance = unsafe { GetModuleHandleW(null()) };
    anyhow::ensure!(
        !instance.is_null(),
        "GetModuleHandleW failed for scroll preview"
    );
    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: CS_OWNDC,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        lpszClassName: CLASS_NAME.as_ptr(),
        ..Default::default()
    };
    let atom = unsafe { RegisterClassExW(&class) };
    anyhow::ensure!(
        atom != 0 || unsafe { GetLastError() } == ERROR_CLASS_ALREADY_EXISTS,
        "register Windows scroll preview class failed"
    );
    Ok(())
}

struct RightPreview {
    hwnd: HWND,
    renderer: Option<ScrollPreviewRenderer>,
    surface: Option<wgl::Surface>,
    width: u32,
    height: u32,
    visible: bool,
    error: Option<anyhow::Error>,
}

impl RightPreview {
    fn render(&mut self) {
        let (Some(renderer), Some(surface)) = (&self.renderer, &self.surface) else {
            return;
        };
        if let Err(error) = surface.ensure_current() {
            self.error = Some(error.context("activate scroll preview OpenGL context"));
            return;
        }
        renderer.render(self.width, self.height);
        if let Err(error) = surface.present() {
            self.error = Some(error.context("present scroll preview"));
        }
    }
}

impl RightPreview {
    fn update(&mut self, patch: PreviewPatch<'_>) -> Result<()> {
        if let Some(error) = self.error.take() {
            return Err(error);
        }
        self.upload(patch)?;
        self.redraw_now()
    }

    fn upload(&mut self, patch: PreviewPatch<'_>) -> Result<()> {
        self.surface
            .as_ref()
            .context("scroll preview surface is closed")?
            .ensure_current()
            .context("activate scroll preview OpenGL context")?;
        self.renderer
            .as_mut()
            .context("scroll preview renderer is closed")?
            .update(patch)?;
        Ok(())
    }

    fn redraw_now(&mut self) -> Result<()> {
        anyhow::ensure!(
            unsafe {
                RedrawWindow(
                    self.hwnd,
                    null(),
                    null_mut(),
                    RDW_INVALIDATE | RDW_UPDATENOW | RDW_NOERASE,
                )
            } != 0,
            "redraw scroll preview failed"
        );
        if let Some(error) = self.error.take() {
            return Err(error);
        }
        Ok(())
    }

    fn hide_for_close(&mut self) -> bool {
        if !self.visible || unsafe { IsWindow(self.hwnd) } == 0 {
            return false;
        }
        unsafe { ShowWindow(self.hwnd, SW_HIDE) };
        self.visible = false;
        true
    }
}

impl Drop for RightPreview {
    fn drop(&mut self) {
        if self.hide_for_close() {
            unsafe {
                let _ = DwmFlush();
            }
        }
        unsafe { SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0) };
        if let Some(surface) = self.surface.as_ref() {
            let _ = surface.ensure_current();
        }
        self.renderer.take();
        self.surface.take();
        if unsafe { IsWindow(self.hwnd) } != 0 {
            unsafe { DestroyWindow(self.hwnd) };
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut RightPreview;
    if state.is_null() {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    let state = unsafe { &mut *state };
    match message {
        WM_PAINT => {
            let mut paint: PAINTSTRUCT = unsafe { zeroed() };
            unsafe { BeginPaint(hwnd, &mut paint) };
            state.render();
            unsafe { EndPaint(hwnd, &paint) };
            0
        }
        WM_ERASEBKGND => 1,
        WM_SIZE => {
            state.width = (lparam as u32 & 0xffff).max(1);
            state.height = ((lparam as u32 >> 16) & 0xffff).max(1);
            0
        }
        WM_CLOSE => 0,
        WM_DESTROY => 0,
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
