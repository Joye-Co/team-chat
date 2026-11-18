// Card v2.1 builders (catalog tc.card/2.1): one function per block type, containers that take
// child blocks or ids, `composeCard` that flattens the children into the card's flat block
// list, and `parts` for part refs. The member picks structure and meaning; the client owns
// layout and style, so no builder takes a style. See docs/bots.md, "Cards v2.1".
import {
  CARD_21_TYPES,
  CARD_CATALOG_21,
  type CardAsk,
  type CardV2,
  type CardV2Action,
  type CardV2Block,
  checkDiagramSource,
  type DiagramCheck,
  type PartRef,
} from "@chat/protocol";
import { checkCardV2 } from "./cards.ts";

type BlockOf<T extends CardV2Block["type"]> = Extract<CardV2Block, { type: T }>;
/** A builder's input: the block without `type`; `fallbackText` gets a default where the protocol needs one. */
type Input<T extends CardV2Block["type"]> = Omit<BlockOf<T>, "type">;
type InputFb<T extends CardV2Block["type"]> = Omit<Input<T>, "fallbackText"> & { fallbackText?: string };

/** A child of a container: a block (it joins the card's flat list) or the id of a block of the card. */
export type Child = string | CardV2Block;

const NESTED = Symbol("nested");
type WithNested = CardV2Block & { [NESTED]?: CardV2Block[] };

/** Keeps the nested child blocks next to the container without putting them on the wire. */
function nest<B extends CardV2Block>(block: B, children: Child[]): B {
  const objs = children.filter((c): c is CardV2Block => typeof c !== "string");
  if (objs.length > 0) Object.defineProperty(block, NESTED, { value: objs, enumerable: false });
  return block;
}

function ids(children: Child[]): string[] {
  return children.map((c) => (typeof c === "string" ? c : c.id));
}

/** Cuts plain text to `max` code points (with "…"). */
function cut(s: string, max: number): string {
  const cps = [...s];
  return cps.length <= max ? s : `${cps.slice(0, max - 1).join("")}…`;
}

function fb<T extends { fallbackText?: string }>(o: T, text: string): T & { fallbackText: string } {
  return { ...o, fallbackText: o.fallbackText ?? cut(text, 300) };
}

