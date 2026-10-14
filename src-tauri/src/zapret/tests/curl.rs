//! Проверки через системный curl.exe — те же параметры и та же классификация,
//! что в test zapret.ps1 (результаты сопоставимы со скриптом).

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::sys::cmd;

pub fn curl_path() -> Option<PathBuf> {
    let sys = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| r"C:\Windows".into());
    let p = sys.join(r"System32\curl.exe");
    p.is_file().then_some(p)
}

/// curl без прокси из переменных окружения (HTTP_PROXY и т. п.): проверяем, что открывается
/// через zapret, а не через прокси — иначе результаты ничего не говорят о стратегии.
pub fn command(curl: &Path) -> std::process::Command {
    let mut c = cmd::hidden(&curl.to_string_lossy());
    c.args(["--noproxy", "*"]);
    c
}

pub const LABELS: [&str; 3] = ["HTTP", "TLS1.2", "TLS1.3"];

fn protocol_args(label: &str) -> &'static [&'static str] {
    match label {
        "HTTP" => &["--http1.1"],
        "TLS1.2" => &["--tlsv1.2", "--tls-max", "1.2"],
        _ => &["--tlsv1.3", "--tls-max", "1.3"],
    }
}

fn is_unsupported(exit: Option<i32>, text: &str) -> bool {
    let t = text.to_lowercase();
    exit == Some(35)
        || [
            "does not support",
            "not supported",
            "unsupported protocol",
            "unrecognized option",
            "unknown option",
            "unsupported option",
            "unsupported feature",
            "schannel",
        ]
        .iter()
        .any(|p| t.contains(p))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tok {
    Ok,
    Error,
    Unsup,
    Ssl,
}

impl Tok {
    pub fn as_str(self) -> &'static str {
        match self {
            Tok::Ok => "OK",
            Tok::Error => "ERROR",
            Tok::Unsup => "UNSUP",
            Tok::Ssl => "SSL",
        }
    }
}

pub fn classify_standard(exit: Option<i32>, stderr: &str) -> Tok {
    let e = stderr.to_lowercase();
    // Подмена DNS или сертификата провайдером.
    if [
        "could not resolve host",
        "certificate",
        "ssl certificate problem",
        "self signed",
        "self-signed",
        "certificate verify failed",
        "unable to get local issuer certificate",
    ]
    .iter()
    .any(|p| e.contains(p))
    {
        return Tok::Ssl;
    }
    if is_unsupported(exit, stderr) {
        return Tok::Unsup;
    }
    if exit == Some(0) {
        Tok::Ok
    } else {
        Tok::Error
    }
}

/// HEAD-запросы HTTP/1.1, TLS 1.2, TLS 1.3 к URL.
pub fn standard(curl: &Path, url: &str, timeout: u32) -> Vec<(&'static str, Tok)> {
    let connect = timeout.min(2).to_string();
    let timeout = timeout.to_string();
    LABELS
        .iter()
        .map(|label| {
            let out = command(curl)
                .args([
                    "-I",
                    "-s",
                    "-m",
                    &timeout,
                    "--connect-timeout",
                    &connect,
                    "-o",
                    "NUL",
                    "-w",
                    "%{http_code}",
                    "--show-error",
                ])
                .args(protocol_args(label))
                .arg(url)
                .output();
            let tok = match out {
                Ok(o) => classify_standard(o.status.code(), &String::from_utf8_lossy(&o.stderr)),
                Err(_) => Tok::Error,
            };
            (*label, tok)
        })
        .collect()
}

