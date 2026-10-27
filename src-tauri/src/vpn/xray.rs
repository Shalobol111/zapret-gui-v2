//! Xray-core — соединение с VPN-серверами VLESS/VMess/Trojan/Shadowsocks. sing-box остаётся
//! маршрутизатором (TUN/прокси, списки) и отдаёт трафик «через VPN» в Xray по локальному SOCKS.
//! Xray — эталонная реализация этих протоколов: с ней работают Reality-серверы, которые не
//! принимают sing-box, и транспорт xhttp. Ядро — официальный релиз XTLS/Xray-core.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::Duration;

use serde_json::{json, Map, Value};
use url::Url;

use super::links::{b64_decode, Server};
use crate::sys::cmd;
use crate::zapret::updater;

const API_LATEST: &str = "https://api.github.com/repos/XTLS/Xray-core/releases/latest";
const ASSET: &str = "Xray-windows-64.zip";

pub fn core_dir() -> PathBuf {
    super::dir().join("xray")
}
pub fn exe() -> PathBuf {
    core_dir().join("xray.exe")
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

// ── Скачивание ─────────────────────────────────────────────────

pub struct Release {
    pub version: String,
    url: String,
    sha256: Option<String>,
}

fn parse_release(v: &Value) -> Option<Release> {
    let version = v.get("tag_name")?.as_str()?.trim_start_matches('v').to_string();
    let a = v.get("assets")?.as_array()?.iter().find(|a| a.get("name").and_then(Value::as_str) == Some(ASSET))?;
    Some(Release {
        version,
        url: a.get("browser_download_url")?.as_str()?.to_string(),
        sha256: a.get("digest").and_then(Value::as_str).and_then(|d| d.strip_prefix("sha256:")).map(str::to_lowercase),
    })
}

pub async fn latest() -> Result<Release, String> {
    let text = crate::net::get_text(API_LATEST, Duration::from_secs(15)).await?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("неверный ответ GitHub: {e}"))?;
    parse_release(&v).ok_or_else(|| format!("в последнем релизе Xray нет {ASSET}"))
}

pub fn newer(latest: &str) -> bool {
    match version() {
        Some(cur) => updater::compare(latest, &cur) == Some(std::cmp::Ordering::Greater),
        None => true,
    }
}

/// Скачать и распаковать (Xray должен быть остановлен). В архиве — xray.exe и geo-файлы.
pub async fn install(r: &Release, mut progress: impl FnMut(u8)) -> Result<(), String> {
    let tmp = super::dir().join("download-xray");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let zip = tmp.join("xray.zip");
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
            return Err("контрольная сумма Xray не совпала — файл повреждён, попробуйте ещё раз".into());
        }
    }
    let out = tmp.join("x");
    updater::extract(&zip, &out)?;
    if !out.join("xray.exe").is_file() {
        return Err("в архиве нет xray.exe".into());
    }
    let _ = fs::remove_dir_all(core_dir());
    fs::create_dir_all(core_dir()).map_err(|e| e.to_string())?;
    for e in fs::read_dir(&out).map_err(|e| e.to_string())?.flatten() {
        if e.path().is_file() {
            fs::copy(e.path(), core_dir().join(e.file_name()))
                .map_err(|e| format!("не удалось скопировать Xray: {e}"))?;
        }
    }
    let _ = fs::write(version_file(), &r.version);
    let _ = fs::remove_dir_all(&tmp);
    Ok(())
}

// ── Ссылка → outbound Xray ─────────────────────────────────────

fn pct(s: &str) -> String {
    percent_encoding::percent_decode_str(s).decode_utf8_lossy().into_owned()
}

fn q<'a>(m: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    m.get(k).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn truthy(v: Option<&str>) -> bool {
    matches!(v, Some("1" | "true" | "True" | "yes"))
}

