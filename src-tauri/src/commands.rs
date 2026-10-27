//! Команды IPC. Тонкий слой: проверки прав и пути, вызов zapret/*, запись в журнал.

use std::path::PathBuf;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::window::Color;
use tauri::{AppHandle, Manager, State, Theme, WebviewWindow};

use crate::journal::{self, Entry};
use crate::monitor::{self, Status};
use crate::settings::{Settings, ThemeChoice};
use crate::state::{self, AppState};
use crate::sys::schtasks;
use crate::sys::theme::{self, Mode};
use crate::zapret::filters::{self, GameMode, IpsetMode};
use crate::zapret::hosts::{self, Plan};
use crate::zapret::layout::{self, ZapretInfo};
use crate::zapret::tests::{self, model::Kind, model::Run};
use crate::zapret::{diagnostics, discord, extras, runner, service, site, updater, userlists};

type Res<T> = Result<T, String>;

/// Блокирующая работа вне главного потока; после неё статус пересчитывается сразу.
async fn blocking<T: Send + 'static>(app: &AppHandle, f: impl FnOnce(&AppState) -> Res<T> + Send + 'static) -> Res<T> {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let st = app.state::<AppState>();
        st.touch();
        let r = f(&st);
        st.touch();
        st.poke();
        r
    })
    .await
    .map_err(|e| e.to_string())?
}

// ── Окно и настройки ───────────────────────────────────────────

pub fn effective_mode(choice: ThemeChoice) -> Mode {
    match choice {
        ThemeChoice::Dark => Mode::Dark,
        ThemeChoice::Light => Mode::Light,
        ThemeChoice::System => theme::system_mode(),
    }
}

/// Заголовок окна и фон WebView до первой отрисовки — в цвет темы (`--bg` из макета).
pub fn apply_window_theme(window: &WebviewWindow, mode: Mode) {
    let (theme, bg) = match mode {
        Mode::Dark => (Theme::Dark, Color(0x0D, 0x0F, 0x12, 0xFF)),
        Mode::Light => (Theme::Light, Color(0xF4, 0xF5, 0xF7, 0xFF)),
    };
    let _ = window.set_theme(Some(theme));
    let _ = window.set_background_color(Some(bg));
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    settings: Settings,
    is_admin: bool,
    system_theme: Mode,
    app_version: String,
    /// Запуск из автозагрузки (--minimized): окно не показываем, если включён трей.
    start_hidden: bool,
}

#[tauri::command]
pub fn bootstrap(app: AppHandle, state: State<AppState>) -> Bootstrap {
    // Настройки читаются один раз и в отдельном операторе: временный MutexGuard живёт до конца
    // оператора, и второй `lock()` в том же литерале (он выполнялся только при --minimized)
    // навсегда вешал главный поток — команда синхронная. Так и зависал автозапуск.
    let settings = state.settings.lock().unwrap().get().clone();
    let start_hidden = state.start_hidden && settings.minimize_to_tray;
    Bootstrap {
        settings,
        is_admin: state.is_admin,
        system_theme: theme::system_mode(),
        app_version: app.package_info().version.to_string(),
        start_hidden,
    }
}

#[tauri::command]
pub fn settings_set(patch: Value, window: WebviewWindow, state: State<AppState>) -> Res<Settings> {
    let next = state.settings.lock().unwrap().patch(&patch)?;
    if patch.get("theme").is_some() {
        apply_window_theme(&window, effective_mode(next.theme));
    }
    Ok(next)
}

#[tauri::command]
pub fn app_quit(app: AppHandle) {
    app.exit(0);
}

// ── Статус и журнал ────────────────────────────────────────────

#[tauri::command]
pub async fn status_get(app: AppHandle) -> Res<Status> {
    blocking(&app, |st| Ok(monitor::compute(st))).await
}

#[tauri::command]
pub fn journal_get() -> Vec<Entry> {
    journal::snapshot()
}

// ── Папка zapret ───────────────────────────────────────────────

