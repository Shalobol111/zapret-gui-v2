<script lang="ts">
  import Button from "../components/Button.svelte";
  import PageHeader from "../components/PageHeader.svelte";
  import Segmented from "../components/Segmented.svelte";
  import Select from "../components/Select.svelte";
  import Switch from "../components/Switch.svelte";
  import * as api from "../lib/api";
  import { offerRestart } from "../lib/restart";
  import { app, bypassOn, patchSettings, setTheme, themeChoice } from "../lib/state.svelte";
  import { askDialog, confirmDialog, plural, toast } from "../lib/ui.svelte";
  import type { AutopilotMode, FakeKind, Fakes, Settings, ThemeChoice, ZapretInfo } from "../lib/types";

  const needAdmin = "Нужны права администратора";
  const fail = (e: unknown) => toast(api.errorText(e), "err");

  // ── Папка zapret ─────────────────────────────────────────────
  let path = $state(app.settings?.zapretPath ?? "");
  let preview = $state<ZapretInfo | null>(null); // проверка введённого, пока не сохранено
  let checking = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  // Путь поменялся снаружи (автопоиск при старте, «Обзор…», установка) — подтягиваем в поле.
  // Источник — статус ядра: настройки в интерфейсе загружены до автопоиска.
  let lastSaved = $state(app.settings?.zapretPath ?? "");
  $effect(() => {
    const saved = app.status?.zapret.path ?? app.settings?.zapretPath ?? "";
    if (saved !== lastSaved) {
      lastSaved = saved;
      path = saved;
      preview = null;
      if (app.settings) app.settings.zapretPath = saved || null;
    }
  });

  const saved = $derived(app.status?.zapret);
  const shown = $derived(preview ?? (path.trim() === (saved?.path ?? "") ? saved : null));
  /** В поле не та папка, что сохранена: версия и обновление ниже относятся к сохранённой — их прячем. */
  const editing = $derived(path.trim() !== (saved?.path ?? ""));
  /** Куда скачать zapret, если в выбранной папке его нет: в неё саму (пустая) или в подпапку. */
  const downloadDest = $derived(
    shown && !shown.valid && shown.path ? (shown.empty ? shown.path : `${shown.path.replace(/[\\/]+$/, "")}\\zapret`) : null,
  );
  const strategiesWord = (n: number) => plural(n, ["стратегия", "стратегии", "стратегий"]);

  function onInput() {
    clearTimeout(timer);
    preview = null;
    const p = path.trim();
    if (!p) return;
    timer = setTimeout(async () => {
      checking = true;
      try {
        preview = await api.zapretInspect(p);
      } finally {
        checking = false;
      }
    }, 400);
  }

  async function commit(p = path.trim()) {
    if (!p || p === lastSaved) return;
    try {
      const info = await api.zapretSetPath(p);
      lastSaved = info.path ?? "";
      path = lastSaved;
      preview = null;
      if (app.settings) app.settings.zapretPath = info.path;
      toast(`Папка zapret сохранена: ${info.strategies.length} ${strategiesWord(info.strategies.length)}`, "ok");
    } catch {
      // Причину показываем под полем (и там же — «Скачать zapret сюда»), а не всплывающим окном.
      preview = await api.zapretInspect(p).catch(() => null);
    }
  }

  async function browse() {
    const p = await api.pickFolder();
    if (!p) return;
    path = p;
    clearTimeout(timer);
    checking = true;
    try {
      const info = await api.zapretInspect(p);
      if (info.valid) await commit(p);
      else preview = info;
    } finally {
      checking = false;
    }
  }

  function restoreSaved() {
    clearTimeout(timer);
    path = saved?.path ?? "";
    preview = null;
  }

  async function detect() {
    const found = await api.zapretDetect();
    if (!found.length) {
      toast("Сборка zapret не найдена в типичных папках — укажите её через «Обзор…» или скачайте", "warn");
      return;
    }
    path = found[0];
    await commit(found[0]);
  }

  // ── Обновление и установка ───────────────────────────────────
  const upd = $derived(app.status?.update);
  const applying = $derived(upd?.state === "applying");
  let busyUpdate = $state(false);

  const updText = $derived.by(() => {
    if (!upd) return { text: "", tone: "faint" };
    switch (upd.state) {
      case "checking":
        return { text: "Проверка обновлений…", tone: "faint" };
      case "available":
        return { text: `Доступна ${upd.latest} — списки доменов сохранятся`, tone: "warn" };
      case "current":
        return { text: "Установлена последняя версия", tone: "faint" };
      case "error":
        return { text: `Не удалось проверить: ${upd.error}`, tone: "err" };
      case "applying":
        return { text: `${upd.stage ?? "Обновление"}${upd.progress != null ? ` · ${upd.progress} %` : "…"}`, tone: "faint" };
      default:
        return { text: "Проверка обновлений при запуске выключена", tone: "faint" };
    }
  });

  async function checkUpdate() {
    busyUpdate = true;
    try {
      const r = await api.updateCheck();
      if (r.state === "current") toast("Установлена последняя версия zapret", "ok");
    } catch (e) {
      fail(e);
    } finally {
      busyUpdate = false;
    }
  }

  async function applyUpdate() {
    const svc = app.status?.service;
    const ok = await confirmDialog({
      title: `Обновить zapret до ${upd?.latest}?`,
      text:
        "Архив будет скачан с GitHub и проверен по контрольной сумме. Обход и служба будут остановлены, файлы заменены.\n" +
        "Свои списки доменов, IPSet и настройки фильтров сохранятся." +
        (svc && svc.state !== "not_installed" ? `\nСлужба будет переустановлена со стратегией «${svc.strategy}».` : ""),
      ok: "Обновить",
    });
    if (!ok) return;
    busyUpdate = true;
    try {
      toast(await api.updateApply(), "ok");
    } catch (e) {
      fail(e);
    } finally {
      busyUpdate = false;
    }
  }

  async function download() {
    const r = await askDialog({
      title: "Скачать zapret?",
      text:
        "Последняя версия zapret-discord-youtube будет скачана с GitHub и распакована в C:\\zapret.\n" +
        "Путь без кириллицы и пробелов — так советует Flowseal.",
      ok: "Скачать",
      alt: "Другая папка…",
    });
    if (r === "cancel") return;
    let dest = "C:\\zapret";
    if (r === "alt") {
      const p = await api.pickFolder();
      if (!p) return;
      const info = await api.zapretInspect(p);
      // Выбрали папку, где zapret уже есть, — просто используем её.
      if (info.valid) return commit(p);
      dest = info.empty ? p : `${p.replace(/[\\/]+$/, "")}\\zapret`;
    }
    await install(dest);
  }

  /** Кнопка под полем: в выбранной папке нет zapret — скачать туда (или в подпапку). */
  async function downloadHere() {
    if (!downloadDest) return;
    const ok = await confirmDialog({
      title: "Скачать zapret?",
      text:
        `Последняя версия zapret-discord-youtube будет скачана с GitHub и распакована в ${downloadDest}.` +
        (shown?.empty ? "" : "\nВыбранная папка не пуста, поэтому zapret будет в подпапке «zapret»."),
      ok: "Скачать",
    });
    if (ok) await install(downloadDest);
  }

  async function install(dest: string) {
    busyUpdate = true;
    try {
      toast(await api.zapretDownload(dest), "ok");
    } catch (e) {
      fail(e);
    } finally {
      busyUpdate = false;
    }
  }

  // ── Поведение ────────────────────────────────────────────────
  let busySwitch = $state<string | null>(null);

  async function toggle(
    key: "autostart" | "minimizeToTray" | "checkUpdates" | "notifications" | "startBypassOnLaunch",
    value: boolean,
  ) {
    if (!app.settings) return;
    busySwitch = key;
    try {
      let next: Settings;
      if (key === "autostart") next = await api.autostartSet(value);
      else if (key === "checkUpdates") next = await api.checkUpdatesSet(value);
      else if (key === "notifications") next = await api.notificationsSet(value);
      else if (key === "startBypassOnLaunch") next = await api.setSettings({ startBypassOnLaunch: value });
      else next = await api.setSettings({ minimizeToTray: value });
      app.settings = next;
    } catch (e) {
      fail(e);
    } finally {
      busySwitch = null;
    }
  }

  // ── Автопилот ────────────────────────────────────────────────
  const autopilotModes: { value: AutopilotMode; label: string }[] = [
    { value: "auto", label: "Автоматически" },
    { value: "warn", label: "Предупреждать" },
    { value: "off", label: "Выкл" },
  ];
  const autopilotHint = $derived(
    ({
      auto: "Раз в 15 минут проверяет Discord и YouTube. При сбое сам подберёт и включит рабочую стратегию",
      warn: "Раз в 15 минут проверяет Discord и YouTube. При сбое пришлёт уведомление с кнопкой «Подобрать»",
      off: "Не следит, работает ли стратегия",
    } as const)[app.settings?.autopilot ?? "warn"],
  );
  /** «Проверить сейчас»: итог — уведомлением, когда автопилот закончит проверку. */
  let apSince = $state<number | null>(null);
  async function checkNow() {
    if (!bypassOn()) return toast("Обход выключен — проверять нечего", "info");
    apSince = Date.now() - 1000;
    try {
      await api.autopilotCheck();
    } catch (e) {
      apSince = null;
      fail(e);
    }
  }
  $effect(() => {
    const a = app.status?.autopilot;
    if (apSince === null || !a || a.phase !== "idle" || (a.lastCheck ?? 0) < apSince) return;
    apSince = null;
    if (a.outcome === "ok") toast("Discord и YouTube открываются", "ok");
    else if (a.outcome === "offline") toast("Нет подключения к интернету", "warn");
    else toast(a.message ?? "Найден сбой", "warn");
  });
  const apBusy = $derived(apSince !== null || (app.status?.autopilot.phase ?? "idle") !== "idle");

  const themes: { value: ThemeChoice; label: string }[] = [
    { value: "system", label: "Системная" },
    { value: "dark", label: "Тёмная" },
    { value: "light", label: "Светлая" },
  ];
  const themeHint = $derived(
    themeChoice() === "system"
      ? `Как в Windows — сейчас ${app.systemTheme === "light" ? "светлая" : "тёмная"}`
      : "Переключается плавно, без перезапуска",
  );

  // ── Дополнительно ────────────────────────────────────────────
  let fakes = $state<Fakes | null>(null);
  let ipsetN = $state<number | null>(null);
  let tcp = $state("");
  let udp = $state("");
  let busyExtra = $state<string | null>(null);

  const extrasKey = $derived(saved?.valid ? `${saved.path}|${app.status?.filters?.ipset}` : "");
  $effect(() => {
    if (!extrasKey) {
      fakes = null;
      ipsetN = null;
      return;
    }
    api.fakesGet().then((f) => (fakes = f)).catch(() => (fakes = null));
    api.ipsetCount().then((n) => (ipsetN = n)).catch(() => (ipsetN = null));
  });
  // Порты — из статуса; поле не перетираем, пока пользователь его правит.
  const filters = $derived(app.status?.filters);
  let portsTouched = $state(false);
  $effect(() => {
    if (filters && !portsTouched) {
      tcp = filters.tcpPorts;
      udp = filters.udpPorts;
    }
  });
  const portsChanged = $derived(!!filters && (tcp.trim() !== filters.tcpPorts || udp.trim() !== filters.udpPorts));
  const testing = $derived(!!app.status?.testsRunning);

  async function applyPorts() {
    busyExtra = "ports";
    try {
      await api.gamePortsSet(tcp.trim(), udp.trim());
      portsTouched = false;
      offerRestart("Порты Game Filter изменены", true);
    } catch (e) {
      fail(e);
    } finally {
      busyExtra = null;
    }
  }

  async function setFake(kind: FakeKind, name: string) {
    busyExtra = kind;
    try {
      await api.fakesSet(kind, name);
      fakes = await api.fakesGet();
      // Фейк — содержимое файла, а не аргумент: службе достаточно перезапуска.
      offerRestart("Активный фейк заменён");
    } catch (e) {
      fail(e);
    } finally {
      busyExtra = null;
    }
  }

  async function updateIpset() {
    busyExtra = "ipset";
    try {
      ipsetN = await api.ipsetUpdate();
      offerRestart(`IPSet-список обновлён: ${ipsetN} записей`);
    } catch (e) {
      fail(e);
    } finally {
      busyExtra = null;
    }
  }

  async function removeServices() {
    const ok = await confirmDialog({
      title: "Удалить службы?",
      text: "Служба zapret и драйвер WinDivert будут остановлены и удалены. Обход выключится и не запустится после перезагрузки.",
      ok: "Удалить",
      danger: true,
    });
    if (!ok) return;
    busyExtra = "remove";
    try {
      await api.serviceRemove();
      toast("Службы удалены", "ok");
    } catch (e) {
      fail(e);
    } finally {
      busyExtra = null;
    }
  }

  const fakeOptions = $derived((fakes?.files ?? []).map((f) => ({ value: f, label: f })));

  // ── Обновление ZapretGUI ─────────────────────────────────────
  const appUpd = $derived(app.status?.appUpdate);
  let appUpdBusy = $state(false);
  const appUpdText = $derived(
    ({
      idle: "",
      checking: "· проверка…",
      current: "· установлена последняя версия",
      available: `· доступна ${appUpd?.latest ?? ""}`,
      downloading: "· скачивание обновления…",
      installing: "· установка…",
      error: `· не удалось проверить: ${appUpd?.error ?? ""}`,
    } as const)[appUpd?.state ?? "idle"],
  );
  async function checkAppUpdate() {
    appUpdBusy = true;
    try {
      const r = await api.appUpdateCheck();
      if (r.state === "available") app.updatePrompt = true;
      else if (r.state === "current") toast("Установлена последняя версия ZapretGUI", "ok");
      else if (r.state === "error") toast(r.error ?? "Не удалось проверить обновления", "err");
    } catch (e) {
      fail(e);
    } finally {
      appUpdBusy = false;
    }
  }

  // ── Настройки GUI ────────────────────────────────────────────
  async function exportSettings() {
    const p = await api.pickSettingsFile("save");
    if (p) await api.settingsExport(p).then(() => toast("Настройки экспортированы", "ok"), fail);
  }
  async function importSettings() {
    const p = await api.pickSettingsFile("open");
    if (!p) return;
    try {
      app.settings = await api.settingsImport(p);
      toast("Настройки импортированы", "ok");
    } catch (e) {
      fail(e);
    }
  }
  async function resetSettings() {
    const ok = await confirmDialog({
      title: "Сбросить настройки программы?",
      text: "Тема, трей, автозапуск, журнал и выбор стратегий для проверки вернутся к значениям по умолчанию. Папка zapret и сам zapret не изменятся.",
      ok: "Сбросить",
      danger: true,
    });
    if (!ok) return;
    try {
      app.settings = await api.settingsReset();
      toast("Настройки сброшены", "ok");
    } catch (e) {
      fail(e);
    }
  }
