// Card v2: semantic blocks, intents, asks with inputs, and answers. It travels in the optional
// `Message.cardV2` next to a server-derived v1 `card`, so 1.0.x clients still render the
// message. See docs/protocol.md, "Cards v2".
import { z } from "zod";
import {
  ActionId,
  Card,
  type CardAction,
  type CardBlock,
  CardTone,
  CodeBlock,
  DividerBlock,
  FieldsBlock,
  ImageBlock,
  LinkBlock,
  type LinkTarget,
  ListBlock,
  ProgressBlock,
  TableBlock,
  TextBlock,
  WebUrl,
} from "./card.ts";
import { Count, eventBase, Id, Millis, Rid, Seq, utf8Bytes } from "./common.ts";
import { RunPlan } from "./plan.ts";

export const CARD_V2_VERSION = 2 as const;
/** Max size of a v2 card: UTF-8 bytes of its JSON text. */
export const MAX_CARD_V2_BYTES = 65_536;
/** Max blocks of a v2 card, nested group children included. */
export const MAX_CARD_V2_BLOCKS = 50;
export const MAX_CARD_V2_ACTIONS = 10;
export const MAX_GROUP_BLOCKS = 20;
export const MAX_DECISION_OPTIONS = 10;
export const MAX_INPUT_OPTIONS = 50;
export const MAX_CHART_SERIES = 8;
export const MAX_CHART_POINTS = 200;
export const MAX_ASK_FROM = 20;
/** Max UTF-8 bytes of `data` as JSON. */
export const MAX_CARD_DATA_BYTES = 16_384;
/** Max UTF-8 bytes of `inputs` and `editedValues` of one answer, as JSON together. */
export const MAX_CARD_ANSWER_BYTES = 65_536;
/** Max UTF-8 bytes of an input's `default` as JSON. */
export const MAX_INPUT_DEFAULT_BYTES = 4096;

const Short = (max: number) => z.string().min(1).max(max);
const Text = (max: number) => z.string().max(max);
const jsonMax = (max: number, what: string) =>
  z.json().refine((v) => utf8Bytes(JSON.stringify(v)) <= max, { message: `${what} exceeds ${max} bytes` });

/** Block, option and action ids: 1..64 of `[A-Za-z0-9_.:-]`. */
export const BlockId = ActionId;

export const CardIntent = z.enum(["ask", "done", "working", "info"]);
export type CardIntent = z.infer<typeof CardIntent>;

/**
 * Who may answer an ask: member ids, `"requester"` (the topic's creator; a bot that knows a
 * run's requester lists that member's id) and `"owners"` (every owner).
 */
export const AskFrom = z.string().min(1).max(64);
export const AskIrreversible = z.object({
  /** The action, e.g. "发布". */
  verb: Short(40),
  /** What it acts on, e.g. "qqbot 1.4 到正式环境". */
  target: Short(200),
  /** What cannot be undone. */
  consequence: Short(500),
  /** When set, the answerer types this text before the submit action is enabled. */
  typeToConfirm: Short(100).optional(),
});
export type AskIrreversible = z.infer<typeof AskIrreversible>;

export const CardAsk = z.object({
  from: z.array(AskFrom).min(1).max(MAX_ASK_FROM),
  /** After this time the ask takes no answer (`conflict`). */
  expiresAt: Millis.optional(),
  /** An answer is a group decision (群里的决定). */
  record: z.boolean().optional(),
  irreversible: AskIrreversible.optional(),
});
export type CardAsk = z.infer<typeof CardAsk>;

export const RefTarget = z.discriminatedUnion("kind", [
  z.object({
    kind: z.literal("pr"),
    url: WebUrl,
    repo: Text(200).optional(),
    number: z.int().positive().optional(),
    /** Plain text, e.g. "open", "merged", "closed", "draft". */
    state: Short(32).optional(),
  }),
  z.object({ kind: z.literal("artifact"), artifactId: Id, version: z.int().positive().optional() }),
  z.object({ kind: z.literal("topic"), groupId: Id, topicId: Id }),
  z.object({ kind: z.literal("member"), memberId: Id }),
  z.object({ kind: z.literal("url"), url: WebUrl }),
]);
export type RefTarget = z.infer<typeof RefTarget>;

