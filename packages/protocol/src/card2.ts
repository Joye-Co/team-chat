// Card v2: semantic blocks, intents, asks with inputs, and answers. It travels in the optional
// `Message.cardV2` next to a server-derived v1 `card`, so 1.0.x clients still render the
// message. See docs/protocol.md, "Cards v2" and "Cards v2.1: composition and interaction".
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
import { checkDiagramSource, MAX_DIAGRAM_SOURCE_BYTES } from "./diagram.ts";
import { RunPlan } from "./plan.ts";

export const CARD_V2_VERSION = 2 as const;
/** Catalog of a card without `catalog`. */
export const CARD_CATALOG_20 = "tc.card/2.0" as const;
/** Containers, choice variants, live events, `buttons`, `diagram` and `embed` (1.1.0). */
export const CARD_CATALOG_21 = "tc.card/2.1" as const;
/** Catalogs this version reads and writes. */
export const CARD_CATALOGS = [CARD_CATALOG_20, CARD_CATALOG_21] as const;
/** Max size of a v2 card: UTF-8 bytes of its JSON text. */
export const MAX_CARD_V2_BYTES = 65_536;
/** Max blocks of a v2 card: leaves and containers, inline group children included. */
export const MAX_CARD_V2_BLOCKS = 80;
export const MAX_CARD_V2_ACTIONS = 10;
export const MAX_GROUP_BLOCKS = 20;
export const MAX_DECISION_OPTIONS = 10;
export const MAX_INPUT_OPTIONS = 50;
export const MAX_CHART_SERIES = 5;
export const MAX_CHART_POINTS = 200;
export const MAX_ASK_FROM = 20;
/** Max UTF-8 bytes of `data` as JSON. */
export const MAX_CARD_DATA_BYTES = 16_384;
/** The highest card `rev`: clients store it as an unsigned 32-bit integer. */
export const MAX_CARD_REV = 4_294_967_295;
/** A Card v2 `rev`: 1..MAX_CARD_REV. */
export const CardRev = z.int().positive().max(MAX_CARD_REV);
/** Max UTF-8 bytes of `inputs` and `editedValues` of one answer, as JSON together. */
export const MAX_CARD_ANSWER_BYTES = 65_536;
/** Max UTF-8 bytes of an input's `default` as JSON. */
export const MAX_INPUT_DEFAULT_BYTES = 4096;
/** A container has depth <= 2 (a top-level block has depth 0), so a leaf has depth <= 3. */
export const MAX_CONTAINER_DEPTH = 2;
/** Max input blocks of a card (a questionnaire counts each child). */
export const MAX_CARD_V2_INPUT_BLOCKS = 12;
/** Max blocks that declare live `events`. */
export const MAX_CARD_V2_EVENT_BLOCKS = 20;
/** Max blocks with `emphasis: "high"`. */
export const MAX_EMPHASIS_HIGH = 2;
/** Max `diagram` blocks of a card. */
export const MAX_DIAGRAM_BLOCKS = 2;
/** Max revs of one card message (`too_large`, reason `card.rev_limit`). */
export const MAX_CARD_V2_REVS = 300;
/** Max UTF-8 bytes of a `card.event` value, and of its `inputs`. */
export const MAX_CARD_EVENT_VALUE_BYTES = 4096;
export const MAX_CARD_EVENT_INPUTS_BYTES = 16_384;
/** Max ops of one `card.patch`. */
export const MAX_PATCH_OPS = 50;
/** Max answers kept in `cardState.answers` (polls). */
export const MAX_CARD_STATE_ANSWERS = 500;

const Short = (max: number) => z.string().min(1).max(max);
const Text = (max: number) => z.string().max(max);

/** `ack.error.reason` of a card that breaks a count or range limit (V11-INTERACT §2.5). */
export const CARD_LIMITS = "card.limits";
/** The reason named at the start of a schema issue message (`card.limits: …`), or undefined. */
export function issueReason(message: string | undefined): string | undefined {
  return message ? /^((?:card|interact|artifact)\.[a-z_]+):/.exec(message)?.[1] : undefined;
}
/** Error of a count or range check of a card shape: it names `card.limits`, as Rust does. */
const LIMIT = {
  error: (i: { code: string; maximum?: unknown; minimum?: unknown }) =>
    `${CARD_LIMITS}: ${i.code === "too_big" ? `at most ${String(i.maximum)}` : i.code === "too_small" ? `at least ${String(i.minimum)}` : "out of range"}`,
};
/**
 * A union whose issue keeps the reason of a branch (`card.limits: …`). A plain union reports
 * "Invalid input" and hides the limit, while Rust names `card.limits`.
 */
const namedUnion = <const T extends readonly [z.ZodType, z.ZodType, ...z.ZodType[]]>(options: T) =>
  z.union(options, {
    error: (iss) => {
      const branches = "errors" in iss && Array.isArray(iss.errors) ? (iss.errors as { message?: string }[][]) : [];
      return branches.flat().find((x) => issueReason(x.message) !== undefined)?.message;
    },
  });
const jsonMax = (max: number, what: string) =>
  z.json().refine((v) => utf8Bytes(JSON.stringify(v)) <= max, { message: `${what} exceeds ${max} bytes` });

/** Block, option and action ids: 1..64 of `[A-Za-z0-9_.:-]`. `root` and `$…` are reserved. */
export const BlockId = ActionId;

export const CardIntent = z.enum(["ask", "done", "working", "info"]);
export type CardIntent = z.infer<typeof CardIntent>;
/**
 * 1.1: what the author's work on the card is doing now, set by the author (send, edit or a
 * `card` patch; null clears it): `executing` (蓝 处理中) or `failed` (红 失败). Absent: the band
 * follows `intent`.
 */
export const CardWorkStatus = z.enum(["executing", "failed"]);
export type CardWorkStatus = z.infer<typeof CardWorkStatus>;

// ---------------------------------------------------------------------------
// Parts (引用提问 and part events)
// ---------------------------------------------------------------------------

/**
 * The kind of a part of a block. Open set: a reader treats an unknown kind as the whole
 * block. Known kinds: cell row column point bar series metric item step line hunk image node
 * element.
 */
export const PartKind = z.string().regex(/^[a-z]{1,16}$/);
export type PartKind = z.infer<typeof PartKind>;
export const PART_KINDS = ["cell", "row", "column", "point", "bar", "series", "metric", "item", "step", "line", "hunk", "image", "node", "element"] as const;
/** Max entries of `PartRef.keys`. */
export const MAX_PART_KEYS = 4;
const PartKeyValue = z.union([z.string().max(256), z.int().nonnegative()]);
/** One part of a block: a table cell, a chart point, a plan step, a diagram node, … */
export const PartRef = z.object({
  kind: PartKind,
  /** Key names are 1..16 of `[a-z]`; string values <= 256 code points, numbers integers >= 0. */
  keys: z
    .record(z.string().regex(/^[a-z]{1,16}$/), PartKeyValue)
    .refine((k) => Object.keys(k).length <= MAX_PART_KEYS, { message: `at most ${MAX_PART_KEYS} keys` })
    .optional(),
});
export type PartRef = z.infer<typeof PartRef>;

// ---------------------------------------------------------------------------
// Live events
// ---------------------------------------------------------------------------

/** A `card.event` name. Open set on read: an unknown name in `events` is ignored. */
export const CardEventName = z.string().regex(/^[a-z][A-Za-z0-9_.-]{0,63}$/);
export type CardEventName = z.infer<typeof CardEventName>;
/** The event catalog of tc.card/2.1. */
export const CARD_EVENT_NAMES = [
  "choice.change",
  "choice.submit",
  "input.change",
  "input.submit",
  "chart.select",
  "chart.clear",
  "metric.press",
  "table.rowSelect",
  "table.sort",
  "button.press",
  "draft.edit",
  "compare.pick",
  "tab.open",
  "action.press",
  "answer.retract",
] as const;

