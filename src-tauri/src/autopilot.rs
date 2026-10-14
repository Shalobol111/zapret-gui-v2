//! Автопилот: раз в 15 минут проверяет, открываются ли Discord и YouTube с работающей
//! стратегией (winws не перезапускается). Сбой подтверждается повторной проверкой через
//! минуту; дальше — по режиму из Настроек: предупредить уведомлением или самому подобрать
//! рабочую стратегию быстрым подбором и включить её (не чаще раза в час).
//!
//! `ZAPRETGUI_AUTOPILOT_SIMULATE=1` — имитация для проверки: интервалы короче, а стратегия,
//! работавшая при первой проверке, считается «сломавшейся» (YouTube не открывается) и
//! исключается из подбора.

use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Manager};

use crate::journal;
use crate::monitor::{self, BypassState, Source};
use crate::notify::{self, Screen};
use crate::state::AppState;
use crate::zapret::tests::{self as zt, curl};
use crate::zapret::{runner, service};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Сам подбирает и включает рабочую стратегию.
    Auto,
    /// Уведомление с кнопкой «Подобрать».
    #[default]
    Warn,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    #[default]
    Idle,
    Checking,
    /// Сбой найден, через минуту — повторная проверка.
    Confirming,
    Picking,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Ok,
    Fail,
    Offline,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Switch {
    pub from: String,
    pub to: String,
    pub at: u64,
}

/// Для подписи на Главной (часть Status).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub phase: Phase,
    pub last_check: Option<u64>,
    pub outcome: Option<Outcome>,
    /// «YouTube не открывается» и т. п.
    pub message: Option<String>,
    pub switched: Option<Switch>,
}

struct Timing {
    interval: Duration,
    confirm: Duration,
    /// Сколько обход должен проработать до первой проверки.
    grace: Duration,
    pick_cooldown: Duration,
}

fn simulate() -> bool {
    // `set X=1 && …` в cmd оставляет пробел в значении.
    std::env::var("ZAPRETGUI_AUTOPILOT_SIMULATE").is_ok_and(|v| v.trim() == "1")
}

fn timing() -> Timing {
    if simulate() {
        Timing {
            interval: Duration::from_secs(120),
            confirm: Duration::from_secs(10),
            grace: Duration::from_secs(15),
            pick_cooldown: Duration::ZERO,
        }
    } else {
        Timing {
            interval: Duration::from_secs(15 * 60),
            confirm: Duration::from_secs(60),
            grace: Duration::from_secs(60),
            pick_cooldown: Duration::from_secs(60 * 60),
        }
    }
}

/// Стратегия, «сломанная» в режиме имитации.
static SIMULATED_BROKEN: Mutex<Option<String>> = Mutex::new(None);
static WAKE: Mutex<Option<Sender<()>>> = Mutex::new(None);

/// «Проверить сейчас» из Настроек.
pub fn check_now() {
    if let Some(tx) = WAKE.lock().unwrap().as_ref() {
        let _ = tx.send(());
    }
}

// ── Проверка здоровья ──────────────────────────────────────────

const DISCORD: [&str; 2] = ["discord.com", "gateway.discord.gg"];
const YOUTUBE: [&str; 2] = ["www.youtube.com", "redirector.googlevideo.com"];
const CONNECT_URLS: [&str; 4] = [
    "http://www.msftconnecttest.com/connecttest.txt",
    "http://cp.cloudflare.com/generate_204",
    "http://connectivitycheck.gstatic.com/generate_204",
    "https://ya.ru",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Ok,
    Offline,
    Fail { discord: bool, youtube: bool },
}

impl Health {
    fn what(self) -> String {
        match self {
            Health::Fail { discord: false, youtube: false } => "Discord и YouTube не открываются".into(),
            Health::Fail { discord: false, .. } => "Discord не открывается".into(),
            Health::Fail { .. } => "YouTube не открывается".into(),
            _ => String::new(),
        }
    }
    fn services(self) -> &'static str {
        match self {
            Health::Fail { discord: false, youtube: false } => "Discord и YouTube",
            Health::Fail { discord: false, .. } => "Discord",
            _ => "YouTube",
        }
    }
}

