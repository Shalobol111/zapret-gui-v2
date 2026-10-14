//! Обновление и первая установка zapret-discord-youtube.
//!
//! Версия: `.service/version.txt` (тот же источник, что у service.bat), запасной —
//! GitHub API `releases/latest`. Архив: ассет `zapret-discord-youtube-<ver>.zip`,
//! SHA-256 сверяется с полем `digest`, если GitHub его отдаёт.

use std::cmp::Ordering;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::journal;
use crate::net;

const VERSION_URL: &str = "https://raw.githubusercontent.com/Flowseal/zapret-discord-youtube/main/.service/version.txt";
const API_URL: &str = "https://api.github.com/repos/Flowseal/zapret-discord-youtube/releases/latest";
pub const RELEASES_URL: &str = "https://github.com/Flowseal/zapret-discord-youtube/releases/latest";

// ── Версии ─────────────────────────────────────────────────────

/// «1.9.9c» → ([1, 9, 9], "c"). Буквенный суффикс сравнивается после чисел.
fn parse_version(v: &str) -> Option<(Vec<u32>, String)> {
    let v = v.trim().trim_start_matches(['v', 'V']);
    if v.is_empty() {
        return None;
    }
    let mut nums = Vec::new();
    let mut suffix = String::new();
    for token in v.split('.') {
        let digits: String = token.chars().take_while(|c| c.is_ascii_digit()).collect();
        let rest = &token[digits.len()..];
        if digits.is_empty() || !rest.chars().all(|c| c.is_ascii_alphabetic()) {
            return None;
        }
        nums.push(digits.parse().ok()?);
        if !rest.is_empty() {
            suffix = rest.to_lowercase();
        }
    }
    Some((nums, suffix))
}

/// `None` — хотя бы одну версию не разобрать (тогда обновление не предлагаем).
pub fn compare(a: &str, b: &str) -> Option<Ordering> {
    let (a, b) = (parse_version(a)?, parse_version(b)?);
    Some(a.cmp(&b))
}

// ── Проверка ───────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub version: String,
    pub url: String,
    pub size: Option<u64>,
    pub sha256: Option<String>,
}

pub async fn latest_version() -> Result<String, String> {
    match net::get_text(VERSION_URL, Duration::from_secs(10)).await {
        Ok(v) if parse_version(v.trim()).is_some() => return Ok(v.trim().to_string()),
        Ok(_) => {}
        Err(e) => tracing::debug!("version.txt недоступен: {e}"),
    }
    Ok(release_asset().await?.version)
}

pub fn parse_release(json: &Value) -> Option<Asset> {
    let version = json.get("tag_name")?.as_str()?.trim().to_string();
    let asset = json.get("assets")?.as_array()?.iter().find(|a| {
        a.get("name").and_then(Value::as_str).is_some_and(|n| {
            let n = n.to_lowercase();
            n.starts_with("zapret-discord-youtube") && n.ends_with(".zip")
        })
    })?;
    Some(Asset {
        version,
        url: asset.get("browser_download_url")?.as_str()?.to_string(),
        size: asset.get("size").and_then(Value::as_u64),
        sha256: asset
            .get("digest")
            .and_then(Value::as_str)
            .and_then(|d| d.strip_prefix("sha256:"))
            .map(|d| d.to_lowercase()),
    })
}

pub async fn release_asset() -> Result<Asset, String> {
    let text = net::get_text(API_URL, Duration::from_secs(15)).await?;
    let json: Value = serde_json::from_str(&text).map_err(|e| format!("неверный ответ GitHub: {e}"))?;
    parse_release(&json).ok_or_else(|| "в последнем релизе нет zip-архива".into())
}

// ── Скачивание и распаковка ────────────────────────────────────