/** Who may send a block's live events. Answers always follow `ask.from`. */
export const EmitByKeyword = z.enum(["ask", "requester", "members", "owners"]);
export const EmitBy = z.union([EmitByKeyword, z.object({ memberIds: z.array(Id).min(1).max(MAX_ASK_FROM) })]);
export type EmitBy = z.infer<typeof EmitBy>;
export const Emphasis = z.enum(["high", "low"]);
export type Emphasis = z.infer<typeof Emphasis>;
export const Deliver = z.enum(["immediate", "onSubmit"]);
export type Deliver = z.infer<typeof Deliver>;

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
  /** 2.1: the ask round; default "a1". A new id opens a new round. */
  id: BlockId.optional(),
  from: z.array(AskFrom).min(1, LIMIT).max(MAX_ASK_FROM, LIMIT),
  /** 2.1: a blocking ask sets the topic `waiting_answer`. Default: `intent === "ask"`. */
  blocking: z.boolean().optional(),
  /** After this time the ask takes no answer (`conflict`). */
  expiresAt: Millis.optional(),
  /** 2.1: what the member promises until the answer, e.g. "你确认之前，我不会发到全员群。" */
  promise: Short(200).optional(),
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

/** Fields every block has (2.1 hints; they never style). */
const base = {
  id: BlockId,
  emphasis: Emphasis.optional(),
  /** Live events this block sends (`card.event`); absent = none. */
  events: z.array(CardEventName).max(16, LIMIT).optional(),
  /** Input blocks: the commit is the answer (`immediate`) or goes with a submit (`onSubmit`). */
  deliver: Deliver.optional(),
  emitBy: EmitBy.optional(),
  /** Plain text for readers that do not know this block type. Required on 2.1 leaves. */
  fallbackText: Short(300).optional(),
};
const id = base;
/** Same shape as `ArtifactRef` (message.ts); kept here so message.ts can import this module. */
const ArtifactRef = z.object({ artifactId: Id, version: z.int().positive() });
const Option_ = z.object({
  id: ActionId,
  label: Short(200),
  detail: Text(1000).optional(),
  /** 2.1 */
  recommended: z.boolean().optional(),
});
const InputOption = z.object({ id: ActionId, label: Short(200) });
/** Fields of the 2.1 input blocks. */
const question = {
  label: Short(200),
  required: z.boolean().optional(),
  help: Text(300).optional(),
};

export const InputKind = z.enum(["text", "textarea", "number", "select", "date", "switch", "chips", "slider"], {
  error: (i) => (i.input === "secret" ? "card.secret_deferred: no secret inputs in 1.1.0" : undefined),
});
export type InputKind = z.infer<typeof InputKind>;
export const ChartKind = z.enum(["line", "bar", "area", "spark"]);
export type ChartKind = z.infer<typeof ChartKind>;
export const MetricBetter = z.enum(["up", "down"]);
export type MetricBetter = z.infer<typeof MetricBetter>;
export const MetricTone = z.enum(["good", "bad", "neutral"]);
export type MetricTone = z.infer<typeof MetricTone>;
export const EvidenceState = z.enum(["pass", "fail", "info"]);
export type EvidenceState = z.infer<typeof EvidenceState>;
export const TableSelect = z.enum(["single", "multiple"]);
export type TableSelect = z.infer<typeof TableSelect>;

// v1 block types, each with an id.
export const TextBlockV2 = TextBlock.extend(id);
export const FieldsBlockV2 = FieldsBlock.extend(id);
export const TableBlockV2 = TableBlock.safeExtend({
  ...id,
  /** 2.1 */
  title: Short(80).optional(),
  /** 2.1: one stable key per row (part refs, `table.rowSelect`); default the row index. */
  rowKeys: z.array(Short(64)).max(200, LIMIT).optional(),
  /** 2.1: the table is an input; its value is `{value}` / `{values}` of row keys. */
  select: TableSelect.optional(),
  /** 2.1: local sort by header; the member gets `table.sort` only when declared. */
  sortable: z.boolean().optional(),
});
export const ListBlockV2 = ListBlock.extend(id);
export const ProgressBlockV2 = ProgressBlock.extend(id);
export const CodeBlockV2 = CodeBlock.extend(id);
export const ImageBlockV2 = ImageBlock.extend(id);
export const DividerBlockV2 = DividerBlock.extend(id);
export const LinkBlockV2 = LinkBlock.extend(id);

