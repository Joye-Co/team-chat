// V1 message extensions: quotes, attachments, 产出 references, cards and message kinds.
// See docs/protocol.md, "Messages: quotes, attachments, cards".
import { z } from "zod";
import { Card } from "./card.ts";
import { BlockId, CardRev, CardState, CardV2, Cause, ReadCardV2, PartRef } from "./card2.ts";
import { Count, Id, Mime, Sha256, utf8Bytes } from "./common.ts";
import { MediaInfo } from "./media.ts";

export const MAX_ATTACHMENTS = 20;
export const MAX_ARTIFACT_REFS = 20;
/** `replyTo.excerpt` is the first this-many code points of the quoted body. */
export const MAX_EXCERPT_CHARS = 200;
/** Max file name length, in code points. */
export const MAX_FILE_NAME_CHARS = 255;

export const MessageKind = z.enum(["text", "card", "system"]);
export type MessageKind = z.infer<typeof MessageKind>;

/** A quoted message. The server fills `authorId` and `excerpt`. */
export const ReplyTo = z.object({
  messageId: Id,
  authorId: Id,
  excerpt: z.string().max(MAX_EXCERPT_CHARS),
});
export type ReplyTo = z.infer<typeof ReplyTo>;

/** What a client sends to quote a message. Other fields are ignored. */
export const ReplyToRef = z.object({ messageId: Id });
export type ReplyToRef = z.infer<typeof ReplyToRef>;

/** An uploaded file, as a message attachment. */
export const FileRef = z.object({
  fileId: Id,
  name: z.string().min(1).max(MAX_FILE_NAME_CHARS),
  size: Count,
  mime: Mime,
  /** Pixel size, images only. */
  width: z.int().positive().optional(),
  height: z.int().positive().optional(),
  sha256: Sha256,
  /** 1.1: what the file is, for inline previews (computed by the sender). */
  media: MediaInfo.optional(),
  /** 1.1: the frame of a 产出 version this file was taken from (把这一帧发到话题), `t` in seconds. */
  source: z.object({ artifactId: Id, version: z.int().positive(), t: z.number().nonnegative() }).optional(),
});
export type FileRef = z.infer<typeof FileRef>;

/** A reference to one version of a 产出. */
export const ArtifactRef = z.object({
  artifactId: Id,
  version: z.int().positive(),
});
export type ArtifactRef = z.infer<typeof ArtifactRef>;

const Attachments = z.array(FileRef).max(MAX_ATTACHMENTS);

// ---------------------------------------------------------------------------
// References to a part (引用提问), 1.1
// ---------------------------------------------------------------------------

/** Max `blockRefs` of one message. */
export const MAX_BLOCK_REFS = 5;
/** Max code points of a ref label. */
export const MAX_BLOCK_REF_LABEL_CHARS = 120;
/** Max UTF-8 bytes of a ref `value` as JSON. */
export const MAX_BLOCK_REF_VALUE_BYTES = 2048;
/** Max v1 card blocks (`v1:<0..49>`) and Markdown blocks (`md:<0..199>`) a ref can name. */
export const MAX_BLOCK_REF_V1_INDEX = 49;
export const MAX_BLOCK_REF_MD_INDEX = 199;

/**
 * A ref's `blockId`: a Card v2 block id, `card` (the whole card), `page` (a 产出 page),
 * `v1:<index>` (a v1 card block) or `md:<index>` (a top-level Markdown block of the body).
 */
export function isBlockRefId(id: string): boolean {
  const m = /^(v1|md):(0|[1-9][0-9]{0,2})$/.exec(id);
  if (m) return Number(m[2]) <= (m[1] === "v1" ? MAX_BLOCK_REF_V1_INDEX : MAX_BLOCK_REF_MD_INDEX);
  if (id.startsWith("v1:") || id.startsWith("md:")) return false;
  return BlockId.safeParse(id).success;
}
export const BlockRefId = z.string().refine(isBlockRefId, { message: "blockId is a block id, card, page, v1:<n> or md:<n>" });

const RefLabel = (min: number) =>
  z
    .string()
    .min(min)
    .max(MAX_BLOCK_REF_LABEL_CHARS)
    .refine((s) => ![...s].some((ch) => (ch.codePointAt(0) ?? 0) < 0x20 || ch === "\u007f"), {
      message: "label has no control characters",
    });
