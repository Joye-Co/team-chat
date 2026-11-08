// `/v1/memories` (docs/protocol.md, "Memory"): list, write, versions, restore, the daily
// 记忆更新 and its undo; and the run's memory note (`POST /v1/machine/runs/:id/memories`).
import {
  CreateMemoryRequest,
  MachineMemoryNoteRequest,
  type MemoriesResponse,
  type MemoryItem,
  type MemoryUpdate,
  type MemoryUpdatesResponse,
  type MemoryVersionsResponse,
  UpdateMemoryRequest,
} from "@chat/protocol";
import type { AuthContext } from "../core/auth";
import { getGroup } from "../core/db";
import { ApiError } from "../core/errors";
import { registerRoute } from "../core/registry";
import { machineRun } from "../bots/routes";
import { type Who, requireAccess } from "./access";
import { type MemoryChange, type MemoryRow, createMemory, getRow, getUpdate, saveChange, toItem, toUpdate } from "./store";

/** Notes one run may write. */
export const MAX_NOTES_PER_RUN = 10;

function who(auth: AuthContext | null): Who {
  const a = auth as AuthContext;
  return { memberId: a.memberId, level: a.member.level, kind: a.member.kind };
}

function intParam(url: URL, name: string, min: number, max: number, fallback: number): number {
  const v = url.searchParams.get(name);
  if (v === null || v === "") return fallback;
  const n = Number(v);
  if (!Number.isInteger(n) || n < min || n > max) throw new ApiError("invalid", `${name} must be an integer ${min}..${max}`);
  return n;
}

/** The memory, when the caller may read (and, with `write`, change) its scope. */
async function visible(db: D1Database, w: Who, id: string, write: boolean): Promise<MemoryRow> {
  const row = await getRow(db, id);
  if (!row) throw new ApiError("not_found", "memory not found");
  await requireAccess(db, w, row.scope, write);
  return row;
}

