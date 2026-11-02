// Admin routes for the archive and for restore. All need the X-Admin-Secret header.
//   POST /v1/admin/archive/run                 run the daily archive now
//   GET  /v1/admin/archive?prefix=&cursor=     list archive objects of this env's bucket
//   GET  /v1/admin/archive/object?key=         one archive object (gzip bytes)
//   POST /v1/admin/import/groups/:id           gzip NDJSON of archived events -> empty GroupRoom
//   GET  /v1/admin/import/d1                   row counts per D1 table (restore pre-check)
//   POST /v1/admin/import/d1                   { table, columns, rows } -> INSERT OR IGNORE
//   POST /v1/admin/import/d1/done              records the D1 restore in the audit log
import { z } from "zod";
import { audit } from "../core/audit";
import { timingSafeEqualString } from "../core/crypto";
import { type Env, roomStub } from "../core/env";
import { ApiError, unwrap } from "../core/errors";
import { readText } from "../core/http";
import { type ArchivedEvent, registerRoute } from "../core/registry";
import { runArchive } from "./archive";
import { gunzip } from "./gzip";

const MAX_IMPORT_BYTES = 32 * 1024 * 1024;
const MAX_IMPORT_TEXT = 256 * 1024 * 1024;
const IMPORT_SLICE = 1000;
const D1_IMPORT_BYTES = 8 * 1024 * 1024;
const D1_BATCH = 200;

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
        details: { files: report.files.length, events: report.events, complete: report.complete },
      });
      return report;
    },
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
