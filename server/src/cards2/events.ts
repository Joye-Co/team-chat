// `card.event` (INTERACT §3): every interaction with a v2 card. Answers are stored
// (`card.answered`, then `message.updated` with the new `cardState` and v1 card); retracts are
// stored (`card.retracted`); every other event is live: a transient `card.event` to the card's
// author only, plus an audit entry. Validation order: §3.7.
import {
  type CardAnswer,
  type CardEventAck,
  type CardState,
  type CardV2,
  type CardV2Action,
  type CardV2Block,
  type Cause,
  type Expect,
  type PartRef,
  allBlocks,
  takesInput,
} from "@chat/protocol";
import { auditStatement } from "../core/audit";
import { ApiError } from "../core/errors";
import { ulid } from "../core/ids";
import { TokenBucket } from "../core/ratelimit";
import type { Actor, RoomContext, RoomTx } from "../core/registry";
import { type CardEventDelivery, expectFor } from "./delivery";
import { eventValueProblem, eventsOf, inputProblem } from "./inputs";
import { auditValue } from "../core/auditvalue";
import {
  type Directory,
  deriveV1,
  directory,
  pollOf,
  refreshTopicWaiting,
  retally,
  revSurface,
  stateForWrite,
  surfaceIds,
  surfaceKey,
  syncTimer,
} from "./state";
import { EVENT_KEEP_MS, type MessageRow, extOf, messageRow, pruneEvents } from "./store";

export type CardEventArgs = {
  messageId: string;
  rev: number;
  clientEventId?: string;
  blockId?: string;
  actionId?: string;
  event: string;
  value?: unknown;
  inputs?: Record<string, unknown>;
  editedValues?: Record<string, string>;
  part?: PartRef;
};

type Block = CardV2Block;

/** Answers: 1 per second per member and message. Live: 5/s (burst 10) per member and message, 120/min per member and group. */
const ANSWER_INTERVAL_MS = 1000;
const LIVE_RATE = { rate: 5, burst: 10 };
const LIVE_PER_MINUTE = 120;
/** The answerer may take an answer back for this long (unless the author acted earlier). */
export const RETRACT_WINDOW_MS = 10 * 60_000;

const answeredAt = new Map<string, number>();
const liveBuckets = new Map<string, TokenBucket>();
const liveMinute = new Map<string, { window: number; n: number }>();

const COMMITS = new Set(["choice.submit", "input.submit", "compare.pick"]);
/** Footer actions that answer an open ask (`press` is live, `link` sends nothing). */
const isAnswerAction = (a: CardV2Action) => a.kind !== "press" && a.kind !== "link";

function prune<V>(m: Map<string, V>, keep: (v: V) => boolean): void {
  if (m.size <= 2000) return;
  for (const [k, v] of m) if (!keep(v)) m.delete(k);
}

function rateAnswer(key: string, now: number): void {
  const last = answeredAt.get(key);
  if (last !== undefined && now - last < ANSWER_INTERVAL_MS) throw new ApiError("rate_limited", "one answer per second");
  answeredAt.set(key, now);
  prune(answeredAt, (t) => now - t < ANSWER_INTERVAL_MS);
}

function rateLive(groupId: string, actorId: string, messageId: string, now: number): void {
  const k = `${groupId}:${actorId}:${messageId}`;
  let b = liveBuckets.get(k);
  if (!b) {
    b = new TokenBucket(LIVE_RATE.rate, LIVE_RATE.burst);
    liveBuckets.set(k, b);
  }
  const window = Math.floor(now / 60_000);
  const gk = `${groupId}:${actorId}`;
  const m = liveMinute.get(gk);
  const n = m && m.window === window ? m.n : 0;
  if (n >= LIVE_PER_MINUTE) throw new ApiError("rate_limited", "too many card events this minute");
  if (!b.take()) throw new ApiError("rate_limited", "too many card events");
  liveMinute.set(gk, { window, n: n + 1 });
  prune(liveMinute, (v) => v.window === window);
  if (liveBuckets.size > 2000) liveBuckets.clear();
}

