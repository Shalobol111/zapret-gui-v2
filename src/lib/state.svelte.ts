// Общее состояние интерфейса (Svelte 5 runes).

import * as api from "./api";
import type { LogEntry, Mode, Screen, Settings, Status, TestRun, ThemeChoice } from "./types";

export const app = $state({
  ready: false,
  isAdmin: true,
  appVersion: "",
  systemTheme: "dark" as Mode,
  settings: null as Settings | null,
  screen: "main" as Screen,
  status: null as Status | null,
  journal: [] as LogEntry[],
  tests: null as TestRun | null,
  /** В «Списках» есть несохранённые правки — точка в меню и вопрос при закрытии окна. */
  listsDirty: false,
  /** Быстрый подбор запущен «Починить Discord» — он сам применит итог, тост TestsCard не нужен. */
  pickOwner: null as "discord" | "wizard" | null,
});

const JOURNAL_LIMIT = 1000;

export function bypassOn(): boolean {
  return app.status?.bypass.state === "on";
}

export function themeChoice(): ThemeChoice {
  return app.settings?.theme ?? "system";
}

export function effectiveMode(): Mode {
  const choice = themeChoice();
  return choice === "system" ? app.systemTheme : choice;
}

/** Частичное обновление настроек: в интерфейсе — сразу, на диске — следом. */
export async function patchSettings(patch: Partial<Settings>) {
  if (!app.settings) return;
  const prev = { ...app.settings };
  Object.assign(app.settings, patch);
  try {
    app.settings = await api.setSettings(patch);
  } catch (e) {
    console.error("Не удалось сохранить настройки", e);
    app.settings = prev;
  }
}

export function setTheme(choice: ThemeChoice) {
  if (app.settings?.theme === choice) return;
  return patchSettings({ theme: choice });
}

/** Кнопка солнце/луна: переключает на противоположную текущей (из «Системной» тоже). */
export function toggleTheme() {
  return setTheme(effectiveMode() === "dark" ? "light" : "dark");
}

export function go(screen: Screen) {
  app.screen = screen;
}

/** Подписки на события ядра; вызывается один раз при старте. */
export async function connect() {
  await api.onStatus((s) => {
    app.status = s;
  });
  await api.onLog((e) => {
    if (app.journal.some((x) => x.id === e.id)) return;
    app.journal.push(e);
    if (app.journal.length > JOURNAL_LIMIT) app.journal.splice(0, app.journal.length - JOURNAL_LIMIT);
  });
  await api.onTests((r) => {
    app.tests = r;
  });
  const [status, journal, tests] = await Promise.all([api.statusGet(), api.journalGet(), api.testsLast()]);
  app.status = status;
  app.tests ??= tests;
  // События могли прийти раньше снимка — объединяем по id.
  const known = new Set(journal.map((e) => e.id));
  app.journal = [...journal, ...app.journal.filter((e) => !known.has(e.id))];
}
