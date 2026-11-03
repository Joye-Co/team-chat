// Card v2 answers and 产出 page events (docs/protocol.md, "Cards v2" and "产出 events"). A
// `card.answer` frame goes to the group room, which checks it against the stored card and
// appends `card.answered`. An `artifact.event` frame is not stored (1.1, D9): when the
// version declares the event, it goes to the version's creator bot as a transient
// `artifact.event` and through the bots feature's outbox to a runner bot's run.
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
import { ulid } from "../core/ids";
import { handOffPageEvent } from "../bots/room";

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
    if (b.type === "group") for (const c of b.blocks ?? []) ids.add(c.id);
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

type PageEventArgs = { artifactId: string; version: number; name: string; kind?: "emit" | "ask"; payload?: unknown };

registerRoomRpc(ARTIFACT_EVENT_RPC, async (room: RoomContext, actor, args) => {
  if (!actor) throw new ApiError("internal", "actor required");
  if (!room.isMember(actor.id)) throw new ApiError("forbidden", "not a member of this group");
  const a = args as PageEventArgs;
  const art = room.sql.exec<{ topic_id: string }>("SELECT topic_id FROM artifacts WHERE id = ?", a.artifactId).toArray()[0];
  const v = room.sql
    .exec<{ events: string | null; created_by: string }>(
      "SELECT events, created_by FROM artifact_versions WHERE artifact_id = ? AND version = ?",
      a.artifactId,
      a.version,
    )
    .toArray()[0];
  if (!art || !v) throw new ApiError("not_found", "产出 version not found");
  const declared = v.events ? (JSON.parse(v.events) as ArtifactEventDecl[]) : [];
  const decl = declared.find((d) => d.name === a.name);
  if (!decl || (a.kind !== undefined && (decl.kind ?? "emit") !== a.kind)) {
    throw new ApiError("forbidden", "the page does not declare this event");
  }
  const key = `${room.groupId}:${actor.id}`;
  let bucket = pageBuckets.get(key);
  if (!bucket) {
    bucket = new TokenBucket(PAGE_EVENT_RATE.rate, PAGE_EVENT_RATE.burst);
    pageBuckets.set(key, bucket);
  }
  if (!bucket.take()) throw new ApiError("rate_limited", "too many page events");
  const eventId = ulid();
  const at = Date.now();
  const kind = decl.kind ?? "emit";
  await handOffPageEvent(room, { eventId, actorId: actor.id, artifactId: a.artifactId, version: a.version, name: a.name, payload: a.payload });
  // To the version's creator (SDK bots). Runner bots get it through the outbox above.
  const transient: Record<string, unknown> = {
    type: "artifact.event",
    groupId: room.groupId,
    topicId: art.topic_id,
    artifactId: a.artifactId,
    version: a.version,
    eventId,
    name: a.name,
    kind,
    actorId: actor.id,
    at,
  };
  if (a.payload !== undefined) transient.payload = a.payload;
  if (v.created_by !== actor.id) room.broadcast({ t: "transient", event: transient }, { to: [v.created_by] });
  // `expect` is refined by the delivery feature (presence, runs); this is the author.
  return { eventId, expect: { by: v.created_by, mode: "live" } };
});

registerFrameHandler<ClientFrameOf<"artifact.event">>("artifact.event", {
  rate: "write",
  async handle(frame, c) {
    await c.requireMember(frame.groupId);
    const { artifactId, version, name, kind, payload } = frame;
    return unwrap(await roomStub(c.env, frame.groupId).rpc(frame.groupId, ARTIFACT_EVENT_RPC, c.actor, { artifactId, version, name, kind, payload }));
  },
});
