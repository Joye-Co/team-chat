// Daily archive to R2 (the env's FILES bucket):
//   archive/groups/<groupId>/<from>-<to>.ndjson.gz   stored events with from <= seq <= to
//   archive/groups/<groupId>/state/<YYYY-MM-DD>.json.gz   per-member room state (reads, follows, prefs)
//   archive/members/<memberId>/state/<YYYY-MM-DD>.json.gz  member gateway state (group read
//                                                    cursors, GitHub hook topic map), once a day
//   archive/d1/<YYYY-MM-DD>.json.gz                 every D1 table (hashes only, no secrets)
// Resumable and idempotent: D1 table ops_archive keeps the last archived seq per group and is
// moved forward after each file is written. A run stops at its budget and the next run goes on
// after the last group it finished (ops_meta archive.cursor). A failing group is logged and
// skipped; archive.lastCompleteAt records the end of the newest pass with no failure.
// A message deleted after it was archived is scrubbed from the older files by the run that
// archives its delete event.
// A crash between the R2 write and the state update makes the next file overlap the last one;
// restore skips seqs it already has, so overlap is harmless.
import { type Env, gatewayStub, roomStub } from "../core/env";
import { unwrap } from "../core/errors";
import type { Logger } from "../core/logger";
import type { ArchivedEvent } from "../core/registry";
import { gunzip, gzip } from "./gzip";
import { DELETED_TITLE } from "../core/scrub";

export const GROUP_PREFIX = "archive/groups/";
export const D1_PREFIX = "archive/d1/";
/** Files replaced after a point-in-time restore of the room: kept, never restored. */
export const SUPERSEDED = "superseded";
/** Events per archive file. Keeps one file well under the import request limit. */
export const FILE_EVENTS = 5000;
const PAGE = 1000;
const D1_PAGE = 1000;
const SEQ_DIGITS = 12;

/** Tables that are not part of a D1 dump: archive state belongs to the bucket it describes. */
const SKIP_TABLES = /^(sqlite_|_cf_|d1_migrations$|ops_)/;
/** Columns that would hold a plain secret. Digests (`*_hash`) and encrypted values are kept. */
const SECRET_COLUMN = /(^|_)(secret|password|passwd|token|api_key|private_key|invite_code)$/i;
export const REDACTED = "[redacted]";

export function groupFileKey(groupId: string, from: number, to: number): string {
  const p = (n: number) => String(n).padStart(SEQ_DIGITS, "0");
  return `${GROUP_PREFIX}${groupId}/${p(from)}-${p(to)}.ndjson.gz`;
}

export function parseGroupFileKey(key: string): { groupId: string; from: number; to: number } | null {
  const m = /^archive\/groups\/([^/]+)\/(\d+)-(\d+)\.ndjson\.gz$/.exec(key);
  return m ? { groupId: m[1], from: Number(m[2]), to: Number(m[3]) } : null;
}

export const MEMBER_PREFIX = "archive/members/";

/** One member gateway's state (group read cursors, hook topic map) on one day. */
export function memberStateKey(memberId: string, date: string): string {
  return `${MEMBER_PREFIX}${memberId}/state/${date}.json.gz`;
}

export function groupStateKey(groupId: string, date: string): string {
  return `${GROUP_PREFIX}${groupId}/state/${date}.json.gz`;
}

export function utcDate(ms: number): string {
  return new Date(ms).toISOString().slice(0, 10);
}

export interface ArchiveOptions {
  now?: number;
  /** Stop starting new groups or pages after this many ms (wall time). */
  budgetMs?: number;
  /** Stop after this many events in this run. */
  maxEvents?: number;
  /** Skip the D1 dump. */
  skipD1?: boolean;
  log?: Logger;
}

export interface ArchiveReport {
  groups: number;
  files: string[];
  events: number;
  d1Key: string | null;
  /** The pass reached the last group. A pass that stopped at its budget goes on in the next run.
   *  True with failures too: see `failed`. */
  complete: boolean;
  /** Groups whose archive failed in this run (logged with the error); the next run retries them. */
  failed: string[];
  /** Older archive files rewritten to scrub messages that were deleted after they were archived. */
  scrubbed: string[];
}

/** Where a pass that stopped at its budget goes on: the last group id it finished. */
interface PassCursor {
  after: string;
  startedAt: number;
  failed: number;
}

export interface ArchiveStatus {
  /** End of the newest pass that archived every group with no failure. */
  lastCompleteAt: number | null;
  lastRun: { at: number; complete: boolean; failed: number } | null;
}

