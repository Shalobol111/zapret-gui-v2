//! Значок в трее: два состояния (обход включён/выключен), меню
//! «Открыть · Включить/Выключить обход · Стратегия ▸ · Выход».

use std::sync::Mutex;

use serde_json::json;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::journal;
use crate::monitor::{BypassState, Status};
use crate::state::AppState;
use crate::zapret::{runner, service};

const ID: &str = "main";
const ICON_ON: &[u8] = include_bytes!("../icons/tray-on.png");
const ICON_OFF: &[u8] = include_bytes!("../icons/tray-off.png");

/// Меню перестраивается только при смене того, что в нём видно.
static LAST: Mutex<Option<String>> = Mutex::new(None);

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    TrayIconBuilder::with_id(ID)
        .icon(Image::from_bytes(ICON_OFF)?)
        .tooltip("ZapretGUI")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| on_menu(app, e.id.as_ref()))
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn build_menu(app: &AppHandle, s: &Status, current: Option<&str>) -> tauri::Result<Menu<Wry>> {
    let on = s.bypass.state == BypassState::On;
    let busy = matches!(s.bypass.state, BypassState::Starting | BypassState::Stopping) || s.tests_running;
    let svc = s.service.state != service::State::NotInstalled;
    let can = s.zapret.valid && s.is_admin && !busy;

    let open = MenuItem::with_id(app, "open", "Открыть", true, None::<&str>)?;
    let toggle =
        MenuItem::with_id(app, "toggle", if on { "Выключить обход" } else { "Включить обход" }, can, None::<&str>)?;
    let items: Vec<CheckMenuItem<Wry>> = s
        .zapret
        .strategies
        .iter()
        .map(|st| {
            let checked = current == Some(st.name.as_str());
            // При установленной службе стратегию меняют в окне — нужна переустановка с подтверждением.
            CheckMenuItem::with_id(app, format!("strategy:{}", st.name), &st.name, can && !svc, checked, None::<&str>)
        })
        .collect::<tauri::Result<_>>()?;
    let refs: Vec<&dyn tauri::menu::IsMenuItem<Wry>> =
        items.iter().map(|i| i as &dyn tauri::menu::IsMenuItem<Wry>).collect();
    let strategies = Submenu::with_items(app, "Стратегия", !items.is_empty(), &refs)?;
    let quit = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
    Menu::with_items(
        app,
        &[
            &open,
            &PredefinedMenuItem::separator(app)?,
            &toggle,
            &strategies,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )
}

/// Вызывается монитором при каждом изменении статуса.
pub fn update(app: &AppHandle, s: &Status) {
    let Some(tray) = app.tray_by_id(ID) else { return };
    let on = s.bypass.state == BypassState::On;
    let last_strategy = app.state::<AppState>().settings.lock().unwrap().get().last_strategy.clone();
    let current = s.bypass.strategy.clone().or(s.service.strategy.clone()).or(last_strategy);

    let tooltip = match (&s.bypass.state, &s.bypass.strategy) {
        (BypassState::On, Some(name)) => format!("ZapretGUI — обход включён · {name}"),
        (BypassState::On, None) => "ZapretGUI — обход включён".into(),
        (BypassState::Starting, _) => "ZapretGUI — запуск…".into(),
        (BypassState::Stopping, _) => "ZapretGUI — остановка…".into(),
        (BypassState::Off, _) => "ZapretGUI — обход выключен".into(),
    };
    let key = format!(
        "{on}|{:?}|{current:?}|{:?}|{}|{}|{}",
        s.bypass.state,
        s.service.state,
        s.tests_running,
        s.is_admin && s.zapret.valid,
        s.zapret.strategies.iter().map(|x| x.name.as_str()).collect::<Vec<_>>().join(";")
    );
    let mut last = LAST.lock().unwrap();
    if last.as_deref() == Some(key.as_str()) {
        return;
    }
    *last = Some(key);
    let _ = tray.set_tooltip(Some(tooltip));
    if let Ok(icon) = Image::from_bytes(if on { ICON_ON } else { ICON_OFF }) {
        let _ = tray.set_icon(Some(icon));
    }
    if let Ok(menu) = build_menu(app, s, current.as_deref()) {
        let _ = tray.set_menu(Some(menu));
    }
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        "open" => show_main(app),
        // Выход решает окно: в «Списках» могут быть несохранённые правки.
        "quit" => {
            if app.emit("app://quit", ()).is_err() {
                app.exit(0);
            }
        }
        "toggle" => spawn(app, None),
        other => {
            if let Some(name) = other.strip_prefix("strategy:") {
                spawn(app, Some(name.to_string()));
            }
        }
    }
}

/// Включить/выключить обход (`None`) или сменить стратегию — та же логика, что у кнопки питания.
fn spawn(app: &AppHandle, strategy: Option<String>) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let st = app.state::<AppState>();
        st.touch();
        let result = (|| -> Result<(), String> {
            st.require_admin()?;
            st.require_idle()?;
            let root = st.require_root()?;
            let svc = service::status(Some(&root));
            let running = st.runner.lock().unwrap().is_active() || crate::sys::process::exists("winws.exe");
            match strategy {
                Some(name) => {
                    let _ = st.settings.lock().unwrap().patch(&json!({ "lastStrategy": name }));
                    if running && svc.state == service::State::NotInstalled {
                        if st.runner.lock().unwrap().is_active() {
                            runner::stop(&st.runner)?;
                        }
                        runner::start(&st.runner, &root, &name)?;
                    }
                    Ok(())
                }
                None if svc.state == service::State::Running => service::stop(),
                None if svc.state != service::State::NotInstalled => service::start(),
                None if running => runner::stop(&st.runner),
                None => {
                    let names: Vec<String> = st.zapret_info().strategies.into_iter().map(|s| s.name).collect();
                    let last = st.settings.lock().unwrap().get().last_strategy.clone();
                    let name =
                        last.filter(|l| names.contains(l)).or_else(|| names.first().cloned()).ok_or("нет стратегий")?;
                    runner::start(&st.runner, &root, &name)
                }
            }
        })();
        if let Err(e) = result {
            journal::err(format!("Трей: {e}"));
            show_main(&app);
        }
        st.touch();
        st.poke();
    });
}