/** Choose one option (or several with `multiple`): CARD21 `choice` (wire name `decision`). */
export const DecisionBlock = z.object({
  type: z.literal("decision"),
  ...id,
  /** 2.1: the question. */
  label: Short(200).optional(),
  required: z.boolean().optional(),
  help: Text(300).optional(),
  options: z.array(Option_).min(2, LIMIT).max(MAX_DECISION_OPTIONS, LIMIT),
  /** More than one option may be chosen. */
  multiple: z.boolean().optional(),
  /** Option ids chosen at first. */
  default: z.array(ActionId).max(MAX_DECISION_OPTIONS, LIMIT).optional(),
  /** 2.1, `multiple` only: 1 <= minSelect <= maxSelect <= options. */
  minSelect: z.int().positive().optional(),
  maxSelect: z.int().positive().optional(),
  /** 2.1: an "other" choice with text (value `{value: "other", other}`, <= 60). */
  allowOther: z.object({ label: Short(40), placeholder: Text(60).optional() }).optional(),
});
export const InputBlock = z.object({
  type: z.literal("input"),
  ...id,
  kind: InputKind,
  label: Short(200),
  placeholder: Text(200).optional(),
  required: z.boolean().optional(),
  help: Text(300).optional(),
  /** Choices of `select` and `chips`. */
  options: z.array(InputOption).max(MAX_INPUT_OPTIONS, LIMIT).optional(),
  min: z.number().optional(),
  max: z.number().optional(),
  /** 2.1, `slider`: the step (> 0). */
  step: z.number().positive().optional(),
  /** 2.1, `slider` and `number`: the unit shown next to the value. */
  unit: Text(16).optional(),
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
  /** 2.1: overrides the tone the client infers from `better` and `delta`. */
  tone: MetricTone.optional(),
  note: Text(200).optional(),
});
export const ChartSeries = z.object({
  /** 2.1: the series id of `chart.select` and part refs; default the name. */
  id: BlockId.optional(),
  name: Short(100),
  values: z.array(z.number()).max(MAX_CHART_POINTS, LIMIT),
});
export const ChartBlock = z.object({
  type: z.literal("chart"),
  ...id,
  kind: ChartKind,
  label: Text(200).optional(),
  unit: Text(16).optional(),
  /** Labels of the x axis, one per point. */
  x: z.array(Text(40)).max(MAX_CHART_POINTS, LIMIT).optional(),
  series: z.array(ChartSeries).min(1, LIMIT).max(MAX_CHART_SERIES, LIMIT),
  /** 2.1: one sentence that says what the chart shows (v1 card, VoiceOver). */
  summary: Text(280).optional(),
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
/** 1.1: test counts of an evidence line, from the run's output (read these, not the label). */
export const EvidenceTests = z.object({ passed: Count, failed: Count, skipped: Count.optional() });
export type EvidenceTests = z.infer<typeof EvidenceTests>;
/** 1.1: a screen recording (录屏): a video file of the group, its length and a poster frame. */
export const EvidenceRecording = z.object({ fileId: Id, durationMs: Count, posterFileId: Id.optional() });
export type EvidenceRecording = z.infer<typeof EvidenceRecording>;
export const EvidenceBlock = z.object({
  type: z.literal("evidence"),
  ...id,
  label: Short(200),
  state: EvidenceState.optional(),
  detail: Text(1000).optional(),
  fileId: Id.optional(),
  url: WebUrl.optional(),
  artifact: ArtifactRef.optional(),
  /** 1.1 */
  tests: EvidenceTests.optional(),
  /** 1.1: a 录屏 tile. */
  recording: EvidenceRecording.optional(),
});
export const RefBlock = z.object({ type: z.literal("ref"), ...id, label: Short(200), target: RefTarget });

// --- 2.1 leaves (catalog tc.card/2.1) --------------------------------------

export const RankBlock = z.object({
  type: z.literal("rank"),
  ...id,
  ...question,
  items: z.array(z.object({ id: ActionId, label: Short(200), meta: Text(80).optional() })).min(2, LIMIT).max(8, LIMIT),
  /** A line after the first `after` items, e.g. "前 3 个进 4.0". */
  cut: z.object({ after: z.int().positive(), label: Short(40) }).optional(),
  /** Only the first n ids are the answer. */
  rankTop: z.int().positive().optional(),
});
export const RatingBlock = z.object({
  type: z.literal("rating"),
  ...id,
  ...question,
  /** Always 5. */
  max: z.literal(5, LIMIT),
  /** Words for some values, keys "1".."5". */
  labels: z.partialRecord(z.enum(["1", "2", "3", "4", "5"]), Short(20)).optional(),
});
export const ScaleBlock = z.object({
  type: z.literal("scale"),
  ...id,
  ...question,
  min: z.int(),
  max: z.int(),
  /** Words at the two ends. */
  ends: z.tuple([Short(20), Short(20)]).optional(),
});
export const ConfirmBlock = z.object({
  type: z.literal("confirm"),
  ...id,
  label: Short(200),
  yes: Short(20),
  no: Short(20),
  detail: Text(500).optional(),
});
export const ImageChoiceBlock = z.object({
  type: z.literal("imageChoice"),
  ...id,
  ...question,
  multiple: z.boolean().optional(),
  options: z
    .array(z.object({ id: ActionId, label: Short(80), fileId: Id, recommended: z.boolean().optional() }))
    .min(2, LIMIT)
    .max(4, LIMIT),
});
export const MatrixBlock = z.object({
  type: z.literal("matrix"),
  ...id,
  ...question,
  rows: z.array(z.object({ id: ActionId, label: Short(80), required: z.boolean().optional() })).min(1, LIMIT).max(8, LIMIT),
  columns: z.array(z.object({ id: ActionId, label: Short(40) })).min(2, LIMIT).max(5, LIMIT),
});
export const SlotsBlock = z.object({
  type: z.literal("slots"),
  ...id,
  ...question,
  multiple: z.boolean().optional(),
  /** IANA time zone the slots are shown in. */
  tz: Short(64),
  slots: z
    .array(z.object({ id: ActionId, start: Millis, end: Millis, conflicts: z.array(Id).max(50, LIMIT).optional() }))
    .min(1, LIMIT)
    .max(12, LIMIT),
});
export const PollVoters = namedUnion([z.literal("group"), z.array(Id).min(1, LIMIT).max(200, LIMIT)]);
export const PollBlock = z.object({
  type: z.literal("poll"),
  ...id,
  ...question,
  options: z.array(InputOption).min(2, LIMIT).max(6, LIMIT),
  voters: PollVoters,
  closesAt: Millis,
  /** Voter ids per option are shown (`cardState.tallyBy`). */
  public: z.boolean().optional(),
  /** A voter may change the vote until `closesAt`. */
  changeable: z.boolean().optional(),
});
export const ButtonsBlock = z.object({
  type: z.literal("buttons"),
  ...id,
  /** Inline buttons; they send `button.press` and never answer an ask. */
  buttons: z
    .array(
      z.object({
        id: ActionId,
        label: Short(20),
        style: z.enum(["default", "danger"]).optional(),
        confirm: z.object({ title: Short(200), body: Text(1000).optional() }).optional(),
      }),
    )
    .min(1, LIMIT)
    .max(3, LIMIT),
});
export const DiagramBlock = z.object({
  type: z.literal("diagram"),
  ...id,
  /** Open set on read (shown as code when unknown); writes accept only "mermaid". */
  syntax: Short(32),
  /** 1..16384 UTF-8 bytes; checked with `checkDiagramSource`. */
  source: z.string().min(1),
  title: Short(80).optional(),
  caption: Short(280).optional(),
  alt: Short(1000).optional(),
});
export const EmbedBlock = z.object({
  type: z.literal("embed"),
  ...id,
  /** A 产出 version of the same group, shown live inline. `events` names its declared events. */
  artifactId: Id,
  version: z.int().positive(),
  /** 160..480 pt; the client owns scrolling. */
  height: z.int().min(160, LIMIT).max(480, LIMIT).optional(),
  title: Short(80).optional(),
});

/** Every block type except containers. */
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
  // 2.1
  RankBlock,
  RatingBlock,
  ScaleBlock,
  ConfirmBlock,
  ImageChoiceBlock,
  MatrixBlock,
  SlotsBlock,
  PollBlock,
  ButtonsBlock,
  DiagramBlock,
  EmbedBlock,
]);
export type CardV2LeafBlock = z.infer<typeof CardV2LeafBlock>;

const Children = z.array(BlockId).max(12, LIMIT);
/**
 * A titled section. 2.0: children inline in `blocks` (leaves only). 2.1: children by id in
 * `children` (any block of the card). Exactly one of the two.
 */
export const GroupBlock = z.object({
  type: z.literal("group"),
  ...id,
  label: Text(200).optional(),
  blocks: z.array(CardV2LeafBlock).max(MAX_GROUP_BLOCKS, LIMIT).optional(),
  /** 2.1 */
  children: Children.optional(),
  title: Short(40).optional(),
  /** Shown when collapsed. */
  summary: Short(60).optional(),
  collapsible: z.boolean().optional(),
  collapsed: z.boolean().optional(),
});
export const StackBlock = z.object({ type: z.literal("stack"), ...id, children: Children });
export const RowBlock = z.object({
  type: z.literal("row"),
  ...id,
  children: Children,
  /** Relative column widths, one per child; default all 1. */
  weights: z.array(z.int().min(1, LIMIT).max(3, LIMIT)).max(3, LIMIT).optional(),
});
export const CompareOption = z.object({
  id: ActionId,
  label: Short(24),
  pros: z.array(Short(40)).max(2, LIMIT).optional(),
  cons: z.array(Short(40)).max(2, LIMIT).optional(),
  preview: z.union([z.object({ fileId: Id }), z.object({ artifactId: Id, version: z.int().positive() })]).optional(),
  recommended: z.boolean().optional(),
  children: z.array(BlockId).max(6, LIMIT).optional(),
});
export const CompareBlock = z.object({
  type: z.literal("compare"),
  ...id,
  question: Short(200).optional(),
  options: z.array(CompareOption).max(4, LIMIT),
  /** A text note with the pick (`{value, note}`). */
  note: z.object({ label: Short(40), maxLength: z.int().positive().max(1000).optional() }).optional(),
});
export const TabsBlock = z.object({
  type: z.literal("tabs"),
  ...id,
  tabs: z.array(z.object({ id: ActionId, label: Short(16), children: Children })).max(5, LIMIT),
  /** The tab shown first. */
  selected: ActionId.optional(),
});
export const QuestionnaireBlock = z.object({
  type: z.literal("questionnaire"),
  ...id,
  children: Children,
  submitLabel: Short(20).optional(),
  /** Answers reach the member as aggregates only. */
  anonymous: z.boolean().optional(),
});

export const CardV2Block = z.discriminatedUnion("type", [
  ...CardV2LeafBlock.options,
  GroupBlock,
  StackBlock,
  RowBlock,
  CompareBlock,
  TabsBlock,
  QuestionnaireBlock,
], { error: (i) => (i.code === "invalid_union" ? "card.unknown_block: unknown block type" : undefined) });
export type CardV2Block = z.infer<typeof CardV2Block>;

