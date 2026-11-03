// Delivery of interactions to the member that made the card or the 产出 version (INTERACT §6):
// live card events and page events go to an SDK member as a transient on its "interact/1"
// connections, and to a runner member's run through BotDesk (`run.interaction`, coalesced, or
// a short follow-up run). `expectFor` is the `expect` of an ack (§6.4). The group room's
// `card.event` handler and the 产出 bridge call these; they never store anything.
import type { Expect, PartRef } from "@chat/protocol";
import { type Env, gatewayStub } from "../core/env";
import { featureTransient } from "../core/gateway";
import type { RoomContext } from "../core/registry";
import { type CardEventDelivery, registerCardEventHandler, registerRequesterResolver } from "../cards2/delivery";
import { answerBlocks } from "./answers";
import { deskStub } from "./env";
import { memberInfo } from "./room";

/** The `hello.features` entry of clients and SDKs that take the 1.1 interaction transients. */
export const INTERACT_FEATURE = "interact/1";

/** `expect.etaMs` per responder (INTERACT §6.4). */
export const ETA_MS = { live: 2000, runIdle: 10_000, runInTurn: 20_000, starting: 40_000 } as const;

/** What BotDesk gets for a live card event or a page event. */
export interface InteractionIn {
  groupId: string;
  topicId: string;
  eventId: string;
  source:
    | { kind: "card"; messageId: string; blockId?: string; actionId?: string; rev: number }
    | { kind: "artifact"; artifactId: string; version: number; eventId: string; ask: boolean };
  /** Card event name, or the page event name. */
  event: string;
  value?: unknown;
  actorId: string;
  /** Card: the block (or footer action) at that rev, for the envelope. */
  block?: unknown;
  /** Card title or 产出 title, for the text form. */
  title?: string;
  /** 产出: the run that made the version. */
  runId?: string | null;
}

/** A live card event, as the group room's `card.event` handler hands it over. */
export interface LiveCardEvent {
  eventId: string;
  messageId: string;
  topicId: string;
  rev: number;
  blockId?: string;
  actionId?: string;
  event: string;
  value?: unknown;
  inputs?: Record<string, unknown>;
  part?: PartRef;
  actorId: string;
  at: number;
  /** The block (or footer action) the event names, at the card's current rev. */
  block?: unknown;
  /** The card's title. */
  title?: string;
}

/** A page event, as the 产出 bridge hands it over. */
export interface LivePageEvent {
  eventId: string;
  topicId: string;
  artifactId: string;
  version: number;
  name: string;
  kind: "emit" | "ask";
  payload?: unknown;
  actorId: string;
  at: number;
  title?: string;
  runId?: string | null;
}

type Responder = { kind: "none" } | { kind: "runner" } | { kind: "live" };

async function responder(env: Env, authorId: string): Promise<Responder> {
  const m = (await memberInfo(env.DB, [authorId])).get(authorId);
  if (!m || m.deactivated) return { kind: "none" };
  if (m.kind === "bot" && m.runner) return { kind: "runner" };
  return { kind: "live" };
}

async function liveExpect(env: Env, authorId: string): Promise<Expect> {
  let online = false;
  try {
    online = await gatewayStub(env, authorId).onlineWith(INTERACT_FEATURE);
  } catch {
    online = false;
  }
  return online ? { by: authorId, mode: "live", etaMs: ETA_MS.live } : { by: authorId, mode: "offline" };
}

/**
 * The `expect` of an ack for an event to `authorId` in the topic, without delivering anything:
 * for stored answers (they reach the member through the group log and the bots outbox).
 */
export async function expectFor(env: Env, authorId: string, topicId: string): Promise<Expect> {
  const r = await responder(env, authorId);
  if (r.kind === "none") return { by: authorId, mode: "none" };
  if (r.kind === "live") return liveExpect(env, authorId);
  try {
    return { by: authorId, ...(await deskStub(env, authorId).expectFor(authorId, topicId)) };
  } catch {
    return { by: authorId, mode: "none" };
  }
}

async function toRunner(env: Env, authorId: string, x: InteractionIn): Promise<Expect> {
  try {
    return { by: authorId, ...(await deskStub(env, authorId).interaction(authorId, x)) };
  } catch {
    return { by: authorId, mode: "none" };
  }
}

/**
 * A live card event for the card's author: the transient `card.event` to an SDK member's
 * "interact/1" connections, or BotDesk for a runner member. Returns the ack's `expect`.
 */
