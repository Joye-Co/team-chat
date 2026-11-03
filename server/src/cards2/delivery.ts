// Extension points for the delivery feature (BotDesk: runner members, follow-up runs, the
// requester of an active run). The cards feature calls them; without a registration it
// answers for SDK members and people from their connection presence.
import type { Expect, PartRef } from "@chat/protocol";
import { gatewayStub } from "../core/env";
import type { RoomContext } from "../core/registry";

/** A card event as the author gets it (live: transient `card.event`; stored: `card.answered`). */
export interface CardEventDelivery {
  groupId: string;
  topicId: string;
  messageId: string;
  /** The card's author: who responds. */
  authorId: string;
  eventId: string;
  rev: number;
  event: string;
  blockId?: string;
  actionId?: string;
  value?: unknown;
  inputs?: Record<string, unknown>;
  part?: PartRef;
  actorId: string;
  at: number;
  /** True for an answer (stored as `card.answered`, delivered by its after-commit hook). */
  stored: boolean;
  answerId?: string;
  askId?: string;
}

/**
 * Called for every accepted card event, after the room wrote it. Returns the `expect` of the
 * ack, or undefined to let the next handler (finally the default: presence) decide. For a live
 * event the transient to the author's connections is already sent; a runner delivery adds the
 * run input here.
 */
export type CardEventHandler = (room: RoomContext, e: CardEventDelivery) => Promise<Expect | undefined> | Expect | undefined;
export const cardEventHandlers: CardEventHandler[] = [];
export function registerCardEventHandler(h: CardEventHandler): void {
  cardEventHandlers.push(h);
}

/** The requester of a member's active run in a topic (`emitBy`/`ask.from` "requester"). */
export type RequesterResolver = (room: RoomContext, authorId: string, topicId: string) => Promise<string | undefined> | string | undefined;
export const requesterResolvers: RequesterResolver[] = [];
export function registerRequesterResolver(r: RequesterResolver): void {
  requesterResolvers.push(r);
}

/**
 * The author started acting on an answer (`input.accepted` of its `run.answer`): the answerer
 * can no longer take it back (INTERACT §3.6).
 */
export function closeRetractWindow(sql: SqlStorage, answerId: string, at = Date.now()): void {
  sql.exec("INSERT OR IGNORE INTO card_consumed (answer_id, at) VALUES (?, ?)", answerId, at);
}

/** Default `expect`: an SDK member (or person) connected with `interact/1` is `live`, else `offline`. */
export async function defaultExpect(room: RoomContext, authorId: string): Promise<Expect> {
  let online = false;
  try {
    online = await gatewayStub(room.env, authorId).featureOnline("interact/1");
  } catch (err) {
    room.log.warn("presence check failed", { err });
  }
  return online ? { by: authorId, mode: "live", etaMs: 2000 } : { by: authorId, mode: "offline" };
}

export async function expectFor(room: RoomContext, e: CardEventDelivery): Promise<Expect> {
  for (const h of cardEventHandlers) {
    try {
      const x = await h(room, e);
      if (x) return x;
    } catch (err) {
      room.log.error("card event handler failed", { err });
    }
  }
  return defaultExpect(room, e.authorId);
}
