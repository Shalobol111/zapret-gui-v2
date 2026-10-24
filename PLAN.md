# PLAN.md — ZapretGUI 2.0 (Rust + Tauri 2)

План реализации программы для управления сборкой
[Flowseal/zapret-discord-youtube](https://github.com/Flowseal/zapret-discord-youtube).
Внешний вид задают [DESIGN.md](DESIGN.md) и макет
https://claude.ai/artifact/9LB5De7j8kPWiQT6h2NeMm (`project/Main|Check|Lists|Settings.dc.html`).
Если DESIGN.md и макет расходятся, верен макет. Этот файл описывает, **как** построить
программу, и переносит Tk-специфичные пункты DESIGN.md на Tauri.

Дата анализа: 2026-10-13. Актуальная версия Flowseal: **1.10.3** (релиз 2026-09-20, коммит `eb16c14`).

---

## Статус реализации (2026-10-13)

Все шаги §10 выполнены: каркас, Главная, Проверка, Списки, Настройки, финальная проверка.
Сборка: `zapret-gui.exe` (≈6 МБ) и установщик NSIS `ZapretGUI_2.0.0_x64-setup.exe` (≈4 МБ).
41 Rust-тест (+4 `--ignored`: три сетевых и живой тест парсера на свежем релизе Flowseal), svelte-check, clippy и rustfmt без замечаний; CI в `.github/workflows`.

Не проверено вживую (нужны права администратора, проверяет пользователь): запуск winws, установка службы,
прогон тестов стратегий, «Исправить автоматически», запись hosts, автозапуск, применение обновления.

Отличия от плана и макета (осознанные):
- Таблица «Проверки»: короткие имена (`ALT2` вместо `general (ALT2)`), колонки 56 px вместо 64 — иначе имя
  не помещается; «Итог» считается по всем разделам `targets.txt`; в узком окне бейдж «лучшая» — точкой.
- Карточка «Служба Windows»: кнопки переносятся на две строки (в макете они не помещаются в 300 px).
- Диагностика: 9 строк вместо 6 (добавлены «Папка zapret», «Файл hosts», «Кэш Discord»).
- Настройки: добавлена карточка «Дополнительно»; поле папки zapret сделано на шаге 2, а не 5;
  если выбрана папка без zapret — под полем ошибка и «Скачать zapret сюда / в подпапку».
- Системное уведомление «работает в трее» не сделано (нужен отдельный плагин).
- Тема Windows отслеживается через `RegNotifyChangeKeyValue`, а не опросом.

Версия 2.1 (§14): все шаги 7–11 выполнены — движок быстрого подбора и уведомления, «Мой сайт»,
«Починить Discord», Автопилот, мастер первого запуска; 56 Rust-тестов (+9 `--ignored`).
- Мастер: показывается, пока `onboardingDone = false`; у тех, кто обновился с уже настроенной папкой
  zapret, флаг ставится сам. Для варианта «Запускать программу» добавлена настройка «Включать обход при
  запуске программы» (Настройки → Поведение) — автозапуск без неё только открывал бы программу в трее.
  Без прав администратора после шага 1 мастер объясняет, что нужны права, и закрывается.
- Все проверки через curl идут с `--noproxy *`: прокси из переменных окружения не должен влиять на оценку
  стратегий; признак интернета у автопилота — любой из четырёх адресов (msftconnecttest бывает недоступен).
- Автопилот: «Проверить сейчас» в Настройках; при сбое в режиме «Предупреждать» уведомление показывается
  один раз на стратегию (а не каждые 15 минут), в Главной — подпись с ссылкой «Подобрать». Имитация сбоя —
  `ZAPRETGUI_AUTOPILOT_SIMULATE=1` (README → Разработка).
- «Починить Discord»: пять строк вместо шести из §14.5 — закрытие и очистка кэша объединены. Обход
  включается на шаге проверки голосовых фильтров (последней стратегией), подбор — по целям быстрого
  подбора (Discord + YouTube), чтобы смена стратегии не сломала YouTube. Найденная стратегия
  применяется сразу. Discord запускается через ярлык из меню «Пуск» (через проводник — без прав администратора).

---

## 0. Коротко

| Вопрос | Решение |
|---|---|
| Кодовая база | Новый проект. Из [Shalobol111/zapret-gui](https://github.com/Shalobol111/zapret-gui) переносим идеи и проверенные алгоритмы (обновление zapret, сравнение версий, автозапуск, фильтры), но переписываем их на Rust |
| Стек | **Tauri 2**: ядро на Rust, интерфейс Svelte 5 + TypeScript + Vite в WebView2 |
| Управление zapret | **Напрямую из Rust, без `service.bat`**. Аргументы `winws.exe` берём из `general*.bat`, процесс запускаем сами, службу создаём через Service Control Manager. Файлы состояния ведём в тех же форматах, что `service.bat`, так что обе программы видят изменения друг друга |
| Права | Всегда от администратора (манифест `requireAdministrator`). Автозапуск через Планировщик задач без UAC |
| Функции сверх макета (согласованы) | «Исправить автоматически», «Установить с этой стратегией», скачивание zapret при первом запуске, расширенные настройки (порты Game Filter, замена активных фейков, обновление IPSet, удаление служб) |
| ОС | Windows 10 1809+ / 11, x64 |

---

## 1. Анализ Flowseal/zapret-discord-youtube

### 1.1 Состав релиза

Релизный архив `zapret-discord-youtube-<ver>.zip` (также `.rar`, `.tar.gz`) собирается из
`bin/`, `lists/`, `utils/` и `*.bat`. Папка `.service/` в релиз **не входит**: она лежит только
в репозитории, и с неё скачиваются `version.txt`, `hosts` и `ipset-service.txt`.

```
<zapret>\
  general.bat, general (ALT).bat … general (ALT13).bat, general (EXP).bat,
  general (FAKE TLS AUTO[ ALT|ALT2|ALT3]).bat, general (SIMPLE FAKE[ ALT|ALT2]).bat   ← 22 стратегии
  service.bat                     ← меню управления, LOCAL_VERSION=1.10.3
  bin\  winws.exe, WinDivert.dll, WinDivert64.sys, cygwin1.dll,
        *.bin (фейки: quic_initial_*, tls_clienthello_*, stun*.bin),
        ACTIVE_DISCORD_UDP.bin, ACTIVE_GAME_UDP.bin   ← «активные» фейки (копии одного из *.bin)
  lists\ list-general.txt, list-google.txt, list-exclude.txt, ipset-all.txt, ipset-exclude.txt
         *-user.txt                ← создаются при первом запуске, в git не хранятся
         ipset-all.txt.backup      ← загруженный IP-список, когда IPSet выключен
  utils\ test zapret.ps1, targets.txt, check_updates.enabled, game_filter.enabled (если включён)
         test results\             ← отчёты тестов
```

### 1.2 Как работает стратегия

Каждый `general*.bat` устроен одинаково:

1. `call service.bat status_zapret`: если служба `zapret` работает, выводит сообщение и делает `pause`.
   Затем включает TCP timestamps (`netsh interface tcp set global timestamps=enabled`).
2. `call service.bat check_updates`: фоновая проверка версии, если есть `utils\check_updates.enabled`.
3. `call service.bat load_game_filter`: выставляет `%GameFilterTCP%` и `%GameFilterUDP%`.
   Когда фильтр выключен, значение равно **`12`** (порт-заглушка).
4. `call service.bat load_user_lists`: создаёт недостающие `*-user.txt`.
5. `start "zapret: %~n0" /min "%BIN%winws.exe" <аргументы через ^-переносы>`.

В аргументах встречаются только `%BIN%`, `%LISTS%`, `%GameFilterTCP%` и `%GameFilterUDP%`.
Единственная особенность экранирования: `^!` в стратегиях `FAKE TLS AUTO*` (например,
`--dpi-desync-fake-tls=^!`), который превращается в `!`. Ровно один вызов `winws.exe` на файл.
Значит, аргументы можно надёжно извлечь парсером, не запуская `cmd`.

### 1.3 Функции `service.bat` и как их делаем мы

| Пункт меню `service.bat` | Механизм в оригинале | ZapretGUI 2.0 |
|---|---|---|
| 1. Install Service | Разбирает выбранный `.bat`, затем `sc create zapret binPath= "\"…\winws.exe\" ARGS" DisplayName= "zapret" start= auto`, `sc description … "Zapret DPI bypass software"`, `sc start`, пишет имя стратегии в `HKLM\System\CurrentControlSet\Services\zapret` → `zapret-discord-youtube` (REG_SZ) | То же через SCM API (крейт `windows-service`), та же запись в реестре. `service.bat → Check Status` увидит нашу службу |
| 2. Remove Services | `net stop` + `sc delete zapret`, `taskkill winws.exe`, остановка и удаление `WinDivert` и `WinDivert14` | То же через SCM |
| 3. Check Status | `sc query`, `tasklist` | Фоновый монитор состояния (§5.4) |
| 4. Game Filter | Файл `utils\game_filter.enabled`: `mode=disabled\|all\|tcp\|udp`, `tcp=<диапазоны>`, `udp=<диапазоны>` (по умолчанию `1024-65535`). Принимает и старый формат (одно слово) | Чтение и запись в том же формате, с той же валидацией диапазонов |
| 5. IPSet Filter | Содержимое `lists\ipset-all.txt`: пустой файл = `any`; строка `203.0.113.113/32` = `none`; иначе `loaded`. При переходе из `loaded` список переименовывается в `ipset-all.txt.backup`, при возврате восстанавливается | Те же правила и те же имена файлов |
| 6. Auto-Update Check | Наличие файла `utils\check_updates.enabled` | Переключатель в Настройках синхронизируется с этим файлом (§12) |
| 7. Replace active fakes | Копирует выбранный `bin\*.bin` в `ACTIVE_DISCORD_UDP.bin` или `ACTIVE_GAME_UDP.bin`; текущий определяет по SHA-256 | Расширенные настройки |
| 8. Update IPSet List | Скачивает `.service/ipset-service.txt` в `lists\ipset-all.txt` | Расширенные настройки. Если IPSet не в режиме `loaded`, пишем в `.backup`, чтобы не сломать текущий режим |
| 9. Update Hosts File | Скачивает `.service/hosts`, сверяет первую и последнюю строки, открывает Блокнот для ручного копирования | Кнопка «Обновить hosts». Мы работаем от администратора и можем записать файл сами: показываем, что изменится, после подтверждения обновляем блок между маркерами и сохраняем резервную копию (§5.9) |
| 10. Check for Updates | `raw.githubusercontent.com/…/main/.service/version.txt` против `LOCAL_VERSION` | `UpdateChecker` (§5.10) |
| 11. Run Diagnostics | 15 проверок, интерактивное удаление конфликтов, очистка кэша Discord | Карточка «Диагностика» и «Исправить автоматически» (§5.7) |
| 12. Run Tests | Запуск `utils\test zapret.ps1` в отдельном окне PowerShell | Свой движок тестов на Rust по алгоритму скрипта (§5.8) |

### 1.4 Тесты (`utils/test zapret.ps1`)

- **Стандартный режим.** Цели берутся из `utils\targets.txt` (строки `Key = "https://…"` или
  `Key = "PING:1.2.3.4"`, разделы отмечены комментариями `### Discord`, `### YouTube`,
  `### Google`, `### Cloudflare`, `### Public DNS`). Для каждой стратегии: остановить `winws`,
  запустить `.bat`, подождать появления `winws` (до 5 с, затем ещё 300 мс). Потом параллельно
  (8–16 потоков) выполнить `curl.exe -I -s -m 4 --connect-timeout 2` в трёх вариантах
  (`--http1.1`, `--tlsv1.2 --tls-max 1.2`, `--tlsv1.3 --tls-max 1.3`) и ICMP-пинг с таймаутом 1 с.
  Результаты: `OK`, `ERROR`, `UNSUP` (код 35 или «not supported») и `SSL` (подмена DNS или сертификата).
- **Режим DPI (TCP 16–20 КБ).** Набор целей скачивается с
  `https://hyperion-cs.github.io/dpi-checkers/ru/tcp-16-20/suite.v2.json`. По каждой цели и
  каждому из трёх протоколов отправляется POST на 64 КБ случайных данных. `LIKELY_BLOCKED`
  означает, что данные ушли, ответ не пришёл и сработал таймаут. На время теста IPSet переводится
  в `any`: копия в `lists\ipset-all.test-backup.txt` и флаг `ipset_switched.flag` в корне;
  восстановление при следующем запуске, если тест прервали.
- **Лучшая стратегия** = больше всего `OK` по HTTP, при равенстве больше удачных пингов.
- Перед тестом проверяется, что служба `zapret` **не установлена** (иначе скрипт отказывается
  работать), запоминаются запущенные `winws`, после теста они восстанавливаются.
- Отчёт сохраняется в `utils\test results\test_results_YYYY-MM-DD_HH-mm-ss.txt`.

### 1.5 Диагностика (`service.bat`, пункт 11) — полный список проверок

BFE (служба Base Filtering Engine) · системный прокси (`HKCU\…\Internet Settings` ProxyEnable/ProxyServer) ·
TCP timestamps · процесс `AdguardSvc.exe` · службы Killer · Intel Connectivity Network Service ·
Check Point (`TracSrvWrapper`, `EPWD`) · SmartByte · кириллица в пути · папка внутри OneDrive ·
наличие `bin\*.sys` · VPN-службы · Secure DNS (`HKLM\…\Dnscache\InterfaceSpecificParameters\*\DohFlags > 0`) ·
записи youtube.com/youtu.be в hosts · «висящий» WinDivert (служба работает без `winws`) ·
конфликтующие обходы (`GoodbyeDPI`, `discordfix_zapret`, `winws1`, `winws2`) · очистка кэша
Discord (Stable/PTB/Canary/Development: `Cache`, `Code Cache`, `GPUCache`).

Важно: `sc query` без параметров показывает **только запущенные** службы, поэтому поиск
Killer/Intel/VPN/SmartByte/Check Point в оригинале идёт только среди активных служб.
Повторяем это поведение.

---

## 2. Анализ Shalobol111/zapret-gui (v1.0.0)

Python 3 + CustomTkinter, один файл `zapret_gui.py` (около 2850 строк), PyInstaller.

**Что берём (идеи и алгоритмы):**
- `FilterManager`: работа с фильтрами через файлы, без запуска `.bat`.
- `UpdateChecker`: два источника версии (`version.txt`, затем GitHub API) и понятные тексты
  сетевых ошибок; сравнение версий с буквенными суффиксами (`1.9.9c`).
- `ZapretUpdater`: остановка служб и `winws`, распаковка, поиск корня по `service.bat`;
  сохранение `*-user.txt`, `ipset-all.txt(.backup)`, `*.enabled`; переименование занятых файлов в `.old`.
- `AutostartManager`: `schtasks /sc onlogon /rl highest`. Это единственный способ запускать
  программу с манифестом администратора без UAC.
- Трей, экспорт, импорт и сброс настроек.

**Найденные ошибки на текущей версии Flowseal 1.10.3 (не переносим):**

| # | Ошибка | Последствие |
|---|---|---|
| 1 | Кнопка «Обновить hosts» вызывает пункт меню **8**, а это теперь *Update IPSet List* | `ipset-all.txt` перезаписывается вместо обновления hosts |
| 2 | «Диагностика» вызывает пункт **10**, а это теперь *Check for Updates* | Диагностика не запускается |
| 3 | Регулярное выражение ищет `set /p "choice=Input file index`, а сейчас там `Input option (0-N…)` | Установка службы молча не происходит |
| 4 | Game Filter читается по первой строке, а `service.bat` теперь пишет `mode=all` | Включённый фильтр всегда показывается как «off»; диапазоны портов теряются |
| 5 | Стратегия запускается через `shell=True` и видимую консоль `.bat` | Мигающие окна; `pause` в `status_zapret` зависает, если служба работает |
| 6 | Каждое действие патчит копию `service.bat` регулярками | Ломается при любом изменении меню у Flowseal (причина ошибок 1–3) |
| 7 | После обновления zapret служба просто перезапускается | В `binPath` остаются **старые** аргументы; новые стратегии не применяются |

Вывод: опора на номера пунктов меню и тексты `service.bat` ненадёжна. Работаем с файлами
состояния и системными API напрямую (§5).

---

## 3. Технологии

### 3.1 Почему Tauri 2
- Макет написан на HTML/CSS: токены, отступы, SVG-иконки и CSS-переходы темы переносятся
  почти дословно. Это требование «строго по макету».
- Плавная смена темы (DESIGN §6) делается одним правилом `transition` из макета, без
  пересчёта цветов в цикле.
- WebView2 уже есть в Windows 10/11. Размер `.exe` около 8–12 МБ против примерно 20 МБ у
  PyInstaller, и ложных срабатываний антивирусов меньше.
- Минус: потребление памяти около 80–150 МБ (WebView2). Для такой программы это приемлемо.

### 3.2 Зависимости

**Rust (`src-tauri/Cargo.toml`)**

| Крейт | Назначение |
|---|---|
| `tauri` 2 (features `tray-icon`, `image-png`) | Окно, IPC, трей |
| `tauri-plugin-single-instance` | Одна копия программы; повторный запуск показывает окно |
| `tauri-plugin-window-state` | Размер и положение окна |
| `tauri-plugin-dialog` | Выбор папки и файлов (Обзор…, Экспорт, Импорт) |
| `tauri-plugin-clipboard-manager` | «Вставить из буфера» |
| `tauri-plugin-opener` | Открыть ссылку или папку |
| `windows-service` | Создание, удаление, запуск, остановка и опрос служб |
| `windows` (Win32) | Проверка прав (TokenElevation), ICMP (`IcmpSendEcho2`), `EnumServicesStatusExW`, `RegNotifyChangeKeyValue`, Job Objects |
| `winreg` | Реестр (имя стратегии службы, прокси, DoH, тема Windows) |
| `sysinfo` | Процессы `winws.exe`, `AdguardSvc.exe`, Discord: PID, время старта, командная строка |
| `tokio` (рантайм Tauri) | Фоновые задачи, параллельные проверки, отмена |
| `reqwest` (`native-tls` = Schannel) | Версия, скачивание релиза, hosts, ipset, набор DPI |
| `zip`, `sha2` | Распаковка релиза, проверка SHA-256 |
| `serde`, `serde_json` | Настройки, IPC |
| `regex` | Разбор `targets.txt`, версий, вывода `netsh` |
| `tracing`, `tracing-appender` | Журнал в файл и в интерфейс |
| `thiserror` / `anyhow` | Ошибки |

**Frontend (`package.json`)**: `svelte@5`, `@sveltejs/vite-plugin-svelte`, `vite`,
`typescript`, `@tauri-apps/api@2` и плагины, `@fontsource/onest` (400/500/600, latin + cyrillic),
`@fontsource/jetbrains-mono` (400/500), `codemirror@6` (`@codemirror/view`, `state`, `language`,
`commands`; номера строк, виртуализация больших файлов). Иконки — inline-SVG с путями **из макета**
(они совпадают с Lucide: power, activity, list, settings, sun, moon, shield-check, chevron-down,
play, arrow-left-right).

**Инструменты разработки**: Rust stable (MSVC), Visual Studio Build Tools 2022 (C++),
Node.js 22 LTS, `@tauri-apps/cli@2`.

---

## 4. Архитектура

### 4.1 Слои

```
┌────────────── WebView2 (Svelte) ──────────────┐
│ Screens: Main · Check · Lists · Settings      │
│ Components · Stores (status, settings, log)   │
│ api.ts — типизированные invoke / listen       │
└───────────────▲──────────────┬────────────────┘
       events   │              │ commands (invoke)
┌───────────────┴──────────────▼────────────────┐
│ commands/*  — тонкий слой IPC, валидация      │
├───────────────────────────────────────────────┤
│ zapret/*    — доменная логика (без Tauri)     │
│ sys/*       — Win32: службы, реестр, процессы │
│ settings · log · monitor (фоновый опрос)      │
└───────────────────────────────────────────────┘
```

Модули `zapret/*` и `sys/*` не зависят от Tauri, поэтому их можно тестировать обычным `cargo test`.

### 4.2 Структура репозитория

```
zapret-gui/
├─ PLAN.md, DESIGN.md, README.md, LICENSE
├─ package.json, vite.config.ts, tsconfig.json, index.html
├─ src/                              # интерфейс
│  ├─ main.ts, App.svelte            # каркас: Sidebar + активный экран, Ctrl+1…4
│  ├─ styles/tokens.css              # токены тёмной и светлой темы ДОСЛОВНО из макета
│  ├─ styles/base.css                # шрифты, transition темы, focus-visible, reduced-motion
│  ├─ lib/api.ts, lib/types.ts       # обёртки invoke/listen + типы (зеркало Rust)
│  ├─ lib/stores/{status,settings,log,theme,tests}.ts
│  ├─ lib/icons.ts                   # SVG-пути из макета
│  ├─ components/
│  │   Sidebar, NavItem, AdminBadge, ThemeButton, Card, H1Row,
│  │   Button (primary|secondary|danger), Segmented, Switch, Select,
│  │   Dot, StatusPill, ResultCell, Badge, PowerButton, LogView,
│  │   ListEditor (CodeMirror), Dialog, ConfirmDialog, Toast, Spinner
│  └─ screens/ Main.svelte, Check.svelte, Lists.svelte, Settings.svelte
└─ src-tauri/
   ├─ Cargo.toml, build.rs           # манифест requireAdministrator
   ├─ tauri.conf.json, capabilities/default.json
   ├─ icons/                         # иконки приложения и трея (вкл/выкл)
   ├─ tests/fixtures/flowseal-1.10.3/  # все general*.bat, targets.txt (MIT) для тестов парсера
   └─ src/
      ├─ main.rs, lib.rs             # builder, плагины, трей, состояние, запуск монитора
      ├─ state.rs                    # AppState: настройки, раннер, тесты, кэш статуса
      ├─ events.rs                   # имена событий и payload-типы
      ├─ settings.rs                 # JSON-настройки, миграция с zapret-gui
      ├─ log.rs                      # tracing → файл + кольцевой буфер → событие
      ├─ monitor.rs                  # опрос winws/службы, рассылка status://changed
      ├─ commands/ {app,bypass,service,filters,lists,diag,tests,update,system}.rs
      ├─ zapret/
      │   layout.rs      # пути к bin/lists/utils, проверка папки, версия LOCAL_VERSION
      │   strategy.rs    # поиск *.bat, натуральная сортировка как в service.bat
      │   batparse.rs    # извлечение аргументов winws из .bat
      │   runner.rs      # запуск и остановка winws.exe, захват вывода
      │   service.rs     # служба zapret поверх sys::scm
      │   filters.rs     # IPSet и Game Filter
      │   fakes.rs       # активные фейки
      │   userlists.rs   # создание *-user.txt, чтение и запись списков
      │   hosts.rs       # обновление hosts
      │   ipset.rs       # обновление ipset-all
      │   diagnostics.rs, fixes.rs
      │   tests/ {mod,targets,standard,dpi,curl,ping,scoring,report}.rs
      │   updater.rs     # проверка версии, скачивание, применение, первая установка
      └─ sys/ {elevation,scm,registry,process,netsh,schtasks,theme,net}.rs
```

### 4.3 IPC — команды

| Группа | Команды |
|---|---|
| app | `bootstrap() → {settings, isAdmin, systemTheme, appVersion, status}` · `quit()` |
| settings | `settings_get()` · `settings_set(patch)` · `settings_export(path)` · `settings_import(path)` · `settings_reset()` |
| zapret | `zapret_detect() → [кандидаты путей]` · `zapret_validate(path)` · `zapret_set_path(path)` · `strategies_list()` |
| bypass | `bypass_start(strategy)` · `bypass_stop()` · `bypass_restart()` |
| service | `service_install(strategy)` · `service_remove()` · `service_start()` · `service_stop()` |
| filters | `filters_get()` · `ipset_set(mode)` · `game_set(mode)` · `game_ports_set(tcp, udp)` |
| extras | `fakes_get()` · `fakes_set(kind, file)` · `ipset_update()` · `hosts_check()` · `hosts_apply()` |
| lists | `lists_index()` · `list_read(id) → {text, hash, eol, count}` · `list_save(id, text, baseHash)` |
| diag | `diag_run()` · `diag_fix(actions[])` |
| tests | `tests_start({kind, strategies[]})` · `tests_cancel()` · `tests_last()` |
| update | `update_check()` · `update_apply()` · `zapret_download(dest)` |
| system | `autostart_set(bool)` · `open_path(id)` · `open_url(id)` (только URL из белого списка) |

**События:** `status://changed` (полный снимок `Status`, только при изменении), `log://line`,
`tests://progress`, `diag://item`, `update://progress`, `theme://system`, `nav://goto`
(из трея и меню).

### 4.4 Модель состояния (Rust → TS)

```ts
type Status = {
  zapret:  { path: string|null; valid: boolean; error?: string; version?: string; strategies: string[] };
  bypass:  { state: 'off'|'starting'|'on'|'stopping'; source?: 'gui'|'service'|'external';
             strategy?: string; pid?: number; startedAt?: number /* unix ms */ };
  service: { state: 'not_installed'|'running'|'stopped'|'pending'; strategy?: string; outdatedArgs?: boolean };
  filters: { ipset: 'off'|'any'|'loaded'; game: 'off'|'tcp'|'udp'|'tcp-udp'; tcpPorts: string; udpPorts: string; backupExists: boolean };
  update:  { state: 'idle'|'checking'|'current'|'available'|'error'; latest?: string; error?: string };
  isAdmin: boolean;
};
```

`monitor.rs` опрашивает процессы и службу раз в 1,5 с (`sysinfo` обновляет только процессы,
без CPU и дисков) и отправляет событие только при изменении. Таймер «работает 00:42:17»
считается во фронтенде от `startedAt` через `setInterval(1000)`, без IPC каждую секунду (DESIGN §7).

### 4.5 Настройки

Файл `%APPDATA%\ZapretGUI\settings.json`:

```json
{
  "schema": 1,
  "zapretPath": "C:\\zapret",
  "lastStrategy": "general (ALT2)",
  "theme": "system",                // system | dark | light
  "autostart": false,
  "minimizeToTray": true,
  "checkUpdates": true,
  "logCollapsed": false,
  "stopBypassOnExit": true,
  "restartOnFilterChange": "ask",   // ask | always | never
  "lastTests": { "kind": "standard", "file": "tests-last.json" }
}
```

- При первом запуске, если рядом с прошлой версией лежит `gui_settings.json` от zapret-gui,
  переносим `program_path`, `last_strategy`, `theme`, `minimize_to_tray` и `check_updates`.
- Запись атомарная: временный файл и `rename`.
- Экспорт, импорт и сброс — карточка «Настройки GUI». Импорт проверяет схему и игнорирует
  неизвестные ключи.

### 4.6 Журнал

- `tracing` пишет в `%APPDATA%\ZapretGUI\logs\gui.log` (ротация по дням, хранить 7 файлов)
  и в кольцевой буфер на 1000 записей.
- Записи для интерфейса: `{time, level: ok|info|warn|err, text}`. `ok` — отдельный уровень
  для успешных действий («Стратегия «…» запущена»).
- Вывод `winws.exe` (stdout и stderr) идёт в журнал с префиксом `winws:`. Это главный источник
  причин, если стратегия не стартует (нет драйвера, занят WinDivert, ошибка в аргументах).
- Счётчик в шапке журнала: число `warn` и `err` за сессию, с правильным склонением
  («1 предупреждение», «3 предупреждения», «2 ошибки»).

---

## 5. Ядро zapret (Rust)

### 5.1 `layout.rs` — папка zapret
- Папка считается правильной, если в ней есть `service.bat`, `bin\winws.exe` и хотя бы один
  стратегический `.bat`.
- Отдельно проверяем `bin\WinDivert64.sys` и `bin\WinDivert.dll`. Если файлов нет, скорее
  всего их удалил антивирус: показываем подсказку про исключения.
- Версия: `set "LOCAL_VERSION=…"` из `service.bat`.
- Автопоиск при первом запуске: `C:\zapret*`, `C:\Tools\*`, `%USERPROFILE%\{Desktop,Downloads,Documents}\*`
  (глубина 2; ищем `service.bat` + `bin\winws.exe`).

### 5.2 `strategy.rs` + `batparse.rs` — стратегии
- Список: все `*.bat` в корне, кроме `service*`, с натуральной сортировкой
  (числа дополняются нулями до 8 знаков, как в `service.bat`). Отображаемое имя — имя файла
  без `.bat`, как в макете («general (ALT2)»).
- Парсер:
  1. Читаем файл как UTF-8 (с BOM или без).
  2. Находим строку с `winws.exe` и склеиваем следующие строки, пока текущая заканчивается на `^`.
  3. Берём хвост после `winws.exe"`.
  4. Подставляем `%BIN%` → `<root>\bin\`, `%LISTS%` → `<root>\lists\`, `%~dp0` → `<root>\`,
     `%GameFilterTCP%`, `%GameFilterUDP%`, `%GameFilter%` → значения из `filters.rs`
     (`12`, если фильтр выключен), `%%` → `%`.
  5. Снимаем `^`-экранирование вне кавычек (`^!` → `!`).
  6. Разбиваем по пробелам с учётом кавычек → `Vec<String>`.
- Любая неизвестная `%VAR%`, ноль или больше одного `winws.exe` → `ParseError` с понятным
  текстом. Для запуска есть **запасной режим**: выполнить сам `.bat` скрытым `cmd /c` с
  `NO_UPDATE_CHECK=1` и `stdin=NUL` (чтобы `pause` не зависал). Установка службы в этом
  случае недоступна, и интерфейс объясняет почему.
- Тесты парсера: golden-тесты на всех 22 стратегиях из `tests/fixtures`. Проверяем, что
  аргументы не пустые, все пути существуют после подстановки, нет символов `%` и `^`,
  а `--wf-tcp` и `--wf-udp` содержат значения Game Filter.

### 5.3 `runner.rs` — запуск обхода
- Перед стартом, как делают `.bat`:
  - создать недостающие `*-user.txt` с теми же значениями по умолчанию:
    `ipset-exclude-user.txt` = `203.0.113.113/32`;
    `list-general-user.txt` = `# Never leave this file empty` + `domain.example.abc`;
    `list-exclude-user.txt` = `domain.example.abc`;
  - включить TCP timestamps, если выключены.
- Запуск: `Command::new(<root>\bin\winws.exe).args(args).current_dir(<root>\bin)`,
  флаг `CREATE_NO_WINDOW`, stdout и stderr через пайпы в журнал. Rust сам корректно
  экранирует аргументы для `CreateProcess`.
- Состояние `starting` длится, пока процесс жив 1,5 с. Если он завершился раньше, переходим
  в `off`, а последние строки вывода показываем как ошибку.
- Остановка: сначала `terminate` нашего процесса, затем завершение всех `winws.exe`
  (`sysinfo`), ждём до 3 с.
- Если служба `zapret` работает, ручной запуск запрещён (как в `status_zapret`): кнопка
  питания в этом случае управляет службой (§6.4).
- Внешний `winws.exe` (запущен вручную через `.bat` или другой программой) распознаём по
  командной строке. Сравниваем аргументы с разобранными стратегиями и показываем имя, если
  совпало, иначе «запущен вне программы». Время работы берём из времени старта процесса.
- Выход из программы: если `stopBypassOnExit` и обход запущен из GUI, останавливаем `winws`.
  Чтобы обход работал без программы, нужна служба; это подсказывает карточка «Служба Windows».

### 5.4 `service.rs` — служба `zapret`
- **Установка** (`service_install(strategy)`):
  1. Разобрать стратегию; при ошибке отказать.
  2. Остановить ручной `winws`.
  3. Остановить и удалить старую службу `zapret`.
  4. `CreateService("zapret", display "zapret", SERVICE_AUTO_START, binPath = "<bin>\winws.exe" <args>)`.
     Аргументы с пробелами берём в кавычки по правилам MSVCRT.
  5. Описание «Zapret DPI bypass software», затем запуск.
  6. Записать в реестр `zapret-discord-youtube = <имя стратегии>`.
  7. Сохранить хэш аргументов в настройках (`serviceArgsHash`).
- **Статус**: `not_installed | running | stopped | pending` (STOP_PENDING → предупреждение
  «возможен конфликт с другим обходом», как в оригинале), имя стратегии из реестра.
  `outdatedArgs = true`, если `binPath` отличается от аргументов, которые получились бы сейчас.
  Так бывает после смены фильтров, редактирования `.bat` или обновления zapret; интерфейс
  предлагает «Переустановить».
- **Удаление**: остановить и удалить `zapret`, завершить `winws.exe`, остановить и удалить
  `WinDivert` и `WinDivert14`.
- Важно: Game Filter и IPSet-режим, заданный через `%GameFilter%`, «вшиты» в `binPath`.
  После их изменения служба продолжит работать со старыми значениями, пока её не переустановят.
  Списки доменов и IP читаются из файлов, поэтому для них хватит перезапуска службы.

### 5.5 `filters.rs` — фильтры
- IPSet: `get` по правилам §1.3. `set(mode)` выполняет переходы с `ipset-all.txt.backup`
  так же, как `service.bat`:
  - `loaded → off`: переименовать в `.backup` и записать заглушку;
  - `loaded → any`: переименовать в `.backup` и создать пустой файл;
  - `off ↔ any`: перезаписать файл;
  - `→ loaded`: восстановить `.backup`; если копии нет, ошибка «Сначала обновите IPSet-список».
- Game: чтение нового формата (`mode=`, `tcp=`, `udp=`) и старого (одно слово `all|tcp|udp`).
  Запись всегда в новом формате. Диапазоны проверяются по правилам `service.bat`: числа 1–65535,
  `a-b` при a ≤ b, через запятую, без пробелов.
- После изменения, если обход запущен: в зависимости от `restartOnFilterChange` выполняется
  перезапуск или показывается уведомление «Перезапустить сейчас?». Если установлена служба,
  уведомление предлагает переустановить её (см. §5.4).
- Значения интерфейса ↔ файл: `Выкл/Любой/Загруженный` ↔ `off(none)/any/loaded`;
  `Выкл/TCP/UDP/TCP+UDP` ↔ `disabled/tcp/udp/all`.

### 5.6 `userlists.rs` — списки (экран «Списки»)

| Пункт в интерфейсе (группа) | Файл | Примечание |
|---|---|---|
| Свои домены (Домены) | `lists\list-general-user.txt` | Основной для пользователя |
| Основной список (Домены) | `lists\list-general.txt` | Плашка «перезапишется при обновлении zapret» |
| Исключения (Домены) | `lists\list-exclude-user.txt` | В макете подписано `list-exclude.txt`, но этот файл перезаписывается при обновлении; решено открывать `-user` (§13) |
| IPSet (IP-адреса) | `lists\ipset-all.txt` | Если IPSet не в режиме `loaded`, редактируем `ipset-all.txt.backup` и показываем пояснение. Файл около 570 КБ, 30+ тыс. строк, поэтому нужен CodeMirror с виртуализацией |
| Исключения IP (IP-адреса) | `lists\ipset-exclude-user.txt` | То же замечание, что для «Исключений» |

- Чтение возвращает текст, хэш, тип окончаний строк (CRLF/LF) и количество записей.
- Сохранение атомарное и с проверкой `baseHash`: если файл изменили снаружи, показываем
  диалог «Файл изменён другой программой — перезаписать / загрузить заново».
- Пишем UTF-8 без BOM, сохраняя исходные окончания строк.
- Подвал «9 доменов» = непустые строки без `#`. Для IP-списков — «N адресов».
- Подсветка некорректных строк (не домен и не IP/CIDR) цветом `warn` с подсказкой. Сохранять
  это не мешает.
- «Вставить из буфера»: добавить строки в конец, убрать дубликаты и пробелы, вывести в журнал
  «Добавлено N, пропущено M дубликатов».
- `list-general-user.txt` не должен оставаться пустым (комментарий Flowseal «Never leave this
  file empty»): при сохранении пустого файла добавляем `domain.example.abc`.
- Доступ только к файлам из этой таблицы: `id` → путь на стороне Rust, произвольные пути из UI
  не принимаются.

### 5.7 `diagnostics.rs` + `fixes.rs` — диагностика

Строки карточки (группировка под макет, где около 6 строк; список прокручивается):

| Строка | Что проверяем | Цвет и текст результата | Автоисправление |
|---|---|---|---|
| Base Filtering Engine | служба BFE работает | ок / `err` «не запущена» | Запустить BFE (`StartService`) |
| Системный прокси | ProxyEnable / ProxyServer | нет / `warn` «включён: host:port» | — (только подсказка, иначе можно сломать корпоративную сеть) |
| TCP timestamps | `netsh interface tcp show global` (через `chcp 437`, чтобы вывод был на английском, как в оригинале) | вкл / `warn` «выкл» | `netsh interface tcp set global timestamps=enabled` |
| Adguard, Killer, VPN | `AdguardSvc.exe`; активные службы: Killer, Intel Connectivity Network, TracSrvWrapper/EPWD, SmartByte, *VPN* | не найдены / `err` «найдено: …» (во всплывающей подсказке список и ссылки на issues) | — (только подсказка) |
| Конфликт WinDivert | WinDivert работает без `winws`; службы GoodbyeDPI, discordfix_zapret, winws1, winws2 | нет / `err` «…» | Остановить и удалить конфликтующие службы, затем WinDivert и WinDivert14 |
| Secure DNS | DohFlags > 0 в реестре | ок / `err` «не настроен» | — (кнопка «Как настроить», открывает `ms-settings:network-status` и инструкцию из README) |
| Папка zapret | кириллица в пути, OneDrive, наличие `WinDivert64.sys` | ок / `warn` или `err` | — (подсказка «переместите в C:\zapret») |
| Файл hosts | записи youtube.com / youtu.be | ок / `warn` | Закомментировать эти строки (с резервной копией) |
| Кэш Discord | наличие папок кэша у 4 версий Discord | — | Закрыть Discord и удалить `Cache`, `Code Cache`, `GPUCache` |

- Проверки выполняются параллельно, каждая отправляет `diag://item`, и строки заполняются по
  мере готовности. Пока проверка идёт, вместо точки показывается спиннер.
- «Исправить автоматически» открывает диалог со списком доступных исправлений (галочки,
  по умолчанию включены безопасные: TCP timestamps, BFE). Удаление служб и кэша Discord
  отмечается вручную. После применения диагностика запускается заново.
- Подсказка внизу карточки собирается из найденных проблем («Включите TCP timestamps и
  Secure DNS — это частая причина, почему стратегии не работают»).
- Ссылки на issues Flowseal — те же, что в `service.bat`.

### 5.8 `tests/*` — проверка стратегий

Движок повторяет алгоритм `test zapret.ps1`, но отдаёт структурированные данные в таблицу.

- **Подготовка.**
  - Если служба `zapret` установлена: диалог «Тесты не работают при установленной службе.
    Остановить её на время проверки?». Мы останавливаем службу (не удаляем) и запускаем после теста.
  - Запоминаем запущенный `winws` и после теста возвращаем прежнее состояние.
  - Проверяем `curl.exe` (`%SystemRoot%\System32\curl.exe`).
  - Если есть `ipset_switched.flag`, сначала восстанавливаем IPSet.
- **Стандартный тест.**
  - Цели из `utils\targets.txt` (тот же формат; при отсутствии файла — встроенный список,
    как в скрипте). Колонки таблицы — разделы `### Discord`, `### YouTube`, `### Google`,
    как в макете. Остальные разделы (Cloudflare, DNS) входят в «Итог» и видны в подсказке строки.
  - Для каждой стратегии: остановить `winws`, запустить стратегию через `runner` (без `.bat`),
    дождаться процесса (5 с + 300 мс), затем параллельно 16 задач `curl.exe` с теми же
    параметрами и пинг через `IcmpSendEcho2` с таймаутом 1000 мс.
  - Классификация как в скрипте: OK / ERROR / UNSUP / SSL.
  - Ячейка группы: «ок», если у каждой URL-цели раздела все поддерживаемые проверки OK
    (UNSUP не учитывается); «нет» в остальных случаях; «—», если стратегия не запустилась или
    ещё не проверена. В подсказке детали: «3/4 цели, TLS1.3 ERROR у gateway.discord.gg».
  - «Итог» = число разделов «ок» / число разделов («2/3», как в макете).
- **DPI-тест (TCP 16–20).**
  - IPSet → `any` с теми же `ipset-all.test-backup.txt` и `ipset_switched.flag`
    (совместимость со скриптом); восстановление в `finally` и при следующем запуске программы.
  - Набор целей скачивается из `suite.v2.json`. Если скачать не удалось, тест не запускается
    и выводится ошибка.
  - Колонки заменяются на «HTTP · TLS 1.2 · TLS 1.3 · Итог»: ячейка «ок», если ни у одной цели
    нет FAIL или LIKELY_BLOCKED по этому протоколу; «Итог» = OK / всего.
- **Лучшая стратегия** — по формуле скрипта (максимум OK, при равенстве максимум удачных
  пингов): фон строки `row` и бейдж «лучшая». После теста появляется уведомление
  «Лучшая: general (ALT2) · [Выбрать на Главной]».
- **Отмена**: во время теста кнопка «Проверить» превращается в «Остановить». Отмена
  останавливает `winws`, восстанавливает IPSet и прежнее состояние.
- **Сохранение**:
  - отчёт в `utils\test results\test_results_<дата>.txt` в формате скрипта (совместимость);
  - JSON последнего прогона в `%APPDATA%\ZapretGUI\tests-last.json`, чтобы таблица
    восстанавливалась при следующем запуске.
- Выбор стратегий — чекбоксы; «выбрано 4 из 6». Выбор сохраняется между запусками.

### 5.9 `hosts.rs` — «Обновить hosts»
1. Скачать `https://raw.githubusercontent.com/Flowseal/zapret-discord-youtube/refs/heads/main/.service/hosts`
   (с параметром против кэша).
2. Сравнить с `%SystemRoot%\System32\drivers\etc\hosts`.
3. Если всё актуально, сообщить «hosts актуален».
4. Иначе показать диалог: «будут добавлены N строк, заменены M».
5. После подтверждения: резервная копия `hosts.zapretgui.bak`; удалить старый блок между
   `# zapret-gui begin` / `# zapret-gui end` и строки с теми же доменами, что в новом файле;
   дописать новый блок; `ipconfig /flushdns`.
6. Если запись заблокирована (антивирус защищает hosts), использовать запасной путь
   оригинала: открыть скачанный файл и папку hosts и показать инструкцию.

### 5.10 `updater.rs` — обновление и установка zapret
- **Проверка.** Источник 1: `raw.githubusercontent.com/Flowseal/zapret-discord-youtube/main/.service/version.txt`
  (`Cache-Control: no-cache`, таймаут 10 с). Источник 2: GitHub API `releases/latest` → `tag_name`.
  Сравнение версий по алгоритму zapret-gui. При сетевой ошибке статус `error` с понятным текстом,
  устаревшей версию в этом случае **не** считаем.
- **Скачивание.** Ассет `zapret-discord-youtube-<ver>.zip` из API. Проверяем SHA-256 по полю
  `digest` ассета (если GitHub его отдаёт) и размер. Защищаемся от zip-slip. Корень ищем по
  `service.bat`.
- **Применение.**
  1. Запомнить состояние: служба и её стратегия, ручной `winws` и его стратегия.
  2. Остановить службы `zapret`, `WinDivert`, `WinDivert14` и все `winws`.
  3. Скопировать файлы, **сохраняя** `*-user.txt`, `ipset-all.txt`, `ipset-all.txt.backup`,
     `*.enabled`, `ACTIVE_*.bin` (если пользователь менял фейки) и `utils\test results\`.
     Занятые файлы переименовываем в `.old`.
  4. Если служба была, **переустановить** её с той же стратегией (новые аргументы). Если
     стратегии больше нет, сообщить об этом и оставить службу удалённой.
  5. Вернуть ручной запуск, если он был.
- **Первая установка** (папка не задана или неверна): «Скачать zapret» → выбор папки
  (по умолчанию `C:\zapret`; проверяем, что в пути нет кириллицы, пробелов и OneDrive) →
  скачивание и распаковка → проверка, что `WinDivert64.sys` и `winws.exe` на месте (иначе
  подсказка про антивирус) → путь сохраняется → список стратегий обновляется.
  Файлы пишем сами, поэтому метки Mark-of-the-Web («Разблокировать» из README) не появляется.
- Проверка при старте выполняется тихо, раз за запуск, если `checkUpdates` включён. Результат
  отображается в шапке Главной («zapret 1.10.3 · обновлений нет» / «· доступна 1.10.4») и в
  карточке Настроек.

### 5.11 `fakes.rs`, `ipset.rs` — расширенные настройки
- Активные фейки: список `bin\*.bin` без `ACTIVE_*`; текущий определяется по SHA-256;
  замена копированием. После замены — перезапуск или переустановка (как в §5.5).
- Обновление IPSet-списка: скачать `.service/ipset-service.txt` в `ipset-all.txt` (если режим
  `loaded`) или в `.backup`; в журнал пишется число строк.

### 5.12 `sys/*` — системный уровень
- `elevation.rs`: `TokenElevation`. Без прав (только при запуске из исходников) — плашка
  «Без прав администратора», действия, требующие прав, недоступны с подсказкой.
- `theme.rs`: `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\AppsUseLightTheme`
  и поток с `RegNotifyChangeKeyValue`, который при изменении отправляет `theme://system`.
  Вместо опроса раз в несколько секунд из DESIGN §6.5 используем мгновенное уведомление без
  нагрузки. Читать тему через `prefers-color-scheme` в WebView нельзя: мы сами задаём тему окна,
  и WebView начинает отражать её, а не системную.
- `schtasks.rs`: задача `ZapretGUI_Autostart` (`/sc onlogon /rl highest /tr "\"<exe>\" --minimized"`).
  Флаг `--minimized` запускает программу сразу в трей.
- `netsh.rs`: TCP timestamps (с `chcp 437`), `ipconfig /flushdns`. Все внешние вызовы делаются
  через `Command` с аргументами массивом, без сборки строк для `cmd /c`.

---

## 6. Интерфейс

### 6.1 Перенос DESIGN.md с Tk на Tauri

| DESIGN.md | Реализация в Tauri |
|---|---|
| §0 Окно 960×640, минимум 780×600 | `tauri.conf.json`: `width 960, height 640, minWidth 780, minHeight 600`; `tauri-plugin-window-state`. Системная рамка Windows, тёмный или светлый заголовок через `window.setTheme()` |
| §0 Без эмодзи, монохромные иконки | Только SVG из `icons.ts`, `stroke="currentColor"`, толщина 1.8 (меню) и 2 (питание, логотип) |
| §2 Каркас, sidebar 208 px | CSS из макета без изменений (flex, отступы 20/12, пункты меню по 40 px) |
| §3 Экраны | Svelte-экраны, разметка повторяет `*.dc.html` |
| §4 Токены | `tokens.css`: оба набора переменных дословно из макета (`.zr` и `.zr.light`), включая `--accent-soft`, `--accent-ring`, `--err-soft`, `--segshadow`. В CSS полупрозрачность работает, смешивать цвета заранее не нужно |
| §4 Главная кнопка одна на экран | Компонент `Button variant="primary"`; в ревью проверяем, что на экране не больше одной |
| §5 Шрифты Onest / JetBrains Mono, `AddFontResourceExW` | `@fontsource` (woff2) встраиваются в сборку, интернет не нужен. Запасной стек: `'Onest', 'Segoe UI', system-ui` / `'JetBrains Mono', Consolas, monospace` |
| §5 Иконки Lucide → PNG через Pillow, `CTkImage` | Inline-SVG; цвет берётся из `currentColor` и анимируется вместе с темой |
| §6 `ThemeManager`, анимация 400–500 мс, ease-out | Правило из макета: `.zr, .zr * { transition: background-color .5s, border-color .5s, color .5s, box-shadow .5s; cubic-bezier(.4,0,.2,1) }`; при `prefers-reduced-motion` анимация отключается. CodeMirror и журнал тоже используют переменные, поэтому меняют цвет плавно (замена «перекраски тегов `tk.Text`») |
| §6.4 Запасной вариант с `-alpha` | Не нужен |
| §6.5 «Системная» тема | `sys/theme.rs` + событие `theme://system` |
| §6.6 Один параметр для кнопки солнце/луна и сегмента | `settings.theme`. Кнопка в sidebar переключает между тёмной и светлой (если было `system`, переходит на противоположную текущей), как в `Settings.dc.html` |
| §7 Блокировка кнопок во время операций | Кнопки в состоянии `busy`: `disabled` и текст «Проверка…», «Обновление…», «Установка…» |
| §7 Фокус 2 px `accent`, Ctrl+1…4 | `:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px }` из макета; горячие клавиши через `keydown` на `window` |
| §7 Долгие операции в фоне | Асинхронные команды Tauri, прогресс через события |
| §7 Таймер раз в секунду | Считается на фронтенде, см. §4.4 |

**Без вспышки при запуске:**
- окно создаётся скрытым (`visible: false`) с `backgroundColor` нужной темы;
- `bootstrap()` до первой отрисовки ставит класс темы на корень (без перехода: класс
  `no-transition` снимается через один кадр);
- затем окно показывается (`show()`), если нет флага `--minimized`.

**Масштаб 100–150 %**: WebView2 сам учитывает DPI. На шаге 6 проверяем, что размеры
окна в логических пикселях дают ту же раскладку.

### 6.2 Общие компоненты (по макету)

| Компонент | Параметры из макета |
|---|---|
| NavItem | высота 40, radius 10, padding 0 12, gap 12, иконка 18. Активный: `raised`, `tx`, 500; неактивный: `dim`. У «Главной» справа точка 6 px `accent`, когда обход включён |
| AdminBadge | высота 40, рамка `line`, щит 14 px `accent`, 12 `dim`; без прав — щит `warn`, текст «Без прав администратора» |
| ThemeButton | 40×40, рамка `line`, иконка 17; солнце в тёмной теме, луна в светлой |
| Card | `card`, рамка 1 `line`, radius 16 |
| Segmented | дорожка `seg` + рамка `line`, radius 9, padding 3, gap 2; сегмент высотой 28 (30 в шапке «Проверки»), radius 7, 12/500; выбранный — `segon` + `segshadow` |
| Switch | 40×24, radius 12; вкл — `accent` и ручка `onaccent` (left 19); выкл — `line2` и ручка `dim` (left 3); ручка двигается за 0,18 с |
| Button | primary: `tx` / текст `bg`, 13/600; secondary: прозрачная, рамка `line2`, текст `tx`; danger: текст `err`, рамка `danger-line`. Высоты 30/32/34/36/38 — как в конкретном месте макета |
| Select | высота 40, radius 10, `raised`, рамка `line2`, chevron 16 справа. Свой выпадающий список (нативный `<select>` в WebView2 не поддерживает тему и плавную смену цвета) |
| ResultCell | моноширинный 12, padding 2 8, radius 6; «ок» `accent` на `accent-soft`, «нет» `err` на `err-soft`, «—» `mute` |
| Badge «лучшая» | 11, padding 2 8, radius 6, `accent` на `accent-soft` |
| LogView | моноширинный 12/22, колонки 72 / 44 / 1fr; виртуальная прокрутка, автопрокрутка вниз, если пользователь не прокрутил вверх |

**Компоненты вне макета** (нужны для логики; оформляем по тем же токенам, вид согласуем на скриншотах):
- **Dialog / ConfirmDialog**: карточка radius 16 по центру, затемнение фона, одна главная кнопка.
- **Toast**: снизу справа, `card` и рамка `line`, автоскрытие через 5 с, может содержать
  кнопку действия («Перезапустить»).
- **Spinner**: кольцо 12 px цвета `faint`, вместо точки в строках диагностики и ячеек во время теста.

### 6.3 Каркас и навигация
- `App.svelte`: Sidebar и область `main` (padding 24/28, gap 16). Экраны переключаются без
  перезагрузки, состояние каждого сохраняется: несохранённый текст редактора и таблица тестов
  остаются при переходах.
- Ctrl+1…4 — разделы; Ctrl+S — сохранить список (на «Списках»).
- Уход со «Списков» с несохранёнными изменениями → диалог «Сохранить / Не сохранять / Отмена».
- Узкое окно (780–899 px): sidebar сворачивается до 64 px (§13, п. 2).

### 6.4 Главная (`Main.dc.html`)
- **Шапка**: «Главная»; справа `zapret <ver> · обновлений нет` / `· доступна X` (клик →
  Настройки) / `· папка не выбрана`.
- **Кнопка питания и состояние.** Логика зависит от службы:
  - *Служба не установлена*: питание запускает или останавливает `winws` с выбранной стратегией.
    Смена стратегии во время работы сразу перезапускает обход (журнал: «Стратегия изменена → …»).
  - *Служба установлена*: питание запускает или останавливает **службу**. В списке показана
    стратегия службы. Выбор другой стратегии → диалог «Служба установлена с «X». Переустановить
    с «Y»?».
  - Заголовок: «Обход включён» / «Обход выключен» (22/600) с точкой 8 px (`accent` / `mute`).
  - Подзаголовок: «general (ALT2) · работает 00:42:17», при работе службы
    «general (ALT2) · служба · работает …», при внешнем запуске «запущен вне программы · …».
  - Пока идёт запуск или остановка: «Запуск…» / «Остановка…», кнопка недоступна.
  - Нет папки zapret: «Укажите папку zapret в Настройках» и ссылка-кнопка, питание недоступно.
- **Стратегия**: свой Select (ширина до 380) и ссылка «Подобрать лучшую →» на «Проверку».
  Выбор сохраняется в `lastStrategy`. Стратегии, которые не удалось разобрать, помечаются
  «запуск через .bat».
- **Фильтры**: два Segmented (§5.5). Пока файл меняется, сегменты недоступны. При ошибке
  показывается уведомление, и выбор возвращается к фактическому состоянию.
- **Служба Windows**:
  - статус справа: «Не установлена» (`warn`) / «Работает» (`accent`) / «Остановлена» (`dim`) /
    «Ожидание остановки» (`warn`);
  - описание: «Без службы обход выключится после перезагрузки.»; когда служба установлена —
    «Установлена с «general (ALT2)»». Если `outdatedArgs`, описание «Параметры службы устарели»
    (`warn`) и кнопка «Переустановить»;
  - кнопки: «Установить с этой стратегией» (главная; при установленной службе и другой
    выбранной стратегии — «Переустановить с этой стратегией») и «Обновить hosts» (вторичная).
- **Журнал**: шапка 44 px, «Журнал», счётчик предупреждений, «Свернуть» / «Развернуть»;
  состояние хранится в `logCollapsed`. Свёрнутый журнал не растягивается, карточки сверху
  остаются на месте.

### 6.5 Проверка (`Check.dc.html`)
- Справа от заголовка Segmented: «Сайты · HTTP / Ping» | «DPI · TCP 16–20». Во время теста
  переключатель недоступен.
- **Стратегии** (карточка с гибкой шириной):
  - шапка: «Стратегии», «выбрано N из M», главная кнопка «▶ Проверить» (иконка play 12) /
    «Остановить» во время теста;
  - таблица `28px | 1fr | 64 | 64 | 64 | 64`, строки по 44 px; заголовки колонок 11,
    uppercase, разрядка 0.06em;
  - текущая проверяемая строка подсвечена `row`, у её ячеек спиннер;
  - под шапкой во время теста тонкая строка прогресса «3/6 · general (ALT3) · Discord…»
    (12 `faint`; в макете её нет, вид согласуем);
  - клик по имени стратегии раскрывает детали по каждой цели (моноширинный 12, `tx2`).
- **Диагностика** (260 px): «Запустить» (вторичная) → строки §5.7 → подсказка → «Исправить
  автоматически» (вторичная с фоном `raised`, как в макете; недоступна, если исправлять нечего).
  Диагностика один раз запускается сама при первом открытии экрана за сессию.

### 6.6 Списки (`Lists.dc.html`)
- Шапка: «Списки», справа «Изменения применяются после перезапуска стратегии». После
  сохранения при запущенном обходе показывается уведомление «Перезапустить» (для службы —
  перезапуск службы).
- Слева карточка 220 px (группы «Домены» и «IP-адреса», §5.6). Активный пункт `raised`,
  неактивные `tx2`, имя файла 11 моно `faint`. У изменённого, но не сохранённого файла точка `warn`.
- Справа редактор:
  - шапка: имя файла (моно 13), «● изменён» (`warn`), «Вставить из буфера», «Отменить»
    (откат к сохранённому), главная «Сохранить»;
  - CodeMirror: фон `editor`, номера строк `gutter`, комментарии `#` цветом `mute`, моно 13/24,
    отступы 12/16;
  - подвал 36 px: «9 доменов · UTF-8 · Ctrl+S — сохранить».

### 6.7 Настройки (`Settings.dc.html`)
- **zapret**:
  - «Папка с service.bat»: поле (моно) и «Обзор…». Проверка идёт при вводе (с задержкой) и
    после выбора папки;
  - под полем «Найдено 22 стратегии» (`accent`) или ошибка (`err`): «В папке нет service.bat»,
    «Нет bin\winws.exe», «Нет WinDivert64.sys — проверьте карантин антивируса»;
  - папка не задана: вместо ошибки текст «Папка не выбрана» и кнопка **«Скачать zapret»** (§5.10);
  - разделитель; «Версия X»; «Доступна Y — списки доменов сохранятся» (`warn`); главная кнопка
    «Обновить», если есть обновление, иначе вторичная «Проверить». Во время работы —
    «Проверка…» / «Обновление… 42 %».
- **Поведение**: три переключателя с подписями и подсказками дословно из макета: автозапуск
  (через Планировщик задач, без запроса UAC), трей (обход продолжит работать в фоне),
  проверка обновлений (тихо, при запуске). Строка «Тема» с Segmented «Системная | Тёмная |
  Светлая» и подсказкой «Как в Windows — сейчас тёмная» / «Переключается плавно, без перезапуска».
- **Дополнительно** (новая карточка, в макете её нет; тот же стиль строк, что в «Поведении»):
  - «Порты Game Filter»: два поля (TCP, UDP; моно, по умолчанию `1024-65535`), проверка
    по §5.5, кнопка «Применить»;
  - «Активные фейки»: два Select («Discord UDP», «Game UDP») со списком `bin\*.bin`;
  - «IPSet-список»: «Обновить» и подпись «N адресов · обновлён ДД.ММ»;
  - «Служба zapret»: «Удалить службы» (danger, с подтверждением) — по DESIGN §1 опасное
    действие убрано с Главной;
  - «Открыть папку zapret» / «Открыть журнал программы» — ссылки.
- **Настройки GUI**: «Экспорт», «Импорт», «Сбросить» (danger, с подтверждением).
- Экран прокручивается (`overflow: auto`, как в макете).

### 6.8 Трей
- Иконка: стрелки «туда-обратно» из логотипа. Два варианта: `accent` (обход включён) и
  серый (выключен). Подсказка: «zapret — обход включён · general (ALT2)».
- Меню: «Открыть» (по умолчанию, двойной клик) · «Включить обход» / «Выключить обход» ·
  «Стратегия ▸» (радиосписок) · разделитель · «Выход».
- Закрытие окна при `minimizeToTray` прячет его в трей; при первом таком закрытии
  показываем уведомление Windows «ZapretGUI работает в трее».

---

## 7. Безопасность

Программа работает от администратора и управляет сетевым драйвером, поэтому:
- **WebView закрыт**: CSP `default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'`,
  никакого удалённого контента. В `capabilities` разрешены только нужные команды; `shell` не
  подключён; `opener` открывает только URL из белого списка (github.com/Flowseal, issues из
  диагностики, ms-settings:).
- **Пути**: из UI не принимаются произвольные пути для чтения и записи. Списки задаются по `id`.
  Папку zapret пользователь выбирает только через диалог или поле, затем она проверяется.
- **Процессы**: никаких строк для `cmd /c`, только `Command` с массивом аргументов. Исключение —
  запасной запуск `.bat` (§5.2); путь к нему берётся из проверенного списка стратегий.
- **Сеть**: только HTTPS к `raw.githubusercontent.com`, `api.github.com`, `github.com`
  (и CDN релизов), `hyperion-cs.github.io`. Проверка SHA-256, если GitHub отдаёт `digest`.
  Защита от zip-slip.
- **hosts и службы** меняются только после явного подтверждения в диалоге, с резервной копией.
- **Диагностика** ничего не удаляет без отметки пользователя.

---

## 8. Сборка и распространение
- `npm run tauri build` → `target/release/zapret-gui.exe` (портативная версия) и NSIS-установщик
  `ZapretGUI_2.0.0_x64-setup.exe`.
- `build.rs`: `tauri_build::try_build(Attributes::new().windows_attributes(WindowsAttributes::new().app_manifest(MANIFEST)))`.
  Манифест: `requireAdministrator`, `PerMonitorV2` DPI, совместимость с Win10/11.
- WebView2: `webviewInstallMode: { type: "embedBootstrapper" }` для старых систем без WebView2.
- Идентификатор `io.github.shalobol111.zapretgui` (можно поменять), имя продукта `ZapretGUI`, версия `2.0.0`.
- CI (GitHub Actions, `windows-latest`): `cargo fmt --check`, `cargo clippy -D warnings`,
  `cargo test`, `npm run check` (svelte-check), сборка; при теге `v*` — релиз с `.exe`,
  установщиком и SHA-256.
- Подпись кода — по возможности (снижает число срабатываний SmartScreen и антивирусов).
- README: требования, первый запуск, отличия от zapret-gui 1.x, раздел про антивирус и WinDivert.

---

## 9. Тестирование

| Уровень | Что проверяем |
|---|---|
| Unit (Rust) | `batparse` на 22 реальных стратегиях; натуральная сортировка; `filters` (все переходы IPSet, оба формата Game Filter, валидация портов) во временных папках; сравнение версий; разбор `targets.txt`; классификация вывода curl; формула лучшей стратегии; создание `*-user.txt`; копирование при обновлении с сохранением пользовательских файлов; zip-slip |
| Интеграционные (Rust, `#[ignore]`, только на ВМ с правами) | установка, статус и удаление службы; запуск и остановка `winws`; запись hosts |
| Frontend | `svelte-check`; vitest для форматтеров (склонения, время работы, разбор статуса) |
| Ручной чек-лист (ВМ Win10 и Win11) | первый запуск без zapret → скачивание; запуск каждой стратегии; служба (установить → перезагрузка → работает); смена фильтров при работающей службе → `outdatedArgs`; тесты обоих типов с отменой посередине; диагностика с подстроенными проблемами (выключенные timestamps, фиктивная служба GoodbyeDPI); обновление zapret со старой версии (1.10.2 → 1.10.3) с установленной службой; трей и автозапуск после входа в систему; совместимость с `service.bat` (Check Status видит нашу службу; изменения фильтров из `service.bat` видны в GUI) |
| Визуальная проверка | скриншоты каждого экрана в обеих темах при 100 % и 150 % рядом с макетом (DESIGN §8) |

---

## 10. План работ

Порядок соответствует DESIGN.md §8. После каждого шага — короткий отчёт и скриншоты в обеих темах.

| Шаг | Содержание | Готово, когда |
|---|---|---|
| **1. Каркас** | Проект Tauri 2 + Svelte + TS; манифест администратора; настройки и журнал; `tokens.css` из макета; шрифты; иконки; Sidebar (логотип, 4 пункта, AdminBadge, ThemeButton); переключение экранов-заглушек с H1; Ctrl+1…4; плавная смена темы; системная тема (`theme.rs`); запуск без вспышки; одна копия программы | Окно 960×640 выглядит как sidebar макета в обеих темах; тема переключается плавно за 0,5 с; при смене темы Windows режим «Системная» подхватывает её |
| **2. Главная** | `layout`, `strategy`, `batparse` (+ golden-тесты), `runner`, `monitor`, `filters`, `service`, `hosts`; экран полностью: питание, Select, фильтры, служба («Установить с этой стратегией»), журнал | Запуск и остановка всех стратегий; служба ставится и видна в `service.bat → Check Status`; фильтры совместимы с `service.bat` в обе стороны |
| **3. Проверка** | `tests` (стандартный и DPI, отмена, восстановление IPSet, отчёт), `diagnostics`, `fixes`, экран, «Исправить автоматически» | Результаты совпадают с `test zapret.ps1` на той же машине (сравнить «Best config»); прерванный DPI-тест не оставляет IPSet в режиме `any` |
| **4. Списки** | `userlists`, CodeMirror, подсчёт записей, валидация, буфер обмена, Ctrl+S, защита несохранённых изменений, проверка внешних изменений файла | `ipset-all.txt` на 30 тыс. строк открывается мгновенно; Ctrl+S работает; уведомление о перезапуске появляется |
| **5. Настройки** | путь, автопоиск, «Скачать zapret», `updater` (проверка и применение с переустановкой службы), «Поведение», «Дополнительно», «Настройки GUI», трей, автозапуск, миграция с zapret-gui | Обновление 1.10.2 → 1.10.3 сохраняет пользовательские списки и переустанавливает службу; автозапуск без UAC после входа |
| **6. Финал** | Проверка в обеих темах при 100 % и 150 %, узкое окно 780 px, доступность с клавиатуры, README, CI, сборка `.exe` и установщика | Чек-лист §9 пройден; релизная сборка весит меньше 15 МБ |

---

## 11. Риски

| Риск | Что делаем |
|---|---|
| Flowseal изменит формат стратегий (например, несколько вызовов `winws` или новые переменные) | Парсер выдаёт явную ошибку → запасной запуск `.bat`; golden-тесты обновляем при каждом релизе Flowseal; CI-задача раз в неделю скачивает свежий релиз и прогоняет парсер |
| Антивирус удаляет WinDivert или блокирует hosts | Проверка файлов после установки и обновления, подсказки про исключения; ручной режим для hosts |
| GitHub недоступен у провайдера | Два источника версии; понятные сообщения; рекомендация проверять обновления при включённом обходе; записи hosts от Flowseal покрывают домены GitHub |
| Вывод `netsh` на другом языке | `chcp 437` (как в оригинале); на случай неудачи — разбор по ключевому слову и проверка кода возврата |
| Диагностика под администратором видит HKCU администратора, а не пользователя | Так же работает и оригинал. Учитываем при проверке прокси: при «Run as» под другим пользователем показываем предупреждение |
| WebView2 отсутствует или повреждён | Встроенный загрузчик WebView2 в установщике |
| Память WebView2 (около 100 МБ) | Окно в трее не рисует; журнал ограничен 1000 строками; CodeMirror виртуализирует строки |

---

## 12. Совместимость с `service.bat`
Обе программы можно использовать по очереди, ничего не ломая:
- одинаковые файлы и форматы: `game_filter.enabled`, `ipset-all.txt(.backup)`, `*-user.txt`,
  `check_updates.enabled`, `ACTIVE_*.bin`;
- одинаковая служба: имя `zapret`, описание, `start= auto`, запись `zapret-discord-youtube` в реестре;
- одинаковые артефакты тестов: `ipset-all.test-backup.txt`, `ipset_switched.flag`, `utils\test results\`;
- переключатель «Проверять обновления zapret» в GUI синхронизируется с `utils\check_updates.enabled`,
  чтобы ручной запуск `.bat` вёл себя так же.

---

## 13. Решения по открытым вопросам (2026-10-13)
1. **Файлы «Исключений» — решено: user-файлы.** «Исключения» открывают `list-exclude-user.txt`,
   «Исключения IP» — `ipset-exclude-user.txt`; подписи имён файлов в карточке соответствуют
   реальным файлам. Базовые `list-exclude.txt` и `ipset-exclude.txt` не показываются.
2. **Минимальная ширина 780 px — решено: сворачивать меню.** При ширине окна меньше 900 px
   sidebar сворачивается до 64 px: остаются логотип-квадрат, иконки пунктов (с подсказкой по
   наведению), точка «обход включён» в углу иконки «Главной», щит прав и кнопка темы
   (друг под другом). Ширина анимируется 0,2 с.
3. **Остановка обхода при выходе — решено: оставить.** `stopBypassOnExit = true` без
   переключателя в интерфейсе; постоянная работа — задача службы.
4. **Вид новых элементов** (Dialog, Toast, строка прогресса тестов, карточка «Дополнительно»)
   согласуем по скриншотам на шагах 2, 3 и 5.


---

## 14. Версия 2.1 — фишки (план, 2026-10-14)

Цель: «поставил — и YouTube с Discord работают, а если провайдер сломает стратегию, программа сама
это заметит». Четыре функции выбраны пользователем; решения по ним:

| Вопрос | Решение |
|---|---|
| Автопилот при сбое | Режим в Настройках: «Автоматически / Предупреждать / Выкл», по умолчанию «Предупреждать» |
| Где «Мой сайт» и «Починить Discord» | В «Проверке»: третий режим «Мой сайт» в переключателе типа теста; «Починить Discord» — кнопка в карточке «Диагностика». Пятого раздела меню нет |
| Мастер настройки | Только при первом запуске |

### 14.1 Общий движок: быстрый подбор стратегии
Четыре функции опираются на одно: быстро проверить стратегии **по нескольким целям** и выбрать рабочую.
Существующий тест (§5.8) проверяет 16 целей у всех стратегий — 3–5 минут; для автопилота, мастера и
«Моего сайта» это долго.

- `tests::quick_pick(targets, order, stop_at_first_ok)` — тот же запуск winws и тот же curl, но:
  - набор целей задаётся вызывающим (Discord + YouTube для автопилота и мастера, один домен для
    «Моего сайта», Discord для «Починить Discord»);
  - порядок стратегий: сначала **история удачных** (последняя рабочая, лучшие из прошлых проверок),
    затем остальные в натуральном порядке;
  - остановка на первой стратегии, у которой прошли все цели (обычно 10–40 с вместо минут);
  - отмена, восстановление прежнего состояния и IPSet — как у обычной проверки.
- История: `%APPDATA%\ZapretGUI\history.json` — для каждой стратегии время последнего успеха и неудачи.
  Пополняется обычными проверками, быстрым подбором и автопилотом.
- Новый вид прогона `Kind::Custom` с колонками HTTP / TLS 1.2 / TLS 1.3 — для «Моего сайта» таблица
  показывает результат по выбранному домену.

### 14.2 Уведомления Windows
- `tauri-plugin-notification` (уведомления Windows, иконка программы).
- Когда показываются:
  - автопилот сменил стратегию или не нашёл рабочую;
  - обход неожиданно остановился (winws упал, служба остановилась не по команде программы);
  - доступна новая версия zapret (проверка при запуске);
  - проверка стратегий закончилась, пока окно было свёрнуто в трей;
  - первый раз окно спрятано в трей — «ZapretGUI продолжает работать в трее».
- Клик по уведомлению открывает окно на нужном экране.
- Настройки → «Поведение»: переключатель «Уведомления Windows» (по умолчанию вкл).

### 14.3 Автопилот
- Настройки → «Поведение»: строка «Автопилот» с переключателем «Автоматически | Предупреждать | Выкл»
  и подсказкой («Раз в 15 минут проверяет Discord и YouTube…»).
- Работает, только когда обход включён (вручную, службой или внешний winws), не идёт проверка и есть
  интернет.
- **Проверка здоровья** раз в 15 минут без перезапуска winws:
  1. контроль интернета — `http://www.msftconnecttest.com/connecttest.txt` (проверка подключения Windows);
     нет ответа — сети нет, проверку пропускаем;
  2. по цели на Discord (`discord.com`, `gateway.discord.gg`) и YouTube (`www.youtube.com`,
     `redirector.googlevideo.com`): curl TLS 1.3, при неподдержке — TLS 1.2;
  3. «сбой» — не открывается ни одна цель Discord или ни одна цель YouTube; подтверждается повторной
     проверкой через 60 с (защита от случайных обрывов).
- **Предупреждать**: уведомление «Стратегия «ALT2» перестала открывать YouTube» с кнопкой «Подобрать» —
  она открывает окно и запускает быстрый подбор.
- **Автоматически**: быстрый подбор (§14.1) по целям Discord + YouTube → найдена рабочая — применить
  (ручной обход перезапускается, служба переустанавливается) и уведомить; не найдена — вернуть прежнюю,
  уведомить «Рабочая стратегия не найдена — запустите полную проверку». Не чаще раза в час.
- Главная: под строкой состояния мелкая подпись «Автопилот · проверено в 12:40» (или «нашёл сбой…»).
- Всё пишется в журнал.

### 14.4 «Мой сайт» (Проверка)
- Переключатель типа теста: «Сайты · HTTP / Ping | DPI · TCP 16–20 | Мой сайт».
- Карточка слева: поле «Адрес сайта» (принимает домен или ссылку) и «Проверить»; ниже — пошаговый
  результат строками, как в «Диагностике» (точка · проверка · итог):
  1. **Списки zapret** — домен или родительский домен есть в `list-general*.txt` / `list-google.txt`;
     не исключён в `list-exclude*.txt`; IP сайта в IPSet и режим IPSet.
  2. **DNS** — системный ответ против DoH (Cloudflare `1.1.1.1/dns-query`, запасной — Google): если
     системный DNS отдаёт заглушку или адреса, которых нет в DoH, — «провайдер подменяет DNS» →
     совет включить Secure DNS.
  3. **Доступ с обходом** — curl HTTP / TLS 1.2 / TLS 1.3 при текущей стратегии.
  4. **Ответ сайта** — 403/451 при успешном TLS: «сайт сам ограничивает доступ из вашей страны —
     zapret тут не поможет» (честно, чтобы не перебирать стратегии впустую).
- **Вердикт** одной фразой и действия:
  - «Добавить в Свои домены» (если домена нет в списках) — с предложением перезапустить обход;
  - «Подобрать стратегию для этого сайта» — быстрый подбор по этому домену (§14.1) с таблицей результатов;
  - «Сравнить без обхода» — по желанию, временно останавливает winws (видно, режет ли сайт провайдер).
- Карточка «Диагностика» справа остаётся.

### 14.5 «Починить Discord» (Диагностика)
- Кнопка «Починить Discord» в карточке «Диагностика» (вторичная, над «Исправить автоматически»).
- Диалог со списком шагов и галочками; затем выполнение с прогрессом по шагам:
  1. закрыть Discord (Stable / PTB / Canary / Development), если запущен;
  2. очистить кэш (`Cache`, `Code Cache`, `GPUCache`) — уже есть в «Исправить автоматически»;
  3. обновить hosts записями Flowseal для голосовых серверов (§5.9);
  4. проверить, что обход включён и стратегия содержит UDP-фильтры голоса Discord
     (`19294-19344`, `50000-50100`); нет — предупредить;
  5. проверить `discord.com`, `gateway.discord.gg`, `cdn.discordapp.com`, `updates.discord.com`;
     не открываются — быстрый подбор по целям Discord;
  6. запустить Discord снова (`%LOCALAPPDATA%\Discord\Update.exe --processStart Discord.exe`).
- Итог: что сделано и что осталось. Голосовой UDP напрямую проверить нельзя (нужен протокол Discord) —
  это честно указано в итоге.

### 14.6 Мастер первого запуска
- Показывается один раз: настройка `onboardingDone = false` и при старте нет рабочей папки zapret.
  «Пропустить» тоже ставит `onboardingDone = true`.
- Полноэкранная карточка поверх окна (те же токены, одна главная кнопка на шаг), четыре шага с точками
  прогресса:
  1. **zapret** — «Найден в …» (автопоиск) или «Скачать в C:\zapret» / «Указать папку».
  2. **Подбор** — быстрый подбор по Discord + YouTube с прогрессом («Проверяю ALT3… 4 из 22»), можно
     пропустить.
  3. **Готово к работе** — найденная стратегия, кнопка «Включить», после включения — короткая проверка.
  4. **Запускать вместе с Windows?** — «Установить службу (рекомендуется)» / «Запускать программу» /
     «Не нужно».
- Без прав администратора шаги 2–4 недоступны с объяснением (в релизе права есть всегда).

### 14.7 Порядок работ (продолжение §10)

| Шаг | Содержание | Готово, когда |
|---|---|---|
| **7. Движок и уведомления** | `quick_pick`, история удачных стратегий, `Kind::Custom`, плагин уведомлений, уведомления о падении обхода и об обновлении, переключатель в Настройках | Быстрый подбор находит рабочую стратегию за секунды–десятки секунд; уведомления появляются и открывают нужный экран |
| **8. «Мой сайт»** | Режим в «Проверке», DNS/DoH, проверка списков и IPSet, вердикт, добавление в список, подбор под сайт | Для заблокированного сайта не из списков вердикт «добавьте в Свои домены» и после подбора сайт открывается |
| **9. «Починить Discord»** | Диалог шагов, перезапуск Discord, проверка UDP-фильтров | Сценарий проходит целиком с Discord и без него |
| **10. Автопилот** | Проверка здоровья, три режима, подбор и применение, подпись на Главной | Сломанная стратегия (имитация) заменяется рабочей в режиме «Автоматически» и вызывает уведомление в «Предупреждать» |
| **11. Мастер** | Четыре шага, автопоиск или скачивание, подбор, служба или автозапуск | Чистая система: от первого запуска до работающего YouTube — без захода в другие разделы |

### 14.8 Риски
- **Ложные срабатывания автопилота** (сеть пропала, Wi-Fi переключился) — контроль интернета и
  повторная проверка через 60 с, подбор не чаще раза в час.
- **Подбор рвёт соединение на время перебора** — в «Предупреждать» без согласия ничего не
  перебирается; в «Автоматически» перебор начинается, только когда соединение и так сломано.
- **DoH заблокирован** — два DoH-провайдера; если оба недоступны, шаг DNS «не удалось проверить»,
  остальные шаги работают.
- **Уведомления выключены в Windows (фокусировка внимания)** — дублируются в журнал; автопилот работает
  независимо от них.


---

## 15. Telegram: TgWsProxy (2026-10-24)

Обход блокировки Telegram — локальный MTProto-прокси [Flowseal/tg-ws-proxy](https://github.com/Flowseal/tg-ws-proxy):
Telegram Desktop → 127.0.0.1:1443 → WebSocket к серверам Telegram (резерв — Cloudflare). zapret для него не нужен.

| Вопрос | Решение (пользователь) |
|---|---|
| Где в интерфейсе | Пятый раздел меню «Telegram» (Ctrl+4; «Настройки» — Ctrl+5) |
| Откуда exe | Скачивается с GitHub (`TgWsProxy_windows.exe`, Windows 10+ x64) с проверкой SHA-256 из релиза |
| Подход | Официальный exe, не порт на Rust. У него свой значок в трее — скрыть нельзя |

Устройство (`src-tauri/src/tgproxy.rs`):
- Папка `%APPDATA%\ZapretGUI\tg-ws-proxy\` с `TgWsProxy_data` рядом с exe — портативный режим TgWsProxy:
  его config.json и лог там, отдельно установленная копия (`%APPDATA%\TgWsProxy`) не затрагивается.
- Конфиг пишет ZapretGUI: host 127.0.0.1, порт, секрет (16 случайных байт), DC → IP, Cloudflare;
  `check_updates` и `autostart` самого TgWsProxy выключены, окно первого запуска пропускается (маркер).
- Запуск через `explorer.exe <exe>` — без прав администратора; ссылка `tg://proxy` тоже открывается через
  проводник, Telegram стартует с обычными правами. Остановка — завершение процессов нашей копии (по пути exe).
- Копия, запущенная отдельно, распознаётся по имени exe из релизов: предупреждение и «Закрыть и запустить здесь»
  (у TgWsProxy одиночный экземпляр через мьютекс).
- Статус (установлен, версия, работает, порт слушается, время работы) — в общем Status монитора.
- Обновление: проверка при запуске (если включена проверка обновлений), «обновить до …» в шапке раздела.
- Настройка «Запускать вместе с ZapretGUI». При выходе ZapretGUI прокси не останавливается.

Проверено вживую (`live_tg_cycle`): скачивание и SHA-256, запуск через проводник, порт за 14 с (первая
распаковка), смена порта с перезапуском, остановка без остатков.

