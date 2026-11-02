// Shared CSS and render helpers for the v2 mockups. Every page is self-contained: system fonts,
// inline Lucide SVG icons (the same set gpui-kit ships), and the jojo-friends character SVGs from
// apps/desktop/assets/characters embedded as data URIs.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { cssVars, dark, light, palettes } from "./tokens.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
export const REPO = join(HERE, "../../../..");
const ICONS = JSON.parse(readFileSync(join(HERE, "icons.json"), "utf8"));

export const esc = (s) => String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

export function icon(name, size = 16, cls = "") {
  const body = ICONS[name];
  if (!body) throw new Error(`missing icon ${name}`);
  return `<svg class="i ${cls}" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${body}</svg>`;
}

const SIZES = [16, 20, 24, 32, 48, 64, 128];
const charCache = new Map();
/** A bot's character at its live status (idle, working, needs-input, success, error, offline). */
export function character(id, status = "idle", size = 32) {
  const file = SIZES.find((s) => s >= size) ?? 128;
  const key = `${id}/${status}-${file}`;
  if (!charCache.has(key)) {
    const svg = readFileSync(join(REPO, `apps/desktop/assets/characters/${id}/${status}-${file}.svg`));
    charCache.set(key, `data:image/svg+xml;base64,${svg.toString("base64")}`);
  }
  return `<img class="char" width="${size}" height="${size}" src="${charCache.get(key)}" alt="">`;
}

/** Initials: one character for a CJK name, two letters for a Latin name (DESIGN §7). */
export function initials(name) {
  const n = name.trim().replace(/^\p{Extended_Pictographic}\s*/u, "");
  const chars = [...n];
  if (/[一-鿿]/.test(chars[0] ?? "")) return chars[0];
  const words = n.split(/\s+/).filter(Boolean);
  const a = [...(words[0] ?? "?")][0];
  const b = words.length > 1 ? [...words[words.length - 1]][0] : "";
  return (a + b).toUpperCase();
}

/** Member avatar. People: circle with initials. Bots: their character at its status. */
export function avatar(m, size, mode, { presence = true, ring = "var(--content)" } = {}) {
  let inner;
  if (m.char) inner = character(m.char, m.state ?? "idle", size);
  else {
    const [bg, fg] = palettes[m.palette ?? "slate"][mode];
    const fs = Math.round(size * (initials(m.name).length > 1 ? 0.38 : 0.45));
    inner = `<span class="mono" style="background:${bg};color:${fg};font-size:${fs}px">${esc(initials(m.name))}</span>`;
  }
  const dotSize = Math.max(7, Math.round(size * 0.28));
  const dot = presence && !m.char && m.online !== undefined
    ? `<span class="pdot ${m.online ? "on" : "off"}" style="width:${dotSize}px;height:${dotSize}px;--ring:${ring}"></span>` : "";
  return `<span class="av ${m.char ? "bot" : "person"}" style="width:${size}px;height:${size}px">${inner}${dot}</span>`;
}

export function groupTile(name, palette, mode, size = 20) {
  const [bg, fg] = palettes[palette][mode];
  const ch = ([...name.replace(/^\p{Extended_Pictographic}\s*/u, "").trim()][0] ?? "?").toUpperCase();
  return `<span class="gtile" style="width:${size}px;height:${size}px;background:${bg};color:${fg};font-size:${Math.round(size * 0.55)}px">${esc(ch)}</span>`;
}

const STATUS = {
  in_progress: ["circle-dot", "info", "进行中"],
  waiting_answer: ["circle-question-mark", "you", "回答"],
  waiting_review: ["eye", "you", "审"],
  blocked: ["circle-alert", "danger", "卡住了"],
  done: ["circle-check", "success", "完成"],
};
/**
 * Status chip (DESIGN §2.5): one shape, tint variants, no border. `you` = the viewer is in
 * waitingOn. Topic rows pass `row: true`: 进行中 is the default there and shows no chip.
 */
export function chip(status, { you = false, name = "", row = false } = {}) {
  if (!status || status === "open") return "";
  if (row && status === "in_progress") return "";
  const [glyph, tone, word] = STATUS[status];
  let label = word;
  let t = tone;
  if (status === "waiting_answer" || status === "waiting_review") {
    if (you) label = `等你${word}`;
    else { t = "neutral"; label = name && !row ? `等<span class="cn">${esc(name)}</span>${word}` : `等${word}`; }
  }
  return `<span class="chip t-${t}">${icon(glyph, 12)}<span class="cl">${label}</span></span>`;
}

