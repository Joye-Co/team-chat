// V1 message extensions: quotes, attachments, 产出 references, cards and message kinds.
// See docs/protocol.md, "Messages: quotes, attachments, cards".
import { z } from "zod";
import { Card } from "./card.ts";
import { CardV2, ReadCardV2 } from "./card2.ts";
import { Count, Id, Mime, Sha256 } from "./common.ts";

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
});
export type FileRef = z.infer<typeof FileRef>;

/** A reference to one version of a 产出. */
export const ArtifactRef = z.object({
  artifactId: Id,
  version: z.int().positive(),
});
export type ArtifactRef = z.infer<typeof ArtifactRef>;

const Attachments = z.array(FileRef).max(MAX_ATTACHMENTS);
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
};

/** Fields added to the `send` client frame (and `POST /v1/groups/:id/messages`). */
export const sendFields = {
  replyTo: ReplyToRef.optional(),
  attachments: Attachments.optional(),
  artifactRefs: ArtifactRefs.optional(),
  card: Card.optional(),
  /** Makes a card message; without `card` the server derives the v1 card (`cardV2ToV1`). */
  cardV2: CardV2.optional(),
};

/** Fields added to the `edit` client frame. Present = replace; absent = keep. */
export const editFields = {
  attachments: Attachments.optional(),
  artifactRefs: ArtifactRefs.optional(),
  card: Card.optional(),
  /** A card message only; `rev` must be higher than the stored one. */
  cardV2: CardV2.optional(),
};

/** Fields added to `message.updated` event data. Present = replaced with this value. */
export const messageUpdatedFields = {
  attachments: Attachments.optional(),
  artifactRefs: ArtifactRefs.optional(),
  card: Card.optional(),
  cardV2: ReadCardV2,
};
