// What a run can read of its topic (docs/machine-protocol.md, "Topic context") and what its prompt
// carried (docs/protocol.md, "Run context"). The daemon calls the machine routes for chat-run.
// A run reads its own group and the other groups of the person it works for (context/reach.ts,
// docs/plans/usable/design-visibility.zh.md 3.3).
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
  type RunGroupsResponse,
  type SearchHit,
} from "@chat/protocol";
import type { Env } from "../core/env";
import { roomStub } from "../core/env";
import { type GroupRow, groupMemberIds, viewGroup } from "../core/db";
import { ApiError, unwrap } from "../core/errors";
import type { Actor } from "../core/registry";
import { registerRoute } from "../core/registry";
import { logger } from "../core/logger";
import { machineRun } from "../bots/routes";
import { memberInfo } from "../bots/room";
import { deskStub } from "../bots/env";
import { runIndex } from "../bots/store";
import { fileKey } from "../files/routes";
import { serveObject } from "../files/store";
import { newestFirst, ROOM_SEARCH_RPC } from "../search/frame";
import { DECISIONS_RPC } from "../cards2/decisions";
import { type CardRead, groupMembers, stateIds, type TopicStateData, topicStateText } from "./read";
import { type ContextRow, HISTORY_BODY_CHARS, renderMessage } from "./render";
import { teamZone } from "../memory/consolidate";
import { readableGroups, requireReadable } from "./reach";

const log = logger.child({ obj: "context" });

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

/** Ids that go into SQL and URLs: ULID-like words only. */
const ID_RE = /^[A-Za-z0-9_-]{1,64}$/;

/** A machine read's run, its AI member as the viewer, and the person it reads for (server only). */
type Reader = Awaited<ReturnType<typeof machineRun>> & { actor: Actor };

async function reader(request: Request, env: Env, runId: string): Promise<Reader> {
  const r = await machineRun(request, env, runId, { read: true });
  return { ...r, actor: { id: r.run.bot_id, level: "member", kind: "bot" } };
}

/**
 * A room read of group `gid` (design-visibility 3.2): the run's own group through `rpc` as before;
 * another group, checked readable by the caller, through `readAs` with the AI member as viewer.
 */
async function readRoom(env: Env, rd: Reader, gid: string, name: string, args: unknown): Promise<unknown> {
  const stub = roomStub(env, gid);
  return unwrap(gid === rd.run.group_id ? await stub.rpc(gid, name, rd.actor, args) : await stub.readAs(gid, name, rd.actor, args));
}

/** Remembers a successful read of another group on the run (design-visibility 3.3). */
async function noteRead(env: Env, rd: Reader, gids: Iterable<string>): Promise<void> {
  const desk = deskStub(env, rd.run.bot_id);
  for (const gid of new Set(gids)) if (gid !== rd.run.group_id) await desk.noteRead(rd.run.bot_id, rd.run.id, gid);
}

/** Names of groups of kind "group" (other groups a run reads are never DMs). */
async function groupNames(env: Env, gids: string[]): Promise<Map<string, string>> {
  if (gids.length === 0) return new Map();
  const rows = await env.DB.prepare("SELECT id, name FROM groups WHERE id IN (SELECT value FROM json_each(?)) AND deleted_at IS NULL")
    .bind(JSON.stringify(gids))
    .all<{ id: string; name: string }>();
  return new Map(rows.results.map((r) => [r.id, r.name]));
}

/** `groupId`/`groupName` of a result from another group; nothing for the run's own group. */
async function groupTag(env: Env, rd: Reader, gid: string): Promise<{ groupId?: string; groupName?: string }> {
  if (gid === rd.run.group_id) return {};
  const name = (await groupNames(env, [gid])).get(gid);
  return name !== undefined ? { groupId: gid, groupName: name } : { groupId: gid };
}

/** `?group=`: a group the run may read (`not_found` otherwise), else the run's own group. */
async function readGroup(env: Env, rd: Reader, url: URL): Promise<string> {
  const g = url.searchParams.get("group");
  if (g === null || g === "" || g === rd.run.group_id) return rd.run.group_id;
  if (!ID_RE.test(g)) throw new ApiError("invalid", "group must be a group id");
  await requireReadable(env.DB, rd.run, rd.forId, g);
  return g;
}

