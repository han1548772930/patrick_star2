mod capture;
mod capture_exclusion;
mod clipboard;
mod cursor;
mod folder_dialog;
pub(crate) mod hotkey;
mod ocr;
mod overlay;
mod pin;
mod save_dialog;
mod scroll_capture;
mod scroll_overlay;
mod scroll_preview;
mod single_instance;
mod slint_event_loop;
mod slint_window;
mod wgl;
mod window_locator;

use std::ptr::{null, null_mut};
use std::rc::Rc;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::Graphics::Dwm::DwmFlush;
use windows_sys::Win32::Graphics::Gdi::{
    MONITOR_DEFAULTTONEAREST, MonitorFromPoint, RDW_ALLCHILDREN, RDW_ERASE, RDW_INVALIDATE,
    RDW_UPDATENOW, RedrawWindow,
};
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI,
    SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::WindowsAndMessaging::GetDesktopWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
};

use crate::model::{DesktopFrame, PointI, RgbaFrame};
use crate::ocr::{OcrDocument, OcrLanguage, TextRecognizer};
use crate::platform::{
    ActiveScrollCapture, Availability, Capabilities, CaptureOverlay, CaptureOverlayResult,
    DesktopCapture, DirectoryPicker, GlobalShortcutHost, GlobalShortcutRegistration,
    ImageClipboard, ImageSaveDialog, ImageSaveTarget, PinnedImageHost, PlatformCapabilities,
    ScrollCaptureSource, ScrollPreview, ScrollPreviewHost, Shortcut, SingleInstanceGuard,
    SingleInstanceHost, TextClipboard, WindowFrame, WindowFrameAnchor, WindowFrameConfig,
    WindowFrameEvent, WindowFrameHost,
};

pub struct Backend;

pub(crate) fn ui_font_paths() -> Vec<std::path::PathBuf> {
    let Some(directory) = std::env::var_os("WINDIR")
        .map(std::path::PathBuf::from)
        .map(|windows| windows.join("Fonts"))
    else {
        return Vec::new();
    };
    ["msyh.ttc", "segoeui.ttf", "seguiemj.ttf"]
        .into_iter()
        .map(|name| directory.join(name))
        .filter(|path| path.is_file())
        .collect()
}

pub(super) fn set_preview_cursor(
    window: &slint::Window,
    cursor: crate::model::PointerCursor,
    popup: Option<crate::model::Rect>,
) {
    slint_window::set_preview_cursor(window, cursor, popup);
}

