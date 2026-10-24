//! Telegram: локальный MTProto-прокси TgWsProxy (Flowseal/tg-ws-proxy).
//!
//! Управляем официальным `TgWsProxy_windows.exe` в портативном режиме: рядом с exe лежит
//! папка `TgWsProxy_data` — там его config.json и лог, поэтому наша копия не мешает копии,
//! установленной отдельно (`%APPDATA%\TgWsProxy`). Конфиг пишем сами (порт, секрет, DC),
//! собственную проверку обновлений и окно первого запуска TgWsProxy отключаем — обновления
//! и инструкция в ZapretGUI. У программы свой значок в трее, скрыть его нельзя.
//!
//! Запуск — через проводник (`explorer.exe <exe>`): у ZapretGUI права администратора, а прокси
//! и открываемому им Telegram они не нужны.

use std::fs;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tauri::{AppHandle, Manager};

use crate::journal;
use crate::state::AppState;
use crate::sys::{cmd, process};
use crate::zapret::updater;

const API_LATEST: &str = "https://api.github.com/repos/Flowseal/tg-ws-proxy/releases/latest";
pub const RELEASES_URL: &str = "https://github.com/Flowseal/tg-ws-proxy/releases";
const ASSET: &str = "TgWsProxy_windows.exe";
/// Имена exe из релизов Flowseal — по ним узнаём копию, запущенную не из ZapretGUI.
const KNOWN_EXES: [&str; 5] = [
    "TgWsProxy_windows.exe",
    "TgWsProxy_windows_arm64.exe",
    "TgWsProxy_windows_7_64bit.exe",
    "TgWsProxy_windows_7_32bit.exe",
    "TgWsProxy.exe",
];
pub const DEFAULT_PORT: u16 = 1443;
pub const DEFAULT_DC: [&str; 2] = ["2:149.154.167.220", "4:149.154.167.220"];

/// Папка TgWsProxy. Путь нормализуется к обратным слэшам: `explorer.exe` не запускает
/// `C:/…`, а пути процессов Windows отдаёт только с `\`.
pub fn dir() -> PathBuf {
    let p = crate::settings::data_dir().join("tg-ws-proxy");
    PathBuf::from(p.to_string_lossy().replace('/', "\\"))
}
fn exe() -> PathBuf {
    dir().join(ASSET)
}
fn data_dir() -> PathBuf {
    dir().join("TgWsProxy_data")
}
fn config_file() -> PathBuf {
    data_dir().join("config.json")
}
pub fn log_file() -> PathBuf {
    data_dir().join("proxy.log")
}
fn version_file() -> PathBuf {
    dir().join("version.txt")
}

pub fn installed() -> bool {
    exe().is_file()
}

pub fn version() -> Option<String> {
    fs::read_to_string(version_file()).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

// ── Конфиг ─────────────────────────────────────────────────────

/// Настройки, которые меняются из ZapretGUI. Остальные ключи config.json не трогаем.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub port: u16,
    pub secret: String,
    /// «DC:IP» по строке, как в настройках TgWsProxy.
    pub dc_ip: Vec<String>,
    /// Запасной путь через Cloudflare, если WebSocket к Telegram недоступен.
    pub cfproxy: bool,
}

fn new_secret() -> String {
    let mut b = [0u8; 16];
    if getrandom::fill(&mut b).is_err() {
        // Не должно случаться; на всякий случай — время и PID, лишь бы не пустой секрет.
        let seed = crate::zapret::runner::now_ms() ^ ((std::process::id() as u64) << 32);
        b[..8].copy_from_slice(&seed.to_le_bytes());
        b[8..].copy_from_slice(&seed.rotate_left(29).to_le_bytes());
    }
    crate::net::hex(&b)
}

