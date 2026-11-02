// Extension points. Feature modules (server/src/<feature>/) call these at module load time;
// server/src/features.ts imports every feature module so the registrations run in the Worker
// and in both Durable Object classes (they share one module graph).
import type { ClientFrame, Event, Member } from "@chat/protocol";
import type { z } from "zod";
import type { AuthContext } from "./auth";
import type { Env } from "./env";
import type { Method } from "./http";
import type { Logger } from "./logger";
import type { MigrationStep } from "./migrations";

/** Who performs an action inside the Durable Objects. */
export interface Actor {
  id: string;
  level: Member["level"];
  kind: Member["kind"];
  /** Credential (token id) the action came with, for audit. */
  tokenId?: string;
}

// ---------------------------------------------------------------------------
// HTTP routes
// ---------------------------------------------------------------------------

export interface RouteContext {
  request: Request;
  env: Env;
  ctx: ExecutionContext;
  url: URL;
  params: Record<string, string>;
  /** Set when the route's auth is "member" or "owner". */
  auth: AuthContext | null;
  log: Logger;
  /** Reads and validates the JSON body; default limit is the route's `bodyLimit`. */
  body<S extends z.ZodType>(schema: S, limit?: number): Promise<z.infer<S>>;
}

export interface RouteDef {
  method: Method;
  /** Path pattern with `:param` segments, e.g. "/v1/files/:id". */
  path: string;
  /** "none": public; "member": valid bearer token; "owner": token of an owner. */
  auth: "none" | "member" | "owner";
  /** Max request body bytes for `body()`. Default 32 KiB. */
  bodyLimit?: number;
  /** Return a Response, or any JSON value (sent with status 200). Throw ApiError to fail. */
  handler(c: RouteContext): Promise<Response | unknown> | Response | unknown;
}

export const routes: RouteDef[] = [];
export function registerRoute(def: RouteDef): void {
  routes.push(def);
}

// ---------------------------------------------------------------------------
// WebSocket frames (MemberGateway)
// ---------------------------------------------------------------------------

export interface FrameContext {
  env: Env;
  actor: Actor;
  log: Logger;
  /** Sends one frame to the socket that sent this frame. */
  reply(frame: unknown): void;
  /** Sends one frame to every ready socket of this member. */
  broadcast(frame: unknown): void;
  /** Throws `forbidden` unless the member belongs to the group. */
  requireMember(groupId: string): Promise<void>;
  /** Gateway SQLite, for feature state kept per member. */
  sql: SqlStorage;
}

export type RateClass = "write" | "read" | "delta" | "none";

export interface FrameHandler<F = Record<string, unknown> & { t: string }> {
  /** Validates frames of this type. Needed when `t` is not part of ClientFrame. */
  schema?: z.ZodType<F>;
  rate?: RateClass;
  /**
   * Handles the frame. For frames with `rid`, the return value becomes `ack.result` unless the
   * handler returns `{ reply: frame }`, which is sent as is. Throw ApiError to fail.
   */
  handle(frame: F, c: FrameContext): Promise<unknown> | unknown;
}

export const frameHandlers = new Map<string, FrameHandler<any>>();
export function registerFrameHandler<F extends { t: string }>(t: string, h: FrameHandler<F>): void {
  if (frameHandlers.has(t)) throw new Error(`frame handler for "${t}" already registered`);
  frameHandlers.set(t, h);
}

/** Extra validation of an incoming frame before its handler runs (core or feature). Throw ApiError. */
export type FrameValidator = (frame: ClientFrame | (Record<string, unknown> & { t: string }), c: FrameContext) =>
  | Promise<void>
  | void;
export const frameValidators = new Map<string, FrameValidator[]>();
/** `t` = a frame type, or "*" for every frame. */
export function registerFrameValidator(t: string, v: FrameValidator): void {
  const list = frameValidators.get(t) ?? [];
  list.push(v);
  frameValidators.set(t, list);
}

