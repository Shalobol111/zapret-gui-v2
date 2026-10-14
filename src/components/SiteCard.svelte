<script lang="ts">
  // «Мой сайт»: почему не открывается конкретный сайт. Строки — как в «Диагностике»
  // (точка · проверка · итог), ниже — вердикт и действия. Подбор стратегии под сайт
  // показывается здесь же строкой прогресса и итогом, без таблицы.
  import Button from "./Button.svelte";
  import Spinner from "./Spinner.svelte";
  import * as api from "../lib/api";
  import { offerRestart } from "../lib/restart";
  import { app, go } from "../lib/state.svelte";
  import { askDialog, confirmDialog, toast } from "../lib/ui.svelte";
  import type { SiteAction, SiteProbe, SiteReport } from "../lib/types";

  const RECENT_KEY = "zgui-recent-sites";
  function loadRecent(): string[] {
    try {
      return JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
    } catch {
      return [];
    }
  }

  let input = $state("");
  let report = $state<SiteReport | null>(null);
  let checking = $state(false);
  let busy = $state<SiteAction | null>(null);
  let compare = $state<SiteProbe | null>(null);
  let recent = $state<string[]>(loadRecent());
  const fail = (e: unknown) => toast(api.errorText(e), "err");

  function remember(domain: string) {
    recent = [domain, ...recent.filter((d) => d !== domain)].slice(0, 5);
    try {
      localStorage.setItem(RECENT_KEY, JSON.stringify(recent));
    } catch {
      /* не критично */
    }
  }

  async function check(value = input) {
    if (!value.trim() || checking) return;
    input = value;
    checking = true;
    compare = null;
    try {
      report = await api.siteCheck(value);
      input = report.domain;
      remember(report.domain);
    } catch (e) {
      fail(e);
    } finally {
      checking = false;
    }
  }

  // ── Подбор под сайт ──────────────────────────────────────────
  const pick = $derived(app.tests?.kind === "custom" && app.tests.site === report?.domain ? app.tests : null);

  async function startPick(stopService = false) {
    if (!report) return;
    try {
      await api.testsSite(report.domain, stopService);
    } catch (e) {
      if (api.errorText(e) !== "service") return fail(e);
      const ok = await confirmDialog({
        title: "Остановить службу на время подбора?",
        text: "Установлена служба zapret — во время перебора стратегий она мешает. Служба будет остановлена и снова запущена после подбора.",
        ok: "Остановить и подобрать",
      });
      if (ok) await startPick(true);
    }
  }

  async function apply(name: string) {
    try {
      if (app.status?.service.state && app.status.service.state !== "not_installed") await api.serviceInstall(name);
      else await api.bypassStart(name);
      toast(`Включена стратегия «${name}»`, "ok");
      await check(report?.domain);
    } catch (e) {
      fail(e);
    }
  }

  // ── Действия из вердикта ─────────────────────────────────────
  async function run(action: SiteAction) {
    if (!report) return;
    busy = action;
    try {
      switch (action) {
        case "add_to_list": {
          const added = await api.siteAdd(report.domain);
          if (added) offerRestart(`${report.domain} добавлен в «Свои домены»`);
          else toast("Домен уже есть в «Своих доменах»", "info");
          // Теперь сайт в списках — следующий шаг: подобрать стратегию.
          report.actions = ["pick", ...report.actions.filter((a) => a !== "add_to_list" && a !== "pick")];
          const lists = report.items.find((i) => i.id === "lists");
          if (lists) Object.assign(lists, { level: "ok", value: "есть", detail: `list-general-user.txt: «${report.domain}»` });
          if (!report.reachable) {
            report.verdict = "Домен в «Своих доменах». Подберите стратегию, с которой сайт откроется.";
            report.verdictLevel = "warn";
          }
          break;
        }
        case "pick":
          await startPick();
          break;
        case "enable_bypass":
          go("main");
          break;
        case "open_exclude":
          go("lists");
          break;
        case "secure_dns": {
          const r = await askDialog({
            title: "Как включить Secure DNS",
            text:
              "Windows 11: Параметры → Сеть и Интернет → ваше подключение → Назначение DNS-сервера → «Изменить», " +
              "укажите DNS (например 1.1.1.1 и 8.8.8.8) и включите шифрование «Только зашифрованный (DNS по HTTPS)».\n\n" +
              "Браузер: в настройках Chrome/Edge включите «Использовать безопасный DNS» и выберите поставщика, отличного от «по умолчанию».",
            ok: "Открыть параметры сети",
            cancel: "Закрыть",
          });
          if (r === "ok") await api.openPath("dns");
          break;
        }
        case "compare":
          compare = await api.siteCompare(report.domain);
          break;
      }
    } catch (e) {
      fail(e);
    } finally {
      busy = null;
    }
  }

  const labels: Record<SiteAction, string> = {
    add_to_list: "Добавить в Свои домены",
    pick: "Подобрать стратегию",
    enable_bypass: "Включить обход",
    secure_dns: "Как включить Secure DNS",
    open_exclude: "Открыть Списки",
    compare: "Сравнить без обхода",
  };
  const needsAdmin: SiteAction[] = ["pick", "compare"];
  const short = (name: string) => name.replace(/^general\s*\((.+)\)$/i, "$1");
