use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use windows_sys::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, WPARAM,
};
use windows_sys::Win32::Graphics::Dwm::DwmFlush;
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, PAINTSTRUCT, RDW_INVALIDATE, RDW_NOERASE, RDW_UPDATENOW, RedrawWindow,
    ScreenToClient,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, VK_CONTROL, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME,
    VK_LEFT, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_UP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CS_DBLCLKS, CS_OWNDC, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GWLP_USERDATA, GetCursorPos, GetForegroundWindow, GetMessageW, GetWindowLongPtrW, IDC_ARROW,
    IsWindow, IsWindowVisible, LoadCursorW, MSG, PostMessageW, PostQuitMessage, RegisterClassExW,
    SW_HIDE, SW_SHOWNOACTIVATE, SetForegroundWindow, SetWindowLongPtrW, ShowWindow,
    TranslateMessage, WM_ACTIVATE, WM_ACTIVATEAPP, WM_APP, WM_CHAR, WM_CLOSE, WM_DESTROY,
    WM_DISPLAYCHANGE, WM_DPICHANGED, WM_ERASEBKGND, WM_KEYDOWN, WM_KILLFOCUS,
    WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE,
    WM_NCACTIVATE, WM_PAINT, WM_RBUTTONUP, WM_SETCURSOR, WM_SETFOCUS, WM_SHOWWINDOW, WM_SIZE,
    WM_WINDOWPOSCHANGED, WM_WINDOWPOSCHANGING, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};

use crate::model::{
    CaptureIntent, CaptureOutcome, DesktopFrame, EditorKey, OverlayAction, OverlayFeatures,
    OverlayLayout, OverlaySession, Point, PointI, Rect, Tool,
};
use crate::platform::{
    CaptureOverlayHandoff, CaptureOverlayResult, NativeCursorHost, WindowLocator,
};
use crate::rendering::OverlayRenderer;

use super::{cursor, dpi_scale_at, refresh_desktop_after_overlay, wgl, window_locator};

const CLASS_NAME: &[u16] = &[
    'P' as u16, 'a' as u16, 't' as u16, 'r' as u16, 'i' as u16, 'c' as u16, 'k' as u16, 'S' as u16,
    't' as u16, 'a' as u16, 'r' as u16, '2' as u16, 'O' as u16, 'v' as u16, 'e' as u16, 'r' as u16,
    'l' as u16, 'a' as u16, 'y' as u16, 0,
];
const WM_OVERLAY_RENDER: u32 = WM_APP + 0x271;

pub fn run(frame: DesktopFrame, features: OverlayFeatures) -> Result<CaptureOverlayResult> {
    let previous_foreground = unsafe { GetForegroundWindow() };
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

    let result = run_window(hwnd, previous_foreground, frame, features);
    if !result.as_ref().is_ok_and(|result| result.handoff.is_some()) {
        destroy_overlay_window(hwnd);
    }
    result
}

