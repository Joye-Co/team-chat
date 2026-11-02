// Shared CSS and render helpers for the v2.1 mockups. Every page is self-contained: system fonts,
// inline Lucide SVG icons (the same set gpui-kit ships, plus `type`), and the jojo-friends
// character SVGs from apps/desktop/assets/characters embedded as data URIs.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { cssVars, dark, light } from "./tokens.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
export const REPO = join(HERE, "../../../..");
const ICONS = JSON.parse(readFileSync(join(HERE, "icons.json"), "utf8"));

export const esc = (s) => String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

export function icon(name, size = 16, cls = "") {
  const body = ICONS[name];
  if (!body) throw new Error(`missing icon ${name}`);
  return `<svg class="i ${cls}" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${body}</svg>`;
}

/** Removes the `<g>` element that encloses `at` (with its children) from an SVG string. */
function dropGroupAround(svg, at) {
  const start = svg.lastIndexOf("<g", at);
  if (start < 0) return svg;
  let depth = 0;
  const re = /<g[\s>]|<\/g>/g;
  re.lastIndex = start;
  for (let m = re.exec(svg); m; m = re.exec(svg)) {
    depth += m[0].startsWith("</") ? -1 : 1;
    if (depth === 0) return svg.slice(0, start) + svg.slice(m.index + 4);
  }
  return svg;
}

/**
 * Small-size character art (DESIGN §9.3): at 40 pt and below the status lamp at the top corner
 * and the question bubble read as notification badges, so they are removed. The face keeps the
 * expression; the status moves to the bottom-right pip.
 */
function smallArt(svg) {
  let s = svg;
  const glow = s.indexOf("-g-glow)\"");
  if (glow > 0) s = dropGroupAround(s, glow);
  const bubble = s.search(/fill="#ffffff" stroke="#[0-9a-f]{6}" stroke-linejoin="round"/);
  if (bubble > 0) s = dropGroupAround(s, bubble);
  const RING = '<g opacity="1" fill="none" stroke-linecap="round"';
  for (let ring = s.indexOf(RING); ring > 0; ring = s.indexOf(RING)) {
    const next = dropGroupAround(s, ring);
    if (next === s) break;
    s = next;
  }
  return s.replace(/<path transform="[^"]*"[^>]*d="M-13,-13 H13 L-13,13 H13"[^>]*>(<\/path>)?/g, "");
}

const SIZES = [16, 20, 24, 32, 48, 64, 128];
const charCache = new Map();
/** A bot's character at its live status (idle, working, needs-input, success, error, offline). */
export function character(id, status = "idle", size = 32) {
  const file = SIZES.find((s) => s >= size) ?? 128;
  const small = size <= 40;
  const key = `${id}/${status}-${file}-${small}`;
  if (!charCache.has(key)) {
    let svg = readFileSync(join(REPO, `apps/desktop/assets/characters/${id}/${status}-${file}.svg`), "utf8");
    if (small) svg = smallArt(svg);
    charCache.set(key, `data:image/svg+xml;base64,${Buffer.from(svg).toString("base64")}`);
  }
  return `<img class="char" width="${size}" height="${size}" src="${charCache.get(key)}" alt="">`;
}

/** Initials: one character for a CJK name, two letters for a Latin name (DESIGN §9.3). */
export function initials(name) {
  const n = name.trim().replace(/^\p{Extended_Pictographic}(‍\p{Extended_Pictographic})*\s*/u, "");
  const chars = [...n];
  if (/[一-鿿]/.test(chars[0] ?? "")) return chars[0];
  const words = n.split(/\s+/).filter(Boolean);
  const a = [...(words[0] ?? "?")][0];
  const b = words.length > 1 ? [...words[words.length - 1]][0] : "";
  return (a + b).toUpperCase();
}

/**
 * Member avatar. People: neutral circle with initials and a presence dot. Bots: their character;
 * a status pip only while the bot works or waits for someone.
 */
