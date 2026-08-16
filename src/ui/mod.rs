//! Slint windows for editable preview, settings, and other normal UI.

mod preview;
mod settings;
mod tray;

#[allow(unused_imports)]
pub(crate) use preview::PreviewWindow;
pub(crate) use settings::{OcrLanguageChoice, SettingsDialog};
pub(crate) use tray::AppTray;
