use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};

use anyhow::{Context, Result, anyhow};
use windows_sys::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::Graphics::Dwm::DwmFlush;
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT, UpdateWindow,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, VK_CONTROL, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME,
    VK_LEFT, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_UP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CS_DBLCLKS, CS_OWNDC, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GWLP_USERDATA, GetMessageW, GetWindowLongPtrW, IDC_ARROW, LoadCursorW, MSG, PostQuitMessage,
    RegisterClassExW, SW_HIDE, SW_SHOW, SetForegroundWindow, SetWindowLongPtrW, ShowWindow,
    TranslateMessage, WM_CHAR, WM_CLOSE, WM_DESTROY, WM_DISPLAYCHANGE, WM_ERASEBKGND, WM_KEYDOWN,
    WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WM_RBUTTONUP,
    WM_SETCURSOR, WM_SIZE, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::model::{
    CaptureIntent, CaptureOutcome, DesktopFrame, EditorKey, OverlayAction, OverlayFeatures,
    OverlayLayout, OverlaySession, Point, PointI, Rect, Tool,
};
use crate::platform::{NativeCursorHost, WindowLocator};
use crate::rendering::OverlayRenderer;

use super::{cursor, wgl, window_locator};

const CLASS_NAME: &[u16] = &[
    'P' as u16, 'a' as u16, 't' as u16, 'r' as u16, 'i' as u16, 'c' as u16, 'k' as u16, 'S' as u16,
    't' as u16, 'a' as u16, 'r' as u16, '2' as u16, 'O' as u16, 'v' as u16, 'e' as u16, 'r' as u16,
    'l' as u16, 'a' as u16, 'y' as u16, 0,
];

pub fn run(frame: DesktopFrame, features: OverlayFeatures) -> Result<CaptureOutcome> {
    let instance = unsafe { GetModuleHandleW(null()) };
    anyhow::ensure!(!instance.is_null(), "GetModuleHandleW failed");
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
        "RegisterClassExW failed"
    );

    let title = wide("Patrick Star Capture Overlay");
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            CLASS_NAME.as_ptr(),
            title.as_ptr(),
            WS_POPUP,
            frame.bounds.left,
            frame.bounds.top,
            frame.bounds.width() as i32,
            frame.bounds.height() as i32,
            null_mut(),
            null_mut(),
            instance,
            null(),
        )
    };
    anyhow::ensure!(!hwnd.is_null(), "CreateWindowExW failed");

    let result = run_window(hwnd, frame, features);
    unsafe { DestroyWindow(hwnd) };
    result
}

