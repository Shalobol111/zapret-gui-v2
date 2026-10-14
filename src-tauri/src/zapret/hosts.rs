//! «Обновить hosts»: записи Flowseal для веб-Telegram и голосовых каналов Discord.
//!
//! service.bat только открывает Блокнот для ручного копирования. Мы работаем от
//! администратора и пишем файл сами — после подтверждения, с резервной копией,
//! новые записи кладём в блок между маркерами, чтобы следующее обновление заменило его целиком.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;

pub const URL: &str =
    "https://raw.githubusercontent.com/Flowseal/zapret-discord-youtube/refs/heads/main/.service/hosts";
const BEGIN: &str = "# zapret-gui begin — Flowseal/zapret-discord-youtube .service/hosts";
const END: &str = "# zapret-gui end";

pub fn hosts_path() -> PathBuf {
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    root.join(r"System32\drivers\etc\hosts")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub up_to_date: bool,
    pub add: usize,
    pub replace: usize,
    pub total: usize,
}

/// (ip, host) из строк вида `1.2.3.4 host1 host2 # комментарий`.
fn entries(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let mut parts = line.split_whitespace();
        let Some(ip) = parts.next() else { continue };
        for host in parts {
            out.push((ip.to_string(), host.to_ascii_lowercase()));
        }
    }
    out
}

pub fn plan(current: &str, remote: &str) -> Plan {
    let have: HashMap<String, String> = entries(current).into_iter().map(|(ip, h)| (h, ip)).collect();
    let want = entries(remote);
    let (mut add, mut replace) = (0, 0);
    for (ip, host) in &want {
        match have.get(host) {
            Some(cur) if cur == ip => {}
            Some(_) => replace += 1,
            None => add += 1,
        }
    }
    Plan { up_to_date: add == 0 && replace == 0, add, replace, total: want.len() }
}

/// Новый текст hosts: старый блок и строки с теми же доменами убираются, блок дописывается в конец.
pub fn merge(current: &str, remote: &str) -> String {
    let remote_hosts: HashSet<String> = entries(remote).into_iter().map(|(_, h)| h).collect();
    let mut out: Vec<&str> = Vec::new();
    let mut in_block = false;
    for line in current.lines() {
        let t = line.trim();
        if t == BEGIN || t.starts_with("# zapret-gui begin") {
            in_block = true;
            continue;
        }
        if in_block {
            if t == END {
                in_block = false;
            }
            continue;
        }
        let ours = entries(line).iter().any(|(_, h)| remote_hosts.contains(h));
        if !ours {
            out.push(line);
        }
    }
    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    let mut text = out.join("\r\n");
    if !text.is_empty() {
        text.push_str("\r\n\r\n");
    }
    text.push_str(BEGIN);
    text.push_str("\r\n");
    for line in remote.lines().map(str::trim).filter(|l| !l.is_empty()) {
        text.push_str(line);
        text.push_str("\r\n");
    }
    text.push_str(END);
    text.push_str("\r\n");
    text
}

pub async fn fetch() -> Result<String, String> {
    let url = format!("{URL}?t={}", super::runner::now_ms());
    let text = crate::net::get_text(&url, Duration::from_secs(15)).await?;
    if entries(&text).is_empty() {
        return Err("скачанный файл пуст или повреждён".into());
    }
    Ok(text)
}

pub fn read_current() -> Result<String, String> {
    fs::read(hosts_path())
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .map_err(|e| format!("не удалось прочитать hosts: {e}"))
}

pub fn apply(remote: &str) -> Result<(), String> {
    let path = hosts_path();
    let current = read_current()?;
    fs::write(path.with_file_name("hosts.zapretgui.bak"), &current)
        .map_err(|e| format!("не удалось сохранить резервную копию: {e}"))?;
    fs::write(&path, merge(&current, remote))
        .map_err(|e| format!("не удалось записать hosts ({e}). Возможно, файл защищает антивирус"))?;
    crate::sys::netsh::flush_dns();
    Ok(())
}

/// Запасной путь, как в service.bat: открыть скачанный файл и папку с hosts.
pub fn open_manual(remote: &str) -> Result<(), String> {
    let tmp = std::env::temp_dir().join("zapret_hosts.txt");
    fs::write(&tmp, remote).map_err(|e| e.to_string())?;
    let _ = crate::sys::cmd::hidden("notepad.exe").arg(&tmp).spawn();
    let _ = crate::sys::cmd::hidden("explorer.exe").arg(format!("/select,{}", hosts_path().display())).spawn();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const REMOTE: &str = "1.1.1.1 web.telegram.org\r\n2.2.2.2 discord.media a.discord.media\r\n";

    #[test]
    fn plan_counts_additions_and_replacements() {
        let current = "127.0.0.1 localhost\r\n9.9.9.9 web.telegram.org\r\n2.2.2.2 discord.media\r\n";
        let p = plan(current, REMOTE);
        assert_eq!(p, Plan { up_to_date: false, add: 1, replace: 1, total: 3 });
        assert!(plan(&merge(current, REMOTE), REMOTE).up_to_date);
    }

    /// Сетевой: `cargo test -- --ignored fetch_real_hosts`
    #[test]
    #[ignore]
    fn fetch_real_hosts() {
        let rt = tauri::async_runtime::block_on(fetch());
        let text = rt.expect("скачать hosts Flowseal");
        assert!(entries(&text).len() > 5, "{text}");
        let p = plan(&read_current().unwrap(), &text);
        println!("{p:?}");
    }

    #[test]
    fn merge_replaces_old_block_and_keeps_user_lines() {
        let current = "# comment\r\n127.0.0.1 localhost\r\n9.9.9.9 web.telegram.org\r\n";
        let once = merge(current, REMOTE);
        assert!(once.contains("127.0.0.1 localhost"));
        assert!(!once.contains("9.9.9.9"));
        let twice = merge(&once, "3.3.3.3 web.telegram.org\r\n");
        assert_eq!(twice.matches(BEGIN).count(), 1);
        assert!(twice.contains("3.3.3.3 web.telegram.org"));
        assert!(!twice.contains("discord.media"));
        assert!(twice.contains("# comment"));
    }
}