/// Скачивает и распаковывает релиз во временную папку; возвращает корень сборки
/// (папку с service.bat) и временную папку для удаления.
pub async fn fetch_release(
    asset: &Asset,
    progress: impl FnMut(u64, Option<u64>),
) -> Result<(PathBuf, PathBuf), String> {
    let tmp = std::env::temp_dir().join(format!("zapretgui-upd-{}", crate::zapret::runner::now_ms()));
    fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let zip_path = tmp.join("release.zip");
    let sha = net::download(&asset.url, &zip_path, progress).await?;
    let len = fs::metadata(&zip_path).map(|m| m.len()).unwrap_or(0);
    if asset.size.is_some_and(|s| s != len) || len < 1024 {
        return Err("скачанный архив повреждён (неверный размер)".into());
    }
    if let Some(expected) = &asset.sha256 {
        if &sha != expected {
            return Err("контрольная сумма архива не совпадает с опубликованной на GitHub".into());
        }
    }
    let out = tmp.join("x");
    extract(&zip_path, &out)?;
    let root = find_root(&out).ok_or("в архиве не найдена сборка zapret (нет service.bat)")?;
    Ok((root, tmp))
}

/// Распаковка с защитой от путей вида `../` (zip-slip).
pub fn extract(zip_path: &Path, dest: &Path) -> Result<(), String> {
    let file = fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| "архив повреждён (некорректный ZIP)".to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = entry.enclosed_name() else {
            return Err(format!("недопустимый путь в архиве: {}", entry.name()));
        };
        let path = dest.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&path).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = fs::File::create(&path).map_err(|e| e.to_string())?;
        io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn find_root(dir: &Path) -> Option<PathBuf> {
    if dir.join("service.bat").is_file() {
        return Some(dir.to_path_buf());
    }
    fs::read_dir(dir).ok()?.flatten().filter(|e| e.path().is_dir()).find_map(|e| find_root(&e.path()))
}

// ── Замена файлов ──────────────────────────────────────────────

/// Пользовательские данные, которые обновление не перезаписывает.
pub fn is_user_file(rel: &Path) -> bool {
    let name = rel.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let in_results = rel.components().any(|c| c.as_os_str().eq_ignore_ascii_case("test results"));
    in_results
        || name.ends_with("-user.txt")
        || name == "ipset-all.txt"
        || name == "ipset-all.txt.backup"
        || name.ends_with(".enabled")
        || (name.starts_with("active_") && name.ends_with(".bin"))
}

/// Копирует дерево поверх установки. Занятый файл переименовывается в `.zgui_old`
/// (на Windows это работает и для открытых файлов), затем удаляется.
pub fn copy_tree(src: &Path, dst: &Path, keep_user: bool) -> Result<usize, String> {
    let mut copied = 0;
    let mut stale = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        let dir = src.join(&rel);
        fs::create_dir_all(dst.join(&rel)).map_err(|e| e.to_string())?;
        for entry in fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
            let rel_child = rel.join(entry.file_name());
            if entry.path().is_dir() {
                stack.push(rel_child);
                continue;
            }
            let target = dst.join(&rel_child);
            if keep_user && target.exists() && is_user_file(&rel_child) {
                continue;
            }
            if fs::copy(entry.path(), &target).is_err() {
                let old = target.with_extension("zgui_old");
                let _ = fs::remove_file(&old);
                fs::rename(&target, &old).map_err(|e| format!("файл занят: {} ({e})", rel_child.display()))?;
                fs::copy(entry.path(), &target).map_err(|e| format!("{}: {e}", rel_child.display()))?;
                stale.push(old);
            }
            copied += 1;
        }
    }
    for old in stale {
        let _ = fs::remove_file(old); // ещё занят — удалит следующая попытка
    }
    Ok(copied)
}

// ── Состояние для интерфейса ───────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Idle,
    Checking,
    Current,
    Available,
    Error,
    Applying,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    pub state: State,
    pub latest: Option<String>,
    pub error: Option<String>,
    /// Этап применения: «Скачивание», «Замена файлов»…
    pub stage: Option<String>,
    pub progress: Option<u8>,
}

impl Default for Update {
    fn default() -> Self {
        Update { state: State::Idle, latest: None, error: None, stage: None, progress: None }
    }
}