const META_CURSOR = "archive.cursor";
const META_COMPLETE = "archive.lastCompleteAt";
const META_RUN = "archive.lastRun";
const META_GATEWAYS = "archive.gatewaysDate";

async function getMeta<T>(env: Env, key: string): Promise<T | null> {
  const r = await env.DB.prepare("SELECT value FROM ops_meta WHERE key = ?").bind(key).first<{ value: string }>();
  return r ? (JSON.parse(r.value) as T) : null;
}

async function setMeta(env: Env, key: string, value: unknown): Promise<void> {
  if (value === null) {
    await env.DB.prepare("DELETE FROM ops_meta WHERE key = ?").bind(key).run();
    return;
  }
  await env.DB.prepare("INSERT OR REPLACE INTO ops_meta (key, value, updated_at) VALUES (?, ?, ?)")
    .bind(key, JSON.stringify(value), Date.now())
    .run();
}

export async function archiveStatus(env: Env): Promise<ArchiveStatus> {
  return {
    lastCompleteAt: await getMeta<number>(env, META_COMPLETE),
    lastRun: await getMeta<ArchiveStatus["lastRun"]>(env, META_RUN),
  };
}

type Obj = Record<string, unknown>;

/** What a delete removes from a message besides its body (core/scrub.ts and the files feature). */
const DELETED_FIELDS = ["card", "replyTo", "reactions", "attachments", "artifactRefs"];

function dropFields(o: Obj): boolean {
  let changed = false;
  for (const k of DELETED_FIELDS) {
    if (k in o) {
      delete o[k];
      changed = true;
    }
  }
  return changed;
}

/**
 * Same change as the live delete makes to stored events (GroupRoom.removeNow): the deleted
 * message loses its content, and quotes of it lose their excerpt.
 */
export function scrubArchivedEvent(e: ArchivedEvent, ids: Set<string>, topicIds: Set<string> = new Set()): boolean {
  const d = (e.data ?? {}) as Obj;
  // A deleted topic (topic.delete) also loses its title, as GroupRoom.topicDelete does.
  if (e.topicId && topicIds.has(e.topicId)) {
    if (e.type === "topic.created" && d.topic && typeof d.topic === "object") {
      const t = d.topic as Obj;
      if (t.title === DELETED_TITLE) return false;
      t.title = DELETED_TITLE;
      return true;
    }
    if (e.type === "topic.updated" && typeof d.title === "string") {
      if (d.title === DELETED_TITLE) return false;
      d.title = DELETED_TITLE;
      return true;
    }
  }
  if (e.type === "message.created" && d.message && typeof d.message === "object") {
    const m = d.message as Obj;
    const reply = m.replyTo as Obj | undefined;
    let changed = false;
    if (reply && typeof reply === "object" && ids.has(reply.messageId as string) && reply.excerpt !== "") {
      reply.excerpt = "";
      changed = true;
    }
    if (!e.messageId || !ids.has(e.messageId)) return changed;
    if (m.body !== "") {
      m.body = "";
      changed = true;
    }
    return dropFields(m) || changed;
  }
  if (!e.messageId || !ids.has(e.messageId)) return false;
  if (e.type === "message.updated") {
    let changed = false;
    if (d.body !== "") {
      d.body = "";
      changed = true;
    }
    for (const k of ["card", "attachments", "artifactRefs"]) {
      if (k in d) {
        delete d[k];
        changed = true;
      }
    }
    return changed;
  }
  return false;
}

async function listGroupFiles(env: Env, groupId: string): Promise<{ key: string; from: number; to: number }[]> {
  const out: { key: string; from: number; to: number }[] = [];
  let cursor: string | undefined;
  do {
    const page = await env.FILES.list({ prefix: `${GROUP_PREFIX}${groupId}/`, cursor, limit: 1000 });
    for (const o of page.objects) {
      const p = parseGroupFileKey(o.key);
      if (p) out.push({ key: o.key, from: p.from, to: p.to });
    }
    cursor = page.truncated ? page.cursor : undefined;
  } while (cursor);
  return out;
}

async function readEventFile(env: Env, key: string): Promise<ArchivedEvent[]> {
  const obj = await env.FILES.get(key);
  if (!obj) return [];
  const text = await gunzip(await obj.arrayBuffer());
  return text
    .split("\n")
    .filter((l) => l !== "")
    .map((l) => JSON.parse(l) as ArchivedEvent);
}

