use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};

use anyhow::{Context, Result};
use windows_sys::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::Graphics::Dwm::DwmFlush;
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CombineRgn, CreateRectRgn, DeleteObject, EndPaint, InvalidateRect, PAINTSTRUCT,
    RGN_DIFF, SetWindowRgn, UpdateWindow,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, VK_ESCAPE, VK_RETURN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CS_DBLCLKS, CS_OWNDC, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA,
    GetWindowLongPtrW, IDC_ARROW, IDC_HAND, IsWindow, LoadCursorW, RegisterClassExW, SW_HIDE,
    SW_SHOW, SetCursor, SetForegroundWindow, SetWindowLongPtrW, ShowWindow, WM_CLOSE, WM_DESTROY,
    WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WM_SETCURSOR,
    WM_SIZE, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::model::{DesktopFrame, Point, Rect, RectI, ScrollAction, ScrollLayout};
use crate::rendering::OverlayRenderer;

use super::{capture_exclusion, scroll_capture, wgl};

const CLASS_NAME: &[u16] = &[
    'P' as u16, 'a' as u16, 't' as u16, 'r' as u16, 'i' as u16, 'c' as u16, 'k' as u16, 'S' as u16,
    't' as u16, 'a' as u16, 'r' as u16, '2' as u16, 'S' as u16, 'c' as u16, 'r' as u16, 'o' as u16,
    'l' as u16, 'l' as u16, 'O' as u16, 'v' as u16, 'e' as u16, 'r' as u16, 'l' as u16, 'a' as u16,
    'y' as u16, 0,
];

pub(super) fn open(background: &DesktopFrame, selection: RectI) -> Result<Box<Window>> {
    register_class()?;
    let bounds = background.bounds;
    let instance = unsafe { GetModuleHandleW(null()) };
    let title = wide("Patrick Star Scroll Overlay");
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            CLASS_NAME.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            bounds.left,
            bounds.top,
            bounds.width() as i32,
            bounds.height() as i32,
            null_mut(),
            null_mut(),
            instance,
            null(),
        )
    };
    anyhow::ensure!(!hwnd.is_null(), "create Windows scroll overlay failed");

    let local_selection = RectI::new(
        selection.left.saturating_sub(bounds.left),
        selection.top.saturating_sub(bounds.top),
        selection.width(),
        selection.height(),
    );
    if let Err(error) = apply_hole(
        hwnd,
        RectI::new(0, 0, bounds.width(), bounds.height()),
        local_selection,
    ) {
        unsafe { DestroyWindow(hwnd) };
        return Err(error).context("cut selected region out of scroll overlay");
    }
    if let Err(error) = capture_exclusion::apply(hwnd) {
        eprintln!("exclude scroll overlay from capture failed: {error:#}");
    }

    let surface = match wgl::Surface::new(hwnd) {
        Ok(surface) => surface,
        Err(error) => {
            unsafe { DestroyWindow(hwnd) };
            return Err(error).context("create WGL surface for scroll overlay");
        }
    };
    let mut renderer =
        match unsafe { OverlayRenderer::new(background, |name| surface.proc_address(name)) } {
            Ok(renderer) => renderer,
            Err(error) => {
                drop(surface);
                unsafe { DestroyWindow(hwnd) };
                return Err(error).context("initialize scroll overlay renderer");
            }
        };
    for font in super::ui_font_paths() {
        renderer.load_font(&font);
    }
    let mut window = Box::new(Window {
        hwnd,
        renderer: Some(renderer),
        surface: Some(surface),
        selection: Rect::new(
            local_selection.left as f32,
            local_selection.top as f32,
            local_selection.right() as f32,
            local_selection.bottom() as f32,
        ),
        width: bounds.width(),
        height: bounds.height(),
        hovered: None,
        pressed: None,
        visible: false,
        error: None,
    });
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, (&mut *window as *mut Window) as isize);
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        UpdateWindow(hwnd);
    }
    window.visible = true;
    Ok(window)
}

pub(super) struct Window {
    hwnd: HWND,
    renderer: Option<OverlayRenderer>,
    surface: Option<wgl::Surface>,
    selection: Rect,
    width: u32,
    height: u32,
    hovered: Option<ScrollAction>,
    pressed: Option<ScrollAction>,
    visible: bool,
    error: Option<anyhow::Error>,
}

impl Window {
    fn surface_bounds(&self) -> Rect {
        Rect::new(0.0, 0.0, self.width as f32, self.height as f32)
    }

    fn action_at(&self, point: Point) -> Option<ScrollAction> {
        ScrollLayout::new(self.selection, self.surface_bounds()).action_at(point)
    }

