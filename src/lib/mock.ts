// Заглушка Rust-ядра для запуска интерфейса в обычном браузере (vite):
// вёрстка, скриншоты состояний, которые без прав администратора не получить.
// Параметры URL: ?state=off|on|service|external  &admin=0  &path=none  &theme=dark|light

import type { Cell, DiagItem, Fakes, ListInfo, LogEntry, LogLevel, Settings, Status, StrategyInfo, TestKind, TestRow, TestRun } from "./types";

const params = new URLSearchParams(location.search);
const KEY = "zgui-mock-settings";

const STRATEGIES: StrategyInfo[] = [
  "general (ALT)", "general (ALT2)", "general (ALT3)", "general (ALT4)", "general (ALT5)",
  "general (ALT6)", "general (ALT7)", "general (ALT8)", "general (ALT9)", "general (ALT10)",
  "general (ALT11)", "general (ALT12)", "general (ALT13)", "general (EXP)",
  "general (FAKE TLS AUTO ALT)", "general (FAKE TLS AUTO ALT2)", "general (FAKE TLS AUTO ALT3)",
  "general (FAKE TLS AUTO)", "general (SIMPLE FAKE ALT)", "general (SIMPLE FAKE ALT2)",
  "general (SIMPLE FAKE)", "general",
].map((name) => ({ name, direct: true }));

let settings: Settings = {
  schema: 1,
  zapretPath: params.get("path") === "none" ? null : "C:\\Tools\\zapret-discord-youtube",
  lastStrategy: "general (ALT2)",
  theme: (params.get("theme") as Settings["theme"]) ?? "system",
  autostart: false,
  minimizeToTray: true,
  checkUpdates: true,
  logCollapsed: false,
  stopBypassOnExit: true,
  restartOnFilterChange: "ask",
  testSelection: ["general", "general (ALT)", "general (ALT2)", "general (FAKE TLS AUTO)"],
  notifications: true,
  trayHintShown: false,
  autopilot: (params.get("apmode") as Settings["autopilot"]) ?? "warn",
  onboardingDone: params.get("wizard") !== "1",
  startBypassOnLaunch: false,
  tgAutostart: false,
};
try {
  const raw = localStorage.getItem(KEY);
  if (raw && !params.has("theme")) settings = { ...settings, ...JSON.parse(raw) };
} catch {
  /* приватный режим */
}
if (params.get("wizard") === "1") settings.onboardingDone = false;

const mode = params.get("state") ?? "off";
const started = Date.now() - (42 * 60 + 17) * 1000;

const status: Status = {
  zapret: settings.zapretPath
    ? { path: settings.zapretPath, valid: true, error: null, warning: null, version: "1.10.3", strategies: STRATEGIES, empty: false }
    : { path: null, valid: false, error: null, warning: null, version: null, strategies: [], empty: false },
  bypass:
    mode === "off"
      ? { state: "off", source: null, strategy: null, pid: null, startedAt: null }
      : {
          state: "on",
          source: mode === "service" ? "service" : mode === "external" ? "external" : "gui",
          strategy: "general (ALT2)",
          pid: 4242,
          startedAt: started,
        },
  service:
    mode === "service"
      ? { state: "running", strategy: "general (ALT2)", outdatedArgs: false, stopPending: false, missingExe: false }
      : { state: "not_installed", strategy: null, outdatedArgs: false, stopPending: false, missingExe: false },
  filters: settings.zapretPath
    ? { ipset: "any", game: "off", tcpPorts: "1024-65535", udpPorts: "1024-65535", backupExists: true }
    : null,
  isAdmin: params.get("admin") !== "0",
  testsRunning: false,
  update:
    params.get("update") === "available"
      ? { state: "available", latest: "1.10.4", error: null, stage: null, progress: null }
      : { state: "current", latest: "1.10.3", error: null, stage: null, progress: null },
  autopilot: mockAutopilot(params.get("ap")),
  telegram: mockTelegram(params.get("tg")),
};

/** ?tg=none|off|on|external|update — состояние Telegram-прокси. */
function mockTelegram(tg: string | null): Status["telegram"] {
  const base = {
    installed: true, version: "1.11.2", latest: null as string | null, running: false, listening: false, port: 1443,
    startedAt: null as number | null, external: null as string | null, busy: null as string | null, progress: null as number | null,
  };
  switch (tg) {
    case "none":
      return { ...base, installed: false, version: null };
    case "on":
      return { ...base, running: true, listening: true, startedAt: Date.now() - 754_000 };
    case "external":
      return { ...base, external: "C:\Users\me\Downloads\TgWsProxy_windows.exe" };
    case "update":
      return { ...base, running: true, listening: true, startedAt: Date.now() - 3_600_000, latest: "1.12.0" };
    default:
      return base;
  }
}

