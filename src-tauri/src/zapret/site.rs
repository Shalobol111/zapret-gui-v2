//! «Мой сайт»: почему не открывается конкретный сайт и что с этим сделать.
//!
//! Шаги: есть ли домен в списках zapret (и не исключён ли), подменяет ли провайдер DNS
//! (системный ответ против DNS-over-HTTPS), попадает ли IP в IPSet, открывается ли сайт
//! сейчас и что он отвечает. Итог — вердикт одной фразой и список действий.

use std::fs;
use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use super::diagnostics::Level;
use super::filters::{self, IpsetMode};
use super::tests::curl::{self, Tok};

/// «https://www.Example.com/path», «пример.рф», «example.com:443» → домен в нижнем регистре
/// (кириллица — в punycode, как его видит zapret в SNI).
pub fn normalize(input: &str) -> Result<String, String> {
    let s = input.trim();
    if s.is_empty() {
        return Err("Введите адрес сайта".into());
    }
    let with_scheme = if s.contains("://") { s.to_string() } else { format!("https://{s}") };
    let url = url::Url::parse(&with_scheme).map_err(|_| "Не похоже на адрес сайта".to_string())?;
    let host = url.host_str().ok_or("Не похоже на адрес сайта")?.trim_end_matches('.').to_lowercase();
    if host.parse::<IpAddr>().is_ok() || host.trim_start_matches('[').contains(':') {
        return Err("Введите домен, а не IP-адрес".into());
    }
    if !host.contains('.') {
        return Err("Введите полный домен, например rutracker.org".into());
    }
    Ok(host)
}

// ── Списки ─────────────────────────────────────────────────────

/// Строка hostlist: `example.com` — домен и поддомены, `^example.com` — только сам домен.
fn host_matches(entry: &str, domain: &str) -> bool {
    let e = entry.trim().to_lowercase();
    if e.is_empty() || e.starts_with('#') {
        return false;
    }
    match e.strip_prefix('^') {
        Some(exact) => domain == exact,
        None => domain == e || domain.ends_with(&format!(".{e}")),
    }
}

/// Первый файл из списка, где встречается домен.
fn find_in(root: &Path, files: &[&str], domain: &str) -> Option<(String, String)> {
    files.iter().find_map(|f| {
        let text = fs::read(root.join("lists").join(f)).ok()?;
        let text = String::from_utf8_lossy(&text);
        text.lines().find(|l| host_matches(l, domain)).map(|l| (f.to_string(), l.trim().to_string()))
    })
}

const INCLUDE: [&str; 3] = ["list-general-user.txt", "list-general.txt", "list-google.txt"];
const EXCLUDE: [&str; 2] = ["list-exclude-user.txt", "list-exclude.txt"];

// ── IPSet ──────────────────────────────────────────────────────

/// Попадает ли IPv4 в строку ipset: адрес, подсеть `/n` или диапазон `a-b`.
fn ip_in_entry(ip: Ipv4Addr, entry: &str) -> bool {
    let e = entry.trim();
    if e.is_empty() || e.starts_with('#') {
        return false;
    }
    let ip = u32::from(ip);
    if let Some((a, b)) = e.split_once('-') {
        return match (a.trim().parse::<Ipv4Addr>(), b.trim().parse::<Ipv4Addr>()) {
            (Ok(a), Ok(b)) => (u32::from(a)..=u32::from(b)).contains(&ip),
            _ => false,
        };
    }
    let (addr, prefix) = e.split_once('/').unwrap_or((e, "32"));
    match (addr.parse::<Ipv4Addr>(), prefix.parse::<u32>()) {
        (Ok(net), Ok(p)) if p <= 32 => {
            let mask = if p == 0 { 0 } else { u32::MAX << (32 - p) };
            (u32::from(net) & mask) == (ip & mask)
        }
        _ => false,
    }
}

fn ip_in_file(path: &Path, ips: &[Ipv4Addr]) -> Option<Ipv4Addr> {
    let text = fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&text);
    let lines: Vec<&str> = text.lines().collect();
    ips.iter().copied().find(|ip| lines.iter().any(|l| ip_in_entry(*ip, l)))
}

// ── DNS ────────────────────────────────────────────────────────

fn system_ips(domain: &str) -> Vec<Ipv4Addr> {
    (domain, 443)
        .to_socket_addrs()
        .map(|it| {
            it.filter_map(|a| match a.ip() {
                IpAddr::V4(v4) => Some(v4),
                IpAddr::V6(_) => None,
            })
            .collect()
        })
        .unwrap_or_default()
}