/// streamSettings из параметров ссылки (имена — как в v2rayN/Happ).
fn stream(m: &Map<String, Value>, host: &str, default_security: &str) -> Value {
    let network = match q(m, "type").unwrap_or("tcp") {
        "raw" | "tcp" => "tcp",
        "splithttp" | "http" | "h2" => "xhttp",
        other => other,
    };
    let security = q(m, "security").unwrap_or(default_security);
    let mut s = json!({ "network": network, "security": security });
    let sni = q(m, "sni").or(q(m, "peer")).unwrap_or(host);
    match security {
        "tls" => {
            let mut t = json!({ "serverName": sni });
            if let Some(fp) = q(m, "fp") {
                t["fingerprint"] = json!(fp);
            }
            if let Some(a) = q(m, "alpn") {
                t["alpn"] = json!(a.split(',').map(str::trim).collect::<Vec<_>>());
            }
            if truthy(q(m, "allowinsecure").or(q(m, "insecure"))) {
                t["allowInsecure"] = json!(true);
            }
            s["tlsSettings"] = t;
        }
        "reality" => {
            let mut t = json!({
                "serverName": sni,
                "fingerprint": q(m, "fp").unwrap_or("chrome"),
                "publicKey": q(m, "pbk").unwrap_or_default(),
                "shortId": q(m, "sid").unwrap_or_default(),
            });
            if let Some(spx) = q(m, "spx") {
                t["spiderX"] = json!(pct(spx));
            }
            if let Some(pqv) = q(m, "pqv") {
                t["mldsa65Verify"] = json!(pqv);
            }
            s["realitySettings"] = t;
        }
        _ => {}
    }
    let path = q(m, "path").map(pct);
    let h = q(m, "host");
    match network {
        "ws" => {
            let mut w = json!({ "path": path.unwrap_or_else(|| "/".into()) });
            if let Some(h) = h {
                w["host"] = json!(h);
            }
            s["wsSettings"] = w;
        }
        "grpc" => {
            s["grpcSettings"] = json!({
                "serviceName": q(m, "servicename").map(pct).unwrap_or_default(),
                "multiMode": q(m, "mode") == Some("multi"),
            });
        }
        "xhttp" => {
            let mut x = json!({ "path": path.unwrap_or_else(|| "/".into()), "mode": q(m, "mode").unwrap_or("auto") });
            if let Some(h) = h {
                x["host"] = json!(h);
            }
            if let Some(extra) = q(m, "extra").and_then(|e| serde_json::from_str::<Value>(e).ok()) {
                x["extra"] = extra;
            }
            s["xhttpSettings"] = x;
        }
        "httpupgrade" => {
            let mut u = json!({ "path": path.unwrap_or_else(|| "/".into()) });
            if let Some(h) = h {
                u["host"] = json!(h);
            }
            s["httpupgradeSettings"] = u;
        }
        "tcp" if q(m, "headertype") == Some("http") => {
            let mut req = json!({ "path": [path.unwrap_or_else(|| "/".into())] });
            if let Some(h) = h {
                req["headers"] = json!({ "Host": h.split(',').collect::<Vec<_>>() });
            }
            s["tcpSettings"] = json!({ "header": { "type": "http", "request": req } });
        }
        _ => {}
    }
    // Фрагментация TLS-приветствия от провайдера (параметр fm у Happ) — как есть.
    if let Some(fm) = q(m, "fm").and_then(|f| serde_json::from_str::<Value>(f).ok()) {
        s["finalmask"] = fm;
    }
    s
}

fn url_parts(link: &str) -> Result<(Url, String, u16, Map<String, Value>), String> {
    let u = Url::parse(link).map_err(|_| "ссылка повреждена")?;
    let host = u.host_str().ok_or("нет адреса сервера")?.trim_matches(['[', ']']).to_string();
    let port = u.port().ok_or("нет порта")?;
    let m: Map<String, Value> =
        u.query_pairs().map(|(k, v)| (k.to_lowercase(), Value::String(v.into_owned()))).collect();
    Ok((u, host, port, m))
}