</script>

<PageHeader title="Настройки" />

<section class="card" aria-labelledby="zp">
  <h2 id="zp">zapret</h2>
  <div class="path-block" class:sep={saved?.valid && !editing}>
    <label for="zpath">Папка с service.bat</label>
    <div class="path-row">
      <input
        id="zpath"
        type="text"
        spellcheck="false"
        placeholder="C:\zapret"
        bind:value={path}
        oninput={onInput}
        onkeydown={(e) => e.key === "Enter" && commit()}
        onblur={() => shown?.valid && commit()}
      />
      <Button height={38} onclick={browse}>Обзор…</Button>
    </div>
    {#if checking}
      <span class="msg faint">Проверка…</span>
    {:else if !path.trim()}
      <span class="msg faint">
        Папка не выбрана ·
        <button type="button" class="link" onclick={detect}>Найти автоматически</button>
      </span>
    {:else if shown?.valid}
      <span class="msg ok"><span class="mdot"></span>Найдено {shown.strategies.length} {strategiesWord(shown.strategies.length)}</span>
      {#if shown.warning}<span class="msg warn"><span class="mdot"></span>{shown.warning}</span>{/if}
    {:else if shown}
      <span class="msg err"><span class="mdot"></span>{shown.error}</span>
      <div class="download">
        <span class="hint">
          {#if downloadDest}Скачать последнюю версию с GitHub в <span class="mono">{downloadDest}</span>{/if}
          {#if saved?.valid}
            {#if downloadDest}·{/if}
            <button type="button" class="link" onclick={restoreSaved}>Вернуть прежнюю папку</button>
          {/if}
        </span>
        {#if downloadDest}
          <Button variant="primary" height={34} disabled={busyUpdate || applying} onclick={downloadHere}
            >{applying ? updText.text : shown.empty ? "Скачать zapret сюда" : "Скачать в подпапку zapret"}</Button
          >
        {/if}
      </div>
    {/if}
    {#if !saved?.valid && !path.trim()}
      <div class="download">
        <span class="hint">Нет zapret? Программа скачает последнюю версию с GitHub.</span>
        <Button variant="primary" height={34} disabled={busyUpdate || applying} onclick={download}
          >{applying ? updText.text : "Скачать zapret"}</Button
        >
      </div>
    {/if}
  </div>
  {#if saved?.valid && !editing}
    <div class="row last">
      <div class="text">
        <span class="label">Версия {saved.version ?? "неизвестна"}</span>
        <span class="hint {updText.tone}">{updText.text}</span>
      </div>
      {#if upd?.state === "available" || applying}
        <Button
          variant="primary"
          height={34}
          disabled={busyUpdate || applying || !app.isAdmin || testing}
          title={app.isAdmin ? undefined : needAdmin}
          onclick={applyUpdate}>{applying ? "Обновление…" : "Обновить"}</Button
        >
      {:else}
        <Button height={34} disabled={busyUpdate || upd?.state === "checking"} onclick={checkUpdate}
          >{upd?.state === "checking" ? "Проверка…" : "Проверить"}</Button
        >
      {/if}
    </div>
  {/if}
</section>

<section class="card" aria-labelledby="bh">
  <h2 id="bh">Поведение</h2>
  <div class="row">
    <div class="text">
      <span class="label">Запускать вместе с Windows</span>
      <span class="hint">Через Планировщик задач, без запроса UAC</span>
    </div>
    <Switch
      label="Запускать вместе с Windows"
      checked={!!app.settings?.autostart}
      disabled={busySwitch !== null || !app.isAdmin}
      title={app.isAdmin ? undefined : needAdmin}
      onchange={(v) => toggle("autostart", v)}
    />
  </div>
  <div class="row">
    <div class="text">
      <span class="label">Включать обход при запуске программы</span>
      <span class="hint">Последней стратегией. Не нужно, если установлена служба</span>
    </div>
    <Switch
      label="Включать обход при запуске программы"
      checked={!!app.settings?.startBypassOnLaunch}
      disabled={busySwitch !== null}
      onchange={(v) => toggle("startBypassOnLaunch", v)}
    />
  </div>
  <div class="row">
    <div class="text">
      <span class="label">Сворачивать в трей при закрытии</span>
      <span class="hint">Обход продолжит работать в фоне</span>
    </div>
    <Switch
      label="Сворачивать в трей при закрытии"
      checked={!!app.settings?.minimizeToTray}
      disabled={busySwitch !== null}
      onchange={(v) => toggle("minimizeToTray", v)}
    />
  </div>
  <div class="row">
    <div class="text">
      <span class="label">Проверять обновления</span>
      <span class="hint">zapret, TgWsProxy и самой ZapretGUI — тихо, при запуске; сообщит только о новой версии</span>
    </div>
    <Switch
      label="Проверять обновления"
      checked={!!app.settings?.checkUpdates}
      disabled={busySwitch !== null}
      onchange={(v) => toggle("checkUpdates", v)}
    />
  </div>
  <div class="row">
    <div class="text">
      <span class="label">Автопилот</span>
      <span class="hint"
        >{autopilotHint} ·
        <button type="button" class="link" disabled={apBusy} onclick={checkNow}
          >{apBusy ? "Проверяю…" : "Проверить сейчас"}</button
        ></span
      >
    </div>
    <Segmented
      label="Автопилот"
      options={autopilotModes}
      value={app.settings?.autopilot ?? "warn"}
      onchange={(v) => patchSettings({ autopilot: v })}
    />
  </div>
  <div class="row">
    <div class="text">
      <span class="label">Уведомления Windows</span>
      <span class="hint"
        >Если обход остановится, проверка закончится в трее или выйдет новая версия zapret ·
        <button type="button" class="link" onclick={() => api.notifyTest().catch(fail)}>Отправить тестовое</button></span
      >
    </div>
    <Switch
      label="Уведомления Windows"
      checked={!!app.settings?.notifications}
      disabled={busySwitch !== null}
      onchange={(v) => toggle("notifications", v)}
    />
  </div>
  <div class="row last">
    <div class="text">
      <span class="label">Тема</span>
      <span class="hint">{themeHint}</span>
    </div>
    <Segmented label="Тема" options={themes} value={themeChoice()} onchange={setTheme} />
  </div>
</section>

<section class="card" aria-labelledby="adv">
  <h2 id="adv">Дополнительно</h2>
  {#if saved?.valid}
    <div class="row">
      <div class="text">
        <span class="label">Порты Game Filter</span>
        <span class="hint">Для режима «Игры» на Главной</span>
      </div>
      <div class="ports">
        <label class="port"><span>TCP</span><input type="text" spellcheck="false" bind:value={tcp} oninput={() => (portsTouched = true)} /></label>
        <label class="port"><span>UDP</span><input type="text" spellcheck="false" bind:value={udp} oninput={() => (portsTouched = true)} /></label>
        {#if portsChanged}
          <Button height={32} size={12} disabled={busyExtra !== null || testing} onclick={applyPorts}>Применить</Button>
        {/if}
      </div>
    </div>
    <div class="row">
      <div class="text">
        <span class="label">Фейк для Discord UDP</span>
        <span class="hint">Пакет-обманка для голосовых каналов (Replace active fakes)</span>
      </div>
      <div class="sel">
        <Select
          options={fakeOptions}
          value={fakes?.discord ?? null}
          placeholder="свой файл"
          disabled={!fakes || busyExtra !== null || testing}
          onchange={(v) => setFake("discord", v)}
        />
      </div>
    </div>
    <div class="row">
      <div class="text">
        <span class="label">Фейк для Game Filter UDP</span>
        <span class="hint">Пакет-обманка для игр</span>
      </div>
      <div class="sel">
        <Select
          options={fakeOptions}
          value={fakes?.game ?? null}
          placeholder="свой файл"
          disabled={!fakes || busyExtra !== null || testing}
          onchange={(v) => setFake("game", v)}
        />
      </div>
    </div>
    <div class="row">
      <div class="text">
        <span class="label">IPSet-список</span>
        <span class="hint"
          >{ipsetN != null ? `${ipsetN} ${plural(ipsetN, ["запись", "записи", "записей"])} · ` : ""}актуальный список IP из репозитория Flowseal</span
        >
      </div>
      <Button height={34} disabled={busyExtra !== null} onclick={updateIpset}>{busyExtra === "ipset" ? "Обновление…" : "Обновить список"}</Button>
    </div>
  {/if}
  <div class="row">
    <div class="text">
      <span class="label">Службы zapret и WinDivert</span>
      <span class="hint">Остановить и удалить службу и драйвер — как «Remove Services» в service.bat</span>
    </div>
    <Button
      variant="danger"
      height={34}
      disabled={!app.isAdmin || busyExtra !== null || testing}
      title={app.isAdmin ? undefined : needAdmin}
      onclick={removeServices}>{busyExtra === "remove" ? "Удаление…" : "Удалить службы"}</Button
    >
  </div>
  <div class="row last links">
    {#if saved?.valid}
      <button type="button" class="link" onclick={() => api.openPath("zapret").catch(fail)}>Папка zapret</button>
      <button type="button" class="link" onclick={() => api.openPath("results").catch(fail)}>Результаты проверок</button>
    {/if}
    <button type="button" class="link" onclick={() => api.openPath("logs").catch(fail)}>Журнал программы</button>
    <button type="button" class="link" onclick={() => api.openPath("releases").catch(fail)}>Релизы Flowseal</button>
  </div>
</section>

<section class="card gui" aria-label="Версия программы">
  <span class="label grow"
    >ZapretGUI {app.appVersion}
    <span class="hint inline {appUpd?.state === 'available' ? 'warn' : appUpd?.state === 'error' ? 'err' : ''}">{appUpdText}</span></span
  >
  {#if appUpd?.state === "available"}
    <Button variant="primary" height={34} onclick={() => (app.updatePrompt = true)}>Обновить до {appUpd.latest}</Button>
  {:else}
    <Button height={34} disabled={appUpd?.state === "checking" || appUpdBusy} onclick={checkAppUpdate}
      >{appUpd?.state === "checking" || appUpdBusy ? "Проверка…" : "Проверить"}</Button
    >
  {/if}
</section>

<section class="card gui" aria-label="Резервная копия">
  <span class="label grow">Настройки GUI</span>
  <Button height={34} onclick={exportSettings}>Экспорт</Button>
  <Button height={34} onclick={importSettings}>Импорт</Button>
  <Button variant="danger" height={34} onclick={resetSettings}>Сбросить</Button>
</section>

<style>
  .card {
    display: flex;
    flex-direction: column;
    border-radius: 16px;
    background: var(--card);
    border: 1px solid var(--line);
    flex-shrink: 0;
  }
  h2 {
    padding: 16px 20px 6px;
    font-size: 14px;
    font-weight: 600;
  }
  .path-block {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 6px 20px 16px;
  }
  .path-block.sep {
    border-bottom: 1px solid var(--line);
  }
  label {
    font-size: 12px;
    color: var(--dim);
  }
  .path-row {
    display: flex;
    gap: 8px;
  }
  input[type="text"] {
    flex-grow: 1;
    min-width: 0;
    height: 38px;
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
  .msg {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
  }
  .mdot {
    width: 6px;
    height: 6px;
    border-radius: 3px;
    background: currentColor;
    flex-shrink: 0;
  }
  .msg.ok {
    color: var(--accent);
  }
  .msg.warn,
  .hint.warn {
    color: var(--warn);
  }
  .msg.err,
  .hint.err {
    color: var(--err);
  }
  .msg.faint {
    color: var(--faint);
  }
  .download {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 8px;
  }
  .download .hint {
    flex-grow: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .mono {
    font-family: var(--font-mono);
    color: var(--dim);
  }
  .link {
    border: none;
    background: none;
    padding: 0;
    color: var(--accent);
    font-size: 12px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 10px 20px;
    min-height: 44px;
  }
  .row.last {
    padding-bottom: 16px;
  }
  .text {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .label {
    font-size: 14px;
  }
  .hint {
    font-size: 12px;
    color: var(--faint);
  }
  .ports {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .port {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .port input {
    width: 118px;
    flex-grow: 0;
    height: 32px;
    font-size: 12px;
  }
  .sel {
    width: 240px;
    flex-shrink: 0;
  }
  .links {
    gap: 18px;
  }
  .gui {
    flex-direction: row;
    align-items: center;
    gap: 8px;
    padding: 14px 20px;
  }
  .grow {
    flex-grow: 1;
  }
</style>
