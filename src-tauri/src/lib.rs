mod autopilot;
mod commands;
mod journal;
mod log;
mod monitor;
mod net;
mod notify;
mod settings;
mod state;
mod sys;
mod tgproxy;
mod tray;
mod update;
mod zapret;

use std::time::Duration;

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, RunEvent};

use commands::{apply_window_theme, effective_mode};
use settings::{SettingsStore, ThemeChoice};
use state::AppState;
use sys::theme;

/// «Включать обход при запуске программы»: последней стратегией, если обход ещё не работает
/// (служба или уже запущенный winws — не трогаем).
fn start_bypass_on_launch(st: &AppState, info: &zapret::layout::ZapretInfo, last: Option<&str>) {
    let Some(root) = info.path.as_deref().map(std::path::Path::new) else { return };
    if zapret::service::status(Some(root)).state != zapret::service::State::NotInstalled
        || sys::process::exists("winws.exe")
    {
        return;
    }
    let name = last
        .filter(|n| info.strategies.iter().any(|s| s.name == *n))
        .or(info.strategies.first().map(|s| s.name.as_str()));
    if let Some(name) = name {
        st.touch();
        match zapret::runner::start(&st.runner, root, name) {
            Ok(()) => journal::ok(format!("Обход включён при запуске программы («{name}»)")),
            Err(e) => journal::err(format!("Не удалось включить обход при запуске: {e}")),
        }
    }
}

