// `card.patch` (INTERACT §4.1): the author changes its card in place. Ops apply in order on a
// copy; the result is checked as a whole card (the write schema: style keys and every write
// rule). Nothing changes when an op or the result fails.
import { CardV2, type CardV2Block, type PatchOp, childIds, MAX_CARD_V2_REVS } from "@chat/protocol";
import { ApiError, schemaReason } from "../core/errors";

type Block = CardV2Block;
type Draft = Record<string, unknown> & { blocks: Block[]; data?: unknown };

const fail = (i: number, message: string): never => {
  throw new ApiError("invalid", `op ${i}: ${message}`);
};

/** The container (or compare option, or tab) whose children list `parent` names. */
function childList(blocks: Block[], parent: string): string[] | undefined {
  for (const b of blocks) {
    if (b.id === parent) {
      if (b.type === "group" && b.blocks !== undefined) return undefined;
      if (b.type === "group" || b.type === "stack" || b.type === "row" || b.type === "questionnaire") {
        b.children = b.children ?? [];
        return b.children;
      }
      return undefined;
    }
    if (b.type === "compare") {
      const o = b.options.find((x) => x.id === parent);
      if (o) {
        o.children = o.children ?? [];
        return o.children;
      }
    }
    if (b.type === "tabs") {
      const t = b.tabs.find((x) => x.id === parent);
      if (t) return t.children;
    }
  }
  return undefined;
}

function place(list: string[], id: string, op: { after?: string; before?: string; index?: number }, i: number): void {
  if (op.after !== undefined) {
    const k = list.indexOf(op.after);
    if (k < 0) fail(i, `no ${op.after} to insert after`);
    list.splice(k + 1, 0, id);
  } else if (op.before !== undefined) {
    const k = list.indexOf(op.before);
    if (k < 0) fail(i, `no ${op.before} to insert before`);
    list.splice(k, 0, id);
  } else if (op.index !== undefined) list.splice(Math.min(op.index, list.length), 0, id);
  else list.push(id);
}

/** Sets or deletes the value at a JSON Pointer ("" = the whole document). */
function pointer(doc: unknown, path: string, value: unknown, del: boolean, i: number): unknown {
  if (path === "") return del ? undefined : value;
  if (!path.startsWith("/")) fail(i, "a data path is a JSON Pointer");
  const keys = path
    .slice(1)
    .split("/")
    .map((k) => k.replace(/~1/g, "/").replace(/~0/g, "~"));
  const root = (doc && typeof doc === "object" ? structuredClone(doc) : {}) as Record<string, unknown>;
  let cur: Record<string, unknown> | unknown[] = root;
  for (let k = 0; k < keys.length - 1; k++) {
    const key = keys[k];
    const next = (cur as Record<string, unknown>)[key];
    if (!next || typeof next !== "object") {
      if (del) return root;
      (cur as Record<string, unknown>)[key] = {};
    }
    cur = (cur as Record<string, unknown>)[key] as Record<string, unknown>;
  }
  const last = keys[keys.length - 1];
  if (Array.isArray(cur)) {
    const n = last === "-" ? cur.length : Number(last);
    if (!Number.isInteger(n) || n < 0 || n > cur.length) fail(i, "bad array index in data path");
    if (del) cur.splice(n, 1);
    else if (n === cur.length) cur.push(value);
    else cur[n] = value;
  } else if (del) delete (cur as Record<string, unknown>)[last];
  else (cur as Record<string, unknown>)[last] = value;
  return root;
}

/**
 * Applies `ops` to a copy of `card` and returns the checked result with rev + 1. Throws
 * `invalid` (op index in the message, or the write rule's `reason`) and changes nothing.
 */
