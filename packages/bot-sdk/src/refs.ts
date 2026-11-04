// 引用提问 on the bot side: the question without the server's quote lines, the top-level
// Markdown block counter (best effort; the ref's `value` is always the reliable data), and the
// lookup of a block and a part of a message by the rules of docs/protocol.md "References to a
// part".
import type { BlockRef, Card, CardV2, CardV2Block, Message, PartRef } from "@chat/protocol";

/** The prefix line text of a ref whose source was deleted. */
export const DELETED_REF_LABEL = "原消息已删除";

/** The quote lines the server writes in front of a body for these refs. */
export function refPrefix(refs: BlockRef[]): string {
  return refs.map((r) => `> ↳ ${r.deleted ? DELETED_REF_LABEL : r.label}`).join("\n");
}

/** The body without the exact ref prefix; the body unchanged when it does not start with it. */
export function stripRefPrefix(body: string, refs: BlockRef[] | undefined): string {
  if (!refs || refs.length === 0) return body;
  const p = refPrefix(refs);
  if (body === p) return "";
  if (body.startsWith(`${p}\n\n`)) return body.slice(p.length + 2);
  return body;
}

export type MarkdownBlockKind = "paragraph" | "heading" | "list" | "quote" | "code" | "table" | "rule" | "html";
export interface MarkdownBlock {
  kind: MarkdownBlockKind;
  /** The source text of the block. */
  text: string;
  /** Code fences: the info string (for example "mermaid"). */
  lang?: string;
}

/**
 * Top-level Markdown blocks in source order (paragraph, heading, list as one block, quote,
 * code, table, thematic break, HTML). A small counter, not a full CommonMark parser.
 */
