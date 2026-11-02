-- Files, 产出 index, diagnostics, releases and a small fixed-window rate counter (server/src/files).
-- Bytes live in R2 (binding FILES): files/<groupId>/<fileId>, avatars/<memberId>,
-- diagnostics/<memberId>/<id>.gz, releases/<version>/<file>.
CREATE TABLE files (
  id TEXT PRIMARY KEY,
  group_id TEXT NOT NULL,
  topic_id TEXT,
  name TEXT NOT NULL,
  size INTEGER NOT NULL,
  mime TEXT NOT NULL,
  sha256 TEXT NOT NULL,
  width INTEGER,
  height INTEGER,
  status TEXT NOT NULL CHECK (status IN ('pending', 'ready')),
  created_by TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  ready_at INTEGER
);
CREATE INDEX files_group ON files(group_id, created_at);
CREATE INDEX files_pending ON files(status, created_at);

-- Maps a 产出 id to its group. The group room holds the 产出 records and versions.
CREATE TABLE artifacts (
  id TEXT PRIMARY KEY,
  group_id TEXT NOT NULL,
  topic_id TEXT NOT NULL,
  created_by TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE INDEX artifacts_group ON artifacts(group_id);

CREATE TABLE diagnostics (
  id TEXT PRIMARY KEY,
  member_id TEXT NOT NULL,
  size INTEGER NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE INDEX diagnostics_member ON diagnostics(member_id, created_at);
CREATE INDEX diagnostics_at ON diagnostics(created_at);

-- status 'uploading' while the DMG streams into R2; only 'ready' rows are served.
CREATE TABLE releases (
  version TEXT PRIMARY KEY,
  major INTEGER NOT NULL,
  minor INTEGER NOT NULL,
  patch INTEGER NOT NULL,
  file_name TEXT NOT NULL,
  size INTEGER NOT NULL,
  sha256 TEXT NOT NULL,
  notes TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL CHECK (status IN ('uploading', 'ready')),
  created_at INTEGER NOT NULL,
  published_at INTEGER
);
CREATE INDEX releases_order ON releases(status, major, minor, patch);

CREATE TABLE files_rate_hits (
  key TEXT NOT NULL,
  window INTEGER NOT NULL, -- window start, ms
  count INTEGER NOT NULL,
  PRIMARY KEY (key, window)
);
