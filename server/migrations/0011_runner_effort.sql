-- 思考强度 of a runner-backed bot (docs/protocol.md, "Bot runs"). NULL = the CLI's default.
ALTER TABLE bot_runners ADD COLUMN effort TEXT;
