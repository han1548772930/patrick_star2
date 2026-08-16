use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{CStr, c_void};
use std::mem::{ManuallyDrop, size_of, zeroed};
use std::num::NonZeroU32;
use std::ptr::{null, null_mut};
use std::rc::{Rc, Weak};

use slint::platform::femtovg_renderer::{FemtoVGRenderer, OpenGLInterface};
use slint::platform::{
    Key, LayoutConstraints, PlatformError, PointerEventButton, Renderer, WindowAdapter,
    WindowEvent, WindowProperties,
};
use slint::{
    LogicalPosition, PhysicalPosition, PhysicalSize, SharedString, WindowPosition, WindowSize,
};
use windows_sys::Win32::Foundation::{
    ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT, ScreenToClient,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem, GetDpiForWindow,
    GetSystemMetricsForDpi, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, MAPVK_VSC_TO_VK_EX, MapVirtualKeyW, ReleaseCapture, SetCapture, TME_LEAVE,
    TRACKMOUSEEVENT, TrackMouseEvent, VK_BACK, VK_CAPITAL, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END,
    VK_ESCAPE, VK_F1, VK_F24, VK_HOME, VK_INSERT, VK_LCONTROL, VK_LEFT, VK_LMENU, VK_LSHIFT,
    VK_LWIN, VK_MENU, VK_NEXT, VK_PAUSE, VK_PRIOR, VK_RCONTROL, VK_RETURN, VK_RIGHT, VK_RMENU,
    VK_RSHIFT, VK_RWIN, VK_SCROLL, VK_SHIFT, VK_TAB, VK_UP,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CS_DBLCLKS, CS_OWNDC, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
    GWLP_USERDATA, GetClientRect, GetCursorPos, GetWindowLongPtrW, GetWindowRect, HTBOTTOM,
    HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT,
    HTTOPRIGHT, IDC_ARROW, IDC_CROSS, IDC_HAND, IDC_IBEAM, IsIconic, IsWindow, IsZoomed,
    LoadCursorW, MINMAXINFO, RegisterClassExW, SC_CLOSE, SC_MAXIMIZE, SC_MINIMIZE, SC_RESTORE,
    SIZE_MINIMIZED, SM_CXPADDEDBORDER, SM_CXSIZEFRAME, SW_HIDE, SW_MAXIMIZE, SW_MINIMIZE,
    SW_RESTORE, SW_SHOW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SetCursor, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow, UNICODE_NOCHAR,
    WM_CANCELMODE, WM_CAPTURECHANGED, WM_CHAR, WM_CLOSE, WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND,
    WM_GETMINMAXINFO, WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_NCCALCSIZE, WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_PAINT, WM_RBUTTONDBLCLK,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WM_SETFOCUS, WM_SIZE, WM_SYSCHAR, WM_SYSCOMMAND,
    WM_SYSKEYDOWN, WM_SYSKEYUP, WM_UNICHAR, WM_XBUTTONDBLCLK, WM_XBUTTONDOWN, WM_XBUTTONUP,
    WNDCLASSEXW, WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_EX_APPWINDOW, WS_MAXIMIZEBOX, WS_MINIMIZEBOX,
    WS_POPUP, WS_THICKFRAME, XBUTTON1, XBUTTON2,
};

use super::{capture_exclusion, wgl};

const CLASS_NAME: &[u16] = &[
    'P' as u16, 'a' as u16, 't' as u16, 'r' as u16, 'i' as u16, 'c' as u16, 'k' as u16, 'S' as u16,
    't' as u16, 'a' as u16, 'r' as u16, '2' as u16, 'S' as u16, 'l' as u16, 'i' as u16, 'n' as u16,
    't' as u16, 'W' as u16, 'i' as u16, 'n' as u16, 'd' as u16, 'o' as u16, 'w' as u16, 0,
];

const DEFAULT_LOGICAL_WIDTH: f32 = 1080.0;
const DEFAULT_LOGICAL_HEIGHT: f32 = 720.0;
const TITLEBAR_LOGICAL_HEIGHT: f32 = 36.0;
const TITLEBAR_BUTTONS_LOGICAL_WIDTH: f32 = 132.0;
const PREVIEW_HEADER_LOGICAL_HEIGHT: f32 = 86.0;
const PREVIEW_STATUS_LOGICAL_HEIGHT: f32 = 28.0;
const OCR_PANEL_MIN_LOGICAL_WIDTH: f32 = 280.0;
const OCR_PANEL_MAX_LOGICAL_WIDTH: f32 = 360.0;
const WHEEL_LOGICAL_PIXELS_PER_NOTCH: f32 = 60.0;
const WHEEL_DELTA: f32 = 120.0;
const WM_MOUSELEAVE: u32 = 0x02a3;
const MK_LBUTTON: u32 = 0x0001;
const MK_RBUTTON: u32 = 0x0002;
const MK_MBUTTON: u32 = 0x0010;
const MK_XBUTTON1: u32 = 0x0020;
const MK_XBUTTON2: u32 = 0x0040;
const XBUTTON_MASK: u32 = MK_XBUTTON1 | MK_XBUTTON2;

