// Group room side of files and 产出. The room keeps its own copy of every ready file of the group
// and every 产出 version, so message sends and edits check attachments and 产出 references
// synchronously, inside the room that owns the group: nobody can reference another group's files.
import {
  type Artifact,
  type ArtifactEventDecl,
  type ArtifactFile,
  type ArtifactKind,
  type ArtifactVersion,
  type FileRef,
  MEDIA_LIMITS,
  MediaInfo,
  mediaFileProblem,
  mediaProblem,
} from "@chat/protocol";
import { ApiError } from "../core/errors";
import {
  type Actor,
  type ArchivedEvent,
  type RoomContext,
  registerAfterCommit,
  registerEditExtension,
  registerEventImporter,
  registerEventWriter,
  registerMessageExtension,
  registerRoomMigration,
  registerRoomRpc,
} from "../core/registry";
import { schemaPatternProblem } from "./pattern";

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

// 1.1: the run that made a version and the events its page may send (docs/protocol.md, "产出 events").
registerRoomMigration({
  name: "files-3-events",
  up: `
    ALTER TABLE artifact_versions ADD COLUMN run_id TEXT;
    ALTER TABLE artifact_versions ADD COLUMN events TEXT;
  `,
});

// 1.1: media metadata of a version (V11-MEDIA §2), its latest data snapshot (one per version,
// INTERACT §5.4) and the page events of the last 24 h (idempotency, reply routing, causes).
registerRoomMigration({
  name: "files-4-bridge",
  up: `
    ALTER TABLE artifact_versions ADD COLUMN media TEXT;
    ALTER TABLE artifact_versions ADD COLUMN data_rev INTEGER NOT NULL DEFAULT 0;
    CREATE TABLE artifact_data (
      artifact_id TEXT NOT NULL,
      version INTEGER NOT NULL,
      data TEXT NOT NULL,
      data_rev INTEGER NOT NULL,
      push_id TEXT,
      updated_at INTEGER NOT NULL,
      PRIMARY KEY (artifact_id, version)
    );
    CREATE TABLE page_events (
      event_id TEXT PRIMARY KEY,
      artifact_id TEXT NOT NULL,
      version INTEGER NOT NULL,
      actor_id TEXT NOT NULL,
      bot_id TEXT NOT NULL,
      kind TEXT NOT NULL,
      client_event_id TEXT,
      ack TEXT NOT NULL,
      at INTEGER NOT NULL,
      reply_hash TEXT,
      replied_at INTEGER
    );
    CREATE UNIQUE INDEX page_events_client ON page_events(actor_id, artifact_id, version, client_event_id);
    CREATE INDEX page_events_at ON page_events(at);
  `,
});

// 1.1: a page event's name (its `Cause.event`) and how it was resolved, once (INTERACT §4.3).
registerRoomMigration({
  name: "files-5-page-resolve",
  up: `
    ALTER TABLE page_events ADD COLUMN name TEXT;
    ALTER TABLE page_events ADD COLUMN resolved TEXT;
  `,
});

