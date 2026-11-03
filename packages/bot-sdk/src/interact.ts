// Helpers of the 1.1 interaction loop: `CardDraft` records `card.patch` ops against the card a
// handler sees, and `opsFit` tells whether recorded ops still fit a newer rev (the SDK rebases
// a patch on `card.stale_rev` only when they do).
import type { CardAsk, CardV2, CardV2Action, CardV2Block, PatchOp } from "@chat/protocol";
import { flattenBlocks } from "./blocks.ts";

type CardProps = Extract<PatchOp, { op: "card" }>["props"];
type Placement = { parent?: string; after?: string; before?: string; index?: number };

/** Child ids a block names (containers, compare options, tabs). */
function childrenOf(b: CardV2Block): string[] {
  switch (b.type) {
    case "group":
      return b.children ?? [];
    case "stack":
    case "row":
    case "questionnaire":
      return b.children;
    case "compare":
      return b.options.flatMap((o) => o.children ?? []);
    case "tabs":
      return b.tabs.flatMap((t) => t.children);
    default:
      return [];
  }
}

/** Ids a `parent` of an insert may name: blocks, compare option ids and tab ids. */
function parentIds(card: CardV2): Set<string> {
  const out = new Set<string>();
  for (const b of card.blocks) {
    out.add(b.id);
    if (b.type === "compare") for (const o of b.options) out.add(o.id);
    if (b.type === "tabs") for (const t of b.tabs) out.add(t.id);
    if (b.type === "group") for (const c of b.blocks ?? []) out.add(c.id);
  }
  return out;
}

/**
 * True when every op still targets an existing id of `card` (ids an earlier op inserts count;
 * ids an earlier op removes do not). Used before a rebase.
 */
export function opsFit(card: CardV2, ops: PatchOp[]): boolean {
  const known = parentIds(card);
  const byId = new Map(card.blocks.map((b) => [b.id, b] as const));
  const drop = (id: string) => {
    known.delete(id);
    const b = byId.get(id);
    if (b) for (const c of childrenOf(b)) drop(c);
  };
  for (const op of ops) {
    switch (op.op) {
      case "set":
      case "replace":
        if (!known.has(op.id)) return false;
        if (op.op === "replace") byId.set(op.id, op.block);
        break;
      case "remove":
        if (!known.has(op.id)) return false;
        drop(op.id);
        break;
      case "insert":
        if (known.has(op.block.id)) return false;
        for (const ref of [op.parent, op.after, op.before]) if (ref !== undefined && !known.has(ref)) return false;
        known.add(op.block.id);
        byId.set(op.block.id, op.block);
        if (op.block.type === "compare") for (const o of op.block.options) known.add(o.id);
        if (op.block.type === "tabs") for (const t of op.block.tabs) known.add(t.id);
        break;
      default:
        break;
    }
  }
  return true;
}

/**
 * Records patch ops. `card` is the rev the ops are made for (read it, do not change it).
 * Inserting a container also inserts its nested child blocks (built with `blocks.*`).
 */
export class CardDraft {
  readonly card: CardV2;
  readonly ops: PatchOp[] = [];

  constructor(card: CardV2) {
    this.card = card;
  }

  /** A block of the card by id (top level or a 2.0 inline group child). */
  block(id: string): CardV2Block | undefined {
    for (const b of this.card.blocks) {
      if (b.id === id) return b;
      if (b.type === "group") for (const c of b.blocks ?? []) if (c.id === id) return c as CardV2Block;
    }
    return undefined;
  }

  /** Shallow-merges `props` into a block (not id, type, children, options or tabs). */
  set(id: string, props: Record<string, unknown>): this {
    this.ops.push({ op: "set", id, props: props as Extract<PatchOp, { op: "set" }>["props"] });
    return this;
  }
  /** Replaces a whole block (same id, any type). */
  replace(id: string, block: CardV2Block): this {
    this.ops.push({ op: "replace", id, block: { ...block, id } });
    for (const c of flattenBlocks([block]).slice(1)) this.ops.push({ op: "insert", block: c });
    return this;
  }
  /** Adds a block; `parent` is a container, compare option or tab id. Default: appended at the top level. */
  insert(block: CardV2Block, at: Placement = {}): this {
    const [first, ...nested] = flattenBlocks([block]);
    if (!first) return this;
    this.ops.push({ op: "insert", block: first, ...at });
    for (const c of nested) this.ops.push({ op: "insert", block: c });
    return this;
  }
  /** Removes a block, its children too. */
  remove(id: string): this {
    this.ops.push({ op: "remove", id });
    return this;
  }
  /** Sets (or with no value deletes) a JSON Pointer into `data`; "" is the whole value. */
  data(path: string, value?: unknown): this {
    const op: PatchOp = { op: "data", path };
    if (value !== undefined) op.value = value as never;
    this.ops.push(op);
    return this;
  }
  /** Card fields: title, summary, intent, fallback, actions, and `ask` (null closes it; a new id opens a new round). */
  cardProps(props: CardProps): this {
    this.ops.push({ op: "card", props });
    return this;
  }
  /** Opens a new ask round (a new `ask.id`), or changes the open one (same id). */
  ask(ask: CardAsk | null): this {
    return this.cardProps({ ask });
  }
  actions(actions: CardV2Action[]): this {
    return this.cardProps({ actions });
  }
}

/** What `patch` accepts: ops, or a function that records them on a draft of the newest rev. */
export type PatchInput = PatchOp[] | ((draft: CardDraft) => void);

/** The ops of one input against `card`. */
export function opsOf(card: CardV2, input: PatchInput): PatchOp[] {
  if (Array.isArray(input)) return input;
  const d = new CardDraft(card);
  input(d);
  return d.ops;
}
