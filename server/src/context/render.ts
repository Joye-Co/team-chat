// Messages as compact lines for a run's prompt and the chat-run context ops (docs/protocol.md,
// "Run context"). Pure: rows in, text out. Nothing that is not text is dropped: files, 产出,
// quotes and cards each get a short line the run can follow by id.
import { cut } from "../bots/dispatch";

/** A stored message as the context reads it (`messages` row with its `ext` parsed). */
export interface ContextRow {
  id: string;
  seq: number;
  authorId: string;
  at: number;
  body: string;
  kind: string;
  ext: MessageExt;
}

/** The parts of `Message` that live in `messages.ext` and matter to a run. */
export interface MessageExt {
  replyTo?: { messageId: string; authorId: string; excerpt: string };
  attachments?: { fileId: string; name: string; mime: string; size: number; width?: number; height?: number }[];
  artifactRefs?: { artifactId: string; version: number }[];
  card?: { title?: string };
  cardV2?: { title?: string; fallback?: string };
  cardState?: { answers?: { actorId: string; value?: unknown; inputs?: Record<string, unknown>; retractedAt?: number }[]; answerCount?: number };
  blockRefs?: { label: string; messageId?: string; artifact?: { artifactId: string; version: number } }[];
}

/** Body characters of one message in the prompt; the rest is read by id. */
export const LINE_BODY_CHARS = 600;
/** Body characters of one message in `topic.history`. */
export const HISTORY_BODY_CHARS = 4000;

export type Names = (id: string) => string;

export function parseExt(ext: string | null): MessageExt {
  if (!ext) return {};
  try {
    const v = JSON.parse(ext) as unknown;
    return v && typeof v === "object" ? (v as MessageExt) : {};
  } catch {
    return {};
  }
}

/** `11-08 14:02` in UTC+8, the team's clock (the prompt only needs a stable, short stamp). */
export function stamp(at: number): string {
  const d = new Date(at + 8 * 3600_000);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(d.getUTCMonth() + 1)}-${p(d.getUTCDate())} ${p(d.getUTCHours())}:${p(d.getUTCMinutes())}`;
}

function oneLine(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

function short(v: unknown, max = 80): string {
  if (v === undefined) return "";
  const s = typeof v === "string" ? v : JSON.stringify(v);
  return cut(oneLine(s ?? ""), max);
}

export interface RenderStats {
  files: number;
  cards: number;
  refs: number;
}

/**
 * One message as a header line plus indented detail lines. `bodyChars` cuts the body (0 = whole);
 * `more` is the hint after a cut body (how to read the rest), or none.
 */
export function renderMessage(r: ContextRow, names: Names, bodyChars = LINE_BODY_CHARS, more = ""): string {
  const who = names(r.authorId);
  const head = `[#${r.seq} m ${r.id} · ${who} · ${stamp(r.at)}]`;
  const lines: string[] = [];
  const body = r.body.trim();
  if (r.kind === "system") {
    lines.push(`${head} [${oneLine(body)}]`);
  } else {
    let text = body;
    if (bodyChars > 0 && [...text].length > bodyChars) text = `${cut(text, bodyChars)}${more ? ` (${more})` : ""}`;
    lines.push(text === "" ? head : `${head} ${text}`);
  }
  const x = r.ext;
  if (x.replyTo) lines.push(`  ↪ quotes m ${x.replyTo.messageId} (${names(x.replyTo.authorId)}: ${short(x.replyTo.excerpt, 120)})`);
  for (const b of x.blockRefs ?? []) {
    const src = b.messageId ? `m ${b.messageId}` : b.artifact ? `产出 a ${b.artifact.artifactId} v${b.artifact.version}` : "";
    lines.push(`  ↪ asks about ${src}${b.label ? ` (${short(b.label, 120)})` : ""}`);
  }
  for (const f of x.attachments ?? []) {
    const dim = f.width && f.height ? ` · ${f.width}×${f.height}` : "";
    lines.push(`  [file f ${f.fileId} · ${short(f.name, 120)} · ${f.mime} · ${bytes(f.size)}${dim}]`);
  }
  for (const a of x.artifactRefs ?? []) lines.push(`  [产出 a ${a.artifactId} v${a.version}]`);
  const title = x.cardV2?.title ?? x.card?.title;
  if (x.cardV2 || x.card) {
    const answers = (x.cardState?.answers ?? []).filter((a) => a.retractedAt === undefined);
    let state = "";
    if (answers.length > 0) {
      const shown = answers.slice(-3).map((a) => {
        const v = a.value !== undefined ? a.value : a.inputs;
        return `${names(a.actorId)}${v === undefined ? "" : ` ${short(v, 60)}`}`;
      });
      state = ` · ${answers.length} answers: ${shown.join("; ")}`;
    } else if (x.cardState?.answerCount) {
      state = ` · ${x.cardState.answerCount} answers`;
    }
    lines.push(`  [card · ${short(title ?? "", 120)}${state}]`);
  }
  return lines.join("\n");
}

function bytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(0)} KB`;
  return `${(n / 1024 ** 2).toFixed(1)} MB`;
}

export interface Selection {
  /** Lines in topic order. */
  text: string;
  /** Rows carried, in topic order. */
  rows: ContextRow[];
  /** Messages left out between the first message and the newest ones. */
  omitted: number;
  chars: number;
  stats: RenderStats;
}

/**
 * The topic's lines within `budget` characters: the first message always, then the newest
 * ones, newest first, until the budget is spent. `newest` is newest first and may include the
 * first message; `total` is how many messages the topic has in the range the caller read.
 */
export function selectLines(
  first: ContextRow | undefined,
  newest: ContextRow[],
  total: number,
  names: Names,
  budget: number,
  opts: { more?: (r: ContextRow) => string; gap?: (omitted: number, beforeSeq: number) => string } = {},
): Selection {
  const render = (r: ContextRow) => renderMessage(r, names, LINE_BODY_CHARS, opts.more?.(r) ?? "");
  const head = first ? render(first) : "";
  let used = [...head].length;
  const tail: { r: ContextRow; text: string }[] = [];
  for (const r of newest) {
    if (first && r.id === first.id) break;
    const t = render(r);
    const n = [...t].length + 1;
    if (tail.length > 0 && used + n > budget) break;
    used += n;
    tail.push({ r, text: t });
  }
  tail.reverse();
  const carried = (first ? 1 : 0) + tail.length;
  const omitted = Math.max(0, total - carried);
  const parts: string[] = [];
  if (first) parts.push(head);
  if (omitted > 0) {
    const beforeSeq = tail[0]?.r.seq ?? Number.MAX_SAFE_INTEGER;
    parts.push(opts.gap ? opts.gap(omitted, beforeSeq) : `[… ${omitted} messages left out …]`);
  }
  parts.push(...tail.map((x) => x.text));
  const text = parts.join("\n");
  const rows = [...(first ? [first] : []), ...tail.map((x) => x.r)];
  return { text, rows, omitted, chars: [...text].length, stats: countStats(rows) };
}

export function countStats(rows: ContextRow[]): RenderStats {
  const s: RenderStats = { files: 0, cards: 0, refs: 0 };
  for (const r of rows) {
    s.files += r.ext.attachments?.length ?? 0;
    s.refs += r.ext.blockRefs?.length ?? 0;
    if (r.ext.card || r.ext.cardV2) s.cards += 1;
  }
  return s;
}