const id = { id: BlockId };
/** Same shape as `ArtifactRef` (message.ts); kept here so message.ts can import this module. */
const ArtifactRef = z.object({ artifactId: Id, version: z.int().positive() });
const Option_ = z.object({ id: ActionId, label: Short(200), detail: Text(1000).optional() });
const InputOption = z.object({ id: ActionId, label: Short(200) });

export const InputKind = z.enum(["text", "textarea", "number", "select", "date", "switch", "chips"]);
export type InputKind = z.infer<typeof InputKind>;
export const ChartKind = z.enum(["line", "bar", "area", "spark"]);
export type ChartKind = z.infer<typeof ChartKind>;
export const MetricBetter = z.enum(["up", "down"]);
export type MetricBetter = z.infer<typeof MetricBetter>;
export const EvidenceState = z.enum(["pass", "fail", "info"]);
export type EvidenceState = z.infer<typeof EvidenceState>;

// v1 block types, each with an id.
export const TextBlockV2 = TextBlock.extend(id);
export const FieldsBlockV2 = FieldsBlock.extend(id);
export const TableBlockV2 = TableBlock.safeExtend(id);
export const ListBlockV2 = ListBlock.extend(id);
export const ProgressBlockV2 = ProgressBlock.extend(id);
export const CodeBlockV2 = CodeBlock.extend(id);
export const ImageBlockV2 = ImageBlock.extend(id);
export const DividerBlockV2 = DividerBlock.extend(id);
export const LinkBlockV2 = LinkBlock.extend(id);

export const DecisionBlock = z.object({
  type: z.literal("decision"),
  ...id,
  options: z.array(Option_).min(2).max(MAX_DECISION_OPTIONS),
  /** More than one option may be chosen. */
  multiple: z.boolean().optional(),
  /** Option ids chosen at first. */
  default: z.array(ActionId).max(MAX_DECISION_OPTIONS).optional(),
});
export const InputBlock = z.object({
  type: z.literal("input"),
  ...id,
  kind: InputKind,
  label: Short(200),
  placeholder: Text(200).optional(),
  required: z.boolean().optional(),
  /** Choices of `select` and `chips`. */
  options: z.array(InputOption).max(MAX_INPUT_OPTIONS).optional(),
  min: z.number().optional(),
  max: z.number().optional(),
  maxLength: z.int().positive().max(32_768).optional(),
  default: jsonMax(MAX_INPUT_DEFAULT_BYTES, "default").optional(),
});
export const DraftBlock = z.object({
  type: z.literal("draft"),
  ...id,
  label: Text(200).optional(),
  /** Markdown the answerer may edit before submit (answer `editedValues[id]`). */
  text: Text(32_768),
  editable: z.boolean().optional(),
});
export const PlanBlock = z.object({
  type: z.literal("plan"),
  ...id,
  /** Show this run's live plan; `items` is the snapshot for readers without the run. */
  runId: Id.optional(),
  items: RunPlan.optional(),
});
export const MetricBlock = z.object({
  type: z.literal("metric"),
  ...id,
  label: Short(100),
  value: z.number(),
  unit: Text(16).optional(),
  /** Change against the last value, in the same unit. */
  delta: z.number().optional(),
  /** Which direction of `delta` is good. */
  better: MetricBetter.optional(),
  note: Text(200).optional(),
});
export const ChartBlock = z.object({
  type: z.literal("chart"),
  ...id,
  kind: ChartKind,
  label: Text(200).optional(),
  unit: Text(16).optional(),
  /** Labels of the x axis, one per point. */
  x: z.array(Text(40)).max(MAX_CHART_POINTS).optional(),
  series: z
    .array(z.object({ name: Short(100), values: z.array(z.number()).max(MAX_CHART_POINTS) }))
    .min(1)
    .max(MAX_CHART_SERIES),
});
export const DiffBlock = z.object({
  type: z.literal("diff"),
  ...id,
  path: Text(1024).optional(),
  /** Unified diff. */
  text: Text(32_768),
  additions: Count.optional(),
  deletions: Count.optional(),
});
export const EvidenceBlock = z.object({
  type: z.literal("evidence"),
  ...id,
  label: Short(200),
  state: EvidenceState.optional(),
  detail: Text(1000).optional(),
  fileId: Id.optional(),
  url: WebUrl.optional(),
  artifact: ArtifactRef.optional(),
});
export const RefBlock = z.object({ type: z.literal("ref"), ...id, label: Short(200), target: RefTarget });

