// Admin routes for the archive and for restore. All need the X-Admin-Secret header.
//   POST /v1/admin/archive/run                 run the daily archive now (goes on where the last run stopped)
//   GET  /v1/ops/archive                       archive freshness (any member token; times only)
//   GET  /v1/admin/archive?prefix=&cursor=     list archive objects of this env's bucket
//   GET  /v1/admin/archive/object?key=         one archive object (gzip bytes)
//   POST /v1/admin/import/groups/:id           gzip NDJSON of archived events -> empty GroupRoom
//   POST /v1/admin/import/groups/:id/state     gzip JSON room state snapshot, right after the events
//   POST /v1/admin/import/groups/:id/files     refill the room's ready-file records from D1 files
//   POST /v1/admin/import/members/:id/state    gzip JSON member gateway snapshot (cursors, hook map)
//   GET  /v1/admin/import/d1                   row counts per D1 table (restore pre-check)
//   POST /v1/admin/import/d1                   { table, columns, rows } -> INSERT OR IGNORE
//   POST /v1/admin/pitr/groups/:id             { at, apply? } point-in-time recovery of one room
//   POST /v1/admin/import/d1/done              records the D1 restore in the audit log
//   GET  /v1/admin/revocations                 revoked tokens and machines, deleted hooks, deactivated members
//   POST /v1/admin/revocations                 apply such a list again (after a D1 restore)
import { z } from "zod";
import { audit } from "../core/audit";
import { timingSafeEqualString } from "../core/crypto";
import { type Env, gatewayStub, roomStub } from "../core/env";
import { ApiError, unwrap } from "../core/errors";
import { readText } from "../core/http";
import { type ArchivedEvent, registerRoute } from "../core/registry";
import { archiveStatus, runArchive, supersedeAfter } from "./archive";
import { gunzip } from "./gzip";

const MAX_IMPORT_BYTES = 32 * 1024 * 1024;
const MAX_IMPORT_TEXT = 256 * 1024 * 1024;
const IMPORT_SLICE = 1000;
const D1_IMPORT_BYTES = 8 * 1024 * 1024;
const D1_BATCH = 200;
const FILES_PAGE = 500;
const PITR_MARGIN_MS = 60_000;
const REVOCATIONS_BYTES = 4 * 1024 * 1024;

const Revoked = z.object({ id: z.string().min(1).max(64), at: z.int().nonnegative() });
const Revocations = z.object({
  version: z.literal(1),
  environment: z.string().max(32).optional(),
  at: z.int().nonnegative().optional(),
  tokens: z.array(Revoked).max(50_000),
  machines: z.array(Revoked).max(10_000),
  hooks: z.array(Revoked).max(10_000),
  members: z.array(z.string().min(1).max(64)).max(10_000),
});

type FileRecordD1 = {
  id: string;
  topic_id: string | null;
  name: string;
  size: number;
  mime: string;
  sha256: string;
  width: number | null;
  height: number | null;
  created_by: string;
  created_at: number;
};

export async function requireAdmin(request: Request, env: Env): Promise<void> {
  const given = request.headers.get("x-admin-secret") ?? "";
  const expected = env.ADMIN_SECRET ?? "";
  const ok = await timingSafeEqualString(given, expected);
  if (!expected || !ok) throw new ApiError("unauthorized", "invalid admin secret");
}

const ArchivedEventSchema = z.object({
  seq: z.int().positive(),
  type: z.string().min(1).max(64),
  at: z.int().nonnegative(),
  actorId: z.string().min(1).max(64),
  topicId: z.string().max(64).nullish(),
  messageId: z.string().max(64).nullish(),
  data: z.record(z.string(), z.unknown()),
});

const ArchiveKey = z.string().startsWith("archive/").max(512).refine((k) => !k.includes(".."), "bad key");

async function readBytes(request: Request, limit: number): Promise<Uint8Array> {
  const declared = request.headers.get("content-length");
  if (declared && Number(declared) > limit) throw new ApiError("too_large", "request body too large");
  const buf = new Uint8Array(await request.arrayBuffer());
  if (buf.byteLength > limit) throw new ApiError("too_large", "request body too large");
  return buf;
}