/** Block types of tc.card/2.1 (not in 2.0). */
export const CARD_21_TYPES = new Set([
  "rank",
  "rating",
  "scale",
  "confirm",
  "imageChoice",
  "matrix",
  "slots",
  "poll",
  "buttons",
  "diagram",
  "embed",
  "stack",
  "row",
  "compare",
  "tabs",
  "questionnaire",
]);
/** 2.1 leaves that need `fallbackText`, and the 2.1 containers that ask (compare, questionnaire). */
const NEEDS_FALLBACK = new Set([
  "rank",
  "rating",
  "scale",
  "confirm",
  "imageChoice",
  "matrix",
  "slots",
  "poll",
  "buttons",
  "diagram",
  "embed",
  "compare",
  "questionnaire",
]);
const CONTAINERS = new Set(["group", "stack", "row", "compare", "tabs", "questionnaire"]);

export const CardActionKind = z.enum(["submit", "cancel", "link", "reject", "press"]);
export type CardActionKind = z.infer<typeof CardActionKind>;
export const CardV2ActionStyle = z.enum(["primary", "default", "danger"]);
export type CardV2ActionStyle = z.infer<typeof CardV2ActionStyle>;

export const CardV2Action = z.object({
  id: ActionId,
  label: Short(40),
  style: CardV2ActionStyle.optional(),
  /** Absent = "submit". `reject` answers no; `press` is a live event (never an answer). */
  kind: CardActionKind.optional(),
  /** Where a `link` action goes. Required for `link`. */
  target: RefTarget.optional(),
  /** When set, the client asks before it sends the answer. */
  confirm: z.object({ title: Short(200), body: Text(1000).optional() }).optional(),
  /** 2.1, `submit` and `reject`: the input blocks whose values go in `inputs`. */
  inputs: z.array(BlockId).max(MAX_CARD_V2_INPUT_BLOCKS, LIMIT).optional(),
  /**
   * 1.1: what the action means to readers, as open text. Known: `ACTION_ROLE_APPROVE`, the
   * action of a review ask that approves the change (merge). Unknown roles mean nothing.
   */
  role: z.string().min(1).max(32, LIMIT).optional(),
});
export type CardV2Action = z.infer<typeof CardV2Action>;
/** `CardV2Action.role` of the action that approves a reviewed change (合并中 follows it). */
export const ACTION_ROLE_APPROVE = "approve";

/** The card as readers see it: shapes only, no write rules. */
export const CardV2Shape = z.object({
  v: z.literal(CARD_V2_VERSION),
  /** 2.1: absent = tc.card/2.0. Writes accept only `CARD_CATALOGS`. */
  catalog: Short(32).optional(),
  /** 1 at first; every edit sends a higher rev. Answers name the rev they answer. */
  rev: CardRev,
  intent: CardIntent,
  /** 1.1: the author's work state (`CardWorkStatus`); absent = follow `intent`. */
  status: CardWorkStatus.optional(),
  title: Short(200),
  /** 2.1: one plain sentence under the title. */
  summary: Text(280).optional(),
  /** Plain text for old clients, notifications and search; also the v1 card's first block. */
  fallback: Text(4000),
  ask: CardAsk.optional(),
  blocks: z.array(CardV2Block).max(MAX_CARD_V2_BLOCKS, LIMIT),
  actions: z.array(CardV2Action).max(MAX_CARD_V2_ACTIONS, LIMIT).optional(),
  /** Bot state, returned to nobody; at most 16 KiB. */
  data: jsonMax(MAX_CARD_DATA_BYTES, "data").optional(),
});

// ---------------------------------------------------------------------------
// Write rules (what the server checks on send, edit and after a patch)
// ---------------------------------------------------------------------------

/** Keys that style a block; rejected at any depth of a block or action (`card.style_property`). */
export const CARD_STYLE_KEYS = new Set([
  "style",
  "class",
  "className",
  "css",
  "html",
  "color",
  "colour",
  "background",
  "bg",
  "font",
  "fontSize",
  "fontWeight",
  "padding",
  "margin",
  "gap",
  "width",
  "height",
  "border",
  "radius",
  "shadow",
  "icon",
]);

/** A problem a write rule found: `reason` goes to `ack.error.reason`. */
export type CardProblem = { reason: string; message: string };

function styleKeyIn(v: unknown, allow: Set<string>): string | undefined {
  if (Array.isArray(v)) {
    for (const x of v) {
      const k = styleKeyIn(x, new Set());
      if (k) return k;
    }
    return undefined;
  }
  if (!v || typeof v !== "object") return undefined;
  for (const [k, x] of Object.entries(v)) {
    if (CARD_STYLE_KEYS.has(k) && !allow.has(k)) return k;
    // Inline `buttons` keep their semantic `style`.
    const inner = k === "buttons" ? new Set(["style"]) : new Set<string>();
    if (Array.isArray(x) && k === "buttons") {
      for (const b of x) {
        const s = styleKeyIn(b, inner);
        if (s) return s;
      }
      continue;
    }
    const s = styleKeyIn(x, new Set());
    if (s) return s;
  }
  return undefined;
}

/**
 * The first style key in a raw card (blocks and actions), or undefined. `style` on actions
 * and inline buttons and `height` on `embed` are semantic and allowed.
 */
export function cardStyleKey(raw: unknown): string | undefined {
  if (!raw || typeof raw !== "object") return undefined;
  const c = raw as { blocks?: unknown; actions?: unknown };
  for (const b of Array.isArray(c.blocks) ? c.blocks : []) {
    const allow = new Set<string>(b && typeof b === "object" && (b as { type?: unknown }).type === "embed" ? ["height"] : []);
    const k = styleKeyIn(b, allow);
    if (k) return k;
  }
  for (const a of Array.isArray(c.actions) ? c.actions : []) {
    const k = styleKeyIn(a, new Set(["style"]));
    if (k) return k;
  }
  return undefined;
}

type AnyBlock = CardV2Block | CardV2LeafBlock;

/** Child ids a container names (`children`, compare option and tab children). */
export function childIds(b: AnyBlock): string[] {
  switch (b.type) {
    case "group":
    case "stack":
    case "row":
    case "questionnaire":
      return b.children ?? [];
    case "compare":
      return b.options.flatMap((o) => o.children ?? []);
    case "tabs":
      return b.tabs.flatMap((t) => t.children);
    default:
      return [];
  }
}

/** True for blocks whose value goes into an answer or an input event. */
export function takesInput(b: AnyBlock): boolean {
  switch (b.type) {
    case "decision":
    case "input":
    case "rank":
    case "rating":
    case "scale":
    case "confirm":
    case "imageChoice":
    case "matrix":
    case "slots":
    case "poll":
    case "compare":
      return true;
    case "draft":
      return b.editable === true;
    case "table":
      return b.select !== undefined;
    default:
      return false;
  }
}

function immediateAllowed(b: AnyBlock): boolean {
  switch (b.type) {
    case "decision":
    case "imageChoice":
      return b.multiple !== true;
    case "confirm":
    case "compare":
    case "rating":
    case "scale":
      return true;
    case "input":
      return b.kind === "text" || b.kind === "number";
    default:
      return false;
  }
}

/** Every block of a card: top-level list, then inline (2.0) group children. */
export function allBlocks(card: Pick<CardV2, "blocks">): AnyBlock[] {
  const out: AnyBlock[] = [];
  for (const b of card.blocks) {
    out.push(b);
    if (b.type === "group") out.push(...(b.blocks ?? []));
  }
  return out;
}

