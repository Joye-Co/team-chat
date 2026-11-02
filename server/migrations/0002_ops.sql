-- Daily archive state: the last group-log seq written to R2, per group.
CREATE TABLE ops_archive (
  group_id TEXT PRIMARY KEY,
  last_seq INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
