use std::path::{Path, PathBuf};
use std::time::Instant;

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
    pub window_frame: Availability,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDirection {
    Up,
    Down,
    Unknown,
}

/// Platform-neutral frame and input metadata produced by a scrolling source.
#[derive(Clone)]
pub struct CapturedScrollFrame {
    pub frame: RgbaFrame,
    pub captured_at: Instant,
    pub direction: ScrollDirection,
    pub wheel_sequence: u64,
    pub native_scroll_position: Option<i64>,
    pub discontinuity: bool,
}

impl std::fmt::Debug for CapturedScrollFrame {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CapturedScrollFrame")
            .field("bounds", &self.frame.bounds())
            .field("captured_at", &self.captured_at)
            .field("direction", &self.direction)
            .field("wheel_sequence", &self.wheel_sequence)
            .field("native_scroll_position", &self.native_scroll_position)
            .field("discontinuity", &self.discontinuity)
            .finish()
    }
}

#[derive(Debug)]
pub enum ScrollCaptureEvent {
    Frame(CapturedScrollFrame),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptionButton {
    Minimize,
    Maximize,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowFrameAnchor {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowFrameClientArea {
    pub anchor: WindowFrameAnchor,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl WindowFrameClientArea {
    pub const fn left(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            anchor: WindowFrameAnchor::Left,
            x,
            y,
            width,
            height,
        }
    }

    pub const fn right(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            anchor: WindowFrameAnchor::Right,
            x,
            y,
            width,
            height,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindowFrameConfig {
    pub titlebar_height: f32,
    pub caption_button_width: f32,
    pub minimum_width: f32,
    pub minimum_height: f32,
    pub rounded_corners: bool,
    pub always_on_top: bool,
    pub client_areas: Vec<WindowFrameClientArea>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowFrameEvent {
    Installed,
    CaptionHoverChanged(Option<CaptionButton>),
    Detached,
    Failed(String),
}

/// Keeps a platform window frame attached for at least as long as the UI window.
pub trait WindowFrame {}

/// Installs native window-manager behavior around an existing Slint window.
pub trait WindowFrameHost {
    fn attach_window_frame(
        &self,
        window: &slint::Window,
        config: WindowFrameConfig,
        on_event: Box<dyn Fn(WindowFrameEvent) + 'static>,
    ) -> anyhow::Result<Box<dyn WindowFrame>>;
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
    + WindowFrameHost
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
        + WindowFrameHost
        + TextRecognizer
        + PlatformCapabilities
{
}
