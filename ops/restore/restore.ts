#!/usr/bin/env bun
// Restore from the R2 archive into a target environment. Every step is idempotent: re-run the
// same command after a failure and it goes on where it stopped.
//
// Source: --from <env> [--from-url <base>] [--from-secret-file <path>]   (the env's admin API)
//     or: --from-dir <folder>             (a folder that holds archive/..., e.g. from the R2 dashboard)
// Target: --to <env> [--to-url <base>] [--to-secret-file <path>]
// Admin secrets default to ~/.config/team-chat/<env>/admin-secret (mode 600).
//
//   bun ops/restore/restore.ts d1    <source> <target> [--date YYYY-MM-DD] [--resume]
//   bun ops/restore/restore.ts group <source> <target> --group <id> [--date YYYY-MM-DD]
//   bun ops/restore/restore.ts env   <source> <target> [--date YYYY-MM-DD] [--resume]
//   bun ops/restore/restore.ts fetch <source> --out <folder>      copy the archive to a folder
//
// Order for a whole environment: D1 first (members, tokens, groups), then every group's events,
// then its room state, then its ready-file records (from the restored D1 files table). The target must have its D1 migrations applied and no members (or pass
// --resume to go on with a restore that stopped).
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import {
  AdminApi,
  type ArchiveSource,
  type D1Dump,
  dirSource,
  fail,
  flag,
  gunzipText,
  httpSource,
  parseArgs,
  parseD1Key,
  parseGroupFileKey,
  parseStateKey,
} from "./lib";

const D1_CHUNK_ROWS = 500;
const D1_CHUNK_BYTES = 4 * 1024 * 1024;

export function sourceFrom(flags: Record<string, string | true>): ArchiveSource {
  const dir = flag(flags, "from-dir");
  return dir ? dirSource(dir) : httpSource(AdminApi.fromFlags(flags, "from"));
}

async function pickD1(source: ArchiveSource, date?: string): Promise<{ key: string; dump: D1Dump }> {
  const keys = (await source.list("archive/d1/")).filter((k) => parseD1Key(k) && (!date || parseD1Key(k)! <= date));
  const key = keys.at(-1);
  if (!key) fail(`no D1 dump in ${source.describe}${date ? ` on or before ${date}` : ""}`);
  return { key, dump: JSON.parse(gunzipText(await source.get(key))) as D1Dump };
}

export async function restoreD1(
  source: ArchiveSource,
  target: AdminApi,
  opts: { date?: string; resume?: boolean } = {},
): Promise<{ key: string; dump: D1Dump; rows: number; inserted: number; skippedTables: string[] }> {
  const { key, dump } = await pickD1(source, opts.date);
  const status = await target.json<{ tables: Record<string, number> }>("/v1/admin/import/d1");
  if ((status.tables.members ?? 0) > 0 && !opts.resume) {
    fail(`target ${target.base} already has members; restore needs an empty env (or --resume)`);
  }
  let rows = 0;
  let inserted = 0;
  const skippedTables: string[] = [];
  for (const t of dump.tables) {
    if (!(t.name in status.tables)) {
      skippedTables.push(t.name);
      console.warn(`warning: target has no table ${t.name}; ${t.rows.length} rows not restored`);
      continue;
    }
    let i = 0;
    while (i < t.rows.length) {
      const chunk: unknown[][] = [];
      let bytes = 0;
      while (i < t.rows.length && chunk.length < D1_CHUNK_ROWS) {
        const size = JSON.stringify(t.rows[i]).length;
        if (chunk.length > 0 && bytes + size > D1_CHUNK_BYTES) break;
        chunk.push(t.rows[i]);
        bytes += size;
        i++;
      }
      const r = await target.json<{ inserted: number }>("/v1/admin/import/d1", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ table: t.name, columns: t.columns, rows: chunk }),
      });
      rows += chunk.length;
      inserted += r.inserted;
    }
  }
  await target.json("/v1/admin/import/d1/done", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ source: `${dump.environment}:${key}`, tables: dump.tables.length }),
  });
  return { key, dump, rows, inserted, skippedTables };
}

