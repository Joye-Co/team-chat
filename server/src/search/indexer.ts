// Room side of search: the FTS5 index in each GroupRoom's SQLite (packages/search tokens), kept
// in step with the event log inside the commit transaction, and a batched backfill for rooms that
// had messages before the `search-1` migration.
//
// Docs: one per message (key = message id) and one per topic title (key = "topic:" + topic id).
// sort_key = the message's createdSeq; a title uses the topic.created seq (root message seq - 1).
import { type Exec, MessageSearchIndex, SEARCH_SCHEMA, TOKENIZER_VERSION } from "@chat/search";
import { logger } from "../core/logger";
import type { ArchivedEvent, RoomContext } from "../core/registry";

export const TITLE_PREFIX = "topic:";
/** Rows per backfill transaction; keeps each transaction short (about 10 ms of CPU). */
export const BACKFILL_BATCH = 500;

const log = logger.child({ obj: "search" });

export function searchIndex(sql: SqlStorage): MessageSearchIndex {
  const exec: Exec = (q, params) => sql.exec(q, ...params).toArray() as Record<string, unknown>[];
  return new MessageSearchIndex(exec);
}

/** Migration `search-1`: the index tables and the backfill cursor for older messages. */
export function migrateSearch1(sql: SqlStorage): void {
  for (const s of SEARCH_SCHEMA) sql.exec(s);
  sql.exec(`CREATE TABLE search_backfill (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    until_seq INTEGER NOT NULL,
    msg_cursor INTEGER NOT NULL,
    topic_cursor TEXT NOT NULL,
    topics_done INTEGER NOT NULL,
    tokenizer INTEGER NOT NULL
  )`);
  const until = sql.exec<{ s: number | null }>("SELECT max(created_seq) AS s FROM messages").one().s ?? 0;
  const topics = sql.exec<{ n: number }>("SELECT count(*) AS n FROM topics").one().n;
  sql.exec(
    "INSERT INTO search_backfill (id, until_seq, msg_cursor, topic_cursor, topics_done, tokenizer) VALUES (1, ?, 0, '', ?, ?)",
    until,
    topics === 0 ? 1 : 0,
    TOKENIZER_VERSION,
  );
}

type Data = Record<string, unknown>;

function upsertTitle(sql: SqlStorage, topicId: string, title: string, sortKey: number): void {
  searchIndex(sql).upsert({ messageId: TITLE_PREFIX + topicId, topicId, sortKey, text: title });
}

/** Applies one stored event to the index. Runs inside the commit (or import) transaction. */
export function indexEvent(e: ArchivedEvent, sql: SqlStorage): void {
  try {
    const data = (e.data ?? {}) as Data;
    switch (e.type) {
      case "message.created": {
        const m = data.message as Data | undefined;
        if (!m || typeof m.id !== "string" || typeof m.topicId !== "string") return;
        if (m.kind === "system") return;
        searchIndex(sql).upsert({
          messageId: m.id,
          topicId: m.topicId,
          sortKey: Number(m.createdSeq ?? e.seq),
          text: typeof m.body === "string" ? m.body : "",
        });
        return;
      }
      case "message.updated": {
        const id = typeof data.messageId === "string" ? data.messageId : e.messageId;
        if (!id || typeof data.body !== "string") return;
        const row = sql
          .exec<{ topic_id: string; created_seq: number; kind: string; deleted_at: number | null }>(
            "SELECT topic_id, created_seq, kind, deleted_at FROM messages WHERE id = ?",
            id,
          )
          .toArray()[0];
        if (!row || row.kind === "system" || row.deleted_at !== null) return;
        searchIndex(sql).upsert({ messageId: id, topicId: row.topic_id, sortKey: row.created_seq, text: data.body });
        return;
      }
      case "message.deleted": {
        const id = typeof data.messageId === "string" ? data.messageId : e.messageId;
        if (id) searchIndex(sql).delete(id);
        return;
      }
      case "topic.deleted": {
        // Its messages left the index as they were scrubbed; the title goes too.
        const topicId = typeof data.topicId === "string" ? data.topicId : e.topicId;
        if (topicId) searchIndex(sql).delete(TITLE_PREFIX + topicId);
        return;
      }
      case "topic.created": {
        const t = data.topic as Data | undefined;
        if (!t || typeof t.id !== "string" || typeof t.title !== "string") return;
        upsertTitle(sql, t.id, t.title, e.seq);
        return;
      }
      case "topic.updated": {
        const topicId = typeof data.topicId === "string" ? data.topicId : e.topicId;
        if (!topicId || typeof data.title !== "string") return;
        const prev = sql
          .exec<{ sort_key: number }>("SELECT sort_key FROM search_docs WHERE message_id = ?", TITLE_PREFIX + topicId)
          .toArray()[0];
        upsertTitle(sql, topicId, data.title, prev?.sort_key ?? rootSortKey(sql, topicId) ?? e.seq);
        return;
      }
    }
  } catch (err) {
    // The index must never block a chat write. A missed entry is fixed by the next edit.
    log.error("search index update failed", { err, type: e.type, seq: e.seq });
  }
}

