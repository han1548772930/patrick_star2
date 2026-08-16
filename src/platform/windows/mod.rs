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

use std::rc::Rc;

use crate::model::{DesktopFrame, RgbaFrame};
use crate::ocr::{OcrDocument, OcrLanguage, TextRecognizer};
use crate::platform::{
    ActiveScrollCapture, Availability, Capabilities, CaptureOverlay, CaptureOverlayResult,
    DesktopCapture, DirectoryPicker, GlobalShortcutHost, GlobalShortcutRegistration,
    ImageClipboard, ImageSaveDialog, ImageSaveTarget, PinnedImageHost, PlatformCapabilities,
    ScrollCaptureSource, ScrollPreview, ScrollPreviewHost, Shortcut, SingleInstanceGuard,
    SingleInstanceHost, TextClipboard,
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

pub fn install_slint_platform() -> anyhow::Result<Box<dyn GlobalShortcutHost>> {
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
        }
    }
}
