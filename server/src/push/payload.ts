// What a push says and offers (docs/protocol.md, "Push"). Pure: no I/O, unit-tested directly.
import { MAX_PUSH_ACTIONS, PUSH_CATEGORY_ASK, type PushAction, type PushAsk, type PushData } from "@chat/protocol";

/** Block types that need input before an answer: a card with one of them is answered in the app. */
const INPUT_BLOCKS = new Set([
  "decision",
  "input",
  "draft",
  "rank",
  "rating",
  "scale",
  "confirm",
  "imageChoice",
  "matrix",
  "slots",
  "poll",
  "buttons",
  "compare",
  "questionnaire",
]);

function hasInputBlock(v: unknown, depth = 0): boolean {
  if (depth > 8 || !v || typeof v !== "object") return false;
  if (Array.isArray(v)) return v.some((x) => hasInputBlock(x, depth + 1));
  const o = v as Record<string, unknown>;
  if (typeof o.type === "string" && INPUT_BLOCKS.has(o.type)) return true;
  return Object.values(o).some((x) => hasInputBlock(x, depth + 1));
}

type CardLike = {
  rev?: number;
  ask?: { irreversible?: unknown } | undefined;
  blocks?: unknown;
  actions?: { id: string; label: string; kind?: string; style?: string; confirm?: unknown; inputs?: unknown[]; role?: string }[];
};

/**
 * The answers a notification may offer for this card: footer actions that need no more input and
 * can be taken back. Empty when the ask is irreversible or the card has input blocks.
 */
export function quickActions(card: CardLike | undefined): PushAction[] {
  if (!card?.ask || card.ask.irreversible || hasInputBlock(card.blocks)) return [];
  const out: PushAction[] = [];
  for (const a of card.actions ?? []) {
    const kind = a.kind ?? "submit";
    if (kind !== "submit" && kind !== "reject") continue;
    if (a.confirm || (a.inputs && a.inputs.length > 0) || a.style === "danger" || a.role === "approve") continue;
    out.push({ id: a.id, label: a.label.slice(0, 40) });
  }
  return out.slice(0, MAX_PUSH_ACTIONS);
}

/** Cuts text to `max` code points with an ellipsis, on one line. */
export function excerpt(text: string, max = 140): string {
  const line = text.replace(/\s+/g, " ").trim();
  const chars = [...line];
  return chars.length <= max ? line : `${chars.slice(0, max - 1).join("")}…`;
}

export interface AlertInput {
  data: PushData;
  /** Group name; for a 私聊 the other member's name. */
  groupName: string;
  isDm: boolean;
  actorName: string;
  /** Message text or card title, for `mention` and `message`. */
  text?: string;
  topicTitle?: string;
  /** For `result` from a 产出. */
  artifactTitle?: string;
}

/** The alert's title and body. Product nouns only (成员, 群, 话题, 产出). */
export function alertText(i: AlertInput): { title: string; body: string } {
  const title = i.groupName;
  const topic = i.topicTitle ? excerpt(i.topicTitle, 60) : "";
  const event = i.data.notifyEvent;
  if (event === "blocked") return { title, body: topic ? `卡住了：${topic}` : `${i.actorName} 卡住了` };
  switch (i.data.reason) {
    case "mention":
    case "message": {
      const text = excerpt(i.text || "发来一条消息");
      return { title, body: i.isDm ? text : `${i.actorName}：${text}` };
    }
    case "waiting":
      return { title, body: topic ? `等你回复：${topic}` : `${i.actorName} 在等你回复` };
    case "result":
      if (i.artifactTitle) return { title, body: `新产出：${excerpt(i.artifactTitle, 60)}` };
      return { title, body: topic ? `有结果了：${topic}` : "有结果了" };
    default:
      return { title, body: excerpt(i.text || "有新消息") };
  }
}

/**
 * The category of a push with answers: one per card and rev, so its buttons are that card's
 * answers. The app registers it (the notification extension when the push arrives, the app for
 * notifications it finds delivered); until then the notification has no buttons and a tap opens.
 */
export function askCategory(ask: PushAsk): string {
  return `${PUSH_CATEGORY_ASK}.${ask.messageId}.${ask.rev}`;
}

/** The APNs JSON body of an alert push. */
export function alertPayload(data: PushData, alert: { title: string; body: string }, badge: number): Record<string, unknown> {
  const aps: Record<string, unknown> = {
    alert,
    badge,
    sound: "default",
    "thread-id": data.topicId ?? data.groupId,
    "mutable-content": 1,
  };
  if (data.ask) aps.category = askCategory(data.ask);
  return { aps, tc: data };
}

/** The APNs JSON body of a badge-only push. */
export function badgePayload(badge: number): Record<string, unknown> {
  return { aps: { badge } };
}

export type { PushAsk };
