<script lang="ts">
  // Мастер первого запуска: zapret → подбор → включение → автозапуск. Полноэкранная карточка
  // поверх окна, одна главная кнопка на шаг. «Пропустить» в любой момент — всё это есть в разделах.
  import Button from "./Button.svelte";
  import Select from "./Select.svelte";
  import Spinner from "./Spinner.svelte";
  import * as api from "../lib/api";
  import { app, go, patchSettings } from "../lib/state.svelte";
  import { confirmDialog, toast } from "../lib/ui.svelte";
  import type { TestRun } from "../lib/types";

  const STEPS = ["zapret", "Подбор", "Включение", "Автозапуск"];
  const DEFAULT_DEST = "C:\\zapret";
  const fail = (e: unknown) => toast(api.errorText(e), "err");
  const short = (name: string) => name.replace(/^general\s*\((.+)\)$/i, "$1");

  let step = $state(0);
  const st = $derived(app.status);
  const zapret = $derived(st?.zapret);
  const admin = $derived(app.isAdmin);
  const svcInstalled = $derived(!!st?.service && st.service.state !== "not_installed");

  function finish(message?: string) {
    app.pickOwner = null;
    patchSettings({ onboardingDone: true });
    go("main");
    if (message) toast(message, "ok");
  }

  // ── 1. zapret ────────────────────────────────────────────────
  let downloading = $state(false);
  let zapretError = $state<string | null>(null);

  async function download(dest = DEFAULT_DEST) {
    downloading = true;
    zapretError = null;
    try {
      await api.zapretDownload(dest);
      step = 1;
    } catch (e) {
      zapretError = api.errorText(e);
    } finally {
      downloading = false;
    }
  }

  async function pickFolder() {
    const p = await api.pickFolder();
    if (!p) return;
    zapretError = null;
    try {
      const info = await api.zapretInspect(p);
      if (info.valid) {
        await api.zapretSetPath(p);
        return;
      }
      const dest = info.empty ? p : `${p.replace(/[\\/]+$/, "")}\\zapret`;
      const ok = await confirmDialog({
        title: "В папке нет zapret",
        text: `Скачать последнюю версию zapret-discord-youtube в ${dest}?`,
        ok: "Скачать",
      });
      if (ok) await download(dest);
    } catch (e) {
      zapretError = api.errorText(e);
    }
  }

  const progress = $derived(st?.update.state === "applying" ? st.update : null);

  // ── 2. Подбор ────────────────────────────────────────────────
  let picking = $state(false);
  let picked = $state<string | null>(null);
  let pickDone = $state<TestRun | null>(null);

  const pickNow = $derived.by(() => {
    const r = app.tests;
    if (!picking || !r?.running) return null;
    const i = r.rows.findIndex((x) => x.state === "running");
    return { i: Math.max(i, 0), total: r.rows.length, name: i >= 0 ? short(r.rows[i].name) : null };
  });

  function waitPick(since: number): Promise<TestRun> {
    return new Promise((resolve) => {
      const t = setInterval(() => {
        const r = app.tests;
        if (r && r.quick && !r.running && r.startedAt >= since) {
          clearInterval(t);
          resolve(r);
        }
      }, 300);
    });
  }

  async function pick() {
    const since = Date.now() - 1000;
    picking = true;
    pickDone = null;
    app.pickOwner = "wizard";
    try {
      await api.testsQuick(svcInstalled);
      const run = await waitPick(since);
      pickDone = run;
      if (run.best && !run.cancelled) {
        picked = run.best;
        strategy = run.best;
        step = 2;
      }
    } catch (e) {
      fail(e);
    } finally {
      picking = false;
    }
  }

  // ── 3. Включение ─────────────────────────────────────────────
  const names = $derived(zapret?.strategies.map((s) => s.name) ?? []);
  let strategy = $state<string | null>(null);
  const chosen = $derived(
    strategy && names.includes(strategy)
      ? strategy
      : (app.settings?.lastStrategy && names.includes(app.settings.lastStrategy) ? app.settings.lastStrategy : null) ??
          (names.includes("general") ? "general" : (names[0] ?? null)),
  );
  let enabling = $state(false);
  let checkSince = $state<number | null>(null);
  const bypassOn = $derived(st?.bypass.state === "on");
  const runningName = $derived(bypassOn ? st?.bypass.strategy : null);

  async function enable() {
    if (!chosen) return;
    enabling = true;
    try {
      if (svcInstalled) await api.serviceInstall(chosen);
      else await api.bypassStart(chosen);
      // Короткая проверка: та же, что у автопилота.
      checkSince = Date.now() - 1000;
      await api.autopilotCheck();
    } catch (e) {
      fail(e);
    } finally {
      enabling = false;
    }
  }

  const check = $derived.by(() => {
    const a = st?.autopilot;
    if (checkSince === null || !a) return null;
    if (a.phase !== "idle" || (a.lastCheck ?? 0) < checkSince) return { state: "run" as const, text: "Проверяю Discord и YouTube…" };
    if (a.outcome === "ok") return { state: "ok" as const, text: "Discord и YouTube открываются" };
    if (a.outcome === "offline") return { state: "warn" as const, text: "Нет подключения к интернету — проверьте сеть" };
    return { state: "warn" as const, text: `${a.message ?? "Есть сбой"} — попробуйте полную проверку в разделе «Проверка»` };
  });

  // Только в браузере (заглушка): ?wizstep=2&picked=… — для скриншотов вёрстки.
  if (!api.inTauri) {
    const q = new URLSearchParams(location.search);
    step = Number(q.get("wizstep")) || 0;
    picked = strategy = q.get("picked");
  }

  // ── 4. Автозапуск ────────────────────────────────────────────
  type Start = "service" | "app" | "none";
  let start = $state<Start>("service");
  let saving = $state(false);

  async function done() {
    const name = runningName ?? chosen;
    saving = true;
    try {
      if (start === "service" && name && !(svcInstalled && st?.service.strategy === name)) await api.serviceInstall(name);
      if (start === "app") {
        app.settings = await api.autostartSet(true);
        await patchSettings({ startBypassOnLaunch: true });
      }
      finish(
        start === "service"
          ? "Готово: служба zapret будет запускаться вместе с Windows"
          : start === "app"
            ? "Готово: ZapretGUI будет запускаться вместе с Windows и включать обход"
            : "Готово",
      );
    } catch (e) {
      fail(e);
    } finally {
      saving = false;
    }
  }

  const starts: { id: Start; title: string; hint: string }[] = [
    {
      id: "service",
      title: "Установить службу (рекомендуется)",
      hint: "Обход работает сразу после включения компьютера, даже если программа закрыта",
    },
    {
      id: "app",
      title: "Запускать программу",
      hint: "ZapretGUI стартует в трее и включает обход последней стратегией",
    },
    { id: "none", title: "Не нужно", hint: "Включайте обход кнопкой на Главной, когда он нужен" },
  ];
