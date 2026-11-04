// The two-way 产出 bridge (INTERACT §5.4, V11-INC R5, R9, R10): page events (`artifact.event`)
// go to the version's creator and to the audit, never to the group log; the creator bot pushes
// one latest data snapshot per version (`artifact.data`, HTTP `…/data`), shared by every viewer;
// it answers a page `ask` once, within 120 s, to the asker only (`artifact.reply`); and a member
// attaches a version's preview later (`artifact.previewReady`, stored).
import {
  ArtifactDataFrame,
  type ArtifactEventDecl,
  ArtifactEventFrame,
  ArtifactReplyFrame,
  type ClientFrameOf,
  type Expect,
  MAX_ARTIFACT_DATA_INLINE_BYTES,
  PutArtifactMediaRequest,
  type PutArtifactMediaResponse,
  PutArtifactDataRequest,
  utf8Bytes,
} from "@chat/protocol";
import type { AuditEntry } from "../core/audit";
import { auditValue } from "../core/auditvalue";
import type { AuthContext } from "../core/auth";
import { sha256Hex } from "../core/crypto";
import { isGroupMember } from "../core/db";
import { type Env, roomStub } from "../core/env";
import { ApiError, unwrap } from "../core/errors";
import { featureTransient } from "../core/gateway";
import { ulid } from "../core/ids";
import { TokenBucket } from "../core/ratelimit";
import { type Actor, type RoomContext, registerFrameHandler, registerRoomRpc, registerRoute } from "../core/registry";
import { deliverPageEvent, INTERACT_FEATURE } from "../bots/interact";
import { markPageResolved } from "./page-events";
import { artifactRow, checkMedia, toVersion, type VersionRow, versionRow } from "./room";
import { MAX_PATTERN_INPUT, patternProblem, patternTest } from "./pattern";
import { actorOf } from "./store";

export const ARTIFACT_EVENT_RPC = "artifacts.event";
export const ARTIFACT_DATA_RPC = "artifacts.data";
export const ARTIFACT_GET_DATA_RPC = "artifacts.getData";
export const ARTIFACT_REPLY_RPC = "artifacts.reply";
export const ARTIFACT_PREVIEW_RPC = "artifacts.previewReady";

/** Page events and asks, per viewer per version: 5/s, bursts of 10, 300 per hour (§5.6). */
const PAGE_EVENT_RATE = { rate: 5, burst: 10 };
const PAGE_EVENT_HOURLY = 300;
/** Data pushes per version: 2/s, bursts of 4. */
const DATA_RATE = { rate: 2, burst: 4 };
/** A reply must come within this long of its event. */
export const REPLY_WINDOW_MS = 120_000;
/** Idempotency of page events, and how long they are kept. */
const KEEP_EVENTS_MS = 24 * 3600_000;
/** A `cause` is an event of the same 产出 version from the last 30 minutes. */
const CAUSE_WINDOW_MS = 30 * 60_000;

const buckets = new Map<string, TokenBucket[]>();

function takeRate(key: string, limits: TokenBucket[] | (() => TokenBucket[])): boolean {
  let list = buckets.get(key);
  if (!list) {
    list = typeof limits === "function" ? limits() : limits;
    buckets.set(key, list);
    if (buckets.size > 10_000) buckets.delete(buckets.keys().next().value as string);
  }
  return list.every((b) => b.take());
}

type PageEventRow = {
  event_id: string;
  artifact_id: string;
  version: number;
  actor_id: string;
  bot_id: string;
  kind: string;
  client_event_id: string | null;
  ack: string;
  at: number;
  reply_hash: string | null;
  replied_at: number | null;
};

/** The audit insert of a room write, with the actor's credential when it has one. */
function auditOf(room: RoomContext, a: Actor, entry: AuditEntry, at: number): D1PreparedStatement {
  return room.env.DB.prepare("INSERT INTO audit (actor_id, credential_id, action, target, details, at) VALUES (?, ?, ?, ?, ?, ?)").bind(
    a.id,
    a.tokenId ?? null,
    entry.action,
    entry.target ?? null,
    JSON.stringify(entry.details ?? {}),
    at,
  );
}

function requireActor(actor: Actor | null): Actor {
  if (!actor) throw new ApiError("internal", "actor required");
  return actor;
}

