//! Local SQLite cache. The UI reads through the synchronous query methods; the sync
//! engine is the only writer of server state. WAL mode, one writer connection and
//! one reader connection, so UI reads do not wait for a write transaction.

pub mod migrations;
mod rows;

use crate::proto::{
    Event, EventData, GroupMemberChangeData, GroupSummary, Member, Message, MessageDeletedData,
    MessageUpdatedData, SendResult, Topic, TopicStatus, TopicUpdatedData,
};
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
    /// An event type this client does not know (added by a newer server). It keeps
    /// the seq order and is otherwise skipped.
    Unknown { seq: u64, group_id: String, event_type: String },
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
}

/// What a welcome or directory frame changed.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct DirectoryOutcome {
    pub members_changed: bool,
    pub groups_changed: BTreeSet<String>,
    pub groups_removed: Vec<String>,
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
    conn.pragma_update(None, "foreign_keys", "ON")?;
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
    pub fn with_reader<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> StoreResult<T> {
        Ok(f(&lock(&self.inner.reader))?)
    }

    /// Runs a closure in a write transaction (for feature modules).
    pub fn with_writer<T>(&self, f: impl FnOnce(&Transaction<'_>) -> StoreResult<T>) -> StoreResult<T> {
        let mut conn = lock(&self.inner.writer);
        let tx = conn.transaction()?;
        let v = f(&tx)?;
        tx.commit()?;
        Ok(v)
    }

    // ------------------------------------------------------------------
    // Directory
    // ------------------------------------------------------------------

    /// Applies a `welcome`: me, all members, and the full list of my groups.
    /// Groups that are not in the list are removed from the cache.
    pub fn apply_welcome(
        &self,
        me: &Member,
        members: &[Member],
        groups: &[GroupSummary],
    ) -> StoreResult<DirectoryOutcome> {
        self.with_writer(|tx| {
            tx.execute(
                "INSERT INTO meta (key, value) VALUES ('me', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [serde_json::to_string(me)?],
            )?;
            let mut out = DirectoryOutcome::default();
            upsert_member(tx, me)?;
            for m in members {
                upsert_member(tx, m)?;
            }
            out.members_changed = true;
            let keep: BTreeSet<&str> = groups.iter().map(|g| g.group.id.as_str()).collect();
            let existing: Vec<String> = {
                let mut st = tx.prepare("SELECT id FROM groups")?;
                st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?
            };
            for id in existing {
                if !keep.contains(id.as_str()) {
                    remove_group(tx, &id)?;
                    out.groups_removed.push(id);
                }
            }
            for g in groups {
                upsert_group_summary(tx, &me.id, g)?;
                out.groups_changed.insert(g.group.id.clone());
            }
            Ok(out)
        })
    }

    /// Applies a `directory` frame.
    pub fn apply_directory(
        &self,
        members: Option<&[Member]>,
        groups: Option<&[GroupSummary]>,
        removed: Option<&[String]>,
    ) -> StoreResult<DirectoryOutcome> {
        let me = self.me_id()?.unwrap_or_default();
        self.with_writer(|tx| {
            let mut out = DirectoryOutcome::default();
            if let Some(ms) = members {
                for m in ms {
                    upsert_member(tx, m)?;
                }
                out.members_changed = !ms.is_empty();
            }
            if let Some(gs) = groups {
                for g in gs {
                    upsert_group_summary(tx, &me, g)?;
                    out.groups_changed.insert(g.group.id.clone());
                }
            }
            if let Some(ids) = removed {
                for id in ids {
                    remove_group(tx, id)?;
                    out.groups_removed.push(id.clone());
                }
            }
            Ok(out)
        })
    }

    // ------------------------------------------------------------------
    // Event log
    // ------------------------------------------------------------------

    /// Applies events of one group strictly in seq order: events at or below
    /// `lastApplied` are skipped, and application stops at the first gap.
    /// `events` may be in any order and contain duplicates.
    pub fn apply_events(&self, group_id: &str, events: &[Event]) -> StoreResult<ApplyOutcome> {
        let entries: Vec<LogEntry> = events.iter().cloned().map(LogEntry::Known).collect();
        self.apply_log(group_id, &entries)
    }

    /// Like `apply_events`, for log entries that may include event types this client
    /// does not know: those only advance `lastApplied`.
    pub fn apply_log(&self, group_id: &str, entries: &[LogEntry]) -> StoreResult<ApplyOutcome> {
        let me = self.me_id()?.unwrap_or_default();
        let mut sorted: Vec<&LogEntry> = entries.iter().filter(|e| e.group_id() == group_id).collect();
        sorted.sort_by_key(|e| e.seq());
        self.with_writer(|tx| {
            let mut last = last_applied_tx(tx, group_id)?;
            let mut out = ApplyOutcome::default();
            for entry in sorted {
                let seq = entry.seq();
                if seq <= last {
                    continue;
                }
                if seq != last + 1 {
                    out.gap_at = Some(seq);
                    break;
                }
                if let LogEntry::Known(ev) = entry {
                    apply_one(tx, &me, ev, &mut out)?;
                }
                last = seq;
                out.applied += 1;
            }
            if out.applied > 0 {
                tx.execute(
                    "INSERT INTO sync_state (group_id, last_applied) VALUES (?1, ?2)
                     ON CONFLICT(group_id) DO UPDATE SET last_applied = excluded.last_applied",
                    params![group_id, last],
                )?;
                tx.execute(
                    "UPDATE groups SET last_seq = MAX(last_seq, ?2) WHERE id = ?1",
                    params![group_id, last],
                )?;
            }
            out.last_applied = last;
            Ok(out)
        })
    }

    /// Stores messages from a `history` answer. Rows already in the cache stay as they
    /// are (events keep them current). Marks the topic complete when `has_more` is false.
    pub fn insert_history(&self, topic_id: &str, messages: &[Message], has_more: bool) -> StoreResult<usize> {
        self.with_writer(|tx| {
            let mut n = 0;
            for m in messages {
                n += insert_message_if_absent(tx, m)?;
            }
            if !has_more {
                tx.execute("UPDATE topics SET history_complete = 1 WHERE id = ?1", [topic_id])?;
            }
            Ok(n)
        })
    }

    /// Records a successful `send` ack: stores the message and topic when they are not
    /// cached yet, and removes the outbox row.
    pub fn ack_send(&self, client_msg_id: &str, result: &SendResult) -> StoreResult<()> {
        self.with_writer(|tx| {
            insert_message_if_absent(tx, &result.message)?;
            if let Some(t) = &result.topic {
                insert_topic_if_absent(tx, t, result.message.created_at)?;
            }
            tx.execute("DELETE FROM outbox WHERE id = ?1", [client_msg_id])?;
            Ok(())
        })
    }

    // ------------------------------------------------------------------
    // Read cursors
    // ------------------------------------------------------------------

    /// Moves a read cursor forward locally. `topic_id = None` is the group cursor.
    /// Returns the group cursor to send to the server when it moved forward.
    pub fn mark_read(&self, group_id: &str, topic_id: Option<&str>, seq: u64) -> StoreResult<Option<u64>> {
        let me = self.me_id()?.unwrap_or_default();
        self.with_writer(|tx| {
            let key = topic_id.unwrap_or("");
            tx.execute(
                "INSERT INTO read_cursors (group_id, topic_id, seq, synced) VALUES (?1, ?2, ?3, 0)
                 ON CONFLICT(group_id, topic_id) DO UPDATE SET seq = excluded.seq, synced = 0
                 WHERE excluded.seq > read_cursors.seq",
                params![group_id, key, seq],
            )?;
            let before: u64 = tx
                .query_row("SELECT read_seq FROM groups WHERE id = ?1", [group_id], |r| r.get(0))
                .optional()?
                .unwrap_or(0);
            if topic_id.is_none() {
                tx.execute(
                    "UPDATE groups SET read_seq = MAX(read_seq, ?2) WHERE id = ?1",
                    params![group_id, seq],
                )?;
            }
            recount_unread(tx, &me, group_id)?;
            // When every topic is read, the group cursor moves to the newest seq.
            let (unread, last_seq): (u64, u64) = tx
                .query_row("SELECT unread, last_seq FROM groups WHERE id = ?1", [group_id], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .optional()?
                .unwrap_or((1, 0));
            if unread == 0 && last_seq > 0 {
                tx.execute(
                    "UPDATE groups SET read_seq = MAX(read_seq, ?2) WHERE id = ?1",
                    params![group_id, last_seq],
                )?;
            }
            let after: u64 = tx
                .query_row("SELECT read_seq FROM groups WHERE id = ?1", [group_id], |r| r.get(0))
                .optional()?
                .unwrap_or(0);
            if after > before {
                tx.execute(
                    "INSERT INTO read_cursors (group_id, topic_id, seq, synced) VALUES (?1, '', ?2, 0)
                     ON CONFLICT(group_id, topic_id) DO UPDATE SET seq = MAX(read_cursors.seq, excluded.seq), synced = 0",
                    params![group_id, after],
                )?;
                Ok(Some(after))
            } else {
                Ok(None)
            }
        })
    }

    /// Group cursors not yet sent to the server.
    pub fn unsynced_group_reads(&self) -> StoreResult<Vec<(String, u64)>> {
        self.with_reader(|c| {
            let mut st = c.prepare("SELECT group_id, seq FROM read_cursors WHERE topic_id = '' AND synced = 0")?;
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect()
        })
    }

    pub fn mark_read_synced(&self, group_id: &str, seq: u64) -> StoreResult<()> {
        self.with_writer(|tx| {
            tx.execute(
                "UPDATE read_cursors SET synced = 1 WHERE group_id = ?1 AND topic_id = '' AND seq <= ?2",
                params![group_id, seq],
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
            let mut st = c.prepare(
                "SELECT id, payload, attempts, last_error, state, created_at FROM outbox ORDER BY ord",
            )?;
            st.query_map([], rows::outbox_from_row)?.collect()
        })
    }

    pub fn outbox_get(&self, id: &str) -> StoreResult<Option<OutboxItem>> {
        self.with_reader(|c| {
            c.query_row(
                "SELECT id, payload, attempts, last_error, state, created_at FROM outbox WHERE id = ?1",
                [id],
                rows::outbox_from_row,
            )
            .optional()
        })
    }

    pub fn outbox_attempt(&self, id: &str) -> StoreResult<()> {
        self.with_writer(|tx| {
            tx.execute("UPDATE outbox SET attempts = attempts + 1 WHERE id = ?1", [id])?;
            Ok(())
        })
    }

    /// Sets the state and last error of an outbox row.
    pub fn outbox_set_state(&self, id: &str, state: OutboxState, error: Option<&str>) -> StoreResult<()> {
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

    /// Unacked sends into an existing topic, oldest first (the UI shows them as pending).
    pub fn pending_sends_in_topic(&self, topic_id: &str) -> StoreResult<Vec<OutboxItem>> {
        self.with_reader(|c| {
            let mut st = c.prepare(
                "SELECT id, payload, attempts, last_error, state, created_at FROM outbox
                 WHERE topic_id = ?1 AND kind = 'send' ORDER BY ord",
            )?;
            st.query_map([topic_id], rows::outbox_from_row)?.collect()
        })
    }

    /// Unacked sends that start a new topic in a group.
    pub fn pending_new_topics(&self, group_id: &str) -> StoreResult<Vec<OutboxItem>> {
        self.with_reader(|c| {
            let mut st = c.prepare(
                "SELECT id, payload, attempts, last_error, state, created_at FROM outbox
                 WHERE group_id = ?1 AND topic_id IS NULL AND kind = 'send' ORDER BY ord",
            )?;
            st.query_map([group_id], rows::outbox_from_row)?.collect()
        })
    }

    // ------------------------------------------------------------------
    // Queries for the UI (reader connection)
    // ------------------------------------------------------------------

    pub fn me(&self) -> StoreResult<Option<Member>> {
        let s: Option<String> = self.with_reader(|c| {
            c.query_row("SELECT value FROM meta WHERE key = 'me'", [], |r| r.get(0)).optional()
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
            let mut st = c.prepare(&format!("SELECT {} FROM members ORDER BY name", rows::MEMBER_COLS))?;
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

    /// My groups by last activity, newest first, with unread and mention counts.
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

    /// Topics of a group by last activity (newest first), with per-topic unread counts.
    pub fn topics(&self, group_id: &str, limit: usize) -> StoreResult<Vec<TopicRow>> {
        let me = self.me_id()?.unwrap_or_default();
        self.with_reader(|c| {
            let mut st = c.prepare(&format!(
                "SELECT {}, ({}) AS unread FROM topics t WHERE t.group_id = ?1 ORDER BY t.last_seq DESC LIMIT ?3",
                rows::TOPIC_COLS,
                rows::TOPIC_UNREAD_SQL
            ))?;
            st.query_map(params![group_id, me, limit as u64], rows::topic_from_row)?.collect()
        })
    }

    pub fn topic(&self, topic_id: &str) -> StoreResult<Option<TopicRow>> {
        let me = self.me_id()?.unwrap_or_default();
        self.with_reader(|c| {
            c.query_row(
                &format!(
                    "SELECT {}, ({}) AS unread FROM topics t WHERE t.id = ?1",
                    rows::TOPIC_COLS,
                    rows::TOPIC_UNREAD_SQL
                ),
                params![topic_id, me],
                rows::topic_from_row,
            )
            .optional()
        })
    }

    /// Up to `limit` messages of a topic created before `before_seq` (or the newest when
    /// `None`), returned oldest first.
    pub fn messages_before(&self, topic_id: &str, before_seq: Option<u64>, limit: usize) -> StoreResult<Vec<Message>> {
        self.with_reader(|c| {
            let mut st = c.prepare_cached(&format!(
                "SELECT {} FROM messages WHERE topic_id = ?1 AND created_seq < ?2 ORDER BY created_seq DESC LIMIT ?3",
                rows::MESSAGE_COLS
            ))?;
            let mut v: Vec<Message> = st
                .query_map(
                    params![topic_id, before_seq.map_or(i64::MAX, |s| s.min(i64::MAX as u64) as i64), limit as u64],
                    rows::message_from_row,
                )?
                .collect::<Result<_, _>>()?;
            v.reverse();
            Ok(v)
        })
    }

    /// Up to `limit` messages of a topic created after `after_seq`, oldest first.
    pub fn messages_after(&self, topic_id: &str, after_seq: u64, limit: usize) -> StoreResult<Vec<Message>> {
        self.with_reader(|c| {
            let mut st = c.prepare_cached(&format!(
                "SELECT {} FROM messages WHERE topic_id = ?1 AND created_seq > ?2 ORDER BY created_seq ASC LIMIT ?3",
                rows::MESSAGE_COLS
            ))?;
            st.query_map(params![topic_id, after_seq, limit as u64], rows::message_from_row)?
                .collect()
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
        self.with_reader(|c| c.query_row("SELECT COUNT(*) FROM messages WHERE topic_id = ?1", [topic_id], |r| r.get(0)))
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
            Ok(c.query_row("SELECT last_applied FROM sync_state WHERE group_id = ?1", [group_id], |r| r.get(0))
                .optional()?
                .unwrap_or(0))
        })
    }

    /// Oldest cached `createdSeq` of a topic, the `beforeSeq` for loading older history.
    pub fn oldest_cached_seq(&self, topic_id: &str) -> StoreResult<Option<u64>> {
        self.with_reader(|c| {
            c.query_row("SELECT MIN(created_seq) FROM messages WHERE topic_id = ?1", [topic_id], |r| r.get(0))
        })
    }

    /// Group id of a cached topic.
    pub fn topic_group(&self, topic_id: &str) -> StoreResult<Option<String>> {
        self.with_reader(|c| {
            c.query_row("SELECT group_id FROM topics WHERE id = ?1", [topic_id], |r| r.get(0)).optional()
        })
    }
}

// ----------------------------------------------------------------------
// Write helpers (inside a transaction)
// ----------------------------------------------------------------------

fn last_applied_tx(tx: &Transaction<'_>, group_id: &str) -> rusqlite::Result<u64> {
    Ok(tx
        .query_row("SELECT last_applied FROM sync_state WHERE group_id = ?1", [group_id], |r| r.get(0))
        .optional()?
        .unwrap_or(0))
}

fn upsert_member(tx: &Transaction<'_>, m: &Member) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO members (id, kind, name, role, level, avatar_url, deactivated) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET kind = excluded.kind, name = excluded.name, role = excluded.role,
           level = excluded.level, avatar_url = excluded.avatar_url, deactivated = excluded.deactivated",
        params![
            m.id,
            rows::enum_str(&m.kind)?,
            m.name,
            m.role(),
            rows::enum_str(&m.level)?,
            m.avatar_url(),
            m.is_deactivated()
        ],
    )?;
    Ok(())
}

fn upsert_group_summary(tx: &Transaction<'_>, me: &str, s: &GroupSummary) -> StoreResult<()> {
    let g = &s.group;
    // The local read cursor may be ahead of the server (a read not yet sent).
    let local_read: u64 = tx
        .query_row(
            "SELECT seq FROM read_cursors WHERE group_id = ?1 AND topic_id = ''",
            [&g.id],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(0);
    tx.execute(
        "INSERT INTO groups (id, kind, name, member_ids, created_by, created_at, last_seq, read_seq, unread, mentions, summary_seq)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?7)
         ON CONFLICT(id) DO UPDATE SET kind = excluded.kind, name = excluded.name, member_ids = excluded.member_ids,
           created_by = excluded.created_by, created_at = excluded.created_at,
           last_seq = MAX(groups.last_seq, excluded.last_seq), read_seq = excluded.read_seq,
           unread = excluded.unread, mentions = excluded.mentions, summary_seq = excluded.summary_seq",
        params![
            g.id,
            rows::enum_str(&g.kind)?,
            g.name,
            serde_json::to_string(&g.member_ids)?,
            g.created_by,
            g.created_at,
            s.last_seq,
            s.read_seq.max(local_read),
            s.unread,
            s.mentions
        ],
    )?;
    let has_topic_cursors: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM read_cursors WHERE group_id = ?1 AND topic_id != '' AND seq > ?2)",
        params![g.id, s.read_seq],
        |r| r.get(0),
    )?;
    if local_read > s.read_seq || has_topic_cursors {
        recount_unread(tx, me, &g.id)?;
    }
    Ok(())
}

fn remove_group(tx: &Transaction<'_>, id: &str) -> StoreResult<()> {
    for sql in [
        "DELETE FROM messages WHERE group_id = ?1",
        "DELETE FROM topics WHERE group_id = ?1",
        "DELETE FROM sync_state WHERE group_id = ?1",
        "DELETE FROM read_cursors WHERE group_id = ?1",
        "DELETE FROM outbox WHERE group_id = ?1",
        "DELETE FROM groups WHERE id = ?1",
    ] {
        tx.execute(sql, [id])?;
    }
    Ok(())
}

/// Recomputes unread and mention counts of a group from the cache and the cursors.
fn recount_unread(tx: &Transaction<'_>, me: &str, group_id: &str) -> StoreResult<()> {
    let mention_pat = format!("%\"{me}\"%");
    tx.execute(
        "UPDATE groups SET
           unread = (SELECT COUNT(*) FROM messages m
                     LEFT JOIN read_cursors rc ON rc.group_id = m.group_id AND rc.topic_id = m.topic_id
                     WHERE m.group_id = ?1 AND m.author_id != ?2
                       AND m.created_seq > groups.read_seq AND m.created_seq > COALESCE(rc.seq, 0)),
           mentions = (SELECT COUNT(*) FROM messages m
                     LEFT JOIN read_cursors rc ON rc.group_id = m.group_id AND rc.topic_id = m.topic_id
                     WHERE m.group_id = ?1 AND m.author_id != ?2 AND m.mentions LIKE ?3
                       AND m.created_seq > groups.read_seq AND m.created_seq > COALESCE(rc.seq, 0)),
           summary_seq = last_seq
         WHERE id = ?1",
        params![group_id, me, mention_pat],
    )?;
    Ok(())
}

fn insert_message_if_absent(tx: &Transaction<'_>, m: &Message) -> StoreResult<usize> {
    Ok(tx.execute(
        &format!(
            "INSERT OR IGNORE INTO messages ({}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            rows::MESSAGE_COLS
        ),
        rusqlite::params_from_iter(rows::message_values(m)?),
    )?)
}

fn insert_topic_if_absent(tx: &Transaction<'_>, t: &Topic, activity: u64) -> StoreResult<()> {
    tx.execute(
        "INSERT OR IGNORE INTO topics (id, group_id, title, status, created_by, created_at, root_message_id, last_seq, message_count, last_activity_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            t.id,
            t.group_id,
            t.title,
            rows::enum_str(&t.status)?,
            t.created_by,
            t.created_at,
            t.root_message_id,
            t.last_seq,
            t.message_count,
            activity.max(t.created_at)
        ],
    )?;
    Ok(())
}

fn apply_one(tx: &Transaction<'_>, me: &str, ev: &Event, out: &mut ApplyOutcome) -> StoreResult<()> {
    let gid = &ev.group_id;
    match &ev.data {
        EventData::TopicCreated(d) => {
            let topic = &d.topic; {
            tx.execute(
                "INSERT INTO topics (id, group_id, title, status, created_by, created_at, root_message_id, last_seq, message_count, last_activity_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(id) DO UPDATE SET title = excluded.title, status = excluded.status,
                   last_seq = MAX(topics.last_seq, excluded.last_seq),
                   message_count = MAX(topics.message_count, excluded.message_count),
                   last_activity_at = MAX(topics.last_activity_at, excluded.last_activity_at)",
                params![
                    topic.id,
                    topic.group_id,
                    topic.title,
                    rows::enum_str(&topic.status)?,
                    topic.created_by,
                    topic.created_at,
                    topic.root_message_id,
                    ev.seq.max(topic.last_seq),
                    topic.message_count,
                    ev.at
                ],
            )?;
            touch_group(tx, gid, ev.at)?;
            out.topic_list_changed = true;
            }
        }
        EventData::TopicUpdated(TopicUpdatedData { topic_id, title, status }) => {
            let status = match status {
                Some(s) => Some(rows::enum_str::<TopicStatus>(s)?),
                None => None,
            };
            tx.execute(
                "UPDATE topics SET title = COALESCE(?2, title), status = COALESCE(?3, status), last_seq = MAX(last_seq, ?4)
                 WHERE id = ?1",
                params![topic_id, title, status, ev.seq],
            )?;
            out.topic_list_changed = true;
        }
        EventData::MessageCreated(d) => {
            let message = &d.message;
            let m = message;
            tx.execute(
                &format!(
                    "INSERT INTO messages ({}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                     ON CONFLICT(id) DO UPDATE SET body = excluded.body, mentions = excluded.mentions,
                       created_seq = excluded.created_seq, edited_at = excluded.edited_at,
                       deleted_at = excluded.deleted_at, streaming = excluded.streaming",
                    rows::MESSAGE_COLS
                ),
                rusqlite::params_from_iter(rows::message_values(m)?),
            )?;
            // The topic snapshot in topic.created already counts its root message.
            let root: bool = tx
                .query_row("SELECT root_message_id = ?2 FROM topics WHERE id = ?1", params![m.topic_id, m.id], |r| r.get(0))
                .optional()?
                .unwrap_or(false);
            if root {
                tx.execute(
                    "UPDATE topics SET message_count = MAX(message_count, 1), last_seq = MAX(last_seq, ?2),
                       last_activity_at = MAX(last_activity_at, ?3) WHERE id = ?1",
                    params![m.topic_id, ev.seq, m.created_at],
                )?;
            } else {
                tx.execute(
                    "UPDATE topics SET message_count = message_count + 1, last_seq = MAX(last_seq, ?2),
                       last_activity_at = MAX(last_activity_at, ?3) WHERE id = ?1",
                    params![m.topic_id, ev.seq, m.created_at],
                )?;
            }
            // Unread counters: only events newer than the server summary count.
            if m.author_id != me {
                let mention = m.mentions.iter().any(|x| x == me) as u64;
                let topic_read: u64 = tx
                    .query_row(
                        "SELECT seq FROM read_cursors WHERE group_id = ?1 AND topic_id = ?2",
                        params![gid, m.topic_id],
                        |r| r.get(0),
                    )
                    .optional()?
                    .unwrap_or(0);
                tx.execute(
                    "UPDATE groups SET unread = unread + 1, mentions = mentions + ?3
                     WHERE id = ?1 AND ?2 > summary_seq AND ?2 > read_seq AND ?2 > ?4",
                    params![gid, ev.seq, mention, topic_read],
                )?;
            } else {
                out.my_client_msg_ids.push(m.client_msg_id.clone());
                tx.execute("DELETE FROM outbox WHERE id = ?1 AND kind = 'send'", [&m.client_msg_id])?;
            }
            touch_group(tx, gid, m.created_at)?;
            out.created.push((m.topic_id.clone(), m.author_id.clone()));
            out.message_topics.insert(m.topic_id.clone());
            out.topic_list_changed = true;
            out.group_changed = true;
        }
        EventData::MessageUpdated(MessageUpdatedData { message_id, body, edited_at, streaming }) => {
            out.replaced_bodies.push(message_id.clone());
            let topic: Option<String> = tx
                .query_row(
                    "UPDATE messages SET body = ?2, edited_at = ?3, streaming = ?4 WHERE id = ?1 AND deleted_at IS NULL
                     RETURNING topic_id",
                    params![message_id, body, edited_at, streaming],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(t) = topic {
                tx.execute("UPDATE topics SET last_seq = MAX(last_seq, ?2) WHERE id = ?1", params![t, ev.seq])?;
                out.message_topics.insert(t);
            }
        }
        EventData::MessageDeleted(MessageDeletedData { message_id, deleted_at }) => {
            out.replaced_bodies.push(message_id.clone());
            let topic: Option<String> = tx
                .query_row(
                    "UPDATE messages SET body = '', deleted_at = ?2, streaming = 0 WHERE id = ?1 RETURNING topic_id",
                    params![message_id, deleted_at],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(t) = topic {
                out.message_topics.insert(t);
            }
        }
        EventData::GroupUpdated(d) => {
            let name = &d.name;
            tx.execute("UPDATE groups SET name = ?2 WHERE id = ?1", params![gid, name])?;
            out.group_changed = true;
        }
        EventData::GroupMemberAdded(GroupMemberChangeData { member_id })
        | EventData::GroupMemberRemoved(GroupMemberChangeData { member_id }) => {
            let add = matches!(ev.data, EventData::GroupMemberAdded(_));
            let ids: Option<String> = tx
                .query_row("SELECT member_ids FROM groups WHERE id = ?1", [gid], |r| r.get(0))
                .optional()?;
            if let Some(ids) = ids {
                let mut v: Vec<String> = serde_json::from_str(&ids)?;
                v.retain(|x| x != member_id);
                if add {
                    v.push(member_id.clone());
                }
                tx.execute(
                    "UPDATE groups SET member_ids = ?2 WHERE id = ?1",
                    params![gid, serde_json::to_string(&v)?],
                )?;
            }
            out.group_changed = true;
        }
    }
    Ok(())
}

fn touch_group(tx: &Transaction<'_>, gid: &str, at: u64) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE groups SET last_activity_at = MAX(last_activity_at, ?2) WHERE id = ?1",
        params![gid, at],
    )?;
    Ok(())
}