fn run_window(
    hwnd: HWND,
    frame: DesktopFrame,
    features: OverlayFeatures,
) -> Result<CaptureOutcome> {
    let surface = wgl::Surface::new(hwnd).context("failed to create WGL surface")?;
    let mut renderer = unsafe {
        OverlayRenderer::new(&frame, |name| surface.proc_address(name))
            .context("failed to initialize OpenGL/FemtoVG overlay")?
    };
    for font in windows_ui_fonts() {
        renderer.load_font(&font);
    }
    let bounds = frame.bounds;
    let mut session = OverlaySession::with_features(bounds, features);
    if let Some(mode) = std::env::var_os("PATRICK_STAR2_ANNOTATION_SMOKE") {
        let tool = if mode == "mosaic" {
            Tool::Mosaic
        } else {
            Tool::Rectangle
        };
        prepare_annotation_smoke(&mut session, bounds.local_bounds(), tool);
    }
    let mut state = Box::new(WindowState {
        renderer,
        surface,
        hwnd,
        locator: window_locator::Detector::new(hwnd),
        cursor_host: cursor::Host::new(),
        session,
        frame: Some(frame),
        width: bounds.width(),
        height: bounds.height(),
        outcome: CaptureOutcome::Cancelled,
        error: None,
        pending_high_surrogate: None,
        exit_after_first_frame: std::env::var_os("PATRICK_STAR2_SMOKE_TEST").is_some(),
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

    let loop_result = message_loop();
    unsafe {
        // Hide while the window and its front buffer are still valid. Destroying
        // the WGL resources first can leave the final dimmed frame in DWM until
        // another desktop window happens to invalidate that area.
        ReleaseCapture();
        ShowWindow(hwnd, SW_HIDE);
        let _ = DwmFlush();
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
    }
    let outcome = std::mem::replace(&mut state.outcome, CaptureOutcome::Cancelled);
    let error = state.error.take();
    drop(state);
    loop_result?;
    if let Some(error) = error {
        return Err(error.context("failed to export capture"));
    }
    Ok(outcome)
}

fn prepare_annotation_smoke(session: &mut OverlaySession, surface: Rect, tool: Tool) {
    let left = surface.width() * 0.3;
    let top = surface.height() * 0.25;
    let right = (left + 600.0).min(surface.right - 40.0);
    let bottom = (top + 400.0).min(surface.bottom - 80.0);
    session.pointer_down(Point::new(left, top));
    session.pointer_move(Point::new(right, bottom), None);
    session.pointer_up(Point::new(right, bottom));
    session.activate(OverlayAction::Tool(tool));
    session.pointer_down(Point::new(left + 100.0, top + 100.0));
    session.pointer_move(Point::new(left + 320.0, top + 260.0), None);
    session.pointer_up(Point::new(left + 320.0, top + 260.0));
}

fn message_loop() -> Result<()> {
    let mut message = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
        if result == -1 {
            return Err(anyhow!("GetMessageW failed"));
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
    // GPU resources must be dropped while the WGL surface is still current.
    renderer: OverlayRenderer,
    surface: wgl::Surface,
    hwnd: HWND,
    locator: window_locator::Detector,
    cursor_host: cursor::Host,
    session: OverlaySession,
    frame: Option<DesktopFrame>,
    width: u32,
    height: u32,
    outcome: CaptureOutcome,
    error: Option<anyhow::Error>,
    pending_high_surrogate: Option<u16>,
    exit_after_first_frame: bool,
}

impl WindowState {
    fn invalidate(&self) {
        unsafe { InvalidateRect(self.hwnd, null(), 0) };
    }

    fn point(&self, lparam: LPARAM) -> Point {
        let x = (lparam as u32 & 0xffff) as u16 as i16 as f32;
        let y = ((lparam as u32 >> 16) & 0xffff) as u16 as i16 as f32;
        Point::new(x, y)
    }

    fn action_at(&self, point: Point) -> Option<OverlayAction> {
        let selection = self.session.selection().rect()?;
        OverlayLayout::for_tool(
            selection,
            Rect::new(0.0, 0.0, self.width as f32, self.height as f32),
            self.session.active_tool(),
        )
        .action_at(point)
    }

    fn apply_pointer_cursor(&mut self, point: Point) {
        let toolbar_hovered = self
            .action_at(point)
            .is_some_and(|action| self.session.action_enabled(action));
        let cursor = self.session.pointer_cursor(point, toolbar_hovered);
        self.cursor_host.set_cursor(cursor);
    }

    fn reapply_pointer_cursor(&mut self) {
        let point = self.session.cursor().unwrap_or(Point::new(0.0, 0.0));
        self.apply_pointer_cursor(point);
    }

    fn finish(&mut self, intent: CaptureIntent) {
        if self.session.selection().rect().is_none() {
            return;
        }
        let Some(frame) = self.frame.as_ref() else {
            return;
        };
        match self.renderer.export(frame, &self.session) {
            Ok(image) => {
                let desktop = self
                    .frame
                    .take()
                    .expect("capture frame exists until the session finishes");
                self.outcome = CaptureOutcome::Confirmed {
                    image,
                    intent,
                    desktop,
                };
            }
            Err(error) => self.error = Some(error),
        }
        unsafe { PostQuitMessage(0) };
    }

    fn activate_action(&mut self, action: OverlayAction) {
        match action {
            OverlayAction::Cancel => unsafe { PostQuitMessage(0) },
            OverlayAction::Confirm => self.finish(CaptureIntent::Clipboard),
            OverlayAction::Save => self.finish(CaptureIntent::Save),
            OverlayAction::Pin => self.finish(CaptureIntent::Pin),
            OverlayAction::ExtractText => self.finish(CaptureIntent::ExtractText),
            OverlayAction::ScrollCapture => self.finish(CaptureIntent::ScrollCapture),
            OverlayAction::Tool(_) | OverlayAction::Option(_) | OverlayAction::Undo => {
                self.session.activate(action);
            }
            OverlayAction::Languages => {}
        }
    }

    fn insert_utf16(&mut self, unit: u16) -> bool {
        if (0xd800..=0xdbff).contains(&unit) {
            self.pending_high_surrogate = Some(unit);
            return false;
        }
        let character = if (0xdc00..=0xdfff).contains(&unit) {
            let Some(high) = self.pending_high_surrogate.take() else {
                return false;
            };
            char::decode_utf16([high, unit]).next().and_then(Result::ok)
        } else {
            self.pending_high_surrogate = None;
            char::from_u32(unit as u32)
        };
        character.is_some_and(|character| self.session.insert_character(character))
    }

    fn render(&mut self) {
        let Some(frame) = self.frame.as_ref() else {
            return;
        };
        self.renderer
            .render(self.width.max(1), self.height.max(1), frame, &self.session);
        if self.surface.present().is_err() {
            unsafe { PostQuitMessage(1) };
        } else if self.exit_after_first_frame {
            unsafe { PostQuitMessage(0) };
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
        WM_LBUTTONDOWN => {
            let point = state.point(lparam);
            match state.action_at(point) {
                Some(action) => {
                    let changed = state.session.set_hovered_action(Some(action));
                    if state.session.press_action(action) {
                        unsafe { SetCapture(hwnd) };
                        state.invalidate();
                    } else if changed {
                        state.invalidate();
                    }
                }
                None => {
                    if state.session.pointer_down(point) {
                        unsafe { SetCapture(hwnd) };
                        state.invalidate();
                    }
                }
            }
            state.apply_pointer_cursor(point);
            0
        }
        WM_LBUTTONDBLCLK => {
            let point = state.point(lparam);
            if state.session.double_click(point) {
                state.invalidate();
            }
            state.apply_pointer_cursor(point);
            0
        }
        WM_MOUSEMOVE => {
            let point = state.point(lparam);
            let pointer_action = state.action_at(point);
            let hover_changed = state.session.set_hovered_action(pointer_action);
            let target = (pointer_action.is_none() && state.session.wants_target()).then(|| {
                let desktop = PointI::new(
                    state.frame.as_ref().map_or(0, |frame| frame.bounds.left)
                        + point.x.floor() as i32,
                    state.frame.as_ref().map_or(0, |frame| frame.bounds.top)
                        + point.y.floor() as i32,
                );
                state.locator.target_at(desktop)
            });
            if hover_changed || state.session.pointer_move(point, target.flatten()) {
                state.invalidate();
            }
            state.apply_pointer_cursor(point);
            0
        }
        WM_LBUTTONUP => {
            let point = state.point(lparam);
            let pointer_action = state.action_at(point);
            let had_action_press = state.session.pressed_action().is_some();
            unsafe { ReleaseCapture() };
            if had_action_press {
                let action = state.session.release_action(pointer_action);
                state.session.set_hovered_action(pointer_action);
                if let Some(action) = action {
                    state.activate_action(action);
                }
                state.invalidate();
            } else {
                let changed = state.session.pointer_up(point);
                let hover_changed = state.session.set_hovered_action(pointer_action);
                if changed || hover_changed {
                    state.invalidate();
                }
            }
            state.apply_pointer_cursor(point);
            0
        }
        WM_SETCURSOR => {
            state.reapply_pointer_cursor();
            1
        }
        WM_RBUTTONUP | WM_CLOSE | WM_DISPLAYCHANGE => {
            unsafe { PostQuitMessage(0) };
            0
        }
        WM_KEYDOWN => {
            handle_key(state, wparam as u16);
            0
        }
        WM_CHAR => {
            if state.insert_utf16(wparam as u16) {
                state.invalidate();
            }
            0
        }
        WM_SIZE => {
            let width = (lparam as u32 & 0xffff).max(1);
            let height = ((lparam as u32 >> 16) & 0xffff).max(1);
            state.width = width;
            state.height = height;
            state.session.resize(width, height);
            state.invalidate();
            0
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn handle_key(state: &mut WindowState, key: u16) {
    match key {
        VK_ESCAPE => {
            if state.session.editor_key(EditorKey::Escape) {
                state.invalidate();
            } else {
                unsafe { PostQuitMessage(0) };
            }
        }
        VK_RETURN => {
            if state.session.editor_key(EditorKey::Enter) {
                state.invalidate();
            } else {
                state.finish(CaptureIntent::Clipboard);
            }
        }
        0x5a if key_down(VK_CONTROL) => {
            if state.session.editor_key(EditorKey::Undo) {
                state.invalidate();
            }
        }
        0x59 if key_down(VK_CONTROL) => {
            if state.session.editor_key(EditorKey::Redo) {
                state.invalidate();
            }
        }
        0x41 if key_down(VK_CONTROL) && !state.session.selection_locked() => {
            state.session.select_all();
            state.invalidate();
        }
        0x08 => {
            if state.session.editor_key(EditorKey::Backspace) {
                state.invalidate();
            } else if !state.session.selection_locked() {
                state.session.clear();
                state.invalidate();
            }
        }
        0x2e => {
            if state.session.editor_key(EditorKey::Delete) {
                state.invalidate();
            } else if !state.session.selection_locked() {
                state.session.clear();
                state.invalidate();
            }
        }
        VK_HOME | VK_END => {
            let editor_key = if key == VK_HOME {
                EditorKey::Home
            } else {
                EditorKey::End
            };
            if state.session.editor_key(editor_key) {
                state.invalidate();
            }
        }
        VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN => {
            let editor_key = match key {
                VK_LEFT => EditorKey::Left,
                VK_RIGHT => EditorKey::Right,
                VK_UP => EditorKey::Up,
                VK_DOWN => EditorKey::Down,
                _ => unreachable!(),
            };
            if state.session.editor_key(editor_key) {
                state.invalidate();
                return;
            }
            let distance = if key_down(VK_SHIFT) { 10.0 } else { 1.0 };
            let (dx, dy) = match key {
                VK_LEFT => (-distance, 0.0),
                VK_RIGHT => (distance, 0.0),
                VK_UP => (0.0, -distance),
                VK_DOWN => (0.0, distance),
                _ => unreachable!(),
            };
            if state.session.nudge_selection(dx, dy) {
                state.invalidate();
            }
        }
        _ => {}
    }
}

fn key_down(key: u16) -> bool {
    unsafe { GetKeyState(key as i32) < 0 }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn windows_ui_fonts() -> impl Iterator<Item = std::path::PathBuf> {
    let windows = std::env::var_os("WINDIR");
    let fonts = windows.map(|windows| std::path::PathBuf::from(windows).join("Fonts"));
    ["msyh.ttc", "segoeui.ttf", "seguiemj.ttf"]
        .into_iter()
        .filter_map(move |name| fonts.as_ref().map(|fonts| fonts.join(name)))
        .filter(|path| path.is_file())
}
