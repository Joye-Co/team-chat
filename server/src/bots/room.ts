// Group room side of the bots feature: the one responsible bot per topic, `run.updated` events,
// and the hand-off of human messages and card presses to the responsible bot's BotDesk. The
// hand-off is written to an outbox in the same transaction as the event and delivered after
// commit; a failed delivery is retried by the room alarm until the desk accepts it.
import { type BlockRef, type BotRun, type CardV2, type Event, isMainTopicId, type Message, type TopicContext } from "@chat/protocol";
import { ApiError } from "../core/errors";
import {
  type ArchivedEvent,
  registerAfterCommit,
  registerEventImporter,
  registerEventWriter,
  registerRoomAlarm,
  registerRoomMigration,
  registerRoomRpc,
} from "../core/registry";
import { storedCard } from "../cards2/decisions";
import { ANONYMOUS_ID, storedPrivacy, viewMessage } from "../cards2/privacy";
import type { AnswerData } from "./answers";
import { CONTEXT_MESSAGES, type ContextMessage } from "./dispatch";
import type { Env } from "../core/env";
import type { Logger } from "../core/logger";
import { deskStub } from "./env";
import { type RefSource, resolveRefs, stripRefPrefix } from "./refs";
import { type ContextRow, parseExt } from "../context/render";

registerRoomMigration({
  name: "bots-1",
  up: `
    CREATE TABLE bot_topics (
      topic_id TEXT PRIMARY KEY,
      bot_id TEXT NOT NULL,
      claimed_at INTEGER NOT NULL
    );
  `,
});

registerRoomMigration({
  name: "bots-2",
  up: `
    CREATE TABLE bot_outbox (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      kind TEXT NOT NULL,
      topic_id TEXT NOT NULL,
      payload TEXT NOT NULL,
      attempts INTEGER NOT NULL DEFAULT 0,
      next_at INTEGER NOT NULL DEFAULT 0,
      created_at INTEGER NOT NULL
    );
  `,
});

function claimedBot(sql: SqlStorage, topicId: string): string | null {
  return (
    sql.exec<{ bot_id: string }>("SELECT bot_id FROM bot_topics WHERE topic_id = ?", topicId).toArray()[0]?.bot_id ?? null
  );
}

/**
 * The bot a card press in this topic goes to: the responsible bot, or in the 群聊 (which has
 * none) the card's author when that bot has run there.
 */
function cardOwner(sql: SqlStorage, topicId: string, authorId: string): string | null {
  if (!isMainTopicId(topicId)) return claimedBot(sql, topicId);
  const ran = sql
    .exec("SELECT 1 FROM events WHERE type = 'run.updated' AND topic_id = ? AND actor_id = ? LIMIT 1", topicId, authorId)
    .toArray();
  return ran.length > 0 ? authorId : null;
}

export interface ClaimResult {
  /** The topic's responsible bot after the claim (this bot, or the one that was first). */
  botId: string;
  title: string;
  messages: ContextMessage[];
  /** The topic's first message (Run context). */
  first?: ContextRow;
  /** The newest messages, newest first (at most `CONTEXT_SCAN`). */
  newest?: ContextRow[];
  /** Messages of the topic (not deleted). */
  total?: number;
  /** The topic is a side chat about this topic (1.1, `Topic.contextOf`). */
  contextOf?: TopicContext;
  /** With `contextOf`: the side chat's own person (the DM's other member), the asker. */
  askerId?: string;
}

/** What `bots.context` returns: the side chat's source topic up to `uptoSeq`. */
export interface SourceContext {
  title: string;
  messages: ContextMessage[];
}

/** Newest messages read for a run's prompt; the character budget picks from these. */
export const CONTEXT_SCAN = 400;

type ContextDbRow = { id: string; created_seq: number; author_id: string; created_at: number; body: string; kind: string | null; ext: string | null };
const CONTEXT_COLS = "id, created_seq, author_id, created_at, body, kind, ext";

function toContextRow(r: ContextDbRow): ContextRow {
  return { id: r.id, seq: r.created_seq, authorId: r.author_id, at: r.created_at, body: r.body, kind: r.kind ?? "text", ext: parseExt(r.ext) };
}

/**
 * Messages of a topic as context rows, as `viewerId` may see them (private cards hide their
 * voters): `where` adds conditions on top of topic and not deleted.
 */
