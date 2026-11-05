// V1 per-topic read positions, unread and mention counts, follow and mute state.
// See docs/protocol.md, "Read positions per topic".
import { z } from "zod";
import { Count, Id, Seq } from "./common.ts";

/** One member's state in one topic. */
export const TopicState = z.object({
  topicId: Id,
  /** Messages of this topic with `createdSeq <= readSeq` are read. Never below the group `readSeq`. */
  readSeq: Seq,
  /** Unread messages in this topic not written by me. */
  unread: Count,
  /** Unread messages in this topic that mention me. */
  mentions: Count,
  /** I follow this topic. Absent means false. */
  following: z.boolean().optional(),
  /** I muted this topic: only mentions notify. Absent means false. */
  muted: z.boolean().optional(),
});
export type TopicState = z.infer<typeof TopicState>;

/** Field added to the `read` client frame: with it, move only this topic's read position. */
export const readFields = {
  topicId: Id.optional(),
};

/** Field added to `GroupSummary`: every topic with unread > 0, mentions > 0, following or muted. */
export const groupSummaryReadFields = {
  topics: z.array(TopicState).optional(),
};

/**
 * `{ t: "reads" }`: my read state changed (from this or another device of mine, or a
 * follow or mute change). Group totals plus only the topics that changed.
 */
export const ReadsFrame = z.object({
  t: z.literal("reads"),
  groupId: Id,
  readSeq: Seq,
  unread: Count,
  mentions: Count,
  topics: z.array(TopicState),
});
export type ReadsFrame = z.infer<typeof ReadsFrame>;
