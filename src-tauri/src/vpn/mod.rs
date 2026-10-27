//! VPN через sing-box вместе с zapret (PLAN.md §17): сайты из VPN-списков — через VPN-сервер,
//! остальное напрямую (там работает zapret). Подписки и ключи, маршрутизация, системный прокси или TUN.

pub mod config;
pub mod links;
pub mod lists;
pub mod singbox;
pub mod subscription;
pub mod sysproxy;
pub mod xray;

use std::collections::HashMap;
use std::fs;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::journal;
use crate::state::AppState;
use links::Server;
use subscription::UserInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Inbound {
    /// Системный прокси Windows (127.0.0.1:port) — без драйвера, только программы, уважающие прокси.
    #[default]
    Proxy,
    /// Виртуальный адаптер (wintun) — весь трафик всех программ.
    Tun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RouteMode {
    /// Готовые наборы и свой список — через VPN, остальное напрямую (zapret).
    #[default]
    Lists,
    /// Всё через VPN, кроме российских сайтов.
    AllExceptRu,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    pub id: String,
    pub url: String,
    pub name: String,
    pub updated_at: Option<u64>,
    pub error: Option<String>,
    pub info: Option<UserInfo>,
    pub servers: Vec<Server>,
}

/// `%APPDATA%\ZapretGUI\vpn\vpn.json` — подписки, ключи и настройки VPN.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Store {
    pub subscriptions: Vec<Subscription>,
    /// Серверы из одиночных ссылок.
    pub manual: Vec<Server>,
    /// id выбранного сервера; `None` — автоматически самый быстрый.
    pub selected: Option<String>,
    pub route_mode: RouteMode,
    pub inbound: Inbound,
    pub presets: Vec<String>,
    /// Сайты из списков zapret — напрямую через zapret.
    pub zapret_direct: bool,
    pub port: u16,
    /// Включать VPN при запуске ZapretGUI.
    pub autostart: bool,
    /// Системный прокси до нашего включения (пока он наш) — вернуть после остановки или сбоя.
    pub saved_proxy: Option<sysproxy::Saved>,
}

impl Default for Store {
    fn default() -> Self {
        Store {
            subscriptions: vec![],
            manual: vec![],
            selected: None,
            route_mode: RouteMode::Lists,
            inbound: Inbound::Proxy,
            presets: vec!["blocked".into(), "ai".into()],
            zapret_direct: true,
            port: 2080,
            autostart: false,
            saved_proxy: None,
        }
    }
}

/// Папка VPN (обратные слэши — пути уходят в конфиг sing-box и в проводник).
pub fn dir() -> PathBuf {
    let p = crate::settings::data_dir().join("vpn");
    PathBuf::from(p.to_string_lossy().replace('/', "\\"))
}
fn store_file() -> PathBuf {
    dir().join("vpn.json")
}
fn config_file() -> PathBuf {
    dir().join("config.json")
}
pub fn log_file() -> PathBuf {
    dir().join("sing-box.log")
}