/// Те же запросы, но выбранные протоколы — параллельно: для быстрого подбора, где важна
/// скорость на нерабочих стратегиях (каждый запрос может ждать таймаут).
pub fn check_parallel(curl: &Path, url: &str, timeout: u32, labels: &[&'static str]) -> Vec<(&'static str, Tok)> {
    let connect = timeout.min(2).to_string();
    let t = timeout.to_string();
    std::thread::scope(|s| {
        let handles: Vec<_> = labels
            .iter()
            .map(|label| {
                let (connect, t) = (&connect, &t);
                s.spawn(move || {
                    let out = command(curl)
                        .args(["-I", "-s", "-m", t, "--connect-timeout", connect, "-o", "NUL", "-w", "%{http_code}"])
                        .arg("--show-error")
                        .args(protocol_args(label))
                        .arg(url)
                        .output();
                    let tok = match out {
                        Ok(o) => classify_standard(o.status.code(), &String::from_utf8_lossy(&o.stderr)),
                        Err(_) => Tok::Error,
                    };
                    (*label, tok)
                })
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DpiStatus {
    Ok,
    Fail,
    Unsupported,
    LikelyBlocked,
}

impl DpiStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            DpiStatus::Ok => "OK",
            DpiStatus::Fail => "FAIL",
            DpiStatus::Unsupported => "UNSUPPORTED",
            DpiStatus::LikelyBlocked => "LIKELY_BLOCKED",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DpiLine {
    pub label: &'static str,
    pub status: DpiStatus,
    pub code: String,
    pub up: u64,
    pub down: u64,
    pub time: f64,
}

/// Разбор вывода `-w "%{http_code} %{size_upload} %{size_download} %{time_total}"`.
pub fn classify_dpi(label: &'static str, exit: Option<i32>, text: &str, timeout: u32) -> DpiLine {
    let text = text.trim();
    let parts: Vec<&str> = text.split_whitespace().collect();
    let parsed = (parts.len() == 4 && parts[0].len() == 3 && parts[0].bytes().all(|b| b.is_ascii_digit()))
        .then(|| (parts[1].parse::<u64>(), parts[2].parse::<u64>(), parts[3].parse::<f64>()));
    let (code, up, down, time) = match parsed {
        Some((Ok(u), Ok(d), Ok(t))) => (parts[0].to_string(), u, d, t),
        _ if is_unsupported(exit, text) || text.contains("SSL") => ("UNSUP".into(), 0, 0, -1.0),
        _ if !text.is_empty() => ("ERR".into(), 0, 0, -1.0),
        _ => ("NA".into(), 0, 0, -1.0),
    };
    let mut status = if code == "UNSUP" {
        DpiStatus::Unsupported
    } else if exit != Some(0) || code == "ERR" || code == "NA" {
        DpiStatus::Fail
    } else {
        DpiStatus::Ok
    };
    // Данные ушли, ответа нет, сработал таймаут — характерная «заморозка» на 16–20 КБ.
    if up > 0 && down == 0 && time >= timeout as f64 && exit != Some(0) {
        status = DpiStatus::LikelyBlocked;
    }
    DpiLine { label, status, code, up, down, time }
}

pub fn dpi(curl: &Path, host: &str, payload: &Path, timeout: u32, range: u32) -> Vec<DpiLine> {
    let range = format!("0-{}", range - 1);
    let connect = timeout.min(3).to_string();
    let t = timeout.to_string();
    let data = format!("@{}", payload.display());
    LABELS
        .iter()
        .map(|label| {
            let out = command(curl)
                .args(["--range", &range, "-m", &t, "--connect-timeout", &connect])
                .args(["-w", "%{http_code} %{size_upload} %{size_download} %{time_total}"])
                .args(["-o", "NUL", "-X", "POST", "--data-binary", &data, "-s"])
                .args(protocol_args(label))
                .arg(format!("https://{host}"))
                .output();
            match out {
                Ok(o) => classify_dpi(label, o.status.code(), &cmd::text(&o), timeout),
                Err(_) => classify_dpi(label, None, "", timeout),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_classification() {
        assert_eq!(classify_standard(Some(0), ""), Tok::Ok);
        assert_eq!(classify_standard(Some(28), "curl: (28) Operation timed out"), Tok::Error);
        assert_eq!(classify_standard(Some(35), "curl: (35) schannel: failed"), Tok::Unsup);
        assert_eq!(classify_standard(Some(60), "curl: (60) SSL certificate problem: self signed"), Tok::Ssl);
        assert_eq!(classify_standard(Some(6), "curl: (6) Could not resolve host: x"), Tok::Ssl);
    }

    #[test]
    fn dpi_classification() {
        let ok = classify_dpi("HTTP", Some(0), "200 65536 1234 0.512", 5);
        assert_eq!((ok.status, ok.code.as_str(), ok.up), (DpiStatus::Ok, "200", 65536));
        let frozen = classify_dpi("TLS1.3", Some(28), "000 65536 0 5.004", 5);
        assert_eq!(frozen.status, DpiStatus::LikelyBlocked);
        let refused = classify_dpi("TLS1.2", Some(7), "000 0 0 0.010", 5);
        assert_eq!(refused.status, DpiStatus::Fail);
        assert_eq!(classify_dpi("TLS1.3", Some(35), "", 5).status, DpiStatus::Unsupported);
    }
}