function rootSortKey(sql: SqlStorage, topicId: string): number | undefined {
  const r = sql
    .exec<{ s: number }>(
      "SELECT m.created_seq - 1 AS s FROM topics t JOIN messages m ON m.id = t.root_message_id WHERE t.id = ?",
      topicId,
    )
    .toArray()[0];
  return r?.s;
}

type BackfillRow = { until_seq: number; msg_cursor: number; topic_cursor: string; topics_done: number };

function backfillState(sql: SqlStorage): BackfillRow | undefined {
  return sql
    .exec<BackfillRow>("SELECT until_seq, msg_cursor, topic_cursor, topics_done FROM search_backfill WHERE id = 1")
    .toArray()[0];
}

export function backfillPending(sql: SqlStorage): boolean {
  const s = backfillState(sql);
  return !!s && (s.msg_cursor < s.until_seq || s.topics_done === 0);
}

/**
 * One backfill batch in its own transaction. Reads the current rows, so edits and deletes made
 * between batches are respected. Returns true while more work is left.
 */
export function backfillStep(room: RoomContext, batch = BACKFILL_BATCH): boolean {
  return room.commit(() => {
    const sql = room.sql;
    const s = backfillState(sql);
    if (!s) return false;
    const idx = searchIndex(sql);
    if (s.topics_done === 0) {
      const topics = sql
        .exec<{ id: string; title: string; root_message_id: string }>(
          "SELECT id, title, root_message_id FROM topics WHERE id > ? ORDER BY id LIMIT ?",
          s.topic_cursor,
          batch,
        )
        .toArray();
      for (const t of topics) {
        upsertTitle(sql, t.id, t.title, rootSortKey(sql, t.id) ?? 0);
      }
      const last = topics[topics.length - 1];
      sql.exec(
        "UPDATE search_backfill SET topic_cursor = ?, topics_done = ? WHERE id = 1",
        last ? last.id : s.topic_cursor,
        topics.length < batch ? 1 : 0,
      );
      return true;
    }
    if (s.msg_cursor >= s.until_seq) return false;
    const rows = sql
      .exec<{ id: string; topic_id: string; body: string; created_seq: number; kind: string; deleted_at: number | null }>(
        `SELECT id, topic_id, body, created_seq, kind, deleted_at FROM messages
         WHERE created_seq > ? AND created_seq <= ? ORDER BY created_seq LIMIT ?`,
        s.msg_cursor,
        s.until_seq,
        batch,
      )
      .toArray();
    for (const r of rows) {
      if (r.deleted_at !== null || r.kind === "system") idx.delete(r.id);
      else idx.upsert({ messageId: r.id, topicId: r.topic_id, sortKey: r.created_seq, text: r.body });
    }
    const cursor = rows.length < batch ? s.until_seq : rows[rows.length - 1].created_seq;
    sql.exec("UPDATE search_backfill SET msg_cursor = ? WHERE id = 1", cursor);
    return cursor < s.until_seq;
  });
}

const running = new Map<string, Promise<void>>();

/**
 * Runs the backfill to the end, yielding between batches so the room keeps serving writes.
 * Concurrent callers share one run.
 */
export function ensureBackfilled(room: RoomContext): Promise<void> {
  if (!backfillPending(room.sql)) return Promise.resolve();
  let p = running.get(room.groupId);
  if (!p) {
    p = (async () => {
      const start = Date.now();
      try {
        while (backfillStep(room)) await new Promise((r) => setTimeout(r, 0));
        log.info("search backfill done", { groupId: room.groupId, ms: Date.now() - start });
      } finally {
        running.delete(room.groupId);
      }
    })();
    running.set(room.groupId, p);
  }
  return p;
}
