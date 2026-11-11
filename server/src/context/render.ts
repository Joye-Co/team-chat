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
  /** `title` is the 产出's current title, added when the row is read (bots/room.ts `contextRows`). */
  artifactRefs?: { artifactId: string; version: number; title?: string }[];
  card?: { title?: string };
  cardV2?: { title?: string; fallback?: string };
  cardState?: { answers?: { actorId: string; value?: unknown; inputs?: Record<string, unknown>; retractedAt?: number }[]; answerCount?: number };
  blockRefs?: { label: string; messageId?: string; artifact?: { artifactId: string; version: number } }[];
}

/** Body characters of one message in the prompt; the rest is read by id. */
export const LINE_BODY_CHARS = 600;
/** Body characters of the topic's first message in the prompt: the request is carried whole. */
export const FIRST_BODY_CHARS = 4000;
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

/** The clock of a team that set no time zone (`teamZone` in memory/consolidate.ts). */
export const DEFAULT_STAMP_ZONE = "Asia/Shanghai";

/** `11-08 14:02` on the team's clock `zone` (the prompt only needs a stable, short stamp). */
export function stamp(at: number, zone = DEFAULT_STAMP_ZONE): string {
  let parts: Record<string, string>;
  try {
    parts = Object.fromEntries(
      new Intl.DateTimeFormat("en-CA", { timeZone: zone, month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", hourCycle: "h23" })
        .formatToParts(new Date(at))
        .map((x) => [x.type, x.value]),
    ) as Record<string, string>;
  } catch {
    // An unknown zone: the default clock.
    return zone === DEFAULT_STAMP_ZONE ? "" : stamp(at);
  }
  return `${parts.month}-${parts.day} ${parts.hour}:${parts.minute}`;
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
export function renderMessage(r: ContextRow, names: Names, bodyChars = LINE_BODY_CHARS, more = "", zone = DEFAULT_STAMP_ZONE): string {
  const who = names(r.authorId);
  const head = `[#${r.seq} m ${r.id} · ${who} · ${stamp(r.at, zone)}]`;
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
  if (x.replyTo) lines.push(quoteLine(x.replyTo, names));
  for (const b of x.blockRefs ?? []) {
    const src = b.messageId ? `m ${b.messageId}` : b.artifact ? `产出 a ${b.artifact.artifactId} v${b.artifact.version}` : "";
    lines.push(`  ↪ asks about ${src}${b.label ? ` (${short(b.label, 120)})` : ""}`);
  }
  lines.push(...fileLines(x));
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

function quoteLine(q: NonNullable<MessageExt["replyTo"]>, names: Names): string {
  return `  ↪ quotes m ${q.messageId} (${names(q.authorId)}: ${short(q.excerpt, 120)})`;
}

function fileLines(x: MessageExt): string[] {
  const lines: string[] = [];
  for (const f of x.attachments ?? []) {
    const dim = f.width && f.height ? ` · ${f.width}×${f.height}` : "";
    lines.push(`  [file f ${f.fileId} · ${short(f.name, 120)} · ${f.mime} · ${bytes(f.size)}${dim}]`);
  }
  for (const a of x.artifactRefs ?? []) lines.push(`  [产出 a ${a.artifactId} v${a.version}${a.title ? ` · ${short(a.title, 120)}` : ""}]`);
  return lines;
}

/** What a request or follow-up message carries besides its text (`TopicMessage.ext`). */
export type RequestExt = Pick<MessageExt, "replyTo" | "attachments" | "artifactRefs">;

/** The parts of a stored message's ext that a request carries along with its body. */
export function requestExt(x: MessageExt): RequestExt | undefined {
  const out: RequestExt = {};
  if (x.replyTo) out.replyTo = x.replyTo;
  if (x.attachments && x.attachments.length > 0) out.attachments = x.attachments;
  if (x.artifactRefs && x.artifactRefs.length > 0) out.artifactRefs = x.artifactRefs;
  return Object.keys(out).length > 0 ? out : undefined;
}

/**
 * A request or follow-up as the run and the front desk read it: the body, then the same
 * indented lines a context row has for the message it quotes, its files and its 产出, so a
 * file is an id the run can fetch (`chat-run file get`). A message with only an image is not empty.
 */
export function requestText(body: string, ext: RequestExt | undefined, names: Names): string {
  if (!ext) return body;
  const lines = [...(ext.replyTo ? [quoteLine(ext.replyTo, names)] : []), ...fileLines(ext)];
  if (lines.length === 0) return body;
  return body.trim() === "" ? lines.join("\n") : `${body}\n${lines.join("\n")}`;
}

/** The author ids a request's lines name. */
export function requestAuthors(ext: RequestExt | undefined): string[] {
  return ext?.replyTo ? [ext.replyTo.authorId] : [];
}

function bytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(0)} KB`;
  return `${(n / 1024 ** 2).toFixed(1)} MB`;
}

/** The topic lines with the rolling summary in front, when messages were left out. */
export function withSummary(sel: Pick<Selection, "text" | "omitted">, summary?: { text: string; uptoSeq: number }): string {
  if (!summary || sel.omitted === 0) return sel.text;
  return `Summary of the topic up to #${summary.uptoSeq} (the messages left out are in it):\n${summary.text}\n\n${sel.text}`;
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
  opts: { more?: (r: ContextRow) => string; gap?: (omitted: number, beforeSeq: number) => string; zone?: string } = {},
): Selection {
  const render = (r: ContextRow, chars = LINE_BODY_CHARS) => renderMessage(r, names, chars, opts.more?.(r) ?? "", opts.zone);
  const head = first ? render(first, FIRST_BODY_CHARS) : "";
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