/// Адрес-заглушка: так провайдеры и «блокирующие» DNS подменяют ответ.
fn is_stub(ip: Ipv4Addr) -> bool {
    ip.is_unspecified() || ip.is_loopback() || ip.is_private() || ip.is_link_local()
}

async fn doh(domain: &str) -> Result<Vec<Ipv4Addr>, String> {
    let mut last = String::new();
    for base in ["https://cloudflare-dns.com/dns-query", "https://dns.google/resolve"] {
        let client = crate::net::client(Duration::from_secs(6))?;
        let url = url::Url::parse_with_params(base, &[("name", domain), ("type", "A")]).map_err(|e| e.to_string())?;
        let resp = client.get(url).header("Accept", "application/dns-json").send().await;
        let text = match resp {
            Ok(r) if r.status().is_success() => r.text().await.map_err(crate::net::friendly)?,
            Ok(r) => {
                last = format!("сервер ответил {}", r.status());
                continue;
            }
            Err(e) => {
                last = crate::net::friendly(e);
                continue;
            }
        };
        let json: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let ips = json
            .get("Answer")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter(|r| r.get("type").and_then(Value::as_u64) == Some(1))
                    .filter_map(|r| r.get("data")?.as_str()?.parse().ok())
                    .collect()
            })
            .unwrap_or_default();
        return Ok(ips);
    }
    Err(last)
}

// ── Доступ ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    pub tls12: Tok,
    pub tls13: Tok,
    /// HTTP-код ответа, если соединение установилось.
    pub code: Option<u16>,
}

impl Probe {
    pub fn reachable(&self) -> bool {
        self.tls12 == Tok::Ok || self.tls13 == Tok::Ok
    }
    /// 403/451 при рабочем TLS — сайт сам не пускает (обычно геоблокировка).
    pub fn geo_blocked(&self) -> bool {
        self.reachable() && matches!(self.code, Some(403 | 451))
    }
    fn summary(&self) -> String {
        format!("TLS 1.2: {} · TLS 1.3: {}", self.tls12.as_str(), self.tls13.as_str())
    }
}

pub fn probe(curl_exe: &Path, domain: &str) -> Probe {
    let url = format!("https://{domain}");
    let run = |args: &[&str]| -> (Tok, Option<u16>) {
        let out = curl::command(curl_exe)
            .args(["-I", "-s", "-m", "6", "--connect-timeout", "4", "-o", "NUL", "-w", "%{http_code}", "--show-error"])
            .args(args)
            .arg(&url)
            .output();
        match out {
            Ok(o) => {
                let tok = curl::classify_standard(o.status.code(), &String::from_utf8_lossy(&o.stderr));
                let code = String::from_utf8_lossy(&o.stdout).trim().parse::<u16>().ok().filter(|c| *c > 0);
                (tok, code)
            }
            Err(_) => (Tok::Error, None),
        }
    };
    let (a, b) = std::thread::scope(|s| {
        let a = s.spawn(|| run(&["--tlsv1.2", "--tls-max", "1.2"]));
        let b = s.spawn(|| run(&["--tlsv1.3", "--tls-max", "1.3"]));
        (a.join().unwrap_or((Tok::Error, None)), b.join().unwrap_or((Tok::Error, None)))
    });
    Probe { tls12: a.0, tls13: b.0, code: b.1.or(a.1) }
}

// ── Отчёт ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: &'static str,
    pub title: &'static str,
    pub level: Level,
    pub value: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Добавить домен в «Свои домены».
    AddToList,
    /// Подобрать стратегию под сайт.
    Pick,
    /// Включить обход (сейчас выключен).
    EnableBypass,
    /// Включить Secure DNS (провайдер подменяет DNS).
    SecureDns,
    /// Открыть «Списки», чтобы убрать домен из исключений.
    OpenExclude,
    /// Сравнить с выключенным обходом.
    Compare,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub domain: String,
    pub items: Vec<Item>,
    pub verdict: String,
    pub verdict_level: Level,
    pub actions: Vec<Action>,
    pub reachable: bool,
}

/// Входные данные для вердикта — отдельно от сбора, чтобы проверять логику тестами.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub listed_in: Option<String>,
    pub excluded_in: Option<String>,
    pub dns_spoof: bool,
    pub dns_fail: bool,
    /// Адрес не находится ни системным DNS, ни через DoH — опечатка или сайта нет.
    pub no_host: bool,
    pub bypass_on: bool,
    pub reachable: bool,
    pub geo_blocked: bool,
    pub code: Option<u16>,
}

