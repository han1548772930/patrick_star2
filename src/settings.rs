//! Persisted settings shown by the system-settings window.

use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, ensure};
use serde_json::{Value, json};

use crate::platform::{Shortcut, ShortcutKey, ShortcutModifiers};

const SETTINGS_VERSION: u64 = 1;
const APP_DIRECTORY: &str = "patrick_star";
const SETTINGS_FILE: &str = "settings.json";

pub fn parse_shortcut(text: &str) -> Result<Shortcut> {
    let parts: Vec<_> = text.split('+').map(str::trim).collect();
    ensure!(
        parts.len() >= 2 && parts.iter().all(|part| !part.is_empty()),
        "快捷键必须包含修饰键和按键"
    );

    let mut modifiers = ShortcutModifiers::default();
    let mut key = None;
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => modifiers.control = true,
            "alt" => modifiers.alt = true,
            "shift" => modifiers.shift = true,
            "win" | "windows" | "logo" => modifiers.logo = true,
            _ if key.is_none() => key = Some(parse_shortcut_key(part)?),
            _ => return Err(anyhow!("快捷键只能包含一个普通按键")),
        }
    }

    ensure!(
        modifiers.control || modifiers.alt || modifiers.shift || modifiers.logo,
        "快捷键至少需要一个修饰键"
    );
    let key = key.ok_or_else(|| anyhow!("快捷键缺少普通按键"))?;
    Ok(Shortcut { modifiers, key })
}

pub fn describe_shortcut(shortcut: Shortcut) -> String {
    let mut parts = Vec::with_capacity(5);
    if shortcut.modifiers.control {
        parts.push("Ctrl".to_owned());
    }
    if shortcut.modifiers.alt {
        parts.push("Alt".to_owned());
    }
    if shortcut.modifiers.shift {
        parts.push("Shift".to_owned());
    }
    if shortcut.modifiers.logo {
        parts.push("Win".to_owned());
    }
    parts.push(match shortcut.key {
        ShortcutKey::Character(character) => character.to_string(),
        ShortcutKey::Function(number) => format!("F{number}"),
    });
    parts.join("+")
}

fn default_capture_shortcut() -> Shortcut {
    Shortcut {
        modifiers: ShortcutModifiers {
            control: true,
            alt: true,
            ..ShortcutModifiers::default()
        },
        key: ShortcutKey::Character('S'),
    }
}

fn parse_shortcut_key(text: &str) -> Result<ShortcutKey> {
    if let Some(number) = text
        .strip_prefix('F')
        .or_else(|| text.strip_prefix('f'))
        .and_then(|number| number.parse::<u8>().ok())
    {
        ensure!((1..=24).contains(&number), "功能键必须是 F1 到 F24");
        return Ok(ShortcutKey::Function(number));
    }

    let mut characters = text.chars();
    let character = characters
        .next()
        .filter(|_| characters.next().is_none())
        .filter(char::is_ascii_alphanumeric)
        .ok_or_else(|| anyhow!("普通按键必须是 A-Z、0-9 或 F1-F24"))?;
    Ok(ShortcutKey::Character(character.to_ascii_uppercase()))
}

/// The former second tab contained exactly these three settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub capture_shortcut: Shortcut,
    pub save_directory: PathBuf,
    /// `None` asks the platform OCR backend to follow the user's system languages.
    pub ocr_language: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            capture_shortcut: default_capture_shortcut(),
            save_directory: default_save_directory(),
            ocr_language: None,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.save_directory.as_os_str().is_empty(),
            "保存路径不能为空"
        );
        if let Some(language) = &self.ocr_language {
            ensure!(!language.trim().is_empty(), "OCR 语言标识不能为空");
        }
        Ok(())
    }

    pub fn from_json(source: &str) -> Result<Self> {
        let root: Value = serde_json::from_str(source).context("设置文件不是有效的 JSON")?;
        let object = root
            .as_object()
            .ok_or_else(|| anyhow!("设置文件根节点必须是对象"))?;
        let version = object
            .get("version")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("设置文件缺少版本号"))?;
        ensure!(version == SETTINGS_VERSION, "不支持设置文件版本 {version}");

        let shortcut = object
            .get("captureShortcut")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("设置文件缺少截图热键"))?;
        let save_directory = object
            .get("saveDirectory")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("设置文件缺少保存路径"))?;
        let ocr_language = match object.get("ocrLanguage") {
            Some(Value::Null) | None => None,
            Some(Value::String(language)) => Some(language.clone()),
            Some(_) => return Err(anyhow!("OCR 语言必须是字符串或 null")),
        };

        let settings = Self {
            capture_shortcut: parse_shortcut(shortcut)?,
            save_directory: PathBuf::from(save_directory),
            ocr_language,
        };
        settings.validate()?;
        Ok(settings)
    }

    pub fn to_json(&self) -> Result<String> {
        self.validate()?;
        let save_directory = self
            .save_directory
            .to_str()
            .ok_or_else(|| anyhow!("保存路径不是有效的 UTF-8"))?;
        let value = json!({
            "version": SETTINGS_VERSION,
            "captureShortcut": describe_shortcut(self.capture_shortcut),
            "saveDirectory": save_directory,
            "ocrLanguage": self.ocr_language,
        });
        let mut encoded = serde_json::to_string_pretty(&value).context("无法序列化设置")?;
        encoded.push('\n');
        Ok(encoded)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn for_current_user() -> Result<Self> {
        Ok(Self::at(default_settings_path()?))
    }

    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn load(&self) -> Result<Settings> {
        let source = match fs::read_to_string(&self.path) {
            Ok(source) => source,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Settings::default());
            }
            Err(error) => {
                return Err(error).with_context(|| format!("无法读取设置 {}", self.path.display()));
            }
        };
        Settings::from_json(&source)
            .with_context(|| format!("无法加载设置 {}", self.path.display()))
    }

    pub fn save(&self, settings: &Settings) -> Result<()> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| anyhow!("设置路径没有父目录"))?;
        fs::create_dir_all(parent)
            .with_context(|| format!("无法创建设置目录 {}", parent.display()))?;

        let encoded = settings.to_json()?;
        let mut file = File::create(&self.path)
            .with_context(|| format!("无法创建设置文件 {}", self.path.display()))?;
        file.write_all(encoded.as_bytes())
            .with_context(|| format!("无法写入设置文件 {}", self.path.display()))?;
        file.sync_all()
            .with_context(|| format!("无法提交设置文件 {}", self.path.display()))?;
        Ok(())
    }
}