// ---------------------------------------------------------------------------
// GroupRoom: RPC handlers, migrations, message extensions
// ---------------------------------------------------------------------------

export interface EventDraft {
  type: string;
  actorId: string;
  topicId?: string | null;
  messageId?: string | null;
  data: unknown;
}

export interface RoomTx {
  /** Seq the next appended event gets. */
  nextSeq(): number;
  /** Appends one event to the log inside the current transaction. */
  append(draft: EventDraft): Event;
}

export interface RoomContext {
  groupId: string;
  env: Env;
  sql: SqlStorage;
  log: Logger;
  memberIds(): string[];
  isMember(memberId: string): boolean;
  /** Runs `fn` in one SQLite transaction; events it appends are fanned out and hooked after commit. */
  commit<T>(fn: (tx: RoomTx) => T): T;
  /** Sends a frame (e.g. a transient event) to the gateways of all members, or the given ones. */
  broadcast(frame: unknown, opts?: { to?: string[]; except?: string }): void;
  waitUntil(p: Promise<unknown>): void;
}

export type RoomRpcHandler = (room: RoomContext, actor: Actor | null, args: unknown) => Promise<unknown> | unknown;
export const roomRpcHandlers = new Map<string, RoomRpcHandler>();
/** Called through `GroupRoom.rpc(groupId, name, actor, args)`; throw ApiError to fail. */
export function registerRoomRpc(name: string, h: RoomRpcHandler): void {
  if (roomRpcHandlers.has(name)) throw new Error(`room rpc "${name}" already registered`);
  roomRpcHandlers.set(name, h);
}

export const roomMigrations: MigrationStep[] = [];
/** Feature steps run after the core steps, in registration order. */
export function registerRoomMigration(step: MigrationStep): void {
  roomMigrations.push(step);
}

/**
 * Adds fields to a message at send time. Runs inside the room before the write; return extra
 * fields to store (JSON, in `messages.ext`) and merge into the Message, or undefined. Throw
 * ApiError to reject the send.
 */
export type MessageExtension = (
  frame: Record<string, unknown>,
  room: RoomContext,
  actor: Actor,
) => Record<string, unknown> | undefined;
export const messageExtensions: MessageExtension[] = [];
export function registerMessageExtension(ext: MessageExtension): void {
  messageExtensions.push(ext);
}

// ---------------------------------------------------------------------------
// MemberGateway: RPC handlers and migrations
// ---------------------------------------------------------------------------

export interface GatewayContext {
  env: Env;
  sql: SqlStorage;
  log: Logger;
  /** Sends a frame to every ready socket of this member. */
  broadcast(frame: unknown): void;
  online(): boolean;
}
export type GatewayRpcHandler = (gw: GatewayContext, args: unknown) => Promise<unknown> | unknown;
export const gatewayRpcHandlers = new Map<string, GatewayRpcHandler>();
export function registerGatewayRpc(name: string, h: GatewayRpcHandler): void {
  if (gatewayRpcHandlers.has(name)) throw new Error(`gateway rpc "${name}" already registered`);
  gatewayRpcHandlers.set(name, h);
}
export const gatewayMigrations: MigrationStep[] = [];
export function registerGatewayMigration(step: MigrationStep): void {
  gatewayMigrations.push(step);
}

// ---------------------------------------------------------------------------
// Hooks after each committed event
// ---------------------------------------------------------------------------

export interface HookContext {
  env: Env;
  groupId: string;
  /** Members of the group after the commit. */
  memberIds: string[];
  log: Logger;
  waitUntil(p: Promise<unknown>): void;
  /** Read access to the room's SQLite (do not write from a hook; use a room RPC). */
  sql: SqlStorage;
}

export type AfterCommitHook = (event: Event, h: HookContext) => Promise<void> | void;
export const afterCommitHooks: AfterCommitHook[] = [];
/** Runs after the event is committed, without delaying the sender's ack. Errors are logged. */
export function registerAfterCommit(hook: AfterCommitHook): void {
  afterCommitHooks.push(hook);
}
