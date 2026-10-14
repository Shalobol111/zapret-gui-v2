<script lang="ts">
  // Карточка «Диагностика» из макета (260 px): строки 36 px — точка, название, результат.
  import Button from "./Button.svelte";
  import DiscordFix from "./DiscordFix.svelte";
  import Modal from "./Modal.svelte";
  import Spinner from "./Spinner.svelte";
  import * as api from "../lib/api";
  import { app } from "../lib/state.svelte";
  import { toast } from "../lib/ui.svelte";
  import type { DiagItem, FixId } from "../lib/types";

  let { visible }: { visible: boolean } = $props();

  let items = $state<DiagItem[]>([]);
  let running = $state(false);
  let ran = false;

  async function runDiag() {
    running = true;
    try {
      items = await api.diagRun();
    } catch (e) {
      toast(api.errorText(e), "err");
    } finally {
      running = false;
    }
  }

  // Первое открытие экрана за сессию — диагностика запускается сама.
  $effect(() => {
    if (visible && !ran) {
      ran = true;
      runDiag();
    }
  });

  const problems = $derived(items.filter((i) => i.level === "warn" || i.level === "err"));
  const fixable = $derived(items.filter((i) => i.fix));
  const hint = $derived(
    !items.length
      ? "Проверка распространённых причин, по которым zapret может не работать."
      : problems.length
        ? `${problems.map((p) => p.title).join(", ")} — частая причина, почему стратегии не работают.`
        : "Проблем не найдено.",
  );

  // ── Исправления ──────────────────────────────────────────────
  let dialog = $state(false);
  let chosen = $state<Record<string, boolean>>({});
  let fixing = $state(false);
  let discord = $state(false);

  function openFixes() {
    chosen = Object.fromEntries(fixable.map((i) => [i.fix!, i.fixSafe || i.level === "err" || i.level === "warn"]));
    dialog = true;
  }

  async function applyFixes() {
    const list = Object.entries(chosen).filter(([, v]) => v).map(([k]) => k as FixId);
    dialog = false;
    if (!list.length) return;
    fixing = true;
    try {
      const res = await api.diagFix(list);
      const failed = res.filter((r) => !r.ok);
      if (failed.length) toast(failed.map((f) => f.message).join("; "), "err");
      else toast(res.length === 1 ? res[0].message : `Готово: ${res.length} исправления`, "ok");
    } catch (e) {
      toast(api.errorText(e), "err");
    } finally {
      fixing = false;
      await runDiag();
    }
  }
</script>

<section class="card" aria-labelledby="dg">
  <div class="head">
    <h2 id="dg">Диагностика</h2>
    <Button height={30} size={12} disabled={running || fixing} onclick={runDiag}>{running ? "Проверка…" : "Запустить"}</Button>
  </div>
  <ul>
    {#each items as it (it.id)}
      <li title={it.detail ?? undefined}>
        {#if running}<Spinner size={8} />{:else}<span class="dot {it.level}"></span>{/if}
        <span class="title">{it.title}</span>
        <span class="value {it.level}">{it.value}</span>
      </li>
    {:else}
      {#if running}<li class="wait"><Spinner /> Проверка…</li>{/if}
    {/each}
  </ul>
  <div class="foot">
    <p>{hint}</p>
    <button
      type="button"
      class="fix"
      disabled={running || fixing || !!app.status?.testsRunning}
      onclick={() => (discord = true)}>Починить Discord</button
    >
    <button
      type="button"
      class="fix"
      disabled={!fixable.length || !app.isAdmin || running || fixing || !!app.status?.testsRunning}
      title={app.isAdmin ? undefined : "Нужны права администратора"}
      onclick={openFixes}>{fixing ? "Исправление…" : "Исправить автоматически"}</button
    >
  </div>
</section>

{#if discord}
  <DiscordFix
    onclose={() => {
      discord = false;
      runDiag();
    }}
  />
{/if}

{#if dialog}
  <Modal title="Исправить автоматически" width={440} onclose={() => (dialog = false)}>
    <p class="lead">Выберите, что сделать. Удаление служб и очистка кэша Discord не отмечены заранее.</p>
    {#each fixable as it (it.fix)}
      <label class="opt">
        <input type="checkbox" bind:checked={chosen[it.fix!]} />
        <span>
          <span class="opt-title">{it.fixLabel}</span>
          {#if it.detail}<span class="opt-hint">{it.detail}</span>{/if}
        </span>
      </label>
    {/each}
    {#snippet buttons()}
      <Button onclick={() => (dialog = false)}>Отмена</Button>
      <Button variant="primary" disabled={!Object.values(chosen).some(Boolean)} onclick={applyFixes}>Исправить</Button>
    {/snippet}
  </Modal>
{/if}

<style>
  .card {
    width: 260px;
    flex-shrink: 0;
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
    justify-content: space-between;
    padding: 14px 18px;
    border-bottom: 1px solid var(--line);
  }
  h2 {
    font-size: 14px;
    font-weight: 600;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 8px 18px;
    display: flex;
    flex-direction: column;
    flex-grow: 1;
    overflow: auto;
  }
  li {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 36px;
    font-size: 13px;
    cursor: default;
  }
  li.wait {
    color: var(--faint);
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 3px;
    flex-shrink: 0;
    background: var(--accent);
  }
  .dot.info {
    background: var(--mute);
  }
  .dot.warn {
    background: var(--warn);
  }
  .dot.err {
    background: var(--err);
  }
  .title {
    flex-grow: 1;
    min-width: 0;
  }
  .value {
    font-size: 12px;
    color: var(--faint);
    text-align: right;
  }
  .value.warn {
    color: var(--warn);
  }
  .value.err {
    color: var(--err);
  }
  .foot {
    padding: 14px 18px;
    border-top: 1px solid var(--line);
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  p {
    margin: 0;
    font-size: 12px;
    line-height: 1.5;
    color: var(--dim);
  }
  .fix {
    height: 34px;
    border-radius: 9px;
    border: 1px solid var(--line2);
    background: var(--raised);
    color: var(--tx);
    font-size: 13px;
  }
  .fix:disabled {
    cursor: default;
    opacity: 0.5;
  }
  .lead {
    margin-bottom: 12px;
    font-size: 13px;
  }
  .opt {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 8px 0;
    cursor: pointer;
  }
  .opt input {
    margin-top: 2px;
  }
  .opt-title {
    display: block;
    color: var(--tx);
  }
  .opt-hint {
    display: block;
    font-size: 12px;
    color: var(--faint);
  }
</style>
