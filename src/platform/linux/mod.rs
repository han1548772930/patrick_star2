//! Linux backend selection between X11 and Wayland.

mod single_instance;
mod wayland;
mod x11;

use crate::model::{DesktopFrame, RgbaFrame};
use crate::ocr::{OcrDocument, OcrLanguage, TextRecognizer};
use crate::platform::{
    ActiveScrollCapture, Capabilities, CaptureOverlay, CaptureOverlayResult, DesktopCapture,
    DirectoryPicker, ImageClipboard, ImageSaveDialog, ImageSaveTarget, PinnedImageHost,
    PlatformCapabilities, ScrollCaptureSource, ScrollPreview, ScrollPreviewHost,
    SingleInstanceGuard, SingleInstanceHost, TextClipboard, WindowFrame, WindowFrameConfig,
    WindowFrameEvent, WindowFrameHost,
};

pub struct Backend {
    session: Session,
}

enum Session {
    X11(x11::Backend),
    Wayland(wayland::Backend),
}

impl Backend {
    pub fn current() -> Self {
        let session = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            Session::Wayland(wayland::Backend)
        } else {
            Session::X11(x11::Backend)
        };
        Self { session }
    }
}

impl DesktopCapture for Backend {
    fn capture_virtual_desktop(&self) -> anyhow::Result<DesktopFrame> {
        match &self.session {
            Session::X11(backend) => backend.capture_virtual_desktop(),
            Session::Wayland(backend) => backend.capture_virtual_desktop(),
        }
    }
}

impl CaptureOverlay for Backend {
    fn run_capture_overlay(
        &self,
        frame: DesktopFrame,
        features: crate::model::OverlayFeatures,
    ) -> anyhow::Result<CaptureOverlayResult> {
        match &self.session {
            Session::X11(backend) => backend.run_capture_overlay(frame, features),
            Session::Wayland(backend) => backend.run_capture_overlay(frame, features),
        }
    }
}

impl SingleInstanceHost for Backend {
    fn acquire_single_instance(&self) -> anyhow::Result<Option<Box<dyn SingleInstanceGuard>>> {
        single_instance::acquire()
    }
}

impl ImageClipboard for Backend {
    fn write_image(&self, image: &RgbaFrame) -> anyhow::Result<()> {
        match &self.session {
            Session::X11(backend) => backend.write_image(image),
            Session::Wayland(backend) => backend.write_image(image),
        }
    }
}

impl TextClipboard for Backend {
    fn write_text(&self, text: &str) -> anyhow::Result<()> {
        match &self.session {
            Session::X11(backend) => backend.write_text(text),
            Session::Wayland(backend) => backend.write_text(text),
        }
    }
}

impl TextRecognizer for Backend {
    fn available_languages(&self) -> anyhow::Result<Vec<OcrLanguage>> {
        match &self.session {
            Session::X11(backend) => backend.available_languages(),
            Session::Wayland(backend) => backend.available_languages(),
        }
    }

    fn recognize_text(
        &self,
        image: &RgbaFrame,
        language_tag: Option<&str>,
    ) -> anyhow::Result<OcrDocument> {
        match &self.session {
            Session::X11(backend) => backend.recognize_text(image, language_tag),
            Session::Wayland(backend) => backend.recognize_text(image, language_tag),
        }
    }
}

impl ImageSaveDialog for Backend {
    fn choose_image_target(
        &self,
        initial_directory: Option<&std::path::Path>,
    ) -> anyhow::Result<Option<ImageSaveTarget>> {
        match &self.session {
            Session::X11(backend) => backend.choose_image_target(initial_directory),
            Session::Wayland(backend) => backend.choose_image_target(initial_directory),
        }
    }
}

impl DirectoryPicker for Backend {
    fn choose_directory(
        &self,
        initial_directory: &std::path::Path,
    ) -> anyhow::Result<Option<std::path::PathBuf>> {
        match &self.session {
            Session::X11(backend) => backend.choose_directory(initial_directory),
            Session::Wayland(backend) => backend.choose_directory(initial_directory),
        }
    }
}

impl PinnedImageHost for Backend {
    fn show_pinned_image(&self, image: RgbaFrame) -> anyhow::Result<()> {
        match &self.session {
            Session::X11(backend) => backend.show_pinned_image(image),
            Session::Wayland(backend) => backend.show_pinned_image(image),
        }
    }
}

impl ScrollCaptureSource for Backend {
    fn start_scroll_capture(
        &self,
        bounds: crate::model::RectI,
    ) -> anyhow::Result<Box<dyn ActiveScrollCapture>> {
        match &self.session {
            Session::X11(backend) => backend.start_scroll_capture(bounds),
            Session::Wayland(backend) => backend.start_scroll_capture(bounds),
        }
    }

    fn cancel_scroll_capture(&self) -> anyhow::Result<()> {
        match &self.session {
            Session::X11(backend) => backend.cancel_scroll_capture(),
            Session::Wayland(backend) => backend.cancel_scroll_capture(),
        }
    }
}

impl ScrollPreviewHost for Backend {
    fn open_scroll_preview(
        &self,
        desktop: &DesktopFrame,
        initial: &RgbaFrame,
    ) -> anyhow::Result<Box<dyn ScrollPreview>> {
        match &self.session {
            Session::X11(backend) => backend.open_scroll_preview(desktop, initial),
            Session::Wayland(backend) => backend.open_scroll_preview(desktop, initial),
        }
    }
}

impl WindowFrameHost for Backend {
    fn attach_window_frame(
        &self,
        _window: &slint::Window,
        _config: WindowFrameConfig,
        _on_event: Box<dyn Fn(WindowFrameEvent) + 'static>,
    ) -> anyhow::Result<Box<dyn WindowFrame>> {
        anyhow::bail!("Linux native Slint window frame is not implemented yet")
    }
}

impl PlatformCapabilities for Backend {
    fn capabilities(&self) -> Capabilities {
        match &self.session {
            Session::X11(backend) => backend.capabilities(),
            Session::Wayland(backend) => backend.capabilities(),
        }
    }
}
