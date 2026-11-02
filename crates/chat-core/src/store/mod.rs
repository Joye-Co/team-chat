//! Local SQLite cache. The UI reads through the synchronous query methods; the sync
//! engine is the only writer of server state. WAL mode, one writer connection and
//! one reader connection, so UI reads do not wait for a write transaction.
//!
//! Feature modules extend the cache with their own named migrations
//! ([`Store::open_with`]) and their own tables through [`Store::with_writer`] and
//! [`Store::with_reader`].

mod apply;
pub mod migrations;
mod rows;

use crate::proto::{Event, Member, Message};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

pub use migrations::Migration;
pub use rows::*;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub type StoreResult<T> = Result<T, StoreError>;

/// Thread-safe handle to the cache. Cloning shares the same connections.
#[derive(Clone)]
pub struct Store {
    inner: Arc<Inner>,
}

struct Inner {
    writer: Mutex<Connection>,
    reader: Mutex<Connection>,
}

/// One entry of a group log as received.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)] // Known is the common case; boxing it costs an allocation per event.
pub enum LogEntry {
    Known(Event),
    /// An event this client cannot read (a type added by a newer server). It keeps
    /// its place in the seq order and is otherwise skipped.
    Unknown {
        seq: u64,
        group_id: String,
        event_type: String,
    },
}

impl LogEntry {
    pub fn seq(&self) -> u64 {
        match self {
            LogEntry::Known(e) => e.seq,
            LogEntry::Unknown { seq, .. } => *seq,
        }
    }
    pub fn group_id(&self) -> &str {
        match self {
            LogEntry::Known(e) => &e.group_id,
            LogEntry::Unknown { group_id, .. } => group_id,
        }
    }
}

/// What one `apply_events` call changed.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ApplyOutcome {
    /// Events applied in this call.
    pub applied: usize,
    /// `lastApplied` of the group after the call.
    pub last_applied: u64,
    /// First seq that could not be applied because an earlier seq is missing.
    pub gap_at: Option<u64>,
    pub group_changed: bool,
    pub topic_list_changed: bool,
    /// Topics whose messages changed.
    pub message_topics: BTreeSet<String>,
    /// Client message ids of my own messages that this call created.
    pub my_client_msg_ids: Vec<String>,
    /// (topic id, author id) of every message this call created.
    pub created: Vec<(String, String)>,
    /// Messages whose stored body this call replaced (edited, finished or deleted).
    pub replaced_bodies: Vec<String>,
    /// Applied events the core cache does not model (`card.actioned`, `artifact.*`),
    /// for feature modules and the UI.
    pub other_events: Vec<Event>,
}

/// What a welcome, directory or reads frame changed.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct DirectoryOutcome {
    pub members_changed: bool,
    pub groups_changed: BTreeSet<String>,
    pub groups_removed: Vec<String>,
}

/// A read cursor that the server does not have yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRead {
    pub group_id: String,
    /// `None` = the group cursor.
    pub topic_id: Option<String>,
    pub seq: u64,
}

fn lock(m: &Mutex<Connection>) -> MutexGuard<'_, Connection> {
    // A panic while holding the lock leaves SQLite consistent (transactions roll back).
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn open_conn(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(conn)
}

impl Store {
    /// Opens or creates the cache at `path` and runs the core migrations.
    pub fn open(path: &Path) -> StoreResult<Store> {
        Self::open_with(path, &[])
    }