async function writeEventFile(env: Env, groupId: string, key: string, rows: ArchivedEvent[]): Promise<void> {
  const from = rows[0].seq;
  const to = rows[rows.length - 1].seq;
  await env.FILES.put(key, await gzip(`${rows.map((e) => JSON.stringify(e)).join("\n")}\n`), {
    httpMetadata: { contentType: "application/gzip" },
    customMetadata: { groupId, from: String(from), to: String(to), events: String(rows.length) },
  });
}

/**
 * Messages deleted after their events were archived: rewrite the archive files in
 * [fromSeq, upTo] that hold them, so R2 keeps no deleted body or card. Idempotent.
 */
async function scrubArchive(
  env: Env,
  groupId: string,
  ids: Set<string>,
  fromSeq: number,
  upTo: number,
  topicIds: Set<string> = new Set(),
): Promise<string[]> {
  const rewritten: string[] = [];
  for (const f of await listGroupFiles(env, groupId)) {
    if (f.to < fromSeq || f.from > upTo) continue;
    const rows = await readEventFile(env, f.key);
    let changed = false;
    for (const e of rows) if (scrubArchivedEvent(e, ids, topicIds)) changed = true;
    if (!changed) continue;
    await writeEventFile(env, groupId, f.key, rows);
    rewritten.push(f.key);
  }
  return rewritten;
}

export async function runArchive(env: Env, opts: ArchiveOptions = {}): Promise<ArchiveReport> {
  const now = opts.now ?? Date.now();
  const date = utcDate(now);
  const started = Date.now();
  const budgetMs = opts.budgetMs ?? 10 * 60 * 1000;
  const maxEvents = opts.maxEvents ?? 500_000;
  const report: ArchiveReport = { groups: 0, files: [], events: 0, d1Key: null, complete: true, failed: [], scrubbed: [] };
  const overBudget = () => Date.now() - started > budgetMs || report.events >= maxEvents;

  // A pass that stopped at its budget goes on after the last group it finished, so repeated
  // short runs (the admin route) reach every group.
  const cursor = await getMeta<PassCursor>(env, META_CURSOR);
  const pass: PassCursor = cursor ?? { after: "", startedAt: now, failed: 0 };
  if (!opts.skipD1) {
    const key = `${D1_PREFIX}${date}.json.gz`;
    report.d1Key = cursor && (await env.FILES.head(key)) ? key : await dumpD1(env, now);
  }

  const groups = await env.DB.prepare(
    `SELECT g.id AS id, COALESCE(a.last_seq, 0) AS last_seq, a.state_date AS state_date FROM groups g
     LEFT JOIN ops_archive a ON a.group_id = g.id WHERE g.id > ? AND g.deleted_at IS NULL ORDER BY g.id`,
  )
    .bind(pass.after)
    .all<{ id: string; last_seq: number; state_date: string | null }>();

  let stopped = false;
  for (const g of groups.results) {
    // Every run finishes at least one group, so repeated short runs always make progress.
    if (report.groups > 0 && overBudget()) {
      stopped = true;
      break;
    }
    report.groups++;
    try {
      const done = await archiveGroup(env, g, date, report, overBudget, opts.log);
      if (!done) {
        stopped = true;
        break;
      }
    } catch (err) {
      // One failing group must not stop the others. The next run retries it from its last seq.
      report.failed.push(g.id);
      pass.failed++;
      opts.log?.error("group archive failed", { groupId: g.id, err: err instanceof Error ? err.message : String(err) });
    }
    pass.after = g.id;
  }
  // Member gateways once a day, after a pass reached the last group: a few RPCs per member.
  if (!stopped && (await getMeta<string>(env, META_GATEWAYS)) !== date) {
    let ok = true;
    const members = await env.DB.prepare("SELECT id FROM members ORDER BY id").all<{ id: string }>();
    for (const m of members.results) {
      try {
        const state = await gatewayStub(env, m.id).rpc("ops.exportGateway", null);
        const key = memberStateKey(m.id, date);
        await env.FILES.put(key, await gzip(JSON.stringify(state)), { httpMetadata: { contentType: "application/gzip" } });
        report.files.push(key);
      } catch (err) {
        ok = false;
        report.failed.push(`member:${m.id}`);
        pass.failed++;
        opts.log?.error("member archive failed", { memberId: m.id, err: err instanceof Error ? err.message : String(err) });
      }
    }
    if (ok) await setMeta(env, META_GATEWAYS, date);
  }
  report.complete = !stopped;
  if (stopped) {
    await setMeta(env, META_CURSOR, pass);
  } else {
    await setMeta(env, META_CURSOR, null);
    if (pass.failed === 0) await setMeta(env, META_COMPLETE, Date.now());
  }
  await setMeta(env, META_RUN, { at: Date.now(), complete: report.complete, failed: report.failed.length });
  const fields = {
    groups: report.groups,
    files: report.files.length,
    events: report.events,
    complete: report.complete,
    failed: report.failed.length,
    scrubbed: report.scrubbed.length,
    ms: Date.now() - started,
  };
  if (report.failed.length > 0) opts.log?.error("archive run had failures", fields);
  else opts.log?.info("archive run", fields);
  return report;
}

