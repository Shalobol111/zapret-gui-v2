pub mod batparse;
pub mod diagnostics;
pub mod discord;
pub mod extras;
pub mod filters;
pub mod hosts;
pub mod layout;
pub mod runner;
pub mod service;
pub mod site;
pub mod strategy;
pub mod tests;
pub mod updater;
pub mod userlists;

use std::path::Path;

use batparse::Vars;
use strategy::Strategy;

/// Переменные для подстановки в стратегию с учётом текущего Game Filter.
pub fn vars(root: &Path) -> Vars {
    let (game_tcp, game_udp) = filters::game(root).vars();
    Vars { root: root.to_path_buf(), game_tcp, game_udp }
}

/// Стратегия по имени и её аргументы winws.
pub fn strategy_args(root: &Path, name: &str) -> Result<(Strategy, Vec<String>), String> {
    let s = strategy::find(root, name).ok_or_else(|| format!("стратегия «{name}» не найдена"))?;
    let args = batparse::parse_file(&s.file, &vars(root)).map_err(|e| format!("«{name}»: {e}"))?;
    Ok((s, args))
}

/// Имя стратегии, чьи аргументы совпадают с данными (для winws, запущенного не нами).
pub fn match_strategy(root: &Path, args: &[String]) -> Option<String> {
    let v = vars(root);
    strategy::list(root)
        .into_iter()
        .find(|s| batparse::parse_file(&s.file, &v).map(|a| a == args).unwrap_or(false))
        .map(|s| s.name)
}
