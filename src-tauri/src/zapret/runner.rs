//! Ручной запуск обхода: winws.exe с аргументами из стратегии, без консоли.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::{filters, layout::Layout, strategy, userlists};
use crate::journal;
use crate::sys::{cmd, netsh, process};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Starting,
    Running,
    Stopping,
}

pub struct Runner {
    pub phase: Phase,
    pub strategy: Option<String>,
    pub started_at: Option<u64>,
    child: Option<Child>,
    /// PID winws.exe (при запуске через .bat — найденный после старта).
    pub pid: Option<u32>,
}

impl Default for Runner {
    fn default() -> Self {
        Self { phase: Phase::Idle, strategy: None, started_at: None, child: None, pid: None }
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Последние строки вывода winws — показываем их, если процесс сразу упал.
type Tail = Arc<Mutex<VecDeque<String>>>;

fn pipe_reader(stream: impl Read + Send + 'static, tail: Tail) {
    thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }
            tracing::info!("winws: {line}");
            let lower = line.to_lowercase();
            if ["error", "fail", "cannot", "could not", "unable"].iter().any(|w| lower.contains(w)) {
                journal::warn(format!("winws: {line}"));
            }
            let mut t = tail.lock().unwrap();
            if t.len() == 8 {
                t.pop_front();
            }
            t.push_back(line);
        }
    });
}

impl Runner {
    pub fn is_active(&self) -> bool {
        self.phase != Phase::Idle
    }

    fn clear(&mut self) {
        *self = Runner::default();
    }

    /// Проверяет, не завершился ли наш winws сам. Возвращает true, если упал.
    pub fn reap(&mut self) -> bool {
        if self.phase != Phase::Running {
            return false;
        }
        let pid = self.pid;
        let dead = match (&mut self.child, pid) {
            (Some(child), _) => matches!(child.try_wait(), Ok(Some(_))),
            (None, Some(pid)) => !process::find("winws.exe").iter().any(|p| p.pid == pid),
            _ => false,
        };
        if dead {
            let name = self.strategy.clone().unwrap_or_default();
            journal::err(format!("winws.exe неожиданно завершился (стратегия «{name}»)"));
            self.clear();
        }
        dead
    }
}

/// Запуск. Вызывается вне блокировки состояния: `runner` захватывается коротко.
pub fn start(runner: &Mutex<Runner>, root: &Path, name: &str) -> Result<(), String> {
    {
        let mut r = runner.lock().unwrap();
        if r.is_active() {
            return Err("обход уже запускается или работает".into());
        }
        r.phase = Phase::Starting;
        r.strategy = Some(name.to_string());
    }
    let result = start_inner(runner, root, name);
    if let Err(e) = &result {
        journal::err(format!("Не удалось запустить «{name}»: {e}"));
        runner.lock().unwrap().clear();
    }
    result
}

