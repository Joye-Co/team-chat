// MemberGateway: one per member. Holds the member's WebSockets (hibernatable), routes frames
// to group rooms over RPC, delivers events, stores read cursors, reports presence.
import { DurableObject } from "cloudflare:workers";
import {
  ClientFrame,
  type ClientFrameOf,
  type GroupSummary,
  type Member,
  MAX_CARD_ANSWER_BYTES,
  MAX_CARD_BYTES,
  MAX_FRAME_BYTES,
  PROTOCOL_VERSION,
  SERVER_FEATURES,
  type ServerFrame,
} from "@chat/protocol";
import { getMember, groupsOfMember, isGroupMember, listMembers, toMember, viewGroup } from "./db";
import { type Env, gatewayStub, roomStub } from "./env";
import type { WriteOrder } from "./room";
import { ApiError, schemaReason, toErrorBody, unwrap } from "./errors";
import { logger as baseLogger, type Logger } from "./logger";
import { type MigrationStep, runMigrations } from "./migrations";
import { LIMITS, TokenBucket } from "./ratelimit";
import {
  type Actor,
  type FrameContext,
  type GatewayContext,
  type RateClass,
  frameHandlers,
  frameValidators,
  gatewayMigrations,
  gatewayRpcHandlers,
} from "./registry";

export const GATEWAY_CORE_MIGRATIONS: MigrationStep[] = [
  {
    name: "core-1",
    up: `
      CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
      CREATE TABLE cursors (group_id TEXT PRIMARY KEY, seq INTEGER NOT NULL, updated_at INTEGER NOT NULL);
      CREATE TABLE memberships (group_id TEXT PRIMARY KEY);
    `,
  },
];

export function gatewayMigrationSteps(): MigrationStep[] {
  return [...GATEWAY_CORE_MIGRATIONS, ...gatewayMigrations];
}

/** Per-socket state; survives hibernation as the socket attachment. */
export interface SocketState {
  memberId: string;
  tokenId: string;
  level: Actor["level"];
  kind: Actor["kind"];
  ready: boolean;
  connectedAt: number;
  /** Socket id for write order; older attachments have none (see `socketId`). */
  sid?: string;
  client?: { name: string; version: string };
  /** 1.1: `hello.features` of this socket (e.g. "interact/1"). */
  features?: string[];
  /** 1.1: `hello.supportedCatalogs` of this socket (e.g. "tc.card/2.1"). */
  catalogs?: string[];
}

/**
 * Key of a frame that only sockets with this `hello.features` entry get (1.1 transients such as
 * `card.event` or `artifact.data`). The gateway removes the key before it sends the frame.
 */
export const ONLY_FEATURE = "onlyFeature";

/** Wraps a transient event so only sockets that sent `feature` in their hello get it. */
export function featureTransient(feature: string, event: Record<string, unknown>): Record<string, unknown> {
  return { t: "transient", event, [ONLY_FEATURE]: feature };
}

/** The text of `frame` for a socket with these features, or null when it must not get it. */
function frameText(frame: unknown, features: string[] | undefined, catalogs?: string[]): string | null {
  if (frame && typeof frame === "object" && ONLY_FEATURE in frame) {
    const { [ONLY_FEATURE]: need, ...rest } = frame as Record<string, unknown>;
    if (!features?.includes(need as string)) return null;
    return JSON.stringify(rest);
  }
  if (!featureAllows(features, frame)) return null;
  return JSON.stringify(withoutV1Fallback(frame, catalogs) ?? frame);
}

/** The catalog of a Card v2 (absent = tc.card/2.0). */
function catalogOf(card: unknown): string | undefined {
  if (!card || typeof card !== "object") return undefined;
  const c = (card as { catalog?: unknown }).catalog;
  return typeof c === "string" ? c : "tc.card/2.0";
}

/**
 * A live `message.created` / `message.updated` event without the derived v1 `card` for a socket
 * whose `hello.supportedCatalogs` has the catalog of the event's `cardV2`: that client renders
 * the v2 card, so the v1 fallback is only for the others. Undefined when nothing changes.
 */