function versionOf(room: RoomContext, artifactId: string, version: number): { v: VersionRow; topicId: string; title: string } {
  const art = artifactRow(room, artifactId);
  const v = art ? versionRow(room, artifactId, version) : undefined;
  if (!art || !v) throw new ApiError("not_found", "产出 version not found");
  return { v, topicId: art.topic_id, title: art.title };
}

/** The version's bridge is on when a bot member of the group, not deactivated, made it (R10). */
async function bridgeOpen(room: RoomContext, createdBy: string): Promise<boolean> {
  if (!room.isMember(createdBy)) return false;
  const m = await room.env.DB.prepare("SELECT kind, deactivated FROM members WHERE id = ?")
    .bind(createdBy)
    .first<{ kind: string; deactivated: number }>();
  return Boolean(m && m.kind === "bot" && !m.deactivated);
}

// ---------------------------------------------------------------------------
// TcSchema (INTERACT §5.3): a JSON Schema subset, checked against page payloads
// ---------------------------------------------------------------------------

type TcSchema = {
  type?: string | string[];
  properties?: Record<string, TcSchema>;
  required?: string[];
  enum?: unknown[];
  items?: TcSchema;
  maxItems?: number;
  minimum?: number;
  maximum?: number;
  maxLength?: number;
  pattern?: string;
};

function typeOf(v: unknown): string {
  if (v === null) return "null";
  if (Array.isArray(v)) return "array";
  if (typeof v === "number") return Number.isInteger(v) ? "integer" : "number";
  return typeof v;
}

/** The first way `value` breaks `schema` (with its JSON path), or undefined. */
export function tcSchemaViolation(schema: TcSchema, value: unknown, path = "payload"): string | undefined {
  if (schema.type !== undefined) {
    const want = Array.isArray(schema.type) ? schema.type : [schema.type];
    const have = typeOf(value);
    if (!want.some((t) => t === have || (t === "number" && have === "integer"))) return `${path} is not ${want.join(" or ")}`;
  }
  if (schema.enum && !schema.enum.some((e) => JSON.stringify(e) === JSON.stringify(value))) return `${path} is not one of the allowed values`;
  if (typeof value === "string") {
    if (schema.maxLength !== undefined && [...value].length > schema.maxLength) return `${path} is too long`;
    if (schema.pattern !== undefined) {
      // Linear time, never a RegExp on the viewer's string (SEC-4; files/pattern.ts).
      if (patternProblem(schema.pattern)) return `${path} pattern is not supported`;
      if ([...value].length > MAX_PATTERN_INPUT) return `${path} is too long for the pattern`;
      if (!patternTest(schema.pattern, value)) return `${path} does not match the pattern`;
    }
  }
  if (typeof value === "number") {
    if (schema.minimum !== undefined && value < schema.minimum) return `${path} is below the minimum`;
    if (schema.maximum !== undefined && value > schema.maximum) return `${path} is above the maximum`;
  }
  if (Array.isArray(value)) {
    if (schema.maxItems !== undefined && value.length > schema.maxItems) return `${path} has too many items`;
    if (schema.items) {
      for (let i = 0; i < value.length; i++) {
        const e = tcSchemaViolation(schema.items, value[i], `${path}[${i}]`);
        if (e) return e;
      }
    }
  }
  if (value && typeof value === "object" && !Array.isArray(value)) {
    const obj = value as Record<string, unknown>;
    for (const k of schema.required ?? []) if (!(k in obj)) return `${path}.${k} is required`;
    for (const [k, sub] of Object.entries(schema.properties ?? {})) {
      if (k in obj) {
        const e = tcSchemaViolation(sub, obj[k], `${path}.${k}`);
        if (e) return e;
      }
    }
  }
  return undefined;
}

// ---------------------------------------------------------------------------
// Page events
// ---------------------------------------------------------------------------

type PageEventArgs = { artifactId: string; version: number; name: string; kind?: "emit" | "ask"; payload?: unknown; clientEventId?: string };
export type PageEventAck = { eventId: string; duplicate?: boolean; expect: Expect };

