// V1 files and 产出 (artifacts) over HTTP, and their group events.
// See docs/protocol.md, "Files" and "产出 (artifacts)".
import { z } from "zod";
import { Count, eventBase, Id, Millis, Mime, Sha256, Title } from "./common.ts";
import { MAX_FILE_NAME_CHARS } from "./message.ts";

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

export const ArtifactVersion = z.object({
  artifactId: Id,
  version: z.int().positive(),
  /** Path of the file to open: the HTML page, or the only file. */
  entry: ArtifactPath,
  files: z.array(ArtifactFile).min(1).max(MAX_ARTIFACT_FILES),
  note: z.string().max(2000).optional(),
  createdBy: Id,
  createdAt: Millis,
});
export type ArtifactVersion = z.infer<typeof ArtifactVersion>;

/** A file of a new version: an uploaded file (`ready`, same group) at a path. */
const VersionFileInput = z.object({ path: ArtifactPath, fileId: Id });

const versionInput = {
  entry: ArtifactPath,
  files: z.array(VersionFileInput).min(1).max(MAX_ARTIFACT_FILES),
  note: z.string().max(2000).optional(),
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
