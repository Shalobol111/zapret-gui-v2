//! Системный прокси Windows (Параметры → Сеть → Прокси): включить на sing-box и вернуть как было.
//! Настройки — в HKCU\…\Internet Settings, после записи программы оповещаются через WinINet.

use serde::{Deserialize, Serialize};
use windows::Win32::Networking::WinInet::{
    InternetSetOptionW, INTERNET_OPTION_REFRESH, INTERNET_OPTION_SETTINGS_CHANGED,
};
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};
use winreg::RegKey;

const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";
/// Локальные адреса — мимо прокси.
const BYPASS: &str =
    "localhost;127.*;10.*;172.16.*;172.17.*;172.18.*;172.19.*;172.2*;172.30.*;172.31.*;192.168.*;<local>";

/// Что было до включения — чтобы вернуть.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    pub enable: u32,
    pub server: Option<String>,
    pub bypass: Option<String>,
}

fn notify() {
    unsafe {
        let _ = InternetSetOptionW(None, INTERNET_OPTION_SETTINGS_CHANGED, None, 0);
        let _ = InternetSetOptionW(None, INTERNET_OPTION_REFRESH, None, 0);
    }
}

pub fn current() -> Saved {
    let Ok(k) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(KEY, KEY_READ) else {
        return Saved::default();
    };
    Saved {
        enable: k.get_value::<u32, _>("ProxyEnable").unwrap_or(0),
        server: k.get_value::<String, _>("ProxyServer").ok(),
        bypass: k.get_value::<String, _>("ProxyOverride").ok(),
    }
}

pub fn address(port: u16) -> String {
    format!("127.0.0.1:{port}")
}

/// Включён ли системный прокси именно на нас.
pub fn is_ours(port: u16) -> bool {
    let c = current();
    c.enable == 1 && c.server.as_deref() == Some(address(port).as_str())
}

/// Включить прокси на 127.0.0.1:port; возвращает прежние настройки.
pub fn enable(port: u16) -> Result<Saved, String> {
    let before = current();
    let k = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(KEY, KEY_SET_VALUE)
        .map_err(|e| format!("нет доступа к настройкам прокси: {e}"))?;
    k.set_value("ProxyServer", &address(port)).map_err(|e| e.to_string())?;
    k.set_value("ProxyOverride", &BYPASS).map_err(|e| e.to_string())?;
    k.set_value("ProxyEnable", &1u32).map_err(|e| e.to_string())?;
    notify();
    Ok(before)
}

/// Вернуть прежние настройки (если прокси всё ещё наш — иначе пользователь уже поменял его сам).
pub fn restore(saved: &Saved, port: u16) -> Result<(), String> {
    if !is_ours(port) {
        return Ok(());
    }
    let k = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(KEY, KEY_SET_VALUE)
        .map_err(|e| format!("нет доступа к настройкам прокси: {e}"))?;
    let ours = address(port);
    let enable = if saved.server.as_deref() == Some(ours.as_str()) { 0 } else { saved.enable };
    k.set_value("ProxyEnable", &enable).map_err(|e| e.to_string())?;
    match saved.server.as_deref().filter(|s| *s != ours) {
        Some(s) => k.set_value("ProxyServer", &s).map_err(|e| e.to_string())?,
        None => {
            let _ = k.delete_value("ProxyServer");
        }
    }
    match &saved.bypass {
        Some(b) => k.set_value("ProxyOverride", b).map_err(|e| e.to_string())?,
        None => {
            let _ = k.delete_value("ProxyOverride");
        }
    }
    notify();
    Ok(())
}
