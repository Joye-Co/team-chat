// Shared CSS and small render helpers for every mockup.
import { avatarPalettes, cssVars, dark, light } from "./tokens.mjs";
import { loadIcons } from "./icons.mjs";

const ICONS = loadIcons();

export function icon(name, size = 16, extra = "") {
  const body = ICONS[name];
  if (!body) throw new Error(`missing icon ${name}`);
  return `<svg class="i ${extra}" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${body}</svg>`;
}

export const esc = (s) =>
  String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/** Monogram: the last two characters of a Chinese name, the first letters of a Latin name. */
export function monogram(name, size = 32, bot = false) {
  const n = name.trim();
  const chars = [...n];
  if (/^[一-鿿]+$/.test(n)) {
    if (size < 28 || chars.length === 1) return chars[0];
    return bot ? chars.slice(0, 2).join("") : chars.slice(-2).join("");
  }
  const words = n.split(/\s+/).filter((w) => /\p{L}/u.test(w));
  if (words.length === 0) return chars[0] ?? "?";
  const first = [...words[0]].find((c) => /\p{L}/u.test(c)) ?? "";
  const last = words.length > 1 ? ([...words[words.length - 1]].find((c) => /\p{L}/u.test(c)) ?? "") : "";
  if (/[一-鿿]/.test(first) || /[一-鿿]/.test(last)) return first.toUpperCase();
  return (first + last).toUpperCase();
}

/**
 * Avatar. People are circles, bots are squircles with a small mark.
 * opts: { size, bot, presence: 'online' | 'offline' | undefined, palette, ring }
 */
export function avatar(name, opts = {}) {
  const { size = 32, bot = false, presence, palette = "slate", mode = "light", ring = "var(--content)" } = opts;
  const [bg, fg] = avatarPalettes[palette][mode];
  const mono = monogram(name, size, bot);
  const isCjk = /[一-鿿]/.test(mono);
  const fs = Math.round(size * (isCjk ? (mono.length > 1 ? 0.34 : 0.44) : 0.38));
  const radius = bot ? Math.round(size * 0.28) : size / 2;
  const mark = bot
    ? `<span class="botmark" style="--s:${Math.max(11, Math.round(size * 0.42))}px;--ring:${ring}">${icon("bot", Math.max(8, Math.round(size * 0.28)))}</span>`
    : "";
  const dot =
    presence === "online"
      ? `<span class="presence" style="--s:${Math.max(8, Math.round(size * 0.3))}px;--ring:${ring}"></span>`
      : "";
  return `<span class="avatar" style="width:${size}px;height:${size}px;border-radius:${radius}px;background:${bg};color:${fg};font-size:${fs}px">${esc(mono)}${mark}${dot}</span>`;
}

export function groupTile(name, palette, mode, size = 20) {
  const [bg, fg] = avatarPalettes[palette][mode];
  const ch = [...name.trim()][0];
  return `<span class="gtile" style="width:${size}px;height:${size}px;background:${bg};color:${fg};font-size:${Math.round(size * 0.55)}px">${esc(ch)}</span>`;
}

const STATUS = {
  in_progress: { cls: "st-progress", label: () => "进行中" },
  waiting_answer: { cls: (you) => (you ? "st-you" : "st-other"), label: (you, n) => (you ? "等你回答" : n ? `等${n}回答` : "等回答") },
  waiting_review: { cls: (you) => (you ? "st-you" : "st-other"), label: (you, n) => (you ? "等你审" : n ? `等${n}审` : "等审") },
  blocked: { cls: "st-blocked", label: () => "卡住了" },
  done: { cls: "st-done", label: () => "完成" },
};

/** Topic status chip. `you`: the viewer is in waitingOn. `name`: the member waited on. */
export function chip(status, { you = false, name = "" } = {}) {
  if (!status || status === "open") return "";
  const s = STATUS[status];
  const cls = typeof s.cls === "function" ? s.cls(you) : s.cls;
  const dot = cls === "st-other" ? `<span class="wdot"></span>` : "";
  let label = s.label(you, name);
  let inner = `<span class="chip-t">${esc(label)}</span>`;
  if (!you && name && cls === "st-other") {
    const verb = status === "waiting_review" ? "审" : "回答";
    inner = `<span class="chip-t">等<span class="chip-name">${esc(name)}</span>${verb}</span>`;
  }
  return `<span class="chip ${cls}" title="${esc(label)}">${dot}${inner}</span>`;
}

