// Extension points. Feature modules (server/src/<feature>/) call these at module load time;
// server/src/features.ts imports every feature module so the registrations run in the Worker
// and in both Durable Object classes (they share one module graph).
import type { ClientFrame, Event, Member, Message, TopicStatus } from "@chat/protocol";
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
  /** In-memory facts for event decorators (e.g. who was just added to waitingOn). Never stored. */
  hints?: Record<string, unknown>;
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
  /** "group" or "dm"; null only before the room has loaded its group from D1. */
  groupKind: "group" | "dm" | null;
  /** Runs `fn` in one SQLite transaction; events it appends are fanned out and hooked after commit. */
  commit<T>(fn: (tx: RoomTx) => T): T;
  /** Sends a frame (e.g. a transient event) to the gateways of all members, or the given ones. */
  broadcast(frame: unknown, opts?: { to?: string[]; except?: string }): void;
  waitUntil(p: Promise<unknown>): void;
  /** Wakes the room's alarm handlers (`registerRoomAlarm`) at `at` or earlier. */
  wakeAt(at: number): void;
  // cards2: card writes that read a message, await, then write it run one at a time per key.
  /** Runs `fn` after every earlier `serial` call with the same key has settled. */
  serial<T>(key: string, fn: () => Promise<T>): Promise<T>;
}

export type RoomRpcHandler = (room: RoomContext, actor: Actor | null, args: unknown) => Promise<unknown> | unknown;
export const roomRpcHandlers = new Map<string, RoomRpcHandler>();
/** Called through `GroupRoom.rpc(groupId, name, actor, args)`; throw ApiError to fail. */
export function registerRoomRpc(name: string, h: RoomRpcHandler): void {
  if (roomRpcHandlers.has(name)) throw new Error(`room rpc "${name}" already registered`);
  roomRpcHandlers.set(name, h);
}

/**
 * Runs when the room's alarm fires (set with `wakeAt`). Retries of durable feature work, such
 * as an outbox. Errors are logged; a handler that needs another wake calls `wakeAt` again.
 */