export function markdownBlocks(source: string): MarkdownBlock[] {
  const lines = source.replace(/\r\n?/g, "\n").split("\n");
  const out: MarkdownBlock[] = [];
  let i = 0;
  const isBlank = (l: string | undefined) => l === undefined || l.trim() === "";
  const isList = (l: string) => /^ {0,3}([-*+]|\d{1,9}[.)])\s/.test(l);
  const isRule = (l: string) => /^ {0,3}([-*_])( *\1){2,} *$/.test(l);
  const isTableSep = (l: string | undefined) => l !== undefined && /^ {0,3}\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/.test(l) && l.includes("-");
  while (i < lines.length) {
    const l = lines[i] as string;
    if (isBlank(l)) {
      i++;
      continue;
    }
    const fence = /^ {0,3}(`{3,}|~{3,})\s*([^`\s]*)/.exec(l);
    if (fence) {
      const mark = fence[1] as string;
      const start = i++;
      while (i < lines.length && !(lines[i] as string).trimStart().startsWith(mark)) i++;
      i = Math.min(i + 1, lines.length);
      out.push({ kind: "code", text: lines.slice(start, i).join("\n"), lang: fence[2] || undefined });
      continue;
    }
    if (/^ {0,3}#{1,6}(\s|$)/.test(l)) {
      out.push({ kind: "heading", text: l });
      i++;
      continue;
    }
    if (isRule(l)) {
      out.push({ kind: "rule", text: l });
      i++;
      continue;
    }
    const start = i;
    let kind: MarkdownBlockKind;
    if (/^ {0,3}>/.test(l)) kind = "quote";
    else if (/^ {0,3}</.test(l)) kind = "html";
    else if (/^( {4}|\t)/.test(l)) kind = "code";
    else if (isList(l)) kind = "list";
    else if (l.includes("|") && isTableSep(lines[i + 1])) kind = "table";
    else kind = "paragraph";
    i++;
    while (i < lines.length && !isBlank(lines[i])) {
      const n = lines[i] as string;
      if (kind === "paragraph" && (/^ {0,3}(#{1,6}\s|`{3,}|~{3,}|>)/.test(n) || isRule(n) || isList(n))) break;
      if (kind === "table" && !n.includes("|")) break;
      i++;
    }
    // A list keeps going over blank lines when the next item or an indented line follows.
    if (kind === "list") {
      while (i < lines.length) {
        let j = i;
        while (j < lines.length && isBlank(lines[j])) j++;
        const n = lines[j];
        if (n === undefined || !(isList(n) || /^( {2,}|\t)/.test(n))) break;
        i = j + 1;
        while (i < lines.length && !isBlank(lines[i])) i++;
      }
    }
    out.push({ kind, text: lines.slice(start, i).join("\n") });
  }
  return out;
}

/** Cells of a Markdown table block: header first. */
function tableCells(text: string): string[][] {
  return text
    .split("\n")
    .filter((_, i) => i !== 1)
    .map((l) =>
      l
        .trim()
        .replace(/^\||\|$/g, "")
        .split("|")
        .map((c) => c.trim()),
    );
}

/** Every block of a v2 card: top-level list and 2.0 inline group children. */
function v2Blocks(card: CardV2): CardV2Block[] {
  const out: CardV2Block[] = [];
  for (const b of card.blocks) {
    out.push(b);
    if (b.type === "group") for (const c of b.blocks ?? []) out.push(c as CardV2Block);
  }
  return out;
}

/** The block a ref names in a message: a v2 block, the card, a v1 block, or a Markdown block. */
export function findBlock(m: Message, blockId: string, body: string): unknown {
  if (blockId === "card") return m.cardV2 ?? m.card;
  const v1 = /^v1:(\d+)$/.exec(blockId);
  if (v1) return (m.card as Card | undefined)?.blocks[Number(v1[1])];
  const md = /^md:(\d+)$/.exec(blockId);
  if (md) return markdownBlocks(body)[Number(md[1])];
  return m.cardV2 ? v2Blocks(m.cardV2).find((b) => b.id === blockId) : undefined;
}

/** An entry by id (when `idOf` is given), else by 0-based index (a number or a digit string). */
function at<T>(list: readonly T[] | undefined, key: string | number | undefined, idOf?: (x: T) => string): T | undefined {
  if (!list || key === undefined) return undefined;
  if (idOf) {
    const byId = list.find((x) => idOf(x) === String(key));
    if (byId !== undefined) return byId;
  }
  const i = typeof key === "number" ? key : /^\d+$/.test(key) ? Number(key) : -1;
  return i >= 0 ? list[i] : undefined;
}

/** Hunks of a unified diff of one file. */
function hunks(text: string): string[] {
  const parts = text.split(/\n(?=@@ )/);
  return parts.filter((p) => p.startsWith("@@ "));
}

/**
 * The part a `PartRef` names in a block, or undefined when it does not exist. An unknown kind
 * refers to the whole block (the block is returned).
 */
export function findPart(block: unknown, part: PartRef | undefined): unknown {
  if (!part) return block;
  if (!block || typeof block !== "object") return undefined;
  const b = block as Record<string, unknown> & { type?: string; kind?: string };
  const k = part.keys ?? {};
  const type = b.type ?? (b.kind === "table" ? "mdtable" : b.kind === "code" ? "mdcode" : undefined);
  switch (part.kind) {
    case "cell":
    case "row":
    case "column": {
      let rows: string[][];
      let columns: string[];
      let rowKeys: string[] | undefined;
      if (type === "table") {
        rows = b.rows as string[][];
        columns = b.columns as string[];
        rowKeys = b.rowKeys as string[] | undefined;
      } else if (type === "mdtable") {
        const cells = tableCells(String((block as { text: string }).text));
        columns = cells[0] ?? [];
        rows = cells.slice(1);
      } else return undefined;
      const rowIndex = (key: string | number | undefined): number => {
        if (key === undefined) return -1;
        if (rowKeys) {
          const i = rowKeys.indexOf(String(key));
          if (i >= 0) return i;
        }
        return typeof key === "number" ? key : /^\d+$/.test(key) ? Number(key) : -1;
      };
      if (part.kind === "column") {
        const c = Number(k.col);
        return Number.isInteger(c) && c >= 0 && c < columns.length ? { column: columns[c], values: rows.map((r) => r[c]) } : undefined;
      }
      const row = rows[rowIndex(k.row)];
      if (!row) return undefined;
      if (part.kind === "row") return { row, cells: Object.fromEntries(columns.map((c, i) => [c, row[i]])) };
      const c = Number(k.col);
      return Number.isInteger(c) && c >= 0 && c < columns.length ? { row: row[0], column: columns[c], display: row[c] } : undefined;
    }
    case "point":
    case "bar":
    case "series": {
      if (type !== "chart") return undefined;
      const series = (b.series as { id?: string; name: string; values: number[] }[]).find((s) => (s.id ?? s.name) === k.series);
      if (!series) return undefined;
      if (part.kind === "series") return { series: series.id ?? series.name, name: series.name, values: series.values, x: b.x };
      const xs = b.x as string[] | undefined;
      const i = typeof k.x === "number" ? k.x : (xs?.indexOf(String(k.x)) ?? -1);
      if (i < 0 || i >= series.values.length) return undefined;
      return { series: series.id ?? series.name, x: xs?.[i] ?? i, value: series.values[i] };
    }
    case "metric":
      return type === "metric" ? block : undefined;
    case "item": {
      const item = k.item;
      switch (type) {
        case "list":
          return at(b.items as unknown[], item);
        case "fields":
          return at(b.fields as unknown[], item);
        case "evidence":
          return block;
        case "decision":
        case "poll":
        case "imageChoice":
          return at(b.options as { id: string }[], item, (o) => o.id);
        case "rank":
          return at(b.items as { id: string }[], item, (o) => o.id);
        case "diff":
          return String(b.path ?? "") === String(item) ? block : undefined;
        default:
          return undefined;
      }
    }
    case "step":
      return type === "plan" ? at(b.items as unknown[], k.step) : undefined;
    case "line": {
      const text = type === "code" || type === "mdcode" ? String(b.text ?? "") : type === "diff" ? String(b.text ?? "") : undefined;
      if (text === undefined) return undefined;
      const n = Number(k.n);
      if (type === "diff") {
        // Count lines of the asked side through the hunks.
        const side = k.side === "old" ? "old" : "new";
        for (const h of hunks(text)) {
          const m = /^@@ -(\d+)(?:,\d+)? \+(\d+)/.exec(h);
          if (!m) continue;
          let line = Number(side === "old" ? m[1] : m[2]);
          for (const l of h.split("\n").slice(1)) {
            const mark = l[0];
            const counts = side === "old" ? mark === " " || mark === "-" : mark === " " || mark === "+";
            if (!counts) continue;
            if (line === n) return { n, side, text: l.slice(1) };
            line++;
          }
        }
        return undefined;
      }
      const lines = type === "mdcode" ? text.split("\n").slice(1, -1) : text.split("\n");
      return Number.isInteger(n) && n >= 1 && n <= lines.length ? { n, text: lines[n - 1] } : undefined;
    }
    case "hunk": {
      if (type !== "diff") return undefined;
      const h = hunks(String(b.text ?? ""))[Number(k.hunk)];
      return h === undefined ? undefined : { header: h.split("\n")[0], patch: h };
    }
    case "image":
      return type === "image" || type === "evidence" ? block : undefined;
    case "node": {
      const source = type === "diagram" ? String(b.source ?? "") : type === "mdcode" ? String(b.text ?? "") : undefined;
      if (source === undefined || typeof k.node !== "string") return undefined;
      const id = k.node.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      const m = new RegExp(`(?:^|[\\s;>-])${id}\\s*(?:\\[\\[|\\[\\(|\\(\\(|\\[|\\(|\\{|>)\\s*"?([^\\]\\)\\}"]+?)"?\\s*(?:\\]\\]|\\)\\]|\\)\\)|\\]|\\)|\\})`, "m").exec(source);
      if (m) return { node: k.node, label: m[1]?.trim() };
      return new RegExp(`(?:^|[\\s;>-])${id}(?:$|[\\s;-])`, "m").test(source) ? { node: k.node, label: k.node } : undefined;
    }
    case "element":
      return block;
    default:
      // Open set: an unknown kind refers to the whole block.
      return block;
  }
}