/** Archives one group's new events and, once a day or after new events, its room state. */
async function archiveGroup(
  env: Env,
  g: { id: string; last_seq: number; state_date: string | null },
  date: string,
  report: ArchiveReport,
  overBudget: () => boolean,
  log?: Logger,
): Promise<boolean> {
  const room = roomStub(env, g.id);
  let after = g.last_seq;
  let archivedTo = g.last_seq;
  let exported = 0;
  let buf: ArchivedEvent[] = [];
  const flush = async () => {
    if (buf.length === 0) return;
    const from = buf[0].seq;
    const to = buf[buf.length - 1].seq;
    const key = groupFileKey(g.id, from, to);
    await writeEventFile(env, g.id, key, buf);
    await env.DB.prepare(
      `INSERT INTO ops_archive (group_id, last_seq, updated_at) VALUES (?, ?, ?)
       ON CONFLICT (group_id) DO UPDATE SET last_seq = excluded.last_seq, updated_at = excluded.updated_at
       WHERE excluded.last_seq > ops_archive.last_seq`,
    )
      .bind(g.id, to, Date.now())
      .run();
    archivedTo = to;
    report.files.push(key);
    buf = [];
  };
  let finished = true;
  for (;;) {
    const page = unwrap(await room.exportRows(g.id, after, PAGE));
    if (page.lastSeq < g.last_seq) {
      log?.warn("room log is behind the archive", { groupId: g.id, lastSeq: page.lastSeq, archived: g.last_seq });
    }
    // Deletes of messages whose events are already in R2: scrub those files before this page
    // (and its delete event) counts as archived.
    const deleted = new Set<string>();
    const deletedTopics: string[] = [];
    for (const r of page.rows) {
      if (r.type === "topic.deleted") {
        const t = r.topicId ?? ((r.data as Obj)?.topicId as string | undefined);
        if (t) deletedTopics.push(t);
        continue;
      }
      if (r.type !== "message.deleted") continue;
      const id = r.messageId ?? ((r.data as Obj)?.messageId as string | undefined);
      if (id) deleted.add(id);
    }
    // A deleted topic stores no message.deleted events: its messages are scrubbed the same way.
    if (deletedTopics.length > 0 && archivedTo > 0) {
      const ids = unwrap(await room.rpc(g.id, "ops.topicMessageIds", null, { topicIds: deletedTopics })) as string[];
      for (const id of ids) deleted.add(id);
    }
    if (deleted.size > 0 && archivedTo > 0) {
      const seqs = unwrap(await room.rpc(g.id, "ops.messageSeqs", null, { ids: [...deleted] })) as Record<string, number>;
      const old = new Set([...deleted].filter((id) => seqs[id] !== undefined && seqs[id] <= archivedTo));
      if (old.size > 0) {
        // A deleted topic's own events (topic.created) come just before its first message.
        const fromSeq = Math.min(...[...old].map((id) => seqs[id])) - (deletedTopics.length > 0 ? 1 : 0);
        report.scrubbed.push(...(await scrubArchive(env, g.id, old, Math.max(1, fromSeq), archivedTo, new Set(deletedTopics))));
      }
    }
    buf.push(...page.rows);
    report.events += page.rows.length;
    exported += page.rows.length;
    if (page.rows.length > 0) after = page.rows[page.rows.length - 1].seq;
    if (buf.length >= FILE_EVENTS) await flush();
    if (!page.hasMore) break;
    if (overBudget()) {
      finished = false;
      break;
    }
  }
  await flush();
  if (exported > 0 || g.state_date !== date) {
    const state = unwrap(await room.rpc(g.id, "ops.exportState", null, {}));
    const stateKey = groupStateKey(g.id, date);
    await env.FILES.put(stateKey, await gzip(JSON.stringify(state)), { httpMetadata: { contentType: "application/gzip" } });
    await env.DB.prepare(
      `INSERT INTO ops_archive (group_id, last_seq, updated_at, state_date) VALUES (?, ?, ?, ?)
       ON CONFLICT (group_id) DO UPDATE SET state_date = excluded.state_date`,
    )
      .bind(g.id, archivedTo, Date.now(), date)
      .run();
    report.files.push(stateKey);
  }
  return finished;
}

