// Group room side of the bots feature: the one responsible bot per topic, `run.updated` events,
// and the after-commit hook that sends human messages to the responsible bot's BotDesk.
import type { BotRun, Event, Message } from "@chat/protocol";
import { ApiError } from "../core/errors";
import {
  registerAfterCommit,
  registerEventImporter,
  registerRoomMigration,
  registerRoomRpc,
} from "../core/registry";
import { CONTEXT_MESSAGES, type ContextMessage } from "./dispatch";
import { deskStub } from "./env";

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

function claimedBot(sql: SqlStorage, topicId: string): string | null {
  return (
    sql.exec<{ bot_id: string }>("SELECT bot_id FROM bot_topics WHERE topic_id = ?", topicId).toArray()[0]?.bot_id ?? null
  );
}

export interface ClaimResult {
  /** The topic's responsible bot after the claim (this bot, or the one that was first). */
  botId: string;
  title: string;
  messages: ContextMessage[];
}

/** Makes the calling bot the topic's responsible bot unless another bot already is. */
registerRoomRpc("bots.claim", (room, actor, args): ClaimResult => {
  const { topicId } = args as { topicId: string };
  if (!actor || actor.kind !== "bot") throw new ApiError("forbidden", "bots only");
  const topic = room.sql.exec<{ title: string }>("SELECT title FROM topics WHERE id = ?", topicId).toArray()[0];
  if (!topic) throw new ApiError("not_found", "topic not found");
  room.sql.exec(
    "INSERT OR IGNORE INTO bot_topics (topic_id, bot_id, claimed_at) VALUES (?, ?, ?)",
    topicId,
    actor.id,
    Date.now(),
  );
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
  return { botId: claimedBot(room.sql, topicId) as string, title: topic.title, messages };
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
  if (!run?.topicId || !run.botId) return;
  sql.exec(
    "INSERT OR IGNORE INTO bot_topics (topic_id, bot_id, claimed_at) VALUES (?, ?, ?)",
    run.topicId,
    run.botId,
    event.at,
  );
});

/** What BotDesk.onMessage gets. */
export interface TopicMessage {
  groupId: string;
  topicId: string;
  messageId: string;
  authorId: string;
  body: string;
  mentioned: boolean;
}

registerAfterCommit(async (event: Event, h) => {
  if (event.type === "card.actioned") {
    const { messageId, actionId } = event.data as { messageId: string; actionId: string };
    const row = h.sql
      .exec<{ author_id: string; topic_id: string }>("SELECT author_id, topic_id FROM messages WHERE id = ?", messageId)
      .toArray()[0];
    if (!row || claimedBot(h.sql, row.topic_id) !== row.author_id) return;
    await deskStub(h.env, row.author_id).cardAction(row.author_id, { groupId: h.groupId, messageId, actionId, actorId: event.actorId });
    return;
  }
  if (event.type !== "message.created") return;
  const message = (event.data as { message: Message }).message;
  if (message.kind === "system") return;
  const claimed = claimedBot(h.sql, message.topicId);
  if (claimed === message.authorId) return;
  if (!claimed && message.mentions.length === 0) return;
  const ids = [...new Set([message.authorId, ...message.mentions])];
  const rows = await h.env.DB.prepare(
    `SELECT m.id, m.kind, r.member_id IS NOT NULL AS runner FROM members m
     LEFT JOIN bot_runners r ON r.member_id = m.id AND r.enabled = 1
     WHERE m.id IN (${ids.map(() => "?").join(",")}) AND m.deactivated = 0`,
  )
    .bind(...ids)
    .all<{ id: string; kind: string; runner: number }>();
  const byId = new Map(rows.results.map((r) => [r.id, r]));
  // A bot's messages never reach another bot: no loops.
  const author = byId.get(message.authorId);
  if (!author || author.kind !== "human") return;
  const target = claimed ?? message.mentions.find((id) => byId.get(id)?.runner);
  if (!target) return;
  const args: TopicMessage = {
    groupId: h.groupId,
    topicId: message.topicId,
    messageId: message.id,
    authorId: message.authorId,
    body: message.body,
    mentioned: message.mentions.includes(target),
  };
  await deskStub(h.env, target).onMessage(target, args);
});
