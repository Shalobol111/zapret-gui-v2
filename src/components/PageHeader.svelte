<script lang="ts">
  import type { Snippet } from "svelte";

  // Шапка экрана из макета: H1 20/600 и справа мелкий статус или элемент управления.
  let { title, align = "baseline", children }: { title: string; align?: "baseline" | "center"; children?: Snippet } =
    $props();
</script>

<div class="head" style:align-items={align}>
  <h1>{title}</h1>
  {@render children?.()}
</div>

<style>
  .head {
    display: flex;
    justify-content: space-between;
    gap: 16px;
    flex-shrink: 0;
  }
  h1 {
    font-size: 20px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }
</style>
