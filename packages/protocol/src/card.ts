// V1 cards: a small declarative UI schema that bots and integrations post in messages.
// See docs/protocol.md, "Cards".
import { z } from "zod";
import { eventBase, Id, Rid, utf8Bytes } from "./common.ts";

/** Max size of a card: UTF-8 bytes of its JSON text. */
export const MAX_CARD_BYTES = 65_536;
export const MAX_CARD_BLOCKS = 50;
export const MAX_CARD_ACTIONS = 10;
export const MAX_TABLE_COLUMNS = 20;
export const MAX_TABLE_ROWS = 200;
export const MAX_LIST_ITEMS = 200;
export const MAX_FIELDS = 50;
/** Max UTF-8 bytes of an action `value` as JSON. */
export const MAX_ACTION_VALUE_BYTES = 4096;

const Short = (max: number) => z.string().min(1).max(max);
const Text = (max: number) => z.string().max(max);

export const CardTone = z.enum(["neutral", "info", "success", "warning", "danger"]);
export type CardTone = z.infer<typeof CardTone>;

export const CardListStatus = z.enum(["todo", "in_progress", "done", "failed", "skipped"]);
export type CardListStatus = z.infer<typeof CardListStatus>;

export const ActionStyle = z.enum(["primary", "default", "danger"]);
export type ActionStyle = z.infer<typeof ActionStyle>;

/** http or https URL. */
export const WebUrl = z.string().max(2048).regex(/^https?:\/\/[^\s]+$/);

export const LinkTarget = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("url"), url: WebUrl }),
  z.object({ kind: z.literal("topic"), groupId: Id, topicId: Id }),
  z.object({ kind: z.literal("artifact"), artifactId: Id, version: z.int().positive().optional() }),
]);
export type LinkTarget = z.infer<typeof LinkTarget>;

export const TextBlock = z.object({ type: z.literal("text"), text: Text(8000) });
export const FieldsBlock = z.object({
  type: z.literal("fields"),
  fields: z.array(z.object({ label: Short(100), value: Text(1000) })).min(1).max(MAX_FIELDS),
});
export const TableBlock = z
  .object({
    type: z.literal("table"),
    columns: z.array(Text(100)).min(1).max(MAX_TABLE_COLUMNS),
    rows: z.array(z.array(Text(1000))).max(MAX_TABLE_ROWS),
  })
  .refine((t) => t.rows.every((r) => r.length === t.columns.length), {
    message: "every row has one cell per column",
    path: ["rows"],
  });
export const ListBlock = z.object({
  type: z.literal("list"),
  items: z
    .array(z.object({ text: Short(1000), status: CardListStatus.optional() }))
    .min(1)
    .max(MAX_LIST_ITEMS),
});
export const ProgressBlock = z.object({
  type: z.literal("progress"),
  value: z.number().min(0).max(1),
  label: Text(200).optional(),
});
export const CodeBlock = z.object({
  type: z.literal("code"),
  lang: Text(32).optional(),
  text: Text(32_768),
});
export const ImageBlock = z.object({ type: z.literal("image"), fileId: Id, alt: Text(500).optional() });
export const DividerBlock = z.object({ type: z.literal("divider") });
export const LinkBlock = z.object({ type: z.literal("link"), label: Short(200), target: LinkTarget });

export const CardBlock = z.discriminatedUnion("type", [
  TextBlock,
  FieldsBlock,
  TableBlock,
  ListBlock,
  ProgressBlock,
  CodeBlock,
  ImageBlock,
  DividerBlock,
  LinkBlock,
]);
export type CardBlock = z.infer<typeof CardBlock>;

/** Action ids: letters, digits and `_ . : -`. */
export const ActionId = z.string().min(1).max(64).regex(/^[A-Za-z0-9_.:-]+$/);

export const CardAction = z.object({
  id: ActionId,
  label: Short(40),
  style: ActionStyle.optional(),
  /** When set, the client asks this question before it sends the action. */
  confirm: Short(200).optional(),
  /** Any JSON, at most 4 KiB. Sent back in `card.actioned`. */
  value: z
    .json()
    .refine((v) => utf8Bytes(JSON.stringify(v)) <= MAX_ACTION_VALUE_BYTES, { message: "value exceeds 4 KiB" })
    .optional(),
});
export type CardAction = z.infer<typeof CardAction>;

export const Card = z
  .object({
    v: z.literal(1),
    title: Short(200),
    tone: CardTone.optional(),
    status: z.object({ label: Short(40) }).optional(),
    blocks: z.array(CardBlock).max(MAX_CARD_BLOCKS),
    actions: z.array(CardAction).max(MAX_CARD_ACTIONS).optional(),
  })
  .refine((c) => new Set((c.actions ?? []).map((a) => a.id)).size === (c.actions ?? []).length, {
    message: "action ids are unique",
    path: ["actions"],
  })
  .refine((c) => utf8Bytes(JSON.stringify(c)) <= MAX_CARD_BYTES, { message: "card exceeds 64 KiB" });
export type Card = z.infer<typeof Card>;

/** `{ t: "card.action" }`: press a card button. ack.result is omitted. */
export const CardActionFrame = z.object({
  t: z.literal("card.action"),
  rid: Rid,
  groupId: Id,
  messageId: Id,
  actionId: ActionId,
  /** Ignored in V1: the event carries the action's own `value`. */
  value: z.json().optional(),
});
export type CardActionFrame = z.infer<typeof CardActionFrame>;

/** Stored event: a member pressed a card button. `actorId` is the presser. */
export const CardActionedEvent = z.object({
  ...eventBase,
  type: z.literal("card.actioned"),
  data: z.object({
    messageId: Id,
    actionId: ActionId,
    value: z.json().optional(),
  }),
});
