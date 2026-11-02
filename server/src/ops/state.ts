// Per-member room state that is not in the event log (per-topic read positions, follow flags,
// mute and notify preferences). The archive snapshots it daily next to the group's events and
// restore writes it back after the events. Room RPCs, called with actor null (internal only).
import { ApiError } from "../core/errors";
import { registerRoomRpc } from "../core/registry";

/** Room tables copied whole. Each must have a primary key so a re-import replaces rows. */
export const ROOM_STATE_TABLES = ["topic_states", "member_prefs"] as const;

export interface RoomStateSnapshot {
  version: 1;
  lastSeq: number;
  tables: Record<string, { columns: string[]; rows: unknown[][] }>;
}

function hasTable(sql: SqlStorage, name: string): boolean {
  return sql.exec("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?", name).toArray().length > 0;
}

function columnsOf(sql: SqlStorage, name: string): string[] {
  return sql
    .exec<{ name: string }>("SELECT name FROM pragma_table_info(?)", name)
    .toArray()
    .map((c) => c.name);
}

function lastSeq(sql: SqlStorage): number {
  return sql.exec<{ s: number | null }>("SELECT max(seq) AS s FROM events").one().s ?? 0;
}

registerRoomRpc("ops.exportState", (room, actor) => {
  if (actor) throw new ApiError("forbidden", "internal only");
  const out: RoomStateSnapshot = { version: 1, lastSeq: lastSeq(room.sql), tables: {} };
  for (const t of ROOM_STATE_TABLES) {
    if (!hasTable(room.sql, t)) continue;
    const columns = columnsOf(room.sql, t);
    const rows = [...room.sql.exec(`SELECT ${columns.map((c) => `"${c}"`).join(", ")} FROM ${t}`).raw()] as unknown[][];
    out.tables[t] = { columns, rows };
  }
  return out;
});

registerRoomRpc("ops.importState", (room, actor, args) => {
  if (actor) throw new ApiError("forbidden", "internal only");
  const snap = args as RoomStateSnapshot;
  const marker = room.sql
    .exec<{ value: string }>("SELECT value FROM meta WHERE key = 'import_last_seq'")
    .toArray()[0]?.value;
  if (marker === undefined || Number(marker) !== lastSeq(room.sql)) {
    throw new ApiError("conflict", "room state can only be imported right after its events");
  }
  let rows = 0;
  for (const t of ROOM_STATE_TABLES) {
    const table = snap.tables?.[t];
    if (!table || !hasTable(room.sql, t)) continue;
    const known = new Set(columnsOf(room.sql, t));
    const idx = table.columns.map((c, i) => (known.has(c) ? i : -1)).filter((i) => i >= 0);
    if (idx.length === 0) continue;
    const cols = idx.map((i) => `"${table.columns[i]}"`).join(", ");
    const stmt = `INSERT OR REPLACE INTO ${t} (${cols}) VALUES (${idx.map(() => "?").join(", ")})`;
    for (const r of table.rows) {
      room.sql.exec(stmt, ...idx.map((i) => r[i] as SqlStorageValue));
      rows++;
    }
  }
  return { rows };
});
