<script lang="ts">
  // «Починить Discord»: выбор шагов галочками → выполнение по шагам с прогрессом → итог.
  // Шаги выполняет ядро (discord_step); подбор стратегии — обычный быстрый подбор, его
  // итог применяется здесь же (служба переустанавливается, ручной обход перезапускается).
  import Button from "./Button.svelte";
  import Modal from "./Modal.svelte";
  import Spinner from "./Spinner.svelte";
  import * as api from "../lib/api";
  import { onMount } from "svelte";
  import { app } from "../lib/state.svelte";
  import { toast } from "../lib/ui.svelte";
  import type { DiscordInfo, DiscordStep, DiscordStepResult, TestRun } from "../lib/types";

  let { onclose }: { onclose: () => void } = $props();

  type Row = {
    id: DiscordStep;
    title: string;
    hint: string;
    enabled: boolean;
    checked: boolean;
    state: "wait" | "run" | "done" | "skip";
    res: DiscordStepResult | null;
  };

  let info = $state<DiscordInfo | null>(null);
  let rows = $state<Row[]>([]);
  let phase = $state<"choose" | "run" | "done">("choose");
  let picking = $state(false);
  /** Что происходит внутри шага «Сайты» после подбора: включение стратегии и повторная проверка. */
  let pickStage = $state<string | null>(null);
  let applied = $state<string | null>(null);

  const short = (name: string) => name.replace(/^general\s*\((.+)\)$/i, "$1");
  const names = (list: { name: string }[]) => list.map((c) => c.name).join(", ");
  const needAdmin = "Нужны права администратора";

  function build(i: DiscordInfo): Row[] {
    const running = i.clients.filter((c) => c.running);
    const admin = app.isAdmin;
    const row = (id: DiscordStep, title: string, hint: string, enabled = true): Row => ({
      id, title, hint, enabled, checked: enabled, state: "wait", res: null,
    });
    return [
      row(
        "clean",
        "Закрыть Discord и очистить кэш",
        !i.clients.length
          ? "Discord не установлен"
          : running.length
            ? `Сейчас запущен ${names(running)} — он будет закрыт. Настройки и вход в аккаунт не затрагиваются`
            : "Папки Cache, Code Cache и GPUCache. Настройки и вход в аккаунт не затрагиваются",
        i.clients.length > 0,
      ),
      row("hosts", "Обновить hosts", admin ? "Адреса голосовых серверов Discord из списка Flowseal, с резервной копией" : needAdmin, admin),
      row(
        "voice",
        "Проверить обход и голосовые фильтры",
        !admin
          ? needAdmin
          : i.bypassOn
            ? `Обрабатывает ли стратегия${i.strategy ? ` «${short(i.strategy)}»` : ""} UDP-порты голосовых каналов`
            : i.strategy
              ? `Обход выключен — будет включён со стратегией «${short(i.strategy)}»`
              : "Обход выключен — стратегию подберёт следующий шаг",
        admin,
      ),
      row(
        "sites",
        "Проверить сайты Discord",
        "discord.com, gateway.discord.gg, cdn.discordapp.com, updates.discord.com. Не открываются — подбор стратегии" +
          (i.service ? " (служба будет остановлена на время подбора)" : ""),
      ),
      row(
        "launch",
        "Запустить Discord",
        !i.clients.length ? "Discord не установлен" : running.length ? `${names(running)} — будет запущен снова` : names(i.clients.slice(0, 1)),
        i.clients.length > 0,
      ),
    ];
  }

  onMount(() => {
    api
      .discordInfo()
      .then((i) => {
        info = i;
        rows = build(i);
      })
      .catch((e) => {
        onclose();
        toast(api.errorText(e), "err");
      });
  });

  // ── Подбор стратегии внутри шага «Сайты» ─────────────────────
  function waitPick(since: number): Promise<TestRun> {
    return new Promise((resolve) => {
      const t = setInterval(() => {
        const r = app.tests;
        if (r && r.quick && !r.running && r.startedAt >= since) {
          clearInterval(t);
          resolve(r);
        }
      }, 300);
    });
  }

  const pickText = $derived.by(() => {
    const r = app.tests;
    if (pickStage) return pickStage;
    if (!picking || !r?.running) return "подбор стратегии…";
    const i = r.rows.findIndex((x) => x.state === "running");
    return i < 0 ? (r.phase ?? "подбор…") : `подбор: «${short(r.rows[i].name)}» · ${i + 1} из ${r.rows.length}`;
  });

  async function pickAndApply(first: DiscordStepResult): Promise<DiscordStepResult> {
    const since = Date.now() - 1000;
    picking = true;
    app.pickOwner = "discord";
    try {
      await api.testsQuick(true);
      const run = await waitPick(since);
      if (run.cancelled) return { ...first, level: "warn", value: "подбор остановлен" };
      if (run.error) return { ...first, detail: run.error };
      if (!run.best)
        return { ...first, value: "рабочая стратегия не найдена", detail: "Запустите полную проверку стратегий в «Проверке»" };
      const best = run.best;
      pickStage = `включаю «${short(best)}»…`;
      if (app.status?.service.state && app.status.service.state !== "not_installed") await api.serviceInstall(best);
      else await api.bypassStart(best);
      applied = best;
      pickStage = "проверяю снова…";
      const again = await api.discordStep("sites");
      return again.level === "ok"
        ? { ...again, value: `открываются с «${short(best)}»`, detail: `Включена стратегия «${best}»` }
        : { ...again, detail: `Включена стратегия «${best}», но всё ещё не открываются: ${again.detail}` };
    } finally {
      picking = false;
      pickStage = null;
    }
  }

  async function runAll() {
    phase = "run";
    let closed: string[] = [];
    for (const row of rows) {
      if (!row.checked || !row.enabled) {
        row.state = "skip";
        continue;
      }
      row.state = "run";
      try {
        let res = await api.discordStep(row.id, row.id === "launch" ? closed : []);
        if (row.id === "clean") closed = res.closed;
        if (row.id === "sites" && res.pick) res = await pickAndApply(res);
        row.res = res;
      } catch (e) {
        row.res = { level: "err", value: "ошибка", detail: api.errorText(e), closed: [], pick: false };
      }
      row.state = "done";
    }
    phase = "done";
    app.pickOwner = null;
  }

  const problems = $derived(rows.filter((r) => r.res && (r.res.level === "err" || r.res.level === "warn")));
  const close = () => {
    if (phase !== "run") onclose();
  };
