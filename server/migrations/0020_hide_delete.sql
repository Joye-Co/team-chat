-- Hide and delete (docs/protocol.md, "Hide and delete"). A deleted 群 keeps its row (other
-- tables still name its id) but leaves every lookup; its memberships are removed. A deleted AI
-- member keeps its row so old messages keep their author (it is also deactivated).
ALTER TABLE groups ADD COLUMN deleted_at INTEGER;
ALTER TABLE members ADD COLUMN deleted_at INTEGER;
