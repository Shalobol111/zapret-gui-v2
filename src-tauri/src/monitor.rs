//! Фоновый опрос: обход, служба, фильтры. Событие `status://changed` уходит
//! только при изменении; таймер «работает 00:42:17» считает интерфейс сам.

use std::sync::mpsc;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;
use crate::sys::process;
use crate::zapret::layout::ZapretInfo;
use crate::zapret::runner::Phase;
use crate::zapret::{filters, match_strategy, service};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BypassState {
    Off,
    Starting,
    On,
    Stopping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Gui,
    Service,
    External,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bypass {
    pub state: BypassState,
    pub source: Option<Source>,
    pub strategy: Option<String>,
    pub pid: Option<u32>,
    pub started_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Filters {
    pub ipset: filters::IpsetMode,
    pub game: filters::GameMode,
    pub tcp_ports: String,
    pub udp_ports: String,
    pub backup_exists: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub zapret: ZapretInfo,
    pub bypass: Bypass,
    pub service: service::Status,
    pub filters: Option<Filters>,
    pub is_admin: bool,
    /// Идёт проверка стратегий — Главная блокирует управление обходом.
    pub tests_running: bool,
    pub update: crate::zapret::updater::Update,
    pub autopilot: crate::autopilot::Info,
    pub telegram: crate::tgproxy::Status,
    pub app_update: crate::selfupdate::Info,
    pub vpn: crate::vpn::Status,
}

pub fn compute(state: &AppState) -> Status {
    let zapret = state.zapret_info();
    let root = zapret.valid.then(|| std::path::PathBuf::from(zapret.path.clone().unwrap()));

    let filters = root.as_deref().map(|r| {
        let g = filters::game(r);
        Filters {
            ipset: filters::ipset_mode(r),
            game: g.mode,
            tcp_ports: g.tcp,
            udp_ports: g.udp,
            backup_exists: filters::ipset_backup_exists(r),
        }
    });

    let svc = service::status(root.as_deref());
    let procs = process::find("winws.exe");
    let mut runner = state.runner.lock().unwrap();
    runner.reap();

    let off = Bypass { state: BypassState::Off, source: None, strategy: None, pid: None, started_at: None };
    let bypass = match runner.phase {
        Phase::Starting => Bypass {
            state: BypassState::Starting,
            source: Some(Source::Gui),
            strategy: runner.strategy.clone(),
            ..off.clone()
        },
        Phase::Stopping => Bypass {
            state: BypassState::Stopping,
            source: Some(Source::Gui),
            strategy: runner.strategy.clone(),
            ..off.clone()
        },
        Phase::Running => Bypass {
            state: BypassState::On,
            source: Some(Source::Gui),
            strategy: runner.strategy.clone(),
            pid: runner.pid,
            started_at: runner.started_at,
        },
        Phase::Idle => {
            let first = procs.first();
            match (svc.state, first) {
                (service::State::Pending, _) => Bypass {
                    state: if svc.stop_pending { BypassState::Stopping } else { BypassState::Starting },
                    source: Some(Source::Service),
                    strategy: svc.strategy.clone(),
                    ..off.clone()
                },
                (service::State::Running, p) => Bypass {
                    state: BypassState::On,
                    source: Some(Source::Service),
                    strategy: svc.strategy.clone(),
                    pid: p.map(|p| p.pid),
                    started_at: p.map(|p| p.start_time * 1000),
                },
                (_, Some(p)) => Bypass {
                    state: BypassState::On,
                    source: Some(Source::External),
                    strategy: root.as_deref().and_then(|r| match_strategy(r, &p.args)),
                    pid: Some(p.pid),
                    started_at: Some(p.start_time * 1000),
                },
                (_, None) => off,
            }
        }
    };
    drop(runner);

    let tests_running = state.tests.lock().unwrap().as_ref().is_some_and(|r| r.running);
    let update = state.update.lock().unwrap().clone();
    let autopilot = state.autopilot.lock().unwrap().clone();
    let tg_shared = state.tg.lock().unwrap().clone();
    let telegram = crate::tgproxy::status(&tg_shared);
    let app_update = state.self_update.lock().unwrap().clone();
    let vpn = crate::vpn::status(&mut state.vpn.lock().unwrap(), &crate::vpn::load());
    Status {
        zapret,
        bypass,
        service: svc,
        filters,
        is_admin: state.is_admin,
        tests_running,
        update,
        autopilot,
        telegram,
        app_update,
        vpn,
    }
}

/// Обход был включён и выключился сам: winws упал, служба остановилась, внешний winws закрыли.
/// Собственные действия программы (последние 15 с), проверки и обновление — не в счёт.
fn notify_unexpected_stop(app: &AppHandle, prev: &Status, now: &Status) {
    use crate::zapret::updater::State as Upd;
    if prev.bypass.state != BypassState::On || now.bypass.state != BypassState::Off {
        return;
    }
    let st = app.state::<AppState>();
    let busy = prev.tests_running
        || now.tests_running
        || prev.update.state == Upd::Applying
        || now.update.state == Upd::Applying;
    if busy || st.acted_recently() {
        return;
    }
    let what = match (prev.bypass.source, prev.bypass.strategy.as_deref()) {
        (Some(Source::Service), Some(s)) => format!("Служба zapret («{s}») остановилась."),
        (Some(Source::Service), None) => "Служба zapret остановилась.".to_string(),
        (_, Some(s)) => format!("Стратегия «{s}» перестала работать: winws.exe завершился."),
        (_, None) => "winws.exe завершился.".to_string(),
    };
    crate::journal::warn(format!("Обход неожиданно остановился. {what}"));
    crate::notify::show(
        app,
        "Обход остановился",
        &format!("{what} Откройте ZapretGUI, чтобы включить снова."),
        crate::notify::Screen::Main,
    );
}

pub fn start(app: AppHandle) {
    let (tx, rx) = mpsc::channel::<()>();
    *app.state::<AppState>().poke.lock().unwrap() = Some(tx);
    std::thread::Builder::new()
        .name("monitor".into())
        .spawn(move || {
            let mut last: Option<Status> = None;
            loop {
                let status = compute(&app.state::<AppState>());
                if let Some(prev) = &last {
                    notify_unexpected_stop(&app, prev, &status);
                }
                if last.as_ref() != Some(&status) {
                    let _ = app.emit("status://changed", &status);
                    crate::tray::update(&app, &status);
                    last = Some(status);
                }
                match rx.recv_timeout(Duration::from_millis(1500)) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => while rx.try_recv().is_ok() {},
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        })
        .expect("не удалось запустить поток monitor");
}