pub(crate) fn create() -> Result<Rc<dyn WindowAdapter>, PlatformError> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    register_window_class()?;

    let initial_dpi = system_dpi();
    let initial_scale = dpi_scale(initial_dpi);
    let width = logical_length_to_physical(DEFAULT_LOGICAL_WIDTH, initial_scale);
    let height = logical_length_to_physical(DEFAULT_LOGICAL_HEIGHT, initial_scale);
    let instance = unsafe { GetModuleHandleW(null()) };
    if instance.is_null() {
        return Err(platform_error("GetModuleHandleW failed for Slint window"));
    }

    let title = wide("Patrick Star");
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_APPWINDOW,
            CLASS_NAME.as_ptr(),
            title.as_ptr(),
            WS_POPUP
                | WS_THICKFRAME
                | WS_MINIMIZEBOX
                | WS_MAXIMIZEBOX
                | WS_CLIPCHILDREN
                | WS_CLIPSIBLINGS,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            width,
            height,
            null_mut(),
            null_mut(),
            instance,
            null(),
        )
    };
    if hwnd.is_null() {
        return Err(platform_error("CreateWindowExW failed for Slint window"));
    }
    if let Err(error) = capture_exclusion::apply(hwnd) {
        eprintln!("exclude Slint window from capture failed: {error:#}");
    }

    let surface = match wgl::Surface::new(hwnd) {
        Ok(surface) => surface,
        Err(error) => {
            unsafe { DestroyWindow(hwnd) };
            return Err(platform_error(format!(
                "create WGL surface for Slint window: {error:#}"
            )));
        }
    };
    let renderer = match FemtoVGRenderer::new(WglInterface(surface)) {
        Ok(renderer) => renderer,
        Err(error) => {
            unsafe { DestroyWindow(hwnd) };
            return Err(platform_error(format!(
                "initialize Slint FemtoVG renderer: {error}"
            )));
        }
    };

    let physical_size = client_size(hwnd).unwrap_or_else(|| {
        PhysicalSize::new(
            u32::try_from(width.max(1)).unwrap_or(1),
            u32::try_from(height.max(1)).unwrap_or(1),
        )
    });
    let dpi = window_dpi(hwnd);
    let adapter = Rc::<NativeWindow>::new_cyclic(move |weak: &Weak<NativeWindow>| {
        let weak_adapter: Weak<dyn WindowAdapter> = weak.clone();
        NativeWindow {
            hwnd,
            window: slint::Window::new(weak_adapter),
            renderer: ManuallyDrop::new(renderer),
            physical_size: Cell::new(physical_size),
            dpi: Cell::new(dpi),
            visible: Cell::new(false),
            redraw_pending: Cell::new(false),
            mouse_leave_tracked: Cell::new(false),
            pointer_captured: Cell::new(false),
            has_explicit_size: Cell::new(false),
            preferred_size_applied: Cell::new(false),
            constraints: Cell::new(LayoutConstraints::default()),
            content_kind: Cell::new(WindowContentKind::Other),
            character_keys: RefCell::new(HashMap::new()),
            pending_high_surrogate: Cell::new(None),
            render_error: RefCell::new(None),
        }
    });

    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Rc::as_ptr(&adapter) as isize);
        SetWindowPos(
            hwnd,
            null_mut(),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    if let Some(size) = client_size(hwnd) {
        adapter.physical_size.set(size);
    }
    adapter.dispatch(WindowEvent::ScaleFactorChanged {
        scale_factor: adapter.scale_factor(),
    });
    Ok(adapter)
}

fn register_window_class() -> Result<(), PlatformError> {
    let instance = unsafe { GetModuleHandleW(null()) };
    if instance.is_null() {
        return Err(platform_error("GetModuleHandleW failed for Slint class"));
    }
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
    if atom == 0 && unsafe { GetLastError() } != ERROR_CLASS_ALREADY_EXISTS {
        return Err(platform_error("RegisterClassExW failed for Slint window"));
    }
    Ok(())
}

struct WglInterface(wgl::Surface);

unsafe impl OpenGLInterface for WglInterface {
    fn ensure_current(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.0.ensure_current().map_err(Into::into)
    }

    fn swap_buffers(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.0.present().map_err(Into::into)
    }

