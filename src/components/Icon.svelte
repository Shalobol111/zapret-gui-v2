<script lang="ts">
  import { ICONS, type IconName } from "../lib/icons";

  let { name, size = 18, stroke = 1.8 }: { name: IconName; size?: number; stroke?: number } = $props();
  const def = $derived(ICONS[name]);
</script>

<svg
  aria-hidden="true"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill={def.filled ? "currentColor" : "none"}
  stroke={def.filled ? "none" : "currentColor"}
  stroke-width={stroke}
  stroke-linecap="round"
  stroke-linejoin="round"
>
  {@html def.body}
</svg>

<style>
  svg {
    flex-shrink: 0;
    display: block;
  }
</style>
