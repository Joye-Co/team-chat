// HTTP routes for files, 产出 and avatars. Every request checks group membership in D1.
import {
  AddArtifactVersionRequest,
  type Artifact,
  type ArtifactResponse,
  type ArtifactVersionResponse,
  CreateArtifactRequest,
  CreateFileRequest,
  type FileMeta,
  isArtifactPath,
  MAX_AVATAR_BYTES,
  MAX_FILE_BYTES,
} from "@chat/protocol";
import { z } from "zod";
import { auditStatement } from "../core/audit";
import type { AuthContext } from "../core/auth";
import { getMember, isGroupMember, toMember } from "../core/db";
import { type Env, roomStub } from "../core/env";
import { ApiError, unwrap } from "../core/errors";
import { ulid } from "../core/ids";
import { registerRoute } from "../core/registry";
import { notifyMembersChanged } from "../core/routes";
import type { RegisterFileArgs } from "./room";
import {
  actorOf,
  checkDeclaredLength,
  contentDisposition,
  INLINE_MIMES,
  putVerified,
  rateHit,
  readBytes,
  SAFE_HEADERS,
  serveObject,
} from "./store";

/** Files a member may create per hour, and their declared bytes. */
export const FILE_RATE = { windowMs: 3600_000, files: 500, bytes: 5 * 1024 ** 3 };
/** Bytes of a member's files that are not deleted, at most. */
export const MEMBER_FILE_QUOTA = 100 * 1024 ** 3;

/** A `pending` file older than this is deleted. */
export const PENDING_TTL_MS = 24 * 3600_000;

export const fileKey = (groupId: string, fileId: string) => `files/${groupId}/${fileId}`;
export const avatarKey = (memberId: string) => `avatars/${memberId}`;

export type FileRow = {
  id: string;
  group_id: string;
  topic_id: string | null;
  name: string;
  size: number;
  mime: string;
  sha256: string;
  width: number | null;
  height: number | null;
  status: "pending" | "ready";
  created_by: string;
  created_at: number;
  /** Set when every message that used the file was deleted. */
  deleted_at?: number | null;
};

export function toFileMeta(r: FileRow): FileMeta {
  const f: FileMeta = {
    id: r.id,
    groupId: r.group_id,
    topicId: r.topic_id,
    name: r.name,
    size: r.size,
    mime: r.mime,
    sha256: r.sha256,
    status: r.status,
    createdBy: r.created_by,
    createdAt: r.created_at,
  };
  if (r.width !== null) f.width = r.width;
  if (r.height !== null) f.height = r.height;
  return f;
}

/** Not a member and unknown id look the same, so ids of other groups do not leak. */
async function requireGroupMember(env: Env, groupId: string, auth: AuthContext): Promise<void> {
  if (!(await isGroupMember(env.DB, groupId, auth.memberId))) throw new ApiError("not_found", "not found");
}

async function loadFile(env: Env, id: string, auth: AuthContext): Promise<FileRow> {
  const row = await env.DB.prepare("SELECT * FROM files WHERE id = ?").bind(id).first<FileRow>();
  if (!row || row.deleted_at) throw new ApiError("not_found", "file not found");
  await requireGroupMember(env, row.group_id, auth);
  return row;
}

async function deleteStalePending(env: Env): Promise<void> {
  const cutoff = Date.now() - PENDING_TTL_MS;
  const stale = await env.DB.prepare(
    "SELECT id, group_id FROM files WHERE status = 'pending' AND created_at < ? LIMIT 50",
  )
    .bind(cutoff)
    .all<{ id: string; group_id: string }>();
  if (stale.results.length === 0) return;
  await env.FILES.delete(stale.results.map((r) => fileKey(r.group_id, r.id)));
  await env.DB.batch(
    stale.results.map((r) => env.DB.prepare("DELETE FROM files WHERE id = ? AND status = 'pending'").bind(r.id)),
  );
}