    fn resize(
        &self,
        _width: NonZeroU32,
        _height: NonZeroU32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    fn get_proc_address(&self, name: &CStr) -> *const c_void {
        self.0.proc_address(name)
    }
}

struct NativeWindow {
    hwnd: HWND,
    window: slint::Window,
    // This must be destroyed before DestroyWindow invalidates the surface's HDC.
    renderer: ManuallyDrop<FemtoVGRenderer>,
    physical_size: Cell<PhysicalSize>,
    dpi: Cell<u32>,
    visible: Cell<bool>,
    redraw_pending: Cell<bool>,
    mouse_leave_tracked: Cell<bool>,
    pointer_captured: Cell<bool>,
    has_explicit_size: Cell<bool>,
    preferred_size_applied: Cell<bool>,
    constraints: Cell<LayoutConstraints>,
    content_kind: Cell<WindowContentKind>,
    character_keys: RefCell<HashMap<u32, SharedString>>,
    pending_high_surrogate: Cell<Option<u16>>,
    render_error: RefCell<Option<String>>,
}

impl NativeWindow {
    fn scale_factor(&self) -> f32 {
        dpi_scale(self.dpi.get())
    }

    fn dispatch(&self, event: WindowEvent) {
        if let Err(error) = self.window.try_dispatch_event(event) {
            eprintln!("Slint window event failed: {error}");
        }
    }

    fn dispatch_size(&self, size: PhysicalSize) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.physical_size.set(size);
        self.dispatch(WindowEvent::Resized {
            size: size.to_logical(self.scale_factor()),
        });
    }

    fn sync_dpi_and_size(&self) {
        let dpi = window_dpi(self.hwnd);
        if self.dpi.replace(dpi) != dpi {
            self.dispatch(WindowEvent::ScaleFactorChanged {
                scale_factor: dpi_scale(dpi),
            });
        }
        if let Some(size) = client_size(self.hwnd) {
            self.dispatch_size(size);
        }
    }

    fn render(&self) {
        self.redraw_pending.set(false);
        let size = self.physical_size.get();
        if size.width == 0 || size.height == 0 {
            return;
        }
        if let Err(error) = self.renderer.render() {
            let message = error.to_string();
            let mut previous = self.render_error.borrow_mut();
            if previous.as_deref() != Some(message.as_str()) {
                eprintln!("render Slint FemtoVG window failed: {message}");
                *previous = Some(message);
            }
        } else {
            self.render_error.borrow_mut().take();
        }
    }

    fn invalidate(&self) {
        if !self.redraw_pending.replace(true) {
            unsafe { InvalidateRect(self.hwnd, null(), 0) };
        }
    }

    fn pointer_position(&self, lparam: LPARAM) -> LogicalPosition {
        let physical = PhysicalPosition::new(signed_low_word(lparam), signed_high_word(lparam));
        physical.to_logical(self.scale_factor())
    }

    fn wheel_position(&self, lparam: LPARAM) -> LogicalPosition {
        let mut point = POINT {
            x: signed_low_word(lparam),
            y: signed_high_word(lparam),
        };
        unsafe { ScreenToClient(self.hwnd, &mut point) };
        PhysicalPosition::new(point.x, point.y).to_logical(self.scale_factor())
    }

    fn track_mouse_leave(&self) {
        if self.mouse_leave_tracked.replace(true) {
            return;
        }
        let mut tracking = TRACKMOUSEEVENT {
            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: self.hwnd,
            dwHoverTime: 0,
        };
        if unsafe { TrackMouseEvent(&mut tracking) } == 0 {
            self.mouse_leave_tracked.set(false);
        }
    }

    fn pointer_pressed(&self, lparam: LPARAM, button: PointerEventButton) {
        unsafe { SetCapture(self.hwnd) };
        self.pointer_captured.set(true);
        self.dispatch(WindowEvent::PointerPressed {
            position: self.pointer_position(lparam),
            button,
        });
    }

    fn pointer_released(&self, wparam: WPARAM, lparam: LPARAM, button: PointerEventButton) {
        self.dispatch(WindowEvent::PointerReleased {
            position: self.pointer_position(lparam),
            button,
        });
        let down_mask = MK_LBUTTON | MK_RBUTTON | MK_MBUTTON | XBUTTON_MASK;
        if wparam as u32 & down_mask == 0 {
            self.pointer_captured.set(false);
            unsafe { ReleaseCapture() };
            let position = self.pointer_position(lparam);
            let size = self.physical_size.get().to_logical(self.scale_factor());
            if position.x < 0.0
                || position.y < 0.0
                || position.x >= size.width
                || position.y >= size.height
            {
                self.dispatch(WindowEvent::PointerExited);
            }
        }
    }

    fn dispatch_wheel(&self, lparam: LPARAM, wparam: WPARAM, horizontal: bool) {
        let wheel_delta = signed_high_word(wparam as LPARAM) as f32;
        let logical_delta = wheel_delta / WHEEL_DELTA * WHEEL_LOGICAL_PIXELS_PER_NOTCH;
        let (delta_x, delta_y) = if horizontal {
            (logical_delta, 0.0)
        } else {
            (0.0, logical_delta)
        };
        self.dispatch(WindowEvent::PointerScrolled {
            position: self.wheel_position(lparam),
            delta_x,
            delta_y,
        });
    }