/** Every block type except `group`. */
export const CardV2LeafBlock = z.discriminatedUnion("type", [
  TextBlockV2,
  FieldsBlockV2,
  TableBlockV2,
  ListBlockV2,
  ProgressBlockV2,
  CodeBlockV2,
  ImageBlockV2,
  DividerBlockV2,
  LinkBlockV2,
  DecisionBlock,
  InputBlock,
  DraftBlock,
  PlanBlock,
  MetricBlock,
  ChartBlock,
  DiffBlock,
  EvidenceBlock,
  RefBlock,
]);
export type CardV2LeafBlock = z.infer<typeof CardV2LeafBlock>;
export const GroupBlock = z.object({
  type: z.literal("group"),
  ...id,
  label: Text(200).optional(),
  /** One level only: no group inside a group. */
  blocks: z.array(CardV2LeafBlock).min(1).max(MAX_GROUP_BLOCKS),
});
export const CardV2Block = z.discriminatedUnion("type", [...CardV2LeafBlock.options, GroupBlock]);
export type CardV2Block = z.infer<typeof CardV2Block>;

export const CardActionKind = z.enum(["submit", "cancel", "link"]);
export type CardActionKind = z.infer<typeof CardActionKind>;
export const CardV2ActionStyle = z.enum(["primary", "default", "danger"]);
export type CardV2ActionStyle = z.infer<typeof CardV2ActionStyle>;

export const CardV2Action = z.object({
  id: ActionId,
  label: Short(40),
  style: CardV2ActionStyle.optional(),
  /** Absent = "submit". */
  kind: CardActionKind.optional(),
  /** Where a `link` action goes. Required for `link`. */
  target: RefTarget.optional(),
  /** When set, the client asks before it sends the answer. */
  confirm: z.object({ title: Short(200), body: Text(1000).optional() }).optional(),
});
export type CardV2Action = z.infer<typeof CardV2Action>;

function blockIds(blocks: CardV2Block[]): string[] {
  return blocks.flatMap((b) => (b.type === "group" ? [b.id, ...b.blocks.map((c) => c.id)] : [b.id]));
}

export const CardV2 = z
  .object({
    v: z.literal(CARD_V2_VERSION),
    /** 1 at first; every edit sends a higher rev. Answers name the rev they answer. */
    rev: z.int().positive(),
    intent: CardIntent,
    title: Short(200),
    /** Plain text for old clients, notifications and search; also the v1 card's first block. */
    fallback: Text(4000),
    ask: CardAsk.optional(),
    blocks: z.array(CardV2Block).max(MAX_CARD_V2_BLOCKS),
    actions: z.array(CardV2Action).max(MAX_CARD_V2_ACTIONS).optional(),
    /** Bot state, returned to nobody; at most 16 KiB. */
    data: jsonMax(MAX_CARD_DATA_BYTES, "data").optional(),
  })
  .refine((c) => blockIds(c.blocks).length <= MAX_CARD_V2_BLOCKS, {
    message: `at most ${MAX_CARD_V2_BLOCKS} blocks`,
    path: ["blocks"],
  })
  .refine((c) => new Set(blockIds(c.blocks)).size === blockIds(c.blocks).length, {
    message: "block ids are unique",
    path: ["blocks"],
  })
  .refine((c) => new Set((c.actions ?? []).map((a) => a.id)).size === (c.actions ?? []).length, {
    message: "action ids are unique",
    path: ["actions"],
  })
  .refine((c) => (c.actions ?? []).every((a) => (a.kind === "link") === (a.target !== undefined)), {
    message: "a link action has a target, other actions have none",
    path: ["actions"],
  })
  .refine((c) => utf8Bytes(JSON.stringify(c)) <= MAX_CARD_V2_BYTES, { message: "card exceeds 64 KiB" });