export function applyPatch(card: CardV2, ops: PatchOp[]): CardV2 {
  if (card.rev >= MAX_CARD_V2_REVS) throw new ApiError("too_large", `at most ${MAX_CARD_V2_REVS} revs per card`, "card.rev_limit");
  const d = structuredClone(card) as unknown as Draft;
  ops.forEach((op, i) => {
    const at = (id: string) => d.blocks.findIndex((b) => b.id === id);
    switch (op.op) {
      case "set": {
        const k = at(op.id);
        if (k < 0) fail(i, `no block ${op.id}`);
        d.blocks[k] = { ...d.blocks[k], ...(op.props as object), id: d.blocks[k].id, type: d.blocks[k].type } as Block;
        break;
      }
      case "replace": {
        const k = at(op.id);
        if (k < 0) fail(i, `no block ${op.id}`);
        if (op.block.id !== op.id) fail(i, "replace keeps the block id");
        d.blocks[k] = structuredClone(op.block);
        break;
      }
      case "insert": {
        if (at(op.block.id) >= 0) fail(i, `block ${op.block.id} exists`);
        if (op.parent !== undefined) {
          const list = childList(d.blocks, op.parent);
          if (!list) fail(i, `no container ${op.parent}`);
          place(list as string[], op.block.id, op, i);
          d.blocks.push(structuredClone(op.block));
        } else {
          // Top-level reading order is the order of the blocks no container names.
          const named = new Set(d.blocks.flatMap((b) => childIds(b)));
          const top = d.blocks.filter((b) => !named.has(b.id)).map((b) => b.id);
          let k = d.blocks.length;
          if (op.before !== undefined) {
            if (!top.includes(op.before)) fail(i, `no top-level ${op.before} to insert before`);
            k = at(op.before);
          } else if (op.after !== undefined) {
            if (!top.includes(op.after)) fail(i, `no top-level ${op.after} to insert after`);
            k = at(op.after) + 1;
          } else if (op.index !== undefined && op.index < top.length) k = at(top[op.index]);
          d.blocks.splice(k, 0, structuredClone(op.block));
        }
        break;
      }
      case "remove": {
        if (at(op.id) < 0) fail(i, `no block ${op.id}`);
        const gone = new Set([op.id]);
        const walk = (id: string) => {
          const b = d.blocks.find((x) => x.id === id);
          if (b) for (const c of childIds(b)) if (!gone.has(c)) {
            gone.add(c);
            walk(c);
          }
        };
        walk(op.id);
        d.blocks = d.blocks.filter((b) => !gone.has(b.id));
        for (const b of d.blocks) {
          if ("children" in b && Array.isArray(b.children)) b.children = b.children.filter((c) => !gone.has(c));
          if (b.type === "compare") for (const o of b.options) if (o.children) o.children = o.children.filter((c) => !gone.has(c));
          if (b.type === "tabs") for (const t of b.tabs) t.children = t.children.filter((c) => !gone.has(c));
        }
        break;
      }
      case "data": {
        const next = pointer(d.data, op.path, op.value, op.value === undefined, i);
        if (next === undefined) delete d.data;
        else d.data = next;
        break;
      }
      case "card": {
        for (const [k, v] of Object.entries(op.props)) {
          if (v === undefined) continue;
          if ((k === "ask" || k === "status") && v === null) delete d[k];
          else d[k] = structuredClone(v);
        }
        break;
      }
    }
  });
  d.rev = card.rev + 1;
  return checkCard(d);
}

/** Parses a card with the write schema; the first problem becomes `invalid` with its reason. */
export function checkCard(raw: unknown): CardV2 {
  const r = CardV2.safeParse(raw);
  if (r.success) return r.data;
  const issue = r.error.issues.find((x) => schemaReason({}, x.message)) ?? r.error.issues[0];
  const reason =
    schemaReason(raw as Record<string, unknown>, issue?.message) ?? schemaReason({ cardV2: raw }, issue?.message, ["cardV2", ...(issue?.path ?? [])]);
  if (/exceeds 64 KiB/.test(issue?.message ?? "")) throw new ApiError("too_large", "card exceeds 64 KiB");
  throw new ApiError("invalid", `the patched card: ${issue?.path.join(".") || "(root)"} ${issue?.message ?? ""}`.trim(), reason);
}
