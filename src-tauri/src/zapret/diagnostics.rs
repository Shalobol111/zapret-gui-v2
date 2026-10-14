//! Диагностика — проверки из service.bat (Run Diagnostics), сгруппированные
//! в строки карточки по макету, и автоисправления (только по выбору пользователя).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
use winreg::RegKey;

use super::{discord, hosts, service};
use crate::journal;
use crate::sys::{netsh, process, scm, services};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Ok,
    Info,
    Warn,
    Err,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fix {
    StartBfe,
    EnableTimestamps,
    RemoveConflicts,
    CommentYoutubeHosts,
    ClearDiscordCache,
}

impl Fix {
    pub fn label(self) -> &'static str {
        match self {
            Fix::StartBfe => "Запустить службу Base Filtering Engine",
            Fix::EnableTimestamps => "Включить TCP timestamps",
            Fix::RemoveConflicts => "Остановить и удалить конфликтующие службы и «висящий» WinDivert",
            Fix::CommentYoutubeHosts => "Закомментировать записи YouTube в hosts (с резервной копией)",
            Fix::ClearDiscordCache => "Закрыть Discord и очистить его кэш",
        }
    }
    /// Безопасные исправления отмечены в диалоге заранее.
    pub fn safe(self) -> bool {
        matches!(self, Fix::StartBfe | Fix::EnableTimestamps)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: &'static str,
    pub title: &'static str,
    pub level: Level,
    pub value: String,
    pub detail: Option<String>,
    pub fix: Option<Fix>,
    pub fix_label: Option<&'static str>,
    pub fix_safe: bool,
}

fn item(id: &'static str, title: &'static str, level: Level, value: impl Into<String>) -> Item {
    Item { id, title, level, value: value.into(), detail: None, fix: None, fix_label: None, fix_safe: false }
}

impl Item {
    fn detail(mut self, d: impl Into<String>) -> Self {
        self.detail = Some(d.into());
        self
    }
    fn fix(mut self, f: Fix) -> Self {
        self.fix = Some(f);
        self.fix_label = Some(f.label());
        self.fix_safe = f.safe();
        self
    }
}

fn check_bfe() -> Item {
    if scm::state("BFE") == scm::State::Running {
        item("bfe", "Base Filtering Engine", Level::Ok, "ок")
    } else {
        item("bfe", "Base Filtering Engine", Level::Err, "не запущена")
            .detail("Служба BFE нужна для работы WinDivert")
            .fix(Fix::StartBfe)
    }
}

fn check_proxy() -> Item {
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
        .ok();
    let enabled = key.as_ref().and_then(|k| k.get_value::<u32, _>("ProxyEnable").ok()).unwrap_or(0) == 1;
    if !enabled {
        return item("proxy", "Системный прокси", Level::Ok, "нет");
    }
    let server = key.and_then(|k| k.get_value::<String, _>("ProxyServer").ok()).unwrap_or_default();
    item("proxy", "Системный прокси", Level::Warn, "включён")
        .detail(format!("Прокси: {server}. Убедитесь, что он нужен, иначе отключите его в параметрах Windows"))
}

fn check_timestamps() -> Item {
    match netsh::tcp_timestamps_enabled() {
        Some(true) => item("tcp_ts", "TCP timestamps", Level::Ok, "вкл"),
        Some(false) => item("tcp_ts", "TCP timestamps", Level::Warn, "выкл")
            .detail("Без TCP timestamps часть стратегий не работает")
            .fix(Fix::EnableTimestamps),
        None => item("tcp_ts", "TCP timestamps", Level::Info, "неизвестно"),
    }
}

/// Adguard, Killer, Intel Connectivity, Check Point, SmartByte, VPN — как в service.bat.
fn check_conflicts() -> Item {
    let active = services::active();
    let has = |pred: &dyn Fn(&str) -> bool| {
        active.iter().any(|s| pred(&s.name.to_lowercase()) || pred(&s.display.to_lowercase()))
    };
    let mut errs: Vec<String> = Vec::new();
    if process::exists("AdguardSvc.exe") {
        errs.push("Adguard (issues/417)".into());
    }
    if has(&|s| s.contains("killer")) {
        errs.push("Killer".into());
    }
    if has(&|s| s.contains("intel") && s.contains("connectivity") && s.contains("network")) {
        errs.push("Intel Connectivity Network Service".into());
    }
    if has(&|s| s.contains("tracsrvwrapper") || s.contains("epwd")) {
        errs.push("Check Point".into());
    }
    if has(&|s| s.contains("smartbyte")) {
        errs.push("SmartByte".into());
    }
    let vpn: Vec<String> = active
        .iter()
        .filter(|s| s.name.to_lowercase().contains("vpn") || s.display.to_lowercase().contains("vpn"))
        .map(|s| s.name.clone())
        .collect();
    let title = "Adguard, Killer, VPN";
    if errs.is_empty() && vpn.is_empty() {
        return item("conflicts", title, Level::Ok, "не найдены");
    }
    let mut names = errs.clone();
    if !vpn.is_empty() {
        names.push(format!("VPN ({})", vpn.join(", ")));
    }
    let level = if errs.is_empty() { Level::Warn } else { Level::Err };
    item("conflicts", title, level, format!("найдено: {}", names.len()))
        .detail(format!("{} — эти программы конфликтуют с zapret, отключите или удалите их", names.join(", ")))
}

