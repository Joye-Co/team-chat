// Group decisions (群里的决定, docs/protocol.md "Cards v2"): answers to v2 asks whose
// `ask.record` is true at the answered rev. The event writer keeps the question and the choice
// when the answer is stored, because the card's author usually edits the card afterwards (a
// later rev without `ask`). The inputs stay in the event, so a delete scrubs them in one place.
import { type CardV2, type CardV2Block, DecisionsResponse, type GroupDecision, MAX_DECISIONS_PAGE } from "@chat/protocol";
import { z } from "zod";
import { isGroupMember } from "../core/db";
import { roomStub } from "../core/env";
import { ApiError, unwrap } from "../core/errors";
import { type ArchivedEvent, registerEventWriter, registerRoomMigration, registerRoomRpc, registerRoute } from "../core/registry";
import type { CardAnswerArgs } from "./index";

export const DECISIONS_RPC = "cards.decisions";
const DEFAULT_PAGE = 50;

registerRoomMigration({
  name: "cards2-1",
  up: `
    CREATE TABLE card_decisions (
      seq INTEGER PRIMARY KEY,
      message_id TEXT NOT NULL,
      topic_id TEXT NOT NULL,
      rev INTEGER NOT NULL,
      title TEXT NOT NULL,
      action_id TEXT NOT NULL,
      action_label TEXT NOT NULL,
      choices TEXT,
      actor_id TEXT NOT NULL,
      at INTEGER NOT NULL
    );
    CREATE INDEX card_decisions_message ON card_decisions(message_id);
  `,
});

/** Option ids of an input value: `"a"`, `["a"]`, `{value}`, `{values}`. */
function pickedIds(v: unknown): unknown[] {
  if (Array.isArray(v)) return v;
  if (typeof v === "string") return [v];
  if (v && typeof v === "object") {
    const o = v as { value?: unknown; values?: unknown };
    if (Array.isArray(o.values)) return o.values;
    if (o.value !== undefined) return [o.value];
  }
  return [];
}

/**
 * Labels of the options an answer chose, in block order: `decision`, `compare`, `imageChoice`,
 * `poll` and `confirm` blocks, from `inputs` and from the value of an immediate commit.
 */
export function chosenLabels(card: CardV2, inputs: Record<string, unknown> | undefined, commit?: { blockId: string; value: unknown }): string[] {
  const out: string[] = [];
  const visit = (b: CardV2Block) => {
    if (b.type === "group") return (b.blocks ?? []).forEach(visit);
    const v = commit && commit.blockId === b.id ? commit.value : inputs?.[b.id];
    if (v === undefined) return;
    const ids = pickedIds(v);
    if (b.type === "confirm") {
      if (ids[0] === true) out.push(b.yes);
      if (ids[0] === false) out.push(b.no);
      return;
    }
    if (b.type !== "decision" && b.type !== "compare" && b.type !== "imageChoice" && b.type !== "poll") return;
    for (const o of b.options as { id: string; label: string }[]) if (ids.includes(o.id)) out.push(o.label);
  };
  card.blocks.forEach(visit);
  return out;
}

/** The stored v2 card of a message, or undefined. */
export function storedCard(sql: SqlStorage, messageId: string): { card: CardV2; authorId: string; topicId: string } | undefined {
  const row = sql
    .exec<{ ext: string | null; author_id: string; topic_id: string }>("SELECT ext, author_id, topic_id FROM messages WHERE id = ?", messageId)
    .toArray()[0];
  const card = row?.ext ? (JSON.parse(row.ext) as { cardV2?: CardV2 }).cardV2 : undefined;
  return row && card ? { card, authorId: row.author_id, topicId: row.topic_id } : undefined;
}

