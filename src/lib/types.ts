// Зеркало типов Rust (settings.rs, monitor.rs, zapret/*). Менять синхронно.

export type ThemeChoice = "system" | "dark" | "light";
export type Mode = "dark" | "light";
export type RestartPolicy = "ask" | "always" | "never";

export interface Settings {
  schema: number;
  zapretPath: string | null;
  lastStrategy: string | null;
  theme: ThemeChoice;
  autostart: boolean;
  minimizeToTray: boolean;
  checkUpdates: boolean;
  logCollapsed: boolean;
  stopBypassOnExit: boolean;
  restartOnFilterChange: RestartPolicy;
  testSelection: string[] | null;
  notifications: boolean;
  trayHintShown: boolean;
  autopilot: AutopilotMode;
  onboardingDone: boolean;
  startBypassOnLaunch: boolean;
  tgAutostart: boolean;
  resumeBypass: string | null;
  appUpdateDismissed: string | null;
}

// ── Автопилот (autopilot.rs) ───────────────────────────────────
export type AutopilotMode = "auto" | "warn" | "off";

export interface AutopilotInfo {
  phase: "idle" | "checking" | "confirming" | "picking";
  lastCheck: number | null;
  outcome: "ok" | "fail" | "offline" | null;
  message: string | null;
  switched: { from: string; to: string; at: number } | null;
}

export interface Bootstrap {
  settings: Settings;
  isAdmin: boolean;
  systemTheme: Mode;
  appVersion: string;
  startHidden: boolean;
}

export type Screen = "main" | "check" | "lists" | "telegram" | "vpn" | "settings";

export interface StrategyInfo {
  name: string;
  /** Аргументы извлечены — запуск без .bat и служба доступны. */
  direct: boolean;
}

export interface ZapretInfo {
  path: string | null;
  valid: boolean;
  error: string | null;
  warning: string | null;
  version: string | null;
  strategies: StrategyInfo[];
  /** Папки нет или она пуста — туда можно скачать zapret. */
  empty: boolean;
}

export type BypassState = "off" | "starting" | "on" | "stopping";
export type BypassSource = "gui" | "service" | "external";

export interface Bypass {
  state: BypassState;
  source: BypassSource | null;
  strategy: string | null;
  pid: number | null;
  startedAt: number | null;
}

export type ServiceState = "not_installed" | "running" | "stopped" | "pending";

export interface ServiceStatus {
  state: ServiceState;
  strategy: string | null;
  outdatedArgs: boolean;
  stopPending: boolean;
  /** winws.exe из binPath не существует — служба не запустится, нужна переустановка. */
  missingExe: boolean;
}

export type IpsetMode = "off" | "any" | "loaded";
export type GameMode = "off" | "tcp" | "udp" | "tcp-udp";

export interface Filters {
  ipset: IpsetMode;
  game: GameMode;
  tcpPorts: string;
  udpPorts: string;
  backupExists: boolean;
}

export interface Status {
  zapret: ZapretInfo;
  bypass: Bypass;
  service: ServiceStatus;
  filters: Filters | null;
  isAdmin: boolean;
  testsRunning: boolean;
  update: UpdateInfo;
  autopilot: AutopilotInfo;
  telegram: TgStatus;
  appUpdate: AppUpdateInfo;
  vpn: VpnStatus;
}

// ── VPN (vpn/*.rs) ─────────────────────────────────────────────
export type VpnInbound = "proxy" | "tun";
export type VpnRouteMode = "lists" | "all_except_ru";

export interface VpnStatus {
  installed: boolean;
  version: string | null;
  latest: string | null;
  running: boolean;
  ready: boolean;
  inbound: VpnInbound | null;
  routeMode: VpnRouteMode | null;
  server: string | null;
  port: number;
  startedAt: number | null;
  busy: string | null;
  progress: number | null;
  error: string | null;
  servers: number;
  xrayInstalled: boolean;
  xrayVersion: string | null;
  viaXray: boolean;
}

export interface VpnServer {
  id: string;
  name: string;
  protocol: string;
  host: string;
  port: number;
  outbound: unknown | null;
  unsupported: string | null;
  link?: string | null;
}

export interface VpnSubscription {
  id: string;
  url: string;
  name: string;
  updatedAt: number | null;
  error: string | null;
  info: { upload: number; download: number; total: number; expire: number } | null;
  servers: VpnServer[];
}

export interface VpnStore {
  subscriptions: VpnSubscription[];
  manual: VpnServer[];
  selected: string | null;
  routeMode: VpnRouteMode;
  inbound: VpnInbound;
  presets: string[];
  zapretDirect: boolean;
  port: number;
  autostart: boolean;
}

export interface VpnView {
  store: VpnStore;
  presets: { id: string; title: string; hint: string; count: number }[];
  pings: Record<string, number | null>;
}

