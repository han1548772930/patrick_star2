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
    ActiveScrollCapture, Availability, Capabilities, CaptureOverlay, DesktopCapture,
    DirectoryPicker, GlobalShortcutHost, GlobalShortcutRegistration, ImageClipboard,
    ImageFileFormat, ImageSaveDialog, ImageSaveTarget, NativeCursorHost, PinnedImageHost,
    PlatformBackend, PlatformCapabilities, ScrollCaptureEvent, ScrollCaptureIntent,
    ScrollCaptureSource, ScrollPreview, ScrollPreviewHost, Shortcut, ShortcutKey,
    ShortcutModifiers, SingleInstanceGuard, SingleInstanceHost, TextClipboard, WindowLocator,
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