const CONFLICTING: [&str; 4] = ["GoodbyeDPI", "discordfix_zapret", "winws1", "winws2"];

fn check_windivert() -> Item {
    let found: Vec<&str> = CONFLICTING.iter().copied().filter(|n| scm::state(n) != scm::State::NotInstalled).collect();
    let windivert = scm::state("WinDivert");
    let orphan = !process::exists("winws.exe") && matches!(windivert, scm::State::Running | scm::State::StopPending);
    let title = "Конфликт WinDivert";
    if !found.is_empty() {
        return item("windivert", title, Level::Err, format!("найдено: {}", found.join(", ")))
            .detail("Другие обходы держат драйвер WinDivert")
            .fix(Fix::RemoveConflicts);
    }
    if orphan {
        return item("windivert", title, Level::Warn, "висит без winws")
            .detail("Служба WinDivert работает, а winws.exe — нет. Обычно её оставил другой обход")
            .fix(Fix::RemoveConflicts);
    }
    if service::status(None).stop_pending {
        return item("windivert", title, Level::Warn, "служба зависла")
            .detail("Служба zapret в состоянии STOP_PENDING — признак конфликта с другим обходом")
            .fix(Fix::RemoveConflicts);
    }
    item("windivert", title, Level::Ok, "нет")
}

/// DohFlags > 0 где-либо под InterfaceSpecificParameters (Get-ChildItem -Recurse в service.bat).
fn doh_enabled() -> bool {
    fn walk(key: &RegKey, depth: u32) -> bool {
        if key.get_value::<u32, _>("DohFlags").map(|v| v > 0).unwrap_or(false)
            || key.get_value::<u64, _>("DohFlags").map(|v| v > 0).unwrap_or(false)
        {
            return true;
        }
        depth < 6
            && key.enum_keys().flatten().filter_map(|name| key.open_subkey(name).ok()).any(|sub| walk(&sub, depth + 1))
    }
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"System\CurrentControlSet\Services\Dnscache\InterfaceSpecificParameters")
        .map(|k| walk(&k, 0))
        .unwrap_or(false)
}

fn check_dns() -> Item {
    if doh_enabled() {
        item("dns", "Secure DNS", Level::Ok, "настроен")
    } else {
        item("dns", "Secure DNS", Level::Err, "не настроен").detail(
            "Windows не использует зашифрованный DNS. Включите его в параметрах Windows 11 или в браузере \
             (не «поставщик по умолчанию»). Если он уже включён в браузере — предупреждение можно не учитывать",
        )
    }
}

fn check_path(root: Option<&Path>) -> Item {
    let title = "Папка zapret";
    let Some(root) = root else { return item("path", title, Level::Info, "не выбрана") };
    let s = root.display().to_string();
    if let Some(od) = std::env::var_os("OneDrive").map(PathBuf::from) {
        if !od.as_os_str().is_empty() && root.starts_with(&od) {
            return item("path", title, Level::Err, "в OneDrive")
                .detail("Перенесите zapret из OneDrive, например в C:\\zapret");
        }
    }
    if !root.join("bin").join("WinDivert64.sys").is_file() {
        return item("path", title, Level::Err, "нет WinDivert64.sys").detail(
            "Файл драйвера удалён — скорее всего, антивирусом. Добавьте папку в исключения и переустановите zapret",
        );
    }
    if s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё')) {
        return item("path", title, Level::Warn, "кириллица в пути")
            .detail("Если обход не работает, перенесите zapret, например в C:\\zapret");
    }
    item("path", title, Level::Ok, "ок")
}

fn youtube_host_lines(text: &str) -> usize {
    text.lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with('#') && (t.contains("youtube.com") || t.contains("youtu.be"))
        })
        .count()
}