/** Guesses a better type for `application/octet-stream` 产出 files from the extension. */
const EXT_MIME: Record<string, string> = {
  html: "text/html",
  htm: "text/html",
  css: "text/css",
  js: "text/javascript",
  mjs: "text/javascript",
  json: "application/json",
  svg: "image/svg+xml",
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  woff: "font/woff",
  woff2: "font/woff2",
  ttf: "font/ttf",
  otf: "font/otf",
  txt: "text/plain",
  md: "text/markdown",
  csv: "text/csv",
  pdf: "application/pdf",
  wasm: "application/wasm",
};

function artifactMime(mime: string, path: string): string {
  if (mime !== "application/octet-stream") return mime;
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  return EXT_MIME[ext] ?? mime;
}

/** Text types get an explicit charset so pages and scripts decode as UTF-8. */
function withCharset(mime: string): string {
  if (mime.startsWith("text/") || mime === "application/json" || mime === "image/svg+xml") {
    return `${mime}; charset=utf-8`;
  }
  return mime;
}

export const ARTIFACT_CSP =
  "sandbox; default-src 'none'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; " +
  "script-src 'self' 'unsafe-inline'; font-src 'self' data:";

const IMAGE_TYPES: { mime: string; test: (b: Uint8Array) => boolean }[] = [
  { mime: "image/png", test: (b) => b.length > 8 && b[0] === 0x89 && b[1] === 0x50 && b[2] === 0x4e && b[3] === 0x47 },
  { mime: "image/jpeg", test: (b) => b.length > 3 && b[0] === 0xff && b[1] === 0xd8 && b[2] === 0xff },
  {
    mime: "image/webp",
    test: (b) =>
      b.length > 12 &&
      String.fromCharCode(b[0], b[1], b[2], b[3]) === "RIFF" &&
      String.fromCharCode(b[8], b[9], b[10], b[11]) === "WEBP",
  },
];