export function badge(count, { mention = false, max = 999 } = {}) {
  if (!count) return "";
  const text = count > max ? `${max}+` : String(count);
  if (mention) return `<span class="badge mention">@${text}</span>`;
  return `<span class="badge">${text}</span>`;
}

export function kbd(...keys) {
  return keys.map((k) => `<span class="kbd">${esc(k)}</span>`).join("");
}

export function trafficLights() {
  return `<div class="traffic"><span class="tl r"></span><span class="tl y"></span><span class="tl g"></span></div>`;
}

export function page({ title, theme, body, width = 1440, height = 900, extraCss = "" }) {
  return `<!doctype html>
<html lang="zh-CN" data-theme="${theme}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=${width}">
<title>${esc(title)}</title>
<style>
${BASE_CSS}
${extraCss}
</style>
</head>
<body style="width:${width}px;height:${height}px">
${body}
</body>
</html>
`;
}

export const BASE_CSS = `
:root[data-theme="light"] {
  ${cssVars(light)}
  color-scheme: light;
}
:root[data-theme="dark"] {
  ${cssVars(dark)}
  color-scheme: dark;
}
* { box-sizing: border-box; margin: 0; padding: 0; }
html, body { overflow: hidden; }
body {
  font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", sans-serif;
  font-size: 13px; line-height: 18px; color: var(--text-1);
  background: var(--content);
  -webkit-font-smoothing: antialiased;
  font-feature-settings: "kern";
  font-kerning: normal;
}
.num, time, .badge, .count, .kbd { font-variant-numeric: tabular-nums; }
.mono, code, pre { font-family: ui-monospace, "SF Mono", Menlo, monospace; }
.i { flex-shrink: 0; display: block; }
.row { display: flex; align-items: center; }
.col { display: flex; flex-direction: column; }
.grow { flex: 1 1 auto; min-width: 0; }
.ell { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; }
.t2 { color: var(--text-2); }
.t3 { color: var(--text-3); }

/* Desktop backdrop (stands in for the wallpaper the window blurs) */
.desk {
  position: absolute; inset: 0;
  background:
    radial-gradient(1200px 700px at 15% 10%, var(--desk-a), transparent 70%),
    radial-gradient(1000px 800px at 85% 90%, var(--desk-b), transparent 70%),
    linear-gradient(160deg, var(--desk-a), var(--desk-b));
}

/* Avatars */
.avatar {
  position: relative; flex-shrink: 0; display: inline-flex; align-items: center; justify-content: center;
  font-weight: 500; letter-spacing: 0; line-height: 1; user-select: none;
}
.botmark {
  position: absolute; right: calc(var(--s) * -0.22); bottom: calc(var(--s) * -0.22);
  width: var(--s); height: var(--s); border-radius: 50%;
  background: var(--bot-mark); color: #fff; display: flex; align-items: center; justify-content: center;
  box-shadow: 0 0 0 1.5px var(--ring);
}
.presence {
  position: absolute; right: -1px; bottom: -1px; width: var(--s); height: var(--s); border-radius: 50%;
  background: var(--online); box-shadow: 0 0 0 1.5px var(--ring);
}
.gtile {
  flex-shrink: 0; display: inline-flex; align-items: center; justify-content: center; border-radius: 5.5px;
  font-weight: 600; line-height: 1;
}

/* Chips and badges */
.chip {
  display: inline-flex; align-items: center; gap: 4px; height: 18px; padding: 0 6px; border-radius: 5px;
  font-size: 11px; line-height: 14px; font-weight: 500; white-space: nowrap; flex-shrink: 0; max-width: 168px;
}
.chip .chip-t { display: inline-flex; min-width: 0; overflow: hidden; }
.chip .chip-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; max-width: 104px; }
.chip.st-progress { background: var(--st-progress-bg); color: var(--st-progress); }
.chip.st-you { background: var(--st-you-bg); color: var(--st-you); font-weight: 600; }
.chip.st-other { background: var(--st-other-bg); color: var(--st-other); }
.chip.st-blocked { background: var(--st-blocked-bg); color: var(--st-blocked); }
.chip.st-done { background: var(--st-done-bg); color: var(--st-done); }
.wdot { width: 6px; height: 6px; border-radius: 50%; background: var(--st-wait-dot); flex-shrink: 0; }
.badge {
  display: inline-flex; align-items: center; justify-content: center; min-width: 18px; height: 18px; padding: 0 6px;
  border-radius: 9px; background: var(--badge-bg); color: var(--badge-text); font-size: 11px; font-weight: 500; flex-shrink: 0;
}
.badge.mention { background: var(--badge-mention-bg); color: var(--on-accent); font-weight: 600; }
.udot { width: 8px; height: 8px; border-radius: 50%; background: var(--accent); flex-shrink: 0; }
.kbd {
  display: inline-flex; align-items: center; justify-content: center; min-width: 18px; height: 18px; padding: 0 4px;
  border-radius: 4px; font-size: 11px; color: var(--text-2); background: var(--fill-field);
  font-family: -apple-system, BlinkMacSystemFont, sans-serif;
}

/* Window chrome */
.traffic { display: flex; gap: 8px; }
.tl { width: 12px; height: 12px; border-radius: 50%; box-shadow: inset 0 0 0 0.5px rgba(0,0,0,0.12); }
.tl.r { background: #ff5f57; } .tl.y { background: #febc2e; } .tl.g { background: #28c840; }
:root[data-theme="dark"] .tl { box-shadow: inset 0 0 0 0.5px rgba(255,255,255,0.10); }
.window-inactive .tl { background: var(--fill-pressed); }

/* Buttons (gpui-component Button, restyled) */
.btn {
  display: inline-flex; align-items: center; justify-content: center; gap: 6px; height: 28px; padding: 0 12px;
  border-radius: 7px; font-size: 13px; font-weight: 500; white-space: nowrap; border: 0; cursor: default;
  font-family: inherit; color: var(--text-1); background: var(--control); box-shadow: var(--shadow-control);
}
.btn.primary { background: var(--accent); color: var(--on-accent); box-shadow: 0 0 0 0.5px rgba(0,0,0,0.08), 0 1px 1.5px rgba(0,0,0,0.10); }
.btn.ghost { background: transparent; box-shadow: none; color: var(--text-2); padding: 0 6px; }
.btn.sm { height: 24px; padding: 0 10px; font-size: 12px; border-radius: 6px; }
.btn.lg { height: 34px; padding: 0 16px; font-size: 14px; border-radius: 9px; }
.iconbtn {
  width: 28px; height: 28px; border-radius: 7px; display: inline-flex; align-items: center; justify-content: center;
  color: var(--text-2); flex-shrink: 0;
}
.capsule {
  display: inline-flex; align-items: center; height: 32px; padding: 0 2px; border-radius: 16px;
  background: var(--toolbar-capsule); box-shadow: var(--shadow-capsule);
}
.capsule .iconbtn { width: 32px; height: 28px; border-radius: 14px; }

/* Fields */
.field {
  display: flex; align-items: center; gap: 6px; height: 28px; padding: 0 8px; border-radius: 7px;
  background: var(--fill-field); color: var(--text-3); font-size: 13px;
}
.input {
  display: flex; align-items: center; height: 32px; padding: 0 10px; border-radius: 8px; background: var(--control);
  box-shadow: var(--shadow-control); font-size: 13px; color: var(--text-1);
}
.input.focus { box-shadow: 0 0 0 0.5px var(--accent), 0 0 0 3.5px var(--focus-ring); }
.caret { display: inline-block; width: 1.5px; height: 17px; background: var(--accent); vertical-align: -3px; margin-left: 1px; }

/* Popovers and menus */
.popover {
  position: absolute; background: var(--elevated); border-radius: 10px; box-shadow: var(--shadow-popover);
  padding: 5px;
}
.menu-head { font-size: 11px; font-weight: 600; color: var(--text-3); padding: 6px 8px 4px; }
.menu-item { display: flex; align-items: center; gap: 8px; height: 36px; padding: 0 8px; border-radius: 6px; }
.menu-item.active { background: var(--accent); color: var(--on-accent); }
.menu-item.active .t2, .menu-item.active .t3 { color: rgba(255,255,255,0.78); }
.menu-item.active .kbd { background: rgba(255,255,255,0.2); color: #fff; }

/* Segmented control (gpui-component TabBar segmented) */
.seg { display: inline-flex; padding: 2px; border-radius: 8px; background: var(--fill-field); gap: 0; }
.seg > span { height: 22px; padding: 0 10px; display: inline-flex; align-items: center; gap: 4px; border-radius: 6px; font-size: 12px; color: var(--text-2); font-weight: 500; }
.seg > span.on { background: var(--control); color: var(--text-1); box-shadow: var(--shadow-control); }
:root[data-theme="dark"] .seg > span.on { background: rgba(255,255,255,0.16); }
`;
