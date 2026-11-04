// Page events as causes (INTERACT §4.1-§4.3, review F2). A member names a page event of its own
// 产出 version like a card event: `interaction.resolve`, `send.causeEventId`, `card.patch.cause`
// and `artifact.data.cause`. The group room keeps page events in `page_events` (files/bridge.ts)
// and card events in `card_events` (cards2); cards2 calls these when the event id is not a
// card event. No imports beyond types, so cards2 can use them without a cycle.
import type { Cause } from "@chat/protocol";

/** A cause is an event of the last 30 minutes. */
export const PAGE_CAUSE_WINDOW_MS = 30 * 60_000;
const EVENT_NAME_RE = /^[a-z][A-Za-z0-9_.-]{0,63}$/;

export type PageEventCauseRow = { event_id: string; actor_id: string; bot_id: string; name: string | null; at: number; resolved: string | null; replied_at: number | null };

export function pageEventRow(sql: SqlStorage, eventId: string): PageEventCauseRow | undefined {
  return sql
    .exec<PageEventCauseRow>("SELECT event_id, actor_id, bot_id, name, at, resolved, replied_at FROM page_events WHERE event_id = ?", eventId)
    .toArray()[0] as PageEventCauseRow | undefined;
}

export function pageCauseOf(r: PageEventCauseRow): Cause {
  return { eventId: r.event_id, actorId: r.actor_id, at: r.at, event: r.name && EVENT_NAME_RE.test(r.name) ? r.name : "artifact.event" };
}

/** The cause `receiverId` names: a page event it received in the last 30 minutes, or undefined. */
export function pageCause(sql: SqlStorage, eventId: string, receiverId: string, now = Date.now()): Cause | undefined {
  const r = pageEventRow(sql, eventId);
  if (!r || r.bot_id !== receiverId || r.at < now - PAGE_CAUSE_WINDOW_MS) return undefined;
  return pageCauseOf(r);
}

/** Marks a page event resolved; true when it was not resolved before. */
export function markPageResolved(sql: SqlStorage, eventId: string, result: string): boolean {
  return sql.exec("UPDATE page_events SET resolved = ? WHERE event_id = ? AND resolved IS NULL", result, eventId).rowsWritten > 0;
}
