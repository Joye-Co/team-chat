// Rebuilds the core room tables (topics, messages, mentions) from imported archive events.
// Membership is not replayed: on restore the room takes its members from D1, which is restored
// first. Feature modules register their own importers for their tables and event types.
import { type ArchivedEvent, registerEventImporter } from "../core/registry";

const CORE_MESSAGE_KEYS = new Set([
  "id",
  "clientMsgId",
  "groupId",
  "topicId",
  "authorId",
  "body",
  "mentions",
  "createdSeq",
  "createdAt",
  "editedAt",
  "deletedAt",
  "streaming",
]);
const CORE_UPDATE_KEYS = new Set(["messageId", "body", "editedAt", "streaming"]);

type Obj = Record<string, unknown>;

function extOf(obj: Obj, core: Set<string>): Obj | null {
  const ext: Obj = {};
  let n = 0;
  for (const [k, v] of Object.entries(obj)) {
    if (core.has(k) || v === undefined) continue;
    ext[k] = v;
    n++;
  }
  return n > 0 ? ext : null;
}

function touch(sql: SqlStorage, topicId: string | null | undefined, seq: number, addMessage = false): void {
  if (!topicId) return;
  sql.exec(
    "UPDATE topics SET last_seq = ?, message_count = message_count + ? WHERE id = ?",
    seq,
    addMessage ? 1 : 0,
    topicId,
  );
}

function messageTopic(sql: SqlStorage, messageId: string): string | null {
  return sql.exec<{ topic_id: string }>("SELECT topic_id FROM messages WHERE id = ?", messageId).toArray()[0]?.topic_id ?? null;
}

export function replayCoreEvent(e: ArchivedEvent, sql: SqlStorage): void {
  const data = (e.data ?? {}) as Obj;
  switch (e.type) {
    case "topic.created": {
      const t = data.topic as Obj;
      sql.exec(
        `INSERT INTO topics (id, title, status, created_by, created_at, root_message_id, last_seq, message_count)
         VALUES (?, ?, ?, ?, ?, ?, ?, 0)`,
        t.id as string,
        t.title as string,
        (t.status as string) ?? "open",
        t.createdBy as string,
        t.createdAt as number,
        t.rootMessageId as string,
        e.seq,
      );
      return;
    }
    case "topic.updated": {
      const sets: string[] = ["last_seq = ?"];
      const args: unknown[] = [e.seq];
      if (typeof data.title === "string") {
        sets.push("title = ?");
        args.push(data.title);
      }
      if (typeof data.status === "string") {
        sets.push("status = ?");
        args.push(data.status);
      }
      sql.exec(`UPDATE topics SET ${sets.join(", ")} WHERE id = ?`, ...args, data.topicId as string);
      return;
    }
    case "message.created": {
      const m = data.message as Obj;
      const mentions = Array.isArray(m.mentions) ? (m.mentions as string[]) : [];
      const ext = extOf(m, CORE_MESSAGE_KEYS);
      sql.exec(
        `INSERT INTO messages (id, client_msg_id, topic_id, author_id, body, mentions, created_seq, created_at,
                               edited_at, deleted_at, streaming, ext)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
        m.id as string,
        m.clientMsgId as string,
        m.topicId as string,
        m.authorId as string,
        (m.body as string) ?? "",
        JSON.stringify(mentions),
        e.seq,
        m.createdAt as number,
        (m.editedAt as number | null) ?? null,
        (m.deletedAt as number | null) ?? null,
        m.streaming === true ? 1 : 0,
        ext ? JSON.stringify(ext) : null,
      );
      for (const id of new Set(mentions)) {
        sql.exec("INSERT OR IGNORE INTO mentions (member_id, seq, message_id) VALUES (?, ?, ?)", id, e.seq, m.id as string);
      }
      touch(sql, m.topicId as string, e.seq, true);
      return;
    }
    case "message.updated": {
      const id = data.messageId as string;
      const row = sql.exec<{ ext: string | null }>("SELECT ext FROM messages WHERE id = ?", id).toArray()[0];
      if (!row) return;
      const extra = extOf(data, CORE_UPDATE_KEYS);
      const ext = extra ? { ...(row.ext ? (JSON.parse(row.ext) as Obj) : {}), ...extra } : null;
      sql.exec(
        "UPDATE messages SET body = ?, edited_at = ?, streaming = ?, ext = COALESCE(?, ext) WHERE id = ?",
        (data.body as string) ?? "",
        (data.editedAt as number) ?? e.at,
        data.streaming === true ? 1 : 0,
        ext ? JSON.stringify(ext) : null,
        id,
      );
      touch(sql, e.topicId ?? messageTopic(sql, id), e.seq);
      return;
    }
    case "message.deleted": {
      const id = data.messageId as string;
      sql.exec(
        "UPDATE messages SET body = '', deleted_at = ?, streaming = 0 WHERE id = ?",
        (data.deletedAt as number) ?? e.at,
        id,
      );
      touch(sql, e.topicId ?? messageTopic(sql, id), e.seq);
      return;
    }
    default:
      // group.* events: membership comes from D1. Feature events: their own importers.
      return;
  }
}

registerEventImporter(replayCoreEvent);