export function contextRows(sql: SqlStorage, viewerId: string, topicId: string, where: string, params: unknown[], order: "ASC" | "DESC", limit: number): ContextRow[] {
  return sql
    .exec<ContextDbRow>(
      `SELECT ${CONTEXT_COLS} FROM messages WHERE topic_id = ? AND deleted_at IS NULL ${where} ORDER BY created_seq ${order} LIMIT ?`,
      topicId,
      ...params,
      limit,
    )
    .toArray()
    .map((r) => {
      const row = toContextRow(r);
      if (row.ext.cardState) {
        const seen = viewMessage({ id: row.id, cardState: row.ext.cardState } as unknown as Message, viewerId, sql);
        row.ext = { ...row.ext, cardState: seen.cardState as ContextRow["ext"]["cardState"] };
      }
      return row;
    });
}

function topicCount(sql: SqlStorage, topicId: string, where = "", params: unknown[] = []): number {
  return (
    sql.exec<{ n: number }>(`SELECT COUNT(*) AS n FROM messages WHERE topic_id = ? AND deleted_at IS NULL ${where}`, topicId, ...params).toArray()[0]?.n ?? 0
  );
}

/** Messages of a side chat's source topic given to the aside run. */
export const SIDE_CHAT_CONTEXT_MESSAGES = 60;

/**
 * Makes the calling bot the topic's responsible bot unless another bot already is. The 群聊
 * has no responsible bot: every bot it mentions may work there (docs/protocol.md, "群聊").
 */
registerRoomRpc("bots.claim", (room, actor, args): ClaimResult => {
  // peek: read the topic without becoming its responsible bot (the front desk reads first).
  const { topicId, peek } = args as { topicId: string; peek?: boolean };
  if (!actor || actor.kind !== "bot") throw new ApiError("forbidden", "bots only");
  const topic = room.sql.exec<{ title: string; context: string | null }>("SELECT title, context FROM topics WHERE id = ?", topicId).toArray()[0];
  if (!topic) throw new ApiError("not_found", "topic not found");
  const main = isMainTopicId(topicId);
  if (!main && !peek) {
    room.sql.exec(
      "INSERT OR IGNORE INTO bot_topics (topic_id, bot_id, claimed_at) VALUES (?, ?, ?)",
      topicId,
      actor.id,
      Date.now(),
    );
  }
  const messages = room.sql
    .exec<{ id: string; author_id: string; body: string }>(
      `SELECT id, author_id, body FROM messages WHERE topic_id = ? AND deleted_at IS NULL
       ORDER BY created_seq DESC LIMIT ?`,
      topicId,
      CONTEXT_MESSAGES,
    )
    .toArray()
    .reverse()
    .map((m) => ({ id: m.id, authorId: m.author_id, body: m.body }));
  const out: ClaimResult = { botId: main ? actor.id : (claimedBot(room.sql, topicId) ?? actor.id), title: topic.title, messages };
  out.newest = contextRows(room.sql, actor.id, topicId, "", [], "DESC", CONTEXT_SCAN);
  out.first = contextRows(room.sql, actor.id, topicId, "", [], "ASC", 1)[0];
  out.total = topicCount(room.sql, topicId);
  // Only a DM topic can be a side chat (the room checks this when it stores the context).
  if (topic.context && room.groupKind === "dm") {
    out.contextOf = JSON.parse(topic.context) as TopicContext;
    const asker = room.memberIds().find((id) => id !== actor.id);
    if (asker) out.askerId = asker;
  }
  return out;
});

/**
 * The source topic of a side chat, read by its aside run: messages up to `uptoSeq`, oldest
 * first. The calling bot and the asker must both still be members of this group.
 */
registerRoomRpc("bots.context", (room, actor, args): SourceContext => {
  const { topicId, uptoSeq, askerId } = args as { topicId: string; uptoSeq: number; askerId: string };
  if (!actor || actor.kind !== "bot") throw new ApiError("forbidden", "bots only");
  if (!room.isMember(actor.id) || !room.isMember(askerId)) throw new ApiError("forbidden", "not a member of this group");
  const topic = room.sql.exec<{ title: string }>("SELECT title FROM topics WHERE id = ?", topicId).toArray()[0];
  if (!topic) throw new ApiError("not_found", "topic not found");
  const messages = room.sql
    .exec<{ id: string; author_id: string; body: string }>(
      `SELECT id, author_id, body FROM messages WHERE topic_id = ? AND deleted_at IS NULL AND created_seq <= ?
       ORDER BY created_seq DESC LIMIT ?`,
      topicId,
      uptoSeq,
      SIDE_CHAT_CONTEXT_MESSAGES,
    )
    .toArray()
    .reverse()
    .map((m) => ({ id: m.id, authorId: m.author_id, body: m.body }));
  return { title: topic.title, messages };
});

