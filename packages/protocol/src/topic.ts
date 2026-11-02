// V1 topic extensions: who the topic waits on, its responsible member, last activity,
// and following a topic. See docs/protocol.md, "Topics: waitingOn, owner, follow".
import { z } from "zod";
import { Id, Millis, Rid } from "./common.ts";

/** Max entries in `waitingOn`. */
export const MAX_WAITING_ON = 20;

const WaitingOn = z.array(Id).max(MAX_WAITING_ON);

/** Fields added to `Topic`. Absent `waitingOn` means `[]`; absent `ownerId` means `null`. */
export const topicFields = {
  /** Members the topic waits on (status chip 等你回答 / 等你审). */
  waitingOn: WaitingOn.optional(),
  /** The one responsible member, or null. */
  ownerId: Id.nullable().optional(),
  /** `createdAt` of the newest message in the topic. */
  lastMessageAt: Millis.optional(),
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
