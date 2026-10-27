// SVG-иконки дословно из макета (совпадают с Lucide). Рисуются через <Icon>,
// цвет — currentColor, поэтому анимируются вместе с темой.

export type IconName =
  | "logo"
  | "power"
  | "activity"
  | "list"
  | "settings"
  | "shield"
  | "shieldAlert"
  | "sun"
  | "moon"
  | "chevronDown"
  | "play"
  | "send"
  | "globe";

export interface IconDef {
  /** Элементы внутри <svg viewBox="0 0 24 24">. */
  body: string;
  /** Заливка вместо обводки (иконка «play» на кнопке «Проверить»). */
  filled?: boolean;
}

export const ICONS: Record<IconName, IconDef> = {
  // Lucide «globe» — VPN.
  globe: {
    body: '<circle cx="12" cy="12" r="10"/><path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20"/><path d="M2 12h20"/>',
  },
  // Lucide «send» — бумажный самолётик (Telegram).
  send: {
    body:
      '<path d="M14.536 21.686a.5.5 0 0 0 .937-.024l6.5-19a.496.496 0 0 0-.635-.635l-19 6.5a.5.5 0 0 0-.024.937l7.93 3.18a2 2 0 0 1 1.112 1.11z"/>' +
      '<path d="m21.854 2.147-10.94 10.939"/>',
  },
  logo: { body: '<path d="M4 7h16M4 17h16"/><path d="M17 4l3 3-3 3"/><path d="M7 14l-3 3 3 3"/>' },
  power: { body: '<path d="M12 3v8"/><path d="M6.3 6.8a8 8 0 1 0 11.4 0"/>' },
  activity: { body: '<path d="M3 12h4l3-8 4 16 3-8h4"/>' },
  list: { body: '<path d="M8 6h13M8 12h13M8 18h13"/><path d="M3 6h.01M3 12h.01M3 18h.01"/>' },
  settings: {
    body:
      '<circle cx="12" cy="12" r="3"/>' +
      '<path d="M12 2v3M12 19v3M2 12h3M19 12h3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M4.9 19.1L7 17M17 7l2.1-2.1"/>',
  },
  shield: { body: '<path d="M12 3l8 4v5c0 5-3.5 8-8 9-4.5-1-8-4-8-9V7z"/><path d="M9 12l2 2 4-4"/>' },
  // Без прав: тот же щит, вместо галочки — восклицательный знак.
  shieldAlert: { body: '<path d="M12 3l8 4v5c0 5-3.5 8-8 9-4.5-1-8-4-8-9V7z"/><path d="M12 8v4M12 16h.01"/>' },
  sun: {
    body:
      '<circle cx="12" cy="12" r="4"/>' +
      '<path d="M12 2v2M12 20v2M2 12h2M20 12h2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>',
  },
  moon: { body: '<path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z"/>' },
  chevronDown: { body: '<path d="M6 9l6 6 6-6"/>' },
  play: { body: '<path d="M7 4l13 8-13 8z"/>', filled: true },
};