/**
 * The first readable group whose room answers `name`: the run's own group first, then the other
 * groups it may read in parallel; `not_found` when none does. Ids of topics and cards work across
 * the groups a run may read.
 */
async function findIn<T>(env: Env, rd: Reader, name: string, args: unknown): Promise<{ gid: string; value: T }> {
  const own = await roomStub(env, rd.run.group_id).rpc(rd.run.group_id, name, rd.actor, args);
  if (own.ok) return { gid: rd.run.group_id, value: own.value as T };
  if (own.error.code !== "not_found") unwrap(own);
  const others = [...(await readableGroups(env.DB, rd.run, rd.forId))].filter((g) => g !== rd.run.group_id);
  const found = await Promise.all(
    others.map(async (gid) => {
      try {
        const r = await roomStub(env, gid).readAs(gid, name, rd.actor, args);
        return r.ok ? { gid, value: r.value as T } : null;
      } catch (err) {
        log.warn("read of another group failed", { err, groupId: gid });
        return null;
      }
    }),
  );
  const hit = found.find((x) => x !== null);
  if (!hit) throw new ApiError("not_found", "not found");
  return hit;
}

/**
 * The group and topic a read names with `?group=` and `?topic=`. With another group: a topic of
 * it (`topic` is then required, unless `topicOptional`). Without: the run's topic, or `topic` in
 * the run's own group, else in the other groups the run may read.
 */
