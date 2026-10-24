//! Извлечение аргументов winws.exe из стратегии general*.bat без запуска cmd.
//!
//! Все стратегии Flowseal устроены одинаково: одна команда
//! `start "zapret: %~n0" /min "%BIN%winws.exe" <аргументы с ^-переносами>`,
//! в аргументах только %BIN%, %LISTS%, %GameFilterTCP%, %GameFilterUDP%
//! и экранирование `^!`. Всё неожиданное — явная ошибка, а не догадка.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Vars {
    pub root: PathBuf,
    pub game_tcp: String,
    pub game_udp: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Read(String),
    NoWinws,
    MultipleWinws,
    UnknownVariable(String),
    UnbalancedQuotes,
    Empty,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(e) => write!(f, "не удалось прочитать файл: {e}"),
            Self::NoWinws => f.write_str("в файле нет запуска winws.exe"),
            Self::MultipleWinws => f.write_str("в файле несколько запусков winws.exe"),
            Self::UnknownVariable(v) => write!(f, "неизвестная переменная %{v}%"),
            Self::UnbalancedQuotes => f.write_str("незакрытая кавычка в аргументах"),
            Self::Empty => f.write_str("у winws.exe нет аргументов"),
        }
    }
}

pub fn parse_file(path: &Path, vars: &Vars) -> Result<Vec<String>, ParseError> {
    let bytes = fs::read(path).map_err(|e| ParseError::Read(e.to_string()))?;
    let text = String::from_utf8_lossy(&bytes);
    parse_text(text.trim_start_matches('\u{feff}'), vars)
}

pub fn parse_text(text: &str, vars: &Vars) -> Result<Vec<String>, ParseError> {
    let line = winws_line(text)?;
    let lower = line.to_lowercase();
    let idx = lower.find("winws.exe").ok_or(ParseError::NoWinws)?;
    let mut tail = &line[idx + "winws.exe".len()..];
    // Закрывающая кавычка пути "%BIN%winws.exe"
    if let Some(rest) = tail.strip_prefix('"') {
        tail = rest;
    }
    let expanded = expand(tail, vars)?;
    let args = tokenize(&expanded)?;
    if args.is_empty() {
        return Err(ParseError::Empty);
    }
    Ok(args)
}

/// Склеивает логическую строку с winws.exe: строки, оканчивающиеся на `^`, продолжаются.
fn winws_line(text: &str) -> Result<String, ParseError> {
    let lines: Vec<&str> = text.lines().collect();
    let is_comment = |l: &str| {
        let t = l.trim_start().to_lowercase();
        t.starts_with("::") || t.starts_with("rem ") || t == "rem"
    };
    let hits: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| !is_comment(l) && l.to_lowercase().contains("winws.exe"))
        .map(|(i, _)| i)
        .collect();
    let start = match hits.as_slice() {
        [] => return Err(ParseError::NoWinws),
        [i] => *i,
        _ => return Err(ParseError::MultipleWinws),
    };
    let mut out = String::new();
    for l in &lines[start..] {
        let t = l.trim_end();
        if let Some(body) = t.strip_suffix('^') {
            out.push_str(body);
            out.push(' ');
        } else {
            out.push_str(t);
            break;
        }
    }
    Ok(out)
}

