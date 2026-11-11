-- Release files for platforms other than the DMG (server/src/files/ops.ts). The DMG of a
-- release stays a row of `releases`; the Windows installer is a row here with platform
-- 'windows'. Bytes live in R2 next to the DMG: releases/<version>/<file>.
CREATE TABLE release_assets (
  version TEXT NOT NULL,
  platform TEXT NOT NULL,
  major INTEGER NOT NULL,
  minor INTEGER NOT NULL,
  patch INTEGER NOT NULL,
  file_name TEXT NOT NULL,
  size INTEGER NOT NULL,
  sha256 TEXT NOT NULL,
  notes TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL CHECK (status IN ('uploading', 'ready')),
  created_at INTEGER NOT NULL,
  published_at INTEGER,
  withdrawn_at INTEGER,
  PRIMARY KEY (version, platform)
);
CREATE INDEX release_assets_order ON release_assets(platform, status, major, minor, patch);