registerRoomRpc(ARTIFACT_EVENT_RPC, async (room, actor, args): Promise<PageEventAck> => {
  const a = requireActor(actor);
  const e = args as PageEventArgs;
  if (!room.isMember(a.id)) throw new ApiError("forbidden", "not a member of this group");
  // People only (D11): a bot never emits.
  if (a.kind !== "human") throw new ApiError("forbidden", "only people send page events", "interact.bot_emitter");
  const { v, topicId, title } = versionOf(room, e.artifactId, e.version);
  const now = Date.now();
  const repeat = (): PageEventAck | undefined => {
    if (!e.clientEventId) return undefined;
    const seen = room.sql
      .exec<PageEventRow>(
        "SELECT * FROM page_events WHERE actor_id = ? AND artifact_id = ? AND version = ? AND client_event_id = ? AND at > ?",
        a.id,
        e.artifactId,
        e.version,
        e.clientEventId,
        now - KEEP_EVENTS_MS,
      )
      .toArray()[0] as PageEventRow | undefined;
    return seen ? { ...(JSON.parse(seen.ack) as PageEventAck), duplicate: true } : undefined;
  };
  const first = repeat();
  if (first) return first;
  if (!(await bridgeOpen(room, v.created_by))) throw new ApiError("forbidden", "this page sends nothing back", "artifact.no_bridge");
  const declared = v.events ? (JSON.parse(v.events) as ArtifactEventDecl[]) : [];
  const decl = declared.find((d) => d.name === e.name);
  const kind = e.kind ?? "emit";
  if (!decl || (decl.kind ?? "emit") !== kind) {
    throw new ApiError("invalid", "the page does not declare this event", "artifact.undeclared_event");
  }
  if (decl.schema !== undefined) {
    const problem = tcSchemaViolation(decl.schema as TcSchema, e.payload ?? null);
    if (problem) throw new ApiError("invalid", problem, "artifact.bad_payload");
  }
  const rateKey = `page:${room.groupId}:${a.id}:${e.artifactId}:${e.version}`;
  if (!takeRate(rateKey, () => [new TokenBucket(PAGE_EVENT_RATE.rate, PAGE_EVENT_RATE.burst), new TokenBucket(PAGE_EVENT_HOURLY / 3600, PAGE_EVENT_HOURLY)])) {
    throw new ApiError("rate_limited", "too many page events");
  }
  // The row exists before the event goes out (F3): a fast reply or data push finds it, and a
  // repeat that comes while delivery waits is a duplicate. Same order as cards2 (insertEvent,
  // then finishAck).
  const again = repeat();
  if (again) return again;
  const eventId = ulid(now);
  room.sql.exec("DELETE FROM page_events WHERE at < ?", now - KEEP_EVENTS_MS);
  room.sql.exec(
    `INSERT INTO page_events (event_id, artifact_id, version, actor_id, bot_id, kind, client_event_id, ack, at, name)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    eventId,
    e.artifactId,
    e.version,
    a.id,
    v.created_by,
    kind,
    e.clientEventId ?? null,
    JSON.stringify({ eventId, expect: { by: v.created_by, mode: "none" } } satisfies PageEventAck),
    now,
    e.name,
  );
  let expect: Expect;
  try {
    expect = await deliverPageEvent(room, v.created_by, {
      eventId,
      topicId,
      artifactId: e.artifactId,
      version: e.version,
      name: e.name,
      kind,
      payload: e.payload,
      actorId: a.id,
      at: now,
      title,
      runId: v.run_id ?? null,
    });
  } catch (err) {
    // The event did not go out: remove the placeholder so a resend with the same clientEventId
    // delivers it instead of a duplicate with mode none.
    room.sql.exec("DELETE FROM page_events WHERE event_id = ?", eventId);
    throw err;
  }
  const ack: PageEventAck = { eventId, expect };
  room.sql.exec("UPDATE page_events SET ack = ? WHERE event_id = ?", JSON.stringify(ack), eventId);
  const details: Record<string, unknown> = { groupId: room.groupId, artifactId: e.artifactId, version: e.version, name: e.name, kind, eventId };
  const payload = auditValue(e.payload);
  if (payload !== undefined) details.payload = payload;
  room.waitUntil(
    auditOf(room, a, { action: "artifact.event", target: e.artifactId, details }, now)
      .run()
      .catch((err) => room.log.warn("page event audit failed", { err })),
  );
  return ack;
});

registerFrameHandler<ClientFrameOf<"artifact.event">>("artifact.event", {
  schema: ArtifactEventFrame as never,
  rate: "write",
  async handle(frame, c) {
    await c.requireMember(frame.groupId);
    const { artifactId, version, name, kind, payload, clientEventId } = frame;
    return unwrap(
      await roomStub(c.env, frame.groupId).rpc(frame.groupId, ARTIFACT_EVENT_RPC, c.actor, { artifactId, version, name, kind, payload, clientEventId }),
    );
  },
});

// ---------------------------------------------------------------------------
// Data snapshots
// ---------------------------------------------------------------------------

type DataArgs = { artifactId: string; version: number; data: unknown; baseDataRev?: number; cause?: string; clientPushId?: string };

/** Stores a new snapshot of the version's data and tells every member (the creator bot only). */
registerRoomRpc(ARTIFACT_DATA_RPC, (room, actor, args): { dataRev: number; duplicate?: boolean } => {
  const a = requireActor(actor);
  const d = args as DataArgs;
  if (!room.isMember(a.id)) throw new ApiError("forbidden", "not a member of this group");
  const { v } = versionOf(room, d.artifactId, d.version);
  if (v.created_by !== a.id || a.kind !== "bot") throw new ApiError("forbidden", "only the bot that made the version pushes its data");
  const cur = room.sql
    .exec<{ data_rev: number; push_id: string | null }>("SELECT data_rev, push_id FROM artifact_data WHERE artifact_id = ? AND version = ?", d.artifactId, d.version)
    .toArray()[0];
  const rev = cur?.data_rev ?? 0;
  if (d.clientPushId && cur?.push_id === d.clientPushId) return { dataRev: rev, duplicate: true };
  if (d.baseDataRev !== undefined && d.baseDataRev !== rev) throw new ApiError("conflict", "the data changed", "artifact.stale_data");
  const now = Date.now();
  let causeActor: string | undefined;
  if (d.cause !== undefined) {
    const cause = room.sql
      .exec<{ actor_id: string }>(
        "SELECT actor_id FROM page_events WHERE event_id = ? AND artifact_id = ? AND version = ? AND at > ?",
        d.cause,
        d.artifactId,
        d.version,
        now - CAUSE_WINDOW_MS,
      )
      .toArray()[0];
    if (!cause) throw new ApiError("invalid", "cause is not a recent event of this page", "interact.bad_cause");
    causeActor = cause.actor_id;
  }
  if (!takeRate(`data:${room.groupId}:${d.artifactId}:${d.version}`, () => [new TokenBucket(DATA_RATE.rate, DATA_RATE.burst)])) {
    throw new ApiError("rate_limited", "too many data pushes for this version");
  }
  const text = JSON.stringify(d.data);
  const next = rev + 1;
  room.sql.exec(
    "INSERT OR REPLACE INTO artifact_data (artifact_id, version, data, data_rev, push_id, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
    d.artifactId,
    d.version,
    text,
    next,
    d.clientPushId ?? null,
    now,
  );
  room.sql.exec("UPDATE artifact_versions SET data_rev = ? WHERE artifact_id = ? AND version = ?", next, d.artifactId, d.version);
  const bytes = utf8Bytes(text);
  const event: Record<string, unknown> = { type: "artifact.data", groupId: room.groupId, artifactId: d.artifactId, version: d.version, dataRev: next };
  // Inline up to 64 KiB; larger snapshots are read with GET …/data.
  if (bytes <= MAX_ARTIFACT_DATA_INLINE_BYTES) event.data = d.data;
  if (d.cause !== undefined) event.cause = d.cause;
  room.broadcast(featureTransient(INTERACT_FEATURE, event));
  // A push that names the event answers it: the emitter's pending state clears (INTERACT §4.3).
  if (d.cause !== undefined && causeActor) {
    markPageResolved(room.sql, d.cause, "replied");
    room.broadcast(featureTransient(INTERACT_FEATURE, { type: "interaction.resolved", groupId: room.groupId, eventId: d.cause, result: "replied" }), { to: [causeActor] });
  }
  room.waitUntil(
    auditOf(room, a, { action: "artifact.data", target: d.artifactId, details: { groupId: room.groupId, version: d.version, dataRev: next, bytes } }, now)
      .run()
      .catch((err) => room.log.warn("data audit failed", { err })),
  );
  return { dataRev: next };
});

registerRoomRpc(ARTIFACT_GET_DATA_RPC, (room, actor, args) => {
  const a = requireActor(actor);
  const { artifactId, version } = args as { artifactId: string; version: number };
  if (!room.isMember(a.id)) throw new ApiError("forbidden", "not a member of this group");
  const { v } = versionOf(room, artifactId, version);
  const row = room.sql
    .exec<{ data: string; data_rev: number; updated_at: number }>("SELECT data, data_rev, updated_at FROM artifact_data WHERE artifact_id = ? AND version = ?", artifactId, version)
    .toArray()[0];
  if (!row) return { data: null, dataRev: 0, updatedAt: v.created_at };
  return { data: JSON.parse(row.data) as unknown, dataRev: row.data_rev, updatedAt: row.updated_at };
});

registerFrameHandler<ClientFrameOf<"artifact.data">>("artifact.data", {
  schema: ArtifactDataFrame as never,
  rate: "write",
  async handle(frame, c) {
    await c.requireMember(frame.groupId);
    const { artifactId, version, data, baseDataRev, cause, clientPushId } = frame;
    const out = unwrap(
      await roomStub(c.env, frame.groupId).rpc(frame.groupId, ARTIFACT_DATA_RPC, c.actor, { artifactId, version, data, baseDataRev, cause, clientPushId }),
    ) as { dataRev: number };
    return { dataRev: out.dataRev };
  },
});

// ---------------------------------------------------------------------------
// Replies to page asks
// ---------------------------------------------------------------------------

type ReplyArgs = { eventId: string; ok: boolean; payload?: unknown; error?: { code: string; message: string } };

registerRoomRpc(ARTIFACT_REPLY_RPC, async (room, actor, args): Promise<{ duplicate?: boolean }> => {
  const a = requireActor(actor);
  const r = args as ReplyArgs;
  if (!room.isMember(a.id)) throw new ApiError("forbidden", "not a member of this group");
  const ev = room.sql.exec<PageEventRow>("SELECT * FROM page_events WHERE event_id = ?", r.eventId).toArray()[0] as PageEventRow | undefined;
  if (!ev) throw new ApiError("not_found", "no such page event");
  // Only the member that received the event answers it.
  if (ev.bot_id !== a.id) throw new ApiError("forbidden", "only the receiver of the event answers it");
  if (ev.kind !== "ask") throw new ApiError("invalid", "the event is not an ask", "artifact.not_ask");
  const body = r.ok ? { ok: true, payload: r.payload ?? null } : { ok: false, error: r.error };
  const hash = await sha256Hex(JSON.stringify(body));
  if (ev.replied_at !== null) {
    if (ev.reply_hash === hash) return { duplicate: true };
    throw new ApiError("conflict", "the event has its answer", "artifact.reply_closed");
  }
  const now = Date.now();
  if (now - ev.at > REPLY_WINDOW_MS) throw new ApiError("conflict", "the page stopped waiting", "artifact.reply_closed");
  room.sql.exec("UPDATE page_events SET reply_hash = ?, replied_at = ?, resolved = coalesce(resolved, 'replied') WHERE event_id = ?", hash, now, ev.event_id);
  const reply: Record<string, unknown> = { type: "artifact.reply", groupId: room.groupId, eventId: ev.event_id, ok: r.ok };
  if (r.ok && r.payload !== undefined) reply.payload = r.payload;
  if (!r.ok) reply.error = r.error;
  // Private to the asker (D12).
  room.broadcast(featureTransient(INTERACT_FEATURE, reply), { to: [ev.actor_id] });
  room.broadcast(
    featureTransient(INTERACT_FEATURE, { type: "interaction.resolved", groupId: room.groupId, eventId: ev.event_id, result: "replied" }),
    { to: [ev.actor_id] },
  );
  return {};
});

registerFrameHandler<ClientFrameOf<"artifact.reply">>("artifact.reply", {
  schema: ArtifactReplyFrame as never,
  rate: "write",
  async handle(frame, c) {
    await c.requireMember(frame.groupId);
    const { eventId, ok, payload, error } = frame;
    const out = unwrap(await roomStub(c.env, frame.groupId).rpc(frame.groupId, ARTIFACT_REPLY_RPC, c.actor, { eventId, ok, payload, error })) as {
      duplicate?: boolean;
    };
    return out.duplicate ? { duplicate: true } : undefined;
  },
});

// ---------------------------------------------------------------------------
// Previews attached later (stored `artifact.previewReady`)
// ---------------------------------------------------------------------------

/**
 * Sets a version's media after the upload. The version's creator always may; another member
 * only while the stored preview is not ready (the fallback of V11-MEDIA §3).
 */
registerRoomRpc(ARTIFACT_PREVIEW_RPC, (room, actor, args) => {
  const a = requireActor(actor);
  const { artifactId, version, media: raw } = args as { artifactId: string; version: number; media: unknown };
  if (!room.isMember(a.id)) throw new ApiError("forbidden", "not a member of this group");
  const { v, topicId } = versionOf(room, artifactId, version);
  const media = checkMedia(room, raw);
  const stored = v.media ? (JSON.parse(v.media) as { preview?: string }) : undefined;
  if (v.created_by !== a.id && stored?.preview === "ready") throw new ApiError("conflict", "the version has its preview", "artifact.preview_ready");
  return room.commit((tx) => {
    room.sql.exec("UPDATE artifact_versions SET media = ? WHERE artifact_id = ? AND version = ?", JSON.stringify(media), artifactId, version);
    const seq = tx.nextSeq();
    tx.append({ type: "artifact.previewReady", actorId: a.id, topicId, data: { artifactId, version, media } });
    room.sql.exec("UPDATE topics SET last_seq = ? WHERE id = ?", seq, topicId);
    return { version: toVersion(versionRow(room, artifactId, version) as VersionRow) };
  });
});

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

async function artifactGroup(env: Env, id: string, auth: AuthContext): Promise<string> {
  const row = await env.DB.prepare("SELECT group_id FROM artifacts WHERE id = ?").bind(id).first<{ group_id: string }>();
  if (!row || !(await isGroupMember(env.DB, row.group_id, auth.memberId))) throw new ApiError("not_found", "产出 not found");
  return row.group_id;
}

function versionParam(v: string): number {
  if (!/^[1-9][0-9]{0,8}$/.test(v)) throw new ApiError("invalid", "bad version");
  return Number(v);
}

registerRoute({
  method: "GET",
  path: "/v1/artifacts/:id/versions/:v/data",
  auth: "member",
  async handler({ env, params, auth }) {
    const a = auth as AuthContext;
    const version = versionParam(params.v);
    const groupId = await artifactGroup(env, params.id, a);
    return unwrap(await roomStub(env, groupId).rpc(groupId, ARTIFACT_GET_DATA_RPC, actorOf(a), { artifactId: params.id, version }));
  },
});

registerRoute({
  method: "PUT",
  path: "/v1/artifacts/:id/versions/:v/data",
  auth: "member",
  bodyLimit: 300 * 1024,
  async handler({ env, params, body, auth }) {
    const a = auth as AuthContext;
    const version = versionParam(params.v);
    const groupId = await artifactGroup(env, params.id, a);
    const req = await body(PutArtifactDataRequest);
    const out = unwrap(
      await roomStub(env, groupId).rpc(groupId, ARTIFACT_DATA_RPC, actorOf(a), {
        artifactId: params.id,
        version,
        data: req.data,
        baseDataRev: req.baseDataRev,
        cause: req.cause,
      }),
    ) as { dataRev: number };
    return { dataRev: out.dataRev };
  },
});


registerRoute({
  method: "PUT",
  path: "/v1/artifacts/:id/versions/:v/media",
  auth: "member",
  async handler({ env, params, body, auth }) {
    const a = auth as AuthContext;
    const version = versionParam(params.v);
    const groupId = await artifactGroup(env, params.id, a);
    const req = await body(PutArtifactMediaRequest);
    return unwrap(
      await roomStub(env, groupId).rpc(groupId, ARTIFACT_PREVIEW_RPC, actorOf(a), { artifactId: params.id, version, media: req.media }),
    ) as PutArtifactMediaResponse;
  },
});
