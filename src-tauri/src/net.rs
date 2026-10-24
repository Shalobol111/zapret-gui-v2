//! HTTP: общий клиент, понятные тексты ошибок (идея из zapret-gui), скачивание файлов.

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use sha2::{Digest, Sha256};

pub fn friendly(e: reqwest::Error) -> String {
    let text = format!("{e:?}").to_lowercase();
    if e.is_timeout() || text.contains("timed out") {
        "сервер не отвечает (таймаут). Возможно, домен заблокирован — попробуйте при включённом обходе".into()
    } else if text.contains("certificate") || text.contains("ssl") || text.contains("tls") {
        "не удалось установить защищённое соединение — возможно, домен заблокирован".into()
    } else if text.contains("dns") || text.contains("resolve") || text.contains("11001") {
        "нет подключения к интернету или DNS недоступен".into()
    } else if e.is_connect() {
        "не удалось связаться с сервером".into()
    } else {
        format!("ошибка сети: {e}")
    }
}

pub fn client(timeout: Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(concat!("ZapretGUI/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())
}

pub async fn get_text(url: &str, timeout: Duration) -> Result<String, String> {
    let resp = client(timeout)?.get(url).header("Cache-Control", "no-cache").send().await.map_err(friendly)?;
    if !resp.status().is_success() {
        return Err(format!("сервер ответил {}", resp.status()));
    }
    resp.text().await.map_err(friendly)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    // Файлы фейков маленькие (≈1 КБ) — читаем целиком.
    let bytes = std::fs::read(path)?;
    Ok(hex(&Sha256::digest(&bytes)))
}

/// Скачивает в файл; `progress(получено, всего)`. Возвращает SHA-256.
pub async fn download(url: &str, dest: &Path, mut progress: impl FnMut(u64, Option<u64>)) -> Result<String, String> {
    let mut resp = client(Duration::from_secs(300))?.get(url).send().await.map_err(friendly)?;
    if !resp.status().is_success() {
        return Err(format!("сервер ответил {}", resp.status()));
    }
    let total = resp.content_length();
    let mut file = File::create(dest).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut got = 0u64;
    while let Some(chunk) = resp.chunk().await.map_err(friendly)? {
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        hasher.update(&chunk);
        got += chunk.len() as u64;
        progress(got, total);
    }
    file.flush().map_err(|e| e.to_string())?;
    Ok(hex(&hasher.finalize()))
}