fn check_hosts() -> Item {
    let n = hosts::read_current().map(|t| youtube_host_lines(&t)).unwrap_or(0);
    if n == 0 {
        item("hosts", "Файл hosts", Level::Ok, "ок")
    } else {
        item("hosts", "Файл hosts", Level::Warn, "есть записи YouTube")
            .detail(format!("Строк с youtube.com / youtu.be: {n}. Они могут мешать работе YouTube"))
            .fix(Fix::CommentYoutubeHosts)
    }
}

fn check_discord() -> Item {
    if discord::installed().is_empty() {
        return item("discord", "Кэш Discord", Level::Info, "Discord не найден");
    }
    item("discord", "Кэш Discord", Level::Info, "можно очистить")
        .detail("Помогает, если Discord неожиданно перестал работать")
        .fix(Fix::ClearDiscordCache)
}

pub fn run(root: Option<&Path>) -> Vec<Item> {
    let root = root.map(Path::to_path_buf);
    std::thread::scope(|s| {
        let jobs: Vec<Box<dyn FnOnce() -> Item + Send>> = vec![
            Box::new(check_bfe),
            Box::new(check_proxy),
            Box::new(check_timestamps),
            Box::new(check_conflicts),
            Box::new(check_windivert),
            Box::new(check_dns),
            Box::new(move || check_path(root.as_deref())),
            Box::new(check_hosts),
            Box::new(check_discord),
        ];
        let handles: Vec<_> = jobs.into_iter().map(|j| s.spawn(j)).collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    })
}

fn apply(fix: Fix) -> Result<String, String> {
    match fix {
        Fix::StartBfe => scm::start("BFE").map(|_| "Служба Base Filtering Engine запущена".into()),
        Fix::EnableTimestamps => netsh::enable_tcp_timestamps().map(|_| "TCP timestamps включены".into()),
        Fix::RemoveConflicts => {
            let mut removed = Vec::new();
            for name in CONFLICTING {
                if scm::state(name) != scm::State::NotInstalled {
                    scm::remove(name).map_err(|e| format!("{name}: {e}"))?;
                    removed.push(name);
                }
            }
            if !process::exists("winws.exe") || !removed.is_empty() {
                for drv in ["WinDivert", "WinDivert14"] {
                    let _ = scm::stop(drv, Duration::from_secs(5));
                    let _ = scm::remove(drv);
                }
            }
            Ok(if removed.is_empty() {
                "Служба WinDivert удалена".into()
            } else {
                format!("Удалены службы: {}, WinDivert", removed.join(", "))
            })
        }
        Fix::CommentYoutubeHosts => {
            let path = hosts::hosts_path();
            let text = hosts::read_current()?;
            fs::write(path.with_file_name("hosts.zapretgui.bak"), &text).map_err(|e| e.to_string())?;
            let fixed: Vec<String> = text
                .lines()
                .map(|l| {
                    let t = l.trim_start();
                    if !t.starts_with('#') && (t.contains("youtube.com") || t.contains("youtu.be")) {
                        format!("# {l}")
                    } else {
                        l.to_string()
                    }
                })
                .collect();
            fs::write(&path, fixed.join("\r\n") + "\r\n").map_err(|e| format!("не удалось записать hosts: {e}"))?;
            netsh::flush_dns();
            Ok("Записи YouTube в hosts закомментированы (копия: hosts.zapretgui.bak)".into())
        }
        Fix::ClearDiscordCache => {
            discord::close();
            Ok(format!("Кэш Discord очищен (папок: {})", discord::clear_cache()))
        }
    }
}

/// Применяет выбранные исправления по очереди; ошибки не прерывают остальные.
pub fn apply_all(fixes: &[Fix]) -> Vec<(Fix, Result<String, String>)> {
    fixes
        .iter()
        .map(|f| {
            let r = apply(*f);
            match &r {
                Ok(msg) => journal::ok(msg.clone()),
                Err(e) => journal::err(format!("{}: {e}", f.label())),
            }
            (*f, r)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_lines_ignore_comments() {
        let t = "# 1.2.3.4 youtube.com\r\n1.2.3.4 www.youtube.com\r\n5.6.7.8 youtu.be\r\n127.0.0.1 localhost";
        assert_eq!(youtube_host_lines(t), 2);
    }

    #[test]
    fn diagnostics_run_without_admin() {
        let items = run(None);
        for i in &items {
            println!("{:<22} {:?} {} {:?}", i.title, i.level, i.value, i.fix);
        }
        assert_eq!(items.len(), 9);
        assert!(items.iter().any(|i| i.id == "dns"));
    }
}
