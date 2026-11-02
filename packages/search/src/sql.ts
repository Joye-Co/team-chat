// SQL for a messages search index in SQLite (Durable Object storage, D1 or bun:sqlite).
//
// Two tables:
// - search_docs: one row per indexed message (message id, topic id, sort key). Its integer
//   primary key is the rowid of the FTS row, so edits and deletes by message id use an index.
// - search_fts: the FTS5 table with one column, `tokens`, from tokenizeForIndex().
//
// Only `?` placeholders are used, so the statements run on every SQLite binding.

import { buildMatchQuery, type MatchQueryOptions, tokenizeForIndex } from "./tokenize.ts"

export type SqlValue = string | number | null
export interface Statement {
  sql: string
  params: SqlValue[]
}

/**
 * Schema statements. Run them in order, once, as the named Durable Object migration
 * `search-1` (see SEARCH_MIGRATIONS). All are idempotent.
 */
export const SEARCH_SCHEMA: readonly string[] = [
  `CREATE TABLE IF NOT EXISTS search_docs (
    doc_id INTEGER PRIMARY KEY,
    message_id TEXT NOT NULL UNIQUE,
    topic_id TEXT NOT NULL,
    sort_key INTEGER NOT NULL
  )`,
  "CREATE INDEX IF NOT EXISTS search_docs_topic ON search_docs (topic_id, sort_key)",
  `CREATE VIRTUAL TABLE IF NOT EXISTS search_fts USING fts5 (
    tokens,
    tokenize = "unicode61 remove_diacritics 2 tokenchars '_'"
  )`,
]

/** Named migrations, for the Durable Object `migrations` table. */
export const SEARCH_MIGRATIONS: readonly { name: string; statements: readonly string[] }[] = [
  { name: "search-1", statements: SEARCH_SCHEMA },
]

export interface IndexInput {
  messageId: string
  topicId: string
  /** Sort key for "newest first": the message seq (preferred) or createdAt in ms. */
  sortKey: number
  /** The message body as the user wrote it (Markdown). */
  text: string
}

/** Statements that remove a message from the index. Safe when it is not indexed. */
export function deleteStatements(messageId: string): Statement[] {
  return [
    {
      sql: "DELETE FROM search_fts WHERE rowid = (SELECT doc_id FROM search_docs WHERE message_id = ?)",
      params: [messageId],
    },
    { sql: "DELETE FROM search_docs WHERE message_id = ?", params: [messageId] },
  ]
}

/**
 * Statements that index a new message or replace the index of an edited one. Use the same
 * call for insert and edit. When the text has nothing searchable, it removes the message.
 * Run them in one transaction (in a Durable Object, inside `transactionSync`, or in the same
 * synchronous block as the message write).
 */
export function upsertStatements(input: IndexInput): Statement[] {
  const tokens = tokenizeForIndex(input.text)
  if (!tokens) return deleteStatements(input.messageId)
  return [
    {
      sql: `INSERT INTO search_docs (message_id, topic_id, sort_key) VALUES (?, ?, ?)
        ON CONFLICT (message_id) DO UPDATE SET topic_id = excluded.topic_id, sort_key = excluded.sort_key`,
      params: [input.messageId, input.topicId, input.sortKey],
    },
    {
      sql: "DELETE FROM search_fts WHERE rowid = (SELECT doc_id FROM search_docs WHERE message_id = ?)",
      params: [input.messageId],
    },
    {
      sql: "INSERT INTO search_fts (rowid, tokens) VALUES ((SELECT doc_id FROM search_docs WHERE message_id = ?), ?)",
      params: [input.messageId, tokens],
    },
  ]
}

/** Statements that remove every message of a topic from the index. */
export function deleteTopicStatements(topicId: string): Statement[] {
  return [
    {
      sql: "DELETE FROM search_fts WHERE rowid IN (SELECT doc_id FROM search_docs WHERE topic_id = ?)",
      params: [topicId],
    },
    { sql: "DELETE FROM search_docs WHERE topic_id = ?", params: [topicId] },
  ]
}

export interface SearchInput extends MatchQueryOptions {
  query: string
  /** Only this topic. */
  topicId?: string
  /** "recent" (default): newest first. "relevance": FTS5 bm25 rank, then newest. */
  order?: "recent" | "relevance"
  /** Only messages with sort_key below this value (the next page of "recent"). */
  beforeSortKey?: number
  /** Default 50, at most 200. */
  limit?: number
}

export interface SearchRow {
  messageId: string
  topicId: string
  sortKey: number
}

/**
 * The search statement, or null when the query has nothing searchable (return no results).
 * Rows have columns messageId, topicId, sortKey.
 */
export function searchStatement(input: SearchInput): Statement | null {
  const match = buildMatchQuery(input.query, input)
  if (match === null) return null
  const limit = Math.max(1, Math.min(200, Math.floor(input.limit ?? 50)))
  const where = ["search_fts MATCH ?"]
  const params: SqlValue[] = [match]
  if (input.topicId !== undefined) {
    where.push("d.topic_id = ?")
    params.push(input.topicId)
  }
  if (input.beforeSortKey !== undefined) {
    where.push("d.sort_key < ?")
    params.push(input.beforeSortKey)
  }
  const order = input.order === "relevance" ? "search_fts.rank, d.sort_key DESC" : "d.sort_key DESC"
  params.push(limit)
  return {
    sql: `SELECT d.message_id AS messageId, d.topic_id AS topicId, d.sort_key AS sortKey
      FROM search_fts JOIN search_docs d ON d.doc_id = search_fts.rowid
      WHERE ${where.join(" AND ")}
      ORDER BY ${order}
      LIMIT ?`,
    params,
  }
}

/**
 * Runs one statement and returns its rows. Adapters:
 * - Durable Object: (sql, params) => ctx.storage.sql.exec(sql, ...params).toArray()
 * - bun:sqlite:     (sql, params) => db.query(sql).all(...params)
 */
export type Exec = (sql: string, params: SqlValue[]) => Record<string, unknown>[]

/** A small wrapper over the statements above for synchronous SQLite (Durable Objects). */
export class MessageSearchIndex {
  constructor(private readonly exec: Exec) {}

  /** Creates the tables. Prefer running SEARCH_MIGRATIONS through your migration runner. */
  migrate(): void {
    for (const sql of SEARCH_SCHEMA) this.exec(sql, [])
  }

  /** Index a new message or re-index an edited one. */
  upsert(input: IndexInput): void {
    for (const s of upsertStatements(input)) this.exec(s.sql, s.params)
  }

  delete(messageId: string): void {
    for (const s of deleteStatements(messageId)) this.exec(s.sql, s.params)
  }

  deleteTopic(topicId: string): void {
    for (const s of deleteTopicStatements(topicId)) this.exec(s.sql, s.params)
  }

  search(input: SearchInput): SearchRow[] {
    const s = searchStatement(input)
    if (!s) return []
    return this.exec(s.sql, s.params).map((r) => ({
      messageId: String(r.messageId),
      topicId: String(r.topicId),
      sortKey: Number(r.sortKey),
    }))
  }
}