/**
 * Messages of a topic after `afterSeq` that are not `exceptAuthor`'s, newest first (a resumed
 * run's new input), with how many there are.
 */
registerRoomRpc("bots.since", (room, actor, args) => {
  const { topicId, afterSeq, exceptAuthor } = args as { topicId: string; afterSeq: number; exceptAuthor: string };
  if (!actor || actor.kind !== "bot") throw new ApiError("forbidden", "bots only");
  const where = "AND created_seq > ? AND author_id != ?";
  return {
    newest: contextRows(room.sql, actor.id, topicId, where, [afterSeq, exceptAuthor], "DESC", CONTEXT_SCAN),
    total: topicCount(room.sql, topicId, where, [afterSeq, exceptAuthor]),
  };
});

/** A page of a topic for chat-run `topic.history`, oldest first (machine-protocol.md, "Topic context"). */
registerRoomRpc("bots.history", (room, actor, args) => {
  const { topicId, before, after, limit } = args as { topicId: string; before?: number; after?: number; limit: number };
  if (!actor || actor.kind !== "bot") throw new ApiError("forbidden", "bots only");
  const conds: string[] = [];
  const params: unknown[] = [];
  if (before !== undefined) {
    conds.push("AND created_seq < ?");
    params.push(before);
  }
  if (after !== undefined) {
    conds.push("AND created_seq > ?");
    params.push(after);
  }
  // Reading forward (after) goes oldest first; otherwise the newest page before the point.
  const forward = after !== undefined && before === undefined;
  const rows = contextRows(room.sql, actor.id, topicId, conds.join(" "), params, forward ? "ASC" : "DESC", limit + 1);
  const hasMore = rows.length > limit;
  const page = rows.slice(0, limit);
  return { rows: forward ? page : page.reverse(), hasMore };
});

/** One message of a topic, whole (chat-run `topic.message`). */
registerRoomRpc("bots.message", (room, actor, args) => {
  const { topicId, messageId } = args as { topicId: string; messageId: string };
  if (!actor || actor.kind !== "bot") throw new ApiError("forbidden", "bots only");
  const row = contextRows(room.sql, actor.id, topicId, "AND id = ?", [messageId], "ASC", 1)[0];
  if (!row) throw new ApiError("not_found", "message not found");
  return { row };
});

/** The author and body of a message that started a run (retrying a run made before 1.1). */
registerRoomRpc("bots.request", (room, actor, args) => {
  const { messageId } = args as { messageId: string };
  if (!actor || actor.kind !== "bot") throw new ApiError("forbidden", "bots only");
  const m = room.sql
    .exec<{ author_id: string; body: string }>("SELECT author_id, body FROM messages WHERE id = ? AND deleted_at IS NULL", messageId)
    .toArray()[0];
  return m ? { authorId: m.author_id, body: m.body } : null;
});

/** Checks that a topic exists in this group (external bots start runs in it). */
registerRoomRpc("bots.topicExists", (room, actor, args) => {
  const { topicId } = args as { topicId: string };
  if (!actor || actor.kind !== "bot") throw new ApiError("forbidden", "bots only");
  const topic = room.sql.exec<{ id: string }>("SELECT id FROM topics WHERE id = ?", topicId).toArray()[0];
  if (!topic) throw new ApiError("not_found", "topic not found");
  return { ok: true };
});

/** Stores a `run.updated` event (actor = the bot) for clients. */
registerRoomRpc("bots.runUpdated", (room, actor, args) => {
  const { run } = args as { run: BotRun };
  if (!actor || actor.id !== run.botId) throw new ApiError("forbidden", "only the run's bot");
  const event = room.commit((tx) =>
    tx.append({ type: "run.updated", actorId: actor.id, topicId: run.topicId, data: { run } }),
  );
  return { seq: event.seq };
});