/** Who may send a live event of this block (INTERACT §3.5). */
function mayEmit(b: Block | undefined, state: CardState, actor: Actor, surface: Set<string>, room: RoomContext): boolean {
  const open = state.ask?.status === "open";
  const emitBy = b?.emitBy ?? (b && open && surface.has(b.id) ? "ask" : "members");
  if (emitBy === "members") return room.isMember(actor.id);
  if (emitBy === "owners") return actor.level === "owner";
  if (emitBy === "ask") return open ? (state.ask?.from ?? []).includes(actor.id) : room.isMember(actor.id);
  if (emitBy === "requester") return (state.emitters?.[b?.id ?? ""] ?? []).includes(actor.id);
  return emitBy.memberIds.includes(actor.id);
}

/** The card, its state (made when an older card has none) and the message row. */
export async function loadCard(room: RoomContext, messageId: string, dir: Directory) {
  const row = messageRow(room.sql, messageId);
  if (!row || row.deleted_at !== null || row.kind !== "card") throw new ApiError("not_found", "card message not found");
  const ext = extOf(row);
  const card = ext.cardV2;
  if (!card) throw new ApiError("not_found", "not a v2 card");
  const state = ext.cardState ?? (await stateForWrite(room, dir, card, undefined, row.author_id, row.topic_id, row.created_at));
  return { row, ext, card, state };
}

/**
 * Stores a new card state (and optionally a new card) on the message and appends
 * `message.updated` with the full card, the state, the refreshed v1 card and the cause.
 */
export function writeCardUpdate(
  room: RoomContext,
  tx: RoomTx,
  row: MessageRow,
  ext: Record<string, unknown>,
  actorId: string,
  dir: Directory,
  change: { card: CardV2; state: CardState; cardChanged: boolean; cause?: Cause; body?: string; now: number },
): void {
  const card = deriveV1(change.card, change.state, dir);
  const next: Record<string, unknown> = { ...ext, cardV2: change.card, card, cardState: change.state };
  if (change.cause) next.cause = change.cause;
  const body = change.body ?? row.body;
  const editedAt = change.cardChanged ? change.now : (row.edited_at ?? row.created_at);
  const seq = tx.nextSeq();
  room.sql.exec(
    "UPDATE messages SET ext = ?, body = ?, edited_at = ?, streaming = 0 WHERE id = ?",
    JSON.stringify(next),
    body,
    change.cardChanged ? change.now : row.edited_at,
    row.id,
  );
  const data: Record<string, unknown> = { messageId: row.id, body, editedAt, streaming: false, card, cardState: change.state };
  if (change.cardChanged) data.cardV2 = change.card;
  if (change.cause) data.cause = change.cause;
  tx.append({ type: "message.updated", actorId, topicId: row.topic_id, messageId: row.id, data });
  room.sql.exec("UPDATE topics SET last_seq = ? WHERE id = ?", seq, row.topic_id);
  syncTimer(room, row.id, change.state);
  refreshTopicWaiting(room, tx, row.topic_id, actorId);
}

function findBlock(card: CardV2, id: string): Block | undefined {
  return allBlocks(card).find((b) => b.id === id) as Block | undefined;
}

