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
    CS_OWNDC, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetWindowLongPtrW,
    HWND_TOPMOST, IDC_ARROW, IsWindow, LoadCursorW, RegisterClassExW, SW_HIDE, SW_SHOWNOACTIVATE,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, WM_CLOSE, WM_DESTROY, WM_ERASEBKGND, WM_PAINT, WM_SIZE, WNDCLASSEXW,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::model::{DesktopFrame, RectI, RgbaFrame};
use crate::platform::ScrollPreview;
use crate::rendering::ScrollPreviewRenderer;
use crate::scroll::{PreviewPatch, PreviewRegion};

use super::{capture_exclusion, refresh_desktop_after_overlay, scroll_overlay, wgl};

const CLASS_NAME: &[u16] = &[
    'P' as u16, 'a' as u16, 't' as u16, 'r' as u16, 'i' as u16, 'c' as u16, 'k' as u16, 'S' as u16,
    't' as u16, 'a' as u16, 'r' as u16, '2' as u16, 'S' as u16, 'c' as u16, 'r' as u16, 'o' as u16,
    'l' as u16, 'l' as u16, 'P' as u16, 'r' as u16, 'e' as u16, 'v' as u16, 'i' as u16, 'e' as u16,
    'w' as u16, 0,
];

const PREVIEW_WIDTH: i32 = 280;
const PREVIEW_GAP: i32 = 12;

pub fn open(desktop: &DesktopFrame, initial: &RgbaFrame) -> Result<Box<dyn ScrollPreview>> {
    let geometry = right_preview_geometry(desktop, initial)?;
    let overlay = scroll_overlay::open(desktop, initial.bounds(), geometry.bounds)?;
    let right = match open_right(initial, geometry) {
        Ok(right) => right,
        Err(error) => {
            drop(overlay);
            refresh_desktop_after_overlay();
            return Err(error);
        }
    };
    Ok(Box::new(WindowsScrollPreview {
        overlay: Some(overlay),
        right: Some(right),
    }))
}

struct WindowsScrollPreview {
    overlay: Option<Box<scroll_overlay::Window>>,
    right: Option<Box<RightPreview>>,
}

impl ScrollPreview for WindowsScrollPreview {
    fn update(&mut self, patch: PreviewPatch<'_>) -> Result<()> {
        self.right
            .as_mut()
            .context("scroll preview is closed")?
            .update(patch)
    }
}