fn run_window(
    hwnd: HWND,
    previous_foreground: HWND,
    frame: DesktopFrame,
    features: OverlayFeatures,
) -> Result<CaptureOverlayResult> {
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
        previous_foreground,
        locator: window_locator::Detector::new(hwnd),
        cursor_host: cursor::Host::new(),
        session,
        frame: Some(frame),
        width: bounds.width(),
        height: bounds.height(),
        dpi_scale: window_dpi_scale(hwnd),
        outcome: CaptureOutcome::Cancelled,
        error: None,
        pending_high_surrogate: None,
        exit_after_first_frame: std::env::var_os("PATRICK_STAR2_SMOKE_TEST").is_some(),
        trace: OverlayTrace::from_env(),
        pending_repaint: "window-created",
        render_requested: false,
    });
    unsafe {
        SetWindowLongPtrW(
            hwnd,
            GWLP_USERDATA,
            (&mut *state as *mut WindowState) as isize,
        );
    }
    state.trace_event(
        "window-created",
        format!(
            "hwnd={:p} bounds={bounds:?} dpi_scale={:.3} foreground={:p}",
            hwnd, state.dpi_scale, previous_foreground
        ),
    );
    state.prime_pointer_state();
    // Present a complete frame while the popup is still hidden. Showing a
    // newly-created WGL window before its first swap briefly exposes the
    // class/background surface on some compositors.
    state.render("initial-hidden");
    unsafe {
        let flush_started = Instant::now();
        let flush_result = DwmFlush();
        state.trace_event(
            "initial-hidden-dwm-flush",
            format!(
                "result={flush_result:#x} elapsed_us={}",
                flush_started.elapsed().as_micros()
            ),
        );
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        // Consume the visible window's initial update region now. Later
        // interaction frames use WM_OVERLAY_RENDER and are not Win32 paint
        // damage; WM_PAINT remains reserved for genuine system exposure.
        state.pending_repaint = "initial-visible-redraw";
        let redraw_result = RedrawWindow(
            hwnd,
            null(),
            null_mut(),
            RDW_INVALIDATE | RDW_UPDATENOW | RDW_NOERASE,
        );
        state.trace_event(
            "initial-visible-redraw",
            format!("result={redraw_result} visible={}", IsWindowVisible(hwnd)),
        );
        let flush_started = Instant::now();
        let flush_result = DwmFlush();
        state.trace_event(
            "initial-visible-dwm-flush",
            format!(
                "result={flush_result:#x} elapsed_us={}",
                flush_started.elapsed().as_micros()
            ),
        );
        let foreground_result = SetForegroundWindow(hwnd);
        state.trace_event(
            "set-foreground",
            format!(
                "result={foreground_result} foreground={:p}",
                GetForegroundWindow()
            ),
        );
    }

    let loop_result = message_loop();
    let outcome = std::mem::replace(&mut state.outcome, CaptureOutcome::Cancelled);
    let error = state.error.take();

    let keep_until_scroll_overlay_is_ready = loop_result.is_ok()
        && error.is_none()
        && matches!(
            &outcome,
            CaptureOutcome::Confirmed {
                intent: CaptureIntent::ScrollCapture,
                ..
            }
        );
    if keep_until_scroll_overlay_is_ready {
        return Ok(CaptureOverlayResult {
            outcome,
            handoff: Some(Box::new(WindowsOverlayHandoff {
                hwnd,
                state: Some(state),
            })),
        });
    }

    close_overlay(hwnd, &mut state);
    drop(state);
    loop_result?;
    if let Some(error) = error {
        return Err(error.context("failed to export capture"));
    }
    Ok(CaptureOverlayResult::complete(outcome))
}

struct WindowsOverlayHandoff {
    hwnd: HWND,
    state: Option<Box<WindowState>>,
}

impl CaptureOverlayHandoff for WindowsOverlayHandoff {}

impl Drop for WindowsOverlayHandoff {
    fn drop(&mut self) {
        if let Some(mut state) = self.state.take() {
            close_overlay(self.hwnd, &mut state);
            drop(state);
        }
        destroy_overlay_window(self.hwnd);
    }
}

fn destroy_overlay_window(hwnd: HWND) {
    if unsafe { IsWindow(hwnd) } != 0 {
        unsafe { DestroyWindow(hwnd) };
    }
    refresh_desktop_after_overlay();
}

fn close_overlay(hwnd: HWND, state: &mut WindowState) {
    unsafe {
        // Keep the final front buffer valid until the window is hidden. Otherwise
        // DWM can retain the dimmed desktop after the OpenGL context is destroyed.
        ReleaseCapture();
        ShowWindow(hwnd, SW_HIDE);
        let _ = DwmFlush();
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        if !state.previous_foreground.is_null()
            && state.previous_foreground != hwnd
            && IsWindow(state.previous_foreground) != 0
        {
            SetForegroundWindow(state.previous_foreground);
        }
    }
    let _ = state.surface.ensure_current();
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
    previous_foreground: HWND,
    locator: window_locator::Detector,
    cursor_host: cursor::Host,
    session: OverlaySession,
    frame: Option<DesktopFrame>,
    width: u32,
    height: u32,
    dpi_scale: f32,
    outcome: CaptureOutcome,
    error: Option<anyhow::Error>,
    pending_high_surrogate: Option<u16>,
    exit_after_first_frame: bool,
    trace: Option<OverlayTrace>,
    pending_repaint: &'static str,
    render_requested: bool,
}

struct OverlayTrace {
    started: Instant,
    sequence: u32,
    mouse_moves: u32,
    paints: u32,
}

impl OverlayTrace {
    fn from_env() -> Option<Self> {
        std::env::var_os("PATRICK_STAR2_OVERLAY_TRACE").map(|_| Self {
            started: Instant::now(),
            sequence: 0,
            mouse_moves: 0,
            paints: 0,
        })
    }

