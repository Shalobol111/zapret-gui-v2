<script lang="ts">
  // Telegram: локальный MTProto-прокси TgWsProxy (Flowseal/tg-ws-proxy). Карточка состояния —
  // как на Главной; ниже ручное подключение и настройки прокси.
  import Button from "../components/Button.svelte";
  import PageHeader from "../components/PageHeader.svelte";
  import PowerButton from "../components/PowerButton.svelte";
  import Switch from "../components/Switch.svelte";
  import * as api from "../lib/api";
  import { app, patchSettings } from "../lib/state.svelte";
  import { duration, toast } from "../lib/ui.svelte";
  import type { TgConfig } from "../lib/types";

  const fail = (e: unknown) => toast(api.errorText(e), "err");
  const tg = $derived(app.status?.telegram);
  const installed = $derived(!!tg?.installed);
  const running = $derived(!!tg?.running);

  // ── Настройки прокси (config.json TgWsProxy) ─────────────────
  let cfg = $state<TgConfig | null>(null);
  let form = $state({ port: "1443", secret: "", dc: "", cfproxy: true });
  let saving = $state(false);

  function fill(c: TgConfig) {
    cfg = c;
    form = { port: String(c.port), secret: c.secret, dc: c.dcIp.join("\n"), cfproxy: c.cfproxy };
  }
  async function load() {
    try {
      fill(await api.tgConfig());
    } catch (e) {
      fail(e);
    }
  }

  // Конфиг читаем при первом показе раздела и после скачивания (тогда появляется секрет).
  let loadedFor = $state<boolean | null>(null);
  $effect(() => {
    if (app.screen === "telegram" && tg && loadedFor !== tg.installed) {
      loadedFor = tg.installed;
      load();
    }
  });

  const dcList = $derived(
    form.dc
      .split(/[\s,;]+/)
      .map((s) => s.trim())
      .filter(Boolean),
  );
  const dirty = $derived(
    !!cfg &&
      (form.port !== String(cfg.port) ||
        form.secret.toLowerCase() !== cfg.secret ||
        form.cfproxy !== cfg.cfproxy ||
        dcList.join("\n") !== cfg.dcIp.join("\n")),
  );
  const portNum = $derived(Number(form.port));
  const portOk = $derived(Number.isInteger(portNum) && portNum >= 1024 && portNum <= 65535);
  const secretOk = $derived(/^[0-9a-f]{32}$/i.test(form.secret));
  const dcBad = $derived(dcList.find((d) => !/^\d{1,3}:(\d{1,3}\.){3}\d{1,3}$/.test(d)) ?? null);

  async function save() {
    saving = true;
    try {
      fill(await api.tgConfigSave({ port: portNum, secret: form.secret, dcIp: dcList, cfproxy: form.cfproxy }));
      toast(running ? "Сохранено, прокси перезапущен — подключите Telegram заново" : "Настройки сохранены", "ok", {
        label: "Подключить",
        run: connect,
      });
    } catch (e) {
      fail(e);
    } finally {
      saving = false;
    }
  }

  async function newSecret() {
    form.secret = await api.tgNewSecret();
  }

  // ── Действия ─────────────────────────────────────────────────
  let busy = $state<null | "power" | "install" | "external" | "connect">(null);
  async function run(kind: NonNullable<typeof busy>, fn: () => Promise<unknown>) {
    if (busy) return;
    busy = kind;
    try {
      await fn();
    } catch (e) {
      fail(e);
    } finally {
      busy = null;
    }
  }

  const toggle = () => run("power", () => (running ? api.tgStop() : api.tgStart()));
  const install = () =>
    run("install", async () => {
      toast(await api.tgInstall(), "ok");
      await load();
    });
  const closeExternal = () => run("external", () => api.tgCloseExternal());
  function connect() {
    run("connect", async () => {
      if (!running) await api.tgStart();
      await api.tgConnect();
      toast("Telegram откроет окно «Подключить прокси» — нажмите «Подключиться»", "info");
    });
  }
  async function copy(text: string, what: string) {
    try {
      await api.writeClipboard(text);
      toast(`${what} скопирован${what === "Ссылка" ? "а" : ""}`, "ok");
    } catch (e) {
      fail(e);
    }
  }

  // ── Тексты ───────────────────────────────────────────────────
  let now = $state(Date.now());
  $effect(() => {
    if (!running || !tg?.startedAt) return;
    now = Date.now();
    const t = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(t);
  });

  const title = $derived(
    tg?.busy ? `${tg.busy}…` : running ? (tg?.listening ? "Прокси включён" : "Запуск…") : "Прокси выключен",
  );
  const subtitle = $derived.by(() => {
    if (!tg) return "";
    if (!installed) return "TgWsProxy от Flowseal ещё не скачан — около 21 МБ с GitHub";
    if (running) {
      const up = tg.startedAt ? ` · работает ${duration(now - tg.startedAt)}` : "";
      return tg.listening ? `127.0.0.1:${tg.port}${up}` : "TgWsProxy запускается — несколько секунд";
    }
    return "Telegram Desktop подключается через локальный MTProto-прокси — без VPN и сторонних серверов";
  });
  const headerStatus = $derived(
    !tg?.installed
      ? "TgWsProxy · не скачан"
      : `TgWsProxy ${tg.version ?? ""}`.trim() + (tg.latest ? "" : " · обновлений нет"),
  );
  const secretShown = $derived(cfg?.secret ? `dd${cfg.secret}` : "—");
