// Topic links (1.1): structured state of things a topic points at, such as a pull request
// with its checks. See docs/protocol.md, "Topic links".
import { z } from "zod";
import { WebUrl } from "./card.ts";
import { Count, eventBase, Id } from "./common.ts";

export const MAX_LINK_CHECKS = 100;

export const PrCheck = z.object({
  name: z.string().min(1).max(200),
  /** GitHub conclusion in plain text, e.g. "success", "failure"; "pending" while it runs. */
  conclusion: z.string().min(1).max(32),
});
export type PrCheck = z.infer<typeof PrCheck>;

export const TopicLink = z.discriminatedUnion("kind", [
  z.object({
    kind: z.literal("pr"),
    /** `owner/name`. */
    repo: z.string().min(1).max(200),
    number: z.int().positive(),
    url: WebUrl,
    title: z.string().max(500),
    /** Plain text: "open", "draft", "merged" or "closed". */
    state: z.string().min(1).max(32),
    checks: z.array(PrCheck).max(MAX_LINK_CHECKS),
    additions: Count.optional(),
    deletions: Count.optional(),
    changedFiles: Count.optional(),
  }),
]);
export type TopicLink = z.infer<typeof TopicLink>;

/** Stored event: the newest state of a link of a topic. A link with the same `kind` and `url` replaces the older one. */
export const TopicLinkedEvent = z.object({
  ...eventBase,
  type: z.literal("topic.linked"),
  data: z.object({ topicId: Id, link: TopicLink }),
});