// ── Обновление ZapretGUI (selfupdate.rs) ───────────────────────
export interface AppUpdateInfo {
  state: "idle" | "checking" | "current" | "available" | "downloading" | "installing" | "error";
  current: string;
  latest: string | null;
  notes: string | null;
  url: string | null;
  progress: number | null;
  error: string | null;
  portable: boolean;
}

// ── Telegram-прокси (tgproxy.rs) ───────────────────────────────
export interface TgStatus {
  installed: boolean;
  version: string | null;
  latest: string | null;
  running: boolean;
  listening: boolean;
  port: number;
  startedAt: number | null;
  external: string | null;
  busy: string | null;
  progress: number | null;
}

export interface TgConfig {
  port: number;
  secret: string;
  dcIp: string[];
  cfproxy: boolean;
  link: string | null;
}

export type LogLevel = "ok" | "info" | "warn" | "err";

export interface LogEntry {
  id: number;
  time: string;
  level: LogLevel;
  text: string;
}

export interface HostsPlan {
  upToDate: boolean;
  add: number;
  replace: number;
  total: number;
}

// ── Проверка стратегий (zapret/tests/model.rs) ─────────────────
export type TestKind = "standard" | "dpi" | "custom";
export type CellState = "ok" | "fail" | "none";
export type RowState = "pending" | "running" | "done" | "failed" | "skipped";

export interface Cell {
  state: CellState;
  hint: string;
}

export interface Detail {
  target: string;
  text: string;
  ok: boolean;
}

export interface TestRow {
  name: string;
  state: RowState;
  cells: Cell[];
  total: string | null;
  score: number;
  pingOk: number;
  details: Detail[];
  error: string | null;
}

export interface TestRun {
  kind: TestKind;
  columns: string[];
  rows: TestRow[];
  running: boolean;
  cancelled: boolean;
  best: string | null;
  startedAt: number;
  finishedAt: number | null;
  phase: string | null;
  report: string | null;
  error: string | null;
  /** Быстрый подбор: Discord + YouTube до первой рабочей стратегии. */
  quick: boolean;
  site: string | null;
}

// ── Диагностика (zapret/diagnostics.rs) ────────────────────────
export type DiagLevel = "ok" | "info" | "warn" | "err";
export type FixId = "start_bfe" | "enable_timestamps" | "remove_conflicts" | "comment_youtube_hosts" | "clear_discord_cache";

export interface DiagItem {
  id: string;
  title: string;
  level: DiagLevel;
  value: string;
  detail: string | null;
  fix: FixId | null;
  fixLabel: string | null;
  fixSafe: boolean;
}

export interface FixResult {
  fix: FixId;
  ok: boolean;
  message: string;
}
// ── Списки (zapret/userlists.rs) ───────────────────────────────
export type ListKind = "domains" | "ips";

export interface ListInfo {
  id: string;
  title: string;
  file: string;
  kind: ListKind;
}

export interface ListData extends ListInfo {
  text: string;
  hash: string;
  crlf: boolean;
  note: string | null;
}
// ── Обновление zapret (zapret/updater.rs) ──────────────────────
export type UpdateState = "idle" | "checking" | "current" | "available" | "error" | "applying";

export interface UpdateInfo {
  state: UpdateState;
  latest: string | null;
  error: string | null;
  stage: string | null;
  progress: number | null;
}

// ── Дополнительно (zapret/extras.rs) ───────────────────────────
export type FakeKind = "discord" | "game";

export interface Fakes {
  files: string[];
  discord: string | null;
  game: string | null;
}
// ── «Мой сайт» (zapret/site.rs) ────────────────────────────────
export type SiteAction = "add_to_list" | "pick" | "enable_bypass" | "secure_dns" | "open_exclude" | "compare";

export interface SiteItem {
  id: string;
  title: string;
  level: DiagLevel;
  value: string;
  detail: string | null;
}

export interface SiteReport {
  domain: string;
  items: SiteItem[];
  verdict: string;
  verdictLevel: DiagLevel;
  actions: SiteAction[];
  reachable: boolean;
}

export interface SiteProbe {
  tls12: "ok" | "error" | "unsup" | "ssl";
  tls13: "ok" | "error" | "unsup" | "ssl";
  code: number | null;
}
// ── «Починить Discord» (zapret/discord.rs) ─────────────────────
export type DiscordStep = "clean" | "hosts" | "voice" | "sites" | "launch";

export interface DiscordClient {
  id: string;
  name: string;
  running: boolean;
}

export interface DiscordInfo {
  clients: DiscordClient[];
  bypassOn: boolean;
  strategy: string | null;
  service: boolean;
}

export interface DiscordStepResult {
  level: DiagLevel;
  value: string;
  detail: string | null;
  closed: string[];
  pick: boolean;
}
