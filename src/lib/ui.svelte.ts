// Уведомления (Toast) и диалоги подтверждения — общие для всех экранов.

export type ToastKind = "info" | "ok" | "warn" | "err";

export interface ToastItem {
  id: number;
  kind: ToastKind;
  text: string;
  action?: { label: string; run: () => void };
}

export interface ConfirmOptions {
  title: string;
  text: string;
  ok: string;
  /** Третья кнопка, например «Не сохранять». */
  alt?: string;
  cancel?: string;
  danger?: boolean;
}

export type Choice = "ok" | "alt" | "cancel";

interface ConfirmState extends ConfirmOptions {
  resolve: (v: Choice) => void;
}

export const ui = $state({
  toasts: [] as ToastItem[],
  confirm: null as ConfirmState | null,
});

let nextId = 1;

export function toast(text: string, kind: ToastKind = "info", action?: ToastItem["action"]) {
  const id = nextId++;
  ui.toasts.push({ id, kind, text, action });
  if (ui.toasts.length > 3) ui.toasts.shift();
  setTimeout(() => dismiss(id), action ? 9000 : 5000);
}

export function dismiss(id: number) {
  const i = ui.toasts.findIndex((t) => t.id === id);
  if (i >= 0) ui.toasts.splice(i, 1);
}

/** Диалог с двумя или тремя кнопками. Esc и клик мимо — «cancel». */
export function askDialog(opts: ConfirmOptions): Promise<Choice> {
  ui.confirm?.resolve("cancel");
  return new Promise((resolve) => {
    ui.confirm = { ...opts, resolve };
  });
}

export async function confirmDialog(opts: ConfirmOptions): Promise<boolean> {
  return (await askDialog(opts)) === "ok";
}

export function closeConfirm(result: Choice | boolean) {
  const c = ui.confirm;
  ui.confirm = null;
  c?.resolve(result === true ? "ok" : result === false ? "cancel" : result);
}

/** Склонение: plural(3, ["предупреждение", "предупреждения", "предупреждений"]). */
export function plural(n: number, forms: [string, string, string]): string {
  const m10 = n % 10;
  const m100 = n % 100;
  if (m10 === 1 && m100 !== 11) return forms[0];
  if (m10 >= 2 && m10 <= 4 && (m100 < 12 || m100 > 14)) return forms[1];
  return forms[2];
}

/** 2537 с → «00:42:17». */
export function duration(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(Math.floor(s / 3600))}:${p(Math.floor((s % 3600) / 60))}:${p(s % 60)}`;
}
