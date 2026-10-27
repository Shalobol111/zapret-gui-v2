//! Ссылки на VPN-серверы (`vless://`, `vmess://`, `trojan://`, `ss://`, `hysteria2://`, `tuic://`)
//! → outbound для sing-box. Формат ссылок — как у v2rayN/Xray, поля — по документации sing-box.

use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use url::Url;

/// Сервер из ссылки или подписки.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    /// Стабильный id (хэш ссылки) — выбор сервера переживает обновление подписки.
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub host: String,
    pub port: u16,
    /// Outbound sing-box (без `tag` — его ставит генератор конфига). `None` — не поддерживается.
    pub outbound: Option<Value>,
    /// Почему сервер не поддерживается (например, транспорт xhttp есть только в Xray).
    pub unsupported: Option<String>,
}

pub fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    [&STANDARD, &STANDARD_NO_PAD, &URL_SAFE, &URL_SAFE_NO_PAD].iter().find_map(|e| e.decode(s.as_bytes()).ok())
}

fn pct(s: &str) -> String {
    percent_encoding::percent_decode_str(s).decode_utf8_lossy().into_owned()
}

fn server_id(link: &str) -> String {
    use sha2::{Digest, Sha256};
    crate::net::hex(&Sha256::digest(link.trim().as_bytes())[..6])
}

/// Параметры запроса ссылки (`?type=ws&security=reality…`).
fn query(u: &Url) -> Map<String, Value> {
    u.query_pairs().map(|(k, v)| (k.to_lowercase(), Value::String(v.into_owned()))).collect()
}

fn q<'a>(m: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    m.get(k).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn truthy(v: Option<&str>) -> bool {
    matches!(v, Some("1" | "true" | "True" | "yes"))
}

fn alpn(v: Option<&str>) -> Option<Value> {
    v.map(|s| json!(s.split(',').map(str::trim).filter(|x| !x.is_empty()).collect::<Vec<_>>()))
}

/// Блок `tls` sing-box из параметров ссылки (security=tls|reality, sni, fp, pbk, sid, alpn, allowInsecure).
fn tls(m: &Map<String, Value>, host: &str, force: bool) -> Option<Value> {
    let security = q(m, "security").unwrap_or(if force { "tls" } else { "none" });
    if security == "none" {
        return None;
    }
    let mut t = json!({ "enabled": true, "server_name": q(m, "sni").or(q(m, "peer")).unwrap_or(host) });
    if let Some(a) = alpn(q(m, "alpn")) {
        t["alpn"] = a;
    }
    if truthy(q(m, "allowinsecure").or(q(m, "insecure")).or(q(m, "allow_insecure"))) {
        t["insecure"] = json!(true);
    }
    let fp = q(m, "fp").map(str::to_string).or_else(|| (security == "reality").then(|| "chrome".to_string()));
    if let Some(fp) = fp {
        t["utls"] = json!({ "enabled": true, "fingerprint": fp });
    }
    if security == "reality" {
        t["reality"] = json!({
            "enabled": true,
            "public_key": q(m, "pbk").unwrap_or_default(),
            "short_id": q(m, "sid").unwrap_or_default(),
        });
    }
    Some(t)
}

/// Транспорт sing-box (ws, grpc, http, httpupgrade). `Err` — транспорт не поддерживается.
fn transport(m: &Map<String, Value>) -> Result<Option<Value>, String> {
    let host = q(m, "host");
    let path = q(m, "path").map(pct);
    match q(m, "type").unwrap_or("tcp") {
        "tcp" | "raw" => {
            if q(m, "headertype") == Some("http") {
                let mut t = json!({ "type": "http" });
                if let Some(h) = host {
                    t["host"] = json!(h.split(',').collect::<Vec<_>>());
                }
                if let Some(p) = path {
                    t["path"] = json!(p);
                }
                Ok(Some(t))
            } else {
                Ok(None)
            }
        }
        "ws" => {
            let mut t = json!({ "type": "ws", "path": path.unwrap_or_else(|| "/".into()) });
            if let Some(h) = host {
                t["headers"] = json!({ "Host": h });
            }
            Ok(Some(t))
        }
        "grpc" => Ok(Some(json!({ "type": "grpc", "service_name": q(m, "servicename").unwrap_or_default() }))),
        "http" | "h2" => {
            let mut t = json!({ "type": "http", "path": path.unwrap_or_else(|| "/".into()) });
            if let Some(h) = host {
                t["host"] = json!(h.split(',').collect::<Vec<_>>());
            }
            Ok(Some(t))
        }
        "httpupgrade" => {
            let mut t = json!({ "type": "httpupgrade", "path": path.unwrap_or_else(|| "/".into()) });
            if let Some(h) = host {
                t["host"] = json!(h);
            }
            Ok(Some(t))
        }
        other => Err(format!("транспорт {other} не поддерживается sing-box (он есть только в Xray)")),
    }
}