pub fn verdict(f: &Facts) -> (String, Level, Vec<Action>) {
    use Action::*;
    if f.no_host {
        return ("Сайт с таким адресом не найден — проверьте написание.".into(), Level::Err, vec![]);
    }
    if f.geo_blocked {
        return (
            format!(
                "Сайт отвечает, но не пускает (код {}). Скорее всего, он сам ограничивает доступ из вашей страны — zapret тут не поможет.",
                f.code.unwrap_or(403)
            ),
            Level::Warn,
            vec![],
        );
    }
    if f.reachable {
        let text = if f.bypass_on || f.listed_in.is_none() {
            "Сайт открывается. Если в браузере он не грузится, очистите кэш браузера или проверьте его настройки прокси и DNS."
        } else {
            "Сайт открывается даже без обхода — провайдер его не блокирует."
        };
        return (text.into(), Level::Ok, vec![]);
    }
    if f.dns_fail || f.dns_spoof {
        let mut actions = vec![SecureDns];
        if f.listed_in.is_none() && f.excluded_in.is_none() {
            actions.push(AddToList);
        }
        return (
            "Провайдер подменяет DNS — браузер получает неверный адрес сайта. Включите Secure DNS в Windows или браузере.".into(),
            Level::Err,
            actions,
        );
    }
    if let Some(file) = &f.excluded_in {
        return (
            format!("Сайт в исключениях ({file}) — обход его намеренно не трогает. Уберите его оттуда в «Списках»."),
            Level::Err,
            vec![OpenExclude],
        );
    }
    if f.listed_in.is_none() {
        return (
            "Сайта нет в списках zapret — обход его не трогает. Добавьте его в «Свои домены» и подберите стратегию."
                .into(),
            Level::Err,
            vec![AddToList, Compare],
        );
    }
    if !f.bypass_on {
        return ("Сайт в списках, но обход выключен. Включите его на Главной.".into(), Level::Warn, vec![EnableBypass]);
    }
    (
        "Сайт в списках, но текущая стратегия его не открывает. Подберите стратегию для этого сайта.".into(),
        Level::Err,
        vec![Pick, Compare],
    )
}

