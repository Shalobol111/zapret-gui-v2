//! Проверка стратегий — алгоритм utils/test zapret.ps1, результаты — в таблицу.
//!
//! Для каждой стратегии: остановить winws, запустить стратегию, дождаться winws,
//! параллельно проверить цели, остановить. До и после — сохранить и вернуть то,
//! что работало (ручной обход, внешний winws, служба), а для DPI-теста — IPSet.

pub mod curl;
pub mod history;
pub mod model;
mod report;
pub mod targets;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use model::{CellState, DpiTarget, Kind, Row, RowState, Run, TargetResult, DPI_COLUMNS, STANDARD_COLUMNS};

use super::{filters, layout::Layout, runner, service};
use crate::journal;
use crate::state::AppState;
use crate::sys::process;

const STD_TIMEOUT: u32 = 4;
/// Быстрый подбор: короче таймаут, протоколы параллельно — нерабочая стратегия отсеивается за ~4 с.
const QUICK_TIMEOUT: u32 = 3;
const QUICK_LABELS: [&str; 2] = ["TLS1.2", "TLS1.3"];
const DPI_TIMEOUT: u32 = 5;
const DPI_RANGE: u32 = 65536;
const PARALLEL: usize = 16;
const SUITE_URL: &str = "https://hyperion-cs.github.io/dpi-checkers/ru/tcp-16-20/suite.v2.json";
const IPSET_TEST_BACKUP: &str = "ipset-all.test-backup.txt";
const IPSET_FLAG: &str = "ipset_switched.flag";

fn last_file() -> PathBuf {
    crate::settings::data_dir().join("tests-last.json")
}

/// Последний прогон с диска (таблица переживает перезапуск программы).
pub fn load_last() -> Option<Run> {
    let text = fs::read_to_string(last_file()).ok()?;
    let mut run: Run = serde_json::from_str(&text).ok()?;
    if run.running {
        run.running = false;
        run.cancelled = true;
        run.phase = None;
    }
    Some(run)
}

fn publish(app: &AppHandle, run: &Run) {
    *app.state::<AppState>().tests.lock().unwrap() = Some(run.clone());
    let _ = app.emit("tests://progress", run);
}

/// IPSet, оставшийся в режиме any после прерванного DPI-теста (наш или скрипта).
pub fn recover_ipset(root: &Path) -> bool {
    let flag = root.join(IPSET_FLAG);
    if !flag.exists() {
        return false;
    }
    let backup = root.join("lists").join(IPSET_TEST_BACKUP);
    if backup.exists() {
        let _ = fs::rename(&backup, root.join("lists").join("ipset-all.txt"));
    }
    let _ = fs::remove_file(flag);
    true
}

pub(crate) struct Restore {
    service_was_running: bool,
    gui_strategy: Option<String>,
    external_args: Option<Vec<String>>,
    ipset_switched: bool,
}

/// Что проверять. Обычная проверка — `Request::full`, быстрый подбор — `Request::quick`,
/// «Мой сайт» — `Request::site`.
#[derive(Debug, Clone)]
pub struct Request {
    pub kind: Kind,
    /// Порядок важен: для быстрого подбора — из истории (см. history.rs).
    pub strategies: Vec<String>,
    pub stop_service: bool,
    /// Только Discord + YouTube, только TLS, остановка на первой рабочей.
    pub quick: bool,
    /// Домен для Kind::Custom.
    pub site: Option<String>,
}

impl Request {
    pub fn full(kind: Kind, strategies: Vec<String>, stop_service: bool) -> Self {
        Request { kind, strategies, stop_service, quick: false, site: None }
    }

    /// Быстрый подбор по всем стратегиям в порядке истории; первой — `current`.
    pub fn quick(root: &Path, current: Option<&str>, stop_service: bool) -> Self {
        let names: Vec<String> = super::strategy::list(root).into_iter().map(|s| s.name).collect();
        let strategies = history::History::load().order(&names, current);
        Request { kind: Kind::Standard, strategies, stop_service, quick: true, site: None }
    }

    /// Подбор под один сайт: та же история, остановка на первой стратегии, где сайт открылся.
    pub fn site(root: &Path, domain: &str, current: Option<&str>, stop_service: bool) -> Self {
        let mut r = Request::quick(root, current, stop_service);
        r.kind = Kind::Custom;
        r.site = Some(domain.to_string());
        r
    }

    fn stop_at_first_ok(&self) -> bool {
        self.quick || self.kind == Kind::Custom
    }
}

