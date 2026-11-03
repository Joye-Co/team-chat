// Card v2 answers and 产出 page events (docs/protocol.md, "Cards v2" and "产出 events"). A
// `card.answer` frame goes to the group room, which checks it against the stored card and
// appends `card.answered`; an `artifact.event` frame appends `artifact.evented` when the
// version declares the event. Routing answers and events to the bot's run is the bots
// feature's job (its event writer and outbox).
import {
  type ArtifactEventDecl,
  type CardAnswerFrame,
  type CardV2,
  type CardV2Block,
  type ClientFrameOf,
} from "@chat/protocol";
import { roomStub } from "../core/env";
import { ApiError, unwrap } from "../core/errors";
import { TokenBucket } from "../core/ratelimit";
import { type Actor, type RoomContext, registerFrameHandler, registerRoomRpc } from "../core/registry";

import "./decisions";

export const CARD_ANSWER_RPC = "cards.answer";
export const ARTIFACT_EVENT_RPC = "artifacts.event";

/** At most one answer per member per message per this many ms. */
const ANSWER_INTERVAL_MS = 1000;
/** Page events: 5 per second per member, bursts of 10. */
const PAGE_EVENT_RATE = { rate: 5, burst: 10 };

export type CardAnswerArgs = Pick<CardAnswerFrame, "messageId" | "rev" | "actionId" | "inputs" | "editedValues">;
type MessageRow = { topic_id: string; kind: string | null; ext: string | null; deleted_at: number | null };

const answeredAt = new Map<string, number>();
const pageBuckets = new Map<string, TokenBucket>();

function blockIds(blocks: CardV2Block[]): Set<string> {
  const ids = new Set<string>();
  for (const b of blocks) {
    ids.add(b.id);
    if (b.type === "group") for (const c of b.blocks) ids.add(c.id);
  }
  return ids;
}

function mayAnswer(card: CardV2, actor: Actor, topicCreator: string | null): boolean {
  if (!card.ask) return true;
  return card.ask.from.some((f) =>
    f === "owners" ? actor.level === "owner" : f === "requester" ? actor.id === topicCreator : f === actor.id,
  );
}

/** Checks an answer against the stored card and appends `card.answered`. Returns its seq. */
export function answerCard(room: RoomContext, actor: Actor, a: CardAnswerArgs): { seq: number } {
  const row = room.sql
    .exec<MessageRow>("SELECT topic_id, kind, ext, deleted_at FROM messages WHERE id = ?", a.messageId)
    .toArray()[0] as MessageRow | undefined;
  if (!row || row.deleted_at !== null || row.kind !== "card") throw new ApiError("not_found", "card message not found");
  const card = row.ext ? (JSON.parse(row.ext) as { cardV2?: CardV2 }).cardV2 : undefined;
  if (!card) throw new ApiError("not_found", "not a v2 card");
  if (a.rev !== card.rev) throw new ApiError("conflict", "the card changed");
  const action = (card.actions ?? []).find((x) => x.id === a.actionId && (x.kind ?? "submit") !== "link");
  if (!action) throw new ApiError("not_found", "card action not found");
  const ids = blockIds(card.blocks);
  for (const k of [...Object.keys(a.inputs ?? {}), ...Object.keys(a.editedValues ?? {})]) {
    if (!ids.has(k)) throw new ApiError("invalid", `no block ${k} in the card`);
  }
  if (card.ask) {
    const topic = room.sql.exec<{ created_by: string }>("SELECT created_by FROM topics WHERE id = ?", row.topic_id).toArray()[0];
    if (!mayAnswer(card, actor, topic?.created_by ?? null)) throw new ApiError("forbidden", "this ask is for other members");
    if (card.ask.expiresAt !== undefined && Date.now() >= card.ask.expiresAt) throw new ApiError("conflict", "the ask expired");
    const taken = room.sql
      .exec(
        "SELECT 1 FROM events WHERE message_id = ? AND type = 'card.answered' AND json_extract(data, '$.rev') = ? LIMIT 1",
        a.messageId,
        a.rev,
      )
      .toArray();
    if (taken.length > 0) throw new ApiError("conflict", "the ask was answered");
  }
  const key = `${room.groupId}:${actor.id}:${a.messageId}`;
  const now = Date.now();
  const last = answeredAt.get(key);
  if (last !== undefined && now - last < ANSWER_INTERVAL_MS) throw new ApiError("rate_limited", "one answer per second");
  answeredAt.set(key, now);
  if (answeredAt.size > 1000) for (const [k, t] of answeredAt) if (now - t >= ANSWER_INTERVAL_MS) answeredAt.delete(k);

  const data: CardAnswerArgs = { messageId: a.messageId, rev: a.rev, actionId: a.actionId };
  if (a.inputs !== undefined) data.inputs = a.inputs;
  if (a.editedValues !== undefined) data.editedValues = a.editedValues;
  const event = room.commit((tx) => {
    const seq = tx.nextSeq();
    room.sql.exec("UPDATE topics SET last_seq = ? WHERE id = ?", seq, row.topic_id);
    return tx.append({ type: "card.answered", actorId: actor.id, topicId: row.topic_id, messageId: a.messageId, data });
  });
  return { seq: event.seq };
}

