//! Цели стандартного теста из `utils\targets.txt` (формат test zapret.ps1):
//! `Key = "https://host"` — HTTP/TLS + пинг, `Key = "PING:1.2.3.4"` — только пинг.
//! Разделы `### Discord` и т.п. дают колонки таблицы.

use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub key: String,
    pub group: String,
    pub url: Option<String>,
    pub ping: String,
}

const DEFAULTS: &[(&str, &str, &str)] = &[
    ("Discord", "DiscordMain", "https://discord.com"),
    ("Discord", "DiscordGateway", "https://gateway.discord.gg"),
    ("Discord", "DiscordCDN", "https://cdn.discordapp.com"),
    ("Discord", "DiscordUpdates", "https://updates.discord.com"),
    ("YouTube", "YouTubeWeb", "https://www.youtube.com"),
    ("YouTube", "YouTubeShort", "https://youtu.be"),
    ("YouTube", "YouTubeImage", "https://i.ytimg.com"),
    ("YouTube", "YouTubeVideoRedirect", "https://redirector.googlevideo.com"),
    ("Google", "GoogleMain", "https://www.google.com"),
    ("Google", "GoogleGstatic", "https://www.gstatic.com"),
    ("Cloudflare", "CloudflareWeb", "https://www.cloudflare.com"),
    ("Cloudflare", "CloudflareCDN", "https://cdnjs.cloudflare.com"),
    ("Public DNS", "CloudflareDNS1111", "PING:1.1.1.1"),
    ("Public DNS", "CloudflareDNS1001", "PING:1.0.0.1"),
    ("Public DNS", "GoogleDNS8888", "PING:8.8.8.8"),
    ("Public DNS", "GoogleDNS8844", "PING:8.8.4.4"),
    ("Public DNS", "Quad9DNS9999", "PING:9.9.9.9"),
];

fn make(group: &str, key: &str, value: &str) -> Target {
    if let Some(ip) = value.strip_prefix("PING:") {
        Target { key: key.into(), group: group.into(), url: None, ping: ip.trim().into() }
    } else {
        let host = value.split("://").nth(1).unwrap_or(value).split('/').next().unwrap_or("").to_string();
        Target { key: key.into(), group: group.into(), url: Some(value.into()), ping: host }
    }
}

/// Раздел без заголовка — по префиксу ключа (DiscordMain → Discord).
fn guess_group(key: &str) -> String {
    let lower = key.to_lowercase();
    for g in ["Discord", "YouTube", "Google", "Cloudflare"] {
        if lower.starts_with(&g.to_lowercase()) {
            return g.into();
        }
    }
    "Другое".into()
}

pub fn parse(text: &str) -> Vec<Target> {
    let mut group: Option<String> = None;
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(h) = t.strip_prefix("###") {
            // «Public DNS (PING-only)» → «Public DNS»
            let h = h.split('(').next().unwrap_or(h).trim();
            group = (!h.is_empty()).then(|| h.to_string());
            continue;
        }
        if t.starts_with('#') || t.is_empty() {
            continue;
        }
        let Some((key, value)) = t.split_once('=') else { continue };
        let key = key.trim();
        let value = value.trim();
        let valid_key = !key.is_empty() && key.chars().all(|c| c.is_alphanumeric() || c == '_');
        let Some(value) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else { continue };
        if !valid_key || value.is_empty() {
            continue;
        }
        let g = group.clone().unwrap_or_else(|| guess_group(key));
        // Повтор ключа заменяет предыдущий (Add-OrSet в скрипте).
        out.retain(|x: &Target| x.key != key);
        out.push(make(&g, key, value));
    }
    out
}

pub fn load(root: &Path) -> Vec<Target> {
    let parsed = fs::read(root.join("utils").join("targets.txt"))
        .map(|b| parse(&String::from_utf8_lossy(&b)))
        .unwrap_or_default();
    if parsed.is_empty() {
        DEFAULTS.iter().map(|(g, k, v)| make(g, k, v)).collect()
    } else {
        parsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flowseal_targets() {
        let text = include_str!("../../../tests/fixtures/flowseal-1.10.3/targets.txt");
        let t = parse(text);
        assert_eq!(t.len(), 16);
        assert_eq!(t[0], make("Discord", "DiscordMain", "https://discord.com"));
        assert_eq!(t[1].ping, "gateway.discord.gg");
        let dns = t.iter().find(|x| x.key == "GoogleDNS8888").unwrap();
        assert_eq!((dns.group.as_str(), dns.url.as_deref(), dns.ping.as_str()), ("Public DNS", None, "8.8.8.8"));
    }

    #[test]
    fn groups_by_prefix_without_headers() {
        let t = parse("YouTubeX = \"https://y.example\"\nFoo = \"PING:1.2.3.4\"\nbad key = \"x\"");
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].group, "YouTube");
        assert_eq!(t[1].group, "Другое");
    }
}
