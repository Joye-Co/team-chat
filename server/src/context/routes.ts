// What a run can read of its topic (docs/machine-protocol.md, "Topic context") and what its prompt
// carried (docs/protocol.md, "Run context"). The daemon calls the machine routes for chat-run.
import {
  type Artifact,
  type ArtifactResponse,
  type ContextHit,
  type ContextMessage,
  type DecisionsResponse,
  MAX_RUN_ARTIFACT_TEXT,
  type RunArtifactResponse,
  type RunCardResponse,
  type RunContext,
  type RunDecisionsResponse,
  type SearchHit,
} from "@chat/protocol";
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
import { DECISIONS_RPC } from "../cards2/decisions";
import { type CardRead, groupMembers, stateIds, type TopicStateData, topicStateText } from "./read";
import { type ContextRow, HISTORY_BODY_CHARS, renderMessage } from "./render";

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
    text: renderMessage(r, name, bodyChars, bodyChars > 0 ? `rest: chat-run topic message --id ${r.id}` : ""),
  }));
}

/** Ids that go into SQL and URLs: ULID-like words only. */
const ID_RE = /^[A-Za-z0-9_-]{1,64}$/;

/**
 * The topic a read names with `?topic=`: a topic of the run's own group (never another group:
 * the room is the group's), else the run's topic.
 */
async function readTopic(env: Env, run: { group_id: string; topic_id: string; bot_id: string }, url: URL): Promise<string> {
  const t = url.searchParams.get("topic");
  if (t === null || t === "" || t === run.topic_id) return run.topic_id;
  if (!ID_RE.test(t)) throw new ApiError("invalid", "topic must be a topic id");
  const actor: Actor = { id: run.bot_id, level: "member", kind: "bot" };
  unwrap(await roomStub(env, run.group_id).rpc(run.group_id, "bots.topicExists", actor, { topicId: t }));
  return t;
}

/** Entry files `artifact get` gives as text. */
function isText(mime: string): boolean {
  const m = mime.split(";")[0].trim().toLowerCase();
  return (
    m.startsWith("text/") ||
    ["application/json", "application/xml", "application/javascript", "application/x-yaml", "application/yaml", "application/toml", "image/svg+xml"].includes(m) ||
    m.endsWith("+json") ||
    m.endsWith("+xml")
  );
}

/** Bytes of an entry file read as text at most (the text is then cut to MAX_RUN_ARTIFACT_TEXT characters). */
const MAX_TEXT_BYTES = 2 * 1024 ** 2;

