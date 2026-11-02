//! Named migrations. Each step runs once, in order, inside one transaction.
//! Feature modules append steps with their own names (`files-1`, `search-1`, ...);
//! never edit or reorder a released step.

use rusqlite::{Connection, params};

pub struct Migration {
    pub name: &'static str,
    pub sql: &'static str,
}

pub const MIGRATIONS: &[Migration] = &[
    Migration {
        name: "core-1",
        sql: r#"
CREATE TABLE meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE members (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  name TEXT NOT NULL,
  role TEXT,
  level TEXT NOT NULL,
  avatar_url TEXT,
  deactivated INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE groups (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  name TEXT NOT NULL,
  member_ids TEXT NOT NULL,
  created_by TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  last_seq INTEGER NOT NULL DEFAULT 0,
  read_seq INTEGER NOT NULL DEFAULT 0,
  unread INTEGER NOT NULL DEFAULT 0,
  mentions INTEGER NOT NULL DEFAULT 0,
  summary_seq INTEGER NOT NULL DEFAULT 0,
  last_activity_at INTEGER NOT NULL DEFAULT 0,
  muted INTEGER NOT NULL DEFAULT 0,
  notify TEXT
);
CREATE INDEX groups_by_activity ON groups(last_activity_at DESC);
CREATE TABLE sync_state (
  group_id TEXT PRIMARY KEY,
  last_applied INTEGER NOT NULL
);
CREATE TABLE topics (
  id TEXT PRIMARY KEY,
  group_id TEXT NOT NULL,
  title TEXT NOT NULL,
  status TEXT NOT NULL,
  created_by TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  root_message_id TEXT NOT NULL,
  last_seq INTEGER NOT NULL,
  message_count INTEGER NOT NULL,
  last_activity_at INTEGER NOT NULL,
  history_complete INTEGER NOT NULL DEFAULT 0,
  owner_id TEXT,
  waiting_on TEXT NOT NULL DEFAULT '[]',
  last_message_at INTEGER,
  raw TEXT NOT NULL
);
CREATE INDEX topics_by_activity ON topics(group_id, last_seq DESC);
CREATE TABLE messages (
  id TEXT PRIMARY KEY,
  client_msg_id TEXT NOT NULL,
  group_id TEXT NOT NULL,
  topic_id TEXT NOT NULL,
  author_id TEXT NOT NULL,
  body TEXT NOT NULL,
  mentions TEXT NOT NULL,
  created_seq INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  edited_at INTEGER,
  deleted_at INTEGER,
  streaming INTEGER NOT NULL DEFAULT 0,
  kind TEXT NOT NULL DEFAULT 'text',
  raw TEXT NOT NULL
);
CREATE INDEX messages_by_topic ON messages(topic_id, created_seq);
CREATE INDEX messages_by_group ON messages(group_id, created_seq);
CREATE TABLE read_cursors (
  group_id TEXT NOT NULL,
  topic_id TEXT NOT NULL DEFAULT '',
  seq INTEGER NOT NULL,
  synced INTEGER NOT NULL DEFAULT 0,
  following INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (group_id, topic_id)
);
CREATE TABLE outbox (
  ord INTEGER PRIMARY KEY AUTOINCREMENT,
  id TEXT NOT NULL UNIQUE,
  kind TEXT NOT NULL,
  group_id TEXT NOT NULL,
  topic_id TEXT,
  payload TEXT NOT NULL,
  attempts INTEGER NOT NULL DEFAULT 0,
  last_error TEXT,
  state TEXT NOT NULL DEFAULT 'pending',
  created_at INTEGER NOT NULL
);
CREATE INDEX outbox_by_topic ON outbox(topic_id, ord);
"#,
    },
    Migration {
        name: "ext-1",
        sql: r#"
CREATE TABLE artifacts (
  id TEXT PRIMARY KEY,
  group_id TEXT NOT NULL,
  topic_id TEXT NOT NULL,
  current_version INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  raw TEXT NOT NULL
);
CREATE INDEX artifacts_by_topic ON artifacts(topic_id, updated_at DESC);
CREATE TABLE artifact_versions (
  artifact_id TEXT NOT NULL,
  version INTEGER NOT NULL,
  raw TEXT NOT NULL,
  PRIMARY KEY (artifact_id, version)
);
CREATE TABLE card_actions (
  group_id TEXT NOT NULL,
  seq INTEGER NOT NULL,
  message_id TEXT NOT NULL,
  actor_id TEXT NOT NULL,
  action_id TEXT NOT NULL,
  at INTEGER NOT NULL,
  PRIMARY KEY (group_id, seq)
);
CREATE INDEX card_actions_by_message ON card_actions(message_id, seq);
CREATE TABLE file_cache (
  sha256 TEXT PRIMARY KEY,
  path TEXT NOT NULL,
  size INTEGER NOT NULL,
  last_used INTEGER NOT NULL
);
CREATE INDEX file_cache_by_use ON file_cache(last_used);
CREATE TABLE runs (
  id TEXT PRIMARY KEY,
  group_id TEXT NOT NULL,
  topic_id TEXT NOT NULL,
  updated_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  raw TEXT NOT NULL
);
CREATE INDEX runs_by_topic ON runs(topic_id, created_at DESC);
"#,
    },
];

pub fn run(conn: &mut Connection, list: &[Migration]) -> rusqlite::Result<Vec<&'static str>> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS migrations (name TEXT PRIMARY KEY, applied_at INTEGER NOT NULL)",
    )?;
    let mut applied = Vec::new();
    for m in list {
        let tx = conn.transaction()?;
        let done: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM migrations WHERE name = ?1)",
            [m.name],
            |r| r.get(0),
        )?;
        if !done {
            tx.execute_batch(m.sql)?;
            tx.execute(
                "INSERT INTO migrations (name, applied_at) VALUES (?1, ?2)",
                params![m.name, crate::now_ms()],
            )?;
            applied.push(m.name);
        }
        tx.commit()?;
    }
    Ok(applied)
}