export type CardV2 = z.infer<typeof CardV2>;

/** `Message.cardV2` as readers see it: a value this version cannot read is dropped (the v1 `card` stays). */
export const ReadCardV2 = CardV2.optional().catch(undefined);

const AnswerInputs = z.record(BlockId, z.json());
const AnswerEdits = z.record(BlockId, Text(32_768));
const answerFields = {
  messageId: Id,
  /** The `rev` of the card the answer was made on. */
  rev: z.int().positive(),
  actionId: ActionId,
  /** Values of `decision` (option id or ids) and `input` blocks, by block id. */
  inputs: AnswerInputs.optional(),
  /** Edited text of `draft` blocks, by block id. */
  editedValues: AnswerEdits.optional(),
};
const answerSize = (a: { inputs?: unknown; editedValues?: unknown }) =>
  utf8Bytes(JSON.stringify([a.inputs ?? null, a.editedValues ?? null])) <= MAX_CARD_ANSWER_BYTES;

/** `{ t: "card.answer" }`: answer a v2 card. ack.result is omitted. */
export const CardAnswerFrame = z
  .object({ t: z.literal("card.answer"), rid: Rid, groupId: Id, ...answerFields })
  .refine(answerSize, { message: "answer exceeds 64 KiB" });
export type CardAnswerFrame = z.infer<typeof CardAnswerFrame>;

/** Stored event: a member answered a v2 card. `actorId` is the answerer. */
export const CardAnsweredEvent = z.object({
  ...eventBase,
  type: z.literal("card.answered"),
  data: z.object(answerFields),
});

/** Max decisions in one `GET /v1/groups/:id/decisions` page. */
export const MAX_DECISIONS_PAGE = 200;

/**
 * A group decision (群里的决定): an answer to a v2 ask whose `ask.record` was true at the
 * answered rev. `GET /v1/groups/:id/decisions`, newest first.
 */
export const GroupDecision = z.object({
  /** Seq of the `card.answered` event. */
  seq: Seq,
  messageId: Id,
  topicId: Id,
  rev: z.int().positive(),
  /** The card title at the answered rev: the question. */
  title: z.string().min(1).max(200),
  actionId: ActionId,
  actionLabel: z.string().min(1).max(40),
  /** Labels of the chosen options of the card's `decision` blocks, in block order. */
  choices: z.array(z.string().min(1).max(200)).max(MAX_DECISION_OPTIONS * MAX_CARD_V2_BLOCKS).optional(),
  inputs: AnswerInputs.optional(),
  editedValues: AnswerEdits.optional(),
  answeredBy: Id,
  answeredAt: Millis,
});
export type GroupDecision = z.infer<typeof GroupDecision>;

export const DecisionsResponse = z.object({ decisions: z.array(GroupDecision), hasMore: z.boolean() });
export type DecisionsResponse = z.infer<typeof DecisionsResponse>;

// ---------------------------------------------------------------------------
// v1 card derived from a v2 card (for 1.0.x clients)
// ---------------------------------------------------------------------------

const TONE: Record<CardIntent, CardTone> = { ask: "warning", done: "success", working: "info", info: "neutral" };

/** True when a v1 press can answer the card: no inputs, drafts or decisions to fill. */
export function pressableAsV1(card: CardV2): boolean {
  const needs = (b: CardV2Block): boolean =>
    b.type === "input" || b.type === "draft" || b.type === "decision" || (b.type === "group" && b.blocks.some(needs));
  return !card.blocks.some(needs);
}

function cut(s: string, max: number): string {
  const cps = [...s];
  return cps.length <= max ? s : `${cps.slice(0, max - 1).join("")}…`;
}