export function avatar(m, size, mode, { presence = true, ring = "var(--content)" } = {}) {
  let inner;
  if (m.char) inner = character(m.char, m.state ?? "idle", size);
  else {
    const fs = Math.round(size * (initials(m.name).length > 1 ? 0.38 : 0.44));
    inner = `<span class="mono" style="font-size:${fs}px">${esc(initials(m.name))}</span>`;
  }
  const d = size >= 32 ? 10 : 8;
  let dot = "";
  if (presence && !m.char && m.online !== undefined) dot = `<span class="pdot ${m.online ? "on" : "off"}" style="width:${d}px;height:${d}px;--ring:${ring}"></span>`;
  const pip = { working: "work", "needs-input": "wait", error: "err" }[m.state];
  if (presence && m.char && pip && size <= 36) dot = `<span class="pdot ${pip}" style="width:${d}px;height:${d}px;--ring:${ring}"></span>`;
  return `<span class="av ${m.char ? "bot" : "person"}" style="width:${size}px;height:${size}px">${inner}${dot}</span>`;
}

/** Group tile (switcher, dialogs): one neutral fill and the first character. */
export function groupTile(name, size = 24) {
  const ch = ([...name.replace(/^\p{Extended_Pictographic}\s*/u, "").trim()][0] ?? "?").toUpperCase();
  return `<span class="gtile" style="width:${size}px;height:${size}px;font-size:${Math.round(size * 0.5)}px">${esc(ch)}</span>`;
}

/** Latin next to Han gets one space (DESIGN §7.2). */
const spaced = (name) => (/^[A-Za-z0-9]/.test(name) ? ` ${name} ` : name);

const STATUS = {
  in_progress: ["circle-dot", "info", "进行中"],
  waiting_answer: ["circle-question-mark", "you", "回答"],
  waiting_review: ["eye", "you", "审"],
  blocked: ["circle-alert", "danger", "卡住了"],
  done: ["circle-check", "success", "完成"],
};
/**
 * Status chip (DESIGN §8.6). Filled chip only for 等你… (tone `you`) and 进行中 in the header;
 * every other status is a quiet label (glyph + word, no fill). Topic rows pass `row: true`:
 * 进行中 shows nothing there and names are dropped.
 */
export function chip(status, { you = false, name = "", row = false } = {}) {
  if (!status || status === "open") return "";
  if (row && status === "in_progress") return "";
  const [glyph, tone, word] = STATUS[status];
  if ((status === "waiting_answer" || status === "waiting_review") && you) {
    return `<span class="chip t-you">${icon(glyph, 14)}<span class="cl">等你${word}</span></span>`;
  }
  if (status === "waiting_answer" || status === "waiting_review") {
    const label = name && !row ? `等<span class="cn">${esc(spaced(name))}</span>${word}` : `等${word}`;
    return `<span class="lbl-s t-neutral">${icon(glyph, 14)}<span class="cl">${label}</span></span>`;
  }
  if (status === "in_progress") return `<span class="chip t-info">${icon(glyph, 14)}<span class="cl">${word}</span></span>`;
  return `<span class="lbl-s t-${tone}">${icon(glyph, 14)}<span class="cl">${word}</span></span>`;
}

export const kbd = (k) => `<span class="kbd">${k}</span>`;
export const badge = (n) => `<span class="badge">${n}</span>`;
export const count = (n) => `<span class="count">${n}</span>`;

/** Middle ellipsis that keeps the end of a file name (DESIGN §12). */
export function midEll(s, max = 44, keep = 12) {
  const c = [...s];
  if (c.length <= max) return s;
  return `${c.slice(0, max - keep - 1).join("")}…${c.slice(-keep).join("")}`;
}