/// Есть ли интернет: без него сбой стратегии не ищем. Несколько адресов параллельно —
/// любой HTTP-ответ годится. Одного адреса мало: провайдер может не пропускать, например,
/// проверку подключения Windows (msftconnecttest), хотя интернет работает.
fn online(curl_exe: &Path) -> bool {
    std::thread::scope(|s| {
        let handles: Vec<_> = CONNECT_URLS
            .iter()
            .map(|url| {
                s.spawn(move || {
                    let out = curl::command(curl_exe)
                        .args(["-s", "-o", "NUL", "-m", "6", "--connect-timeout", "4", "-w", "%{http_code}", url])
                        .output();
                    out.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u16>().is_ok_and(|c| c > 0))
                })
            })
            .collect();
        handles.into_iter().any(|h| h.join().unwrap_or(false))
    })
}

fn opens(curl_exe: &Path, host: &str) -> bool {
    curl::check_parallel(curl_exe, &format!("https://{host}"), 5, &["TLS1.2", "TLS1.3"])
        .iter()
        .any(|(_, t)| *t == curl::Tok::Ok)
}

/// Хотя бы одна цель Discord и одна цель YouTube должны открываться.
pub fn health(curl_exe: &Path, strategy: Option<&str>) -> Health {
    if !online(curl_exe) {
        return Health::Offline;
    }
    let (d, y) = std::thread::scope(|s| {
        let d: Vec<_> = DISCORD.iter().map(|h| s.spawn(move || opens(curl_exe, h))).collect();
        let y: Vec<_> = YOUTUBE.iter().map(|h| s.spawn(move || opens(curl_exe, h))).collect();
        let any = |v: Vec<std::thread::ScopedJoinHandle<bool>>| v.into_iter().any(|h| h.join().unwrap_or(false));
        (any(d), any(y))
    });
    let mut youtube = y;
    if simulate() {
        let mut broken = SIMULATED_BROKEN.lock().unwrap();
        if broken.is_none() {
            *broken = strategy.map(str::to_string);
            journal::warn(format!("Автопилот (имитация): «{}» считается сломанной", strategy.unwrap_or("?")));
        }
        if strategy.is_some() && broken.as_deref() == strategy {
            youtube = false;
        }
    }
    if d && youtube {
        Health::Ok
    } else {
        Health::Fail { discord: d, youtube }
    }
}

// ── Поток ──────────────────────────────────────────────────────

fn publish(app: &AppHandle, f: impl FnOnce(&mut Info)) {
    let st = app.state::<AppState>();
    f(&mut st.autopilot.lock().unwrap());
    st.poke();
}

fn mode(st: &AppState) -> Mode {
    st.settings.lock().unwrap().get().autopilot
}

/// Ожидание; `true` — разбудили кнопкой «Проверить сейчас».
fn wait(rx: &Receiver<()>, d: Duration) -> bool {
    match rx.recv_timeout(d) {
        Ok(()) => {
            while rx.try_recv().is_ok() {}
            true
        }
        Err(RecvTimeoutError::Timeout) => false,
        Err(RecvTimeoutError::Disconnected) => {
            std::thread::sleep(d);
            false
        }
    }
}

/// Что сейчас работает и можно ли проверять. `None` — проверять нечего (обход выключен,
/// идёт проверка стратегий или обновление).
fn running(st: &AppState) -> Option<(Option<String>, Option<Source>, Option<u64>)> {
    let s = monitor::compute(st);
    let busy = s.tests_running || s.update.state == crate::zapret::updater::State::Applying;
    (s.bypass.state == BypassState::On && !busy).then_some((s.bypass.strategy, s.bypass.source, s.bypass.started_at))
}

struct Loop {
    app: AppHandle,
    t: Timing,
    last_check: Option<Instant>,
    last_pick: Option<Instant>,
    /// Для какой стратегии уже показано предупреждение — не повторять каждые 15 минут.
    warned: Option<String>,
}

pub fn start(app: AppHandle) {
    let (tx, rx) = mpsc::channel::<()>();
    *WAKE.lock().unwrap() = Some(tx);
    if simulate() {
        journal::warn("Автопилот в режиме имитации (ZAPRETGUI_AUTOPILOT_SIMULATE=1)");
    }
    let mut lp = Loop { app, t: timing(), last_check: None, last_pick: None, warned: None };
    std::thread::Builder::new()
        .name("autopilot".into())
        .spawn(move || loop {
            let forced = wait(&rx, Duration::from_secs(if simulate() { 5 } else { 30 }));
            lp.tick(&rx, forced);
        })
        .expect("не удалось запустить поток autopilot");
}

