//! Сценарии обновления zapret поверх zapret/updater.rs: проверка, применение
//! (остановить → заменить файлы → переустановить службу с новыми параметрами)
//! и первая установка. Прогресс уходит в интерфейс через статус (`update`).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;
use tauri::{AppHandle, Manager};

use crate::journal;
use crate::state::AppState;
use crate::sys::{process, scm};
use crate::zapret::updater::{self, State, Update};
use crate::zapret::{runner, service};

fn set(app: &AppHandle, f: impl FnOnce(&mut Update)) {
    let st = app.state::<AppState>();
    f(&mut st.update.lock().unwrap());
    st.poke();
}

fn stage(app: &AppHandle, stage: &str, progress: Option<u8>) {
    set(app, |u| {
        u.state = State::Applying;
        u.stage = Some(stage.into());
        u.progress = progress;
    });
}

/// Проверка версии. `manual` — нажали «Проверить» (пишем в журнал и «нет обновлений»).
pub async fn check(app: &AppHandle, manual: bool) -> Update {
    let local = app.state::<AppState>().zapret_info().version;
    set(app, |u| *u = Update { state: State::Checking, ..Update::default() });
    let result = match updater::latest_version().await {
        Err(e) => Update { state: State::Error, error: Some(e), ..Update::default() },
        Ok(latest) => match local.as_deref().and_then(|l| updater::compare(l, &latest)) {
            Some(std::cmp::Ordering::Less) => {
                Update { state: State::Available, latest: Some(latest), ..Update::default() }
            }
            Some(_) => Update { state: State::Current, latest: Some(latest), ..Update::default() },
            // Нечитаемая версия — не предлагаем обновление.
            None => Update {
                state: State::Error,
                latest: Some(latest),
                error: Some("не удалось определить установленную версию".into()),
                ..Update::default()
            },
        },
    };
    updater::log_result(&result, local.as_deref(), manual);
    if !manual && result.state == State::Available {
        crate::notify::show(
            app,
            &format!("Доступен zapret {}", result.latest.as_deref().unwrap_or("")),
            &format!(
                "Установлена версия {}. Обновить можно в Настройках — свои списки сохранятся.",
                local.as_deref().unwrap_or("?")
            ),
            crate::notify::Screen::Settings,
        );
    }
    set(app, |u| *u = result.clone());
    result
}

fn fail(app: &AppHandle, msg: String) -> String {
    journal::err(format!("Обновление zapret: {msg}"));
    set(app, |u| {
        u.state = State::Error;
        u.error = Some(msg.clone());
        u.stage = None;
        u.progress = None;
    });
    msg
}

fn progress_cb<'a>(app: &'a AppHandle, label: &'static str) -> impl FnMut(u64, Option<u64>) + 'a {
    let mut last = 255u8;
    move |got, total| {
        let p = total.map(|t| ((got * 100) / t.max(1)).min(100) as u8);
        if p != Some(last) {
            last = p.unwrap_or(0);
            stage(app, label, p);
        }
    }
}

pub async fn apply(app: AppHandle) -> Result<String, String> {
    let st = app.state::<AppState>();
    st.require_admin()?;
    st.require_idle()?;
    let root = st.require_root()?;

    stage(&app, "Получение ссылки на релиз", None);
    let asset = updater::release_asset().await.map_err(|e| fail(&app, e))?;
    let (src, tmp) =
        updater::fetch_release(&asset, progress_cb(&app, "Скачивание")).await.map_err(|e| fail(&app, e))?;

    let a = app.clone();
    let r = tauri::async_runtime::spawn_blocking(move || replace_files(&a, &root, &src))
        .await
        .map_err(|e| e.to_string())?;
    let _ = fs::remove_dir_all(tmp);
    match r {
        Ok(msg) => {
            journal::ok(&msg);
            st.invalidate_zapret();
            set(&app, |u| {
                *u = Update { state: State::Current, latest: Some(asset.version.clone()), ..Update::default() }
            });
            Ok(msg)
        }
        Err(e) => Err(fail(&app, e)),
    }
}

