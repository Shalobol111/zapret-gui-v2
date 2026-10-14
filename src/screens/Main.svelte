<script lang="ts">
  import Button from "../components/Button.svelte";
  import LogCard from "../components/LogCard.svelte";
  import PageHeader from "../components/PageHeader.svelte";
  import PowerButton from "../components/PowerButton.svelte";
  import Segmented from "../components/Segmented.svelte";
  import Select from "../components/Select.svelte";
  import * as api from "../lib/api";
  import { app, go, patchSettings } from "../lib/state.svelte";
  import { confirmDialog, duration, toast } from "../lib/ui.svelte";
  import type { GameMode, IpsetMode } from "../lib/types";

  const st = $derived(app.status);
  const zapret = $derived(st?.zapret);
  const ready = $derived(!!zapret?.valid);
  const bypass = $derived(st?.bypass);
  const svc = $derived(st?.service);
  const svcInstalled = $derived(!!svc && svc.state !== "not_installed");
  const admin = $derived(app.isAdmin);
  const testing = $derived(!!st?.testsRunning);

  // ── Выбранная стратегия ──────────────────────────────────────
  // Если обход работает (или стоит служба) — показываем то, что реально запущено.
  let chosen = $state<string | null>(null);
  const names = $derived(zapret?.strategies.map((s) => s.name) ?? []);
  const selected = $derived.by(() => {
    const running =
      (svcInstalled ? svc?.strategy : null) ?? (bypass?.state !== "off" ? bypass?.strategy : null);
    const pick = running ?? chosen ?? app.settings?.lastStrategy ?? null;
    return pick && names.includes(pick) ? pick : (running ?? names[0] ?? null);
  });
  const options = $derived(
    (zapret?.strategies ?? []).map((s) => ({ value: s.name, label: s.name, note: s.direct ? undefined : "через .bat" })),
  );

  // ── Действия ─────────────────────────────────────────────────
  let busy = $state<null | "power" | "install" | "hosts" | "filter">(null);

  /** Одно действие за раз; ошибка ядра — в уведомление. true — если успешно. */
  async function run(kind: NonNullable<typeof busy>, fn: () => Promise<unknown>): Promise<boolean> {
    if (busy) return false;
    busy = kind;
    try {
      await fn();
      return true;
    } catch (e) {
      toast(api.errorText(e), "err");
      return false;
    } finally {
      busy = null;
    }
  }

  function togglePower() {
    if (!selected) return;
    // Служба указывает на удалённый winws.exe — запуск бесполезен, предлагаем переустановку.
    if (svcInstalled && svc?.missingExe && svc.state !== "running") {
      install();
      return;
    }
    run("power", async () => {
      if (svcInstalled) {
        if (svc!.state === "running") await api.serviceStop();
        else await api.serviceStart();
      } else if (bypass?.state === "on") {
        await api.bypassStop();
      } else {
        await api.bypassStart(selected);
      }
    });
  }

  async function onStrategy(name: string) {
    if (svcInstalled) {
      const ok = await confirmDialog({
        title: "Переустановить службу?",
        text: `Служба установлена со стратегией «${svc!.strategy}».\nПереустановить её со стратегией «${name}»?`,
        ok: "Переустановить",
      });
      if (ok) run("install", () => api.serviceInstall(name));
      return;
    }
    chosen = name;
    patchSettings({ lastStrategy: name });
    if (bypass?.state === "on") run("power", () => api.bypassStart(name));
  }

  async function install() {
    if (!selected) return;
    const reinstall = svcInstalled;
    const ok = await confirmDialog({
      title: reinstall ? "Переустановить службу?" : "Установить службу?",
      text:
        `Служба zapret со стратегией «${selected}» будет запускаться вместе с Windows.` +
        (bypass?.source === "gui" ? "\nРучной запуск будет остановлен." : ""),
      ok: reinstall ? "Переустановить" : "Установить",
    });
    if (ok) run("install", () => api.serviceInstall(selected));
  }

  function updateHosts() {
    run("hosts", async () => {
      const plan = await api.hostsCheck();
      if (plan.upToDate) {
        toast("Файл hosts актуален", "ok");
        return;
      }
      const parts = [plan.add && `добавлено ${plan.add}`, plan.replace && `заменено ${plan.replace}`].filter(Boolean);
      const ok = await confirmDialog({
        title: "Обновить hosts?",
        text:
          `Записи Flowseal для веб-Telegram и голосовых каналов Discord: будет ${parts.join(", ")}.\n` +
          "Старая версия сохранится в hosts.zapretgui.bak.",
        ok: "Обновить",
      });
      if (ok) {
        await api.hostsApply();
        toast("Файл hosts обновлён", "ok");
      }
    });
  }

  /** После смены фильтра работающий обход нужно перезапустить, а службу — переустановить. */
  function afterFilterChange() {
    if (!bypass || bypass.state !== "on") return;
    if (bypass.source === "service") {
      const name = svc?.strategy;
      if (name) toast("Служба работает со старыми параметрами фильтров", "warn", {
        label: "Переустановить",
        run: () => run("install", () => api.serviceInstall(name)),
      });
      return;
    }
    const name = bypass.strategy ?? selected;
    if (!name) return;
    const restart = () => run("power", () => api.bypassStart(name));
    const policy = app.settings?.restartOnFilterChange ?? "ask";
    if (policy === "always") restart();
    else if (policy === "ask") toast("Фильтр изменён — изменения вступят в силу после перезапуска", "info", { label: "Перезапустить", run: restart });
  }

  const setIpset = async (m: IpsetMode) => (await run("filter", () => api.ipsetSet(m))) && afterFilterChange();
  const setGame = async (m: GameMode) => (await run("filter", () => api.gameSet(m))) && afterFilterChange();

  // ── Тексты состояния ─────────────────────────────────────────
  let now = $state(Date.now());
  $effect(() => {
    if (bypass?.state !== "on" || !bypass.startedAt) return;
    now = Date.now();
    const t = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(t);
  });

  const on = $derived(bypass?.state === "on");
  const transitional = $derived(bypass?.state === "starting" || bypass?.state === "stopping" || svc?.state === "pending");
  const title = $derived(
    bypass?.state === "starting" ? "Запуск…" : bypass?.state === "stopping" ? "Остановка…" : on ? "Обход включён" : "Обход выключен",
  );
  const subtitle = $derived.by(() => {
    if (testing) return "Идёт проверка стратегий — управление обходом временно недоступно";
    if (on && bypass) {
      const via = bypass.source === "service" ? "служба · " : bypass.source === "external" ? "запущен вне программы · " : "";
      const up = bypass.startedAt ? `работает ${duration(now - bypass.startedAt)}` : "работает";
      return `${bypass.strategy ?? "неизвестная стратегия"} · ${via}${up}`;
    }
    if (transitional) return selected ?? "";
    if (!zapret?.path) return "Укажите папку zapret в Настройках";
    if (!ready) return zapret.error ?? "Папка zapret не подходит";
    if (!admin) return "Нужны права администратора, чтобы запустить обход";
    if (svc?.state === "stopped" && svc.missingExe) return "Служба не запустится — нажмите, чтобы переустановить её с выбранной стратегией";
    if (svc?.state === "stopped") return `Служба остановлена · нажмите, чтобы запустить «${svc.strategy ?? selected}»`;
    return "Нажмите кнопку, чтобы запустить выбранную стратегию";
  });
  const headerStatus = $derived.by(() => {
    if (!zapret?.valid) return zapret?.path ? "zapret · папка не подходит" : "zapret · папка не выбрана";
    const base = `zapret ${zapret.version ?? ""}`.trim();
    const u = st?.update;
    if (u?.state === "current") return `${base} · обновлений нет`;
    if (u?.state === "checking") return `${base} · проверка обновлений…`;
    if (u?.state === "applying") return `${base} · обновление…`;
    return base;
  });
  // ── Автопилот: подпись под состоянием ────────────────────────
  const ap = $derived(st?.autopilot);
  const hhmm = (t: number) => new Date(t).toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });
  const short = (name: string) => name.replace(/^general\s*\((.+)\)$/i, "$1");
  const apText = $derived.by(() => {
    const mode = app.settings?.autopilot ?? "warn";
    if (!ap || mode === "off" || (ap.phase === "idle" && !on)) return null;
    if (ap.phase === "checking") return "Автопилот · проверяю Discord и YouTube…";
    if (ap.phase === "confirming") return `Автопилот · ${ap.message ?? "сбой"} — перепроверяю через минуту`;
    if (ap.phase === "picking") return "Автопилот · подбираю рабочую стратегию…";
    if (ap.outcome === "fail") return `Автопилот · ${ap.message ?? "найден сбой"}`;
    if (ap.outcome === "offline") return "Автопилот · нет интернета, проверю позже";
    const sw = ap.switched && Date.now() - ap.switched.at < 6 * 3600_000 ? ap.switched : null;
    if (sw) {
      const later = ap.lastCheck && ap.lastCheck - sw.at > 60_000 ? ` · проверено в ${hhmm(ap.lastCheck)}` : "";
      return `Автопилот · в ${hhmm(sw.at)} заменил «${short(sw.from)}» на «${short(sw.to)}»${later}`;
    }
    if (ap.lastCheck) return `Автопилот · проверено в ${hhmm(ap.lastCheck)}`;
    return "Автопилот · первая проверка через минуту";
  });
  const apFail = $derived(ap?.phase === "idle" && ap.outcome === "fail");

  /** «Подобрать» из подписи автопилота — быстрый подбор в «Проверке». */
  async function pickNow() {
    go("check");
    try {
      await api.testsQuick(false);
    } catch (e) {
      if (api.errorText(e) !== "service") return toast(api.errorText(e), "err");
      const ok = await confirmDialog({
        title: "Остановить службу на время подбора?",
        text: "Установлена служба zapret — во время перебора стратегий она мешает. Служба будет остановлена и снова запущена после подбора.",
        ok: "Остановить и подобрать",
      });
      if (ok) await api.testsQuick(true).catch((e2) => toast(api.errorText(e2), "err"));
    }
  }

  const updateAvailable = $derived(st?.update.state === "available" ? st.update.latest : null);
  // ── Служба ───────────────────────────────────────────────────
  const svcLabel = $derived(
    ({ not_installed: "Не установлена", running: "Работает", stopped: "Остановлена", pending: "Ожидание…" } as const)[
      svc?.state ?? "not_installed"
    ],
  );
  const svcTone = $derived(
    svc?.state === "running" ? "accent" : svc?.state === "stopped" ? "dim" : "warn",
  );
  const svcText = $derived.by(() => {
    if (!svcInstalled) return "Без службы обход выключится после перезагрузки.";
    if (svc?.stopPending) return "Служба зависла при остановке — возможен конфликт с другим обходом.";
    if (svc?.missingExe)
      return `Служба ссылается на winws.exe, которого больше нет (папку zapret переместили или удалили) — переустановите её.`;
    if (svc?.outdatedArgs) return "Служба установлена из другой папки или со старыми параметрами — переустановите её.";
    return `Установлена со стратегией «${svc?.strategy ?? "?"}». Обход запускается вместе с Windows.`;
  });
  const installLabel = $derived(
    !svcInstalled ? "Установить с этой стратегией" : svc?.outdatedArgs || svc?.missingExe ? "Переустановить с этой стратегией" : null,
  );
  const needAdmin = "Нужны права администратора";