/// Outbound Xray для сервера (с тегом `tag`).
pub fn outbound(s: &Server, tag: &str) -> Result<Value, String> {
    let link = s.link.as_deref().ok_or("нет исходной ссылки — обновите подписку")?;
    let mut o = match s.protocol.as_str() {
        "VLESS" => {
            let (u, host, port, m) = url_parts(link)?;
            let mut user = json!({ "id": pct(u.username()), "encryption": q(&m, "encryption").unwrap_or("none") });
            if let Some(f) = q(&m, "flow") {
                user["flow"] = json!(f);
            }
            json!({
                "protocol": "vless",
                "settings": { "vnext": [{ "address": host, "port": port, "users": [user] }] },
                "streamSettings": stream(&m, &host, "none"),
            })
        }
        "Trojan" => {
            let (u, host, port, m) = url_parts(link)?;
            json!({
                "protocol": "trojan",
                "settings": { "servers": [{ "address": host, "port": port, "password": pct(u.username()) }] },
                "streamSettings": stream(&m, &host, "tls"),
            })
        }
        "VMess" => {
            let raw = b64_decode(link.trim_start_matches("vmess://")).ok_or("ссылка vmess не в base64")?;
            let j: Value = serde_json::from_slice(&raw).map_err(|_| "ссылка vmess повреждена")?;
            let g = |k: &str| {
                j.get(k)
                    .map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))
                    .filter(|v| !v.is_empty())
            };
            let host = g("add").ok_or("нет адреса сервера")?;
            let port: u16 = g("port").and_then(|p| p.trim_matches('"').parse().ok()).ok_or("нет порта")?;
            let mut m = Map::new();
            for (k, key) in
                [("type", "net"), ("host", "host"), ("path", "path"), ("sni", "sni"), ("alpn", "alpn"), ("fp", "fp")]
            {
                if let Some(v) = g(key) {
                    m.insert(k.into(), json!(v));
                }
            }
            if g("type").as_deref() == Some("http") {
                m.insert("headertype".into(), json!("http"));
            }
            if g("tls").as_deref() == Some("tls") {
                m.insert("security".into(), json!("tls"));
            }
            json!({
                "protocol": "vmess",
                "settings": { "vnext": [{ "address": host, "port": port, "users": [{
                    "id": g("id").unwrap_or_default(),
                    "alterId": g("aid").and_then(|a| a.parse::<u32>().ok()).unwrap_or(0),
                    "security": g("scy").unwrap_or_else(|| "auto".into()),
                }] }] },
                "streamSettings": stream(&m, &host, "none"),
            })
        }
        "Shadowsocks" => {
            let sb = s.outbound.as_ref().ok_or("ссылка ss повреждена")?;
            json!({
                "protocol": "shadowsocks",
                "settings": { "servers": [{
                    "address": sb["server"], "port": sb["server_port"],
                    "method": sb["method"], "password": sb["password"],
                }] },
            })
        }
        other => return Err(format!("{other} через Xray не поддерживается")),
    };
    o["tag"] = json!(tag);
    Ok(o)
}

// ── Конфиг ─────────────────────────────────────────────────────

/// SOCKS на 127.0.0.1:port → выбранный сервер или автовыбор быстрейшего (leastPing).
pub fn config(servers: &[&Server], port: u16, log_file: &str) -> Result<Value, String> {
    let mut outbounds = Vec::new();
    let routing = if servers.len() == 1 {
        outbounds.push(outbound(servers[0], "proxy")?);
        json!({ "rules": [{ "type": "field", "inboundTag": ["in"], "outboundTag": "proxy" }] })
    } else {
        for (n, s) in servers.iter().enumerate() {
            match outbound(s, &format!("s{n}")) {
                Ok(o) => outbounds.push(o),
                Err(e) => crate::journal::warn(format!("VPN: сервер «{}» пропущен: {e}", s.name)),
            }
        }
        if outbounds.is_empty() {
            return Err("ни один сервер не удалось подготовить для Xray".into());
        }
        json!({
            "balancers": [{ "tag": "auto", "selector": ["s"], "strategy": { "type": "leastPing" } }],
            "rules": [{ "type": "field", "inboundTag": ["in"], "balancerTag": "auto" }],
        })
    };
    let mut cfg = json!({
        "log": { "loglevel": "warning", "error": log_file, "access": "none" },
        "inbounds": [{ "tag": "in", "listen": "127.0.0.1", "port": port, "protocol": "socks", "settings": { "udp": true } }],
        "outbounds": outbounds,
        "routing": routing,
    });
    if servers.len() > 1 {
        cfg["observatory"] = json!({
            "subjectSelector": ["s"],
            "probeUrl": "https://www.gstatic.com/generate_204",
            "probeInterval": "1m",
            "enableConcurrency": true,
        });
    }
    Ok(cfg)
}