async function readTopic(env: Env, rd: Reader, url: URL, opts: { topicOptional?: boolean } = {}): Promise<{ gid: string; topicId?: string }> {
  const { run } = rd;
  const t = url.searchParams.get("topic");
  const named = t !== null && t !== "";
  if (named && !ID_RE.test(t)) throw new ApiError("invalid", "topic must be a topic id");
  const gid = await readGroup(env, rd, url);
  if (gid !== run.group_id) {
    if (!named) {
      if (opts.topicOptional) return { gid };
      throw new ApiError("invalid", "topic is required with another group");
    }
    await readRoom(env, rd, gid, "bots.topicExists", { topicId: t });
    return { gid, topicId: t };
  }
  if (!named || t === run.topic_id) return { gid, topicId: run.topic_id };
  if (url.searchParams.get("group")) {
    await readRoom(env, rd, gid, "bots.topicExists", { topicId: t });
    return { gid, topicId: t };
  }
  return { gid: (await findIn(env, rd, "bots.topicExists", { topicId: t })).gid, topicId: t };
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

/** Search hits of the groups a run may read, newest first (as search/frame.ts merges a person's groups). */
async function searchAll(env: Env, rd: Reader, args: { query: string; limit: number }): Promise<SearchHit[]> {
  const gids = [...(await readableGroups(env.DB, rd.run, rd.forId))];
  const results = await Promise.all(
    gids.map(async (gid) => {
      try {
        const stub = roomStub(env, gid);
        return { gid, r: gid === rd.run.group_id ? await stub.rpc(gid, ROOM_SEARCH_RPC, rd.actor, args) : await stub.readAs(gid, ROOM_SEARCH_RPC, rd.actor, args) };
      } catch (err) {
        log.error("search room call failed", { err, groupId: gid });
        return { gid, r: null };
      }
    }),
  );
  const hits: SearchHit[] = [];
  let failed = 0;
  for (const { gid, r } of results) {
    if (r?.ok) hits.push(...(r.value as SearchHit[]).map((h) => ({ ...h, groupId: gid })));
    else if (r && (r.error.code === "forbidden" || r.error.code === "not_found")) continue;
    else {
      failed++;
      if (r) log.warn("search room refused", { groupId: gid, code: r.error.code });
    }
  }
  if (failed > 0 && failed === gids.length) throw new ApiError("internal", "search failed");
  hits.sort(newestFirst);
  return hits.slice(0, args.limit);
}

export function registerContextRoutes(): void {
  registerRoute({
    method: "GET",
    path: "/v1/machine/runs/:id/topic/messages",
    auth: "none",
    async handler({ request, env, params, url }) {
      const rd = await reader(request, env, params.id);
      const before = intParam(url, "before", 0, Number.MAX_SAFE_INTEGER);
      const after = intParam(url, "after", 0, Number.MAX_SAFE_INTEGER);
      const limit = intParam(url, "limit", 1, 100, 30) as number;
      const { gid, topicId } = await readTopic(env, rd, url);
      const page = (await readRoom(env, rd, gid, "bots.history", { topicId, before, after, limit })) as { rows: ContextRow[]; hasMore: boolean };
      await noteRead(env, rd, [gid]);
      return { messages: await toMessages(env, page.rows, HISTORY_BODY_CHARS), hasMore: page.hasMore };
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/machine/runs/:id/topic/messages/:messageId",
    auth: "none",
    async handler({ request, env, params, url }) {
      const rd = await reader(request, env, params.id);
      if (!ID_RE.test(params.messageId)) throw new ApiError("invalid", "messageId must be an id");
      // In another group without `topic`, the message's own topic.
      const { gid, topicId } = await readTopic(env, rd, url, { topicOptional: true });
      const res = (await readRoom(env, rd, gid, "bots.message", { topicId, messageId: params.messageId })) as { row: ContextRow };
      await noteRead(env, rd, [gid]);
      const [message] = await toMessages(env, [res.row], 0);
      return { message };
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/machine/runs/:id/search",
    auth: "none",
    async handler({ request, env, params, url }) {
      const rd = await reader(request, env, params.id);
      const { run } = rd;
      const query = (url.searchParams.get("q") ?? "").trim();
      if (query === "") throw new ApiError("invalid", "q is required");
      const scope = url.searchParams.get("scope") ?? "topic";
      if (scope !== "topic" && scope !== "group" && scope !== "all") throw new ApiError("invalid", "scope is topic, group or all");
      const g = url.searchParams.get("group");
      if (g !== null && g !== "" && scope !== "group") throw new ApiError("invalid", "group goes with scope group");
      const limit = intParam(url, "limit", 1, 50, 20) as number;
      let hits: SearchHit[];
      if (scope === "all") {
        hits = await searchAll(env, rd, { query, limit });
      } else {
        const gid = scope === "group" ? await readGroup(env, rd, url) : run.group_id;
        const args = { query, limit, ...(scope === "topic" ? { topicId: run.topic_id } : {}) };
        hits = ((await readRoom(env, rd, gid, ROOM_SEARCH_RPC, args)) as SearchHit[]).map((h) => ({ ...h, groupId: gid }));
      }
      const others = [...new Set(hits.map((h) => h.groupId).filter((x) => x !== run.group_id))];
      await noteRead(env, rd, others);
      const [name, group] = await Promise.all([names(env, [...new Set(hits.map((h) => h.authorId))]), groupNames(env, others)]);
      const out: ContextHit[] = hits.map((h) => ({
        messageId: h.messageId,
        topicId: h.topicId,
        seq: h.seq,
        authorName: name(h.authorId),
        at: h.createdAt,
        snippet: h.snippet,
        ...(h.groupId !== run.group_id ? { groupId: h.groupId, ...(group.has(h.groupId) ? { groupName: group.get(h.groupId) } : {}) } : {}),
      }));
      return { hits: out };
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/machine/runs/:id/files/:fileId",
    auth: "none",
    async handler({ request, env, params }) {
      const rd = await reader(request, env, params.id);
      const row = await env.DB.prepare("SELECT id, group_id, name, size, mime, status, deleted_at FROM files WHERE id = ?")
        .bind(params.fileId)
        .first<{ id: string; group_id: string; name: string; size: number; mime: string; status: string; deleted_at: number | null }>();
      if (!row || row.deleted_at || row.status !== "ready") throw new ApiError("not_found", "file not found");
      // Files of groups the run may not read look like unknown ones.
      await requireReadable(env.DB, rd.run, rd.forId, row.group_id);
      if (row.size > MAX_RUN_FILE_BYTES) throw new ApiError("too_large", "file is larger than 100 MiB");
      await noteRead(env, rd, [row.group_id]);
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
      const rd = await reader(request, env, params.id);
      const scope = url.searchParams.get("scope") ?? "topic";
      if (scope !== "topic" && scope !== "group") throw new ApiError("invalid", "scope is topic or group");
      const at = scope === "topic" ? await readTopic(env, rd, url) : { gid: await readGroup(env, rd, url), topicId: undefined };
      const list = (await readRoom(env, rd, at.gid, "artifacts.list", { topicId: at.topicId })) as Artifact[];
      await noteRead(env, rd, [at.gid]);
      return { artifacts: list.slice(0, 100) };
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/machine/runs/:id/artifacts/:artifactId",
    auth: "none",
    async handler({ request, env, params, url }) {
      const rd = await reader(request, env, params.id);
      if (!ID_RE.test(params.artifactId)) throw new ApiError("invalid", "artifactId must be an id");
      // The 产出's group from D1; one without an index row is looked up in the run's own group.
      const ix = await env.DB.prepare("SELECT group_id FROM artifacts WHERE id = ?").bind(params.artifactId).first<{ group_id: string }>();
      const gid = ix?.group_id ?? rd.run.group_id;
      await requireReadable(env.DB, rd.run, rd.forId, gid);
      const got = (await readRoom(env, rd, gid, "artifacts.get", { artifactId: params.artifactId })) as ArtifactResponse;
      const wanted = intParam(url, "version", 1, 1_000_000_000, got.artifact.currentVersion) as number;
      const v = got.versions.find((x) => x.version === wanted);
      if (!v) throw new ApiError("not_found", "version not found");
      await noteRead(env, rd, [gid]);
      const out: RunArtifactResponse = { artifact: got.artifact, versions: got.versions, version: v.version };
      const entry = v.files.find((f) => f.path === v.entry);
      if (entry && isText(entry.mime)) {
        const obj = entry.size <= MAX_TEXT_BYTES ? await env.FILES.get(fileKey(gid, entry.fileId)) : null;
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
      const rd = await reader(request, env, params.id);
      if (!ID_RE.test(params.messageId)) throw new ApiError("invalid", "messageId must be an id");
      const { gid, value: res } = await findIn<CardRead>(env, rd, "context.card", { messageId: params.messageId });
      await noteRead(env, rd, [gid]);
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
        ...(await groupTag(env, rd, gid)),
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
      const rd = await reader(request, env, params.id);
      const limit = intParam(url, "limit", 1, 100, 20) as number;
      const gid = await readGroup(env, rd, url);
      const res = (await readRoom(env, rd, gid, DECISIONS_RPC, { limit })) as DecisionsResponse;
      await noteRead(env, rd, [gid]);
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
    async handler({ request, env, params, url }) {
      const rd = await reader(request, env, params.id);
      const gid = await readGroup(env, rd, url);
      const members = await groupMembers(env.DB, gid);
      await noteRead(env, rd, [gid]);
      return { members };
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/machine/runs/:id/groups",
    auth: "none",
    async handler({ request, env, params }) {
      const rd = await reader(request, env, params.id);
      const ids = [...(await readableGroups(env.DB, rd.run, rd.forId))];
      const rows = (
        await env.DB.prepare("SELECT * FROM groups WHERE id IN (SELECT value FROM json_each(?)) AND deleted_at IS NULL").bind(JSON.stringify(ids)).all<GroupRow>()
      ).results;
      const byId = new Map(rows.map((r) => [r.id, r]));
      const groups: RunGroupsResponse["groups"] = [];
      for (const id of ids) {
        const row = byId.get(id);
        if (!row) continue;
        let name = row.name;
        // The run's own group may be a DM: named as its AI member sees it (viewGroup).
        if (row.kind === "dm") {
          const memberIds = await groupMemberIds(env.DB, id);
          const info = await memberInfo(env.DB, memberIds);
          name = viewGroup(row, memberIds, rd.run.bot_id, new Map([...info].map(([k, v]) => [k, v.name]))).name;
        }
        groups.push({ id, name, kind: row.kind, ...(row.purpose ? { purpose: row.purpose } : {}), current: id === rd.run.group_id });
      }
      return { groups } satisfies RunGroupsResponse;
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/machine/runs/:id/topic/state",
    auth: "none",
    async handler({ request, env, params, url }) {
      const rd = await reader(request, env, params.id);
      const { gid, topicId } = await readTopic(env, rd, url);
      const data = (await readRoom(env, rd, gid, "context.topicState", { topicId })) as TopicStateData;
      await noteRead(env, rd, [gid]);
      const name = await names(env, [...new Set(stateIds(data))]);
      return { topicId: topicId as string, ...(await groupTag(env, rd, gid)), title: data.title, text: topicStateText(data, name) };
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
