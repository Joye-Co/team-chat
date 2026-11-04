// V1 files and 产出 (artifacts) over HTTP, and their group events.
// See docs/protocol.md, "Files" and "产出 (artifacts)".
import { z } from "zod";
import { Count, eventBase, Id, Millis, Mime, Rid, Sha256, Title, utf8Bytes } from "./common.ts";
import { MAX_FILE_NAME_CHARS } from "./message.ts";
import { ClientEventId, Expect } from "./card2.ts";
import { MediaInfo, MediaInfoInput } from "./media.ts";

/**
 * Max bytes of one file upload. Cloudflare Workers accept request bodies up to 100 MB
 * (decimal) on the Free and Pro plans, so the limit is 100,000,000 bytes, not 100 MiB.
 */
export const MAX_FILE_BYTES = 100_000_000;
/** Max bytes of an avatar image. */
export const MAX_AVATAR_BYTES = 1_048_576;
export const MAX_ARTIFACT_FILES = 500;
export const MAX_ARTIFACT_PATH_CHARS = 255;

const FileName = z.string().min(1).max(MAX_FILE_NAME_CHARS);
const FileSize = z.int().nonnegative().max(MAX_FILE_BYTES);
const Pixels = z.int().positive();

export const FileStatus = z.enum(["pending", "ready"]);
export type FileStatus = z.infer<typeof FileStatus>;

/** A stored file. `pending` until its content is uploaded and the SHA-256 matched. */
export const FileMeta = z.object({
  id: Id,
  groupId: Id,
  topicId: Id.nullable().optional(),
  name: FileName,
  size: FileSize,
  mime: Mime,
  sha256: Sha256,
  width: Pixels.optional(),
  height: Pixels.optional(),
  status: FileStatus,
  createdBy: Id,
  createdAt: Millis,
});
export type FileMeta = z.infer<typeof FileMeta>;

/** `POST /v1/files` */
export const CreateFileRequest = z.object({
  groupId: Id,
  topicId: Id.optional(),
  name: FileName,
  size: FileSize,
  mime: Mime,
  sha256: Sha256,
  width: Pixels.optional(),
  height: Pixels.optional(),
});
export type CreateFileRequest = z.infer<typeof CreateFileRequest>;
export const CreateFileResponse = z.object({
  file: FileMeta,
  /** Absolute URL for `PUT` of the raw content (same bearer token). */
  uploadUrl: z.string().min(1),
});
export type CreateFileResponse = z.infer<typeof CreateFileResponse>;
/** `PUT /v1/files/:id/content` and `GET /v1/files/:id` */
export const FileResponse = FileMeta;

export const ArtifactKind = z.enum(["html", "file"]);
export type ArtifactKind = z.infer<typeof ArtifactKind>;

/**
 * True for a safe relative path inside a 产出 version: segments split by `/`, none empty,
 * none `.` or `..`, and no `\\` or NUL anywhere.
 */
export function isArtifactPath(path: string): boolean {
  if (path.includes("\\") || path.includes("\0")) return false;
  return path.split("/").every((seg) => seg !== "" && seg !== "." && seg !== "..");
}

export const ArtifactPath = z
  .string()
  .min(1)
  .max(MAX_ARTIFACT_PATH_CHARS)
  .refine(isArtifactPath, { message: "unsafe artifact path" });

export const Artifact = z.object({
  id: Id,
  groupId: Id,
  topicId: Id,
  title: Title,
  kind: ArtifactKind,
  currentVersion: z.int().positive(),
  createdBy: Id,
  createdAt: Millis,
  updatedAt: Millis,
});
export type Artifact = z.infer<typeof Artifact>;

export const ArtifactFile = z.object({
  path: ArtifactPath,
  fileId: Id,
  size: Count,
  mime: Mime,
});
export type ArtifactFile = z.infer<typeof ArtifactFile>;

