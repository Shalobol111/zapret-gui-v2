//! Журнал программы: файл `%APPDATA%\ZapretGUI\logs\gui.log.YYYY-MM-DD`
//! (ротация по дням) и stderr в отладочной сборке.
//! Кольцевой буфер и передача в карточку «Журнал» появятся на шаге 2.

use std::sync::OnceLock;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

static GUARD: OnceLock<WorkerGuard> = OnceLock::new();

pub fn init() {
    let dir = crate::settings::data_dir().join("logs");
    // max_log_files читает папку при создании — без неё appender пишет ошибку в stderr.
    let _ = std::fs::create_dir_all(&dir);
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("gui.log")
        .max_log_files(7)
        .build(&dir);

    let file_layer = appender.ok().map(|appender| {
        let (writer, guard) = tracing_appender::non_blocking(appender);
        let _ = GUARD.set(guard);
        fmt::layer().with_ansi(false).with_target(false).with_writer(writer)
    });
    let stderr_layer = cfg!(debug_assertions).then(|| fmt::layer().with_target(false));

    let _ = tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(file_layer)
        .with(stderr_layer)
        .try_init();
}
