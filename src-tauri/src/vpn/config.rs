//! Генератор config.json для sing-box (формат 1.12+: правила-действия sniff/hijack-dns, новые DNS-серверы).

use serde_json::{json, Value};

use super::{Inbound, RouteMode};

/// Всё, из чего собирается конфиг.
pub struct Input {
    pub inbound: Inbound,
    pub route_mode: RouteMode,
    pub port: u16,
    /// Серверы для выхода «proxy»: один — выбранный, несколько — автовыбор быстрейшего (urltest).
    pub servers: Vec<Value>,
    pub vpn_custom: Vec<String>,
    pub direct_custom: Vec<String>,
    /// Домены из списков zapret — напрямую (их обходит zapret). Пусто — опция выключена.
    pub zapret_direct: Vec<String>,
    /// Режим «Списки → VPN»: домены включённых наборов.
    pub vpn_presets: Vec<String>,
    /// Режим «Всё через VPN, кроме РФ»: российские домены.
    pub ru_direct: Vec<String>,
    pub log_file: String,
}

/// sing-box: суффикс с точкой в начале совпадает только с поддоменами — убираем точку.
fn suffixes(list: &[String]) -> Vec<String> {
    let mut v: Vec<String> =
        list.iter().map(|d| d.trim_start_matches('.').to_string()).filter(|d| !d.is_empty()).collect();
    v.sort();
    v.dedup();
    v
}

fn rule_set(tag: &str, domains: &[String]) -> Value {
    json!({ "type": "inline", "tag": tag, "rules": [{ "domain_suffix": suffixes(domains) }] })
}