function withoutV1Fallback(frame: unknown, catalogs: string[] | undefined): unknown {
  if (!catalogs || catalogs.length === 0 || !frame || typeof frame !== "object") return undefined;
  const f = frame as { t?: unknown; event?: { type?: unknown; data?: Record<string, unknown> } };
  if (f.t !== "event" || !f.event?.data) return undefined;
  const data = f.event.data;
  if (f.event.type === "message.created") {
    const m = data.message as Record<string, unknown> | undefined;
    const cat = catalogOf(m?.cardV2);
    if (!m || m.card === undefined || !cat || !catalogs.includes(cat)) return undefined;
    const { card: _c, ...message } = m;
    return { ...f, event: { ...f.event, data: { ...data, message } } };
  }
  if (f.event.type === "message.updated") {
    const cat = catalogOf(data.cardV2);
    if (data.card === undefined || !cat || !catalogs.includes(cat)) return undefined;
    const { card: _c, ...rest } = data;
    return { ...f, event: { ...f.event, data: rest } };
  }
  return undefined;
}

/** True when the frame is a live event that may carry a Card v2 with its v1 fallback. */
function hasV1Fallback(frame: unknown): boolean {
  const f = frame as { t?: unknown; event?: { type?: unknown } } | null;
  return !!f && f.t === "event" && (f.event?.type === "message.created" || f.event?.type === "message.updated");
}

/** Transient types that only `interact/1` sockets get (docs/protocol.md, "Features and catalogs"). */
export const INTERACT_TRANSIENTS = new Set(["card.event", "interaction.resolved", "artifact.event", "artifact.data", "artifact.reply"]);
export const INTERACT_FEATURE = "interact/1";

/** True when a socket with these features may get this frame. */
function featureAllows(features: string[] | undefined, frame: unknown): boolean {
  if (!frame || typeof frame !== "object") return true;
  const f = frame as { t?: unknown; event?: { type?: unknown } };
  if (f.t !== "transient" || typeof f.event?.type !== "string" || !INTERACT_TRANSIENTS.has(f.event.type)) return true;
  return features?.includes(INTERACT_FEATURE) === true;
}


/** Internal headers the Worker sets after authenticating; client copies are stripped. */
export const INTERNAL_HEADERS = {
  memberId: "x-chat-member-id",
  tokenId: "x-chat-token-id",
  level: "x-chat-level",
  kind: "x-chat-kind",
} as const;

const PING_TEXT = '{"t":"ping"}';
const PONG_TEXT = '{"t":"pong"}';

/** Frame types handled by the core (feature handlers may not take these). */
const CORE_RATE: Record<string, RateClass> = {
  hello: "none",
  ping: "none",
  send: "write",
  edit: "write",
  delete: "write",
  "topic.update": "write",
  delta: "delta",
  typing: "read",
  read: "read",
  fetch: "read",
  history: "read",
};
/**
 * Frames that reach the room in arrival order per socket (writes and streaming). The gateway
 * starts each one's room RPC as soon as the frame arrives, and numbers it; the room runs them in
 * that order (`GroupRoom.ordered`). Starting the RPC at once matters on a deployed Worker: a
 * Durable Object counts every outgoing call against its newest unfinished incoming request.
 * A write started after a room delivery to this gateway would nest one subrequest level deeper
 * than that delivery, the room's next delivery one level deeper again, and the runtime refuses a
 * call chain at depth 16 ("Subrequest depth limit exceeded", after about 8 pipelined writes).
 * When a frame arrives, its own WebSocket event is the newest request.
 */
const ORDERED = new Set(["send", "edit", "delete", "topic.update", "delta"]);

function socketId(st: SocketState): string {
  return st.sid ?? `${st.tokenId}.${st.connectedAt}`;
}

