<script lang="ts" generics="T extends string">
  // Сегментный переключатель из макета: дорожка seg + рамка line, radius 9, padding 3;
  // сегмент 28 px (30 в шапке «Проверки»), radius 7, 12/500; выбранный — segon + segshadow.
  let {
    options,
    value,
    label,
    height = 28,
    disabled = false,
    onchange,
  }: {
    options: { value: T; label: string }[];
    value: T;
    label: string;
    height?: 28 | 30;
    disabled?: boolean;
    onchange: (v: T) => void;
  } = $props();
</script>

<div class="track" role="group" aria-label={label}>
  {#each options as o (o.value)}
    <button
      type="button"
      class="seg"
      class:on={o.value === value}
      style:height="{height}px"
      style:padding="0 {height === 30 ? 14 : 12}px"
      aria-pressed={o.value === value}
      {disabled}
      onclick={() => o.value !== value && onchange(o.value)}>{o.label}</button
    >
  {/each}
</div>

<style>
  .track {
    display: flex;
    padding: 3px;
    gap: 2px;
    border-radius: 9px;
    background: var(--seg);
    border: 1px solid var(--line);
    flex-shrink: 0;
  }
  .seg {
    border: none;
    border-radius: 7px;
    font-size: 12px;
    font-weight: 500;
    background: transparent;
    color: var(--dim);
    white-space: nowrap;
  }
  .seg.on {
    background: var(--segon);
    color: var(--tx);
    box-shadow: var(--segshadow);
  }
  .seg:disabled {
    cursor: default;
    opacity: 0.6;
  }
</style>
