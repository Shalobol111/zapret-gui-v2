//! Поиск и завершение процессов (winws.exe и др.).

use std::sync::Mutex;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

#[derive(Debug, Clone, PartialEq)]
pub struct ProcInfo {
    pub pid: u32,
    /// Время старта, unix-секунды.
    pub start_time: u64,
    /// Аргументы без имени программы.
    pub args: Vec<String>,
}

static SYS: Mutex<Option<System>> = Mutex::new(None);

fn with_sys<T>(f: impl FnOnce(&mut System) -> T) -> T {
    let mut guard = SYS.lock().unwrap();
    let sys = guard.get_or_insert_with(System::new);
    f(sys)
}

/// Все процессы с данным именем (без учёта регистра). Командная строка читается
/// только у них — опрос раз в 1,5 с почти не нагружает систему.
pub fn find(name: &str) -> Vec<ProcInfo> {
    with_sys(|sys| {
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
        let pids: Vec<Pid> = sys
            .processes()
            .values()
            .filter(|p| p.name().to_string_lossy().eq_ignore_ascii_case(name))
            .map(|p| p.pid())
            .collect();
        if pids.is_empty() {
            return vec![];
        }
        sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&pids),
            true,
            ProcessRefreshKind::nothing().with_cmd(UpdateKind::OnlyIfNotSet),
        );
        pids.iter()
            .filter_map(|pid| sys.process(*pid))
            .map(|p| ProcInfo {
                pid: p.pid().as_u32(),
                start_time: p.start_time(),
                args: p.cmd().iter().skip(1).map(|a| a.to_string_lossy().into_owned()).collect(),
            })
            .collect()
    })
}

pub fn exists(name: &str) -> bool {
    !find(name).is_empty()
}

/// Завершает все процессы с именем; возвращает число завершённых.
pub fn kill_all(name: &str) -> usize {
    with_sys(|sys| {
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
        sys.processes()
            .values()
            .filter(|p| p.name().to_string_lossy().eq_ignore_ascii_case(name))
            .filter(|p| p.kill())
            .count()
    })
}