export class MemberGateway extends DurableObject<Env> {
  private readonly sql: SqlStorage;
  private memberId: string | null = null;
  private log: Logger = baseLogger.child({ obj: "MemberGateway" });
  private readonly buckets = {
    write: new TokenBucket(LIMITS.write.rate, LIMITS.write.burst),
    delta: new TokenBucket(LIMITS.delta.rate, LIMITS.delta.burst),
    read: new TokenBucket(LIMITS.read.rate, LIMITS.read.burst),
  };
  /** Set while an ordered frame of a socket waits for I/O before its room RPC; later ones queue. */
  private readonly barriers = new WeakMap<WebSocket, Promise<void>>();
  /** Last write number per socket and group (see `ORDERED`). */
  private readonly writeSeq = new Map<string, number>();
  /**
   * Sockets inside `hello` (directory loading). Frames delivered meanwhile are kept here and sent
   * after `welcome`, so an event committed after its room's summary was read is not lost.
   */
  private readonly syncing = new Map<WebSocket, string[]>();
  /** Changes with each instance, so numbers restart after hibernation or a restart. */
  private readonly epoch = crypto.randomUUID().slice(0, 8);

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    this.sql = ctx.storage.sql;
    ctx.blockConcurrencyWhile(async () => {
      runMigrations(ctx.storage, gatewayMigrationSteps());
      const row = this.sql.exec<{ value: string }>("SELECT value FROM meta WHERE key = 'member_id'").toArray()[0];
      this.memberId = row?.value ?? null;
      if (this.memberId) this.log = baseLogger.child({ obj: "MemberGateway", memberId: this.memberId });
    });
    ctx.setWebSocketAutoResponse(new WebSocketRequestResponsePair(PING_TEXT, PONG_TEXT));
  }

  private bindMember(memberId: string): void {
    if (this.memberId === memberId) return;
    if (this.memberId !== null) throw new ApiError("internal", "gateway bound to another member");
    this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES ('member_id', ?)", memberId);
    this.memberId = memberId;
    this.log = baseLogger.child({ obj: "MemberGateway", memberId });
  }

  // -------------------------------------------------------------------------
  // WebSocket lifecycle
  // -------------------------------------------------------------------------

  async fetch(request: Request): Promise<Response> {
    if (request.headers.get("upgrade")?.toLowerCase() !== "websocket") {
      return new Response("expected websocket", { status: 426 });
    }
    const memberId = request.headers.get(INTERNAL_HEADERS.memberId);
    const tokenId = request.headers.get(INTERNAL_HEADERS.tokenId);
    const level = request.headers.get(INTERNAL_HEADERS.level) as Actor["level"] | null;
    const kind = request.headers.get(INTERNAL_HEADERS.kind) as Actor["kind"] | null;
    if (!memberId || !tokenId || !level || !kind) return new Response("unauthorized", { status: 401 });
    this.bindMember(memberId);

    const wasOnline = this.openSockets().length > 0;
    const pair = new WebSocketPair();
    const [client, server] = Object.values(pair) as [WebSocket, WebSocket];
    this.ctx.acceptWebSocket(server, [`tok:${tokenId}`]);
    const state: SocketState = {
      memberId,
      tokenId,
      level,
      kind,
      ready: false,
      connectedAt: Date.now(),
      sid: crypto.randomUUID(),
    };
    server.serializeAttachment(state);
    if (!wasOnline) this.ctx.waitUntil(this.reportPresence(true));
    return new Response(null, { status: 101, webSocket: client });
  }

  private openSockets(except?: WebSocket): WebSocket[] {
    return this.ctx.getWebSockets().filter((ws) => ws !== except && ws.readyState === WebSocket.OPEN);
  }

  private readySockets(): WebSocket[] {
    return this.openSockets().filter((ws) => (ws.deserializeAttachment() as SocketState | null)?.ready);
  }

  async webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): Promise<void> {
    if (typeof message !== "string") {
      if (message.byteLength > MAX_FRAME_BYTES) return this.sendError(ws, "too_large", "frame exceeds 1 MiB");
      return this.sendError(ws, "invalid", "binary frames are not supported");
    }
    if (message === PING_TEXT) return this.send(ws, { t: "pong" });
    const parsed = this.parse(message);
    if (!parsed.ok) {
      if (parsed.rid) return this.send(ws, { t: "ack", rid: parsed.rid, ok: false, error: parsed.error });
      return this.sendError(ws, parsed.error.code, parsed.error.message);
    }
    const frame = parsed.frame;
    if (ORDERED.has(frame.t)) {
      // Fast path: the room RPC starts in this event, with no I/O before it.
      const barrier = this.barriers.get(ws);
      if (!barrier && this.isMemberCached(frame.groupId)) return this.dispatch(ws, frame);
      // Slow path (membership not cached yet): this frame and the ordered frames after it on this
      // socket wait for the membership check, in order.
      const next = (barrier ?? Promise.resolve()).then(() => this.dispatch(ws, frame));
      const done = next.catch(() => {});
      this.barriers.set(ws, done);
      void done.then(() => {
        if (this.barriers.get(ws) === done) this.barriers.delete(ws);
      });
      return next;
    }
    return this.dispatch(ws, frame);
  }

  async webSocketClose(ws: WebSocket, code: number, _reason: string, _wasClean: boolean): Promise<void> {
    try {
      ws.close(code === 1005 || code === 1006 ? 1000 : code, "closed");
    } catch {
      // Already closed.
    }
    await this.afterSocketGone(ws);
  }

  async webSocketError(ws: WebSocket, _error: unknown): Promise<void> {
    await this.afterSocketGone(ws);
  }

  private async afterSocketGone(ws: WebSocket): Promise<void> {
    const st = ws.deserializeAttachment() as SocketState | null;
    if (st) {
      const prefix = `${socketId(st)}:`;
      for (const k of this.writeSeq.keys()) if (k.startsWith(prefix)) this.writeSeq.delete(k);
    }
    if (this.openSockets(ws).length === 0) await this.reportPresence(false);
  }

  private async reportPresence(online: boolean): Promise<void> {
    if (!this.memberId) return;
    const memberId = this.memberId;
    const groups = this.sql.exec<{ group_id: string }>("SELECT group_id FROM memberships").toArray();
    await Promise.allSettled(
      groups.map((g) => roomStub(this.env, g.group_id).presence(g.group_id, memberId, online)),
    );
  }

  // -------------------------------------------------------------------------
  // Frame parsing and dispatch
  // -------------------------------------------------------------------------

  private parse(
    text: string,
  ):
    | { ok: true; frame: Record<string, unknown> & { t: string } }
    | { ok: false; rid?: string; error: { code: "invalid" | "too_large"; message: string; reason?: string } } {
    if (text.length > MAX_FRAME_BYTES || new TextEncoder().encode(text).byteLength > MAX_FRAME_BYTES) {
      return { ok: false, error: { code: "too_large", message: "frame exceeds 1 MiB" } };
    }
    let raw: unknown;
    try {
      raw = JSON.parse(text);
    } catch {
      return { ok: false, error: { code: "invalid", message: "frame is not valid JSON" } };
    }
    if (!raw || typeof raw !== "object" || Array.isArray(raw)) {
      return { ok: false, error: { code: "invalid", message: "frame must be an object" } };
    }
    const obj = raw as Record<string, unknown>;
    const rid = typeof obj.rid === "string" && obj.rid.length > 0 && obj.rid.length <= 128 ? obj.rid : undefined;
    const t = obj.t;
    if (typeof t !== "string") return { ok: false, rid, error: { code: "invalid", message: "missing frame type" } };
    const feature = frameHandlers.get(t);
    // A frame type of the protocol without a feature handler yet still gets its schema checked,
    // then `dispatch` answers that the type is not handled.
    const known = ClientFrame.options.some((o) => ("shape" in o ? o.shape.t.value : undefined) === t);
    const schema = t in CORE_RATE ? ClientFrame : feature?.schema ?? (feature || known ? ClientFrame : undefined);
    if (!schema) return { ok: false, rid, error: { code: "invalid", message: `unknown frame type ${t}` } };
    const result = schema.safeParse(raw);
    if (!result.success) {
      for (const c of [obj.card, obj.cardV2]) {
        if (c !== undefined && new TextEncoder().encode(JSON.stringify(c)).byteLength > MAX_CARD_BYTES) {
          return { ok: false, rid, error: { code: "too_large", message: "card exceeds 64 KiB" } };
        }
      }
      if (
        t === "card.answer" &&
        new TextEncoder().encode(JSON.stringify([obj.inputs ?? null, obj.editedValues ?? null])).byteLength > MAX_CARD_ANSWER_BYTES
      ) {
        return { ok: false, rid, error: { code: "too_large", message: "answer exceeds 64 KiB" } };
      }
      const issue = result.error.issues.find((i) => schemaReason({}, i.message)) ?? result.error.issues[0];
      const where = issue?.path.join(".") || "(root)";
      const message = `invalid frame: ${where} ${issue?.message ?? ""}`.trim();
      const reason = schemaReason(obj, issue?.message, issue?.path);
      if (reason === "card.limits" && /exceeds 64 KiB/.test(issue?.message ?? "")) {
        return { ok: false, rid, error: { code: "too_large", message: "card exceeds 64 KiB" } };
      }
      return { ok: false, rid, error: reason ? { code: "invalid", message, reason } : { code: "invalid", message } };
    }
    return { ok: true, frame: result.data as Record<string, unknown> & { t: string } };
  }

  private async dispatch(ws: WebSocket, frame: Record<string, unknown> & { t: string }): Promise<void> {
    const state = ws.deserializeAttachment() as SocketState | null;
    if (!state) return;
    const rid = typeof frame.rid === "string" ? frame.rid : undefined;
    try {
      if (frame.t === "ping") return this.send(ws, { t: "pong" });
      if (frame.t === "hello") {
        // One hello per socket: each one loads the whole directory.
        if (state.ready || this.syncing.has(ws)) throw new ApiError("invalid", "hello was already sent");
        return await this.hello(ws, state, frame as ClientFrameOf<"hello">);
      }
      if (!state.ready) throw new ApiError("invalid", "send hello first");

      const feature = frameHandlers.get(frame.t);
      const rate = CORE_RATE[frame.t] ?? feature?.rate ?? "write";
      if (rate !== "none" && !this.buckets[rate].take()) {
        throw new ApiError("rate_limited", "too many frames", undefined, this.buckets[rate].waitMs());
      }

      const c = this.frameContext(ws, state);
      for (const v of [...(frameValidators.get("*") ?? []), ...(frameValidators.get(frame.t) ?? [])]) {
        await v(frame as ClientFrame, c);
      }
      if (frame.t in CORE_RATE) return await this.core(ws, state, frame as ClientFrame, rid);
      if (!feature) throw new ApiError("invalid", `unknown frame type ${frame.t}`);
      const out = await feature.handle(frame, c);
      if (rid) {
        if (out && typeof out === "object" && "reply" in out) this.send(ws, (out as { reply: unknown }).reply);
        else this.send(ws, out === undefined ? { t: "ack", rid, ok: true } : { t: "ack", rid, ok: true, result: out });
      }
    } catch (err) {
      const error = toErrorBody(err);
      if (!(err instanceof ApiError)) this.log.error("frame failed", { err, t: frame.t });
      if (rid) this.send(ws, { t: "ack", rid, ok: false, error });
      else this.send(ws, { t: "error", error });
    }
  }

  private actor(state: SocketState): Actor {
    return { id: state.memberId, level: state.level, kind: state.kind, tokenId: state.tokenId };
  }

  private frameContext(ws: WebSocket, state: SocketState): FrameContext {
    return {
      env: this.env,
      actor: this.actor(state),
      log: this.log,
      sql: this.sql,
      reply: (f) => this.send(ws, f),
      broadcast: (f) => this.broadcastFrame(f),
      requireMember: (groupId) => this.requireMember(groupId),
    };
  }

  private isMemberCached(groupId: unknown): boolean {
    if (typeof groupId !== "string") return false;
    return this.sql.exec("SELECT 1 FROM memberships WHERE group_id = ?", groupId).toArray().length > 0;
  }

  /** The next write number of this socket in this group. */
  private writeOrder(state: SocketState, groupId: string): WriteOrder {
    const key = `${this.epoch}:${socketId(state)}`;
    const k = `${socketId(state)}:${groupId}`;
    const n = (this.writeSeq.get(k) ?? 0) + 1;
    this.writeSeq.set(k, n);
    return { key, n };
  }

  /** Membership check: local cache first, then D1 (the cache may lag a just-made change). */
  private async requireMember(groupId: string): Promise<void> {
    const cached = this.sql.exec("SELECT 1 FROM memberships WHERE group_id = ?", groupId).toArray().length > 0;
    if (cached) return;
    if (!this.memberId || !(await isGroupMember(this.env.DB, groupId, this.memberId))) {
      throw new ApiError("forbidden", "not a member of this group");
    }
    this.sql.exec("INSERT OR IGNORE INTO memberships (group_id) VALUES (?)", groupId);
  }

  private async core(ws: WebSocket, state: SocketState, frame: ClientFrame, rid?: string): Promise<void> {
    const actor = this.actor(state);
    const ack = (result?: unknown) => {
      if (rid) this.send(ws, result === undefined ? { t: "ack", rid, ok: true } : { t: "ack", rid, ok: true, result });
    };
    switch (frame.t) {
      case "send": {
        await this.requireMember(frame.groupId);
        return ack(unwrap(await roomStub(this.env, frame.groupId).send(frame.groupId, actor, frame, this.writeOrder(state, frame.groupId))));
      }
      case "edit": {
        await this.requireMember(frame.groupId);
        return ack(unwrap(await roomStub(this.env, frame.groupId).edit(frame.groupId, actor, frame, this.writeOrder(state, frame.groupId))));
      }
      case "delete": {
        await this.requireMember(frame.groupId);
        return ack(unwrap(await roomStub(this.env, frame.groupId).remove(frame.groupId, actor, frame.messageId, this.writeOrder(state, frame.groupId))));
      }
      case "topic.update": {
        await this.requireMember(frame.groupId);
        return ack(unwrap(await roomStub(this.env, frame.groupId).topicUpdate(frame.groupId, actor, frame, this.writeOrder(state, frame.groupId))));
      }
      case "delta": {
        await this.requireMember(frame.groupId);
        unwrap(await roomStub(this.env, frame.groupId).delta(frame.groupId, actor, frame.messageId, frame.append, this.writeOrder(state, frame.groupId)));
        return;
      }
      case "typing": {
        await this.requireMember(frame.groupId);
        unwrap(await roomStub(this.env, frame.groupId).typing(frame.groupId, actor, frame.topicId));
        return;
      }
      case "read": {
        await this.requireMember(frame.groupId);
        if (frame.topicId) await this.markTopicRead(frame.groupId, frame.topicId, frame.seq);
        else await this.markRead(frame.groupId, frame.seq);
        return;
      }
      case "fetch": {
        await this.requireMember(frame.groupId);
        const page = unwrap(
          await roomStub(this.env, frame.groupId).fetchEvents(frame.groupId, actor, frame.afterSeq, frame.limit),
        );
        return this.send(ws, { t: "page", rid: frame.rid, groupId: frame.groupId, ...page });
      }
      case "history": {
        await this.requireMember(frame.groupId);
        const h = unwrap(
          await roomStub(this.env, frame.groupId).history(
            frame.groupId,
            actor,
            frame.topicId,
            frame.beforeSeq,
            frame.limit,
          ),
        );
        return this.send(ws, { t: "history", rid: frame.rid, groupId: frame.groupId, topicId: frame.topicId, ...h });
      }
      default:
        throw new ApiError("invalid", "unexpected frame");
    }
  }

  private async hello(ws: WebSocket, state: SocketState, frame: ClientFrameOf<"hello">): Promise<void> {
    if (frame.protocol !== PROTOCOL_VERSION) {
      this.send(ws, { t: "error", error: { code: "unsupported_protocol", message: `protocol ${PROTOCOL_VERSION} only` } });
      ws.close(4000, "unsupported protocol");
      return;
    }
    const memberRow = await getMember(this.env.DB, state.memberId);
    if (!memberRow || memberRow.deactivated) {
      this.send(ws, { t: "error", error: { code: "unauthorized", message: "member is deactivated" } });
      ws.close(4001, "unauthorized");
      return;
    }
    const tokenOk = await this.env.DB.prepare("SELECT 1 FROM tokens WHERE id = ? AND revoked_at IS NULL")
      .bind(state.tokenId)
      .first();
    if (!tokenOk) {
      this.send(ws, { t: "error", error: { code: "unauthorized", message: "token revoked" } });
      ws.close(4001, "unauthorized");
      return;
    }
    // Before syncing starts, so frames buffered during the directory load see the features.
    if (frame.features || frame.supportedCatalogs) {
      if (frame.features) state.features = frame.features.slice(0, 32);
      // Only the Card v2 catalogs this server knows (tc.card/2.x); the rest is ignored.
      if (frame.supportedCatalogs) state.catalogs = frame.supportedCatalogs.filter((c) => /^tc\.card\/2\.\d+$/.test(c)).slice(0, 8);
      ws.serializeAttachment(state);
    }
    this.syncing.set(ws, []);
    let buffered: string[] = [];
    let members: Member[];
    try {
      const dir = await this.loadDirectory(state.memberId);
      members = dir.members;
      const groups = dir.groups;
      state.ready = true;
      state.client = { name: frame.client.name.slice(0, 64), version: frame.client.version.slice(0, 32) };
      ws.serializeAttachment(state);
      this.send(ws, {
        t: "welcome",
        protocol: PROTOCOL_VERSION,
        me: toMember(memberRow),
        members,
        groups,
        serverTime: Date.now(),
        features: [...SERVER_FEATURES],
      });
      // Frames delivered while the directory loaded. Some may be older than welcome's lastSeq;
      // clients drop seqs they already have.
      buffered = this.syncing.get(ws) ?? [];
    } finally {
      this.syncing.delete(ws);
    }
    for (const text of buffered) this.sendText(ws, text);
    await this.presenceSnapshot(ws, state, members);
  }

  private async presenceSnapshot(ws: WebSocket, state: SocketState, members: Member[]): Promise<void> {
    // Presence snapshot: ask the other members' gateways (the truth is their open sockets).
    const others = members.filter((m) => m.id !== state.memberId && !m.deactivated);
    const online = await Promise.allSettled(others.map((m) => gatewayStub(this.env, m.id).isOnline()));
    const at = Date.now();
    online.forEach((r, i) => {
      if (r.status === "fulfilled" && r.value) {
        this.send(ws, { t: "transient", event: { type: "presence", memberId: others[i].id, online: true, at } });
      }
    });
  }

  /** Members and GroupSummaries of this member; refreshes the membership cache from D1. */
  private async loadDirectory(memberId: string, onlyGroupIds?: string[]) {
    const [memberRows, groupRows] = await Promise.all([listMembers(this.env.DB), groupsOfMember(this.env.DB, memberId)]);
    if (!onlyGroupIds) {
      this.ctx.storage.transactionSync(() => {
        this.sql.exec("DELETE FROM memberships");
        for (const g of groupRows) this.sql.exec("INSERT OR IGNORE INTO memberships (group_id) VALUES (?)", g.row.id);
      });
    }
    const names = new Map(memberRows.map((m) => [m.id, m.name]));
    const wanted = onlyGroupIds ? groupRows.filter((g) => onlyGroupIds.includes(g.row.id)) : groupRows;
    const groups = await Promise.all(
      wanted.map(async ({ row, memberIds }): Promise<GroupSummary> => {
        const readSeq = this.cursor(row.id);
        const group = viewGroup(row, memberIds, memberId, names);
        try {
          const counts = unwrap(await roomStub(this.env, row.id).summary(row.id, memberId, readSeq));
          return { group, ...counts };
        } catch (err) {
          // One unreachable room must not block the others; the client catches up by seq.
          this.log.warn("room summary failed", { groupId: row.id, err });
          return { group, lastSeq: readSeq, readSeq, unread: 0, mentions: 0 };
        }
      }),
    );
    return { members: memberRows.map(toMember), groups };
  }

  private cursor(groupId: string): number {
    return this.sql.exec<{ seq: number }>("SELECT seq FROM cursors WHERE group_id = ?", groupId).toArray()[0]?.seq ?? 0;
  }

  private async markRead(groupId: string, seq: number): Promise<void> {
    const memberId = this.memberId as string;
    const prev = this.cursor(groupId);
    if (seq <= prev) return;
    const counts = unwrap(await roomStub(this.env, groupId).summary(groupId, memberId, seq));
    if (counts.readSeq <= prev) return;
    this.sql.exec(
      `INSERT INTO cursors (group_id, seq, updated_at) VALUES (?, ?, ?)
       ON CONFLICT (group_id) DO UPDATE SET seq = max(seq, excluded.seq), updated_at = excluded.updated_at`,
      groupId,
      counts.readSeq,
      Date.now(),
    );
    // Sync the new read position and counts to every device of this member. A group-wide read
    // lists every topic that still has unread messages or is followed.
    this.broadcastFrame({
      t: "reads",
      groupId,
      readSeq: counts.readSeq,
      unread: counts.unread,
      mentions: counts.mentions,
      topics: counts.topics,
    });
  }

  private async markTopicRead(groupId: string, topicId: string, seq: number): Promise<void> {
    const memberId = this.memberId as string;
    const r = unwrap(await roomStub(this.env, groupId).readTopic(groupId, memberId, topicId, seq, this.cursor(groupId)));
    if (!r.changed) return;
    this.broadcastFrame({ t: "reads", groupId, readSeq: r.readSeq, unread: r.unread, mentions: r.mentions, topics: [r.changed] });
  }

  // -------------------------------------------------------------------------
  // Output
  // -------------------------------------------------------------------------

  private send(ws: WebSocket, frame: ServerFrame | Record<string, unknown> | unknown): void {
    this.sendText(ws, JSON.stringify(frame));
  }

  private sendText(ws: WebSocket, text: string): void {
    try {
      ws.send(text);
    } catch (err) {
      // Normal when the socket closed meanwhile; on an open socket the frame was refused.
      if (ws.readyState === WebSocket.OPEN) this.log.warn("socket send failed", { err, bytes: text.length });
    }
  }

  private sendError(ws: WebSocket, code: ApiError["code"], message: string): void {
    this.send(ws, { t: "error", error: { code, message } });
  }

  private broadcastFrame(frame: unknown): void {
    this.sendFrames([frame]);
  }

  /** Sends frames to every ready socket (and buffers them for sockets still syncing), by feature. */
  private sendFrames(frames: unknown[]): number {
    const sockets = this.readySockets();
    const plain = frames.every((f) => !(f && typeof f === "object" && ONLY_FEATURE in f) && featureAllows(undefined, f) && !hasV1Fallback(f));
    const shared = plain ? frames.map((f) => JSON.stringify(f)) : null;
    const textsFor = (ws: WebSocket) => {
      if (shared) return shared;
      const st = ws.deserializeAttachment() as SocketState | null;
      return frames.map((f) => frameText(f, st?.features, st?.catalogs)).filter((t): t is string => t !== null);
    };
    for (const ws of sockets) for (const text of textsFor(ws)) this.sendText(ws, text);
    for (const [ws, buf] of this.syncing) {
      if (ws.readyState === WebSocket.OPEN) buf.push(...textsFor(ws));
    }
    return sockets.length;
  }

  /** True when a ready socket of this member sent `feature` in its hello (1.1). */
  async onlineWith(feature: string): Promise<boolean> {
    return this.featureOnline(feature);
  }

  // -------------------------------------------------------------------------
  // RPC (rooms and the API Worker)
  // -------------------------------------------------------------------------

  /**
   * Delivers frames (events, transient events, directory) to every ready socket. The 1.1
   * transient types go only to sockets that sent `hello.features` with `interact/1`.
   */
  async deliver(frames: unknown[]): Promise<number> {
    return this.sendFrames(frames);
  }

  /** True when a ready socket of this member sent `hello.features` with this feature. */
  async featureOnline(feature: string): Promise<boolean> {
    return this.readySockets().some((ws) => (ws.deserializeAttachment() as SocketState | null)?.features?.includes(feature) === true);
  }

  /** The member's level changed: update every socket so frames use the new permissions at once. */
  async setLevel(level: Actor["level"]): Promise<number> {
    const sockets = this.ctx.getWebSockets();
    for (const ws of sockets) {
      const st = ws.deserializeAttachment() as SocketState | null;
      if (st) ws.serializeAttachment({ ...st, level });
    }
    return sockets.length;
  }

  async isOnline(): Promise<boolean> {
    return this.openSockets().length > 0;
  }

  /** GroupSummaries of this member (all groups, or the given ones). */
  async summaries(memberId: string, groupIds?: string[]): Promise<GroupSummary[]> {
    this.bindMember(memberId);
    return (await this.loadDirectory(memberId, groupIds)).groups;
  }

  /** A group or this member's membership changed: refresh the cache and push a directory frame. */
  async groupChanged(memberId: string, groupId: string): Promise<void> {
    this.bindMember(memberId);
    if (await isGroupMember(this.env.DB, groupId, memberId)) {
      this.sql.exec("INSERT OR IGNORE INTO memberships (group_id) VALUES (?)", groupId);
      const { groups } = await this.loadDirectory(memberId, [groupId]);
      this.broadcastFrame({ t: "directory", groups });
    } else {
      this.sql.exec("DELETE FROM memberships WHERE group_id = ?", groupId);
      this.broadcastFrame({ t: "directory", removedGroupIds: [groupId] });
    }
  }

  /**
   * Closes sockets: those authenticated with the given token ids, or all of them (for a
   * revoked-everything or deactivated member). Returns how many were closed.
   */
  async closeSockets(opts: { tokenIds?: string[]; code?: number; reason?: string } = {}): Promise<number> {
    const sockets = opts.tokenIds
      ? opts.tokenIds.flatMap((id) => this.ctx.getWebSockets(`tok:${id}`))
      : this.ctx.getWebSockets();
    for (const ws of sockets) {
      try {
        ws.send(JSON.stringify({ t: "error", error: { code: "unauthorized", message: opts.reason ?? "signed out" } }));
        ws.close(opts.code ?? 4001, "unauthorized");
      } catch {
        // Already closed.
      }
    }
    if (sockets.length > 0 && this.openSockets().length === 0) this.ctx.waitUntil(this.reportPresence(false));
    return sockets.length;
  }

  /** Feature RPC registered with `registerGatewayRpc`. */
  async rpc(name: string, args: unknown): Promise<unknown> {
    const h = gatewayRpcHandlers.get(name);
    if (!h) throw new Error(`unknown gateway rpc ${name}`);
    const c: GatewayContext = {
      env: this.env,
      sql: this.sql,
      log: this.log,
      broadcast: (f) => this.broadcastFrame(f),
      online: () => this.openSockets().length > 0,
    };
    return h(c, args);
  }
}
