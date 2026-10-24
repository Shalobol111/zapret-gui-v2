<script lang="ts">
  import AppUpdateDialog from "./components/AppUpdateDialog.svelte";
  import ConfirmHost from "./components/ConfirmHost.svelte";
  import Onboarding from "./components/Onboarding.svelte";
  import Sidebar from "./components/Sidebar.svelte";
  import ToastHost from "./components/ToastHost.svelte";
  import Main from "./screens/Main.svelte";
  import Check from "./screens/Check.svelte";
  import Lists from "./screens/Lists.svelte";
  import Telegram from "./screens/Telegram.svelte";
  import Settings from "./screens/Settings.svelte";
  import { app, effectiveMode, go } from "./lib/state.svelte";
  import type { Screen } from "./lib/types";

  const ORDER: Screen[] = ["main", "check", "lists", "telegram", "settings"];
  let width = $state(window.innerWidth);

  // Класс темы на <html>: CSS-переходы из макета анимируют смену за 0,5 с.
  $effect(() => {
    const light = effectiveMode() === "light";
    document.documentElement.classList.toggle("light", light);
    document.documentElement.classList.toggle("dark", !light);
  });

  function onKey(e: KeyboardEvent) {
    if (!e.ctrlKey || e.altKey || e.shiftKey || e.metaKey) return;
    const i = Number(e.key) - 1;
    if (i >= 0 && i < ORDER.length) {
      e.preventDefault();
      go(ORDER[i]);
    }
  }
</script>

<svelte:window bind:innerWidth={width} onkeydown={onKey} />

<div class="shell">
  <Sidebar compact={width < 900} />
  <!-- Экраны не размонтируются: несохранённый текст и результаты тестов переживают переходы. -->
  <main hidden={app.screen !== "main"}><Main /></main>
  <main hidden={app.screen !== "check"}><Check /></main>
  <main hidden={app.screen !== "lists"}><Lists /></main>
  <main hidden={app.screen !== "telegram"} class="scroll"><Telegram /></main>
  <main hidden={app.screen !== "settings"} class="scroll"><Settings /></main>
</div>
{#if app.settings && !app.settings.onboardingDone}<Onboarding />{/if}
<AppUpdateDialog />
<ToastHost />
<ConfirmHost />

<style>
  .shell {
    height: 100%;
    display: flex;
    background: var(--bg);
    color: var(--tx);
  }
  main {
    flex-grow: 1;
    min-width: 0;
    padding: 24px 28px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  main[hidden] {
    display: none;
  }
  main.scroll {
    overflow: auto;
  }
</style>