</script>

<PageHeader title="Главная">
  <span class="head-status"
    >{headerStatus}{#if updateAvailable}&nbsp;·&nbsp;<button type="button" class="link upd" onclick={() => go("settings")}
        >доступна {updateAvailable}</button
      >{/if}</span
  >
</PageHeader>

<section class="card state" aria-label="Состояние обхода">
  <PowerButton
    {on}
    busy={busy === "power" || busy === "install" || transitional}
    disabled={!ready || !admin || !selected || testing}
    title={!admin ? needAdmin : undefined}
    onclick={togglePower}
  />
  <div class="state-body">
    <div class="titles">
      <div class="title-row">
        <span class="dot" class:on></span>
        <span class="title">{title}</span>
      </div>
      <span class="sub">
        {subtitle}
        {#if !zapret?.path || (zapret && !zapret.valid)}
          · <button type="button" class="link" onclick={() => go("settings")}>Открыть Настройки</button>
        {/if}
      </span>
      {#if apText}
        <span class="auto" class:warn={apFail}
          >{apText}{#if apFail && admin && !testing}{" · "}<button type="button" class="link" onclick={pickNow}
              >Подобрать</button
            >{/if}</span
        >
      {/if}
    </div>
    <div class="pick">
      <div class="field">
        <label for="strategy">Стратегия</label>
        <Select
          id="strategy"
          {options}
          value={selected}
          placeholder="Нет стратегий"
          disabled={!ready || busy !== null || transitional || testing}
          onchange={onStrategy}
        />
      </div>
      <button type="button" class="link best" onclick={() => go("check")}>Подобрать лучшую →</button>
    </div>
  </div>
</section>

<div class="row2">
  <section class="card small" aria-labelledby="flt">
    <h2 id="flt">Фильтры</h2>
    <div class="line">
      <span class="lbl">IPSet</span>
      <Segmented
        label="IPSet"
        value={st?.filters?.ipset ?? "off"}
        disabled={!st?.filters || busy === "filter" || testing}
        options={[
          { value: "off", label: "Выкл" },
          { value: "any", label: "Любой" },
          { value: "loaded", label: "Загруженный" },
        ]}
        onchange={setIpset}
      />
    </div>
    <div class="line">
      <span class="lbl">Игры</span>
      <Segmented
        label="Game filter"
        value={st?.filters?.game ?? "off"}
        disabled={!st?.filters || busy === "filter" || testing}
        options={[
          { value: "off", label: "Выкл" },
          { value: "tcp", label: "TCP" },
          { value: "udp", label: "UDP" },
          { value: "tcp-udp", label: "TCP+UDP" },
        ]}
        onchange={setGame}
      />
    </div>
  </section>

  <section class="card small svc" aria-labelledby="svc">
    <div class="svc-head">
      <h2 id="svc">Служба Windows</h2>
      <span class="pill {svcTone}"><span class="pdot"></span>{svcLabel}</span>
    </div>
    <p class:warn={svc?.outdatedArgs || svc?.stopPending || svc?.missingExe}>{svcText}</p>
    <div class="buttons">
      {#if installLabel}
        <Button
          variant="primary"
          disabled={!ready || !admin || !selected || busy !== null || testing}
          title={!admin ? needAdmin : undefined}
          onclick={install}>{busy === "install" ? "Установка…" : installLabel}</Button
        >
      {/if}
      <Button disabled={!admin || busy !== null || testing} title={!admin ? needAdmin : undefined} onclick={updateHosts}
        >{busy === "hosts" ? "Проверка…" : "Обновить hosts"}</Button
      >
    </div>
  </section>
</div>

<LogCard />

<style>
  .head-status {
    font-size: 12px;
    color: var(--faint);
  }
  .link.upd {
    font-size: 12px;
    color: var(--warn);
  }
  .card {
    border-radius: 16px;
    background: var(--card);
    border: 1px solid var(--line);
    flex-shrink: 0;
  }
  .state {
    display: flex;
    align-items: center;
    gap: 28px;
    padding: 24px 28px;
  }
  .state-body {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .titles {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .title-row {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 4px;
    background: var(--mute);
    flex-shrink: 0;
  }
  .dot.on {
    background: var(--accent);
  }
  .title {
    font-size: 22px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }
  .sub {
    font-size: 13px;
    color: var(--dim);
    font-variant-numeric: tabular-nums;
  }
  .auto {
    margin-top: 2px;
    font-size: 12px;
    color: var(--faint);
    font-variant-numeric: tabular-nums;
  }
  .auto.warn {
    color: var(--warn);
  }
  .auto .link {
    font-size: 12px;
  }
  .pick {
    display: flex;
    align-items: flex-end;
    gap: 10px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    flex-grow: 1;
    max-width: 380px;
    min-width: 0;
  }
  label {
    font-size: 12px;
    color: var(--dim);
  }
  .link {
    border: none;
    background: none;
    padding: 0;
    color: var(--accent);
    font-size: 13px;
  }
  .best {
    height: 40px;
    padding: 0 4px;
    white-space: nowrap;
  }
  .row2 {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 16px;
    flex-shrink: 0;
  }
  .small {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 18px 20px;
  }
  .svc {
    gap: 12px;
  }
  h2 {
    font-size: 14px;
    font-weight: 600;
  }
  .line {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .lbl {
    font-size: 13px;
    color: var(--dim);
  }
  .svc-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .pill {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
  }
  .pdot {
    width: 6px;
    height: 6px;
    border-radius: 3px;
    background: currentColor;
  }
  .pill.accent {
    color: var(--accent);
  }
  .pill.dim {
    color: var(--dim);
  }
  .pill.warn {
    color: var(--warn);
  }
  p {
    margin: 0;
    font-size: 13px;
    line-height: 1.45;
    color: var(--dim);
  }
  p.warn {
    color: var(--warn);
  }
  .buttons {
    display: flex;
    gap: 8px;
    margin-top: auto;
    flex-wrap: wrap;
  }
</style>