const blockRefFields = {
  /** The source message (same group). Exactly one of `messageId` / `artifact`. */
  messageId: Id.optional(),
  /** A 产出 version (same group). */
  artifact: z.object({ artifactId: Id, version: z.int().positive() }).optional(),
  /**
   * 1.1: the group of the source when it is not this group: a side chat (单独问) quoting a
   * message or 产出 of the group it is about. The sender must be a member of both.
   */
  sourceGroupId: Id.optional(),
  /** The Card v2 `rev` the sender saw. */
  cardRev: CardRev.optional(),
  blockId: BlockRefId,
  /** Absent = the whole block. */
  part: PartRef.optional(),
  /** The sender's snapshot of what they saw (<= 2048 bytes as JSON). */
  value: z
    .json()
    .refine((v) => utf8Bytes(JSON.stringify(v)) <= MAX_BLOCK_REF_VALUE_BYTES, { message: "value exceeds 2048 bytes" })
    .optional(),
};
const oneSource = (r: { messageId?: string; artifact?: unknown }) => (r.messageId === undefined) !== (r.artifact === undefined);

/** What a client sends: a reference to one part of a message block or a 产出 page. */
export const BlockRefInput = z
  .object({ ...blockRefFields, label: RefLabel(1) })
  .refine(oneSource, { message: "exactly one of messageId and artifact" });
export type BlockRefInput = z.infer<typeof BlockRefInput>;

/** What the server stores and sends. A deleted source: `label` "", no `value`, `deleted: true`. */
export const BlockRef = z
  .object({
    ...blockRefFields,
    label: RefLabel(0),
    /** Author of the source message, or creator of the 产出 version. */
    sourceAuthorId: Id.optional(),
    sourceTopicId: Id,
    deleted: z.literal(true).optional(),
  })
  .refine(oneSource, { message: "exactly one of messageId and artifact" });
export type BlockRef = z.infer<typeof BlockRef>;
const BlockRefs = z.array(BlockRef).max(MAX_BLOCK_REFS);
const BlockRefInputs = z.array(BlockRefInput).min(1).max(MAX_BLOCK_REFS);

/** The quote line the server writes in front of the body for each ref (1.0.x readers). */
export function blockRefPrefix(labels: string[]): string {
  return labels.map((l) => `> ↳ ${l}`).join("\n");
}
const ArtifactRefs = z.array(ArtifactRef).max(MAX_ARTIFACT_REFS);

/** Fields added to `Message`. Absent: kind "text", no quote, no attachments, no card. */
export const messageFields = {
  kind: MessageKind.optional(),
  replyTo: ReplyTo.optional(),
  attachments: Attachments.optional(),
  artifactRefs: ArtifactRefs.optional(),
  card: Card.optional(),
  /** Card v2 (1.1). Next to it the server stores a derived v1 `card` for older readers. */
  cardV2: ReadCardV2,
  /** 2.1: server-owned state of the card (asks, answers, tallies). */
  cardState: CardState.optional(),
  /** 2.1: the event this message (or its newest rev) answered. */
  cause: Cause.optional(),
  /** 1.1: the parts this message asks about (引用提问). */
  blockRefs: BlockRefs.optional(),
};

/** Fields added to the `send` client frame (and `POST /v1/groups/:id/messages`). */
export const sendFields = {
  replyTo: ReplyToRef.optional(),
  attachments: Attachments.optional(),
  artifactRefs: ArtifactRefs.optional(),
  card: Card.optional(),
  /** Makes a card message; without `card` the server derives the v1 card (`cardV2ToV1`). */
  cardV2: CardV2.optional(),
  /** 2.1: the event (`eventId`) this message answers; the server fills `Message.cause`. */
  causeEventId: Id.optional(),
  /** 1.1: refs to parts; the server writes their quote lines in front of the body. */
  blockRefs: BlockRefInputs.optional(),
};

/** Fields added to the `edit` client frame. Present = replace; absent = keep. */
export const editFields = {
  attachments: Attachments.optional(),
  artifactRefs: ArtifactRefs.optional(),
  card: Card.optional(),
  /** A card message only; `rev` must be higher than the stored one. */
  cardV2: CardV2.optional(),
  /** 2.1: with `cardV2`, the rev the edit is based on (`conflict`, `card.stale_rev`). */
  baseRev: CardRev.optional(),
  /** 2.1: the event this edit answers. */
  causeEventId: Id.optional(),
};

/** Fields added to `message.updated` event data. Present = replaced with this value. */
export const messageUpdatedFields = {
  attachments: Attachments.optional(),
  artifactRefs: ArtifactRefs.optional(),
  card: Card.optional(),
  cardV2: ReadCardV2,
  cardState: CardState.optional(),
  cause: Cause.optional(),
  blockRefs: BlockRefs.optional(),
};
