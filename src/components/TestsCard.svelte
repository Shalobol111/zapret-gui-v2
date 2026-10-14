<script lang="ts">
  // Карточка «Стратегии» из макета: таблица 28 | 1fr | 64 | 64 | 64 | 64, строки 44 px.
  import Button from "./Button.svelte";
  import Icon from "./Icon.svelte";
  import ResultCell from "./ResultCell.svelte";
  import Spinner from "./Spinner.svelte";
  import * as api from "../lib/api";
  import { app, go, patchSettings } from "../lib/state.svelte";
  import { confirmDialog, toast } from "../lib/ui.svelte";
  import type { TestKind, TestRow } from "../lib/types";

  let { kind }: { kind: TestKind } = $props();

  const names = $derived(app.status?.zapret.strategies.map((s) => s.name) ?? []);
  const selection = $derived(new Set(app.settings?.testSelection ?? names));
  const picked = $derived(names.filter((n) => selection.has(n)));
  const run = $derived(app.tests);
  const running = $derived(!!run?.running);
  // Результаты показываем только для выбранного типа теста.
  const shown = $derived(run && run.kind === kind ? run : null);
  const columns = $derived(shown?.columns ?? (kind === "standard" ? ["Discord", "YouTube", "Google"] : ["HTTP", "TLS 1.2", "TLS 1.3"]));
  const rowOf = (name: string): TestRow | undefined => shown?.rows.find((r) => r.name === name);
  /** «general (ALT2)» → «ALT2»: у всех стратегий Flowseal общий префикс, а колонка узкая. */
  const short = (name: string) => name.replace(/^general\s*\((.+)\)$/i, "$1");

  let open = $state<string | null>(null);
  let starting = $state(false);

  function setSelection(next: string[]) {
    patchSettings({ testSelection: next.length === names.length ? null : next });
  }
  function toggle(name: string) {
    setSelection(selection.has(name) ? picked.filter((n) => n !== name) : names.filter((n) => selection.has(n) || n === name));
  }
  function toggleAll() {
    setSelection(picked.length === names.length ? [] : names);
  }

  /** Запуск с вопросом про службу: при установленной службе ядро отвечает "service". */
  async function withService(fn: (stopService: boolean) => Promise<void>) {
    starting = true;
    try {
      await fn(false);
    } catch (e) {
      if (api.errorText(e) === "service") {
        const ok = await confirmDialog({
          title: "Остановить службу на время проверки?",
          text: "Установлена служба zapret — во время проверки она мешает запускать стратегии. Служба будет остановлена и снова запущена после проверки.",
          ok: "Остановить и проверить",
        });
        if (ok) await fn(true).catch((e2) => toast(api.errorText(e2), "err"));
      } else {
        toast(api.errorText(e), "err");
      }
    } finally {
      starting = false;
    }
  }

  const start = () => withService((s) => api.testsStart(kind, picked, s));
  const quick = () => withService((s) => api.testsQuick(s));

  /** Включить найденную стратегию: служба — переустановить, иначе запустить вручную. */
  async function apply(name: string) {
    try {
      if (app.status?.service.state && app.status.service.state !== "not_installed") await api.serviceInstall(name);
      else await api.bypassStart(name);
      go("main");
    } catch (e) {
      toast(api.errorText(e), "err");
    }
  }
  // Конец прогона — предложить лучшую стратегию.
  let wasRunning = $state(false);
  $effect(() => {
    const r = app.tests;
    if (wasRunning && r && !r.running && !app.pickOwner) {
      if (r.error) toast(r.error, "err");
      else if (r.best && !r.cancelled && r.quick) {
        const best = r.best;
        toast(`Рабочая стратегия: ${best}`, "ok", { label: "Включить", run: () => apply(best) });
      } else if (r.best && !r.cancelled)
        toast(`Лучшая стратегия: ${r.best}`, "ok", {
          label: "Выбрать",
          run: () => {
            patchSettings({ lastStrategy: r.best });
            go("main");
          },
        });
      else if (!r.cancelled && r.quick) toast("Быстрый подбор не нашёл рабочую стратегию — запустите полную проверку", "warn");
      else if (!r.cancelled) toast("Ни одна стратегия не прошла проверку", "warn");
    }
    wasRunning = !!r?.running;
  });

  const needAdmin = "Нужны права администратора";
</script>

