-- 沉淀 (memory, docs/protocol.md "Memory"): what the team, a group, an AI member and a person
-- keep. scope = 'team' | 'group:<id>' | 'member:<id>' | 'person:<id>'. AI-written items start
-- 'pending' and become active through the daily consolidation; a person's are active at once.
CREATE TABLE memories (
  id TEXT PRIMARY KEY,
  scope TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('convention', 'fact', 'decision', 'preference', 'lesson', 'playbook')),
  text TEXT NOT NULL,
  when_text TEXT,
  -- JSON { kind: 'message' | 'run' | 'card' | 'manual', ids: string[] }
  source TEXT NOT NULL,
  note TEXT,
  author_id TEXT NOT NULL,
  human INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL CHECK (status IN ('active', 'retired', 'pending')),
  confidence REAL NOT NULL DEFAULT 1,
  uses INTEGER NOT NULL DEFAULT 0,
  last_used_at INTEGER,
  version INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX memories_scope ON memories(scope, status, updated_at);
CREATE INDEX memories_pending ON memories(status, created_at);

-- Every version of a memory (snapshot = the MemoryItem JSON); by_id NULL = the consolidation.
CREATE TABLE memory_versions (
  memory_id TEXT NOT NULL,
  version INTEGER NOT NULL,
  snapshot TEXT NOT NULL,
  by_id TEXT,
  at INTEGER NOT NULL,
  PRIMARY KEY (memory_id, version)
);

-- The daily 记忆更新: one row per change the consolidation made; each can be undone.
CREATE TABLE memory_updates (
  id TEXT PRIMARY KEY,
  day TEXT NOT NULL,
  scope TEXT NOT NULL,
  memory_id TEXT NOT NULL,
  change TEXT NOT NULL CHECK (change IN ('added', 'changed', 'retired', 'flagged')),
  before_json TEXT,
  after_json TEXT,
  note TEXT,
  undone_at INTEGER,
  at INTEGER NOT NULL
);
CREATE INDEX memory_updates_scope ON memory_updates(scope, day, at);

-- One consolidation per scope and local day (idempotence of the cron).
CREATE TABLE memory_runs (
  day TEXT NOT NULL,
  scope TEXT NOT NULL,
  at INTEGER NOT NULL,
  ops INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (day, scope)
);