/// Проверки, подготовка состояния. Возвращает корень zapret, curl и начальный прогон.
fn begin(app: &AppHandle, req: &Request) -> Result<(PathBuf, PathBuf, Run), String> {
    let st = app.state::<AppState>();
    st.require_admin()?;
    let root = st.require_root()?;
    if req.strategies.is_empty() {
        return Err("Отметьте хотя бы одну стратегию".into());
    }
    if req.kind == Kind::Custom && req.site.as_deref().is_none_or(str::is_empty) {
        return Err("Укажите сайт".into());
    }
    let curl = curl::curl_path().ok_or("Не найден curl.exe (входит в Windows 10 1803 и новее)")?;
    let svc = service::status(Some(&root));
    if svc.state != service::State::NotInstalled && !req.stop_service {
        return Err("service".into()); // интерфейс спросит, остановить ли службу
    }
    let mut tests = st.tests.lock().unwrap();
    if tests.as_ref().is_some_and(|r| r.running) {
        return Err("Проверка уже идёт".into());
    }
    let columns = match req.kind {
        Kind::Standard => STANDARD_COLUMNS,
        Kind::Dpi | Kind::Custom => DPI_COLUMNS,
    };
    let run = Run {
        kind: req.kind,
        columns: columns.iter().map(|s| s.to_string()).collect(),
        rows: req.strategies.iter().map(|s| Row::pending(s, columns.len())).collect(),
        running: true,
        cancelled: false,
        best: None,
        started_at: runner::now_ms(),
        finished_at: None,
        phase: Some("Подготовка…".into()),
        report: None,
        error: None,
        quick: req.quick,
        site: req.site.clone(),
    };
    *tests = Some(run.clone());
    drop(tests);
    st.tests_cancel.store(false, Ordering::SeqCst);
    let _ = app.emit("tests://progress", &run);
    st.poke();
    Ok((root, curl, run))
}