export async function restoreGroup(
  source: ArchiveSource,
  target: AdminApi,
  groupId: string,
  opts: { date?: string } = {},
): Promise<{ groupId: string; files: number; imported: number; lastSeq: number; stateRows: number; fileRows: number }> {
  const keys = await source.list(`archive/groups/${groupId}/`);
  const files = keys
    .map((k) => ({ k, p: parseGroupFileKey(k) }))
    .filter((x) => x.p)
    .sort((a, b) => a.p!.from - b.p!.from || a.p!.to - b.p!.to);
  let imported = 0;
  let lastSeq = 0;
  for (const f of files) {
    if (f.p!.to <= lastSeq) continue; // fully covered by an earlier file
    const r = await target.json<{ imported: number; lastSeq: number }>(`/v1/admin/import/groups/${groupId}`, {
      method: "POST",
      headers: { "content-type": "application/gzip" },
      body: await source.get(f.k),
    });
    imported += r.imported;
    lastSeq = r.lastSeq;
  }
  const states = keys
    .map((k) => ({ k, p: parseStateKey(k) }))
    .filter((x) => x.p && (!opts.date || x.p.date <= opts.date))
    .sort((a, b) => a.p!.date.localeCompare(b.p!.date));
  let stateRows = 0;
  const state = states.at(-1);
  if (state && files.length > 0) {
    const r = await target.json<{ rows: number }>(`/v1/admin/import/groups/${groupId}/state`, {
      method: "POST",
      headers: { "content-type": "application/gzip" },
      body: await source.get(state.k),
    });
    stateRows = r.rows;
  }
  // Ready-file records (attachments and 产出 files) have no event: refill them from D1.
  let fileRows = 0;
  if (files.length > 0) {
    const r = await target.json<{ rows: number }>(`/v1/admin/import/groups/${groupId}/files`, { method: "POST" });
    fileRows = r.rows;
  }
  return { groupId, files: files.length, imported, lastSeq, stateRows, fileRows };
}

export async function restoreEnv(
  source: ArchiveSource,
  target: AdminApi,
  opts: { date?: string; resume?: boolean } = {},
) {
  const d1 = await restoreD1(source, target, opts);
  const groupsTable = d1.dump.tables.find((t) => t.name === "groups");
  const idCol = groupsTable?.columns.indexOf("id") ?? -1;
  const groupIds = groupsTable && idCol >= 0 ? groupsTable.rows.map((r) => r[idCol] as string) : [];
  const groups = [];
  for (const id of groupIds) groups.push(await restoreGroup(source, target, id, opts));
  return { d1: { key: d1.key, rows: d1.rows, inserted: d1.inserted, skippedTables: d1.skippedTables }, groups };
}

export async function fetchArchive(source: ArchiveSource, out: string): Promise<number> {
  const keys = await source.list("archive/");
  for (const k of keys) {
    const path = join(out, k);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, await source.get(k));
  }
  return keys.length;
}

if (import.meta.main) {
  const { command, flags } = parseArgs(process.argv.slice(2));
  const date = flag(flags, "date");
  const resume = flags.resume === true;
  const run = async () => {
    switch (command) {
      case "d1": {
        const r = await restoreD1(sourceFrom(flags), AdminApi.fromFlags(flags, "to"), { date, resume });
        return { key: r.key, rows: r.rows, inserted: r.inserted, skippedTables: r.skippedTables };
      }
      case "group": {
        const id = flag(flags, "group") ?? fail("--group <id> is required");
        return restoreGroup(sourceFrom(flags), AdminApi.fromFlags(flags, "to"), id, { date });
      }
      case "env":
        return restoreEnv(sourceFrom(flags), AdminApi.fromFlags(flags, "to"), { date, resume });
      case "fetch": {
        const out = flag(flags, "out") ?? fail("--out <folder> is required");
        return { objects: await fetchArchive(sourceFrom(flags), out), out };
      }
      default:
        fail("usage: restore.ts <d1|group|env|fetch> ... (see the header of ops/restore/restore.ts)");
    }
  };
  run()
    .then((r) => console.log(JSON.stringify(r, null, 2)))
    .catch((err) => fail(err instanceof Error ? err.message : String(err)));
}
