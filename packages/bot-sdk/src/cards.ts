// Card v2 builders (1.1): small functions that make valid v2 cards for the common bot moves:
// ask a question, offer a decision, ask for approval, show a draft to edit, ask for a form,
// and report a result with evidence. Each one checks the card against the protocol schema and
// throws ChatError `invalid` when it does not fit. See docs/bots.md, "Cards v2".
import {
  type ArtifactRef,
  type Card,
  type CardAsk,
  type CardV2,
  CardV2 as CardV2Schema,
  type CardV2Action,
  type CardV2Block,
  type InputKind,
  type RefTarget,
  cardV2ToV1,
} from "@chat/protocol";
import { ChatError } from "./errors.ts";

/** Ids the builders give their blocks and actions; `CardAnswerContext` reads them by default. */
export const CARD_IDS = {
  text: "text",
  choice: "choice",
  draft: "draft",
  submit: "submit",
  approve: "approve",
  deny: "deny",
  cancel: "cancel",
  /** The leading `fail` evidence block of a failed done card. */
  failed: "failed",
} as const;

/** Who may answer, and the other ask fields. `from` is member ids, "requester" or "owners". */
export interface AskOptions {
  from: string[];
  expiresAt?: number;
  /** The answer is a group decision (群里的决定). */
  record?: boolean;
  irreversible?: CardAsk["irreversible"];
}

interface Common {
  title: string;
  /** Plain text for old clients, notifications and search. Default: `text`, or the title. */
  fallback?: string;
  /** First a text block with this Markdown. */
  text?: string;
  /** Extra blocks after the builder's own blocks. */
  blocks?: CardV2Block[];
  /** Bot state stored in the card (at most 16 KiB); answers do not carry it. */
  data?: CardV2["data"];
  /** The card revision; 1 for a new card. */
  rev?: number;
}

export type AskCardOptions = Common & AskOptions & { actions: CardV2Action[] };

export interface ChoiceOption {
  id: string;
  label: string;
  detail?: string;
}

export type DecisionCardOptions = Common &
  AskOptions & {
    options: (string | ChoiceOption)[];
    multiple?: boolean;
    /** Option ids chosen at first. */
    default?: string[];
    submitLabel?: string;
    /** Adds a cancel action with this label. */
    cancelLabel?: string;
  };

export type ApprovalCardOptions = Common &
  AskOptions & {
    approveLabel?: string;
    denyLabel?: string;
    /** The client asks once more before it sends the approval. */
    confirm?: { title: string; body?: string };
  };

export type DraftCardOptions = Common &
  AskOptions & {
    draft: string;
    draftLabel?: string;
    /** Default true: the answerer may edit the draft before sending. */
    editable?: boolean;
    sendLabel?: string;
    cancelLabel?: string;
  };

export interface FormInput {
  id: string;
  kind: InputKind;
  label: string;
  placeholder?: string;
  required?: boolean;
  /** Choices of `select` and `chips`. */
  options?: (string | { id: string; label: string })[];
  min?: number;
  max?: number;
  maxLength?: number;
  default?: unknown;
}

export type FormCardOptions = Common &
  AskOptions & {
    inputs: FormInput[];
    submitLabel?: string;
    cancelLabel?: string;
  };

/** One proof of a result: a check that passed or failed, a file, a link or a 产出 version. */
export interface Evidence {
  label: string;
  state?: "pass" | "fail" | "info";
  detail?: string;
  fileId?: string;
  url?: string;
  artifact?: ArtifactRef;
}

export type DoneCardOptions = Common & {
  /** What was done, in one or two sentences (the text block and the fallback). */
  summary: string;
  evidence?: Evidence[];
  /** Links to what the work made: pull requests, 产出, topics, pages. */
  refs?: { label: string; target: RefTarget }[];
  metrics?: { label: string; value: number; unit?: string; delta?: number; better?: "up" | "down"; note?: string }[];
  /** The work failed: the card says so (a leading `fail` evidence block) but keeps intent "done". */
  failed?: boolean;
  /** The label of that block; default "没有完成". */
  failedLabel?: string;
};

/** Makes the id of a free-text option: its position, so ids stay valid for any label. */
function optionOf(o: string | ChoiceOption, i: number): ChoiceOption {
  return typeof o === "string" ? { id: `o${i + 1}`, label: o } : o;
}

function askOf(o: AskOptions): CardAsk {
  const ask: CardAsk = { from: o.from };
  if (o.expiresAt !== undefined) ask.expiresAt = o.expiresAt;
  if (o.record !== undefined) ask.record = o.record;
  if (o.irreversible) ask.irreversible = o.irreversible;
  return ask;
}

function cancelAction(label: string | undefined): CardV2Action[] {
  return label ? [{ id: CARD_IDS.cancel, label, kind: "cancel" }] : [];
}

/** Checks a v2 card and returns it parsed. Throws ChatError `invalid` with the first issue. */
export function checkCardV2(card: CardV2): CardV2 {
  const r = CardV2Schema.safeParse(card);
  if (!r.success) {
    const issue = r.error.issues[0];
    throw new ChatError("invalid", `card: ${issue?.path.join(".") || "(root)"} ${issue?.message ?? ""}`.trim());
  }
  return r.data;
}