    fn dispatch_key(&self, virtual_key: u16, lparam: LPARAM, pressed: bool) -> bool {
        let scan_code = scan_code(lparam);
        let shortcut_text = (pressed
            && unsafe { GetKeyState(VK_CONTROL as i32) } < 0
            && ((b'0' as u16..=b'9' as u16).contains(&virtual_key)
                || (b'A' as u16..=b'Z' as u16).contains(&virtual_key)))
        .then(|| SharedString::from(char::from_u32(u32::from(virtual_key)).unwrap_or_default()));
        if let Some(text) = shortcut_text.as_ref() {
            self.character_keys
                .borrow_mut()
                .insert(scan_code, text.clone());
        }
        let Some(text) = mapped_key(virtual_key, lparam)
            .or(shortcut_text)
            .or_else(|| {
                (!pressed)
                    .then(|| self.character_keys.borrow_mut().remove(&scan_code))
                    .flatten()
            })
        else {
            return false;
        };
        let event = if pressed {
            if key_is_repeat(lparam) {
                WindowEvent::KeyPressRepeated { text }
            } else {
                WindowEvent::KeyPressed { text }
            }
        } else {
            WindowEvent::KeyReleased { text }
        };
        self.dispatch(event);
        true
    }

    fn dispatch_utf16_character(&self, code_unit: u16, lparam: LPARAM) {
        if (0xd800..=0xdbff).contains(&code_unit) {
            self.pending_high_surrogate.set(Some(code_unit));
            return;
        }
        let character = if (0xdc00..=0xdfff).contains(&code_unit) {
            let Some(high) = self.pending_high_surrogate.take() else {
                return;
            };
            char::decode_utf16([high, code_unit])
                .next()
                .and_then(Result::ok)
        } else {
            self.pending_high_surrogate.take();
            char::from_u32(u32::from(code_unit))
        };
        if let Some(character) = character {
            self.dispatch_character(character, lparam);
        }
    }

    fn dispatch_character(&self, character: char, lparam: LPARAM) {
        if character.is_control() {
            return;
        }
        let text = SharedString::from(character);
        self.character_keys
            .borrow_mut()
            .insert(scan_code(lparam), text.clone());
        self.dispatch(if key_is_repeat(lparam) {
            WindowEvent::KeyPressRepeated { text }
        } else {
            WindowEvent::KeyPressed { text }
        });
    }

    fn hit_test(&self, lparam: LPARAM) -> LRESULT {
        let mut bounds = RECT::default();
        if unsafe { GetWindowRect(self.hwnd, &mut bounds) } == 0 {
            return HTCLIENT as LRESULT;
        }
        let x = signed_low_word(lparam) - bounds.left;
        let y = signed_high_word(lparam) - bounds.top;
        let width = bounds.right - bounds.left;
        let height = bounds.bottom - bounds.top;
        let maximized = unsafe { IsZoomed(self.hwnd) } != 0;
        let border = if maximized {
            0
        } else {
            resize_border(self.dpi.get())
        };
        let titlebar_height =
            logical_length_to_physical(TITLEBAR_LOGICAL_HEIGHT, self.scale_factor());
        let titlebar_buttons_width =
            logical_length_to_physical(TITLEBAR_BUTTONS_LOGICAL_WIDTH, self.scale_factor());
        hit_test_region(
            width,
            height,
            x,
            y,
            border,
            titlebar_height,
            titlebar_buttons_width,
        ) as LRESULT
    }

    fn apply_client_cursor(&self) {
        let mut point = POINT::default();
        if unsafe { GetCursorPos(&mut point) } == 0
            || unsafe { ScreenToClient(self.hwnd, &mut point) } == 0
        {
            return;
        }
        let size = self.physical_size.get();
        let cursor = client_cursor(
            self.content_kind.get(),
            i32::try_from(size.width).unwrap_or(i32::MAX),
            i32::try_from(size.height).unwrap_or(i32::MAX),
            point.x,
            point.y,
            self.scale_factor(),
        );
        unsafe { SetCursor(LoadCursorW(null_mut(), cursor.resource())) };
    }

    fn apply_min_max_constraints(&self, lparam: LPARAM) {
        let info = unsafe { &mut *(lparam as *mut MINMAXINFO) };
        let constraints = self.constraints.get();
        let scale = self.scale_factor();
        if let Some(minimum) = constraints.min {
            info.ptMinTrackSize.x = logical_length_to_physical(minimum.width, scale);
            info.ptMinTrackSize.y = logical_length_to_physical(minimum.height, scale);
        }
        if let Some(maximum) = constraints.max {
            info.ptMaxTrackSize.x = logical_length_to_physical(maximum.width, scale);
            info.ptMaxTrackSize.y = logical_length_to_physical(maximum.height, scale);
        }
    }

