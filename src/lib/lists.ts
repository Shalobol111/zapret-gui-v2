// Строки списков: проверка, подсчёт записей, разбор вставки из буфера.

import type { ListKind } from "./types";

const LABEL = "[a-z0-9_](?:[a-z0-9_-]{0,61}[a-z0-9_])?";
/** Домен hostlist zapret; `^` в начале — без поддоменов (как в list-exclude.txt). */
const DOMAIN = new RegExp(`^\\^?(?:${LABEL}\\.)*${LABEL}$`, "i");

function ipv4(s: string): boolean {
  const parts = s.split(".");
  return parts.length === 4 && parts.every((p) => /^\d{1,3}$/.test(p) && Number(p) <= 255);
}

function ipv6(s: string): boolean {
  return /^[0-9a-f:]+$/i.test(s) && s.includes(":") && (s.match(/::/g)?.length ?? 0) <= 1 && s.split(":").length <= 8;
}

/** IP, подсеть (`/24`) или диапазон IPv4 (`1.2.3.4-1.2.3.10`). */
function ipEntry(s: string): boolean {
  const range = s.split("-");
  if (range.length === 2) return ipv4(range[0]) && ipv4(range[1]);
  const [addr, prefix, extra] = s.split("/");
  if (extra !== undefined) return false;
  if (ipv4(addr)) return prefix === undefined || (/^\d{1,2}$/.test(prefix) && Number(prefix) <= 32);
  if (ipv6(addr)) return prefix === undefined || (/^\d{1,3}$/.test(prefix) && Number(prefix) <= 128);
  return false;
}

export function isComment(line: string): boolean {
  return line.trimStart().startsWith("#");
}

/** true — строка пустая, комментарий или корректная запись. */
export function isValid(line: string, kind: ListKind): boolean {
  const t = line.trim();
  if (!t || t.startsWith("#")) return true;
  return kind === "domains" ? DOMAIN.test(t) : ipEntry(t);
}

export function stats(text: string, kind: ListKind): { entries: number; invalid: number } {
  let entries = 0;
  let invalid = 0;
  for (const line of text.split("\n")) {
    const t = line.trim();
    if (!t || t.startsWith("#")) continue;
    entries++;
    if (!(kind === "domains" ? DOMAIN.test(t) : ipEntry(t))) invalid++;
  }
  return { entries, invalid };
}

/** «https://www.Example.com/path» → «example.com». */
function toDomain(token: string): string {
  let s = token.trim().toLowerCase();
  s = s.replace(/^[a-z][a-z0-9+.-]*:\/\//, "");
  s = s.split(/[/?#]/)[0].split("@").pop() ?? "";
  s = s.replace(/:\d+$/, "").replace(/\.$/, "");
  return s.replace(/^www\./, "");
}

/**
 * Записи из буфера обмена: по одной на строку (или через пробелы/запятые),
 * ссылки превращаются в домены, дубликаты и уже имеющиеся записи пропускаются.
 */
export function parsePaste(clip: string, kind: ListKind, existing: string): { added: string[]; skipped: number } {
  const have = new Set(
    existing
      .split("\n")
      .map((l) => l.trim().toLowerCase())
      .filter((l) => l && !l.startsWith("#")),
  );
  const added: string[] = [];
  let skipped = 0;
  for (const raw of clip.split(/[\s,;]+/)) {
    if (!raw || raw.startsWith("#")) continue;
    const entry = kind === "domains" ? toDomain(raw) : raw.trim();
    if (!entry || !isValid(entry, kind) || have.has(entry)) {
      skipped++;
      continue;
    }
    have.add(entry);
    added.push(entry);
  }
  return { added, skipped };
}