export function parseNdjson(text: string): ArchivedEvent[] {
  const out: ArchivedEvent[] = [];
  let line = 0;
  for (const raw of text.split("\n")) {
    line++;
    if (raw.trim() === "") continue;
    let value: unknown;
    try {
      value = JSON.parse(raw);
    } catch {
      throw new ApiError("invalid", `line ${line}: not JSON`);
    }
    const r = ArchivedEventSchema.safeParse(value);
    if (!r.success) throw new ApiError("invalid", `line ${line}: not an archived event`);
    out.push(r.data);
  }
  return out;
}

const D1Chunk = z.object({
  table: z.string().min(1).max(128),
  columns: z.array(z.string().min(1).max(128)).min(1).max(200),
  rows: z.array(z.array(z.unknown())).max(5000),
});

const StateSnapshot = z.object({
  version: z.literal(1),
  lastSeq: z.int().nonnegative(),
  tables: z.record(
    z.string(),
    z.object({ columns: z.array(z.string().max(128)).max(100), rows: z.array(z.array(z.union([z.string(), z.number(), z.null()]))) }),
  ),
});

const GatewaySnapshot = z.object({
  version: z.literal(1),
  tables: z.record(
    z.string(),
    z.object({ columns: z.array(z.string().max(128)).max(100), rows: z.array(z.array(z.union([z.string(), z.number(), z.null()]))) }),
  ),
});

const quote = (id: string) => `"${id.replace(/"/g, '""')}"`;

async function tableNames(env: Env): Promise<string[]> {
  const r = await env.DB.prepare(
    "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE '_cf_%' ORDER BY rowid",
  ).all<{ name: string }>();
  return r.results.map((x) => x.name);
}