// In the commit (and import) transaction: the message still holds the answered rev.
registerEventWriter((event: ArchivedEvent, sql) => {
  if (event.type !== "card.answered") return;
  const d = event.data as CardAnswerArgs;
  const stored = storedCard(sql, d.messageId);
  if (!stored || stored.card.rev !== d.rev || stored.card.ask?.record !== true) return;
  // An immediate commit (2.1) has no action: the block is the "action", its pick the label.
  const commit = d.actionId === undefined && d.blockId ? { blockId: d.blockId, value: d.value } : undefined;
  const choices = chosenLabels(stored.card, d.inputs, commit);
  const action = d.actionId !== undefined ? stored.card.actions?.find((a) => a.id === d.actionId) : commit && choices[0] ? { id: commit.blockId, label: [...choices[0]].slice(0, 40).join("") } : undefined;
  if (!action) return;
  sql.exec(
    `INSERT OR IGNORE INTO card_decisions (seq, message_id, topic_id, rev, title, action_id, action_label, choices, actor_id, at)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    event.seq,
    d.messageId,
    stored.topicId,
    d.rev,
    stored.card.title,
    action.id,
    action.label,
    choices.length > 0 ? JSON.stringify(choices) : null,
    event.actorId,
    event.at,
  );
});

// A deleted card message keeps no decision rows (live deletes and imports).
registerEventWriter((event: ArchivedEvent, sql) => {
  if (event.type !== "message.deleted") return;
  const messageId = (event.data as { messageId?: string }).messageId ?? event.messageId;
  if (messageId) sql.exec("DELETE FROM card_decisions WHERE message_id = ?", messageId);
});

type DecisionRow = {
  seq: number;
  message_id: string;
  topic_id: string;
  rev: number;
  title: string;
  action_id: string;
  action_label: string;
  choices: string | null;
  actor_id: string;
  at: number;
  data: string;
};

registerRoomRpc(DECISIONS_RPC, (room, actor, args): DecisionsResponse => {
  if (!actor || !room.isMember(actor.id)) throw new ApiError("forbidden", "not a member of this group");
  const { beforeSeq, limit } = args as { beforeSeq?: number; limit: number };
  const rows = room.sql
    .exec<DecisionRow>(
      `SELECT d.*, e.data FROM card_decisions d
       JOIN messages m ON m.id = d.message_id AND m.deleted_at IS NULL
       JOIN events e ON e.seq = d.seq
       WHERE d.seq < ? ORDER BY d.seq DESC LIMIT ?`,
      beforeSeq ?? Number.MAX_SAFE_INTEGER,
      limit + 1,
    )
    .toArray() as DecisionRow[];
  const decisions = rows.slice(0, limit).map((r): GroupDecision => {
    const d = JSON.parse(r.data) as CardAnswerArgs;
    const out: GroupDecision = {
      seq: r.seq,
      messageId: r.message_id,
      topicId: r.topic_id,
      rev: r.rev,
      title: r.title,
      actionId: r.action_id,
      actionLabel: r.action_label,
      answeredBy: r.actor_id,
      answeredAt: r.at,
    };
    if (r.choices) out.choices = JSON.parse(r.choices) as string[];
    if (d.inputs !== undefined) out.inputs = d.inputs as GroupDecision["inputs"];
    if (d.editedValues !== undefined) out.editedValues = d.editedValues;
    return out;
  });
  return { decisions, hasMore: rows.length > limit };
});

const Query = z.object({
  beforeSeq: z.coerce.number().int().positive().optional(),
  limit: z.coerce.number().int().min(1).max(MAX_DECISIONS_PAGE).default(DEFAULT_PAGE),
});

registerRoute({
  method: "GET",
  path: "/v1/groups/:id/decisions",
  auth: "member",
  async handler({ env, url, params, auth }) {
    const a = auth as NonNullable<typeof auth>;
    const q = Query.safeParse(Object.fromEntries(url.searchParams));
    if (!q.success) throw new ApiError("invalid", "bad beforeSeq or limit");
    if (!(await isGroupMember(env.DB, params.id, a.memberId))) throw new ApiError("not_found", "group not found");
    const actor = { id: a.memberId, level: a.member.level, kind: a.member.kind, tokenId: a.tokenId };
    return unwrap(await roomStub(env, params.id).rpc(params.id, DECISIONS_RPC, actor, q.data)) as DecisionsResponse;
  },
});
