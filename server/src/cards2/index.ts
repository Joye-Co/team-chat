// Card v2.1 interaction loop (docs/protocol.md, "Interaction loop"): `card.event` and its short
// forms (`card.answer`, `card.answer.retract`, and a v1 `card.action` on a v2 card),
// `card.patch`, `interaction.resolve`; card state, causes and refs on send, edit and delete
// (./hooks). An `artifact.event` frame is not stored (1.1, D9): when the version declares the
// event, it goes to the version's creator bot as a transient `artifact.event` and through the
// bots feature's outbox to a runner bot's run.
import {
  type ArtifactEventDecl,
  type CardPatchAck,
  type ClientFrameOf,
  type Message,
  type PatchOp,
} from "@chat/protocol";
import { roomStub } from "../core/env";
import { ApiError, unwrap } from "../core/errors";
import { TokenBucket } from "../core/ratelimit";
import { type Actor, type RoomContext, registerFrameHandler, registerRoomRpc } from "../core/registry";
import { ulid } from "../core/ids";
import { handOffPageEvent } from "../bots/room";
import { type CardEventArgs, cardEvent, loadCard, writeCardUpdate } from "./events";
import { checkCause, resolved } from "./hooks";
import { applyPatch } from "./patch";
import { checkEmbeds, directory, isPick, recordRev, stateForWrite } from "./state";
import { EVENT_KEEP_MS, eventRow, pruneEvents } from "./store";

import "./decisions";

export {
  type CardEventDelivery,
  type CardEventHandler,
  type RequesterResolver,
  closeRetractWindow,
  registerCardEventHandler,
  registerRequesterResolver,
} from "./delivery";

export const CARD_EVENT_RPC = "cards.event";
export const CARD_PRESS_RPC = "cards.press";
export const CARD_PATCH_RPC = "cards.patch";
export const CARD_RESOLVE_RPC = "cards.resolve";
export const ARTIFACT_EVENT_RPC = "artifacts.event";

/** Page events: 5 per second per member, bursts of 10. */
const PAGE_EVENT_RATE = { rate: 5, burst: 10 };
/** Patches: bursts of 3 per message, then 1 per second. */
const PATCH_RATE = { rate: 1, burst: 3 };

/** Data of a stored `card.answered` event (2.0 fields and the 2.1 ones). */
export type CardAnswerArgs = {
  messageId: string;
  rev: number;
  actionId?: string;
  inputs?: Record<string, unknown>;
  editedValues?: Record<string, string>;
  answerId?: string;
  askId?: string;
  eventId?: string;
  event?: string;
  blockId?: string;
  value?: unknown;
};

const pageBuckets = new Map<string, TokenBucket>();
const patchBuckets = new Map<string, TokenBucket>();

function member(room: RoomContext, actor: Actor | null): Actor {
  if (!actor) throw new ApiError("internal", "actor required");
  if (!room.isMember(actor.id)) throw new ApiError("forbidden", "not a member of this group");
  return actor;
}

registerRoomRpc(CARD_EVENT_RPC, (room, actor, args) => cardEvent(room, member(room, actor), args as CardEventArgs));

/**
 * A v1 `card.action` on a v2 card (1.0.x clients press the derived v1 buttons): a pick
 * button answers its block with that value; another button is `action.press` without inputs.
 */
registerRoomRpc(CARD_PRESS_RPC, async (room, actor, args) => {
  const a = member(room, actor);
  const { messageId, actionId } = args as { messageId: string; actionId: string };
  const dir = await directory(room);
  const { ext, card } = await loadCard(room, messageId, dir);
  const v1 = ext.card as { actions?: { id: string; value?: unknown }[] } | undefined;
  const pick = v1?.actions?.find((x) => x.id === actionId)?.value;
  if (isPick(pick)) {
    const block = card.blocks.find((b) => b.id === pick.blockId);
    if (!block) throw new ApiError("not_found", "block not found", "card.block_gone");
    const value = { value: pick.value };
    const commits = block.deliver === "immediate" || block.type === "compare" || block.type === "confirm";
    if (commits) {
      const event = block.type === "compare" ? "compare.pick" : block.type === "input" ? "input.submit" : "choice.submit";
      return cardEvent(room, a, { messageId, rev: card.rev, blockId: block.id, event, value });
    }
    const submit = card.actions?.find((x) => (x.kind ?? "submit") === "submit" && x.inputs?.length === 1 && x.inputs[0] === block.id);
    if (!submit) throw new ApiError("not_found", "card action not found", "card.block_gone");
    return cardEvent(room, a, { messageId, rev: card.rev, actionId: submit.id, event: "action.press", inputs: { [block.id]: value } });
  }
  return cardEvent(room, a, { messageId, rev: card.rev, actionId, event: "action.press" });
});