/// Запуск в фоне (кнопки в интерфейсе); результат приходит событиями `tests://progress`.
pub fn start(app: &AppHandle, req: Request) -> Result<(), String> {
    let (root, curl, run) = begin(app, &req)?;
    let app = app.clone();
    thread::Builder::new()
        .name("tests".into())
        .spawn(move || {
            run_all(&app, root, curl, run, &req);
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Синхронный запуск для автопилота и мастера (вызывать из фонового потока).
pub fn run_blocking(app: &AppHandle, req: Request) -> Result<Run, String> {
    let (root, curl, run) = begin(app, &req)?;
    Ok(run_all(app, root, curl, run, &req))
}

/// Цели быстрого подбора: по две на Discord и YouTube из targets.txt (главная страница и
/// gateway/видеосервер — то, без чего сервисы не работают).
pub fn quick_targets(all: &[targets::Target]) -> Vec<targets::Target> {
    let pick = |group: &str, prefer: &[&str]| -> Vec<targets::Target> {
        let mut g: Vec<targets::Target> =
            all.iter().filter(|t| t.url.is_some() && t.group.eq_ignore_ascii_case(group)).cloned().collect();
        g.sort_by_key(|t| prefer.iter().position(|p| t.key.to_lowercase().contains(p)).unwrap_or(usize::MAX));
        g.truncate(2);
        g
    };
    let mut out = pick("Discord", &["gateway", "main"]);
    out.extend(pick("YouTube", &["redirect", "web"]));
    out
}

pub fn site_target(domain: &str) -> targets::Target {
    targets::Target {
        key: domain.to_string(),
        group: "Сайт".into(),
        url: Some(format!("https://{domain}")),
        ping: domain.to_string(),
    }
}
pub fn cancel(app: &AppHandle) {
    app.state::<AppState>().tests_cancel.store(true, Ordering::SeqCst);
}

fn cancelled(app: &AppHandle) -> bool {
    app.state::<AppState>().tests_cancel.load(Ordering::SeqCst)
}

pub(crate) fn prepare(app: &AppHandle, root: &Path, kind: Kind) -> Restore {
    let st = app.state::<AppState>();
    if recover_ipset(root) {
        journal::warn("IPSet восстановлен после прерванной проверки");
    }
    let mut r = Restore { service_was_running: false, gui_strategy: None, external_args: None, ipset_switched: false };

    if service::status(None).state == service::State::Running {
        match service::stop() {
            Ok(()) => {
                r.service_was_running = true;
                journal::info("Служба zapret остановлена на время проверки");
            }
            Err(e) => journal::warn(format!("Не удалось остановить службу: {e}")),
        }
    }
    let gui = {
        let runner = st.runner.lock().unwrap();
        runner.is_active().then(|| runner.strategy.clone()).flatten()
    };
    if let Some(name) = gui {
        let _ = runner::stop(&st.runner);
        r.gui_strategy = Some(name);
    } else if let Some(p) = process::find("winws.exe").first() {
        r.external_args = Some(p.args.clone());
    }

    if kind == Kind::Dpi && filters::ipset_mode(root) != filters::IpsetMode::Any {
        let lists = root.join("lists");
        let list = lists.join("ipset-all.txt");
        let ok = (|| -> std::io::Result<()> {
            if list.exists() {
                fs::copy(&list, lists.join(IPSET_TEST_BACKUP))?;
            } else {
                fs::write(lists.join(IPSET_TEST_BACKUP), "")?;
            }
            fs::write(root.join(IPSET_FLAG), "")?;
            fs::write(&list, "")
        })();
        match ok {
            Ok(()) => {
                r.ipset_switched = true;
                journal::info("IPSet временно переключён в «Любой» для точного DPI-теста");
            }
            Err(e) => journal::warn(format!("Не удалось переключить IPSet: {e}")),
        }
    }
    r
}

pub(crate) fn restore(app: &AppHandle, root: &Path, r: Restore) {
    process::kill_all("winws.exe");
    if r.ipset_switched && recover_ipset(root) {
        journal::info("IPSet восстановлен");
    }
    let st = app.state::<AppState>();
    if r.service_was_running {
        match service::start() {
            Ok(()) => journal::info("Служба zapret снова запущена"),
            Err(e) => journal::err(format!("Не удалось запустить службу после проверки: {e}")),
        }
    } else if let Some(name) = r.gui_strategy {
        let _ = runner::start(&st.runner, root, &name);
    } else if let Some(args) = r.external_args {
        let layout = Layout::new(root);
        let started =
            crate::sys::cmd::hidden(&layout.winws().to_string_lossy()).args(&args).current_dir(layout.bin()).spawn();
        match started {
            Ok(_) => journal::info("Ранее запущенный winws.exe восстановлен"),
            Err(e) => journal::warn(format!("Не удалось восстановить winws.exe: {e}")),
        }
    }
}

/// Запуск стратегии и ожидание winws. `Err` — стратегия не стартовала.
fn launch(root: &Path, name: &str) -> Result<Option<std::process::Child>, String> {
    process::kill_all("winws.exe");
    thread::sleep(Duration::from_millis(300));
    let mut child = runner::spawn_quiet(root, name)?;
    match &mut child {
        Some(c) => {
            thread::sleep(Duration::from_millis(800));
            if !matches!(c.try_wait(), Ok(None)) {
                return Err("winws.exe завершился сразу после запуска".into());
            }
        }
        None => {
            let t0 = Instant::now();
            while !process::exists("winws.exe") {
                if t0.elapsed() > Duration::from_secs(5) {
                    return Err("winws.exe не появился за 5 с".into());
                }
                thread::sleep(Duration::from_millis(200));
            }
            thread::sleep(Duration::from_millis(300));
        }
    }
    Ok(child)
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Как test zapret.ps1: три протокола по очереди + пинг.
    Full,
    /// Быстрый подбор: TLS 1.2/1.3 параллельно, короткий таймаут, без пинга.
    Quick,
    /// Один сайт: три протокола параллельно, без пинга.
    Site,
}

fn standard_checks(app: &AppHandle, curl: &Path, targets: &[targets::Target], mode: Mode) -> Option<Vec<TargetResult>> {
    let mut out = Vec::with_capacity(targets.len());
    for chunk in targets.chunks(PARALLEL) {
        if cancelled(app) {
            return None;
        }
        let part: Vec<TargetResult> = thread::scope(|s| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|t| {
                    s.spawn(move || {
                        let tokens = match (t.url.as_deref(), mode) {
                            (None, _) => vec![],
                            (Some(u), Mode::Full) => curl::standard(curl, u, STD_TIMEOUT),
                            (Some(u), Mode::Quick) => curl::check_parallel(curl, u, QUICK_TIMEOUT, &QUICK_LABELS),
                            (Some(u), Mode::Site) => curl::check_parallel(curl, u, STD_TIMEOUT, &curl::LABELS),
                        };
                        let pinged = mode == Mode::Full || t.url.is_none();
                        TargetResult {
                            target: t.clone(),
                            tokens,
                            ping: if pinged { crate::sys::ping::ping(&t.ping, 1000) } else { None },
                            pinged,
                        }
                    })
                })
                .collect();
            handles.into_iter().filter_map(|h| h.join().ok()).collect()
        });
        out.extend(part);
    }
    Some(out)
}
fn dpi_checks(
    app: &AppHandle,
    curl: &Path,
    suite: &[DpiTarget],
    payload: &Path,
) -> Option<Vec<(DpiTarget, Vec<curl::DpiLine>)>> {
    let mut out = Vec::with_capacity(suite.len());
    for chunk in suite.chunks(PARALLEL) {
        if cancelled(app) {
            return None;
        }
        let part: Vec<_> = thread::scope(|s| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|t| s.spawn(move || (t.clone(), curl::dpi(curl, &t.host, payload, DPI_TIMEOUT, DPI_RANGE))))
                .collect();
            handles.into_iter().filter_map(|h| h.join().ok()).collect()
        });
        out.extend(part);
    }
    Some(out)
}

