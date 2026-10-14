<script lang="ts">
  // Уведомления снизу справа (в макете их нет — оформлены токенами, вид согласуем).
  import { dismiss, ui } from "../lib/ui.svelte";
</script>

<div class="host" aria-live="polite">
  {#each ui.toasts as t (t.id)}
    <div class="toast" role="status">
      <span class="dot {t.kind}"></span>
      <span class="text">{t.text}</span>
      {#if t.action}
        <button
          type="button"
          class="action"
          onclick={() => {
            t.action!.run();
            dismiss(t.id);
          }}>{t.action.label}</button
        >
      {/if}
      <button type="button" class="close" aria-label="Закрыть" onclick={() => dismiss(t.id)}>×</button>
    </div>
  {/each}
</div>

<style>
  .host {
    position: fixed;
    right: 20px;
    bottom: 20px;
    z-index: 50;
    display: flex;
    flex-direction: column;
    gap: 8px;
    align-items: flex-end;
    pointer-events: none;
  }
  .toast {
    pointer-events: auto;
    display: flex;
    align-items: center;
    gap: 10px;
    max-width: 380px;
    padding: 10px 10px 10px 14px;
    border-radius: 12px;
    background: var(--card);
    border: 1px solid var(--line2);
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.25);
    font-size: 13px;
    line-height: 1.4;
    animation: rise 0.18s var(--theme-ease);
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 3px;
    flex-shrink: 0;
    background: var(--faint);
  }
  .dot.ok {
    background: var(--accent);
  }
  .dot.warn {
    background: var(--warn);
  }
  .dot.err {
    background: var(--err);
  }
  .text {
    color: var(--tx2);
  }
  .action {
    height: 28px;
    padding: 0 10px;
    border-radius: 7px;
    border: 1px solid var(--line2);
    background: transparent;
    color: var(--tx);
    font-size: 12px;
    flex-shrink: 0;
  }
  .close {
    width: 24px;
    height: 24px;
    border: none;
    background: transparent;
    color: var(--faint);
    font-size: 16px;
    line-height: 1;
    flex-shrink: 0;
  }
</style>
