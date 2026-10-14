//! Запуск системных утилит без консольного окна.

use std::os::windows::process::CommandExt;
use std::process::{Command, Output, Stdio};

pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn hidden(program: &str) -> Command {
    let mut c = Command::new(program);
    c.creation_flags(CREATE_NO_WINDOW).stdin(Stdio::null());
    c
}

pub fn run(program: &str, args: &[&str]) -> std::io::Result<Output> {
    hidden(program).args(args).output()
}

/// Вывод консольных утилит Windows — в OEM-кодировке. Для наших проверок
/// хватает ASCII (вызываем с chcp 437), остальное заменяется.
pub fn text(out: &Output) -> String {
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    s
}
