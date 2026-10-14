<script lang="ts">
  // Карточка «Журнал» из макета: шапка-кнопка 44 px, моноширинный 12/22,
  // колонки время 72 · уровень 44 · текст. Новые записи — сверху, как в макете.
  import { app, patchSettings } from "../lib/state.svelte";
  import { plural } from "../lib/ui.svelte";

  const collapsed = $derived(app.settings?.logCollapsed ?? false);
  const warns = $derived(app.journal.filter((e) => e.level === "warn").length);
  const errs = $derived(app.journal.filter((e) => e.level === "err").length);
  const rows = $derived([...app.journal].reverse());
</script>

<section class="card" class:collapsed aria-label="Журнал">
  <button
    type="button"
    class="head"
    aria-expanded={!collapsed}
    onclick={() => patchSettings({ logCollapsed: !collapsed })}
  >
    <span>Журнал</span>
    {#if warns}<span class="count warn">{warns} {plural(warns, ["предупреждение", "предупреждения", "предупреждений"])}</span>{/if}
    {#if errs}<span class="count err">{errs} {plural(errs, ["ошибка", "ошибки", "ошибок"])}</span>{/if}
    <span class="grow"></span>
    <span class="hint">{collapsed ? "Развернуть" : "Свернуть"}</span>
  </button>
  {#if !collapsed}
    <div class="body">
      {#each rows as e (e.id)}
        <div class="row">
          <span class="time">{e.time}</span>
          <span class="lvl {e.level}">{e.level}</span>
          <span class="text">{e.text}</span>
        </div>
      {:else}
        <div class="empty">Пока пусто</div>
      {/each}
    </div>
  {/if}
</section>

<style>
  .card {
    flex-grow: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    border-radius: 16px;
    background: var(--card);
    border: 1px solid var(--line);
    overflow: hidden;
  }
  .card.collapsed {
    flex-grow: 0;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 44px;
    flex-shrink: 0;
    padding: 0 20px;
    border: none;
    background: transparent;
    color: var(--tx);
    font-size: 14px;
    font-weight: 600;
    text-align: left;
  }
  /* Кнопка вплотную к краям карточки с overflow: hidden — обводку фокуса рисуем внутрь. */
  .head:focus-visible {
    outline-offset: -2px;
    border-radius: 16px 16px 0 0;
  }
  .count {
    font-size: 12px;
    font-weight: 400;
  }
  .count.warn {
    color: var(--warn);
  }
  .count.err {
    color: var(--err);
  }
  .grow {
    flex-grow: 1;
  }
  .hint {
    font-size: 12px;
    font-weight: 400;
    color: var(--faint);
  }
  .body {
    flex-grow: 1;
    min-height: 0;
    overflow: auto;
    padding: 8px 20px 14px;
    border-top: 1px solid var(--line);
    font-family: var(--font-mono);
    font-size: 12px;
    line-height: 22px;
    user-select: text;
    cursor: text;
  }
  .row {
    display: grid;
    grid-template-columns: 72px 44px 1fr;
  }
  .time {
    color: var(--mute);
  }
  .lvl.ok {
    color: var(--accent);
  }
  .lvl.info {
    color: var(--faint);
  }
  .lvl.warn {
    color: var(--warn);
  }
  .lvl.err {
    color: var(--err);
  }
  .text {
    color: var(--tx2);
    overflow-wrap: anywhere;
  }
  .empty {
    color: var(--faint);
  }
</style>