</script>

<Modal title="Починить Discord" width={500} onclose={close}>
  {#if !info}
    <p class="lead wait"><Spinner /> Ищу Discord…</p>
  {:else if phase === "choose"}
    <p class="lead">
      {#if info.clients.length}
        Найден {names(info.clients)}. Отметьте, что сделать, — шаги выполнятся по порядку.
      {:else}
        Discord не найден — можно обновить hosts и проверить, открываются ли сайты Discord.
      {/if}
    </p>
    {#each rows as r (r.id)}
      <label class="opt" class:off={!r.enabled}>
        <input type="checkbox" bind:checked={r.checked} disabled={!r.enabled} />
        <span>
          <span class="opt-title">{r.title}</span>
          <span class="opt-hint">{r.hint}</span>
        </span>
      </label>
    {/each}
  {:else}
    <ul class="steps">
      {#each rows as r (r.id)}
        <li class:skip={r.state === "skip"}>
          {#if r.state === "run"}<Spinner size={8} />{:else}<span class="dot {r.state === 'done' ? r.res?.level : 'wait'}"></span>{/if}
          <span class="title">{r.title}</span>
          <span class="value {r.res?.level ?? ''}">
            {#if r.state === "run"}{r.id === "sites" && picking ? pickText : "выполняется…"}
            {:else if r.state === "skip"}пропущено
            {:else if r.res}{r.res.value}
            {/if}
          </span>
          {#if r.res?.detail && r.state === "done"}<span class="detail">{r.res.detail}</span>{/if}
        </li>
      {/each}
    </ul>
    {#if phase === "done"}
      <p class="summary">
        {#if problems.length}
          Сделано, но осталось: {problems.map((p) => p.title.toLowerCase()).join(", ")} — подробности выше.
        {:else}
          Готово{applied ? `, включена стратегия «${short(applied)}»` : ""}.
        {/if}
        Голосовую связь программа проверить не может — для этого нужен протокол Discord. Зайдите в голосовой канал;
        если голоса нет, попробуйте другую стратегию в «Проверке».
      </p>
    {/if}
  {/if}

  {#snippet buttons()}
    {#if phase === "choose"}
      <Button onclick={onclose}>Отмена</Button>
      <Button variant="primary" disabled={!info || !rows.some((r) => r.checked && r.enabled)} onclick={runAll}>Начать</Button>
    {:else if phase === "run"}
      <Button disabled={!picking || !!pickStage} onclick={() => api.testsCancel()}>{picking ? "Остановить подбор" : "Выполняется…"}</Button>
    {:else}
      <Button variant="primary" onclick={onclose}>Готово</Button>
    {/if}
  {/snippet}
</Modal>

<style>
  .lead {
    margin-bottom: 12px;
    font-size: 13px;
    line-height: 1.5;
  }
  .lead.wait {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--faint);
  }
  .opt {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 8px 0;
    cursor: pointer;
  }
  .opt.off {
    cursor: default;
    opacity: 0.55;
  }
  .opt input {
    margin-top: 2px;
  }
  .opt-title {
    display: block;
    color: var(--tx);
  }
  .opt-hint {
    display: block;
    font-size: 12px;
    line-height: 1.45;
    color: var(--faint);
  }
  .steps {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .steps li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    column-gap: 10px;
    padding: 9px 0;
    font-size: 13px;
    border-bottom: 1px solid var(--line);
  }
  .steps li:last-child {
    border-bottom: 0;
  }
  .steps li.skip {
    color: var(--faint);
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 3px;
    flex-shrink: 0;
    background: var(--accent);
  }
  .dot.wait,
  .dot.info {
    background: var(--mute);
  }
  .dot.warn {
    background: var(--warn);
  }
  .dot.err {
    background: var(--err);
  }
  .title {
    flex: 1 1 0;
    min-width: 0;
  }
  .value {
    font-size: 12px;
    color: var(--faint);
    text-align: right;
  }
  .value.ok {
    color: var(--accent);
  }
  .value.warn {
    color: var(--warn);
  }
  .value.err {
    color: var(--err);
  }
  .detail {
    flex-basis: 100%;
    padding-left: 16px;
    margin-top: 3px;
    font-size: 12px;
    line-height: 1.45;
    color: var(--faint);
  }
  .summary {
    margin: 14px 0 0;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--dim);
  }
</style>