fn default_save_directory() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn default_settings_path() -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("APPDATA 未设置"))?;

    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("HOME 未设置"))?
        .join("Library")
        .join("Application Support");

    #[cfg(all(unix, not(target_os = "macos")))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| anyhow!("XDG_CONFIG_HOME 和 HOME 均未设置"))?;

    Ok(base.join(APP_DIRECTORY).join(SETTINGS_FILE))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

    fn temporary_settings_path() -> PathBuf {
        let sequence = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "patrick-star2-settings-{}-{sequence}.json",
            std::process::id()
        ))
    }

    #[test]
    fn shortcut_parser_supports_every_platform_neutral_key_form() {
        let shortcut = parse_shortcut("control + shift + f12").unwrap();
        assert!(shortcut.modifiers.control);
        assert!(shortcut.modifiers.shift);
        assert_eq!(shortcut.key, ShortcutKey::Function(12));
        assert_eq!(describe_shortcut(shortcut), "Ctrl+Shift+F12");

        let digit = parse_shortcut("Win+1").unwrap();
        assert_eq!(digit.key, ShortcutKey::Character('1'));
        assert_eq!(describe_shortcut(digit), "Win+1");
    }

    #[test]
    fn shortcut_parser_rejects_incomplete_or_ambiguous_bindings() {
        for invalid in ["S", "Ctrl", "Ctrl+S+T", "Ctrl+F25", "Ctrl++S"] {
            assert!(parse_shortcut(invalid).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn json_round_trip_contains_only_the_former_system_tab_fields() {
        let settings = Settings {
            save_directory: PathBuf::from(r"C:\captures"),
            ocr_language: Some("zh-Hans-CN".to_owned()),
            ..Settings::default()
        };
        let encoded = settings.to_json().unwrap();
        let decoded = Settings::from_json(&encoded).unwrap();

        assert_eq!(decoded, settings);
        assert!(encoded.contains("captureShortcut"));
        assert!(encoded.contains("saveDirectory"));
        assert!(encoded.contains("ocrLanguage"));
        for removed in [
            "font",
            "weight",
            "italic",
            "underline",
            "strikeout",
            "stroke",
            "color",
            "autoCopy",
            "magnifier",
        ] {
            assert!(
                !encoded.contains(removed),
                "persisted removed field {removed}"
            );
        }
    }

    #[test]
    fn store_defaults_when_missing_and_round_trips_after_save() {
        let path = temporary_settings_path();
        let _ = fs::remove_file(&path);
        let store = SettingsStore::at(&path);
        assert_eq!(store.load().unwrap(), Settings::default());

        let expected = Settings {
            capture_shortcut: parse_shortcut("Ctrl+Alt+F2").unwrap(),
            save_directory: PathBuf::from(r"C:\screenshots"),
            ocr_language: Some("en-US".to_owned()),
        };
        store.save(&expected).unwrap();
        assert_eq!(store.load().unwrap(), expected);

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn malformed_settings_are_reported_instead_of_silently_overwriting_them() {
        let path = temporary_settings_path();
        fs::write(&path, r#"{"version":1,"captureShortcut":"S"}"#).unwrap();
        let error = SettingsStore::at(&path).load().unwrap_err().to_string();
        assert!(error.contains("无法加载设置"));
        fs::remove_file(path).unwrap();
    }
}