    fn event(&mut self, name: &str, detail: String) {
        if self.sequence >= 128 {
            return;
        }
        self.sequence += 1;
        eprintln!(
            "[overlay-trace #{:03} +{:>8}us] {name}: {detail}",
            self.sequence,
            self.started.elapsed().as_micros()
        );
    }
}

impl WindowState {
    fn trace_event(&mut self, name: &str, detail: String) {
        if let Some(trace) = self.trace.as_mut() {
            trace.event(name, detail);
        }
    }

    fn prime_pointer_state(&mut self) {
        let Some(frame) = self.frame.as_ref() else {
            self.trace_event("prime-pointer", "frame-missing".to_owned());
            return;
        };
        let mut cursor = POINT::default();
        if unsafe { GetCursorPos(&mut cursor) } == 0 {
            self.trace_event("prime-pointer", "GetCursorPos-failed".to_owned());
            return;
        }
        let desktop = PointI::new(cursor.x, cursor.y);
        if !frame.bounds.contains(desktop) {
            self.trace_event(
                "prime-pointer",
                format!("screen={desktop:?} outside={:?}", frame.bounds),
            );
            return;
        }
        if unsafe { ScreenToClient(self.hwnd, &mut cursor) } == 0
            || cursor.x < 0
            || cursor.y < 0
            || cursor.x as u32 >= self.width
            || cursor.y as u32 >= self.height
        {
            self.trace_event(
                "prime-pointer",
                format!("ScreenToClient-invalid screen={desktop:?} client=({}, {})", cursor.x, cursor.y),
            );
            return;
        }
        let point = Point::new(cursor.x as f32, cursor.y as f32);
        let detect_started = Instant::now();
        let target = self
            .session
            .wants_target()
            .then(|| self.locator.target_at(desktop))
            .flatten();
        let detect_us = detect_started.elapsed().as_micros();
        let changed = self.session.pointer_move(point, target);
        let highlight = self.session.highlight();
        self.trace_event(
            "prime-pointer",
            format!(
                "screen={desktop:?} client={point:?} target={target:?} highlight={highlight:?} changed={changed} detect_us={detect_us}"
            ),
        );
        self.apply_pointer_cursor(point);
    }

    fn request_render(&mut self, reason: &'static str) {
        self.pending_repaint = reason;
        if self.render_requested {
            self.trace_event("render-coalesced", format!("reason={reason}"));
            return;
        }
        let result = unsafe { PostMessageW(self.hwnd, WM_OVERLAY_RENDER, 0, 0) };
        if result == 0 {
            self.error = Some(anyhow!(
                "PostMessageW(capture overlay render) failed: {}",
                std::io::Error::last_os_error()
            ));
            unsafe { PostQuitMessage(1) };
            return;
        }
        self.render_requested = true;
        self.trace_event("render-requested", format!("reason={reason}"));
    }

    fn point(&self, lparam: LPARAM) -> Point {
        let x = (lparam as u32 & 0xffff) as u16 as i16 as f32;
        let y = ((lparam as u32 >> 16) & 0xffff) as u16 as i16 as f32;
        Point::new(x, y)
    }

    fn action_at(&self, point: Point) -> Option<OverlayAction> {
        let selection = self.session.selection().rect()?;
        OverlayLayout::for_tool_scaled(
            selection,
            Rect::new(0.0, 0.0, self.width as f32, self.height as f32),
            self.session.active_tool(),
            self.toolbar_dpi_scale(selection),
        )
        .action_at(point)
    }