/// `xray run -test` — ошибки конфига до запуска.
pub fn check(config: &Path) -> Result<(), String> {
    let out = cmd::hidden(&exe().to_string_lossy())
        .args(["run", "-test", "-c"])
        .arg(config)
        .current_dir(core_dir())
        .output()
        .map_err(|e| format!("не удалось запустить Xray: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let text = cmd::text(&out);
        Err(text.lines().rfind(|l| !l.trim().is_empty()).unwrap_or("конфиг не принят").trim().to_string())
    }
}

pub fn spawn(config: &Path) -> Result<Child, String> {
    cmd::hidden(&exe().to_string_lossy())
        .args(["run", "-c"])
        .arg(config)
        .current_dir(core_dir())
        .spawn()
        .map_err(|e| format!("не удалось запустить Xray: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vpn::links;

    #[test]
    fn vless_reality_vision() {
        let s = links::parse("vless://uid@h.example:8443?encryption=none&flow=xtls-rprx-vision&type=tcp&security=reality&sni=kelvra.pro&fp=qq&pbk=PK&sid=ab&spx=%2F#F").unwrap();
        let o = outbound(&s, "proxy").unwrap();
        assert_eq!(o["protocol"], "vless");
        assert_eq!(o["settings"]["vnext"][0]["users"][0]["flow"], "xtls-rprx-vision");
        let r = &o["streamSettings"]["realitySettings"];
        assert_eq!(
            (r["serverName"].as_str(), r["fingerprint"].as_str(), r["publicKey"].as_str()),
            (Some("kelvra.pro"), Some("qq"), Some("PK"))
        );
        assert_eq!(r["spiderX"], "/");
        assert_eq!(o["streamSettings"]["network"], "tcp");
    }

    #[test]
    fn xhttp_with_extra_and_ws_tls() {
        let s = links::parse(r#"vless://u@h.example:443?type=xhttp&path=%2Fp&host=cdn.example&mode=auto&security=reality&sni=cdn.example&pbk=K&sid=1&extra=%7B%22xPaddingBytes%22%3A%22100-1000%22%7D#X"#).unwrap();
        let o = outbound(&s, "s0").unwrap();
        let x = &o["streamSettings"]["xhttpSettings"];
        assert_eq!(
            (x["path"].as_str(), x["host"].as_str(), x["mode"].as_str()),
            (Some("/p"), Some("cdn.example"), Some("auto"))
        );
        assert_eq!(x["extra"]["xPaddingBytes"], "100-1000");
        let w = links::parse("vless://u@h.example:443?type=ws&path=%2Fws&host=w.example&security=tls&sni=w.example&fp=edge&alpn=http%2F1.1#W").unwrap();
        let o = outbound(&w, "s1").unwrap();
        assert_eq!(o["streamSettings"]["wsSettings"]["host"], "w.example");
        assert_eq!(o["streamSettings"]["tlsSettings"]["alpn"][0], "http/1.1");
    }

    #[test]
    fn trojan_ss_and_balancer() {
        let t = links::parse("trojan://pw@t.example:443?sni=t.example#T").unwrap();
        let ss = links::parse("ss://YWVzLTI1Ni1nY206cHc@9.9.9.9:8388#S").unwrap();
        let cfg = config(&[&t, &ss], 2081, "x.log").unwrap();
        assert_eq!(cfg["outbounds"][0]["protocol"], "trojan");
        assert_eq!(cfg["outbounds"][0]["streamSettings"]["security"], "tls");
        assert_eq!(cfg["outbounds"][1]["settings"]["servers"][0]["method"], "aes-256-gcm");
        assert_eq!(cfg["routing"]["balancers"][0]["strategy"]["type"], "leastPing");
        assert_eq!(cfg["inbounds"][0]["port"], 2081);
        let one = config(&[&t], 2081, "x.log").unwrap();
        assert!(one.get("observatory").is_none());
        assert_eq!(one["outbounds"][0]["tag"], "proxy");
    }
}