#[tauri::command]
pub async fn zapret_inspect(path: String) -> Res<ZapretInfo> {
    tauri::async_runtime::spawn_blocking(move || state::inspect(&PathBuf::from(path.trim())))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn zapret_detect() -> Res<Vec<String>> {
    tauri::async_runtime::spawn_blocking(|| layout::detect().iter().map(|p| p.display().to_string()).collect())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn zapret_set_path(app: AppHandle, path: String) -> Res<ZapretInfo> {
    blocking(&app, move |st| {
        let path = PathBuf::from(path.trim());
        let info = state::inspect(&path);
        if !info.valid {
            return Err(info.error.unwrap_or_else(|| "папка не подходит".into()));
        }
        st.settings.lock().unwrap().patch(&json!({ "zapretPath": path.display().to_string() }))?;
        st.invalidate_zapret();
        journal::ok(format!("Папка zapret: {} · найдено стратегий: {}", path.display(), info.strategies.len()));
        Ok(info)
    })
    .await
}

// ── Обход ──────────────────────────────────────────────────────

fn remember_strategy(st: &AppState, name: &str) {
    let _ = st.settings.lock().unwrap().patch(&json!({ "lastStrategy": name }));
}

#[tauri::command]
pub async fn bypass_start(app: AppHandle, strategy: String) -> Res<()> {
    blocking(&app, move |st| {
        st.require_admin()?;
        st.require_idle()?;
        let root = st.require_root()?;
        if service::status(Some(&root)).state != service::State::NotInstalled {
            return Err("Установлена служба zapret — обход управляется ею".into());
        }
        remember_strategy(st, &strategy);
        // Смена стратегии на ходу — перезапуск.
        if st.runner.lock().unwrap().is_active() {
            runner::stop(&st.runner)?;
        }
        runner::start(&st.runner, &root, &strategy)
    })
    .await
}

#[tauri::command]
pub async fn bypass_stop(app: AppHandle) -> Res<()> {
    blocking(&app, |st| {
        st.require_admin()?;
        st.require_idle()?;
        if service::status(None).state == service::State::Running {
            return Err("Обход работает как служба — остановите службу".into());
        }
        runner::stop(&st.runner)
    })
    .await
}

// ── Служба ─────────────────────────────────────────────────────

#[tauri::command]
pub async fn service_install(app: AppHandle, strategy: String) -> Res<()> {
    blocking(&app, move |st| {
        st.require_admin()?;
        st.require_idle()?;
        let root = st.require_root()?;
        if st.runner.lock().unwrap().is_active() {
            runner::stop(&st.runner)?;
        }
        remember_strategy(st, &strategy);
        match service::install(&root, &strategy) {
            Ok(()) => {
                journal::ok(format!("Служба zapret установлена со стратегией «{strategy}»"));
                Ok(())
            }
            Err(e) => {
                journal::err(format!("Не удалось установить службу: {e}"));
                Err(e)
            }
        }
    })
    .await
}

#[tauri::command]
pub async fn service_remove(app: AppHandle) -> Res<()> {
    blocking(&app, |st| {
        st.require_admin()?;
        st.require_idle()?;
        service::remove_all().inspect_err(|e| journal::err(format!("Не удалось удалить службы: {e}")))?;
        journal::ok("Службы zapret и WinDivert удалены");
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn service_start(app: AppHandle) -> Res<()> {
    blocking(&app, |st| {
        st.require_admin()?;
        st.require_idle()?;
        service::start().inspect_err(|e| journal::err(format!("Служба не запустилась: {e}")))?;
        journal::ok("Служба zapret запущена");
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn service_stop(app: AppHandle) -> Res<()> {
    blocking(&app, |st| {
        st.require_admin()?;
        st.require_idle()?;
        service::stop().inspect_err(|e| journal::err(format!("Служба не остановилась: {e}")))?;
        journal::info("Служба zapret остановлена");
        Ok(())
    })
    .await
}

// ── Фильтры ────────────────────────────────────────────────────

#[tauri::command]
pub async fn ipset_set(app: AppHandle, mode: IpsetMode) -> Res<()> {
    blocking(&app, move |st| {
        st.require_idle()?;
        let root = st.require_root()?;
        filters::set_ipset(&root, mode)?;
        journal::info(format!(
            "IPSet → {}",
            match mode {
                IpsetMode::Off => "выкл",
                IpsetMode::Any => "любой",
                IpsetMode::Loaded => "загруженный",
            }
        ));
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn game_set(app: AppHandle, mode: GameMode) -> Res<()> {
    blocking(&app, move |st| {
        st.require_idle()?;
        let root = st.require_root()?;
        filters::set_game(&root, mode, None, None)?;
        journal::info(format!(
            "Game filter → {}",
            match mode {
                GameMode::Off => "выкл",
                GameMode::Tcp => "TCP",
                GameMode::Udp => "UDP",
                GameMode::TcpUdp => "TCP+UDP",
            }
        ));
        Ok(())
    })
    .await
}

// ── hosts ──────────────────────────────────────────────────────

#[tauri::command]
pub async fn hosts_check(app: AppHandle) -> Res<Plan> {
    let remote = hosts::fetch().await.inspect_err(|e| journal::err(format!("hosts: {e}")))?;
    let current = hosts::read_current()?;
    let plan = hosts::plan(&current, &remote);
    *app.state::<AppState>().hosts_remote.lock().unwrap() = Some(remote);
    if plan.up_to_date {
        journal::ok("Файл hosts актуален");
    }
    Ok(plan)
}

#[tauri::command]
pub async fn hosts_apply(app: AppHandle) -> Res<()> {
    blocking(&app, |st| {
        st.require_admin()?;
        st.require_idle()?;
        let remote = st.hosts_remote.lock().unwrap().clone().ok_or("Сначала проверьте hosts")?;
        match hosts::apply(&remote) {
            Ok(()) => {
                journal::ok("Файл hosts обновлён (резервная копия: hosts.zapretgui.bak)");
                Ok(())
            }
            Err(e) => {
                journal::err(format!("hosts: {e}"));
                hosts::open_manual(&remote)?;
                Err(format!("{e}. Открыт скачанный файл — скопируйте его содержимое в hosts вручную"))
            }
        }
    })
    .await
}

// ── Проверка стратегий ─────────────────────────────────────────

#[tauri::command]
pub async fn tests_start(app: AppHandle, kind: Kind, strategies: Vec<String>, stop_service: bool) -> Res<()> {
    let a = app.clone();
    tauri::async_runtime::spawn_blocking(move || tests::start(&a, tests::Request::full(kind, strategies, stop_service)))
        .await
        .map_err(|e| e.to_string())?
}

/// Быстрый подбор: Discord + YouTube, сначала стратегии, работавшие раньше, до первой рабочей.
#[tauri::command]
pub async fn tests_quick(app: AppHandle, stop_service: bool) -> Res<()> {
    let a = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let st = a.state::<AppState>();
        let root = st.require_root()?;
        let current = current_strategy(&st, &root);
        tests::start(&a, tests::Request::quick(&root, current.as_deref(), stop_service))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Стратегия, которая сейчас работает (служба, ручной или внешний запуск) или выбрана последней.
pub fn current_strategy(st: &AppState, root: &std::path::Path) -> Option<String> {
    let svc = service::status(Some(root));
    if svc.state != service::State::NotInstalled {
        return svc.strategy;
    }
    let gui = st.runner.lock().unwrap().strategy.clone();
    gui.or_else(|| {
        crate::sys::process::find("winws.exe").first().and_then(|p| crate::zapret::match_strategy(root, &p.args))
    })
    .or_else(|| st.settings.lock().unwrap().get().last_strategy.clone())
}

/// Окно спрятано в трей (вызывает интерфейс) — один раз показать подсказку.
#[tauri::command]
pub fn tray_hint(app: AppHandle) {
    crate::notify::tray_hint_once(&app);
}

/// Тестовое уведомление из Настроек — показывается даже при выключенном переключателе.
#[tauri::command]
pub fn notify_test(app: AppHandle) -> Res<()> {
    crate::notify::show_always(
        &app,
        "Уведомления ZapretGUI работают",
        "Так программа сообщит, если обход остановится или выйдет новая версия zapret.",
        crate::notify::Screen::Settings,
    )
}

#[tauri::command]
pub fn notifications_set(enabled: bool, state: State<AppState>) -> Res<Settings> {
    state.settings.lock().unwrap().patch(&json!({ "notifications": enabled }))
}

#[tauri::command]
pub fn tests_cancel(app: AppHandle) {
    tests::cancel(&app);
}

#[tauri::command]
pub fn tests_last(state: State<AppState>) -> Option<Run> {
    state.tests.lock().unwrap().clone()
}

// ── Диагностика ────────────────────────────────────────────────

#[tauri::command]
pub async fn diag_run(app: AppHandle) -> Res<Vec<diagnostics::Item>> {
    blocking(&app, |st| Ok(diagnostics::run(st.root().as_deref()))).await
}

#[derive(Serialize)]
pub struct FixResult {
    fix: diagnostics::Fix,
    ok: bool,
    message: String,
}

#[tauri::command]
pub async fn diag_fix(app: AppHandle, fixes: Vec<diagnostics::Fix>) -> Res<Vec<FixResult>> {
    blocking(&app, move |st| {
        st.require_admin()?;
        st.require_idle()?;
        Ok(diagnostics::apply_all(&fixes)
            .into_iter()
            .map(|(fix, r)| match r {
                Ok(message) => FixResult { fix, ok: true, message },
                Err(message) => FixResult { fix, ok: false, message },
            })
            .collect())
    })
    .await
}
// ── Списки ─────────────────────────────────────────────────────

#[tauri::command]
pub async fn lists_index(app: AppHandle) -> Res<Vec<userlists::ListInfo>> {
    blocking(&app, |st| Ok(userlists::index(&st.require_root()?))).await
}

#[tauri::command]
pub async fn list_read(app: AppHandle, id: String) -> Res<userlists::ListData> {
    blocking(&app, move |st| userlists::read(&st.require_root()?, &id)).await
}

/// Ошибка "conflict" — файл изменили снаружи; интерфейс предложит перезаписать или перечитать.
#[tauri::command]
pub async fn list_save(
    app: AppHandle,
    id: String,
    text: String,
    base_hash: String,
    force: bool,
) -> Res<userlists::ListData> {
    blocking(&app, move |st| {
        let root = st.require_root()?;
        match userlists::save(&root, &id, &text, &base_hash, force) {
            Ok(d) => {
                journal::ok(format!("Сохранён список {}", d.info.file));
                Ok(d)
            }
            Err(userlists::SaveError::Conflict) => Err("conflict".into()),
            Err(userlists::SaveError::Other(e)) => {
                journal::err(e.clone());
                Err(e)
            }
        }
    })
    .await
}
// ── Обновление и установка zapret ──────────────────────────────

#[tauri::command]
pub async fn update_check(app: AppHandle) -> Res<updater::Update> {
    Ok(crate::update::check(&app, true).await)
}

#[tauri::command]
pub async fn update_apply(app: AppHandle) -> Res<String> {
    crate::update::apply(app).await
}

#[tauri::command]
pub async fn zapret_download(app: AppHandle, dest: String) -> Res<String> {
    crate::update::install(app, dest).await
}

// ── Поведение ──────────────────────────────────────────────────

#[tauri::command]
pub async fn autostart_set(app: AppHandle, enabled: bool) -> Res<Settings> {
    blocking(&app, move |st| {
        st.require_admin()?;
        if enabled {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            schtasks::enable(&exe)?;
            journal::ok("Автозапуск включён (Планировщик задач)");
        } else {
            schtasks::disable()?;
            journal::info("Автозапуск выключен");
        }
        st.settings.lock().unwrap().patch(&json!({ "autostart": enabled }))
    })
    .await
}

/// Флаг `utils\check_updates.enabled` — его же читают стратегии при ручном запуске .bat.
pub fn sync_check_updates_flag(root: &std::path::Path, enabled: bool) {
    let flag = root.join("utils").join("check_updates.enabled");
    let _ = if enabled { std::fs::write(&flag, "ENABLED \r\n") } else { std::fs::remove_file(&flag) };
}

#[tauri::command]
pub async fn check_updates_set(app: AppHandle, enabled: bool) -> Res<Settings> {
    blocking(&app, move |st| {
        if let Some(root) = st.root() {
            sync_check_updates_flag(&root, enabled);
        }
        st.settings.lock().unwrap().patch(&json!({ "checkUpdates": enabled }))
    })
    .await
}

// ── Дополнительно ──────────────────────────────────────────────

#[tauri::command]
pub async fn fakes_get(app: AppHandle) -> Res<extras::Fakes> {
    blocking(&app, |st| Ok(extras::fakes(&st.require_root()?))).await
}

#[tauri::command]
pub async fn fakes_set(app: AppHandle, kind: extras::FakeKind, name: String) -> Res<()> {
    blocking(&app, move |st| {
        st.require_idle()?;
        extras::set_fake(&st.require_root()?, kind, &name)?;
        journal::info(format!(
            "Активный фейк {} → {name}",
            match kind {
                extras::FakeKind::Discord => "Discord UDP",
                extras::FakeKind::Game => "Game UDP",
            }
        ));
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn ipset_count(app: AppHandle) -> Res<usize> {
    blocking(&app, |st| Ok(extras::ipset_count(&st.require_root()?))).await
}

#[tauri::command]
pub async fn ipset_update(app: AppHandle) -> Res<usize> {
    let root = app.state::<AppState>().require_root()?;
    let n = extras::update_ipset(&root).await.inspect_err(|e| journal::err(format!("IPSet-список: {e}")))?;
    journal::ok(format!("IPSet-список обновлён: {n} записей"));
    app.state::<AppState>().poke();
    Ok(n)
}

#[tauri::command]
pub async fn game_ports_set(app: AppHandle, tcp: String, udp: String) -> Res<()> {
    blocking(&app, move |st| {
        st.require_idle()?;
        let root = st.require_root()?;
        let mode = filters::game(&root).mode;
        filters::set_game(&root, mode, Some(&tcp), Some(&udp))?;
        journal::info(format!("Порты Game Filter: TCP {tcp} · UDP {udp}"));
        Ok(())
    })
    .await
}

#[tauri::command]
pub fn open_path(target: String, state: State<AppState>) -> Res<()> {
    let path = match target.as_str() {
        "zapret" => state.root().ok_or("папка zapret не выбрана")?,
        "logs" => crate::settings::data_dir().join("logs"),
        "results" => state.require_root()?.join("utils").join("test results"),
        // Фиксированные адреса: страница релизов (браузер) и сетевые параметры Windows (Secure DNS).
        // VPN: лог sing-box и свои списки — файлами (откроются в Блокноте).
        "vpnlog" | "vpnlist-vpn" | "vpnlist-direct" => {
            crate::vpn::lists::ensure_custom();
            let target = match target.as_str() {
                "vpnlog" => crate::vpn::log_file(),
                "vpnlist-vpn" => crate::vpn::lists::custom_path(crate::vpn::lists::VPN_FILE),
                _ => crate::vpn::lists::custom_path(crate::vpn::lists::DIRECT_FILE),
            };
            let target = if target.is_file() { target } else { crate::vpn::dir() };
            return crate::sys::cmd::hidden("explorer.exe").arg(target).spawn().map(|_| ()).map_err(|e| e.to_string());
        }
        // Лог TgWsProxy — файлом (откроется в Блокноте), папку — если лога ещё нет.
        "tglog" => {
            let log = crate::tgproxy::log_file();
            let target = if log.is_file() { log } else { crate::tgproxy::dir() };
            return crate::sys::cmd::hidden("explorer.exe").arg(target).spawn().map(|_| ()).map_err(|e| e.to_string());
        }
        "releases" | "dns" | "tgreleases" | "appreleases" => {
            let target = match target.as_str() {
                "dns" => "ms-settings:network",
                "tgreleases" => crate::tgproxy::RELEASES_URL,
                "appreleases" => crate::selfupdate::RELEASES_URL,
                _ => updater::RELEASES_URL,
            };
            return crate::sys::cmd::hidden("explorer.exe").arg(target).spawn().map(|_| ()).map_err(|e| e.to_string());
        }
        _ => return Err("неизвестная папка".into()),
    };
    let _ = std::fs::create_dir_all(&path);
    crate::sys::cmd::hidden("explorer.exe").arg(&path).spawn().map(|_| ()).map_err(|e| e.to_string())
}

// ── Настройки GUI ──────────────────────────────────────────────

#[tauri::command]
pub fn settings_export(path: String, state: State<AppState>) -> Res<()> {
    let text = serde_json::to_string_pretty(state.settings.lock().unwrap().get()).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("не удалось сохранить: {e}"))?;
    journal::ok(format!("Настройки экспортированы в {path}"));
    Ok(())
}

/// Наш формат или gui_settings.json от zapret-gui 1.x (ключи в snake_case).
fn import_patch(v: &Value) -> Value {
    let Some(obj) = v.as_object() else { return json!({}) };
    if !obj.contains_key("program_path") {
        return v.clone();
    }
    let mut p = serde_json::Map::new();
    if let Some(s) = obj.get("program_path").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        p.insert("zapretPath".into(), json!(s));
    }
    if let Some(t) = obj.get("theme").and_then(Value::as_str) {
        let t = t.to_lowercase();
        if ["dark", "light", "system"].contains(&t.as_str()) {
            p.insert("theme".into(), json!(t));
        }
    }
    for (old, new) in
        [("minimize_to_tray", "minimizeToTray"), ("check_updates", "checkUpdates"), ("autostart", "autostart")]
    {
        if let Some(b) = obj.get(old).and_then(Value::as_bool) {
            p.insert(new.into(), json!(b));
        }
    }
    Value::Object(p)
}

#[tauri::command]
pub async fn settings_import(app: AppHandle, path: String, window: WebviewWindow) -> Res<Settings> {
    blocking(&app, move |st| {
        let text = std::fs::read_to_string(&path).map_err(|e| format!("не удалось прочитать файл: {e}"))?;
        let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
            .map_err(|_| "это не файл настроек".to_string())?;
        let mut patch = import_patch(&v);
        // Схему и выбор тестов не переносим; путь — только если там есть zapret.
        if let Some(o) = patch.as_object_mut() {
            o.remove("schema");
            if o.get("zapretPath")
                .and_then(Value::as_str)
                .is_some_and(|p| !state::inspect(std::path::Path::new(p)).valid)
            {
                o.remove("zapretPath");
            }
        }
        let before = st.settings.lock().unwrap().get().autostart;
        let next = st.settings.lock().unwrap().patch(&patch)?;
        if next.autostart != before && st.is_admin {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let _ = if next.autostart { schtasks::enable(&exe) } else { schtasks::disable() };
        }
        st.invalidate_zapret();
        apply_window_theme(&window, effective_mode(next.theme));
        journal::ok("Настройки импортированы");
        Ok(next)
    })
    .await
}

#[tauri::command]
pub async fn settings_reset(app: AppHandle, window: WebviewWindow) -> Res<Settings> {
    blocking(&app, move |st| {
        let cur = st.settings.lock().unwrap().get().clone();
        let defaults = serde_json::to_value(Settings::default()).map_err(|e| e.to_string())?;
        st.settings.lock().unwrap().patch(&defaults)?;
        // Папку zapret оставляем — без неё программа бесполезна, а найти её заново — лишний шаг.
        let next = st.settings.lock().unwrap().patch(&json!({ "zapretPath": cur.zapret_path }))?;
        if cur.autostart {
            let _ = schtasks::disable();
        }
        apply_window_theme(&window, effective_mode(next.theme));
        journal::info("Настройки программы сброшены");
        Ok(next)
    })
    .await
}

// ── «Мой сайт» ─────────────────────────────────────────────────

#[tauri::command]
pub async fn site_check(app: AppHandle, input: String) -> Res<site::Report> {
    blocking(&app, move |st| {
        let root = st.require_root()?;
        let domain = site::normalize(&input)?;
        let bypass_on = monitor::compute(st).bypass.state == monitor::BypassState::On;
        let report = site::check(&root, &domain, bypass_on)?;
        journal::info(format!("Мой сайт: {domain} — {}", report.verdict));
        Ok(report)
    })
    .await
}

#[tauri::command]
pub async fn site_add(app: AppHandle, domain: String) -> Res<bool> {
    blocking(&app, move |st| {
        let root = st.require_root()?;
        let domain = site::normalize(&domain)?;
        let added = site::add_to_user_list(&root, &domain)?;
        if added {
            journal::ok(format!("{domain} добавлен в «Свои домены»"));
        }
        Ok(added)
    })
    .await
}

/// Та же проверка доступа, но с выключенным на время обходом (служба — тоже).
#[tauri::command]
pub async fn site_compare(app: AppHandle, domain: String) -> Res<site::Probe> {
    let a = app.clone();
    blocking(&app, move |st| {
        st.require_admin()?;
        st.require_idle()?;
        let root = st.require_root()?;
        let domain = site::normalize(&domain)?;
        let curl = tests::curl::curl_path().ok_or("Не найден curl.exe")?;
        let paused = tests::prepare(&a, &root, Kind::Custom);
        crate::sys::process::kill_all("winws.exe");
        std::thread::sleep(std::time::Duration::from_millis(500));
        let probe = site::probe(&curl, &domain);
        tests::restore(&a, &root, paused);
        journal::info(format!(
            "Мой сайт: {domain} без обхода — {}",
            if probe.reachable() { "открывается" } else { "не открывается" }
        ));
        Ok(probe)
    })
    .await
}

/// Подбор стратегии под сайт: до первой стратегии, с которой он открылся.
#[tauri::command]
pub async fn tests_site(app: AppHandle, domain: String, stop_service: bool) -> Res<()> {
    let a = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let st = a.state::<AppState>();
        let root = st.require_root()?;
        let domain = site::normalize(&domain)?;
        let current = current_strategy(&st, &root);
        tests::start(&a, tests::Request::site(&root, &domain, current.as_deref(), stop_service))
    })
    .await
    .map_err(|e| e.to_string())?
}

// ── «Починить Discord» ─────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscordInfo {
    clients: Vec<discord::Installed>,
    bypass_on: bool,
    strategy: Option<String>,
    service: bool,
}

#[tauri::command]
pub async fn discord_info(app: AppHandle) -> Res<DiscordInfo> {
    blocking(&app, |st| {
        let bypass = monitor::compute(st).bypass;
        let strategy = st.root().and_then(|root| current_strategy(st, &root));
        Ok(DiscordInfo {
            clients: discord::list(),
            bypass_on: bypass.state == monitor::BypassState::On,
            strategy,
            service: service::status(None).state != service::State::NotInstalled,
        })
    })
    .await
}

#[tauri::command]
pub async fn discord_step(app: AppHandle, step: discord::Step, ids: Vec<String>) -> Res<discord::StepResult> {
    use diagnostics::Level;
    use discord::{Step, StepResult};
    blocking(&app, move |st| {
        let r = match step {
            Step::Clean => {
                if discord::installed().is_empty() {
                    return Ok(StepResult::new(Level::Info, "Discord не найден"));
                }
                let closed = discord::close();
                let cleared = discord::clear_cache();
                let value = if closed.is_empty() { "кэш очищен" } else { "закрыт, кэш очищен" };
                let mut r = StepResult::new(Level::Ok, value)
                    .detail(format!("Удалено папок кэша: {cleared}. Настройки и вход в аккаунт не затронуты"));
                r.closed = closed;
                r
            }
            Step::Hosts => {
                st.require_admin()?;
                st.require_idle()?;
                let remote = match tauri::async_runtime::block_on(hosts::fetch()) {
                    Ok(t) => t,
                    Err(e) => {
                        return Ok(StepResult::new(Level::Warn, "не удалось скачать")
                            .detail(format!("Список Flowseal недоступен: {e}")))
                    }
                };
                let plan = hosts::plan(&hosts::read_current()?, &remote);
                if plan.up_to_date {
                    StepResult::new(Level::Ok, "актуален")
                } else {
                    match hosts::apply(&remote) {
                        Ok(()) => StepResult::new(Level::Ok, "обновлён").detail(format!(
                            "Добавлено записей: {}, заменено: {}. Резервная копия — hosts.zapretgui.bak",
                            plan.add, plan.replace
                        )),
                        Err(e) => StepResult::new(Level::Err, "не записан").detail(e),
                    }
                }
            }
            Step::Voice => {
                st.require_admin()?;
                st.require_idle()?;
                let root = st.require_root()?;
                let mut started = None;
                if monitor::compute(st).bypass.state != monitor::BypassState::On {
                    let Some(name) = current_strategy(st, &root) else {
                        return Ok(StepResult::new(Level::Warn, "обход выключен")
                            .detail("Стратегия ещё не выбрана — её подберёт следующий шаг"));
                    };
                    let res = if service::status(Some(&root)).state != service::State::NotInstalled {
                        service::start()
                    } else {
                        remember_strategy(st, &name);
                        runner::start(&st.runner, &root, &name)
                    };
                    if let Err(e) = res {
                        return Ok(StepResult::new(Level::Err, "обход не запустился").detail(e));
                    }
                    journal::ok(format!("Починить Discord: обход включён («{name}»)"));
                    std::thread::sleep(std::time::Duration::from_millis(800));
                    started = Some(name);
                }
                // Командной строки может не быть (нет доступа к процессу) — тогда аргументы из файла стратегии.
                let running =
                    crate::sys::process::find("winws.exe").into_iter().map(|p| p.args).find(|a| !a.is_empty());
                let args = running.or_else(|| {
                    current_strategy(st, &root)
                        .and_then(|n| crate::zapret::strategy_args(&root, &n).ok())
                        .map(|(_, a)| a)
                });
                let Some(args) = args else {
                    return Ok(StepResult::new(Level::Info, "не удалось проверить")
                        .detail("Не удалось прочитать параметры winws.exe и определить стратегию"));
                };
                let missing = discord::missing_voice_ports(&args);
                let on = match &started {
                    Some(n) => format!("Обход включён со стратегией «{n}». "),
                    None => String::new(),
                };
                if missing.is_empty() {
                    StepResult::new(Level::Ok, if started.is_some() { "обход включён" } else { "в порядке" }).detail(
                        format!(
                            "{on}Голосовые порты UDP {} обрабатываются",
                            discord::format_ports(&discord::VOICE_PORTS)
                        ),
                    )
                } else {
                    StepResult::new(Level::Warn, "нет UDP-фильтров голоса").detail(format!(
                        "{on}Стратегия не обрабатывает порты {} — голосовые каналы могут не подключаться. \
                         Выберите стратегию из набора Flowseal",
                        discord::format_ports(&missing)
                    ))
                }
            }
            Step::Sites => {
                let curl = tests::curl::curl_path().ok_or("Не найден curl.exe")?;
                let failing = discord::failing_sites(&curl);
                if failing.is_empty() {
                    StepResult::new(Level::Ok, "открываются").detail(discord::SITES.join(", "))
                } else {
                    let mut r = StepResult::new(
                        Level::Err,
                        format!("не открываются: {} из {}", failing.len(), discord::SITES.len()),
                    )
                    .detail(failing.join(", "));
                    r.pick = st.require_admin().is_ok() && st.root().is_some();
                    r
                }
            }
            Step::Launch => {
                let mut ids = ids;
                if ids.is_empty() {
                    ids.extend(discord::installed().first().map(|v| v.id.to_string()));
                }
                if ids.is_empty() {
                    return Ok(StepResult::new(Level::Info, "Discord не найден"));
                }
                match discord::launch(&ids) {
                    Ok(names) => StepResult::new(Level::Ok, "запущен").detail(names.join(", ")),
                    Err(e) => StepResult::new(Level::Err, "не запустился").detail(e),
                }
            }
        };
        journal::info(format!("Починить Discord: {:?} — {}", step, r.value));
        Ok(r)
    })
    .await
}

// ── Автопилот ──────────────────────────────────────────────────

/// «Проверить сейчас»: проверка здоровья вне очереди (результат придёт в статусе).
#[tauri::command]
pub fn autopilot_check() {
    crate::autopilot::check_now();
}

// ── Telegram-прокси ────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TgConfig {
    #[serde(flatten)]
    config: crate::tgproxy::Config,
    link: Option<String>,
}

fn tg_view(c: crate::tgproxy::Config) -> TgConfig {
    let link = crate::tgproxy::valid_secret(&c.secret).then(|| crate::tgproxy::link(&c));
    TgConfig { config: c, link }
}

#[tauri::command]
pub async fn tg_config(app: AppHandle) -> Res<TgConfig> {
    blocking(&app, |_| {
        let c = if crate::tgproxy::installed() { crate::tgproxy::ensure_config()? } else { crate::tgproxy::config() };
        Ok(tg_view(c))
    })
    .await
}

/// Сохранить настройки прокси; работающий прокси перезапускается.
#[tauri::command]
pub async fn tg_config_save(app: AppHandle, config: crate::tgproxy::Config) -> Res<TgConfig> {
    blocking(&app, move |st| {
        let saved = crate::tgproxy::save_config(&config)?;
        journal::ok(format!("Настройки Telegram-прокси сохранены · порт {}", saved.port));
        if st.status_tg_running() {
            crate::tgproxy::restart(st.is_admin)?;
        }
        Ok(tg_view(saved))
    })
    .await
}

#[tauri::command]
pub fn tg_new_secret() -> String {
    crate::tgproxy::regenerate_secret()
}

#[tauri::command]
pub async fn tg_install(app: AppHandle) -> Res<String> {
    crate::tgproxy::install(app).await
}

#[tauri::command]
pub async fn tg_check(app: AppHandle) -> Res<Option<String>> {
    crate::tgproxy::check(&app).await
}

#[tauri::command]
pub async fn tg_start(app: AppHandle) -> Res<()> {
    blocking(&app, |st| {
        crate::tgproxy::start(st.is_admin).inspect_err(|e| journal::err(format!("Telegram-прокси: {e}")))
    })
    .await
}

#[tauri::command]
pub async fn tg_stop(app: AppHandle) -> Res<()> {
    blocking(&app, |_| crate::tgproxy::stop()).await
}

#[tauri::command]
pub async fn tg_restart(app: AppHandle) -> Res<()> {
    blocking(&app, |st| crate::tgproxy::restart(st.is_admin)).await
}

/// Закрыть отдельно запущенный TgWsProxy и запустить копию ZapretGUI.
#[tauri::command]
pub async fn tg_close_external(app: AppHandle) -> Res<()> {
    blocking(&app, |st| {
        crate::tgproxy::close_external()?;
        crate::tgproxy::start(st.is_admin)
    })
    .await
}

#[tauri::command]
pub async fn tg_connect(app: AppHandle) -> Res<()> {
    blocking(&app, |_| crate::tgproxy::connect_telegram()).await
}

// ── Обновление ZapretGUI ───────────────────────────────────────

#[tauri::command]
pub async fn app_update_check(app: AppHandle) -> Res<crate::selfupdate::Info> {
    Ok(crate::selfupdate::check(&app, true).await)
}

/// Скачать и установить новую версию; при успехе программа закроется и запустится заново.
#[tauri::command]
pub async fn app_update_install(app: AppHandle) -> Res<()> {
    app.state::<AppState>().require_admin()?;
    app.state::<AppState>().require_idle()?;
    crate::selfupdate::install(app).await
}

// ── VPN (sing-box) ─────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VpnView {
    store: crate::vpn::Store,
    presets: Vec<crate::vpn::lists::PresetInfo>,
    pings: std::collections::HashMap<String, Option<u32>>,
}

fn vpn_view_now(st: &AppState) -> VpnView {
    let mut store = crate::vpn::load();
    store.saved_proxy = None;
    VpnView { store, presets: crate::vpn::lists::infos(), pings: st.vpn.lock().unwrap().pings.clone() }
}

#[tauri::command]
pub async fn vpn_view(app: AppHandle) -> Res<VpnView> {
    blocking(&app, |st| Ok(vpn_view_now(st))).await
}

fn vpn_busy(app: &AppHandle, busy: Option<&str>, progress: Option<u8>) {
    let st = app.state::<AppState>();
    {
        let mut rt = st.vpn.lock().unwrap();
        rt.busy = busy.map(str::to_string);
        rt.progress = progress;
    }
    st.poke();
}

/// Скачать или обновить ядра VPN — sing-box и Xray (каждое, если его нет или вышла новая версия).
/// Работающий VPN перезапускается.
#[tauri::command]
pub async fn vpn_install(app: AppHandle) -> Res<String> {
    vpn_busy(&app, Some("Получение ссылок на релизы"), None);
    let r = async {
        let sb = crate::vpn::singbox::latest().await?;
        let xr = crate::vpn::xray::latest().await?;
        let need_sb = !crate::vpn::singbox::installed() || crate::vpn::singbox::newer(&sb.version);
        let need_xr = !crate::vpn::xray::installed() || crate::vpn::xray::newer(&xr.version);
        if !need_sb && !need_xr {
            return Ok::<String, String>("Ядра VPN — последних версий".into());
        }
        let running = app.state::<AppState>().vpn.lock().unwrap().child.is_some();
        if running {
            crate::vpn::stop(&app.state::<AppState>())?;
        }
        let mut done = Vec::new();
        if need_sb {
            let a = app.clone();
            crate::vpn::singbox::install(&sb, move |p| vpn_busy(&a, Some("Скачивание sing-box"), Some(p))).await?;
            done.push(format!("sing-box {}", sb.version));
        }
        if need_xr {
            let a = app.clone();
            crate::vpn::xray::install(&xr, move |p| vpn_busy(&a, Some("Скачивание Xray"), Some(p))).await?;
            done.push(format!("Xray {}", xr.version));
        }
        app.state::<AppState>().vpn.lock().unwrap().latest = None;
        let msg = format!("Установлено: {}", done.join(", "));
        journal::ok(format!("VPN: {msg}"));
        if running {
            vpn_busy(&app, Some("Запуск"), None);
            let a = app.clone();
            tauri::async_runtime::spawn_blocking(move || crate::vpn::start(&a.state::<AppState>()))
                .await
                .map_err(|e| e.to_string())??;
        }
        Ok(msg)
    }
    .await;
    vpn_busy(&app, None, None);
    r.inspect_err(|e| journal::err(format!("Ядра VPN: {e}")))
}

/// Подписка (https://…) или ключи (vless://, ss://…).
#[tauri::command]
pub async fn vpn_add(app: AppHandle, input: String) -> Res<VpnView> {
    vpn_busy(&app, Some("Загрузка подписки"), None);
    let r = crate::vpn::add(&input).await;
    vpn_busy(&app, None, None);
    let msg = r.inspect_err(|e| journal::err(format!("VPN: {e}")))?;
    journal::info(format!("VPN: {msg}"));
    Ok(vpn_view_now(&app.state::<AppState>()))
}

#[tauri::command]
pub async fn vpn_update(app: AppHandle, id: Option<String>) -> Res<VpnView> {
    vpn_busy(&app, Some("Обновление подписки"), None);
    let r = crate::vpn::update(id.as_deref()).await;
    vpn_busy(&app, None, None);
    let n = r.inspect_err(|e| journal::err(format!("VPN: подписка не обновилась: {e}")))?;
    journal::ok(format!("VPN: подписки обновлены · серверов: {n}"));
    Ok(vpn_view_now(&app.state::<AppState>()))
}

/// Удалить подписку или сервер из одиночных ссылок.
#[tauri::command]
pub async fn vpn_remove(app: AppHandle, id: String) -> Res<VpnView> {
    blocking(&app, move |st| {
        let mut s = crate::vpn::load();
        s.subscriptions.retain(|x| x.id != id);
        s.manual.retain(|x| x.id != id);
        if s.selected.as_deref() == Some(id.as_str()) {
            s.selected = None;
        }
        crate::vpn::save(&s)?;
        Ok(vpn_view_now(st))
    })
    .await
}

/// Выбрать сервер (`None` — автоматически быстрейший); работающий VPN перезапускается.
#[tauri::command]
pub async fn vpn_select(app: AppHandle, id: Option<String>) -> Res<VpnView> {
    blocking(&app, move |st| {
        let mut s = crate::vpn::load();
        s.selected = id;
        crate::vpn::save(&s)?;
        if st.vpn.lock().unwrap().child.is_some() {
            crate::vpn::start(st)?;
        }
        Ok(vpn_view_now(st))
    })
    .await
}

/// Настройки VPN (routeMode, inbound, presets, zapretDirect, port, autostart).
#[tauri::command]
pub async fn vpn_set(app: AppHandle, patch: Value) -> Res<VpnView> {
    blocking(&app, move |st| {
        let current = crate::vpn::load();
        let mut v = serde_json::to_value(&current).map_err(|e| e.to_string())?;
        if let (Some(obj), Value::Object(p)) = (v.as_object_mut(), patch) {
            for (k, val) in p {
                if matches!(k.as_str(), "routeMode" | "inbound" | "presets" | "zapretDirect" | "port" | "autostart") {
                    obj.insert(k, val);
                }
            }
        }
        let next: crate::vpn::Store = serde_json::from_value(v).map_err(|e| format!("неверные настройки VPN: {e}"))?;
        if next.port < 1024 {
            return Err("порт должен быть от 1024 до 65535".into());
        }
        crate::vpn::save(&next)?;
        let restart = st.vpn.lock().unwrap().child.is_some()
            && (next.route_mode != current.route_mode
                || next.inbound != current.inbound
                || next.presets != current.presets
                || next.zapret_direct != current.zapret_direct
                || next.port != current.port);
        if restart {
            crate::vpn::start(st)?;
        }
        Ok(vpn_view_now(st))
    })
    .await
}

#[tauri::command]
pub async fn vpn_start(app: AppHandle) -> Res<()> {
    // Серверы, сохранённые до появления Xray, не помнят исходных ссылок — обновляем подписки.
    if crate::vpn::load().subscriptions.iter().any(|sub| sub.servers.iter().any(|sv| sv.link.is_none())) {
        vpn_busy(&app, Some("Обновление подписки"), None);
        if let Err(e) = crate::vpn::update(None).await {
            journal::warn(format!("VPN: подписка не обновилась: {e}"));
        }
    }
    vpn_busy(&app, Some("Подготовка списков"), None);
    let store = crate::vpn::load();
    let mut need: Vec<&crate::vpn::lists::Preset> =
        store.presets.iter().filter_map(|id| crate::vpn::lists::preset(id)).collect();
    need.push(&crate::vpn::lists::RU_DIRECT);
    let errors = crate::vpn::lists::refresh(&need, false).await;
    for e in errors {
        journal::warn(format!("VPN: список не обновился ({e}) — используется сохранённый"));
    }
    vpn_busy(&app, Some("Запуск"), None);
    let r = blocking(&app, |st| {
        st.require_admin()?;
        crate::vpn::start(st)
    })
    .await;
    vpn_busy(&app, None, None);
    if let Err(e) = &r {
        app.state::<AppState>().vpn.lock().unwrap().error = Some(e.clone());
        journal::err(format!("VPN не включился: {e}"));
    }
    r
}

#[tauri::command]
pub async fn vpn_stop(app: AppHandle) -> Res<()> {
    blocking(&app, |st| {
        st.vpn.lock().unwrap().error = None;
        crate::vpn::stop(st)
    })
    .await
}

/// Задержка до серверов (TCP); результат сохраняется для списка серверов.
#[tauri::command]
pub async fn vpn_ping(app: AppHandle) -> Res<std::collections::HashMap<String, Option<u32>>> {
    blocking(&app, |st| {
        let pings = crate::vpn::ping(&crate::vpn::load());
        st.vpn.lock().unwrap().pings = pings.clone();
        Ok(pings)
    })
    .await
}
