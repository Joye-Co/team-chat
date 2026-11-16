// Topic links (1.1): structured state of things a topic points at, such as a pull request
// with its checks. See docs/protocol.md, "Topic links".
import { z } from "zod";
import { WebUrl } from "./card.ts";
import { Count, eventBase, Id, Millis, Sha256 } from "./common.ts";

export const MAX_LINK_CHECKS = 100;

export const PrCheck = z.object({
  name: z.string().min(1).max(200),
  /** GitHub conclusion in plain text, e.g. "success", "failure"; "pending" while it runs. */
  conclusion: z.string().min(1).max(32),
});
export type PrCheck = z.infer<typeof PrCheck>;

/**
 * 1.1: a queued merge of a pull request, from GitHub (`enqueued`, `auto_merge_enabled`,
 * `dequeued`, `auto_merge_disabled`). `error` is set when the merge stopped without merging.
 */
export const PrMerge = z.object({
  queuedAt: Millis,
  /** Plain text from GitHub, e.g. "merge conflict", "checks failed". */
  error: z.string().min(1).max(200).optional(),
});
export type PrMerge = z.infer<typeof PrMerge>;

/** At most this many files in a `PrDiff`. */
export const MAX_PR_DIFF_FILES = 300;
/** The longest patch of one file in a `PrDiff`; a longer one is left out (`patch` absent). */
export const MAX_PR_PATCH_CHARS = 65536;

/**
 * The diff of a pull request, uploaded as a JSON file (`PrDiff`) by the computer of the run that
 * named it and read by clients from the file. `size` and `sha256` let a client cache it.
 */
export const PrDiffRef = z.object({
  fileId: Id.max(64),
  name: z.string().min(1).max(200),
  size: Count,
  sha256: Sha256,
});
export type PrDiffRef = z.infer<typeof PrDiffRef>;

/** One file of a pull request's diff, as GitHub lists it (`GET /repos/{o}/{r}/pulls/{n}/files`). */
export const PrDiffFile = z.object({
  path: z.string().min(1).max(1024),
  /** The old path of a renamed file. */
  previousPath: z.string().min(1).max(1024).optional(),
  /** GitHub's word: "added", "removed", "modified", "renamed", "copied", "changed", "unchanged". */
  status: z.string().min(1).max(32),
  additions: Count,
  deletions: Count,
  /** The file's unified diff hunks; absent for a binary file or one GitHub or the size limit left out. */
  patch: z.string().max(MAX_PR_PATCH_CHARS).optional(),
});
export type PrDiffFile = z.infer<typeof PrDiffFile>;

/** The content of a `PrDiffRef` file. `truncated`: the pull request has more files than listed. */
export const PrDiff = z.object({
  files: z.array(PrDiffFile).max(MAX_PR_DIFF_FILES),
  truncated: z.boolean().optional(),
});
export type PrDiff = z.infer<typeof PrDiff>;

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
    /** 1.1: the pull request waits to merge; absent = not queued (or merged). */
    merge: PrMerge.optional(),
    /** The pull request's diff, uploaded by a run's computer; absent = not known. */
    diff: PrDiffRef.optional(),
  }),
]);
export type TopicLink = z.infer<typeof TopicLink>;

/** Stored event: the newest state of a link of a topic. A link with the same `kind` and `url` replaces the older one. */
export const TopicLinkedEvent = z.object({
  ...eventBase,
  type: z.literal("topic.linked"),
  data: z.object({ topicId: Id, link: TopicLink }),
});
