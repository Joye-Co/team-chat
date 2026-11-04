#!/usr/bin/env bun
// Restore from the R2 archive into a target environment. Every step is idempotent: re-run the
// same command after a failure and it goes on where it stopped.
//
// Source: --from <env> [--from-url <base>] [--from-secret-file <path>]   (the env's admin API)
//     or: --from-dir <folder>             (a folder that holds archive/..., e.g. from the R2 dashboard)
// Target: --to <env> [--to-url <base>] [--to-secret-file <path>]
// Admin secrets default to ~/.config/team-chat/<env>/admin-secret (mode 600).
//
//   bun ops/restore/restore.ts d1    <source> <target> [--date YYYY-MM-DD] [--resume] [--revocations <file>]
//   bun ops/restore/restore.ts group <source> <target> --group <id> [--from-group <id>] [--date YYYY-MM-DD]
//
// --from-group: restore that group's archive into the group --group (another group, for example
// a new empty group made for the incident). The target group must exist in the target D1 and have
// no events yet; its members come from the target D1. The source group id and every topic, message
// and 产出 id get new ids (the same on a re-run). Attachment and 产出 files stay owned by the
// source group (their D1 rows name it), see docs/RUNBOOK.md.
//   bun ops/restore/restore.ts env   <source> <target> [--date YYYY-MM-DD] [--resume] [--revocations <file>]
//
// --revocations: a file from `bun server/scripts/admin.ts revocations-save` taken from the source
// before the restore. A D1 dump is older than revocations made after it; they are applied again
// right after the D1 rows, so a revoked token or deactivated member never works again.
//   bun ops/restore/restore.ts fetch <source> --out <folder>      copy the archive to a folder
//
// Order for a whole environment: D1 first (members, tokens, groups), then every group's events,
// then its room state, then its ready-file records (from the restored D1 files table), then
// each member gateway's state (group read cursors, GitHub hook topic map). The target must have its D1 migrations applied and no members (or pass
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
  parseMemberStateKey,
  parseStateKey,
  readSecretFile,
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
  opts: { date?: string; resume?: boolean; revocations?: string } = {},
): Promise<{ key: string; dump: D1Dump; rows: number; inserted: number; skippedTables: string[]; reapplied: number }> {
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
  let reapplied = 0;
  if (opts.revocations) {
    const r = await target.json<{ applied: number }>("/v1/admin/revocations", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: readSecretFile(opts.revocations, "revocations file"),
    });
    reapplied = r.applied;
  }
  await target.json("/v1/admin/import/d1/done", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ source: `${dump.environment}:${key}`, tables: dump.tables.length }),
  });
  return { key, dump, rows, inserted, skippedTables, reapplied };
}

/** Replaces every string value found in `map` (exact match, at any depth). Ids are unique. */
export function remapValue(v: unknown, map: Map<string, string>): unknown {
  if (typeof v === "string") return map.get(v) ?? v;
  if (Array.isArray(v)) return v.map((x) => remapValue(x, map));
  if (v && typeof v === "object") {
    const out: Record<string, unknown> = {};
    for (const [k, x] of Object.entries(v)) out[k] = remapValue(x, map);
    return out;
  }
  return v;
}

const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/**
 * A new id for `oldId` in the target group: same for every run (a re-run goes on where it
 * stopped), unique per target group. A ULID keeps its 10-character time part, so ids still sort
 * by creation time.
 */
export function copiedId(toGroup: string, oldId: string): string {
  const h = new Bun.CryptoHasher("sha256").update(`restore-copy:${toGroup}:${oldId}`).digest();
  let tail = "";
  for (let i = 0; i < 26; i++) tail += CROCKFORD[h[i] & 31];
  return /^[0-9A-HJKMNP-TV-Z]{26}$/.test(oldId) ? oldId.slice(0, 10) + tail.slice(0, 16) : tail;
}

/** Keys whose string values are file ids (attachments, 产出 version files, media). */
const FILE_ID_KEYS = new Set(["fileId", "posterFileId", "peaksFileId", "transcriptFileId", "thumbFileId"]);

