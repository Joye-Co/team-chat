-- A file whose every message was deleted: no longer served, its R2 object is removed.
ALTER TABLE files ADD COLUMN deleted_at INTEGER;
CREATE INDEX files_creator ON files(created_by);
