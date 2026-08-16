//! Slint settings window containing only the original system-settings tab.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use anyhow::Result;
use slint::{CloseRequestResponse, ComponentHandle, ModelRc, SharedString, VecModel};

use crate::settings::{Settings, describe_shortcut, parse_shortcut};

slint::slint! {
    import { Button, ComboBox, LineEdit } from "std-widgets.slint";

    export component SettingsWindow inherits Window {
        title: "Patrick Star 设置";
        preferred-width: 520px;
        preferred-height: 344px;
        min-width: 440px;
        min-height: 320px;
        no-frame: true;
        background: transparent;
        default-font-size: 14px;

        in-out property <string> capture-shortcut;
        in-out property <string> save-directory;
        in property <[string]> ocr-languages;
        in-out property <int> selected-ocr-index: 0;
        in-out property <string> error-message;

        callback browse-requested;
        callback accept-requested;
        callback cancel-requested;
        callback request-close;

        Rectangle {
            width: parent.width;
            height: parent.height;
            background: #f7f8fa;

            Rectangle {
                width: parent.width;
                height: 36px;
                background: #202328;

                Text {
                    x: 14px;
                    width: parent.width - 58px;
                    height: parent.height;
                    text: "Patrick Star 设置";
                    color: #f2f3f5;
                    font-size: 13px;
                    font-weight: 600;
                    vertical-alignment: center;
                }

                Rectangle {
                    x: parent.width - 44px;
                    width: 44px;
                    height: parent.height;
                    background: close-touch.has-hover ? #c42b1c : transparent;

                    Image {
                        x: 14px;
                        y: 10px;
                        width: 16px;
                        height: 16px;
                        source: @image-url("../../assets/icons/x.svg");
                        image-fit: contain;
                        colorize: #eef0f2;
                    }
                    close-touch := TouchArea {
                        mouse-cursor: pointer;
                        clicked => { root.request-close(); }
                    }
                }
            }

            VerticalLayout {
                y: 36px;
                width: parent.width;
                height: parent.height - 36px;
                padding: 24px;
                spacing: 8px;

                Text {
                    text: "系统设置";
                    color: #1d2024;
                    font-size: 20px;
                    font-weight: 600;
                }

                Rectangle { height: 4px; }

                Text { text: "截图热键"; color: #30343a; }
                LineEdit {
                    text <=> root.capture-shortcut;
                    placeholder-text: "Ctrl+Alt+S";
                }

                Text { text: "保存路径"; color: #30343a; }
                HorizontalLayout {
                    spacing: 8px;
                    LineEdit {
                        horizontal-stretch: 1;
                        text <=> root.save-directory;
                    }
                    Button {
                        text: "浏览...";
                        clicked => { root.browse-requested(); }
                    }
                }

                Text { text: "OCR 语言"; color: #30343a; }
                ComboBox {
                    model: root.ocr-languages;
                    current-index <=> root.selected-ocr-index;
                }

                if root.error-message != "" : Text {
                    text: root.error-message;
                    color: #b42318;
                    wrap: word-wrap;
                }

                Rectangle { vertical-stretch: 1; }

                HorizontalLayout {
                    alignment: end;
                    spacing: 8px;
                    Button {
                        text: "取消";
                        clicked => { root.cancel-requested(); }
                    }
                    Button {
                        text: "保存";
                        primary: true;
                        clicked => { root.accept-requested(); }
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OcrLanguageChoice {
    pub id: String,
    pub label: String,
}

impl OcrLanguageChoice {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }
}

pub struct SettingsDialog {
    window: SettingsWindow,
    language_ids: Rc<Vec<Option<String>>>,
}

impl SettingsDialog {
    pub fn new(initial: &Settings, languages: &[OcrLanguageChoice]) -> Result<Self> {
        let window = SettingsWindow::new()?;
        window.set_capture_shortcut(describe_shortcut(initial.capture_shortcut).into());
        window.set_save_directory(initial.save_directory.to_string_lossy().into_owned().into());

        let mut language_ids = vec![None];
        let mut labels = vec![SharedString::from("跟随系统语言")];
        for language in languages {
            let id = language.id.trim();
            if id.is_empty()
                || language_ids
                    .iter()
                    .any(|known| known.as_deref() == Some(id))
            {
                continue;
            }
            language_ids.push(Some(id.to_owned()));
            labels.push(SharedString::from(if language.label.trim().is_empty() {
                id
            } else {
                language.label.trim()
            }));
        }

        let selected_index = initial
            .ocr_language
            .as_deref()
            .and_then(|selected| {
                language_ids
                    .iter()
                    .position(|known| known.as_deref() == Some(selected))
            })
            .unwrap_or_else(|| {
                let Some(selected) = initial.ocr_language.as_deref() else {
                    return 0;
                };
                language_ids.push(Some(selected.to_owned()));
                labels.push(SharedString::from(format!("{selected}（当前不可用）")));
                language_ids.len() - 1
            });

        window.set_ocr_languages(ModelRc::new(VecModel::from(labels)));
        window.set_selected_ocr_index(selected_index as i32);

        let weak = window.as_weak();
        window.on_cancel_requested(move || {
            if let Some(window) = weak.upgrade() {
                let _ = window.hide();
            }
        });
        let weak = window.as_weak();
        window.on_request_close(move || {
            if let Some(window) = weak.upgrade() {
                let _ = window.hide();
            }
        });
        window
            .window()
            .on_close_requested(|| CloseRequestResponse::HideWindow);

        Ok(Self {
            window,
            language_ids: Rc::new(language_ids),
        })
    }

    pub fn show(&self, settings: &Settings) -> Result<()> {
        self.window
            .set_capture_shortcut(describe_shortcut(settings.capture_shortcut).into());
        self.window.set_save_directory(
            settings
                .save_directory
                .to_string_lossy()
                .into_owned()
                .into(),
        );
        let selected_index = settings
            .ocr_language
            .as_deref()
            .and_then(|selected| {
                self.language_ids
                    .iter()
                    .position(|known| known.as_deref() == Some(selected))
            })
            .unwrap_or(0);
        self.window.set_selected_ocr_index(selected_index as i32);
        self.window.set_error_message(SharedString::default());
        self.window.show()?;
        Ok(())
    }

    pub fn on_browse<F>(&self, mut browse: F)
    where
        F: FnMut(&Path) -> Result<Option<PathBuf>> + 'static,
    {
        let weak = self.window.as_weak();
        self.window.on_browse_requested(move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            let current = PathBuf::from(window.get_save_directory().as_str());
            match browse(&current) {
                Ok(Some(path)) => {
                    window.set_save_directory(path.to_string_lossy().into_owned().into());
                    window.set_error_message(SharedString::default());
                }
                Ok(None) => {}
                Err(error) => {
                    window.set_error_message(format!("无法选择保存路径：{error:#}").into());
                }
            }
        });
    }

    pub fn on_save<F>(&self, mut save: F)
    where
        F: FnMut(Settings) -> Result<()> + 'static,
    {
        let weak = self.window.as_weak();
        let language_ids = self.language_ids.clone();
        self.window.on_accept_requested(move || {
            let Some(window) = weak.upgrade() else {
                return;
            };
            match settings_from_window(&window, &language_ids).and_then(&mut save) {
                Ok(()) => {
                    window.set_error_message(SharedString::default());
                    let _ = window.hide();
                }
                Err(error) => {
                    window.set_error_message(format!("设置未保存：{error:#}").into());
                }
            }
        });
    }
}

fn settings_from_window(
    window: &SettingsWindow,
    language_ids: &[Option<String>],
) -> Result<Settings> {
    let capture_shortcut = parse_shortcut(window.get_capture_shortcut().as_str())?;
    let save_directory = window.get_save_directory().trim().to_owned();
    let selected_index = usize::try_from(window.get_selected_ocr_index())
        .map_err(|_| anyhow::anyhow!("OCR 语言选择无效"))?;
    let ocr_language = language_ids
        .get(selected_index)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("OCR 语言选择无效"))?;
    let settings = Settings {
        capture_shortcut,
        save_directory: PathBuf::from(save_directory),
        ocr_language,
    };
    settings.validate()?;
    Ok(settings)
}
