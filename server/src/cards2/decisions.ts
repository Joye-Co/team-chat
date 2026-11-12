// Group decisions (群里的决定, docs/protocol.md "Cards v2"): answers to v2 asks whose
// `ask.record` is true at the answered rev, except on private cards. The event writer keeps the question and the choice
// when the answer is stored, because the card's author usually edits the card afterwards (a
// later rev without `ask`). The inputs stay in the event, so a delete scrubs them in one place.
import {
  type CardV2,
  type CardV2Block,
  DecisionsResponse,
  type GroupDecision,
  MAX_DECISIONS_PAGE,
  RecordDecisionRequest,
  type RecordDecisionResponse,
  type RecordedDecision,
} from "@chat/protocol";
import { z } from "zod";
import { isGroupMember } from "../core/db";
import { roomStub } from "../core/env";
import { ApiError, unwrap } from "../core/errors";
import { ulid } from "../core/ids";
import { type ArchivedEvent, registerEventWriter, registerRoomMigration, registerRoomRpc, registerRoute } from "../core/registry";
import type { CardAnswerArgs } from "./index";
import { hidesCause } from "./privacy";

export const DECISIONS_RPC = "cards.decisions";
export const RECORD_DECISION_RPC = "cards.recordDecision";
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
  recordAnswerDecision(sql, event, storedCard(sql, d.messageId));
});

/**
 * Stores the group decision of a `card.answered` event, given the card at the answered rev
 * (the live writer passes the stored card; the backfill the card from the log).
 */
export function recordAnswerDecision(sql: SqlStorage, event: ArchivedEvent, stored: { card: CardV2; topicId: string } | undefined): void {
  const d = event.data as CardAnswerArgs;
  if (!stored || stored.card.rev !== d.rev || stored.card.ask?.record !== true) return;
  // A private card (a non-public poll, an anonymous questionnaire) records no group decision:
  // a decision names who answered (cards2/privacy).
  if (hidesCause(sql, d.messageId)) return;
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
}

// A deleted card message keeps no decision rows (live deletes and imports).
registerEventWriter((event: ArchivedEvent, sql) => {
  if (event.type !== "message.deleted") return;
  const messageId = (event.data as { messageId?: string }).messageId ?? event.messageId;
  if (messageId) sql.exec("DELETE FROM card_decisions WHERE message_id = ?", messageId);
});

// 记一条: decisions recorded by hand, one `decision.recorded` event each (live and imports).
registerRoomMigration({
  name: "cards2-7",
  up: `
    CREATE TABLE recorded_decisions (
      seq INTEGER PRIMARY KEY,
      id TEXT NOT NULL,
      topic_id TEXT,
      text TEXT NOT NULL,
      recorded_by TEXT NOT NULL,
      recorded_at INTEGER NOT NULL
    );
  `,
});

registerEventWriter((event: ArchivedEvent, sql) => {
  if (event.type !== "decision.recorded") return;
  const d = (event.data as { decision?: RecordedDecision }).decision;
  if (!d) return;
  sql.exec(
    "INSERT OR IGNORE INTO recorded_decisions (seq, id, topic_id, text, recorded_by, recorded_at) VALUES (?, ?, ?, ?, ?, ?)",
    event.seq,
    d.id,
    d.topicId,
    d.text,
    d.recordedBy,
    d.recordedAt,
  );
});

registerRoomRpc(RECORD_DECISION_RPC, (room, actor, args): RecordDecisionResponse => {
  if (!actor || !room.isMember(actor.id)) throw new ApiError("forbidden", "not a member of this group");
  const { text, topicId } = args as { text: string; topicId?: string };
  if (topicId !== undefined && room.sql.exec("SELECT 1 FROM topics WHERE id = ?", topicId).toArray().length === 0) {
    throw new ApiError("not_found", "topic not found");
  }
  const now = Date.now();
  const decision = room.commit((tx): RecordedDecision => {
    const d: RecordedDecision = { id: ulid(now), seq: tx.nextSeq(), topicId: topicId ?? null, text: text.trim(), recordedBy: actor.id, recordedAt: now };
    tx.append({ type: "decision.recorded", actorId: actor.id, topicId: topicId ?? null, data: { decision: d } });
    return d;
  });
  return { decision };
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
  if (!actor || !(room.readingAs || room.isMember(actor.id))) throw new ApiError("forbidden", "not a member of this group");
  const { beforeSeq, limit } = args as { beforeSeq?: number; limit: number };
  const rows = room.sql
    .exec<DecisionRow>(
      `SELECT d.*, e.data FROM card_decisions d
       JOIN messages m ON m.id = d.message_id AND m.deleted_at IS NULL
       JOIN events e ON e.seq = d.seq
       WHERE d.seq < ? AND d.message_id NOT IN (SELECT message_id FROM card_private)
       ORDER BY d.seq DESC LIMIT ?`,
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
  const recordedRows = room.sql
    .exec<{ seq: number; id: string; topic_id: string | null; text: string; recorded_by: string; recorded_at: number }>(
      "SELECT * FROM recorded_decisions WHERE seq < ? ORDER BY seq DESC LIMIT ?",
      beforeSeq ?? Number.MAX_SAFE_INTEGER,
      limit + 1,
    )
    .toArray();
  const recorded = recordedRows.slice(0, limit).map(
    (r): RecordedDecision => ({ id: r.id, seq: r.seq, topicId: r.topic_id, text: r.text, recordedBy: r.recorded_by, recordedAt: r.recorded_at }),
  );
  return { decisions, hasMore: rows.length > limit || recordedRows.length > limit, recorded };
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

registerRoute({
  method: "POST",
  path: "/v1/groups/:id/decisions",
  auth: "member",
  async handler({ env, params, body, auth }) {
    const a = auth as NonNullable<typeof auth>;
    const req = await body(RecordDecisionRequest);
    if (!(await isGroupMember(env.DB, params.id, a.memberId))) throw new ApiError("not_found", "group not found");
    const actor = { id: a.memberId, level: a.member.level, kind: a.member.kind, tokenId: a.tokenId };
    return unwrap(await roomStub(env, params.id).rpc(params.id, RECORD_DECISION_RPC, actor, req)) as RecordDecisionResponse;
  },
});
