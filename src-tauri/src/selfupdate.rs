//! Обновление самой ZapretGUI с GitHub (Shalobol111/zapret-gui-v2).
//!
//! Последний релиз берётся из GitHub API; версия — тег без «v» (2.1.0), сравнивается с версией
//! программы. Установленная копия (рядом лежит uninstall.exe) обновляется установщиком
//! (`*setup*.exe`, режим `/P /UPDATE` — без вопросов), portable — заменой exe. Файл сверяется с
//! SHA-256 из релиза (поле digest или SHA256SUMS.txt).
//!
//! Замену делает маленький cmd-скрипт: он ждёт, пока ZapretGUI закроется, ставит новую версию
//! и запускает её. Скрипт наследует права администратора — ни установщик, ни перезапуск не
//! спрашивают UAC повторно.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::journal;
use crate::state::AppState;
use crate::zapret::updater;

pub const REPO: &str = "Shalobol111/zapret-gui-v2";
pub const RELEASES_URL: &str = "https://github.com/Shalobol111/zapret-gui-v2/releases";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum State {
    #[default]
    Idle,
    Checking,
    Current,
    Available,
    Downloading,
    Installing,
    Error,
}

/// Для интерфейса (часть Status).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub state: State,
    pub current: String,
    pub latest: Option<String>,
    /// Текст релиза (Markdown) — «Что нового».
    pub notes: Option<String>,
    /// Страница релиза.
    pub url: Option<String>,
    pub progress: Option<u8>,
    pub error: Option<String>,
    /// Portable-копия (обновляется заменой exe, а не установщиком).
    pub portable: bool,
}

#[derive(Debug, Clone)]
struct Asset {
    name: String,
    url: String,
    sha256: Option<String>,
}

#[derive(Debug, Clone)]
struct Release {
    version: String,
    notes: String,
    page: String,
    assets: Vec<Asset>,
}

pub fn current_version(app: &AppHandle) -> String {
    app.package_info().version.to_string()
}

/// Установлена ли программа установщиком: Tauri кладёт uninstall.exe рядом с exe.
pub fn is_installed() -> bool {
    std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("uninstall.exe").is_file())).unwrap_or(false)
}

fn parse_release(v: &Value) -> Option<Release> {
    let tag = v.get("tag_name")?.as_str()?;
    let assets = v
        .get("assets")?
        .as_array()?
        .iter()
        .filter_map(|a| {
            Some(Asset {
                name: a.get("name")?.as_str()?.to_string(),
                url: a.get("browser_download_url")?.as_str()?.to_string(),
                sha256: a
                    .get("digest")
                    .and_then(Value::as_str)
                    .and_then(|d| d.strip_prefix("sha256:"))
                    .map(str::to_lowercase),
            })
        })
        .collect();
    Some(Release {
        version: tag.trim_start_matches(['v', 'V']).to_string(),
        notes: v.get("body").and_then(Value::as_str).unwrap_or_default().replace("\r\n", "\n"),
        page: v.get("html_url").and_then(Value::as_str).unwrap_or(RELEASES_URL).to_string(),
        assets,
    })
}

/// Файл под этот способ установки: установщик для установленной копии, exe — для portable.
fn pick_asset(assets: &[Asset], installed: bool) -> Option<&Asset> {
    let exe = |a: &&Asset| a.name.to_lowercase().ends_with(".exe");
    let setup = |a: &&Asset| a.name.to_lowercase().contains("setup");
    if installed {
        assets.iter().filter(exe).find(setup)
    } else {
        assets.iter().filter(exe).filter(|a| !setup(a)).find(|a| a.name.to_lowercase().starts_with("zapretgui"))
    }
}

/// SHA-256 из SHA256SUMS.txt (`<hash>  <имя>`), если у файла нет digest.
fn sha_from_sums(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let hash = it.next()?;
        let file = it.next()?.trim_start_matches('*');
        (file.eq_ignore_ascii_case(name) && hash.len() == 64).then(|| hash.to_lowercase())
    })
}

fn set(app: &AppHandle, f: impl FnOnce(&mut Info)) {
    let st = app.state::<AppState>();
    f(&mut st.self_update.lock().unwrap());
    st.poke();
}

async fn latest_release() -> Result<Release, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let text = crate::net::get_text(&url, Duration::from_secs(15)).await?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("неверный ответ GitHub: {e}"))?;
    parse_release(&v).ok_or_else(|| "в репозитории нет релизов".into())
}

