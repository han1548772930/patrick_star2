use std::path::{Path, PathBuf};

use crate::model::{
    CaptureOutcome, DesktopFrame, DetectedTarget, OverlayFeatures, PointI, PointerCursor, RgbaFrame,
};
use crate::ocr::TextRecognizer;
use crate::scroll::PreviewPatch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(target_os = "windows", allow(dead_code))]
pub enum Availability {
    Native,
    Portal,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub desktop_capture: Availability,
    pub window_detection: Availability,
    pub image_clipboard: Availability,
    pub text_clipboard: Availability,
    pub image_save: Availability,
    pub pinned_image: Availability,
    pub text_recognition: Availability,
    pub scroll_capture_source: Availability,
    pub scroll_preview: Availability,
    pub global_shortcut: Availability,
    pub tray: Availability,
    pub capture_exclusion: Availability,
}

pub trait DesktopCapture {
    fn capture_virtual_desktop(&self) -> anyhow::Result<DesktopFrame>;
}

/// Keeps a native capture overlay alive while a follow-up native surface is prepared.
pub trait CaptureOverlayHandoff {}

pub struct CaptureOverlayResult {
    pub outcome: CaptureOutcome,
    pub handoff: Option<Box<dyn CaptureOverlayHandoff>>,
}

impl CaptureOverlayResult {
    pub fn complete(outcome: CaptureOutcome) -> Self {
        Self {
            outcome,
            handoff: None,
        }
    }
}

pub trait CaptureOverlay {
    fn run_capture_overlay(
        &self,
        frame: DesktopFrame,
        features: OverlayFeatures,
    ) -> anyhow::Result<CaptureOverlayResult>;
}

/// Keeps this process registered as the sole application instance.
pub trait SingleInstanceGuard {}

pub trait SingleInstanceHost {
    /// Returns `None` when another instance already owns the application lock.
    fn acquire_single_instance(&self) -> anyhow::Result<Option<Box<dyn SingleInstanceGuard>>>;
}

pub trait ImageClipboard {
    fn write_image(&self, image: &RgbaFrame) -> anyhow::Result<()>;
}

pub trait TextClipboard {
    fn write_text(&self, text: &str) -> anyhow::Result<()>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFileFormat {
    Png,
    Jpeg,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageSaveTarget {
    pub path: PathBuf,
    pub format: ImageFileFormat,
}

pub trait ImageSaveDialog {
    fn choose_image_target(
        &self,
        initial_directory: Option<&Path>,
    ) -> anyhow::Result<Option<ImageSaveTarget>>;
}

pub trait DirectoryPicker {
    fn choose_directory(&self, initial_directory: &Path) -> anyhow::Result<Option<PathBuf>>;
}

pub trait PinnedImageHost {
    fn show_pinned_image(&self, image: RgbaFrame) -> anyhow::Result<()>;
}

#[derive(Debug)]
pub enum ScrollCaptureEvent {
    Frame(RgbaFrame),
    Finished(ScrollCaptureIntent),
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollCaptureIntent {
    Edit,
    Save,
    Clipboard,
}

pub trait ActiveScrollCapture {
    fn next_event(&mut self) -> anyhow::Result<ScrollCaptureEvent>;
}

pub trait ScrollCaptureSource {
    fn start_scroll_capture(
        &self,
        bounds: crate::model::RectI,
    ) -> anyhow::Result<Box<dyn ActiveScrollCapture>>;

    fn cancel_scroll_capture(&self) -> anyhow::Result<()>;
}

pub trait ScrollPreview {
    fn update(&mut self, patch: PreviewPatch<'_>) -> anyhow::Result<()>;
}

pub trait ScrollPreviewHost {
    fn open_scroll_preview(
        &self,
        desktop: &DesktopFrame,
        initial: &RgbaFrame,
    ) -> anyhow::Result<Box<dyn ScrollPreview>>;
}

pub trait PlatformCapabilities {
    fn capabilities(&self) -> Capabilities;
}

/// Cached native window/control discovery used by the capture overlay.
pub trait WindowLocator {
    fn target_at(&mut self, point: PointI) -> Option<DetectedTarget>;
}

pub trait NativeCursorHost {
    fn set_cursor(&mut self, cursor: PointerCursor);
}

#[allow(dead_code)]
pub trait PlatformWindowActions {
    fn start_drag(&self) -> anyhow::Result<()>;
    fn minimize(&self) -> anyhow::Result<()>;
    fn toggle_maximize(&self) -> anyhow::Result<()>;
    fn close(&self) -> anyhow::Result<()>;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ShortcutModifiers {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub logo: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShortcutKey {
    Character(char),
    #[allow(dead_code)]
    Function(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub modifiers: ShortcutModifiers,
    pub key: ShortcutKey,
}

/// Keeps a global shortcut active until the registration is dropped.
pub trait GlobalShortcutRegistration {}

pub trait GlobalShortcutHost {
    fn register_global_shortcut(
        &self,
        shortcut: Shortcut,
        callback: Box<dyn FnMut() + 'static>,
    ) -> anyhow::Result<Box<dyn GlobalShortcutRegistration>>;
}

/// The capabilities required by the capture application composition root.
/// Native backends implement the smaller traits directly; this trait only
/// gives the application one platform-neutral bound.
pub trait PlatformBackend:
    SingleInstanceHost
    + DesktopCapture
    + CaptureOverlay
    + ImageClipboard
    + TextClipboard
    + ImageSaveDialog
    + DirectoryPicker
    + PinnedImageHost
    + ScrollCaptureSource
    + ScrollPreviewHost
    + TextRecognizer
    + PlatformCapabilities
{
}

impl<T> PlatformBackend for T where
    T: SingleInstanceHost
        + DesktopCapture
        + CaptureOverlay
        + ImageClipboard
        + TextClipboard
        + ImageSaveDialog
        + DirectoryPicker
        + PinnedImageHost
        + ScrollCaptureSource
        + ScrollPreviewHost
        + TextRecognizer
        + PlatformCapabilities
{
}
