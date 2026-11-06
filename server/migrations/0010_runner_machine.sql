-- 1.2.2: an owner can pin a runner-backed bot to one machine (docs/protocol.md, "Bot runs").
-- NULL = any machine with the runner's labels (the 1.2.1 behavior).
ALTER TABLE bot_runners ADD COLUMN machine_id TEXT;
