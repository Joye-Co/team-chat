// Private answers (CARD21 §5): a poll with `public` not true returns counts only, and an
// anonymous questionnaire gives the card's author aggregates and hides who answered from
// every other viewer. The room keeps the full answers (the server and the audit need them);
// what a member gets live, in `fetch` pages, in history and in acks is that member's view:
// - the answerer: their own answers, in full;
// - the card's author: other answers of an anonymous questionnaire with `actorId` "anonymous",
//   and `card.answered` with the values but without the voter; of a poll, the tally only;
// - every other member: the tally, no other answers, `card.answered`/`card.retracted` without
//   the voter or the values.
// Once a card was private, it stays private (its log already holds private answers).
import type { CardAnswer, CardState, CardV2, Cause, Event, Message } from "@chat/protocol";
import { registerEventView, registerMessageView, registerRoomMigration } from "../core/registry";

registerRoomMigration({
  name: "cards2-5",
  up: "CREATE TABLE card_private (message_id TEXT PRIMARY KEY, anonymous INTEGER NOT NULL);",
});

/** The `actorId` other viewers get in place of a hidden voter. */
export const ANONYMOUS_ID = "anonymous";

/** The privacy of a card: undefined (public), "poll" (counts only) or "anonymous". */
export function privacyOf(card: CardV2): "poll" | "anonymous" | undefined {
  if (card.blocks.some((b) => b.type === "questionnaire" && b.anonymous === true)) return "anonymous";
  if (card.blocks.some((b) => b.type === "poll" && b.public !== true)) return "poll";
  return undefined;
}

/** Records that a stored card is private (call on every card write, in its transaction). */
export function markPrivate(sql: SqlStorage, messageId: string, card: CardV2): void {
  const p = privacyOf(card);
  if (!p) return;
  sql.exec(
    `INSERT INTO card_private (message_id, anonymous) VALUES (?, ?)
     ON CONFLICT (message_id) DO UPDATE SET anonymous = max(anonymous, excluded.anonymous)`,
    messageId,
    p === "anonymous" ? 1 : 0,
  );
}

type Private = { authorId: string; anonymous: boolean };

function privateOf(sql: SqlStorage, messageId: string | undefined): Private | undefined {
  if (!messageId) return undefined;
  const r = sql
    .exec<{ anonymous: number; author_id: string }>(
      "SELECT p.anonymous, m.author_id FROM card_private p JOIN messages m ON m.id = p.message_id WHERE p.message_id = ?",
      messageId,
    )
    .toArray()[0];
  return r ? { authorId: r.author_id, anonymous: r.anonymous === 1 } : undefined;
}

/** True when the card a cause names is private (its stored cause then hides the voter). */
export function hidesCause(sql: SqlStorage, eventMessageId: string): boolean {
  return privateOf(sql, eventMessageId) !== undefined;
}

/**
 * The actor id the card's author (an SDK member or a runner's run) gets for an event on the
 * card: `ANONYMOUS_ID` when the card is private. Only the room and the owners' audit keep the
 * real actor.
 */
export function shownActor(sql: SqlStorage, messageId: string, actorId: string): string {
  return hidesCause(sql, messageId) ? ANONYMOUS_ID : actorId;
}

/** A cause as other viewers may see it. */
export function publicCause(sql: SqlStorage, cause: Cause, eventMessageId: string): Cause {
  return hidesCause(sql, eventMessageId) ? { ...cause, actorId: ANONYMOUS_ID } : cause;
}

/** The cause as stored next to a card or message: without the voter when its card is private. */
export function shownCause(sql: SqlStorage, cause: Cause): Cause {
  const r = sql.exec<{ message_id: string }>("SELECT message_id FROM card_events WHERE event_id = ?", cause.eventId).toArray()[0];
  return r ? publicCause(sql, cause, r.message_id) : cause;
}

/** The card state `viewerId` may see. */
export function viewState(state: CardState, viewerId: string, p: Private): CardState {
  const author = viewerId === p.authorId;
  const answers: CardAnswer[] = [];
  for (const a of state.answers) {
    if (a.actorId === viewerId) answers.push(a);
    else if (author && p.anonymous) answers.push({ ...a, actorId: ANONYMOUS_ID });
  }
  const out: CardState = { ...state, answers };
  if (out.ask?.answeredBy !== undefined && out.ask.answeredBy !== viewerId) {
    const { answeredBy: _b, ...ask } = out.ask;
    out.ask = ask;
  }
  if (out.lastCause && out.lastCause.actorId !== viewerId) out.lastCause = { ...out.lastCause, actorId: ANONYMOUS_ID };
  delete out.tallyBy;
  return out;
}

/** The message `viewerId` may see. */
export function viewMessage(m: Message, viewerId: string, sql: SqlStorage): Message {
  if (!m.cardState) return m;
  const p = privateOf(sql, m.id);
  return p ? { ...m, cardState: viewState(m.cardState, viewerId, p) } : m;
}

registerMessageView(viewMessage);

registerEventView((event, viewerId, sql) => {
  const e = event as Event & { data: Record<string, unknown> };
  switch (e.type) {
    case "card.answered":
    case "card.retracted": {
      if (e.actorId === viewerId) return event;
      const p = privateOf(sql, e.data.messageId as string);
      if (!p) return event;
      const data = { ...e.data };
      if (!(e.type === "card.answered" && viewerId === p.authorId)) {
        delete data.value;
        delete data.inputs;
        delete data.editedValues;
      }
      return { ...e, actorId: ANONYMOUS_ID, data } as Event;
    }
    case "message.updated": {
      const state = e.data.cardState as CardState | undefined;
      if (!state) return event;
      const p = privateOf(sql, e.data.messageId as string);
      if (!p) return event;
      // The state change of an answer or retract carries the voter as its actor.
      const actorId = e.actorId === viewerId || e.actorId === p.authorId ? e.actorId : ANONYMOUS_ID;
      return { ...e, actorId, data: { ...e.data, cardState: viewState(state, viewerId, p) } } as Event;
    }
    case "message.created": {
      const m = e.data.message as Message | undefined;
      if (!m?.cardState) return event;
      const v = viewMessage(m, viewerId, sql);
      return v === m ? event : ({ ...e, data: { ...e.data, message: v } } as Event);
    }
    default:
      return event;
  }
});
