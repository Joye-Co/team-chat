// Group room side of files and 产出. The room keeps its own copy of every ready file of the group
// and every 产出 version, so message sends and edits check attachments and 产出 references
// synchronously, inside the room that owns the group: nobody can reference another group's files.
import type { Artifact, ArtifactFile, ArtifactKind, ArtifactVersion, FileRef } from "@chat/protocol";
import { ApiError } from "../core/errors";
import {
  type Actor,
  type RoomContext,
  registerEditExtension,
  registerMessageExtension,
  registerRoomMigration,
  registerRoomRpc,
} from "../core/registry";

registerRoomMigration({
  name: "files-1",
  up: `
    CREATE TABLE file_records (
      id TEXT PRIMARY KEY,
      topic_id TEXT,
      name TEXT NOT NULL,
      size INTEGER NOT NULL,
      mime TEXT NOT NULL,
      sha256 TEXT NOT NULL,
      width INTEGER,
      height INTEGER,
      created_by TEXT NOT NULL,
      created_at INTEGER NOT NULL
    );
    CREATE TABLE artifacts (
      id TEXT PRIMARY KEY,
      topic_id TEXT NOT NULL,
      title TEXT NOT NULL,
      kind TEXT NOT NULL,
      current_version INTEGER NOT NULL,
      created_by TEXT NOT NULL,
      created_at INTEGER NOT NULL,
      updated_at INTEGER NOT NULL
    );
    CREATE INDEX artifacts_topic ON artifacts(topic_id, updated_at);
    CREATE INDEX artifacts_updated ON artifacts(updated_at);
    CREATE TABLE artifact_versions (
      artifact_id TEXT NOT NULL,
      version INTEGER NOT NULL,
      entry TEXT NOT NULL,
      files TEXT NOT NULL,
      note TEXT,
      created_by TEXT NOT NULL,
      created_at INTEGER NOT NULL,
      PRIMARY KEY (artifact_id, version)
    );
  `,
});

type FileRecordRow = {
  id: string;
  topic_id: string | null;
  name: string;
  size: number;
  mime: string;
  sha256: string;
  width: number | null;
  height: number | null;
  created_by: string;
  created_at: number;
};

type ArtifactRow = {
  id: string;
  topic_id: string;
  title: string;
  kind: ArtifactKind;
  current_version: number;
  created_by: string;
  created_at: number;
  updated_at: number;
};

type VersionRow = {
  artifact_id: string;
  version: number;
  entry: string;
  files: string;
  note: string | null;
  created_by: string;
  created_at: number;
};

export interface RegisterFileArgs {
  id: string;
  topicId: string | null;
  name: string;
  size: number;
  mime: string;
  sha256: string;
  width: number | null;
  height: number | null;
  createdBy: string;
  createdAt: number;
}

export interface VersionInput {
  entry: string;
  files: { path: string; fileId: string }[];
  note?: string;
}

function fileRecord(room: RoomContext, id: string): FileRecordRow | undefined {
  return room.sql.exec<FileRecordRow>("SELECT * FROM file_records WHERE id = ?", id).toArray()[0] as
    | FileRecordRow
    | undefined;
}

function toFileRef(r: FileRecordRow): FileRef {
  const ref: FileRef = { fileId: r.id, name: r.name, size: r.size, mime: r.mime, sha256: r.sha256 };
  if (r.width !== null) ref.width = r.width;
  if (r.height !== null) ref.height = r.height;
  return ref;
}

function artifactRow(room: RoomContext, id: string): ArtifactRow | undefined {
  return room.sql.exec<ArtifactRow>("SELECT * FROM artifacts WHERE id = ?", id).toArray()[0] as
    | ArtifactRow
    | undefined;
}

function toArtifact(room: RoomContext, r: ArtifactRow): Artifact {
  return {
    id: r.id,
    groupId: room.groupId,
    topicId: r.topic_id,
    title: r.title,
    kind: r.kind,
    currentVersion: r.current_version,
    createdBy: r.created_by,
    createdAt: r.created_at,
    updatedAt: r.updated_at,
  };
}

function toVersion(r: VersionRow): ArtifactVersion {
  const v: ArtifactVersion = {
    artifactId: r.artifact_id,
    version: r.version,
    entry: r.entry,
    files: JSON.parse(r.files) as ArtifactFile[],
    createdBy: r.created_by,
    createdAt: r.created_at,
  };
  if (r.note !== null) v.note = r.note;
  return v;
}

