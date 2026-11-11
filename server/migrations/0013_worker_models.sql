-- 设置 › 模型: the team's model and effort per CLI runtime (claude, codex). A member's own runner
-- model or effort wins; NULL = the CLI's own default.
CREATE TABLE worker_models (
  runtime TEXT PRIMARY KEY,
  model TEXT,
  effort TEXT,
  updated_at INTEGER NOT NULL,
  updated_by TEXT
);
