-- 1.1: files over 100 MB upload in parts (R2 multipart upload). `upload_id` is the R2 upload of a
-- pending file; `file_parts` holds the parts uploaded so far. Old rows have no upload id.
ALTER TABLE files ADD COLUMN upload_id TEXT;
CREATE TABLE file_parts (
  file_id TEXT NOT NULL,
  part INTEGER NOT NULL,
  etag TEXT NOT NULL,
  size INTEGER NOT NULL,
  PRIMARY KEY (file_id, part)
);