/** Input checks of an answer or a submit: known blocks, the action's list, required, values. */
function checkInputs(card: CardV2, allowed: Set<string>, inputs: Record<string, unknown> | undefined, edits: Record<string, string> | undefined, answer: boolean): void {
  for (const [id, v] of Object.entries(inputs ?? {})) {
    const b = findBlock(card, id);
    if (!b) throw new ApiError("invalid", `no block ${id} in the card`);
    if (!allowed.has(id)) throw new ApiError("invalid", `block ${id} is not an input of this action`);
    const p = inputProblem(b, v);
    if (p) throw new ApiError("invalid", `${id}: ${p}`);
  }
  for (const id of Object.keys(edits ?? {})) {
    const b = findBlock(card, id);
    if (!b) throw new ApiError("invalid", `no block ${id} in the card`);
    if (b.type !== "draft") throw new ApiError("invalid", `block ${id} is not a draft`);
  }
  if (!answer) return;
  for (const id of allowed) {
    const b = findBlock(card, id);
    const required = b && "required" in b && b.required === true;
    if (required && inputs?.[id] === undefined) throw new ApiError("invalid", `${id} is required`);
  }
}

function inputSet(card: CardV2, action: CardV2Action | undefined, container: Block | undefined): Set<string> {
  if (container?.type === "questionnaire") return new Set(container.children);
  if (action?.inputs) return new Set(action.inputs);
  return new Set(allBlocks(card).filter((b) => takesInput(b as Block) || b.type === "draft").map((b) => b.id));
}

/** Handles one card event (all forms). Returns the ack result. */
export async function cardEvent(room: RoomContext, actor: Actor, a: CardEventArgs): Promise<CardEventAck> {
  const now = Date.now();
  if (actor.kind === "bot") throw new ApiError("forbidden", "only people send card events", "interact.bot_emitter");
  if (a.clientEventId) {
    const dup = room.sql
      .exec<{ ack: string }>(
        "SELECT ack FROM card_events WHERE actor_id = ? AND message_id = ? AND client_event_id = ? AND at >= ?",
        actor.id,
        a.messageId,
        a.clientEventId,
        now - EVENT_KEEP_MS,
      )
      .toArray()[0];
    if (dup) return { ...(JSON.parse(dup.ack) as CardEventAck), duplicate: true };
  }
  const dir = await directory(room);
  const { row, ext, card, state } = await loadCard(room, a.messageId, dir);
  if (a.rev > card.rev) throw new ApiError("invalid", "rev is newer than the card");
  const author = dir.get(row.author_id);
  if (!author || author.deactivated) throw new ApiError("forbidden", "the card's author cannot respond", "interact.no_responder");
  if (a.event === "answer.retract") return retract(room, actor, a, { row, ext, card, state, dir, now });

  // The target in the current rev, and whether this gesture is an answer.
  const surface = surfaceIds(card);
  let block: Block | undefined;
  let action: CardV2Action | undefined;
  let answerShaped = false;
  if (a.actionId !== undefined) {
    action = card.actions?.find((x) => x.id === a.actionId);
    if (!action) throw new ApiError("not_found", "card action not found", "card.block_gone");
    if (action.kind === "link") throw new ApiError("not_found", "a link action sends nothing");
    if (a.event !== "action.press") throw new ApiError("invalid", "a footer action sends action.press", "card.undeclared_event");
    answerShaped = isAnswerAction(action);
  } else if (a.blockId !== undefined) {
    block = findBlock(card, a.blockId);
    if (!block) throw new ApiError("not_found", "block not found", "card.block_gone");
    if (!eventsOf(block).includes(a.event)) throw new ApiError("invalid", `${block.type} does not send ${a.event}`, "card.undeclared_event");
    const commit =
      (COMMITS.has(a.event) && (block.deliver === "immediate" || block.type === "confirm" || block.type === "compare" || block.type === "poll")) ||
      (block.type === "questionnaire" && a.event === "action.press");
    if (!commit && !(block.events ?? []).includes(a.event)) {
      throw new ApiError("invalid", `${a.event} is not declared on ${block.id}`, "card.undeclared_event");
    }
    answerShaped = commit && surface.has(block.id);
  } else throw new ApiError("invalid", "blockId or actionId is required");

  // The card's current ask (a rev without `ask` keeps the old round's state for the record).
  const ask = card.ask ? state.ask : undefined;
  const answer = answerShaped && ask !== undefined;
  if (answer) {
    if (!ask.from.includes(actor.id)) throw new ApiError("forbidden", "this ask is for other members");
  } else if (!mayEmit(block, state, actor, surface, room)) {
    throw new ApiError("forbidden", "this block is not for you");
  }

  // Values against the block definitions.
  if (block && block.type !== "questionnaire") {
    const p = eventValueProblem(block, a.event, a.value);
    if (p) throw new ApiError("invalid", `${block.id}: ${p}`);
  }
  if (a.inputs !== undefined || a.editedValues !== undefined || (answer && (action || block?.type === "questionnaire"))) {
    checkInputs(card, inputSet(card, action, block), a.inputs, a.editedValues, answer && (action === undefined || (action.kind ?? "submit") === "submit"));
  }

  if (answer) {
    const poll = pollOf(card);
    switch (ask.status) {
      case "answered":
        throw new ApiError("conflict", "the ask was answered", "card.already_answered");
      case "expired":
        throw new ApiError("conflict", "the ask expired", "card.expired");
      case "closed":
      case "superseded":
        throw new ApiError("conflict", "the ask was replaced", "card.superseded");
    }
    if (ask.expiresAt !== undefined && now >= ask.expiresAt) throw new ApiError("conflict", "the ask expired", "card.expired");
    if (a.rev < card.rev) {
      const then = revSurface(room.sql, row.id, a.rev);
      if (then !== undefined && then !== surfaceKey(card)) throw new ApiError("conflict", "the question changed", "card.superseded");
    }
    if (poll && !poll.changeable && state.answers.some((x) => x.actorId === actor.id && x.askId === ask.id && x.retractedAt === undefined)) {
      throw new ApiError("conflict", "you voted already", "card.already_answered");
    }
    // The ask checks come first, so a second answer learns that the ask is answered.
    rateAnswer(`${room.groupId}:${actor.id}:${a.messageId}`, now);
    return storeAnswer(room, actor, a, { row, ext, card, state, dir, now, block, action });
  }
  rateLive(room.groupId, actor.id, a.messageId, now);
  return liveEvent(room, actor, a, { row, card, now });
}

