<script lang="ts">
  // Боковое меню из макета: 208 px, padding 20/12, фон side, граница line справа.
  // При ширине окна < 900 px сворачивается до 64 px — только иконки (PLAN.md §13).
  import Icon from "./Icon.svelte";
  import type { IconName } from "../lib/icons";
  import type { Screen } from "../lib/types";
  import { app, bypassOn, effectiveMode, go, toggleTheme } from "../lib/state.svelte";

  let { compact = false }: { compact?: boolean } = $props();

  const items: { id: Screen; label: string; icon: IconName }[] = [
    { id: "main", label: "Главная", icon: "power" },
    { id: "check", label: "Проверка", icon: "activity" },
    { id: "lists", label: "Списки", icon: "list" },
    { id: "telegram", label: "Telegram", icon: "send" },
    { id: "settings", label: "Настройки", icon: "settings" },
  ];

  const light = $derived(effectiveMode() === "light");
  const themeLabel = $derived(light ? "Тёмная тема" : "Светлая тема");
  const adminText = $derived(app.isAdmin ? "Администратор" : "Без прав администратора");
</script>

<nav aria-label="Разделы" class:compact>
  <div class="brand">
    <div class="logo"><Icon name="logo" size={16} stroke={2} /></div>
    <div class="brand-text fade">
      <span class="name">zapret</span>
      <span class="ver"
        >GUI {app.appVersion.split(".").slice(0, 2).join(".") || "2"}{#if app.status?.appUpdate.state === "available"}<button
            type="button"
            class="upd"
            title={`Доступна версия ${app.status.appUpdate.latest}`}
            onclick={() => (app.updatePrompt = true)}>· обновить</button
          >{/if}</span
      >
    </div>
  </div>

  {#each items as it, i (it.id)}
    {@const active = app.screen === it.id}
    <button
      type="button"
      class="item"
      class:active
      aria-current={active ? "page" : undefined}
      title={compact ? `${it.label} (Ctrl+${i + 1})` : undefined}
      onclick={() => go(it.id)}
    >
      <Icon name={it.icon} />
      <span class="label fade">{it.label}</span>
      {#if it.id === "main" && bypassOn()}
        <span class="spacer"></span>
        <span class="run-dot" aria-label="обход включён"></span>
      {:else if it.id === "telegram" && app.status?.telegram.running}
        <span class="spacer"></span>
        <span class="run-dot" aria-label="прокси работает"></span>
      {:else if it.id === "lists" && app.listsDirty}
        <span class="spacer"></span>
        <span class="run-dot warn" aria-label="есть несохранённые изменения"></span>
      {/if}
    </button>
  {/each}

  <div class="grow"></div>

  <div class="bottom">
    <div class="admin" class:warn={!app.isAdmin} title={compact ? adminText : undefined}>
      <span class="shield"><Icon name={app.isAdmin ? "shield" : "shieldAlert"} size={14} stroke={2} /></span>
      <span class="admin-text fade">{adminText}</span>
    </div>
    <button type="button" class="theme" aria-label={themeLabel} title={themeLabel} onclick={toggleTheme}>
      <Icon name={light ? "moon" : "sun"} size={17} />
    </button>
  </div>
</nav>

<style>
  .upd {
    margin-left: 4px;
    padding: 0;
    border: none;
    background: none;
    font: inherit;
    color: var(--warn);
  }
  nav {
    width: 208px;
    flex-shrink: 0;
    padding: 20px 12px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    background: var(--side);
    border-right: 1px solid var(--line);
    overflow: hidden;
    transition:
      width 0.2s var(--theme-ease),
      background-color var(--theme-dur) var(--theme-ease),
      border-color var(--theme-dur) var(--theme-ease);
  }
  nav.compact {
    width: 64px;
  }

  .fade {
    white-space: nowrap;
    transition:
      opacity 0.15s var(--theme-ease),
      color var(--theme-dur) var(--theme-ease);
  }
  .compact .fade {
    opacity: 0;
    pointer-events: none;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 10px 22px;
    transition: padding 0.2s var(--theme-ease);
  }
  .compact .brand {
    padding: 4px 6px 22px;
  }
  .logo {
    width: 28px;
    height: 28px;
    flex-shrink: 0;
    border-radius: 8px;
    background: var(--raised);
    border: 1px solid var(--line2);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--accent);
  }
  .brand-text {
    display: flex;
    flex-direction: column;
  }
  .name {
    font-weight: 600;
    font-size: 15px;
    letter-spacing: -0.01em;
  }
  .ver {
    font-size: 11px;
    color: var(--faint);
  }

  .item {
    position: relative;
    display: flex;
    align-items: center;
    gap: 12px;
    height: 40px;
    padding: 0 12px;
    border: none;
    border-radius: 10px;
    background: transparent;
    color: var(--dim);
    font-size: 14px;
    text-align: left;
    overflow: hidden;
    transition:
      padding 0.2s var(--theme-ease),
      background-color var(--theme-dur) var(--theme-ease),
      color var(--theme-dur) var(--theme-ease);
  }
  .compact .item {
    padding: 0 11px;
    overflow: visible;
  }
  .item.active {
    background: var(--raised);
    color: var(--tx);
    font-weight: 500;
  }
  .spacer {
    flex-grow: 1;
  }
  .run-dot {
    width: 6px;
    height: 6px;
    border-radius: 3px;
    background: var(--accent);
    flex-shrink: 0;
  }
  .run-dot.warn {
    background: var(--warn);
  }
  /* В свёрнутом меню точка уходит в правый верхний угол иконки. */
  .compact .run-dot {
    position: absolute;
    left: 27px;
    top: 9px;
  }

  .grow {
    flex-grow: 1;
  }

  .bottom {
    display: flex;
    gap: 8px;
  }
  .compact .bottom {
    flex-direction: column;
  }
  .admin {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 40px;
    padding: 0 12px;
    border-radius: 10px;
    border: 1px solid var(--line);
    font-size: 12px;
    line-height: 1.2;
    color: var(--dim);
  }
  .shield {
    display: flex;
    color: var(--accent);
  }
  .admin.warn .shield {
    color: var(--warn);
  }
  .admin.warn .admin-text {
    white-space: normal;
  }
  .compact .admin {
    flex-grow: 0;
    width: 40px;
    padding: 0;
    justify-content: center;
  }
  .compact .admin-text {
    display: none;
  }

  .theme {
    width: 40px;
    height: 40px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 10px;
    border: 1px solid var(--line);
    background: transparent;
    color: var(--dim);
  }
</style>
