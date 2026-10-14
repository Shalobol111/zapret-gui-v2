//! Автозапуск через Планировщик задач: с наивысшими правами и без запроса UAC
//! (запись в реестре Run не работает для программ с requireAdministrator).
//!
//! Задача создаётся из XML: у `schtasks /create /sc onlogon` по умолчанию лимит
//! работы 72 часа и запрет запуска от батареи — программа закрылась бы через трое
//! суток или не стартовала бы на ноутбуке.

use std::fs;
use std::path::Path;

use super::cmd;

const TASK: &str = "ZapretGUI_Autostart";

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn current_user() -> String {
    let user = std::env::var("USERNAME").unwrap_or_default();
    match std::env::var("USERDOMAIN") {
        Ok(d) if !d.is_empty() => format!("{d}\\{user}"),
        _ => user,
    }
}

pub fn task_xml(exe: &Path, user: &str) -> String {
    let exe = xml_escape(&exe.display().to_string());
    let user = xml_escape(user);
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>ZapretGUI: запуск при входе в Windows (свёрнуто в трей)</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
      <Delay>PT5S</Delay>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Enabled>true</Enabled>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{exe}</Command>
      <Arguments>--minimized</Arguments>
    </Exec>
  </Actions>
</Task>
"#
    )
}

pub fn is_enabled() -> bool {
    cmd::run("schtasks", &["/query", "/tn", TASK]).map(|o| o.status.success()).unwrap_or(false)
}

pub fn enable(exe: &Path) -> Result<(), String> {
    let xml = task_xml(exe, &current_user());
    // schtasks надёжно читает XML только в UTF-16 LE с BOM.
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
    let tmp = std::env::temp_dir().join(format!("zapretgui-task-{}.xml", std::process::id()));
    fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    let out = cmd::run("schtasks", &["/create", "/tn", TASK, "/xml", &tmp.to_string_lossy(), "/f"]);
    let _ = fs::remove_file(&tmp);
    let out = out.map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("schtasks: {}", cmd::text(&out).trim()))
    }
}

pub fn disable() -> Result<(), String> {
    if !is_enabled() {
        return Ok(());
    }
    let out = cmd::run("schtasks", &["/delete", "/tn", TASK, "/f"]).map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("schtasks: {}", cmd::text(&out).trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_escapes_and_keeps_limits_off() {
        let x = task_xml(Path::new(r"C:\Apps & Tools\zapret-gui.exe"), r"PC\Иван");
        assert!(x.contains(r"<Command>C:\Apps &amp; Tools\zapret-gui.exe</Command>"));
        assert!(x.contains(r"<UserId>PC\Иван</UserId>"));
        assert!(x.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(x.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"));
        assert!(x.contains("<RunLevel>HighestAvailable</RunLevel>"));
    }
}
