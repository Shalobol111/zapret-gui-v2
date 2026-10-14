//! Уведомления Windows (WinRT toast).
//!
//! Плагин уведомлений Tauri на Windows не обрабатывает клик и молчит у неустановленной
//! (portable) программы. Поэтому: регистрируем AppUserModelID в
//! `HKCU\Software\Classes\AppUserModelId\<identifier>` (имя и иконка — так Windows показывает
//! уведомления программ без ярлыка в «Пуске») и показываем toast напрямую; клик открывает окно
//! на нужном экране.

use std::path::PathBuf;

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};
use tauri_winrt_notification::Toast;
use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

use crate::state::AppState;

const ICON: &[u8] = include_bytes!("../icons/128x128.png");

fn app_id(app: &AppHandle) -> String {
    app.config().identifier.clone()
}

/// Однократно при старте: регистрация имени и иконки для уведомлений.
pub fn register(app: &AppHandle) {
    let icon: PathBuf = crate::settings::data_dir().join("notify-icon.png");
    if !icon.is_file() {
        let _ = std::fs::create_dir_all(crate::settings::data_dir());
        let _ = std::fs::write(&icon, ICON);
    }
    register_id(&app_id(app), &icon);
}

fn register_id(id: &str, icon: &std::path::Path) {
    let path = format!(r"Software\Classes\AppUserModelId\{id}");
    match RegKey::predef(HKEY_CURRENT_USER).create_subkey(&path) {
        Ok((key, _)) => {
            let _ = key.set_value("DisplayName", &"ZapretGUI");
            let _ = key.set_value("IconUri", &icon.display().to_string());
        }
        Err(e) => tracing::warn!("Не удалось зарегистрировать уведомления: {e}"),
    }
}

/// Экран, который откроется по клику на уведомление.
#[derive(Debug, Clone, Copy)]
pub enum Screen {
    Main,
    Check,
    Settings,
}

impl Screen {
    fn id(self) -> &'static str {
        match self {
            Screen::Main => "main",
            Screen::Check => "check",
            Screen::Settings => "settings",
        }
    }
}

/// Показать уведомление, если они включены в настройках. Ошибки не критичны — всё
/// важное и так пишется в журнал.
pub fn show(app: &AppHandle, title: &str, body: &str, screen: Screen) {
    if !app.state::<AppState>().settings.lock().unwrap().get().notifications {
        return;
    }
    if let Err(e) = show_always(app, title, body, screen) {
        tracing::warn!("Уведомление не показано: {e}");
    }
}

/// Показ без учёта настройки (тестовое уведомление из Настроек).
pub fn show_always(app: &AppHandle, title: &str, body: &str, screen: Screen) -> Result<(), String> {
    let handle = app.clone();
    Toast::new(&app_id(app))
        .title(title)
        .text1(body)
        .on_activated(move |_| {
            crate::tray::show_main(&handle);
            let _ = handle.emit("nav://goto", screen.id());
            Ok(())
        })
        .show()
        .map_err(|e| format!("Windows не показал уведомление: {e}"))
}
/// Сбой стратегии (автопилот в режиме «Предупреждать»): кнопка «Подобрать» открывает
/// «Проверку» и сразу запускает быстрый подбор, клик по самому уведомлению — Главную.
pub fn show_pick(app: &AppHandle, title: &str, body: &str) {
    if !app.state::<AppState>().settings.lock().unwrap().get().notifications {
        return;
    }
    let handle = app.clone();
    let r = Toast::new(&app_id(app))
        .title(title)
        .text1(body)
        .add_button("Подобрать", "pick")
        .on_activated(move |action| {
            crate::tray::show_main(&handle);
            let pick = action.as_deref() == Some("pick");
            let _ = handle.emit("nav://goto", if pick { Screen::Check.id() } else { Screen::Main.id() });
            if pick {
                let app = handle.clone();
                std::thread::spawn(move || {
                    let st = app.state::<AppState>();
                    let r = st.require_root().and_then(|root| {
                        let current = crate::commands::current_strategy(&st, &root);
                        crate::zapret::tests::start(
                            &app,
                            crate::zapret::tests::Request::quick(&root, current.as_deref(), true),
                        )
                    });
                    if let Err(e) = r {
                        crate::journal::err(format!("Быстрый подбор не запустился: {e}"));
                    }
                });
            }
            Ok(())
        })
        .show();
    if let Err(e) = r {
        tracing::warn!("Уведомление не показано: {e}");
    }
}

/// Первый раз окно спрятано в трей — подсказать, что программа работает.
pub fn tray_hint_once(app: &AppHandle) {
    let st = app.state::<AppState>();
    if st.settings.lock().unwrap().get().tray_hint_shown {
        return;
    }
    let _ = st.settings.lock().unwrap().patch(&json!({ "trayHintShown": true }));
    show(app, "ZapretGUI работает в трее", "Обход продолжает работать. Открыть окно — значок у часов.", Screen::Main);
}

#[cfg(test)]
mod tests {
    /// Показывает настоящее уведомление: `cargo test -- --ignored live_toast`.
    #[test]
    #[ignore]
    fn live_toast() {
        let id = "io.github.shalobol111.zapretgui.dev";
        let icon = std::env::temp_dir().join("zapretgui-notify-test.png");
        std::fs::write(&icon, super::ICON).unwrap();
        super::register_id(id, &icon);
        tauri_winrt_notification::Toast::new(id)
            .title("Проверка уведомлений ZapretGUI")
            .text1("Если вы это видите — уведомления работают и у portable-версии.")
            .show()
            .expect("toast");
    }
}