function leafProblem(b: AnyBlock): string | undefined {
  switch (b.type) {
    case "decision": {
      const ids = new Set(b.options.map((o) => o.id));
      if (ids.size !== b.options.length) return "option ids are unique";
      if ((b.default ?? []).some((d) => !ids.has(d))) return "default names an unknown option";
      if (b.multiple !== true && (b.default?.length ?? 0) > 1) return "a single choice has at most one default";
      if ((b.minSelect !== undefined || b.maxSelect !== undefined) && b.multiple !== true) return "minSelect and maxSelect need multiple";
      const min = b.minSelect ?? 1;
      const max = b.maxSelect ?? b.options.length;
      if (min > max || max > b.options.length) return "1 <= minSelect <= maxSelect <= options";
      return undefined;
    }
    case "rank": {
      if (new Set(b.items.map((i) => i.id)).size !== b.items.length) return "item ids are unique";
      if (b.cut && b.cut.after >= b.items.length) return "cut.after is less than the items";
      if (b.rankTop !== undefined && b.rankTop > b.items.length) return "rankTop is at most the items";
      return undefined;
    }
    case "scale":
      return b.min < b.max && b.max - b.min <= 10 ? undefined : "min < max and at most 11 steps";
    case "imageChoice":
      return new Set(b.options.map((o) => o.id)).size === b.options.length ? undefined : "option ids are unique";
    case "matrix":
      if (new Set(b.rows.map((r) => r.id)).size !== b.rows.length) return "row ids are unique";
      return new Set(b.columns.map((c) => c.id)).size === b.columns.length ? undefined : "column ids are unique";
    case "slots":
      if (new Set(b.slots.map((s) => s.id)).size !== b.slots.length) return "slot ids are unique";
      return b.slots.every((s) => s.start < s.end) ? undefined : "a slot starts before it ends";
    case "poll":
      return new Set(b.options.map((o) => o.id)).size === b.options.length ? undefined : "option ids are unique";
    case "buttons":
      return new Set(b.buttons.map((x) => x.id)).size === b.buttons.length ? undefined : "button ids are unique";
    case "input":
      if (b.kind === "slider") {
        if (b.min === undefined || b.max === undefined || b.min >= b.max) return "a slider has min < max";
      }
      return undefined;
    case "table":
      if (b.rowKeys !== undefined) {
        if (b.rowKeys.length !== b.rows.length) return "one row key per row";
        if (new Set(b.rowKeys).size !== b.rowKeys.length) return "row keys are unique";
      }
      return undefined;
    case "chart": {
      const ids = b.series.map((s) => s.id ?? s.name);
      return new Set(ids).size === ids.length ? undefined : "series ids are unique";
    }
    case "diagram": {
      if (b.syntax !== "mermaid") return "syntax must be mermaid";
      if (utf8Bytes(b.source) > MAX_DIAGRAM_SOURCE_BYTES) return `source exceeds ${MAX_DIAGRAM_SOURCE_BYTES} bytes`;
      const r = checkDiagramSource(b.source);
      return r.ok ? undefined : `diagram: ${r.reason}`;
    }
    default:
      return undefined;
  }
}

/**
 * The first write rule a card breaks, or undefined (docs/protocol.md "Cards v2.1"): catalog,
 * ids, containers (children exist, one parent each, no cycle, depth, nesting, counts),
 * limits, variants, `deliver`, decision surfaces, one primary action, diagrams.
 */
export function cardV2Problem(card: z.infer<typeof CardV2Shape>): CardProblem | undefined {
  const bad = (reason: string, message: string): CardProblem => ({ reason, message });
  const catalog = card.catalog ?? CARD_CATALOG_20;
  if (!(CARD_CATALOGS as readonly string[]).includes(catalog)) return bad("card.unknown_catalog", `unknown catalog ${catalog}`);
  const all = allBlocks(card);
  if (all.length > MAX_CARD_V2_BLOCKS) return bad("card.limits", `at most ${MAX_CARD_V2_BLOCKS} blocks`);
  const byId = new Map<string, AnyBlock>();
  for (const b of all) {
    if (byId.has(b.id)) return bad("card.limits", "block ids are unique");
    if (b.id === "root" || b.id.startsWith("$")) return bad("card.limits", `block id ${b.id} is reserved`);
    byId.set(b.id, b);
  }
  for (const b of all) {
    if (catalog === CARD_CATALOG_20 && CARD_21_TYPES.has(b.type)) return bad("card.unknown_block", `${b.type} needs catalog ${CARD_CATALOG_21}`);
    if (NEEDS_FALLBACK.has(b.type) && b.fallbackText === undefined) return bad("card.limits", `${b.type} ${b.id} needs fallbackText`);
    const p = leafProblem(b);
    if (p) return bad("card.limits", `${b.type} ${b.id}: ${p}`);
  }
  // Containers: children, parents, counts.
  const top = new Set(card.blocks.map((b) => b.id));
  const parent = new Map<string, string>();
  for (const b of card.blocks) {
    if (b.type === "group" && (b.blocks !== undefined) === (b.children !== undefined)) {
      return bad("card.limits", `group ${b.id} has either blocks or children`);
    }
    if (b.type === "group" && b.blocks !== undefined && b.blocks.length < 1) return bad("card.limits", `group ${b.id} needs 1..${MAX_GROUP_BLOCKS} blocks`);
    const kids = childIds(b);
    const count = (lo: number, hi: number, n: number, what: string) =>
      n < lo || n > hi ? bad("card.limits", `${b.type} ${b.id}: ${lo}..${hi} ${what}`) : undefined;
    let p: CardProblem | undefined;
    switch (b.type) {
      case "row":
        p = count(2, 3, kids.length, "children");
        if (!p && b.weights && b.weights.length !== kids.length) p = bad("card.limits", `row ${b.id}: one weight per child`);
        break;
      case "stack":
        p = count(1, 12, kids.length, "children");
        break;
      case "group":
        if (b.children) p = count(1, 12, kids.length, "children");
        break;
      case "questionnaire":
        p = count(2, 5, kids.length, "children");
        break;
      case "compare":
        p = count(2, 4, b.options.length, "options");
        if (!p && new Set(b.options.map((o) => o.id)).size !== b.options.length) p = bad("card.limits", `compare ${b.id}: option ids are unique`);
        break;
      case "tabs":
        p = count(2, 5, b.tabs.length, "tabs");
        if (!p && b.tabs.some((t) => t.children.length < 1)) p = bad("card.limits", `tabs ${b.id}: 1..12 children per tab`);
        if (!p && new Set(b.tabs.map((t) => t.id)).size !== b.tabs.length) p = bad("card.limits", `tabs ${b.id}: tab ids are unique`);
        if (!p && b.selected !== undefined && !b.tabs.some((t) => t.id === b.selected)) p = bad("card.limits", `tabs ${b.id}: selected names no tab`);
        break;
    }
    if (p) return p;
    for (const k of kids) {
      if (!top.has(k)) return bad("card.limits", `${b.type} ${b.id} names no block ${k}`);
      if (k === b.id) return bad("card.depth", `${b.id} contains itself`);
      if (parent.has(k)) return bad("card.limits", `block ${k} has two parents`);
      parent.set(k, b.id);
    }
  }
  // Depth and nesting, walking down from the top-level blocks; a cycle is never reached.
  const depth = new Map<string, number>();
  const ancestors = new Map<string, string[]>();
  const roots = card.blocks.filter((b) => !parent.has(b.id));
  const stack: { id: string; d: number; up: string[] }[] = roots.map((b) => ({ id: b.id, d: 0, up: [] }));
  while (stack.length > 0) {
    const { id: bid, d, up } = stack.pop() as { id: string; d: number; up: string[] };
    if (depth.has(bid)) continue;
    depth.set(bid, d);
    ancestors.set(bid, up);
    const b = byId.get(bid) as AnyBlock;
    if (CONTAINERS.has(b.type) && d > MAX_CONTAINER_DEPTH) return bad("card.depth", `${b.type} ${b.id} is nested too deep`);
    if (b.type === "group" && b.blocks !== undefined && d + 1 > MAX_CONTAINER_DEPTH + 1) return bad("card.depth", `group ${b.id} is nested too deep`);
    const upTypes = up.map((u) => (byId.get(u) as AnyBlock).type);
    const directParent = upTypes.at(-1);
    if (b.type === "row" && directParent === "row") return bad("card.limits", `row ${b.id} is inside a row`);
    if (b.type === "tabs" && upTypes.some((t) => t === "tabs" || t === "row" || t === "compare")) return bad("card.limits", `tabs ${b.id} is inside tabs, a row or a compare`);
    if (b.type === "compare" && upTypes.some((t) => t === "row" || t === "compare")) return bad("card.limits", `compare ${b.id} is inside a row or a compare`);
    if (directParent === "questionnaire" && CONTAINERS.has(b.type)) return bad("card.limits", `questionnaire children are leaves`);
    for (const k of childIds(b)) stack.push({ id: k, d: d + 1, up: [...up, bid] });
  }
  for (const b of card.blocks) if (!depth.has(b.id)) return bad("card.depth", `block ${b.id} is in a cycle`);
  // Counts over the card.
  if (all.filter(takesInput).length > MAX_CARD_V2_INPUT_BLOCKS) return bad("card.limits", `at most ${MAX_CARD_V2_INPUT_BLOCKS} input blocks`);
  if (all.filter((b) => (b.events?.length ?? 0) > 0).length > MAX_CARD_V2_EVENT_BLOCKS) return bad("card.limits", `at most ${MAX_CARD_V2_EVENT_BLOCKS} blocks with events`);
  if (all.filter((b) => b.emphasis === "high").length > MAX_EMPHASIS_HIGH) return bad("card.limits", `at most ${MAX_EMPHASIS_HIGH} blocks with emphasis high`);
  if (all.filter((b) => b.type === "diagram").length > MAX_DIAGRAM_BLOCKS) return bad("card.limits", `at most ${MAX_DIAGRAM_BLOCKS} diagrams`);
  for (const b of all) {
    if (b.deliver !== undefined && !takesInput(b)) return bad("card.limits", `${b.id}: deliver is for input blocks`);
    if (b.deliver === "immediate" && !immediateAllowed(b)) return bad("card.limits", `${b.id}: immediate is for single choices, confirm, compare, rating, scale and single-line inputs`);
  }
  // Actions.
  const actions = card.actions ?? [];
  if (new Set(actions.map((a) => a.id)).size !== actions.length) return bad("card.limits", "action ids are unique");
  if (actions.filter((a) => a.style === "primary").length > 1) return bad("card.limits", "at most one primary action");
  for (const a of actions) {
    if ((a.kind === "link") !== (a.target !== undefined)) return bad("card.limits", "a link action has a target, other actions have none");
    if (a.inputs !== undefined) {
      if (a.kind !== undefined && a.kind !== "submit" && a.kind !== "reject") return bad("card.limits", `action ${a.id}: only submit and reject take inputs`);
      for (const i of a.inputs) {
        const b = byId.get(i);
        if (!b || !takesInput(b)) return bad("card.limits", `action ${a.id} names no input block ${i}`);
      }
    }
  }
  // One decision surface per open ask.
  if (card.ask) {
    let surfaces = actions.some((a) => (a.inputs?.length ?? 0) > 0) ? 1 : 0;
    for (const b of all) {
      if (b.type === "compare" || b.type === "questionnaire") surfaces += 1;
      else if (b.deliver === "immediate") surfaces += 1;
    }
    if (surfaces > 1) return bad("card.decision_surfaces", "one decision surface per ask");
  }
  return undefined;
}