registerRoomRpc(CARD_PATCH_RPC, async (room, actor, args): Promise<CardPatchAck> => {
  const a = member(room, actor);
  const f = args as { messageId: string; baseRev: number; ops: PatchOp[]; cause?: string; clientPatchId?: string };
  const now = Date.now();
  if (f.clientPatchId) {
    const dup = room.sql
      .exec<{ ack: string }>(
        "SELECT ack FROM card_patches WHERE actor_id = ? AND message_id = ? AND client_patch_id = ? AND at >= ?",
        a.id,
        f.messageId,
        f.clientPatchId,
        now - EVENT_KEEP_MS,
      )
      .toArray()[0];
    if (dup) return { ...(JSON.parse(dup.ack) as CardPatchAck), duplicate: true };
  }
  const dir = await directory(room);
  const { row, ext, card, state } = await loadCard(room, f.messageId, dir);
  if (row.author_id !== a.id) throw new ApiError("forbidden", "only the author can patch a card");
  if (f.baseRev !== card.rev) throw new ApiError("conflict", "the card has a newer rev", "card.stale_rev");
  const cause = f.cause ? checkCause(room, f.cause, a.id, row.id) : undefined;
  const next = applyPatch(card, f.ops);
  checkEmbeds(room.sql, next);
  const key = `${room.groupId}:${row.id}`;
  let bucket = patchBuckets.get(key);
  if (!bucket) {
    bucket = new TokenBucket(PATCH_RATE.rate, PATCH_RATE.burst);
    patchBuckets.set(key, bucket);
    if (patchBuckets.size > 2000) patchBuckets.clear();
  }
  if (!bucket.take()) throw new ApiError("rate_limited", "too many patches for this card");
  const nextState = await stateForWrite(room, dir, next, state, row.author_id, row.topic_id, now);
  if (cause) nextState.lastCause = cause;
  let ack: CardPatchAck | undefined;
  room.commit((tx) => {
    writeCardUpdate(room, tx, row, ext, a.id, dir, { card: next, state: nextState, cardChanged: true, cause, body: next.fallback || next.title, now });
    recordRev(room.sql, row.id, next);
    const m = room.sql.exec<Record<string, SqlStorageValue>>("SELECT * FROM messages WHERE id = ?", row.id).toArray()[0];
    ack = { message: toMessage(room, m), rev: next.rev };
    if (f.clientPatchId) {
      room.sql.exec(
        "INSERT OR REPLACE INTO card_patches (actor_id, message_id, client_patch_id, at, ack) VALUES (?, ?, ?, ?, ?)",
        a.id,
        row.id,
        f.clientPatchId,
        now,
        JSON.stringify(ack),
      );
    }
    pruneEvents(room.sql, now);
  });
  if (cause) resolved(room, cause, "patched", row.id);
  return ack as CardPatchAck;
});

function toMessage(room: RoomContext, r: Record<string, SqlStorageValue>): Message {
  const m: Message = {
    ...(r.ext ? (JSON.parse(r.ext as string) as Record<string, unknown>) : {}),
    id: r.id as string,
    clientMsgId: r.client_msg_id as string,
    groupId: room.groupId,
    topicId: r.topic_id as string,
    authorId: r.author_id as string,
    body: r.body as string,
    mentions: JSON.parse(r.mentions as string) as string[],
    createdSeq: r.created_seq as number,
    createdAt: r.created_at as number,
    editedAt: (r.edited_at as number | null) ?? null,
    deletedAt: (r.deleted_at as number | null) ?? null,
    kind: (r.kind as Message["kind"]) ?? "text",
  };
  if (r.streaming) m.streaming = true;
  return m;
}

registerRoomRpc(CARD_RESOLVE_RPC, (room, actor, args) => {
  const a = member(room, actor);
  const f = args as { eventId: string; result: "ignored" | "failed"; note?: string };
  const r = eventRow(room.sql, f.eventId);
  if (!r) throw new ApiError("not_found", "event not found");
  if (r.author_id !== a.id) throw new ApiError("forbidden", "only the card's author resolves its events");
  if (r.resolved) throw new ApiError("conflict", "the event was resolved");
  resolved(room, { eventId: r.event_id, actorId: r.actor_id, at: r.at, event: r.event }, f.result, r.message_id, f.note);
  return undefined;
});

const rpc = async (frame: { groupId: string }, c: { env: Parameters<typeof roomStub>[0]; actor: Actor; requireMember(g: string): Promise<void> }, name: string, args: unknown) => {
  await c.requireMember(frame.groupId);
  return unwrap(await roomStub(c.env, frame.groupId).rpc(frame.groupId, name, c.actor, args));
};

registerFrameHandler<ClientFrameOf<"card.event">>("card.event", {
  rate: "write",
  handle(f, c) {
    const { messageId, rev, clientEventId, blockId, actionId, event, value, inputs, part } = f;
    return rpc(f, c, CARD_EVENT_RPC, { messageId, rev, clientEventId, blockId, actionId, event, value, inputs, part });
  },
});

registerFrameHandler<ClientFrameOf<"card.answer">>("card.answer", {
  rate: "write",
  handle(f, c) {
    const { messageId, rev, actionId, inputs, editedValues, clientEventId } = f;
    return rpc(f, c, CARD_EVENT_RPC, { messageId, rev, actionId, inputs, editedValues, clientEventId, event: "action.press" });
  },
});

registerFrameHandler<ClientFrameOf<"card.answer.retract">>("card.answer.retract", {
  rate: "write",
  handle(f, c) {
    return rpc(f, c, CARD_EVENT_RPC, { messageId: f.messageId, rev: 1, event: "answer.retract", value: { answerId: f.answerId } });
  },
});

registerFrameHandler<ClientFrameOf<"card.patch">>("card.patch", {
  rate: "write",
  handle(f, c) {
    const { messageId, baseRev, ops, cause, clientPatchId } = f;
    return rpc(f, c, CARD_PATCH_RPC, { messageId, baseRev, ops, cause, clientPatchId });
  },
});

registerFrameHandler<ClientFrameOf<"interaction.resolve">>("interaction.resolve", {
  rate: "write",
  async handle(f, c) {
    await rpc(f, c, CARD_RESOLVE_RPC, { eventId: f.eventId, result: f.result, note: f.note });
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
