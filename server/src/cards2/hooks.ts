// Message writes of the interaction loop: Card v2 revs and state on send and edit, causes
// (`send.causeEventId`, `edit.causeEventId`), refs to parts, and what a delete clears. Also
// the room alarm that expires asks and closes polls.
import type { BlockRef, BlockRefInput, CardState, CardV2, Cause, Message } from "@chat/protocol";
import { MAX_CARD_V2_REVS } from "@chat/protocol";
import { ApiError } from "../core/errors";
import { type RoomContext, registerMessageWriteHook, registerRoomAlarm } from "../core/registry";
import { cardLock, writeCardUpdate } from "./events";
import { markPrivate, shownCause } from "./privacy";
import { checkBodyLength, clearRefsTo, indexRefs, resolveRefs, withPrefix, withoutPrefix } from "./refs";
import { checkEmbeds, deriveV1, directory, recordRev, refreshTopicWaiting, rememberRequester, stateForWrite, syncTimer } from "./state";
import { CAUSE_MAX_AGE_MS, causeOf, eventRow, extOf, messageRow } from "./store";

/**
 * The cause an author names (INTERACT §4.1): an event of the last 30 minutes on one of the
 * author's cards (`messageId` set: that card). Else `invalid`, `interact.bad_cause`.
 */
export function checkCause(room: RoomContext, eventId: string, authorId: string, messageId?: string): Cause {
  const r = eventRow(room.sql, eventId);
  if (!r || r.author_id !== authorId || (messageId !== undefined && r.message_id !== messageId) || r.at < Date.now() - CAUSE_MAX_AGE_MS) {
    throw new ApiError("invalid", "cause is not a recent event of this card", "interact.bad_cause");
  }
  return causeOf(r);
}

/** Sends `interaction.resolved` to the emitter's connections and marks the event resolved. */
export function resolved(room: RoomContext, cause: Cause, result: "patched" | "posted" | "replied" | "ignored" | "failed", messageId?: string, note?: string): void {
  room.sql.exec("UPDATE card_events SET resolved = ? WHERE event_id = ?", result, cause.eventId);
  const event: Record<string, unknown> = { type: "interaction.resolved", groupId: room.groupId, eventId: cause.eventId, result };
  if (messageId) event.messageId = messageId;
  if (note) event.note = note;
  room.broadcast({ t: "transient", event }, { to: [cause.actorId] });
}