</script>

<div class="overlay">
  <div class="card" role="dialog" aria-modal="true" aria-labelledby="wz-title">
    <div class="top">
      <ol class="dots" aria-label="Шаги">
        {#each STEPS as s, i (s)}
          <li class:done={i < step} class:cur={i === step} aria-current={i === step ? "step" : undefined}>
            <span class="dot"></span><span class="lbl">{s}</span>
          </li>
        {/each}
      </ol>
      <button type="button" class="link skip" disabled={picking || enabling || saving || downloading} onclick={() => finish()}
        >Пропустить</button
      >
    </div>

    {#if step === 0}
      <h1 id="wz-title">Добро пожаловать в ZapretGUI</h1>
      {#if downloading}
        <p>Скачиваю последнюю версию zapret-discord-youtube с GitHub…</p>
        <div class="bar" aria-hidden="true"><span style:width="{progress?.progress ?? 8}%"></span></div>
        <p class="mute">{progress?.stage ?? "Подготовка"}{progress?.progress != null ? ` · ${progress.progress}%` : ""}</p>
      {:else if zapret?.valid}
        <p>Программа будет управлять этой копией zapret: запускать стратегии, ставить службу и обновлять её.</p>
        <div class="found">
          <span class="ok-dot"></span>
          <div>
            <div class="found-title">Найден zapret {zapret.version ?? ""}</div>
            <div class="mono">{zapret.path}</div>
          </div>
        </div>
      {:else}
        <p>
          zapret на компьютере не найден. Программа скачает последнюю версию zapret-discord-youtube (Flowseal) с GitHub —
          в папку без кириллицы и пробелов, как советует автор.
        </p>
      {/if}
      {#if zapretError}<p class="err">{zapretError}</p>{/if}
      <div class="buttons">
        {#if zapret?.valid}
          <Button disabled={downloading} onclick={pickFolder}>Другая папка…</Button>
          <Button variant="primary" onclick={() => (step = 1)}>Дальше</Button>
        {:else}
          <Button disabled={downloading} onclick={pickFolder}>Указать папку…</Button>
          <Button variant="primary" disabled={downloading} onclick={() => download()}
            >{downloading ? "Скачивание…" : `Скачать в ${DEFAULT_DEST}`}</Button
          >
        {/if}
      </div>
    {:else if !admin}
      <h1 id="wz-title">Нужны права администратора</h1>
      <p>
        Подбор стратегии, включение обхода и служба работают только с правами администратора — драйвер WinDivert
        перехватывает трафик всей системы. Запустите ZapretGUI от имени администратора — всё это есть на Главной и
        в разделе «Проверка».
      </p>
      <div class="buttons">
        <Button variant="primary" onclick={() => finish()}>Закрыть мастер</Button>
      </div>
    {:else if step === 1}
      <h1 id="wz-title">Подбор стратегии</h1>
      <p>
        Программа по очереди проверит стратегии и найдёт ту, с которой открываются Discord и YouTube. Обычно это 10–60
        секунд; на это время интернет может прерываться.
      </p>
      {#if svcInstalled}<p class="mute">Служба zapret на время подбора будет остановлена.</p>{/if}
      {#if picking}
        <div class="bar" aria-hidden="true">
          <span style:width="{pickNow ? ((pickNow.i + 0.5) / pickNow.total) * 100 : 4}%"></span>
        </div>
        <p class="mute row">
          <Spinner size={10} />
          {pickNow?.name ? `Проверяю «${pickNow.name}» · ${pickNow.i + 1} из ${pickNow.total}` : "Подготовка…"}
        </p>
      {:else if pickDone && !pickDone.best}
        <p class="warn">
          {pickDone.cancelled
            ? "Подбор остановлен."
            : pickDone.error
              ? pickDone.error
              : "Быстрый подбор не нашёл рабочую стратегию. Можно включить стратегию по умолчанию, а позже запустить полную проверку в разделе «Проверка»."}
        </p>
      {/if}
      <div class="buttons">
        {#if picking}
          <Button onclick={() => api.testsCancel()}>Остановить</Button>
        {:else}
          <Button onclick={() => (step = 2)}>{pickDone ? "Дальше" : "Пропустить"}</Button>
          <Button variant="primary" onclick={pick}>{pickDone ? "Повторить" : "Подобрать"}</Button>
        {/if}
      </div>
    {:else if step === 2}
      <h1 id="wz-title">{picked ? "Стратегия найдена" : "Включение обхода"}</h1>
      <p>
        {#if bypassOn}Обход работает. Осталось решить, запускать ли его вместе с Windows.
        {:else if picked}С «{short(picked)}» открываются Discord и YouTube. Включите обход — и можно пользоваться.
        {:else}Выберите стратегию и включите обход. Подобрать лучшую можно позже в разделе «Проверка».{/if}
      </p>
      <div class="field">
        <label for="wz-strategy">Стратегия</label>
        <Select
          id="wz-strategy"
          options={names.map((n) => ({ value: n, label: n }))}
          value={runningName ?? chosen}
          disabled={enabling || bypassOn}
          onchange={(v) => (strategy = v)}
        />
      </div>
      {#if bypassOn}
        <p class="state ok"><span class="ok-dot"></span>Обход включён · {runningName}</p>
      {/if}
      {#if check}
        <p class="state {check.state}">
          {#if check.state === "run"}<Spinner size={10} />{:else}<span class="ok-dot {check.state}"></span>{/if}{check.text}
        </p>
      {/if}
      <div class="buttons">
        {#if bypassOn}
          <Button variant="primary" disabled={check?.state === "run"} onclick={() => (step = 3)}>Дальше</Button>
        {:else}
          <Button onclick={() => (step = 3)}>Позже</Button>
          <Button variant="primary" disabled={!chosen || enabling} onclick={enable}>{enabling ? "Включение…" : "Включить"}</Button>
        {/if}
      </div>
    {:else}
      <h1 id="wz-title">Запускать вместе с Windows?</h1>
      <p>Без этого обход выключится после перезагрузки компьютера.</p>
      <div class="choices" role="radiogroup" aria-label="Запуск вместе с Windows">
        {#each starts as s (s.id)}
          <label class="choice" class:on={start === s.id}>
            <input type="radio" name="wz-start" value={s.id} bind:group={start} />
            <span>
              <span class="c-title">{s.title}</span>
              <span class="c-hint">{s.hint}</span>
            </span>
          </label>
        {/each}
      </div>
      <div class="buttons">
        <Button variant="primary" disabled={saving} onclick={done}>{saving ? "Сохранение…" : "Готово"}</Button>
      </div>
    {/if}
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 30;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    background: var(--bg);
    overflow: auto;
  }
  .card {
    width: 560px;
    max-width: 100%;
    margin: auto;
    padding: 28px 32px 26px;
    border-radius: 20px;
    background: var(--card);
    border: 1px solid var(--line);
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 8px;
  }
  .dots {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    gap: 16px;
  }
  .dots li {
    display: flex;
    align-items: center;
    gap: 7px;
    font-size: 12px;
    color: var(--faint);
  }
  .dots .dot {
    width: 8px;
    height: 8px;
    border-radius: 4px;
    background: var(--line2);
  }
  .dots li.done .dot {
    background: var(--accent);
    opacity: 0.55;
  }
  .dots li.cur {
    color: var(--tx);
  }
  .dots li.cur .dot {
    background: var(--accent);
  }
  .skip {
    border: none;
    background: none;
    padding: 0;
    font-size: 12px;
    color: var(--faint);
  }
  .skip:hover:not(:disabled) {
    color: var(--tx);
  }
  .skip:disabled {
    opacity: 0.4;
    cursor: default;
  }
  h1 {
    margin: 0;
    font-size: 20px;
    font-weight: 600;
  }
  p {
    margin: 0;
    font-size: 13.5px;
    line-height: 1.55;
    color: var(--dim);
  }
  p.mute {
    font-size: 12.5px;
    color: var(--faint);
  }
  p.row,
  p.state {
    display: flex;
    align-items: center;
    gap: 9px;
  }
  p.state {
    font-size: 13px;
    color: var(--tx);
  }
  p.err {
    color: var(--err);
    font-size: 12.5px;
  }
  p.warn,
  p.state.warn {
    color: var(--warn);
    font-size: 12.5px;
  }
  .found {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 14px 16px;
    border-radius: 12px;
    background: var(--raised);
    border: 1px solid var(--line);
  }
  .found .ok-dot {
    margin-top: 6px;
  }
  .found-title {
    font-size: 13.5px;
    font-weight: 600;
    margin-bottom: 3px;
  }
  .mono {
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--dim);
    word-break: break-all;
  }
  .ok-dot {
    width: 7px;
    height: 7px;
    border-radius: 4px;
    flex-shrink: 0;
    background: var(--accent);
  }
  .ok-dot.warn {
    background: var(--warn);
  }
  .bar {
    height: 6px;
    border-radius: 3px;
    background: var(--raised);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    border-radius: 3px;
    background: var(--accent);
    transition: width 0.4s ease;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .field label {
    font-size: 12px;
    color: var(--faint);
  }
  .choices {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .choice {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 12px 14px;
    border-radius: 12px;
    border: 1px solid var(--line);
    cursor: pointer;
  }
  .choice.on {
    border-color: var(--accent);
    background: var(--raised);
  }
  .choice input {
    margin-top: 3px;
    accent-color: var(--accent);
  }
  .c-title {
    display: block;
    font-size: 13.5px;
    color: var(--tx);
  }
  .c-hint {
    display: block;
    margin-top: 2px;
    font-size: 12px;
    color: var(--faint);
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    margin-top: 10px;
  }
  @media (max-width: 640px) {
    .dots .lbl {
      display: none;
    }
  }
</style>