fn host_port(u: &Url) -> Result<(String, u16), String> {
    let host = u.host_str().ok_or("нет адреса сервера")?.trim_matches(['[', ']']).to_string();
    let port = u.port().ok_or("нет порта")?;
    Ok((host, port))
}

fn name_of(u: &Url, fallback: &str) -> String {
    u.fragment()
        .map(pct)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

fn server(link: &str, name: String, protocol: &str, host: String, port: u16, out: Result<Value, String>) -> Server {
    let (outbound, unsupported) = match out {
        Ok(v) => (Some(v), None),
        Err(e) => (None, Some(e)),
    };
    Server { id: server_id(link), name, protocol: protocol.into(), host, port, outbound, unsupported }
}

fn vless(link: &str, u: &Url) -> Result<Server, String> {
    let (host, port) = host_port(u)?;
    let m = query(u);
    let out = (|| {
        let mut o = json!({ "type": "vless", "server": host, "server_port": port, "uuid": pct(u.username()) });
        if let Some(f) = q(&m, "flow") {
            o["flow"] = json!(f);
        }
        if let Some(t) = tls(&m, &host, false) {
            o["tls"] = t;
        }
        if let Some(t) = transport(&m)? {
            o["transport"] = t;
        }
        o["packet_encoding"] = json!("xudp");
        Ok(o)
    })();
    Ok(server(link, name_of(u, &host), "VLESS", host.clone(), port, out))
}

fn trojan(link: &str, u: &Url) -> Result<Server, String> {
    let (host, port) = host_port(u)?;
    let m = query(u);
    let out = (|| {
        let mut o = json!({ "type": "trojan", "server": host, "server_port": port, "password": pct(u.username()) });
        if let Some(t) = tls(&m, &host, true) {
            o["tls"] = t;
        }
        if let Some(t) = transport(&m)? {
            o["transport"] = t;
        }
        Ok(o)
    })();
    Ok(server(link, name_of(u, &host), "Trojan", host.clone(), port, out))
}

fn vmess(link: &str, body: &str) -> Result<Server, String> {
    let raw = b64_decode(body).ok_or("ссылка vmess не в base64")?;
    let j: Value = serde_json::from_slice(&raw).map_err(|_| "ссылка vmess повреждена")?;
    let s = |k: &str| {
        j.get(k).map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())).filter(|v| !v.is_empty())
    };
    let host = s("add").ok_or("нет адреса сервера")?;
    let port: u16 = s("port").and_then(|p| p.trim_matches('"').parse().ok()).ok_or("нет порта")?;
    let mut m = Map::new();
    for (k, key) in
        [("type", "net"), ("host", "host"), ("path", "path"), ("sni", "sni"), ("alpn", "alpn"), ("fp", "fp")]
    {
        if let Some(v) = s(key) {
            m.insert(k.into(), json!(v));
        }
    }
    if s("type").as_deref() == Some("http") {
        m.insert("headertype".into(), json!("http"));
    }
    if s("tls").as_deref() == Some("tls") {
        m.insert("security".into(), json!("tls"));
    }
    let out = (|| {
        let mut o = json!({
            "type": "vmess", "server": host, "server_port": port, "uuid": s("id").unwrap_or_default(),
            "security": s("scy").unwrap_or_else(|| "auto".into()),
            "alter_id": s("aid").and_then(|a| a.parse::<u32>().ok()).unwrap_or(0),
        });
        if let Some(t) = tls(&m, &host, false) {
            o["tls"] = t;
        }
        if let Some(t) = transport(&m)? {
            o["transport"] = t;
        }
        Ok(o)
    })();
    let name = s("ps").unwrap_or_else(|| host.clone());
    Ok(server(link, name, "VMess", host, port, out))
}