    fn invalidate(&self) {
        unsafe { InvalidateRect(self.hwnd, null(), 0) };
    }

    fn render(&mut self) {
        let (Some(renderer), Some(surface)) = (&mut self.renderer, &self.surface) else {
            return;
        };
        if let Err(error) = surface.ensure_current() {
            self.error = Some(error.context("activate scroll overlay OpenGL context"));
            scroll_capture::request(ScrollAction::Cancel);
            return;
        }
        renderer.render_scroll(
            self.width.max(1),
            self.height.max(1),
            self.selection,
            self.hovered,
            self.pressed,
        );
        if let Err(error) = surface.present() {
            self.error = Some(error.context("present scroll overlay"));
            scroll_capture::request(ScrollAction::Cancel);
        }
    }

    fn set_cursor(&self) {
        let cursor = if self.hovered.is_some() {
            IDC_HAND
        } else {
            IDC_ARROW
        };
        unsafe { SetCursor(LoadCursorW(null_mut(), cursor)) };
    }

    pub(super) fn hide_for_close(&mut self) -> bool {
        if !self.visible || unsafe { IsWindow(self.hwnd) } == 0 {
            return false;
        }
        unsafe { ShowWindow(self.hwnd, SW_HIDE) };
        self.visible = false;
        true
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        unsafe { ReleaseCapture() };
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
    let state = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Window;
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
        WM_MOUSEMOVE => {
            let hovered = state.action_at(point(lparam));
            if state.hovered != hovered {
                state.hovered = hovered;
                state.invalidate();
            }
            state.set_cursor();
            0
        }
        WM_LBUTTONDOWN => {
            state.pressed = state.action_at(point(lparam));
            if state.pressed.is_some() {
                unsafe { SetCapture(hwnd) };
                state.invalidate();
            }
            0
        }
        WM_LBUTTONUP => {
            unsafe { ReleaseCapture() };
            let released = state.action_at(point(lparam));
            let action = state
                .pressed
                .take()
                .filter(|pressed| Some(*pressed) == released);
            state.hovered = released;
            if let Some(action) = action {
                scroll_capture::request(action);
            } else {
                state.invalidate();
            }
            0
        }
        WM_SETCURSOR => {
            state.set_cursor();
            1
        }
        WM_KEYDOWN => {
            match wparam as u16 {
                VK_ESCAPE => scroll_capture::request(ScrollAction::Cancel),
                VK_RETURN => scroll_capture::request(ScrollAction::Edit),
                _ => {}
            }
            0
        }
        WM_SIZE => {
            state.width = (lparam as u32 & 0xffff).max(1);
            state.height = ((lparam as u32 >> 16) & 0xffff).max(1);
            state.invalidate();
            0
        }
        WM_CLOSE => {
            scroll_capture::request(ScrollAction::Cancel);
            0
        }
        WM_DESTROY => 0,
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn apply_hole(hwnd: HWND, client: RectI, hole: RectI) -> Result<()> {
    let outer = unsafe { CreateRectRgn(client.left, client.top, client.right(), client.bottom()) };
    let excluded = unsafe { CreateRectRgn(hole.left, hole.top, hole.right(), hole.bottom()) };
    if outer.is_null() || excluded.is_null() {
        if !outer.is_null() {
            unsafe { DeleteObject(outer) };
        }
        if !excluded.is_null() {
            unsafe { DeleteObject(excluded) };
        }
        anyhow::bail!("CreateRectRgn failed for scroll overlay");
    }
    let combined = unsafe { CombineRgn(outer, outer, excluded, RGN_DIFF) };
    unsafe { DeleteObject(excluded) };
    if combined == 0 {
        unsafe { DeleteObject(outer) };
        anyhow::bail!("CombineRgn failed for scroll overlay");
    }
    if unsafe { SetWindowRgn(hwnd, outer, 1) } == 0 {
        unsafe { DeleteObject(outer) };
        anyhow::bail!("SetWindowRgn failed for scroll overlay");
    }
    Ok(())
}

fn point(lparam: LPARAM) -> Point {
    Point::new(
        (lparam as u32 & 0xffff) as u16 as i16 as f32,
        ((lparam as u32 >> 16) & 0xffff) as u16 as i16 as f32,
    )
}

fn register_class() -> Result<()> {
    let instance = unsafe { GetModuleHandleW(null()) };
    anyhow::ensure!(
        !instance.is_null(),
        "GetModuleHandleW failed for scroll overlay"
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
        "register Windows scroll overlay class failed"
    );
    Ok(())
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