    fn set_physical_size(&self, size: PhysicalSize) {
        let width = i32::try_from(size.width.max(1)).unwrap_or(i32::MAX);
        let height = i32::try_from(size.height.max(1)).unwrap_or(i32::MAX);
        unsafe {
            SetWindowPos(
                self.hwnd,
                null_mut(),
                0,
                0,
                width,
                height,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
    }

    fn handle_system_command(&self, command: u32) -> bool {
        match command & 0xfff0 {
            SC_CLOSE => self.dispatch(WindowEvent::CloseRequested),
            SC_MINIMIZE => self.window.set_minimized(true),
            SC_MAXIMIZE => self.window.set_maximized(true),
            SC_RESTORE => {
                if unsafe { IsIconic(self.hwnd) } != 0 {
                    self.window.set_minimized(false);
                }
                if unsafe { IsZoomed(self.hwnd) } != 0 {
                    self.window.set_maximized(false);
                }
            }
            _ => return false,
        }
        true
    }
}

impl WindowAdapter for NativeWindow {
    fn window(&self) -> &slint::Window {
        &self.window
    }

    fn set_visible(&self, visible: bool) -> Result<(), PlatformError> {
        if self.visible.replace(visible) == visible {
            return Ok(());
        }
        if visible {
            self.sync_dpi_and_size();
            unsafe { ShowWindow(self.hwnd, SW_SHOW) };
            self.invalidate();
        } else {
            unsafe { ShowWindow(self.hwnd, SW_HIDE) };
            self.redraw_pending.set(false);
        }
        Ok(())
    }

    fn position(&self) -> Option<PhysicalPosition> {
        let mut bounds = RECT::default();
        (unsafe { GetWindowRect(self.hwnd, &mut bounds) } != 0)
            .then(|| PhysicalPosition::new(bounds.left, bounds.top))
    }

    fn set_position(&self, position: WindowPosition) {
        let position = position.to_physical(self.scale_factor());
        unsafe {
            SetWindowPos(
                self.hwnd,
                null_mut(),
                position.x,
                position.y,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
    }

    fn set_size(&self, size: WindowSize) {
        self.has_explicit_size.set(true);
        self.set_physical_size(size.to_physical(self.scale_factor()));
    }

    fn size(&self) -> PhysicalSize {
        self.physical_size.get()
    }

    fn request_redraw(&self) {
        self.invalidate();
    }

    fn renderer(&self) -> &dyn Renderer {
        &*self.renderer
    }

    fn update_window_properties(&self, properties: WindowProperties<'_>) {
        let title = wide(properties.title().as_str());
        self.content_kind
            .set(WindowContentKind::from_title(properties.title().as_str()));
        unsafe { SetWindowTextW(self.hwnd, title.as_ptr()) };

        let constraints = properties.layout_constraints();
        self.constraints.set(constraints);
        if !self.has_explicit_size.get()
            && !self.preferred_size_applied.replace(true)
            && constraints.preferred.width > 0.0
            && constraints.preferred.height > 0.0
        {
            self.set_physical_size(constraints.preferred.to_physical(self.scale_factor()));
        }

        if properties.is_minimized() {
            unsafe { ShowWindow(self.hwnd, SW_MINIMIZE) };
        } else if properties.is_fullscreen() || properties.is_maximized() {
            unsafe { ShowWindow(self.hwnd, SW_MAXIMIZE) };
        } else if unsafe { IsIconic(self.hwnd) } != 0 || unsafe { IsZoomed(self.hwnd) } != 0 {
            unsafe { ShowWindow(self.hwnd, SW_RESTORE) };
        }
        self.sync_dpi_and_size();
        self.invalidate();
    }
}

impl Drop for NativeWindow {
    fn drop(&mut self) {
        unsafe {
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            ManuallyDrop::drop(&mut self.renderer);
            if IsWindow(self.hwnd) != 0 {
                DestroyWindow(self.hwnd);
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
    let adapter_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const NativeWindow;
    if adapter_ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    let adapter = unsafe { &*adapter_ptr };

    match message {
        WM_PAINT => {
            let mut paint: PAINTSTRUCT = unsafe { zeroed() };
            unsafe { BeginPaint(hwnd, &mut paint) };
            adapter.render();
            unsafe { EndPaint(hwnd, &paint) };
            0
        }
        WM_ERASEBKGND => 1,
        WM_NCCALCSIZE => 0,
        WM_NCHITTEST => adapter.hit_test(lparam),
        WM_SETCURSOR if low_word(lparam) == HTCLIENT => {
            adapter.apply_client_cursor();
            1
        }
        WM_GETMINMAXINFO => {
            adapter.apply_min_max_constraints(lparam);
            0
        }
        WM_NCLBUTTONDBLCLK if wparam as u32 == HTCAPTION => {
            adapter.window.set_maximized(unsafe { IsZoomed(hwnd) } == 0);
            0
        }
        WM_SYSCOMMAND if adapter.handle_system_command(wparam as u32) => 0,
        WM_CLOSE => {
            adapter.dispatch(WindowEvent::CloseRequested);
            0
        }
        WM_DESTROY => {
            adapter.visible.set(false);
            adapter.redraw_pending.set(false);
            0
        }
        WM_SETFOCUS => {
            adapter.dispatch(WindowEvent::WindowActiveChanged(true));
            0
        }
        WM_KILLFOCUS => {
            adapter.character_keys.borrow_mut().clear();
            adapter.pending_high_surrogate.set(None);
            adapter.dispatch(WindowEvent::WindowActiveChanged(false));
            0
        }
        WM_CANCELMODE | WM_CAPTURECHANGED => {
            if adapter.pointer_captured.replace(false) {
                adapter.dispatch(WindowEvent::PointerExited);
            }
            0
        }
        WM_SIZE => {
            if wparam as u32 != SIZE_MINIMIZED {
                adapter.dispatch_size(PhysicalSize::new(low_word(lparam), high_word(lparam)));
                adapter.invalidate();
            }
            0
        }
        WM_DPICHANGED => {
            let dpi = low_word(wparam as LPARAM).max(1);
            adapter.dpi.set(dpi);
            adapter.dispatch(WindowEvent::ScaleFactorChanged {
                scale_factor: dpi_scale(dpi),
            });
            let suggested = unsafe { &*(lparam as *const RECT) };
            unsafe {
                SetWindowPos(
                    hwnd,
                    null_mut(),
                    suggested.left,
                    suggested.top,
                    suggested.right - suggested.left,
                    suggested.bottom - suggested.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )
            };
            if let Some(size) = client_size(hwnd) {
                adapter.dispatch_size(size);
            }
            adapter.invalidate();
            0
        }
        WM_MOUSEMOVE => {
            adapter.track_mouse_leave();
            adapter.dispatch(WindowEvent::PointerMoved {
                position: adapter.pointer_position(lparam),
            });
            0
        }
        WM_MOUSELEAVE => {
            adapter.mouse_leave_tracked.set(false);
            if !adapter.pointer_captured.get() {
                adapter.dispatch(WindowEvent::PointerExited);
            }
            0
        }
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
            adapter.pointer_pressed(lparam, PointerEventButton::Left);
            0
        }
        WM_LBUTTONUP => {
            adapter.pointer_released(wparam, lparam, PointerEventButton::Left);
            0
        }
        WM_RBUTTONDOWN | WM_RBUTTONDBLCLK => {
            adapter.pointer_pressed(lparam, PointerEventButton::Right);
            0
        }
        WM_RBUTTONUP => {
            adapter.pointer_released(wparam, lparam, PointerEventButton::Right);
            0
        }
        WM_MBUTTONDOWN | WM_MBUTTONDBLCLK => {
            adapter.pointer_pressed(lparam, PointerEventButton::Middle);
            0
        }
        WM_MBUTTONUP => {
            adapter.pointer_released(wparam, lparam, PointerEventButton::Middle);
            0
        }
        WM_XBUTTONDOWN | WM_XBUTTONDBLCLK => {
            adapter.pointer_pressed(lparam, xbutton(wparam));
            1
        }
        WM_XBUTTONUP => {
            adapter.pointer_released(wparam, lparam, xbutton(wparam));
            1
        }
        WM_MOUSEWHEEL => {
            adapter.dispatch_wheel(lparam, wparam, false);
            0
        }
        WM_MOUSEHWHEEL => {
            adapter.dispatch_wheel(lparam, wparam, true);
            0
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let handled = adapter.dispatch_key(wparam as u16, lparam, true);
            if message == WM_SYSKEYDOWN || !handled {
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            } else {
                0
            }
        }
        WM_KEYUP | WM_SYSKEYUP => {
            let handled = adapter.dispatch_key(wparam as u16, lparam, false);
            if message == WM_SYSKEYUP || !handled {
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            } else {
                0
            }
        }
        WM_CHAR | WM_SYSCHAR => {
            adapter.dispatch_utf16_character(wparam as u16, lparam);
            0
        }
        WM_UNICHAR if wparam as u32 == UNICODE_NOCHAR => 1,
        WM_UNICHAR => {
            if let Some(character) = char::from_u32(wparam as u32) {
                adapter.dispatch_character(character, lparam);
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn mapped_key(virtual_key: u16, lparam: LPARAM) -> Option<SharedString> {
    let key = match virtual_key {
        VK_BACK => Key::Backspace,
        VK_TAB => Key::Tab,
        VK_RETURN => Key::Return,
        VK_ESCAPE => Key::Escape,
        VK_DELETE => Key::Delete,
        VK_SHIFT => {
            let scan = (lparam as u32 >> 16) & 0xff;
            if unsafe { MapVirtualKeyW(scan, MAPVK_VSC_TO_VK_EX) } as u16 == VK_RSHIFT {
                Key::ShiftR
            } else {
                Key::Shift
            }
        }
        VK_LSHIFT => Key::Shift,
        VK_RSHIFT => Key::ShiftR,
        VK_CONTROL | VK_LCONTROL => {
            if key_is_extended(lparam) {
                Key::ControlR
            } else {
                Key::Control
            }
        }
        VK_RCONTROL => Key::ControlR,
        VK_MENU | VK_LMENU => {
            if key_is_extended(lparam) {
                Key::AltGr
            } else {
                Key::Alt
            }
        }
        VK_RMENU => Key::AltGr,
        VK_LWIN => Key::Meta,
        VK_RWIN => Key::MetaR,
        VK_CAPITAL => Key::CapsLock,
        VK_UP => Key::UpArrow,
        VK_DOWN => Key::DownArrow,
        VK_LEFT => Key::LeftArrow,
        VK_RIGHT => Key::RightArrow,
        VK_INSERT => Key::Insert,
        VK_HOME => Key::Home,
        VK_END => Key::End,
        VK_PRIOR => Key::PageUp,
        VK_NEXT => Key::PageDown,
        VK_SCROLL => Key::ScrollLock,
        VK_PAUSE => Key::Pause,
        _ if (VK_F1..=VK_F24).contains(&virtual_key) => {
            let code = char::from(Key::F1) as u32 + u32::from(virtual_key - VK_F1);
            return char::from_u32(code).map(SharedString::from);
        }
        _ => return None,
    };
    Some(key.into())
}

fn xbutton(wparam: WPARAM) -> PointerEventButton {
    if high_word(wparam as LPARAM) as u16 == XBUTTON1 {
        PointerEventButton::Back
    } else if high_word(wparam as LPARAM) as u16 == XBUTTON2 {
        PointerEventButton::Forward
    } else {
        PointerEventButton::Other
    }
}

fn hit_test_region(
    width: i32,
    height: i32,
    x: i32,
    y: i32,
    border: i32,
    titlebar_height: i32,
    titlebar_buttons_width: i32,
) -> u32 {
    let left = border > 0 && x >= 0 && x < border;
    let right = border > 0 && x < width && x >= width - border;
    let top = border > 0 && y >= 0 && y < border;
    let bottom = border > 0 && y < height && y >= height - border;
    match (left, right, top, bottom) {
        (true, _, true, _) => HTTOPLEFT,
        (_, true, true, _) => HTTOPRIGHT,
        (true, _, _, true) => HTBOTTOMLEFT,
        (_, true, _, true) => HTBOTTOMRIGHT,
        (true, _, _, _) => HTLEFT,
        (_, true, _, _) => HTRIGHT,
        (_, _, true, _) => HTTOP,
        (_, _, _, true) => HTBOTTOM,
        _ if x >= 0
            && x < (width - titlebar_buttons_width).max(0)
            && y >= 0
            && y < titlebar_height =>
        {
            HTCAPTION
        }
        _ => HTCLIENT,
    }
}

fn resize_border(dpi: u32) -> i32 {
    let frame = unsafe { GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi.max(96)) };
    let padding = unsafe { GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi.max(96)) };
    (frame + padding).max(1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowContentKind {
    Other,
    Preview,
    OcrPreview,
}

impl WindowContentKind {
    fn from_title(title: &str) -> Self {
        match title {
            "Patrick Star" => Self::Preview,
            "截图预览" => Self::OcrPreview,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClientCursor {
    Arrow,
    Hand,
    Crosshair,
    Text,
}

impl ClientCursor {
    const fn resource(self) -> windows_sys::core::PCWSTR {
        match self {
            Self::Arrow => IDC_ARROW,
            Self::Hand => IDC_HAND,
            Self::Crosshair => IDC_CROSS,
            Self::Text => IDC_IBEAM,
        }
    }
}

fn client_cursor(
    kind: WindowContentKind,
    width: i32,
    height: i32,
    x: i32,
    y: i32,
    scale_factor: f32,
) -> ClientCursor {
    if x < 0 || y < 0 || x >= width || y >= height {
        return ClientCursor::Arrow;
    }
    let titlebar_height = logical_length_to_physical(TITLEBAR_LOGICAL_HEIGHT, scale_factor);
    let titlebar_buttons = logical_length_to_physical(TITLEBAR_BUTTONS_LOGICAL_WIDTH, scale_factor);
    if y < titlebar_height && x >= (width - titlebar_buttons).max(0) {
        return ClientCursor::Hand;
    }
    if !matches!(
        kind,
        WindowContentKind::Preview | WindowContentKind::OcrPreview
    ) {
        return ClientCursor::Arrow;
    }

    let header_height = logical_length_to_physical(PREVIEW_HEADER_LOGICAL_HEIGHT, scale_factor);
    if y >= titlebar_height && y < header_height {
        return ClientCursor::Hand;
    }
    let status_height = logical_length_to_physical(PREVIEW_STATUS_LOGICAL_HEIGHT, scale_factor);
    if y < header_height || y >= height.saturating_sub(status_height) {
        return ClientCursor::Arrow;
    }
    if kind == WindowContentKind::OcrPreview {
        let logical_width = width as f32 / scale_factor.max(0.01);
        let panel_width =
            (logical_width * 0.3).clamp(OCR_PANEL_MIN_LOGICAL_WIDTH, OCR_PANEL_MAX_LOGICAL_WIDTH);
        let panel_width = logical_length_to_physical(panel_width, scale_factor);
        if x >= width.saturating_sub(panel_width) {
            return ClientCursor::Text;
        }
    }
    ClientCursor::Crosshair
}

fn client_size(hwnd: HWND) -> Option<PhysicalSize> {
    let mut bounds = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut bounds) } == 0 {
        return None;
    }
    Some(PhysicalSize::new(
        u32::try_from((bounds.right - bounds.left).max(0)).ok()?,
        u32::try_from((bounds.bottom - bounds.top).max(0)).ok()?,
    ))
}

fn window_dpi(hwnd: HWND) -> u32 {
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 { system_dpi() } else { dpi }
}

fn system_dpi() -> u32 {
    let dpi = unsafe { GetDpiForSystem() };
    dpi.max(96)
}

fn dpi_scale(dpi: u32) -> f32 {
    dpi.max(1) as f32 / 96.0
}

fn logical_length_to_physical(length: f32, scale_factor: f32) -> i32 {
    (length.max(1.0) * scale_factor)
        .round()
        .clamp(1.0, i32::MAX as f32) as i32
}

fn low_word(value: LPARAM) -> u32 {
    value as u32 & 0xffff
}

fn high_word(value: LPARAM) -> u32 {
    value as u32 >> 16 & 0xffff
}

fn signed_low_word(value: LPARAM) -> i32 {
    low_word(value) as u16 as i16 as i32
}

fn signed_high_word(value: LPARAM) -> i32 {
    high_word(value) as u16 as i16 as i32
}

fn scan_code(lparam: LPARAM) -> u32 {
    high_word(lparam) & 0x01ff
}

fn key_is_repeat(lparam: LPARAM) -> bool {
    lparam as usize & (1 << 30) != 0
}

fn key_is_extended(lparam: LPARAM) -> bool {
    lparam as usize & (1 << 24) != 0
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn platform_error(message: impl Into<String>) -> PlatformError {
    PlatformError::Other(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_lengths_follow_dpi_scale() {
        assert_eq!(logical_length_to_physical(100.0, 1.0), 100);
        assert_eq!(logical_length_to_physical(100.0, 1.5), 150);
        assert_eq!(logical_length_to_physical(0.0, 2.0), 2);
    }

    #[test]
    fn resize_edges_take_priority_over_custom_titlebar() {
        assert_eq!(hit_test_region(1080, 720, 2, 2, 8, 36, 132), HTTOPLEFT);
        assert_eq!(hit_test_region(1080, 720, 500, 2, 8, 36, 132), HTTOP);
        assert_eq!(hit_test_region(1080, 720, 500, 20, 8, 36, 132), HTCAPTION);
    }

    #[test]
    fn titlebar_buttons_remain_slint_client_area() {
        assert_eq!(hit_test_region(1080, 720, 1000, 20, 8, 36, 132), HTCLIENT);
        assert_eq!(hit_test_region(1080, 720, 500, 100, 8, 36, 132), HTCLIENT);
    }

    #[test]
    fn ocr_preview_uses_native_cursors_for_commands_canvas_and_text() {
        let kind = WindowContentKind::OcrPreview;
        assert_eq!(
            client_cursor(kind, 1080, 720, 1000, 20, 1.0),
            ClientCursor::Hand
        );
        assert_eq!(
            client_cursor(kind, 1080, 720, 200, 60, 1.0),
            ClientCursor::Hand
        );
        assert_eq!(
            client_cursor(kind, 1080, 720, 400, 300, 1.0),
            ClientCursor::Crosshair
        );
        assert_eq!(
            client_cursor(kind, 1080, 720, 900, 300, 1.0),
            ClientCursor::Text
        );
        assert_eq!(
            client_cursor(kind, 1080, 720, 900, 710, 1.0),
            ClientCursor::Arrow
        );
    }

    #[test]
    fn ocr_cursor_regions_scale_with_window_dpi() {
        let kind = WindowContentKind::OcrPreview;
        assert_eq!(
            client_cursor(kind, 1620, 1080, 300, 90, 1.5),
            ClientCursor::Hand
        );
        assert_eq!(
            client_cursor(kind, 1620, 1080, 1350, 450, 1.5),
            ClientCursor::Text
        );
    }

    #[test]
    fn maximized_window_has_no_resize_edge() {
        assert_eq!(hit_test_region(1080, 720, 2, 2, 0, 36, 132), HTCAPTION);
    }

    #[test]
    fn shift_virtual_key_uses_extended_scan_mapping() {
        let scan = 0x36usize << 16;
        let mapped = unsafe { MapVirtualKeyW(0x36, MAPVK_VSC_TO_VK_EX) } as u16;
        assert_eq!(mapped, VK_RSHIFT);
        assert_eq!(mapped_key(mapped, scan as LPARAM), Some(Key::ShiftR.into()));
    }
}
