//! Результаты проверки для таблицы «Стратегии» и подсчёт лучшей стратегии.

use serde::{Deserialize, Serialize};

use super::curl::{DpiLine, DpiStatus, Tok};
use super::targets::Target;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Standard,
    Dpi,
    /// Один сайт («Мой сайт»): колонки HTTP / TLS 1.2 / TLS 1.3.
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CellState {
    Ok,
    Fail,
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    pub state: CellState,
    pub hint: String,
}

impl Cell {
    pub fn none() -> Self {
        Cell { state: CellState::None, hint: String::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Detail {
    pub target: String,
    pub text: String,
    pub ok: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RowState {
    Pending,
    Running,
    Done,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub name: String,
    pub state: RowState,
    pub cells: Vec<Cell>,
    pub total: Option<String>,
    pub score: u32,
    pub ping_ok: u32,
    pub details: Vec<Detail>,
    pub error: Option<String>,
}

impl Row {
    pub fn pending(name: &str, columns: usize) -> Self {
        Row {
            name: name.into(),
            state: RowState::Pending,
            cells: vec![Cell::none(); columns],
            total: None,
            score: 0,
            ping_ok: 0,
            details: vec![],
            error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub kind: Kind,
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
    pub running: bool,
    pub cancelled: bool,
    pub best: Option<String>,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    /// Что происходит сейчас: «Подготовка…», «Discord…».
    pub phase: Option<String>,
    pub report: Option<String>,
    pub error: Option<String>,
    /// Быстрый подбор: только Discord и YouTube, остановка на первой рабочей стратегии.
    #[serde(default)]
    pub quick: bool,
    /// Сайт для Kind::Custom.
    #[serde(default)]
    pub site: Option<String>,
}

pub const STANDARD_COLUMNS: [&str; 3] = ["Discord", "YouTube", "Google"];
pub const DPI_COLUMNS: [&str; 3] = ["HTTP", "TLS 1.2", "TLS 1.3"];

/// Результат одной цели стандартного теста.
#[derive(Debug, Clone)]
pub struct TargetResult {
    pub target: Target,
    pub tokens: Vec<(&'static str, Tok)>,
    pub ping: Option<u32>,
    /// Пинговали ли цель (быстрый подбор не пингует).
    pub pinged: bool,
}

impl TargetResult {
    pub fn ok(&self) -> bool {
        if self.target.url.is_some() {
            let considered: Vec<_> = self.tokens.iter().filter(|(_, t)| *t != Tok::Unsup).collect();
            !considered.is_empty() && considered.iter().all(|(_, t)| *t == Tok::Ok)
        } else {
            self.ping.is_some()
        }
    }

    pub fn line(&self) -> String {
        let ping = self.ping.map(|ms| format!("{ms} ms")).unwrap_or_else(|| "Timeout".into());
        if self.target.url.is_some() {
            let toks: Vec<String> = self.tokens.iter().map(|(l, t)| format!("{l}:{}", t.as_str())).collect();
            if !self.pinged {
                return toks.join(" ");
            }
            format!("{} | Ping: {ping}", toks.join(" "))
        } else {
            format!("Ping: {ping}")
        }
    }
}

/// Строка таблицы по результатам стандартного теста.
pub fn standard_row(name: &str, results: &[TargetResult]) -> Row {
    let mut groups: Vec<(String, Vec<&TargetResult>)> = Vec::new();
    for r in results {
        match groups.iter_mut().find(|(g, _)| *g == r.target.group) {
            Some((_, v)) => v.push(r),
            None => groups.push((r.target.group.clone(), vec![r])),
        }
    }
    let group_ok = |items: &[&TargetResult]| items.iter().all(|r| r.ok());

    let cells = STANDARD_COLUMNS
        .iter()
        .map(|col| match groups.iter().find(|(g, _)| g.eq_ignore_ascii_case(col)) {
            None => Cell::none(),
            Some((_, items)) => {
                let ok = items.iter().filter(|r| r.ok()).count();
                let bad: Vec<String> =
                    items.iter().filter(|r| !r.ok()).map(|r| format!("{}: {}", r.target.ping, r.line())).collect();
                let mut hint = format!("{ok}/{} целей", items.len());
                if !bad.is_empty() {
                    hint.push_str(" · ");
                    hint.push_str(&bad.join("; "));
                }
                Cell { state: if group_ok(items) { CellState::Ok } else { CellState::Fail }, hint }
            }
        })
        .collect();

    let groups_ok = groups.iter().filter(|(_, v)| group_ok(v)).count();
    let score = results.iter().flat_map(|r| &r.tokens).filter(|(_, t)| *t == Tok::Ok).count() as u32;
    let ping_ok = results.iter().filter(|r| r.ping.is_some()).count() as u32;
    let details = results.iter().map(|r| Detail { target: r.target.key.clone(), text: r.line(), ok: r.ok() }).collect();
    Row {
        name: name.into(),
        state: RowState::Done,
        cells,
        total: Some(format!("{groups_ok}/{}", groups.len())),
        score,
        ping_ok,
        details,
        error: None,
    }
}

/// Все цели прошли (для остановки быстрого подбора на первой рабочей стратегии).
pub fn all_ok(results: &[TargetResult]) -> bool {
    !results.is_empty() && results.iter().all(TargetResult::ok)
}

/// Строка «Моего сайта»: колонки — протоколы, итог — сколько запросов прошло.
pub fn custom_row(name: &str, results: &[TargetResult]) -> Row {
    let cells = super::curl::LABELS
        .iter()
        .map(|label| {
            let toks: Vec<Tok> =
                results.iter().flat_map(|r| r.tokens.iter().filter(|(l, _)| l == label).map(|(_, t)| *t)).collect();
            let considered: Vec<Tok> = toks.iter().copied().filter(|t| *t != Tok::Unsup).collect();
            if considered.is_empty() {
                return Cell { state: CellState::None, hint: "не поддерживается".into() };
            }
            let ok = considered.iter().all(|t| *t == Tok::Ok);
            let hint = considered.iter().map(|t| t.as_str()).collect::<Vec<_>>().join(", ");
            Cell { state: if ok { CellState::Ok } else { CellState::Fail }, hint }
        })
        .collect();
    let toks: Vec<Tok> = results.iter().flat_map(|r| r.tokens.iter().map(|(_, t)| *t)).collect();
    let considered = toks.iter().filter(|t| **t != Tok::Unsup).count();
    let ok = toks.iter().filter(|t| **t == Tok::Ok).count() as u32;
    let details = results.iter().map(|r| Detail { target: r.target.key.clone(), text: r.line(), ok: r.ok() }).collect();
    Row {
        name: name.into(),
        state: RowState::Done,
        cells,
        total: Some(format!("{ok}/{considered}")),
        score: ok,
        ping_ok: 0,
        details,
        error: None,
    }
}

/// Цель DPI-набора (hyperion-cs/dpi-checkers).
#[derive(Debug, Clone, Deserialize)]
pub struct DpiTarget {
    pub id: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub country: String,
    pub host: String,
}

pub fn dpi_row(name: &str, results: &[(DpiTarget, Vec<DpiLine>)]) -> Row {
    let labels = super::curl::LABELS;
    let cells = labels
        .iter()
        .map(|label| {
            let lines: Vec<(&DpiTarget, &DpiLine)> = results
                .iter()
                .flat_map(|(t, ls)| ls.iter().filter(|l| l.label == *label).map(move |l| (t, l)))
                .collect();
            let bad: Vec<&(&DpiTarget, &DpiLine)> =
                lines.iter().filter(|(_, l)| matches!(l.status, DpiStatus::Fail | DpiStatus::LikelyBlocked)).collect();
            let considered = lines.iter().filter(|(_, l)| l.status != DpiStatus::Unsupported).count();
            if considered == 0 {
                return Cell { state: CellState::None, hint: "не поддерживается".into() };
            }
            let blocked = bad.iter().filter(|(_, l)| l.status == DpiStatus::LikelyBlocked).count();
            let mut hint = format!("{}/{considered} целей без блокировки", considered - bad.len());
            if blocked > 0 {
                hint.push_str(&format!(" · заморозка 16–20 КБ: {blocked}"));
            }
            Cell { state: if bad.is_empty() { CellState::Ok } else { CellState::Fail }, hint }
        })
        .collect();

    let all: Vec<&DpiLine> = results.iter().flat_map(|(_, ls)| ls).collect();
    let ok = all.iter().filter(|l| l.status == DpiStatus::Ok).count() as u32;
    let considered = all.iter().filter(|l| l.status != DpiStatus::Unsupported).count();
    let details = results
        .iter()
        .map(|(t, ls)| {
            let toks: Vec<String> = ls.iter().map(|l| format!("{}:{}", l.label, l.status.as_str())).collect();
            let country = if t.country.is_empty() { String::new() } else { format!("[{}] ", t.country) };
            Detail {
                target: format!("{country}{} {}", t.provider, t.id),
                text: toks.join(" "),
                ok: ls.iter().all(|l| matches!(l.status, DpiStatus::Ok | DpiStatus::Unsupported)),
            }
        })
        .collect();
    Row {
        name: name.into(),
        state: RowState::Done,
        cells,
        total: Some(format!("{ok}/{considered}")),
        score: ok,
        ping_ok: 0,
        details,
        error: None,
    }
}

/// Лучшая стратегия по формуле скрипта: больше OK, при равенстве — больше удачных пингов.
/// Ничья — первая по порядку; ни одного OK — лучшей нет.
pub fn best(rows: &[Row]) -> Option<String> {
    let mut best: Option<&Row> = None;
    for r in rows.iter().filter(|r| r.state == RowState::Done && r.score > 0) {
        match best {
            None => best = Some(r),
            Some(b) if r.score > b.score || (r.score == b.score && r.ping_ok > b.ping_ok) => best = Some(r),
            _ => {}
        }
    }
    best.map(|r| r.name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(group: &str, key: &str, url: bool) -> Target {
        Target { key: key.into(), group: group.into(), url: url.then(|| format!("https://{key}")), ping: key.into() }
    }

    fn res(group: &str, key: &str, toks: [Tok; 3], ping: Option<u32>) -> TargetResult {
        TargetResult {
            target: target(group, key, true),
            tokens: vec![("HTTP", toks[0]), ("TLS1.2", toks[1]), ("TLS1.3", toks[2])],
            ping,
            pinged: true,
        }
    }

    #[test]
    fn groups_cells_and_total() {
        use Tok::*;
        let results = vec![
            res("Discord", "d1", [Ok, Ok, Ok], Some(10)),
            res("Discord", "d2", [Ok, Ok, Unsup], Some(10)),
            res("YouTube", "y1", [Ok, Error, Ok], None),
            res("Cloudflare", "c1", [Ok, Ok, Ok], Some(5)),
            TargetResult {
                target: target("Public DNS", "8.8.8.8", false),
                tokens: vec![],
                ping: Some(3),
                pinged: true,
            },
        ];
        let row = standard_row("general", &results);
        assert_eq!(
            row.cells.iter().map(|c| c.state).collect::<Vec<_>>(),
            vec![CellState::Ok, CellState::Fail, CellState::None]
        );
        assert_eq!(row.total.as_deref(), Some("3/4"));
        assert_eq!(row.score, 10);
        assert_eq!(row.ping_ok, 4);
        assert!(row.cells[1].hint.starts_with("0/1 целей"));
    }

    #[test]
    fn best_prefers_score_then_ping_then_order() {
        let mk = |name: &str, score, ping| Row { score, ping_ok: ping, state: RowState::Done, ..Row::pending(name, 3) };
        assert_eq!(best(&[mk("a", 5, 1), mk("b", 7, 0), mk("c", 7, 2)]).as_deref(), Some("c"));
        assert_eq!(best(&[mk("a", 7, 2), mk("b", 7, 2)]).as_deref(), Some("a"));
        assert_eq!(best(&[mk("a", 0, 5)]), None);
    }
}