impl Loop {
    fn tick(&mut self, rx: &Receiver<()>, forced: bool) {
        let app = self.app.clone();
        let st = app.state::<AppState>();
        let mode = mode(&st);
        if mode == Mode::Off && !forced {
            return;
        }
        let Some((strategy, _, started_at)) = running(&st) else {
            if forced {
                journal::info("Автопилот: обход выключен — проверять нечего");
            }
            return;
        };
        if !forced {
            let up = started_at.map(|t| runner::now_ms().saturating_sub(t)).unwrap_or(u64::MAX);
            let due = self.last_check.is_none_or(|t| t.elapsed() >= self.t.interval);
            if up < self.t.grace.as_millis() as u64 || !due || st.acted_recently() {
                return;
            }
        }
        let Some(curl_exe) = curl::curl_path() else { return };
        self.last_check = Some(Instant::now());
        publish(&app, |i| i.phase = Phase::Checking);
        let mut h = health(&curl_exe, strategy.as_deref());

        if let Health::Fail { .. } = h {
            publish(&app, |i| {
                i.phase = Phase::Confirming;
                i.message = Some(h.what());
            });
            journal::warn(format!("Автопилот: {} — перепроверка через {} с", h.what(), self.t.confirm.as_secs()));
            wait(rx, self.t.confirm);
            // За минуту могли выключить обход или сменить стратегию — тогда этот сбой не в счёт.
            match running(&st) {
                Some((s, _, _)) if s == strategy => h = health(&curl_exe, strategy.as_deref()),
                _ => {
                    publish(&app, |i| *i = Info { switched: i.switched.clone(), ..Info::default() });
                    return;
                }
            }
        }

        let now = runner::now_ms();
        match h {
            Health::Ok => {
                if self.warned.take().is_some() || forced {
                    journal::ok("Автопилот: Discord и YouTube открываются");
                }
                publish(&app, |i| {
                    *i = Info {
                        last_check: Some(now),
                        outcome: Some(Outcome::Ok),
                        switched: i.switched.clone(),
                        ..Info::default()
                    }
                });
            }
            Health::Offline => {
                journal::info("Автопилот: нет подключения к интернету — проверка отложена");
                publish(&app, |i| {
                    *i = Info {
                        last_check: Some(now),
                        outcome: Some(Outcome::Offline),
                        switched: i.switched.clone(),
                        ..Info::default()
                    }
                });
            }
            Health::Fail { .. } => self.on_failure(h, strategy, mode, now),
        }
    }

    fn on_failure(&mut self, h: Health, strategy: Option<String>, mode: Mode, now: u64) {
        let app = self.app.clone();
        let st = app.state::<AppState>();
        let name = strategy.clone().unwrap_or_else(|| "текущая".into());
        journal::warn(format!("Автопилот: стратегия «{name}» перестала открывать {}", h.services()));
        publish(&app, |i| {
            i.phase = Phase::Idle;
            i.last_check = Some(now);
            i.outcome = Some(Outcome::Fail);
            i.message = Some(h.what());
        });

        let cooled = self.last_pick.is_none_or(|t| t.elapsed() >= self.t.pick_cooldown);
        let can_pick = mode == Mode::Auto && st.is_admin && st.root().is_some() && cooled;
        if !can_pick {
            if mode == Mode::Auto && !cooled {
                journal::info("Автопилот: подбор уже был меньше часа назад — только предупреждение");
            }
            if self.warned.as_deref() != Some(name.as_str()) {
                self.warned = Some(name.clone());
                notify::show_pick(
                    &app,
                    &format!("Стратегия «{name}» перестала открывать {}", h.services()),
                    "Быстрый подбор найдёт рабочую стратегию за полминуты.",
                );
            }
            return;
        }
        self.last_pick = Some(Instant::now());
        self.pick(h, strategy);
    }

