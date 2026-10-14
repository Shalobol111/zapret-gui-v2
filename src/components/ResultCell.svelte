<script lang="ts">
  // Плашка результата из макета: моно 12, padding 2 8, radius 6.
  // «ок» — accent на accent-soft, «нет» — err на err-soft, «—» — mute.
  import Spinner from "./Spinner.svelte";
  import type { Cell } from "../lib/types";

  let { cell, busy = false }: { cell: Cell | undefined; busy?: boolean } = $props();
  const state = $derived(cell?.state ?? "none");
</script>

{#if busy}
  <span class="wrap"><Spinner /></span>
{:else}
  <span class="cell {state}" title={cell?.hint || undefined}>{state === "ok" ? "ок" : state === "fail" ? "нет" : "—"}</span>
{/if}

<style>
  .wrap {
    justify-self: center;
    display: flex;
  }
  .cell {
    justify-self: center;
    font-family: var(--font-mono);
    font-size: 12px;
    padding: 2px 8px;
    border-radius: 6px;
    color: var(--mute);
  }
  .ok {
    color: var(--accent);
    background: var(--accent-soft);
  }
  .fail {
    color: var(--err);
    background: var(--err-soft);
  }
</style>
