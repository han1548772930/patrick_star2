//! Wayland/EGL hosting with Portal and PipeWire capture capabilities.

use crate::model::{DesktopFrame, RgbaFrame};
use crate::ocr::{OcrDocument, OcrLanguage, TextRecognizer};
use crate::platform::{
    ActiveScrollCapture, Availability, Capabilities, CaptureOverlay, CaptureOverlayResult,
    DesktopCapture, DirectoryPicker, ImageClipboard, ImageSaveDialog, ImageSaveTarget,
    PinnedImageHost, PlatformCapabilities, ScrollCaptureSource, ScrollPreview, ScrollPreviewHost,
    TextClipboard,
};

pub struct Backend;

impl DesktopCapture for Backend {
    fn capture_virtual_desktop(&self) -> anyhow::Result<DesktopFrame> {
        anyhow::bail!("Wayland Portal/PipeWire capture backend is not implemented yet")
    }
}

impl CaptureOverlay for Backend {
    fn run_capture_overlay(
        &self,
        _frame: DesktopFrame,
        _features: crate::model::OverlayFeatures,
    ) -> anyhow::Result<CaptureOverlayResult> {
        anyhow::bail!("Wayland EGL overlay backend is not implemented yet")
    }
}

impl ImageClipboard for Backend {
    fn write_image(&self, _image: &RgbaFrame) -> anyhow::Result<()> {
        anyhow::bail!("Wayland image clipboard backend is not implemented yet")
    }
}

impl TextClipboard for Backend {
    fn write_text(&self, _text: &str) -> anyhow::Result<()> {
        anyhow::bail!("Wayland text clipboard backend is not implemented yet")
    }
}

impl TextRecognizer for Backend {
    fn available_languages(&self) -> anyhow::Result<Vec<OcrLanguage>> {
        anyhow::bail!("Linux OCR backend is not implemented yet")
    }

    fn recognize_text(
        &self,
        _image: &RgbaFrame,
        _language_tag: Option<&str>,
    ) -> anyhow::Result<OcrDocument> {
        anyhow::bail!("Linux OCR backend is not implemented yet")
    }
}

impl ImageSaveDialog for Backend {
    fn choose_image_target(
        &self,
        _initial_directory: Option<&std::path::Path>,
    ) -> anyhow::Result<Option<ImageSaveTarget>> {
        anyhow::bail!("Wayland image save dialog is not implemented yet")
    }
}

impl DirectoryPicker for Backend {
    fn choose_directory(
        &self,
        _initial_directory: &std::path::Path,
    ) -> anyhow::Result<Option<std::path::PathBuf>> {
        anyhow::bail!("Wayland directory picker is not implemented yet")
    }
}

impl PinnedImageHost for Backend {
    fn show_pinned_image(&self, _image: RgbaFrame) -> anyhow::Result<()> {
        anyhow::bail!("Wayland pinned image host is not implemented yet")
    }
}

impl ScrollCaptureSource for Backend {
    fn start_scroll_capture(
        &self,
        _bounds: crate::model::RectI,
    ) -> anyhow::Result<Box<dyn ActiveScrollCapture>> {
        anyhow::bail!("Wayland scroll capture requires a Portal/PipeWire session")
    }

    fn cancel_scroll_capture(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

impl ScrollPreviewHost for Backend {
    fn open_scroll_preview(
        &self,
        _desktop: &DesktopFrame,
        _initial: &RgbaFrame,
    ) -> anyhow::Result<Box<dyn ScrollPreview>> {
        anyhow::bail!("Wayland scroll preview host is not implemented yet")
    }
}

impl PlatformCapabilities for Backend {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            desktop_capture: Availability::Unavailable,
            window_detection: Availability::Unavailable,
            image_clipboard: Availability::Unavailable,
            text_clipboard: Availability::Unavailable,
            image_save: Availability::Unavailable,
            pinned_image: Availability::Unavailable,
            text_recognition: Availability::Unavailable,
            scroll_capture_source: Availability::Unavailable,
            scroll_preview: Availability::Unavailable,
            global_shortcut: Availability::Unavailable,
            tray: Availability::Unavailable,
            capture_exclusion: Availability::Unavailable,
        }
    }
}