/// Shadowsocks: SIP002 `ss://base64(method:pass)@host:port#name` и старый `ss://base64(method:pass@host:port)#name`.
fn shadowsocks(link: &str) -> Result<Server, String> {
    let rest = link.trim_start_matches("ss://");
    let (main, frag) = rest.split_once('#').unwrap_or((rest, ""));
    let main = main.split('?').next().unwrap_or(main).trim_end_matches('/');
    let (userinfo, hostport) = match main.rsplit_once('@') {
        Some((ui, hp)) => (
            b64_decode(&pct(ui)).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_else(|| pct(ui)),
            hp.to_string(),
        ),
        None => {
            let decoded = String::from_utf8(b64_decode(main).ok_or("ссылка ss повреждена")?)
                .map_err(|_| "ссылка ss повреждена")?;
            let (ui, hp) = decoded.rsplit_once('@').ok_or("ссылка ss повреждена")?;
            (ui.to_string(), hp.to_string())
        }
    };
    let (method, password) = userinfo.split_once(':').ok_or("в ссылке ss нет метода шифрования")?;
    let u = Url::parse(&format!("ss://x@{hostport}")).map_err(|_| "неверный адрес сервера")?;
    let (host, port) = host_port(&u)?;
    let name = Some(pct(frag)).filter(|s| !s.trim().is_empty()).unwrap_or_else(|| host.clone());
    let o =
        json!({ "type": "shadowsocks", "server": host, "server_port": port, "method": method, "password": password });
    Ok(server(link, name, "Shadowsocks", host, port, Ok(o)))
}

fn hysteria2(link: &str, u: &Url) -> Result<Server, String> {
    let (host, port) = host_port(u)?;
    let m = query(u);
    let password = match u.password() {
        Some(p) => format!("{}:{}", pct(u.username()), pct(p)),
        None => pct(u.username()),
    };
    let mut o = json!({ "type": "hysteria2", "server": host, "server_port": port, "password": password });
    o["tls"] = tls(&m, &host, true).unwrap_or(json!({ "enabled": true, "server_name": host }));
    if let Some(obfs) = q(&m, "obfs") {
        o["obfs"] = json!({ "type": obfs, "password": q(&m, "obfs-password").unwrap_or_default() });
    }
    Ok(server(link, name_of(u, &host), "Hysteria2", host.clone(), port, Ok(o)))
}

fn tuic(link: &str, u: &Url) -> Result<Server, String> {
    let (host, port) = host_port(u)?;
    let m = query(u);
    let mut o = json!({
        "type": "tuic", "server": host, "server_port": port,
        "uuid": pct(u.username()), "password": u.password().map(pct).unwrap_or_default(),
        "congestion_control": q(&m, "congestion_control").unwrap_or("bbr"),
        "udp_relay_mode": q(&m, "udp_relay_mode").unwrap_or("native"),
    });
    let mut t = tls(&m, &host, true).unwrap_or(json!({ "enabled": true, "server_name": host }));
    if t.get("alpn").is_none() {
        t["alpn"] = json!(["h3"]);
    }
    o["tls"] = t;
    Ok(server(link, name_of(u, &host), "TUIC", host.clone(), port, Ok(o)))
}

/// Одна ссылка → сервер. `Err` — это не ссылка на сервер или она повреждена.
pub fn parse(link: &str) -> Result<Server, String> {
    let link = link.trim();
    let scheme = link.split("://").next().unwrap_or_default().to_lowercase();
    match scheme.as_str() {
        "vmess" => vmess(link, &link[8..]),
        "ss" => shadowsocks(link),
        "vless" | "trojan" | "hysteria2" | "hy2" | "tuic" => {
            let u = Url::parse(link).map_err(|_| "ссылка повреждена")?;
            match scheme.as_str() {
                "vless" => vless(link, &u),
                "trojan" => trojan(link, &u),
                "tuic" => tuic(link, &u),
                _ => hysteria2(link, &u),
            }
        }
        "" => Err("это не ссылка".into()),
        other => Err(format!("{other}:// не поддерживается")),
    }
}