<section class="card" aria-labelledby="st">
  <div class="head">
    <h2 id="st">Стратегии</h2>
    <span class="meta">
      {#if running && run?.phase}<Spinner size={11} /> {run.phase}{:else}выбрано {picked.length} из {names.length}{/if}
    </span>
    <span class="grow"></span>
    {#if running}
      <Button variant="primary" height={34} onclick={() => api.testsCancel()}>Остановить</Button>
    {:else}
      <Button
        variant="primary"
        height={34}
        disabled={!picked.length || !app.isAdmin || starting || !app.status?.zapret.valid}
        title={app.isAdmin ? undefined : needAdmin}
        onclick={start}
      >
        <Icon name="play" size={12} />{starting ? "Запуск…" : "Проверить"}
      </Button>
    {/if}
  </div>

  <div class="grid colhead">
    <input
      type="checkbox"
      aria-label="Выбрать все"
      checked={picked.length === names.length && names.length > 0}
      indeterminate={picked.length > 0 && picked.length < names.length}
      disabled={running || !names.length}
      onchange={toggleAll}
    />
    <span>Профиль</span>
    {#each columns as c (c)}<span class="c">{c}</span>{/each}
    <span class="c">Итог</span>
  </div>

  <div class="rows">
    {#each names as name (name)}
      {@const r = rowOf(name)}
      {@const best = shown?.best === name && !running}
      {@const busy = r?.state === "running"}
      <div class="grid row" class:hl={best || busy}>
        <input type="checkbox" aria-label={name} checked={selection.has(name)} disabled={running} onchange={() => toggle(name)} />
        <span class="name-cell">
          <button
            type="button"
            class="name"
            aria-expanded={r?.details.length ? open === name : undefined}
            onclick={() => (r?.details.length ? (open = open === name ? null : name) : !running && toggle(name))}
            title={name}>{short(name)}</button
          >
          {#if best}<span class="badge" title="Лучшая стратегия по результатам проверки">лучшая</span>{/if}
        </span>
        {#each columns as _, i (i)}<ResultCell cell={r?.cells[i]} {busy} />{/each}
        {#if r?.state === "failed"}
          <span class="total err" title={r.error ?? undefined}>ошибка</span>
        {:else}
          <span class="total">{r?.total ?? "—"}</span>
        {/if}
      </div>
      {#if open === name && r?.details.length}
        <div class="details">
          {#each r.details as d (d.target)}
            <div class="dline"><span class="ddot" class:ok={d.ok}></span><span class="dt">{d.target}</span><span>{d.text}</span></div>
          {/each}
        </div>
      {/if}
    {:else}
      <div class="empty">Стратегии появятся, когда будет выбрана папка zapret</div>
    {/each}
  </div>

  {#if kind === "standard"}
    <div class="foot">
      <span class="foot-text" title="Проверяет Discord и YouTube, начиная со стратегий, которые работали раньше, и останавливается на первой рабочей">
        {#if shown?.quick && !running}Быстрый подбор: {shown.best ? `работает «${short(shown.best)}»` : "рабочая не найдена"}
        {:else}Быстрый подбор: Discord и YouTube{/if}
      </span>
      <Button
        height={30}
        size={12}
        disabled={running || starting || !app.isAdmin || !app.status?.zapret.valid}
        title={app.isAdmin ? undefined : needAdmin}
        onclick={quick}>Подобрать</Button
      >
    </div>
  {/if}
</section>

<style>
  .card {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    border-radius: 16px;
    background: var(--card);
    border: 1px solid var(--line);
    overflow: hidden;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 14px 18px;
    border-bottom: 1px solid var(--line);
    min-height: 63px;
  }
  h2 {
    font-size: 14px;
    font-weight: 600;
  }
  .meta {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--faint);
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .grow {
    flex-grow: 1;
  }
  .grid {
    display: grid;
    grid-template-columns: 28px minmax(0, 1fr) 56px 56px 56px 56px;
    align-items: center;
    column-gap: 8px;
  }
  .colhead {
    padding: 10px 18px 6px;
    font-size: 11px;
    color: var(--faint);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .c {
    text-align: center;
  }
  .rows {
    flex-grow: 1;
    overflow: auto;
    /* Место под полосу прокрутки резервируется всегда — колонки не съезжают относительно заголовка. */
    scrollbar-gutter: stable;
    padding: 0 0 10px 10px;
    container-type: inline-size;
  }
  /* Узкая таблица (окно 780–900 px): бейдж «лучшая» превращается в точку, чтобы не вытеснять имя. */
  @container (max-width: 400px) {
    .rows .badge {
      width: 8px;
      height: 8px;
      padding: 0;
      border-radius: 4px;
      background: var(--accent);
      font-size: 0;
    }
  }
  .row {
    height: 44px;
    padding: 0 8px;
    border-radius: 10px;
    font-size: 13px;
  }
  .row.hl {
    background: var(--row);
  }
  .name-cell {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .name {
    border: none;
    background: none;
    padding: 0;
    color: var(--tx);
    font-size: 13px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    text-align: left;
  }
  .badge {
    font-size: 11px;
    padding: 1px 6px;
    border-radius: 6px;
    background: var(--accent-soft);
    color: var(--accent);
    flex-shrink: 0;
  }
  .total {
    text-align: center;
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--tx2);
  }
  .total.err {
    color: var(--err);
    font-family: var(--font-ui);
  }
  .details {
    margin: 2px 8px 8px 44px;
    padding: 8px 12px;
    border-radius: 8px;
    background: var(--editor);
    font-family: var(--font-mono);
    font-size: 12px;
    line-height: 20px;
    color: var(--tx2);
    user-select: text;
  }
  .dline {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .ddot {
    width: 6px;
    height: 6px;
    border-radius: 3px;
    background: var(--err);
    flex-shrink: 0;
  }
  .ddot.ok {
    background: var(--accent);
  }
  .dt {
    min-width: 170px;
    color: var(--dim);
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 14px 10px 18px;
    border-top: 1px solid var(--line);
  }
  .foot-text {
    flex-grow: 1;
    min-width: 0;
    font-size: 12px;
    color: var(--faint);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .empty {
    padding: 24px 8px;
    color: var(--faint);
    font-size: 13px;
    text-align: center;
  }
</style>
