//! Rust-side state and callbacks for the Slint settings window.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use anyhow::Result;
use slint::{CloseRequestResponse, ComponentHandle, ModelRc, SharedString, VecModel};

use super::{SettingsWindow, caption_button_value};
use crate::platform::{WindowFrame, WindowFrameConfig, WindowFrameEvent, WindowFrameHost};
use crate::settings::{Settings, describe_shortcut, parse_shortcut};

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
    _frame: Box<dyn WindowFrame>,
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
        window
            .window()
            .on_close_requested(|| CloseRequestResponse::HideWindow);

        let weak = window.as_weak();
        let frame = crate::platform::current().attach_window_frame(
            window.window(),
            WindowFrameConfig {
                titlebar_height: 36.0,
                caption_button_width: 44.0,
                minimum_width: 440.0,
                minimum_height: 320.0,
                rounded_corners: true,
                always_on_top: false,
                client_areas: Vec::new(),
            },
            Box::new(move |event| match event {
                WindowFrameEvent::CaptionHoverChanged(button) => {
                    if let Some(window) = weak.upgrade() {
                        window.set_caption_hover(caption_button_value(button));
                    }
                }
                WindowFrameEvent::Failed(error) => {
                    eprintln!("settings window frame failed: {error}");
                }
                WindowFrameEvent::Installed | WindowFrameEvent::Detached => {}
            }),
        )?;

        Ok(Self {
            window,
            language_ids: Rc::new(language_ids),
            _frame: frame,
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