export async function deliverCardEvent(room: RoomContext, authorId: string, e: LiveCardEvent): Promise<Expect> {
  const r = await responder(room.env, authorId);
  if (r.kind === "none") return { by: authorId, mode: "none" };
  if (r.kind === "runner") {
    const source: InteractionIn["source"] = { kind: "card", messageId: e.messageId, rev: e.rev };
    if (e.blockId) source.blockId = e.blockId;
    if (e.actionId) source.actionId = e.actionId;
    const x: InteractionIn = { groupId: room.groupId, topicId: e.topicId, eventId: e.eventId, source, event: e.event, actorId: e.actorId };
    const value = e.inputs !== undefined ? { ...(e.value !== undefined ? { value: e.value } : {}), inputs: e.inputs } : e.value;
    if (value !== undefined) x.value = value;
    if (e.block !== undefined) x.block = e.block;
    if (e.title) x.title = e.title;
    return toRunner(room.env, authorId, x);
  }
  const event: Record<string, unknown> = {
    type: "card.event",
    groupId: room.groupId,
    topicId: e.topicId,
    messageId: e.messageId,
    eventId: e.eventId,
    rev: e.rev,
    event: e.event,
    actorId: e.actorId,
    at: e.at,
  };
  for (const k of ["blockId", "actionId", "value", "inputs", "part"] as const) if (e[k] !== undefined) event[k] = e[k];
  room.broadcast(featureTransient(INTERACT_FEATURE, event), { to: [authorId] });
  return liveExpect(room.env, authorId);
}

/** A page event for the version's creator (transient `artifact.event`, or BotDesk). */
export async function deliverPageEvent(room: RoomContext, creatorId: string, e: LivePageEvent): Promise<Expect> {
  const r = await responder(room.env, creatorId);
  if (r.kind === "none") return { by: creatorId, mode: "none" };
  if (r.kind === "runner") {
    const x: InteractionIn = {
      groupId: room.groupId,
      topicId: e.topicId,
      eventId: e.eventId,
      source: { kind: "artifact", artifactId: e.artifactId, version: e.version, eventId: e.eventId, ask: e.kind === "ask" },
      event: e.name,
      actorId: e.actorId,
      runId: e.runId ?? null,
    };
    if (e.payload !== undefined) x.value = e.payload;
    if (e.title) x.title = e.title;
    return toRunner(room.env, creatorId, x);
  }
  const event: Record<string, unknown> = {
    type: "artifact.event",
    groupId: room.groupId,
    topicId: e.topicId,
    artifactId: e.artifactId,
    version: e.version,
    eventId: e.eventId,
    name: e.name,
    kind: e.kind,
    actorId: e.actorId,
    at: e.at,
  };
  if (e.payload !== undefined) event.payload = e.payload;
  room.broadcast(featureTransient(INTERACT_FEATURE, event), { to: [creatorId] });
  return liveExpect(room.env, creatorId);
}

type CardRow = { ext: string | null };

/** The block (or footer action) a card event names, and the card's title, from the room's copy. */
function cardContext(room: RoomContext, e: CardEventDelivery): { block?: unknown; title?: string } {
  const row = room.sql.exec<CardRow>("SELECT ext FROM messages WHERE id = ?", e.messageId).toArray()[0];
  const card = row?.ext ? (JSON.parse(row.ext) as { cardV2?: { title?: string; blocks: unknown[]; actions?: { id: string }[] } }).cardV2 : undefined;
  if (!card) return {};
  const out: { block?: unknown; title?: string } = {};
  if (card.title) out.title = card.title;
  if (e.blockId) out.block = answerBlocks(card as never, { blockId: e.blockId })[e.blockId];
  else if (e.actionId) out.block = card.actions?.find((a) => a.id === e.actionId);
  return out;
}

/**
 * The cards feature's hand-off (INTERACT §6.2): a live card event for a runner member goes to
 * its run through BotDesk (the transient to SDK connections is the cards feature's). A stored
 * answer reaches the run through the bots outbox; here it only gets the runner's `expect`.
 * SDK members and people are left to the default (connection presence).
 */
registerCardEventHandler(async (room, e) => {
  const r = await responder(room.env, e.authorId);
  if (r.kind === "none") return { by: e.authorId, mode: "none" };
  if (r.kind !== "runner") return undefined;
  if (e.stored) return expectFor(room.env, e.authorId, e.topicId);
  const ctx = cardContext(room, e);
  const live: LiveCardEvent = {
    eventId: e.eventId,
    messageId: e.messageId,
    topicId: e.topicId,
    rev: e.rev,
    event: e.event,
    actorId: e.actorId,
    at: e.at,
  };
  for (const k of ["blockId", "actionId", "value", "inputs", "part"] as const) if (e[k] !== undefined) (live as unknown as Record<string, unknown>)[k] = e[k];
  if (ctx.block !== undefined) live.block = ctx.block;
  if (ctx.title) live.title = ctx.title;
  return deliverCardEvent(room, e.authorId, live);
});

/** "requester" of a runner member's card: the person who asked its active run in the topic. */
registerRequesterResolver(async (room, authorId, topicId) => {
  const r = await responder(room.env, authorId);
  if (r.kind !== "runner") return undefined;
  try {
    return (await deskStub(room.env, authorId).requesterFor(authorId, topicId)) ?? undefined;
  } catch (err) {
    room.log.warn("requester lookup failed", { err });
    return undefined;
  }
});