fn expand(s: &str, vars: &Vars) -> Result<String, ParseError> {
    let mut root = vars.root.display().to_string();
    if !root.ends_with('\\') {
        root.push('\\');
    }
    let mut out = String::with_capacity(s.len() * 2);
    let mut rest = s;
    while let Some(pos) = rest.find('%') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        if let Some(r) = after.strip_prefix('%') {
            out.push('%');
            rest = r;
            continue;
        }
        if let Some(r) = after.strip_prefix("~dp0") {
            out.push_str(&root);
            rest = r;
            continue;
        }
        let end = after.find('%').ok_or_else(|| ParseError::UnknownVariable(after.chars().take(24).collect()))?;
        let name = &after[..end];
        let value = match name.to_ascii_lowercase().as_str() {
            "bin" => format!("{root}bin\\"),
            "lists" => format!("{root}lists\\"),
            "gamefiltertcp" => vars.game_tcp.clone(),
            "gamefilterudp" => vars.game_udp.clone(),
            // Старое имя из service.bat: одна переменная на оба протокола.
            "gamefilter" => vars.game_tcp.clone(),
            _ => return Err(ParseError::UnknownVariable(name.to_string())),
        };
        out.push_str(&value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Разбивает строку как cmd: пробелы вне кавычек разделяют аргументы,
/// кавычки снимаются, `^x` вне кавычек превращается в `x`.
fn tokenize(s: &str) -> Result<Vec<String>, ParseError> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut has_token = false;
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                has_token = true;
            }
            '^' if !in_quotes => {
                if let Some(next) = chars.next() {
                    cur.push(next);
                    has_token = true;
                }
            }
            c if c.is_whitespace() && !in_quotes => {
                if has_token {
                    args.push(std::mem::take(&mut cur));
                    has_token = false;
                }
            }
            c => {
                cur.push(c);
                has_token = true;
            }
        }
    }
    if in_quotes {
        return Err(ParseError::UnbalancedQuotes);
    }
    if has_token {
        args.push(cur);
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/flowseal-1.10.3")
    }

    fn vars(tcp: &str, udp: &str) -> Vars {
        Vars { root: PathBuf::from(r"C:\zapret"), game_tcp: tcp.into(), game_udp: udp.into() }
    }

    #[test]
    fn parses_every_flowseal_strategy() {
        let strategies = crate::zapret::strategy::list(&fixtures());
        assert_eq!(strategies.len(), 22);
        for s in strategies {
            let args = parse_file(&s.file, &vars("12", "12")).unwrap_or_else(|e| panic!("{}: {e}", s.name));
            assert!(args.len() > 20, "{}: слишком мало аргументов", s.name);
            assert!(args[0].starts_with("--wf-tcp="), "{}: первый аргумент {}", s.name, args[0]);
            for a in &args {
                assert!(!a.contains('%') && !a.contains('^') && !a.contains('"'), "{}: {a}", s.name);
                assert!(a.starts_with("--"), "{}: аргумент без -- : {a}", s.name);
            }
            // Все пути развёрнуты в папку zapret.
            assert!(args.iter().any(|a| a.contains(r"C:\zapret\lists\list-general.txt")), "{}", s.name);
        }
    }

    /// Свежий релиз Flowseal (CI раз в неделю): `ZAPRET_LIVE_DIR=<папка> cargo test -- --ignored live_flowseal`.
    /// Ловит изменения формата стратегий раньше пользователей.
    #[test]
    #[ignore]
    fn live_flowseal_strategies_parse() {
        let dir = std::env::var("ZAPRET_LIVE_DIR").expect("укажите ZAPRET_LIVE_DIR");
        let root = PathBuf::from(dir);
        let strategies = crate::zapret::strategy::list(&root);
        assert!(strategies.len() >= 10, "стратегий слишком мало: {}", strategies.len());
        let v = Vars { root: root.clone(), game_tcp: "12".into(), game_udp: "12".into() };
        let failed: Vec<String> = strategies
            .iter()
            .filter_map(|s| parse_file(&s.file, &v).err().map(|e| format!("{}: {e}", s.name)))
            .collect();
        assert!(failed.is_empty(), "не разобраны: {failed:#?}");
        println!("разобрано стратегий: {}", strategies.len());
    }

    #[test]
    fn general_bat_exact_head() {
        let args = parse_file(&fixtures().join("general.bat"), &vars("12", "12")).unwrap();
        assert_eq!(args[0], "--wf-tcp=80,443,2053,2083,2087,2096,8443,12");
        assert_eq!(args[1], "--wf-udp=443,19294-19344,50000-50100,12");
        assert_eq!(args[2], "--filter-udp=443");
        assert_eq!(args[3], r"--hostlist=C:\zapret\lists\list-general.txt");
        assert_eq!(args.iter().filter(|a| *a == "--new").count(), 8);
    }

    #[test]
    fn game_filter_values_are_substituted() {
        let args = parse_file(&fixtures().join("general.bat"), &vars("1024-65535", "12")).unwrap();
        assert!(args[0].ends_with(",1024-65535"));
        assert!(args.contains(&"--filter-tcp=1024-65535".to_string()));
        assert!(args.contains(&"--filter-udp=12".to_string()));
    }

    #[test]
    fn caret_bang_becomes_bang() {
        let args = parse_file(&fixtures().join("general (FAKE TLS AUTO).bat"), &vars("12", "12")).unwrap();
        assert!(args.contains(&"--dpi-desync-fake-tls=!".to_string()));
    }

    #[test]
    fn errors_are_explicit() {
        let v = vars("12", "12");
        assert_eq!(parse_text("echo hi", &v), Err(ParseError::NoWinws));
        assert_eq!(
            parse_text("start \"\" \"%BIN%winws.exe\" --x=%FOO%", &v),
            Err(ParseError::UnknownVariable("FOO".into()))
        );
        assert_eq!(parse_text("\"%BIN%winws.exe\" --a\r\n\"%BIN%winws.exe\" --b", &v), Err(ParseError::MultipleWinws));
        assert_eq!(parse_text(":: winws.exe в комментарии\r\n\"%BIN%winws.exe\" --a=1", &v), Ok(vec!["--a=1".into()]));
    }
}