/// Полная проверка. Блокирующая (curl, DNS) — вызывать вне главного потока.
pub fn check(root: &Path, domain: &str, bypass_on: bool) -> Result<Report, String> {
    let curl_exe = curl::curl_path().ok_or("Не найден curl.exe (входит в Windows 10 1803 и новее)")?;
    let (sys, doh_res, p) = std::thread::scope(|s| {
        let sys = s.spawn(|| system_ips(domain));
        let doh = s.spawn(|| tauri::async_runtime::block_on(doh(domain)));
        let p = s.spawn(|| probe(&curl_exe, domain));
        (sys.join().unwrap_or_default(), doh.join().unwrap_or(Err("ошибка".into())), p.join().ok())
    });
    let p = p.unwrap_or(Probe { tls12: Tok::Error, tls13: Tok::Error, code: None });

    let mut items = Vec::new();
    let mut f =
        Facts { bypass_on, reachable: p.reachable(), geo_blocked: p.geo_blocked(), code: p.code, ..Facts::default() };

    // 1. Списки
    let listed = find_in(root, &INCLUDE, domain);
    let excluded = find_in(root, &EXCLUDE, domain);
    items.push(match (&listed, &excluded) {
        (_, Some((file, line))) => Item {
            id: "lists",
            title: "Списки zapret",
            level: Level::Err,
            value: "в исключениях".into(),
            detail: Some(format!("{file}: «{line}»")),
        },
        (Some((file, line)), None) => Item {
            id: "lists",
            title: "Списки zapret",
            level: Level::Ok,
            value: "есть".into(),
            detail: Some(format!("{file}: «{line}»")),
        },
        (None, None) => Item {
            id: "lists",
            title: "Списки zapret",
            level: Level::Warn,
            value: "нет".into(),
            detail: Some("Домена нет ни в одном списке — стратегии его не обрабатывают".into()),
        },
    });
    f.listed_in = listed.map(|(file, _)| file);
    f.excluded_in = excluded.map(|(file, _)| file);

    // 2. DNS
    let show = |v: &[Ipv4Addr]| v.iter().take(3).map(|i| i.to_string()).collect::<Vec<_>>().join(", ");
    items.push(match &doh_res {
        _ if !sys.is_empty() && sys.iter().all(|i| is_stub(*i)) => {
            f.dns_spoof = true;
            Item {
                id: "dns",
                title: "DNS",
                level: Level::Err,
                value: "подменён".into(),
                detail: Some(format!("Система получает адрес-заглушку {}", show(&sys))),
            }
        }
        Ok(d) if sys.is_empty() && !d.is_empty() => {
            f.dns_fail = true;
            Item {
                id: "dns",
                title: "DNS",
                level: Level::Err,
                value: "не находит".into(),
                detail: Some(format!("Системный DNS не отвечает, а DNS-over-HTTPS даёт {}", show(d))),
            }
        }
        Ok(d) if d.is_empty() && sys.is_empty() => {
            f.no_host = true;
            Item {
                id: "dns",
                title: "DNS",
                level: Level::Err,
                value: "нет такого сайта".into(),
                detail: Some(
                    "Адрес не находится ни системным DNS, ни через DNS-over-HTTPS — проверьте написание".into(),
                ),
            }
        }
        Ok(_) => Item { id: "dns", title: "DNS", level: Level::Ok, value: "ок".into(), detail: Some(show(&sys)) },
        Err(e) => Item {
            id: "dns",
            title: "DNS",
            level: Level::Info,
            value: if sys.is_empty() { "не находит".into() } else { "не проверен".into() },
            detail: Some(format!("DNS-over-HTTPS недоступен ({e}); системный ответ: {}", show(&sys))),
        },
    });

    // 3. IPSet
    let mode = filters::ipset_mode(root);
    let lists = root.join("lists");
    let ips: Vec<Ipv4Addr> = sys
        .iter()
        .chain(doh_res.as_ref().map(|d| d.as_slice()).unwrap_or(&[]))
        .copied()
        .filter(|i| !is_stub(*i))
        .collect();
    let in_set = if mode == IpsetMode::Loaded { ip_in_file(&lists.join("ipset-all.txt"), &ips) } else { None };
    let in_excl =
        ip_in_file(&lists.join("ipset-exclude.txt"), &ips).or(ip_in_file(&lists.join("ipset-exclude-user.txt"), &ips));
    items.push(match (mode, in_set, in_excl) {
        (_, _, Some(ip)) => Item {
            id: "ipset",
            title: "IPSet",
            level: Level::Warn,
            value: "в исключениях".into(),
            detail: Some(format!("{ip} в ipset-exclude — фильтр по IP его не трогает")),
        },
        (IpsetMode::Any, _, _) => Item {
            id: "ipset",
            title: "IPSet",
            level: Level::Info,
            value: "любой IP".into(),
            detail: Some("Режим «Любой» — фильтр по IP охватывает все адреса".into()),
        },
        (IpsetMode::Loaded, Some(ip), _) => Item {
            id: "ipset",
            title: "IPSet",
            level: Level::Ok,
            value: "есть".into(),
            detail: Some(format!("{ip} в ipset-all.txt")),
        },
        (IpsetMode::Loaded, None, _) => {
            Item { id: "ipset", title: "IPSet", level: Level::Info, value: "нет".into(), detail: None }
        }
        (IpsetMode::Off, _, _) => Item {
            id: "ipset",
            title: "IPSet",
            level: Level::Info,
            value: "выключен".into(),
            detail: Some("Фильтр по IP выключен — работают только списки доменов".into()),
        },
    });

    // 4–5. Доступ и ответ
    items.push(Item {
        id: "access",
        title: if bypass_on { "Доступ с обходом" } else { "Доступ без обхода" },
        level: if p.reachable() { Level::Ok } else { Level::Err },
        value: if p.reachable() { "открывается".into() } else { "не открывается".into() },
        detail: Some(p.summary()),
    });
    if let Some(code) = p.code.filter(|_| p.reachable()) {
        items.push(Item {
            id: "answer",
            title: "Ответ сайта",
            level: if p.geo_blocked() { Level::Warn } else { Level::Ok },
            value: format!("код {code}"),
            detail: p.geo_blocked().then(|| "Сайт отказывает в доступе — обычно это ограничение по стране".into()),
        });
    }

    // Сайт и так открывается — исключение в списках не проблема, а просто факт.
    if p.reachable() {
        for i in items.iter_mut().filter(|i| i.id == "lists" && i.level == Level::Err) {
            i.level = Level::Info;
        }
    }
    let (verdict, verdict_level, actions) = verdict(&f);
    Ok(Report { domain: domain.to_string(), items, verdict, verdict_level, actions, reachable: p.reachable() })
}

