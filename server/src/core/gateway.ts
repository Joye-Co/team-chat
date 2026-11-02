// MemberGateway: one per member. Holds the member's WebSockets (hibernatable), routes frames
// to group rooms over RPC, delivers events, stores read cursors, reports presence.
import { DurableObject } from "cloudflare:workers";
import {
  ClientFrame,
  type ClientFrameOf,
  type GroupSummary,
  MAX_FRAME_BYTES,
  PROTOCOL_VERSION,
  type ServerFrame,
} from "@chat/protocol";
import { getMember, groupsOfMember, isGroupMember, listMembers, toMember, viewGroup } from "./db";
import { type Env, gatewayStub, roomStub } from "./env";
import { ApiError, toErrorBody, unwrap } from "./errors";
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
  client?: { name: string; version: string };
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
  typing: "none",
  read: "none",
  fetch: "read",
  history: "read",
};
/** Frames processed strictly in arrival order per socket (writes and streaming). */
const ORDERED = new Set(["send", "edit", "delete", "topic.update", "delta"]);

export class MemberGateway extends DurableObject<Env> {
  private readonly sql: SqlStorage;
  private memberId: string | null = null;
  private log: Logger = baseLogger.child({ obj: "MemberGateway" });
  private readonly buckets = {
    write: new TokenBucket(LIMITS.write.rate, LIMITS.write.burst),
    delta: new TokenBucket(LIMITS.delta.rate, LIMITS.delta.burst),
    read: new TokenBucket(LIMITS.read.rate, LIMITS.read.burst),
  };
  private readonly chains = new WeakMap<WebSocket, Promise<void>>();

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
    const state: SocketState = { memberId, tokenId, level, kind, ready: false, connectedAt: Date.now() };
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
      // Writes and deltas from one socket are processed one at a time, in order.
      const prev = this.chains.get(ws) ?? Promise.resolve();
      const next = prev.then(() => this.dispatch(ws, frame));
      this.chains.set(ws, next.catch(() => {}));
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
    | { ok: false; rid?: string; error: { code: "invalid" | "too_large"; message: string } } {
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
    const schema = t in CORE_RATE ? ClientFrame : feature?.schema ?? (feature ? ClientFrame : undefined);
    if (!schema) return { ok: false, rid, error: { code: "invalid", message: `unknown frame type ${t}` } };
    const result = schema.safeParse(raw);
    if (!result.success) {
      const issue = result.error.issues[0];
      const where = issue?.path.join(".") || "(root)";
      return { ok: false, rid, error: { code: "invalid", message: `invalid frame: ${where} ${issue?.message ?? ""}`.trim() } };
    }
    return { ok: true, frame: result.data as Record<string, unknown> & { t: string } };
  }

  private async dispatch(ws: WebSocket, frame: Record<string, unknown> & { t: string }): Promise<void> {
    const state = ws.deserializeAttachment() as SocketState | null;
    if (!state) return;
    const rid = typeof frame.rid === "string" ? frame.rid : undefined;
    try {
      if (frame.t === "ping") return this.send(ws, { t: "pong" });
      if (frame.t === "hello") return await this.hello(ws, state, frame as ClientFrameOf<"hello">);
      if (!state.ready) throw new ApiError("invalid", "send hello first");

      const feature = frameHandlers.get(frame.t);
      const rate = CORE_RATE[frame.t] ?? feature?.rate ?? "write";
      if (rate !== "none" && !this.buckets[rate].take()) throw new ApiError("rate_limited", "too many frames");

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
        return ack(unwrap(await roomStub(this.env, frame.groupId).send(frame.groupId, actor, frame)));
      }
      case "edit": {
        await this.requireMember(frame.groupId);
        return ack(unwrap(await roomStub(this.env, frame.groupId).edit(frame.groupId, actor, frame)));
      }
      case "delete": {
        await this.requireMember(frame.groupId);
        return ack(unwrap(await roomStub(this.env, frame.groupId).remove(frame.groupId, actor, frame.messageId)));
      }
      case "topic.update": {
        await this.requireMember(frame.groupId);
        return ack(unwrap(await roomStub(this.env, frame.groupId).topicUpdate(frame.groupId, actor, frame)));
      }
      case "delta": {
        await this.requireMember(frame.groupId);
        unwrap(await roomStub(this.env, frame.groupId).delta(frame.groupId, actor, frame.messageId, frame.append));
        return;
      }
      case "typing": {
        await this.requireMember(frame.groupId);
        unwrap(await roomStub(this.env, frame.groupId).typing(frame.groupId, actor, frame.topicId));
        return;
      }
      case "read": {
        await this.requireMember(frame.groupId);
        await this.markRead(frame.groupId, frame.seq);
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
    const { members, groups } = await this.loadDirectory(state.memberId);
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
    });
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
    // Sync the new read position and counts to every device of this member.
    const { groups } = await this.loadDirectory(memberId, [groupId]);
    if (groups.length > 0) this.broadcastFrame({ t: "directory", groups });
  }

  // -------------------------------------------------------------------------
  // Output
  // -------------------------------------------------------------------------

  private send(ws: WebSocket, frame: ServerFrame | Record<string, unknown> | unknown): void {
    try {
      ws.send(JSON.stringify(frame));
    } catch {
      // Socket closed meanwhile.
    }
  }

  private sendError(ws: WebSocket, code: ApiError["code"], message: string): void {
    this.send(ws, { t: "error", error: { code, message } });
  }

  private broadcastFrame(frame: unknown): void {
    const text = JSON.stringify(frame);
    for (const ws of this.readySockets()) {
      try {
        ws.send(text);
      } catch {
        // Closed meanwhile.
      }
    }
  }

  // -------------------------------------------------------------------------
  // RPC (rooms and the API Worker)
  // -------------------------------------------------------------------------

  /** Delivers frames (events, transient events, directory) to every ready socket. */
  async deliver(frames: unknown[]): Promise<number> {
    const sockets = this.readySockets();
    for (const f of frames) {
      const text = JSON.stringify(f);
      for (const ws of sockets) {
        try {
          ws.send(text);
        } catch {
          // Closed meanwhile.
        }
      }
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
