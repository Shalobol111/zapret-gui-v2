//! TCP timestamps (их включают все стратегии через `service.bat status_zapret`).

use super::cmd;

/// `None` — не удалось определить.
pub fn tcp_timestamps_enabled() -> Option<bool> {
    // chcp 437 переключает netsh на английский вывод — так же проверяет service.bat.
    let out = cmd::hidden("cmd").args(["/d", "/c", "chcp 437 >nul & netsh interface tcp show global"]).output().ok()?;
    let text = cmd::text(&out).to_lowercase();
    let line = text.lines().find(|l| l.contains("timestamps"))?;
    Some(line.contains("enabled"))
}

pub fn enable_tcp_timestamps() -> Result<(), String> {
    let out =
        cmd::run("netsh", &["interface", "tcp", "set", "global", "timestamps=enabled"]).map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(cmd::text(&out).trim().to_string())
    }
}

pub fn flush_dns() {
    let _ = cmd::run("ipconfig", &["/flushdns"]);
}
