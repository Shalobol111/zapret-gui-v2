//! Список стратегий: все *.bat в корне, кроме service*, в «натуральном» порядке —
//! так же, как их показывает service.bat (числа дополняются нулями до 8 знаков).

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct Strategy {
    /// Имя файла без .bat — так стратегия называется в интерфейсе и в реестре службы.
    pub name: String,
    pub file: PathBuf,
}

pub fn natural_key(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 16);
    let mut digits = String::new();
    for c in name.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        if !digits.is_empty() {
            out.push_str(&format!("{digits:0>8}"));
            digits.clear();
        }
        out.extend(c.to_lowercase());
    }
    if !digits.is_empty() {
        out.push_str(&format!("{digits:0>8}"));
    }
    out
}

pub fn list(root: &Path) -> Vec<Strategy> {
    let Ok(entries) = fs::read_dir(root) else { return vec![] };
    let mut out: Vec<Strategy> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let file_name = e.file_name().to_string_lossy().into_owned();
            let lower = file_name.to_lowercase();
            if !lower.ends_with(".bat") || lower.starts_with("service") {
                return None;
            }
            let name = file_name[..file_name.len() - 4].to_string();
            Some(Strategy { name, file: e.path() })
        })
        .collect();
    out.sort_by_cached_key(|s| natural_key(&format!("{}.bat", s.name)));
    out
}

pub fn find(root: &Path, name: &str) -> Option<Strategy> {
    list(root).into_iter().find(|s| s.name.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_matches_service_bat() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/flowseal-1.10.3");
        let names: Vec<String> = list(&root).into_iter().map(|s| s.name).collect();
        assert_eq!(names.len(), 22);
        assert_eq!(names[0], "general (ALT)");
        assert_eq!(names[1], "general (ALT2)");
        // ALT10 идёт после ALT9, а не после ALT1.
        let alt9 = names.iter().position(|n| n == "general (ALT9)").unwrap();
        assert_eq!(names[alt9 + 1], "general (ALT10)");
        assert_eq!(names.last().unwrap(), "general");
    }
}
