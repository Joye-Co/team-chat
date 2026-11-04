// Room tables of the interaction loop (docs/protocol.md, "Interaction loop"): events for
// idempotency, causes and resolves; patch ids; the decision surface of every rev; ask and poll
// timers; answers a run took (retract window); refs to parts (for deletes).
import type { CardState, CardV2, Cause } from "@chat/protocol";
import { registerRoomMigration } from "../core/registry";

registerRoomMigration({
  name: "cards2-2",
  up: `
    CREATE TABLE card_events (
      event_id TEXT PRIMARY KEY,
      message_id TEXT NOT NULL,
      author_id TEXT NOT NULL,
      actor_id TEXT NOT NULL,
      client_event_id TEXT,
      event TEXT NOT NULL,
      block_id TEXT,
      action_id TEXT,
      stored INTEGER NOT NULL,
      at INTEGER NOT NULL,
      ack TEXT NOT NULL,
      resolved TEXT
    );
    CREATE UNIQUE INDEX card_events_client ON card_events(actor_id, message_id, client_event_id)
      WHERE client_event_id IS NOT NULL;
    CREATE INDEX card_events_at ON card_events(at);
    CREATE TABLE card_patches (
      actor_id TEXT NOT NULL,
      message_id TEXT NOT NULL,
      client_patch_id TEXT NOT NULL,
      at INTEGER NOT NULL,
      ack TEXT NOT NULL,
      PRIMARY KEY (actor_id, message_id, client_patch_id)
    );
    CREATE TABLE card_revs (
      message_id TEXT NOT NULL,
      rev INTEGER NOT NULL,
      surface TEXT NOT NULL,
      PRIMARY KEY (message_id, rev)
    );
    CREATE TABLE card_timers (message_id TEXT PRIMARY KEY, at INTEGER NOT NULL);
    CREATE INDEX card_timers_at ON card_timers(at);
    CREATE TABLE card_consumed (answer_id TEXT PRIMARY KEY, at INTEGER NOT NULL);
    CREATE TABLE block_refs (
      message_id TEXT NOT NULL,
      source_message_id TEXT,
      artifact_id TEXT
    );
    CREATE INDEX block_refs_source ON block_refs(source_message_id) WHERE source_message_id IS NOT NULL;
    CREATE INDEX block_refs_artifact ON block_refs(artifact_id) WHERE artifact_id IS NOT NULL;
  `,
});

/** The card's rev when an answer was stored (the retract window closes on a newer rev). */
registerRoomMigration({ name: "cards2-3", up: "ALTER TABLE card_events ADD COLUMN card_rev INTEGER;" });

/** `ask.from` "requester" as resolved for an ask round (INTERACT §3.5: resolved once, kept). */
registerRoomMigration({
  name: "cards2-4",
  up: "CREATE TABLE card_requesters (message_id TEXT NOT NULL, ask_id TEXT NOT NULL, member_id TEXT, PRIMARY KEY (message_id, ask_id));",
});

/** Idempotency keys and causes live this long. */
export const EVENT_KEEP_MS = 24 * 3600_000;
/** A cause must be an event of the last 30 minutes. */
export const CAUSE_MAX_AGE_MS = 30 * 60_000;

export type MessageRow = {
  id: string;
  topic_id: string;
  author_id: string;
  body: string;
  kind: string | null;
  ext: string | null;
  deleted_at: number | null;
  created_at: number;
  edited_at: number | null;
  created_seq: number;
};

export type MessageExt = Record<string, unknown> & { cardV2?: CardV2; cardState?: CardState; cause?: Cause };

export function messageRow(sql: SqlStorage, id: string): MessageRow | undefined {
  return sql.exec<MessageRow>("SELECT * FROM messages WHERE id = ?", id).toArray()[0] as MessageRow | undefined;
}

export function extOf(row: MessageRow): MessageExt {
  return row.ext ? (JSON.parse(row.ext) as MessageExt) : {};
}

export type EventRow = {
  event_id: string;
  message_id: string;
  author_id: string;
  actor_id: string;
  client_event_id: string | null;
  event: string;
  block_id: string | null;
  action_id: string | null;
  stored: number;
  at: number;
  ack: string;
  resolved: string | null;
};

export function eventRow(sql: SqlStorage, eventId: string): EventRow | undefined {
  return sql.exec<EventRow>("SELECT * FROM card_events WHERE event_id = ?", eventId).toArray()[0] as EventRow | undefined;
}

export function causeOf(r: EventRow): Cause {
  const c: Cause = { eventId: r.event_id, actorId: r.actor_id, at: r.at, event: r.event };
  if (r.block_id) c.blockId = r.block_id;
  if (r.action_id) c.actionId = r.action_id;
  return c;
}

/** Drops idempotency rows older than a day, now and then. */
export function pruneEvents(sql: SqlStorage, now: number): void {
  if (Math.random() >= 0.02) return;
  sql.exec("DELETE FROM card_events WHERE at < ?", now - EVENT_KEEP_MS);
  sql.exec("DELETE FROM card_patches WHERE at < ?", now - EVENT_KEEP_MS);
}