/** Every file id the value names under a `FILE_ID_KEYS` key, at any depth. */
export function fileIdsIn(v: unknown, out: Set<string> = new Set()): Set<string> {
  if (Array.isArray(v)) for (const x of v) fileIdsIn(x, out);
  else if (v && typeof v === "object") {
    for (const [k, x] of Object.entries(v)) {
      if (FILE_ID_KEYS.has(k) && typeof x === "string" && x !== "") out.add(x);
      else fileIdsIn(x, out);
    }
  }
  return out;
}

/** The kind of each id a copy maps (for the copy-files admin call). */
export type CopyKinds = { topics: Set<string>; files: Set<string>; artifacts: Set<string> };

/**
 * The id map of a copy into another group: the source group id to the target id, and every
 * topic, message, 产出 and file id the archived events name to a new id. Clients key topics,
 * messages and 产出 by id alone, so a copy next to its source must not reuse them. Files get new
 * ids too (GAPS SF-6): the copy gets its own D1 rows and R2 objects (`copy-files`), so its
 * attachments and 产出 open. `kinds`, when given, collects which ids are topics, files and 产出.
 */
export function copyIdMap(fromGroup: string, toGroup: string, ndjsonTexts: string[], kinds?: CopyKinds): Map<string, string> {
  const map = new Map<string, string>([[fromGroup, toGroup]]);
  const add = (id: unknown) => {
    if (typeof id === "string" && id !== "" && !map.has(id)) map.set(id, copiedId(toGroup, id));
  };
  for (const text of ndjsonTexts) {
    for (const line of text.split("\n")) {
      if (line.trim() === "") continue;
      const e = JSON.parse(line) as { type?: string; topicId?: unknown; messageId?: unknown; data?: any };
      if (e.type === "topic.created") {
        add(e.data?.topic?.id);
        if (typeof e.data?.topic?.id === "string") kinds?.topics.add(e.data.topic.id);
      } else if (e.type === "message.created") add(e.data?.message?.id);
      else if (e.type === "artifact.created") {
        add(e.data?.artifact?.id);
        if (typeof e.data?.artifact?.id === "string") kinds?.artifacts.add(e.data.artifact.id);
      }
      for (const f of fileIdsIn(e.data)) {
        add(f);
        kinds?.files.add(f);
      }
    }
  }
  return map;
}

function remapGzip(text: string, map: Map<string, string>, ndjson: boolean): Uint8Array {
  const out = ndjson
    ? text
        .split("\n")
        .map((line) => (line.trim() === "" ? line : JSON.stringify(remapValue(JSON.parse(line), map))))
        .join("\n")
    : JSON.stringify(remapValue(JSON.parse(text), map));
  return Bun.gzipSync(new TextEncoder().encode(out));
}

const GROUP_ID = /^[A-Za-z0-9_-]{1,64}$/;
/** Files and 产出 per copy-files call (server COPY_FILES_MAX). */
const COPY_CHUNK = 20;

