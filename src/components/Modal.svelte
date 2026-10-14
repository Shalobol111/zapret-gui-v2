<script lang="ts">
  // Модальное окно (в макете его нет — карточка по токенам, одна главная кнопка).
  import type { Snippet } from "svelte";

  let {
    title,
    width = 420,
    onclose,
    children,
    buttons,
  }: { title: string; width?: number; onclose: () => void; children: Snippet; buttons: Snippet } = $props();

  let box = $state<HTMLDivElement>();
  const titleId = `md-${Math.random().toString(36).slice(2, 8)}`;

  $effect(() => {
    // Фокус на главную (последнюю) кнопку — Enter подтверждает.
    requestAnimationFrame(() => {
      const btns = box?.querySelectorAll<HTMLButtonElement>(".buttons button:not(:disabled)");
      btns?.[btns.length - 1]?.focus();
    });
  });

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="overlay" role="presentation" onmousedown={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby={titleId} style:width="{width}px" bind:this={box}>
    <h2 id={titleId}>{title}</h2>
    <div class="body">{@render children()}</div>
    <div class="buttons">{@render buttons()}</div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 40;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(5, 7, 9, 0.55);
    animation: fade 0.15s ease-out;
  }
  :global(.zr.light) .overlay {
    background: rgba(20, 24, 30, 0.28);
  }
  .dialog {
    max-width: calc(100% - 48px);
    max-height: calc(100% - 48px);
    display: flex;
    flex-direction: column;
    padding: 20px 22px 18px;
    border-radius: 16px;
    background: var(--card);
    border: 1px solid var(--line2);
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.35);
    animation: rise 0.18s var(--theme-ease);
  }
  h2 {
    font-size: 15px;
    font-weight: 600;
  }
  .body {
    margin: 8px 0 18px;
    font-size: 13px;
    line-height: 1.5;
    color: var(--dim);
    overflow: auto;
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(8px) scale(0.98);
    }
  }
</style>