registerMessageWriteHook({
  async send(frame, room, actor, draft) {
    const f = frame as { cardV2?: CardV2; card?: unknown; causeEventId?: string; blockRefs?: BlockRefInput[]; topicId?: string };
    const ext: Record<string, unknown> = { ...(draft.ext ?? {}) };
    const cause = f.causeEventId ? checkCause(room, f.causeEventId, actor.id) : undefined;
    if (cause) ext.cause = shownCause(room.sql, cause);
    let refs: BlockRef[] | undefined;
    if (f.blockRefs && f.blockRefs.length > 0) {
      refs = resolveRefs(room, f.blockRefs);
      draft.body = withPrefix(refs, draft.body);
      checkBodyLength(draft.body);
      ext.blockRefs = refs;
    }
    let card: CardV2 | undefined;
    let state: CardState | undefined;
    if (f.cardV2) {
      const now = Date.now();
      const dir = await directory(room);
      // The server owns the rev: 1 on send.
      card = { ...f.cardV2, rev: 1 };
      checkEmbeds(room.sql, card);
      state = await stateForWrite(room, dir, card, undefined, actor.id, f.topicId ?? null, now);
      if (cause) state.lastCause = shownCause(room.sql, cause);
      ext.cardV2 = card;
      ext.cardState = state;
      if (f.card === undefined) ext.card = deriveV1(card, state, dir);
    }
    if (!cause && !refs && !card) return undefined;
    draft.ext = ext;
    return {
      inTx(tx, message) {
        if (refs) indexRefs(room.sql, message.id, refs);
        if (card && state) {
          recordRev(room.sql, message.id, card);
          markPrivate(room.sql, message.id, card);
          rememberRequester(room.sql, message.id, state);
          syncTimer(room, message.id, state);
          if (state.ask?.status === "open" && state.ask.blocking) refreshTopicWaiting(room, tx, message.topicId, actor.id);
        }
      },
      after(message) {
        if (cause) resolved(room, cause, "posted", message.id);
      },
    };
  },

  async edit(frame, room, actor, current, draft) {
    const f = frame as { cardV2?: CardV2; card?: unknown; baseRev?: number; causeEventId?: string; body: string };
    const cause = f.causeEventId ? checkCause(room, f.causeEventId, actor.id, current.id) : undefined;
    const refs = current.blockRefs;
    if (refs && refs.length > 0) {
      // An old device sends the body with the quote lines; a new one without.
      draft.body = withPrefix(refs, withoutPrefix(refs, draft.body));
      checkBodyLength(draft.body);
    }
    let card: CardV2 | undefined;
    let state: CardState | undefined;
    if (f.cardV2) {
      const stored = current.cardV2;
      if (stored) {
        if (f.baseRev !== undefined ? f.baseRev !== stored.rev : f.cardV2.rev <= stored.rev) {
          throw new ApiError("conflict", "the card has a newer rev", "card.stale_rev");
        }
        if (stored.rev >= MAX_CARD_V2_REVS) throw new ApiError("too_large", `at most ${MAX_CARD_V2_REVS} revs per card`, "card.rev_limit");
      }
      const dir = await directory(room);
      card = { ...f.cardV2, rev: (stored?.rev ?? 0) + 1 };
      checkEmbeds(room.sql, card);
      state = await stateForWrite(room, dir, card, current.cardState, current.authorId, current.topicId, Date.now(), current.id);
      if (cause) state.lastCause = shownCause(room.sql, cause);
      draft.changed.cardV2 = card;
      draft.changed.cardState = state;
      if (f.card === undefined) draft.changed.card = deriveV1(card, state, dir);
      if (f.body === "") draft.body = card.fallback || card.title;
    } else if (f.baseRev !== undefined) throw new ApiError("invalid", "baseRev goes with cardV2");
    if (cause) draft.changed.cause = shownCause(room.sql, cause);
    if (!cause && !card) return undefined;
    return {
      inTx(tx, message) {
        if (card && state) {
          recordRev(room.sql, message.id, card);
          markPrivate(room.sql, message.id, card);
          syncTimer(room, message.id, state);
          refreshTopicWaiting(room, tx, message.topicId, actor.id);
        }
      },
      after(message) {
        if (cause) resolved(room, cause, "patched", message.id);
      },
    };
  },

  remove(room, tx, message: Message, actorId) {
    clearRefsTo(room, tx, { messageId: message.id }, actorId);
    room.sql.exec("DELETE FROM block_refs WHERE message_id = ?", message.id);
    if (message.cardV2) {
      room.sql.exec("DELETE FROM card_timers WHERE message_id = ?", message.id);
      if (message.cardState?.ask?.status === "open" && message.cardState.ask.blocking) refreshTopicWaiting(room, tx, message.topicId, actorId);
    }
  },
});

/** Expires asks whose `expiresAt` passed and closes polls at `closesAt`. */
registerRoomAlarm(async (room) => {
  const now = Date.now();
  const due = room.sql.exec<{ message_id: string }>("SELECT message_id FROM card_timers WHERE at <= ?", now).toArray();
  if (due.length > 0) {
    const dir = await directory(room);
    for (const { message_id } of due) {
      await room.serial(cardLock(message_id), async () => {
        const row = messageRow(room.sql, message_id);
        const ext = row ? extOf(row) : {};
        const card = ext.cardV2;
        const state = ext.cardState;
        if (!row || row.deleted_at !== null || !card || !state?.ask || state.ask.status !== "open") {
          room.sql.exec("DELETE FROM card_timers WHERE message_id = ?", message_id);
          return;
        }
        if ((state.ask.expiresAt ?? Number.POSITIVE_INFINITY) > now) return;
        const poll = card.blocks.some((b) => b.type === "poll");
        const next: CardState = { ...state, ask: { ...state.ask, status: poll ? "closed" : "expired" } };
        room.commit((tx) => writeCardUpdate(room, tx, row, ext, row.author_id, dir, { card, state: next, cardChanged: false, now }));
      });
    }
  }
  const next = room.sql.exec<{ at: number | null }>("SELECT min(at) AS at FROM card_timers").toArray()[0]?.at;
  if (next !== null && next !== undefined) room.wakeAt(Math.max(next, now + 50));
});