/** The card a member writes: shapes, no style keys, and every write rule (`cardV2Problem`). */
export const CardV2 = z.preprocess(
  (raw, ctx) => {
    const k = cardStyleKey(raw);
    if (k) ctx.addIssue({ code: "custom", message: `card.style_property: ${k} is a style key`, input: raw });
    return raw;
  },
  CardV2Shape.superRefine((c, ctx) => {
    const p = cardV2Problem(c);
    if (p) ctx.addIssue({ code: "custom", message: `${p.reason}: ${p.message}`, path: ["blocks"] });
    if (utf8Bytes(JSON.stringify(c)) > MAX_CARD_V2_BYTES) ctx.addIssue({ code: "custom", message: "card exceeds 64 KiB" });
  }),
);
export type CardV2 = z.infer<typeof CardV2Shape>;

/** `Message.cardV2` as readers see it: a value this version cannot read is dropped (the v1 `card` stays). */
export const ReadCardV2 = CardV2Shape.optional().catch(undefined);

const AnswerInputs = z.record(BlockId, z.json());
const AnswerEdits = z.record(BlockId, Text(32_768));
const answerFields = {
  messageId: Id,
  /** The `rev` of the card the answer was made on. */
  rev: CardRev,
  actionId: ActionId,
  /** Values of `decision` (option id or ids) and `input` blocks, by block id. */
  inputs: AnswerInputs.optional(),
  /** Edited text of `draft` blocks, by block id. */
  editedValues: AnswerEdits.optional(),
  /** 1.1: sha256 (hex) of the draft text the answer approves (`draftContentHash`). */
  contentHash: z.string().regex(/^[0-9a-f]{64}$/).optional(),
};
const answerSize = (a: { inputs?: unknown; editedValues?: unknown }) =>
  utf8Bytes(JSON.stringify([a.inputs ?? null, a.editedValues ?? null])) <= MAX_CARD_ANSWER_BYTES;
const jsonBytes = (v: unknown) => (v === undefined ? 0 : utf8Bytes(JSON.stringify(v)));

/** Client event ids (ULIDs made by the client): the idempotency key of `card.event` and `artifact.event`. */
export const ClientEventId = z.string().min(1).max(64);

/**
 * `{ t: "card.answer" }`: answer a v2 card. Short form of `card.event {event: "action.press"}`;
 * ack.result is a `CardEventAck` from a 1.1 server.
 */
export const CardAnswerFrame = z
  .object({ t: z.literal("card.answer"), rid: Rid, groupId: Id, ...answerFields, clientEventId: ClientEventId.optional() })
  .refine(answerSize, { message: "answer exceeds 64 KiB" });
export type CardAnswerFrame = z.infer<typeof CardAnswerFrame>;

/** `{ t: "card.answer.retract" }`: short form of `card.event {event: "answer.retract", value: {answerId}}`. */
export const CardAnswerRetractFrame = z.object({
  t: z.literal("card.answer.retract"),
  rid: Rid,
  groupId: Id,
  messageId: Id,
  answerId: Id,
});
export type CardAnswerRetractFrame = z.infer<typeof CardAnswerRetractFrame>;

/** V11-INC R3: only `chart.select` (a point or a bar) and `metric.press` (the metric) name a part. */
export function partAllowed(event: string, kind: string): boolean {
  return (event === "chart.select" && (kind === "point" || kind === "bar")) || (event === "metric.press" && kind === "metric");
}

/**
 * `{ t: "card.event" }` (2.1): one interaction with a v2 card. Exactly one of `blockId` /
 * `actionId`, except `answer.retract` (neither). `inputs` only with `action.press`. `part`
 * names the part an event targets (`chart.select`: point or bar; `metric.press`: metric).
 */
