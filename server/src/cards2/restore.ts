// Restore from the archive (ops): rebuilds the cards2 room tables that the event log can give
// back, in seq order, after the core importer replayed the same event: the decision surface of
// every rev (card_revs), private cards (card_private), refs to parts (block_refs), ask and poll
// timers (card_timers) and the topic status base of open blocking asks (card_topic_base).
// Tables the log cannot give back (card_events, card_requesters, card_consumed) go in the
// daily room state snapshot (ops/state.ts). Group decisions are an event writer (./decisions).
import type { BlockRef, CardState, CardV2 } from "@chat/protocol";
import { type ArchivedEvent, registerEventImporter } from "../core/registry";
import { markPrivate } from "./privacy";
import { indexRefs } from "./refs";
import { baseOf, blockingWait, recordRev, saveBase } from "./state";

type Obj = Record<string, unknown>;

function importTimer(sql: SqlStorage, messageId: string, state: CardState): void {
  const at = state.ask?.status === "open" ? state.ask.expiresAt : undefined;
  if (at === undefined) sql.exec("DELETE FROM card_timers WHERE message_id = ?", messageId);
  else sql.exec("INSERT OR REPLACE INTO card_timers (message_id, at) VALUES (?, ?)", messageId, at);
}

/**
 * The base of a topic while a blocking ask is open: the status the topic had when the first
 * ask opened (the live path saves the same). A `topic.update` made while it waited is in the
 * log only as the combined status, so the restored base is the status before the wait.
 */
function importTopicBase(sql: SqlStorage, topicId: string | null | undefined): void {
  if (!topicId) return;
  const asks = blockingWait(sql, topicId);
  const base = baseOf(sql, topicId);
  if (asks && !base) {
    const t = sql.exec<{ status: string; waiting_on: string }>("SELECT status, waiting_on FROM topics WHERE id = ?", topicId).toArray()[0];
    if (t) saveBase(sql, topicId, { status: t.status as never, waitingOn: JSON.parse(t.waiting_on ?? "[]") as string[] });
  } else if (!asks && base) sql.exec("DELETE FROM card_topic_base WHERE topic_id = ?", topicId);
}

function topicOf(sql: SqlStorage, messageId: string): string | null {
  return sql.exec<{ topic_id: string }>("SELECT topic_id FROM messages WHERE id = ?", messageId).toArray()[0]?.topic_id ?? null;
}

export function importCardEvent(e: ArchivedEvent, sql: SqlStorage): void {
  const data = (e.data ?? {}) as Obj;
  switch (e.type) {
    case "message.created": {
      const m = data.message as Obj | undefined;
      if (!m || typeof m.id !== "string") return;
      const refs = m.blockRefs as BlockRef[] | undefined;
      if (Array.isArray(refs) && refs.length > 0) indexRefs(sql, m.id, refs);
      const card = m.cardV2 as CardV2 | undefined;
      if (!card) return;
      recordRev(sql, m.id, card);
      markPrivate(sql, m.id, card);
      if (m.cardState) importTimer(sql, m.id, m.cardState as CardState);
      importTopicBase(sql, m.topicId as string);
      return;
    }
    case "message.updated": {
      const id = data.messageId as string | undefined;
      if (!id) return;
      const card = data.cardV2 as CardV2 | undefined;
      if (card) {
        recordRev(sql, id, card);
        markPrivate(sql, id, card);
      }
      if (data.cardState) {
        importTimer(sql, id, data.cardState as CardState);
        importTopicBase(sql, e.topicId ?? topicOf(sql, id));
      }
      return;
    }
    case "message.deleted": {
      const id = (data.messageId as string | undefined) ?? e.messageId ?? undefined;
      if (!id) return;
      sql.exec("DELETE FROM block_refs WHERE message_id = ?", id);
      sql.exec("DELETE FROM card_timers WHERE message_id = ?", id);
      importTopicBase(sql, e.topicId ?? topicOf(sql, id));
      return;
    }
    default:
      return;
  }
}

registerEventImporter(importCardEvent);
