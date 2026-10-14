<script lang="ts">
  import type { Snippet } from "svelte";

  // Кнопки из макета. primary — фон tx, текст bg, 13/600 (одна на экран);
  // secondary — прозрачная, рамка line2; danger — текст err, рамка danger-line.
  let {
    variant = "secondary",
    height = 36,
    size = 13,
    disabled = false,
    title,
    onclick,
    children,
  }: {
    variant?: "primary" | "secondary" | "danger";
    height?: 30 | 32 | 34 | 36 | 38;
    size?: 12 | 13;
    disabled?: boolean;
    title?: string;
    onclick?: () => void;
    children: Snippet;
  } = $props();
</script>

<button
  type="button"
  class={variant}
  style:height="{height}px"
  style:font-size="{size}px"
  style:border-radius="{height <= 32 ? 8 : 9}px"
  {disabled}
  {title}
  {onclick}
>
  {@render children()}
</button>

<style>
  button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 0 14px;
    white-space: nowrap;
    flex-shrink: 0;
  }
  .primary {
    border: 1px solid var(--tx);
    background: var(--tx);
    color: var(--bg);
    font-weight: 600;
  }
  .secondary {
    border: 1px solid var(--line2);
    background: transparent;
    color: var(--tx);
  }
  .danger {
    border: 1px solid var(--danger-line);
    background: transparent;
    color: var(--err);
  }
  button:disabled {
    cursor: default;
    opacity: 0.5;
  }
</style>