export const CardEventFrame = z
  .object({
    t: z.literal("card.event"),
    rid: Rid,
    groupId: Id,
    messageId: Id,
    /** The rev the person saw. */
    rev: CardRev,
    clientEventId: ClientEventId,
    blockId: BlockId.optional(),
    actionId: ActionId.optional(),
    event: CardEventName,
    value: z.json().optional(),
    inputs: AnswerInputs.optional(),
    part: PartRef.optional(),
    /** 1.1: sha256 (hex) of the draft text the answer approves (`draftContentHash`). */
    contentHash: z.string().regex(/^[0-9a-f]{64}$/).optional(),
  })
  .refine((f) => (f.event === "answer.retract" ? f.blockId === undefined && f.actionId === undefined : (f.blockId === undefined) !== (f.actionId === undefined)), {
    message: "exactly one of blockId and actionId (neither for answer.retract)",
  })
  .refine((f) => f.inputs === undefined || f.event === "action.press", { message: "inputs only with action.press", path: ["inputs"] })
  .refine((f) => f.part === undefined || partAllowed(f.event, f.part.kind), {
    message: "part only with chart.select (point or bar) and metric.press (metric)",
    path: ["part"],
  })
  .refine((f) => jsonBytes(f.value) <= MAX_CARD_EVENT_VALUE_BYTES, { message: "value exceeds 4 KiB", path: ["value"] })
  .refine((f) => jsonBytes(f.inputs) <= MAX_CARD_EVENT_INPUTS_BYTES, { message: "inputs exceed 16 KiB", path: ["inputs"] });
export type CardEventFrame = z.infer<typeof CardEventFrame>;

/** Who responds to an event, and how soon (the server's estimate). */
export const ExpectMode = z.enum(["live", "run", "starting", "offline", "none"]);
export type ExpectMode = z.infer<typeof ExpectMode>;
export const Expect = z.object({ by: Id, mode: ExpectMode, etaMs: Count.optional() });
export type Expect = z.infer<typeof Expect>;
/** ack.result of `card.event` (and `card.answer` from a 1.1 server). */
export const CardEventAck = z.object({
  eventId: Id,
  stored: z.boolean(),
  answerId: Id.optional(),
  duplicate: z.boolean().optional(),
  expect: Expect,
});
export type CardEventAck = z.infer<typeof CardEventAck>;

/** One op of a `card.patch`. The result is checked as a whole card. */
export const PatchOp = z.discriminatedUnion("op", [
  /** Shallow merge; cannot change id, type, children, options or tabs. */
  z.object({
    op: z.literal("set"),
    id: BlockId,
    props: z.record(z.string(), z.json()).refine((p) => !["id", "type", "children", "options", "tabs"].some((k) => k in p), {
      message: "set cannot change id, type, children, options or tabs",
    }),
  }),
  /** Whole block, same id, any type. */
  z.object({ op: z.literal("replace"), id: BlockId, block: CardV2Block }),
  /** At most one of after / before / index; default: append. `parent` is a container, option or tab id. */
  z
    .object({
      op: z.literal("insert"),
      block: CardV2Block,
      parent: BlockId.optional(),
      after: BlockId.optional(),
      before: BlockId.optional(),
      index: Count.optional(),
    })
    .refine((o) => [o.after, o.before, o.index].filter((x) => x !== undefined).length <= 1, { message: "at most one placement" }),
  /** Also removes the id from its parent; children are removed too. */
  z.object({ op: z.literal("remove"), id: BlockId }),
  /** JSON Pointer into `data` ("" = whole); absent value = delete. */
  z.object({ op: z.literal("data"), path: z.string().max(512), value: z.json().optional() }),
  z.object({
    op: z.literal("card"),
    props: z.object({
      title: Short(200).optional(),
      summary: Text(280).optional(),
      intent: CardIntent.optional(),
      /** 1.1: null clears it. */
      status: CardWorkStatus.nullable().optional(),
      fallback: Text(4000).optional(),
      /** null closes the open ask. */
      ask: CardAsk.nullable().optional(),
      actions: z.array(CardV2Action).max(MAX_CARD_V2_ACTIONS).optional(),
    }),
  }),
]);
export type PatchOp = z.infer<typeof PatchOp>;

/** `{ t: "card.patch" }` (2.1): the author changes its card in place. ack.result is `CardPatchAck`. */
export const CardPatchFrame = z.object({
  t: z.literal("card.patch"),
  rid: Rid,
  groupId: Id,
  messageId: Id,
  /** Must equal the current rev (`conflict`, `card.stale_rev`). */
  baseRev: CardRev,
  ops: z.array(PatchOp).min(1).max(MAX_PATCH_OPS),
  /** The event this patch answers (an eventId of this message from the last 30 minutes). */
  cause: Id.optional(),
  clientPatchId: ClientEventId.optional(),
});
export type CardPatchFrame = z.infer<typeof CardPatchFrame>;

/** How the author answered an event. */
export const InteractionResult = z.enum(["ignored", "failed", "patched", "posted", "replied"]);
export type InteractionResult = z.infer<typeof InteractionResult>;

/** `{ t: "interaction.resolve" }` (2.1): the author answers an event without changing anything. */
export const InteractionResolveFrame = z.object({
  t: z.literal("interaction.resolve"),
  rid: Rid,
  groupId: Id,
  eventId: Id,
  result: z.enum(["ignored", "failed"]),
  /** Plain text shown under the block for 5 s. */
  note: Short(120).optional(),
});
export type InteractionResolveFrame = z.infer<typeof InteractionResolveFrame>;

/** Stored event: a member answered a v2 card. `actorId` is the answerer. */
export const CardAnsweredEvent = z.object({
  ...eventBase,
  type: z.literal("card.answered"),
  data: z.object({
    ...answerFields,
    /** 2.1: absent for the commit of an `immediate` block (then `blockId` is set). */
    actionId: ActionId.optional(),
    answerId: Id.optional(),
    askId: BlockId.optional(),
    eventId: Id.optional(),
    event: CardEventName.optional(),
    blockId: BlockId.optional(),
    value: z.json().optional(),
  }),
});

/** Stored event (2.1): the answerer took an answer back; the ask is open again. */
export const CardRetractedEvent = z.object({
  ...eventBase,
  type: z.literal("card.retracted"),
  data: z.object({ messageId: Id, answerId: Id, askId: BlockId.optional(), eventId: Id.optional() }),
});

/** What an answer or a rev was made for: the event, who sent it, when. Filled by the server. */
export const Cause = z.object({
  eventId: Id,
  actorId: Id,
  at: Millis,
  event: CardEventName,
  blockId: BlockId.optional(),
  actionId: ActionId.optional(),
});
export type Cause = z.infer<typeof Cause>;

export const AskStatus = z.enum(["open", "answered", "expired", "closed", "superseded"]);
export type AskStatus = z.infer<typeof AskStatus>;

/** One answer in `cardState.answers`. */
export const CardAnswer = z.object({
  answerId: Id,
  askId: BlockId,
  eventId: Id,
  actorId: Id,
  at: Millis,
  rev: CardRev,
  event: CardEventName,
  blockId: BlockId.optional(),
  actionId: ActionId.optional(),
  value: z.json().optional(),
  inputs: AnswerInputs.optional(),
  /** Draft field ids the answerer changed. */
  edited: z.array(BlockId).max(MAX_CARD_V2_BLOCKS).optional(),
  /** sha256 of the answered draft text. */
  contentHash: z.string().regex(/^[0-9a-f]{64}$/).optional(),
  retractedAt: Millis.optional(),
});
export type CardAnswer = z.infer<typeof CardAnswer>;