    /// Opens the cache and also runs `extra` migrations (feature modules), after the core.
    pub fn open_with(path: &Path, extra: &[Migration]) -> StoreResult<Store> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut writer = open_conn(path)?;
        migrations::run(&mut writer, migrations::MIGRATIONS)?;
        if !extra.is_empty() {
            migrations::run(&mut writer, extra)?;
        }
        let reader = open_conn(path)?;
        Ok(Store {
            inner: Arc::new(Inner {
                writer: Mutex::new(writer),
                reader: Mutex::new(reader),
            }),
        })
    }

    /// Runs a read-only closure on the reader connection (for feature modules).
    pub fn with_reader<T>(
        &self,
        f: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> StoreResult<T> {
        Ok(f(&lock(&self.inner.reader))?)
    }

    /// Runs a closure in a write transaction (for feature modules).
    pub fn with_writer<T>(
        &self,
        f: impl FnOnce(&Transaction<'_>) -> StoreResult<T>,
    ) -> StoreResult<T> {
        let mut conn = lock(&self.inner.writer);
        let tx = conn.transaction()?;
        let v = f(&tx)?;
        tx.commit()?;
        Ok(v)
    }

    // ------------------------------------------------------------------
    // Read cursors (local side)
    // ------------------------------------------------------------------

    /// Moves a read cursor forward locally: the group cursor when `topic_id` is `None`,
    /// else that topic's cursor (the group cursor stays). Returns the read to send to
    /// the server when the cursor moved.
    pub fn mark_read(
        &self,
        group_id: &str,
        topic_id: Option<&str>,
        seq: u64,
    ) -> StoreResult<Option<PendingRead>> {
        let me = self.me_id()?.unwrap_or_default();
        self.with_writer(|tx| {
            let last_seq: u64 = tx
                .query_row(
                    "SELECT last_seq FROM groups WHERE id = ?1",
                    [group_id],
                    |r| r.get(0),
                )
                .optional()?
                .unwrap_or(seq);
            let seq = seq.min(last_seq);
            let key = topic_id.unwrap_or("");
            let before: u64 = tx
                .query_row(
                    "SELECT seq FROM read_cursors WHERE group_id = ?1 AND topic_id = ?2",
                    params![group_id, key],
                    |r| r.get(0),
                )
                .optional()?
                .unwrap_or(0);
            let group_before: u64 = tx
                .query_row(
                    "SELECT read_seq FROM groups WHERE id = ?1",
                    [group_id],
                    |r| r.get(0),
                )
                .optional()?
                .unwrap_or(0);
            let moved = seq > before && (topic_id.is_some() || seq > group_before);
            if !moved {
                return Ok(None);
            }
            tx.execute(
                "INSERT INTO read_cursors (group_id, topic_id, seq, synced) VALUES (?1, ?2, ?3, 0)
                 ON CONFLICT(group_id, topic_id) DO UPDATE SET seq = excluded.seq, synced = 0",
                params![group_id, key, seq],
            )?;
            if topic_id.is_none() {
                tx.execute(
                    "UPDATE groups SET read_seq = MAX(read_seq, ?2) WHERE id = ?1",
                    params![group_id, seq],
                )?;
            }
            apply::recount_unread(tx, &me, group_id)?;
            Ok(Some(PendingRead {
                group_id: group_id.into(),
                topic_id: topic_id.map(str::to_string),
                seq,
            }))
        })
    }

    /// Read cursors not yet sent to the server.
    pub fn unsynced_reads(&self) -> StoreResult<Vec<PendingRead>> {
        self.with_reader(|c| {
            let mut st =
                c.prepare("SELECT group_id, topic_id, seq FROM read_cursors WHERE synced = 0")?;
            st.query_map([], |r| {
                let t: String = r.get(1)?;
                Ok(PendingRead {
                    group_id: r.get(0)?,
                    topic_id: (!t.is_empty()).then_some(t),
                    seq: r.get(2)?,
                })
            })?
            .collect()
        })
    }

    pub fn mark_read_synced(&self, read: &PendingRead) -> StoreResult<()> {
        self.with_writer(|tx| {
            tx.execute(
                "UPDATE read_cursors SET synced = 1 WHERE group_id = ?1 AND topic_id = ?2 AND seq <= ?3",
                params![read.group_id, read.topic_id.as_deref().unwrap_or(""), read.seq],
            )?;
            Ok(())
        })
    }

    // ------------------------------------------------------------------
    // Outbox
    // ------------------------------------------------------------------

    pub fn outbox_push(&self, item: &OutboxItem) -> StoreResult<()> {
        self.with_writer(|tx| {
            tx.execute(
                "INSERT OR IGNORE INTO outbox (id, kind, group_id, topic_id, payload, attempts, last_error, state, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0, NULL, 'pending', ?6)",
                params![
                    item.id,
                    item.op.kind(),
                    item.op.group_id(),
                    item.op.topic_id(),
                    serde_json::to_string(&item.op)?,
                    item.created_at
                ],
            )?;
            Ok(())
        })
    }

    /// Every outbox row in the order it was added.
    pub fn outbox_all(&self) -> StoreResult<Vec<OutboxItem>> {
        self.with_reader(|c| {
            let mut st = c.prepare(&format!("SELECT {OUTBOX_COLS} FROM outbox ORDER BY ord"))?;
            st.query_map([], rows::outbox_from_row)?.collect()
        })
    }

    pub fn outbox_get(&self, id: &str) -> StoreResult<Option<OutboxItem>> {
        self.with_reader(|c| {
            c.query_row(
                &format!("SELECT {OUTBOX_COLS} FROM outbox WHERE id = ?1"),
                [id],
                rows::outbox_from_row,
            )
            .optional()
        })
    }

    pub fn outbox_attempt(&self, id: &str) -> StoreResult<()> {
        self.with_writer(|tx| {
            tx.execute(
                "UPDATE outbox SET attempts = attempts + 1 WHERE id = ?1",
                [id],
            )?;
            Ok(())
        })
    }

    /// Sets the state and last error of an outbox row.
    pub fn outbox_set_state(
        &self,
        id: &str,
        state: OutboxState,
        error: Option<&str>,
    ) -> StoreResult<()> {
        self.with_writer(|tx| {
            tx.execute(
                "UPDATE outbox SET state = ?2, last_error = ?3 WHERE id = ?1",
                params![id, state.as_str(), error],
            )?;
            Ok(())
        })
    }

    pub fn outbox_remove(&self, id: &str) -> StoreResult<bool> {
        self.with_writer(|tx| Ok(tx.execute("DELETE FROM outbox WHERE id = ?1", [id])? > 0))
    }

    /// Unacked and failed sends into an existing topic, oldest first.
    pub fn pending_sends_in_topic(&self, topic_id: &str) -> StoreResult<Vec<OutboxItem>> {
        self.with_reader(|c| {
            let mut st = c.prepare(&format!(
                "SELECT {OUTBOX_COLS} FROM outbox WHERE topic_id = ?1 AND kind = 'send' ORDER BY ord"
            ))?;
            st.query_map([topic_id], rows::outbox_from_row)?.collect()
        })
    }

    /// Unacked and failed sends that start a new topic in a group.
    pub fn pending_new_topics(&self, group_id: &str) -> StoreResult<Vec<OutboxItem>> {
        self.with_reader(|c| {
            let mut st = c.prepare(&format!(
                "SELECT {OUTBOX_COLS} FROM outbox WHERE group_id = ?1 AND topic_id IS NULL AND kind = 'send' ORDER BY ord"
            ))?;
            st.query_map([group_id], rows::outbox_from_row)?.collect()
        })
    }

    // ------------------------------------------------------------------
    // Queries for the UI (reader connection)
    // ------------------------------------------------------------------

    pub fn me(&self) -> StoreResult<Option<Member>> {
        let s: Option<String> = self.with_reader(|c| {
            c.query_row("SELECT value FROM meta WHERE key = 'me'", [], |r| r.get(0))
                .optional()
        })?;
        Ok(match s {
            Some(s) => Some(serde_json::from_str(&s)?),
            None => None,
        })
    }

    pub fn me_id(&self) -> StoreResult<Option<String>> {
        Ok(self.me()?.map(|m| m.id))
    }

    pub fn members(&self) -> StoreResult<Vec<Member>> {
        self.with_reader(|c| {
            let mut st = c.prepare(&format!(
                "SELECT {} FROM members ORDER BY name",
                rows::MEMBER_COLS
            ))?;
            st.query_map([], rows::member_from_row)?.collect()
        })
    }

    pub fn member(&self, id: &str) -> StoreResult<Option<Member>> {
        self.with_reader(|c| {
            c.query_row(
                &format!("SELECT {} FROM members WHERE id = ?1", rows::MEMBER_COLS),
                [id],
                rows::member_from_row,
            )
            .optional()
        })
    }

    /// My groups by last activity, newest first, with unread and mention totals.
    pub fn groups(&self) -> StoreResult<Vec<GroupRow>> {
        self.with_reader(|c| {
            let mut st = c.prepare(&format!(
                "SELECT {} FROM groups ORDER BY MAX(last_activity_at, created_at) DESC, id",
                rows::GROUP_COLS
            ))?;
            st.query_map([], rows::group_from_row)?.collect()
        })
    }

    pub fn group(&self, id: &str) -> StoreResult<Option<GroupRow>> {
        self.with_reader(|c| {
            c.query_row(
                &format!("SELECT {} FROM groups WHERE id = ?1", rows::GROUP_COLS),
                [id],
                rows::group_from_row,
            )
            .optional()
        })
    }

    /// Topics of a group by last activity (newest first) with my read state.
    pub fn topics(&self, group_id: &str, limit: usize) -> StoreResult<Vec<TopicRow>> {
        let me = self.me_id()?.unwrap_or_default();
        self.with_reader(|c| {
            let mut st = c.prepare_cached(&rows::topic_select(
                "WHERE t.group_id = ?1 ORDER BY t.last_seq DESC LIMIT ?3",
            ))?;
            st.query_map(params![group_id, me, limit as u64], rows::topic_from_row)?
                .collect()
        })
    }

    pub fn topic(&self, topic_id: &str) -> StoreResult<Option<TopicRow>> {
        let me = self.me_id()?.unwrap_or_default();
        self.with_reader(|c| {
            c.prepare_cached(&rows::topic_select("WHERE t.id = ?1"))?
                .query_row(params![topic_id, me], rows::topic_from_row)
                .optional()
        })
    }

    /// Up to `limit` messages of a topic created before `before_seq` (the newest when
    /// `None`), oldest first.
    pub fn messages_before(
        &self,
        topic_id: &str,
        before_seq: Option<u64>,
        limit: usize,
    ) -> StoreResult<Vec<Message>> {
        self.with_reader(|c| {
            let mut st = c.prepare_cached(&format!(
                "SELECT {} FROM messages WHERE topic_id = ?1 AND created_seq < ?2 ORDER BY created_seq DESC LIMIT ?3",
                rows::MESSAGE_COLS
            ))?;
            let before = before_seq.map_or(i64::MAX, |s| s.min(i64::MAX as u64) as i64);
            let mut v: Vec<Message> = st
                .query_map(params![topic_id, before, limit as u64], rows::message_from_row)?
                .collect::<Result<_, _>>()?;
            v.reverse();
            Ok(v)
        })
    }

    /// Up to `limit` messages of a topic created after `after_seq`, oldest first.
    pub fn messages_after(
        &self,
        topic_id: &str,
        after_seq: u64,
        limit: usize,
    ) -> StoreResult<Vec<Message>> {
        self.with_reader(|c| {
            let mut st = c.prepare_cached(&format!(
                "SELECT {} FROM messages WHERE topic_id = ?1 AND created_seq > ?2 ORDER BY created_seq ASC LIMIT ?3",
                rows::MESSAGE_COLS
            ))?;
            st.query_map(params![topic_id, after_seq, limit as u64], rows::message_from_row)?.collect()
        })
    }

    pub fn message(&self, id: &str) -> StoreResult<Option<Message>> {
        self.with_reader(|c| {
            c.query_row(
                &format!("SELECT {} FROM messages WHERE id = ?1", rows::MESSAGE_COLS),
                [id],
                rows::message_from_row,
            )
            .optional()
        })
    }

    pub fn message_count_cached(&self, topic_id: &str) -> StoreResult<u64> {
        self.with_reader(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM messages WHERE topic_id = ?1",
                [topic_id],
                |r| r.get(0),
            )
        })
    }

    /// `lastApplied` per group (the `hello` cursors).
    pub fn cursors(&self) -> StoreResult<BTreeMap<String, u64>> {
        self.with_reader(|c| {
            let mut st = c.prepare("SELECT group_id, last_applied FROM sync_state")?;
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect()
        })
    }

    pub fn last_applied(&self, group_id: &str) -> StoreResult<u64> {
        self.with_reader(|c| {
            Ok(c.query_row(
                "SELECT last_applied FROM sync_state WHERE group_id = ?1",
                [group_id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0))
        })
    }

    /// Oldest cached `createdSeq` of a topic, the `beforeSeq` for loading older history.
    pub fn oldest_cached_seq(&self, topic_id: &str) -> StoreResult<Option<u64>> {
        self.with_reader(|c| {
            c.query_row(
                "SELECT MIN(created_seq) FROM messages WHERE topic_id = ?1",
                [topic_id],
                |r| r.get(0),
            )
        })
    }

    /// Group id of a cached topic.
    pub fn topic_group(&self, topic_id: &str) -> StoreResult<Option<String>> {
        self.with_reader(|c| {
            c.query_row(
                "SELECT group_id FROM topics WHERE id = ?1",
                [topic_id],
                |r| r.get(0),
            )
            .optional()
        })
    }
}
