// What a run can read of its topic (docs/machine-protocol.md, "Topic context") and what its prompt
// carried (docs/protocol.md, "Run context"). The daemon calls the machine routes for chat-run.
import type { ContextHit, ContextMessage, RunContext, SearchHit } from "@chat/protocol";
import type { Env } from "../core/env";
import { roomStub } from "../core/env";
import { ApiError, unwrap } from "../core/errors";
import type { Actor } from "../core/registry";
import { registerRoute } from "../core/registry";
import { machineRun } from "../bots/routes";
import { memberInfo } from "../bots/room";
import { deskStub } from "../bots/env";
import { runIndex } from "../bots/store";
import { fileKey } from "../files/routes";
import { serveObject } from "../files/store";
import { ROOM_SEARCH_RPC } from "../search/frame";
import { type ContextRow, HISTORY_BODY_CHARS, renderMessage } from "./render";
import { teamZone } from "../memory/consolidate";

/** Files a run may fetch (machine-protocol.md: larger ones are `too_large`). */
export const MAX_RUN_FILE_BYTES = 100 * 1024 ** 2;

function intParam(url: URL, name: string, min: number, max: number, fallback?: number): number | undefined {
  const v = url.searchParams.get(name);
  if (v === null || v === "") return fallback;
  const n = Number(v);
  if (!Number.isInteger(n) || n < min || n > max) throw new ApiError("invalid", `${name} must be an integer ${min}..${max}`);
  return n;
}

async function names(env: Env, ids: string[]): Promise<(id: string) => string> {
  const info = ids.length > 0 ? await memberInfo(env.DB, ids) : new Map();
  return (id: string) => info.get(id)?.name ?? id;
}

function rowIds(r: ContextRow): string[] {
  return [r.authorId, ...(r.ext.replyTo ? [r.ext.replyTo.authorId] : []), ...(r.ext.cardState?.answers ?? []).map((a) => a.actorId)];
}

async function toMessages(env: Env, rows: ContextRow[], bodyChars: number): Promise<ContextMessage[]> {
  const name = await names(env, [...new Set(rows.flatMap(rowIds))]);
  return rows.map((r) => ({
    id: r.id,
    seq: r.seq,
    authorId: r.authorId,
    authorName: name(r.authorId),
    at: r.at,
    text: renderMessage(r, name, bodyChars, bodyChars > 0 ? `rest: chat-run topic message --id ${r.id}` : "", teamZone(env)),
  }));
}

export function registerContextRoutes(): void {
  registerRoute({
    method: "GET",
    path: "/v1/machine/runs/:id/topic/messages",
    auth: "none",
    async handler({ request, env, params, url }) {
      const { run } = await machineRun(request, env, params.id);
      const before = intParam(url, "before", 0, Number.MAX_SAFE_INTEGER);
      const after = intParam(url, "after", 0, Number.MAX_SAFE_INTEGER);
      const limit = intParam(url, "limit", 1, 100, 30) as number;
      const actor: Actor = { id: run.bot_id, level: "member", kind: "bot" };
      const page = unwrap(
        await roomStub(env, run.group_id).rpc(run.group_id, "bots.history", actor, { topicId: run.topic_id, before, after, limit }),
      ) as { rows: ContextRow[]; hasMore: boolean };
      return { messages: await toMessages(env, page.rows, HISTORY_BODY_CHARS), hasMore: page.hasMore };
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/machine/runs/:id/topic/messages/:messageId",
    auth: "none",
    async handler({ request, env, params }) {
      const { run } = await machineRun(request, env, params.id);
      const actor: Actor = { id: run.bot_id, level: "member", kind: "bot" };
      const res = unwrap(
        await roomStub(env, run.group_id).rpc(run.group_id, "bots.message", actor, { topicId: run.topic_id, messageId: params.messageId }),
      ) as { row: ContextRow };
      const [message] = await toMessages(env, [res.row], 0);
      return { message };
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/machine/runs/:id/search",
    auth: "none",
    async handler({ request, env, params, url }) {
      const { run } = await machineRun(request, env, params.id);
      const query = (url.searchParams.get("q") ?? "").trim();
      if (query === "") throw new ApiError("invalid", "q is required");
      const scope = url.searchParams.get("scope") ?? "topic";
      if (scope !== "topic" && scope !== "group") throw new ApiError("invalid", "scope is topic or group");
      const limit = intParam(url, "limit", 1, 50, 20) as number;
      const actor: Actor = { id: run.bot_id, level: "member", kind: "bot" };
      const hits = unwrap(
        await roomStub(env, run.group_id).rpc(run.group_id, ROOM_SEARCH_RPC, actor, {
          query,
          limit,
          ...(scope === "topic" ? { topicId: run.topic_id } : {}),
        }),
      ) as SearchHit[];
      const name = await names(env, [...new Set(hits.map((h) => h.authorId))]);
      const out: ContextHit[] = hits.map((h) => ({
        messageId: h.messageId,
        topicId: h.topicId,
        seq: h.seq,
        authorName: name(h.authorId),
        at: h.createdAt,
        snippet: h.snippet,
      }));
      return { hits: out };
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/machine/runs/:id/files/:fileId",
    auth: "none",
    async handler({ request, env, params }) {
      const { run } = await machineRun(request, env, params.id);
      const row = await env.DB.prepare("SELECT id, group_id, name, size, mime, status, deleted_at FROM files WHERE id = ?")
        .bind(params.fileId)
        .first<{ id: string; group_id: string; name: string; size: number; mime: string; status: string; deleted_at: number | null }>();
      // Files of other groups look like unknown ones.
      if (!row || row.deleted_at || row.group_id !== run.group_id || row.status !== "ready") throw new ApiError("not_found", "file not found");
      if (row.size > MAX_RUN_FILE_BYTES) throw new ApiError("too_large", "file is larger than 100 MiB");
      return serveObject(request, env.FILES, fileKey(row.group_id, row.id), {
        size: row.size,
        mime: row.mime,
        disposition: "attachment",
        headers: { "x-file-name": encodeURIComponent(row.name) },
      });
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/runs/:id/context",
    auth: "owner",
    async handler({ env, params }) {
      const row = await runIndex(env.DB, params.id);
      if (!row) throw new ApiError("not_found", "run not found");
      const ctx = (await deskStub(env, row.bot_id).runContext(row.bot_id, row.id)) as RunContext | null;
      if (!ctx) throw new ApiError("not_found", "no context kept for this run");
      return ctx;
    },
  });
}