fn replace_files(app: &AppHandle, root: &Path, src: &Path) -> Result<String, String> {
    let st = app.state::<AppState>();
    stage(app, "Остановка обхода", None);
    let svc = service::status(Some(root));
    let svc_strategy = (svc.state != service::State::NotInstalled).then(|| svc.strategy.clone()).flatten();
    let gui_strategy = {
        let r = st.runner.lock().unwrap();
        r.is_active().then(|| r.strategy.clone()).flatten()
    };
    if gui_strategy.is_some() {
        runner::stop(&st.runner)?;
    }
    for name in ["zapret", "WinDivert", "WinDivert14"] {
        let _ = scm::stop(name, Duration::from_secs(10));
    }
    process::kill_all("winws.exe");
    std::thread::sleep(Duration::from_secs(1)); // даём системе закрыть файлы

    stage(app, "Замена файлов", None);
    let copied = updater::copy_tree(src, root, true)?;
    tracing::info!("Обновление: скопировано файлов {copied}");
    st.invalidate_zapret();
    let version = crate::zapret::layout::read_version(&root.join("service.bat")).unwrap_or_else(|| "?".into());

    stage(app, "Восстановление", None);
    let mut msg = format!("zapret обновлён до версии {version}");
    if let Some(name) = svc_strategy {
        // Переустановка, а не перезапуск: в binPath нужны аргументы новой версии.
        match service::install(root, &name) {
            Ok(()) => msg.push_str(&format!(" · служба переустановлена со стратегией «{name}»")),
            Err(e) => {
                journal::warn(format!("Службу не удалось переустановить со стратегией «{name}»: {e}"));
                msg.push_str(" · служба не восстановлена — установите её заново");
            }
        }
    } else if let Some(name) = gui_strategy {
        if runner::start(&st.runner, root, &name).is_ok() {
            msg.push_str(" · обход снова запущен");
        }
    }
    Ok(msg)
}

/// Первая установка: скачать последний релиз в пустую (или новую) папку.
pub async fn install(app: AppHandle, dest: String) -> Result<String, String> {
    let dest = PathBuf::from(dest.trim());
    if dest.as_os_str().is_empty() {
        return Err("укажите папку".into());
    }
    if dest.join("service.bat").is_file() {
        return Err("в этой папке уже есть zapret — выберите её через «Обзор…»".into());
    }
    if dest.read_dir().is_ok_and(|mut d| d.next().is_some()) {
        return Err(format!("папка {} не пуста — выберите пустую или новую", dest.display()));
    }
    fs::create_dir_all(&dest).map_err(|e| format!("не удалось создать папку: {e}"))?;

    stage(&app, "Получение ссылки на релиз", None);
    let asset = updater::release_asset().await.map_err(|e| fail(&app, e))?;
    let (src, tmp) =
        updater::fetch_release(&asset, progress_cb(&app, "Скачивание zapret")).await.map_err(|e| fail(&app, e))?;
    stage(&app, "Распаковка", None);
    let r = updater::copy_tree(&src, &dest, false);
    let _ = fs::remove_dir_all(tmp);
    r.map_err(|e| fail(&app, e))?;

    let st = app.state::<AppState>();
    let info = crate::state::inspect(&dest);
    if !info.valid {
        // Чаще всего антивирус удалил WinDivert или winws сразу после распаковки.
        return Err(fail(
            &app,
            format!("{} — добавьте папку в исключения антивируса и скачайте заново", info.error.unwrap_or_default()),
        ));
    }
    st.settings.lock().unwrap().patch(&json!({ "zapretPath": dest.display().to_string() }))?;
    st.invalidate_zapret();
    set(&app, |u| *u = Update { state: State::Current, latest: Some(asset.version.clone()), ..Update::default() });
    let msg = format!("zapret {} установлен в {}", asset.version, dest.display());
    journal::ok(&msg);
    if let Some(w) = info.warning {
        journal::warn(w);
    }
    Ok(msg)
}
