-- 1.1: a group's purpose line, and the seq of the room's newest `group.updated` event that D1
-- holds. A rename or purpose write lands only when its seq is newer, so D1 follows the room log
-- whatever order concurrent requests finish in (GAPS SF-8, SF-9). Old rows: no purpose, seq 0.
ALTER TABLE groups ADD COLUMN purpose TEXT;
ALTER TABLE groups ADD COLUMN update_seq INTEGER NOT NULL DEFAULT 0;