export function registerContextRoutes(): void {
  registerRoute({
    method: "GET",
    path: "/v1/machine/runs/:id/topic/messages",
    auth: "none",
    async handler({ request, env, params, url }) {
      const { run } = await machineRun(request, env, params.id, { read: true });
      const before = intParam(url, "before", 0, Number.MAX_SAFE_INTEGER);
      const after = intParam(url, "after", 0, Number.MAX_SAFE_INTEGER);
      const limit = intParam(url, "limit", 1, 100, 30) as number;
      const actor: Actor = { id: run.bot_id, level: "member", kind: "bot" };
      const topicId = await readTopic(env, run, url);
      const page = unwrap(
        await roomStub(env, run.group_id).rpc(run.group_id, "bots.history", actor, { topicId, before, after, limit }),
      ) as { rows: ContextRow[]; hasMore: boolean };
      return { messages: await toMessages(env, page.rows, HISTORY_BODY_CHARS), hasMore: page.hasMore };
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/machine/runs/:id/topic/messages/:messageId",
    auth: "none",
    async handler({ request, env, params, url }) {
      const { run } = await machineRun(request, env, params.id, { read: true });
      const actor: Actor = { id: run.bot_id, level: "member", kind: "bot" };
      const topicId = await readTopic(env, run, url);
      const res = unwrap(
        await roomStub(env, run.group_id).rpc(run.group_id, "bots.message", actor, { topicId, messageId: params.messageId }),
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
      const { run } = await machineRun(request, env, params.id, { read: true });
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
      const { run } = await machineRun(request, env, params.id, { read: true });
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
    path: "/v1/machine/runs/:id/artifacts",
    auth: "none",
    async handler({ request, env, params, url }) {
      const { run } = await machineRun(request, env, params.id, { read: true });
      const scope = url.searchParams.get("scope") ?? "topic";
      if (scope !== "topic" && scope !== "group") throw new ApiError("invalid", "scope is topic or group");
      const actor: Actor = { id: run.bot_id, level: "member", kind: "bot" };
      const topicId = scope === "topic" ? await readTopic(env, run, url) : undefined;
      const list = unwrap(await roomStub(env, run.group_id).rpc(run.group_id, "artifacts.list", actor, { topicId })) as Artifact[];
      return { artifacts: list.slice(0, 100) };
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/machine/runs/:id/artifacts/:artifactId",
    auth: "none",
    async handler({ request, env, params, url }) {
      const { run } = await machineRun(request, env, params.id, { read: true });
      if (!ID_RE.test(params.artifactId)) throw new ApiError("invalid", "artifactId must be an id");
      const actor: Actor = { id: run.bot_id, level: "member", kind: "bot" };
      // 产出 of another group are not in this group's room: not_found.
      const got = unwrap(await roomStub(env, run.group_id).rpc(run.group_id, "artifacts.get", actor, { artifactId: params.artifactId })) as ArtifactResponse;
      const wanted = intParam(url, "version", 1, 1_000_000_000, got.artifact.currentVersion) as number;
      const v = got.versions.find((x) => x.version === wanted);
      if (!v) throw new ApiError("not_found", "version not found");
      const out: RunArtifactResponse = { artifact: got.artifact, versions: got.versions, version: v.version };
      const entry = v.files.find((f) => f.path === v.entry);
      if (entry && isText(entry.mime)) {
        const obj = entry.size <= MAX_TEXT_BYTES ? await env.FILES.get(fileKey(run.group_id, entry.fileId)) : null;
        if (obj) {
          const chars = [...(await obj.text())];
          out.text = chars.slice(0, MAX_RUN_ARTIFACT_TEXT).join("");
          if (chars.length > MAX_RUN_ARTIFACT_TEXT) out.truncated = true;
        } else if (entry.size > MAX_TEXT_BYTES) {
          out.truncated = true;
        }
      }
      return out;
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/machine/runs/:id/cards/:messageId",
    auth: "none",
    async handler({ request, env, params }) {
      const { run } = await machineRun(request, env, params.id, { read: true });
      if (!ID_RE.test(params.messageId)) throw new ApiError("invalid", "messageId must be an id");
      const actor: Actor = { id: run.bot_id, level: "member", kind: "bot" };
      const res = unwrap(await roomStub(env, run.group_id).rpc(run.group_id, "context.card", actor, { messageId: params.messageId })) as CardRead;
      const state = (res.row.ext.cardState ?? {}) as {
        ask?: { status: string; from?: string[] };
        answers?: { actorId: string; at?: number; value?: unknown; inputs?: unknown; retractedAt?: number }[];
        answerCount?: number;
      };
      const answers = (state.answers ?? []).filter((a) => a.retractedAt === undefined);
      const name = await names(env, [...new Set([...answers.map((a) => a.actorId), ...(state.ask?.from ?? [])])]);
      const [message] = await toMessages(env, [res.row], 0);
      type Json = RunCardResponse["card"];
      const out: RunCardResponse = {
        topicId: res.topicId,
        message,
        card: res.card as Json,
        answers: answers.map((a) => ({
          actorId: a.actorId,
          actorName: name(a.actorId),
          ...(a.at !== undefined ? { at: a.at } : {}),
          ...(a.value !== undefined ? { value: a.value as Json } : {}),
          ...(a.inputs !== undefined ? { inputs: a.inputs as Json } : {}),
        })),
      };
      if (state.ask) out.ask = { status: state.ask.status, from: (state.ask.from ?? []).map(name) };
      if (state.answerCount !== undefined) out.answerCount = state.answerCount;
      return out;
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/machine/runs/:id/decisions",
    auth: "none",
    async handler({ request, env, params, url }) {
      const { run } = await machineRun(request, env, params.id, { read: true });
      const limit = intParam(url, "limit", 1, 100, 20) as number;
      const actor: Actor = { id: run.bot_id, level: "member", kind: "bot" };
      const res = unwrap(await roomStub(env, run.group_id).rpc(run.group_id, DECISIONS_RPC, actor, { limit })) as DecisionsResponse;
      const recorded = res.recorded ?? [];
      const name = await names(env, [...new Set([...res.decisions.map((d) => d.answeredBy), ...recorded.map((r) => r.recordedBy)])]);
      const out: RunDecisionsResponse = {
        decisions: res.decisions.map((d) => ({ ...d, answeredByName: name(d.answeredBy) })),
        hasMore: res.hasMore,
      };
      if (recorded.length > 0) out.recorded = recorded.map((r) => ({ ...r, recordedByName: name(r.recordedBy) }));
      return out;
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/machine/runs/:id/members",
    auth: "none",
    async handler({ request, env, params }) {
      const { run } = await machineRun(request, env, params.id, { read: true });
      return { members: await groupMembers(env.DB, run.group_id) };
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/machine/runs/:id/topic/state",
    auth: "none",
    async handler({ request, env, params, url }) {
      const { run } = await machineRun(request, env, params.id, { read: true });
      const actor: Actor = { id: run.bot_id, level: "member", kind: "bot" };
      const topicId = await readTopic(env, run, url);
      const data = unwrap(await roomStub(env, run.group_id).rpc(run.group_id, "context.topicState", actor, { topicId })) as TopicStateData;
      const name = await names(env, [...new Set(stateIds(data))]);
      return { topicId, title: data.title, text: topicStateText(data, name) };
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
