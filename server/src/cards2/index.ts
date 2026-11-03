// Card v2.1 interaction loop (docs/protocol.md, "Interaction loop"): `card.event` and its short
// forms (`card.answer`, `card.answer.retract`, and a v1 `card.action` on a v2 card),
// `card.patch`, `interaction.resolve`; card state, causes and refs on send, edit and delete
// (./hooks). 产出 page events (`artifact.event`) are the files feature's (server/src/files/bridge.ts).
import {
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
import { type CardEventArgs, cardEvent, loadCard, writeCardUpdate } from "./events";
import { closeRetractWindow } from "./delivery";
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
export const CARD_ANSWER_TAKEN_RPC = "cards.answerTaken";

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

const patchBuckets = new Map<string, TokenBucket>();

function member(room: RoomContext, actor: Actor | null): Actor {
  if (!actor) throw new ApiError("internal", "actor required");
  if (!room.isMember(actor.id)) throw new ApiError("forbidden", "not a member of this group");
  return actor;
}

registerRoomRpc(CARD_EVENT_RPC, (room, actor, args) => cardEvent(room, member(room, actor), args as CardEventArgs));

/**
 * BotDesk: the card author's run took an answer (`input.accepted` of its `run.answer`), so the
 * answerer can no longer retract it (INTERACT §3.6). `seq` is the `card.answered` event's seq.
 */
registerRoomRpc(CARD_ANSWER_TAKEN_RPC, (room, actor, args) => {
  const a = member(room, actor);
  const { seq } = args as { seq: number };
  const ev = room.sql
    .exec<{ data: string; message_id: string | null }>("SELECT data, message_id FROM events WHERE seq = ? AND type = 'card.answered'", seq)
    .toArray()[0];
  if (!ev?.message_id) throw new ApiError("not_found", "answer not found");
  const author = room.sql.exec<{ author_id: string }>("SELECT author_id FROM messages WHERE id = ?", ev.message_id).toArray()[0];
  if (author?.author_id !== a.id) throw new ApiError("forbidden", "only the card's author takes its answers");
  const answerId = (JSON.parse(ev.data) as { answerId?: string }).answerId;
  if (answerId) closeRetractWindow(room.sql, answerId);
  return { ok: true };
});

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
  const nextState = await stateForWrite(room, dir, next, state, row.author_id, row.topic_id, now, row.id);
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
