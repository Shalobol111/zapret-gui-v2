//! «Починить Discord» (Проверка → Диагностика): шаги по порядку, каждый — отдельный вызов,
//! чтобы интерфейс показывал прогресс. Подбор стратегии между шагами запускает интерфейс
//! (обычный быстрый подбор), здесь — только то, что касается самого Discord.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::diagnostics::Level;
use super::tests::curl::{self, Tok};
use crate::sys::{cmd, process};

pub struct Variant {
    pub id: &'static str,
    pub name: &'static str,
    pub exe: &'static str,
    /// Папка в %APPDATA% (настройки и кэш).
    roaming: &'static str,
    /// Папка в %LOCALAPPDATA% (Update.exe и версии).
    local: &'static str,
}

pub const VARIANTS: [Variant; 4] = [
    Variant { id: "stable", name: "Discord", exe: "Discord.exe", roaming: "discord", local: "Discord" },
    Variant { id: "ptb", name: "Discord PTB", exe: "DiscordPTB.exe", roaming: "discordptb", local: "DiscordPTB" },
    Variant {
        id: "canary",
        name: "Discord Canary",
        exe: "DiscordCanary.exe",
        roaming: "discordcanary",
        local: "DiscordCanary",
    },
    Variant {
        id: "development",
        name: "Discord Development",
        exe: "DiscordDevelopment.exe",
        roaming: "discorddevelopment",
        local: "DiscordDevelopment",
    },
];

const CACHE_DIRS: [&str; 3] = ["Cache", "Code Cache", "GPUCache"];

/// Голосовые порты Discord из стратегий Flowseal (`--filter-udp=19294-19344,50000-50100`).
pub const VOICE_PORTS: [(u16, u16); 2] = [(19294, 19344), (50000, 50100)];

/// То, без чего клиент Discord не запускается и не подключается.
pub const SITES: [&str; 4] = ["discord.com", "gateway.discord.gg", "cdn.discordapp.com", "updates.discord.com"];

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from).filter(|p| !p.as_os_str().is_empty())
}

impl Variant {
    pub fn roaming_dir(&self) -> Option<PathBuf> {
        env_dir("APPDATA").map(|p| p.join(self.roaming))
    }
    fn update_exe(&self) -> Option<PathBuf> {
        env_dir("LOCALAPPDATA").map(|p| p.join(self.local).join("Update.exe")).filter(|p| p.is_file())
    }
    /// Ярлык из меню «Пуск» (Discord кладёт его в «Discord Inc» или прямо в «Программы»).
    fn shortcut(&self) -> Option<PathBuf> {
        let programs = env_dir("APPDATA")?.join(r"Microsoft\Windows\Start Menu\Programs");
        let file = format!("{}.lnk", self.name);
        [programs.join("Discord Inc").join(&file), programs.join(&file)].into_iter().find(|p| p.is_file())
    }
    pub fn installed(&self) -> bool {
        self.update_exe().is_some() || self.roaming_dir().is_some_and(|p| p.is_dir())
    }
    pub fn running(&self) -> bool {
        process::exists(self.exe)
    }
}

pub fn installed() -> Vec<&'static Variant> {
    VARIANTS.iter().filter(|v| v.installed()).collect()
}

fn variant(id: &str) -> Option<&'static Variant> {
    VARIANTS.iter().find(|v| v.id == id)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    pub id: &'static str,
    pub name: &'static str,
    pub running: bool,
}

pub fn list() -> Vec<Installed> {
    installed().into_iter().map(|v| Installed { id: v.id, name: v.name, running: v.running() }).collect()
}

/// Закрывает запущенные клиенты; возвращает id закрытых (их потом запустим снова).
pub fn close() -> Vec<&'static str> {
    let mut closed = Vec::new();
    for v in &VARIANTS {
        if process::kill_all(v.exe) > 0 {
            closed.push(v.id);
        }
    }
    if !closed.is_empty() {
        // Electron держит файлы кэша ещё долю секунды после завершения.
        std::thread::sleep(Duration::from_millis(800));
    }
    closed
}

/// Удаляет Cache / Code Cache / GPUCache у всех установленных клиентов; возвращает число папок.
pub fn clear_cache() -> usize {
    let mut cleared = 0;
    for dir in installed().into_iter().filter_map(Variant::roaming_dir) {
        for c in CACHE_DIRS {
            let p = dir.join(c);
            if p.is_dir() && fs::remove_dir_all(&p).is_ok() {
                cleared += 1;
            }
        }
    }
    cleared
}

/// Запуск Discord без прав администратора, которые есть у программы: ярлык открывает уже
/// работающий проводник от имени пользователя. Нет ярлыка — `Update.exe --processStart`, как в нём.
pub fn launch(ids: &[String]) -> Result<Vec<&'static str>, String> {
    let mut started = Vec::new();
    for v in ids.iter().filter_map(|id| variant(id)) {
        let spawned = match v.shortcut() {
            Some(lnk) => cmd::hidden("explorer.exe").arg(lnk).spawn(),
            None => {
                let exe = v.update_exe().ok_or_else(|| format!("{}: не найден Update.exe", v.name))?;
                cmd::hidden(&exe.to_string_lossy()).args(["--processStart", v.exe]).spawn()
            }
        };
        spawned.map_err(|e| format!("{}: {e}", v.name))?;
        started.push(v.name);
    }
    Ok(started)
}