/** Node labels of a Mermaid source, in order: `A[label]`, `B(label)`, `C{label}`, `D>label]`. */
export function diagramLabels(source: string): string[] {
  const out: string[] = [];
  const re = /\b[A-Za-z0-9_]+\s*(?:\[\[|\[\(|\(\(|\[|\(|\{|>)\s*"?([^\]\)\}"]+?)"?\s*(?:\]\]|\)\]|\)\)|\]|\)|\})/g;
  for (const m of source.matchAll(re)) {
    const l = m[1]?.trim();
    if (l && !out.includes(l)) out.push(l);
  }
  return out;
}

/** A Mermaid diagram as a fenced block for a message body. */
export function mermaid(source: string): string {
  return `\`\`\`mermaid\n${source.replace(/\n+$/, "")}\n\`\`\``;
}

/** The server's static checks of a diagram source (size, keyword allowlist, forbidden text). No rendering. */
export function checkDiagram(source: string): DiagramCheck {
  return checkDiagramSource(source);
}

export const blocks = {
  // --- 2.0 leaves ------------------------------------------------------------
  /** A paragraph of text. */
  text: (id: string, text: string): CardV2Block => ({ type: "text", id, text }),
  /** Label and value pairs. */
  fields: (o: Input<"fields">): CardV2Block => ({ type: "fields", ...o }),
  /** A table: `columns` and `rows` of cells; `rowKeys` give rows stable ids for refs and row events. */
  table: (o: Input<"table">): CardV2Block => ({ type: "table", ...o }),
  /** A list of items. */
  list: (o: Input<"list">): CardV2Block => ({ type: "list", ...o }),
  /** A progress bar: `value` from 0 to 1. */
  progress: (o: Input<"progress">): CardV2Block => ({ type: "progress", ...o }),
  /** A block of code. */
  code: (o: Input<"code">): CardV2Block => ({ type: "code", ...o }),
  /** A picture from an uploaded file of the group. */
  image: (o: Input<"image">): CardV2Block => ({ type: "image", ...o }),
  /** A line between blocks. */
  divider: (id: string): CardV2Block => ({ type: "divider", id }),
  /** A labelled link to its `target`. */
  link: (o: Input<"link">): CardV2Block => ({ type: "link", ...o }),
  /** CARD21 `choice` (wire name `decision`): single, or `multiple` with min/max, "other", recommended options. */
  decision: (o: Input<"decision">): CardV2Block => ({ type: "decision", ...o }),
  /** One input of an ask (`kind`: text, textarea, number, select, date, switch, chips, slider); its value is in the answer's `inputs[id]`. */
  input: (o: Input<"input">): CardV2Block => ({ type: "input", ...o }),
  /** An input of kind `slider` (2.1): `min` < `max`, optional `step` and `unit`. */
  slider: (o: Omit<Input<"input">, "kind" | "min" | "max"> & { min: number; max: number }): CardV2Block => ({
    type: "input",
    kind: "slider",
    ...o,
  }),
  /** Markdown the answerer may edit before the submit; the edited text is in the answer's `editedValues[id]`. */
  draft: (o: Input<"draft">): CardV2Block => ({ type: "draft", ...o }),
  /** A run's plan: live with `runId`; `items` is the snapshot for readers without the run. */
  plan: (o: Input<"plan">): CardV2Block => ({ type: "plan", ...o }),
  /** One number with its label, `unit` and the change against the last value (`delta`, `better`). */
  metric: (o: Input<"metric">): CardV2Block => ({ type: "metric", ...o }),
  /** A chart (`kind`): `x` labels and 1 or more `series` of values. */
  chart: (o: Input<"chart">): CardV2Block => ({ type: "chart", ...o }),
  /** A unified diff of one file. */
  diff: (o: Input<"diff">): CardV2Block => ({ type: "diff", ...o }),
  /** One line of proof: a label with a state, and a file, link, 产出, test counts or a recording. */
  evidence: (o: Input<"evidence">): CardV2Block => ({ type: "evidence", ...o }),
  /** A labelled reference to a `target` (a pull request, for example). */
  ref: (o: Input<"ref">): CardV2Block => ({ type: "ref", ...o }),

  // --- 2.1 choice variants -----------------------------------------------------
  /** Put 2–8 items in order; `rankTop` takes only the first n as the answer. */
  rank: (o: InputFb<"rank">): CardV2Block => ({ type: "rank", ...fb(o, o.label) }),
  /** 1 to 5 stars (`max` is always 5). */
  rating: (o: Omit<InputFb<"rating">, "max">): CardV2Block => ({ type: "rating", max: 5, ...fb(o, o.label) }),
  /** A whole number from `min` to `max`, with optional words at the two ends. */
  scale: (o: InputFb<"scale">): CardV2Block => ({ type: "scale", ...fb(o, o.label) }),
  /** A yes or no question with its own two labels. */
  confirm: (o: InputFb<"confirm">): CardV2Block => ({ type: "confirm", ...fb(o, `${o.label}（${o.yes} / ${o.no}）`) }),
  /** Choose between 2–4 pictures (uploaded files). */
  imageChoice: (o: InputFb<"imageChoice">): CardV2Block => ({
    type: "imageChoice",
    ...fb(o, `${o.label}：${o.options.map((x) => x.label).join(" / ")}`),
  }),
  /** One pick per row from the same columns. */
  matrix: (o: InputFb<"matrix">): CardV2Block => ({ type: "matrix", ...fb(o, o.label) }),
  /** Pick from up to 12 time slots, shown in the time zone `tz`. */
  slots: (o: InputFb<"slots">): CardV2Block => ({ type: "slots", ...fb(o, o.label) }),
  /** A vote of the group (or of `voters`) on 2–6 options until `closesAt`. */
  poll: (o: InputFb<"poll">): CardV2Block => ({ type: "poll", ...fb(o, `${o.label}：${o.options.map((x) => x.label).join(" / ")}`) }),

  // --- 2.1 other leaves --------------------------------------------------------
  /** 1–3 inline buttons; they send `button.press` (declare it in `events`) and never answer an ask. */
  buttons: (o: InputFb<"buttons">): CardV2Block => ({ type: "buttons", ...fb(o, o.buttons.map((b) => b.label).join(" / ")) }),
  /** A Mermaid diagram. `fallbackText` defaults to `title`, else `图：<first node labels>`. */
  diagram: (o: Omit<InputFb<"diagram">, "syntax"> & { syntax?: string }): CardV2Block => {
    const labels = diagramLabels(o.source).slice(0, 5);
    const text = o.title ?? (labels.length > 0 ? `图：${labels.join("、")}` : "图");
    return { type: "diagram", syntax: o.syntax ?? "mermaid", ...fb(o, text) };
  },
  /** A live 产出 version inline. `events` must be declared on that version. */
  embed: (o: InputFb<"embed">): CardV2Block => ({ type: "embed", ...fb(o, o.title ?? "页面") }),

  // --- 2.1 containers (children: blocks or ids) ---------------------------------
  /** Children one under the other. */
  stack: (id: string, children: Child[]): CardV2Block => nest({ type: "stack", id, children: ids(children) }, children),
  /** 2–3 columns side by side; `weights` (1..3 each) are the only proportion hint. Folds to a stack when narrow. */
  row: (id: string, children: Child[], o: { weights?: (1 | 2 | 3)[]; emphasis?: "high" | "low" } = {}): CardV2Block =>
    nest({ type: "row", id, children: ids(children), ...o }, children),
  /** A titled section, `collapsible` when asked. */
  group: (o: Omit<Input<"group">, "children" | "blocks"> & { children: Child[] }): CardV2Block =>
    nest({ ...o, type: "group", children: ids(o.children) }, o.children),
  /** Up to 4 options side by side (pros, cons, own children) to pick one from. */
  compare: (
    o: Omit<Input<"compare">, "options"> & {
      options: (Omit<BlockOf<"compare">["options"][number], "children"> & { children?: Child[] })[];
    },
  ): CardV2Block => {
    const all: Child[] = o.options.flatMap((x) => x.children ?? []);
    const options = o.options.map((x) => (x.children ? { ...x, children: ids(x.children) } : { ...x }));
    const text = o.question ?? o.options.map((x) => x.label).join(" / ");
    return nest({ ...o, fallbackText: o.fallbackText ?? cut(text || "比较", 300), type: "compare", options } as CardV2Block, all);
  },
  /** Up to 5 tabs, each with its own children; `selected` is the tab shown first. */
  tabs: (o: Omit<Input<"tabs">, "tabs"> & { tabs: { id: string; label: string; children: Child[] }[] }): CardV2Block => {
    const all: Child[] = o.tabs.flatMap((t) => t.children);
    return nest({ ...o, type: "tabs", tabs: o.tabs.map((t) => ({ ...t, children: ids(t.children) })) }, all);
  },
  /** 2–5 numbered questions with one submit. */
  questionnaire: (o: Omit<Input<"questionnaire">, "children"> & { children: Child[] }): CardV2Block =>
    nest(
      { ...o, fallbackText: o.fallbackText ?? `问卷（${o.children.length} 题）`, type: "questionnaire", children: ids(o.children) },
      o.children,
    ),
};
/** CARD21's name for the `decision` block. */
export const choice = blocks.decision;

/** Every block of a list with its nested children after it (depth first). */
export function flattenBlocks(list: CardV2Block[]): CardV2Block[] {
  const out: CardV2Block[] = [];
  const walk = (b: CardV2Block) => {
    out.push(b);
    for (const c of (b as WithNested)[NESTED] ?? []) walk(c);
  };
  for (const b of list) walk(b);
  return out;
}

export interface ComposeCardOptions {
  intent: CardV2["intent"];
  title: string;
  summary?: string;
  /** Plain text for notifications, VoiceOver and 1.0.x readers. Default: summary, else the title. */
  fallback?: string;
  ask?: CardAsk;
  data?: CardV2["data"];
  /** Top-level blocks in reading order; containers bring their nested children. */
  blocks: CardV2Block[];
  actions?: CardV2Action[];
  /** Default "tc.card/2.1" when a 2.1 block type is used, else none (2.0). */
  catalog?: string;
  rev?: number;
}

/** Makes a v2 card from top-level blocks (containers flatten), and checks every write rule. */
export function composeCard(o: ComposeCardOptions): CardV2 {
  const list = flattenBlocks(o.blocks);
  const uses21 = list.some((b) => CARD_21_TYPES.has(b.type) || (b.type === "group" && b.children !== undefined) || b.events !== undefined);
  const card: CardV2 = {
    v: 2,
    rev: o.rev ?? 1,
    intent: o.intent,
    title: o.title,
    fallback: o.fallback ?? o.summary ?? o.title,
    blocks: list,
  };
  const catalog = o.catalog ?? (uses21 ? CARD_CATALOG_21 : undefined);
  if (catalog) card.catalog = catalog;
  if (o.summary !== undefined) card.summary = o.summary;
  if (o.ask) card.ask = o.ask;
  if (o.data !== undefined) card.data = o.data;
  if (o.actions) card.actions = o.actions;
  return checkCardV2(card);
}

/** Part addresses (`PartRef`, docs/protocol.md "References to a part"). Keys are ids or 0-based indices. */
export const parts = {
  cell: (row: string | number, col: number): PartRef => ({ kind: "cell", keys: { row, col } }),
  row: (row: string | number): PartRef => ({ kind: "row", keys: { row } }),
  column: (col: number): PartRef => ({ kind: "column", keys: { col } }),
  point: (series: string, x: string | number): PartRef => ({ kind: "point", keys: { series, x } }),
  bar: (series: string, x: string | number): PartRef => ({ kind: "bar", keys: { series, x } }),
  series: (series: string): PartRef => ({ kind: "series", keys: { series } }),
  metric: (): PartRef => ({ kind: "metric" }),
  item: (item: string | number): PartRef => ({ kind: "item", keys: { item } }),
  step: (step: number): PartRef => ({ kind: "step", keys: { step } }),
  line: (n: number, file?: string): PartRef => ({ kind: "line", keys: file === undefined ? { n } : { file, n } }),
  diffLine: (file: string, side: "old" | "new", n: number): PartRef => ({ kind: "line", keys: { file, side, n } }),
  hunk: (file: string, hunk: number): PartRef => ({ kind: "hunk", keys: { file, hunk } }),
  image: (item?: number): PartRef => (item === undefined ? { kind: "image" } : { kind: "image", keys: { item } }),
  node: (node: string): PartRef => ({ kind: "node", keys: { node } }),
  element: (selector?: string): PartRef => (selector === undefined ? { kind: "element" } : { kind: "element", keys: { selector } }),
};
