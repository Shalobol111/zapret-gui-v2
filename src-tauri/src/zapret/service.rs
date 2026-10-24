//! Служба `zapret` — так же, как её ставит service.bat (Install Service), чтобы
//! service.bat → Check Status видел нашу службу, а мы — его.

use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE};
use winreg::RegKey;

use crate::journal;
use crate::sys::{process, scm};

pub const NAME: &str = "zapret";
const DESCRIPTION: &str = "Zapret DPI bypass software";
const REG_KEY: &str = r"System\CurrentControlSet\Services\zapret";
const REG_VALUE: &str = "zapret-discord-youtube";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    NotInstalled,
    Running,
    Stopped,
    Pending,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub state: State,
    pub strategy: Option<String>,
    /// Параметры в binPath отличаются от тех, что дала бы стратегия сейчас
    /// (сменили фильтры, отредактировали .bat, обновили zapret).
    pub outdated_args: bool,
    /// STOP_PENDING — по service.bat это признак конфликта с другим обходом.
    pub stop_pending: bool,
    /// winws.exe из binPath не существует (папку zapret переместили или удалили) —
    /// служба не запустится, нужна переустановка.
    pub missing_exe: bool,
}

pub fn strategy_name() -> Option<String> {
    let key = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(REG_KEY, KEY_READ).ok()?;
    key.get_value::<String, _>(REG_VALUE).ok()
}

pub fn status(root: Option<&Path>) -> Status {
    let raw = scm::state(NAME);
    let state = match raw {
        scm::State::NotInstalled => State::NotInstalled,
        scm::State::Running => State::Running,
        scm::State::Stopped => State::Stopped,
        scm::State::StartPending | scm::State::StopPending => State::Pending,
    };
    if state == State::NotInstalled {
        return Status { state, strategy: None, outdated_args: false, stop_pending: false, missing_exe: false };
    }
    let strategy = strategy_name();
    let bin = scm::bin_path(NAME);
    let actual = bin.as_deref().map(scm::split_command_line).unwrap_or_default();
    let missing_exe = actual.first().is_some_and(|exe| !Path::new(exe).is_file());
    let outdated_args = match (root, &strategy, &bin) {
        (Some(root), Some(name), Some(_)) => match super::strategy_args(root, name) {
            Ok((_, expected)) => actual.get(1..).map(|a| a != expected.as_slice()).unwrap_or(true),
            Err(_) => false, // стратегии больше нет — показываем имя, но не спорим о параметрах
        },
        _ => false,
    };
    Status { state, strategy, outdated_args, stop_pending: raw == scm::State::StopPending, missing_exe }
}

/// Ставит службу со стратегией: удаляет старую, создаёт, запускает, пишет имя в реестр.
/// Ручной winws должен быть остановлен вызывающей стороной.
pub fn install(root: &Path, name: &str) -> Result<(), String> {
    let (strategy, args) = super::strategy_args(root, name)?;
    super::userlists::ensure(root).map_err(|e| format!("не удалось создать списки *-user.txt: {e}"))?;
    if crate::sys::netsh::tcp_timestamps_enabled() == Some(false) {
        crate::sys::netsh::enable_tcp_timestamps().ok();
    }
    scm::remove(NAME)?;
    // Служба могла держать winws — дожидаемся, пока он закроется.
    process::kill_all("winws.exe");
    let winws = super::layout::Layout::new(root).winws();
    scm::create_and_start(NAME, "zapret", DESCRIPTION, &winws, &args)?;
    let key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(REG_KEY, KEY_SET_VALUE)
        .map_err(|e| format!("не удалось записать имя стратегии в реестр: {e}"))?;
    key.set_value(REG_VALUE, &strategy.name).map_err(|e| e.to_string())?;
    Ok(())
}

/// «Удалить службы» из service.bat: zapret, winws.exe, WinDivert и WinDivert14.
pub fn remove_all() -> Result<(), String> {
    scm::remove(NAME)?;
    let killed = process::kill_all("winws.exe");
    if killed > 0 {
        journal::info(format!("Завершено процессов winws.exe: {killed}"));
    }
    for drv in ["WinDivert", "WinDivert14"] {
        if let Err(e) = scm::remove(drv) {
            journal::warn(format!("{drv}: {e}"));
        }
    }
    Ok(())
}

pub fn start() -> Result<(), String> {
    scm::start(NAME)
}

pub fn stop() -> Result<(), String> {
    scm::stop(NAME, Duration::from_secs(10))
}

#[cfg(test)]
mod tests {
    /// Только чтение настоящей службы: `ZAPRET_ROOT=<папка> cargo test -- --ignored live_service_status`.
    #[test]
    #[ignore]
    fn live_service_status() {
        let root = std::env::var("ZAPRET_ROOT").ok().map(std::path::PathBuf::from);
        let bin = crate::sys::scm::bin_path(super::NAME);
        println!("binPath: {} символов", bin.as_deref().map(str::len).unwrap_or(0));
        println!("{:?}", super::status(root.as_deref()));
    }
}