/// Проверка новой версии. `manual` — нажали «Проверить»: результат пишется в журнал всегда.
pub async fn check(app: &AppHandle, manual: bool) -> Info {
    let current = current_version(app);
    let portable = !is_installed();
    set(app, |i| {
        i.state = State::Checking;
        i.current = current.clone();
        i.portable = portable;
        i.error = None;
    });
    let info = match latest_release().await {
        Ok(r) if updater::compare(&r.version, &current) == Some(std::cmp::Ordering::Greater) => Info {
            state: State::Available,
            current,
            latest: Some(r.version),
            notes: Some(r.notes).filter(|n| !n.trim().is_empty()),
            url: Some(r.page),
            portable,
            ..Info::default()
        },
        Ok(_) => Info { state: State::Current, current, portable, ..Info::default() },
        Err(e) => Info { state: State::Error, current, portable, error: Some(e), ..Info::default() },
    };
    match (&info.state, manual) {
        (State::Available, _) => {
            journal::info(format!("Доступна новая версия ZapretGUI: {}", info.latest.as_deref().unwrap_or("?")))
        }
        (State::Current, true) => journal::ok("Установлена последняя версия ZapretGUI"),
        (State::Error, true) => {
            journal::warn(format!("Не удалось проверить обновления ZapretGUI: {}", info.error.as_deref().unwrap_or("")))
        }
        _ => {}
    }
    set(app, |i| *i = info.clone());
    info
}

/// Скачать новую версию, сверить SHA-256 и запустить замену; программа закроется.
pub async fn install(app: AppHandle) -> Result<(), String> {
    let r = async {
        let installed = is_installed();
        let release = latest_release().await?;
        let asset = pick_asset(&release.assets, installed).cloned().ok_or_else(|| {
            if installed {
                "в релизе нет установщика (*setup*.exe) — скачайте новую версию со страницы релиза".to_string()
            } else {
                "в релизе нет portable-версии (ZapretGUI_….exe) — скачайте её со страницы релиза".to_string()
            }
        })?;
        let sha = match asset.sha256.clone() {
            Some(s) => Some(s),
            None => match release.assets.iter().find(|a| a.name.eq_ignore_ascii_case("SHA256SUMS.txt")) {
                Some(sums) => {
                    sha_from_sums(&crate::net::get_text(&sums.url, Duration::from_secs(15)).await?, &asset.name)
                }
                None => None,
            },
        };

        let dir = std::env::temp_dir().join("zapretgui-update");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let file = dir.join(&asset.name);
        set(&app, |i| {
            i.state = State::Downloading;
            i.progress = Some(0);
            i.error = None;
        });
        let a = app.clone();
        let mut last = 255u8;
        let hash = crate::net::download(&asset.url, &file, move |got, total| {
            if let Some(t) = total.filter(|t| *t > 0) {
                let pct = (got * 100 / t).min(100) as u8;
                if pct != last {
                    last = pct;
                    set(&a, |i| i.progress = Some(pct));
                }
            }
        })
        .await?;
        match &sha {
            Some(want) if &hash != want => {
                return Err("контрольная сумма не совпала — файл повреждён, попробуйте ещё раз".into())
            }
            Some(_) => {}
            None => journal::warn("У файла обновления нет контрольной суммы в релизе — проверка пропущена"),
        }
        set(&app, |i| {
            i.state = State::Installing;
            i.progress = None;
        });
        journal::info(format!("Установка ZapretGUI {} — программа перезапустится", release.version));
        remember_bypass(&app);
        let script = write_script(&dir, &file, installed)?;
        run_script(&script)?;
        Ok::<(), String>(())
    }
    .await;
    match r {
        Ok(()) => {
            // Даём интерфейсу показать «Установка…», затем выходим — скрипт ждёт нашего PID.
            std::thread::sleep(Duration::from_millis(600));
            app.exit(0);
            Ok(())
        }
        Err(e) => {
            journal::err(format!("Обновление ZapretGUI: {e}"));
            set(&app, |i| {
                i.state = State::Error;
                i.progress = None;
                i.error = Some(e.clone());
            });
            Err(e)
        }
    }
}

/// Ручной обход переживает обновление: после перезапуска стратегия включится снова.
fn remember_bypass(app: &AppHandle) {
    let st = app.state::<AppState>();
    let name = {
        let runner = st.runner.lock().unwrap();
        runner.is_active().then(|| runner.strategy.clone()).flatten()
    };
    if let Some(name) = name {
        let _ = st.settings.lock().unwrap().patch(&json!({ "resumeBypass": name }));
    }
}