pub fn valid_secret(s: &str) -> bool {
    s.len() == 32 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// «2:149.154.167.220» — номер DC и IPv4.
pub fn valid_dc(s: &str) -> bool {
    s.split_once(':').is_some_and(|(dc, ip)| dc.parse::<u8>().is_ok() && ip.parse::<std::net::Ipv4Addr>().is_ok())
}

fn read_raw() -> Map<String, Value> {
    fs::read_to_string(config_file())
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

fn write_raw(map: &Map<String, Value>) -> Result<(), String> {
    fs::create_dir_all(data_dir()).map_err(|e| format!("не удалось создать папку TgWsProxy: {e}"))?;
    let text = serde_json::to_string_pretty(map).map_err(|e| e.to_string())?;
    fs::write(config_file(), text).map_err(|e| format!("не удалось записать config.json: {e}"))
}

fn from_raw(m: &Map<String, Value>) -> Config {
    Config {
        port: m.get("port").and_then(Value::as_u64).and_then(|p| u16::try_from(p).ok()).unwrap_or(DEFAULT_PORT),
        secret: m.get("secret").and_then(Value::as_str).unwrap_or_default().to_string(),
        dc_ip: m
            .get("dc_ip")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_else(|| DEFAULT_DC.iter().map(|s| s.to_string()).collect()),
        cfproxy: m.get("cfproxy").and_then(Value::as_bool).unwrap_or(true),
    }
}

/// Конфиг с нашими обязательными ключами: локальный адрес, секрет, без автообновления и
/// автозапуска самого TgWsProxy (этим управляет ZapretGUI). Окно первого запуска пропускаем.
pub fn ensure_config() -> Result<Config, String> {
    let mut m = read_raw();
    let fresh = m.is_empty();
    let mut set = |k: &str, v: Value| {
        if m.get(k) != Some(&v) {
            m.insert(k.to_string(), v);
        }
    };
    set("host", json!("127.0.0.1"));
    set("check_updates", json!(false));
    set("autostart", json!(false));
    if fresh {
        m.insert("port".into(), json!(DEFAULT_PORT));
        m.insert("dc_ip".into(), json!(DEFAULT_DC));
        m.insert("cfproxy".into(), json!(true));
        m.insert("language".into(), json!("ru"));
    }
    if !m.get("secret").and_then(Value::as_str).is_some_and(valid_secret) {
        m.insert("secret".into(), json!(new_secret()));
    }
    write_raw(&m)?;
    let marker = data_dir().join(".first_run_done_mtproto");
    if !marker.exists() {
        let _ = fs::write(marker, "");
    }
    Ok(from_raw(&m))
}

pub fn config() -> Config {
    let m = read_raw();
    if m.is_empty() {
        return Config {
            port: DEFAULT_PORT,
            secret: String::new(),
            dc_ip: DEFAULT_DC.iter().map(|s| s.to_string()).collect(),
            cfproxy: true,
        };
    }
    from_raw(&m)
}

pub fn save_config(c: &Config) -> Result<Config, String> {
    if c.port < 1024 {
        return Err("порт должен быть от 1024 до 65535".into());
    }
    if !valid_secret(&c.secret) {
        return Err("секрет — 32 шестнадцатеричных символа".into());
    }
    if let Some(bad) = c.dc_ip.iter().find(|d| !valid_dc(d)) {
        return Err(format!("«{bad}» — не в формате DC:IP, например 2:149.154.167.220"));
    }
    ensure_config()?;
    let mut m = read_raw();
    m.insert("port".into(), json!(c.port));
    m.insert("secret".into(), json!(c.secret.to_lowercase()));
    m.insert("dc_ip".into(), json!(c.dc_ip));
    m.insert("cfproxy".into(), json!(c.cfproxy));
    write_raw(&m)?;
    Ok(from_raw(&m))
}

pub fn regenerate_secret() -> String {
    new_secret()
}

/// Ссылка, которую понимает Telegram Desktop (как «Скопировать ссылку» в TgWsProxy).
pub fn link(c: &Config) -> String {
    format!("tg://proxy?server=127.0.0.1&port={}&secret=dd{}", c.port, c.secret)
}

// ── Процессы ───────────────────────────────────────────────────

fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().replace('/', "\\").trim_start_matches(r"\\?\").to_lowercase();
    norm(a) == norm(b)
}

/// PID нашей копии (onefile-сборка — два процесса: распаковщик и сам прокси).
fn ours() -> Vec<process::ProcInfo> {
    let mine = exe();
    process::find(ASSET).into_iter().filter(|p| p.exe.as_deref().is_some_and(|e| same_path(e, &mine))).collect()
}

/// TgWsProxy, запущенный не из ZapretGUI (путь к exe, если известен).
pub fn external() -> Option<String> {
    let mine = exe();
    process::find_any(&KNOWN_EXES)
        .into_iter()
        .find(|p| !p.exe.as_deref().is_some_and(|e| same_path(e, &mine)))
        .map(|p| p.exe.map(|e| e.display().to_string()).unwrap_or_else(|| "TgWsProxy".into()))
}

pub fn listening(port: u16) -> bool {
    TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(250)).is_ok()
}

// ── Состояние для интерфейса (часть Status) ────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub installed: bool,
    pub version: Option<String>,
    /// Последняя версия на GitHub (после проверки).
    pub latest: Option<String>,
    pub running: bool,
    /// Порт уже принимает подключения — Telegram можно подключать.
    pub listening: bool,
    pub port: u16,
    pub started_at: Option<u64>,
    /// Копия TgWsProxy, запущенная отдельно (путь).
    pub external: Option<String>,
    /// «Скачивание…», «Перезапуск…» — идёт действие.
    pub busy: Option<String>,
    pub progress: Option<u8>,
}