pub fn load() -> Store {
    fs::read_to_string(store_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save(s: &Store) -> Result<(), String> {
    fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    fs::write(store_file(), text).map_err(|e| format!("не удалось сохранить настройки VPN: {e}"))
}

pub fn servers(s: &Store) -> Vec<&Server> {
    s.subscriptions.iter().flat_map(|sub| sub.servers.iter()).chain(s.manual.iter()).collect()
}

// ── Состояние для интерфейса ───────────────────────────────────

/// Что меняется не из файлов: процесс, действие, задержки серверов.
#[derive(Default)]
pub struct Runtime {
    pub child: Option<Child>,
    /// Xray — если выбранные серверы работают через него.
    pub xray: Option<Child>,
    pub started_at: Option<u64>,
    pub busy: Option<String>,
    pub progress: Option<u8>,
    pub error: Option<String>,
    /// Новая версия sing-box на GitHub.
    pub latest: Option<String>,
    pub pings: HashMap<String, Option<u32>>,
    /// Сервер, с которым запущено (или «Автоматически»).
    pub server_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub installed: bool,
    pub version: Option<String>,
    pub latest: Option<String>,
    pub running: bool,
    /// Прокси принимает подключения (в режиме TUN — процесс жив).
    pub ready: bool,
    pub inbound: Option<Inbound>,
    pub route_mode: Option<RouteMode>,
    pub server: Option<String>,
    pub port: u16,
    pub started_at: Option<u64>,
    pub busy: Option<String>,
    pub progress: Option<u8>,
    pub error: Option<String>,
    pub servers: usize,
    pub xray_installed: bool,
    pub xray_version: Option<String>,
    /// Работает ли VPN через Xray (выбранные серверы — VLESS/VMess/Trojan/Shadowsocks).
    pub via_xray: bool,
}

fn listening(port: u16) -> bool {
    TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(200)).is_ok()
}

pub fn status(rt: &mut Runtime, store: &Store) -> Status {
    // Xray упал — без него трафик «через VPN» не идёт: останавливаем и sing-box, возвращаем прокси.
    let xray_dead = rt.xray.as_mut().is_some_and(|c| !matches!(c.try_wait(), Ok(None)));
    if xray_dead {
        rt.xray = None;
        if let Some(mut c) = rt.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        rt.started_at = None;
        rt.error = Some(xray_error().unwrap_or_else(|| "Xray неожиданно завершился".into()));
        restore_proxy();
    }
    let alive = rt.child.as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None)));
    if !alive && rt.child.is_some() {
        rt.child = None;
        rt.started_at = None;
        if let Some(mut x) = rt.xray.take() {
            let _ = x.kill();
        }
        restore_proxy();
        if rt.error.is_none() {
            rt.error = Some(last_error().unwrap_or_else(|| "sing-box неожиданно завершился".into()));
        }
    }
    Status {
        installed: singbox::installed(),
        version: singbox::version(),
        latest: rt.latest.clone(),
        running: alive,
        ready: alive && (store.inbound == Inbound::Tun || listening(store.port)),
        inbound: alive.then_some(store.inbound),
        route_mode: alive.then_some(store.route_mode),
        server: alive.then(|| rt.server_name.clone()).flatten(),
        port: store.port,
        started_at: rt.started_at,
        busy: rt.busy.clone(),
        progress: rt.progress,
        error: rt.error.clone(),
        servers: servers(store).len(),
        xray_installed: xray::installed(),
        xray_version: xray::version(),
        via_xray: alive && rt.xray.is_some(),
    }
}

/// Последняя ошибка из лога Xray.
fn xray_error() -> Option<String> {
    let text = fs::read_to_string(xray_log_file()).ok()?;
    text.lines().rev().find(|l| l.contains("[Error]") || l.contains("Failed")).map(|l| l.chars().take(300).collect())
}

/// Последняя ошибка из лога sing-box (для понятного сообщения о сбое).
fn last_error() -> Option<String> {
    let text = fs::read_to_string(log_file()).ok()?;
    text.lines()
        .rev()
        .find(|l| l.contains("FATAL") || l.contains("ERROR"))
        .map(|l| l.split_once(']').map(|(_, r)| r).unwrap_or(l).trim().chars().take(300).collect())
}

// ── Запуск и остановка ─────────────────────────────────────────

/// Кто соединяется с VPN-сервером.
#[derive(Debug)]
pub enum Engine {
    /// Xray (VLESS/VMess/Trojan/Shadowsocks): один сервер или автовыбор быстрейшего (leastPing).
    Xray(Vec<Server>),
    /// sing-box напрямую (Hysteria2, TUIC, серверы из JSON-подписки): один или urltest.
    SingBox(Vec<Value>),
}

/// Порт SOCKS, на котором Xray ждёт трафик от sing-box.
pub fn xray_port(store: &Store) -> u16 {
    store.port.wrapping_add(1).max(1025)
}

