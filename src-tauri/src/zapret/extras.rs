//! Расширенные настройки из service.bat: «Replace active fakes» и «Update IPSet List».

use std::fs;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::filters::{self, IpsetMode};
use crate::net;

const IPSET_URL: &str =
    "https://raw.githubusercontent.com/Flowseal/zapret-discord-youtube/refs/heads/main/.service/ipset-service.txt";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FakeKind {
    Discord,
    Game,
}

impl FakeKind {
    fn file(self) -> &'static str {
        match self {
            FakeKind::Discord => "ACTIVE_DISCORD_UDP.bin",
            FakeKind::Game => "ACTIVE_GAME_UDP.bin",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fakes {
    /// Имена файлов `bin\*.bin` без расширения, кроме ACTIVE_*.
    pub files: Vec<String>,
    /// Какой файл сейчас скопирован в ACTIVE_DISCORD_UDP.bin (по SHA-256).
    pub discord: Option<String>,
    pub game: Option<String>,
}

pub fn fakes(root: &Path) -> Fakes {
    let bin = root.join("bin");
    let mut files: Vec<(String, String)> = fs::read_dir(&bin)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let stem = name.strip_suffix(".bin")?.to_string();
                    if stem.to_uppercase().starts_with("ACTIVE_") {
                        return None;
                    }
                    Some((stem, net::sha256_file(&e.path()).ok()?))
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort_by_key(|f| f.0.to_lowercase());
    let current = |kind: FakeKind| {
        let h = net::sha256_file(&bin.join(kind.file())).ok()?;
        files.iter().find(|(_, fh)| *fh == h).map(|(n, _)| n.clone())
    };
    Fakes {
        discord: current(FakeKind::Discord),
        game: current(FakeKind::Game),
        files: files.into_iter().map(|(n, _)| n).collect(),
    }
}

pub fn set_fake(root: &Path, kind: FakeKind, name: &str) -> Result<(), String> {
    let bin = root.join("bin");
    // Только имя из списка — без путей.
    if name.contains(['/', '\\']) || name.contains("..") {
        return Err("недопустимое имя файла".into());
    }
    let src = bin.join(format!("{name}.bin"));
    if !src.is_file() {
        return Err(format!("нет файла bin\\{name}.bin"));
    }
    fs::copy(&src, bin.join(kind.file())).map(|_| ()).map_err(|e| format!("не удалось заменить фейк: {e}"))
}

/// Скачивает актуальный список IP Flowseal. В режиме «Загруженный» — сразу в
/// ipset-all.txt, иначе в .backup (чтобы не сломать текущий режим). Возвращает число записей.
pub async fn update_ipset(root: &Path) -> Result<usize, String> {
    let text = net::get_text(IPSET_URL, Duration::from_secs(60)).await?;
    let count = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).count();
    if count < 10 {
        return Err("скачанный список пуст или повреждён".into());
    }
    let lists = root.join("lists");
    let target = if filters::ipset_mode(root) == IpsetMode::Loaded { "ipset-all.txt" } else { "ipset-all.txt.backup" };
    fs::write(lists.join(target), text).map_err(|e| format!("не удалось сохранить {target}: {e}"))?;
    Ok(count)
}

pub fn ipset_count(root: &Path) -> usize {
    let lists = root.join("lists");
    let file = if filters::ipset_mode(root) == IpsetMode::Loaded { "ipset-all.txt" } else { "ipset-all.txt.backup" };
    fs::read(lists.join(file))
        .map(|b| {
            String::from_utf8_lossy(&b).lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).count()
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_and_replaces_active_fake() {
        let root = std::env::temp_dir().join(format!("zgui-fakes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::write(root.join("bin/stun.bin"), [1u8; 100]).unwrap();
        fs::write(root.join("bin/quic.bin"), [2u8; 100]).unwrap();
        fs::write(root.join("bin/ACTIVE_DISCORD_UDP.bin"), [2u8; 100]).unwrap();
        let f = fakes(&root);
        assert_eq!(f.files, vec!["quic", "stun"]);
        assert_eq!(f.discord.as_deref(), Some("quic"));
        assert_eq!(f.game, None);
        set_fake(&root, FakeKind::Game, "stun").unwrap();
        assert_eq!(fakes(&root).game.as_deref(), Some("stun"));
        assert!(set_fake(&root, FakeKind::Game, "..\\x").is_err());
    }
}