/** Max events a 产出 version declares, and the max size of one event payload as JSON. */
export const MAX_ARTIFACT_EVENTS = 20;
export const MAX_ARTIFACT_EVENT_PAYLOAD_BYTES = 16_384;
/** Max UTF-8 bytes of an `artifact.reply` payload and of a data snapshot, as JSON. */
export const MAX_ARTIFACT_REPLY_BYTES = 65_536;
export const MAX_ARTIFACT_DATA_BYTES = 262_144;
/** A data snapshot up to this size travels inline in the `artifact.data` transient. */
export const MAX_ARTIFACT_DATA_INLINE_BYTES = 65_536;
/** Paths under this prefix are reserved for TeamChat (bundled libraries of the page host). */
export const RESERVED_ARTIFACT_PREFIX = "__teamchat/";
export const ArtifactEventName = z.string().min(1).max(128);
/** Grammar of a declared event name on write. */
export const ARTIFACT_EVENT_NAME_RE = /^[a-z][A-Za-z0-9_.-]{0,63}$/;
export const ArtifactEventKind = z.enum(["emit", "ask"]);
export type ArtifactEventKind = z.infer<typeof ArtifactEventKind>;

const TC_SCHEMA_KEYS = new Set(["type", "properties", "required", "enum", "items", "maxItems", "minimum", "maximum", "maxLength", "pattern", "description"]);
/**
 * Checks a TcSchema: a subset of JSON Schema (type, properties, required, enum, items,
 * maxItems, minimum, maximum, maxLength, pattern <= 200 characters), depth <= 4, no `$ref`.
 * Returns the problem, or undefined.
 */
export function tcSchemaProblem(schema: unknown, depth = 1): string | undefined {
  if (depth > 4) return "schema is nested deeper than 4";
  if (!schema || typeof schema !== "object" || Array.isArray(schema)) return "schema is an object";
  for (const [k, v] of Object.entries(schema)) {
    if (!TC_SCHEMA_KEYS.has(k)) return `schema key ${k} is not supported`;
    if (k === "pattern" && (typeof v !== "string" || v.length > 200)) return "pattern is a string of at most 200 characters";
    if (k === "properties") {
      if (!v || typeof v !== "object" || Array.isArray(v)) return "properties is an object";
      for (const p of Object.values(v)) {
        const e = tcSchemaProblem(p, depth + 1);
        if (e) return e;
      }
    }
    if (k === "items") {
      const e = tcSchemaProblem(v, depth + 1);
      if (e) return e;
    }
  }
  return undefined;
}
export const TcSchema = z.json().refine((v) => tcSchemaProblem(v) === undefined, { message: "unsupported schema" });

/** An event an interactive 产出 page may send. Only declared names of the declared kind are accepted. */
export const ArtifactEventDecl = z.object({
  name: ArtifactEventName,
  label: z.string().max(200).optional(),
  /** 1.1: `emit` (default) or `ask` (the page waits for `artifact.reply`). */
  kind: ArtifactEventKind.optional(),
  /** 1.1: the payload's shape. */
  schema: TcSchema.optional(),
});
export type ArtifactEventDecl = z.infer<typeof ArtifactEventDecl>;
const ArtifactEvents = z.array(ArtifactEventDecl).max(MAX_ARTIFACT_EVENTS);
const ArtifactEventsInput = ArtifactEvents.refine((list) => list.every((d) => ARTIFACT_EVENT_NAME_RE.test(d.name)), {
  message: "event names are 1..64 of [a-z][A-Za-z0-9_.-]",
}).refine((list) => new Set(list.map((d) => d.name)).size === list.length, { message: "event names are unique" });
const jsonMax = (max: number, what: string) =>
  z.json().refine((v) => utf8Bytes(JSON.stringify(v)) <= max, { message: `${what} exceeds ${max} bytes` });

export const ArtifactVersion = z.object({
  artifactId: Id,
  version: z.int().positive(),
  /** Path of the file to open: the HTML page, or the only file. */
  entry: ArtifactPath,
  files: z.array(ArtifactFile).min(1).max(MAX_ARTIFACT_FILES),
  note: z.string().max(2000).optional(),
  createdBy: Id,
  createdAt: Millis,
  /** 1.1: the bot run that made this version. */
  runId: Id.optional(),
  /** 1.1: events the page may send back (bot-made versions only). */
  events: ArtifactEvents.optional(),
  /** 1.1: what the version is, for previews (MEDIA). */
  media: MediaInfo.optional(),
  /** 1.1: rev of the latest data snapshot (0 or absent = none). */
  dataRev: Count.optional(),
});
export type ArtifactVersion = z.infer<typeof ArtifactVersion>;

