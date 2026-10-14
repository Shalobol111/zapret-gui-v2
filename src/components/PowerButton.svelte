<script lang="ts">
  // Кнопка питания 112×112 из макета. Выкл: raised, рамка 2 px line2, иконка dim.
  // Вкл: accent-soft, рамка accent, иконка accent, внешнее кольцо 8 px accent-ring.
  import Icon from "./Icon.svelte";

  let {
    on,
    busy = false,
    disabled = false,
    title,
    onclick,
  }: { on: boolean; busy?: boolean; disabled?: boolean; title?: string; onclick: () => void } = $props();
</script>

<button
  type="button"
  class:on
  class:busy
  aria-label={on ? "Выключить обход" : "Включить обход"}
  aria-pressed={on}
  disabled={disabled || busy}
  {title}
  {onclick}
>
  <Icon name="power" size={40} stroke={2} />
</button>

<style>
  button {
    width: 112px;
    height: 112px;
    flex-shrink: 0;
    border-radius: 56px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--raised);
    border: 2px solid var(--line2);
    color: var(--dim);
    box-shadow: 0 0 0 0 transparent;
  }
  button.on {
    background: var(--accent-soft);
    border-color: var(--accent);
    color: var(--accent);
    box-shadow: 0 0 0 8px var(--accent-ring);
  }
  button:disabled {
    cursor: default;
  }
  button:disabled:not(.busy) {
    opacity: 0.55;
  }
  /* Запуск/остановка: кольцо мягко пульсирует, пока ядро не ответит. */
  button.busy {
    animation: pulse 1.2s ease-in-out infinite;
  }
  @keyframes pulse {
    0%,
    100% {
      box-shadow: 0 0 0 0 var(--accent-ring);
    }
    50% {
      box-shadow: 0 0 0 8px var(--accent-ring);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    button.busy {
      animation: none;
    }
  }
</style>