function linkFor(target: RefTarget): LinkTarget | null {
  switch (target.kind) {
    case "pr":
    case "url":
      return { kind: "url", url: target.url };
    case "artifact":
      return target.version === undefined
        ? { kind: "artifact", artifactId: target.artifactId }
        : { kind: "artifact", artifactId: target.artifactId, version: target.version };
    case "topic":
      return { kind: "topic", groupId: target.groupId, topicId: target.topicId };
    case "member":
      return null;
  }
}

function num(n: number): string {
  return Number.isInteger(n) ? String(n) : String(Math.round(n * 100) / 100);
}

function toV1(b: CardV2Block | CardV2LeafBlock): CardBlock[] {
  switch (b.type) {
    case "text":
      return [{ type: "text", text: b.text }];
    case "fields":
      return [{ type: "fields", fields: b.fields }];
    case "table":
      return [{ type: "table", columns: b.columns, rows: b.rows }];
    case "list":
      return [{ type: "list", items: b.items }];
    case "progress":
      return [b.label === undefined ? { type: "progress", value: b.value } : { type: "progress", value: b.value, label: b.label }];
    case "code":
      return [b.lang === undefined ? { type: "code", text: b.text } : { type: "code", lang: b.lang, text: b.text }];
    case "image":
      return [b.alt === undefined ? { type: "image", fileId: b.fileId } : { type: "image", fileId: b.fileId, alt: b.alt }];
    case "divider":
      return [{ type: "divider" }];
    case "link":
      return [{ type: "link", label: b.label, target: b.target }];
    case "decision":
      return [{ type: "list", items: b.options.map((o) => ({ text: cut(o.label, 1000) })) }];
    case "input":
      return [];
    case "draft":
      return b.text === "" ? [] : [{ type: "text", text: cut(b.text, 8000) }];
    case "plan":
      return b.items && b.items.length > 0
        ? [
            {
              type: "list",
              items: b.items.map((i) => ({
                text: i.text,
                status: i.state === "done" ? "done" : i.state === "current" ? "in_progress" : "todo",
              })),
            },
          ]
        : [];
    case "metric":
      return [{ type: "fields", fields: [{ label: b.label, value: cut(`${num(b.value)}${b.unit ?? ""}`, 1000) }] }];
    case "chart":
      return [];
    case "diff":
      return [{ type: "code", lang: "diff", text: b.text }];
    case "evidence": {
      const status = b.state === "pass" ? "done" : b.state === "fail" ? "failed" : undefined;
      const text = cut(b.detail ? `${b.label}: ${b.detail}` : b.label, 1000);
      return [{ type: "list", items: [status ? { text, status } : { text }] }];
    }
    case "ref": {
      const target = linkFor(b.target);
      return target ? [{ type: "link", label: b.label, target }] : [];
    }
    case "group":
      return b.blocks.flatMap(toV1);
  }
}

/**
 * The v1 `card` the server stores next to `cardV2`: title, a tone from the intent, the
 * fallback text first, the v1-expressible blocks, and the submit and cancel actions when
 * the card has nothing to fill (`pressableAsV1`). See docs/protocol.md, "Cards v2".
 */
export function cardV2ToV1(card: CardV2): Card {
  const blocks: CardBlock[] = [];
  if (card.fallback !== "") blocks.push({ type: "text", text: cut(card.fallback, 8000) });
  for (const b of card.blocks) blocks.push(...toV1(b));
  const v1: Card = { v: 1, title: card.title, tone: TONE[card.intent], blocks: blocks.slice(0, 50) };
  if (pressableAsV1(card)) {
    const actions: CardAction[] = (card.actions ?? [])
      .filter((a) => (a.kind ?? "submit") !== "link")
      .map((a) => {
        const out: CardAction = { id: a.id, label: a.label };
        if (a.style) out.style = a.style;
        if (a.confirm) out.confirm = a.confirm.title;
        return out;
      });
    if (actions.length > 0) v1.actions = actions;
  }
  return v1;
}