const notReserved = (p: string) => !p.startsWith(RESERVED_ARTIFACT_PREFIX);
/** A path a member may write: safe, and not under `__teamchat/`. */
const WritablePath = ArtifactPath.refine(notReserved, { message: `paths under ${RESERVED_ARTIFACT_PREFIX} are reserved` });
/** A file of a new version: an uploaded file (`ready`, same group) at a path. */
const VersionFileInput = z.object({ path: WritablePath, fileId: Id });

const versionInput = {
  entry: WritablePath,
  files: z.array(VersionFileInput).min(1).max(MAX_ARTIFACT_FILES),
  note: z.string().max(2000).optional(),
  /** 1.1: events the page may send; accepted from bot members only. */
  events: ArtifactEventsInput.optional(),
  /** 1.1: media metadata of the version (computed by the sender). */
  media: MediaInfoInput.optional(),
  /** 1.1: the first data snapshot (<= 256 KiB); a new version starts without data otherwise. */
  data: jsonMax(MAX_ARTIFACT_DATA_BYTES, "data").optional(),
  /** 1.1: the caller bot's active run that made it (checked by the server). */
  runId: Id.optional(),
};

/** `POST /v1/artifacts`: makes the 产出 and its version 1. */
export const CreateArtifactRequest = z.object({
  groupId: Id,
  topicId: Id,
  title: Title,
  kind: ArtifactKind,
  ...versionInput,
});
export type CreateArtifactRequest = z.infer<typeof CreateArtifactRequest>;
/** `POST /v1/artifacts/:id/versions` */
export const AddArtifactVersionRequest = z.object({
  ...versionInput,
  /** Renames the 产出. */
  title: Title.optional(),
});
export type AddArtifactVersionRequest = z.infer<typeof AddArtifactVersionRequest>;
/** Response of `POST /v1/artifacts` and `POST /v1/artifacts/:id/versions`. */
export const ArtifactVersionResponse = z.object({ artifact: Artifact, version: ArtifactVersion });
export type ArtifactVersionResponse = z.infer<typeof ArtifactVersionResponse>;
/** `GET /v1/artifacts/:id`: versions newest first. */
export const ArtifactResponse = z.object({ artifact: Artifact, versions: z.array(ArtifactVersion) });
export type ArtifactResponse = z.infer<typeof ArtifactResponse>;
/** `GET /v1/artifacts?groupId=&topicId=` */
export const ArtifactsResponse = z.array(Artifact);
export type ArtifactsResponse = z.infer<typeof ArtifactsResponse>;

export const ArtifactCreatedEvent = z.object({
  ...eventBase,
  type: z.literal("artifact.created"),
  data: z.object({ artifact: Artifact, version: ArtifactVersion }),
});
export const ArtifactVersionAddedEvent = z.object({
  ...eventBase,
  type: z.literal("artifact.versionAdded"),
  data: z.object({ artifact: Artifact, version: ArtifactVersion }),
});

/** `{ t: "artifact.event" }` (1.1): an interactive 产出 page sent an event. ack.result is `ArtifactEventAck`. */
export const ArtifactEventFrame = z
  .object({
    t: z.literal("artifact.event"),
    rid: Rid,
    groupId: Id,
    artifactId: Id,
    version: z.int().positive(),
    /** 1.1: idempotency key (24 h). */
    clientEventId: ClientEventId.optional(),
    name: ArtifactEventName,
    /** 1.1: the declared kind; absent = `emit`. */
    kind: ArtifactEventKind.optional(),
    payload: z.json().optional(),
  })
  .refine((f) => f.payload === undefined || utf8Bytes(JSON.stringify(f.payload)) <= MAX_ARTIFACT_EVENT_PAYLOAD_BYTES, {
    message: "payload exceeds 16 KiB",
    path: ["payload"],
  });
export type ArtifactEventFrame = z.infer<typeof ArtifactEventFrame>;
export const ArtifactEventAck = z.object({ eventId: Id, duplicate: z.boolean().optional(), expect: Expect });
export type ArtifactEventAck = z.infer<typeof ArtifactEventAck>;