/** Server-owned state of a v2 card message (members never write it). */
export const CardState = z.object({
  ask: z
    .object({
      id: BlockId,
      status: AskStatus,
      /** Resolved member ids. */
      from: z.array(Id).max(200),
      blocking: z.boolean(),
      expiresAt: Millis.optional(),
      answeredBy: Id.optional(),
      answeredAt: Millis.optional(),
    })
    .optional(),
  /** Oldest first. */
  answers: z.array(CardAnswer).max(MAX_CARD_STATE_ANSWERS),
  /** Polls: votes per option id; `tallyBy` (public polls) the voters. */
  tally: z.record(BlockId, Count).optional(),
  tallyBy: z.record(BlockId, z.array(Id)).optional(),
  /** `emitBy: "requester"` resolved on write, by block id. */
  emitters: z.record(BlockId, z.array(Id)).optional(),
  /** The event the newest rev answered. */
  lastCause: Cause.optional(),
  /**
   * 1.1, private cards: live answers of the current ask round. Viewers who do not get the
   * answers (an anonymous questionnaire, a private poll) see this count.
   */
  answerCount: Count.optional(),
});
export type CardState = z.infer<typeof CardState>;

/**
 * The text a draft `contentHash` covers: the draft blocks an answer submits, in card order,
 * each with its edited text when edited, joined with U+0000. `contentHash` is its sha256 (hex
 * of the UTF-8 bytes).
 */
export function draftContentText(card: Pick<CardV2, "blocks">, ids: Iterable<string>, edited?: Record<string, string>): string | undefined {
  const want = new Set(ids);
  const texts: string[] = [];
  for (const b of allBlocks(card)) {
    if (b.type !== "draft" || !want.has(b.id)) continue;
    texts.push(edited?.[b.id] ?? b.text);
  }
  return texts.length > 0 ? texts.join("\u0000") : undefined;
}

/** Transient (2.1, to the card's author): a live event of one of its cards. */
export const CardEventTransient = z.object({
  type: z.literal("card.event"),
  groupId: Id,
  topicId: Id,
  messageId: Id,
  eventId: Id,
  rev: CardRev,
  blockId: BlockId.optional(),
  actionId: ActionId.optional(),
  event: CardEventName,
  value: z.json().optional(),
  inputs: AnswerInputs.optional(),
  part: PartRef.optional(),
  actorId: Id,
  at: Millis,
});
/** Transient (2.1, to the emitter's connections): the author answered an event. */
export const InteractionResolvedTransient = z.object({
  type: z.literal("interaction.resolved"),
  groupId: Id,
  eventId: Id,
  result: InteractionResult,
  messageId: Id.optional(),
  note: Short(120).optional(),
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
  rev: CardRev,
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

/** Max characters of a decision recorded by hand (记一条). */
export const MAX_RECORDED_DECISION_CHARS = 1000;

/**
 * 1.1: a decision a member recorded by hand (记一条, `POST /v1/groups/:id/decisions`), stored
 * as `decision.recorded` in the group log.
 */
export const RecordedDecision = z.object({
  id: Id,
  /** Seq of the `decision.recorded` event. */
  seq: Seq,
  /** The 话题 it was recorded in; null for the group. */
  topicId: Id.nullable(),
  text: z.string().min(1).max(MAX_RECORDED_DECISION_CHARS),
  recordedBy: Id,
  recordedAt: Millis,
});
export type RecordedDecision = z.infer<typeof RecordedDecision>;

/** Body of `POST /v1/groups/:id/decisions`. */
export const RecordDecisionRequest = z.object({
  text: z
    .string()
    .max(MAX_RECORDED_DECISION_CHARS)
    .refine((t) => t.trim().length > 0, { message: "text is required" }),
  topicId: Id.optional(),
});
export type RecordDecisionRequest = z.infer<typeof RecordDecisionRequest>;
export const RecordDecisionResponse = z.object({ decision: RecordedDecision });
export type RecordDecisionResponse = z.infer<typeof RecordDecisionResponse>;

/** Stored event: a decision recorded by hand. */
export const DecisionRecordedEvent = z.object({
  ...eventBase,
  type: z.literal("decision.recorded"),
  data: z.object({ decision: RecordedDecision }),
});

export const DecisionsResponse = z.object({
  decisions: z.array(GroupDecision),
  hasMore: z.boolean(),
  /** 1.1: decisions recorded by hand with seq < beforeSeq, newest first (at most `limit`). */
  recorded: z.array(RecordedDecision).optional(),
});
export type DecisionsResponse = z.infer<typeof DecisionsResponse>;

// ---------------------------------------------------------------------------
// v1 card derived from a v2 card (for 1.0.x clients)
// ---------------------------------------------------------------------------

const TONE: Record<CardIntent, CardTone> = { ask: "warning", done: "success", working: "info", info: "neutral" };

/** True when a v1 press can answer the card: no inputs, drafts or decisions to fill. */
export function pressableAsV1(card: CardV2): boolean {
  const needs = (b: CardV2Block): boolean =>
    takesInput(b) || b.type === "draft" || b.type === "questionnaire" || (b.type === "group" && (b.blocks ?? []).some(needs));
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
      return b.summary ? [{ type: "text", text: cut(b.label ? `**${b.label}**：${b.summary}` : b.summary, 8000) }] : [];
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
      return (b.blocks ?? []).flatMap(toV1);
    // 2.1 leaves: their plain text, so 1.0.x readers see what the card asks.
    case "diagram": {
      const out: CardBlock[] = b.title ? [{ type: "text", text: `**${cut(b.title, 200)}**` }] : [];
      out.push({ type: "code", lang: "mermaid", text: cut(b.source, 32_768) });
      return out;
    }
    case "rank":
      return [{ type: "list", items: b.items.map((i) => ({ text: cut(i.label, 1000) })) }];
    case "imageChoice":
    case "poll":
      return [{ type: "list", items: b.options.map((o) => ({ text: cut(o.label, 1000) })) }];
    case "matrix":
      return [{ type: "list", items: b.rows.map((r) => ({ text: cut(r.label, 1000) })) }];
    case "rating":
    case "scale":
    case "confirm":
    case "slots":
      return [{ type: "text", text: cut(b.label, 8000) }];
    case "embed":
      return [{ type: "link", label: cut(b.title ?? b.fallbackText ?? "产出", 200), target: { kind: "artifact", artifactId: b.artifactId, version: b.version } }];
    case "buttons":
      return [];
    // Containers flatten in reading order (see cardV2ToV1).
    case "stack":
    case "row":
    case "compare":
    case "tabs":
    case "questionnaire":
      return [];
  }
}

/** Blocks in reading order: top-level blocks, each container followed by its children. */
export function readingOrder(card: Pick<CardV2, "blocks">): AnyBlock[] {
  const named = new Set(card.blocks.flatMap(childIds));
  const byId = new Map(card.blocks.map((b) => [b.id, b] as const));
  const out: AnyBlock[] = [];
  const seen = new Set<string>();
  const visit = (b: AnyBlock) => {
    if (seen.has(b.id)) return;
    seen.add(b.id);
    out.push(b);
    for (const k of childIds(b)) {
      const c = byId.get(k);
      if (c) visit(c);
    }
  };
  for (const b of card.blocks) if (!named.has(b.id)) visit(b);
  return out;
}

function titleLine(b: AnyBlock): CardBlock[] {
  switch (b.type) {
    case "group":
      return b.children && (b.title ?? b.label) ? [{ type: "text", text: `**${cut((b.title ?? b.label) as string, 200)}**` }] : [];
    case "compare":
      return [
        ...(b.question ? [{ type: "text" as const, text: `**${cut(b.question, 200)}**` }] : []),
        { type: "list" as const, items: b.options.map((o) => ({ text: cut(o.label, 1000) })) },
      ];
    case "tabs":
      return [];
    default:
      return [];
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
  const tabOf = new Map<string, string>();
  for (const b of card.blocks) if (b.type === "tabs") for (const t of b.tabs) if (t.children[0]) tabOf.set(t.children[0], t.label);
  for (const b of readingOrder(card)) {
    const tab = tabOf.get(b.id);
    if (tab) blocks.push({ type: "text", text: `**${tab}**` });
    blocks.push(...titleLine(b), ...toV1(b));
  }
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
