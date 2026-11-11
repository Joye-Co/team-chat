-- Who may read an AI member's memory (docs/protocol.md, "Memory"): the group it was learned in.
-- NULL = no limit. AI-written member memories from before this column do not know their group:
-- 'unknown' keeps them for owners only, out of every prompt, until an owner releases them.
ALTER TABLE memories ADD COLUMN from_group TEXT;
UPDATE memories SET from_group = 'unknown' WHERE scope LIKE 'member:%' AND human = 0;
