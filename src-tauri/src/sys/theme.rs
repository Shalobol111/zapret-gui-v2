//! Тема Windows (светлая/тёмная для приложений) и наблюдение за её сменой.
//!
//! Вместо опроса раз в несколько секунд (DESIGN.md §6.5) поток ждёт
//! `RegNotifyChangeKeyValue` — реакция мгновенная, нагрузки нет.

use serde::Serialize;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_NOTIFY,
    KEY_QUERY_VALUE, REG_NOTIFY_CHANGE_LAST_SET, RRF_RT_REG_DWORD,
};

const PERSONALIZE: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Dark,
    Light,
}

pub fn system_mode() -> Mode {
    let mut value: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PERSONALIZE,
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    // Ключа нет (старые сборки, LTSC) — Windows в этом случае светлая.
    if status != ERROR_SUCCESS || value != 0 {
        Mode::Light
    } else {
        Mode::Dark
    }
}

/// Запускает фоновый поток; `on_change` вызывается только при реальной смене режима.
pub fn watch(on_change: impl Fn(Mode) + Send + 'static) {
    std::thread::Builder::new()
        .name("theme-watch".into())
        .spawn(move || {
            let mut key = HKEY::default();
            let opened =
                unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, PERSONALIZE, None, KEY_NOTIFY | KEY_QUERY_VALUE, &mut key) };
            if opened != ERROR_SUCCESS {
                tracing::warn!("Наблюдение за темой Windows недоступно: {opened:?}");
                return;
            }
            let mut last = system_mode();
            loop {
                let status = unsafe { RegNotifyChangeKeyValue(key, false, REG_NOTIFY_CHANGE_LAST_SET, None, false) };
                if status != ERROR_SUCCESS {
                    tracing::warn!("RegNotifyChangeKeyValue: {status:?}");
                    break;
                }
                let now = system_mode();
                if now != last {
                    last = now;
                    on_change(now);
                }
            }
            unsafe {
                let _ = RegCloseKey(key);
            }
        })
        .expect("не удалось запустить поток theme-watch");
}
