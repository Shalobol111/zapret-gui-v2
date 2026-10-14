<script lang="ts">
  // Редактор списков на CodeMirror 6: номера строк (gutter), комментарии `#` цветом mute,
  // некорректные строки подчёркнуты волной warn, фон editor, моно 13/24 — по макету.
  // Виртуализация строк: ipset-all.txt на 30 тыс. строк открывается мгновенно.
  import { onDestroy, onMount } from "svelte";
  import { Compartment, EditorState, RangeSetBuilder, type Extension } from "@codemirror/state";
  import {
    Decoration,
    EditorView,
    ViewPlugin,
    drawSelection,
    keymap,
    lineNumbers,
    type DecorationSet,
    type ViewUpdate,
  } from "@codemirror/view";
  import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
  import { isComment, isValid } from "../lib/lists";
  import type { ListKind } from "../lib/types";

  let {
    kind,
    onchange,
    onsave,
  }: { kind: ListKind; onchange: (text: string) => void; onsave: () => void } = $props();

  let host: HTMLDivElement;
  let view: EditorView | undefined;
  const lineMarks = new Compartment();

  const comment = Decoration.line({ class: "cm-comment" });
  const invalid = Decoration.line({ class: "cm-invalid", attributes: { title: "Строка не похожа на запись списка" } });

  function marks(k: ListKind): Extension {
    return ViewPlugin.fromClass(
      class {
        decorations: DecorationSet;
        constructor(v: EditorView) {
          this.decorations = this.build(v);
        }
        update(u: ViewUpdate) {
          if (u.docChanged || u.viewportChanged) this.decorations = this.build(u.view);
        }
        build(v: EditorView): DecorationSet {
          const b = new RangeSetBuilder<Decoration>();
          for (const { from, to } of v.visibleRanges) {
            let pos = from;
            while (pos <= to) {
              const line = v.state.doc.lineAt(pos);
              if (isComment(line.text)) b.add(line.from, line.from, comment);
              else if (!isValid(line.text, k)) b.add(line.from, line.from, invalid);
              pos = line.to + 1;
            }
          }
          return b.finish();
        }
      },
      { decorations: (p) => p.decorations },
    );
  }

  const theme = EditorView.theme({
    "&": { height: "100%", background: "var(--editor)", color: "var(--tx2)", fontSize: "13px" },
    "&.cm-focused": { outline: "none" },
    ".cm-scroller": { fontFamily: "var(--font-mono)", lineHeight: "24px" },
    ".cm-content": { padding: "12px 0", caretColor: "var(--tx)" },
    ".cm-line": { padding: "0 16px" },
    ".cm-gutters": { background: "var(--editor)", color: "var(--gutter)", border: "none" },
    ".cm-lineNumbers .cm-gutterElement": { padding: "0 4px 0 18px", minWidth: "40px", textAlign: "right" },
    ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--tx)", borderLeftWidth: "2px" },
    ".cm-selectionBackground": { background: "var(--line2) !important" },
    ".cm-comment": { color: "var(--mute)" },
    ".cm-invalid": {
      textDecoration: "underline wavy var(--warn)",
      textDecorationSkipInk: "none",
      textUnderlineOffset: "5px",
    },
  });

  function makeState(doc: string): EditorState {
    return EditorState.create({
      doc,
      extensions: [
        lineNumbers(),
        history(),
        drawSelection(),
        EditorView.lineWrapping,
        keymap.of([
          { key: "Mod-s", preventDefault: true, run: () => (onsave(), true) },
          ...defaultKeymap,
          ...historyKeymap,
        ]),
        lineMarks.of(marks(kind)),
        theme,
        EditorView.updateListener.of((u) => {
          if (u.docChanged) onchange(u.state.doc.toString());
        }),
        EditorView.contentAttributes.of({ spellcheck: "false", "aria-label": "Содержимое списка" }),
      ],
    });
  }

  onMount(() => {
    view = new EditorView({ state: makeState(""), parent: host });
  });
  onDestroy(() => view?.destroy());

  // Тип списка сменился — пересчитываем подсветку некорректных строк.
  $effect(() => {
    const k = kind;
    view?.dispatch({ effects: lineMarks.reconfigure(marks(k)) });
  });

  /** Новый документ (открыт другой файл): история правок сбрасывается. */
  export function load(text: string) {
    view?.setState(makeState(text));
    view?.scrollDOM.scrollTo({ top: 0 });
  }

  /** Замена текста с сохранением истории (Ctrl+Z вернёт прежний). */
  export function replace(text: string) {
    if (!view) return;
    view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text } });
  }

  /** Дописать строки в конец и прокрутить к ним. */
  export function append(lines: string[]) {
    if (!view || !lines.length) return;
    const doc = view.state.doc;
    const prefix = doc.length && !doc.toString().endsWith("\n") ? "\n" : "";
    const insert = prefix + lines.join("\n") + "\n";
    view.dispatch({
      changes: { from: doc.length, insert },
      selection: { anchor: doc.length + insert.length },
      scrollIntoView: true,
    });
    view.focus();
  }

  export function focus() {
    view?.focus();
  }
</script>

<div class="editor" bind:this={host}></div>

<style>
  .editor {
    flex-grow: 1;
    min-height: 0;
    user-select: text;
    cursor: text;
  }
  .editor :global(.cm-editor) {
    height: 100%;
  }
</style>