let tgCfg = { port: 1443, secret: "3f9c2a71d04e8b56a1c7e9d2b4f60a85", dcIp: ["2:149.154.167.220", "4:149.154.167.220"], cfproxy: true };
const tgView = () => ({ ...tgCfg, link: `tg://proxy?server=127.0.0.1&port=${tgCfg.port}&secret=dd${tgCfg.secret}` });

/** ?ap=ok|fail|confirming|picking|switched|offline — подпись автопилота на Главной. */
function mockAutopilot(ap: string | null): Status["autopilot"] {
  const at = (min: number) => Date.now() - min * 60_000;
  const base = { phase: "idle" as const, lastCheck: at(6), outcome: "ok" as const, message: null, switched: null };
  switch (ap) {
    case "none":
      return { ...base, lastCheck: null, outcome: null };
    case "fail":
      return { ...base, outcome: "fail", message: "YouTube не открывается" };
    case "confirming":
      return { ...base, phase: "confirming", message: "YouTube не открывается" };
    case "picking":
      return { ...base, phase: "picking", outcome: "fail", message: "YouTube не открывается" };
    case "switched":
      return { ...base, lastCheck: at(2), switched: { from: "general (ALT5)", to: "general (ALT2)", at: at(2) } };
    case "offline":
      return { ...base, outcome: "offline" };
    default:
      return base;
  }
}

const FAKES: Fakes = {
  files: ["quic_initial_4pda_to", "quic_initial_www_google_com", "stun", "stun2", "tls_clienthello_www_google_com"],
  discord: "quic_initial_www_google_com",
  game: "stun",
};

async function simulateUpdate(to: string) {
  for (let p = 0; p <= 100; p += 20) {
    status.update = { state: "applying", latest: to, error: null, stage: "Скачивание", progress: p };
    changed();
    await wait(250);
  }
  for (const stage of ["Остановка обхода", "Замена файлов", "Восстановление"]) {
    status.update = { state: "applying", latest: to, error: null, stage, progress: null };
    changed();
    await wait(400);
  }
  status.zapret.version = to;
  status.update = { state: "current", latest: to, error: null, stage: null, progress: null };
  log("ok", `zapret обновлён до версии ${to}`);
  changed();
}

// ── Проверка и диагностика ─────────────────────────────────────
const COLS = ["Discord", "YouTube", "Google"];
const cell = (v: number): Cell =>
  v === 1 ? { state: "ok", hint: "4/4 целей" } : v === 0 ? { state: "fail", hint: "1/4 целей · gateway.discord.gg: TLS1.3 ERROR" } : { state: "none", hint: "" };
function doneRow(name: string, v: [number, number, number], total: string, score: number): TestRow {
  return {
    name,
    state: "done",
    cells: v.map(cell),
    total,
    score,
    pingOk: 14,
    details: [
      { target: "DiscordMain", text: "HTTP:OK TLS1.2:OK TLS1.3:OK | Ping: 24 ms", ok: true },
      { target: "DiscordGateway", text: "HTTP:OK TLS1.2:OK TLS1.3:ERROR | Ping: 31 ms", ok: v[0] === 1 },
      { target: "YouTubeWeb", text: "HTTP:OK TLS1.2:OK TLS1.3:OK | Ping: 18 ms", ok: true },
    ],
    error: null,
  };
}
let testRun: TestRun | null =
  params.get("tests") === "done"
    ? {
        kind: "standard",
        columns: COLS,
        rows: [
          doneRow("general", [1, 0, 1], "3/5", 30),
          doneRow("general (ALT)", [1, 1, 0], "3/5", 31),
          doneRow("general (ALT2)", [1, 1, 1], "5/5", 36),
          doneRow("general (FAKE TLS AUTO)", [0, 1, 1], "3/5", 29),
        ],
        running: false,
        cancelled: false,
        best: "general (ALT2)",
        startedAt: Date.now() - 90_000,
        finishedAt: Date.now() - 2_000,
        phase: null,
        report: "C:\\Tools\\zapret-discord-youtube\\utils\\test results\\test_results_2026-10-13_14-02-11.txt",
        error: null,
        quick: false,
        site: null,
      }
    : null;
