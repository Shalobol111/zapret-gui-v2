//! Списки доменов и IP для экрана «Списки».
//!
//! Пользовательские `*-user.txt` создаются с теми же значениями, что у
//! `service.bat load_user_lists` (стратегии ссылаются на эти файлы и без них не стартуют).
//! Доступ — только к файлам из `LISTS` по id, произвольные пути из интерфейса не принимаются.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::filters::{self, IpsetMode};

pub fn ensure(root: &Path) -> std::io::Result<()> {
    let lists = root.join("lists");
    fs::create_dir_all(&lists)?;
    let defaults: [(&str, &str); 3] = [
        ("ipset-exclude-user.txt", "203.0.113.113/32\r\n"),
        ("list-general-user.txt", "# Never leave this file empty\r\ndomain.example.abc\r\n"),
        ("list-exclude-user.txt", "domain.example.abc\r\n"),
    ];
    for (name, content) in defaults {
        let path = lists.join(name);
        if !path.exists() {
            fs::write(path, content)?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Domains,
    Ips,
}

struct Def {
    id: &'static str,
    title: &'static str,
    file: &'static str,
    kind: Kind,
}

const LISTS: [Def; 5] = [
    Def { id: "user", title: "Свои домены", file: "list-general-user.txt", kind: Kind::Domains },
    Def { id: "general", title: "Основной список", file: "list-general.txt", kind: Kind::Domains },
    Def { id: "exclude", title: "Исключения", file: "list-exclude-user.txt", kind: Kind::Domains },
    Def { id: "ipset", title: "IPSet", file: "ipset-all.txt", kind: Kind::Ips },
    Def { id: "ipset_exclude", title: "Исключения IP", file: "ipset-exclude-user.txt", kind: Kind::Ips },
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListInfo {
    pub id: &'static str,
    pub title: &'static str,
    pub file: String,
    pub kind: Kind,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListData {
    #[serde(flatten)]
    pub info: ListInfo,
    pub text: String,
    pub hash: String,
    pub crlf: bool,
    /// Пояснение над редактором (файл перезаписывается при обновлении, IPSet выключен и т.п.).
    pub note: Option<String>,
}

fn def(id: &str) -> Result<&'static Def, String> {
    LISTS.iter().find(|d| d.id == id).ok_or_else(|| format!("неизвестный список: {id}"))
}

/// Файл на диске. IPSet в режиме «Выкл»/«Любой» хранит загруженный список в
/// `ipset-all.txt.backup` — редактируем его, иначе правка сломала бы режим.
fn resolve(root: &Path, d: &Def) -> (PathBuf, String, Option<String>) {
    let lists = root.join("lists");
    match d.id {
        "ipset" if filters::ipset_mode(root) != IpsetMode::Loaded => (
            lists.join("ipset-all.txt.backup"),
            "ipset-all.txt.backup".into(),
            Some("IPSet сейчас не в режиме «Загруженный» — редактируется сохранённый список, он вернётся при включении режима".into()),
        ),
        "general" => (
            lists.join(d.file),
            d.file.into(),
            Some("Этот файл перезапишется при обновлении zapret — свои домены лучше добавлять в «Свои домены»".into()),
        ),
        _ => (lists.join(d.file), d.file.into(), None),
    }
}

pub fn index(root: &Path) -> Vec<ListInfo> {
    LISTS.iter().map(|d| ListInfo { id: d.id, title: d.title, file: resolve(root, d).1, kind: d.kind }).collect()
}

fn hash(bytes: &[u8]) -> String {
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    format!("{:016x}", h.finish())
}

pub fn read(root: &Path, id: &str) -> Result<ListData, String> {
    ensure(root).map_err(|e| e.to_string())?;
    let d = def(id)?;
    let (path, file, note) = resolve(root, d);
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(format!("не удалось прочитать {file}: {e}")),
    };
    let text = String::from_utf8_lossy(&bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    Ok(ListData {
        info: ListInfo { id: d.id, title: d.title, file, kind: d.kind },
        crlf: text.contains("\r\n") || bytes.is_empty(),
        text: text.replace("\r\n", "\n"),
        hash: hash(&bytes),
        note,
    })
}

fn entries(text: &str) -> usize {
    text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).count()
}

#[derive(Debug, PartialEq, Eq)]
pub enum SaveError {
    /// Файл изменили снаружи после открытия.
    Conflict,
    Other(String),
}

pub fn save(root: &Path, id: &str, text: &str, base_hash: &str, force: bool) -> Result<ListData, SaveError> {
    let d = def(id).map_err(SaveError::Other)?;
    let (path, file, _) = resolve(root, d);
    let current = fs::read(&path).unwrap_or_default();
    if !force && hash(&current) != base_hash {
        return Err(SaveError::Conflict);
    }
    let mut text = text.replace("\r\n", "\n");
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if id == "user" && entries(&text) == 0 {
        // service.bat: «Never leave this file empty» — стратегии ждут хотя бы одну запись.
        text.push_str("domain.example.abc\n");
    }
    if id == "ipset" && file == "ipset-all.txt" && entries(&text) == 0 {
        return Err(SaveError::Other(
            "Пустой ipset-all.txt переключит IPSet в режим «Любой». Чтобы выключить фильтр, используйте переключатель на Главной".into(),
        ));
    }
    let crlf = current.is_empty() || String::from_utf8_lossy(&current).contains("\r\n");
    let out = if crlf { text.replace('\n', "\r\n") } else { text };
    let tmp = path.with_extension("zgui-tmp");
    fs::write(&tmp, out.as_bytes())
        .and_then(|_| fs::rename(&tmp, &path))
        .map_err(|e| SaveError::Other(format!("не удалось сохранить {file}: {e}")))?;
    read(root, id).map_err(SaveError::Other)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zgui-lists-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("lists")).unwrap();
        dir
    }

    #[test]
    fn read_creates_user_lists_and_normalizes_eol() {
        let root = temp_root("read");
        let d = read(&root, "user").unwrap();
        assert!(d.crlf);
        assert_eq!(d.text, "# Never leave this file empty\ndomain.example.abc\n");
    }

    #[test]
    fn save_keeps_crlf_and_detects_conflict() {
        let root = temp_root("save");
        let d = read(&root, "exclude").unwrap();
        let saved = save(&root, "exclude", "a.com\nb.com", &d.hash, false).unwrap();
        assert_eq!(fs::read_to_string(root.join("lists/list-exclude-user.txt")).unwrap(), "a.com\r\nb.com\r\n");
        // Снаружи изменили файл — старый hash больше не подходит.
        fs::write(root.join("lists/list-exclude-user.txt"), "x.com\r\n").unwrap();
        assert_eq!(save(&root, "exclude", "c.com", &saved.hash, false).unwrap_err(), SaveError::Conflict);
        assert!(save(&root, "exclude", "c.com", &saved.hash, true).is_ok());
    }

    #[test]
    fn user_list_never_empty() {
        let root = temp_root("empty");
        let d = read(&root, "user").unwrap();
        save(&root, "user", "# только комментарий", &d.hash, false).unwrap();
        let text = fs::read_to_string(root.join("lists/list-general-user.txt")).unwrap();
        assert!(text.contains("domain.example.abc"));
    }

    #[test]
    fn ipset_edits_backup_when_not_loaded() {
        let root = temp_root("ipset");
        fs::write(root.join("lists/ipset-all.txt"), "203.0.113.113/32\r\n").unwrap();
        fs::write(root.join("lists/ipset-all.txt.backup"), "1.2.3.0/24\r\n").unwrap();
        let d = read(&root, "ipset").unwrap();
        assert_eq!(d.info.file, "ipset-all.txt.backup");
        assert_eq!(d.text, "1.2.3.0/24\n");
        save(&root, "ipset", "5.6.7.8\n", &d.hash, false).unwrap();
        assert_eq!(fs::read_to_string(root.join("lists/ipset-all.txt")).unwrap(), "203.0.113.113/32\r\n");
        assert_eq!(filters::ipset_mode(&root), IpsetMode::Off);
    }

    #[test]
    fn large_ipset_round_trip_is_fast() {
        let root = temp_root("large");
        let big: String =
            (0..30_000u32).map(|i| format!("{}.{}.{}.0/24\r\n", 10 + (i >> 16), (i >> 8) & 255, i & 255)).collect();
        fs::write(root.join("lists/ipset-all.txt"), &big).unwrap();
        let t0 = std::time::Instant::now();
        let d = read(&root, "ipset").unwrap();
        assert_eq!(d.text.lines().count(), 30_000);
        let edited = format!("{}9.9.9.0/24\n", d.text);
        save(&root, "ipset", &edited, &d.hash, false).unwrap();
        assert!(t0.elapsed().as_millis() < 1000, "{:?}", t0.elapsed());
        assert!(fs::read_to_string(root.join("lists/ipset-all.txt")).unwrap().ends_with("9.9.9.0/24\r\n"));
    }

    #[test]
    fn loaded_ipset_refuses_empty_save() {
        let root = temp_root("ipset-empty");
        fs::write(root.join("lists/ipset-all.txt"), "1.2.3.0/24\r\n").unwrap();
        let d = read(&root, "ipset").unwrap();
        assert!(matches!(save(&root, "ipset", "\n# пусто\n", &d.hash, false), Err(SaveError::Other(_))));
        assert_eq!(filters::ipset_mode(&root), IpsetMode::Loaded);
    }
}