fn start_inner(runner: &Mutex<Runner>, root: &Path, name: &str) -> Result<(), String> {
    let layout = Layout::new(root);
    let strat = strategy::find(root, name).ok_or_else(|| format!("стратегия «{name}» не найдена"))?;
    userlists::ensure(root).map_err(|e| format!("не удалось создать списки *-user.txt: {e}"))?;
    if netsh::tcp_timestamps_enabled() == Some(false) {
        match netsh::enable_tcp_timestamps() {
            Ok(()) => journal::info("TCP timestamps включены"),
            Err(e) => journal::warn(format!("Не удалось включить TCP timestamps: {e}")),
        }
    }
    let killed = process::kill_all("winws.exe");
    if killed > 0 {
        journal::info("Остановлен ранее запущенный winws.exe");
        thread::sleep(Duration::from_millis(500));
    }

    let game = filters::game(root);
    journal::info(format!(
        "IPSet: {} · Game filter: {}",
        match filters::ipset_mode(root) {
            filters::IpsetMode::Off => "off",
            filters::IpsetMode::Any => "any",
            filters::IpsetMode::Loaded => "loaded",
        },
        match game.mode {
            filters::GameMode::Off => "off",
            filters::GameMode::Tcp => "tcp",
            filters::GameMode::Udp => "udp",
            filters::GameMode::TcpUdp => "tcp+udp",
        }
    ));

    let tail: Tail = Arc::default();
    let (child, pid) = match super::batparse::parse_file(&strat.file, &super::vars(root)) {
        Ok(args) => {
            let mut child = cmd::hidden(&layout.winws().to_string_lossy())
                .args(&args)
                .current_dir(layout.bin())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| format!("winws.exe не запустился: {e}"))?;
            pipe_reader(child.stdout.take().unwrap(), tail.clone());
            pipe_reader(child.stderr.take().unwrap(), tail.clone());
            let pid = child.id();
            journal::info(format!("winws.exe запущен (PID {pid})"));
            (Some(child), Some(pid))
        }
        Err(e) => {
            // Запасной путь: сам .bat (он вызовет service.bat и `start /min winws.exe`).
            journal::warn(format!("Стратегия «{name}» запущена через .bat: {e}"));
            cmd::hidden("cmd")
                .arg("/d")
                .arg("/c")
                .arg(&strat.file)
                .current_dir(root)
                .env("NO_UPDATE_CHECK", "1")
                .spawn()
                .map_err(|e| format!("не удалось запустить .bat: {e}"))?;
            let t0 = Instant::now();
            let pid = loop {
                if let Some(p) = process::find("winws.exe").first() {
                    break Some(p.pid);
                }
                if t0.elapsed() > Duration::from_secs(5) {
                    break None;
                }
                thread::sleep(Duration::from_millis(200));
            };
            (None, pid)
        }
    };

    {
        let mut r = runner.lock().unwrap();
        r.child = child;
        r.pid = pid;
    }
    // Стратегия считается запущенной, если winws прожил 1,5 с.
    thread::sleep(Duration::from_millis(1500));
    let mut guard = runner.lock().unwrap();
    let r = &mut *guard;
    let alive = match (&mut r.child, r.pid) {
        (Some(c), _) => matches!(c.try_wait(), Ok(None)),
        (None, Some(pid)) => process::find("winws.exe").iter().any(|p| p.pid == pid),
        (None, None) => false,
    };
    if !alive {
        let lines: Vec<String> = tail.lock().unwrap().iter().cloned().collect();
        r.child = None;
        r.pid = None;
        let detail = if lines.is_empty() { String::new() } else { format!(": {}", lines.join(" · ")) };
        return Err(format!("winws.exe завершился сразу после запуска{detail}"));
    }
    r.phase = Phase::Running;
    r.started_at = Some(now_ms());
    drop(guard);
    journal::ok(format!("Стратегия «{name}» запущена"));
    Ok(())
}

/// Тихий запуск стратегии для тестов: без журнала и захвата вывода.
/// `None` — запущено через .bat (процесс winws ищет вызывающая сторона).
pub fn spawn_quiet(root: &Path, name: &str) -> Result<Option<Child>, String> {
    let strat = strategy::find(root, name).ok_or_else(|| format!("стратегия «{name}» не найдена"))?;
    userlists::ensure(root).map_err(|e| e.to_string())?;
    match super::batparse::parse_file(&strat.file, &super::vars(root)) {
        Ok(args) => {
            let layout = Layout::new(root);
            cmd::hidden(&layout.winws().to_string_lossy())
                .args(&args)
                .current_dir(layout.bin())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map(Some)
                .map_err(|e| format!("winws.exe не запустился: {e}"))
        }
        Err(_) => cmd::hidden("cmd")
            .arg("/d")
            .arg("/c")
            .arg(&strat.file)
            .current_dir(root)
            .env("NO_UPDATE_CHECK", "1")
            .spawn()
            .map(|_| None)
            .map_err(|e| format!("не удалось запустить .bat: {e}")),
    }
}

/// Остановка ручного обхода (и любых winws.exe, если служба не работает).
pub fn stop(runner: &Mutex<Runner>) -> Result<(), String> {
    let child = {
        let mut r = runner.lock().unwrap();
        r.phase = Phase::Stopping;
        r.child.take()
    };
    if let Some(mut c) = child {
        let _ = c.kill();
        let _ = c.wait();
    }
    process::kill_all("winws.exe");
    let t0 = Instant::now();
    while process::exists("winws.exe") && t0.elapsed() < Duration::from_secs(3) {
        thread::sleep(Duration::from_millis(150));
    }
    let still = process::exists("winws.exe");
    runner.lock().unwrap().clear();
    if still {
        return Err("winws.exe не завершился — возможно, его держит служба или другой обход".into());
    }
    journal::info("Обход остановлен");
    Ok(())
}
