//! Папка сборки zapret-discord-youtube: пути, проверка, версия, автопоиск.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::strategy::{self, Strategy};

#[derive(Debug, Clone)]
pub struct Layout {
    root: PathBuf,
}

impl Layout {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
    pub fn bin(&self) -> PathBuf {
        self.root.join("bin")
    }
    pub fn winws(&self) -> PathBuf {
        self.bin().join("winws.exe")
    }
    pub fn service_bat(&self) -> PathBuf {
        self.root.join("service.bat")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyInfo {
    pub name: String,
    /// Аргументы winws удалось извлечь — запуск без .bat и установка службы доступны.
    pub direct: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZapretInfo {
    pub path: Option<String>,
    pub valid: bool,
    pub error: Option<String>,
    /// Не критично для запуска, но почти всегда означает карантин антивируса.
    pub warning: Option<String>,
    pub version: Option<String>,
    pub strategies: Vec<StrategyInfo>,
    /// Папки нет или она пуста — туда можно скачать zapret.
    pub empty: bool,
}

impl ZapretInfo {
    pub fn empty() -> Self {
        Self { path: None, valid: false, error: None, warning: None, version: None, strategies: vec![], empty: false }
    }
}

/// Проверяет папку. `parse_ok` сообщает, удаётся ли разобрать стратегию.
pub fn inspect(path: &Path, parse_ok: impl Fn(&Strategy) -> bool) -> ZapretInfo {
    let mut info = ZapretInfo { path: Some(path.display().to_string()), ..ZapretInfo::empty() };
    if !path.is_dir() {
        info.error = Some("Папка не найдена".into());
        info.empty = true;
        return info;
    }
    let layout = Layout::new(path);
    if !layout.service_bat().is_file() {
        info.empty = fs::read_dir(path).map(|mut d| d.next().is_none()).unwrap_or(false);
        info.error = Some(if info.empty {
            "Папка пуста — в ней нет zapret".into()
        } else {
            "В папке нет service.bat — это не сборка zapret".into()
        });
        return info;
    }
    if !layout.winws().is_file() {
        info.error = Some("Нет bin\\winws.exe — проверьте карантин антивируса".into());
        return info;
    }
    let strategies = strategy::list(path);
    if strategies.is_empty() {
        info.error = Some("В папке нет стратегий general*.bat".into());
        return info;
    }
    if !layout.bin().join("WinDivert64.sys").is_file() || !layout.bin().join("WinDivert.dll").is_file() {
        info.warning = Some("Нет WinDivert64.sys или WinDivert.dll — проверьте карантин антивируса".into());
    }
    info.version = read_version(&layout.service_bat());
    info.strategies = strategies.iter().map(|s| StrategyInfo { name: s.name.clone(), direct: parse_ok(s) }).collect();
    info.valid = true;
    info
}

/// `set "LOCAL_VERSION=1.10.3"` из service.bat.
pub fn read_version(service_bat: &Path) -> Option<String> {
    let text = fs::read(service_bat).ok()?;
    let text = String::from_utf8_lossy(&text);
    text.lines().find_map(|line| {
        let idx = line.find("LOCAL_VERSION=")?;
        let value = line[idx + "LOCAL_VERSION=".len()..].trim().trim_end_matches('"').trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

fn looks_like_zapret(dir: &Path) -> bool {
    dir.join("service.bat").is_file() && dir.join("bin").join("winws.exe").is_file()
}

/// Ищет установленные сборки в типичных местах (глубина 2).
pub fn detect() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = vec!["C:\\".into(), "C:\\Tools".into(), "C:\\Program Files".into()];
    if let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
        for sub in ["Desktop", "Downloads", "Documents", "OneDrive\\Desktop"] {
            roots.push(home.join(sub));
        }
    }
    let mut found = Vec::new();
    let mut check = |dir: PathBuf| {
        if looks_like_zapret(&dir) && !found.contains(&dir) {
            found.push(dir);
        }
    };
    for root in roots {
        let Ok(entries) = fs::read_dir(&root) else { continue };
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let is_c_root = root.as_os_str().len() <= 3;
            let name = entry.file_name().to_string_lossy().to_lowercase();
            // В корне диска смотрим только папки с говорящими именами — не обходим весь C:\.
            if is_c_root && !(name.contains("zapret") || name.contains("discord") || name.contains("youtube")) {
                continue;
            }
            check(dir.clone());
            if let Ok(sub) = fs::read_dir(&dir) {
                for e in sub.flatten() {
                    if e.path().is_dir() {
                        check(e.path());
                    }
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_empty_and_foreign_folders() {
        let base = std::env::temp_dir().join(format!("zgui-layout-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let empty = base.join("empty");
        let other = base.join("other");
        fs::create_dir_all(&empty).unwrap();
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("readme.txt"), "x").unwrap();

        let i = inspect(&empty, |_| true);
        assert!(!i.valid && i.empty);
        let i = inspect(&other, |_| true);
        assert!(!i.valid && !i.empty);
        assert!(i.error.unwrap().contains("service.bat"));
        // Несуществующую папку установщик создаст сам.
        assert!(inspect(&base.join("missing"), |_| true).empty);
    }
}
