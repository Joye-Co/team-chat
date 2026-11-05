-- AI 服务: the team's model provider for the server's own model calls (AI topic titles).
-- One row, scope '*'. key_cipher is the API key encrypted with HOOK_SECRET_KEY (AES-GCM,
-- `v1.<iv>.<ciphertext>`); key_hint is its last 4 characters. See docs/protocol.md, "AI 服务".
CREATE TABLE ai_service (
  scope TEXT PRIMARY KEY,
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  base_url TEXT,
  key_cipher TEXT NOT NULL,
  key_hint TEXT NOT NULL,
  updated_at INTEGER NOT NULL,
  updated_by TEXT
);