// Restore from the archive: the responsible bot of a topic is the bot of its first run.
registerEventImporter((event, sql) => {
  if (event.type !== "run.updated") return;
  const run = (event.data as { run?: BotRun }).run;
  if (!run?.topicId || !run.botId || isMainTopicId(run.topicId)) return;
  sql.exec(
    "INSERT OR IGNORE INTO bot_topics (topic_id, bot_id, claimed_at) VALUES (?, ?, ?)",
    run.topicId,
    run.botId,
    event.at,
  );
});

/** 转交: the calling bot gives its topic to another bot (delegate.ts). The 群聊 has no claim. */
registerRoomRpc("bots.transfer", (room, actor, args) => {
  const { topicId, to } = args as { topicId: string; to: string };
  if (!actor || actor.kind !== "bot") throw new ApiError("forbidden", "bots only");
  if (!room.isMember(to)) throw new ApiError("forbidden", "not a member of this group");
  if (isMainTopicId(topicId)) return { transferred: false };
  const owner = claimedBot(room.sql, topicId);
  if (owner && owner !== actor.id) throw new ApiError("forbidden", "only the responsible member hands a topic over");
  room.sql.exec(
    "INSERT INTO bot_topics (topic_id, bot_id, claimed_at) VALUES (?, ?, ?) ON CONFLICT (topic_id) DO UPDATE SET bot_id = excluded.bot_id, claimed_at = excluded.claimed_at",
    topicId,
    to,
    Date.now(),
  );
  return { transferred: true };
});

/** Removes a bot's topic claims in this group (the bot was deactivated). Internal. */
registerRoomRpc("bots.release", (room, actor, args) => {
  const { botId } = args as { botId: string };
  if (actor) throw new ApiError("forbidden", "internal only");
  const r = room.sql.exec("DELETE FROM bot_topics WHERE bot_id = ?", botId);
  return { released: r.rowsWritten };
});

/** What BotDesk.onMessage gets. */
export interface TopicMessage {
  groupId: string;
  topicId: string;
  messageId: string;
  authorId: string;
  /** The author's level, for approvals by text (only the requester or an owner). */
  authorLevel?: "owner" | "member";
  /** The body; with refs, the question without the server's quote lines. */
  body: string;
  mentioned: boolean;
  /** The message the run's acknowledgement reacts to when `messageId` is a synthetic key. */
  ackMessageId?: string;
  /** 1.1 (引用提问): the message's refs, resolved by the room. */
  refs?: RefSource[];
  /** Author of the message this one quotes (`replyTo`), if any. */
  replyToAuthorId?: string;
}

/** Who pressed a card button, as BotDesk.cardAction gets it. */
export interface CardPress {
  groupId: string;
  messageId: string;
  actionId: string;
  actorId: string;
  actorKind: "human" | "bot" | null;
  actorLevel: "owner" | "member" | null;
}

/** A Card v2 answer for the card's author, as BotDesk.cardAnswer gets it. */
export interface CardAnswerIn {
  groupId: string;
  topicId: string;
  /** Seq of the `card.answered` event: the answer's identity. */
  seq: number;
  answer: AnswerData;
  actorId: string;
  actorKind: "human" | "bot" | null;
  actorLevel: "owner" | "member" | null;
  actorName: string;
  /** The card at the answered rev. */
  card: CardV2;
  /** A counts-only poll: the votes per option now, in place of the voter's choice. */
  tally?: Record<string, number>;
}

export type MemberInfo = { id: string; name: string; kind: string; level: string; deactivated: number; runner: number };

export async function memberInfo(db: D1Database, ids: string[]): Promise<Map<string, MemberInfo>> {
  const uniq = [...new Set(ids)];
  const rows = await db
    .prepare(
      `SELECT m.id, m.name, m.kind, m.level, m.deactivated, r.member_id IS NOT NULL AS runner FROM members m
       LEFT JOIN bot_runners r ON r.member_id = m.id AND r.enabled = 1
       WHERE m.id IN (${uniq.map(() => "?").join(",")})`,
    )
    .bind(...uniq)
    .all<MemberInfo>();
  return new Map(rows.results.map((r) => [r.id, r]));
}

