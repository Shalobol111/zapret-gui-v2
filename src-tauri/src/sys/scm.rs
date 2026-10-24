//! Службы Windows через Service Control Manager.

use std::ffi::OsString;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::UI::Shell::CommandLineToArgvW;
use windows_service::service::{
    ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceState, ServiceType,
};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

const ERROR_SERVICE_DOES_NOT_EXIST: i32 = 1060;
const ERROR_SERVICE_NOT_ACTIVE: i32 = 1062;
const ERROR_ACCESS_DENIED: i32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    NotInstalled,
    Running,
    Stopped,
    StartPending,
    StopPending,
}

fn os_code(e: &windows_service::Error) -> Option<i32> {
    match e {
        windows_service::Error::Winapi(io) => io.raw_os_error(),
        _ => None,
    }
}

fn describe(e: windows_service::Error) -> String {
    match os_code(&e) {
        Some(ERROR_ACCESS_DENIED) => "нет прав администратора".into(),
        Some(code) => format!("{} (код {code})", std::io::Error::from_raw_os_error(code)),
        None => e.to_string(),
    }
}

fn manager(access: ServiceManagerAccess) -> Result<ServiceManager, String> {
    ServiceManager::local_computer(None::<&str>, access).map_err(describe)
}

pub fn state(name: &str) -> State {
    let Ok(m) = manager(ServiceManagerAccess::CONNECT) else { return State::NotInstalled };
    let svc = match m.open_service(name, ServiceAccess::QUERY_STATUS) {
        Ok(s) => s,
        Err(_) => return State::NotInstalled,
    };
    match svc.query_status().map(|s| s.current_state) {
        Ok(ServiceState::Running) => State::Running,
        Ok(ServiceState::StartPending | ServiceState::ContinuePending) => State::StartPending,
        Ok(ServiceState::StopPending | ServiceState::PausePending) => State::StopPending,
        _ => State::Stopped,
    }
}

/// Полная командная строка службы (binPath).
///
/// Читается из реестра (`ImagePath`), а не через QueryServiceConfig: windows-service
/// берёт буфер 8 КБ, а строка zapret со всеми аргументами — около 12 КБ (≈6000 символов
/// UTF-16), и запрос молча падал. Так же падает `sc qc zapret`.
pub fn bin_path(name: &str) -> Option<String> {
    let key = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)
        .open_subkey(format!(r"SYSTEM\CurrentControlSet\Services\{name}"))
        .ok()?;
    key.get_value::<String, _>("ImagePath").ok().filter(|s| !s.is_empty())
}

pub fn create_and_start(
    name: &str,
    display: &str,
    description: &str,
    exe: &Path,
    args: &[String],
) -> Result<(), String> {
    let m = manager(ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE)?;
    let info = ServiceInfo {
        name: name.into(),
        display_name: display.into(),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: exe.to_path_buf(),
        launch_arguments: args.iter().map(OsString::from).collect(),
        dependencies: vec![],
        account_name: None, // LocalSystem, как у sc create
        account_password: None,
    };
    let svc = m
        .create_service(&info, ServiceAccess::START | ServiceAccess::CHANGE_CONFIG | ServiceAccess::QUERY_STATUS)
        .map_err(|e| format!("не удалось создать службу: {}", describe(e)))?;
    let _ = svc.set_description(description);
    svc.start::<&str>(&[]).map_err(|e| format!("служба создана, но не запустилась: {}", describe(e)))?;
    Ok(())
}

const ERROR_FILE_NOT_FOUND: i32 = 2;
const ERROR_PATH_NOT_FOUND: i32 = 3;

pub fn start(name: &str) -> Result<(), String> {
    let m = manager(ServiceManagerAccess::CONNECT)?;
    let svc = m.open_service(name, ServiceAccess::START).map_err(describe)?;
    svc.start::<&str>(&[]).map_err(|e| match os_code(&e) {
        Some(ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND) => {
            "служба ссылается на программу, которой больше нет (папку zapret переместили или удалили) — \
             переустановите службу"
                .into()
        }
        _ => describe(e),
    })
}

/// Останавливает службу и ждёт остановки. Отсутствие службы — не ошибка.
pub fn stop(name: &str, timeout: Duration) -> Result<(), String> {
    let m = manager(ServiceManagerAccess::CONNECT)?;
    let svc = match m.open_service(name, ServiceAccess::STOP | ServiceAccess::QUERY_STATUS) {
        Ok(s) => s,
        Err(e) if os_code(&e) == Some(ERROR_SERVICE_DOES_NOT_EXIST) => return Ok(()),
        Err(e) => return Err(describe(e)),
    };
    match svc.stop() {
        Ok(_) => {}
        Err(e) if os_code(&e) == Some(ERROR_SERVICE_NOT_ACTIVE) => return Ok(()),
        Err(e) => return Err(describe(e)),
    }
    let t0 = Instant::now();
    while t0.elapsed() < timeout {
        if matches!(svc.query_status().map(|s| s.current_state), Ok(ServiceState::Stopped)) {
            return Ok(());
        }
        sleep(Duration::from_millis(200));
    }
    Err("служба не остановилась вовремя".into())
}

/// Останавливает и удаляет службу. Отсутствие службы — не ошибка.
pub fn remove(name: &str) -> Result<(), String> {
    stop(name, Duration::from_secs(10)).ok();
    let m = manager(ServiceManagerAccess::CONNECT)?;
    match m.open_service(name, ServiceAccess::DELETE) {
        Ok(svc) => svc.delete().map_err(describe),
        Err(e) if os_code(&e) == Some(ERROR_SERVICE_DOES_NOT_EXIST) => Ok(()),
        Err(e) => Err(describe(e)),
    }
}

/// Разбор командной строки по правилам Windows (CommandLineToArgvW).
pub fn split_command_line(cmdline: &str) -> Vec<String> {
    let wide: Vec<u16> = cmdline.encode_utf16().chain(std::iter::once(0)).collect();
    let mut argc = 0i32;
    unsafe {
        let argv = CommandLineToArgvW(PCWSTR(wide.as_ptr()), &mut argc);
        if argv.is_null() {
            return vec![];
        }
        let out = (0..argc as usize).map(|i| (*argv.add(i)).to_string().unwrap_or_default()).collect();
        let _ = LocalFree(Some(HLOCAL(argv as *mut _)));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_matches_windows_rules() {
        let args = split_command_line(r#""C:\a b\winws.exe" --x=1 "--hostlist=C:\a b\l.txt" --y"#);
        assert_eq!(args, vec![r"C:\a b\winws.exe", "--x=1", r"--hostlist=C:\a b\l.txt", "--y"]);
    }

    #[test]
    fn missing_service_is_not_installed() {
        assert_eq!(state("zapret-gui-no-such-service"), State::NotInstalled);
    }
}