/// Диапазоны портов из всех `--filter-udp=` (значения через запятую, `a-b` или `a`).
fn udp_ranges(args: &[String]) -> Vec<(u16, u16)> {
    args.iter()
        .filter_map(|a| a.strip_prefix("--filter-udp="))
        .flat_map(|v| v.split(','))
        .filter_map(|p| {
            let p = p.trim();
            match p.split_once('-') {
                Some((a, b)) => Some((a.trim().parse().ok()?, b.trim().parse().ok()?)),
                None => p.parse().ok().map(|n| (n, n)),
            }
        })
        .collect()
}

/// Голосовые диапазоны, которые стратегия не покрывает (пусто — всё в порядке).
pub fn missing_voice_ports(args: &[String]) -> Vec<(u16, u16)> {
    let have = udp_ranges(args);
    VOICE_PORTS.iter().copied().filter(|(lo, hi)| !have.iter().any(|(a, b)| a <= lo && hi <= b)).collect()
}

pub fn format_ports(ports: &[(u16, u16)]) -> String {
    ports.iter().map(|(a, b)| format!("{a}–{b}")).collect::<Vec<_>>().join(", ")
}

/// Сайты Discord, которые не открываются (TLS 1.3 или 1.2 — достаточно одного).
pub fn failing_sites(curl_exe: &Path) -> Vec<&'static str> {
    std::thread::scope(|s| {
        let handles: Vec<_> = SITES
            .iter()
            .map(|d| {
                s.spawn(move || {
                    let toks = curl::check_parallel(curl_exe, &format!("https://{d}"), 4, &["TLS1.2", "TLS1.3"]);
                    (*d, toks.iter().any(|(_, t)| *t == Tok::Ok))
                })
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).filter(|(_, ok)| !ok).map(|(d, _)| d).collect()
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    Clean,
    Hosts,
    Voice,
    Sites,
    Launch,
}

/// Итог шага — строка в диалоге: точка, значение, пояснение.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepResult {
    pub level: Level,
    pub value: String,
    pub detail: Option<String>,
    /// Clean — какие клиенты были закрыты (их запустит шаг Launch).
    pub closed: Vec<&'static str>,
    /// Sites — сайты не открываются, нужен подбор стратегии.
    pub pick: bool,
}

impl StepResult {
    pub fn new(level: Level, value: impl Into<String>) -> Self {
        StepResult { level, value: value.into(), detail: None, closed: vec![], pick: false }
    }
    pub fn detail(mut self, d: impl Into<String>) -> Self {
        self.detail = Some(d.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn flowseal_strategy_covers_voice() {
        let a = args(&["--filter-udp=443", "--filter-udp=19294-19344,50000-50100", "--filter-l7=discord,stun"]);
        assert!(missing_voice_ports(&a).is_empty());
    }

    #[test]
    fn wide_game_filter_covers_voice() {
        assert!(missing_voice_ports(&args(&["--filter-udp=443", "--filter-udp=1024-65535"])).is_empty());
    }

    #[test]
    fn missing_ranges_are_reported() {
        let m = missing_voice_ports(&args(&["--filter-udp=443", "--filter-udp=19294-19344"]));
        assert_eq!(m, vec![(50000, 50100)]);
        assert_eq!(format_ports(&missing_voice_ports(&args(&["--filter-tcp=80"]))), "19294–19344, 50000–50100");
    }

    #[test]
    fn all_fixture_strategies_cover_voice() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/flowseal-1.10.3");
        let vars = crate::zapret::batparse::Vars { root: root.clone(), game_tcp: "12".into(), game_udp: "12".into() };
        let mut n = 0;
        for e in fs::read_dir(&root).unwrap().flatten() {
            let p = e.path();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            if !name.starts_with("general") || !name.ends_with(".bat") {
                continue;
            }
            let a = crate::zapret::batparse::parse_file(&p, &vars).unwrap();
            assert!(missing_voice_ports(&a).is_empty(), "{name}");
            n += 1;
        }
        assert!(n > 10);
    }

    /// Живая проверка: `cargo test -- --ignored live_discord --nocapture`
    #[test]
    #[ignore]
    fn live_discord() {
        for c in list() {
            let v = variant(c.id).unwrap();
            println!("{} ({}) running={} shortcut={:?}", c.name, c.id, c.running, v.shortcut());
        }
        let curl = curl::curl_path().expect("curl.exe");
        println!("не открываются: {:?}", failing_sites(&curl));
        if let Some(p) = process::find("winws.exe").first() {
            println!("winws: аргументов {}, нет голосовых портов: {:?}", p.args.len(), missing_voice_ports(&p.args));
        }
    }
}