function versionRow(room: RoomContext, artifactId: string, version: number): VersionRow | undefined {
  return room.sql
    .exec<VersionRow>("SELECT * FROM artifact_versions WHERE artifact_id = ? AND version = ?", artifactId, version)
    .toArray()[0] as VersionRow | undefined;
}

/** Checks a version's files against the room's ready files and returns them with size and mime. */
function resolveVersionFiles(room: RoomContext, kind: ArtifactKind, input: VersionInput): ArtifactFile[] {
  const paths = new Set<string>();
  const files: ArtifactFile[] = [];
  for (const f of input.files) {
    if (paths.has(f.path)) throw new ApiError("invalid", "duplicate path in 产出 version");
    paths.add(f.path);
    const rec = fileRecord(room, f.fileId);
    if (!rec) throw new ApiError("invalid", "file is not a ready file of this group");
    files.push({ path: f.path, fileId: rec.id, size: rec.size, mime: rec.mime });
  }
  const entry = files.find((f) => f.path === input.entry);
  if (!entry) throw new ApiError("invalid", "entry must be one of the paths");
  if (kind === "file" && files.length !== 1) throw new ApiError("invalid", "a file 产出 has exactly one file");
  if (kind === "html" && entry.mime !== "text/html") throw new ApiError("invalid", "the entry of an html 产出 must be text/html");
  return files;
}

function insertVersion(
  room: RoomContext,
  artifactId: string,
  version: number,
  input: VersionInput,
  files: ArtifactFile[],
  actorId: string,
  now: number,
): void {
  room.sql.exec(
    `INSERT INTO artifact_versions (artifact_id, version, entry, files, note, created_by, created_at)
     VALUES (?, ?, ?, ?, ?, ?, ?)`,
    artifactId,
    version,
    input.entry,
    JSON.stringify(files),
    input.note ?? null,
    actorId,
    now,
  );
}

function requireActor(actor: Actor | null): Actor {
  if (!actor) throw new ApiError("internal", "actor required");
  return actor;
}

// ---------------------------------------------------------------------------
// RPCs (called by the files routes in the API Worker)
// ---------------------------------------------------------------------------

