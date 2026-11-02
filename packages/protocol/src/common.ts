// Shared primitives and limits. Area modules import from here, never from index.ts.
import { z } from "zod";

export const PROTOCOL_VERSION = 1 as const;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/** Max message body length, in Unicode code points (zod v4 counts code points). */
export const MAX_BODY_CHARS = 32_768;
/** Max topic title length, in Unicode code points. */
export const MAX_TITLE_CHARS = 200;
/** Max `limit` for `fetch` and `history`. */
export const MAX_PAGE_LIMIT = 500;
/** Max size of one WebSocket frame, in bytes. */
export const MAX_FRAME_BYTES = 1_048_576;
/** Member display name length range. */
export const MIN_NAME_CHARS = 1;
export const MAX_NAME_CHARS = 64;
/** Default topic title: first line of body, cut to this many characters. */
export const DEFAULT_TITLE_CHARS = 80;

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

export const Id = z.string().min(1);
export const ClientMsgId = z.string().min(1).max(64);
/** Milliseconds since the Unix epoch, UTC. */
export const Millis = z.int().nonnegative();
export const Seq = z.int().nonnegative();
export const Count = z.int().nonnegative();
export const Rid = z.string().min(1);
export const Name = z.string().min(MIN_NAME_CHARS).max(MAX_NAME_CHARS);
export const Body = z.string().max(MAX_BODY_CHARS);
export const Title = z.string().min(1).max(MAX_TITLE_CHARS);
export const Limit = z.int().min(1).max(MAX_PAGE_LIMIT);
/** Lowercase hex SHA-256 digest. */
export const Sha256 = z.string().regex(/^[0-9a-f]{64}$/);
/** MIME type such as `image/png`. */
export const Mime = z.string().min(3).max(255).regex(/^[\w.+-]+\/[\w.+-]+$/);

/** Fields every stored group event has. */
export const eventBase = {
  seq: z.int().positive(),
  groupId: Id,
  at: Millis,
  actorId: Id,
};

/** UTF-8 byte length of a string. */
export function utf8Bytes(text: string): number {
  return new TextEncoder().encode(text).byteLength;
}

export const MemberLevel = z.enum(["owner", "member"]);
export type MemberLevel = z.infer<typeof MemberLevel>;
