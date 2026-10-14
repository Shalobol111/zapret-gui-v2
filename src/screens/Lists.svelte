<script lang="ts">
  import Button from "../components/Button.svelte";
  import ListEditor from "../components/ListEditor.svelte";
  import PageHeader from "../components/PageHeader.svelte";
  import * as api from "../lib/api";
  import { parsePaste, stats } from "../lib/lists";
  import { offerRestart } from "../lib/restart";
  import { app, go } from "../lib/state.svelte";
  import { askDialog, plural, toast } from "../lib/ui.svelte";
  import type { ListData, ListInfo } from "../lib/types";

  let index = $state<ListInfo[]>([]);
  let current = $state<ListData | null>(null); // как файл лежит на диске
  let text = $state(""); // текст в редакторе
  let editor = $state<ReturnType<typeof ListEditor>>();
  let saving = $state(false);
  let loading = $state(false);

  const valid = $derived(!!app.status?.zapret.valid);
  const dirty = $derived(!!current && text !== current.text);
  $effect(() => {
    app.listsDirty = dirty;
  });

  // Подсчёт записей — с небольшой задержкой: ipset-all.txt большой.
  let counts = $state({ entries: 0, invalid: 0 });
  let countTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const t = text;
    const kind = current?.kind ?? "domains";
    clearTimeout(countTimer);
    countTimer = setTimeout(() => (counts = stats(t, kind)), 120);
  });

  // Папка zapret или режим IPSet поменялись — перечитываем список файлов.
  // $derived со строкой срабатывает только при реальной смене, а не на каждый статус.
  const indexKey = $derived(valid ? `${app.status?.zapret.path}|${app.status?.filters?.ipset}` : "");
  $effect(() => {
    if (!indexKey) {
      index = [];
      current = null;
      text = "";
      return;
    }
    api.listsIndex().then((list) => {
      index = list;
      if (!current) open(list[0].id, true);
      else {
        const same = list.find((l) => l.id === current!.id);
        // IPSet переключили, а правок нет — открываем актуальный файл.
        if (same && same.file !== current.file && !dirty) open(same.id, true);
      }
    });
  });

  async function open(id: string, force = false) {
    if (!force && current?.id === id) return;
    if (!force && dirty) {
      const r = await askDialog({
        title: "Сохранить изменения?",
        text: `В файле ${current!.file} есть несохранённые изменения.`,
        ok: "Сохранить",
        alt: "Не сохранять",
      });
      if (r === "cancel") return;
      if (r === "ok" && !(await save())) return;
    }
    loading = true;
    try {
      const d = await api.listRead(id);
      current = d;
      text = d.text;
      editor?.load(d.text);
    } catch (e) {
      toast(api.errorText(e), "err");
    } finally {
      loading = false;
    }
  }

  async function save(force = false): Promise<boolean> {
    if (!current || saving) return false;
    if (!dirty && !force) return true;
    saving = true;
    try {
      const d = await api.listSave(current.id, text, current.hash, force);
      current = d;
      // Ядро могло дополнить текст (например, «domain.example.abc» в пустой список).
      if (d.text !== text) {
        text = d.text;
        editor?.replace(d.text);
      }
      offerRestart(`Сохранено: ${d.file}`);
      return true;
    } catch (e) {
      if (api.errorText(e) === "conflict") {
        saving = false;
        const r = await askDialog({
          title: "Файл изменён другой программой",
          text: `${current.file} изменился на диске после открытия. Перезаписать его вашей версией или загрузить заново (ваши правки пропадут)?`,
          ok: "Перезаписать",
          alt: "Загрузить заново",
        });
        if (r === "ok") return save(true);
        if (r === "alt") await open(current.id, true);
        return false;
      }
      toast(api.errorText(e), "err");
      return false;
    } finally {
      saving = false;
    }
  }

  function revert() {
    if (current) editor?.replace(current.text);
  }

  async function paste() {
    if (!current) return;
    try {
      const clip = await api.readClipboard();
      const { added, skipped } = parsePaste(clip, current.kind, text);
      if (!added.length) {
        toast(skipped ? "Все записи из буфера уже есть в списке или некорректны" : "Буфер обмена пуст", "warn");
        return;
      }
      editor?.append(added);
      const unit = current.kind === "domains" ? ["домен", "домена", "доменов"] : ["адрес", "адреса", "адресов"];
      toast(
        `Добавлено ${added.length} ${plural(added.length, unit as [string, string, string])}` +
          (skipped ? `, пропущено ${skipped}` : ""),
        "ok",
      );
    } catch (e) {
      toast(api.errorText(e), "err");
    }
  }

  // Ctrl+S, когда фокус не в редакторе (в редакторе его ловит CodeMirror).
  function onKey(e: KeyboardEvent) {
    if (app.screen !== "lists" || !e.ctrlKey || e.key.toLowerCase() !== "s" || e.shiftKey || e.altKey) return;
    e.preventDefault();
    save();
  }

  const groups = $derived([
    { title: "Домены", items: index.filter((l) => l.kind === "domains") },
    { title: "IP-адреса", items: index.filter((l) => l.kind === "ips") },
  ]);
  const unit = $derived(
    (current?.kind === "ips" ? ["адрес", "адреса", "адресов"] : ["домен", "домена", "доменов"]) as [string, string, string],
  );