/// Все ссылки из текста (по одной в строке; пустые строки и мусор пропускаются).
pub fn parse_many(text: &str) -> Vec<Server> {
    text.lines().map(str::trim).filter(|l| l.contains("://")).filter_map(|l| parse(l).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vless_reality() {
        let s = parse("vless://11111111-2222-3333-4444-555555555555@example.com:443?type=tcp&security=reality&sni=www.microsoft.com&fp=chrome&pbk=PUBKEY&sid=ab12&flow=xtls-rprx-vision#%D0%9D%D0%B8%D0%B4%D0%B5%D1%80%D0%BB%D0%B0%D0%BD%D0%B4%D1%8B").unwrap();
        assert_eq!(s.name, "Нидерланды");
        assert_eq!((s.host.as_str(), s.port), ("example.com", 443));
        let o = s.outbound.unwrap();
        assert_eq!(o["type"], "vless");
        assert_eq!(o["flow"], "xtls-rprx-vision");
        assert_eq!(o["tls"]["server_name"], "www.microsoft.com");
        assert_eq!(o["tls"]["reality"]["public_key"], "PUBKEY");
        assert_eq!(o["tls"]["reality"]["short_id"], "ab12");
        assert_eq!(o["tls"]["utls"]["fingerprint"], "chrome");
        assert!(o.get("transport").is_none());
    }

    #[test]
    fn vless_ws_tls_and_xhttp_unsupported() {
        let s =
            parse("vless://u@1.2.3.4:8443?type=ws&security=tls&path=%2Fws&host=cdn.example.com&sni=cdn.example.com#WS")
                .unwrap();
        let o = s.outbound.unwrap();
        assert_eq!(o["transport"]["type"], "ws");
        assert_eq!(o["transport"]["path"], "/ws");
        assert_eq!(o["transport"]["headers"]["Host"], "cdn.example.com");
        let x = parse("vless://u@h.com:443?type=xhttp&security=reality#X").unwrap();
        assert!(x.outbound.is_none());
        assert!(x.unsupported.unwrap().contains("xhttp"));
    }

    #[test]
    fn vmess_base64_json() {
        let j = r#"{"v":"2","ps":"Германия","add":"de.example.com","port":"443","id":"uuid-1","aid":"0","scy":"auto","net":"ws","type":"none","host":"de.example.com","path":"/v","tls":"tls","sni":"de.example.com"}"#;
        let s = parse(&format!("vmess://{}", STANDARD.encode(j))).unwrap();
        assert_eq!(s.name, "Германия");
        let o = s.outbound.unwrap();
        assert_eq!(o["type"], "vmess");
        assert_eq!(o["server_port"], 443);
        assert_eq!(o["transport"]["path"], "/v");
        assert_eq!(o["tls"]["enabled"], true);
    }

    #[test]
    fn shadowsocks_sip002_legacy_and_outline() {
        let ui = URL_SAFE_NO_PAD.encode("chacha20-ietf-poly1305:secret");
        let s = parse(&format!("ss://{ui}@5.6.7.8:8388#Outline%20key")).unwrap();
        assert_eq!(s.name, "Outline key");
        assert_eq!(s.outbound.as_ref().unwrap()["method"], "chacha20-ietf-poly1305");
        assert_eq!(s.outbound.unwrap()["password"], "secret");
        let legacy = STANDARD.encode("aes-256-gcm:pw@9.9.9.9:443");
        let l = parse(&format!("ss://{legacy}#Old")).unwrap();
        assert_eq!((l.host.as_str(), l.port), ("9.9.9.9", 443));
        let outline = parse(&format!("ss://{ui}@5.6.7.8:8388/?outline=1#O")).unwrap();
        assert_eq!(outline.port, 8388);
    }

    #[test]
    fn trojan_hysteria2_tuic() {
        let t = parse("trojan://pass@t.example.com:443?sni=t.example.com#T").unwrap().outbound.unwrap();
        assert_eq!(t["password"], "pass");
        assert_eq!(t["tls"]["enabled"], true);
        let h = parse("hy2://pw@h.example.com:8443?sni=h.example.com&obfs=salamander&obfs-password=o&insecure=1#H")
            .unwrap();
        let o = h.outbound.unwrap();
        assert_eq!(o["type"], "hysteria2");
        assert_eq!(o["obfs"]["type"], "salamander");
        assert_eq!(o["tls"]["insecure"], true);
        let u = parse("tuic://uuid:pw@u.example.com:443?congestion_control=cubic&sni=u.example.com#U")
            .unwrap()
            .outbound
            .unwrap();
        assert_eq!(u["congestion_control"], "cubic");
        assert_eq!(u["tls"]["alpn"][0], "h3");
    }

    #[test]
    fn garbage_is_rejected_and_ids_are_stable() {
        assert!(parse("hello").is_err());
        assert!(parse("wireguard://x").is_err());
        let a = parse("trojan://p@a.com:443#A").unwrap();
        let b = parse("trojan://p@a.com:443#A").unwrap();
        assert_eq!(a.id, b.id);
        assert_eq!(parse_many("trojan://p@a.com:443#A\n\nмусор\ntrojan://p@b.com:443#B").len(), 2);
    }
}