/** In a DM, the member other than `authorId` (a DM message is addressed to it, @ or not). */
function dmPeer(sql: SqlStorage, authorId: string): string | undefined {
  const kind = sql.exec<{ value: string }>("SELECT value FROM meta WHERE key = 'group_kind'").toArray()[0]?.value;
  if (kind !== "dm") return undefined;
  return sql
    .exec<{ member_id: string }>("SELECT member_id FROM room_members WHERE member_id != ? LIMIT 1", authorId)
    .toArray()[0]?.member_id;
}

/** Events older than this are imports (restore from the archive), not live writes: no hand-off. */
const LIVE_WINDOW_MS = 10 * 60_000;
/** A hand-off the desk has not accepted after this long is dropped (and logged). */
export const HANDOFF_GIVE_UP_MS = 60 * 60_000;
const RETRY_MAX_MS = 5 * 60_000;

type OutboxCard = { botId: string; messageId: string; actionId: string; actorId: string };
type OutboxMessage = Omit<TopicMessage, "mentioned" | "groupId" | "refs"> & { mentions: string[]; blockRefs?: BlockRef[] };
/**
 * The card as it was answered: the author may edit it before the hand-off is delivered.
 * `anonymous`: the card is private (cards2/privacy), so the run gets no actor; such a row holds
 * `ANONYMOUS_ID` as `actorId` (rows written before held the voter) and delivery finds the voter's
 * kind through the card event (`answer.eventId`). `poll`: a poll that shows counts only; the row
 * holds no values and the run gets the tally.
 */
type OutboxAnswer = { botId: string; seq: number; answer: AnswerData; actorId: string; card: CardV2; anonymous?: boolean; poll?: boolean };

/** An answer of a counts-only poll without what the voter chose. */
function withoutValues(a: AnswerData): AnswerData {
  const { value: _v, inputs: _i, editedValues: _e, ...rest } = a;
  return rest;
}

type OutboxRow = {
  id: number;
  /** "artifact" rows (page events before 1.1 made them transient) are dropped. */
  kind: "message" | "card" | "answer" | "artifact";
  topic_id: string;
  payload: string;
  attempts: number;
  next_at: number;
  created_at: number;
};

// In the commit transaction: the messages and card presses that may go to a bot.
registerEventWriter((event: ArchivedEvent, sql) => {
  if (event.at < Date.now() - LIVE_WINDOW_MS) return;
  if (event.type === "card.actioned") {
    const { messageId, actionId } = event.data as { messageId: string; actionId: string };
    const row = sql
      .exec<{ author_id: string; topic_id: string }>("SELECT author_id, topic_id FROM messages WHERE id = ?", messageId)
      .toArray()[0];
    if (!row || cardOwner(sql, row.topic_id, row.author_id) !== row.author_id) return;
    const payload: OutboxCard = { botId: row.author_id, messageId, actionId, actorId: event.actorId };
    enqueue(sql, "card", row.topic_id, payload);
    return;
  }
  if (event.type === "card.answered") {
    // To the card's author; deliver() keeps only runner bots (SDK bots read the event itself).
    const answer = event.data as AnswerData;
    const stored = storedCard(sql, answer.messageId);
    if (!stored || stored.authorId === event.actorId) return;
    const payload: OutboxAnswer = { botId: stored.authorId, seq: event.seq, answer, actorId: event.actorId, card: stored.card };
    // A private card's voter never leaves the room, not even in the outbox (cards2/privacy).
    const privacy = storedPrivacy(sql, answer.messageId);
    if (privacy) {
      payload.anonymous = true;
      payload.actorId = ANONYMOUS_ID;
    }
    if (privacy === "poll") {
      payload.poll = true;
      payload.answer = withoutValues(answer);
    }
    enqueue(sql, "answer", stored.topicId, payload);
    return;
  }
  if (event.type !== "message.created") return;
  const message = (event.data as { message: Message }).message;
  if (message.kind === "system") return;
  const claimed = claimedBot(sql, message.topicId);
  if (claimed === message.authorId) return;
  if (!claimed && message.mentions.length === 0 && !dmPeer(sql, message.authorId)) return;
  const payload: OutboxMessage = {
    topicId: message.topicId,
    messageId: message.id,
    authorId: message.authorId,
    body: message.body,
    mentions: message.mentions,
  };
  if (message.blockRefs && message.blockRefs.length > 0) payload.blockRefs = message.blockRefs;
  enqueue(sql, "message", message.topicId, payload);
});