impl Drop for WindowsScrollPreview {
    fn drop(&mut self) {
        unsafe { ReleaseCapture() };
        let overlay_was_visible = self
            .overlay
            .as_mut()
            .is_some_and(|overlay| overlay.hide_for_close());
        let preview_was_visible = self
            .right
            .as_mut()
            .is_some_and(|preview| preview.hide_for_close());
        if overlay_was_visible || preview_was_visible {
            unsafe {
                let _ = DwmFlush();
            }
        }
        self.overlay.take();
        self.right.take();
        refresh_desktop_after_overlay();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RightPreviewGeometry {
    bounds: RectI,
    bottom_anchor: i32,
    maximum_height: u32,
}

fn right_preview_geometry(
    desktop: &DesktopFrame,
    initial: &RgbaFrame,
) -> Result<RightPreviewGeometry> {
    let selection = initial.bounds();
    let desktop_bounds = desktop.bounds;
    let desktop_width = i32::try_from(desktop_bounds.width())
        .context("virtual desktop width exceeds i32")?
        .max(1);
    let desktop_height = i32::try_from(desktop_bounds.height())
        .context("virtual desktop height exceeds i32")?
        .max(1);
    let preview_width = PREVIEW_WIDTH.min(desktop_width);
    let preferred_right = selection.right().saturating_add(PREVIEW_GAP);
    let left = if preferred_right.saturating_add(preview_width) <= desktop_bounds.right() {
        preferred_right
    } else {
        selection.left.saturating_sub(preview_width + PREVIEW_GAP)
    }
    .clamp(
        desktop_bounds.left,
        desktop_bounds.right().saturating_sub(preview_width),
    );
    let top_limit = desktop_bounds
        .top
        .saturating_add(PREVIEW_GAP.min(desktop_height / 2));
    let bottom_anchor = selection
        .bottom()
        .clamp(top_limit.saturating_add(1), desktop_bounds.bottom());
    let maximum_height = u32::try_from(bottom_anchor.saturating_sub(top_limit))
        .context("scroll preview maximum height exceeds u32")?
        .max(1);
    let height = scaled_preview_height(
        preview_width as u32,
        initial.width(),
        initial.height(),
        maximum_height,
    );
    let top = bottom_anchor.saturating_sub_unsigned(height);
    Ok(RightPreviewGeometry {
        bounds: RectI::new(left, top, preview_width as u32, height),
        bottom_anchor,
        maximum_height,
    })
}

fn scaled_preview_height(
    viewport_width: u32,
    document_width: u32,
    document_height: u32,
    maximum_height: u32,
) -> u32 {
    let width = u64::from(document_width.max(1));
    let numerator = u64::from(document_height.max(1)) * u64::from(viewport_width.max(1));
    let scaled = numerator.div_ceil(width).min(u64::from(u32::MAX)) as u32;
    scaled.clamp(1, maximum_height.max(1))
}

fn open_right(initial: &RgbaFrame, geometry: RightPreviewGeometry) -> Result<Box<RightPreview>> {
    register_class()?;
    let preview_bounds = geometry.bounds;
    let title = wide("Patrick Star Scroll Preview");
    let instance = unsafe { GetModuleHandleW(null()) };
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            CLASS_NAME.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            preview_bounds.left,
            preview_bounds.top,
            preview_bounds.width() as i32,
            preview_bounds.height() as i32,
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
        width: preview_bounds.width(),
        height: preview_bounds.height(),
        left: preview_bounds.left,
        bottom_anchor: geometry.bottom_anchor,
        maximum_height: geometry.maximum_height,
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
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
    preview.visible = true;
    if let Err(error) = preview.redraw_now() {
        drop(preview);
        return Err(error);
    }
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
    left: i32,
    bottom_anchor: i32,
    maximum_height: u32,
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
        let document_size = (patch.document_width, patch.document_height);
        self.upload(patch)?;
        self.resize_for_document(document_size.0, document_size.1)?;
        self.redraw_now()
    }

    fn resize_for_document(&mut self, document_width: u32, document_height: u32) -> Result<()> {
        let height = scaled_preview_height(
            self.width,
            document_width,
            document_height,
            self.maximum_height,
        );
        if height == self.height {
            return Ok(());
        }
        let top = self.bottom_anchor.saturating_sub_unsigned(height);
        anyhow::ensure!(
            unsafe {
                SetWindowPos(
                    self.hwnd,
                    HWND_TOPMOST,
                    self.left,
                    top,
                    self.width as i32,
                    height as i32,
                    SWP_NOACTIVATE,
                )
            } != 0,
            "resize scroll preview failed"
        );
        self.height = height;
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_height_tracks_document_aspect_until_the_screen_limit() {
        assert_eq!(scaled_preview_height(280, 560, 240, 900), 120);
        assert_eq!(scaled_preview_height(280, 560, 960, 900), 480);
        assert_eq!(scaled_preview_height(280, 560, 2400, 900), 900);
    }

    #[test]
    fn growing_preview_keeps_its_bottom_edge_anchored() {
        let desktop = DesktopFrame::new(RectI::new(0, 0, 1920, 1080), vec![0; 1920 * 1080 * 4])
            .unwrap();
        let initial = RgbaFrame::new(RectI::new(300, 200, 600, 400), vec![0; 600 * 400 * 4])
            .unwrap();
        let geometry = right_preview_geometry(&desktop, &initial).unwrap();
        assert_eq!(geometry.bounds.bottom(), geometry.bottom_anchor);
        assert_eq!(geometry.bounds.height(), 187);
    }
}