/// cmd-скрипт: дождаться выхода ZapretGUI, поставить новую версию, запустить её, удалить себя.
///
/// Выход ждём по тому, что exe освободился: запущенный exe нельзя открыть на запись. Без
/// `tasklist | find` — в 2.1.0–2.1.1 этот конвейер зависал и оставлял пустое окно `find`.
fn script_text(exe: &Path, new_file: &Path, installed: bool) -> String {
    let wait = format!(
        "set n=0\r\n:wait\r\n2>nul (>>\"{exe}\" (call )) || (set /a n+=1 & ping -n 2 127.0.0.1 >nul & if %n% lss 60 goto wait)\r\n",
        exe = exe.display()
    );
    let apply = if installed {
        // Установщик Tauri: /P — без вопросов, /UPDATE — не трогать ярлыки и данные.
        format!("\"{}\" /P /UPDATE\r\n", new_file.display())
    } else {
        // Антивирус или проводник могут держать файл секунду-другую — несколько попыток.
        format!(
            "set n=0\r\n:move\r\nmove /y \"{new}\" \"{exe}\" >nul 2>&1 || (set /a n+=1 & ping -n 2 127.0.0.1 >nul & if %n% lss 30 goto move)\r\n",
            new = new_file.display(),
            exe = exe.display()
        )
    };
    format!("@echo off\r\n{wait}{apply}start \"\" \"{}\"\r\n(goto) 2>nul & del \"%~f0\"\r\n", exe.display())
}

fn write_script(dir: &Path, new_file: &Path, installed: bool) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let path = dir.join("update.cmd");
    // cmd читает скрипт в кодировке OEM — пути с кириллицей пишем через chcp 65001.
    let text = format!("@chcp 65001 >nul\r\n{}", script_text(&exe, new_file, installed));
    fs::write(&path, text).map_err(|e| format!("не удалось записать скрипт обновления: {e}"))?;
    Ok(path)
}

/// Скрипт — со скрытой консолью (CREATE_NO_WINDOW): команды внутри неё не открывают окон.
/// DETACHED_PROCESS здесь нельзя: без консоли каждая команда скрипта открывала бы своё окно.
fn run_script(script: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("cmd.exe")
        .arg("/c")
        .arg(script)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("не удалось запустить обновление: {e}"))
}