/// Выход «proxy»: выбранный сервер или автовыбор быстрейшего.
pub fn chosen(store: &Store) -> Result<(Engine, String), String> {
    let all = servers(store);
    let usable: Vec<&Server> = all.iter().copied().filter(|s| s.usable()).collect();
    if usable.is_empty() {
        return Err(if all.is_empty() {
            "добавьте подписку или ключ VPN".into()
        } else {
            "ни один сервер не поддерживается".into()
        });
    }
    if let Some(id) = &store.selected {
        if let Some(s) = usable.iter().find(|s| &s.id == id) {
            let engine = if s.via_xray() {
                Engine::Xray(vec![(*s).clone()])
            } else {
                Engine::SingBox(vec![s.outbound.clone().unwrap()])
            };
            return Ok((engine, s.name.clone()));
        }
    }
    // Автовыбор: не больше 30 серверов — их проверяют по очереди. Предпочтение — серверам Xray.
    let auto = "Автоматически (быстрейший)".to_string();
    let xr: Vec<Server> = usable.iter().filter(|s| s.via_xray()).take(30).map(|s| (*s).clone()).collect();
    if !xr.is_empty() {
        return Ok((Engine::Xray(xr), auto));
    }
    Ok((Engine::SingBox(usable.iter().filter_map(|s| s.outbound.clone()).take(30).collect()), auto))
}

fn xray_config_file() -> PathBuf {
    dir().join("xray-config.json")
}
pub fn xray_log_file() -> PathBuf {
    dir().join("xray.log")
}

/// Собрать конфиги (sing-box и, если нужен, Xray) и проверить их ядрами. Возвращает имя сервера
/// и признак «нужен Xray».
pub fn write_config(store: &Store, zapret_root: Option<&Path>) -> Result<(String, bool), String> {
    lists::ensure_custom();
    fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let (engine, name) = chosen(store)?;
    let (servers, use_xray) = match engine {
        Engine::SingBox(v) => (v, false),
        Engine::Xray(list) => {
            if !xray::installed() {
                return Err("для этих серверов нужен Xray — нажмите «Скачать ядра»".into());
            }
            let refs: Vec<&Server> = list.iter().collect();
            let xcfg = xray::config(&refs, xray_port(store), &xray_log_file().display().to_string())?;
            fs::write(xray_config_file(), serde_json::to_string_pretty(&xcfg).map_err(|e| e.to_string())?)
                .map_err(|e| format!("не удалось записать конфиг Xray: {e}"))?;
            xray::check(&xray_config_file()).map_err(|e| format!("Xray не принял настройки: {e}"))?;
            let socks = serde_json::json!({ "type": "socks", "server": "127.0.0.1", "server_port": xray_port(store), "version": "5" });
            (vec![socks], true)
        }
    };
    let input = config::Input {
        inbound: store.inbound,
        route_mode: store.route_mode,
        port: store.port,
        servers,
        vpn_custom: lists::custom(lists::VPN_FILE),
        direct_custom: lists::custom(lists::DIRECT_FILE),
        zapret_direct: if store.zapret_direct { lists::zapret_domains(zapret_root) } else { vec![] },
        vpn_presets: store.presets.iter().filter_map(|id| lists::preset(id)).flat_map(lists::domains).collect(),
        ru_direct: lists::domains(&lists::RU_DIRECT),
        log_file: log_file().display().to_string(),
    };
    let cfg = config::build(&input);
    fs::write(config_file(), serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?)
        .map_err(|e| format!("не удалось записать конфиг: {e}"))?;
    singbox::check(&config_file()).map_err(|e| format!("sing-box не принял настройки: {e}"))?;
    Ok((name, use_xray))
}