/** `{ t: "artifact.data" }` (1.1): the version's creator bot replaces its data snapshot. ack.result `{dataRev}`. */
export const ArtifactDataFrame = z.object({
  t: z.literal("artifact.data"),
  rid: Rid,
  groupId: Id,
  artifactId: Id,
  version: z.int().positive(),
  data: jsonMax(MAX_ARTIFACT_DATA_BYTES, "data"),
  /** When set, must equal the current rev (`conflict`, `artifact.stale_data`). */
  baseDataRev: Count.optional(),
  /** The event this push answers. */
  cause: Id.optional(),
  clientPushId: ClientEventId.optional(),
});
export type ArtifactDataFrame = z.infer<typeof ArtifactDataFrame>;
export const ArtifactDataAck = z.object({ dataRev: z.int().positive() });
export type ArtifactDataAck = z.infer<typeof ArtifactDataAck>;

export const ArtifactReplyError = z.object({
  code: z.enum(["declined", "failed"]),
  message: z.string().max(200),
  /** 1.1: why, as a short id (`run.ended`: no run of the member answers the page). */
  reason: z.string().min(1).max(64).optional(),
});
/** `{ t: "artifact.reply" }` (1.1): the bot answers a page `ask` (private to the asker). */
export const ArtifactReplyFrame = z
  .object({
    t: z.literal("artifact.reply"),
    rid: Rid,
    groupId: Id,
    eventId: Id,
    ok: z.boolean(),
    payload: jsonMax(MAX_ARTIFACT_REPLY_BYTES, "payload").optional(),
    error: ArtifactReplyError.optional(),
  })
  .refine((f) => (f.ok ? f.error === undefined : f.error !== undefined && f.payload === undefined), {
    message: "ok: true takes a payload, ok: false an error",
  });
export type ArtifactReplyFrame = z.infer<typeof ArtifactReplyFrame>;

/** Transient (1.1, to the version's creator bot): a page event. */
export const ArtifactEventTransient = z.object({
  type: z.literal("artifact.event"),
  groupId: Id,
  topicId: Id,
  artifactId: Id,
  version: z.int().positive(),
  eventId: Id,
  name: ArtifactEventName,
  kind: ArtifactEventKind,
  payload: z.json().optional(),
  actorId: Id,
  at: Millis,
});
/** Transient (1.1, to every member's connections): a new data snapshot; `data` inline when <= 64 KiB. */
export const ArtifactDataTransient = z.object({
  type: z.literal("artifact.data"),
  groupId: Id,
  artifactId: Id,
  version: z.int().positive(),
  dataRev: z.int().positive(),
  data: z.json().optional(),
  cause: Id.optional(),
});
/** Transient (1.1, to the asker's connections only): the bot's answer to a page `ask`. */
export const ArtifactReplyTransient = z.object({
  type: z.literal("artifact.reply"),
  groupId: Id,
  eventId: Id,
  ok: z.boolean(),
  payload: z.json().optional(),
  error: ArtifactReplyError.optional(),
});

/** `GET /v1/artifacts/:id/versions/:v/data` (group members). */
export const ArtifactDataResponse = z.object({ data: z.json().nullable(), dataRev: Count, updatedAt: Millis });
export type ArtifactDataResponse = z.infer<typeof ArtifactDataResponse>;
/** `PUT /v1/artifacts/:id/versions/:v/data` (the version's creator bot). Response `ArtifactDataAck`. */
export const PutArtifactDataRequest = z.object({
  data: jsonMax(MAX_ARTIFACT_DATA_BYTES, "data"),
  baseDataRev: Count.optional(),
  cause: Id.optional(),
});
export type PutArtifactDataRequest = z.infer<typeof PutArtifactDataRequest>;

/**
 * `PUT /v1/artifacts/:id/versions/:v/media` (the version's creator): the media of a version with
 * its preview, sent after the upload. Stored as `artifact.previewReady`.
 */
export const PutArtifactMediaRequest = z.object({ media: MediaInfoInput });
export type PutArtifactMediaRequest = z.infer<typeof PutArtifactMediaRequest>;
/** Answer of `PUT /v1/artifacts/:id/versions/:v/media`: the version with the new media. */
export const PutArtifactMediaResponse = z.object({ version: ArtifactVersion });
export type PutArtifactMediaResponse = z.infer<typeof PutArtifactMediaResponse>;
