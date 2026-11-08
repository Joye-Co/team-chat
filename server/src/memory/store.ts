// D1 `memories`, `memory_versions` and `memory_updates` (migration 0012): rows, versions and
// the daily 记忆更新 lines. Every change makes a new version; a version clash is `conflict`.
import type { MemoryItem, MemoryKind, MemorySource, MemoryStatus, MemoryUpdate } from "@chat/protocol";
import { MAX_MEMORY_CHARS, MAX_MEMORY_NOTE_CHARS, MAX_MEMORY_WHEN_CHARS } from "@chat/protocol";
import { redactText } from "../core/auditvalue";
import { ApiError } from "../core/errors";
import { ulid } from "../core/ids";

export type MemoryRow = {
  id: string;
  scope: string;
  kind: MemoryKind;
  text: string;
  when_text: string | null;
  source: string;
  note: string | null;
  author_id: string;
  human: number;
  status: MemoryStatus;
  confidence: number;
  uses: number;
  last_used_at: number | null;
  version: number;
  created_at: number;
  updated_at: number;
};

type UpdateRow = {
  id: string;
  day: string;
  scope: string;
  memory_id: string;
  change: MemoryUpdate["change"];
  before_json: string | null;
  after_json: string | null;
  note: string | null;
  undone_at: number | null;
  at: number;
};

/** Cuts text to at most `max` code points. */
function cutTo(text: string, max: number): string {
  const cps = [...text];
  return cps.length <= max ? text : `${cps.slice(0, Math.max(0, max - 1)).join("")}…`;
}

/** Text as stored: secrets masked (the daemon's token formats), trimmed, cut. Never stores credentials. */
export function clean(text: string, max: number): string {
  return cutTo(redactText(text).trim(), max);
}
export const cleanText = (t: string) => clean(t, MAX_MEMORY_CHARS);
export const cleanWhen = (t: string | null | undefined) => (t ? clean(t, MAX_MEMORY_WHEN_CHARS) || null : null);
export const cleanNote = (t: string | null | undefined) => (t ? clean(t, MAX_MEMORY_NOTE_CHARS) || null : null);

export function toItem(r: MemoryRow): MemoryItem {
  const item: MemoryItem = {
    id: r.id,
    scope: r.scope,
    kind: r.kind,
    text: r.text,
    source: JSON.parse(r.source) as MemorySource,
    authorId: r.author_id,
    human: r.human === 1,
    status: r.status,
    confidence: r.confidence,
    uses: r.uses,
    lastUsedAt: r.last_used_at,
    version: r.version,
    createdAt: r.created_at,
    updatedAt: r.updated_at,
  };
  if (r.when_text) item.when = r.when_text;
  if (r.note) item.note = r.note;
  return item;
}

export function toUpdate(r: UpdateRow): MemoryUpdate {
  const u: MemoryUpdate = {
    id: r.id,
    day: r.day,
    scope: r.scope,
    memoryId: r.memory_id,
    change: r.change,
    before: r.before_json ? (JSON.parse(r.before_json) as MemoryItem) : null,
    after: r.after_json ? (JSON.parse(r.after_json) as MemoryItem) : null,
    undoneAt: r.undone_at,
    at: r.at,
  };
  if (r.note) u.note = r.note;
  return u;
}

export function getRow(db: D1Database, id: string): Promise<MemoryRow | null> {
  return db.prepare("SELECT * FROM memories WHERE id = ?").bind(id).first<MemoryRow>();
}

export function getUpdate(db: D1Database, id: string): Promise<UpdateRow | null> {
  return db.prepare("SELECT * FROM memory_updates WHERE id = ?").bind(id).first<UpdateRow>();
}

export interface NewMemory {
  scope: string;
  kind: MemoryKind;
  text: string;
  when?: string | null;
  source: MemorySource;
  note?: string | null;
  authorId: string;
  human: boolean;
  status: MemoryStatus;
  confidence: number;
}

function versionStatement(db: D1Database, row: MemoryRow, by: string | null, now: number): D1PreparedStatement {
  return db
    .prepare("INSERT INTO memory_versions (memory_id, version, snapshot, by_id, at) VALUES (?, ?, ?, ?, ?)")
    .bind(row.id, row.version, JSON.stringify(toItem(row)), by, now);
}

