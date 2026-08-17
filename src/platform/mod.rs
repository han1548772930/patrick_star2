mod api;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "linux")]
mod linux;

#[allow(unused_imports)]
pub use api::PlatformWindowActions;
pub use api::{
    ActiveScrollCapture, Availability, Capabilities, CaptionButton, CaptureOverlay,
    CaptureOverlayHandoff, CaptureOverlayResult, CapturedScrollFrame, DesktopCapture,
    DirectoryPicker, GlobalShortcutHost, GlobalShortcutRegistration, ImageClipboard,
    ImageFileFormat, ImageSaveDialog, ImageSaveTarget, NativeCursorHost, PinnedImageHost,
    PlatformBackend, PlatformCapabilities, ScrollCaptureEvent, ScrollCaptureIntent,
    ScrollCaptureSource, ScrollDirection, ScrollPreview, ScrollPreviewHost, Shortcut, ShortcutKey,
    ShortcutModifiers, SingleInstanceGuard, SingleInstanceHost, TextClipboard, WindowFrame,
    WindowFrameAnchor, WindowFrameClientArea, WindowFrameConfig, WindowFrameEvent, WindowFrameHost,
    WindowLocator,
};

#[cfg(target_os = "windows")]
pub fn install_ui_platform() -> anyhow::Result<Box<dyn GlobalShortcutHost>> {
    windows::install_slint_platform()
}

#[cfg(not(target_os = "windows"))]
pub fn install_ui_platform() -> anyhow::Result<Box<dyn GlobalShortcutHost>> {
    anyhow::bail!("the native Slint UI runtime is not implemented on this platform yet")
}

#[cfg(target_os = "windows")]
pub fn current() -> impl PlatformBackend {
    windows::Backend
}

#[cfg(target_os = "windows")]
pub(crate) fn ui_font_paths() -> Vec<std::path::PathBuf> {
    windows::ui_font_paths()
}

#[cfg(target_os = "windows")]
pub(crate) fn set_preview_cursor(
    window: &slint::Window,
    cursor: crate::model::PointerCursor,
    popup: Option<crate::model::Rect>,
) {
    windows::set_preview_cursor(window, cursor, popup);
}

#[cfg(target_os = "macos")]
pub fn current() -> impl PlatformBackend {
    macos::Backend
}

#[cfg(target_os = "macos")]
pub(crate) fn ui_font_paths() -> Vec<std::path::PathBuf> {
    [
        "/System/Library/Fonts/SFNS.ttf",
        "/System/Library/Fonts/Apple Color Emoji.ttc",
    ]
    .into_iter()
    .map(std::path::PathBuf::from)
    .filter(|path| path.is_file())
    .collect()
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn set_preview_cursor(
    _window: &slint::Window,
    _cursor: crate::model::PointerCursor,
    _popup: Option<crate::model::Rect>,
) {
}

#[cfg(target_os = "linux")]
pub fn current() -> impl PlatformBackend {
    linux::Backend::current()
}

#[cfg(target_os = "linux")]
pub(crate) fn ui_font_paths() -> Vec<std::path::PathBuf> {
    [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf",
    ]
    .into_iter()
    .map(std::path::PathBuf::from)
    .filter(|path| path.is_file())
    .collect()
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
compile_error!("Patrick Star supports Windows, macOS, and Linux targets");