const BASE_CSS = `
*{box-sizing:border-box;margin:0;padding:0}
html,body{width:var(--w);height:var(--h);overflow:hidden}
body{font-family:-apple-system,BlinkMacSystemFont,"SF Pro Text","PingFang SC","Hiragino Sans GB","Apple Color Emoji",sans-serif;font-size:14px;line-height:22px;color:var(--text-1);background:var(--content);-webkit-font-smoothing:antialiased;font-variant-numeric:tabular-nums}
.i{flex:none;display:block}
.char{display:block}
.av{position:relative;flex:none;display:inline-flex}
.av .mono{width:100%;height:100%;border-radius:50%;display:flex;align-items:center;justify-content:center;font-weight:600;letter-spacing:0;background:var(--avatar-bg);color:var(--avatar-fg)}
.pdot{position:absolute;right:-2px;bottom:-2px;border-radius:50%;box-shadow:0 0 0 2px var(--ring)}
.pdot.on{background:var(--online)}
.pdot.off{background:var(--ring);box-shadow:inset 0 0 0 1.5px var(--text-3),0 0 0 2px var(--ring)}
.pdot.work{background:var(--accent)}
.pdot.wait{background:var(--you-glyph)}
.pdot.err{background:var(--danger)}
.gtile{flex:none;display:inline-flex;align-items:center;justify-content:center;border-radius:6px;font-weight:600;background:var(--tile);color:var(--text-2)}
.chip{display:inline-flex;align-items:center;gap:4px;height:22px;padding:0 8px 0 6px;border-radius:6px;font-size:12px;line-height:16px;font-weight:500;white-space:nowrap;max-width:184px;flex:none}
.chip .cl,.lbl-s .cl{overflow:hidden;text-overflow:ellipsis;display:inline-flex;min-width:0;white-space:nowrap}
.cn{display:inline-block;max-width:96px;overflow:hidden;text-overflow:ellipsis;white-space:pre}
.lbl-s{display:inline-flex;align-items:center;gap:4px;height:22px;font-size:12px;line-height:16px;font-weight:500;white-space:nowrap;max-width:184px;flex:none}
.t-info{background:var(--info-bg);color:var(--info-text)}
.t-you{background:var(--you-bg);color:var(--you-text);font-weight:600}
.t-you .i{color:var(--you-glyph)}
.lbl-s.t-danger{color:var(--danger-text)}
.lbl-s.t-success,.lbl-s.t-neutral{color:var(--text-3)}
.kbd{display:inline-flex;align-items:center;justify-content:center;min-width:20px;height:20px;padding:0 5px;border-radius:6px;background:var(--field);color:var(--text-2);font-size:12px;line-height:16px;font-weight:500;flex:none}
.badge{display:inline-flex;align-items:center;justify-content:center;min-width:20px;height:20px;padding:0 6px;border-radius:10px;background:var(--badge);color:#fff;font-size:12px;line-height:16px;font-weight:600;flex:none}
.count{display:inline-flex;align-items:center;justify-content:center;min-width:20px;height:20px;padding:0 6px;border-radius:10px;background:var(--count-bg);color:var(--text-2);font-size:12px;line-height:16px;font-weight:600;flex:none}
.btn{display:inline-flex;align-items:center;justify-content:center;gap:6px;height:32px;padding:0 14px;border-radius:8px;font-size:14px;line-height:20px;font-weight:500;white-space:nowrap;flex:none}
.btn .i{margin-left:-2px}
.btn.primary{background:var(--accent);color:var(--on-accent)}
.btn.primary.dis{opacity:.4}
.btn.secondary{background:var(--content);color:var(--text-1);box-shadow:inset 0 0 0 1px var(--border-strong)}
.btn.ghost{color:var(--text-2)}
.btn.sm{height:28px;padding:0 10px;font-size:13px;border-radius:6px}
.btn.lg{height:40px;padding:0 18px;font-size:15px;border-radius:10px}
.ib{width:32px;height:32px;border-radius:8px;display:inline-flex;align-items:center;justify-content:center;color:var(--text-2);flex:none}
.ib.sm{width:28px;height:28px;border-radius:6px}
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