/** The row of a new memory (version 1) and the statements that store it. */
export function insertStatements(db: D1Database, m: NewMemory, by: string | null, now: number): { row: MemoryRow; stmts: D1PreparedStatement[] } {
  const row: MemoryRow = {
    id: ulid(now),
    scope: m.scope,
    kind: m.kind,
    text: cleanText(m.text),
    when_text: cleanWhen(m.when),
    source: JSON.stringify({ kind: m.source.kind, ids: m.source.ids.slice(0, 20) }),
    note: cleanNote(m.note),
    author_id: m.authorId,
    human: m.human ? 1 : 0,
    status: m.status,
    confidence: Math.min(1, Math.max(0, m.confidence)),
    uses: 0,
    last_used_at: null,
    version: 1,
    created_at: now,
    updated_at: now,
  };
  if (row.text === "") throw new ApiError("invalid", "text is empty");
  const stmts = [
    db
      .prepare(
        `INSERT INTO memories (id, scope, kind, text, when_text, source, note, author_id, human, status, confidence, uses, last_used_at, version, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, NULL, 1, ?, ?)`,
      )
      .bind(row.id, row.scope, row.kind, row.text, row.when_text, row.source, row.note, row.author_id, row.human, row.status, row.confidence, now, now),
    versionStatement(db, row, by, now),
  ];
  return { row, stmts };
}

export async function createMemory(db: D1Database, m: NewMemory, by: string | null, now = Date.now()): Promise<MemoryItem> {
  const { row, stmts } = insertStatements(db, m, by, now);
  await db.batch(stmts);
  return toItem(row);
}

export interface MemoryChange {
  kind?: MemoryKind;
  text?: string;
  when?: string | null;
  note?: string | null;
  status?: MemoryStatus;
  human?: boolean;
}

/** The row after `change`, as a new version; null when nothing would change. */
export function changed(row: MemoryRow, change: MemoryChange, now: number): MemoryRow | null {
  const next: MemoryRow = { ...row };
  if (change.kind !== undefined) next.kind = change.kind;
  if (change.text !== undefined) next.text = cleanText(change.text) || row.text;
  if (change.when !== undefined) next.when_text = cleanWhen(change.when);
  if (change.note !== undefined) next.note = cleanNote(change.note);
  if (change.status !== undefined) next.status = change.status;
  if (change.human !== undefined) next.human = change.human ? 1 : 0;
  const same = (["kind", "text", "when_text", "note", "status", "human"] as const).every((k) => next[k] === row[k]);
  if (same) return null;
  next.version = row.version + 1;
  next.updated_at = now;
  return next;
}

/** Stores a new version of a row. The update only applies on the version it was made from. */
export function saveStatements(db: D1Database, prev: MemoryRow, next: MemoryRow, by: string | null, now: number): D1PreparedStatement[] {
  return [
    db
      .prepare(
        `UPDATE memories SET kind = ?, text = ?, when_text = ?, note = ?, status = ?, human = ?, version = ?, updated_at = ?
         WHERE id = ? AND version = ?`,
      )
      .bind(next.kind, next.text, next.when_text, next.note, next.status, next.human, next.version, now, prev.id, prev.version),
    // The version row's primary key fails the batch when another write took this version first.
    versionStatement(db, next, by, now),
  ];
}

/** Runs a batch; a clash on (memory_id, version) becomes `conflict`. */
export async function runBatch(db: D1Database, stmts: D1PreparedStatement[]): Promise<void> {
  try {
    await db.batch(stmts);
  } catch (err) {
    if (/UNIQUE|PRIMARY KEY|constraint/i.test(String((err as Error)?.message ?? err))) {
      throw new ApiError("conflict", "the memory changed at the same time; try again");
    }
    throw err;
  }
}

/** Applies a change as a new version. Returns the row as it is afterwards. */
export async function saveChange(db: D1Database, row: MemoryRow, change: MemoryChange, by: string | null, now = Date.now()): Promise<MemoryRow> {
  const next = changed(row, change, now);
  if (!next) return row;
  await runBatch(db, saveStatements(db, row, next, by, now));
  return next;
}

/** A 记忆更新 line. */
export function updateStatement(
  db: D1Database,
  u: { day: string; scope: string; memoryId: string; change: MemoryUpdate["change"]; before: MemoryRow | null; after: MemoryRow | null; note?: string | null },
  now: number,
): D1PreparedStatement {
  return db
    .prepare(
      "INSERT INTO memory_updates (id, day, scope, memory_id, change, before_json, after_json, note, undone_at, at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL, ?)",
    )
    .bind(
      ulid(now),
      u.day,
      u.scope,
      u.memoryId,
      u.change,
      u.before ? JSON.stringify(toItem(u.before)) : null,
      u.after ? JSON.stringify(toItem(u.after)) : null,
      cleanNote(u.note),
      now,
    );
}
