//! Перечисление служб (для диагностики: Killer, Intel Connectivity, VPN и т.д.).

use windows::core::PCWSTR;
use windows::Win32::System::Services::{
    CloseServiceHandle, EnumServicesStatusExW, OpenSCManagerW, ENUM_SERVICE_STATUS_PROCESSW, SC_ENUM_PROCESS_INFO,
    SC_MANAGER_ENUMERATE_SERVICE, SERVICE_ACTIVE, SERVICE_WIN32,
};

#[derive(Debug, Clone)]
pub struct Svc {
    pub name: String,
    pub display: String,
}

/// Только запущенные службы — так же ищет `sc query` без параметров в service.bat.
pub fn active() -> Vec<Svc> {
    let mut out = Vec::new();
    unsafe {
        let Ok(scm) = OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_ENUMERATE_SERVICE) else {
            return out;
        };
        let mut needed = 0u32;
        let mut count = 0u32;
        let mut resume = 0u32;
        let mut buf: Vec<u8> = Vec::new();
        loop {
            let r = EnumServicesStatusExW(
                scm,
                SC_ENUM_PROCESS_INFO,
                SERVICE_WIN32,
                SERVICE_ACTIVE,
                if buf.is_empty() { None } else { Some(buf.as_mut_slice()) },
                &mut needed,
                &mut count,
                Some(&mut resume),
                PCWSTR::null(),
            );
            if count > 0 {
                let items =
                    std::slice::from_raw_parts(buf.as_ptr() as *const ENUM_SERVICE_STATUS_PROCESSW, count as usize);
                for it in items {
                    out.push(Svc {
                        name: it.lpServiceName.to_string().unwrap_or_default(),
                        display: it.lpDisplayName.to_string().unwrap_or_default(),
                    });
                }
            }
            match r {
                Ok(()) => break,
                Err(_) if needed > 0 => buf = vec![0u8; needed as usize + 4096],
                Err(_) => break,
            }
        }
        let _ = CloseServiceHandle(scm);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn lists_running_services() {
        // Журнал событий работает на любой Windows.
        assert!(super::active().iter().any(|s| s.name.eq_ignore_ascii_case("EventLog")));
    }
}