pub fn build(i: &Input) -> Value {
    let mut outbounds: Vec<Value> = Vec::new();
    match i.servers.len() {
        0 => {}
        1 => {
            let mut o = i.servers[0].clone();
            o["tag"] = json!("proxy");
            outbounds.push(o);
        }
        _ => {
            let tags: Vec<String> = (0..i.servers.len()).map(|n| format!("server-{n}")).collect();
            outbounds.push(json!({
                "type": "urltest", "tag": "proxy", "outbounds": tags,
                "url": "https://www.gstatic.com/generate_204", "interval": "3m", "tolerance": 50,
            }));
            for (n, s) in i.servers.iter().enumerate() {
                let mut o = s.clone();
                o["tag"] = json!(format!("server-{n}"));
                outbounds.push(o);
            }
        }
    }
    outbounds.push(json!({ "type": "direct", "tag": "direct" }));

    let mut rules = vec![json!({ "action": "sniff" })];
    if i.inbound == Inbound::Tun {
        rules.push(json!({ "protocol": "dns", "action": "hijack-dns" }));
    }
    rules.push(json!({ "ip_is_private": true, "outbound": "direct" }));
    let mut sets = Vec::new();
    let mut add = |tag: &str, domains: &[String], outbound: &str, rules: &mut Vec<Value>| {
        if !domains.is_empty() {
            sets.push(rule_set(tag, domains));
            rules.push(json!({ "rule_set": tag, "outbound": outbound }));
        }
    };
    add("direct-custom", &i.direct_custom, "direct", &mut rules);
    add("vpn-custom", &i.vpn_custom, "proxy", &mut rules);
    add("zapret", &i.zapret_direct, "direct", &mut rules);
    let final_out = match i.route_mode {
        RouteMode::Lists => {
            add("vpn-presets", &i.vpn_presets, "proxy", &mut rules);
            "direct"
        }
        RouteMode::AllExceptRu => {
            add("ru-direct", &i.ru_direct, "direct", &mut rules);
            "proxy"
        }
    };

    let inbounds = match i.inbound {
        Inbound::Proxy => {
            vec![json!({ "type": "mixed", "tag": "mixed-in", "listen": "127.0.0.1", "listen_port": i.port })]
        }
        Inbound::Tun => vec![json!({
            "type": "tun", "tag": "tun-in", "interface_name": "ZapretGUI",
            "address": ["172.19.0.1/30", "fdfe:dcba:9876::1/126"],
            "auto_route": true, "strict_route": true, "stack": "mixed",
        })],
    };

    // DNS: сайты, идущие через VPN, резолвятся через VPN (DoH 1.1.1.1), остальное — системным DNS.
    let (dns_rules, dns_final) = match i.route_mode {
        RouteMode::Lists => {
            let vpn_sets: Vec<Value> = sets
                .iter()
                .filter(|s| s["tag"] == "vpn-presets" || s["tag"] == "vpn-custom")
                .map(|s| s["tag"].clone())
                .collect();
            let rules =
                if vpn_sets.is_empty() { vec![] } else { vec![json!({ "rule_set": vpn_sets, "server": "remote" })] };
            (rules, "local")
        }
        RouteMode::AllExceptRu => {
            let direct_sets: Vec<Value> = sets
                .iter()
                .filter(|s| s["tag"] == "ru-direct" || s["tag"] == "direct-custom" || s["tag"] == "zapret")
                .map(|s| s["tag"].clone())
                .collect();
            let rules = if direct_sets.is_empty() {
                vec![]
            } else {
                vec![json!({ "rule_set": direct_sets, "server": "local" })]
            };
            (rules, "remote")
        }
    };

    json!({
        "log": { "level": "warn", "timestamp": true, "output": i.log_file },
        "dns": {
            "servers": [
                { "type": "local", "tag": "local" },
                { "type": "https", "tag": "remote", "server": "1.1.1.1", "detour": "proxy" }
            ],
            "rules": dns_rules,
            "final": dns_final,
        },
        "inbounds": inbounds,
        "outbounds": outbounds,
        "route": {
            "rules": rules,
            "rule_set": sets,
            "final": final_out,
            "auto_detect_interface": true,
            "default_domain_resolver": "local",
        },
        "experimental": { "cache_file": { "enabled": true } },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(mode: RouteMode) -> Input {
        Input {
            inbound: Inbound::Proxy,
            route_mode: mode,
            port: 2080,
            servers: vec![json!({ "type": "trojan", "server": "a.com", "server_port": 443, "password": "p" })],
            vpn_custom: vec!["my-vpn.com".into()],
            direct_custom: vec![],
            zapret_direct: vec!["youtube.com".into(), ".googlevideo.com".into()],
            vpn_presets: vec!["chatgpt.com".into(), ".ua".into()],
            ru_direct: vec!["ru".into()],
            log_file: "log.txt".into(),
        }
    }

    #[test]
    fn lists_mode_order_and_final() {
        let c = build(&input(RouteMode::Lists));
        let rules = c["route"]["rules"].as_array().unwrap();
        let order: Vec<&str> = rules.iter().filter_map(|r| r["rule_set"].as_str()).collect();
        assert_eq!(order, vec!["vpn-custom", "zapret", "vpn-presets"], "zapret раньше наборов VPN");
        assert_eq!(c["route"]["final"], "direct");
        assert_eq!(c["outbounds"][0]["tag"], "proxy");
        let presets = c["route"]["rule_set"].as_array().unwrap().iter().find(|s| s["tag"] == "vpn-presets").unwrap();
        assert_eq!(presets["rules"][0]["domain_suffix"], json!(["chatgpt.com", "ua"]));
        assert!(c["route"]["rule_set"].as_array().unwrap().iter().all(|s| s["tag"] != "ru-direct"));
    }

    #[test]
    fn all_except_ru_and_urltest() {
        let mut i = input(RouteMode::AllExceptRu);
        i.servers.push(json!({ "type": "vless", "server": "b.com", "server_port": 443, "uuid": "u" }));
        let c = build(&i);
        assert_eq!(c["route"]["final"], "proxy");
        assert_eq!(c["outbounds"][0]["type"], "urltest");
        assert_eq!(c["outbounds"][0]["outbounds"], json!(["server-0", "server-1"]));
        assert_eq!(c["outbounds"].as_array().unwrap().last().unwrap()["tag"], "direct");
        assert!(c["route"]["rule_set"].as_array().unwrap().iter().any(|s| s["tag"] == "ru-direct"));
    }

    #[test]
    fn tun_inbound_hijacks_dns() {
        let mut i = input(RouteMode::Lists);
        i.inbound = Inbound::Tun;
        let c = build(&i);
        assert_eq!(c["inbounds"][0]["type"], "tun");
        assert!(c["route"]["rules"].as_array().unwrap().iter().any(|r| r["action"] == "hijack-dns"));
    }
}
