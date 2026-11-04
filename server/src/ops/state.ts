// Per-member room state that is not in the event log (per-topic read positions, follow flags,
// mute and notify preferences), and the 产出 page log (`page_events`, GAPS SF-2). The archive snapshots it daily next to the group's events and
// restore writes it back after the events. Room RPCs, called with actor null (internal only).
import { ApiError } from "../core/errors";
import { registerGatewayRpc, registerRoomRpc } from "../core/registry";

/**
 * Room tables copied whole: state the event log cannot give back. Each must have a primary key
 * so a re-import replaces rows. cards2: idempotency keys and causes of card events, the
 * requester of each ask round, answers a run took; files: the data snapshot and the page log of each 产出 page.
 */
export const ROOM_STATE_TABLES = [
  "topic_states",
  "member_prefs",
  "card_events",
  "card_requesters",
  "card_consumed",
  "artifact_data",
  "page_events",
] as const;

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
  // The data rev a 产出 version shows follows its restored data snapshot.
  if (hasTable(room.sql, "artifact_data")) {
    room.sql.exec(
      `UPDATE artifact_versions SET data_rev = (SELECT d.data_rev FROM artifact_data d
         WHERE d.artifact_id = artifact_versions.artifact_id AND d.version = artifact_versions.version)
       WHERE EXISTS (SELECT 1 FROM artifact_data d WHERE d.artifact_id = artifact_versions.artifact_id AND d.version = artifact_versions.version)`,
    );
  }
  return { rows };
});

/** created_seq of messages, for rewriting archive files that hold a since-deleted message. */
registerRoomRpc("ops.messageSeqs", (room, actor, args) => {
  if (actor) throw new ApiError("forbidden", "internal only");
  const { ids } = args as { ids: string[] };
  const out: Record<string, number> = {};
  for (const id of ids) {
    const r = room.sql.exec<{ s: number }>("SELECT created_seq AS s FROM messages WHERE id = ?", id).toArray()[0];
    if (r) out[id] = r.s;
  }
  return out;
});

/** The newest seq stored at or before `at` (ms), for a point-in-time restore of the archive. */
registerRoomRpc("ops.seqAt", (room, actor, args) => {
  if (actor) throw new ApiError("forbidden", "internal only");
  const { at } = args as { at: number };
  return { seq: room.sql.exec<{ s: number | null }>("SELECT max(seq) AS s FROM events WHERE at <= ?", at).one().s ?? 0 };
});

// ---------------------------------------------------------------------------
// Member gateway state: group read cursors and the GitHub hook topic map
// ---------------------------------------------------------------------------

/** Gateway tables copied whole (those that exist). Each has a primary key. */
export const GATEWAY_STATE_TABLES = ["cursors", "hook_keys", "hook_shas", "hook_deliveries"] as const;

export interface GatewayStateSnapshot {
  version: 1;
  tables: Record<string, { columns: string[]; rows: unknown[][] }>;
}

registerGatewayRpc("ops.exportGateway", (gw) => {
  const out: GatewayStateSnapshot = { version: 1, tables: {} };
  for (const t of GATEWAY_STATE_TABLES) {
    if (!hasTable(gw.sql, t)) continue;
    const columns = columnsOf(gw.sql, t);
    const rows = [...gw.sql.exec(`SELECT ${columns.map((c) => `"${c}"`).join(", ")} FROM ${t}`).raw()] as unknown[][];
    out.tables[t] = { columns, rows };
  }
  return out;
});

/**
 * Restore: merges a snapshot into the gateway. Rows it already has win (the member may have
 * used the app since the restore began), except that a read cursor only moves forward.
 */
registerGatewayRpc("ops.importGateway", (gw, args) => {
  const snap = args as GatewayStateSnapshot;
  let rows = 0;
  for (const t of GATEWAY_STATE_TABLES) {
    const table = snap.tables?.[t];
    if (!table || !hasTable(gw.sql, t)) continue;
    const known = new Set(columnsOf(gw.sql, t));
    const idx = table.columns.map((c, i) => (known.has(c) ? i : -1)).filter((i) => i >= 0);
    if (idx.length === 0) continue;
    const names = idx.map((i) => table.columns[i]);
    const cols = names.map((c) => `"${c}"`).join(", ");
    const values = idx.map(() => "?").join(", ");
    const stmt =
      t === "cursors" && names.includes("group_id") && names.includes("seq")
        ? `INSERT INTO cursors (${cols}) VALUES (${values})
           ON CONFLICT (group_id) DO UPDATE SET seq = max(cursors.seq, excluded.seq)`
        : `INSERT OR IGNORE INTO ${t} (${cols}) VALUES (${values})`;
    for (const r of table.rows) {
      gw.sql.exec(stmt, ...idx.map((i) => r[i] as SqlStorageValue));
      rows++;
    }
  }
  return { rows };
});
