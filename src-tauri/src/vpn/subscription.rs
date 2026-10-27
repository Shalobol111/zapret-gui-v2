//! Подписки: ссылка → список серверов. Ответ — base64-список ссылок (формат v2rayN), простой
//! текст со ссылками или JSON sing-box. Заголовки панелей (Marzban, 3x-ui, Remnawave):
//! `profile-title` — название, `subscription-userinfo` — трафик и срок.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::links::{self, b64_decode, Server};

/// Панели отдают формат по User-Agent; «v2rayN» — список ссылок в base64, его понимают все.
const USER_AGENT: &str = "v2rayN/7.13";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInfo {
    pub upload: u64,
    pub download: u64,
    pub total: u64,
    /// Окончание, unix-секунды (0 — бессрочно).
    pub expire: u64,
}

pub struct Fetched {
    pub title: Option<String>,
    pub info: Option<UserInfo>,
    pub servers: Vec<Server>,
}

/// `upload=1; download=2; total=3; expire=4`.
pub fn parse_userinfo(h: &str) -> Option<UserInfo> {
    let mut u = UserInfo::default();
    let mut any = false;
    for part in h.split(';') {
        let Some((k, v)) = part.split_once('=') else { continue };
        let v: u64 = v.trim().parse().unwrap_or(0);
        any = true;
        match k.trim() {
            "upload" => u.upload = v,
            "download" => u.download = v,
            "total" => u.total = v,
            "expire" => u.expire = v,
            _ => {}
        }
    }
    any.then_some(u)
}

/// `profile-title`: как есть или `base64:…`.
fn parse_title(h: &str) -> Option<String> {
    let t = match h.strip_prefix("base64:") {
        Some(b) => String::from_utf8(b64_decode(b)?).ok()?,
        None => h.to_string(),
    };
    Some(t.trim().to_string()).filter(|t| !t.is_empty())
}

/// Outbound-ы из JSON-конфига sing-box (если подписка отдала его вместо ссылок).
fn from_singbox_json(v: &Value) -> Vec<Server> {
    const KINDS: [&str; 7] = ["vless", "vmess", "trojan", "shadowsocks", "hysteria2", "tuic", "wireguard"];
    v.get("outbounds")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter(|o| o.get("type").and_then(Value::as_str).is_some_and(|t| KINDS.contains(&t)))
                .map(|o| {
                    let mut out = o.clone();
                    let name = out.get("tag").and_then(Value::as_str).unwrap_or("сервер").to_string();
                    if let Some(m) = out.as_object_mut() {
                        m.remove("tag");
                        m.remove("detour");
                    }
                    let host = out.get("server").and_then(Value::as_str).unwrap_or_default().to_string();
                    let port = out.get("server_port").and_then(Value::as_u64).unwrap_or(0) as u16;
                    let protocol = out.get("type").and_then(Value::as_str).unwrap_or_default().to_string();
                    let id = {
                        use sha2::{Digest, Sha256};
                        crate::net::hex(&Sha256::digest(out.to_string().as_bytes())[..6])
                    };
                    Server { id, name, protocol, host, port, outbound: Some(out), unsupported: None }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Тело ответа подписки → серверы.
pub fn parse_body(body: &str) -> Result<Vec<Server>, String> {
    let body = body.trim();
    if body.starts_with('{') {
        let v: Value = serde_json::from_str(body).map_err(|e| format!("неверный JSON: {e}"))?;
        let s = from_singbox_json(&v);
        return if s.is_empty() { Err("в JSON нет серверов".into()) } else { Ok(s) };
    }
    if body.starts_with("proxies:") || body.contains("\nproxies:") {
        return Err("формат Clash не поддерживается — нужна ссылка подписки для v2rayN".into());
    }
    let text = if body.contains("://") {
        body.to_string()
    } else {
        b64_decode(body)
            .and_then(|b| String::from_utf8(b).ok())
            .ok_or("ответ не похож на подписку (ни ссылок, ни base64)")?
    };
    let servers = links::parse_many(&text);
    if servers.is_empty() {
        Err("в подписке нет поддерживаемых серверов".into())
    } else {
        Ok(servers)
    }
}

pub async fn fetch(url: &str) -> Result<Fetched, String> {
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("ссылка подписки должна начинаться с https://".into());
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(url).send().await.map_err(crate::net::friendly)?;
    if !resp.status().is_success() {
        return Err(format!("сервер подписки ответил {}", resp.status()));
    }
    let header = |name: &str| resp.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
    let title = header("profile-title").and_then(|h| parse_title(&h));
    let info = header("subscription-userinfo").and_then(|h| parse_userinfo(&h));
    let body = resp.text().await.map_err(crate::net::friendly)?;
    Ok(Fetched { title, info, servers: parse_body(&body)? })
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn base64_list_plain_and_json() {
        let list = "trojan://p@a.com:443#A\nvless://u@b.com:443?security=reality&pbk=k#B\nss://bad";
        let b64 = base64::engine::general_purpose::STANDARD.encode(list);
        assert_eq!(parse_body(&b64).unwrap().len(), 2);
        assert_eq!(parse_body(list).unwrap()[1].name, "B");
        let j = r#"{"outbounds":[{"type":"vless","tag":"NL","server":"n.com","server_port":443,"uuid":"x"},{"type":"direct","tag":"direct"}]}"#;
        let s = parse_body(j).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "NL");
        assert!(s[0].outbound.as_ref().unwrap().get("tag").is_none());
        assert!(parse_body("proxies:\n  - name: x").unwrap_err().contains("Clash"));
        assert!(parse_body("<html>").is_err());
    }

    #[test]
    fn headers() {
        let u = parse_userinfo("upload=10; download=20; total=1073741824; expire=1767225600").unwrap();
        assert_eq!((u.upload, u.download, u.total, u.expire), (10, 20, 1073741824, 1767225600));
        assert_eq!(parse_title("base64:0JzQvtC5IFZQTg==").as_deref(), Some("Мой VPN"));
        assert_eq!(parse_title("Plain").as_deref(), Some("Plain"));
    }
}