/// Первые записи журнала и автопоиск папки zapret, если она не задана.
fn startup(app: &AppHandle) {
    let st = app.state::<AppState>();
    if !st.is_admin {
        journal::warn("Программа запущена без прав администратора — запуск обхода и служба недоступны");
    }
    let has_path = st.settings.lock().unwrap().get().zapret_path.is_some();
    if !has_path {
        if let Some(found) = zapret::layout::detect().into_iter().next() {
            let _ = st.settings.lock().unwrap().patch(&json!({ "zapretPath": found.display().to_string() }));
            journal::info(format!("Найдена папка zapret: {}", found.display()));
        }
    }
    let info = st.zapret_info();
    match (&info.path, info.valid) {
        (None, _) => journal::warn("Папка zapret не выбрана — укажите её в Настройках"),
        (Some(p), false) => journal::err(format!("Папка zapret «{p}»: {}", info.error.clone().unwrap_or_default())),
        (Some(p), true) => {
            journal::info(format!("Найдено стратегий: {}", info.strategies.len()));
            if zapret::tests::recover_ipset(std::path::Path::new(p)) {
                journal::warn("IPSet восстановлен после прерванной проверки стратегий");
            }
            if let Some(w) = &info.warning {
                journal::warn(w.clone());
            }
            if zapret::service::status(None).state == zapret::service::State::NotInstalled {
                journal::warn("Служба zapret не установлена");
            }
        }
    }

    let settings = st.settings.lock().unwrap().get().clone();
    if tgproxy::installed() {
        if settings.tg_autostart {
            match tgproxy::start(st.is_admin) {
                Ok(()) => {}
                Err(e) => journal::warn(format!("Telegram-прокси не запущен: {e}")),
            }
        }
        if settings.check_updates {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Ok(Some(v)) = tgproxy::check(&app).await {
                    journal::info(format!("Доступна новая версия TgWsProxy: {v}"));
                }
            });
        }
    }
    if settings.start_bypass_on_launch && st.is_admin && info.valid {
        start_bypass_on_launch(&st, &info, settings.last_strategy.as_deref());
    }
    // Автозапуск: обновляем задачу, если программу перенесли в другую папку.
    if settings.autostart && st.is_admin {
        if let Ok(exe) = std::env::current_exe() {
            if let Err(e) = sys::schtasks::enable(&exe) {
                journal::warn(format!("Не удалось обновить задачу автозапуска: {e}"));
            }
        }
    }
    if info.valid {
        if let Some(p) = &info.path {
            commands::sync_check_updates_flag(std::path::Path::new(p), settings.check_updates);
        }
        if settings.check_updates {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                update::check(&app, false).await;
            });
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    log::init();
    let is_admin = sys::elevation::is_elevated();
    tracing::info!(
        "ZapretGUI {} запущен, права администратора: {}",
        env!("CARGO_PKG_VERSION"),
        if is_admin { "да" } else { "нет" }
    );

    let window_state = tauri_plugin_window_state::Builder::default()
        .with_state_flags(tauri_plugin_window_state::StateFlags::all() - tauri_plugin_window_state::StateFlags::VISIBLE)
        .build();

    let app = tauri::Builder::default()
        // Первым: повторный запуск только поднимает уже открытое окно.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show_main(app)))
        .plugin(window_state)
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(AppState::new(SettingsStore::load_default(), is_admin))
        .setup(|app| {
            let handle = app.handle().clone();
            journal::attach(handle.clone());

            let choice = handle.state::<AppState>().settings.lock().unwrap().get().theme;
            if let Some(w) = app.get_webview_window("main") {
                apply_window_theme(&w, effective_mode(choice));
            }

            let watcher_handle = handle.clone();
            theme::watch(move |mode| {
                tracing::info!("Тема Windows изменилась: {mode:?}");
                let _ = watcher_handle.emit("theme://system", mode);
                let follows_system =
                    watcher_handle.state::<AppState>().settings.lock().unwrap().get().theme == ThemeChoice::System;
                if follows_system {
                    if let Some(w) = watcher_handle.get_webview_window("main") {
                        apply_window_theme(&w, mode);
                    }
                }
            });

            notify::register(&handle);
            if let Err(e) = tray::init(&handle) {
                tracing::warn!("Трей недоступен: {e}");
            }

            let bg = handle.clone();
            std::thread::spawn(move || {
                startup(&bg);
                monitor::start(bg.clone());
                autopilot::start(bg.clone());
                // Окно показывает интерфейс после первой отрисовки. Страховка на случай сбоя:
                std::thread::sleep(Duration::from_secs(4));
                if let Some(w) = bg.get_webview_window("main") {
                    let hidden_on_purpose = bg.state::<AppState>().start_hidden;
                    if !hidden_on_purpose && !w.is_visible().unwrap_or(true) {
                        tracing::warn!("Интерфейс не показал окно за 4 с — показываю принудительно");
                        let _ = w.show();
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::settings_set,
            commands::app_quit,
            commands::status_get,
            commands::journal_get,
            commands::zapret_inspect,
            commands::zapret_detect,
            commands::zapret_set_path,
            commands::bypass_start,
            commands::bypass_stop,
            commands::service_install,
            commands::service_remove,
            commands::service_start,
            commands::service_stop,
            commands::ipset_set,
            commands::game_set,
            commands::hosts_check,
            commands::hosts_apply,
            commands::tests_start,
            commands::tests_quick,
            commands::tray_hint,
            commands::notifications_set,
            commands::notify_test,
            commands::site_check,
            commands::site_add,
            commands::site_compare,
            commands::tests_site,
            commands::discord_info,
            commands::discord_step,
            commands::autopilot_check,
            commands::tg_config,
            commands::tg_config_save,
            commands::tg_new_secret,
            commands::tg_install,
            commands::tg_check,
            commands::tg_start,
            commands::tg_stop,
            commands::tg_restart,
            commands::tg_close_external,
            commands::tg_connect,
            commands::tests_cancel,
            commands::tests_last,
            commands::diag_run,
            commands::diag_fix,
            commands::lists_index,
            commands::list_read,
            commands::list_save,
            commands::update_check,
            commands::update_apply,
            commands::zapret_download,
            commands::autostart_set,
            commands::check_updates_set,
            commands::fakes_get,
            commands::fakes_set,
            commands::ipset_count,
            commands::ipset_update,
            commands::game_ports_set,
            commands::open_path,
            commands::settings_export,
            commands::settings_import,
            commands::settings_reset,
        ])
        .build(tauri::generate_context!())
        .expect("ошибка запуска приложения");

    app.run(|app, event| {
        if let RunEvent::Exit = event {
            let st = app.state::<AppState>();
            let stop = st.settings.lock().unwrap().get().stop_bypass_on_exit;
            if stop && st.runner.lock().unwrap().is_active() {
                let _ = zapret::runner::stop(&st.runner);
            }
        }
    });
}