/**
 * After a point-in-time restore of a room to `keep` (its newest seq at the restore point), seqs
 * above `keep` will be written again with new events. Moves the archive files above `keep` to
 * archive/groups/<id>/superseded/<now>/, keeps the part of a straddling file up to `keep`, and
 * moves the group's archive position back, so the new events are archived and never mix with
 * the old ones. Restore reads only files directly under the group folder.
 */
export async function supersedeAfter(env: Env, groupId: string, keep: number, now = Date.now()): Promise<string[]> {
  const moved: string[] = [];
  for (const f of await listGroupFiles(env, groupId)) {
    if (f.to <= keep) continue;
    const obj = await env.FILES.get(f.key);
    if (!obj) continue;
    const bytes = await obj.arrayBuffer();
    const name = f.key.slice(f.key.lastIndexOf("/") + 1);
    await env.FILES.put(`${GROUP_PREFIX}${groupId}/${SUPERSEDED}/${now}/${name}`, bytes, {
      httpMetadata: { contentType: "application/gzip" },
    });
    if (f.from <= keep) {
      const rows = (await readEventFile(env, f.key)).filter((e) => e.seq <= keep);
      if (rows.length > 0) await writeEventFile(env, groupId, groupFileKey(groupId, rows[0].seq, rows[rows.length - 1].seq), rows);
    }
    await env.FILES.delete(f.key);
    moved.push(f.key);
  }
  await env.DB.prepare("UPDATE ops_archive SET last_seq = ?, updated_at = ? WHERE group_id = ? AND last_seq > ?")
    .bind(keep, now, groupId, keep)
    .run();
  return moved;
}

export interface D1Dump {
  version: 1;
  environment: string;
  createdAt: number;
  tables: { name: string; columns: string[]; rows: unknown[][] }[];
}

/** Writes archive/d1/<date>.json.gz with every table in creation order. */
export async function dumpD1(env: Env, now = Date.now()): Promise<string> {
  const master = await env.DB.prepare(
    "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY rowid",
  ).all<{ name: string }>();
  const dump: D1Dump = { version: 1, environment: env.ENVIRONMENT, createdAt: now, tables: [] };
  for (const { name } of master.results) {
    if (SKIP_TABLES.test(name)) continue;
    const q = `"${name.replace(/"/g, '""')}"`;
    const info = await env.DB.prepare(`SELECT name FROM pragma_table_info(?)`).bind(name).all<{ name: string }>();
    const columns = info.results.map((c) => c.name);
    const secret = columns.map((c) => SECRET_COLUMN.test(c));
    const rows: unknown[][] = [];
    const add = (list: Record<string, unknown>[]) => {
      for (const row of list) rows.push(columns.map((c, i) => (secret[i] && row[c] != null ? REDACTED : row[c])));
    };
    try {
      let last = Number.MIN_SAFE_INTEGER;
      for (;;) {
        const r = await env.DB.prepare(`SELECT rowid AS __rowid, * FROM ${q} WHERE rowid > ? ORDER BY rowid LIMIT ?`)
          .bind(last, D1_PAGE)
          .all<Record<string, unknown>>();
        add(r.results);
        if (r.results.length < D1_PAGE) break;
        last = r.results[r.results.length - 1].__rowid as number;
      }
    } catch {
      // WITHOUT ROWID table: read it whole (feature tables are small).
      rows.length = 0;
      add((await env.DB.prepare(`SELECT * FROM ${q}`).all<Record<string, unknown>>()).results);
    }
    dump.tables.push({ name, columns, rows });
  }
  const key = `${D1_PREFIX}${utcDate(now)}.json.gz`;
  await env.FILES.put(key, await gzip(JSON.stringify(dump)), {
    httpMetadata: { contentType: "application/gzip" },
    customMetadata: { environment: env.ENVIRONMENT, tables: String(dump.tables.length) },
  });
  return key;
}