/** A file became ready: keep its record for attachment and 产出 checks. Idempotent. */
registerRoomRpc("files.register", (room, _actor, args) => {
  const f = args as RegisterFileArgs;
  room.sql.exec(
    `INSERT OR REPLACE INTO file_records (id, topic_id, name, size, mime, sha256, width, height, created_by, created_at)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    f.id,
    f.topicId,
    f.name,
    f.size,
    f.mime,
    f.sha256,
    f.width,
    f.height,
    f.createdBy,
    f.createdAt,
  );
  return { ok: true };
});

registerRoomRpc("artifacts.create", (room, actor, args) => {
  const a = requireActor(actor);
  const req = args as VersionInput & { id: string; topicId: string; title: string; kind: ArtifactKind };
  const topic = room.sql.exec("SELECT id FROM topics WHERE id = ?", req.topicId).toArray()[0];
  if (!topic) throw new ApiError("not_found", "topic not found");
  if (artifactRow(room, req.id)) throw new ApiError("conflict", "产出 exists");
  const files = resolveVersionFiles(room, req.kind, req);
  const now = Date.now();
  return room.commit((tx) => {
    room.sql.exec(
      `INSERT INTO artifacts (id, topic_id, title, kind, current_version, created_by, created_at, updated_at)
       VALUES (?, ?, ?, ?, 1, ?, ?, ?)`,
      req.id,
      req.topicId,
      req.title,
      req.kind,
      a.id,
      now,
      now,
    );
    insertVersion(room, req.id, 1, req, files, a.id, now);
    const artifact = toArtifact(room, artifactRow(room, req.id) as ArtifactRow);
    const version = toVersion(versionRow(room, req.id, 1) as VersionRow);
    const seq = tx.nextSeq();
    tx.append({ type: "artifact.created", actorId: a.id, topicId: req.topicId, data: { artifact, version } });
    room.sql.exec("UPDATE topics SET last_seq = ? WHERE id = ?", seq, req.topicId);
    return { artifact, version };
  });
});

registerRoomRpc("artifacts.addVersion", (room, actor, args) => {
  const a = requireActor(actor);
  const req = args as VersionInput & { artifactId: string; title?: string };
  const row = artifactRow(room, req.artifactId);
  if (!row) throw new ApiError("not_found", "产出 not found");
  const files = resolveVersionFiles(room, row.kind, req);
  const now = Date.now();
  const next = row.current_version + 1;
  return room.commit((tx) => {
    insertVersion(room, row.id, next, req, files, a.id, now);
    room.sql.exec(
      "UPDATE artifacts SET current_version = ?, updated_at = ?, title = ? WHERE id = ?",
      next,
      now,
      req.title ?? row.title,
      row.id,
    );
    const artifact = toArtifact(room, artifactRow(room, row.id) as ArtifactRow);
    const version = toVersion(versionRow(room, row.id, next) as VersionRow);
    const seq = tx.nextSeq();
    tx.append({ type: "artifact.versionAdded", actorId: a.id, topicId: row.topic_id, data: { artifact, version } });
    room.sql.exec("UPDATE topics SET last_seq = ? WHERE id = ?", seq, row.topic_id);
    return { artifact, version };
  });
});

registerRoomRpc("artifacts.list", (room, _actor, args) => {
  const { topicId } = args as { topicId?: string };
  const rows = topicId
    ? room.sql.exec<ArtifactRow>(
        "SELECT * FROM artifacts WHERE topic_id = ? ORDER BY updated_at DESC, id DESC",
        topicId,
      )
    : room.sql.exec<ArtifactRow>("SELECT * FROM artifacts ORDER BY updated_at DESC, id DESC");
  return rows.toArray().map((r) => toArtifact(room, r as ArtifactRow));
});

registerRoomRpc("artifacts.get", (room, _actor, args) => {
  const { artifactId } = args as { artifactId: string };
  const row = artifactRow(room, artifactId);
  if (!row) throw new ApiError("not_found", "产出 not found");
  const versions = room.sql
    .exec<VersionRow>("SELECT * FROM artifact_versions WHERE artifact_id = ? ORDER BY version DESC", artifactId)
    .toArray()
    .map((r) => toVersion(r as VersionRow));
  return { artifact: toArtifact(room, row), versions };
});

/** Resolves one file of a version by path. */
registerRoomRpc("artifacts.file", (room, _actor, args) => {
  const { artifactId, version, path } = args as { artifactId: string; version: number; path: string };
  const row = artifactRow(room, artifactId);
  if (!row) throw new ApiError("not_found", "产出 not found");
  const v = versionRow(room, artifactId, version);
  if (!v) throw new ApiError("not_found", "version not found");
  const file = (JSON.parse(v.files) as ArtifactFile[]).find((f) => f.path === path);
  if (!file) throw new ApiError("not_found", "no such file in this version");
  return { kind: row.kind, file };
});

// ---------------------------------------------------------------------------
// Message send and edit: attachments and 产出 references
// ---------------------------------------------------------------------------

function resolveAttachments(room: RoomContext, raw: unknown): FileRef[] {
  if (!Array.isArray(raw)) throw new ApiError("invalid", "attachments must be a list");
  const seen = new Set<string>();
  const out: FileRef[] = [];
  for (const item of raw as { fileId?: unknown }[]) {
    const id = typeof item?.fileId === "string" ? item.fileId : "";
    const rec = id ? fileRecord(room, id) : undefined;
    if (!rec) throw new ApiError("invalid", "attachment is not a ready file of this group");
    if (seen.has(rec.id)) continue;
    seen.add(rec.id);
    out.push(toFileRef(rec));
  }
  return out;
}

function resolveArtifactRefs(room: RoomContext, raw: unknown): { artifactId: string; version: number }[] {
  if (!Array.isArray(raw)) throw new ApiError("invalid", "artifactRefs must be a list");
  const out: { artifactId: string; version: number }[] = [];
  for (const item of raw as { artifactId?: unknown; version?: unknown }[]) {
    const id = typeof item?.artifactId === "string" ? item.artifactId : "";
    const version = typeof item?.version === "number" ? item.version : 0;
    if (!id || !versionRow(room, id, version)) {
      throw new ApiError("invalid", "artifactRef does not name a 产出 version of this group");
    }
    if (!out.some((r) => r.artifactId === id && r.version === version)) out.push({ artifactId: id, version });
  }
  return out;
}

function refsFromFrame(frame: Record<string, unknown>, room: RoomContext, keepEmpty: boolean) {
  let out: Record<string, unknown> | undefined;
  if (frame.attachments !== undefined) {
    const attachments = resolveAttachments(room, frame.attachments);
    if (attachments.length > 0 || keepEmpty) out = { ...out, attachments };
  }
  if (frame.artifactRefs !== undefined) {
    const artifactRefs = resolveArtifactRefs(room, frame.artifactRefs);
    if (artifactRefs.length > 0 || keepEmpty) out = { ...out, artifactRefs };
  }
  return out;
}

registerMessageExtension((frame, room) => refsFromFrame(frame, room, false));
registerEditExtension((frame, room) => refsFromFrame(frame, room, true));