/// IP-адреса VPN-серверов → исключения zapret (`ipset-exclude-user.txt`, блок между маркерами),
/// иначе winws (особенно с IPSet «Любой») искажает VPN-соединение.
pub fn exclude_from_zapret(root: &Path, store: &Store) -> Result<usize, String> {
    const BEGIN: &str = "# zapret-gui vpn begin — адреса VPN-серверов (обновляется программой)";
    const END: &str = "# zapret-gui vpn end";
    let mut ips: Vec<String> = Vec::new();
    for s in servers(store) {
        if let Ok(addrs) = (s.host.as_str(), s.port).to_socket_addrs() {
            for a in addrs {
                let ip = a.ip();
                ips.push(if ip.is_ipv4() { format!("{ip}/32") } else { format!("{ip}/128") });
            }
        }
    }
    ips.sort();
    ips.dedup();
    let path = root.join("lists").join("ipset-exclude-user.txt");
    let current = fs::read_to_string(&path).unwrap_or_default();
    let mut out: Vec<&str> = Vec::new();
    let mut inside = false;
    for line in current.lines() {
        let t = line.trim();
        if t.starts_with("# zapret-gui vpn begin") {
            inside = true;
            continue;
        }
        if inside {
            if t == END {
                inside = false;
            }
            continue;
        }
        out.push(line);
    }
    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    let mut text = out.join("\r\n");
    if !ips.is_empty() {
        if !text.is_empty() {
            text.push_str("\r\n");
        }
        text.push_str(BEGIN);
        for ip in &ips {
            text.push_str("\r\n");
            text.push_str(ip);
        }
        text.push_str("\r\n");
        text.push_str(END);
    }
    text.push_str("\r\n");
    if text != current {
        fs::write(&path, text).map_err(|e| format!("не удалось обновить исключения zapret: {e}"))?;
    }
    Ok(ips.len())
}

/// Убить sing-box из нашей папки, оставшийся от прошлого запуска (сбой, обновление).
fn kill_orphans() {
    let norm = |p: &Path| p.to_string_lossy().to_lowercase();
    // Только наши копии (по пути exe) — Xray из Happ/v2rayN не трогаем.
    for (name, mine) in [("sing-box.exe", singbox::exe()), ("xray.exe", xray::exe())] {
        for p in crate::sys::process::find(name) {
            if p.exe.as_deref().is_some_and(|e| norm(e) == norm(&mine)) {
                crate::sys::process::kill_pid(p.pid);
            }
        }
    }
}

