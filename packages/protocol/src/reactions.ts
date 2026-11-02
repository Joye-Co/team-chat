// Emoji reactions on messages. See docs/protocol.md, "Reactions".
import { z } from "zod";
import { eventBase, Id, Rid } from "./common.ts";

/** Most distinct emoji on one message. */
export const MAX_REACTIONS = 50;
/** One emoji (with its modifiers and joiners), in Unicode code points. */
export const MAX_EMOJI_CHARS = 16;

/** One emoji: 1..16 code points, no whitespace. */
export const Emoji = z
  .string()
  .min(1)
  .refine((s) => [...s].length <= MAX_EMOJI_CHARS, { message: `at most ${MAX_EMOJI_CHARS} code points` })
  .refine((s) => !/\s/u.test(s), { message: "no whitespace" });

/** Who reacted with one emoji, oldest first. */
export const Reaction = z.object({
  emoji: Emoji,
  memberIds: z.array(Id).min(1),
});
export type Reaction = z.infer<typeof Reaction>;

/** Fields added to `Message`. Absent = no reactions. */
export const messageReactionFields = {
  reactions: z.array(Reaction).max(MAX_REACTIONS).optional(),
};

/** `{ t: "react" }`: add (`on: true`) or remove my reaction. ack.result = `ReactResult`. */
export const ReactFrame = z.object({
  t: z.literal("react"),
  rid: Rid,
  groupId: Id,
  messageId: Id,
  emoji: Emoji,
  on: z.boolean(),
});
export type ReactFrame = z.infer<typeof ReactFrame>;

/** `ack.result` of `react`: false when my reaction already was as asked (no event). */
export const ReactResult = z.object({ changed: z.boolean() });
export type ReactResult = z.infer<typeof ReactResult>;

/** Stored event: a member added or removed a reaction. `actorId` is that member. */
export const ReactionChangedEvent = z.object({
  ...eventBase,
  type: z.literal("reaction.changed"),
  data: z.object({
    messageId: Id,
    emoji: Emoji,
    on: z.boolean(),
  }),
});
