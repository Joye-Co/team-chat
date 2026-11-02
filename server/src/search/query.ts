// The room's search RPC: index lookup, then hits built from the stored rows.
import { MAX_SEARCH_LIMIT, MAX_QUERY_CHARS, MAX_SNIPPET_CHARS, type SearchHit } from "@chat/protocol";
import { makeSnippet } from "@chat/search";
import { z } from "zod";
import { ApiError } from "../core/errors";
import type { RoomContext } from "../core/registry";
import { TITLE_PREFIX, ensureBackfilled, searchIndex } from "./indexer";

export const DEFAULT_LIMIT = 20;

export const RoomSearchArgs = z.object({
  query: z.string().min(1).max(MAX_QUERY_CHARS),
  topicId: z.string().min(1).max(128).optional(),
  limit: z.int().min(1).max(MAX_SEARCH_LIMIT).optional(),
});
export type RoomSearchArgs = z.infer<typeof RoomSearchArgs>;

/** UTF-16 ranges from makeSnippet as code-point ranges (the protocol unit). */
function toSnippet(text: string, query: string): Pick<SearchHit, "snippet" | "highlights"> {
  const s = makeSnippet(text, query, { maxLength: 120, before: 30 });
  const cp = (i: number) => [...s.text.slice(0, i)].length;
  const highlights = s.highlights
    .map(([a, b]) => ({ start: cp(a), length: cp(b) - cp(a) }))
    .filter((h) => h.length > 0);
  const snippet = [...s.text].slice(0, MAX_SNIPPET_CHARS).join("");
  return highlights.length > 0 ? { snippet, highlights } : { snippet };
}

type MsgRow = {
  id: string;
  topic_id: string;
  author_id: string;
  body: string;
  created_seq: number;
  created_at: number;
  deleted_at: number | null;
};
type TopicRow = { id: string; title: string; created_by: string; created_at: number; root_message_id: string };

/** Newest first. Deleted messages are never returned; a title match points at the root message. */
export async function searchRoom(room: RoomContext, rawArgs: unknown): Promise<SearchHit[]> {
  const parsed = RoomSearchArgs.safeParse(rawArgs);
  if (!parsed.success) throw new ApiError("invalid", "invalid search arguments");
  const args = parsed.data;
  const limit = args.limit ?? DEFAULT_LIMIT;
  await ensureBackfilled(room);

  const sql = room.sql;
  // A title and its root message can both match: fetch extra rows for the dedupe.
  const rows = searchIndex(sql).search({
    query: args.query,
    topicId: args.topicId,
    order: "recent",
    limit: Math.min(200, limit * 2),
    prefixLast: true,
  });
  const hits: SearchHit[] = [];
  const seen = new Set<string>();
  for (const r of rows) {
    if (hits.length >= limit) break;
    if (r.messageId.startsWith(TITLE_PREFIX)) {
      const t = sql
        .exec<TopicRow>(
          "SELECT id, title, created_by, created_at, root_message_id FROM topics WHERE id = ?",
          r.messageId.slice(TITLE_PREFIX.length),
        )
        .toArray()[0];
      if (!t || seen.has(t.root_message_id)) continue;
      const root = sql
        .exec<{ created_seq: number }>("SELECT created_seq FROM messages WHERE id = ?", t.root_message_id)
        .toArray()[0];
      if (!root) continue;
      seen.add(t.root_message_id);
      hits.push({
        groupId: room.groupId,
        topicId: t.id,
        messageId: t.root_message_id,
        authorId: t.created_by,
        ...toSnippet(t.title, args.query),
        createdAt: t.created_at,
        seq: root.created_seq,
      });
      continue;
    }
    if (seen.has(r.messageId)) continue;
    const m = sql
      .exec<MsgRow>(
        "SELECT id, topic_id, author_id, body, created_seq, created_at, deleted_at FROM messages WHERE id = ?",
        r.messageId,
      )
      .toArray()[0];
    if (!m || m.deleted_at !== null) continue;
    seen.add(m.id);
    hits.push({
      groupId: room.groupId,
      topicId: m.topic_id,
      messageId: m.id,
      authorId: m.author_id,
      ...toSnippet(m.body, args.query),
      createdAt: m.created_at,
      seq: m.created_seq,
    });
  }
  return hits;
}