/// Что меняется не из файлов: последняя версия и текущее действие.
#[derive(Debug, Clone, Default)]
pub struct Shared {
    pub latest: Option<String>,
    pub busy: Option<String>,
    pub progress: Option<u8>,
}

pub fn status(shared: &Shared) -> Status {
    let installed = installed();
    let procs = if installed { ours() } else { vec![] };
    let port = config().port;
    let running = !procs.is_empty();
    Status {
        installed,
        version: version(),
        latest: shared.latest.clone(),
        running,
        listening: running && listening(port),
        port,
        started_at: procs.iter().map(|p| p.start_time * 1000).min(),
        external: external(),
        busy: shared.busy.clone(),
        progress: shared.progress,
    }
}

fn set_busy(app: &AppHandle, busy: Option<&str>, progress: Option<u8>) {
    let st = app.state::<AppState>();
    {
        let mut s = st.tg.lock().unwrap();
        s.busy = busy.map(str::to_string);
        s.progress = progress;
    }
    st.poke();
}

// ── Запуск и остановка ─────────────────────────────────────────

pub fn start(is_admin: bool) -> Result<(), String> {
    if !installed() {
        return Err("TgWsProxy не скачан".into());
    }
    if let Some(path) = external() {
        return Err(format!(
            "TgWsProxy уже запущен отдельно ({path}). Закройте его или нажмите «Закрыть и запустить здесь»"
        ));
    }
    if !ours().is_empty() {
        return Ok(());
    }
    let cfg = ensure_config()?;
    if listening(cfg.port) {
        return Err(format!("порт {} уже занят другой программой — смените порт в настройках прокси", cfg.port));
    }
    // С правами администратора — через проводник, чтобы прокси (и Telegram, который он может
    // открыть из своего меню) работали с обычными правами.
    let spawned = if is_admin {
        cmd::hidden("explorer.exe").arg(exe()).spawn()
    } else {
        std::process::Command::new(exe()).current_dir(dir()).spawn()
    };
    spawned.map_err(|e| format!("не удалось запустить TgWsProxy: {e}"))?;
    // Onefile-сборка распаковывается несколько секунд — ждём процесс (порт догонит статус).
    let t0 = Instant::now();
    while ours().is_empty() {
        if t0.elapsed() > Duration::from_secs(10) {
            return Err("TgWsProxy не запустился за 10 с — возможно, его заблокировал антивирус".into());
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    journal::ok(format!("Telegram-прокси запущен · 127.0.0.1:{}", cfg.port));
    Ok(())
}

pub fn stop() -> Result<(), String> {
    let procs = ours();
    if procs.is_empty() {
        return Ok(());
    }
    for p in &procs {
        process::kill_pid(p.pid);
    }
    let t0 = Instant::now();
    while !ours().is_empty() {
        if t0.elapsed() > Duration::from_secs(5) {
            return Err("TgWsProxy не завершился".into());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    journal::info("Telegram-прокси остановлен");
    Ok(())
}

pub fn restart(is_admin: bool) -> Result<(), String> {
    stop()?;
    std::thread::sleep(Duration::from_millis(700));
    start(is_admin)
}

/// Закрыть отдельно запущенный TgWsProxy (перед запуском нашей копии).
pub fn close_external() -> Result<(), String> {
    let mine = exe();
    let mut n = 0;
    for p in process::find_any(&KNOWN_EXES) {
        if !p.exe.as_deref().is_some_and(|e| same_path(e, &mine)) && process::kill_pid(p.pid) {
            n += 1;
        }
    }
    if n == 0 {
        return Err("не удалось закрыть TgWsProxy — закройте его из значка в трее".into());
    }
    std::thread::sleep(Duration::from_millis(600));
    journal::info("Отдельно запущенный TgWsProxy закрыт");
    Ok(())
}

/// Открыть ссылку tg://proxy — Telegram Desktop предложит подключить прокси.
pub fn connect_telegram() -> Result<(), String> {
    let has_handler =
        winreg::RegKey::predef(winreg::enums::HKEY_CLASSES_ROOT).open_subkey(r"tg\shell\open\command").is_ok();
    if !has_handler {
        return Err("Telegram Desktop не найден (ссылки tg:// не открываются). Установите его с desktop.telegram.org \
             или скопируйте ссылку и откройте её в Telegram"
            .into());
    }
    let cfg = ensure_config()?;
    // Через проводник: Telegram откроется с обычными правами.
    cmd::hidden("explorer.exe").arg(link(&cfg)).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

// ── Скачивание и обновление ────────────────────────────────────

pub struct Release {
    pub version: String,
    url: String,
    sha256: Option<String>,
    size: u64,
}

pub async fn latest() -> Result<Release, String> {
    let text = crate::net::get_text(API_LATEST, Duration::from_secs(15)).await?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("неверный ответ GitHub: {e}"))?;
    parse_release(&v).ok_or_else(|| format!("в последнем релизе нет {ASSET}"))
}

fn parse_release(v: &Value) -> Option<Release> {
    let version = v.get("tag_name")?.as_str()?.trim_start_matches('v').to_string();
    let asset = v.get("assets")?.as_array()?.iter().find(|a| a.get("name").and_then(Value::as_str) == Some(ASSET))?;
    Some(Release {
        version,
        url: asset.get("browser_download_url")?.as_str()?.to_string(),
        sha256: asset
            .get("digest")
            .and_then(Value::as_str)
            .and_then(|d| d.strip_prefix("sha256:"))
            .map(str::to_lowercase),
        size: asset.get("size").and_then(Value::as_u64).unwrap_or(0),
    })
}

/// Есть ли версия новее установленной (для подписи «доступна 1.12.0»).
pub async fn check(app: &AppHandle) -> Result<Option<String>, String> {
    let r = latest().await?;
    let newer = match version() {
        Some(cur) => updater::compare(&r.version, &cur) == Some(std::cmp::Ordering::Greater),
        None => true,
    };
    let latest = newer.then(|| r.version.clone());
    app.state::<AppState>().tg.lock().unwrap().latest = latest.clone();
    app.state::<AppState>().poke();
    Ok(latest)
}

/// Скачать последнюю версию (первая установка или обновление) с проверкой SHA-256.
pub async fn install(app: AppHandle) -> Result<String, String> {
    set_busy(&app, Some("Получение ссылки на релиз"), None);
    let r = match latest().await {
        Ok(r) => r,
        Err(e) => {
            set_busy(&app, None, None);
            return Err(e);
        }
    };
    let result = download_and_replace(&app, &r).await;
    set_busy(&app, None, None);
    match &result {
        Ok(_) => {
            app.state::<AppState>().tg.lock().unwrap().latest = None;
            journal::ok(format!("TgWsProxy {} установлен", r.version));
        }
        Err(e) => journal::err(format!("TgWsProxy: {e}")),
    }
    app.state::<AppState>().poke();
    result
}

async fn download_and_replace(app: &AppHandle, r: &Release) -> Result<String, String> {
    let a = app.clone();
    let tmp = fetch(r, move |pct| set_busy(&a, Some("Скачивание TgWsProxy"), Some(pct))).await?;
    let is_admin = app.state::<AppState>().is_admin;
    let was_running = !ours().is_empty();
    if was_running {
        set_busy(app, Some("Перезапуск"), None);
        stop()?;
        std::thread::sleep(Duration::from_millis(500));
    }
    replace_exe(&tmp, &r.version)?;
    if was_running {
        start(is_admin)?;
    }
    Ok(format!("TgWsProxy {} установлен", r.version))
}

/// Скачивает exe рядом с текущим (`.download`) и сверяет SHA-256 из релиза.
async fn fetch(r: &Release, mut progress: impl FnMut(u8)) -> Result<PathBuf, String> {
    fs::create_dir_all(dir()).map_err(|e| format!("не удалось создать папку: {e}"))?;
    let tmp = dir().join(format!("{ASSET}.download"));
    let size = r.size.max(1);
    let mut last = 255u8;
    let hash = crate::net::download(&r.url, &tmp, |got, total| {
        let pct = (got * 100 / total.unwrap_or(size).max(1)).min(100) as u8;
        if pct != last {
            last = pct;
            progress(pct);
        }
    })
    .await
    .inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })?;
    if let Some(want) = &r.sha256 {
        if &hash != want {
            let _ = fs::remove_file(&tmp);
            return Err("контрольная сумма не совпала — файл повреждён, попробуйте ещё раз".into());
        }
    }
    Ok(tmp)
}

fn replace_exe(tmp: &Path, version: &str) -> Result<(), String> {
    fs::rename(tmp, exe()).map_err(|e| {
        let _ = fs::remove_file(tmp);
        format!("не удалось заменить exe: {e}")
    })?;
    let _ = fs::write(version_file(), version);
    ensure_config()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_and_dc_are_validated() {
        let s = new_secret();
        assert!(valid_secret(&s), "{s}");
        assert_ne!(s, new_secret());
        assert!(!valid_secret("abc"));
        assert!(valid_dc("2:149.154.167.220"));
        assert!(!valid_dc("2-149.154.167.220"));
        assert!(!valid_dc("x:1.2.3.4"));
    }

    #[test]
    fn link_matches_tgwsproxy() {
        let c = Config { port: 1443, secret: "ab".repeat(16), dc_ip: vec![], cfproxy: true };
        assert_eq!(link(&c), format!("tg://proxy?server=127.0.0.1&port=1443&secret=dd{}", "ab".repeat(16)));
    }

    #[test]
    fn release_json_is_parsed() {
        let v = json!({
            "tag_name": "v1.11.2",
            "assets": [
                { "name": "TgWsProxy_windows_arm64.exe", "browser_download_url": "x", "size": 1 },
                { "name": "TgWsProxy_windows.exe", "browser_download_url": "https://example/TgWsProxy_windows.exe",
                  "size": 21797942, "digest": "sha256:54ACAA" }
            ]
        });
        let r = parse_release(&v).unwrap();
        assert_eq!(r.version, "1.11.2");
        assert_eq!(r.url, "https://example/TgWsProxy_windows.exe");
        assert_eq!(r.sha256.as_deref(), Some("54acaa"));
        assert_eq!(r.size, 21797942);
    }

    /// Сетевой: `cargo test -- --ignored live_tg_release --nocapture`
    #[test]
    #[ignore]
    fn live_tg_release() {
        let r = tauri::async_runtime::block_on(latest()).expect("релиз TgWsProxy");
        println!("{} {} {:?} {}", r.version, r.url, r.sha256, r.size);
        assert!(r.sha256.is_some());
    }

    /// Полный цикл на настоящем TgWsProxy (скачивание ~22 МБ, запуск, порт, перезапуск, остановка):
    /// `ZAPRETGUI_DATA_DIR=<временная папка> cargo test -- --ignored live_tg_cycle --nocapture`
    #[test]
    #[ignore]
    fn live_tg_cycle() {
        assert!(
            std::env::var_os("ZAPRETGUI_DATA_DIR").is_some(),
            "задайте ZAPRETGUI_DATA_DIR — не трогаем рабочие данные"
        );
        if !installed() {
            let r = tauri::async_runtime::block_on(latest()).expect("релиз");
            let tmp = tauri::async_runtime::block_on(fetch(&r, |p| {
                if p % 25 == 0 {
                    println!("скачано {p}%")
                }
            }))
            .expect("скачивание и SHA-256");
            replace_exe(&tmp, &r.version).expect("установка");
        }
        println!("версия {:?}, внешний TgWsProxy: {:?}", version(), external());
        let wait_port = |port: u16| {
            let t0 = Instant::now();
            while !listening(port) {
                assert!(t0.elapsed() < Duration::from_secs(25), "порт {port} не открылся");
                std::thread::sleep(Duration::from_millis(300));
            }
            println!("порт {port} открыт за {:.1} с", t0.elapsed().as_secs_f32());
        };
        start(true).expect("запуск через проводник");
        println!("процессы: {:?}", ours().iter().map(|p| p.pid).collect::<Vec<_>>());
        wait_port(config().port);
        let mut c = config();
        println!("ссылка: {}", link(&c));
        c.port = 1444;
        save_config(&c).expect("сохранение");
        restart(true).expect("перезапуск");
        wait_port(1444);
        assert!(!listening(1443) || external().is_some(), "старый порт должен закрыться");
        c.port = DEFAULT_PORT;
        save_config(&c).unwrap();
        stop().expect("остановка");
        assert!(ours().is_empty());
        assert!(!listening(1444));
        println!("остановлен, лог: {}", log_file().display());
    }
}
