// V1 topic extensions: who the topic waits on, its responsible member, last activity,
// and following or muting a topic. See docs/protocol.md, "Topics: waitingOn, owner, follow, mute".
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
  /** The 群's own chat (群聊, 1.1.1): its id is `mainTopicId(groupId)`. Absent = false. */
  main: z.boolean().optional(),
};

/**
 * The 群聊 (1.1.1): every group has one chat topic with a fixed id, so a client can open and
 * send into it before it exists; the server creates it with the first message sent there.
 * See docs/protocol.md, "群聊: the group's own chat".
 */
export const MAIN_TOPIC_PREFIX = "main-";

/** The id of a group's 群聊 topic. */
export function mainTopicId(groupId: string): string {
  return `${MAIN_TOPIC_PREFIX}${groupId}`;
}

/** Whether `topicId` names a 群聊 (of any group; compare with `mainTopicId` for one group). */
export function isMainTopicId(topicId: string): boolean {
  return topicId.startsWith(MAIN_TOPIC_PREFIX);
}

/**
 * 1.1: the 单独问 answer a message was posted from ("post to topic"): the side chat topic in
 * the asker's DM with the bot, and the answer message there. See docs/protocol.md, "Side chats".
 */
export const AsideOrigin = z.object({ groupId: Id, topicId: Id, messageId: Id });
export type AsideOrigin = z.infer<typeof AsideOrigin>;

/** Fields added to the `send` client frame (1.1): start or move on a side chat. */
export const sendContextFields = {
  context: TopicContext.optional(),
  /** 1.1: this send posts a side chat answer to the side chat's source topic. */
  fromAside: AsideOrigin.optional(),
};

/** Fields added to `Message` (1.1). */
export const messageAsideFields = {
  /** The side chat answer this message was posted from. */
  fromAside: AsideOrigin.optional(),
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

/** `{ t: "topic.mute" }`: mute or unmute a topic for me (only mentions notify). ack.result = `TopicState`. */
export const TopicMuteFrame = z.object({
  t: z.literal("topic.mute"),
  rid: Rid,
  groupId: Id,
  topicId: Id,
  muted: z.boolean(),
});
export type TopicMuteFrame = z.infer<typeof TopicMuteFrame>;

/** `{ t: "topic.hide" }`: hide or show a topic for me (until a message mentions me). ack.result = `TopicState`. */
export const TopicHideFrame = z.object({
  t: z.literal("topic.hide"),
  rid: Rid,
  groupId: Id,
  topicId: Id,
  hidden: z.boolean(),
});
export type TopicHideFrame = z.infer<typeof TopicHideFrame>;

/** `{ t: "topic.delete" }`: delete a topic for everyone (its creator or an owner). ack.result = `TopicDeleteResult`. */
export const TopicDeleteFrame = z.object({
  t: z.literal("topic.delete"),
  rid: Rid,
  groupId: Id,
  topicId: Id,
});
export type TopicDeleteFrame = z.infer<typeof TopicDeleteFrame>;

/** ack.result of `topic.delete`, and the data of the `topic.deleted` event. */
export const TopicDeleteResult = z.object({ topicId: Id, deletedAt: Millis });
export type TopicDeleteResult = z.infer<typeof TopicDeleteResult>;
