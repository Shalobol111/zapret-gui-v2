import "./styles/base.css";
import { mount } from "svelte";
import App from "./App.svelte";
import * as api from "./lib/api";
import { app, connect, effectiveMode, go } from "./lib/state.svelte";
import { confirmDialog } from "./lib/ui.svelte";

const nextFrame = () => new Promise<void>((r) => requestAnimationFrame(() => r()));

/** Несохранённые правки в «Списках» — спросить перед выходом из программы. */
async function canQuit(): Promise<boolean> {
  if (!app.listsDirty) return true;
  await api.showMainWindow();
  return confirmDialog({
    title: "Закрыть без сохранения?",
    text: "В «Списках» есть несохранённые изменения — они пропадут.",
    ok: "Закрыть",
    danger: true,
  });
}

async function start() {
  const boot = await api.bootstrap();
  app.settings = boot.settings;
  app.isAdmin = boot.isAdmin;
  app.systemTheme = boot.systemTheme;
  app.appVersion = boot.appVersion;
  // Только в браузере (заглушка): ?screen=check — для скриншотов вёрстки.
  if (!api.inTauri) {
    const s = new URLSearchParams(location.search).get("screen");
    if (s === "main" || s === "check" || s === "lists" || s === "telegram" || s === "settings") app.screen = s;
  }

  const root = document.documentElement;
  root.classList.toggle("light", effectiveMode() === "light");
  root.classList.toggle("dark", effectiveMode() === "dark");

  await api.onSystemTheme((mode) => {
    app.systemTheme = mode;
  });
  await connect();
  // Крестик: в трей (обход работает дальше) или выход из программы.
  await api.onCloseRequested(async () => {
    if (app.settings?.minimizeToTray) {
      void api.trayHint(); // первый раз — уведомление «работает в трее»
      return "hide";
    }
    return (await canQuit()) ? "close" : "cancel";
  });
  await api.onNavGoto((s) => {
    if (s === "main" || s === "check" || s === "lists" || s === "telegram" || s === "settings") go(s);
  });
  await api.onQuitRequest(async () => {
    if (await canQuit()) await api.quit();
  });

  mount(App, { target: document.getElementById("app")! });
  app.ready = true;

  // Окно показываем, когда шрифты загружены и первый кадр нарисован без анимации.
  await document.fonts.ready;
  await nextFrame();
  await nextFrame();
  root.classList.remove("no-transition");
  // Автозапуск (--minimized) — сразу в трей.
  if (!boot.startHidden) await api.showWindow();
}

start().catch((e) => {
  console.error("Ошибка запуска интерфейса", e);
  document.documentElement.classList.remove("no-transition");
  api.showWindow();
});
