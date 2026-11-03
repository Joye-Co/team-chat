// GroupRoom: one per group. Owns the group's event log, topics and messages in its own SQLite,
// assigns seq in the same transaction as the writes, fans events out to member gateways after
// commit, and keeps typing and streaming deltas in memory only.
import { DurableObject } from "cloudflare:workers";
import {
  DEFAULT_TITLE_CHARS,
  MAX_BODY_CHARS,
  MAX_EXCERPT_CHARS,
  type Card,
  type CardV2,
  type ClientFrameOf,
  cardV2ToV1,
  type Event,
  type Message,
  type MessageKind,
  type NotifyLevel,
  type SendResult,
  type Topic,
  type TopicState,
  type TopicContext,
  type TopicStatus,
} from "@chat/protocol";
import { groupMemberIds, getGroup } from "./db";
import { type Env, gatewayStub } from "./env";
import { ApiError, type Result, toErrorBody } from "./errors";
import { ulid } from "./ids";
import { scrubDeletedMessage } from "./scrub";
import { LIMITS } from "./ratelimit";
import { logger as baseLogger, type Logger } from "./logger";
import { type MigrationStep, runMigrations } from "./migrations";
import {
  type Actor,
  type ArchivedEvent,
  type EventDraft,
  type HookContext,
  type RoomContext,
  type RoomTx,
  afterCommitHooks,
  editExtensions,
  eventDecorators,
  eventImporters,
  eventWriters,
  messageExtensions,
  type MessageWriteResult,
  messageWriteHooks,
  roomAlarmHandlers,
  roomMigrations,
  roomRpcHandlers,
} from "./registry";
import {
  ROOM_MESSAGING_MIGRATION,
  type ReadTotals,
  advanceTopicRead,
  getPrefs,
  groupReadState,
  setFollowing,
  topicState,
} from "./topicstate";

export const ROOM_CORE_MIGRATIONS: MigrationStep[] = [
  {
    name: "core-1",
    up: `
      CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
      CREATE TABLE room_members (member_id TEXT PRIMARY KEY, added_at INTEGER NOT NULL);
      CREATE TABLE events (
        seq INTEGER PRIMARY KEY,
        type TEXT NOT NULL,
        at INTEGER NOT NULL,
        actor_id TEXT NOT NULL,
        topic_id TEXT,
        message_id TEXT,
        data TEXT NOT NULL
      );
      CREATE INDEX events_message ON events(message_id) WHERE message_id IS NOT NULL;
      CREATE TABLE topics (
        id TEXT PRIMARY KEY,
        title TEXT NOT NULL,
        status TEXT NOT NULL,
        created_by TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        root_message_id TEXT NOT NULL,
        last_seq INTEGER NOT NULL,
        message_count INTEGER NOT NULL DEFAULT 0
      );
      CREATE TABLE messages (
        id TEXT PRIMARY KEY,
        client_msg_id TEXT NOT NULL,
        topic_id TEXT NOT NULL,
        author_id TEXT NOT NULL,
        body TEXT NOT NULL,
        mentions TEXT NOT NULL DEFAULT '[]',
        created_seq INTEGER NOT NULL,
        created_at INTEGER NOT NULL,
        edited_at INTEGER,
        deleted_at INTEGER,
        streaming INTEGER NOT NULL DEFAULT 0,
        ext TEXT,
        UNIQUE (author_id, client_msg_id)
      );
      CREATE INDEX messages_topic ON messages(topic_id, created_seq);
      CREATE INDEX messages_seq ON messages(created_seq);
      CREATE TABLE mentions (
        member_id TEXT NOT NULL,
        seq INTEGER NOT NULL,
        message_id TEXT NOT NULL,
        PRIMARY KEY (member_id, seq, message_id)
      );
    `,
  },
  {
    // joined_seq: a member added later starts with everything before it read.
    // messages_reply: finds the quotes of a deleted message.
    name: "core-2",
    up: `
      ALTER TABLE room_members ADD COLUMN joined_seq INTEGER NOT NULL DEFAULT 0;
      CREATE INDEX messages_reply ON messages(json_extract(ext, '$.replyTo.messageId'));
    `,
  },
  {
    // 1.1 side chats: the topic a DM topic is about (Topic.contextOf), as JSON.
    name: "core-3",
    up: `ALTER TABLE topics ADD COLUMN context TEXT;`,
  },
];

export function roomMigrationSteps(): MigrationStep[] {
  return [...ROOM_CORE_MIGRATIONS, ROOM_MESSAGING_MIGRATION, ...roomMigrations];
}

type TopicRow = {
  id: string;
  title: string;
  status: TopicStatus;
  created_by: string;
  created_at: number;
  root_message_id: string;
  last_seq: number;
  message_count: number;
  waiting_on: string;
  owner_id: string | null;
  last_message_at: number | null;
  /** JSON of `contextOf` (core-3). */
  context?: string | null;
};

type MessageRow = {
  id: string;
  client_msg_id: string;
  topic_id: string;
  author_id: string;
  body: string;
  mentions: string;
  created_seq: number;
  created_at: number;
  edited_at: number | null;
  deleted_at: number | null;
  streaming: number;
  ext: string | null;
  kind: MessageKind;
};

type EventRow = {
  seq: number;
  type: string;
  at: number;
  actor_id: string;
  data: string;
};

export interface RoomCounts {
  lastSeq: number;
  readSeq: number;
  unread: number;
  mentions: number;
  topics: TopicState[];
  muted: boolean;
  notify: NotifyLevel;
}

/** Result of a per-topic read: the new group totals and the changed topic (none when nothing moved). */
export interface TopicReadResult extends ReadTotals {
  changed: TopicState | null;
}

export interface PageResult {
  events: Event[];
  hasMore: boolean;
  lastSeq: number;
}

const DEFAULT_FETCH_LIMIT = 200;
const DEFAULT_HISTORY_LIMIT = 50;
const MAX_EXPORT_LIMIT = 1000;
const DELTA_FLUSH_MS = 100;
const DELTA_FLUSH_BYTES = 8 * 1024;
const DELIVER_BATCH = 200;
/** Delivery tries per batch before the member's gateway is asked to resync the group. */
const DELIVER_TRIES = 3;
const DELIVER_RETRY_MS = 250;
/** A fetch or history page stops adding rows after this much JSON (it always has one row). */
const MAX_PAGE_BYTES = 4 * 1024 * 1024;
/** A write waits at most this long for the write numbered before it on the same socket. */
const TURN_WAIT_MS = 2000;
/** Socket write orders kept before idle ones are dropped. */
const MAX_TURNS = 256;
const TURN_IDLE_MS = 5 * 60_000;

/** Position of a write among the writes of one gateway socket (see `GroupRoom.ordered`). */
export interface WriteOrder {
  key: string;
  n: number;
}

type SendFrame = ClientFrameOf<"send">;
type EditFrame = ClientFrameOf<"edit">;
type TopicUpdateFrame = ClientFrameOf<"topic.update">;

/** First `MAX_EXCERPT_CHARS` code points, for quotes. */
function excerpt(text: string): string {
  return [...text].slice(0, MAX_EXCERPT_CHARS).join("");
}