    fn pick(&mut self, h: Health, strategy: Option<String>) {
        let app = self.app.clone();
        let st = app.state::<AppState>();
        let Ok(root) = st.require_root() else { return };
        let name = strategy.clone().unwrap_or_else(|| "текущая".into());
        publish(&app, |i| i.phase = Phase::Picking);
        journal::info("Автопилот: быстрый подбор стратегии");

        let mut req = zt::Request::quick(&root, strategy.as_deref(), true);
        if simulate() {
            let broken = SIMULATED_BROKEN.lock().unwrap().clone();
            req.strategies.retain(|s| Some(s) != broken.as_ref());
        }
        let result = zt::run_blocking(&app, req);
        let now = runner::now_ms();
        let done = |msg: Option<String>, outcome: Outcome, switched: Option<Switch>| {
            publish(&app, |i| {
                i.phase = Phase::Idle;
                i.last_check = Some(now);
                i.outcome = Some(outcome);
                i.message = msg;
                if switched.is_some() {
                    i.switched = switched;
                }
            });
        };

        let run = match result {
            Ok(r) => r,
            Err(e) => {
                journal::err(format!("Автопилот: подбор не запустился: {e}"));
                done(Some(h.what()), Outcome::Fail, None);
                return;
            }
        };
        if run.cancelled {
            journal::warn("Автопилот: подбор остановлен");
            done(Some(h.what()), Outcome::Fail, None);
            return;
        }
        match run.best {
            Some(best) if Some(&best) == strategy.as_ref() => {
                journal::ok(format!("Автопилот: «{best}» снова работает"));
                done(None, Outcome::Ok, None);
            }
            Some(best) => match apply(&st, &root, &best) {
                Ok(()) => {
                    journal::ok(format!("Автопилот: «{name}» заменена на «{best}»"));
                    notify::show(
                        &app,
                        "Автопилот сменил стратегию",
                        &format!("«{name}» перестала открывать {} — включена «{best}».", h.services()),
                        Screen::Main,
                    );
                    self.warned = None;
                    done(None, Outcome::Ok, Some(Switch { from: name, to: best, at: now }));
                }
                Err(e) => {
                    journal::err(format!("Автопилот: не удалось включить «{best}»: {e}"));
                    notify::show(&app, "Автопилот не смог сменить стратегию", &e, Screen::Main);
                    done(Some(h.what()), Outcome::Fail, None);
                }
            },
            None => {
                journal::warn("Автопилот: рабочая стратегия не найдена");
                notify::show(
                    &app,
                    "Рабочая стратегия не найдена",
                    &format!(
                        "«{name}» не открывает {}, быстрый подбор не помог — запустите полную проверку.",
                        h.services()
                    ),
                    Screen::Check,
                );
                done(Some(h.what()), Outcome::Fail, None);
            }
        }
    }
}

/// Включить стратегию тем же способом, каким работал обход: служба — переустановить,
/// иначе — ручной запуск (внешний winws тоже заменяется ручным запуском).
fn apply(st: &AppState, root: &Path, name: &str) -> Result<(), String> {
    st.touch();
    let r = if service::status(Some(root)).state != service::State::NotInstalled {
        service::install(root, name)
    } else {
        if st.runner.lock().unwrap().is_active() {
            runner::stop(&st.runner)?;
        }
        runner::start(&st.runner, root, name)
    };
    if r.is_ok() {
        let _ = st.settings.lock().unwrap().patch(&json!({ "lastStrategy": name }));
    }
    st.touch();
    st.poke();
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_texts() {
        assert_eq!(Health::Fail { discord: true, youtube: false }.what(), "YouTube не открывается");
        assert_eq!(Health::Fail { discord: false, youtube: true }.services(), "Discord");
        assert_eq!(Health::Fail { discord: false, youtube: false }.services(), "Discord и YouTube");
    }

    #[test]
    fn mode_serde() {
        assert_eq!(serde_json::to_string(&Mode::Auto).unwrap(), "\"auto\"");
        assert_eq!(serde_json::from_str::<Mode>("\"off\"").unwrap(), Mode::Off);
        assert_eq!(Mode::default(), Mode::Warn);
    }

    /// Живая проверка здоровья через текущую сеть: `cargo test -- --ignored live_health --nocapture`
    #[test]
    #[ignore]
    fn live_health() {
        let c = curl::curl_path().expect("curl.exe");
        println!("online: {}", online(&c));
        println!("{:?}", health(&c, None));
    }
}
