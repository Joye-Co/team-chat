// Daily archive to R2 (the env's FILES bucket):
//   archive/groups/<groupId>/<from>-<to>.ndjson.gz   stored events with from <= seq <= to
//   archive/d1/<YYYY-MM-DD>.json.gz                 every D1 table (hashes only, no secrets)
// Resumable and idempotent: D1 table ops_archive keeps the last archived seq per group and is
// moved forward after each file is written. A run stops at its budget and the next run goes on.
// A crash between the R2 write and the state update makes the next file overlap the last one;
// restore skips seqs it already has, so overlap is harmless.
import { type Env, roomStub } from "../core/env";
import { unwrap } from "../core/errors";
import type { Logger } from "../core/logger";
import type { ArchivedEvent } from "../core/registry";
import { gzip } from "./gzip";

export const GROUP_PREFIX = "archive/groups/";
export const D1_PREFIX = "archive/d1/";
/** Events per archive file. Keeps one file well under the import request limit. */
export const FILE_EVENTS = 5000;
const PAGE = 1000;
const D1_PAGE = 1000;
const SEQ_DIGITS = 12;

/** Tables that are not part of a D1 dump: archive state belongs to the bucket it describes. */
const SKIP_TABLES = /^(sqlite_|_cf_|d1_migrations$|ops_archive$)/;
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

export function utcDate(ms: number): string {
  return new Date(ms).toISOString().slice(0, 10);
}

export interface ArchiveOptions {
  now?: number;
  /** Stop starting new pages after this many ms (wall time). */
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
  complete: boolean;
}

export async function runArchive(env: Env, opts: ArchiveOptions = {}): Promise<ArchiveReport> {
  const now = opts.now ?? Date.now();
  const started = Date.now();
  const budgetMs = opts.budgetMs ?? 10 * 60 * 1000;
  const maxEvents = opts.maxEvents ?? 500_000;
  const report: ArchiveReport = { groups: 0, files: [], events: 0, d1Key: null, complete: true };
  const overBudget = () => Date.now() - started > budgetMs || report.events >= maxEvents;

  if (!opts.skipD1) report.d1Key = await dumpD1(env, now);

  const groups = await env.DB.prepare(
    `SELECT g.id AS id, COALESCE(a.last_seq, 0) AS last_seq FROM groups g
     LEFT JOIN ops_archive a ON a.group_id = g.id ORDER BY g.id`,
  ).all<{ id: string; last_seq: number }>();

  for (const g of groups.results) {
    if (overBudget()) {
      report.complete = false;
      break;
    }
    report.groups++;
    let after = g.last_seq;
    let buf: ArchivedEvent[] = [];
    const flush = async () => {
      if (buf.length === 0) return;
      const from = buf[0].seq;
      const to = buf[buf.length - 1].seq;
      const key = groupFileKey(g.id, from, to);
      const body = await gzip(`${buf.map((e) => JSON.stringify(e)).join("\n")}\n`);
      await env.FILES.put(key, body, {
        httpMetadata: { contentType: "application/gzip" },
        customMetadata: { groupId: g.id, from: String(from), to: String(to), events: String(buf.length) },
      });
      await env.DB.prepare(
        `INSERT INTO ops_archive (group_id, last_seq, updated_at) VALUES (?, ?, ?)
         ON CONFLICT (group_id) DO UPDATE SET last_seq = excluded.last_seq, updated_at = excluded.updated_at
         WHERE excluded.last_seq > ops_archive.last_seq`,
      )
        .bind(g.id, to, Date.now())
        .run();
      report.files.push(key);
      buf = [];
    };
    for (;;) {
      const page = unwrap(await roomStub(env, g.id).exportRows(g.id, after, PAGE));
      if (page.lastSeq < g.last_seq) {
        opts.log?.warn("room log is behind the archive", { groupId: g.id, lastSeq: page.lastSeq, archived: g.last_seq });
      }
      buf.push(...page.rows);
      report.events += page.rows.length;
      if (page.rows.length > 0) after = page.rows[page.rows.length - 1].seq;
      if (buf.length >= FILE_EVENTS) await flush();
      if (!page.hasMore) break;
      if (overBudget()) {
        report.complete = false;
        break;
      }
    }
    await flush();
  }
  opts.log?.info("archive run", {
    groups: report.groups,
    files: report.files.length,
    events: report.events,
    complete: report.complete,
    ms: Date.now() - started,
  });
  return report;
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