export async function restoreGroup(
  source: ArchiveSource,
  target: AdminApi,
  groupId: string,
  opts: { date?: string; fromGroup?: string } = {},
): Promise<{ groupId: string; fromGroup: string; files: number; imported: number; lastSeq: number; stateRows: number; fileRows: number; copiedFiles: number }> {
  const fromGroup = opts.fromGroup ?? groupId;
  if (!GROUP_ID.test(groupId) || !GROUP_ID.test(fromGroup)) fail("a group id has letters, digits, - and _ only");
  const retarget = fromGroup !== groupId;
  const keys = await source.list(`archive/groups/${fromGroup}/`);
  if (retarget && !keys.some((k) => parseGroupFileKey(k))) fail(`no archived events of group ${fromGroup} in ${source.describe}`);
  let map: Map<string, string> | null = null;
  const kinds: CopyKinds = { topics: new Set(), files: new Set(), artifacts: new Set() };
  if (retarget) {
    const texts: string[] = [];
    for (const k of keys) if (parseGroupFileKey(k)) texts.push(gunzipText(await source.get(k)));
    map = copyIdMap(fromGroup, groupId, texts, kinds);
  }
  const read = async (k: string, ndjson: boolean) => {
    const bytes = await source.get(k);
    return map ? remapGzip(gunzipText(bytes), map, ndjson) : bytes;
  };
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
      body: await read(f.k, true),
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
      body: await read(state.k, false),
    });
    stateRows = r.rows;
  }
  // A copy gets its own D1 file and 产出 rows and R2 objects under the new ids, before the room
  // refills its file records from D1.
  let copiedFiles = 0;
  if (map && files.length > 0) {
    const pairs = (ids: Set<string>) => [...ids].map((id) => [id, map!.get(id) as string] as [string, string]);
    const topics = pairs(kinds.topics);
    const fileList = pairs(kinds.files);
    const artList = pairs(kinds.artifacts);
    for (let i = 0; i < Math.max(fileList.length, artList.length); i += COPY_CHUNK) {
      const r = await target.json<{ files: number; artifacts: number }>(`/v1/admin/import/groups/${groupId}/copy-files`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ fromGroup, files: fileList.slice(i, i + COPY_CHUNK), artifacts: artList.slice(i, i + COPY_CHUNK), topics }),
      });
      copiedFiles += r.files;
    }
  }
  // Ready-file records (attachments and 产出 files) have no event: refill them from D1.
  let fileRows = 0;
  if (files.length > 0) {
    const r = await target.json<{ rows: number }>(`/v1/admin/import/groups/${groupId}/files`, { method: "POST" });
    fileRows = r.rows;
  }
  return { groupId, fromGroup, files: files.length, imported, lastSeq, stateRows, fileRows, copiedFiles };
}

export async function restoreEnv(
  source: ArchiveSource,
  target: AdminApi,
  opts: { date?: string; resume?: boolean; revocations?: string } = {},
) {
  const d1 = await restoreD1(source, target, opts);
  const groupsTable = d1.dump.tables.find((t) => t.name === "groups");
  const idCol = groupsTable?.columns.indexOf("id") ?? -1;
  const groupIds = groupsTable && idCol >= 0 ? groupsTable.rows.map((r) => r[idCol] as string) : [];
  const groups = [];
  for (const id of groupIds) groups.push(await restoreGroup(source, target, id, opts));
  const members = await restoreMembers(source, target, opts);
  return { d1: { key: d1.key, rows: d1.rows, inserted: d1.inserted, skippedTables: d1.skippedTables, reapplied: d1.reapplied }, groups, members };
}

/** Each member gateway's newest state snapshot (on or before `date`): read cursors, hook map. */
export async function restoreMembers(
  source: ArchiveSource,
  target: AdminApi,
  opts: { date?: string } = {},
): Promise<{ members: number; rows: number }> {
  const latest = new Map<string, { key: string; date: string }>();
  for (const k of await source.list("archive/members/")) {
    const p = parseMemberStateKey(k);
    if (!p || (opts.date && p.date > opts.date)) continue;
    const prev = latest.get(p.memberId);
    if (!prev || prev.date < p.date) latest.set(p.memberId, { key: k, date: p.date });
  }
  let rows = 0;
  for (const [memberId, { key }] of latest) {
    const r = await target.json<{ rows: number }>(`/v1/admin/import/members/${memberId}/state`, {
      method: "POST",
      headers: { "content-type": "application/gzip" },
      body: await source.get(key),
    });
    rows += r.rows;
  }
  return { members: latest.size, rows };
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
  const revocations = flag(flags, "revocations");
  const run = async () => {
    switch (command) {
      case "d1": {
        const r = await restoreD1(sourceFrom(flags), AdminApi.fromFlags(flags, "to"), { date, resume, revocations });
        return { key: r.key, rows: r.rows, inserted: r.inserted, skippedTables: r.skippedTables, reapplied: r.reapplied };
      }
      case "group": {
        const id = flag(flags, "group") ?? fail("--group <id> is required");
        return restoreGroup(sourceFrom(flags), AdminApi.fromFlags(flags, "to"), id, { date, fromGroup: flag(flags, "from-group") });
      }
      case "env":
        return restoreEnv(sourceFrom(flags), AdminApi.fromFlags(flags, "to"), { date, resume, revocations });
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
