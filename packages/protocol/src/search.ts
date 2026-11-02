// V1 search over messages. See docs/protocol.md, "Search".
import { z } from "zod";
import { Id, Millis, Rid, Seq } from "./common.ts";

export const MAX_SEARCH_LIMIT = 50;
export const MAX_QUERY_CHARS = 200;
export const MAX_SNIPPET_CHARS = 300;

/** `{ t: "search" }`: answered by `searchResults` (or an error `ack`) with the same rid. */
export const SearchFrame = z.object({
  t: z.literal("search"),
  rid: Rid,
  query: z.string().min(1).max(MAX_QUERY_CHARS),
  /** Only this group. Absent: every group I am a member of. */
  groupId: Id.optional(),
  /** Only this topic; needs `groupId`. */
  topicId: Id.optional(),
  /** Default 20. */
  limit: z.int().min(1).max(MAX_SEARCH_LIMIT).optional(),
});
export type SearchFrame = z.infer<typeof SearchFrame>;

/** A matched range in `snippet`, in Unicode code points. */
export const SearchHighlight = z.object({
  start: z.int().nonnegative(),
  length: z.int().positive(),
});

export const SearchHit = z.object({
  groupId: Id,
  topicId: Id,
  messageId: Id,
  authorId: Id,
  /** Plain text around the match. */
  snippet: z.string().max(MAX_SNIPPET_CHARS),
  highlights: z.array(SearchHighlight).optional(),
  createdAt: Millis,
  /** The message's `createdSeq`. */
  seq: Seq,
});
export type SearchHit = z.infer<typeof SearchHit>;

/** Best match first. */
export const SearchResultsFrame = z.object({
  t: z.literal("searchResults"),
  rid: Rid,
  hits: z.array(SearchHit),
});
export type SearchResultsFrame = z.infer<typeof SearchResultsFrame>;