export function registerOpsRoutes(): void {
  registerRoute({
    method: "POST",
    path: "/v1/admin/archive/run",
    auth: "none",
    async handler({ request, env, log }) {
      await requireAdmin(request, env);
      const report = await runArchive(env, { log, budgetMs: 20_000 });
      await audit(env.DB, null, {
        action: "admin.archive.run",
        details: { files: report.files.length, events: report.events, complete: report.complete, failed: report.failed.length },
      });
      return report;
    },
  });

  // Archive freshness for the monitor (its bot token is a member token). Times only.
  registerRoute({
    method: "GET",
    path: "/v1/ops/archive",
    auth: "member",
    handler: ({ env }) => archiveStatus(env),
  });

  registerRoute({
    method: "GET",
    path: "/v1/admin/archive",
    auth: "none",
    async handler({ request, env, url }) {
      await requireAdmin(request, env);
      const prefix = ArchiveKey.safeParse(url.searchParams.get("prefix") ?? "archive/");
      if (!prefix.success) throw new ApiError("invalid", "prefix must start with archive/");
      const cursor = url.searchParams.get("cursor") ?? undefined;
      const list = await env.FILES.list({ prefix: prefix.data, cursor, limit: 1000 });
      return {
        objects: list.objects.map((o) => ({ key: o.key, size: o.size, uploaded: o.uploaded.getTime() })),
        cursor: list.truncated ? list.cursor : null,
      };
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/admin/archive/object",
    auth: "none",
    async handler({ request, env, url }) {
      await requireAdmin(request, env);
      const key = ArchiveKey.safeParse(url.searchParams.get("key"));
      if (!key.success) throw new ApiError("invalid", "key must start with archive/");
      const obj = await env.FILES.get(key.data);
      if (!obj) throw new ApiError("not_found", "no such archive object");
      return new Response(obj.body, {
        headers: { "content-type": "application/gzip", "content-length": String(obj.size) },
      });
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/admin/import/groups/:id",
    auth: "none",
    async handler({ request, env, params, log }) {
      await requireAdmin(request, env);
      const groupId = params.id;
      const bytes = await readBytes(request, MAX_IMPORT_BYTES);
      const isGzip = bytes[0] === 0x1f && bytes[1] === 0x8b;
      let text: string;
      try {
        text = isGzip ? await gunzip(bytes, MAX_IMPORT_TEXT) : new TextDecoder().decode(bytes);
      } catch (err) {
        if (err instanceof RangeError) throw new ApiError("too_large", "archive chunk too large");
        throw new ApiError("invalid", "body is not valid gzip");
      }
      const events = parseNdjson(text).sort((a, b) => a.seq - b.seq);
      let imported = 0;
      let skipped = 0;
      let lastSeq = 0;
      const room = roomStub(env, groupId);
      for (let i = 0; i < events.length || i === 0; i += IMPORT_SLICE) {
        const r = unwrap(await room.importEvents(groupId, events.slice(i, i + IMPORT_SLICE)));
        imported += r.imported;
        skipped += r.skipped;
        lastSeq = r.lastSeq;
        if (events.length === 0) break;
      }
      await audit(env.DB, null, {
        action: "admin.import.group",
        target: groupId,
        details: { imported, skipped, lastSeq },
      });
      log.info("group import", { groupId, imported, skipped, lastSeq });
      return { groupId, imported, skipped, lastSeq };
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/admin/import/groups/:id/state",
    auth: "none",
    async handler({ request, env, params }) {
      await requireAdmin(request, env);
      const bytes = await readBytes(request, MAX_IMPORT_BYTES);
      let snap: unknown;
      try {
        const text = bytes[0] === 0x1f && bytes[1] === 0x8b ? await gunzip(bytes, MAX_IMPORT_TEXT) : new TextDecoder().decode(bytes);
        snap = JSON.parse(text);
      } catch {
        throw new ApiError("invalid", "body is not a gzip JSON state snapshot");
      }
      const parsed = StateSnapshot.safeParse(snap);
      if (!parsed.success) throw new ApiError("invalid", "invalid state snapshot");
      const r = unwrap(await roomStub(env, params.id).rpc(params.id, "ops.importState", null, parsed.data)) as { rows: number };
      await audit(env.DB, null, { action: "admin.import.groupState", target: params.id, details: { rows: r.rows } });
      return { groupId: params.id, rows: r.rows };
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/admin/import/members/:id/state",
    auth: "none",
    async handler({ request, env, params }) {
      await requireAdmin(request, env);
      const bytes = await readBytes(request, MAX_IMPORT_BYTES);
      let snap: unknown;
      try {
        const text = bytes[0] === 0x1f && bytes[1] === 0x8b ? await gunzip(bytes, MAX_IMPORT_TEXT) : new TextDecoder().decode(bytes);
        snap = JSON.parse(text);
      } catch {
        throw new ApiError("invalid", "body is not a gzip JSON gateway snapshot");
      }
      const parsed = GatewaySnapshot.safeParse(snap);
      if (!parsed.success) throw new ApiError("invalid", "invalid gateway snapshot");
      const member = await env.DB.prepare("SELECT id FROM members WHERE id = ?").bind(params.id).first<{ id: string }>();
      if (!member) throw new ApiError("not_found", "no such member");
      const r = (await gatewayStub(env, params.id).rpc("ops.importGateway", parsed.data)) as { rows: number };
      await audit(env.DB, null, { action: "admin.import.memberState", target: params.id, details: { rows: r.rows } });
      return { memberId: params.id, rows: r.rows };
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/admin/import/groups/:id/files",
    auth: "none",
    async handler({ request, env, params }) {
      await requireAdmin(request, env);
      const room = roomStub(env, params.id);
      let rows = 0;
      let after = "";
      for (;;) {
        const page = await env.DB.prepare(
          `SELECT id, topic_id, name, size, mime, sha256, width, height, created_by, created_at FROM files
           WHERE group_id = ? AND status = 'ready' AND deleted_at IS NULL AND id > ? ORDER BY id LIMIT ?`,
        )
          .bind(params.id, after, FILES_PAGE)
          .all<FileRecordD1>();
        if (page.results.length === 0) break;
        const files = page.results.map((f) => ({
          id: f.id,
          topicId: f.topic_id,
          name: f.name,
          size: f.size,
          mime: f.mime,
          sha256: f.sha256,
          width: f.width,
          height: f.height,
          createdBy: f.created_by,
          createdAt: f.created_at,
        }));
        rows += (unwrap(await room.rpc(params.id, "files.importRecords", null, { files })) as { rows: number }).rows;
        after = page.results[page.results.length - 1].id;
        if (page.results.length < FILES_PAGE) break;
      }
      await audit(env.DB, null, { action: "admin.import.groupFiles", target: params.id, details: { rows } });
      return { groupId: params.id, rows };
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/admin/pitr/groups/:id",
    auth: "none",
    async handler({ request, env, params, body }) {
      await requireAdmin(request, env);
      const b = await body(z.object({ at: z.int().positive(), apply: z.boolean().optional() }));
      const now = Date.now();
      if (b.at > now || b.at < now - 30 * 24 * 60 * 60 * 1000) throw new ApiError("invalid", "at must be within the last 30 days");
      const room = roomStub(env, params.id);
      // The restored room will write seqs after its restore point again. Take the newest seq a
      // minute before that point (commit times are close to event times), so no seq the restore
      // keeps is dropped from the archive; re-archiving a few identical seqs is harmless.
      const keep = b.apply
        ? (unwrap(await room.rpc(params.id, "ops.seqAt", null, { at: b.at - PITR_MARGIN_MS })) as { seq: number }).seq
        : 0;
      const r = unwrap(await room.pointInTime(params.id, b.at, b.apply === true));
      let superseded: string[] = [];
      if (r.applied) {
        superseded = await supersedeAfter(env, params.id, keep);
        await audit(env.DB, null, {
          action: "admin.pitr.group",
          target: params.id,
          details: { at: b.at, lastSeqBefore: r.lastSeq, archiveFrom: keep, superseded: superseded.length },
        });
      }
      return { groupId: params.id, ...r, ...(r.applied ? { archiveFrom: keep, superseded } : {}) };
    },
  });

  // Revocations: a D1 restore (Time Travel or an archive dump) goes back to a time before a
  // token was revoked or a member deactivated, and would let a leaked credential work again.
  // Save them before the restore and apply them again after it (docs/RUNBOOK.md).
  registerRoute({
    method: "GET",
    path: "/v1/admin/revocations",
    auth: "none",
    async handler({ request, env }) {
      await requireAdmin(request, env);
      const all = async <T>(sql: string) => (await env.DB.prepare(sql).all<T>()).results;
      return {
        version: 1 as const,
        environment: env.ENVIRONMENT,
        at: Date.now(),
        tokens: await all<{ id: string; at: number }>("SELECT id, revoked_at AS at FROM tokens WHERE revoked_at IS NOT NULL"),
        machines: await all<{ id: string; at: number }>("SELECT id, revoked_at AS at FROM machines WHERE revoked_at IS NOT NULL"),
        hooks: await all<{ id: string; at: number }>("SELECT id, deleted_at AS at FROM hooks WHERE deleted_at IS NOT NULL"),
        members: (await all<{ id: string }>("SELECT id FROM members WHERE deactivated = 1")).map((m) => m.id),
      };
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/admin/revocations",
    auth: "none",
    bodyLimit: REVOCATIONS_BYTES,
    async handler({ request, env, body }) {
      await requireAdmin(request, env);
      const b = await body(Revocations, REVOCATIONS_BYTES);
      const stmts: D1PreparedStatement[] = [];
      for (const t of b.tokens) {
        stmts.push(env.DB.prepare("UPDATE tokens SET revoked_at = ? WHERE id = ? AND revoked_at IS NULL").bind(t.at, t.id));
      }
      for (const m of b.machines) {
        stmts.push(env.DB.prepare("UPDATE machines SET revoked_at = ? WHERE id = ? AND revoked_at IS NULL").bind(m.at, m.id));
      }
      for (const h of b.hooks) {
        stmts.push(env.DB.prepare("UPDATE hooks SET deleted_at = ? WHERE id = ? AND deleted_at IS NULL").bind(h.at, h.id));
      }
      for (const id of b.members) {
        stmts.push(env.DB.prepare("UPDATE members SET deactivated = 1 WHERE id = ? AND deactivated = 0").bind(id));
      }
      const changed: boolean[] = [];
      for (let i = 0; i < stmts.length; i += D1_BATCH) {
        for (const r of await env.DB.batch(stmts.slice(i, i + D1_BATCH))) changed.push((r.meta.changes ?? 0) > 0);
      }
      const applied = changed.filter(Boolean).length;
      // Close the sockets that the restored credentials opened, as the live revoke does.
      const tokenIds = b.tokens.filter((_, i) => changed[i]).map((t) => t.id);
      const byMember = new Map<string, string[]>();
      for (let i = 0; i < tokenIds.length; i += 50) {
        const part = tokenIds.slice(i, i + 50);
        const rows = await env.DB.prepare(`SELECT id, member_id FROM tokens WHERE id IN (${part.map(() => "?").join(", ")})`)
          .bind(...part)
          .all<{ id: string; member_id: string }>();
        for (const r of rows.results) byMember.set(r.member_id, [...(byMember.get(r.member_id) ?? []), r.id]);
      }
      for (const [memberId, ids] of byMember) await gatewayStub(env, memberId).closeSockets({ tokenIds: ids, reason: "token revoked" });
      const offset = b.tokens.length + b.machines.length + b.hooks.length;
      for (const [i, id] of b.members.entries()) {
        if (changed[offset + i]) await gatewayStub(env, id).closeSockets({ reason: "member deactivated" });
      }
      await audit(env.DB, null, { action: "admin.revocations.apply", details: { applied, from: b.environment ?? null } });
      return { applied };
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/admin/import/d1",
    auth: "none",
    async handler({ request, env }) {
      await requireAdmin(request, env);
      const counts: Record<string, number> = {};
      for (const name of await tableNames(env)) {
        if (name === "d1_migrations") continue;
        const r = await env.DB.prepare(`SELECT count(*) AS n FROM ${quote(name)}`).first<{ n: number }>();
        counts[name] = r?.n ?? 0;
      }
      return { environment: env.ENVIRONMENT, tables: counts };
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/admin/import/d1",
    auth: "none",
    async handler({ request, env }) {
      await requireAdmin(request, env);
      const text = await readText(request, D1_IMPORT_BYTES);
      let raw: unknown;
      try {
        raw = JSON.parse(text);
      } catch {
        throw new ApiError("invalid", "body is not valid JSON");
      }
      const parsed = D1Chunk.safeParse(raw);
      if (!parsed.success) throw new ApiError("invalid", "invalid D1 chunk");
      const { table, columns, rows } = parsed.data;
      if (table === "d1_migrations" || table.startsWith("ops_")) throw new ApiError("invalid", "table is not restorable");
      if (!(await tableNames(env)).includes(table)) throw new ApiError("not_found", `no table ${table} in target`);
      const info = await env.DB.prepare("SELECT name FROM pragma_table_info(?)").bind(table).all<{ name: string }>();
      const known = new Set(info.results.map((c) => c.name));
      const missing = columns.filter((c) => !known.has(c));
      if (missing.length > 0) throw new ApiError("invalid", `target table ${table} lacks columns: ${missing.join(", ")}`);
      const sql = `INSERT OR IGNORE INTO ${quote(table)} (${columns.map(quote).join(", ")}) VALUES (${columns.map(() => "?").join(", ")})`;
      let inserted = 0;
      for (let i = 0; i < rows.length; i += D1_BATCH) {
        const stmts = rows.slice(i, i + D1_BATCH).map((row) => {
          if (row.length !== columns.length) throw new ApiError("invalid", "row length does not match columns");
          return env.DB.prepare(sql).bind(...row.map((v) => (v === undefined ? null : v)));
        });
        const results = await env.DB.batch(stmts);
        for (const r of results) inserted += r.meta.changes ?? 0;
      }
      return { table, rows: rows.length, inserted };
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/admin/import/d1/done",
    auth: "none",
    async handler({ request, env, body }) {
      await requireAdmin(request, env);
      const b = await body(z.object({ source: z.string().max(200).optional(), tables: z.int().nonnegative().optional() }));
      await audit(env.DB, null, { action: "admin.import.d1", details: { source: b.source ?? null, tables: b.tables ?? 0 } });
      return { ok: true };
    },
  });
}