function build(o: Common, intent: CardV2["intent"], own: CardV2Block[], extra: Partial<CardV2>): CardV2 {
  const blocks: CardV2Block[] = [];
  if (o.text) blocks.push({ type: "text", id: CARD_IDS.text, text: o.text });
  blocks.push(...own, ...(o.blocks ?? []));
  const card: CardV2 = {
    v: 2,
    rev: o.rev ?? 1,
    intent,
    title: o.title,
    fallback: o.fallback ?? o.text ?? o.title,
    blocks,
    ...extra,
  };
  if (o.data !== undefined) card.data = o.data;
  return checkCardV2(card);
}

/** Any ask: your own blocks and actions, answered by the members `from` names. */
export function askCard(o: AskCardOptions): CardV2 {
  return build(o, "ask", [], { ask: askOf(o), actions: o.actions });
}

/** Pick one option (or several with `multiple`). The answer has `inputs.choice`. */
export function decisionCard(o: DecisionCardOptions): CardV2 {
  const options = o.options.map(optionOf);
  const block: CardV2Block = { type: "decision", id: CARD_IDS.choice, options };
  if (o.multiple) block.multiple = true;
  if (o.default) block.default = o.default;
  return build(o, "ask", [block], {
    ask: askOf(o),
    actions: [{ id: CARD_IDS.submit, label: o.submitLabel ?? "确定", style: "primary" }, ...cancelAction(o.cancelLabel)],
  });
}

/** Approve or deny. The answer's `actionId` is "approve" or "deny"; old clients can press it too. */
export function approvalCard(o: ApprovalCardOptions): CardV2 {
  const approve: CardV2Action = { id: CARD_IDS.approve, label: o.approveLabel ?? "同意", style: o.irreversible ? "danger" : "primary" };
  if (o.confirm) approve.confirm = o.confirm;
  return build(o, "ask", [], {
    ask: askOf(o),
    actions: [approve, { id: CARD_IDS.deny, label: o.denyLabel ?? "拒绝", kind: "cancel" }],
  });
}

/** A draft the answerer may edit, then send. The answer has `editedValues.draft` when edited. */
export function draftCard(o: DraftCardOptions): CardV2 {
  const block: CardV2Block = { type: "draft", id: CARD_IDS.draft, text: o.draft, editable: o.editable ?? true };
  if (o.draftLabel) block.label = o.draftLabel;
  return build(o, "ask", [block], {
    ask: askOf(o),
    actions: [{ id: CARD_IDS.submit, label: o.sendLabel ?? "发送", style: "primary" }, ...cancelAction(o.cancelLabel)],
  });
}

/** A form of inputs. The answer has `inputs[<input id>]`. */
export function formCard(o: FormCardOptions): CardV2 {
  const blocks: CardV2Block[] = o.inputs.map((i) => {
    const b: Extract<CardV2Block, { type: "input" }> = { type: "input", id: i.id, kind: i.kind, label: i.label };
    if (i.placeholder !== undefined) b.placeholder = i.placeholder;
    if (i.required !== undefined) b.required = i.required;
    if (i.options) b.options = i.options.map((x, n) => (typeof x === "string" ? { id: `o${n + 1}`, label: x } : x));
    if (i.min !== undefined) b.min = i.min;
    if (i.max !== undefined) b.max = i.max;
    if (i.maxLength !== undefined) b.maxLength = i.maxLength;
    if (i.default !== undefined) b.default = i.default as never;
    return b;
  });
  return build(o, "ask", blocks, {
    ask: askOf(o),
    actions: [{ id: CARD_IDS.submit, label: o.submitLabel ?? "提交", style: "primary" }, ...cancelAction(o.cancelLabel)],
  });
}

/** The result of a run: a summary with its proof (checks, files, links, 产出) and no actions. */
export function doneCard(o: DoneCardOptions): CardV2 {
  const own: CardV2Block[] = [];
  if (o.failed) own.push({ type: "evidence", id: CARD_IDS.failed, label: o.failedLabel ?? "没有完成", state: "fail" });
  for (const [i, m] of (o.metrics ?? []).entries()) own.push({ type: "metric", id: `metric${i + 1}`, ...m });
  for (const [i, e] of (o.evidence ?? []).entries()) own.push({ type: "evidence", id: `evidence${i + 1}`, ...e });
  for (const [i, r] of (o.refs ?? []).entries()) own.push({ type: "ref", id: `ref${i + 1}`, label: r.label, target: r.target });
  return build({ ...o, text: o.text ?? o.summary, fallback: o.fallback ?? o.summary }, "done", own, {});
}

/**
 * The v1 card to send next to a failed done card: the derived one with tone `danger`, so
 * 1.0.x clients do not show the failure in the success tone. Undefined for other cards.
 */
export function failedV1Card(card: CardV2): Card | undefined {
  const failed = card.intent === "done" && card.blocks.some((b) => b.type === "evidence" && b.id === CARD_IDS.failed && b.state === "fail");
  return failed ? { ...cardV2ToV1(card), tone: "danger" } : undefined;
}

/** The same card as answered: a higher rev and no submit or cancel actions (link actions stay). */
export function answeredCard(card: CardV2, change: Partial<Omit<CardV2, "v" | "rev">> = {}): CardV2 {
  const links = (card.actions ?? []).filter((a) => a.kind === "link");
  const next: CardV2 = { ...card, ...change, v: 2, rev: card.rev + 1 };
  if (change.actions === undefined) {
    if (links.length > 0) next.actions = links;
    else delete next.actions;
  }
  return checkCardV2(next);
}