export function registerFileRoutes(): void {
  // -------------------------------------------------------------------------
  // Files
  // -------------------------------------------------------------------------

  registerRoute({
    method: "POST",
    path: "/v1/files",
    auth: "member",
    async handler({ env, ctx, url, body, auth }) {
      const a = auth as AuthContext;
      const req = await body(CreateFileRequest);
      await requireGroupMember(env, req.groupId, a);
      const row = await reserveFile(env, a, req);
      ctx.waitUntil(deleteStalePending(env).catch(() => {}));
      return { file: toFileMeta(row), uploadUrl: `${url.origin}/v1/files/${row.id}/content` };
    },
  });

  registerRoute({
    method: "PUT",
    path: "/v1/files/:id/content",
    auth: "member",
    async handler({ env, request, params, auth }) {
      const a = auth as AuthContext;
      const row = await loadFile(env, params.id, a);
      if (row.created_by !== a.memberId) throw new ApiError("forbidden", "only the creator may upload");
      if (row.status === "ready") throw new ApiError("conflict", "file already uploaded");
      await completeFile(env, a, row, request);
      return toFileMeta({ ...row, status: "ready" });
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/files/:id",
    auth: "member",
    async handler({ env, params, auth }) {
      return toFileMeta(await loadFile(env, params.id, auth as AuthContext));
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/files/:id/content",
    auth: "member",
    async handler({ env, request, params, auth }) {
      const row = await loadFile(env, params.id, auth as AuthContext);
      if (row.status !== "ready") throw new ApiError("not_found", "file not uploaded yet");
      const inline = INLINE_MIMES.has(row.mime);
      return serveObject(request, env.FILES, fileKey(row.group_id, row.id), {
        size: row.size,
        mime: row.mime === "text/html" ? "text/plain; charset=utf-8" : row.mime,
        disposition: contentDisposition(inline ? "inline" : "attachment", row.name),
        headers: { "cache-control": "private, max-age=31536000, immutable" },
      });
    },
  });

  // -------------------------------------------------------------------------
  // 产出
  // -------------------------------------------------------------------------

  registerRoute({
    method: "POST",
    path: "/v1/artifacts",
    auth: "member",
    // The first data snapshot may be 256 KiB on its own.
    bodyLimit: 640 * 1024,
    async handler({ env, body, auth }) {
      const a = auth as AuthContext;
      const req = await body(CreateArtifactRequest);
      await requireGroupMember(env, req.groupId, a);
      return createArtifactAs(env, a, req);
    },
  });

  registerRoute({
    method: "POST",
    path: "/v1/artifacts/:id/versions",
    auth: "member",
    // The first data snapshot may be 256 KiB on its own.
    bodyLimit: 640 * 1024,
    async handler({ env, params, body, auth }) {
      const a = auth as AuthContext;
      const groupId = await artifactGroup(env, params.id, a);
      const req = await body(AddArtifactVersionRequest);
      return addArtifactVersionAs(env, a, groupId, params.id, req);
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/artifacts",
    auth: "member",
    async handler({ env, url, auth }) {
      const a = auth as AuthContext;
      const q = z
        .object({ groupId: z.string().min(1).max(64), topicId: z.string().min(1).max(64).optional() })
        .safeParse(Object.fromEntries(url.searchParams));
      if (!q.success) throw new ApiError("invalid", "groupId is required");
      await requireGroupMember(env, q.data.groupId, a);
      return unwrap(
        await roomStub(env, q.data.groupId).rpc(q.data.groupId, "artifacts.list", actorOf(a), {
          topicId: q.data.topicId,
        }),
      ) as Artifact[];
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/artifacts/:id",
    auth: "member",
    async handler({ env, params, auth }) {
      const a = auth as AuthContext;
      const groupId = await artifactGroup(env, params.id, a);
      return unwrap(
        await roomStub(env, groupId).rpc(groupId, "artifacts.get", actorOf(a), { artifactId: params.id }),
      ) as ArtifactResponse;
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/artifacts/:id/versions/:v/files/*path",
    auth: "member",
    async handler({ env, request, params, auth }) {
      const a = auth as AuthContext;
      const path = params.path;
      if (path.length > 255 || !isArtifactPath(path)) throw new ApiError("invalid", "unsafe path");
      if (!/^[1-9][0-9]{0,8}$/.test(params.v)) throw new ApiError("invalid", "bad version");
      const groupId = await artifactGroup(env, params.id, a);
      const { kind, file } = unwrap(
        await roomStub(env, groupId).rpc(groupId, "artifacts.file", actorOf(a), {
          artifactId: params.id,
          version: Number(params.v),
          path,
        }),
      ) as { kind: "html" | "file"; file: { path: string; fileId: string; size: number; mime: string } };
      const mime = artifactMime(file.mime, file.path);
      const name = file.path.split("/").pop() as string;
      if (kind === "html") {
        return serveObject(request, env.FILES, fileKey(groupId, file.fileId), {
          size: file.size,
          mime: withCharset(mime),
          disposition: contentDisposition("inline", name),
          headers: { "content-security-policy": ARTIFACT_CSP },
        });
      }
      const inline = INLINE_MIMES.has(mime);
      return serveObject(request, env.FILES, fileKey(groupId, file.fileId), {
        size: file.size,
        mime: mime === "text/html" ? "text/plain; charset=utf-8" : mime,
        disposition: contentDisposition(inline ? "inline" : "attachment", name),
      });
    },
  });

  // -------------------------------------------------------------------------
  // Avatars
  // -------------------------------------------------------------------------

  registerRoute({
    method: "PUT",
    path: "/v1/members/:id/avatar",
    auth: "member",
    async handler({ env, ctx, request, url, params, auth }) {
      const a = auth as AuthContext;
      if (params.id !== a.memberId && a.member.level !== "owner") {
        throw new ApiError("forbidden", "only the member or an owner can change an avatar");
      }
      const target = await getMember(env.DB, params.id);
      if (!target) throw new ApiError("not_found", "member not found");
      const bytes = await readBytes(request, MAX_AVATAR_BYTES);
      const type = IMAGE_TYPES.find((t) => t.test(bytes));
      if (!type) throw new ApiError("invalid", "avatar must be a PNG, JPEG or WebP image");
      await env.FILES.put(avatarKey(target.id), bytes, { httpMetadata: { contentType: type.mime } });
      const avatarUrl = `${url.origin}/v1/members/${target.id}/avatar?v=${Date.now()}`;
      await env.DB.batch([
        env.DB.prepare("UPDATE members SET avatar_url = ? WHERE id = ?").bind(avatarUrl, target.id),
        auditStatement(env.DB, a, {
          action: "member.avatar",
          target: target.id,
          details: { size: bytes.byteLength, mime: type.mime },
        }),
      ]);
      const updated = { ...target, avatar_url: avatarUrl };
      await notifyMembersChanged(env, (p) => ctx.waitUntil(p), [updated]);
      return toMember(updated);
    },
  });

  registerRoute({
    method: "GET",
    path: "/v1/members/:id/avatar",
    auth: "member",
    async handler({ env, params }) {
      const obj = await env.FILES.get(avatarKey(params.id));
      if (!obj) throw new ApiError("not_found", "no avatar");
      const mime = obj.httpMetadata?.contentType ?? "application/octet-stream";
      return new Response(obj.body, {
        headers: {
          "content-type": INLINE_MIMES.has(mime) ? mime : "application/octet-stream",
          "content-length": String(obj.size),
          "cache-control": "private, max-age=86400",
          etag: obj.httpEtag,
          ...SAFE_HEADERS,
        },
      });
    },
  });
}

async function artifactGroup(env: Env, id: string, auth: AuthContext): Promise<string> {
  const row = await env.DB.prepare("SELECT group_id FROM artifacts WHERE id = ?").bind(id).first<{ group_id: string }>();
  if (!row) throw new ApiError("not_found", "产出 not found");
  await requireGroupMember(env, row.group_id, auth);
  return row.group_id;
}

/** Who writes a file or a 产出: a member's token, or a runner member's run (machine routes). */
export type WriterContext = Pick<AuthContext, "memberId" | "tokenId"> & { member: Pick<AuthContext["member"], "level" | "kind"> };

/**
 * Checks the writer's limits and stores a `pending` file record (audited as `file.create`).
 * The caller checked group membership.
 */
export async function reserveFile(env: Env, a: WriterContext, req: CreateFileRequest): Promise<FileRow> {
  if ((req.width !== undefined || req.height !== undefined) && !req.mime.startsWith("image/")) {
    throw new ApiError("invalid", "width and height are for images only");
  }
  // Per member and hour: a leaked token or a loop cannot fill R2 without limit.
  if (!(await rateHit(env.DB, `files:n:${a.memberId}`, FILE_RATE.windowMs, FILE_RATE.files))) {
    throw new ApiError("rate_limited", "too many files this hour");
  }
  if (!(await rateHit(env.DB, `files:b:${a.memberId}`, FILE_RATE.windowMs, FILE_RATE.bytes, req.size))) {
    throw new ApiError("rate_limited", "too many bytes uploaded this hour");
  }
  const used = await env.DB.prepare("SELECT coalesce(sum(size), 0) AS n FROM files WHERE created_by = ? AND deleted_at IS NULL")
    .bind(a.memberId)
    .first<{ n: number }>();
  if ((used?.n ?? 0) + req.size > MEMBER_FILE_QUOTA) throw new ApiError("too_large", "file storage quota reached");
  const now = Date.now();
  const id = ulid(now);
  await env.DB.batch([
    env.DB.prepare(
      `INSERT INTO files (id, group_id, topic_id, name, size, mime, sha256, width, height, status, created_by, created_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?)`,
    ).bind(
      id,
      req.groupId,
      req.topicId ?? null,
      req.name,
      req.size,
      req.mime.toLowerCase(),
      req.sha256,
      req.width ?? null,
      req.height ?? null,
      a.memberId,
      now,
    ),
    auditStatement(env.DB, a, {
      action: "file.create",
      target: id,
      details: { groupId: req.groupId, size: req.size, mime: req.mime },
    }),
  ]);
  return (await env.DB.prepare("SELECT * FROM files WHERE id = ?").bind(id).first<FileRow>()) as FileRow;
}

/** Stores the bytes of a `pending` file (checked against its size and sha256) and makes it ready. */
export async function completeFile(env: Env, a: WriterContext, row: FileRow, request: Request): Promise<void> {
  if (row.size > MAX_FILE_BYTES) throw new ApiError("too_large", "file too large");
  checkDeclaredLength(request, row.size);
  await putVerified(env.FILES, fileKey(row.group_id, row.id), request.body, row.size, row.sha256, {
    contentType: row.mime,
  });
  const reg: RegisterFileArgs = {
    id: row.id,
    topicId: row.topic_id,
    name: row.name,
    size: row.size,
    mime: row.mime,
    sha256: row.sha256,
    width: row.width,
    height: row.height,
    createdBy: row.created_by,
    createdAt: row.created_at,
  };
  // The room learns about the file before D1 says ready, so a ready file is always usable.
  unwrap(await roomStub(env, row.group_id).rpc(row.group_id, "files.register", null, reg));
  const now = Date.now();
  await env.DB.batch([
    env.DB.prepare("UPDATE files SET status = 'ready', ready_at = ? WHERE id = ? AND status = 'pending'").bind(now, row.id),
    auditStatement(env.DB, a, { action: "file.upload", target: row.id, details: { groupId: row.group_id } }, now),
  ]);
}

/** Makes a 产出 and its version 1 as `a` (audited as `artifact.create`). The caller checked membership. */
export async function createArtifactAs(env: Env, a: WriterContext, req: CreateArtifactRequest): Promise<ArtifactVersionResponse> {
  const now = Date.now();
  const id = ulid(now);
  // Index row first (harmless if the room rejects; removed below), so the id resolves to its group.
  await env.DB.prepare("INSERT INTO artifacts (id, group_id, topic_id, created_by, created_at) VALUES (?, ?, ?, ?, ?)")
    .bind(id, req.groupId, req.topicId, a.memberId, now)
    .run();
  const r = await roomStub(env, req.groupId).rpc(req.groupId, "artifacts.create", actorOf(a as AuthContext), {
    id,
    topicId: req.topicId,
    title: req.title,
    kind: req.kind,
    entry: req.entry,
    files: req.files,
    note: req.note,
    events: req.events,
    runId: req.runId,
    media: req.media,
    data: req.data,
  });
  if (!r.ok) {
    await env.DB.prepare("DELETE FROM artifacts WHERE id = ?").bind(id).run();
    throw new ApiError(r.error.code, r.error.message, r.error.reason);
  }
  await auditStatement(env.DB, a, {
    action: "artifact.create",
    target: id,
    details: { groupId: req.groupId, topicId: req.topicId, kind: req.kind },
  }).run();
  return r.value as ArtifactVersionResponse;
}

/** Adds a version to a 产出 of `groupId` as `a` (audited as `artifact.version.add`). */
export async function addArtifactVersionAs(
  env: Env,
  a: WriterContext,
  groupId: string,
  artifactId: string,
  req: AddArtifactVersionRequest,
): Promise<ArtifactVersionResponse> {
  const out = unwrap(
    await roomStub(env, groupId).rpc(groupId, "artifacts.addVersion", actorOf(a as AuthContext), {
      artifactId,
      entry: req.entry,
      files: req.files,
      note: req.note,
      title: req.title,
      events: req.events,
      runId: req.runId,
      media: req.media,
      data: req.data,
    }),
  ) as ArtifactVersionResponse;
  await auditStatement(env.DB, a, {
    action: "artifact.version.add",
    target: artifactId,
    details: { groupId, version: out.version.version },
  }).run();
  return out;
}

/** The group of a 产出 (D1 index), or null. */
export async function artifactGroupOf(env: Env, id: string): Promise<string | null> {
  const row = await env.DB.prepare("SELECT group_id FROM artifacts WHERE id = ?").bind(id).first<{ group_id: string }>();
  return row?.group_id ?? null;
}
