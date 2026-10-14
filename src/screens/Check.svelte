<script lang="ts">
  import DiagCard from "../components/DiagCard.svelte";
  import PageHeader from "../components/PageHeader.svelte";
  import Segmented from "../components/Segmented.svelte";
  import SiteCard from "../components/SiteCard.svelte";
  import TestsCard from "../components/TestsCard.svelte";
  import { app } from "../lib/state.svelte";
  import type { TestKind } from "../lib/types";

  // Тип теста: по умолчанию — как у последнего прогона.
  let kind = $state<TestKind>(app.tests?.kind ?? "standard");
  $effect(() => {
    if (app.tests?.running) kind = app.tests.kind;
  });
</script>

<PageHeader title="Проверка" align="center">
  <Segmented
    label="Тип теста"
    height={30}
    value={kind}
    disabled={!!app.tests?.running}
    options={[
      { value: "standard", label: "Сайты · HTTP / Ping" },
      { value: "dpi", label: "DPI · TCP 16–20" },
      { value: "custom", label: "Мой сайт" },
    ]}
    onchange={(v) => (kind = v)}
  />
</PageHeader>

<div class="body">
  {#if kind === "custom"}
    <SiteCard />
  {:else}
    <TestsCard {kind} />
  {/if}
  <DiagCard visible={app.screen === "check"} />
</div>

<style>
  .body {
    flex-grow: 1;
    min-height: 0;
    display: flex;
    gap: 16px;
  }
</style>
