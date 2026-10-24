//! Журнал для карточки «Журнал»: кольцевой буфер + событие `log://line`.
//! Каждая запись дублируется в файловый журнал (tracing).

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use windows::Win32::System::SystemInformation::GetLocalTime;

const CAPACITY: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Ok,
    Info,
    Warn,
    Err,
}

#[derive(Debug, Clone, Serialize)]
pub struct Entry {
    pub id: u64,
    pub time: String,
    pub level: Level,
    pub text: String,
}

struct Journal {
    next_id: u64,
    entries: VecDeque<Entry>,
}

static JOURNAL: Mutex<Journal> = Mutex::new(Journal { next_id: 1, entries: VecDeque::new() });
static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn attach(app: AppHandle) {
    let _ = APP.set(app);
}

fn local_time() -> String {
    let t = unsafe { GetLocalTime() };
    format!("{:02}:{:02}:{:02}", t.wHour, t.wMinute, t.wSecond)
}

/// Склонение: plural(3, ["стратегия", "стратегии", "стратегий"]).
pub fn plural(n: usize, forms: [&str; 3]) -> &str {
    let (m10, m100) = (n % 10, n % 100);
    if m10 == 1 && m100 != 11 {
        forms[0]
    } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
        forms[1]
    } else {
        forms[2]
    }
}

/// Метка для имён файлов: `2026-10-13_14-02-11` (как в test zapret.ps1).
pub fn file_stamp() -> String {
    let t = unsafe { GetLocalTime() };
    format!("{:04}-{:02}-{:02}_{:02}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

pub fn push(level: Level, text: impl Into<String>) {
    let text = text.into();
    match level {
        Level::Err => tracing::error!("{text}"),
        Level::Warn => tracing::warn!("{text}"),
        _ => tracing::info!("{text}"),
    }
    let entry = {
        let mut j = JOURNAL.lock().unwrap();
        let entry = Entry { id: j.next_id, time: local_time(), level, text };
        j.next_id += 1;
        if j.entries.len() == CAPACITY {
            j.entries.pop_front();
        }
        j.entries.push_back(entry.clone());
        entry
    };
    if let Some(app) = APP.get() {
        let _ = app.emit("log://line", entry);
    }
}

pub fn ok(text: impl Into<String>) {
    push(Level::Ok, text)
}
pub fn info(text: impl Into<String>) {
    push(Level::Info, text)
}
pub fn warn(text: impl Into<String>) {
    push(Level::Warn, text)
}
pub fn err(text: impl Into<String>) {
    push(Level::Err, text)
}

pub fn snapshot() -> Vec<Entry> {
    JOURNAL.lock().unwrap().entries.iter().cloned().collect()
}