let cancelFlag = false;

let discordPicked = false;

async function simulateTests(kind: TestKind, names: string[], quick = false, site: string | null = null) {
  cancelFlag = false;
  const columns = kind === "standard" ? COLS : ["HTTP", "TLS 1.2", "TLS 1.3"];
  testRun = {
    kind, columns, running: true, cancelled: false, best: null, startedAt: Date.now(), finishedAt: null,
    phase: "Подготовка…", report: null, error: null, quick, site,
    rows: names.map((name) => ({ name, state: "pending", cells: columns.map(() => cell(-1)), total: null, score: 0, pingOk: 0, details: [], error: null })),
  };
  status.testsRunning = true;
  changed();
  emit("tests://progress", testRun);
  for (let i = 0; i < names.length; i++) {
    if (cancelFlag) break;
    testRun.rows[i].state = "running";
    testRun.phase = `${i + 1}/${names.length} · ${names[i]}`;
    emit("tests://progress", testRun);
    await wait(900);
    const v = [0, 1, 2].map(() => (Math.random() > 0.35 ? 1 : 0)) as [number, number, number];
    const ok = v.filter(Boolean).length;
    testRun.rows[i] = doneRow(names[i], v, `${ok + 2}/5`, 24 + ok * 4);
    emit("tests://progress", testRun);
    if ((quick || site) && v[0] && v[1]) {
      testRun.best = names[i];
      break;
    }
  }
  for (const r of testRun.rows) if (r.state === "pending" || r.state === "running") r.state = "skipped";
  testRun.running = false;
  testRun.cancelled = cancelFlag;
  testRun.phase = null;
  testRun.finishedAt = Date.now();
  if (!quick && !site) {
    const best = [...testRun.rows].filter((r) => r.state === "done").sort((a, b) => b.score - a.score)[0];
    testRun.best = best?.name ?? null;
  }
  log(cancelFlag ? "warn" : "ok", cancelFlag ? "Проверка остановлена" : `Проверка завершена · лучшая стратегия: «${testRun.best}»`);
  status.testsRunning = false;
  changed();
  emit("tests://progress", testRun);
}

const LISTS: (ListInfo & { text: string; note: string | null })[] = [
  {
    id: "user", title: "Свои домены", file: "list-general-user.txt", kind: "domains", note: null,
    text: "# Свои домены — по одному на строку\n# Поддомены подхватываются автоматически\n\ndiscord.gg\ndiscordapp.net\ncdn.discordapp.com\ngooglevideo.com\nytimg.com\nrutracker.org\n# работа\nexample.com\nnot a domain\n",
  },
  { id: "general", title: "Основной список", file: "list-general.txt", kind: "domains", note: "Этот файл перезапишется при обновлении zapret — свои домены лучше добавлять в «Свои домены»", text: "discord.com\ndiscord.gg\nyoutube.com\n" },
  { id: "exclude", title: "Исключения", file: "list-exclude-user.txt", kind: "domains", note: null, text: "domain.example.abc\n" },
  { id: "ipset", title: "IPSet", file: "ipset-all.txt.backup", kind: "ips", note: "IPSet сейчас не в режиме «Загруженный» — редактируется сохранённый список, он вернётся при включении режима", text: "1.2.3.0/24\n5.6.7.8\n2001:db8::/32\n300.1.1.1\n" },
  { id: "ipset_exclude", title: "Исключения IP", file: "ipset-exclude-user.txt", kind: "ips", note: null, text: "203.0.113.113/32\n" },
];

// ?big=1 — IPSet на 30 000 строк, как настоящий ipset-all.txt (проверка скорости редактора).
if (params.get("big") === "1") {
  const ip = LISTS.find((l) => l.id === "ipset")!;
  ip.text = Array.from({ length: 30000 }, (_, i) => `${10 + (i >> 16)}.${(i >> 8) & 255}.${i & 255}.0/24`).join("\n") + "\n";
}

