// Типизированные вызовы Rust-ядра. Вне Tauri (vite в браузере — для вёрстки и
// скриншотов) работает заглушка из mock.ts.

import type {
  AppUpdateInfo,
  Bootstrap,
  DiagItem,
  DiscordInfo,
  DiscordStep,
  DiscordStepResult,
  FakeKind,
  Fakes,
  FixId,
  UpdateInfo,
  FixResult,
  ListData,
  ListInfo,
  SiteProbe,
  SiteReport,
  TestKind,
  TestRun,
  TgConfig,
  VpnStore,
  VpnView,
  GameMode,
  HostsPlan,
  IpsetMode,
  LogEntry,
  Mode,
  Settings,
  Status,
  ZapretInfo,
} from "./types";

export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

type Mock = typeof import("./mock");
let mockModule: Promise<Mock> | null = null;
const mock = () => (mockModule ??= import("./mock"));

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!inTauri) return (await mock()).call<T>(cmd, args);
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

async function on<T>(event: string, cb: (payload: T) => void): Promise<() => void> {
  if (!inTauri) return (await mock()).on<T>(event, cb);
  const { listen } = await import("@tauri-apps/api/event");
  return listen<T>(event, (e) => cb(e.payload));
}

/** Текст ошибки команды (Rust возвращает строку). */
export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}

// ── Приложение ─────────────────────────────────────────────────
export const bootstrap = () => call<Bootstrap>("bootstrap");
export const setSettings = (patch: Partial<Settings>) => call<Settings>("settings_set", { patch });
export const onSystemTheme = (cb: (mode: Mode) => void) => on<Mode>("theme://system", cb);

/** Показать окно после первой отрисовки (оно создаётся скрытым). */
export async function showWindow(): Promise<void> {
  if (!inTauri) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow().show();
  await getCurrentWindow().setFocus();
}

// ── Статус и журнал ────────────────────────────────────────────
export const statusGet = () => call<Status>("status_get");
export const onStatus = (cb: (s: Status) => void) => on<Status>("status://changed", cb);
export const journalGet = () => call<LogEntry[]>("journal_get");
export const onLog = (cb: (e: LogEntry) => void) => on<LogEntry>("log://line", cb);

// ── Папка zapret ───────────────────────────────────────────────
export const zapretInspect = (path: string) => call<ZapretInfo>("zapret_inspect", { path });
export const zapretDetect = () => call<string[]>("zapret_detect");
export const zapretSetPath = (path: string) => call<ZapretInfo>("zapret_set_path", { path });

export async function pickFolder(): Promise<string | null> {
  if (!inTauri) return (await mock()).pickFolder();
  const { open } = await import("@tauri-apps/plugin-dialog");
  const r = await open({ directory: true, title: "Папка zapret (с service.bat)" });
  return typeof r === "string" ? r : null;
}

// ── Обход и служба ─────────────────────────────────────────────
export const bypassStart = (strategy: string) => call<void>("bypass_start", { strategy });
export const bypassStop = () => call<void>("bypass_stop");
export const serviceInstall = (strategy: string) => call<void>("service_install", { strategy });
export const serviceRemove = () => call<void>("service_remove");
export const serviceStart = () => call<void>("service_start");
export const serviceStop = () => call<void>("service_stop");

// ── Фильтры и hosts ────────────────────────────────────────────
export const ipsetSet = (mode: IpsetMode) => call<void>("ipset_set", { mode });
export const gameSet = (mode: GameMode) => call<void>("game_set", { mode });
export const hostsCheck = () => call<HostsPlan>("hosts_check");
export const hostsApply = () => call<void>("hosts_apply");

// ── Проверка и диагностика ─────────────────────────────────────
export const testsStart = (kind: TestKind, strategies: string[], stopService: boolean) =>
  call<void>("tests_start", { kind, strategies, stopService });
export const testsCancel = () => call<void>("tests_cancel");
export const testsQuick = (stopService: boolean) => call<void>("tests_quick", { stopService });
export const testsLast = () => call<TestRun | null>("tests_last");
export const onTests = (cb: (r: TestRun) => void) => on<TestRun>("tests://progress", cb);
export const diagRun = () => call<DiagItem[]>("diag_run");
export const diagFix = (fixes: FixId[]) => call<FixResult[]>("diag_fix", { fixes });

// ── «Мой сайт» ─────────────────────────────────────────────────
export const siteCheck = (input: string) => call<SiteReport>("site_check", { input });
export const siteAdd = (domain: string) => call<boolean>("site_add", { domain });
export const siteCompare = (domain: string) => call<SiteProbe>("site_compare", { domain });
export const testsSite = (domain: string, stopService: boolean) => call<void>("tests_site", { domain, stopService });

// ── «Починить Discord» ─────────────────────────────────────────
export const discordInfo = () => call<DiscordInfo>("discord_info");
/** `ids` — для шага launch: какие клиенты запустить (пусто — первый установленный). */
export const discordStep = (step: DiscordStep, ids: string[] = []) => call<DiscordStepResult>("discord_step", { step, ids });

// ── Автопилот ──────────────────────────────────────────────────
export const autopilotCheck = () => call<void>("autopilot_check");

// ── Telegram-прокси ────────────────────────────────────────────
export const tgConfig = () => call<TgConfig>("tg_config");
export const tgConfigSave = (config: Omit<TgConfig, "link">) => call<TgConfig>("tg_config_save", { config });
export const tgNewSecret = () => call<string>("tg_new_secret");
export const tgInstall = () => call<string>("tg_install");
export const tgCheck = () => call<string | null>("tg_check");
export const tgStart = () => call<void>("tg_start");
export const tgStop = () => call<void>("tg_stop");
export const tgRestart = () => call<void>("tg_restart");
export const tgCloseExternal = () => call<void>("tg_close_external");
export const tgConnect = () => call<void>("tg_connect");

