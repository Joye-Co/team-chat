// Helpers of the 1.1 interaction loop: `CardDraft` records `card.patch` ops against the card a
// handler sees, and `opsFit` tells whether recorded ops still fit a newer rev (the SDK rebases
// a patch on `card.stale_rev` only when they do).
import type { CardAsk, CardV2, CardV2Action, CardV2Block, PatchOp } from "@chat/protocol";
import { flattenBlocks } from "./blocks.ts";
import { ChatError } from "./errors.ts";

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

/** The ids a block brings besides its own: compare option ids and tab ids (an insert's `parent`). */
function ownIds(b: CardV2Block): string[] {
  if (b.type === "compare") return b.options.map((o) => o.id);
  if (b.type === "tabs") return b.tabs.map((t) => t.id);
  if (b.type === "group") return (b.blocks ?? []).map((c) => c.id);
  return [];
}

/** The ids ops may name while they are applied in order: an insert adds ids, a remove drops them. */
class KnownIds {
  private readonly known = new Set<string>();
  private readonly byId = new Map<string, CardV2Block>();

  constructor(card: CardV2) {
    for (const b of card.blocks) this.add(b);
  }

  has(id: string): boolean {
    return this.known.has(id);
  }
  add(block: CardV2Block): void {
    this.known.add(block.id);
    this.byId.set(block.id, block);
    for (const id of ownIds(block)) this.known.add(id);
  }
  replace(id: string, block: CardV2Block): void {
    this.byId.set(id, block);
    for (const x of ownIds(block)) this.known.add(x);
  }
  remove(id: string): void {
    this.known.delete(id);
    const b = this.byId.get(id);
    if (b) for (const c of childrenOf(b)) this.remove(c);
  }
  /** The block ids the card has now, in order, for an error text. */
  list(): string {
    const ids = [...this.byId.keys()].filter((id) => this.known.has(id));
    return ids.length > 16 ? `${ids.slice(0, 16).join(", ")}, … (${ids.length} blocks)` : ids.join(", ");
  }
}

/**
 * True when every op still targets an existing id of `card` (ids an earlier op inserts count;
 * ids an earlier op removes do not). Used before a rebase.
 */
export function opsFit(card: CardV2, ops: PatchOp[]): boolean {
  const known = new KnownIds(card);
  for (const op of ops) {
    switch (op.op) {
      case "set":
        if (!known.has(op.id)) return false;
        break;
      case "replace":
        if (!known.has(op.id)) return false;
        known.replace(op.id, op.block);
        break;
      case "remove":
        if (!known.has(op.id)) return false;
        known.remove(op.id);
        break;
      case "insert":
        if (known.has(op.block.id)) return false;
        for (const ref of [op.parent, op.after, op.before]) if (ref !== undefined && !known.has(ref)) return false;
        known.add(op.block);
        break;
      default:
        break;
    }
  }
  return true;
}

type BlockOf<T extends CardV2Block["type"]> = Extract<CardV2Block, { type: T }>;
/** The fields `CardDraft.set` may change of a block of type `T`. */
export type BlockProps<T extends CardV2Block["type"]> = Partial<Omit<BlockOf<T>, "id" | "type">>;

/**
 * Records patch ops. `card` is the rev the ops are made for (read it, do not change it).
 * Inserting a container also inserts its nested child blocks (built with `blocks.*`). An op
 * that names a block the card does not have (counting the ops recorded so far) throws
 * `ChatError` `invalid` with the ids the card has, before anything is sent. Raw op arrays
 * passed to `patch` are not checked here.
 */
export class CardDraft {
  readonly card: CardV2;
  readonly ops: PatchOp[] = [];
  private readonly known: KnownIds;

  constructor(card: CardV2) {
    this.card = card;
    this.known = new KnownIds(card);
  }

  private need(id: string, what: string): void {
    if (this.known.has(id)) return;
    throw new ChatError("invalid", `${what}: no block "${id}" in rev ${this.card.rev}; blocks: ${this.known.list() || "(none)"}`);
  }

  private add(block: CardV2Block, at: Placement): void {
    if (this.known.has(block.id)) {
      throw new ChatError("invalid", `insert: rev ${this.card.rev} has a block "${block.id}" already; use set or replace`);
    }
    this.ops.push({ op: "insert", block, ...at });
    this.known.add(block);
  }

  /** A block of the card by id (top level or a 2.0 inline group child). */
  block(id: string): CardV2Block | undefined {
    for (const b of this.card.blocks) {
      if (b.id === id) return b;
      if (b.type === "group") for (const c of b.blocks ?? []) if (c.id === id) return c as CardV2Block;
    }
    return undefined;
  }

  /**
   * Shallow-merges `props` into a block (not id, type, children, options or tabs). Name the
   * block's type to have the fields checked: `d.set<"metric">("m_dau", { value: 2341 })`.
   */
  set<T extends CardV2Block["type"]>(id: string, props: BlockProps<T>): this;
  set(id: string, props: Record<string, unknown>): this;
  set(id: string, props: Record<string, unknown>): this {
    this.need(id, "set");
    this.ops.push({ op: "set", id, props: props as Extract<PatchOp, { op: "set" }>["props"] });
    return this;
  }
  /** Replaces a whole block (same id, any type). */
  replace(id: string, block: CardV2Block): this {
    this.need(id, "replace");
    const [first, ...nested] = flattenBlocks([block]);
    const next = { ...(first ?? block), id } as CardV2Block;
    this.ops.push({ op: "replace", id, block: next });
    this.known.replace(id, next);
    for (const c of nested) this.add(c, {});
    return this;
  }
  /** Adds a block; `parent` is a container, compare option or tab id. Default: appended at the top level. */
  insert(block: CardV2Block, at: Placement = {}): this {
    const [first, ...nested] = flattenBlocks([block]);
    if (!first) return this;
    for (const key of ["parent", "after", "before"] as const) {
      const ref = at[key];
      if (ref !== undefined) this.need(ref, `insert "${first.id}" ${key}`);
    }
    this.add(first, at);
    for (const c of nested) this.add(c, {});
    return this;
  }
  /** Removes a block, its children too. */
  remove(id: string): this {
    this.need(id, "remove");
    this.ops.push({ op: "remove", id });
    this.known.remove(id);
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
  /** Replaces the card's actions (its submit, cancel and other buttons); `[]` removes them. */
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
