<script lang="ts">
  // Переключатель из макета: 40×24, radius 12. Вкл — accent и ручка onaccent (left 19),
  // выкл — line2 и ручка dim (left 3). Ручка едет за 0,18 с.
  let {
    checked,
    label,
    disabled = false,
    title,
    onchange,
  }: { checked: boolean; label: string; disabled?: boolean; title?: string; onchange: (v: boolean) => void } = $props();
</script>

<button
  type="button"
  role="switch"
  aria-checked={checked}
  aria-label={label}
  class:on={checked}
  {disabled}
  {title}
  onclick={() => onchange(!checked)}
>
  <span class="knob"></span>
</button>

<style>
  button {
    position: relative;
    width: 40px;
    height: 24px;
    flex-shrink: 0;
    border-radius: 12px;
    border: none;
    padding: 0;
    background: var(--line2);
  }
  button.on {
    background: var(--accent);
  }
  button:disabled {
    cursor: default;
    opacity: 0.5;
  }
  .knob {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 18px;
    height: 18px;
    border-radius: 9px;
    background: var(--dim);
    transition:
      left 0.18s var(--theme-ease),
      background-color var(--theme-dur) var(--theme-ease);
  }
  .on .knob {
    left: 19px;
    background: var(--onaccent);
  }
</style>
