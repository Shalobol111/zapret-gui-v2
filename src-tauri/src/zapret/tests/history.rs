//! История проверок стратегий: когда стратегия последний раз открывала Discord и YouTube.
//! Быстрый подбор сначала пробует те, что работали недавно, — обычно находит рабочую
//! с первой-второй попытки. Файл: `%APPDATA%\ZapretGUI\history.json`.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub last_ok: Option<u64>,
    pub last_fail: Option<u64>,
    pub ok_count: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct History {
    pub strategies: HashMap<String, Entry>,
}

fn file() -> PathBuf {
    crate::settings::data_dir().join("history.json")
}

impl History {
    pub fn load() -> Self {
        fs::read_to_string(file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = fs::create_dir_all(crate::settings::data_dir());
            let _ = fs::write(file(), text);
        }
    }

    pub fn record(&mut self, name: &str, ok: bool, now: u64) {
        let e = self.strategies.entry(name.to_string()).or_default();
        if ok {
            e.last_ok = Some(now);
            e.ok_count += 1;
        } else {
            e.last_fail = Some(now);
        }
    }

    /// Порядок перебора: сначала `first` (например, текущая), затем стратегии, которые
    /// работали и с тех пор не ломались, — от недавних к давним; потом остальные в исходном
    /// порядке; последними — те, что недавно не работали.
    pub fn order(&self, names: &[String], first: Option<&str>) -> Vec<String> {
        let score = |n: &String| -> (u8, i128) {
            match self.strategies.get(n) {
                Some(Entry { last_ok: Some(ok), last_fail, .. }) if last_fail.is_none_or(|f| f < *ok) => {
                    (0, -(*ok as i128))
                }
                Some(Entry { last_fail: Some(_), .. }) => (2, 0),
                _ => (1, 0),
            }
        };
        let mut rest: Vec<(usize, &String)> =
            names.iter().enumerate().filter(|(_, n)| Some(n.as_str()) != first).collect();
        rest.sort_by_key(|(i, n)| {
            let (group, recency) = score(n);
            (group, recency, *i)
        });
        let mut out: Vec<String> = Vec::with_capacity(names.len());
        if let Some(f) = first.filter(|f| names.iter().any(|n| n == f)) {
            out.push(f.to_string());
        }
        out.extend(rest.into_iter().map(|(_, n)| n.clone()));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn recent_successes_first_failures_last() {
        let mut h = History::default();
        h.record("B", true, 100);
        h.record("D", true, 200);
        h.record("C", false, 150);
        h.record("A", true, 50);
        h.record("A", false, 60); // работала, потом сломалась
        let order = h.order(&names(&["A", "B", "C", "D", "E"]), None);
        assert_eq!(order, names(&["D", "B", "E", "A", "C"]));
    }

    #[test]
    fn current_goes_first() {
        let mut h = History::default();
        h.record("B", true, 100);
        assert_eq!(h.order(&names(&["A", "B", "C"]), Some("C")), names(&["C", "B", "A"]));
        // Несуществующая «текущая» не добавляется.
        assert_eq!(h.order(&names(&["A", "B"]), Some("X")), names(&["B", "A"]));
    }
}
