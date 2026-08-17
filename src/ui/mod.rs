//! Slint windows for editable preview, settings, and other normal UI.

mod settings;

slint::include_modules!();

pub(crate) use settings::{OcrLanguageChoice, SettingsDialog};

pub(crate) fn caption_button_value(button: Option<crate::platform::CaptionButton>) -> i32 {
    match button {
        None => 0,
        Some(crate::platform::CaptionButton::Minimize) => 1,
        Some(crate::platform::CaptionButton::Maximize) => 2,
        Some(crate::platform::CaptionButton::Close) => 3,
    }
}
