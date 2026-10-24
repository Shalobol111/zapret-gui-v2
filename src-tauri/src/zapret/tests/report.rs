//! Отчёт в `utils\test results\test_results_<дата>.txt` — в формате test zapret.ps1,
//! чтобы его можно было приложить к issue в репозитории Flowseal.

use std::fs;
use std::path::{Path, PathBuf};

use super::model::{Kind, RowState, Run};

pub fn write(root: &Path, run: &Run) -> std::io::Result<PathBuf> {
    let dir = root.join("utils").join("test results");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("test_results_{}.txt", crate::journal::file_stamp()));
    let kind = match run.kind {
        Kind::Standard => "standard",
        Kind::Dpi => "dpi",
        Kind::Custom => "custom",
    };
    let mut lines: Vec<String> = Vec::new();
    for row in &run.rows {
        lines.push(format!("Config: {}.bat (Type: {kind})", row.name));
        match row.state {
            RowState::Done => {
                for d in &row.details {
                    match run.kind {
                        Kind::Standard | Kind::Custom => lines.push(format!("  {} : {}", d.target, d.text)),
                        Kind::Dpi => lines.push(format!("  Target: {} : {}", d.target, d.text)),
                    }
                }
            }
            RowState::Failed => lines.push(format!("  ERROR: {}", row.error.clone().unwrap_or_default())),
            _ => lines.push("  SKIPPED".into()),
        }
        lines.push(String::new());
    }
    lines.push("=== ANALYTICS ===".into());
    let width = run.rows.iter().map(|r| r.name.len() + 4).max().unwrap_or(10);
    for row in run.rows.iter().filter(|r| r.state == RowState::Done) {
        let name = format!("{}.bat", row.name);
        lines.push(format!(
            "{name:<width$} : OK: {:>3}, Ping OK: {:>3}, Groups: {}",
            row.score,
            row.ping_ok,
            row.total.clone().unwrap_or_default()
        ));
    }
    lines.push(format!(
        "Best strategy: {}",
        run.best.as_deref().map(|b| format!("{b}.bat")).unwrap_or_else(|| "none".into())
    ));
    let mut text = lines.join("\r\n");
    text.push_str("\r\n");
    fs::write(&path, text)?;
    Ok(path)
}
