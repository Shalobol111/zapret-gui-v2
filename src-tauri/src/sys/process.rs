//! Поиск и завершение процессов (winws.exe и др.).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use windows::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

#[derive(Debug, Clone, PartialEq)]
pub struct ProcInfo {
    pub pid: u32,
    /// Время старта, unix-секунды.
    pub start_time: u64,
    /// Аргументы без имени программы.
    pub args: Vec<String>,
    /// Путь к exe (если доступен).
    pub exe: Option<PathBuf>,
}

static SYS: Mutex<Option<System>> = Mutex::new(None);

fn with_sys<T>(f: impl FnOnce(&mut System) -> T) -> T {
    let mut guard = SYS.lock().unwrap();
    let sys = guard.get_or_insert_with(System::new);
    f(sys)
}

/// (PID, имя) процессов с любым из имён — снимком ToolHelp (только имена и PID).
fn pids_by_name(names: &[&str]) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut e).is_ok();
        while ok {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            let name = String::from_utf16_lossy(&e.szExeFile[..len]);
            if names.iter().any(|n| name.eq_ignore_ascii_case(n)) {
                out.push((e.th32ProcessID, name.to_lowercase()));
            }
            ok = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

/// Подробности процессов по PID: путь, аргументы и время старта не меняются, пока процесс жив,
/// поэтому читаются один раз. sysinfo на Windows опрашивает всю систему даже ради одного PID
/// (~17 мс), а монитор спрашивает каждые 1,5 с.
static DETAILS: Mutex<Option<HashMap<u32, (String, ProcInfo)>>> = Mutex::new(None);

/// Все процессы с данным именем (без учёта регистра).
pub fn find(name: &str) -> Vec<ProcInfo> {
    find_any(&[name])
}

/// Процессы с любым из имён — за один снимок системы.
pub fn find_any(names: &[&str]) -> Vec<ProcInfo> {
    let found = pids_by_name(names);
    let lower: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
    let mut guard = DETAILS.lock().unwrap();
    let cache = guard.get_or_insert_with(HashMap::new);
    // Завершившиеся процессы этих имён — из кэша вон (PID может достаться другому процессу).
    cache.retain(|pid, (name, _)| !lower.contains(name) || found.iter().any(|(p, _)| p == pid));
    let new: Vec<Pid> =
        found.iter().filter(|(pid, _)| !cache.contains_key(pid)).map(|(pid, _)| Pid::from_u32(*pid)).collect();
    if !new.is_empty() {
        with_sys(|sys| {
            sys.refresh_processes_specifics(
                ProcessesToUpdate::Some(&new),
                true,
                ProcessRefreshKind::nothing().with_cmd(UpdateKind::OnlyIfNotSet).with_exe(UpdateKind::OnlyIfNotSet),
            );
            for pid in &new {
                if let Some(p) = sys.process(*pid) {
                    let info = ProcInfo {
                        pid: p.pid().as_u32(),
                        start_time: p.start_time(),
                        args: p.cmd().iter().skip(1).map(|a| a.to_string_lossy().into_owned()).collect(),
                        exe: p.exe().map(|e| e.to_path_buf()),
                    };
                    let name = found.iter().find(|(id, _)| *id == info.pid).map(|(_, n)| n.clone()).unwrap_or_default();
                    cache.insert(info.pid, (name, info));
                }
            }
        });
    }
    found.iter().filter_map(|(pid, _)| cache.get(pid).map(|(_, i)| i.clone())).collect()
}

/// Жив ли процесс (точечная проверка, без перечисления всех процессов).
fn alive(pid: u32) -> Option<bool> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code).is_ok();
        let _ = CloseHandle(h);
        ok.then_some(code == STILL_ACTIVE.0 as u32)
    }
}

/// Для фонового опроса (раз в 1,5 с): известные процессы проверяются точечно, полный снимок —
/// не чаще раза в 5 с. Новый процесс появится в статусе с задержкой до 5 с — для монитора
/// это незаметно, а снимок на загруженной системе стоит ~10 мс.
pub fn find_cached(names: &[&str]) -> Vec<ProcInfo> {
    const MAX_AGE: Duration = Duration::from_secs(5);
    type Snapshots = HashMap<String, (Instant, Vec<ProcInfo>)>;
    static CACHE: Mutex<Option<Snapshots>> = Mutex::new(None);
    let key = names.join("|").to_lowercase();
    {
        let mut guard = CACHE.lock().unwrap();
        let cache = guard.get_or_insert_with(HashMap::new);
        if let Some((at, list)) = cache.get(&key) {
            // Нет доступа к процессу (без прав администратора) — доверяем снимку до конца его 5 с.
            if at.elapsed() < MAX_AGE && list.iter().all(|p| alive(p.pid) != Some(false)) {
                return list.clone();
            }
        }
    }
    let list = find_any(names);
    CACHE.lock().unwrap().get_or_insert_with(HashMap::new).insert(key, (Instant::now(), list.clone()));
    list
}

pub fn exists(name: &str) -> bool {
    !find(name).is_empty()
}

/// Завершает все процессы с именем; возвращает число завершённых.
pub fn kill_all(name: &str) -> usize {
    pids_by_name(&[name]).into_iter().filter(|(pid, _)| kill_pid(*pid)).count()
}

/// Завершает процесс по PID; `true` — если он был и завершён.
pub fn kill_pid(pid: u32) -> bool {
    with_sys(|sys| {
        let pid = Pid::from_u32(pid);
        sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, ProcessRefreshKind::nothing());
        sys.process(pid).is_some_and(|p| p.kill())
    })
}