fn fetch_suite() -> Result<Vec<DpiTarget>, String> {
    tauri::async_runtime::block_on(async {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(DPI_TIMEOUT as u64 * 2))
            .user_agent(concat!("ZapretGUI/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| e.to_string())?;
        let resp = client.get(SUITE_URL).send().await.map_err(crate::net::friendly)?;
        if !resp.status().is_success() {
            return Err(format!("сервер ответил {}", resp.status()));
        }
        let text = resp.text().await.map_err(crate::net::friendly)?;
        serde_json::from_str::<Vec<DpiTarget>>(&text).map_err(|e| format!("неверный формат набора: {e}"))
    })
}

/// Случайные данные для POST (как RandomNumberGenerator в скрипте; криптостойкость не нужна).
fn write_payload() -> std::io::Result<PathBuf> {
    let mut x = runner::now_ms() ^ 0x9E37_79B9_7F4A_7C15;
    let mut buf = Vec::with_capacity(DPI_RANGE as usize);
    while buf.len() < DPI_RANGE as usize {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        buf.extend_from_slice(&x.to_le_bytes());
    }
    buf.truncate(DPI_RANGE as usize);
    let path = std::env::temp_dir().join(format!("zgui-dpi-{}.bin", std::process::id()));
    fs::write(&path, buf)?;
    Ok(path)
}

fn run_all(app: &AppHandle, root: PathBuf, curl: PathBuf, mut run: Run, req: &Request) -> Run {
    let kind = run.kind;
    let what = match (kind, req.quick) {
        (Kind::Custom, _) => format!("подбор для {}", req.site.as_deref().unwrap_or("")),
        (_, true) => "быстрый подбор (Discord, YouTube)".to_string(),
        (Kind::Standard, false) => "сайты".to_string(),
        (Kind::Dpi, _) => "DPI TCP 16–20".to_string(),
    };
    journal::info(format!(
        "Проверка: {} {} · {what}",
        run.rows.len(),
        crate::journal::plural(run.rows.len(), ["стратегия", "стратегии", "стратегий"]),
    ));
    let restore_info = prepare(app, &root, kind);

    let (targets, mode) = match (kind, req.quick) {
        (Kind::Custom, _) => (vec![site_target(req.site.as_deref().unwrap_or_default())], Mode::Site),
        (_, true) => (quick_targets(&targets::load(&root)), Mode::Quick),
        _ => (targets::load(&root), Mode::Full),
    };
    if targets.is_empty() && kind != Kind::Dpi {
        run.error = Some("Нет целей для проверки — проверьте utils\\targets.txt".into());
    }
    let mut suite = Vec::new();
    let mut payload = None;
    if kind == Kind::Dpi {
        run.phase = Some("Загрузка набора DPI-целей…".into());
        publish(app, &run);
        match fetch_suite().and_then(|s| write_payload().map(|p| (s, p)).map_err(|e| e.to_string())) {
            Ok((s, p)) if !s.is_empty() => {
                suite = s;
                payload = Some(p);
            }
            Ok(_) => run.error = Some("Набор DPI-целей пуст".into()),
            Err(e) => run.error = Some(format!("Не удалось загрузить набор DPI-целей: {e}")),
        }
    }

    let mut history = history::History::load();
    let mut found: Option<String> = None;
    let total = run.rows.len();
    for i in 0..total {
        if run.error.is_some() || cancelled(app) {
            break;
        }
        let name = run.rows[i].name.clone();
        run.rows[i].state = RowState::Running;
        run.phase = Some(format!("{}/{total} · {name}", i + 1));
        publish(app, &run);

        let child = match launch(&root, &name) {
            Ok(c) => c,
            Err(e) => {
                run.rows[i].state = RowState::Failed;
                run.rows[i].error = Some(e);
                publish(app, &run);
                continue;
            }
        };
        let mut row_ok = false;
        let row = match kind {
            Kind::Standard | Kind::Custom => standard_checks(app, &curl, &targets, mode).map(|r| {
                row_ok = model::all_ok(&r);
                if kind == Kind::Custom {
                    model::custom_row(&name, &r)
                } else {
                    model::standard_row(&name, &r)
                }
            }),
            Kind::Dpi => dpi_checks(app, &curl, &suite, payload.as_deref().unwrap()).map(|r| model::dpi_row(&name, &r)),
        };
        if let Some(mut c) = child {
            let _ = c.kill();
            let _ = c.wait();
        }
        process::kill_all("winws.exe");
        match row {
            Some(r) => {
                // История: «работает» — открылись и Discord, и YouTube.
                if kind == Kind::Standard {
                    let ok = r.cells.len() >= 2 && r.cells[..2].iter().all(|c| c.state == CellState::Ok);
                    history.record(&name, ok, runner::now_ms());
                }
                run.rows[i] = r;
            }
            None => run.rows[i].state = RowState::Skipped,
        }
        publish(app, &run);
        if req.stop_at_first_ok() && row_ok {
            found = Some(name);
            break;
        }
    }
    history.save();

    run.cancelled = cancelled(app);
    for r in run.rows.iter_mut().filter(|r| matches!(r.state, RowState::Pending | RowState::Running)) {
        r.state = RowState::Skipped;
    }
    run.phase = Some("Восстановление…".into());
    publish(app, &run);
    restore(app, &root, restore_info);
    if let Some(p) = payload {
        let _ = fs::remove_file(p);
    }

    run.best = if req.stop_at_first_ok() { found } else { model::best(&run.rows) };
    run.running = false;
    run.phase = None;
    run.finished_at = Some(runner::now_ms());
    // Отчёт и сохранённая таблица — только для полной проверки: быстрый подбор не должен
    // затирать результаты последнего полного прогона.
    let full = !req.quick && kind != Kind::Custom;
    if full && run.rows.iter().any(|r| r.state == RowState::Done) {
        match report::write(&root, &run) {
            Ok(p) => run.report = Some(p.display().to_string()),
            Err(e) => journal::warn(format!("Не удалось сохранить отчёт: {e}")),
        }
    }
    if full {
        if let Ok(text) = serde_json::to_string(&run) {
            let _ = fs::write(last_file(), text);
        }
    }
    let summary = match (&run.error, run.cancelled, &run.best) {
        (Some(e), _, _) => {
            journal::err(format!("Проверка не выполнена: {e}"));
            format!("Не выполнена: {e}")
        }
        (None, true, _) => {
            journal::warn("Проверка остановлена");
            "Остановлена".into()
        }
        (None, false, Some(b)) => {
            journal::ok(format!("Проверка завершена · лучшая стратегия: «{b}»"));
            format!("Лучшая стратегия: {b}")
        }
        (None, false, None) => {
            journal::warn("Проверка завершена · ни одна стратегия не прошла проверку");
            "Ни одна стратегия не прошла проверку".into()
        }
    };
    publish(app, &run);
    app.state::<AppState>().poke();
    // Окно в трее — сообщить уведомлением, иначе результат никто не увидит.
    let hidden = app.get_webview_window("main").is_some_and(|w| !w.is_visible().unwrap_or(true));
    if hidden && !run.cancelled {
        crate::notify::show(app, "Проверка стратегий завершена", &summary, crate::notify::Screen::Check);
    }
    run
}

#[cfg(test)]
mod quick_tests {
    use super::*;

    #[test]
    fn quick_targets_are_two_discord_and_two_youtube() {
        let all = targets::parse(include_str!("../../../tests/fixtures/flowseal-1.10.3/targets.txt"));
        let keys: Vec<String> = quick_targets(&all).into_iter().map(|t| t.key).collect();
        assert_eq!(keys, vec!["DiscordGateway", "DiscordMain", "YouTubeVideoRedirect", "YouTubeWeb"]);
    }

    #[test]
    fn site_target_is_https() {
        let t = site_target("rutracker.org");
        assert_eq!(t.url.as_deref(), Some("https://rutracker.org"));
        assert_eq!(t.ping, "rutracker.org");
    }
}
