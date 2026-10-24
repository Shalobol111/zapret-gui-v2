<script lang="ts">
  // «Доступна новая версия ZapretGUI»: что нового (текст релиза), «Позже» / «Установить».
  // Показывается сам один раз за запуск для каждой новой версии (пока её не отложили «Позже»),
  // а также по клику на уведомление Windows и по кнопке в Настройках.
  import Button from "./Button.svelte";
  import Modal from "./Modal.svelte";
  import Spinner from "./Spinner.svelte";
  import * as api from "../lib/api";
  import { app, patchSettings } from "../lib/state.svelte";

  const u = $derived(app.status?.appUpdate);
  const busy = $derived(u?.state === "downloading" || u?.state === "installing");
  let shown = $state<string | null>(null);
  let error = $state<string | null>(null);

  // Новая версия, которую ещё не откладывали, — предложить один раз (не поверх мастера).
  $effect(() => {
    if (
      u?.state === "available" &&
      u.latest &&
      shown !== u.latest &&
      app.settings?.onboardingDone &&
      app.settings.appUpdateDismissed !== u.latest
    ) {
      shown = u.latest;
      app.updatePrompt = true;
    }
  });

  const open = $derived(app.updatePrompt && !!u && (u.state === "available" || busy || !!error));

  function later() {
    if (busy) return;
    if (u?.latest) patchSettings({ appUpdateDismissed: u.latest });
    error = null;
    app.updatePrompt = false;
  }

  async function install() {
    error = null;
    try {
      await api.appUpdateInstall();
    } catch (e) {
      error = api.errorText(e);
    }
  }

  /** Markdown релиза → простые строки: заголовки, пункты, абзацы (без ссылок и **). */
  type Line = { kind: "h" | "li" | "p"; text: string };
  function render(md: string): Line[] {
    const clean = (s: string) =>
      s
        .replace(/!\[[^\]]*\]\([^)]*\)/g, "")
        .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
        .replace(/\*\*|__|`/g, "")
        .trim();
    const out: Line[] = [];
    for (const raw of md.split("\n")) {
      const l = raw.trim();
      if (!l || /^(\||-{3,}|<)/.test(l)) continue;
      if (/^#{1,6}\s/.test(l)) out.push({ kind: "h", text: clean(l.replace(/^#+\s*/, "")) });
      else if (/^[-*+]\s/.test(l)) out.push({ kind: "li", text: clean(l.slice(2)) });
      else out.push({ kind: "p", text: clean(l) });
    }
    return out.filter((x) => x.text).slice(0, 40);
  }
  const lines = $derived(u?.notes ? render(u.notes) : []);
</script>

{#if open && u}
  <Modal title={`Доступна ZapretGUI ${u.latest ?? ""}`} width={480} onclose={later}>
    <p class="lead">
      У вас версия {u.current}. Новая версия скачается с GitHub, программа обновится и запустится снова{u.portable
        ? " — файл заменится на месте"
        : ""}. Настройки и zapret не затрагиваются.
    </p>
    {#if lines.length}
      <div class="notes" aria-label="Что нового">
        {#each lines as l, i (i)}
          {#if l.kind === "h"}<h3>{l.text}</h3>
          {:else if l.kind === "li"}<p class="li">{l.text}</p>
          {:else}<p>{l.text}</p>{/if}
        {/each}
      </div>
    {/if}
    {#if busy}
      <div class="progress">
        <Spinner size={10} />
        <span>
          {u.state === "installing"
            ? "Установка — программа сейчас закроется и запустится снова…"
            : `Скачивание${u.progress != null ? ` · ${u.progress}%` : "…"}`}
        </span>
      </div>
      {#if u.state === "downloading"}
        <div class="bar" aria-hidden="true"><span style:width="{u.progress ?? 4}%"></span></div>
      {/if}
    {:else if error}
      <p class="err">
        {error} ·
        <button type="button" class="link" onclick={() => api.openPath("appreleases")}>Страница релизов</button>
      </p>
    {/if}
    {#snippet buttons()}
      <Button disabled={busy} onclick={later}>Позже</Button>
      <Button variant="primary" disabled={busy || !app.isAdmin} title={app.isAdmin ? undefined : "Нужны права администратора"} onclick={install}
        >{busy ? "Обновление…" : error ? "Повторить" : "Установить"}</Button
      >
    {/snippet}
  </Modal>
{/if}

<style>
  .lead {
    margin: 0 0 12px;
    font-size: 13px;
    line-height: 1.5;
    color: var(--dim);
  }
  .notes {
    max-height: 220px;
    overflow: auto;
    padding: 12px 14px;
    border-radius: 10px;
    background: var(--raised);
    border: 1px solid var(--line);
  }
  .notes h3 {
    margin: 8px 0 4px;
    font-size: 13px;
    font-weight: 600;
  }
  .notes h3:first-child {
    margin-top: 0;
  }
  .notes p {
    margin: 0 0 4px;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--dim);
  }
  .notes .li {
    position: relative;
    padding-left: 14px;
  }
  .notes .li::before {
    content: "•";
    position: absolute;
    left: 2px;
    color: var(--faint);
  }
  .progress {
    margin-top: 14px;
    display: flex;
    align-items: center;
    gap: 9px;
    font-size: 13px;
  }
  .bar {
    margin-top: 8px;
    height: 6px;
    border-radius: 3px;
    background: var(--raised);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.3s ease;
  }
  .err {
    margin: 14px 0 0;
    font-size: 12.5px;
    color: var(--err);
  }
  .link {
    border: none;
    background: none;
    padding: 0;
    font-size: 12.5px;
    color: var(--accent);
  }
</style>
