//! sing-box: скачивание официального релиза (SagerNet/sing-box, windows-amd64) с проверкой SHA-256,
//! проверка конфига и запуск отдельным процессом без окна.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::Duration;

use serde_json::Value;

use crate::sys::cmd;
use crate::zapret::updater;

const API_LATEST: &str = "https://api.github.com/repos/SagerNet/sing-box/releases/latest";

pub fn core_dir() -> PathBuf {
    super::dir().join("core")
}
pub fn exe() -> PathBuf {
    core_dir().join("sing-box.exe")
}
fn version_file() -> PathBuf {
    core_dir().join("version.txt")
}

pub fn installed() -> bool {
    exe().is_file()
}

pub fn version() -> Option<String> {
    fs::read_to_string(version_file()).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

pub struct Release {
    pub version: String,
    url: String,
    sha256: Option<String>,
}

fn parse_release(v: &Value) -> Option<Release> {
    let version = v.get("tag_name")?.as_str()?.trim_start_matches('v').to_string();
    let want = format!("sing-box-{version}-windows-amd64.zip");
    let a =
        v.get("assets")?.as_array()?.iter().find(|a| a.get("name").and_then(Value::as_str) == Some(want.as_str()))?;
    Some(Release {
        version,
        url: a.get("browser_download_url")?.as_str()?.to_string(),
        sha256: a.get("digest").and_then(Value::as_str).and_then(|d| d.strip_prefix("sha256:")).map(str::to_lowercase),
    })
}

pub async fn latest() -> Result<Release, String> {
    let text = crate::net::get_text(API_LATEST, Duration::from_secs(15)).await?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("неверный ответ GitHub: {e}"))?;
    parse_release(&v).ok_or_else(|| "в последнем релизе sing-box нет сборки для Windows x64".into())
}

/// Новее ли версия на GitHub, чем установленная.
pub fn newer(latest: &str) -> bool {
    match version() {
        Some(cur) => updater::compare(latest, &cur) == Some(std::cmp::Ordering::Greater),
        None => true,
    }
}

/// Скачать и распаковать (sing-box должен быть остановлен).
pub async fn install(r: &Release, mut progress: impl FnMut(u8)) -> Result<(), String> {
    let tmp = super::dir().join("download");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let zip = tmp.join("sing-box.zip");
    let mut last = 255u8;
    let hash = crate::net::download(&r.url, &zip, |got, total| {
        if let Some(t) = total.filter(|t| *t > 0) {
            let p = (got * 100 / t).min(100) as u8;
            if p != last {
                last = p;
                progress(p);
            }
        }
    })
    .await?;
    if let Some(want) = &r.sha256 {
        if &hash != want {
            let _ = fs::remove_dir_all(&tmp);
            return Err("контрольная сумма sing-box не совпала — файл повреждён, попробуйте ещё раз".into());
        }
    }
    let out = tmp.join("x");
    updater::extract(&zip, &out)?;
    let found = find_exe(&out).ok_or("в архиве нет sing-box.exe")?;
    let src_dir = found.parent().unwrap_or(&out).to_path_buf();
    let _ = fs::remove_dir_all(core_dir());
    fs::create_dir_all(core_dir()).map_err(|e| e.to_string())?;
    for e in fs::read_dir(&src_dir).map_err(|e| e.to_string())?.flatten() {
        if e.path().is_file() {
            fs::copy(e.path(), core_dir().join(e.file_name()))
                .map_err(|e| format!("не удалось скопировать sing-box: {e}"))?;
        }
    }
    let _ = fs::write(version_file(), &r.version);
    let _ = fs::remove_dir_all(&tmp);
    Ok(())
}

fn find_exe(dir: &Path) -> Option<PathBuf> {
    for e in fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_file() && p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("sing-box.exe")) {
            return Some(p);
        }
        if p.is_dir() {
            if let Some(f) = find_exe(&p) {
                return Some(f);
            }
        }
    }
    None
}

/// `sing-box check` — ошибки конфига до запуска (текст ошибки от sing-box).
pub fn check(config: &Path) -> Result<(), String> {
    let out = cmd::hidden(&exe().to_string_lossy())
        .args(["check", "-c"])
        .arg(config)
        .current_dir(core_dir())
        .output()
        .map_err(|e| format!("не удалось запустить sing-box: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let text = cmd::text(&out);
        Err(text.lines().rfind(|l| !l.trim().is_empty()).unwrap_or("конфиг не принят").trim().to_string())
    }
}

/// Запуск: `sing-box run -c config.json -D <папка для кэша>`.
pub fn spawn(config: &Path) -> Result<Child, String> {
    cmd::hidden(&exe().to_string_lossy())
        .args(["run", "-c"])
        .arg(config)
        .arg("-D")
        .arg(super::dir())
        .current_dir(core_dir())
        .spawn()
        .map_err(|e| format!("не удалось запустить sing-box: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn picks_windows_amd64_zip() {
        let v = json!({ "tag_name": "v1.14.4", "assets": [
            { "name": "sing-box-1.14.4-windows-amd64-legacy-windows-7.zip", "browser_download_url": "legacy" },
            { "name": "sing-box-1.14.4-windows-amd64.zip", "browser_download_url": "ok", "digest": "sha256:ABC" },
            { "name": "sing-box-1.14.4-windows-arm64.zip", "browser_download_url": "arm" }
        ]});
        let r = parse_release(&v).unwrap();
        assert_eq!((r.version.as_str(), r.url.as_str(), r.sha256.as_deref()), ("1.14.4", "ok", Some("abc")));
    }
}
