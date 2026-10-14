// После изменения списков, фейков или портов работающий обход нужно перезапустить,
// а службу — перезапустить (списки) или переустановить (параметры в binPath).

import * as api from "./api";
import { app } from "./state.svelte";
import { toast } from "./ui.svelte";

/** `reinstall` — изменились аргументы winws (фейки, порты), а не содержимое списков. */
export function offerRestart(what: string, reinstall = false) {
  const b = app.status?.bypass;
  if (!b || b.state !== "on") {
    toast(`${what} — изменения вступят в силу при следующем запуске`, "ok");
    return;
  }
  const svc = app.status?.service;
  const fail = (e: unknown) => toast(api.errorText(e), "err");
  if (b.source === "service" && svc?.strategy) {
    const name = svc.strategy;
    toast(`${what} — ${reinstall ? "переустановите" : "перезапустите"} службу, чтобы применить`, "ok", {
      label: reinstall ? "Переустановить" : "Перезапустить",
      run: () =>
        (reinstall ? api.serviceInstall(name) : api.serviceStop().then(() => api.serviceStart())).catch(fail),
    });
  } else if (b.strategy) {
    const name = b.strategy;
    toast(`${what} — перезапустите обход, чтобы применить`, "ok", {
      label: "Перезапустить",
      run: () => api.bypassStart(name).catch(fail),
    });
  }
}