pub fn start(st: &AppState) -> Result<(), String> {
    if !singbox::installed() {
        return Err("sing-box не скачан".into());
    }
    stop(st)?;
    let mut store = load();
    let root = st.root();
    let (name, use_xray) = write_config(&store, root.as_deref())?;
    if let Some(r) = root.as_deref() {
        match exclude_from_zapret(r, &store) {
            Ok(n) if n > 0 => journal::info(format!("VPN: адреса серверов ({n}) добавлены в исключения zapret")),
            Ok(_) => {}
            Err(e) => journal::warn(format!("VPN: {e}")),
        }
    }
    if store.inbound == Inbound::Proxy && listening(store.port) {
        return Err(format!("порт {} уже занят другой программой — смените порт в настройках VPN", store.port));
    }
    let _ = fs::remove_file(log_file());
    // Сначала Xray (если нужен): sing-box отдаёт ему трафик «через VPN» по SOCKS.
    let mut xray_child = None;
    if use_xray {
        let port = xray_port(&store);
        if listening(port) {
            return Err(format!("порт {port} (для Xray) уже занят — смените порт VPN в настройках"));
        }
        let _ = fs::remove_file(xray_log_file());
        let mut x = xray::spawn(&xray_config_file())?;
        let t0 = Instant::now();
        while !listening(port) {
            if let Ok(Some(_)) = x.try_wait() {
                return Err(xray_error().unwrap_or_else(|| "Xray завершился сразу после запуска".into()));
            }
            if t0.elapsed() > Duration::from_secs(10) {
                let _ = x.kill();
                return Err("Xray не начал работу за 10 с".into());
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        xray_child = Some(x);
    }
    let kill_xray = |x: &mut Option<Child>| {
        if let Some(c) = x.as_mut() {
            let _ = c.kill();
        }
    };
    let mut child = match singbox::spawn(&config_file()) {
        Ok(c) => c,
        Err(e) => {
            kill_xray(&mut xray_child);
            return Err(e);
        }
    };
    // Ждём порт (прокси) или просто убеждаемся, что процесс не упал сразу (TUN).
    let t0 = Instant::now();
    loop {
        if let Ok(Some(_)) = child.try_wait() {
            kill_xray(&mut xray_child);
            return Err(last_error().unwrap_or_else(|| "sing-box завершился сразу после запуска".into()));
        }
        let ok = match store.inbound {
            Inbound::Proxy => listening(store.port),
            Inbound::Tun => t0.elapsed() > Duration::from_millis(2500),
        };
        if ok {
            break;
        }
        if t0.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            kill_xray(&mut xray_child);
            return Err("sing-box не начал работу за 10 с".into());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    if store.inbound == Inbound::Proxy {
        match sysproxy::enable(store.port) {
            Ok(before) => {
                if !sysproxy_is_ours(&before, store.port) {
                    store.saved_proxy = Some(before);
                }
                save(&store)?;
            }
            Err(e) => {
                let _ = child.kill();
                kill_xray(&mut xray_child);
                return Err(e);
            }
        }
    }
    let mut rt = st.vpn.lock().unwrap();
    rt.child = Some(child);
    rt.xray = xray_child;
    rt.started_at = Some(crate::zapret::runner::now_ms());
    rt.error = None;
    rt.server_name = Some(name.clone());
    drop(rt);
    let mode = match store.route_mode {
        RouteMode::Lists => "списки → VPN",
        RouteMode::AllExceptRu => "всё через VPN, кроме РФ",
    };
    let core = if use_xray { " · через Xray" } else { "" };
    journal::ok(format!("VPN включён · {name} · {mode}{core}"));
    Ok(())
}

fn sysproxy_is_ours(saved: &sysproxy::Saved, port: u16) -> bool {
    saved.enable == 1 && saved.server.as_deref() == Some(sysproxy::address(port).as_str())
}

/// Остановить sing-box и вернуть системный прокси.
pub fn stop(st: &AppState) -> Result<(), String> {
    let (child, xchild) = {
        let mut rt = st.vpn.lock().unwrap();
        (rt.child.take(), rt.xray.take())
    };
    let was = child.is_some();
    for mut c in [child, xchild].into_iter().flatten() {
        let _ = c.kill();
        let _ = c.wait();
    }
    kill_orphans();
    restore_proxy();
    let mut rt = st.vpn.lock().unwrap();
    rt.started_at = None;
    rt.server_name = None;
    drop(rt);
    if was {
        journal::info("VPN выключен");
    }
    Ok(())
}

/// Вернуть системный прокси, если он остался нашим (остановка, сбой, прошлый запуск).
pub fn restore_proxy() {
    let mut store = load();
    if let Some(saved) = store.saved_proxy.take() {
        if let Err(e) = sysproxy::restore(&saved, store.port) {
            journal::warn(format!("VPN: не удалось вернуть системный прокси: {e}"));
        }
        let _ = save(&store);
    } else if sysproxy::is_ours(store.port) {
        let _ = sysproxy::restore(&sysproxy::Saved::default(), store.port);
    }
}

/// При запуске программы: убрать остатки прошлого сеанса (прокси на мёртвый sing-box).
pub fn recover() {
    kill_orphans();
    restore_proxy();
}

// ── Подписки и серверы ─────────────────────────────────────────

fn sub_id(url: &str) -> String {
    use sha2::{Digest, Sha256};
    crate::net::hex(&Sha256::digest(url.trim().as_bytes())[..6])
}

/// Добавить подписку (ссылка https://) или ключи (одна или несколько ссылок на серверы).
pub async fn add(input: &str) -> Result<String, String> {
    let input = input.trim();
    if input.starts_with("http://") || input.starts_with("https://") {
        let f = subscription::fetch(input).await?;
        let mut store = load();
        let id = sub_id(input);
        let name = f.title.clone().unwrap_or_else(|| {
            url::Url::parse(input)
                .ok()
                .and_then(|u| u.host_str().map(str::to_string))
                .unwrap_or_else(|| "Подписка".into())
        });
        let n = f.servers.len();
        let sub = Subscription {
            id: id.clone(),
            url: input.to_string(),
            name: name.clone(),
            updated_at: Some(crate::zapret::runner::now_ms()),
            error: None,
            info: f.info,
            servers: f.servers,
        };
        match store.subscriptions.iter_mut().find(|s| s.id == id) {
            Some(s) => *s = sub,
            None => store.subscriptions.push(sub),
        }
        save(&store)?;
        journal::ok(format!("VPN: подписка «{name}» — серверов: {n}"));
        return Ok(format!("Подписка «{name}»: серверов — {n}"));
    }
    let found = links::parse_many(input);
    if found.is_empty() {
        return Err(match links::parse(input) {
            Err(e) => format!("не удалось разобрать: {e}"),
            Ok(_) => "не удалось разобрать ссылку".into(),
        });
    }
    let mut store = load();
    let mut added = 0;
    for s in found {
        if !store.manual.iter().any(|m| m.id == s.id) {
            store.manual.push(s);
            added += 1;
        }
    }
    save(&store)?;
    Ok(if added == 1 {
        "Сервер добавлен".into()
    } else {
        format!("Серверов добавлено: {added}")
    })
}

/// Обновить подписку (или все). Ошибка сохраняется у подписки, старые серверы остаются.
pub async fn update(id: Option<&str>) -> Result<usize, String> {
    let store = load();
    let targets: Vec<(String, String)> = store
        .subscriptions
        .iter()
        .filter(|s| id.is_none_or(|i| i == s.id))
        .map(|s| (s.id.clone(), s.url.clone()))
        .collect();
    let mut total = 0;
    let mut last_err = None;
    for (sid, url) in targets {
        let r = subscription::fetch(&url).await;
        let mut store = load();
        if let Some(s) = store.subscriptions.iter_mut().find(|s| s.id == sid) {
            match r {
                Ok(f) => {
                    total += f.servers.len();
                    s.servers = f.servers;
                    s.info = f.info.or(s.info.take());
                    if let Some(t) = f.title {
                        s.name = t;
                    }
                    s.error = None;
                    s.updated_at = Some(crate::zapret::runner::now_ms());
                }
                Err(e) => {
                    s.error = Some(e.clone());
                    last_err = Some(e);
                }
            }
        }
        save(&store)?;
    }
    match (total, last_err) {
        (0, Some(e)) => Err(e),
        _ => Ok(total),
    }
}

/// Задержка до серверов: время TCP-подключения (для QUIC-протоколов — недоступно).
pub fn ping(store: &Store) -> HashMap<String, Option<u32>> {
    let list: Vec<(String, String, u16)> =
        servers(store).iter().map(|s| (s.id.clone(), s.host.clone(), s.port)).collect();
    std::thread::scope(|scope| {
        let handles: Vec<_> = list
            .iter()
            .map(|(id, host, port)| {
                scope.spawn(move || {
                    let t0 = Instant::now();
                    let ok = (host.as_str(), *port)
                        .to_socket_addrs()
                        .ok()
                        .and_then(|mut a| a.next())
                        .is_some_and(|addr| TcpStream::connect_timeout(&addr, Duration::from_secs(3)).is_ok());
                    (id.clone(), ok.then(|| t0.elapsed().as_millis() as u32))
                })
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chosen_picks_selected_or_auto() {
        let mut s = Store::default();
        assert!(chosen(&s).unwrap_err().contains("подписку"));
        s.manual = links::parse_many(
            "trojan://p@a.com:443#A
vless://u@c.com:443?type=xhttp#C
hy2://pw@h.com:8443?sni=h.com#H",
        );
        // Автовыбор — через Xray (xhttp теперь тоже), Hysteria2 в нём не участвует.
        let (auto, name) = chosen(&s).unwrap();
        assert!(matches!(&auto, Engine::Xray(v) if v.len() == 2));
        assert!(name.contains("Автоматически"));
        // Выбран Hysteria2 — его ведёт sing-box.
        s.selected = Some(s.manual[2].id.clone());
        let (one, name) = chosen(&s).unwrap();
        assert!(matches!(&one, Engine::SingBox(v) if v.len() == 1));
        assert_eq!(name, "H");
        s.selected = Some(s.manual[1].id.clone());
        assert!(matches!(chosen(&s).unwrap().0, Engine::Xray(v) if v.len() == 1));
    }

    #[test]
    fn store_defaults() {
        let s: Store = serde_json::from_str("{}").unwrap();
        assert_eq!(s.port, 2080);
        assert!(s.zapret_direct);
        assert_eq!(s.presets, vec!["blocked", "ai"]);
    }

    /// Настоящий sing-box: скачивание, проверка конфига во всех режимах и маршрутизация в режиме
    /// прокси с заведомо недоступным «VPN-сервером»: ya.ru — напрямую (открывается),
    /// chatgpt.com — через «VPN» (не открывается). Системный прокси не трогается.
    /// `ZAPRETGUI_DATA_DIR=<папка> cargo test -- --ignored live_singbox --nocapture`
    #[test]
    #[ignore]
    fn live_singbox() {
        assert!(std::env::var_os("ZAPRETGUI_DATA_DIR").is_some(), "задайте ZAPRETGUI_DATA_DIR");
        if !singbox::installed() {
            let r = tauri::async_runtime::block_on(singbox::latest()).expect("релиз sing-box");
            tauri::async_runtime::block_on(singbox::install(&r, |p| {
                if p % 25 == 0 {
                    println!("скачано {p}%")
                }
            }))
            .expect("скачивание и SHA-256");
        }
        println!("sing-box {:?}", singbox::version());
        let all: Vec<&lists::Preset> = lists::PRESETS.iter().chain([&lists::RU_DIRECT]).collect();
        println!("ошибки списков: {:?}", tauri::async_runtime::block_on(lists::refresh(&all, false)));
        let fake = serde_json::json!({ "type": "trojan", "server": "127.0.0.1", "server_port": 9, "password": "x",
            "tls": { "enabled": true, "server_name": "example.com" } });
        let input = |route_mode, inbound, port| config::Input {
            inbound,
            route_mode,
            port,
            servers: vec![fake.clone()],
            vpn_custom: vec!["my-vpn.example".into()],
            direct_custom: vec!["my-direct.example".into()],
            zapret_direct: vec!["youtube.com".into(), "discord.com".into()],
            vpn_presets: ["blocked", "ai"].iter().filter_map(|id| lists::preset(id)).flat_map(lists::domains).collect(),
            ru_direct: lists::domains(&lists::RU_DIRECT),
            log_file: log_file().display().to_string(),
        };
        fs::create_dir_all(dir()).unwrap();
        for (name, mode, inbound) in [
            ("списки + прокси", RouteMode::Lists, Inbound::Proxy),
            ("всё кроме РФ + прокси", RouteMode::AllExceptRu, Inbound::Proxy),
            ("списки + TUN", RouteMode::Lists, Inbound::Tun),
            ("всё кроме РФ + TUN", RouteMode::AllExceptRu, Inbound::Tun),
        ] {
            let path = dir().join(format!("check-{name}.json").replace(' ', "_"));
            fs::write(&path, serde_json::to_string_pretty(&config::build(&input(mode, inbound, 22080))).unwrap())
                .unwrap();
            match singbox::check(&path) {
                Ok(()) => println!("конфиг «{name}»: принят"),
                Err(e) => panic!("конфиг «{name}» не принят: {e}"),
            }
        }
        let path = dir().join("run-test.json");
        fs::write(
            &path,
            serde_json::to_string(&config::build(&input(RouteMode::Lists, Inbound::Proxy, 22080))).unwrap(),
        )
        .unwrap();
        let mut child = singbox::spawn(&path).expect("запуск");
        let t0 = Instant::now();
        while !listening(22080) {
            assert!(t0.elapsed() < Duration::from_secs(10), "прокси не открылся: {:?}", last_error());
            std::thread::sleep(Duration::from_millis(200));
        }
        let curl = |url: &str| {
            let out = crate::sys::cmd::hidden("curl.exe")
                .args(["-s", "-o", "NUL", "-m", "8", "-w", "%{http_code}", "-x", "socks5h://127.0.0.1:22080", url])
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        let direct = curl("https://ya.ru");
        let vpn = curl("https://chatgpt.com");
        let _ = child.kill();
        let _ = child.wait();
        println!("ya.ru через sing-box: код {direct} (ожидается ответ — напрямую)");
        println!("chatgpt.com через sing-box: код {vpn} (ожидается 000 — ушёл на недоступный «VPN»)");
        assert!(direct != "000" && !direct.is_empty(), "прямой сайт не открылся: {direct}");
        assert_eq!(vpn, "000", "сайт из VPN-списка ушёл не на VPN");
    }

    /// Вся цепочка на настоящих серверах: sing-box (прокси, списки) → Xray → VPN-сервер.
    /// В `ZAPRETGUI_DATA_DIR\vpn\vpn.json` должна лежать настоящая подписка; системный прокси не трогается.
    /// `ZAPRETGUI_DATA_DIR=<папка> cargo test -- --ignored live_xray_chain --nocapture`
    #[test]
    #[ignore]
    fn live_xray_chain() {
        assert!(std::env::var_os("ZAPRETGUI_DATA_DIR").is_some(), "задайте ZAPRETGUI_DATA_DIR");
        if !xray::installed() {
            let r = tauri::async_runtime::block_on(xray::latest()).expect("релиз Xray");
            tauri::async_runtime::block_on(xray::install(&r, |p| {
                if p % 50 == 0 {
                    println!("Xray скачан на {p}%")
                }
            }))
            .expect("скачивание Xray");
        }
        println!("sing-box {:?}, Xray {:?}", singbox::version(), xray::version());
        tauri::async_runtime::block_on(update(None)).expect("обновление подписки (нужны исходные ссылки)");
        lists::ensure_custom();
        fs::write(
            lists::custom_path(lists::VPN_FILE),
            "api.ipify.org
",
        )
        .unwrap();
        let mut store = load();
        store.port = 22080;
        store.inbound = Inbound::Proxy;
        store.route_mode = RouteMode::Lists;
        let pick = |name: &str| servers(&store).iter().find(|s| s.name.contains(name)).map(|s| s.id.clone());
        let curl = |url: &str| {
            let out = crate::sys::cmd::hidden("curl.exe")
                .args(["-s", "-o", "NUL", "-m", "12", "-w", "%{http_code}", "-x", "socks5h://127.0.0.1:22080", url])
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        for (label, sel) in [("Франкфурт", pick("Франкфурт")), ("xhttp №7", pick("№7 LTE")), ("Автоматически", None)]
        {
            store.selected = sel;
            let (name, use_xray) = write_config(&store, None).expect("конфиги");
            let mut x = use_xray.then(|| xray::spawn(&xray_config_file()).expect("запуск Xray"));
            std::thread::sleep(Duration::from_millis(800));
            let mut sb = singbox::spawn(&config_file()).expect("запуск sing-box");
            let t0 = Instant::now();
            while !listening(22080) {
                assert!(t0.elapsed() < Duration::from_secs(10), "sing-box не открыл порт: {:?}", last_error());
                std::thread::sleep(Duration::from_millis(200));
            }
            if sel_is_auto(&store) {
                std::thread::sleep(Duration::from_secs(6)); // балансировщику — время замерить серверы
            }
            let vpn = curl("https://claude.ai");
            let vpn2 = curl("https://www.gstatic.com/generate_204");
            let direct = curl("https://ya.ru");
            let ip = |url: &str| {
                let out = crate::sys::cmd::hidden("curl.exe")
                    .args(["-s", "-m", "12", "-x", "socks5h://127.0.0.1:22080", url])
                    .output()
                    .unwrap();
                String::from_utf8_lossy(&out.stdout).trim().to_string()
            };
            let (ip_vpn, ip_direct) = (ip("https://api.ipify.org"), ip("https://api4.ipify.org"));
            println!("{label}: IP через VPN {ip_vpn} · напрямую {ip_direct}");
            assert!(!ip_vpn.is_empty() && ip_vpn != ip_direct, "{label}: IP через VPN совпал с прямым");
            let _ = sb.kill();
            let _ = sb.wait();
            if let Some(c) = x.as_mut() {
                let _ = c.kill();
                let _ = c.wait();
            }
            println!("{label} ({name}, Xray: {use_xray}): claude.ai {vpn} · gstatic {vpn2} · ya.ru {direct}");
            assert!(vpn != "000", "{label}: claude.ai через VPN не открылся; лог Xray: {:?}", xray_error());
            assert!(direct != "000", "{label}: прямой сайт не открылся");
        }
    }

    fn sel_is_auto(s: &Store) -> bool {
        s.selected.is_none()
    }
}