function enqueue(sql: SqlStorage, kind: OutboxRow["kind"], topicId: string, payload: unknown): void {
  sql.exec(
    "INSERT INTO bot_outbox (kind, topic_id, payload, created_at) VALUES (?, ?, ?, ?)",
    kind,
    topicId,
    JSON.stringify(payload),
    Date.now(),
  );
}

interface DeliverContext {
  env: Env;
  groupId: string;
  sql: SqlStorage;
  log: Logger;
  wakeAt(at: number): void;
}

/** Hands one outbox row to the bot's desk. Throws when it must be tried again. */
async function deliver(c: DeliverContext, row: OutboxRow): Promise<void> {
  if (row.kind === "card") {
    const p = JSON.parse(row.payload) as OutboxCard;
    // Who pressed: BotDesk takes an approval only from a person who may answer it.
    const who = (await memberInfo(c.env.DB, [p.actorId])).get(p.actorId);
    const press: CardPress = {
      groupId: c.groupId,
      messageId: p.messageId,
      actionId: p.actionId,
      actorId: p.actorId,
      actorKind: who && !who.deactivated ? (who.kind as "human" | "bot") : null,
      actorLevel: who && !who.deactivated ? (who.level as "owner" | "member") : null,
    };
    await deskStub(c.env, p.botId).cardAction(p.botId, press);
    return;
  }
  if (row.kind === "artifact") return;
  if (row.kind === "answer") return deliverAnswer(c, row);
  const m = JSON.parse(row.payload) as OutboxMessage;
  // The responsible bot now (a run may have claimed the topic since the message was written).
  const claimed = claimedBot(c.sql, m.topicId);
  if (claimed === m.authorId) return;
  const peer = dmPeer(c.sql, m.authorId);
  if (!claimed && m.mentions.length === 0 && !peer) return;
  const byId = await memberInfo(c.env.DB, [m.authorId, ...m.mentions, ...(claimed ? [claimed] : []), ...(peer ? [peer] : [])]);
  const usable = (id: string | undefined) => {
    const x = id ? byId.get(id) : undefined;
    return Boolean(x && x.kind === "bot" && x.runner && !x.deactivated);
  };
  // A bot's messages never reach another bot: no loops.
  const author = byId.get(m.authorId);
  if (!author || author.kind !== "human" || author.deactivated) return;
  let target: string | undefined;
  if (claimed) {
    // A deactivated (or runner-disabled) bot gets no more work in topics it claimed.
    if (!usable(claimed)) return;
    target = claimed;
  } else {
    target = m.mentions.find((id) => usable(id)) ?? (usable(peer) ? peer : undefined);
  }
  if (!target) return;
  const args: TopicMessage = {
    groupId: c.groupId,
    topicId: m.topicId,
    messageId: m.messageId,
    authorId: m.authorId,
    authorLevel: author.level as "owner" | "member",
    body: m.body,
    mentioned: m.mentions.includes(target) || target === peer,
  };
  if (m.blockRefs && m.blockRefs.length > 0) {
    args.body = stripRefPrefix(m.body, m.blockRefs);
    args.refs = resolveRefs(c.sql, m.blockRefs);
  }
  const ext = c.sql.exec<{ ext: string | null }>("SELECT ext FROM messages WHERE id = ?", m.messageId).toArray()[0]?.ext ?? null;
  const quoted = parseExt(ext).replyTo?.authorId;
  if (quoted) args.replyToAuthorId = quoted;
  await deskStub(c.env, target).onMessage(target, args);
}

/** The voter of a stored answer, from the room's card event (private cards keep it only there). */
function answerActor(sql: SqlStorage, a: OutboxAnswer): string {
  if (a.actorId !== ANONYMOUS_ID || !a.answer.eventId) return a.actorId;
  return sql.exec<{ actor_id: string }>("SELECT actor_id FROM card_events WHERE event_id = ?", a.answer.eventId).toArray()[0]?.actor_id ?? a.actorId;
}

/** The tally of a poll card now (cards2 state), for a counts-only poll's author. */
function currentTally(sql: SqlStorage, messageId: string): Record<string, number> | undefined {
  const row = sql.exec<{ ext: string | null }>("SELECT ext FROM messages WHERE id = ?", messageId).toArray()[0];
  if (!row?.ext) return undefined;
  return (JSON.parse(row.ext) as { cardState?: { tally?: Record<string, number> } }).cardState?.tally;
}

