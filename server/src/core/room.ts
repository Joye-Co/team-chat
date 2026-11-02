// GroupRoom: one per group. Owns the group's event log, topics and messages in its own SQLite,
// assigns seq in the same transaction as the writes, fans events out to member gateways after
// commit, and keeps typing and streaming deltas in memory only.
import { DurableObject } from "cloudflare:workers";
import {
  DEFAULT_TITLE_CHARS,
  MAX_EXCERPT_CHARS,
  type Card,
  type ClientFrameOf,
  type Event,
  type Message,
  type MessageKind,
  type NotifyLevel,
  type SendResult,
  type Topic,
  type TopicState,
  type TopicStatus,
} from "@chat/protocol";
import { groupMemberIds, getGroup } from "./db";
import { type Env, gatewayStub } from "./env";
import { ApiError, type Result, toErrorBody } from "./errors";
import { ulid } from "./ids";
import { LIMITS } from "./ratelimit";
import { logger as baseLogger, type Logger } from "./logger";
import { type MigrationStep, runMigrations } from "./migrations";
import {
  type Actor,
  type EventDraft,
  type HookContext,
  type RoomContext,
  type RoomTx,
  afterCommitHooks,
  editExtensions,
  eventDecorators,
  messageExtensions,
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
      memberIds: [...this.members],
      log: this.log,
      sql: this.sql,
      waitUntil: (p) => this.ctx.waitUntil(p),
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
        try {
          await gatewayStub(this.env, memberId).deliver(batch);
        } catch (err) {
          // Clients catch up by seq; a dead gateway must not stall the others.
          this.log.warn("deliver failed", { memberId, err, frames: batch.length });
        }
      }
    } finally {
      box.busy = false;
      if (box.queue.length === 0) this.outbox.delete(memberId);
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
    };
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
        this.sql.exec("INSERT OR IGNORE INTO room_members (member_id, added_at) VALUES (?, ?)", memberId, Date.now());
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

  async groupUpdated(groupId: string, actorId: string, name: string): Promise<Result<{ seq: number }>> {
    return run(async () => {
      await this.ready(groupId);
      const event = this.commit((tx) => tx.append({ type: "group.updated", actorId, data: { name } }));
      return { seq: event.seq };
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

  async send(groupId: string, actor: Actor, frame: SendFrame): Promise<Result<SendResult>> {
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
      const kind: MessageKind = frame.card ? "card" : "text";
      if (frame.card) ext = { ...ext, card: frame.card };
      for (const e of messageExtensions) {
        const add = e(frame as Record<string, unknown>, room, actor);
        if (add) ext = { ...ext, ...add };
      }
      const body = frame.card && frame.body === "" ? frame.card.title : frame.body;

      const now = Date.now();
      const messageId = ulid(now);
      const streaming = frame.streaming === true;

      return this.commit((tx) => {
        let topicId = frame.topicId;
        let createdTopic: Topic | undefined;
        if (!topicId) {
          topicId = ulid(now);
          const title = frame.title ?? defaultTitle(body);
          const seq = tx.nextSeq();
          this.sql.exec(
            `INSERT INTO topics (id, title, status, created_by, created_at, root_message_id, last_seq, message_count,
                                 owner_id, last_message_at)
             VALUES (?, ?, 'open', ?, ?, ?, ?, 0, ?, ?)`,
            topicId,
            title,
            actor.id,
            now,
            messageId,
            seq,
            actor.id,
            now,
          );
          const topic = this.toTopic(this.topicRow(topicId) as TopicRow);
          tx.append({ type: "topic.created", actorId: actor.id, topicId, data: { topic } });
        } else {
          this.sql.exec("UPDATE topics SET last_message_at = ? WHERE id = ?", now, topicId);
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
        if (!frame.topicId) createdTopic = this.toTopic(this.topicRow(topicId) as TopicRow);
        const result: SendResult = { message };
        if (createdTopic) result.topic = createdTopic;
        return result;
      });
    });
  }

  async edit(groupId: string, actor: Actor, frame: EditFrame): Promise<Result<{ message: Message }>> {
    return run(async () => {
      await this.ready(groupId);
      this.requireMember(actor.id);
      const row = this.messageRow(frame.messageId);
      if (!row) throw new ApiError("not_found", "message not found");
      if (row.author_id !== actor.id) throw new ApiError("forbidden", "only the author can edit");
      if (row.deleted_at !== null) throw new ApiError("conflict", "message is deleted");
      if (frame.card && row.kind !== "card") throw new ApiError("invalid", "only a card message can get a card");
      const current = this.toMessage(row);
      let ext: Record<string, unknown> = row.ext ? (JSON.parse(row.ext) as Record<string, unknown>) : {};
      const changed: Record<string, unknown> = {};
      if (frame.card) changed.card = frame.card;
      const room = this.roomContext();
      for (const e of editExtensions) {
        const add = e(frame as Record<string, unknown>, room, actor, current);
        if (add) Object.assign(changed, add);
      }
      ext = { ...ext, ...changed };
      const card = (ext.card as Card | undefined) ?? null;
      const body = row.kind === "card" && frame.body === "" && card ? card.title : frame.body;
      const streaming = row.streaming === 1 && frame.streaming === true;
      if (!streaming) this.dropDelta(row.id);
      const now = Date.now();
      return this.commit((tx) => {
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
        return { message: this.toMessage(this.messageRow(row.id) as MessageRow) };
      });
    });
  }

  async remove(
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
      return this.commit((tx) => {
        const seq = tx.nextSeq();
        this.sql.exec(
          "UPDATE messages SET body = '', deleted_at = ?, streaming = 0, ext = json_remove(ext, '$.card') WHERE id = ?",
          now,
          messageId,
        );
        // Scrub the body and card from stored events so the log no longer serves deleted content.
        this.sql.exec(
          `UPDATE events SET data = json_remove(json_set(data, '$.message.body', ''), '$.message.card')
           WHERE message_id = ? AND type = 'message.created'`,
          messageId,
        );
        this.sql.exec(
          "UPDATE events SET data = json_remove(json_set(data, '$.body', ''), '$.card') WHERE message_id = ? AND type = 'message.updated'",
          messageId,
        );
        tx.append({
          type: "message.deleted",
          actorId: actor.id,
          topicId: row.topic_id,
          messageId,
          data: { messageId, deletedAt: now },
        });
        this.touchTopic(row.topic_id, seq);
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

  async topicUpdate(groupId: string, actor: Actor, frame: TopicUpdateFrame): Promise<Result<{ topic: Topic }>> {
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
      const card = row.ext ? (JSON.parse(row.ext) as { card?: Card }).card : undefined;
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
  async delta(groupId: string, actor: Actor, messageId: string, append: string): Promise<Result<{ buffered: number }>> {
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

  private page(afterSeq: number, limit: number): PageResult {
    const rows = this.sql
      .exec<EventRow>("SELECT seq, type, at, actor_id, data FROM events WHERE seq > ? ORDER BY seq LIMIT ?", afterSeq, limit + 1)
      .toArray() as EventRow[];
    const hasMore = rows.length > limit;
    return { events: rows.slice(0, limit).map((r) => this.toEvent(r)), hasMore, lastSeq: this.lastSeq };
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
      return { messages: rows.slice(0, n).map((r) => this.toMessage(r)), hasMore: rows.length > n };
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
      return { groupId, ...this.page(afterSeq, Math.max(1, Math.min(limit, MAX_EXPORT_LIMIT))) };
    });
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
