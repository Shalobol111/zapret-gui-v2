<script lang="ts">
  // Выпадающий список из макета: высота 40, radius 10, фон raised, рамка line2,
  // chevron 16 справа. Свой, а не <select>: нативный список WebView2 не берёт тему
  // и не меняет цвет плавно.
  import Icon from "./Icon.svelte";

  type Option = { value: string; label: string; note?: string };
  let {
    id,
    options,
    value,
    disabled = false,
    placeholder = "",
    onchange,
  }: {
    id?: string;
    options: Option[];
    value: string | null;
    disabled?: boolean;
    placeholder?: string;
    onchange: (v: string) => void;
  } = $props();

  let open = $state(false);
  let active = $state(0);
  let root: HTMLDivElement;
  let list = $state<HTMLUListElement>();

  const current = $derived(options.find((o) => o.value === value));
  const listId = `sel-${Math.random().toString(36).slice(2, 8)}`;

  function toggle() {
    if (disabled) return;
    open = !open;
    if (open) {
      active = Math.max(0, options.findIndex((o) => o.value === value));
      requestAnimationFrame(() => list?.children[active]?.scrollIntoView({ block: "nearest" }));
    }
  }

  function pick(i: number) {
    open = false;
    const o = options[i];
    if (o && o.value !== value) onchange(o.value);
  }

  function move(delta: number) {
    active = Math.min(options.length - 1, Math.max(0, active + delta));
    list?.children[active]?.scrollIntoView({ block: "nearest" });
  }

  function onKey(e: KeyboardEvent) {
    if (disabled) return;
    if (!open && ["ArrowDown", "ArrowUp", "Enter", " "].includes(e.key)) {
      e.preventDefault();
      toggle();
      return;
    }
    if (!open) return;
    switch (e.key) {
      case "ArrowDown": e.preventDefault(); move(1); break;
      case "ArrowUp": e.preventDefault(); move(-1); break;
      case "Home": e.preventDefault(); move(-options.length); break;
      case "End": e.preventDefault(); move(options.length); break;
      case "Enter":
      case " ": e.preventDefault(); pick(active); break;
      case "Escape": case "Tab": open = false; break;
    }
  }

  function onDocDown(e: MouseEvent) {
    if (open && !root.contains(e.target as Node)) open = false;
  }
</script>

<svelte:document onmousedown={onDocDown} />

<div class="wrap" bind:this={root}>
  <button
    {id}
    type="button"
    class="trigger"
    role="combobox"
    aria-expanded={open}
    aria-haspopup="listbox"
    aria-controls={listId}
    {disabled}
    onclick={toggle}
    onkeydown={onKey}
  >
    <span class="value" class:placeholder={!current}>{current?.label ?? placeholder}</span>
    {#if current?.note}<span class="note">{current.note}</span>{/if}
    <span class="chev"><Icon name="chevronDown" size={16} stroke={2} /></span>
  </button>
  {#if open}
    <ul class="list" id={listId} role="listbox" bind:this={list}>
      {#each options as o, i (o.value)}
        <!-- Клавиатура обрабатывается на combobox (стрелки, Enter, Esc) — стандартный шаблон listbox. -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <li
          role="option"
          aria-selected={o.value === value}
          class:active={i === active}
          class:selected={o.value === value}
          onmouseenter={() => (active = i)}
          onmousedown={(e) => e.preventDefault()}
          onclick={() => pick(i)}
        >
          <span class="label">{o.label}</span>
          {#if o.note}<span class="note">{o.note}</span>{/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .wrap {
    position: relative;
  }
  .trigger {
    width: 100%;
    height: 40px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 40px 0 14px;
    border-radius: 10px;
    background: var(--raised);
    border: 1px solid var(--line2);
    color: var(--tx);
    font-size: 14px;
    text-align: left;
  }
  .trigger:disabled {
    cursor: default;
    color: var(--dim);
  }
  .value {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .placeholder {
    color: var(--faint);
  }
  .chev {
    position: absolute;
    right: 14px;
    top: 12px;
    color: var(--dim);
    pointer-events: none;
  }
  .list {
    position: absolute;
    z-index: 30;
    top: 44px;
    left: 0;
    right: 0;
    max-height: 280px;
    overflow: auto;
    margin: 0;
    padding: 4px;
    list-style: none;
    border-radius: 10px;
    background: var(--card);
    border: 1px solid var(--line2);
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.28);
  }
  li {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 34px;
    padding: 0 10px;
    border-radius: 7px;
    font-size: 14px;
    color: var(--tx2);
    cursor: pointer;
  }
  li.active {
    background: var(--raised);
    color: var(--tx);
  }
  li.selected {
    color: var(--tx);
    font-weight: 500;
  }
  .label {
    flex-grow: 1;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .note {
    flex-shrink: 0;
    font-size: 11px;
    font-weight: 400;
    color: var(--faint);
  }
</style>