    fn toolbar_dpi_scale(&self, selection: Rect) -> f32 {
        let Some(frame) = self.frame.as_ref() else {
            return self.dpi_scale;
        };
        let center = selection.center();
        dpi_scale_at(
            PointI::new(
                frame.bounds.left.saturating_add(center.x.round() as i32),
                frame.bounds.top.saturating_add(center.y.round() as i32),
            ),
            self.dpi_scale,
        )
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
        if let Err(error) = self.surface.ensure_current() {
            self.error = Some(error.context("activate capture overlay OpenGL context for export"));
            unsafe { PostQuitMessage(1) };
            return;
        }
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

    fn render(&mut self, reason: &'static str) {
        let Some(frame) = self.frame.as_ref() else {
            return;
        };
        let render_started = Instant::now();
        let was_current = self.surface.is_current();
        if let Err(error) = self.surface.ensure_current() {
            self.error = Some(error.context("activate capture overlay OpenGL context"));
            unsafe { PostQuitMessage(1) };
            return;
        }
        let current_us = render_started.elapsed().as_micros();
        let dpi_scale = self
            .session
            .selection()
            .rect()
            .map_or(self.dpi_scale, |selection| self.toolbar_dpi_scale(selection));
        self.renderer.render(
            self.width.max(1),
            self.height.max(1),
            dpi_scale,
            frame,
            &self.session,
        );
        let rendered_us = render_started.elapsed().as_micros();
        let present_started = Instant::now();
        match self.surface.present() {
            Ok(()) => {
                let present_us = present_started.elapsed().as_micros();
                let total_us = render_started.elapsed().as_micros();
                let cursor = self.session.cursor();
                let highlight = self.session.highlight();
                self.trace_event(
                    "render-complete",
                    format!(
                        "reason={reason} cursor={cursor:?} highlight={highlight:?} was_current={was_current} ensure_current_us={current_us} draw_us={} present_us={present_us} total_us={total_us}",
                        rendered_us.saturating_sub(current_us)
                    ),
                );
                if self.exit_after_first_frame {
                    unsafe { PostQuitMessage(0) };
                }
            }
            Err(error) => {
                self.error = Some(error.context("present capture overlay"));
                unsafe { PostQuitMessage(1) };
            }
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
    if let Some(name) = traced_window_message_name(message) {
        state.trace_event(
            name,
            format!(
                "wparam={wparam:#x} lparam={lparam:#x} visible={} foreground={:p}",
                unsafe { IsWindowVisible(hwnd) },
                unsafe { GetForegroundWindow() }
            ),
        );
    }
    match message {
        WM_OVERLAY_RENDER => {
            state.render_requested = false;
            let reason = state.pending_repaint;
            state.pending_repaint = "unspecified";
            state.trace_event("wm-overlay-render", format!("reason={reason}"));
            state.render(reason);
            0
        }
        WM_PAINT => {
            let mut paint: PAINTSTRUCT = unsafe { zeroed() };
            unsafe { BeginPaint(hwnd, &mut paint) };
            let reason = state.pending_repaint;
            state.pending_repaint = "unspecified";
            let paint_index = state.trace.as_mut().map(|trace| {
                trace.paints += 1;
                trace.paints
            });
            if paint_index.is_some_and(|index| index <= 12) {
                state.trace_event(
                    "wm-paint",
                    format!(
                        "index={} reason={reason} erase={} rect=({}, {})-({}, {})",
                        paint_index.unwrap_or_default(),
                        paint.fErase,
                        paint.rcPaint.left,
                        paint.rcPaint.top,
                        paint.rcPaint.right,
                        paint.rcPaint.bottom
                    ),
                );
            }
            state.render(reason);
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
                        state.request_render("left-button-down-action");
                    } else if changed {
                        state.request_render("left-button-down-hover");
                    }
                }
                None => {
                    if state.session.pointer_down(point) {
                        unsafe { SetCapture(hwnd) };
                        state.request_render("left-button-down-selection");
                    }
                }
            }
            state.apply_pointer_cursor(point);
            0
        }
        WM_LBUTTONDBLCLK => {
            let point = state.point(lparam);
            if state.session.double_click(point) {
                state.request_render("left-button-double-click");
            }
            state.apply_pointer_cursor(point);
            0
        }
        WM_MOUSEMOVE => {
            let point = state.point(lparam);
            let before_cursor = state.session.cursor();
            let before_highlight = state.session.highlight();
            let pointer_action = state.action_at(point);
            let hover_changed = state.session.set_hovered_action(pointer_action);
            let detect_started = Instant::now();
            let target = if pointer_action.is_none() && state.session.wants_target() {
                let desktop = PointI::new(
                    state.frame.as_ref().map_or(0, |frame| frame.bounds.left)
                        + point.x.floor() as i32,
                    state.frame.as_ref().map_or(0, |frame| frame.bounds.top)
                        + point.y.floor() as i32,
                );
                state.locator.target_at(desktop)
            } else {
                None
            };
            let detect_us = detect_started.elapsed().as_micros();
            let pointer_changed = state.session.pointer_move(point, target);
            let repaint = hover_changed || pointer_changed;
            let after_highlight = state.session.highlight();
            let mouse_index = state.trace.as_mut().map(|trace| {
                trace.mouse_moves += 1;
                trace.mouse_moves
            });
            if mouse_index.is_some_and(|index| index <= 8) {
                let mut actual_screen = POINT::default();
                let actual_screen_valid = unsafe { GetCursorPos(&mut actual_screen) } != 0;
                let message_screen = PointI::new(
                    state.frame.as_ref().map_or(0, |frame| frame.bounds.left)
                        + point.x.floor() as i32,
                    state.frame.as_ref().map_or(0, |frame| frame.bounds.top)
                        + point.y.floor() as i32,
                );
                state.trace_event(
                    "wm-mousemove",
                    format!(
                        "index={} client={point:?} message_screen={message_screen:?} actual_screen=({}, {}) actual_valid={actual_screen_valid} before_cursor={before_cursor:?} before_highlight={before_highlight:?} action={pointer_action:?} target={target:?} after_highlight={after_highlight:?} hover_changed={hover_changed} pointer_changed={pointer_changed} repaint={repaint} detect_us={detect_us}",
                        mouse_index.unwrap_or_default(),
                        actual_screen.x,
                        actual_screen.y
                    ),
                );
            }
            if repaint {
                state.request_render("mouse-move");
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
                state.request_render("left-button-up-action");
            } else {
                let changed = state.session.pointer_up(point);
                let hover_changed = state.session.set_hovered_action(pointer_action);
                if changed || hover_changed {
                    state.request_render("left-button-up-selection");
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
                state.request_render("text-input");
            }
            0
        }
        WM_SIZE => {
            let width = (lparam as u32 & 0xffff).max(1);
            let height = ((lparam as u32 >> 16) & 0xffff).max(1);
            state.width = width;
            state.height = height;
            state.session.resize(width, height);
            state.request_render("window-size");
            0
        }
        WM_DPICHANGED => {
            let dpi = (wparam as u32 & 0xffff).max(96);
            state.dpi_scale = dpi as f32 / 96.0;
            state.request_render("window-dpi");
            0
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn traced_window_message_name(message: u32) -> Option<&'static str> {
    match message {
        WM_ACTIVATE => Some("wm-activate"),
        WM_ACTIVATEAPP => Some("wm-activateapp"),
        WM_SETFOCUS => Some("wm-setfocus"),
        WM_KILLFOCUS => Some("wm-killfocus"),
        WM_SHOWWINDOW => Some("wm-showwindow"),
        WM_MOUSEACTIVATE => Some("wm-mouseactivate"),
        WM_NCACTIVATE => Some("wm-ncactivate"),
        WM_WINDOWPOSCHANGING => Some("wm-windowposchanging"),
        WM_WINDOWPOSCHANGED => Some("wm-windowposchanged"),
        _ => None,
    }
}

fn window_dpi_scale(hwnd: HWND) -> f32 {
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    dpi as f32 / 96.0
}

fn handle_key(state: &mut WindowState, key: u16) {
    match key {
        VK_ESCAPE => {
            if state.session.editor_key(EditorKey::Escape) {
                state.request_render("key-escape-editor");
            } else {
                unsafe { PostQuitMessage(0) };
            }
        }
        VK_RETURN => {
            if state.session.editor_key(EditorKey::Enter) {
                state.request_render("key-enter-editor");
            } else {
                state.finish(CaptureIntent::Clipboard);
            }
        }
        0x5a if key_down(VK_CONTROL) => {
            if state.session.editor_key(EditorKey::Undo) {
                state.request_render("key-undo");
            }
        }
        0x59 if key_down(VK_CONTROL) => {
            if state.session.editor_key(EditorKey::Redo) {
                state.request_render("key-redo");
            }
        }
        0x41 if key_down(VK_CONTROL) && !state.session.selection_locked() => {
            state.session.select_all();
            state.request_render("key-select-all");
        }
        0x08 => {
            if state.session.editor_key(EditorKey::Backspace) {
                state.request_render("key-backspace-editor");
            } else if !state.session.selection_locked() {
                state.session.clear();
                state.request_render("key-backspace-clear");
            }
        }
        0x2e => {
            if state.session.editor_key(EditorKey::Delete) {
                state.request_render("key-delete-editor");
            } else if !state.session.selection_locked() {
                state.session.clear();
                state.request_render("key-delete-clear");
            }
        }
        VK_HOME | VK_END => {
            let editor_key = if key == VK_HOME {
                EditorKey::Home
            } else {
                EditorKey::End
            };
            if state.session.editor_key(editor_key) {
                state.request_render("key-home-end");
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
                state.request_render("key-arrow-editor");
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
                state.request_render("key-nudge-selection");
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
