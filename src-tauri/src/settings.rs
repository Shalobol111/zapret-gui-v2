//! Настройки программы: `%APPDATA%\ZapretGUI\settings.json`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RestartPolicy {
    Ask,
    Always,
    Never,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub schema: u32,
    pub zapret_path: Option<String>,
    pub last_strategy: Option<String>,
    pub theme: ThemeChoice,
    pub autostart: bool,
    pub minimize_to_tray: bool,
    pub check_updates: bool,
    pub log_collapsed: bool,
    pub stop_bypass_on_exit: bool,
    pub restart_on_filter_change: RestartPolicy,
    /// Отмеченные в «Проверке» стратегии; `None` — все.
    pub test_selection: Option<Vec<String>>,
    /// Уведомления Windows о сбоях, смене стратегии и новых версиях.
    pub notifications: bool,
    /// Подсказка «работает в трее» уже показывалась.
    pub tray_hint_shown: bool,
    /// Автопилот: проверка Discord и YouTube раз в 15 минут и реакция на сбой.
    pub autopilot: crate::autopilot::Mode,
    /// Мастер первого запуска пройден или пропущен.
    pub onboarding_done: bool,
    /// Включать обход последней стратегией при запуске программы (вариант мастера «Запускать программу»).
    pub start_bypass_on_launch: bool,
    /// Запускать Telegram-прокси (TgWsProxy) вместе с ZapretGUI.
    pub tg_autostart: bool,
    /// Стратегия ручного обхода, которую включить после перезапуска при обновлении ZapretGUI.
    pub resume_bypass: Option<String>,
    /// Версия ZapretGUI, для которой окно «Доступна новая версия» закрыли кнопкой «Позже».
    pub app_update_dismissed: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            zapret_path: None,
            last_strategy: None,
            theme: ThemeChoice::System,
            autostart: false,
            minimize_to_tray: true,
            check_updates: true,
            log_collapsed: false,
            stop_bypass_on_exit: true,
            restart_on_filter_change: RestartPolicy::Ask,
            test_selection: None,
            notifications: true,
            tray_hint_shown: false,
            autopilot: crate::autopilot::Mode::Warn,
            onboarding_done: false,
            start_bypass_on_launch: false,
            tg_autostart: false,
            resume_bypass: None,
            app_update_dismissed: None,
        }
    }
}

/// Папка данных программы: настройки, журналы, результаты тестов.
/// `ZAPRETGUI_DATA_DIR` — отдельная папка для разработки, чтобы не трогать рабочие настройки.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("ZAPRETGUI_DATA_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("ZapretGUI")
}

pub struct SettingsStore {
    path: PathBuf,
    data: Settings,
}

impl SettingsStore {
    pub fn load_default() -> Self {
        Self::load(data_dir().join("settings.json"))
    }

    pub fn load(path: PathBuf) -> Self {
        let data = match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!("settings.json повреждён, используются значения по умолчанию: {e}");
                Settings::default()
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Settings::default(),
            Err(e) => {
                tracing::warn!("Не удалось прочитать settings.json: {e}");
                Settings::default()
            }
        };
        Self { path, data }
    }

    pub fn get(&self) -> &Settings {
        &self.data
    }

    /// Применяет частичное обновление (объект JSON с подмножеством ключей).
    /// Неизвестные ключи игнорируются, неверные значения — ошибка, файл не меняется.
    pub fn patch(&mut self, patch: &Value) -> Result<Settings, String> {
        let Value::Object(changes) = patch else {
            return Err("ожидается объект с настройками".into());
        };
        let mut merged = serde_json::to_value(&self.data).map_err(|e| e.to_string())?;
        let obj = merged.as_object_mut().expect("Settings сериализуется в объект");
        for (k, v) in changes {
            if obj.contains_key(k) {
                obj.insert(k.clone(), v.clone());
            }
        }
        let next: Settings = serde_json::from_value(merged).map_err(|e| format!("неверное значение: {e}"))?;
        write_atomic(&self.path, &next).map_err(|e| format!("не удалось сохранить: {e}"))?;
        self.data = next.clone();
        Ok(next)
    }
}

fn write_atomic(path: &Path, data: &Settings) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(data).map_err(io::Error::other)?;
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_store(name: &str) -> SettingsStore {
        let dir = std::env::temp_dir().join(format!("zgui-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        SettingsStore::load(dir.join("settings.json"))
    }

    #[test]
    fn missing_file_gives_defaults() {
        let s = temp_store("missing");
        assert_eq!(s.get().theme, ThemeChoice::System);
        assert!(s.get().minimize_to_tray);
    }

    #[test]
    fn patch_persists_and_ignores_unknown_keys() {
        let mut s = temp_store("patch");
        let next = s.patch(&json!({ "theme": "light", "bogus": 1 })).unwrap();
        assert_eq!(next.theme, ThemeChoice::Light);
        let reloaded = SettingsStore::load(s.path.clone());
        assert_eq!(reloaded.get().theme, ThemeChoice::Light);
    }

    #[test]
    fn patch_rejects_bad_value_without_writing() {
        let mut s = temp_store("bad");
        assert!(s.patch(&json!({ "theme": "purple" })).is_err());
        assert_eq!(s.get().theme, ThemeChoice::System);
        assert!(!s.path.exists());
    }
}