export type RoomAlarmHandler = (room: RoomContext) => Promise<void> | void;
export const roomAlarmHandlers: RoomAlarmHandler[] = [];
export function registerRoomAlarm(h: RoomAlarmHandler): void {
  roomAlarmHandlers.push(h);
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

/**
 * Changes extension fields of a message at edit time. Runs inside the room before the write,
 * with the message as stored. Return the fields to replace (merged into `messages.ext`, the
 * Message and the `message.updated` event data), or undefined. Throw ApiError to reject.
 */
export type EditExtension = (
  frame: Record<string, unknown>,
  room: RoomContext,
  actor: Actor,
  message: Message,
) => Record<string, unknown> | undefined;
export const editExtensions: EditExtension[] = [];
export function registerEditExtension(ext: EditExtension): void {
  editExtensions.push(ext);
}

/**
 * Message write hooks (1.1: Card v2 state, causes, refs). Unlike the extensions above they may
 * change the body, run async checks before the write, and append events inside the write's
 * transaction. Throw ApiError to reject the write.
 */
export interface MessageWriteResult {
  /** Runs in the commit transaction after the message event is appended. */
  inTx?(tx: RoomTx, message: Message): void;
  /** Runs after the commit (transients). */
  after?(message: Message): void;
}
export interface SendDraft {
  body: string;
  ext: Record<string, unknown> | undefined;
}
export interface EditDraft {
  body: string;
  /** Fields replaced in `messages.ext`, the Message and the `message.updated` data. */
  changed: Record<string, unknown>;
}
export interface MessageWriteHook {
  send?(
    frame: Record<string, unknown>,
    room: RoomContext,
    actor: Actor,
    draft: SendDraft,
  ): Promise<MessageWriteResult | undefined> | MessageWriteResult | undefined;
  edit?(
    frame: Record<string, unknown>,
    room: RoomContext,
    actor: Actor,
    current: Message,
    draft: EditDraft,
  ): Promise<MessageWriteResult | undefined> | MessageWriteResult | undefined;
  /** In the delete transaction, after `message.deleted` is appended. */
  remove?(room: RoomContext, tx: RoomTx, message: Message, actorId: string): void;
}
export const messageWriteHooks: MessageWriteHook[] = [];
export function registerMessageWriteHook(h: MessageWriteHook): void {
  messageWriteHooks.push(h);
}

export interface DecorateContext {
  groupId: string;
  groupKind: "group" | "dm" | null;
  /** Room SQLite, read only. */
  sql: SqlStorage;
  /** The draft's `hints`, if any. */
  hints: Record<string, unknown> | undefined;
}
/**
 * Adds fields to the live `event` frame one member gets (e.g. `notify`). Runs in the room after
 * commit, once per event and recipient. Never applied to `page` results.
 */
export type EventDecorator = (event: Event, memberId: string, c: DecorateContext) => Record<string, unknown> | undefined;
export const eventDecorators: EventDecorator[] = [];
export function registerEventDecorator(d: EventDecorator): void {
  eventDecorators.push(d);
}

// cards2: the topic status a `topic.update` stores while server-owned waits are open.
/**
 * Called in the `topic.update` transaction with the status and `waitingOn` the frame asks for
 * (after defaults); returns what the topic gets. A feature that owns a wait (open blocking card
 * asks) keeps the asked status as the one to come back to and returns the combined status.
 */
export type TopicStatusFilter = (sql: SqlStorage, topicId: string, next: { status: TopicStatus; waitingOn: string[] }) => { status: TopicStatus; waitingOn: string[] };
export const topicStatusFilters: TopicStatusFilter[] = [];
export function registerTopicStatusFilter(f: TopicStatusFilter): void {
  topicStatusFilters.push(f);
}

// cards2: what one member may see of a stored event or a message (non-public polls and
// anonymous questionnaires keep voters and their choices from other members).
/** The event as `viewerId` gets it, live and in `fetch` pages (not in exports). */
export type EventView = (event: Event, viewerId: string, sql: SqlStorage) => Event;
export const eventViews: EventView[] = [];
export function registerEventView(v: EventView): void {
  eventViews.push(v);
}
/** The message as `viewerId` gets it in history and in write acks. */
export type MessageView = (message: Message, viewerId: string, sql: SqlStorage) => Message;
export const messageViews: MessageView[] = [];
export function registerMessageView(v: MessageView): void {
  messageViews.push(v);
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
  /** "group" or "dm". */
  groupKind: "group" | "dm" | null;
  /** Members of the group after the commit. */
  memberIds: string[];
  log: Logger;
  waitUntil(p: Promise<unknown>): void;
  /**
   * The room's SQLite. Do not append events from a hook (use a room RPC); a feature may update
   * its own bookkeeping tables, such as an outbox.
   */
  sql: SqlStorage;
  /** Wakes the room's alarm handlers (`registerRoomAlarm`) at `at` or earlier. */
  wakeAt(at: number): void;
}

export type AfterCommitHook = (event: Event, h: HookContext) => Promise<void> | void;
export const afterCommitHooks: AfterCommitHook[] = [];
/** Runs after the event is committed, without delaying the sender's ack. Errors are logged. */
export function registerAfterCommit(hook: AfterCommitHook): void {
  afterCommitHooks.push(hook);
}

/**
 * Runs after a member is deactivated (its tokens are revoked and sockets closed), e.g. to stop
 * a bot's runs. Errors are logged by the caller; the deactivation stands.
 */
export type MemberDeactivatedHook = (env: Env, memberId: string) => Promise<void> | void;
export const memberDeactivatedHooks: MemberDeactivatedHook[] = [];
export function registerMemberDeactivated(hook: MemberDeactivatedHook): void {
  memberDeactivatedHooks.push(hook);
}

/**
 * Runs after members changed (name, level, deactivation, a new member, a bot's runner), so a
 * feature can drop what it cached about them. Awaited by the route; errors are logged.
 */
export type MembersChangedHook = (env: Env, memberIds: string[]) => Promise<void> | void;
export const membersChangedHooks: MembersChangedHook[] = [];
export function registerMembersChanged(hook: MembersChangedHook): void {
  membersChangedHooks.push(hook);
}

// ---------------------------------------------------------------------------
// Cron (scheduled handler) and archive import
// ---------------------------------------------------------------------------

export interface ScheduledContext {
  env: Env;
  ctx: ExecutionContext;
  /** The cron expression that fired, e.g. "0 3 * * *". */
  cron: string;
  scheduledTime: number;
  log: Logger;
}
export type ScheduledHandler = (c: ScheduledContext) => Promise<void> | void;
export const scheduledHandlers: { cron: string; handler: ScheduledHandler }[] = [];
/** `cron` = the expression from wrangler.jsonc `triggers.crons`, or "*" for every trigger. */
export function registerScheduled(cron: string, handler: ScheduledHandler): void {
  scheduledHandlers.push({ cron, handler });
}

/** One stored event row as archived and imported: the Event plus its topic and message ids. */
export interface ArchivedEvent {
  seq: number;
  type: string;
  at: number;
  actorId: string;
  topicId?: string | null;
  messageId?: string | null;
  data: unknown;
}

/**
 * Rebuilds a feature's room tables from one imported event (restore from the archive). Runs
 * inside the import transaction, after the event row is stored, in seq order. Throw to abort.
 */
export type EventImporter = (event: ArchivedEvent, sql: SqlStorage) => void;
export const eventImporters: EventImporter[] = [];
export function registerEventImporter(importer: EventImporter): void {
  eventImporters.push(importer);
}

/**
 * Keeps a feature's derived room tables (e.g. a search index) in step with the log. Runs inside
 * the commit transaction right after each event row is stored, for live writes and for imports.
 * Must be synchronous and must not append events. Throwing aborts the write.
 */
export const eventWriters: EventImporter[] = [];
export function registerEventWriter(writer: EventImporter): void {
  eventWriters.push(writer);
}