export async function writeClipboard(text: string): Promise<void> {
  if (!inTauri) return navigator.clipboard.writeText(text);
  const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
  await writeText(text);
}

// ── Обновление ZapretGUI ───────────────────────────────────────
export const appUpdateCheck = () => call<AppUpdateInfo>("app_update_check");
export const appUpdateInstall = () => call<void>("app_update_install");
/** Уведомление Windows «Доступна ZapretGUI …» → кнопка «Установить». */
export const onAppUpdatePrompt = (cb: () => void) => on<null>("app-update://prompt", () => cb());

// ── VPN ────────────────────────────────────────────────────────
export const vpnView = () => call<VpnView>("vpn_view");
export const vpnInstall = () => call<string>("vpn_install");
/** Подписка (https://…) или ключи (vless://, ss://…). */
export const vpnAdd = (input: string) => call<VpnView>("vpn_add", { input });
export const vpnUpdate = (id: string | null = null) => call<VpnView>("vpn_update", { id });
export const vpnRemove = (id: string) => call<VpnView>("vpn_remove", { id });
export const vpnSelect = (id: string | null) => call<VpnView>("vpn_select", { id });
export const vpnSet = (patch: Partial<VpnStore>) => call<VpnView>("vpn_set", { patch });
export const vpnStart = () => call<void>("vpn_start");
export const vpnStop = () => call<void>("vpn_stop");
export const vpnPing = () => call<Record<string, number | null>>("vpn_ping");

// ── Списки ─────────────────────────────────────────────────────
export const listsIndex = () => call<ListInfo[]>("lists_index");
export const listRead = (id: string) => call<ListData>("list_read", { id });
/** Ошибка "conflict" — файл изменили снаружи после открытия. */
export const listSave = (id: string, text: string, baseHash: string, force = false) =>
  call<ListData>("list_save", { id, text, baseHash, force });

export async function readClipboard(): Promise<string> {
  if (!inTauri) return (await mock()).readClipboard();
  const { readText } = await import("@tauri-apps/plugin-clipboard-manager");
  return (await readText()) ?? "";
}

export type CloseAction = "hide" | "close" | "cancel";

/** Крестик окна: спрятать в трей, закрыть программу или ничего (решает `decide`). */
export async function onCloseRequested(decide: () => Promise<CloseAction>): Promise<void> {
  if (!inTauri) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  const w = getCurrentWindow();
  await w.onCloseRequested(async (e) => {
    e.preventDefault();
    const action = await decide();
    if (action === "hide") await w.hide();
    else if (action === "close") await w.destroy();
  });
}

/** «Выход» из меню трея: окно проверяет несохранённые правки и вызывает quit(). */
export const onQuitRequest = (cb: () => void) => on<null>("app://quit", () => cb());
export const quit = () => call<void>("app_quit");
/** Окно спрятано в трей — ядро один раз покажет уведомление-подсказку. */
export const trayHint = () => call<void>("tray_hint");
export const notificationsSet = (enabled: boolean) => call<Settings>("notifications_set", { enabled });
export const notifyTest = () => call<void>("notify_test");
/** Клик по уведомлению Windows: ядро просит открыть экран. */
export const onNavGoto = (cb: (screen: string) => void) => on<string>("nav://goto", cb);

export async function showMainWindow(): Promise<void> {
  if (!inTauri) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  const w = getCurrentWindow();
  await w.show();
  await w.unminimize();
  await w.setFocus();
}

// ── Обновление zapret ──────────────────────────────────────────
export const updateCheck = () => call<UpdateInfo>("update_check");
export const updateApply = () => call<string>("update_apply");
export const zapretDownload = (dest: string) => call<string>("zapret_download", { dest });

// ── Поведение и дополнительно ──────────────────────────────────
export const autostartSet = (enabled: boolean) => call<Settings>("autostart_set", { enabled });
export const checkUpdatesSet = (enabled: boolean) => call<Settings>("check_updates_set", { enabled });
export const fakesGet = () => call<Fakes>("fakes_get");
export const fakesSet = (kind: FakeKind, name: string) => call<void>("fakes_set", { kind, name });
export const ipsetCount = () => call<number>("ipset_count");
export const ipsetUpdate = () => call<number>("ipset_update");
export const gamePortsSet = (tcp: string, udp: string) => call<void>("game_ports_set", { tcp, udp });
export const openPath = (target: "zapret" | "logs" | "results" | "releases" | "dns" | "tglog" | "tgreleases" | "appreleases" | "vpnlog" | "vpnlist-vpn" | "vpnlist-direct") => call<void>("open_path", { target });

// ── Настройки GUI ──────────────────────────────────────────────
export const settingsExport = (path: string) => call<void>("settings_export", { path });
export const settingsImport = (path: string) => call<Settings>("settings_import", { path });
export const settingsReset = () => call<Settings>("settings_reset");

export async function pickSettingsFile(mode: "save" | "open"): Promise<string | null> {
  if (!inTauri) return mode === "save" ? "C:\\Users\\me\\zapretgui-settings.json" : null;
  const { open, save } = await import("@tauri-apps/plugin-dialog");
  const filters = [{ name: "Настройки ZapretGUI", extensions: ["json"] }];
  const r =
    mode === "save"
      ? await save({ title: "Экспорт настроек", defaultPath: "zapretgui-settings.json", filters })
      : await open({ title: "Импорт настроек (в том числе gui_settings.json от ZapretGUI 1.x)", filters });
  return typeof r === "string" ? r : null;
}