export const kbd = (k) => `<span class="kbd">${k}</span>`;

const BASE_CSS = `
*{box-sizing:border-box;margin:0;padding:0}
html,body{width:var(--w);height:var(--h);overflow:hidden}
body{font-family:-apple-system,BlinkMacSystemFont,"SF Pro Text","PingFang SC","Hiragino Sans GB",sans-serif;font-size:13px;line-height:18px;color:var(--text-1);background:var(--content);-webkit-font-smoothing:antialiased;font-variant-numeric:tabular-nums}
.i{flex:none;display:block}
.char{display:block}
.av{position:relative;flex:none;display:inline-flex}
.av .mono{width:100%;height:100%;border-radius:50%;display:flex;align-items:center;justify-content:center;font-weight:600;letter-spacing:0}
.pdot{position:absolute;right:-1px;bottom:-1px;border-radius:50%;box-shadow:0 0 0 2px var(--ring)}
.pdot.on{background:var(--online)}
.pdot.off{background:var(--content);box-shadow:inset 0 0 0 1.5px var(--text-3),0 0 0 2px var(--ring)}
.gtile{flex:none;display:inline-flex;align-items:center;justify-content:center;border-radius:6px;font-weight:600}
.chip{display:inline-flex;align-items:center;gap:4px;height:20px;padding:0 7px 0 6px;border-radius:6px;font-size:12px;line-height:16px;font-weight:500;white-space:nowrap;max-width:168px;flex:none}
.chip .cl{overflow:hidden;text-overflow:ellipsis;display:inline-flex;min-width:0}
.chip .cn{display:inline-block;max-width:96px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.t-info{background:var(--info-bg);color:var(--info-text)}
.t-you{background:var(--you-bg);color:var(--you-text);font-weight:600}
.t-you .i{color:var(--you-glyph)}
.t-danger{background:var(--danger-bg);color:var(--danger-text)}
.t-success{background:var(--success-bg);color:var(--success-text)}
.t-neutral{background:var(--field);color:var(--text-2)}
.kbd{display:inline-flex;align-items:center;justify-content:center;min-width:18px;height:18px;padding:0 4px;border-radius:4px;background:var(--field);color:var(--text-2);font-size:11px;line-height:14px;font-weight:500}
.badge{display:inline-flex;align-items:center;justify-content:center;min-width:18px;height:18px;padding:0 5px;border-radius:9px;background:var(--badge);color:#fff;font-size:11px;line-height:14px;font-weight:600;flex:none}
.btn{display:inline-flex;align-items:center;justify-content:center;gap:6px;height:28px;padding:0 12px;border-radius:8px;font-size:13px;font-weight:500;white-space:nowrap;flex:none}
.btn.primary{background:var(--accent);color:var(--on-accent)}
.btn.secondary{background:var(--content);color:var(--text-1);box-shadow:0 0 0 0.5px var(--border-strong),0 1px 1px rgba(0,0,0,0.04)}
.btn.ghost{color:var(--text-2)}
.btn.lg{height:36px;padding:0 16px;font-size:14px}
.ib{width:28px;height:28px;border-radius:8px;display:inline-flex;align-items:center;justify-content:center;color:var(--text-2);flex:none}
.ib.hov{background:var(--hover);color:var(--text-1)}
.ell{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;min-width:0}
.t2{color:var(--text-2)} .t3{color:var(--text-3)}
.focus{box-shadow:0 0 0 2px var(--content),0 0 0 4px var(--focus)!important}
`;

export function page({ title, mode, width, height, css, body }) {
  const t = mode === "dark" ? dark : light;
  return `<!doctype html>
<html lang="zh-Hans" data-theme="${mode}"><head><meta charset="utf-8">
<meta name="viewport" content="width=${width}">
<title>${esc(title)}</title>
<style>:root{${cssVars(t)}--w:${width}px;--h:${height}px;color-scheme:${mode}}${BASE_CSS}${css}</style>
</head><body>${body}</body></html>
`;
}