/// Добавить домен в «Свои домены» (если его там ещё нет).
pub fn add_to_user_list(root: &Path, domain: &str) -> Result<bool, String> {
    let data = super::userlists::read(root, "user")?;
    if data.text.lines().any(|l| host_matches(l, domain)) {
        return Ok(false);
    }
    let mut text = data.text.clone();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(domain);
    text.push('\n');
    super::userlists::save(root, "user", &text, &data.hash, false).map_err(|e| match e {
        super::userlists::SaveError::Conflict => "файл изменили — попробуйте ещё раз".to_string(),
        super::userlists::SaveError::Other(e) => e,
    })?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_input() {
        assert_eq!(normalize("https://www.Example.com/path?q=1").unwrap(), "www.example.com");
        assert_eq!(normalize("rutracker.org:443").unwrap(), "rutracker.org");
        assert_eq!(normalize("пример.рф").unwrap(), "xn--e1afmkfd.xn--p1ai");
        assert!(normalize("1.2.3.4").is_err());
        assert!(normalize("localhost").is_err());
        assert!(normalize("  ").is_err());
    }

    #[test]
    fn hostlist_matching() {
        assert!(host_matches("discord.com", "discord.com"));
        assert!(host_matches("discord.com", "cdn.discord.com"));
        assert!(!host_matches("discord.com", "notdiscord.com"));
        assert!(host_matches("^dns.google", "dns.google"));
        assert!(!host_matches("^dns.google", "x.dns.google"));
        assert!(!host_matches("# discord.com", "discord.com"));
    }

    #[test]
    fn ipset_matching() {
        let ip: Ipv4Addr = "10.1.2.3".parse().unwrap();
        assert!(ip_in_entry(ip, "10.0.0.0/8"));
        assert!(ip_in_entry(ip, "10.1.2.3"));
        assert!(ip_in_entry(ip, "10.1.2.0-10.1.2.10"));
        assert!(!ip_in_entry(ip, "11.0.0.0/8"));
        assert!(!ip_in_entry(ip, "2001:db8::/32"));
    }

    #[test]
    fn verdicts_cover_main_cases() {
        let base = Facts { bypass_on: true, ..Facts::default() };
        let (_, lvl, a) = verdict(&base);
        assert_eq!((lvl, a), (Level::Err, vec![Action::AddToList, Action::Compare]));

        let listed = Facts { listed_in: Some("list-general.txt".into()), ..base.clone() };
        assert_eq!(verdict(&listed).2, vec![Action::Pick, Action::Compare]);
        assert_eq!(verdict(&Facts { bypass_on: false, ..listed.clone() }).2, vec![Action::EnableBypass]);
        assert_eq!(verdict(&Facts { dns_spoof: true, ..listed.clone() }).2, vec![Action::SecureDns]);
        assert_eq!(
            verdict(&Facts { excluded_in: Some("list-exclude.txt".into()), ..base.clone() }).2,
            vec![Action::OpenExclude]
        );

        let geo = Facts { reachable: true, geo_blocked: true, code: Some(451), ..base.clone() };
        assert!(verdict(&geo).0.contains("451"));
        assert_eq!(verdict(&Facts { reachable: true, ..base.clone() }).1, Level::Ok);
        assert!(verdict(&Facts { no_host: true, ..base }).2.is_empty());
    }

    /// Сетевой: `ZAPRET_ROOT=<папка> cargo test -- --ignored live_site --nocapture`.
    #[test]
    #[ignore]
    fn live_site() {
        let root = std::path::PathBuf::from(std::env::var("ZAPRET_ROOT").expect("ZAPRET_ROOT"));
        for d in ["ya.ru", "discord.com", "rutracker.org", "chatgpt.com", "nonexistent-zapretgui-test.ru"] {
            let r = check(&root, d, true).unwrap();
            println!("\n== {d}: {} ({:?}) {:?}", r.verdict, r.verdict_level, r.actions);
            for i in &r.items {
                println!("   {:<18} {:?} {} {}", i.title, i.level, i.value, i.detail.clone().unwrap_or_default());
            }
        }
    }

    #[test]
    fn add_to_user_list_skips_duplicates() {
        let root = std::env::temp_dir().join(format!("zgui-site-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("lists")).unwrap();
        assert!(add_to_user_list(&root, "rutracker.org").unwrap());
        assert!(!add_to_user_list(&root, "rutracker.org").unwrap());
        // Поддомен уже покрыт родительским доменом.
        assert!(!add_to_user_list(&root, "www.rutracker.org").unwrap());
        let text = fs::read_to_string(root.join("lists/list-general-user.txt")).unwrap();
        assert_eq!(text.matches("rutracker.org").count(), 1);
    }
}
