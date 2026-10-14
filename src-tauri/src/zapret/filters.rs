//! Фильтры IPSet и Game — в тех же файлах и форматах, что у service.bat.
//!
//! IPSet (`lists\ipset-all.txt`): пустой файл → any; строка-заглушка
//! `203.0.113.113/32` → none (в интерфейсе «Выкл»); иначе → loaded. Загруженный
//! список при выключении переименовывается в `ipset-all.txt.backup`.
//!
//! Game (`utils\game_filter.enabled`): `mode=disabled|all|tcp|udp`, `tcp=…`, `udp=…`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const IPSET_PLACEHOLDER: &str = "203.0.113.113/32";
pub const DEFAULT_RANGE: &str = "1024-65535";
/// Порт-заглушка, которую service.bat подставляет при выключенном Game Filter.
pub const GAME_OFF_PORT: &str = "12";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IpsetMode {
    Off,
    Any,
    Loaded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameMode {
    #[serde(rename = "off")]
    Off,
    #[serde(rename = "tcp")]
    Tcp,
    #[serde(rename = "udp")]
    Udp,
    #[serde(rename = "tcp-udp")]
    TcpUdp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameFilter {
    pub mode: GameMode,
    pub tcp: String,
    pub udp: String,
}

impl GameFilter {
    /// Значения %GameFilterTCP% / %GameFilterUDP% для подстановки в стратегию.
    pub fn vars(&self) -> (String, String) {
        let tcp_on = matches!(self.mode, GameMode::Tcp | GameMode::TcpUdp);
        let udp_on = matches!(self.mode, GameMode::Udp | GameMode::TcpUdp);
        (
            if tcp_on { self.tcp.clone() } else { GAME_OFF_PORT.into() },
            if udp_on { self.udp.clone() } else { GAME_OFF_PORT.into() },
        )
    }
}

fn ipset_file(root: &Path) -> PathBuf {
    root.join("lists").join("ipset-all.txt")
}
fn ipset_backup(root: &Path) -> PathBuf {
    root.join("lists").join("ipset-all.txt.backup")
}
fn game_file(root: &Path) -> PathBuf {
    root.join("utils").join("game_filter.enabled")
}

pub fn ipset_mode(root: &Path) -> IpsetMode {
    match fs::read(ipset_file(root)) {
        Err(_) => IpsetMode::Off, // так считает test zapret.ps1: нет файла — нет фильтра
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            if text.trim().is_empty() {
                IpsetMode::Any
            } else if text.contains(IPSET_PLACEHOLDER) {
                IpsetMode::Off
            } else {
                IpsetMode::Loaded
            }
        }
    }
}

pub fn ipset_backup_exists(root: &Path) -> bool {
    fs::metadata(ipset_backup(root)).map(|m| m.len() > 0).unwrap_or(false)
}

pub fn set_ipset(root: &Path, mode: IpsetMode) -> Result<(), String> {
    let cur = ipset_mode(root);
    if cur == mode {
        return Ok(());
    }
    let list = ipset_file(root);
    let backup = ipset_backup(root);
    let io = |e: io::Error| format!("не удалось изменить IPSet: {e}");
    fs::create_dir_all(list.parent().unwrap()).map_err(io)?;

    if mode == IpsetMode::Loaded {
        if !ipset_backup_exists(root) {
            return Err("Нет сохранённого IP-списка. Сначала обновите IPSet-список".into());
        }
        let _ = fs::remove_file(&list);
        return fs::rename(&backup, &list).map_err(io);
    }
    // Уходим из loaded — сохраняем загруженный список, как service.bat.
    if cur == IpsetMode::Loaded && list.exists() {
        let _ = fs::remove_file(&backup);
        fs::rename(&list, &backup).map_err(io)?;
    }
    let content = if mode == IpsetMode::Off { format!("{IPSET_PLACEHOLDER}\r\n") } else { String::new() };
    fs::write(&list, content).map_err(io)
}

/// Проверка диапазонов портов по правилам service.bat: `1024-1934,1936-65535`.
pub fn validate_ranges(input: &str) -> Option<String> {
    let s: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    if s.is_empty() {
        return None;
    }
    for item in s.split(',') {
        let (a, b) = match item.split_once('-') {
            Some((a, b)) => (a, b),
            None => (item, item),
        };
        let parse = |p: &str| -> Option<u32> {
            if p.is_empty() || p.len() > 5 || p.starts_with('0') || !p.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            p.parse().ok().filter(|n| *n <= 65535)
        };
        let (a, b) = (parse(a)?, parse(b)?);
        if a > b {
            return None;
        }
    }
    Some(s)
}

pub fn game(root: &Path) -> GameFilter {
    let mut gf = GameFilter { mode: GameMode::Off, tcp: DEFAULT_RANGE.into(), udp: DEFAULT_RANGE.into() };
    let Ok(bytes) = fs::read(game_file(root)) else { return gf };
    let text = String::from_utf8_lossy(&bytes);
    let mut mode = "disabled".to_string();
    for line in text.lines() {
        let line = line.trim();
        let (key, value) = line.split_once('=').map(|(k, v)| (k.trim(), v.trim())).unwrap_or((line, ""));
        match key.to_ascii_lowercase().as_str() {
            "mode" => mode = value.to_ascii_lowercase(),
            "all" => mode = "all".into(),
            "tcp" if value.is_empty() => mode = "tcp".into(),
            "udp" if value.is_empty() => mode = "udp".into(),
            "tcp" => {
                if let Some(r) = validate_ranges(value) {
                    gf.tcp = r
                }
            }
            "udp" => {
                if let Some(r) = validate_ranges(value) {
                    gf.udp = r
                }
            }
            _ => {}
        }
    }
    gf.mode = match mode.as_str() {
        "all" => GameMode::TcpUdp,
        "tcp" => GameMode::Tcp,
        "udp" => GameMode::Udp,
        _ => GameMode::Off,
    };
    gf
}

pub fn set_game(root: &Path, mode: GameMode, tcp: Option<&str>, udp: Option<&str>) -> Result<(), String> {
    let cur = game(root);
    let tcp = match tcp {
        Some(t) => validate_ranges(t).ok_or("Неверные порты TCP")?,
        None => cur.tcp,
    };
    let udp = match udp {
        Some(u) => validate_ranges(u).ok_or("Неверные порты UDP")?,
        None => cur.udp,
    };
    let mode_str = match mode {
        GameMode::Off => "disabled",
        GameMode::Tcp => "tcp",
        GameMode::Udp => "udp",
        GameMode::TcpUdp => "all",
    };
    let file = game_file(root);
    fs::create_dir_all(file.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::write(&file, format!("mode={mode_str}\r\ntcp={tcp}\r\nudp={udp}\r\n"))
        .map_err(|e| format!("не удалось изменить Game Filter: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zgui-flt-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("lists")).unwrap();
        fs::create_dir_all(dir.join("utils")).unwrap();
        dir
    }

    #[test]
    fn ipset_round_trip_keeps_loaded_list() {
        let root = temp_root("ipset");
        fs::write(ipset_file(&root), "1.2.3.0/24\r\n5.6.7.8\r\n").unwrap();
        assert_eq!(ipset_mode(&root), IpsetMode::Loaded);

        set_ipset(&root, IpsetMode::Off).unwrap();
        assert_eq!(ipset_mode(&root), IpsetMode::Off);
        assert!(ipset_backup_exists(&root));

        set_ipset(&root, IpsetMode::Any).unwrap();
        assert_eq!(ipset_mode(&root), IpsetMode::Any);
        assert_eq!(fs::metadata(ipset_file(&root)).unwrap().len(), 0, "service.bat считает any только пустой файл");

        set_ipset(&root, IpsetMode::Loaded).unwrap();
        assert_eq!(fs::read_to_string(ipset_file(&root)).unwrap(), "1.2.3.0/24\r\n5.6.7.8\r\n");
        assert!(!ipset_backup(&root).exists());
    }

    #[test]
    fn ipset_loaded_without_backup_fails() {
        let root = temp_root("nobackup");
        fs::write(ipset_file(&root), format!("{IPSET_PLACEHOLDER}\r\n")).unwrap();
        assert!(set_ipset(&root, IpsetMode::Loaded).is_err());
        assert_eq!(ipset_mode(&root), IpsetMode::Off);
    }

    #[test]
    fn game_reads_new_and_legacy_formats() {
        let root = temp_root("game");
        assert_eq!(game(&root).mode, GameMode::Off);

        fs::write(game_file(&root), "mode=all\r\ntcp=1024-2000\r\nudp=bad\r\n").unwrap();
        let g = game(&root);
        assert_eq!(g.mode, GameMode::TcpUdp);
        assert_eq!(g.tcp, "1024-2000");
        assert_eq!(g.udp, DEFAULT_RANGE);

        fs::write(game_file(&root), "udp\r\n").unwrap();
        assert_eq!(game(&root).mode, GameMode::Udp);
        fs::write(game_file(&root), "all\n").unwrap();
        assert_eq!(game(&root).mode, GameMode::TcpUdp);
    }

    #[test]
    fn game_write_and_vars() {
        let root = temp_root("gamew");
        set_game(&root, GameMode::Tcp, Some("1024-1934, 1936-65535"), None).unwrap();
        assert_eq!(
            fs::read_to_string(game_file(&root)).unwrap(),
            "mode=tcp\r\ntcp=1024-1934,1936-65535\r\nudp=1024-65535\r\n"
        );
        let g = game(&root);
        assert_eq!(g.vars(), ("1024-1934,1936-65535".to_string(), "12".to_string()));
        assert!(set_game(&root, GameMode::Udp, None, Some("70000")).is_err());
    }

    #[test]
    fn range_validation() {
        assert_eq!(validate_ranges("1024-65535"), Some("1024-65535".into()));
        assert_eq!(validate_ranges("80,443"), Some("80,443".into()));
        assert_eq!(validate_ranges("0-10"), None);
        assert_eq!(validate_ranges("200-100"), None);
        assert_eq!(validate_ranges("65536"), None);
        assert_eq!(validate_ranges(""), None);
    }
}
