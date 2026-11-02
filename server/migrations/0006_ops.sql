-- Archive run state and release withdrawal (server/src/ops, server/src/files/ops.ts).
-- ops_meta: small key/value state of the daily archive (resume cursor, last complete run).
CREATE TABLE ops_meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);
-- The UTC date of the group's newest room state snapshot: one snapshot per group per day.
ALTER TABLE ops_archive ADD COLUMN state_date TEXT;
-- A withdrawn release is never offered as latest and cannot be downloaded.
ALTER TABLE releases ADD COLUMN withdrawn_at INTEGER;