</script>

<svelte:window onkeydown={onKey} />

<PageHeader title="Списки">
  <span class="status">Изменения применяются после перезапуска стратегии</span>
</PageHeader>

<div class="body">
  <section class="files" aria-label="Файлы">
    {#each groups as g (g.title)}
      <span class="group">{g.title}</span>
      {#each g.items as it (it.id)}
        <button type="button" class="file" class:active={current?.id === it.id} aria-current={current?.id === it.id} onclick={() => open(it.id)}>
          <span class="ftitle">{it.title}{#if current?.id === it.id && dirty}<span class="fdirty" aria-label="изменён"></span>{/if}</span>
          <span class="fname">{it.file}</span>
        </button>
      {/each}
    {/each}
  </section>

  <section class="editor-card" aria-label="Редактор">
    {#if valid}
      <div class="head">
        <span class="name" title={current?.file}>{current?.file ?? "…"}</span>
        {#if dirty}<span class="changed">● изменён</span>{/if}
        <span class="grow"></span>
        <Button height={32} size={12} disabled={!current || loading} onclick={paste}><span class="long">Вставить из буфера</span><span class="short">Вставить</span></Button>
        <Button height={32} size={12} disabled={!dirty} onclick={revert}>Отменить</Button>
        <Button variant="primary" height={32} size={12} disabled={!dirty || saving} onclick={() => save()}
          >{saving ? "Сохранение…" : "Сохранить"}</Button
        >
      </div>
      {#if current?.note}<div class="note">{current.note}</div>{/if}
      <ListEditor bind:this={editor} kind={current?.kind ?? "domains"} onchange={(t) => (text = t)} onsave={() => save()} />
      <div class="foot">
        <span>{counts.entries} {plural(counts.entries, unit)}</span>
        {#if counts.invalid}<span class="warn">{counts.invalid} {plural(counts.invalid, ["строка", "строки", "строк"])} с ошибкой</span>{/if}
        <span class="enc">UTF-8 · {current?.crlf === false ? "LF" : "CRLF"}</span>
        <span class="grow"></span>
        <span>Ctrl+S — сохранить</span>
      </div>
    {:else}
      <div class="empty">
        Укажите папку zapret, чтобы редактировать списки ·
        <button type="button" class="link" onclick={() => go("settings")}>Открыть Настройки</button>
      </div>
    {/if}
  </section>
</div>

<style>
  .status {
    font-size: 12px;
    color: var(--faint);
  }
  .body {
    flex-grow: 1;
    min-height: 0;
    display: flex;
    gap: 16px;
  }
  .files {
    width: 220px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 10px;
    border-radius: 16px;
    background: var(--card);
    border: 1px solid var(--line);
    overflow: auto;
  }
  .group {
    padding: 6px 10px 8px;
    font-size: 11px;
    color: var(--faint);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .group:not(:first-child) {
    padding-top: 14px;
  }
  .file {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    padding: 9px 10px;
    border-radius: 9px;
    border: none;
    background: transparent;
    color: var(--tx2);
    text-align: left;
  }
  .file.active {
    background: var(--raised);
    color: var(--tx);
  }
  .ftitle {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
  }
  .file.active .ftitle {
    font-weight: 500;
  }
  .fdirty {
    width: 6px;
    height: 6px;
    border-radius: 3px;
    background: var(--warn);
  }
  .fname {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--faint);
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .editor-card {
    container-type: inline-size;
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
    gap: 8px;
    padding: 12px 14px 12px 18px;
    border-bottom: 1px solid var(--line);
  }
  .name {
    font-family: var(--font-mono);
    font-size: 13px;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .changed {
    font-size: 12px;
    color: var(--warn);
    white-space: nowrap;
  }
  .grow {
    flex-grow: 1;
  }
  .note {
    padding: 8px 18px;
    font-size: 12px;
    line-height: 1.4;
    color: var(--dim);
    background: var(--editor);
    border-bottom: 1px solid var(--line);
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 16px;
    height: 36px;
    flex-shrink: 0;
    padding: 0 18px;
    border-top: 1px solid var(--line);
    font-size: 12px;
    color: var(--faint);
    white-space: nowrap;
  }
  .foot .warn {
    color: var(--warn);
  }
  .short {
    display: none;
  }
  /* Узкий редактор (окно 780–900 px): короче подпись кнопки, без пометки кодировки. */
  @container (max-width: 440px) {
    .long,
    .enc {
      display: none;
    }
    .short {
      display: inline;
    }
  }
  @media (max-width: 899px) {
    .files {
      width: 184px;
    }
  }
  .empty {
    margin: auto;
    padding: 24px;
    font-size: 13px;
    color: var(--dim);
    text-align: center;
  }
  .link {
    border: none;
    background: none;
    padding: 0;
    color: var(--accent);
    font-size: 13px;
  }
</style>