/** A card answer, to the runner bot that made the card. */
async function deliverAnswer(c: DeliverContext, row: OutboxRow): Promise<void> {
  const a = JSON.parse(row.payload) as OutboxAnswer;
  const voter = answerActor(c.sql, a);
  const byId = await memberInfo(c.env.DB, [a.botId, voter]);
  const bot = byId.get(a.botId);
  // SDK bots and people read the stored event; only runner bots need a hand-off.
  if (!bot || bot.kind !== "bot" || !bot.runner || bot.deactivated) return;
  const who = byId.get(voter);
  const args: CardAnswerIn = {
    groupId: c.groupId,
    topicId: row.topic_id,
    seq: a.seq,
    // Rows written before the poll rule may still hold the values.
    answer: a.poll ? withoutValues(a.answer) : a.answer,
    actorId: a.anonymous ? ANONYMOUS_ID : a.actorId,
    actorKind: who && !who.deactivated ? (who.kind as "human" | "bot") : null,
    actorLevel: who && !who.deactivated && !a.anonymous ? (who.level as "owner" | "member") : null,
    actorName: a.anonymous ? ANONYMOUS_ID : (who?.name ?? a.actorId),
    card: a.card,
  };
  if (a.poll) {
    const tally = currentTally(c.sql, a.answer.messageId);
    if (tally) args.tally = tally;
  }
  await deskStub(c.env, a.botId).cardAnswer(a.botId, args);
}

/** Rooms with a drain running (one per room object; the alarm and hooks share it). */
const draining = new WeakSet<SqlStorage>();

/**
 * Delivers every due outbox row in order. A failure delays the later rows of the same topic
 * (their order matters to the run) and schedules a retry with backoff through the alarm.
 */
export async function drainOutbox(c: DeliverContext): Promise<void> {
  if (draining.has(c.sql)) return;
  draining.add(c.sql);
  try {
    // Rows written while a delivery was in flight are picked up by the next pass.
    for (let pass = 0; pass < 10; pass++) {
      const now = Date.now();
      const due = c.sql
        .exec<OutboxRow>("SELECT * FROM bot_outbox WHERE next_at <= ? ORDER BY id LIMIT 100", now)
        .toArray();
      if (due.length === 0) break;
      const blocked = new Set(
        c.sql
          .exec<{ topic_id: string }>("SELECT DISTINCT topic_id FROM bot_outbox WHERE next_at > ?", now)
          .toArray()
          .map((r) => r.topic_id),
      );
      let delivered = 0;
      for (const row of due) {
        if (blocked.has(row.topic_id)) continue;
        try {
          await deliver(c, row);
          c.sql.exec("DELETE FROM bot_outbox WHERE id = ?", row.id);
          delivered++;
        } catch (err) {
          blocked.add(row.topic_id);
          const attempts = row.attempts + 1;
          if (Date.now() - row.created_at > HANDOFF_GIVE_UP_MS) {
            c.sql.exec("DELETE FROM bot_outbox WHERE id = ?", row.id);
            c.log.error("bot hand-off dropped", { err, kind: row.kind, topicId: row.topic_id, attempts });
            continue;
          }
          const wait = Math.min(1000 * 2 ** Math.min(attempts, 20), RETRY_MAX_MS);
          c.sql.exec("UPDATE bot_outbox SET attempts = ?, next_at = ? WHERE id = ?", attempts, Date.now() + wait, row.id);
          c.log.warn("bot hand-off failed; will retry", { err, kind: row.kind, topicId: row.topic_id, attempts });
        }
      }
      if (delivered === 0) break;
    }
  } finally {
    draining.delete(c.sql);
  }
  const next = c.sql.exec<{ at: number | null }>("SELECT MIN(next_at) AS at FROM bot_outbox").toArray()[0]?.at;
  if (next !== null && next !== undefined) c.wakeAt(Math.max(next, Date.now() + 1000));
}

const HANDED_OFF = new Set(["message.created", "card.actioned", "card.answered"]);

registerAfterCommit(async (event: Event, h) => {
  if (!HANDED_OFF.has(event.type)) return;
  await drainOutbox(h);
});

registerRoomAlarm((room) => drainOutbox(room));