/// Удалить остатки прошлого обновления.
pub fn cleanup() {
    let _ = fs::remove_dir_all(std::env::temp_dir().join("zapretgui-update"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> Asset {
        Asset { name: name.into(), url: format!("https://x/{name}"), sha256: None }
    }

    #[test]
    fn picks_setup_for_installed_and_exe_for_portable() {
        let ci =
            [asset("ZapretGUI_2.1.0_portable.exe"), asset("ZapretGUI_2.1.0_x64-setup.exe"), asset("SHA256SUMS.txt")];
        assert_eq!(pick_asset(&ci, true).unwrap().name, "ZapretGUI_2.1.0_x64-setup.exe");
        assert_eq!(pick_asset(&ci, false).unwrap().name, "ZapretGUI_2.1.0_portable.exe");
        // Как в релизе 1.0.1: только переименованный portable.
        let manual = [asset("ZapretGUI_2.0.1.exe")];
        assert_eq!(pick_asset(&manual, false).unwrap().name, "ZapretGUI_2.0.1.exe");
        assert!(pick_asset(&manual, true).is_none());
        // С 2.1.4 portable в релизе называется просто ZapretGUI.exe — 2.1.2/2.1.3 его тоже находят.
        let plain = [asset("ZapretGUI.exe"), asset("ZapretGUI_2.1.4_x64-setup.exe"), asset("SHA256SUMS.txt")];
        assert_eq!(pick_asset(&plain, false).unwrap().name, "ZapretGUI.exe");
        assert_eq!(pick_asset(&plain, true).unwrap().name, "ZapretGUI_2.1.4_x64-setup.exe");
    }

    #[test]
    fn release_and_sums_are_parsed() {
        let v = json!({
            "tag_name": "v2.1.0", "body": "## Что нового\r\n- Telegram", "html_url": "https://github.com/x/releases/tag/v2.1.0",
            "assets": [{ "name": "ZapretGUI_2.1.0_x64-setup.exe", "browser_download_url": "u", "digest": "sha256:ABC" }]
        });
        let r = parse_release(&v).unwrap();
        assert_eq!(r.version, "2.1.0");
        assert_eq!(r.notes, "## Что нового\n- Telegram");
        assert_eq!(r.assets[0].sha256.as_deref(), Some("abc"));
        let h = "a".repeat(64);
        assert_eq!(
            sha_from_sums(&format!("{h}  ZapretGUI_2.1.0_portable.exe\n"), "ZapretGUI_2.1.0_portable.exe"),
            Some(h)
        );
        assert!(updater::compare("2.1.0", "2.0.0") == Some(std::cmp::Ordering::Greater));
        assert!(updater::compare("1.0.1", "2.0.0") == Some(std::cmp::Ordering::Less));
    }

    #[test]
    fn script_waits_then_applies_and_restarts() {
        let exe = Path::new(r"C:\Program Files\ZapretGUI\zapret-gui.exe");
        let s = script_text(exe, Path::new(r"C:\T\setup.exe"), true);
        assert!(s.contains(r#"(>>"C:\Program Files\ZapretGUI\zapret-gui.exe" (call ))"#), "ждём, пока exe освободится");
        assert!(!s.contains("tasklist") && !s.contains("find "), "без конвейера tasklist | find");
        assert!(s.contains(r#""C:\T\setup.exe" /P /UPDATE"#));
        assert!(s.contains(r#"start "" "C:\Program Files\ZapretGUI\zapret-gui.exe""#));
        let p = script_text(Path::new(r"D:\Apps\ZapretGUI.exe"), Path::new(r"C:\T\ZapretGUI_2.1.0.exe"), false);
        assert!(p.contains(r#"move /y "C:\T\ZapretGUI_2.1.0.exe" "D:\Apps\ZapretGUI.exe""#));
    }

    /// Сетевой: `cargo test -- --ignored live_app_release --nocapture`
    #[test]
    #[ignore]
    fn live_app_release() {
        let r = tauri::async_runtime::block_on(latest_release()).expect("релиз");
        println!(
            "{} · {} · {:?}",
            r.version,
            r.page,
            r.assets.iter().map(|a| (&a.name, &a.sha256)).collect::<Vec<_>>()
        );
        println!(
            "installed: {:?} portable: {:?}",
            pick_asset(&r.assets, true).map(|a| &a.name),
            pick_asset(&r.assets, false).map(|a| &a.name)
        );
    }

    /// Настоящая замена portable-exe скриптом (кириллица и пробелы в пути): вместо ZapretGUI —
    /// копии ping.exe; скрипт ждёт, пока «старая» копия освободит файл.
    /// `ZAPRETGUI_TEST_DIR=<папка> cargo test -- --ignored live_portable_swap --nocapture`
    #[test]
    #[ignore]
    fn live_portable_swap() {
        use std::os::windows::process::CommandExt;
        let base = PathBuf::from(std::env::var("ZAPRETGUI_TEST_DIR").expect("ZAPRETGUI_TEST_DIR"));
        let dir = base.join("Тест обновления");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // «Старая программа» — копия ping.exe, запущенная на ~3 с: пока она работает, её exe занят.
        let src = PathBuf::from(std::env::var("SystemRoot").unwrap()).join("System32").join("PING.EXE");
        let old = dir.join("ZapretGUI.exe");
        let new = dir.join("ZapretGUI_new.exe");
        fs::copy(&src, &old).unwrap();
        let mut bytes = fs::read(&src).unwrap();
        bytes.extend_from_slice(b"ZGUI-NEW");
        fs::write(&new, &bytes).unwrap();
        let mut child = std::process::Command::new(&old)
            .args(["-n", "4", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .creation_flags(0x0800_0000)
            .spawn()
            .unwrap();
        let script = dir.join("update.cmd");
        fs::write(&script, ["@chcp 65001 >nul", &script_text(&old, &new, false)].join("\r\n")).unwrap();
        let t0 = std::time::Instant::now();
        run_script(&script).unwrap();
        while new.exists() || script.exists() {
            assert!(t0.elapsed() < Duration::from_secs(30), "замена не прошла за 30 с");
            std::thread::sleep(Duration::from_millis(200));
        }
        let _ = child.wait();
        let got = fs::read(&old).unwrap();
        assert!(got.ends_with(b"ZGUI-NEW"), "exe не заменён");
        println!("заменено за {:.1} с (exe был занят ~3 с), скрипт удалил себя", t0.elapsed().as_secs_f32());
    }
}