const DIAG: DiagItem[] = [
  { id: "bfe", title: "Base Filtering Engine", level: "ok", value: "ок", detail: null, fix: null, fixLabel: null, fixSafe: false },
  { id: "proxy", title: "Системный прокси", level: "ok", value: "нет", detail: null, fix: null, fixLabel: null, fixSafe: false },
  { id: "tcp_ts", title: "TCP timestamps", level: "warn", value: "выкл", detail: "Без TCP timestamps часть стратегий не работает", fix: "enable_timestamps", fixLabel: "Включить TCP timestamps", fixSafe: true },
  { id: "conflicts", title: "Adguard, Killer, VPN", level: "ok", value: "не найдены", detail: null, fix: null, fixLabel: null, fixSafe: false },
  { id: "windivert", title: "Конфликт WinDivert", level: "ok", value: "нет", detail: null, fix: null, fixLabel: null, fixSafe: false },
  { id: "dns", title: "Secure DNS", level: "err", value: "не настроен", detail: "Windows не использует зашифрованный DNS", fix: null, fixLabel: null, fixSafe: false },
  { id: "path", title: "Папка zapret", level: "ok", value: "ок", detail: null, fix: null, fixLabel: null, fixSafe: false },
  { id: "hosts", title: "Файл hosts", level: "ok", value: "ок", detail: null, fix: null, fixLabel: null, fixSafe: false },
  { id: "discord", title: "Кэш Discord", level: "info", value: "можно очистить", detail: "Помогает, если Discord неожиданно перестал работать", fix: "clear_discord_cache", fixLabel: "Закрыть Discord и очистить его кэш", fixSafe: false },
];

let nextId = 1;
const journal: LogEntry[] = [];
const listeners = new Map<string, Set<(p: unknown) => void>>();

function emit(event: string, payload: unknown) {
  listeners.get(event)?.forEach((cb) => cb(structuredClone(payload)));
}
function stamp(d = new Date()) {
  return d.toTimeString().slice(0, 8);
}
function log(level: LogLevel, text: string, at?: Date) {
  const e = { id: nextId++, time: stamp(at), level, text };
  journal.push(e);
  emit("log://line", e);
}
function changed() {
  emit("status://changed", status);
}
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

// Журнал как в макете.
{
  const t = (s: number) => new Date(Date.now() - s * 1000);
  log("info", "Найдено стратегий: 22", t(34));
  log("warn", "Служба zapret не установлена", t(33));
  if (mode !== "off") {
    log("info", "IPSet: any · Game filter: off", t(1));
    log("info", "winws.exe запущен (PID 4242)", t(0));
    log("ok", "Стратегия «general (ALT2)» запущена", t(0));
  }
}

function requireAdmin() {
  if (!status.isAdmin) throw "Нужны права администратора — перезапустите программу от имени администратора";
}

async function startBypass(name: string) {
  requireAdmin();
  status.bypass = { state: "starting", source: "gui", strategy: name, pid: null, startedAt: null };
  changed();
  await wait(900);
  log("info", `IPSet: ${status.filters!.ipset} · Game filter: ${status.filters!.game}`);
  log("info", "winws.exe запущен (PID 4242)");
  status.bypass = { state: "on", source: "gui", strategy: name, pid: 4242, startedAt: Date.now() };
  log("ok", `Стратегия «${name}» запущена`);
  changed();
}

