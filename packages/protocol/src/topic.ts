// V1 topic extensions: who the topic waits on, its responsible member, last activity,
// and following a topic. See docs/protocol.md, "Topics: waitingOn, owner, follow".
import { z } from "zod";
import { Id, Millis, Rid, Seq } from "./common.ts";

/** Max entries in `waitingOn`. */
export const MAX_WAITING_ON = 20;

const WaitingOn = z.array(Id).max(MAX_WAITING_ON);

/**
 * A side chat (单独问) points at the topic it is about: the asker's DM topic with a bot reads
 * `topicId` of `groupId` up to `uptoSeq`. See docs/protocol.md, "Side chats".
 */
export const TopicContext = z.object({ groupId: Id, topicId: Id, uptoSeq: Seq });
export type TopicContext = z.infer<typeof TopicContext>;

/** Fields added to `Topic`. Absent `waitingOn` means `[]`; absent `ownerId` means `null`. */
export const topicFields = {
  /** Members the topic waits on (status chip 等你回答 / 等你审). */
  waitingOn: WaitingOn.optional(),
  /** The one responsible member, or null. */
  ownerId: Id.nullable().optional(),
  /** `createdAt` of the newest message in the topic. */
  lastMessageAt: Millis.optional(),
  /** Set on a side chat topic (1.1): the topic it is about. */
  contextOf: TopicContext.optional(),
};

/** Fields added to the `send` client frame (1.1): start or move on a side chat. */
export const sendContextFields = {
  context: TopicContext.optional(),
};

/** Fields added to the `topic.update` client frame. Present = replace. */
export const topicUpdateFields = {
  waitingOn: WaitingOn.optional(),
  /** null clears the responsible member. */
  ownerId: Id.nullable().optional(),
};

/** Fields added to `topic.updated` event data. Present = changed to this value. */
export const topicUpdatedFields = {
  waitingOn: WaitingOn.optional(),
  ownerId: Id.nullable().optional(),
  lastMessageAt: Millis.optional(),
  /** A later `send.context` moved the side chat's `uptoSeq`. */
  contextOf: TopicContext.optional(),
};

/** `{ t: "topic.follow" }`: follow or unfollow a topic. ack.result = `TopicState`. */
export const TopicFollowFrame = z.object({
  t: z.literal("topic.follow"),
  rid: Rid,
  groupId: Id,
  topicId: Id,
  follow: z.boolean(),
});
export type TopicFollowFrame = z.infer<typeof TopicFollowFrame>;