pub fn log_result(u: &Update, local: Option<&str>, manual: bool) {
    match u.state {
        State::Available => journal::warn(format!(
            "Доступна новая версия zapret: {} (установлена {})",
            u.latest.as_deref().unwrap_or("?"),
            local.unwrap_or("?")
        )),
        State::Current if manual => {
            journal::ok(format!("Установлена последняя версия zapret ({})", local.unwrap_or("?")))
        }
        State::Error if manual => {
            journal::err(format!("Не удалось проверить обновления: {}", u.error.as_deref().unwrap_or("")))
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn versions_compare_numerically_with_suffix() {
        assert_eq!(compare("1.10.3", "1.9.9"), Some(Ordering::Greater));
        assert_eq!(compare("1.9.9", "1.9.9c"), Some(Ordering::Less));
        assert_eq!(compare("v1.10.3", "1.10.3"), Some(Ordering::Equal));
        assert_eq!(compare("1.10", "1.10.1"), Some(Ordering::Less));
        assert_eq!(compare("abc", "1.0"), None);
    }

    #[test]
    fn release_json_gives_zip_asset_and_digest() {
        let j = json!({
            "tag_name": "1.10.4",
            "assets": [
                { "name": "zapret-discord-youtube-1.10.4.rar", "browser_download_url": "https://x/rar", "size": 1 },
                { "name": "zapret-discord-youtube-1.10.4.zip", "browser_download_url": "https://x/zip", "size": 4096, "digest": "sha256:ABCD" }
            ]
        });
        assert_eq!(
            parse_release(&j),
            Some(Asset {
                version: "1.10.4".into(),
                url: "https://x/zip".into(),
                size: Some(4096),
                sha256: Some("abcd".into())
            })
        );
    }

    #[test]
    fn user_files_are_kept() {
        for p in [
            "lists/list-general-user.txt",
            "lists/ipset-all.txt",
            "utils/game_filter.enabled",
            "bin/ACTIVE_GAME_UDP.bin",
            "utils/test results/a.txt",
        ] {
            assert!(is_user_file(Path::new(p)), "{p}");
        }
        for p in ["lists/list-general.txt", "general.bat", "bin/winws.exe", "utils/targets.txt"] {
            assert!(!is_user_file(Path::new(p)), "{p}");
        }
    }

    #[test]
    fn copy_tree_preserves_user_lists() {
        let base = std::env::temp_dir().join(format!("zgui-upd-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let (src, dst) = (base.join("src"), base.join("dst"));
        fs::create_dir_all(src.join("lists")).unwrap();
        fs::create_dir_all(dst.join("lists")).unwrap();
        fs::write(src.join("general.bat"), "new").unwrap();
        fs::write(src.join("lists/list-general.txt"), "new").unwrap();
        fs::write(src.join("lists/list-general-user.txt"), "default").unwrap();
        fs::write(dst.join("lists/list-general.txt"), "old").unwrap();
        fs::write(dst.join("lists/list-general-user.txt"), "mine").unwrap();
        copy_tree(&src, &dst, true).unwrap();
        assert_eq!(fs::read_to_string(dst.join("general.bat")).unwrap(), "new");
        assert_eq!(fs::read_to_string(dst.join("lists/list-general.txt")).unwrap(), "new");
        assert_eq!(fs::read_to_string(dst.join("lists/list-general-user.txt")).unwrap(), "mine");
    }

    /// Сетевой: скачать, сверить SHA-256 и распаковать настоящий релиз.
    #[test]
    #[ignore]
    fn real_download_and_extract() {
        let asset = tauri::async_runtime::block_on(release_asset()).unwrap();
        let (root, tmp) = tauri::async_runtime::block_on(fetch_release(&asset, |_, _| {})).unwrap();
        assert!(root.join("bin").join("winws.exe").is_file());
        assert!(crate::zapret::strategy::list(&root).len() >= 10);
        println!("ok: {}", root.display());
        let _ = fs::remove_dir_all(tmp);
    }

    /// Сетевой: `cargo test -- --ignored real_release`
    #[test]
    #[ignore]
    fn real_release() {
        let a = tauri::async_runtime::block_on(release_asset()).unwrap();
        println!("{a:?}");
        let v = tauri::async_runtime::block_on(latest_version()).unwrap();
        assert!(parse_version(&v).is_some());
    }
}