</script>

<section class="card" aria-labelledby="site">
  <div class="head">
    <h2 id="site">Мой сайт</h2>
    <form
      class="form"
      onsubmit={(e) => {
        e.preventDefault();
        check();
      }}
    >
      <input
        type="text"
        spellcheck="false"
        placeholder="rutracker.org или ссылка"
        aria-label="Адрес сайта"
        bind:value={input}
        disabled={checking}
      />
      <!-- Button всегда type="button" — форму он не отправляет, поэтому обработчик явный (Enter ловит onsubmit). -->
      <Button variant="primary" height={34} disabled={!input.trim() || checking || !!pick?.running} onclick={() => check()}>
        {checking ? "Проверка…" : "Проверить"}
      </Button>
    </form>
  </div>

  <div class="body">
    {#if !report && !checking}
      <div class="intro">
        <p>
          Введите адрес сайта, который не открывается. Программа проверит, есть ли он в списках zapret, не подменяет ли
          провайдер DNS и открывается ли он сейчас, — и подскажет, что сделать.
        </p>
        {#if recent.length}
          <div class="chips">
            <span class="chips-label">Недавние:</span>
            {#each recent as d (d)}<button type="button" class="chip" onclick={() => check(d)}>{d}</button>{/each}
          </div>
        {/if}
      </div>
    {:else}
      <ul>
        {#if checking}
          <li class="wait"><Spinner /> Проверяю {input.trim()}…</li>
        {:else if report}
          {#each report.items as it (it.id)}
            <li>
              <span class="dot {it.level}"></span>
              <span class="title">{it.title}</span>
              <span class="value {it.level}">{it.value}</span>
              {#if it.detail}<span class="detail">{it.detail}</span>{/if}
            </li>
          {/each}
          {#if compare}
            <li>
              <span class="dot {compare.tls12 === 'ok' || compare.tls13 === 'ok' ? 'ok' : 'err'}"></span>
              <span class="title">Без обхода</span>
              <span class="value">{compare.tls12 === "ok" || compare.tls13 === "ok" ? "открывается" : "не открывается"}</span>
              <span class="detail"
                >{compare.tls12 === "ok" || compare.tls13 === "ok"
                  ? "Провайдер сайт не режет — проблема не в блокировке"
                  : "Провайдер режет сайт — нужен обход"}</span
              >
            </li>
          {/if}
        {/if}
      </ul>
    {/if}
  </div>

  {#if report && !checking}
    <div class="verdict">
      <p class="vtext"><span class="dot {report.verdictLevel}"></span>{report.verdict}</p>

      {#if pick}
        <p class="pick">
          {#if pick.running}
            <Spinner size={11} /> Подбираю: {pick.phase ?? "подготовка…"}
            <button type="button" class="link" onclick={() => api.testsCancel()}>Остановить</button>
          {:else if pick.best}
            <span class="dot ok"></span>Сайт открывается со стратегией «{short(pick.best)}»
          {:else if !pick.cancelled}
            <span class="dot err"></span>Ни одна стратегия не открыла сайт — возможно, его блокируют по IP: попробуйте
            IPSet «Любой» на Главной.
          {/if}
        </p>
      {/if}

      <div class="actions">
        {#if pick && !pick.running && pick.best}
          <Button height={32} size={12} disabled={!app.isAdmin} onclick={() => apply(pick.best!)}>Включить «{short(pick.best)}»</Button>
        {/if}
        {#each report.actions as a (a)}
          <Button
            height={32}
            size={12}
            disabled={busy !== null || !!pick?.running || (needsAdmin.includes(a) && !app.isAdmin) || !!app.status?.testsRunning}
            title={needsAdmin.includes(a) && !app.isAdmin ? "Нужны права администратора" : undefined}
            onclick={() => run(a)}>{busy === a ? "…" : labels[a]}</Button
          >
        {/each}
      </div>
    </div>
  {/if}
</section>

<style>
  .card {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    border-radius: 16px;
    background: var(--card);
    border: 1px solid var(--line);
    overflow: hidden;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 14px 18px;
    border-bottom: 1px solid var(--line);
  }
  h2 {
    font-size: 14px;
    font-weight: 600;
    white-space: nowrap;
  }
  .form {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    gap: 8px;
  }
  input {
    flex-grow: 1;
    min-width: 0;
    height: 34px;
    padding: 0 12px;
    border-radius: 9px;
    background: var(--raised);
    border: 1px solid var(--line2);
    color: var(--tx);
    font-family: var(--font-mono);
    font-size: 13px;
    user-select: text;
  }
  input::placeholder {
    color: var(--faint);
  }
  .body {
    flex-grow: 1;
    min-height: 0;
    overflow: auto;
  }
  .intro {
    padding: 16px 18px;
  }
  .intro p {
    margin: 0;
    font-size: 13px;
    line-height: 1.5;
    color: var(--dim);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    margin-top: 14px;
  }
  .chips-label {
    font-size: 12px;
    color: var(--faint);
  }
  .chip {
    height: 26px;
    padding: 0 10px;
    border-radius: 7px;
    border: 1px solid var(--line2);
    background: transparent;
    color: var(--tx2);
    font-family: var(--font-mono);
    font-size: 12px;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 6px 18px;
  }
  li {
    display: grid;
    grid-template-columns: 6px 1fr auto;
    column-gap: 10px;
    align-items: center;
    padding: 8px 0;
    font-size: 13px;
  }
  li.wait {
    display: flex;
    gap: 8px;
    color: var(--faint);
  }
  .detail {
    grid-column: 2 / 4;
    font-size: 12px;
    color: var(--faint);
    overflow-wrap: anywhere;
    user-select: text;
  }
  .dot {
    display: inline-block;
    width: 6px;
    height: 6px;
    border-radius: 3px;
    flex-shrink: 0;
    background: var(--accent);
  }
  .dot.info {
    background: var(--mute);
  }
  .dot.warn {
    background: var(--warn);
  }
  .dot.err {
    background: var(--err);
  }
  .value {
    font-size: 12px;
    color: var(--faint);
    text-align: right;
  }
  .value.warn {
    color: var(--warn);
  }
  .value.err {
    color: var(--err);
  }
  .verdict {
    padding: 14px 18px;
    border-top: 1px solid var(--line);
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .vtext,
  .pick {
    margin: 0;
    display: flex;
    align-items: baseline;
    gap: 8px;
    font-size: 13px;
    line-height: 1.45;
  }
  .vtext .dot,
  .pick .dot {
    transform: translateY(-1px);
  }
  .pick {
    align-items: center;
    color: var(--dim);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .actions:empty {
    display: none;
  }
  .link {
    border: none;
    background: none;
    padding: 0;
    color: var(--accent);
    font-size: 12px;
  }
</style>