/** At most one card press per member and message per this many ms. */
const CARD_PRESS_INTERVAL_MS = 1000;

export function defaultTitle(body: string): string {
  const firstLine = body.split(/\r?\n/).find((l) => l.trim() !== "") ?? "";
  return [...firstLine.trim()].slice(0, DEFAULT_TITLE_CHARS).join("");
}

/**
 * How many of the first `limit` rows fit in `maxBytes` (string lengths, about the JSON size);
 * at least one, so a page always moves forward.
 */
function fitBytes<R>(rows: R[], limit: number, maxBytes: number, size: (r: R) => number): number {
  let bytes = 0;
  const n = Math.min(rows.length, limit);
  for (let i = 0; i < n; i++) {
    bytes += size(rows[i]) + 128;
    if (bytes > maxBytes && i > 0) return i;
  }
  return n;
}

async function run<T>(fn: () => T | Promise<T>): Promise<Result<T>> {
  try {
    return { ok: true, value: await fn() };
  } catch (err) {
    if (!(err instanceof ApiError)) baseLogger.error("room error", { err });
    return { ok: false, error: toErrorBody(err) };
  }
}

export class GroupRoom extends DurableObject<Env> {
  private readonly sql: SqlStorage;
  private groupId: string | null = null;
  private groupKind: "group" | "dm" | null = null;
  private readonly cardPressAt = new Map<string, number>();
  private members = new Set<string>();
  private lastSeq = 0;
  private log: Logger = baseLogger.child({ obj: "GroupRoom" });
  private readonly outbox = new Map<string, { queue: unknown[]; busy: boolean }>();
  /** Per-socket write order (see `ordered`). */
  private readonly turns = new Map<string, { next: number; waiters: Map<number, () => void>; at: number }>();
  private readonly typingAt = new Map<string, number>();
  private readonly deltas = new Map<
    string,
    { topicId: string; buf: string; bytes: number; timer: ReturnType<typeof setTimeout> | null }
  >();

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    this.sql = ctx.storage.sql;
    ctx.blockConcurrencyWhile(async () => {
      runMigrations(ctx.storage, roomMigrationSteps());
      this.loadState();
    });
  }

  private loadState(): void {
    const meta = this.sql.exec<{ value: string }>("SELECT value FROM meta WHERE key = 'group_id'").toArray();
    this.groupId = meta[0]?.value ?? null;
    const kind = this.sql.exec<{ value: string }>("SELECT value FROM meta WHERE key = 'group_kind'").toArray()[0];
    this.groupKind = (kind?.value as "group" | "dm" | undefined) ?? null;
    if (this.groupId) this.log = baseLogger.child({ obj: "GroupRoom", groupId: this.groupId });
    this.members = new Set(
      this.sql.exec<{ member_id: string }>("SELECT member_id FROM room_members").toArray().map((r) => r.member_id),
    );
    const last = this.sql.exec<{ seq: number }>("SELECT seq FROM events ORDER BY seq DESC LIMIT 1").toArray();
    this.lastSeq = last[0]?.seq ?? 0;
  }

  /** Binds this object to its group on first use; loads membership from D1 if never initialized. */
  private async ready(groupId: string): Promise<void> {
    if (this.groupId === groupId && this.groupKind) return;
    if (this.groupId !== null && this.groupId !== groupId) throw new ApiError("internal", "room bound to another group");
    const group = await getGroup(this.env.DB, groupId);
    if (!group) throw new ApiError("not_found", "group not found");
    if (this.groupId === null) this.bind(groupId, await groupMemberIds(this.env.DB, groupId));
    this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES ('group_kind', ?)", group.kind);
    this.groupKind = group.kind;
  }

  private bind(groupId: string, memberIds: string[]): void {
    if (this.groupId === null) {
      const now = Date.now();
      this.ctx.storage.transactionSync(() => {
        this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES ('group_id', ?)", groupId);
        for (const id of memberIds) {
          this.sql.exec("INSERT OR IGNORE INTO room_members (member_id, added_at) VALUES (?, ?)", id, now);
        }
      });
      this.groupId = groupId;
      this.log = baseLogger.child({ obj: "GroupRoom", groupId });
      for (const id of memberIds) this.members.add(id);
    }
  }

  private requireMember(actorId: string): void {
    if (!this.members.has(actorId)) throw new ApiError("forbidden", "not a member of this group");
  }

  // -------------------------------------------------------------------------
  // Commit and fan-out
  // -------------------------------------------------------------------------

  private commit<T>(fn: (tx: RoomTx) => T, extraRecipients: string[] = []): T {
    const groupId = this.groupId;
    if (!groupId) throw new ApiError("internal", "room not ready");
    const events: Event[] = [];
    const hints: (Record<string, unknown> | undefined)[] = [];
    const startSeq = this.lastSeq;
    const tx: RoomTx = {
      nextSeq: () => this.lastSeq + 1,
      append: (d: EventDraft) => {
        const seq = this.lastSeq + 1;
        const at = Date.now();
        this.sql.exec(
          "INSERT INTO events (seq, type, at, actor_id, topic_id, message_id, data) VALUES (?, ?, ?, ?, ?, ?, ?)",
          seq,
          d.type,
          at,
          d.actorId,
          d.topicId ?? null,
          d.messageId ?? null,
          JSON.stringify(d.data),
        );
        for (const w of eventWriters) {
          w({ seq, type: d.type, at, actorId: d.actorId, topicId: d.topicId, messageId: d.messageId, data: d.data }, this.sql);
        }
        this.lastSeq = seq;
        const event = { seq, groupId, at, actorId: d.actorId, type: d.type, data: d.data } as Event;
        events.push(event);
        hints.push(d.hints);
        return event;
      },
    };
    let value: T;
    try {
      value = this.ctx.storage.transactionSync(() => fn(tx));
    } catch (err) {
      this.lastSeq = startSeq;
      throw err;
    }
    if (events.length > 0) this.afterCommit(events, hints, extraRecipients);
    return value;
  }

  private afterCommit(
    events: Event[],
    hints: (Record<string, unknown> | undefined)[],
    extraRecipients: string[],
  ): void {
    const recipients = new Set([...this.members, ...extraRecipients]);
    events.forEach((event, i) => {
      const dc = { groupId: this.groupId as string, groupKind: this.groupKind, sql: this.sql, hints: hints[i] };
      for (const id of recipients) {
        let frame: Record<string, unknown> = { t: "event", event };
        for (const d of eventDecorators) {
          try {
            const add = d(event, id, dc);
            if (add) frame = { ...frame, ...add };
          } catch (err) {
            this.log.error("event decorator failed", { err, type: event.type, seq: event.seq });
          }
        }
        this.enqueue(id, frame);
      }
    });
    if (afterCommitHooks.length === 0) return;
    const h: HookContext = {
      env: this.env,
      groupId: this.groupId as string,
      groupKind: this.groupKind,
      memberIds: [...this.members],
      log: this.log,
      sql: this.sql,
      waitUntil: (p) => this.ctx.waitUntil(p),
      wakeAt: (at) => this.wakeAt(at),
    };
    for (const event of events) {
      for (const hook of afterCommitHooks) {
        this.ctx.waitUntil(
          (async () => {
            try {
              await hook(event, h);
            } catch (err) {
              this.log.error("after-commit hook failed", { err, type: event.type, seq: event.seq });
            }
          })(),
        );
      }
    }
  }

  /** Per-member ordered outbox: one delivery in flight per gateway, batched; never blocks others. */
  private enqueue(memberId: string, frame: unknown): void {
    let box = this.outbox.get(memberId);
    if (!box) {
      box = { queue: [], busy: false };
      this.outbox.set(memberId, box);
    }
    box.queue.push(frame);
    if (!box.busy) this.ctx.waitUntil(this.drain(memberId, box));
  }

  private async drain(memberId: string, box: { queue: unknown[]; busy: boolean }): Promise<void> {
    box.busy = true;
    try {
      while (box.queue.length > 0) {
        const batch = box.queue.splice(0, DELIVER_BATCH);
        let delivered = false;
        for (let attempt = 1; attempt <= DELIVER_TRIES && !delivered; attempt++) {
          try {
            await gatewayStub(this.env, memberId).deliver(batch);
            delivered = true;
          } catch (err) {
            this.log.warn("deliver failed", { memberId, err, frames: batch.length, attempt });
            if (attempt < DELIVER_TRIES) await new Promise((r) => setTimeout(r, DELIVER_RETRY_MS * attempt));
          }
        }
        // A dead gateway must not stall the others. Frames are lost, so ask the gateway to push
        // the group's current lastSeq (a directory frame): the client then fetches the gap.
        if (!delivered) this.ctx.waitUntil(this.resync(memberId));
      }
    } finally {
      box.busy = false;
      if (box.queue.length === 0) this.outbox.delete(memberId);
    }
  }

  private async resync(memberId: string): Promise<void> {
    const groupId = this.groupId as string;
    for (let attempt = 1; attempt <= DELIVER_TRIES; attempt++) {
      await new Promise((r) => setTimeout(r, 2000 * attempt));
      try {
        await gatewayStub(this.env, memberId).groupChanged(memberId, groupId);
        return;
      } catch (err) {
        this.log.warn("resync failed", { memberId, err, attempt });
      }
    }
  }

  /**
   * Runs the writes of one gateway socket in the order the gateway numbered them. The gateway
   * starts each write RPC at once, when its frame arrives (see `MemberGateway.webSocketMessage`),
   * so writes may overlap; a write whose predecessor has not run yet waits for it, at most
   * `TURN_WAIT_MS`.
   */
  private async ordered<T>(order: WriteOrder | undefined, fn: () => Promise<T>): Promise<T> {
    if (!order) return fn();
    const now = Date.now();
    let turn = this.turns.get(order.key);
    if (!turn) {
      if (this.turns.size >= MAX_TURNS) {
        for (const [k, t] of this.turns) if (now - t.at > TURN_IDLE_MS && t.waiters.size === 0) this.turns.delete(k);
      }
      // Numbers start at 1 per gateway epoch and socket, so a new turn waits for write 1 even
      // when a later write arrives first. (After a room restart mid-stream, the first write
      // waits once, at most TURN_WAIT_MS.)
      turn = { next: 1, waiters: new Map(), at: now };
      this.turns.set(order.key, turn);
    }
    const t = turn;
    if (order.n > t.next) {
      await new Promise<void>((resolve) => {
        const timer = setTimeout(() => {
          t.waiters.delete(order.n);
          this.log.warn("write turn timed out", { expected: t.next, n: order.n });
          resolve();
        }, TURN_WAIT_MS);
        t.waiters.set(order.n, () => {
          clearTimeout(timer);
          resolve();
        });
      });
    }
    try {
      return await fn();
    } finally {
      t.next = Math.max(t.next, order.n + 1);
      t.at = Date.now();
      const w = t.waiters.get(t.next);
      if (w) {
        t.waiters.delete(t.next);
        w();
      }
    }
  }

  private broadcast(frame: unknown, opts: { to?: string[]; except?: string } = {}): void {
    for (const id of opts.to ?? this.members) {
      if (id !== opts.except) this.enqueue(id, frame);
    }
  }

  private roomContext(): RoomContext {
    return {
      groupId: this.groupId as string,
      env: this.env,
      sql: this.sql,
      log: this.log,
      memberIds: () => [...this.members],
      isMember: (id) => this.members.has(id),
      groupKind: this.groupKind,
      commit: (fn) => this.commit(fn),
      broadcast: (frame, opts) => this.broadcast(frame, opts),
      waitUntil: (p) => this.ctx.waitUntil(p),
      wakeAt: (at) => this.wakeAt(at),
      serial: (key, fn) => this.serial(key, fn),
    };
  }

  // cards2: one read-await-write of a message at a time (edits, card patches, answers).
  private readonly serials = new Map<string, Promise<unknown>>();
  private serial<T>(key: string, fn: () => Promise<T>): Promise<T> {
    const prev = this.serials.get(key) ?? Promise.resolve();
    const run = prev.then(fn, fn);
    const tail = run.then(
      () => undefined,
      () => undefined,
    );
    this.serials.set(key, tail);
    void tail.then(() => {
      if (this.serials.get(key) === tail) this.serials.delete(key);
    });
    return run;
  }

  /** Sets the alarm to `at` unless it is already set earlier. */
  private wakeAt(at: number): void {
    this.ctx.waitUntil(
      (async () => {
        const cur = await this.ctx.storage.getAlarm();
        if (cur === null || cur > at) await this.ctx.storage.setAlarm(at);
      })().catch((err) => this.log.error("set alarm failed", { err })),
    );
  }

  /** Feature retries (`registerRoomAlarm`). */
  async alarm(): Promise<void> {
    if (!this.groupId) return;
    const room = this.roomContext();
    for (const h of roomAlarmHandlers) {
      try {
        await h(room);
      } catch (err) {
        this.log.error("room alarm handler failed", { err });
      }
    }
  }

  // -------------------------------------------------------------------------
  // Row mapping
  // -------------------------------------------------------------------------

  private toTopic(r: TopicRow): Topic {
    const t: Topic = {
      id: r.id,
      groupId: this.groupId as string,
      title: r.title,
      status: r.status,
      createdBy: r.created_by,
      createdAt: r.created_at,
      rootMessageId: r.root_message_id,
      lastSeq: r.last_seq,
      messageCount: r.message_count,
      waitingOn: JSON.parse(r.waiting_on ?? "[]") as string[],
      ownerId: r.owner_id ?? null,
    };
    if (r.last_message_at !== null && r.last_message_at !== undefined) t.lastMessageAt = r.last_message_at;
    if (r.context) t.contextOf = JSON.parse(r.context) as TopicContext;
    return t;
  }

  private toMessage(r: MessageRow): Message {
    const m: Message = {
      ...(r.ext ? (JSON.parse(r.ext) as Record<string, unknown>) : {}),
      id: r.id,
      clientMsgId: r.client_msg_id,
      groupId: this.groupId as string,
      topicId: r.topic_id,
      authorId: r.author_id,
      body: r.body,
      mentions: JSON.parse(r.mentions) as string[],
      createdSeq: r.created_seq,
      createdAt: r.created_at,
      editedAt: r.edited_at,
      deletedAt: r.deleted_at,
      kind: r.kind ?? "text",
    };
    if (r.streaming) m.streaming = true;
    return m;
  }

  private toEvent(r: EventRow): Event {
    return {
      seq: r.seq,
      groupId: this.groupId as string,
      at: r.at,
      actorId: r.actor_id,
      type: r.type,
      data: JSON.parse(r.data),
    } as Event;
  }

  private topicRow(id: string): TopicRow | undefined {
    return this.sql.exec<TopicRow>("SELECT * FROM topics WHERE id = ?", id).toArray()[0] as TopicRow | undefined;
  }

  private messageRow(id: string): MessageRow | undefined {
    return this.sql.exec<MessageRow>("SELECT * FROM messages WHERE id = ?", id).toArray()[0] as
      | MessageRow
      | undefined;
  }

  private touchTopic(topicId: string, seq: number, addMessage = false): void {
    this.sql.exec(
      "UPDATE topics SET last_seq = ?, message_count = message_count + ? WHERE id = ?",
      seq,
      addMessage ? 1 : 0,
      topicId,
    );
  }

  // -------------------------------------------------------------------------
  // Membership (called by the API Worker, which owns D1)
  // -------------------------------------------------------------------------

  /** Binds a new group's room and its initial members. Writes no events. Idempotent. */
  async init(groupId: string, memberIds: string[]): Promise<Result<{ lastSeq: number }>> {
    return run(() => {
      if (this.groupId !== null && this.groupId !== groupId) throw new ApiError("internal", "room bound to another group");
      this.bind(groupId, memberIds);
      return { lastSeq: this.lastSeq };
    });
  }

  async memberAdded(groupId: string, actorId: string, memberId: string): Promise<Result<{ seq: number | null }>> {
    return run(async () => {
      await this.ready(groupId);
      if (this.members.has(memberId)) return { seq: null };
      const event = this.commit((tx) => {
        // Everything before the member joined counts as read (its own memberAdded event too).
        this.sql.exec(
          `INSERT INTO room_members (member_id, added_at, joined_seq) VALUES (?, ?, ?)
           ON CONFLICT (member_id) DO UPDATE SET joined_seq = excluded.joined_seq`,
          memberId,
          Date.now(),
          tx.nextSeq(),
        );
        this.members.add(memberId);
        return tx.append({ type: "group.memberAdded", actorId, data: { memberId } });
      });
      return { seq: event.seq };
    });
  }

  async memberRemoved(groupId: string, actorId: string, memberId: string): Promise<Result<{ seq: number | null }>> {
    return run(async () => {
      await this.ready(groupId);
      if (!this.members.has(memberId)) return { seq: null };
      const event = this.commit(
        (tx) => {
          this.sql.exec("DELETE FROM room_members WHERE member_id = ?", memberId);
          this.members.delete(memberId);
          return tx.append({ type: "group.memberRemoved", actorId, data: { memberId } });
        },
        [memberId],
      );
      return { seq: event.seq };
    });
  }

  /**
   * Renames the group in its log. `before` is the name in D1, used while the room has no name
   * of its own yet (no rename since it was made). The same name again appends nothing, so a
   * retry after a failed D1 write is safe. Returns the new event's seq (null = no change).
   */
  async groupUpdated(
    groupId: string,
    actorId: string,
    name: string,
    before: string,
  ): Promise<Result<{ seq: number | null; lastSeq: number }>> {
    return run(async () => {
      await this.ready(groupId);
      const row = this.sql.exec<{ value: string }>("SELECT value FROM meta WHERE key = 'group_name'").toArray()[0];
      if ((row?.value ?? before) === name) return { seq: null, lastSeq: this.lastSeq };
      const event = this.commit((tx) => {
        this.sql.exec(
          "INSERT OR REPLACE INTO meta (key, value) VALUES ('group_name', ?)",
          name,
        );
        return tx.append({ type: "group.updated", actorId, data: { name } });
      });
      return { seq: event.seq, lastSeq: this.lastSeq };
    });
  }

  async memberIds(groupId: string): Promise<Result<string[]>> {
    return run(async () => {
      await this.ready(groupId);
      return [...this.members];
    });
  }

  // -------------------------------------------------------------------------
  // Frames (called by MemberGateway after its own membership check)
  // -------------------------------------------------------------------------

  async summary(groupId: string, memberId: string, readSeq: number): Promise<Result<RoomCounts>> {
    return run(async () => {
      await this.ready(groupId);
      this.requireMember(memberId);
      const read = Math.min(readSeq, this.lastSeq);
      const state = groupReadState(this.sql, memberId, read);
      const prefs = getPrefs(this.sql, memberId, this.groupKind);
      return { lastSeq: this.lastSeq, ...state, muted: prefs.muted, notify: prefs.notify };
    });
  }

  /** Moves one topic's read position forward (`read` with `topicId`). */
  async readTopic(
    groupId: string,
    memberId: string,
    topicId: string,
    seq: number,
    groupRead: number,
  ): Promise<Result<TopicReadResult>> {
    return run(async () => {
      await this.ready(groupId);
      this.requireMember(memberId);
      if (!this.topicRow(topicId)) throw new ApiError("not_found", "topic not found");
      const read = Math.min(groupRead, this.lastSeq);
      const moved = advanceTopicRead(this.sql, memberId, topicId, Math.min(seq, this.lastSeq));
      const { topics: _all, ...totals } = groupReadState(this.sql, memberId, read);
      return { ...totals, changed: moved ? topicState(this.sql, memberId, topicId, read) : null };
    });
  }

  /** Validates a quote and fills its author and excerpt. */
  private replyTo(messageId: string): { messageId: string; authorId: string; excerpt: string } {
    const q = this.messageRow(messageId);
    if (!q) throw new ApiError("not_found", "quoted message not found");
    if (q.deleted_at !== null) throw new ApiError("invalid", "quoted message is deleted");
    const card = q.ext ? ((JSON.parse(q.ext) as { card?: Card }).card ?? null) : null;
    return { messageId: q.id, authorId: q.author_id, excerpt: excerpt(q.body !== "" ? q.body : (card?.title ?? "")) };
  }

  async send(groupId: string, actor: Actor, frame: SendFrame, order?: WriteOrder): Promise<Result<SendResult>> {
    return this.ordered(order, () => this.sendNow(groupId, actor, frame));
  }

  /**
   * `send.context` (side chats, docs/protocol.md): only in a DM. A new topic gets it as
   * `contextOf`; in an existing side chat it must name the same topic and returns the context to
   * store only when `uptoSeq` grew. The cross-group checks run in the sidechat frame validator.
   */
  private sideChatContext(frame: SendFrame): TopicContext | undefined {
    const c = frame.context;
    if (!c) return undefined;
    if (this.groupKind !== "dm") throw new ApiError("invalid", "context is only for a side chat in a DM");
    if (!frame.topicId) return { groupId: c.groupId, topicId: c.topicId, uptoSeq: c.uptoSeq };
    const row = this.topicRow(frame.topicId);
    const cur = row?.context ? (JSON.parse(row.context) as TopicContext) : null;
    if (!cur || cur.groupId !== c.groupId || cur.topicId !== c.topicId) {
      throw new ApiError("invalid", "context must name the side chat's own topic");
    }
    return c.uptoSeq > cur.uptoSeq ? { groupId: c.groupId, topicId: c.topicId, uptoSeq: c.uptoSeq } : undefined;
  }

  private async sendNow(groupId: string, actor: Actor, frame: SendFrame): Promise<Result<SendResult>> {
    return run(async () => {
      await this.ready(groupId);
      this.requireMember(actor.id);

      const existing = this.sql
        .exec<MessageRow>("SELECT * FROM messages WHERE author_id = ? AND client_msg_id = ?", actor.id, frame.clientMsgId)
        .toArray()[0] as MessageRow | undefined;
      if (existing) {
        const topic = this.topicRow(existing.topic_id);
        const result: SendResult = { message: this.toMessage(existing), duplicate: true };
        if (topic && topic.root_message_id === existing.id) result.topic = this.toTopic(topic);
        return result;
      }

      const mentions = [...new Set(frame.mentions ?? [])];
      for (const id of mentions) {
        if (!this.members.has(id)) throw new ApiError("invalid", "mention of a member outside this group");
      }
      if (frame.topicId && !this.topicRow(frame.topicId)) throw new ApiError("not_found", "topic not found");

      const room = this.roomContext();
      let ext: Record<string, unknown> | undefined;
      if (frame.replyTo) ext = { replyTo: this.replyTo(frame.replyTo.messageId) };
      // A Card v2 travels next to a v1 card that 1.0.x clients read (docs/protocol.md, "Cards v2").
      const card = frame.card ?? (frame.cardV2 ? cardV2ToV1(frame.cardV2) : undefined);
      const kind: MessageKind = card ? "card" : "text";
      if (card) ext = { ...ext, card };
      if (frame.cardV2) ext = { ...ext, cardV2: frame.cardV2 };
      for (const e of messageExtensions) {
        const add = e(frame as Record<string, unknown>, room, actor);
        if (add) ext = { ...ext, ...add };
      }
      let body = card && frame.body === "" ? frame.cardV2?.fallback || card.title : frame.body;
      const context = this.sideChatContext(frame);
      const writes: MessageWriteResult[] = [];
      for (const h of messageWriteHooks) {
        if (!h.send) continue;
        const draft = { body, ext };
        const w = await h.send(frame as Record<string, unknown>, room, actor, draft);
        body = draft.body;
        ext = draft.ext;
        if (w) writes.push(w);
      }

      const now = Date.now();
      const messageId = ulid(now);
      const streaming = frame.streaming === true;

      const sent = this.commit((tx) => {
        let topicId = frame.topicId;
        let createdTopic: Topic | undefined;
        if (!topicId) {
          topicId = ulid(now);
          const title = frame.title ?? defaultTitle(body);
          const seq = tx.nextSeq();
          this.sql.exec(
            `INSERT INTO topics (id, title, status, created_by, created_at, root_message_id, last_seq, message_count,
                                 owner_id, last_message_at, context)
             VALUES (?, ?, 'open', ?, ?, ?, ?, 0, ?, ?, ?)`,
            topicId,
            title,
            actor.id,
            now,
            messageId,
            seq,
            actor.id,
            now,
            context ? JSON.stringify(context) : null,
          );
          const topic = this.toTopic(this.topicRow(topicId) as TopicRow);
          tx.append({ type: "topic.created", actorId: actor.id, topicId, data: { topic } });
        } else {
          this.sql.exec("UPDATE topics SET last_message_at = ? WHERE id = ?", now, topicId);
          if (context) {
            // The side chat reads its topic further now.
            const s2 = tx.nextSeq();
            this.sql.exec("UPDATE topics SET context = ?, last_seq = ? WHERE id = ?", JSON.stringify(context), s2, topicId);
            tx.append({ type: "topic.updated", actorId: actor.id, topicId, data: { topicId, contextOf: context } });
          }
        }
        const seq = tx.nextSeq();
        this.sql.exec(
          `INSERT INTO messages (id, client_msg_id, topic_id, author_id, body, mentions, created_seq, created_at, streaming, ext, kind)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
          messageId,
          frame.clientMsgId,
          topicId,
          actor.id,
          body,
          JSON.stringify(mentions),
          seq,
          now,
          streaming ? 1 : 0,
          ext ? JSON.stringify(ext) : null,
          kind,
        );
        for (const id of mentions) {
          this.sql.exec("INSERT OR IGNORE INTO mentions (member_id, seq, message_id) VALUES (?, ?, ?)", id, seq, messageId);
        }
        // Auto-follow: the author (and creator) and every mentioned member.
        for (const id of new Set([actor.id, ...mentions])) setFollowing(this.sql, id, topicId, true);
        const message = this.toMessage(this.messageRow(messageId) as MessageRow);
        tx.append({ type: "message.created", actorId: actor.id, topicId, messageId, data: { message } });
        this.touchTopic(topicId, seq, true);
        for (const w of writes) w.inTx?.(tx, message);
        if (!frame.topicId) createdTopic = this.toTopic(this.topicRow(topicId) as TopicRow);
        const result: SendResult = { message };
        if (createdTopic) result.topic = createdTopic;
        return result;
      });
      for (const w of writes) w.after?.(sent.message);
      return sent;
    });
  }

  async edit(groupId: string, actor: Actor, frame: EditFrame, order?: WriteOrder): Promise<Result<{ message: Message }>> {
    return this.ordered(order, () => this.editNow(groupId, actor, frame));
  }

  private async editNow(groupId: string, actor: Actor, frame: EditFrame): Promise<Result<{ message: Message }>> {
    return run(async () => {
      await this.ready(groupId);
      this.requireMember(actor.id);
      // Card writes of this message wait for this edit (its hooks await) and the reverse.
      const { edited, writes } = await this.serial(`message:${frame.messageId}`, () => this.editLocked(actor, frame));
      for (const w of writes) w.after?.(edited.message);
      return edited;
    });
  }

  private async editLocked(actor: Actor, frame: EditFrame): Promise<{ edited: { message: Message }; writes: MessageWriteResult[] }> {
    const row = this.messageRow(frame.messageId);
    if (!row) throw new ApiError("not_found", "message not found");
    if (row.author_id !== actor.id) throw new ApiError("forbidden", "only the author can edit");
    if (row.deleted_at !== null) throw new ApiError("conflict", "message is deleted");
    if ((frame.card || frame.cardV2) && row.kind !== "card") throw new ApiError("invalid", "only a card message can get a card");
    const current = this.toMessage(row);
    let ext: Record<string, unknown> = row.ext ? (JSON.parse(row.ext) as Record<string, unknown>) : {};
    const changed: Record<string, unknown> = {};
    if (frame.card) changed.card = frame.card;
    const room = this.roomContext();
    for (const e of editExtensions) {
      const add = e(frame as Record<string, unknown>, room, actor, current);
      if (add) Object.assign(changed, add);
    }
    const merged = { ...ext, ...changed };
    const card = (merged.card as Card | undefined) ?? null;
    const fallback = (merged.cardV2 as CardV2 | undefined)?.fallback;
    const draft = { body: row.kind === "card" && frame.body === "" && card ? fallback || card.title : frame.body, changed };
    // Card v2 revs, causes and refs (cards2 feature).
    const writes: MessageWriteResult[] = [];
    for (const h of messageWriteHooks) {
      if (!h.edit) continue;
      const w = await h.edit(frame as Record<string, unknown>, room, actor, current, draft);
      if (w) writes.push(w);
    }
    ext = { ...ext, ...changed };
    const body = draft.body;
    const streaming = row.streaming === 1 && frame.streaming === true;
    if (!streaming) this.dropDelta(row.id);
    const now = Date.now();
    const edited = this.commit((tx) => {
      // The hooks awaited: a delete may have come in between.
      if (this.messageRow(row.id)?.deleted_at !== null) throw new ApiError("conflict", "message is deleted");
      const seq = tx.nextSeq();
      this.sql.exec(
        "UPDATE messages SET body = ?, edited_at = ?, streaming = ?, ext = ? WHERE id = ?",
        body,
        now,
        streaming ? 1 : 0,
        Object.keys(ext).length > 0 ? JSON.stringify(ext) : null,
        row.id,
      );
      tx.append({
        type: "message.updated",
        actorId: actor.id,
        topicId: row.topic_id,
        messageId: row.id,
        data: { ...changed, messageId: row.id, body, editedAt: now, streaming },
      });
      this.touchTopic(row.topic_id, seq);
      const message = this.toMessage(this.messageRow(row.id) as MessageRow);
      for (const w of writes) w.inTx?.(tx, message);
      return { message };
    });
    return { edited, writes };
  }

  async remove(
    groupId: string,
    actor: Actor,
    messageId: string,
    order?: WriteOrder,
  ): Promise<Result<{ messageId: string; deletedAt: number }>> {
    return this.ordered(order, () => this.removeNow(groupId, actor, messageId));
  }

  private async removeNow(
    groupId: string,
    actor: Actor,
    messageId: string,
  ): Promise<Result<{ messageId: string; deletedAt: number }>> {
    return run(async () => {
      await this.ready(groupId);
      this.requireMember(actor.id);
      const row = this.messageRow(messageId);
      if (!row) throw new ApiError("not_found", "message not found");
      if (row.author_id !== actor.id && actor.level !== "owner") {
        throw new ApiError("forbidden", "only the author or an owner can delete");
      }
      if (row.deleted_at !== null) return { messageId, deletedAt: row.deleted_at };
      this.dropDelta(messageId);
      const now = Date.now();
      const removed = this.toMessage(row);
      const room = this.roomContext();
      return this.commit((tx) => {
        const seq = tx.nextSeq();
        // Scrub the content from the row, stored events and quotes, so the log no longer serves it.
        scrubDeletedMessage(this.sql, messageId, now);
        tx.append({
          type: "message.deleted",
          actorId: actor.id,
          topicId: row.topic_id,
          messageId,
          data: { messageId, deletedAt: now },
        });
        this.touchTopic(row.topic_id, seq);
        for (const h of messageWriteHooks) h.remove?.(room, tx, removed, actor.id);
        // Deleting the newest message moves the topic's last activity back.
        const topic = this.topicRow(row.topic_id);
        if (topic && topic.last_message_at !== null && row.created_at >= topic.last_message_at) {
          const newest = this.sql
            .exec<{ at: number | null }>(
              "SELECT max(created_at) AS at FROM messages WHERE topic_id = ? AND deleted_at IS NULL",
              row.topic_id,
            )
            .one().at;
          if (newest !== topic.last_message_at) {
            const s2 = tx.nextSeq();
            this.sql.exec("UPDATE topics SET last_message_at = ?, last_seq = ? WHERE id = ?", newest, s2, row.topic_id);
            const data: { topicId: string; lastMessageAt?: number } = { topicId: row.topic_id };
            if (newest !== null) data.lastMessageAt = newest;
            tx.append({ type: "topic.updated", actorId: actor.id, topicId: row.topic_id, data });
          }
        }
        return { messageId, deletedAt: now };
      });
    });
  }

  async topicUpdate(groupId: string, actor: Actor, frame: TopicUpdateFrame, order?: WriteOrder): Promise<Result<{ topic: Topic }>> {
    return this.ordered(order, () => this.topicUpdateNow(groupId, actor, frame));
  }

  private async topicUpdateNow(groupId: string, actor: Actor, frame: TopicUpdateFrame): Promise<Result<{ topic: Topic }>> {
    return run(async () => {
      await this.ready(groupId);
      this.requireMember(actor.id);
      const row = this.topicRow(frame.topicId);
      if (!row) throw new ApiError("not_found", "topic not found");
      if (
        frame.title === undefined &&
        frame.status === undefined &&
        frame.waitingOn === undefined &&
        frame.ownerId === undefined
      ) {
        throw new ApiError("invalid", "nothing to update");
      }
      const waitingOn = frame.waitingOn === undefined ? undefined : [...new Set(frame.waitingOn)];
      for (const id of [...(waitingOn ?? []), ...(frame.ownerId ? [frame.ownerId] : [])]) {
        if (!this.members.has(id)) throw new ApiError("invalid", "waitingOn and ownerId must be members of this group");
      }
      const before = JSON.parse(row.waiting_on ?? "[]") as string[];
      const waitingAdded = (waitingOn ?? []).filter((id) => !before.includes(id));
      return this.commit((tx) => {
        const seq = tx.nextSeq();
        this.sql.exec(
          "UPDATE topics SET title = ?, status = ?, waiting_on = ?, owner_id = ?, last_seq = ? WHERE id = ?",
          frame.title ?? row.title,
          frame.status ?? row.status,
          waitingOn === undefined ? row.waiting_on : JSON.stringify(waitingOn),
          frame.ownerId === undefined ? row.owner_id : frame.ownerId,
          seq,
          row.id,
        );
        // Auto-follow: members just added to waitingOn and a new owner.
        const follow = new Set(waitingAdded);
        if (frame.ownerId && frame.ownerId !== row.owner_id) follow.add(frame.ownerId);
        for (const id of follow) setFollowing(this.sql, id, row.id, true);
        const data: {
          topicId: string;
          title?: string;
          status?: TopicStatus;
          waitingOn?: string[];
          ownerId?: string | null;
        } = { topicId: row.id };
        if (frame.title !== undefined) data.title = frame.title;
        if (frame.status !== undefined) data.status = frame.status;
        if (waitingOn !== undefined) data.waitingOn = waitingOn;
        if (frame.ownerId !== undefined) data.ownerId = frame.ownerId;
        tx.append({ type: "topic.updated", actorId: actor.id, topicId: row.id, data, hints: { waitingAdded } });
        return { topic: this.toTopic(this.topicRow(row.id) as TopicRow) };
      });
    });
  }

  /** A card button press (`card.action`): stored as `card.actioned` with the action's own value. */
  async cardAction(
    groupId: string,
    actor: Actor,
    messageId: string,
    actionId: string,
  ): Promise<Result<{ seq: number }>> {
    return run(async () => {
      await this.ready(groupId);
      this.requireMember(actor.id);
      const row = this.messageRow(messageId);
      if (!row || row.deleted_at !== null || row.kind !== "card") throw new ApiError("not_found", "card message not found");
      const stored = row.ext ? (JSON.parse(row.ext) as { card?: Card; cardV2?: CardV2 }) : {};
      // A v1 press on a v2 card is `card.event action.press` at the current rev (cards2 feature).
      const press = stored.cardV2 ? roomRpcHandlers.get("cards.press") : undefined;
      if (stored.cardV2 && press) return (await press(this.roomContext(), actor, { messageId, actionId })) as { seq: number };
      const card = stored.card;
      const action = card?.actions?.find((a) => a.id === actionId);
      if (!action) throw new ApiError("not_found", "card action not found");
      const key = `${actor.id}:${messageId}`;
      const now = Date.now();
      const last = this.cardPressAt.get(key);
      if (last !== undefined && now - last < CARD_PRESS_INTERVAL_MS) throw new ApiError("rate_limited", "one press per second");
      this.cardPressAt.set(key, now);
      if (this.cardPressAt.size > 1000) {
        for (const [k, t] of this.cardPressAt) if (now - t >= CARD_PRESS_INTERVAL_MS) this.cardPressAt.delete(k);
      }
      const data: { messageId: string; actionId: string; value?: unknown } = { messageId, actionId };
      if (action.value !== undefined) data.value = action.value;
      const event = this.commit((tx) => {
        const seq = tx.nextSeq();
        this.touchTopic(row.topic_id, seq);
        return tx.append({ type: "card.actioned", actorId: actor.id, topicId: row.topic_id, messageId, data });
      });
      return { seq: event.seq };
    });
  }

  /** Transient. Returns sent=false when throttled (one per 3 s per member and topic). */
  async typing(groupId: string, actor: Actor, topicId: string): Promise<Result<{ sent: boolean }>> {
    return run(async () => {
      await this.ready(groupId);
      this.requireMember(actor.id);
      if (!this.topicRow(topicId)) throw new ApiError("not_found", "topic not found");
      const key = `${actor.id}:${topicId}`;
      const now = Date.now();
      const last = this.typingAt.get(key);
      if (last !== undefined && now - last < LIMITS.typingIntervalMs) return { sent: false };
      this.typingAt.set(key, now);
      if (this.typingAt.size > 1000) {
        for (const [k, t] of this.typingAt) if (now - t >= LIMITS.typingIntervalMs) this.typingAt.delete(k);
      }
      this.broadcast(
        { t: "transient", event: { type: "typing", groupId, topicId, memberId: actor.id, at: now } },
        { except: actor.id },
      );
      return { sent: true };
    });
  }

  /** Streaming delta from the message's author. Coalesced every 100 ms or 8 KiB; never stored. */
  async delta(groupId: string, actor: Actor, messageId: string, append: string, order?: WriteOrder): Promise<Result<{ buffered: number }>> {
    return this.ordered(order, () => this.deltaNow(groupId, actor, messageId, append));
  }

  private async deltaNow(groupId: string, actor: Actor, messageId: string, append: string): Promise<Result<{ buffered: number }>> {
    return run(async () => {
      await this.ready(groupId);
      this.requireMember(actor.id);
      const row = this.messageRow(messageId);
      if (!row) throw new ApiError("not_found", "message not found");
      if (row.author_id !== actor.id) throw new ApiError("forbidden", "only the author can stream");
      if (!row.streaming || row.deleted_at !== null) throw new ApiError("conflict", "message is not streaming");
      let d = this.deltas.get(messageId);
      if (!d) {
        d = { topicId: row.topic_id, buf: "", bytes: 0, timer: null };
        this.deltas.set(messageId, d);
      }
      d.buf += append;
      d.bytes += new TextEncoder().encode(append).byteLength;
      if (d.bytes >= DELTA_FLUSH_BYTES) {
        this.flushDelta(messageId);
      } else if (!d.timer) {
        d.timer = setTimeout(() => this.flushDelta(messageId), DELTA_FLUSH_MS);
      }
      return { buffered: d.bytes };
    });
  }

  private flushDelta(messageId: string): void {
    const d = this.deltas.get(messageId);
    if (!d) return;
    if (d.timer) clearTimeout(d.timer);
    this.deltas.delete(messageId);
    if (d.buf === "") return;
    // Keep the text so far, so members who load the message later see it (no event).
    try {
      this.sql.exec(
        "UPDATE messages SET body = body || ? WHERE id = ? AND streaming = 1 AND deleted_at IS NULL AND length(body) + length(?) <= ?",
        d.buf,
        messageId,
        d.buf,
        MAX_BODY_CHARS,
      );
      this.sql.exec(
        `UPDATE events SET data = json_set(data, '$.message.body', json_extract(data, '$.message.body') || ?)
         WHERE message_id = ? AND type = 'message.created'
           AND EXISTS (SELECT 1 FROM messages WHERE id = ? AND streaming = 1 AND deleted_at IS NULL
                       AND body = json_extract(events.data, '$.message.body') || ?)`,
        d.buf,
        messageId,
        messageId,
        d.buf,
      );
    } catch (err) {
      this.log.warn("delta store failed", { messageId, err });
    }
    this.broadcast({
      t: "transient",
      event: { type: "message.delta", groupId: this.groupId, topicId: d.topicId, messageId, append: d.buf },
    });
  }

  private dropDelta(messageId: string): void {
    // Pending deltas are flushed before the final edit so clients see them in order.
    this.flushDelta(messageId);
  }

  async fetchEvents(groupId: string, actor: Actor, afterSeq: number, limit?: number): Promise<Result<PageResult>> {
    return run(async () => {
      await this.ready(groupId);
      this.requireMember(actor.id);
      return this.page(afterSeq, Math.min(limit ?? DEFAULT_FETCH_LIMIT, 500));
    });
  }

  private page(afterSeq: number, limit: number, maxBytes = MAX_PAGE_BYTES): PageResult {
    const rows = this.sql
      .exec<EventRow>("SELECT seq, type, at, actor_id, data FROM events WHERE seq > ? ORDER BY seq LIMIT ?", afterSeq, limit + 1)
      .toArray() as EventRow[];
    const n = fitBytes(rows, limit, maxBytes, (r) => r.data.length);
    return { events: rows.slice(0, n).map((r) => this.toEvent(r)), hasMore: rows.length > n, lastSeq: this.lastSeq };
  }

  async history(
    groupId: string,
    actor: Actor,
    topicId: string,
    beforeSeq?: number,
    limit?: number,
  ): Promise<Result<{ messages: Message[]; hasMore: boolean }>> {
    return run(async () => {
      await this.ready(groupId);
      this.requireMember(actor.id);
      if (!this.topicRow(topicId)) throw new ApiError("not_found", "topic not found");
      const n = Math.min(limit ?? DEFAULT_HISTORY_LIMIT, 500);
      const rows = this.sql
        .exec<MessageRow>(
          "SELECT * FROM messages WHERE topic_id = ? AND created_seq < ? ORDER BY created_seq DESC LIMIT ?",
          topicId,
          beforeSeq ?? Number.MAX_SAFE_INTEGER,
          n + 1,
        )
        .toArray() as MessageRow[];
      const k = fitBytes(rows, n, MAX_PAGE_BYTES, (r) => r.body.length + (r.ext?.length ?? 0));
      return { messages: rows.slice(0, k).map((r) => this.toMessage(r)), hasMore: rows.length > k };
    });
  }

  /** Presence change of a member, reported by its gateway. Fans out a transient event. */
  async presence(groupId: string, memberId: string, online: boolean): Promise<Result<{ sent: boolean }>> {
    return run(async () => {
      await this.ready(groupId);
      if (!this.members.has(memberId)) return { sent: false };
      this.broadcast(
        { t: "transient", event: { type: "presence", memberId, online, at: Date.now() } },
        { except: memberId },
      );
      return { sent: true };
    });
  }

  /** Events after `afterSeq`, paged, for the daily archive. No membership check: internal only. */
  async exportEvents(
    groupId: string,
    afterSeq: number,
    limit = MAX_EXPORT_LIMIT,
  ): Promise<Result<PageResult & { groupId: string }>> {
    return run(async () => {
      await this.ready(groupId);
      return { groupId, ...this.page(afterSeq, Math.max(1, Math.min(limit, MAX_EXPORT_LIMIT)), Number.MAX_SAFE_INTEGER) };
    });
  }

  /** Stored event rows after `afterSeq` (with topic and message ids), for the daily archive. Internal. */
  async exportRows(
    groupId: string,
    afterSeq: number,
    limit = MAX_EXPORT_LIMIT,
  ): Promise<Result<{ rows: ArchivedEvent[]; hasMore: boolean; lastSeq: number }>> {
    return run(async () => {
      await this.ready(groupId);
      const n = Math.max(1, Math.min(limit, MAX_EXPORT_LIMIT));
      const rows = this.sql
        .exec<EventRow & { topic_id: string | null; message_id: string | null }>(
          "SELECT seq, type, at, actor_id, topic_id, message_id, data FROM events WHERE seq > ? ORDER BY seq LIMIT ?",
          afterSeq,
          n + 1,
        )
        .toArray();
      return {
        rows: rows.slice(0, n).map((r) => ({
          seq: r.seq,
          type: r.type,
          at: r.at,
          actorId: r.actor_id,
          topicId: r.topic_id,
          messageId: r.message_id,
          data: JSON.parse(r.data),
        })),
        hasMore: rows.length > n,
        lastSeq: this.lastSeq,
      };
    });
  }

  /**
   * Restore: appends archived events to a room that has no live events. Rows with seq <= lastSeq
   * are skipped (re-running a chunk is safe); the first new row must be lastSeq + 1. Registered
   * event importers rebuild the room tables. Nothing is fanned out. Internal (admin route only).
   */
  async importEvents(
    groupId: string,
    rows: ArchivedEvent[],
  ): Promise<Result<{ lastSeq: number; imported: number; skipped: number }>> {
    return run(async () => {
      await this.ready(groupId);
      const marker = this.sql
        .exec<{ value: string }>("SELECT value FROM meta WHERE key = 'import_last_seq'")
        .toArray()[0]?.value;
      if (this.lastSeq > 0 && Number(marker) !== this.lastSeq) {
        throw new ApiError("conflict", "room has live events; import needs an empty room");
      }
      const sorted = [...rows].sort((a, b) => a.seq - b.seq);
      const start = this.lastSeq;
      let imported = 0;
      let skipped = 0;
      try {
        this.ctx.storage.transactionSync(() => {
          for (const r of sorted) {
            if (r.seq <= this.lastSeq) {
              skipped++;
              continue;
            }
            if (r.seq !== this.lastSeq + 1) {
              throw new ApiError("invalid", `gap in archive: expected seq ${this.lastSeq + 1}, got ${r.seq}`);
            }
            this.sql.exec(
              "INSERT INTO events (seq, type, at, actor_id, topic_id, message_id, data) VALUES (?, ?, ?, ?, ?, ?, ?)",
              r.seq,
              r.type,
              r.at,
              r.actorId,
              r.topicId ?? null,
              r.messageId ?? null,
              JSON.stringify(r.data),
            );
            for (const importer of eventImporters) importer(r, this.sql);
            for (const w of eventWriters) w(r, this.sql);
            // A member added later starts with the history before its join read (memberAdded).
            if (r.type === "group.memberAdded" && typeof (r.data as { memberId?: unknown })?.memberId === "string") {
              this.sql.exec(
                "UPDATE room_members SET joined_seq = ? WHERE member_id = ?",
                r.seq,
                (r.data as { memberId: string }).memberId,
              );
            }
            this.lastSeq = r.seq;
            imported++;
          }
          this.sql.exec("INSERT OR REPLACE INTO meta (key, value) VALUES ('import_last_seq', ?)", String(this.lastSeq));
        });
      } catch (err) {
        this.lastSeq = start;
        throw err;
      }
      if (imported > 0) this.log.info("events imported", { imported, lastSeq: this.lastSeq });
      return { lastSeq: this.lastSeq, imported, skipped };
    });
  }

  /**
   * Point-in-time recovery of this room's SQLite (30 days). Without `apply` it only returns the
   * bookmark and the current lastSeq. With `apply` the object restarts on that bookmark: every
   * write after `at` is lost. Internal (admin route only); see docs/RUNBOOK.md.
   */
  async pointInTime(
    groupId: string,
    at: number,
    apply: boolean,
  ): Promise<Result<{ bookmark: string; lastSeq: number; applied: boolean }>> {
    const r = await run(async () => {
      await this.ready(groupId);
      let bookmark: string;
      try {
        bookmark = await this.ctx.storage.getBookmarkForTime(at);
      } catch {
        // Local dev (miniflare) has no point-in-time recovery.
        throw new ApiError("invalid", "point-in-time recovery is not available here");
      }
      if (apply) {
        await this.ctx.storage.onNextSessionRestoreBookmark(bookmark);
        this.log.warn("point-in-time restore scheduled", { at, lastSeq: this.lastSeq });
      }
      return { bookmark, lastSeq: this.lastSeq, applied: apply };
    });
    // Restart so the next request opens the restored database.
    if (r.ok && apply) this.ctx.waitUntil(Promise.resolve().then(() => this.ctx.abort("point-in-time restore")));
    return r;
  }

  /** Feature RPC registered with `registerRoomRpc`. */
  async rpc(groupId: string, name: string, actor: Actor | null, args: unknown): Promise<Result<unknown>> {
    return run(async () => {
      const h = roomRpcHandlers.get(name);
      if (!h) throw new ApiError("not_found", `unknown room rpc ${name}`);
      await this.ready(groupId);
      if (actor) this.requireMember(actor.id);
      return h(this.roomContext(), actor, args);
    });
  }
}