registerRoomRpc(CARD_ANSWER_RPC, (room, actor, args) => {
  if (!actor) throw new ApiError("internal", "actor required");
  if (!room.isMember(actor.id)) throw new ApiError("forbidden", "not a member of this group");
  return answerCard(room, actor, args as CardAnswerArgs);
});

registerFrameHandler<ClientFrameOf<"card.answer">>("card.answer", {
  rate: "write",
  async handle(frame, c) {
    await c.requireMember(frame.groupId);
    const { messageId, rev, actionId, inputs, editedValues } = frame;
    unwrap(await roomStub(c.env, frame.groupId).rpc(frame.groupId, CARD_ANSWER_RPC, c.actor, { messageId, rev, actionId, inputs, editedValues }));
  },
});

// ---------------------------------------------------------------------------
// 产出 page events
// ---------------------------------------------------------------------------

type PageEventArgs = { artifactId: string; version: number; name: string; payload?: unknown };

registerRoomRpc(ARTIFACT_EVENT_RPC, (room: RoomContext, actor, args) => {
  if (!actor) throw new ApiError("internal", "actor required");
  if (!room.isMember(actor.id)) throw new ApiError("forbidden", "not a member of this group");
  const a = args as PageEventArgs;
  const art = room.sql.exec<{ topic_id: string }>("SELECT topic_id FROM artifacts WHERE id = ?", a.artifactId).toArray()[0];
  const v = room.sql
    .exec<{ events: string | null }>("SELECT events FROM artifact_versions WHERE artifact_id = ? AND version = ?", a.artifactId, a.version)
    .toArray()[0];
  if (!art || !v) throw new ApiError("not_found", "产出 version not found");
  const declared = v.events ? (JSON.parse(v.events) as ArtifactEventDecl[]) : [];
  if (!declared.some((d) => d.name === a.name)) throw new ApiError("forbidden", "the page does not declare this event");
  const key = `${room.groupId}:${actor.id}`;
  let bucket = pageBuckets.get(key);
  if (!bucket) {
    bucket = new TokenBucket(PAGE_EVENT_RATE.rate, PAGE_EVENT_RATE.burst);
    pageBuckets.set(key, bucket);
  }
  if (!bucket.take()) throw new ApiError("rate_limited", "too many page events");
  const data: PageEventArgs = { artifactId: a.artifactId, version: a.version, name: a.name };
  if (a.payload !== undefined) data.payload = a.payload;
  const event = room.commit((tx) => {
    const seq = tx.nextSeq();
    room.sql.exec("UPDATE topics SET last_seq = ? WHERE id = ?", seq, art.topic_id);
    return tx.append({ type: "artifact.evented", actorId: actor.id, topicId: art.topic_id, data });
  });
  return { seq: event.seq };
});

registerFrameHandler<ClientFrameOf<"artifact.event">>("artifact.event", {
  rate: "write",
  async handle(frame, c) {
    await c.requireMember(frame.groupId);
    const { artifactId, version, name, payload } = frame;
    unwrap(await roomStub(c.env, frame.groupId).rpc(frame.groupId, ARTIFACT_EVENT_RPC, c.actor, { artifactId, version, name, payload }));
  },
});