type Loaded = { row: MessageRow; ext: Record<string, unknown>; card: CardV2; state: CardState; dir: Directory; now: number };

function insertEvent(
  room: RoomContext,
  r: { eventId: string; row: MessageRow; actor: Actor; a: CardEventArgs; stored: boolean; now: number; ack: CardEventAck; cardRev?: number },
): void {
  room.sql.exec(
    `INSERT INTO card_events (event_id, message_id, author_id, actor_id, client_event_id, event, block_id, action_id, stored, at, ack, card_rev)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    r.eventId,
    r.row.id,
    r.row.author_id,
    r.actor.id,
    r.a.clientEventId ?? null,
    r.a.event,
    r.a.blockId ?? null,
    r.a.actionId ?? null,
    r.stored ? 1 : 0,
    r.now,
    JSON.stringify(r.ack),
    r.cardRev ?? null,
  );
  pruneEvents(room.sql, r.now);
}

async function finishAck(room: RoomContext, ack: CardEventAck, d: CardEventDelivery): Promise<CardEventAck> {
  const expect: Expect = await expectFor(room, d);
  const out = { ...ack, expect };
  room.sql.exec("UPDATE card_events SET ack = ? WHERE event_id = ?", JSON.stringify(out), ack.eventId);
  return out;
}

async function storeAnswer(
  room: RoomContext,
  actor: Actor,
  a: CardEventArgs,
  l: Loaded & { block?: Block; action?: CardV2Action },
): Promise<CardEventAck> {
  const { row, card, now } = l;
  const ask = l.state.ask as NonNullable<CardState["ask"]>;
  const eventId = ulid(now);
  const answerId = ulid(now);
  const entry: CardAnswer = { answerId, askId: ask.id, eventId, actorId: actor.id, at: now, rev: a.rev, event: a.event };
  if (a.blockId) entry.blockId = a.blockId;
  if (a.actionId) entry.actionId = a.actionId;
  if (a.value !== undefined) entry.value = a.value as CardAnswer["value"];
  if (a.inputs !== undefined) entry.inputs = a.inputs as CardAnswer["inputs"];
  if (a.editedValues !== undefined) entry.edited = Object.keys(a.editedValues);
  const state: CardState = { ...l.state, answers: [...l.state.answers, entry].slice(-500) };
  const poll = pollOf(card);
  if (!poll) state.ask = { ...ask, status: "answered", answeredBy: actor.id, answeredAt: now };
  else {
    const voted = new Set(state.answers.filter((x) => x.askId === ask.id && x.retractedAt === undefined).map((x) => x.actorId));
    if (ask.from.every((id) => voted.has(id))) state.ask = { ...ask, status: "answered", answeredBy: actor.id, answeredAt: now };
  }
  retally(state, card);
  const ack: CardEventAck = { eventId, stored: true, answerId, expect: { by: row.author_id, mode: "none" } };
  const data: Record<string, unknown> = { messageId: row.id, rev: a.rev, answerId, askId: ask.id, eventId, event: a.event };
  if (a.actionId) data.actionId = a.actionId;
  if (a.blockId) data.blockId = a.blockId;
  if (a.value !== undefined) data.value = a.value;
  if (a.inputs !== undefined) data.inputs = a.inputs;
  if (a.editedValues !== undefined) data.editedValues = a.editedValues;
  room.commit((tx) => {
    tx.append({ type: "card.answered", actorId: actor.id, topicId: row.topic_id, messageId: row.id, data });
    writeCardUpdate(room, tx, row, l.ext, actor.id, l.dir, { card, state, cardChanged: false, now });
    insertEvent(room, { eventId, row, actor, a, stored: true, now, ack, cardRev: card.rev });
  });
  return finishAck(room, ack, {
    groupId: room.groupId,
    topicId: row.topic_id,
    messageId: row.id,
    authorId: row.author_id,
    eventId,
    rev: a.rev,
    event: a.event,
    blockId: a.blockId,
    actionId: a.actionId,
    value: a.value,
    inputs: a.inputs,
    part: a.part,
    actorId: actor.id,
    at: now,
    stored: true,
    answerId,
    askId: ask.id,
  });
}

async function liveEvent(room: RoomContext, actor: Actor, a: CardEventArgs, l: { row: MessageRow; card: CardV2; now: number }): Promise<CardEventAck> {
  const { row, now } = l;
  const eventId = ulid(now);
  const ack: CardEventAck = { eventId, stored: false, expect: { by: row.author_id, mode: "none" } };
  const transient: Record<string, unknown> = {
    type: "card.event",
    groupId: room.groupId,
    topicId: row.topic_id,
    messageId: row.id,
    eventId,
    rev: a.rev,
    event: a.event,
    actorId: actor.id,
    at: now,
  };
  if (a.blockId) transient.blockId = a.blockId;
  if (a.actionId) transient.actionId = a.actionId;
  if (a.value !== undefined) transient.value = a.value;
  if (a.inputs !== undefined) transient.inputs = a.inputs;
  if (a.part !== undefined) transient.part = a.part;
  insertEvent(room, { eventId, row, actor, a, stored: false, now, ack });
  room.broadcast({ t: "transient", event: transient }, { to: [row.author_id] });
  // Audit: who did what to which card; the value is cut to 1 KiB and redacted.
  const details: Record<string, unknown> = { groupId: room.groupId, eventId, event: a.event, rev: a.rev };
  if (a.blockId) details.blockId = a.blockId;
  if (a.actionId) details.actionId = a.actionId;
  const value = auditValue(a.value ?? a.inputs);
  if (value !== undefined) details.value = value;
  room.waitUntil(
    auditStatement(room.env.DB, { memberId: actor.id, tokenId: actor.tokenId ?? "" }, { action: "card.event", target: row.id, details }, now)
      .run()
      .catch((err) => room.log.error("card event audit failed", { err })),
  );
  return finishAck(room, ack, {
    groupId: room.groupId,
    topicId: row.topic_id,
    messageId: row.id,
    authorId: row.author_id,
    eventId,
    rev: a.rev,
    event: a.event,
    blockId: a.blockId,
    actionId: a.actionId,
    value: a.value,
    inputs: a.inputs,
    part: a.part,
    actorId: actor.id,
    at: now,
    stored: false,
  });
}

/** `answer.retract`: the answerer takes an answer back while the author has not acted on it. */
async function retract(room: RoomContext, actor: Actor, a: CardEventArgs, l: Loaded): Promise<CardEventAck> {
  const { row, card, now } = l;
  const v = a.value as { answerId?: unknown } | undefined;
  const answerId = typeof v?.answerId === "string" ? v.answerId : undefined;
  if (!answerId) throw new ApiError("invalid", "answer.retract takes {answerId}");
  const answer = l.state.answers.find((x) => x.answerId === answerId);
  if (!answer) throw new ApiError("not_found", "answer not found");
  if (answer.actorId !== actor.id) throw new ApiError("forbidden", "only the answerer can take an answer back");
  const authorActed =
    room.sql
      .exec("SELECT 1 FROM messages WHERE topic_id = ? AND author_id = ? AND created_at > ? AND deleted_at IS NULL LIMIT 1", row.topic_id, row.author_id, answer.at)
      .toArray().length > 0;
  const consumed = room.sql.exec("SELECT 1 FROM card_consumed WHERE answer_id = ?", answerId).toArray().length > 0;
  // The rev the card had when the answer was stored (an answer may name an older rev whose
  // decision surface did not change); older rows have none and use the answer's rev.
  const atRev =
    room.sql.exec<{ card_rev: number | null }>("SELECT card_rev FROM card_events WHERE event_id = ?", answer.eventId).toArray()[0]?.card_rev ?? answer.rev;
  if (answer.retractedAt !== undefined || card.rev > atRev || authorActed || consumed || now - answer.at > RETRACT_WINDOW_MS) {
    throw new ApiError("conflict", "the answer can no longer be taken back", "card.retract_closed");
  }
  rateAnswer(`${room.groupId}:${actor.id}:${a.messageId}:retract`, now);
  const eventId = ulid(now);
  const answers = l.state.answers.map((x) => (x.answerId === answerId ? { ...x, retractedAt: now } : x));
  const state: CardState = { ...l.state, answers };
  if (state.ask && state.ask.id === answer.askId && state.ask.status === "answered") {
    const { answeredBy: _b, answeredAt: _t, ...rest } = state.ask;
    state.ask = { ...rest, status: "open" };
  }
  retally(state, card);
  const ack: CardEventAck = { eventId, stored: true, answerId, expect: { by: row.author_id, mode: "none" } };
  room.commit((tx) => {
    tx.append({
      type: "card.retracted",
      actorId: actor.id,
      topicId: row.topic_id,
      messageId: row.id,
      data: { messageId: row.id, answerId, askId: answer.askId, eventId },
    });
    writeCardUpdate(room, tx, row, l.ext, actor.id, l.dir, { card, state, cardChanged: false, now });
    insertEvent(room, { eventId, row, actor, a, stored: true, now, ack });
  });
  return finishAck(room, ack, {
    groupId: room.groupId,
    topicId: row.topic_id,
    messageId: row.id,
    authorId: row.author_id,
    eventId,
    rev: a.rev,
    event: a.event,
    value: a.value,
    actorId: actor.id,
    at: now,
    stored: true,
    answerId,
    askId: answer.askId,
  });
}
