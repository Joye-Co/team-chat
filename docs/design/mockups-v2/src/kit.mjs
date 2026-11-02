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
 * Small-size character art (DESIGN §7.9): up to 64 pt the status lamp at the top corner
 * and the question bubble read as notification badges, so they are removed. The face keeps the
 * expression; the status moves to the bottom-right pip.
 */
export function smallArt(svg) {
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
const BOXES = JSON.parse(readFileSync(join(HERE, "char-boxes.json"), "utf8"));
const charCache = new Map();
/**
 * A bot's character at its live status (idle, working, needs-input, success, error, offline).
 * `fit` crops the art to its visible bounds (the idle pose, so a status change does not move it),
 * square by the larger side and centered: every character then fills the same box (DESIGN §7.9).
 */
export function character(id, status = "idle", size = 32, { fit = false, fill = 1 } = {}) {
  const file = SIZES.find((s) => s >= size) ?? 128;
  const small = size <= 64;
  const key = `${id}/${status}-${file}-${small}-${fit}-${fill}`;
  if (!charCache.has(key)) {
    let svg = readFileSync(join(REPO, `apps/desktop/assets/characters/${id}/${status}-${file}.svg`), "utf8");
    if (small) svg = smallArt(svg);
    const b = BOXES[`${id}__idle`];
    if (fit && b) {
      // The art's larger side takes `fill` of the box; its painted center sits at the box center,
      // 2 % above the middle so the face line lands near 46-48 % of the tile (§7.9).
      const side = Math.max(b[2], b[3]) / fill;
      const x = b[0] + b[2] / 2 - side / 2;
      const y = b[1] + b[3] / 2 - side * 0.48;
      svg = svg.replace(/viewBox="[^"]*"/, `viewBox="${x.toFixed(1)} ${y.toFixed(1)} ${side.toFixed(1)} ${side.toFixed(1)}"`);
    }
    charCache.set(key, `data:image/svg+xml;base64,${Buffer.from(svg).toString("base64")}`);
  }
  return `<img class="char" width="${size}" height="${size}" src="${charCache.get(key)}" alt="">`;
}

/**
 * Initials (DESIGN §7.9): one character at 24 pt and below; above that, one Han character for a
 * CJK name and two letters for a Latin name.
 */
export function initials(name, size = 36) {
  const n = name.trim().replace(/^\p{Extended_Pictographic}(\u200d\p{Extended_Pictographic})*\s*/u, "");
  const chars = [...n];
  if (/[一-鿿]/.test(chars[0] ?? "")) return chars[0];
  const words = n.split(/\s+/).filter(Boolean);
  const a = [...(words[0] ?? "?")][0];
  const b = words.length > 1 && size > 24 ? [...words[words.length - 1]][0] : "";
  return (a + b).toUpperCase();
}

/**
 * One mid tint per bot, picked by a stable hash of the character id (DESIGN §2.6): FNV-1a 32-bit
 * with offset basis 2166136261 XOR 91, mod 6. The seed gives the six roster bots seen together
 * most (Pocket, Forge, Watch, Scout, Ledger, Taro) six different hues.
 */
const tint = (id) => {
  let h = (2166136261 ^ 91) >>> 0;
  for (const c of id) h = Math.imul(h ^ c.charCodeAt(0), 16777619) >>> 0;
  return (h % 6) + 1;
};

/**
 * Member avatar (DESIGN §7.9). People: a circle (photo, or initials on the neutral fill) with a
 * presence dot. Bots: a rounded-square tile in the bot's tint with the character's painted bounds
 * fitted to 62 % of the tile (at least 19 % inset on every side, never touching the edge); the
 * shape alone says bot or person. A status pip shows while the bot works, waits or fails.
 * Offline people show no dot. At 24 pt and below the dot is 8 pt with a 2 pt ring and sits on
 * the corner (offset 3 pt), so it does not cover the initial.
 */
export function avatar(m, size, mode, { presence = true, ring = "var(--content)", pip: showPip = true, sel = false } = {}) {
  let inner;
  if (m.char) {
    inner = `<span class="tile b${tint(m.char)}${sel ? " on-sel" : ""}" style="border-radius:${Math.round(size * 0.25)}px">${character(m.char, m.state ?? "idle", size, { fit: true, fill: 0.62 })}</span>`;
  } else {
    const ini = initials(m.name, size);
    const fs = size <= 24 ? 12 : Math.round(size * (ini.length > 1 ? 0.38 : 0.44));
    inner = `<span class="mono${size <= 24 ? " s" : ""}" style="font-size:${fs}px">${esc(ini)}</span>`;
  }
  const d = size >= 32 ? 10 : 8;
  const off = size >= 32 ? -1 : -3;
  const pos = `width:${d}px;height:${d}px;right:${off}px;bottom:${off}px;--ring:${ring}`;
  let dot = "";
  if (presence && !m.char && m.online) dot = `<span class="pdot on" style="${pos}"></span>`;
  const pip = { working: "work", "needs-input": "wait", error: "err" }[m.state];
  if (presence && showPip && m.char && pip && size <= 36) dot = `<span class="pdot ${pip}" style="${pos}"></span>`;
  return `<span class="av ${m.char ? "bot" : "person"}" style="width:${size}px;height:${size}px">${inner}${dot}</span>`;
}

/** Group tile (switcher, dialogs): one neutral fill and the first character. */
export function groupTile(name, size = 24) {
  const ch = ([...name.replace(/^\p{Extended_Pictographic}\s*/u, "").trim()][0] ?? "?").toUpperCase();
  return `<span class="gtile" style="width:${size}px;height:${size}px;font-size:${Math.round(size * 0.5)}px">${esc(ch)}</span>`;
}

/** Latin next to Han gets one space (DESIGN §10). */
const spaced = (name) => (/^[A-Za-z0-9]/.test(name) ? ` ${name} ` : name);

const STATUS = {
  in_progress: ["circle-dot", "info", "进行中"],
  waiting_answer: ["circle-question-mark", "you", "回答"],
  waiting_review: ["eye", "you", "审"],
  blocked: ["circle-alert", "danger", "卡住了"],
  done: ["circle-check", "success", "完成"],
};
/**
 * Status chip and label (DESIGN §7.8). Filled chip only for 等你… (tone `you`) and 进行中 in the header;
 * every other status is a quiet label (glyph + word, no fill). Topic rows pass `row: true`:
 * 进行中 shows nothing there and names are dropped.
 */
export function chip(status, { you = false, name = "", row = false, quiet = false, whoM = null, mode = "light" } = {}) {
  if (!status || status === "open") return "";
  if (row && status === "in_progress") return "";
  const [glyph, tone, word] = STATUS[status];
  const waiting = status === "waiting_answer" || status === "waiting_review";
  // Rows: a compact chip with no glyph, so the preview keeps the line (§7.8).
  if (waiting && you && row) return `<span class="chip sm t-you">${status === "waiting_answer" ? "等你答" : "等你审"}</span>`;
  // Headers and the switcher: the same words without a fill; the list row keeps the one fill.
  if (waiting && you && quiet) return `<span class="lbl-s t-quiet">${icon(glyph, 14)}<span class="cl">等你${word}</span></span>`;
  if (waiting && you) return `<span class="chip t-you">${icon(glyph, 14)}<span class="cl">等你${word}</span></span>`;
  // Headers: the person waited on is a 20 pt avatar, never a name inside the label (§7.8);
  // the full name is in the tooltip and the side panel.
  if (waiting && whoM && !row) return `<span class="lbl-s t-neutral who" title="等 ${esc(name)} ${word}">${avatar(whoM, 20, mode, { presence: false })}<span class="cl">等${word}</span></span>`;
  if (waiting) {
    const label = name && !row ? `等<span class="cn">${esc(spaced(name))}</span>${word}` : `等${word}`;
    return `<span class="lbl-s t-neutral">${icon(glyph, 14)}<span class="cl">${label}</span></span>`;
  }
  if (status === "in_progress") return `<span class="chip t-info">${icon(glyph, 14)}<span class="cl">${word}</span></span>`;
  return `<span class="lbl-s t-${tone}">${icon(glyph, 14)}<span class="cl">${word}</span></span>`;
}

/** Counts above 99 render as 99+ everywhere: badges, count pills, tabs, sections (§7.8). */
export const cap = (n) => (typeof n === "number" && n > 99 ? "99+" : n);
export const kbd = (k) => `<span class="kbd">${k}</span>`;
export const badge = (n) => `<span class="badge">${cap(n)}</span>`;
/** Unread count with no mention (§7.8): a neutral solid pill; red stays for @ and 卡住了. */
export const nbadge = (n) => `<span class="badge n">${cap(n)}</span>`;
/** Titles keep their leading full-width bracket in place: the v2.1 hang is removed (§10). */
export const lead = (html) => html;
export const count = (n) => `<span class="count">${cap(n)}</span>`;

/** Estimated advance of one character in em (system UI font; Han and full-width = 1). */
function adv(c) {
  if (/[\u2E80-\u9FFF\uFF00-\uFFEF\u3000-\u303F]/.test(c)) return 1;
  if (/\p{Extended_Pictographic}/u.test(c)) return 1.25;
  if (/[\u200d\uFE0F]/.test(c)) return 0;
  if (/[ilj.,:;'!|()\[\]]/.test(c)) return 0.3;
  if (c === " ") return 0.28;
  if (/[A-Z]/.test(c)) return 0.64;
  if (/[mwMW]/.test(c)) return 0.86;
  return 0.54;
}
/**
 * End truncation (DESIGN §15): fits `s` in `px` at `font` px. Before the "…" it trims trailing
 * spaces, open brackets and punctuation, so no line ends in " …", "（…" or "v0.9.…".
 */
export function fit(s, px, font = 14) {
  const c = [...s];
  const w = (a) => a.reduce((t, ch) => t + adv(ch), 0) * font;
  if (w(c) <= px) return s;
  const room = px - font * 0.9; // the ellipsis
  let n = c.length;
  while (n > 0 && w(c.slice(0, n)) > room) n--;
  let out = c.slice(0, n).join("");
  // A Latin word cut mid-way to under 4 characters, or a cut version number, goes whole.
  if (/[A-Za-z0-9.]/.test(c[n] ?? "")) {
    const part = out.match(/[A-Za-z0-9.]+$/u)?.[0] ?? "";
    if (part && (part.length < 4 || part.includes("."))) out = out.slice(0, -part.length);
  }
  out = out.replace(/[\s（(【\[「“.。,，、:：·\-]+$/u, "");
  return `${out.replace(/\s+$/u, "")}…`;
}

/** Middle ellipsis that keeps the end of a file name (DESIGN §15). */
export function midEll(s, max = 44, keep = 12) {
  const c = [...s];
  if (c.length <= max) return s;
  return `${c.slice(0, max - keep - 1).join("")}…${c.slice(-keep).join("")}`;
}

const BASE_CSS = `
*{box-sizing:border-box;margin:0;padding:0}
html,body{min-width:var(--pw);min-height:100vh;overflow:hidden}
html.framed,html.framed body{background:var(--desk)}
.desk{width:100vw;min-width:var(--pw);height:100vh;min-height:var(--ph);background:var(--desk);display:flex;align-items:center;justify-content:center}
.frame{width:var(--w);height:var(--h);border-radius:12px;overflow:hidden;position:relative;flex:none;box-shadow:var(--shadow-window)}
:root{--font-ui:-apple-system,BlinkMacSystemFont,"SF Pro Text","PingFang SC","Hiragino Sans GB","Apple Color Emoji",sans-serif}
body{font-family:var(--font-ui);font-size:14px;line-height:22px;color:var(--text-1);background:var(--content);-webkit-font-smoothing:antialiased;font-variant-numeric:tabular-nums}
.i{flex:none;display:block}
.char{display:block}
.av{position:relative;flex:none;display:inline-flex}
.av .mono{width:100%;height:100%;border-radius:50%;display:flex;align-items:center;justify-content:center;font-weight:600;letter-spacing:0;background:var(--avatar-bg);color:var(--avatar-fg)}
.av .mono.s{line-height:16px;padding-bottom:1px}
.pdot{position:absolute;border-radius:50%;box-shadow:0 0 0 2px var(--ring)}
.pdot.on{background:var(--online)}
.pdot.work{background:var(--accent)}
.pdot.wait{background:var(--you-glyph)}
.pdot.err{background:var(--danger)}
.tile{width:100%;height:100%;display:flex;align-items:center;justify-content:center;overflow:hidden}
.tile.on-sel{box-shadow:0 0 0 1px var(--tile-ring)}
.tile.b1{background:var(--bot-1)}.tile.b2{background:var(--bot-2)}.tile.b3{background:var(--bot-3)}.tile.b4{background:var(--bot-4)}.tile.b5{background:var(--bot-5)}.tile.b6{background:var(--bot-6)}
.gtile{flex:none;display:inline-flex;align-items:center;justify-content:center;border-radius:6px;font-weight:600;background:var(--tile);color:var(--text-2)}
.chip{display:inline-flex;align-items:center;gap:4px;height:22px;padding:0 8px 0 6px;border-radius:6px;font-size:12px;line-height:16px;font-weight:500;white-space:nowrap;max-width:184px;flex:none}
.chip .cl,.lbl-s .cl{overflow:hidden;text-overflow:ellipsis;display:inline-flex;min-width:0;white-space:nowrap}
.cn{display:inline-block;max-width:96px;overflow:hidden;text-overflow:ellipsis;white-space:pre}
.lbl-s{display:inline-flex;align-items:center;gap:4px;height:22px;font-size:12px;line-height:16px;font-weight:500;white-space:nowrap;max-width:184px;flex:none}
.t-info{background:var(--info-bg);color:var(--info-text)}
.t-you{background:var(--you-bg);color:var(--you-text);font-weight:600}
.t-you .i{color:var(--you-glyph)}
.chip.sm{height:20px;padding:0 6px;font-size:12px;line-height:16px;font-weight:600;max-width:72px;border-radius:6px}
.lbl-s.t-quiet{color:var(--text-2)}
.lbl-s.t-quiet .i{color:var(--you-glyph)}
.lbl-s.t-danger{color:var(--danger-text)}
.lbl-s.t-success,.lbl-s.t-neutral{color:var(--text-3)}
.kbd{display:inline-flex;align-items:center;justify-content:center;min-width:20px;height:20px;padding:0 5px;border-radius:6px;background:var(--field);color:var(--text-2);font-size:12px;line-height:16px;font-weight:500;flex:none}
.badge{display:inline-flex;align-items:center;justify-content:center;min-width:20px;height:20px;padding:0 6px;border-radius:10px;background:var(--badge);color:#fff;font-size:12px;line-height:16px;font-weight:600;flex:none}
.badge.n{background:var(--count-solid)}
.lbl-s.who{gap:6px}
.lbl-s.who .av{margin:0}
.count{display:inline-flex;align-items:center;justify-content:center;min-width:20px;height:20px;padding:0 6px;border-radius:10px;background:var(--count-bg);color:var(--count-text);font-size:12px;line-height:16px;font-weight:600;flex:none}
.btn{display:inline-flex;align-items:center;justify-content:center;gap:6px;height:32px;padding:0 14px;border-radius:8px;font-size:14px;line-height:20px;font-weight:500;white-space:nowrap;flex:none}
.btn .i{margin-left:-2px}
.btn.primary{background:var(--accent);color:var(--on-accent)}
.btn.primary.dis{background:var(--disabled-bg);color:var(--disabled-text)}
.btn.text{height:22px;padding:0 6px;border-radius:6px;font-size:13px;line-height:20px;font-weight:500;color:var(--accent-text)}
.btn.secondary{background:var(--content);color:var(--text-1);box-shadow:inset 0 0 0 1px var(--border-strong)}
.btn.ghost{color:var(--text-2)}
.btn.sm{height:28px;padding:0 10px;font-size:13px;border-radius:6px}
.btn.md{height:36px;padding:0 14px}
.btn.lg{height:40px;padding:0 18px;font-size:15px;border-radius:10px}
.ib{width:32px;height:32px;border-radius:8px;display:inline-flex;align-items:center;justify-content:center;color:var(--text-2);flex:none}
.ib.sm{width:28px;height:28px;border-radius:6px}
.ib.hov{background:var(--hover);color:var(--text-1)}
.ell{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;min-width:0}
.t2{color:var(--text-2)} .t3{color:var(--text-3)}
.focus{box-shadow:0 0 0 2px var(--content),0 0 0 4px var(--focus)!important}
`;

/** A page. `frame` presents a smaller window on a desk background so its bounds show (§8). */
export function page({ title, mode, width, height, css, body, frame = 0 }) {
  const t = mode === "dark" ? dark : light;
  const pw = width + frame * 2;
  const ph = height + frame * 2;
  const inner = frame ? `<div class="desk"><div class="frame">${body}</div></div>` : body;
  return `<!doctype html>
<html lang="zh-Hans" data-theme="${mode}"${frame ? ' class="framed"' : ""}><head><meta charset="utf-8">
<meta name="viewport" content="width=${pw}">
<title>${esc(title)}</title>
<style>:root{${cssVars(t)}--w:${width}px;--h:${height}px;--pw:${pw}px;--ph:${ph}px;color-scheme:${mode}}${BASE_CSS}${css}</style>
</head><body>${inner}</body></html>
`;
}