pub(super) fn set_slint_window_topmost(window: &slint::Window, topmost: bool) -> anyhow::Result<()> {
    let persistent = window.window_handle();
    let borrowed = persistent
        .window_handle()
        .map_err(|error| anyhow::anyhow!("Slint window handle is unavailable: {error}"))?;
    let RawWindowHandle::Win32(handle) = borrowed.as_raw() else {
        anyhow::bail!("Slint window is not backed by a Win32 HWND");
    };
    let insert_after = if topmost { HWND_TOPMOST } else { HWND_NOTOPMOST };
    let changed = unsafe {
        SetWindowPos(
            handle.hwnd.get() as _,
            insert_after,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
    };
    anyhow::ensure!(changed != 0, "SetWindowPos failed while changing topmost state");
    Ok(())
}

pub(super) fn refresh_desktop_after_overlay() {
    unsafe {
        let desktop = GetDesktopWindow();
        if !desktop.is_null() {
            RedrawWindow(
                desktop,
                null(),
                null_mut(),
                RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_UPDATENOW,
            );
        }
        let _ = DwmFlush();
    }
}

pub(super) fn dpi_scale_at(point: PointI, fallback: f32) -> f32 {
    let monitor = unsafe {
        MonitorFromPoint(
            POINT {
                x: point.x,
                y: point.y,
            },
            MONITOR_DEFAULTTONEAREST,
        )
    };
    if monitor.is_null() {
        return fallback;
    }
    let mut dpi_x = 0;
    let mut dpi_y = 0;
    let result = unsafe {
        GetDpiForMonitor(
            monitor,
            MDT_EFFECTIVE_DPI,
            &mut dpi_x,
            &mut dpi_y,
        )
    };
    if result >= 0 && dpi_x > 0 {
        dpi_x as f32 / 96.0
    } else {
        fallback
    }
}

pub fn install_slint_platform() -> anyhow::Result<Box<dyn GlobalShortcutHost>> {
    // DPI awareness is process-wide and must be selected before the tray
    // backend creates its hidden HWND. Setting it later is rejected by Windows.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let hotkeys = Rc::new(hotkey::Host::new());
    slint::platform::set_platform(Box::new(slint_event_loop::WindowsSlintPlatform::new(
        hotkeys.clone(),
    )))
    .map_err(|error| anyhow::anyhow!(error))?;
    Ok(Box::new(RuntimeShortcutHost(hotkeys)))
}

struct RuntimeShortcutHost(Rc<hotkey::Host>);

impl GlobalShortcutHost for RuntimeShortcutHost {
    fn register_global_shortcut(
        &self,
        shortcut: Shortcut,
        callback: Box<dyn FnMut() + 'static>,
    ) -> anyhow::Result<Box<dyn GlobalShortcutRegistration>> {
        self.0.register_global_shortcut(shortcut, callback)
    }
}

impl DesktopCapture for Backend {
    fn capture_virtual_desktop(&self) -> anyhow::Result<DesktopFrame> {
        capture::capture_virtual_desktop()
    }
}

impl CaptureOverlay for Backend {
    fn run_capture_overlay(
        &self,
        frame: DesktopFrame,
        features: crate::model::OverlayFeatures,
    ) -> anyhow::Result<CaptureOverlayResult> {
        overlay::run(frame, features)
    }
}

impl SingleInstanceHost for Backend {
    fn acquire_single_instance(&self) -> anyhow::Result<Option<Box<dyn SingleInstanceGuard>>> {
        single_instance::acquire()
    }
}

impl ImageClipboard for Backend {
    fn write_image(&self, image: &RgbaFrame) -> anyhow::Result<()> {
        clipboard::write_image(image)
    }
}

impl TextClipboard for Backend {
    fn write_text(&self, text: &str) -> anyhow::Result<()> {
        clipboard::write_text(text)
    }
}

impl TextRecognizer for Backend {
    fn available_languages(&self) -> anyhow::Result<Vec<OcrLanguage>> {
        ocr::available_languages()
    }

    fn recognize_text(
        &self,
        image: &RgbaFrame,
        language_tag: Option<&str>,
    ) -> anyhow::Result<OcrDocument> {
        ocr::recognize(image, language_tag)
    }
}

impl ImageSaveDialog for Backend {
    fn choose_image_target(
        &self,
        initial_directory: Option<&std::path::Path>,
    ) -> anyhow::Result<Option<ImageSaveTarget>> {
        save_dialog::choose_image_target(initial_directory)
    }
}

impl DirectoryPicker for Backend {
    fn choose_directory(
        &self,
        initial_directory: &std::path::Path,
    ) -> anyhow::Result<Option<std::path::PathBuf>> {
        folder_dialog::choose_directory(initial_directory)
    }
}

impl PinnedImageHost for Backend {
    fn show_pinned_image(&self, image: RgbaFrame) -> anyhow::Result<()> {
        pin::show(image)
    }
}

impl ScrollCaptureSource for Backend {
    fn start_scroll_capture(
        &self,
        bounds: crate::model::RectI,
    ) -> anyhow::Result<Box<dyn ActiveScrollCapture>> {
        scroll_capture::start(bounds)
    }

    fn cancel_scroll_capture(&self) -> anyhow::Result<()> {
        scroll_capture::cancel_active()
    }
}

impl ScrollPreviewHost for Backend {
    fn open_scroll_preview(
        &self,
        desktop: &DesktopFrame,
        initial: &RgbaFrame,
    ) -> anyhow::Result<Box<dyn ScrollPreview>> {
        scroll_preview::open(desktop, initial)
    }
}

struct WindowsWindowFrame {
    _controller: slint_borderless::FrameController,
}

impl WindowFrame for WindowsWindowFrame {}

impl WindowFrameHost for Backend {
    fn attach_window_frame(
        &self,
        window: &slint::Window,
        config: WindowFrameConfig,
        on_event: Box<dyn Fn(WindowFrameEvent) + 'static>,
    ) -> anyhow::Result<Box<dyn WindowFrame>> {
        let always_on_top = config.always_on_top;
        let client_areas = config
            .client_areas
            .into_iter()
            .map(|area| match area.anchor {
                WindowFrameAnchor::Left => {
                    slint_borderless::ClientArea::left(area.x, area.y, area.width, area.height)
                }
                WindowFrameAnchor::Right => {
                    slint_borderless::ClientArea::right(area.x, area.y, area.width, area.height)
                }
            })
            .collect();
        let frame = slint_borderless::FrameController::attach_window(
            window,
            slint_borderless::FrameOptions {
                titlebar_height: config.titlebar_height,
                caption_button_width: config.caption_button_width,
                minimum_size: Some(slint_borderless::LogicalSize {
                    width: config.minimum_width,
                    height: config.minimum_height,
                }),
                rounded_corners: config.rounded_corners,
                client_areas,
            },
        )?;
        frame.on_event(move |event| {
            let event = match event {
                slint_borderless::FrameEvent::Installed => WindowFrameEvent::Installed,
                slint_borderless::FrameEvent::CaptionHoverChanged(button) => {
                    WindowFrameEvent::CaptionHoverChanged(button.map(|button| match button {
                        slint_borderless::CaptionButton::Minimize => {
                            crate::platform::CaptionButton::Minimize
                        }
                        slint_borderless::CaptionButton::Maximize => {
                            crate::platform::CaptionButton::Maximize
                        }
                        slint_borderless::CaptionButton::Close => {
                            crate::platform::CaptionButton::Close
                        }
                    }))
                }
                slint_borderless::FrameEvent::Detached => WindowFrameEvent::Detached,
                slint_borderless::FrameEvent::Failed(error) => {
                    WindowFrameEvent::Failed(error.to_string())
                }
            };
            on_event(event);
        });
        if always_on_top {
            set_slint_window_topmost(window, true)?;
        }
        Ok(Box::new(WindowsWindowFrame { _controller: frame }))
    }
}

impl PlatformCapabilities for Backend {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            desktop_capture: Availability::Native,
            window_detection: Availability::Native,
            image_clipboard: Availability::Native,
            text_clipboard: Availability::Native,
            image_save: Availability::Native,
            pinned_image: Availability::Native,
            text_recognition: Availability::Native,
            scroll_capture_source: Availability::Native,
            scroll_preview: Availability::Native,
            global_shortcut: Availability::Native,
            tray: Availability::Native,
            capture_exclusion: Availability::Native,
            window_frame: Availability::Native,
        }
    }
}