export async function call<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  await wait(120);
  const r = await (async (): Promise<unknown> => {
    switch (cmd) {
      case "bootstrap":
        return {
          settings,
          isAdmin: status.isAdmin,
          systemTheme: matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark",
          appVersion: "2.0.0",
          startHidden: false,
        };
      case "settings_set":
        settings = { ...settings, ...(args.patch as Partial<Settings>) };
        try {
          localStorage.setItem(KEY, JSON.stringify(settings));
        } catch {
          /* не критично */
        }
        return settings;
      case "status_get":
        return status;
      case "journal_get":
        return journal;
      case "zapret_inspect":
      case "zapret_set_path": {
        const path = String(args.path);
        if (!path.toLowerCase().includes("zapret")) {
          if (cmd === "zapret_set_path") throw "В папке нет service.bat — это не сборка zapret";
          const empty = path.toLowerCase().includes("empty");
          return { path, valid: false, error: empty ? "Папка пуста — в ней нет zapret" : "В папке нет service.bat — это не сборка zapret", warning: null, version: null, strategies: [], empty };
        }
        const info = { path, valid: true, error: null, warning: null, version: "1.10.3", strategies: STRATEGIES, empty: false };
        if (cmd === "zapret_set_path") {
          settings.zapretPath = path;
          status.zapret = info;
          status.filters ??= { ipset: "any", game: "off", tcpPorts: "1024-65535", udpPorts: "1024-65535", backupExists: true };
          log("ok", `Папка zapret: ${path} · найдено стратегий: 22`);
          changed();
        }
        return info;
      }
      case "zapret_detect":
        return ["C:\\zapret"];
      case "bypass_start":
        await startBypass(String(args.strategy));
        return;
      case "bypass_stop":
        requireAdmin();
        status.bypass = { ...status.bypass, state: "stopping" };
        changed();
        await wait(500);
        status.bypass = { state: "off", source: null, strategy: null, pid: null, startedAt: null };
        log("info", "Обход остановлен");
        changed();
        return;
      case "service_install":
        requireAdmin();
        await wait(800);
        status.service = { state: "running", strategy: String(args.strategy), outdatedArgs: false, stopPending: false, missingExe: false };
        status.bypass = { state: "on", source: "service", strategy: String(args.strategy), pid: 5151, startedAt: Date.now() };
        log("ok", `Служба zapret установлена со стратегией «${args.strategy}»`);
        changed();
        return;
      case "service_remove":
        requireAdmin();
        status.service = { state: "not_installed", strategy: null, outdatedArgs: false, stopPending: false, missingExe: false };
        status.bypass = { state: "off", source: null, strategy: null, pid: null, startedAt: null };
        log("ok", "Службы zapret и WinDivert удалены");
        changed();
        return;
      case "service_start":
        requireAdmin();
        status.service.state = "running";
        status.bypass = { state: "on", source: "service", strategy: status.service.strategy, pid: 5151, startedAt: Date.now() };
        log("ok", "Служба zapret запущена");
        changed();
        return;
      case "service_stop":
        requireAdmin();
        status.service.state = "stopped";
        status.bypass = { state: "off", source: null, strategy: null, pid: null, startedAt: null };
        log("info", "Служба zapret остановлена");
        changed();
        return;
      case "ipset_set":
        status.filters!.ipset = args.mode as never;
        log("info", `IPSet → ${args.mode}`);
        changed();
        return;
      case "game_set":
        status.filters!.game = args.mode as never;
        if (status.service.state !== "not_installed") status.service.outdatedArgs = true;
        log("info", `Game filter → ${args.mode}`);
        changed();
        return;
      case "hosts_check":
        await wait(600);
        return { upToDate: false, add: 12, replace: 2, total: 31 };
      case "hosts_apply":
        requireAdmin();
        log("ok", "Файл hosts обновлён (резервная копия: hosts.zapretgui.bak)");
        return;
      case "tests_start":
        requireAdmin();
        if (status.service.state !== "not_installed" && !args.stopService) throw "service";
        void simulateTests(args.kind as TestKind, args.strategies as string[]);
        return;
      case "tests_quick":
        requireAdmin();
        if (status.service.state !== "not_installed" && !args.stopService) throw "service";
        void simulateTests("standard", ["general (ALT2)", "general (ALT5)", "general", "general (ALT)", "general (ALT3)"], true);
        return;
      case "notify_test":
      case "tray_hint":
        return;
      case "notifications_set":
        settings.notifications = Boolean(args.enabled);
        return settings;
      case "site_check": {
        await wait(900);
        const d = String(args.input).replace(/^https?:\/\//, "").split("/")[0].replace(/^www\./, "").toLowerCase();
        const it = (id: string, title: string, level: DiagItem["level"], value: string, detail: string | null = null) => ({ id, title, level, value, detail });
        if (d.includes("chatgpt"))
          return { domain: d, reachable: true, actions: [], verdictLevel: "warn",
            verdict: "Сайт отвечает, но не пускает (код 403). Скорее всего, он сам ограничивает доступ из вашей страны — zapret тут не поможет.",
            items: [it("lists", "Списки zapret", "warn", "нет", "Домена нет ни в одном списке"), it("dns", "DNS", "ok", "ок", "104.18.32.47, 172.64.155.209"),
              it("ipset", "IPSet", "info", "любой IP", "Режим «Любой» — фильтр по IP охватывает все адреса"), it("access", "Доступ с обходом", "ok", "открывается", "TLS 1.2: OK · TLS 1.3: OK"),
              it("answer", "Ответ сайта", "warn", "код 403", "Сайт отказывает в доступе — обычно это ограничение по стране")] };
        if (d.includes("discord"))
          return { domain: d, reachable: true, actions: [], verdictLevel: "ok", verdict: "Сайт открывается.",
            items: [it("lists", "Списки zapret", "ok", "есть", "list-general.txt: «discord.com»"), it("dns", "DNS", "ok", "ок", "162.159.138.232"),
              it("ipset", "IPSet", "info", "любой IP"), it("access", "Доступ с обходом", "ok", "открывается", "TLS 1.2: OK · TLS 1.3: OK"), it("answer", "Ответ сайта", "ok", "код 200")] };
        return { domain: d, reachable: false, actions: ["add_to_list", "compare"], verdictLevel: "err",
          verdict: "Сайта нет в списках zapret — обход его не трогает. Добавьте его в «Свои домены» и подберите стратегию.",
          items: [it("lists", "Списки zapret", "warn", "нет", "Домена нет ни в одном списке — стратегии его не обрабатывают"), it("dns", "DNS", "ok", "ок", "172.67.182.196, 104.21.32.39"),
            it("ipset", "IPSet", "info", "любой IP", "Режим «Любой» — фильтр по IP охватывает все адреса"), it("access", "Доступ с обходом", "err", "не открывается", "TLS 1.2: ERROR · TLS 1.3: ERROR")] };
      }
      case "site_add":
        await wait(300);
        log("ok", `${args.domain} добавлен в «Свои домены»`);
        return true;
      case "site_compare":
        await wait(1200);
        return { tls12: "error", tls13: "error", code: null };
      case "tests_site":
        requireAdmin();
        void simulateTests("custom", ["general (ALT2)", "general (ALT6)", "general", "general (ALT)"], false, String(args.domain));
        return;
      case "tests_cancel":
        cancelFlag = true;
        return;
      case "tests_last":
        return testRun;
      case "diag_run":
        await wait(700);
        return DIAG;
      case "lists_index":
        return LISTS.map(({ text: _t, note: _n, ...info }) => info);
      case "list_read": {
        const l = LISTS.find((x) => x.id === args.id)!;
        return { ...l, hash: String(l.text.length), crlf: true };
      }
      case "list_save": {
        const l = LISTS.find((x) => x.id === args.id)!;
        l.text = String(args.text).endsWith("\n") ? String(args.text) : `${args.text}\n`;
        log("ok", `Сохранён список ${l.file}`);
        return { ...l, hash: String(l.text.length), crlf: true };
      }
      case "update_check":
        await wait(600);
        return status.update;
      case "update_apply":
        requireAdmin();
        await simulateUpdate(status.update.latest ?? "1.10.4");
        return `zapret обновлён до версии ${status.zapret.version}`;
      case "zapret_download": {
        await simulateUpdate("1.10.3");
        const path = String(args.dest);
        settings.zapretPath = path;
        status.zapret = { path, valid: true, error: null, warning: null, version: "1.10.3", strategies: STRATEGIES, empty: false };
        status.filters = { ipset: "any", game: "off", tcpPorts: "1024-65535", udpPorts: "1024-65535", backupExists: true };
        changed();
        return `zapret 1.10.3 установлен в ${path}`;
      }
      case "autostart_set":
        requireAdmin();
        settings.autostart = Boolean(args.enabled);
        return settings;
      case "check_updates_set":
        settings.checkUpdates = Boolean(args.enabled);
        return settings;
      case "fakes_get":
        return FAKES;
      case "fakes_set":
        FAKES[args.kind as "discord" | "game"] = String(args.name);
        return;
      case "ipset_count":
        return 31412;
      case "ipset_update":
        await wait(800);
        return 31560;
      case "game_ports_set":
        status.filters!.tcpPorts = String(args.tcp);
        status.filters!.udpPorts = String(args.udp);
        changed();
        return;
      case "open_path":
      case "settings_export":
      case "app_quit":
        return;
      case "settings_import":
      case "settings_reset":
        return settings;
      case "discord_info":
        await wait(300);
        return {
          clients: params.get("discord") === "none" ? [] : [{ id: "stable", name: "Discord", running: params.get("discord") !== "closed" }],
          bypassOn: status.bypass.state === "on",
          strategy: status.bypass.strategy ?? settings.lastStrategy,
          service: status.service.state !== "not_installed",
        };
      case "discord_step": {
        await wait(args.step === "sites" ? 1400 : 800);
        const r = (level: string, value: string, detail: string | null = null, extra = {}) => ({ level, value, detail, closed: [], pick: false, ...extra });
        switch (args.step) {
          case "clean":
            return r("ok", "закрыт, кэш очищен", "Удалено папок кэша: 3. Настройки и вход в аккаунт не затронуты", { closed: ["stable"] });
          case "hosts":
            requireAdmin();
            return r("ok", "обновлён", "Добавлено записей: 12, заменено: 0. Резервная копия — hosts.zapretgui.bak");
          case "voice":
            requireAdmin();
            return r("ok", "в порядке", "Голосовые порты UDP 19294–19344, 50000–50100 обрабатываются");
          case "sites":
            if (params.get("dsites") === "fail" && !discordPicked) {
              discordPicked = true;
              return r("err", "не открываются: 2 из 4", "gateway.discord.gg, updates.discord.com", { pick: true });
            }
            return r("ok", "открываются", "discord.com, gateway.discord.gg, cdn.discordapp.com, updates.discord.com");
          case "launch":
            return r("ok", "запущен", "Discord");
        }
        return null;
      }
      case "tg_config":
        return tgView();
      case "tg_config_save":
        tgCfg = { ...(args.config as typeof tgCfg) };
        status.telegram = { ...status.telegram, port: tgCfg.port };
        changed();
        return tgView();
      case "tg_new_secret":
        return Array.from(crypto.getRandomValues(new Uint8Array(16)), (b) => b.toString(16).padStart(2, "0")).join("");
      case "tg_install":
        for (let p = 0; p <= 100; p += 25) {
          status.telegram = { ...status.telegram, busy: "Скачивание TgWsProxy", progress: p };
          changed();
          await wait(400);
        }
        status.telegram = { ...status.telegram, installed: true, version: "1.11.2", latest: null, busy: null, progress: null };
        changed();
        log("ok", "TgWsProxy 1.11.2 установлен");
        return "TgWsProxy 1.11.2 установлен";
      case "tg_check":
        return status.telegram.latest;
      case "tg_start":
      case "tg_close_external":
        status.telegram = { ...status.telegram, external: null, running: true, listening: false, startedAt: Date.now() };
        changed();
        await wait(1200);
        status.telegram = { ...status.telegram, listening: true };
        changed();
        log("ok", "Telegram-прокси запущен · 127.0.0.1:1443");
        return;
      case "tg_stop":
        status.telegram = { ...status.telegram, running: false, listening: false, startedAt: null };
        changed();
        log("info", "Telegram-прокси остановлен");
        return;
      case "tg_restart":
        await wait(600);
        return;
      case "tg_connect":
        return;
      case "autopilot_check":
        status.autopilot = { ...status.autopilot, phase: "checking" };
        changed();
        await wait(1500);
        status.autopilot = { ...status.autopilot, phase: "idle", lastCheck: Date.now(), outcome: "ok", message: null };
        changed();
        log("ok", "Автопилот: Discord и YouTube открываются");
        return;
      case "diag_fix": {
        requireAdmin();
        const fixes = args.fixes as string[];
        for (const d of DIAG) if (d.fix && fixes.includes(d.fix)) Object.assign(d, { level: "ok", value: "ок", fix: null, fixLabel: null });
        return fixes.map((fix) => ({ fix, ok: true, message: "готово" }));
      }
      default:
        throw `mock: неизвестная команда ${cmd}`;
    }
  })();
  return r as T;
}

export async function on<T>(event: string, cb: (payload: T) => void): Promise<() => void> {
  const set = listeners.get(event) ?? new Set();
  listeners.set(event, set);
  const fn = cb as (p: unknown) => void;
  set.add(fn);
  if (event === "theme://system") {
    const mq = matchMedia("(prefers-color-scheme: light)");
    const h = () => cb((mq.matches ? "light" : "dark") as T);
    mq.addEventListener("change", h);
    return () => {
      mq.removeEventListener("change", h);
      set.delete(fn);
    };
  }
  return () => set.delete(fn);
}

export async function readClipboard(): Promise<string> {
  return "https://www.rutracker.org/forum\nnnmclub.to, discord.gg\nexample.com";
}

export async function pickFolder(): Promise<string | null> {
  // ?pick=... — что «выберет» диалог «Обзор…» (например, папка без zapret).
  return params.get("pick") ?? "C:\\zapret";
}
