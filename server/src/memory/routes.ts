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
import type { Env } from "../core/env";
import { getGroup } from "../core/db";
import { roomStub } from "../core/env";
import { ApiError, unwrap } from "../core/errors";
import { type Actor, registerRoute } from "../core/registry";
import { machineRun } from "../bots/routes";
import { type Who, canReadSource, readableSql, requireAccess } from "./access";
import { countUses, readableMemories, search } from "./inject";
import { type MemoryChange, type MemoryRow, createMemory, getRow, getUpdate, saveChange, toItem, toUpdate } from "./store";

/** Notes one run may write. */
export const MAX_NOTES_PER_RUN = 10;
/** Memories one `chat-run memory search` returns at most. */
export const MAX_MEMORY_SEARCH_LIMIT = 50;

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
  await requireAccess(db, w, row.scope, false);
  if (!(await canReadSource(db, w, row))) throw new ApiError("not_found", "memory not found");
  if (write) await requireAccess(db, w, row.scope, true);
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
      const w = who(auth);
      await requireAccess(env.DB, w, scope, false);
      const status = url.searchParams.get("status") ?? "active";
      if (!["active", "pending", "retired", "all"].includes(status)) throw new ApiError("invalid", "status is active, pending, retired or all");
      const q = (url.searchParams.get("q") ?? "").trim().toLowerCase().slice(0, 200);
      const limit = intParam(url, "limit", 1, 500, 200);
      const rows = await env.DB.prepare(
        `SELECT * FROM memories m WHERE scope = ?1 AND (?2 = 'all' OR status = ?2) AND (?3 = '' OR instr(lower(text), ?3) > 0 OR instr(lower(coalesce(when_text, '')), ?3) > 0)
           AND ${readableSql("m", 5, 6)}
         ORDER BY updated_at DESC, id DESC LIMIT ?4`,
      )
        .bind(scope, status, q, limit, w.memberId, w.level === "owner" ? 1 : 0)
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
      const w = who(auth);
      await requireAccess(env.DB, w, scope, false);
      // Lines of memories the caller may not read are left out, the day too when all of them are.
      const readable = (p: number) => `EXISTS (SELECT 1 FROM memories m WHERE m.id = u.memory_id AND ${readableSql("m", p, p + 1)})`;
      const reader = [w.memberId, w.level === "owner" ? 1 : 0];
      let day = url.searchParams.get("day");
      if (day !== null && !/^\d{4}-\d{2}-\d{2}$/.test(day)) throw new ApiError("invalid", "day is YYYY-MM-DD");
      if (day === null) {
        const last = await env.DB.prepare(`SELECT max(day) AS day FROM memory_updates u WHERE scope = ?1 AND ${readable(2)}`)
          .bind(scope, ...reader)
          .first<{ day: string | null }>();
        day = last?.day ?? null;
      }
      if (day === null) return { day: null, updates: [] };
      const rows = await env.DB.prepare(`SELECT * FROM memory_updates u WHERE scope = ?1 AND day = ?2 AND ${readable(3)} ORDER BY at, id LIMIT 500`)
        .bind(scope, day, ...reader)
        .all();
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
      await requireAccess(env.DB, w, u.scope, false);
      const row = await getRow(env.DB, u.memory_id);
      if (!row || !(await canReadSource(env.DB, w, row))) throw new ApiError("not_found", "memory not found");
      await requireAccess(env.DB, w, u.scope, true);
      if (u.undone_at !== null) throw new ApiError("conflict", "already undone");
      if (u.change === "flagged") throw new ApiError("invalid", "a flagged memory was not changed");
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

  // A person releases an AI member's memory learned in a group to everyone who reads its scope.
  registerRoute({
    method: "POST",
    path: "/v1/memories/:id/release",
    auth: "member",
    async handler({ env, auth, params }): Promise<MemoryItem> {
      const w = who(auth);
      // Readers of its group (owners for one of an unknown group) who may also change the scope.
      const row = await visible(env.DB, w, params.id, true);
      if (w.kind !== "human") throw new ApiError("forbidden", "only a person releases a memory");
      return toItem(await saveChange(env.DB, row, { fromGroup: null }, w.memberId));
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

  // A run searches what its prompt could carry (chat-run memory search): the same audience, every
  // item that matches, best first. What it gets back counts as used.
  registerRoute({
    method: "GET",
    path: "/v1/machine/runs/:id/memories",
    auth: "none",
    async handler({ request, env, params, url }): Promise<MemoriesResponse> {
      const { run, info } = await machineRun(request, env, params.id, { read: true });
      const q = (url.searchParams.get("q") ?? "").trim().slice(0, 500);
      if (q === "") throw new ApiError("invalid", "q is required");
      const limit = intParam(url, "limit", 1, MAX_MEMORY_SEARCH_LIMIT, 10);
      const rows = await readableMemories(env.DB, { botId: run.bot_id, groupId: run.group_id, requesterId: info?.requestedBy ?? null });
      const hits = search(rows, q).slice(0, limit);
      const now = Date.now();
      await countUses(env.DB, hits.map((r) => r.id), now);
      return { memories: hits.map((r) => toItem({ ...r, uses: r.uses + 1, last_used_at: now })) };
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
      // What a run learns about itself stays with the group (or direct conversation) it learned it in.
      let scope = `member:${run.bot_id}`;
      let fromGroup: string | null = run.group_id;
      const ids = [run.id];
      if (req.scope === "member") {
        // the default above
      } else if (req.scope === "person") {
        if (!info?.requestedBy) throw new ApiError("invalid", "the run has no requester");
        const person = await env.DB.prepare("SELECT kind FROM members WHERE id = ?").bind(info.requestedBy).first<{ kind: string }>();
        if (person?.kind !== "human") throw new ApiError("invalid", "the run was not asked for by a person");
        scope = `person:${info.requestedBy}`;
        fromGroup = null;
      } else {
        // A direct conversation is private: what it says goes to the member or the person only.
        const group = await getGroup(env.DB, run.group_id);
        if (!group || group.kind !== "group") throw new ApiError("forbidden", "a direct conversation has no group memory");
        // A group convention comes from a person's message in the group; anything else (a web page,
        // a file, the AI member's own idea) stays in the member's own memory of this group.
        if (req.sourceMessageId && (await sourceAuthor(env, run, req.sourceMessageId)) === "human") {
          scope = `group:${run.group_id}`;
          fromGroup = null;
          ids.push(req.sourceMessageId);
        }
      }
      const notes = await env.DB.prepare("SELECT count(*) AS n FROM memories WHERE author_id = ? AND json_extract(source, '$.kind') = 'run' AND json_extract(source, '$.ids[0]') = ?")
        .bind(run.bot_id, run.id)
        .first<{ n: number }>();
      if ((notes?.n ?? 0) >= MAX_NOTES_PER_RUN) throw new ApiError("rate_limited", `at most ${MAX_NOTES_PER_RUN} memory notes per run`);
      return createMemory(
        env.DB,
        // ids[0] stays the run: the per-run limit counts by it.
        { scope, kind: req.kind, text: req.text, when: req.when, source: { kind: "run", ids }, authorId: run.bot_id, human: false, status: "pending", confidence: 0.6, fromGroup },
        run.bot_id,
      );
    },
  });
}

/** The kind of the author of a message in the run's group; null when there is no such message. */
async function sourceAuthor(env: Env, run: { bot_id: string; group_id: string }, messageId: string): Promise<string | null> {
  const actor: Actor = { id: run.bot_id, level: "member", kind: "bot" };
  const m = unwrap(await roomStub(env, run.group_id).rpc(run.group_id, "bots.request", actor, { messageId })) as { authorId: string } | null;
  if (!m) return null;
  const author = await env.DB.prepare("SELECT kind FROM members WHERE id = ?").bind(m.authorId).first<{ kind: string }>();
  return author?.kind ?? null;
}