// Files that only deleted messages used, waiting for D1 and R2 to forget them (see below).
registerRoomMigration({
  name: "files-2",
  up: `CREATE TABLE file_deletions (file_id TEXT PRIMARY KEY, at INTEGER NOT NULL);`,
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

export type ArtifactRow = {
  id: string;
  topic_id: string;
  title: string;
  kind: ArtifactKind;
  current_version: number;
  created_by: string;
  created_at: number;
  updated_at: number;
};

export type VersionRow = {
  artifact_id: string;
  version: number;
  entry: string;
  files: string;
  note: string | null;
  created_by: string;
  created_at: number;
  run_id?: string | null;
  events?: string | null;
  media?: string | null;
  data_rev?: number;
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
  /** Bot members only; dropped for others. */
  events?: ArtifactEventDecl[];
  /** A run of the calling bot in the 产出's topic; checked against D1 `bot_runs`. */
  runId?: string;
  /** 1.1: media metadata (checked against the limits and the group's files). */
  media?: MediaInfo;
  /** 1.1: the first data snapshot. */
  data?: unknown;
}

export function fileRecord(room: RoomContext, id: string): FileRecordRow | undefined {
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

export function artifactRow(room: RoomContext, id: string): ArtifactRow | undefined {
  return room.sql.exec<ArtifactRow>("SELECT * FROM artifacts WHERE id = ?", id).toArray()[0] as
    | ArtifactRow
    | undefined;
}

export function toArtifact(room: RoomContext, r: ArtifactRow): Artifact {
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

export function toVersion(r: VersionRow): ArtifactVersion {
  const v: ArtifactVersion = {
    artifactId: r.artifact_id,
    version: r.version,
    entry: r.entry,
    files: JSON.parse(r.files) as ArtifactFile[],
    createdBy: r.created_by,
    createdAt: r.created_at,
  };
  if (r.note !== null) v.note = r.note;
  if (r.run_id) v.runId = r.run_id;
  if (r.events) v.events = JSON.parse(r.events) as ArtifactEventDecl[];
  if (r.media) v.media = JSON.parse(r.media) as MediaInfo;
  if (r.data_rev) v.dataRev = r.data_rev;
  return v;
}

export function versionRow(room: RoomContext, artifactId: string, version: number): VersionRow | undefined {
  return room.sql
    .exec<VersionRow>("SELECT * FROM artifact_versions WHERE artifact_id = ? AND version = ?", artifactId, version)
    .toArray()[0] as VersionRow | undefined;
}

/** 产出 file paths the client reserves for the files it serves itself. */
export const RESERVED_PATH_PREFIX = "__teamchat/";

/** Checks a version's files against the room's ready files and returns them with size and mime. */
function resolveVersionFiles(room: RoomContext, kind: ArtifactKind, input: VersionInput): ArtifactFile[] {
  const paths = new Set<string>();
  const files: ArtifactFile[] = [];
  for (const f of input.files) {
    if (paths.has(f.path)) throw new ApiError("invalid", "duplicate path in 产出 version");
    // REFS §B: the client serves its own files (the diagram library) under this prefix.
    if (f.path.startsWith(RESERVED_PATH_PREFIX)) throw new ApiError("invalid", `paths under ${RESERVED_PATH_PREFIX} are reserved`);
    paths.add(f.path);
    const rec = fileRecord(room, f.fileId);
    if (!rec) throw new ApiError("invalid", "file is not a ready file of this group");
    files.push({ path: f.path, fileId: rec.id, size: rec.size, mime: rec.mime });
  }
  const entry = files.find((f) => f.path === input.entry);
  if (!entry) throw new ApiError("invalid", "entry must be one of the paths");
  if (kind === "file" && files.length !== 1) throw new ApiError("invalid", "a file 产出 has exactly one file");
  if (kind === "html" && entry.mime !== "text/html") throw new ApiError("invalid", "the entry of an html 产出 must be text/html");
  // V11-MEDIA §4: an HTML version is one bundle of at most 20 MB.
  if (kind === "html" && files.reduce((n, f) => n + f.size, 0) > MEDIA_LIMITS.htmlBytes) {
    throw new ApiError("too_large", "an html 产出 version exceeds 20 MB");
  }
  if (input.media) checkMedia(room, input.media);
  return files;
}

/** The file ids a media record points at (poster, frames, peaks, transcript, image set). */
function mediaFileIds(m: MediaInfo): string[] {
  return [
    m.posterFileId,
    m.peaksFileId,
    m.transcriptFileId,
    ...(m.frames ?? []).map((f) => f.fileId),
    ...(m.images ?? []).flatMap((i) => [i.fileId, i.thumbFileId]),
  ].filter((x): x is string => typeof x === "string");
}

/**
 * Checks media metadata (V11-MEDIA §4): the size limits, and that every file it names is a
 * ready file of this group. The server never decodes media.
 */
export function checkMedia(room: RoomContext, raw: unknown): MediaInfo {
  const parsed = MediaInfo.safeParse(raw);
  if (!parsed.success) throw new ApiError("invalid", `media: ${parsed.error.issues[0]?.message ?? "invalid"}`);
  const problem = mediaProblem(parsed.data);
  if (problem) throw new ApiError("too_large", `media: ${problem}`);
  for (const id of mediaFileIds(parsed.data)) {
    if (!fileRecord(room, id)) throw new ApiError("invalid", "media names a file that is not a ready file of this group");
  }
  const fileProblem = mediaFileProblem(parsed.data, (id) => fileRecord(room, id));
  if (fileProblem) throw new ApiError("too_large", `media: ${fileProblem}`);
  return parsed.data;
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
    `INSERT INTO artifact_versions (artifact_id, version, entry, files, note, created_by, created_at, run_id, events, media, data_rev)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    artifactId,
    version,
    input.entry,
    JSON.stringify(files),
    input.note ?? null,
    actorId,
    now,
    input.runId ?? null,
    input.events && input.events.length > 0 ? JSON.stringify(input.events) : null,
    input.media ? JSON.stringify(input.media) : null,
    input.data !== undefined ? 1 : 0,
  );
  // A new version starts with the data of its request, or none (INTERACT §5.3).
  if (input.data !== undefined) {
    room.sql.exec(
      "INSERT OR REPLACE INTO artifact_data (artifact_id, version, data, data_rev, push_id, updated_at) VALUES (?, ?, ?, 1, NULL, ?)",
      artifactId,
      version,
      JSON.stringify(input.data),
      now,
    );
  }
}

/**
 * Keeps `events`, `data` and `runId` only from a bot (a person's page gets no bridge), and
 * `runId` only for its own run in the topic.
 */
async function botVersionFields(room: RoomContext, actor: Actor, topicId: string, input: VersionInput): Promise<VersionInput> {
  if (actor.kind !== "bot") return { ...input, events: undefined, runId: undefined, data: undefined };
  for (const e of input.events ?? []) {
    const problem = schemaPatternProblem(e.schema);
    if (problem) throw new ApiError("invalid", `event ${e.name}: ${problem}`, "artifact.bad_schema");
  }
  if (!input.runId) return input;
  const run = await room.env.DB.prepare("SELECT 1 AS x FROM bot_runs WHERE id = ? AND bot_id = ? AND topic_id = ?")
    .bind(input.runId, actor.id, topicId)
    .first();
  return run ? input : { ...input, runId: undefined };
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

registerRoomRpc("artifacts.create", async (room, actor, args) => {
  const a = requireActor(actor);
  const given = args as VersionInput & { id: string; topicId: string; title: string; kind: ArtifactKind };
  const req = { ...given, ...(await botVersionFields(room, a, given.topicId, given)) };
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

registerRoomRpc("artifacts.addVersion", async (room, actor, args) => {
  const a = requireActor(actor);
  const given = args as VersionInput & { artifactId: string; title?: string };
  const first = artifactRow(room, given.artifactId);
  if (!first) throw new ApiError("not_found", "产出 not found");
  const req = { ...given, ...(await botVersionFields(room, a, first.topic_id, given)) };
  // Read the row again after the D1 await: another version can commit while it waits.
  const row = artifactRow(room, given.artifactId);
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
    const ref = toFileRef(rec);
    // 1.1: the sender's media metadata, checked against the limits and the group's files.
    const media = (item as { media?: unknown }).media;
    if (media !== undefined) ref.media = checkMedia(room, media);
    // 1.1: 把这一帧发到话题: the frame's 产出 version is one of this group.
    const source = (item as { source?: { artifactId: string; version: number; t: number } }).source;
    if (source !== undefined) {
      if (!versionRow(room, source.artifactId, source.version)) throw new ApiError("invalid", "attachment source is not a 产出 version of this group");
      ref.source = { artifactId: source.artifactId, version: source.version, t: source.t };
    }
    out.push(ref);
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

// ---------------------------------------------------------------------------
// Restore from the archive
// ---------------------------------------------------------------------------

/**
 * Rebuilds artifacts and artifact_versions from artifact.created and artifact.versionAdded:
 * both events carry the whole 产出 and the new version, as the RPCs above write them.
 */
registerEventImporter((e, sql) => {
  if (e.type !== "artifact.created" && e.type !== "artifact.versionAdded") return;
  const data = (e.data ?? {}) as { artifact?: Artifact; version?: ArtifactVersion };
  const a = data.artifact;
  const v = data.version;
  if (!a || !v) return;
  sql.exec(
    `INSERT INTO artifacts (id, topic_id, title, kind, current_version, created_by, created_at, updated_at)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?)
     ON CONFLICT (id) DO UPDATE SET title = excluded.title, current_version = excluded.current_version,
       updated_at = excluded.updated_at`,
    a.id,
    a.topicId,
    a.title,
    a.kind,
    a.currentVersion,
    a.createdBy,
    a.createdAt,
    a.updatedAt,
  );
  sql.exec(
    `INSERT OR REPLACE INTO artifact_versions (artifact_id, version, entry, files, note, created_by, created_at, run_id, events, media)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    v.artifactId,
    v.version,
    v.entry,
    JSON.stringify(v.files),
    v.note ?? null,
    v.createdBy,
    v.createdAt,
    v.runId ?? null,
    v.events && v.events.length > 0 ? JSON.stringify(v.events) : null,
    v.media ? JSON.stringify(v.media) : null,
  );
});

// Restore: a later preview replaces the media of the version (data snapshots are not in the log).
registerEventImporter((e, sql) => {
  if (e.type !== "artifact.previewReady") return;
  const d = (e.data ?? {}) as { artifactId?: string; version?: number; media?: unknown };
  if (!d.artifactId || !d.version || !d.media) return;
  sql.exec("UPDATE artifact_versions SET media = ? WHERE artifact_id = ? AND version = ?", JSON.stringify(d.media), d.artifactId, d.version);
});

/**
 * Restore: refills file_records from the D1 files rows of this group (status ready). Ready
 * files have no event, so the restore tool calls this after the events. Internal only.
 */
registerRoomRpc("files.importRecords", (room, actor, args) => {
  if (actor) throw new ApiError("forbidden", "internal only");
  const { files } = args as { files: RegisterFileArgs[] };
  for (const f of files) {
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
  }
  return { rows: files.length };
});

// ---------------------------------------------------------------------------
// Message delete: attachments and 产出 references go too
// ---------------------------------------------------------------------------

function attachmentIds(ext: string | null): string[] {
  if (!ext) return [];
  const list = (JSON.parse(ext) as { attachments?: { fileId?: unknown }[] }).attachments;
  return Array.isArray(list) ? list.map((a) => a?.fileId).filter((x): x is string => typeof x === "string") : [];
}

/** True when a message other than `messageId` that is not deleted, or a 产出 version, uses the file. */
function fileStillUsed(sql: SqlStorage, fileId: string, messageId: string): boolean {
  const inMessage = sql
    .exec(
      `SELECT 1 FROM messages m, json_each(m.ext, '$.attachments') j
       WHERE m.ext IS NOT NULL AND m.deleted_at IS NULL AND m.id != ? AND json_extract(j.value, '$.fileId') = ? LIMIT 1`,
      messageId,
      fileId,
    )
    .toArray().length > 0;
  if (inMessage) return true;
  return (
    sql
      .exec("SELECT 1 FROM artifact_versions v, json_each(v.files) j WHERE json_extract(j.value, '$.fileId') = ? LIMIT 1", fileId)
      .toArray().length > 0
  );
}

/**
 * Removes the attachments and 产出 references of a deleted message from its row and stored
 * events, and marks files that no other message or 产出 uses for deletion. Live and imports.
 */
function forgetDeletedRefs(event: ArchivedEvent, sql: SqlStorage): void {
  if (event.type !== "message.deleted") return;
  const messageId = (event.data as { messageId?: string }).messageId ?? event.messageId;
  if (!messageId) return;
  const row = sql.exec<{ ext: string | null }>("SELECT ext FROM messages WHERE id = ?", messageId).toArray()[0];
  if (!row) return;
  const fileIds = attachmentIds(row.ext);
  sql.exec("UPDATE messages SET ext = json_remove(ext, '$.attachments', '$.artifactRefs') WHERE id = ?", messageId);
  sql.exec(
    `UPDATE events SET data = json_remove(data, '$.message.attachments', '$.message.artifactRefs')
     WHERE message_id = ? AND type = 'message.created'`,
    messageId,
  );
  sql.exec(
    "UPDATE events SET data = json_remove(data, '$.attachments', '$.artifactRefs') WHERE message_id = ? AND type = 'message.updated'",
    messageId,
  );
  for (const id of new Set(fileIds)) {
    if (fileStillUsed(sql, id, messageId)) continue;
    sql.exec("DELETE FROM file_records WHERE id = ?", id);
    sql.exec("INSERT OR IGNORE INTO file_deletions (file_id, at) VALUES (?, ?)", id, event.at);
  }
}
registerEventWriter(forgetDeletedRefs);

// After a delete commits: D1 stops serving the files, then R2 drops their bytes.
registerAfterCommit(async (event, h) => {
  if (event.type !== "message.deleted") return;
  const rows = h.sql.exec<{ file_id: string }>("SELECT file_id FROM file_deletions LIMIT 100").toArray();
  if (rows.length === 0) return;
  const now = Date.now();
  await h.env.DB.batch(
    rows.map((r) =>
      h.env.DB.prepare("UPDATE files SET deleted_at = ? WHERE id = ? AND group_id = ? AND deleted_at IS NULL").bind(
        now,
        r.file_id,
        h.groupId,
      ),
    ),
  );
  await h.env.FILES.delete(rows.map((r) => `files/${h.groupId}/${r.file_id}`));
  for (const r of rows) h.sql.exec("DELETE FROM file_deletions WHERE file_id = ?", r.file_id);
});
