//! ICMP-пинг через IcmpSendEcho (как System.Net.NetworkInformation.Ping в test zapret.ps1).

use std::net::{IpAddr, ToSocketAddrs};

use windows::Win32::NetworkManagement::IpHelper::{IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY};

/// Время ответа в мс; `None` — таймаут, нет IPv4-адреса или ошибка.
pub fn ping(host: &str, timeout_ms: u32) -> Option<u32> {
    let ip = match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => v4,
        Ok(IpAddr::V6(_)) => return None,
        Err(_) => (host, 0).to_socket_addrs().ok()?.find_map(|a| match a.ip() {
            IpAddr::V4(v4) => Some(v4),
            IpAddr::V6(_) => None,
        })?,
    };
    let payload = *b"zapret-gui-ping-zapret-gui-ping!";
    // Буфер ответа: структура + данные + 8 байт на ICMP-ошибку (по документации).
    let mut reply = vec![0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + payload.len() + 8];
    unsafe {
        let handle = IcmpCreateFile().ok()?;
        let n = IcmpSendEcho(
            handle,
            u32::from_ne_bytes(ip.octets()),
            payload.as_ptr() as *const _,
            payload.len() as u16,
            None,
            reply.as_mut_ptr() as *mut _,
            reply.len() as u32,
            timeout_ms,
        );
        let _ = IcmpCloseHandle(handle);
        if n == 0 {
            return None;
        }
        let r = &*(reply.as_ptr() as *const ICMP_ECHO_REPLY);
        (r.Status == 0).then_some(r.RoundTripTime)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn loopback_answers() {
        assert!(super::ping("127.0.0.1", 1000).is_some());
    }
}
