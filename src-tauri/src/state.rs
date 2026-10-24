use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Sender;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::settings::SettingsStore;
use crate::zapret::layout::{self, ZapretInfo};
use crate::zapret::runner::Runner;
use crate::zapret::tests::model::Run;
use crate::zapret::updater::Update;
use crate::zapret::{batparse, vars};

pub struct ZapretCache {
    pub path: Option<String>,
    pub info: ZapretInfo,
    pub at: Option<Instant>,
}

pub struct AppState {
    pub settings: Mutex<SettingsStore>,
    pub is_admin: bool,
    pub runner: Mutex<Runner>,
    pub zapret: Mutex<ZapretCache>,
    pub poke: Mutex<Option<Sender<()>>>,
    /// Скачанный hosts между «проверить» и «применить».
    pub hosts_remote: Mutex<Option<String>>,
    /// Текущая или последняя проверка стратегий.
    pub tests: Mutex<Option<Run>>,
    pub tests_cancel: AtomicBool,
    pub update: Mutex<Update>,
    /// Запущено с --minimized (автозапуск) — окно не показываем.
    pub start_hidden: bool,
    /// Когда программа сама последний раз меняла обход или службу — чтобы не уведомлять
    /// о собственных действиях как о «неожиданной остановке».
    pub last_action: Mutex<Option<Instant>>,
    /// Состояние автопилота для подписи на Главной.
    pub autopilot: Mutex<crate::autopilot::Info>,
    /// Telegram-прокси: последняя версия на GitHub и текущее действие.
    pub tg: Mutex<crate::tgproxy::Shared>,
    /// Обновление самой ZapretGUI.
    pub self_update: Mutex<crate::selfupdate::Info>,
}

const ZAPRET_TTL: Duration = Duration::from_secs(5);

pub fn inspect(path: &std::path::Path) -> ZapretInfo {
    let v = vars(path);
    layout::inspect(path, |s| batparse::parse_file(&s.file, &v).is_ok())
}

impl AppState {
    pub fn new(mut settings: SettingsStore, is_admin: bool) -> Self {
        // Программа уже настроена (обновление с прошлой версии) — мастер не нужен.
        if !settings.get().onboarding_done && settings.get().zapret_path.is_some() {
            let _ = settings.patch(&serde_json::json!({ "onboardingDone": true }));
        }
        Self {
            settings: Mutex::new(settings),
            is_admin,
            runner: Mutex::new(Runner::default()),
            zapret: Mutex::new(ZapretCache { path: None, info: ZapretInfo::empty(), at: None }),
            poke: Mutex::new(None),
            hosts_remote: Mutex::new(None),
            tests: Mutex::new(crate::zapret::tests::load_last()),
            tests_cancel: AtomicBool::new(false),
            update: Mutex::new(Update::default()),
            start_hidden: std::env::args().any(|a| a == "--minimized"),
            last_action: Mutex::new(None),
            autopilot: Mutex::new(Default::default()),
            tg: Mutex::new(Default::default()),
            self_update: Mutex::new(Default::default()),
        }
    }

    /// Сведения о папке zapret; перепроверяются раз в 5 с или после смены пути.
    pub fn zapret_info(&self) -> ZapretInfo {
        let path = self.settings.lock().unwrap().get().zapret_path.clone();
        let mut cache = self.zapret.lock().unwrap();
        let fresh = cache.path == path && cache.at.is_some_and(|t| t.elapsed() < ZAPRET_TTL);
        if !fresh {
            cache.info = match &path {
                Some(p) => inspect(std::path::Path::new(p)),
                None => ZapretInfo::empty(),
            };
            cache.path = path;
            cache.at = Some(Instant::now());
        }
        cache.info.clone()
    }

    pub fn invalidate_zapret(&self) {
        self.zapret.lock().unwrap().at = None;
    }

    /// Корень zapret, если папка задана и проверена.
    pub fn root(&self) -> Option<PathBuf> {
        let info = self.zapret_info();
        info.valid.then(|| PathBuf::from(info.path.unwrap()))
    }

    pub fn require_root(&self) -> Result<PathBuf, String> {
        self.root().ok_or_else(|| "Укажите папку zapret в Настройках".to_string())
    }

    pub fn require_admin(&self) -> Result<(), String> {
        if self.is_admin {
            Ok(())
        } else {
            Err("Нужны права администратора — перезапустите программу от имени администратора".into())
        }
    }

    pub fn touch(&self) {
        *self.last_action.lock().unwrap() = Some(Instant::now());
    }

    /// Программа сама что-то делала с обходом в последние 15 с.
    pub fn acted_recently(&self) -> bool {
        self.last_action.lock().unwrap().is_some_and(|t| t.elapsed() < Duration::from_secs(15))
    }

    /// Во время проверки стратегий управление обходом и службой заблокировано.
    pub fn require_idle(&self) -> Result<(), String> {
        if self.tests.lock().unwrap().as_ref().is_some_and(|r| r.running) {
            Err("Идёт проверка стратегий — дождитесь окончания или остановите её".into())
        } else {
            Ok(())
        }
    }

    /// Работает ли копия TgWsProxy из ZapretGUI (для перезапуска после смены настроек).
    pub fn status_tg_running(&self) -> bool {
        crate::tgproxy::status(&self.tg.lock().unwrap().clone()).running
    }

    /// Разбудить монитор: статус пересчитается и уйдёт в интерфейс сразу.
    pub fn poke(&self) {
        if let Some(tx) = self.poke.lock().unwrap().as_ref() {
            let _ = tx.send(());
        }
    }
}