</script>

<PageHeader title="Telegram">
  <span class="head-status"
    >{headerStatus}{#if tg?.latest}&nbsp;·&nbsp;<button
        type="button"
        class="link upd"
        disabled={!!busy || !!tg.busy}
        onclick={install}>обновить до {tg.latest}</button
      >{/if}</span
  >
</PageHeader>

<section class="card state" aria-label="Состояние прокси">
  <PowerButton
    on={running}
    busy={busy === "power" || busy === "external" || (running && !tg?.listening) || !!tg?.busy}
    disabled={!installed || !!tg?.external || !!busy}
    title={!installed ? "Сначала скачайте TgWsProxy" : undefined}
    onclick={toggle}
  />
  <div class="state-body">
    <div class="titles">
      <div class="title-row">
        <span class="dot" class:on={running && tg?.listening}></span>
        <span class="title">{title}</span>
      </div>
      <span class="sub">{subtitle}</span>
      {#if tg?.external}
        <span class="note warn"
          >TgWsProxy уже запущен отдельно — две копии одновременно не работают ·
          <button type="button" class="link" disabled={!!busy || !installed} onclick={closeExternal}
            >Закрыть его и запустить здесь</button
          ></span
        >
      {/if}
    </div>
    {#if !installed}
      {#if tg?.busy}
        <div class="bar" aria-hidden="true"><span style:width="{tg.progress ?? 6}%"></span></div>
        <span class="note">{tg.busy}{tg.progress != null ? ` · ${tg.progress}%` : ""}</span>
      {:else}
        <div class="actions">
          <Button variant="primary" disabled={!!busy} onclick={install}>Скачать TgWsProxy</Button>
          <button type="button" class="link" onclick={() => api.openPath("tgreleases")}>Страница проекта</button>
        </div>
      {/if}
    {:else}
      <div class="actions">
        <Button variant="primary" disabled={!!busy || !!tg?.external || !!tg?.busy} onclick={connect}
          >{busy === "connect" ? "Открываю Telegram…" : "Подключить Telegram"}</Button
        >
        <Button disabled={!cfg?.link} onclick={() => cfg?.link && copy(cfg.link, "Ссылка")}>Скопировать ссылку</Button>
      </div>
    {/if}
  </div>
</section>

<div class="row2">
  <section class="card small" aria-labelledby="tg-manual">
    <h2 id="tg-manual">Подключение вручную</h2>
    <p class="hint">
      Если кнопка не сработала: Telegram → Настройки → Продвинутые настройки → Тип подключения → Прокси → Добавить
      прокси.
    </p>
    <dl class="kv">
      <dt>Тип</dt>
      <dd>MTProto</dd>
      <dt>Сервер</dt>
      <dd class="mono">127.0.0.1</dd>
      <dt>Порт</dt>
      <dd class="mono">{cfg?.port ?? tg?.port ?? 1443}</dd>
      <dt>Секрет</dt>
      <dd class="mono secret">
        <span>{secretShown}</span>
        {#if cfg?.secret}
          <button type="button" class="link" onclick={() => copy(secretShown, "Секрет")}>Копировать</button>
        {/if}
      </dd>
    </dl>
    <p class="hint">
      У TgWsProxy свой значок в трее — в его меню те же действия. ZapretGUI при выходе прокси не останавливает.
    </p>
  </section>

  <section class="card small" aria-labelledby="tg-set">
    <h2 id="tg-set">Настройки прокси</h2>
    <div class="grid">
      <label for="tg-port">Порт</label>
      <input id="tg-port" class="mono" class:bad={!portOk} inputmode="numeric" bind:value={form.port} />
      <label for="tg-secret">Секрет</label>
      <div class="with-btn">
        <input id="tg-secret" class="mono" class:bad={!!form.secret && !secretOk} spellcheck="false" bind:value={form.secret} />
        <button type="button" class="link" onclick={newSecret}>Новый</button>
      </div>
      <label for="tg-dc">DC → IP</label>
      <div>
        <textarea id="tg-dc" class="mono" class:bad={!!dcBad} rows="2" spellcheck="false" bind:value={form.dc}></textarea>
        <span class="hint sm">
          {dcBad
            ? `«${dcBad}» — нужен формат DC:IP`
            : "Не грузятся фото и видео? Оставьте только 4:149.154.167.220, не поможет — очистите поле"}
        </span>
      </div>
    </div>
    <div class="line">
      <div class="text">
        <span class="label">Резерв через Cloudflare</span>
        <span class="hint sm">Если WebSocket к Telegram недоступен</span>
      </div>
      <Switch label="Резерв через Cloudflare" checked={form.cfproxy} onchange={(v) => (form.cfproxy = v)} />
    </div>
    <div class="line">
      <div class="text">
        <span class="label">Запускать вместе с ZapretGUI</span>
      </div>
      <Switch
        label="Запускать вместе с ZapretGUI"
        checked={!!app.settings?.tgAutostart}
        onchange={(v) => patchSettings({ tgAutostart: v })}
      />
    </div>
    <div class="foot">
      <button type="button" class="link" disabled={!installed} onclick={() => api.openPath("tglog")}>Открыть лог</button>
      <Button
        variant="primary"
        height={34}
        disabled={!dirty || !portOk || !secretOk || !!dcBad || saving || !installed}
        title={!installed ? "Сначала скачайте TgWsProxy" : undefined}
        onclick={save}>{saving ? "Сохранение…" : running ? "Сохранить и перезапустить" : "Сохранить"}</Button
      >
    </div>
  </section>
</div>

<style>
  .head-status {
    font-size: 12px;
    color: var(--faint);
  }
  .link {
    border: none;
    background: none;
    padding: 0;
    color: var(--accent);
    font-size: 13px;
  }
  .link:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .link.upd {
    font-size: 12px;
    color: var(--warn);
  }
  .card {
    border-radius: 16px;
    background: var(--card);
    border: 1px solid var(--line);
    flex-shrink: 0;
  }
  .state {
    display: flex;
    align-items: center;
    gap: 28px;
    padding: 24px 28px;
  }
  .state-body {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .titles {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .title-row {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 4px;
    background: var(--mute);
    flex-shrink: 0;
  }
  .dot.on {
    background: var(--accent);
  }
  .title {
    font-size: 22px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }
  .sub {
    font-size: 13px;
    color: var(--dim);
    font-variant-numeric: tabular-nums;
  }
  .note {
    font-size: 12px;
    color: var(--faint);
  }
  .note.warn {
    color: var(--warn);
  }
  .note .link {
    font-size: 12px;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  .bar {
    height: 6px;
    max-width: 380px;
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
  .row2 {
    display: flex;
    gap: 16px;
    align-items: stretch;
  }
  .small {
    flex: 1 1 0;
    min-width: 0;
    padding: 18px 20px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  h2 {
    font-size: 14px;
    font-weight: 600;
  }
  .hint {
    margin: 0;
    font-size: 12px;
    line-height: 1.5;
    color: var(--faint);
  }
  .hint.sm {
    display: block;
    margin-top: 4px;
  }
  .kv {
    margin: 0;
    display: grid;
    grid-template-columns: 70px 1fr;
    row-gap: 8px;
    font-size: 13px;
  }
  dt {
    color: var(--dim);
  }
  dd {
    margin: 0;
    min-width: 0;
  }
  .mono {
    font-family: var(--font-mono);
    font-size: 12px;
  }
  .secret {
    display: flex;
    gap: 10px;
    align-items: baseline;
  }
  .secret span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .secret .link {
    font-size: 12px;
    flex-shrink: 0;
  }
  .grid {
    display: grid;
    grid-template-columns: 70px 1fr;
    gap: 10px 12px;
    align-items: start;
  }
  .grid label {
    font-size: 13px;
    color: var(--dim);
    padding-top: 8px;
  }
  input,
  textarea {
    width: 100%;
    box-sizing: border-box;
    padding: 7px 10px;
    border-radius: 9px;
    border: 1px solid var(--line2);
    background: var(--raised);
    color: var(--tx);
    resize: vertical;
  }
  input:focus,
  textarea:focus {
    outline: none;
    border-color: var(--accent);
  }
  .bad {
    border-color: var(--err);
  }
  .with-btn {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .with-btn .link {
    font-size: 12px;
    flex-shrink: 0;
  }
  .line {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .text {
    display: flex;
    flex-direction: column;
  }
  .label {
    font-size: 13px;
  }
  .foot {
    margin-top: auto;
    padding-top: 4px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .foot .link {
    font-size: 12px;
  }
  @media (max-width: 860px) {
    .row2 {
      flex-direction: column;
    }
  }
</style>