export function registerMemoryRoutes(): void {
  registerRoute({
    method: "GET",
    path: "/v1/memories",
    auth: "member",
    async handler({ env, auth, url }): Promise<MemoriesResponse> {
      const scope = url.searchParams.get("scope") ?? "";
      if (scope === "") throw new ApiError("invalid", "scope is required");
      await requireAccess(env.DB, who(auth), scope, false);
      const status = url.searchParams.get("status") ?? "active";
      if (!["active", "pending", "retired", "all"].includes(status)) throw new ApiError("invalid", "status is active, pending, retired or all");
      const q = (url.searchParams.get("q") ?? "").trim().toLowerCase().slice(0, 200);
      const limit = intParam(url, "limit", 1, 500, 200);
      const rows = await env.DB.prepare(
        `SELECT * FROM memories WHERE scope = ?1 AND (?2 = 'all' OR status = ?2) AND (?3 = '' OR instr(lower(text), ?3) > 0 OR instr(lower(coalesce(when_text, '')), ?3) > 0)
         ORDER BY updated_at DESC, id DESC LIMIT ?4`,
      )
        .bind(scope, status, q, limit)
        .all<MemoryRow>();
      return { memories: rows.results.map(toItem) };
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/memories",
    auth: "member",
    async handler({ env, auth, body }): Promise<MemoryItem> {
      const req = await body(CreateMemoryRequest);
      const w = who(auth);
      await requireAccess(env.DB, w, req.scope, true);
      // A person's memory counts at once; an AI member's waits for the daily consolidation.
      const human = w.kind === "human";
      return createMemory(
        env.DB,
        {
          scope: req.scope,
          kind: req.kind,
          text: req.text,
          when: req.when,
          source: req.source ?? { kind: "manual", ids: [] },
          authorId: w.memberId,
          human,
          status: human ? "active" : "pending",
          confidence: human ? 1 : 0.6,
        },
        w.memberId,
      );
    },
  });

  // Before `/v1/memories/:id`: the first matching route wins.
  registerRoute({
    method: "GET",
    path: "/v1/memories/updates",
    auth: "member",
    async handler({ env, auth, url }): Promise<MemoryUpdatesResponse> {
      const scope = url.searchParams.get("scope") ?? "";
      if (scope === "") throw new ApiError("invalid", "scope is required");
      await requireAccess(env.DB, who(auth), scope, false);
      let day = url.searchParams.get("day");
      if (day !== null && !/^\d{4}-\d{2}-\d{2}$/.test(day)) throw new ApiError("invalid", "day is YYYY-MM-DD");
      if (day === null) {
        const last = await env.DB.prepare("SELECT max(day) AS day FROM memory_updates WHERE scope = ?").bind(scope).first<{ day: string | null }>();
        day = last?.day ?? null;
      }
      if (day === null) return { day: null, updates: [] };
      const rows = await env.DB.prepare("SELECT * FROM memory_updates WHERE scope = ? AND day = ? ORDER BY at, id LIMIT 500").bind(scope, day).all();
      return { day, updates: rows.results.map((r) => toUpdate(r as never)) };
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/memories/updates/:id/undo",
    auth: "member",
    async handler({ env, auth, params }): Promise<MemoryUpdate> {
      const w = who(auth);
      const u = await getUpdate(env.DB, params.id);
      if (!u) throw new ApiError("not_found", "update not found");
      await requireAccess(env.DB, w, u.scope, true);
      if (u.undone_at !== null) throw new ApiError("conflict", "already undone");
      if (u.change === "flagged") throw new ApiError("invalid", "a flagged memory was not changed");
      const row = await getRow(env.DB, u.memory_id);
      if (!row) throw new ApiError("not_found", "memory not found");
      const before = u.before_json ? (JSON.parse(u.before_json) as MemoryItem) : null;
      let change: MemoryChange;
      if (u.change === "added") change = { status: "retired" };
      else if (u.change === "retired") change = { status: before && before.status !== "retired" ? before.status : "active" };
      else if (before) change = { kind: before.kind, text: before.text, when: before.when ?? null, note: before.note ?? null, status: before.status };
      else throw new ApiError("invalid", "nothing to put back");
      const now = Date.now();
      await saveChange(env.DB, row, change, w.memberId, now);
      // Only the first undo counts.
      const res = await env.DB.prepare("UPDATE memory_updates SET undone_at = ? WHERE id = ? AND undone_at IS NULL").bind(now, u.id).run();
      if (res.meta.changes === 0) throw new ApiError("conflict", "already undone");
      return toUpdate({ ...u, undone_at: now });
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/memories/:id",
    auth: "member",
    async handler({ env, auth, params }): Promise<MemoryItem> {
      return toItem(await visible(env.DB, who(auth), params.id, false));
    },
  });

  registerRoute({
    method: "PATCH",
    path: "/v1/memories/:id",
    auth: "member",
    async handler({ env, auth, params, body }): Promise<MemoryItem> {
      const req = await body(UpdateMemoryRequest);
      const w = who(auth);
      const row = await visible(env.DB, w, params.id, true);
      const change: MemoryChange = { kind: req.kind, text: req.text, when: req.when, status: req.status };
      // A person's edit of the content makes the memory theirs: the consolidation leaves it alone.
      const edits = req.kind !== undefined || req.text !== undefined || req.when !== undefined;
      if (edits && w.kind === "human") change.human = true;
      // An AI member cannot activate its own pending memory.
      if (w.kind !== "human" && req.status === "active" && row.status === "pending") delete change.status;
      return toItem(await saveChange(env.DB, row, change, w.memberId));
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/memories/:id/versions",
    auth: "member",
    async handler({ env, auth, params }): Promise<MemoryVersionsResponse> {
      const row = await visible(env.DB, who(auth), params.id, false);
      const rows = await env.DB.prepare("SELECT * FROM memory_versions WHERE memory_id = ? ORDER BY version DESC LIMIT 200")
        .bind(row.id)
        .all<{ memory_id: string; version: number; snapshot: string; by_id: string | null; at: number }>();
      return {
        versions: rows.results.map((v) => ({ memoryId: v.memory_id, version: v.version, item: JSON.parse(v.snapshot) as MemoryItem, by: v.by_id, at: v.at })),
      };
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/memories/:id/versions/:version/restore",
    auth: "member",
    async handler({ env, auth, params }): Promise<MemoryItem> {
      const w = who(auth);
      const row = await visible(env.DB, w, params.id, true);
      const version = Number(params.version);
      if (!Number.isInteger(version) || version < 1) throw new ApiError("invalid", "version must be a positive integer");
      const v = await env.DB.prepare("SELECT snapshot FROM memory_versions WHERE memory_id = ? AND version = ?").bind(row.id, version).first<{ snapshot: string }>();
      if (!v) throw new ApiError("not_found", "version not found");
      const old = JSON.parse(v.snapshot) as MemoryItem;
      const change: MemoryChange = { kind: old.kind, text: old.text, when: old.when ?? null, note: old.note ?? null };
      if (old.status !== "pending") change.status = old.status;
      if (w.kind === "human") change.human = true;
      return toItem(await saveChange(env.DB, row, change, w.memberId));
    },
  });

  // A run keeps something for later (chat-run memory note): always pending, the run as source.
  registerRoute({
    method: "POST",
    path: "/v1/machine/runs/:id/memories",
    auth: "none",
    async handler({ request, env, params, body }): Promise<MemoryItem> {
      const { run, info } = await machineRun(request, env, params.id);
      const req = await body(MachineMemoryNoteRequest);
      let scope: string;
      if (req.scope === "member") scope = `member:${run.bot_id}`;
      else if (req.scope === "person") {
        if (!info?.requestedBy) throw new ApiError("invalid", "the run has no requester");
        const person = await env.DB.prepare("SELECT kind FROM members WHERE id = ?").bind(info.requestedBy).first<{ kind: string }>();
        if (person?.kind !== "human") throw new ApiError("invalid", "the run was not asked for by a person");
        scope = `person:${info.requestedBy}`;
      } else {
        // A direct conversation is private: what it says goes to the member or the person only.
        const group = await getGroup(env.DB, run.group_id);
        if (!group || group.kind !== "group") throw new ApiError("forbidden", "a direct conversation has no group memory");
        scope = `group:${run.group_id}`;
      }
      const notes = await env.DB.prepare("SELECT count(*) AS n FROM memories WHERE author_id = ? AND json_extract(source, '$.kind') = 'run' AND json_extract(source, '$.ids[0]') = ?")
        .bind(run.bot_id, run.id)
        .first<{ n: number }>();
      if ((notes?.n ?? 0) >= MAX_NOTES_PER_RUN) throw new ApiError("rate_limited", `at most ${MAX_NOTES_PER_RUN} memory notes per run`);
      return createMemory(
        env.DB,
        { scope, kind: req.kind, text: req.text, when: req.when, source: { kind: "run", ids: [run.id] }, authorId: run.bot_id, human: false, status: "pending", confidence: 0.6 },
        run.bot_id,
      );
    },
  });
}
