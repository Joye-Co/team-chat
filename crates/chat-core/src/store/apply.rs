//! Writes of server state: directory, read states and the group event log.

use super::rows::{self, clear_deleted, enum_str};
use super::{ApplyOutcome, DirectoryOutcome, LogEntry, Store, StoreResult};
use crate::proto::{
    Event, EventData, GroupMemberChangeData, GroupSummary, Member, Message, MessageDeletedData,
    MessageKind, MessageUpdatedData, ReadsFrame, SendResult, Topic, TopicState, TopicUpdatedData,
};
use rusqlite::{OptionalExtension, Transaction, params};
use std::collections::BTreeSet;

impl Store {
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
            let mut out = DirectoryOutcome { members_changed: true, ..Default::default() };
            upsert_member(tx, me)?;
            for m in members {
                upsert_member(tx, m)?;
            }
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
                    if m.id == me {
                        tx.execute(
                            "UPDATE meta SET value = ?1 WHERE key = 'me'",
                            [serde_json::to_string(m)?],
                        )?;
                    }
                }
                out.members_changed = !ms.is_empty();
            }
            for g in groups.unwrap_or_default() {
                upsert_group_summary(tx, &me, g)?;
                out.groups_changed.insert(g.group.id.clone());
            }
            for id in removed.unwrap_or_default() {
                remove_group(tx, id)?;
                out.groups_removed.push(id.clone());
            }
            Ok(out)
        })
    }

    /// Applies a `reads` frame. `received_seq` is the newest seq of the group this
    /// connection received before the frame (applied or buffered): the totals include
    /// those events, so only newer events count locally.
    pub fn apply_reads(&self, f: &ReadsFrame, received_seq: u64) -> StoreResult<()> {
        self.with_writer(|tx| {
            tx.execute(
                "UPDATE groups SET read_seq = MAX(read_seq, ?2), unread = ?3, mentions = ?4,
                   summary_seq = MAX(summary_seq, ?5) WHERE id = ?1",
                params![f.group_id, f.read_seq, f.unread, f.mentions, received_seq],
            )?;
            for t in &f.topics {
                upsert_topic_state(tx, &f.group_id, t)?;
            }
            Ok(())
        })
    }

    /// Stores my state of one topic (the ack of `topic.follow` or `topic.mute`).
    pub fn apply_topic_state(&self, group_id: &str, t: &TopicState) -> StoreResult<()> {
        self.with_writer(|tx| upsert_topic_state(tx, group_id, t))
    }

    /// Applies events of one group strictly in seq order: events at or below
    /// `lastApplied` are skipped, and application stops at the first gap.
    /// `events` may be in any order and contain duplicates.
    pub fn apply_events(&self, group_id: &str, events: &[Event]) -> StoreResult<ApplyOutcome> {
        let entries: Vec<LogEntry> = events.iter().cloned().map(LogEntry::Known).collect();
        self.apply_log(group_id, &entries)
    }

    /// Like `apply_events`, for log entries that may include events this client cannot
    /// read: those only advance `lastApplied`.
    pub fn apply_log(&self, group_id: &str, entries: &[LogEntry]) -> StoreResult<ApplyOutcome> {
        let me = self.me_id()?.unwrap_or_default();
        let mut sorted: Vec<&LogEntry> = entries
            .iter()
            .filter(|e| e.group_id() == group_id)
            .collect();
        sorted.sort_by_key(|e| e.seq());
        self.with_writer(|tx| {
            let mut last: u64 = tx
                .query_row(
                    "SELECT last_applied FROM sync_state WHERE group_id = ?1",
                    [group_id],
                    |r| r.get(0),
                )
                .optional()?
                .unwrap_or(0);
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
    pub fn insert_history(
        &self,
        topic_id: &str,
        messages: &[Message],
        has_more: bool,
    ) -> StoreResult<usize> {
        self.with_writer(|tx| {
            let mut n = 0;
            for m in messages {
                n += insert_message_if_absent(tx, m)?;
            }
            if !has_more {
                tx.execute(
                    "UPDATE topics SET history_complete = 1 WHERE id = ?1",
                    [topic_id],
                )?;
            }
            Ok(n)
        })
    }

    /// Records a successful `send` ack: stores the message and topic when they are not
    /// cached yet, and removes the outbox row. Counters change only through events.
    pub fn ack_send(&self, client_msg_id: &str, result: &SendResult) -> StoreResult<()> {
        self.with_writer(|tx| {
            insert_message_if_absent(tx, &result.message)?;
            if let Some(t) = &result.topic {
                insert_topic(tx, t, result.message.created_at, false)?;
            }
            tx.execute("DELETE FROM outbox WHERE id = ?1", [client_msg_id])?;
            resolve_followups(tx, client_msg_id, &result.message.topic_id)?;
            Ok(())
        })
    }
}

/// Gives the follow-ups of the send `root` its topic id: they become plain sends into it.
pub(super) fn resolve_followups(
    tx: &Transaction<'_>,
    root: &str,
    topic_id: &str,
) -> rusqlite::Result<usize> {
    tx.execute(
        "UPDATE outbox SET topic_id = ?2,
           payload = json_set(json_remove(payload, '$.after_root'), '$.topic_id', ?2)
         WHERE kind = 'send' AND json_extract(payload, '$.after_root') = ?1",
        params![root, topic_id],
    )
}

fn upsert_member(tx: &Transaction<'_>, m: &Member) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO members (id, kind, name, role, level, avatar_url, deactivated, character, reports_to, weekly_budget_cents)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(id) DO UPDATE SET kind = excluded.kind, name = excluded.name, role = excluded.role,
           level = excluded.level, avatar_url = excluded.avatar_url, deactivated = excluded.deactivated,
           character = excluded.character, reports_to = excluded.reports_to,
           weekly_budget_cents = excluded.weekly_budget_cents",
        params![
            m.id,
            enum_str(&m.kind)?,
            m.name,
            m.role(),
            enum_str(&m.level)?,
            m.avatar_url(),
            m.is_deactivated(),
            m.character().map(|c| c.as_str()),
            m.reports_to,
            m.weekly_budget_cents.and_then(|c| i64::try_from(c).ok())
        ],
    )?;
    Ok(())
}

fn upsert_topic_state(tx: &Transaction<'_>, group_id: &str, t: &TopicState) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO read_cursors (group_id, topic_id, seq, synced, following, muted) VALUES (?1, ?2, ?3, 1, ?4, ?5)
         ON CONFLICT(group_id, topic_id) DO UPDATE SET following = excluded.following, muted = excluded.muted,
           synced = CASE WHEN excluded.seq >= read_cursors.seq THEN 1 ELSE read_cursors.synced END,
           seq = MAX(read_cursors.seq, excluded.seq)",
        params![group_id, t.topic_id, t.read_seq, t.is_following(), t.is_muted()],
    )?;
    Ok(())
}

fn upsert_group_summary(tx: &Transaction<'_>, me: &str, s: &GroupSummary) -> StoreResult<()> {
    let g = &s.group;
    // A local read cursor may be ahead of the server (a read not yet sent).
    let local_read: u64 = tx
        .query_row(
            "SELECT seq FROM read_cursors WHERE group_id = ?1 AND topic_id = ''",
            [&g.id],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(0);
    tx.execute(
        "INSERT INTO groups (id, kind, name, member_ids, created_by, created_at, last_seq, read_seq, unread, mentions,
                             summary_seq, muted, notify, purpose)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?7, ?11, ?12, ?13)
         ON CONFLICT(id) DO UPDATE SET kind = excluded.kind, name = excluded.name, member_ids = excluded.member_ids,
           purpose = excluded.purpose,
           created_by = excluded.created_by, created_at = excluded.created_at,
           last_seq = MAX(groups.last_seq, excluded.last_seq), read_seq = excluded.read_seq,
           unread = excluded.unread, mentions = excluded.mentions, summary_seq = excluded.summary_seq,
           muted = excluded.muted, notify = excluded.notify",
        params![
            g.id,
            enum_str(&g.kind)?,
            g.name,
            serde_json::to_string(&g.member_ids)?,
            g.created_by,
            g.created_at,
            s.last_seq,
            s.read_seq.max(local_read),
            s.unread,
            s.mentions,
            s.muted.unwrap_or(false),
            match &s.notify {
                Some(n) => Some(enum_str(n)?),
                None => None,
            },
            g.purpose,
        ],
    )?;
    if let Some(states) = &s.topics {
        // Listed topics carry their state; a topic that is not listed has nothing
        // unread up to lastSeq and is neither followed nor muted.
        let listed: BTreeSet<&str> = states.iter().map(|t| t.topic_id.as_str()).collect();
        let cached: Vec<String> = {
            let mut st = tx.prepare("SELECT id FROM topics WHERE group_id = ?1")?;
            st.query_map([&g.id], |r| r.get(0))?
                .collect::<Result<_, _>>()?
        };
        for t in states {
            upsert_topic_state(tx, &g.id, t)?;
        }
        for tid in cached.iter().filter(|t| !listed.contains(t.as_str())) {
            upsert_topic_state(
                tx,
                &g.id,
                &TopicState {
                    topic_id: tid.clone(),
                    read_seq: s.last_seq,
                    unread: 0,
                    mentions: 0,
                    following: None,
                    muted: None,
                },
            )?;
        }
    }
    let unsynced: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM read_cursors WHERE group_id = ?1 AND synced = 0)",
        [&g.id],
        |r| r.get(0),
    )?;
    if local_read > s.read_seq || unsynced {
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

/// Recomputes the unread and mention totals of a group from the cache and the cursors,
/// with the rule of docs/protocol.md (not mine, not deleted, not system).
pub(super) fn recount_unread(tx: &Transaction<'_>, me: &str, group_id: &str) -> StoreResult<()> {
    let sql = format!(
        "UPDATE groups SET
           unread = (SELECT COUNT(*) FROM messages m
                     LEFT JOIN read_cursors rc ON rc.group_id = m.group_id AND rc.topic_id = m.topic_id
                     WHERE m.group_id = ?1 AND {cond}
                       AND m.created_seq > groups.read_seq AND m.created_seq > COALESCE(rc.seq, 0)),
           mentions = (SELECT COUNT(*) FROM messages m
                     LEFT JOIN read_cursors rc ON rc.group_id = m.group_id AND rc.topic_id = m.topic_id
                     WHERE m.group_id = ?1 AND {cond} AND m.mentions LIKE ?3
                       AND m.created_seq > groups.read_seq AND m.created_seq > COALESCE(rc.seq, 0)),
           summary_seq = last_seq
         WHERE id = ?1",
        cond = rows::UNREAD_COND
    );
    tx.execute(&sql, params![group_id, me, format!("%\"{me}\"%")])?;
    Ok(())
}

/// Takes the messages of one topic with `after < createdSeq <= upto` (newly read) off the
/// group's unread and mention totals. Uses the `messages_by_topic` index.
pub(super) fn subtract_topic_read(
    tx: &Transaction<'_>,
    me: &str,
    group_id: &str,
    topic_id: &str,
    after: u64,
    upto: u64,
) -> StoreResult<()> {
    if upto <= after {
        return Ok(());
    }
    let sql = format!(
        "SELECT COUNT(*), COALESCE(SUM(m.mentions LIKE ?5), 0) FROM messages m
         WHERE m.topic_id = ?1 AND m.group_id = ?6 AND {cond}
           AND m.created_seq > ?3 AND m.created_seq <= ?4",
        cond = rows::UNREAD_COND
    );
    let (read, mentioned): (u64, u64) = tx.query_row(
        &sql,
        params![topic_id, me, after, upto, format!("%\"{me}\"%"), group_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if read > 0 {
        tx.execute(
            "UPDATE groups SET unread = MAX(unread - ?2, 0), mentions = MAX(mentions - ?3, 0)
             WHERE id = ?1",
            params![group_id, read, mentioned],
        )?;
    }
    Ok(())
}

fn insert_message_if_absent(tx: &Transaction<'_>, m: &Message) -> StoreResult<usize> {
    Ok(tx.execute(
        &format!(
            "INSERT OR IGNORE INTO messages ({}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            rows::MESSAGE_COLS
        ),
        rusqlite::params_from_iter(rows::message_values(m)?),
    )?)
}

/// Inserts a topic snapshot; with `replace`, a cached row takes the snapshot's fields.
fn insert_topic(tx: &Transaction<'_>, t: &Topic, activity: u64, replace: bool) -> StoreResult<()> {
    let conflict = if replace {
        "ON CONFLICT(id) DO UPDATE SET title = excluded.title, status = excluded.status,
           last_seq = MAX(topics.last_seq, excluded.last_seq),
           message_count = MAX(topics.message_count, excluded.message_count),
           last_activity_at = MAX(topics.last_activity_at, excluded.last_activity_at),
           owner_id = excluded.owner_id, waiting_on = excluded.waiting_on,
           last_message_at = COALESCE(MAX(topics.last_message_at, excluded.last_message_at), excluded.last_message_at),
           raw = excluded.raw"
    } else {
        "ON CONFLICT(id) DO NOTHING"
    };
    tx.execute(
        &format!(
            "INSERT INTO topics (id, group_id, title, status, created_by, created_at, root_message_id, last_seq,
                                 message_count, last_activity_at, owner_id, waiting_on, last_message_at, raw)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14) {conflict}"
        ),
        params![
            t.id,
            t.group_id,
            t.title,
            enum_str(&t.status)?,
            t.created_by,
            t.created_at,
            t.root_message_id,
            t.last_seq,
            t.message_count,
            activity.max(t.created_at),
            t.owner_id.clone().flatten(),
            serde_json::to_string(t.waiting_on.as_deref().unwrap_or_default())?,
            t.last_message_at,
            serde_json::to_string(t)?
        ],
    )?;
    Ok(())
}

fn update_message_raw(
    tx: &Transaction<'_>,
    id: &str,
    f: impl FnOnce(&mut Message),
) -> StoreResult<()> {
    let raw: Option<String> = tx
        .query_row("SELECT raw FROM messages WHERE id = ?1", [id], |r| r.get(0))
        .optional()?;
    if let Some(raw) = raw {
        let mut m: Message = serde_json::from_str(&raw)?;
        f(&mut m);
        tx.execute(
            "UPDATE messages SET raw = ?2 WHERE id = ?1",
            params![id, serde_json::to_string(&m)?],
        )?;
    }
    Ok(())
}

fn apply_one(
    tx: &Transaction<'_>,
    me: &str,
    ev: &Event,
    out: &mut ApplyOutcome,
) -> StoreResult<()> {
    let gid = &ev.group_id;
    match &ev.data {
        EventData::TopicCreated(d) => {
            let mut t = d.topic.clone();
            t.last_seq = t.last_seq.max(ev.seq);
            insert_topic(tx, &t, ev.at, true)?;
            touch_group(tx, gid, ev.at)?;
            out.topic_list_changed = true;
        }
        EventData::TopicUpdated(TopicUpdatedData {
            topic_id,
            title,
            status,
            waiting_on,
            owner_id,
            last_message_at,
            context_of,
        }) => {
            if let Some(c) = context_of {
                super::v11::set_topic_context(tx, topic_id, c)?;
            }
            let status = match status {
                Some(s) => Some(enum_str(s)?),
                None => None,
            };
            let waiting = match waiting_on {
                Some(w) => Some(serde_json::to_string(w)?),
                None => None,
            };
            tx.execute(
                "UPDATE topics SET title = COALESCE(?2, title), status = COALESCE(?3, status),
                   waiting_on = COALESCE(?4, waiting_on),
                   owner_id = CASE WHEN ?5 THEN ?6 ELSE owner_id END,
                   last_message_at = CASE WHEN ?7 THEN ?8 ELSE last_message_at END,
                   last_seq = MAX(last_seq, ?9)
                 WHERE id = ?1",
                params![
                    topic_id,
                    title,
                    status,
                    waiting,
                    owner_id.is_some(),
                    owner_id.clone().flatten(),
                    last_message_at.is_some(),
                    last_message_at,
                    ev.seq
                ],
            )?;
            out.topic_list_changed = true;
        }
        EventData::MessageCreated(d) => {
            let m = &d.message;
            tx.execute(
                &format!(
                    "INSERT INTO messages ({}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                     ON CONFLICT(id) DO UPDATE SET body = excluded.body, mentions = excluded.mentions,
                       created_seq = excluded.created_seq, edited_at = excluded.edited_at,
                       deleted_at = excluded.deleted_at, streaming = excluded.streaming,
                       kind = excluded.kind, raw = excluded.raw",
                    rows::MESSAGE_COLS
                ),
                rusqlite::params_from_iter(rows::message_values(m)?),
            )?;
            // The topic snapshot in topic.created already counts its root message.
            let root: bool = tx
                .query_row(
                    "SELECT root_message_id = ?2 FROM topics WHERE id = ?1",
                    params![m.topic_id, m.id],
                    |r| r.get(0),
                )
                .optional()?
                .unwrap_or(false);
            tx.execute(
                "UPDATE topics SET message_count = CASE WHEN ?4 THEN MAX(message_count, 1) ELSE message_count + 1 END,
                   last_seq = MAX(last_seq, ?2), last_activity_at = MAX(last_activity_at, ?3),
                   last_message_at = MAX(COALESCE(last_message_at, 0), ?3)
                 WHERE id = ?1",
                params![m.topic_id, ev.seq, m.created_at, root],
            )?;
            let system = m.kind == Some(MessageKind::System);
            if m.author_id != me && !system && !m.is_deleted() {
                // Unread totals: only events newer than the server summary count.
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
                    params![gid, m.created_seq, mention, topic_read],
                )?;
            }
            if m.author_id == me {
                out.my_client_msg_ids.push(m.client_msg_id.clone());
                let removed = tx.execute(
                    "DELETE FROM outbox WHERE id = ?1 AND kind = 'send'",
                    [&m.client_msg_id],
                )?;
                resolve_followups(tx, &m.client_msg_id, &m.topic_id)?;
                if removed > 0 {
                    // Applied before its ack: the ack may never come (the socket can drop).
                    out.sent.push(crate::proto::SendResult {
                        message: m.clone(),
                        topic: None,
                        duplicate: None,
                    });
                }
            }
            touch_group(tx, gid, m.created_at)?;
            out.created.push((m.topic_id.clone(), m.author_id.clone()));
            out.message_topics.insert(m.topic_id.clone());
            out.topic_list_changed = true;
            out.group_changed = true;
        }
        EventData::MessageUpdated(MessageUpdatedData {
            message_id,
            body,
            edited_at,
            streaming,
            attachments,
            artifact_refs,
            card,
            card_v2,
            card_state,
            cause,
            block_refs,
        }) => {
            out.replaced_bodies.push(message_id.clone());
            // A server-made state update (a card answer, retract or expiry; a cleared ref) has
            // no `cardV2` and carries `cardState` or `blockRefs`: not an edit, so the stored
            // `edited_at` stays (docs/protocol.md, "Edit marks").
            let state_only = card_v2.is_none() && (card_state.is_some() || block_refs.is_some());
            let topic: Option<String> = tx
                .query_row(
                    "UPDATE messages SET body = ?2, edited_at = CASE WHEN ?5 THEN edited_at ELSE ?3 END, streaming = ?4
                     WHERE id = ?1 AND deleted_at IS NULL
                     RETURNING topic_id",
                    params![message_id, body, edited_at, streaming, state_only],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(t) = topic {
                if attachments.is_some()
                    || artifact_refs.is_some()
                    || card.is_some()
                    || card_v2.is_some()
                    || card_state.is_some()
                    || cause.is_some()
                    || block_refs.is_some()
                {
                    update_message_raw(tx, message_id, |m| {
                        if let Some(a) = attachments {
                            m.attachments = Some(a.clone());
                        }
                        if let Some(a) = artifact_refs {
                            m.artifact_refs = Some(a.clone());
                        }
                        if let Some(c) = card {
                            m.card = Some(c.clone());
                        }
                        if let Some(c) = card_v2 {
                            m.card_v2 = Some(c.clone());
                        }
                        if let Some(c) = card_state {
                            m.card_state = Some(c.clone());
                        }
                        if let Some(c) = cause {
                            m.cause = Some(c.clone());
                        }
                        if let Some(r) = block_refs {
                            m.block_refs = Some(r.clone());
                        }
                    })?;
                }
                tx.execute(
                    "UPDATE topics SET last_seq = MAX(last_seq, ?2) WHERE id = ?1",
                    params![t, ev.seq],
                )?;
                out.message_topics.insert(t);
            }
        }
        EventData::MessageDeleted(MessageDeletedData {
            message_id,
            deleted_at,
        }) => {
            out.replaced_bodies.push(message_id.clone());
            let topic: Option<String> = tx
                .query_row(
                    "UPDATE messages SET body = '', deleted_at = ?2, streaming = 0 WHERE id = ?1 RETURNING topic_id",
                    params![message_id, deleted_at],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(t) = topic {
                update_message_raw(tx, message_id, clear_deleted)?;
                out.message_topics.insert(t);
            }
        }
        EventData::GroupUpdated(d) => {
            tx.execute(
                "UPDATE groups SET name = ?2 WHERE id = ?1",
                params![gid, d.name],
            )?;
            if let Some(p) = &d.purpose {
                tx.execute(
                    "UPDATE groups SET purpose = ?2 WHERE id = ?1",
                    params![gid, (!p.is_empty()).then_some(p)],
                )?;
            }
            out.group_changed = true;
        }
        EventData::DeskNoted(d) => {
            super::v11::apply_desk_note(tx, gid, &ev.actor_id, &d.text, ev.at, ev.seq)?;
            out.group_changed = true;
            out.other_events.push(ev.clone());
        }
        EventData::GroupMemberAdded(GroupMemberChangeData { member_id })
        | EventData::GroupMemberRemoved(GroupMemberChangeData { member_id }) => {
            let add = matches!(ev.data, EventData::GroupMemberAdded(_));
            let ids: Option<String> = tx
                .query_row("SELECT member_ids FROM groups WHERE id = ?1", [gid], |r| {
                    r.get(0)
                })
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
        EventData::ReactionChanged(d) => {
            let topic: Option<String> = tx
                .query_row(
                    "SELECT topic_id FROM messages WHERE id = ?1",
                    [&d.message_id],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(t) = topic {
                update_message_raw(tx, &d.message_id, |m| {
                    let mut list = m.reactions.take().unwrap_or_default();
                    crate::proto::apply_reaction(&mut list, &d.emoji, &ev.actor_id, d.on);
                    m.reactions = (!list.is_empty()).then_some(list);
                })?;
                out.message_topics.insert(t);
            }
        }
        EventData::CardActioned(d) => {
            super::ext::insert_card_action(tx, ev, d)?;
            if ev.actor_id == me {
                // My press reached the server: drop the sent outbox row for it, so a lost ack
                // does not send the press again after a reconnect.
                let row: Option<(String, Option<String>)> = tx
                    .query_row(
                        "SELECT id, topic_id FROM outbox
                         WHERE kind = 'card_action' AND group_id = ?1 AND attempts > 0
                           AND json_extract(payload, '$.message_id') = ?2
                           AND json_extract(payload, '$.action_id') = ?3
                         ORDER BY ord LIMIT 1",
                        params![gid, d.message_id, d.action_id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                if let Some((id, topic_id)) = row {
                    tx.execute("DELETE FROM outbox WHERE id = ?1", [&id])?;
                    if let Some(t) = topic_id {
                        out.message_topics.insert(t);
                    }
                }
            }
            out.other_events.push(ev.clone());
        }
        EventData::RunUpdated(d) => {
            super::ext::upsert_run(tx, &d.run)?;
            out.run_topics.insert(d.run.topic_id.clone());
            out.other_events.push(ev.clone());
        }
        EventData::ArtifactCreated(d) | EventData::ArtifactVersionAdded(d) => {
            super::ext::upsert_artifact(tx, &d.artifact, std::slice::from_ref(&d.version))?;
            out.artifact_topics.insert(d.artifact.topic_id.clone());
            out.other_events.push(ev.clone());
        }
        EventData::CardAnswered(d) => {
            super::v11::insert_card_answer(tx, ev, d)?;
            if ev.actor_id == me {
                // My answer reached the server: drop its sent outbox row, so a lost ack does
                // not send it again after a reconnect.
                tx.execute(
                    "DELETE FROM outbox WHERE kind = 'card_answer' AND group_id = ?1 AND attempts > 0
                       AND json_extract(payload, '$.message_id') = ?2
                       AND json_extract(payload, '$.action_id') = ?3
                       AND json_extract(payload, '$.rev') = ?4",
                    params![gid, d.message_id, d.action_id, d.rev],
                )?;
                tx.execute(
                    "DELETE FROM outbox WHERE kind = 'card_event' AND group_id = ?1 AND attempts > 0
                       AND json_extract(payload, '$.message_id') = ?2
                       AND json_extract(payload, '$.rev') = ?3
                       AND json_extract(payload, '$.event') != 'answer.retract'",
                    params![gid, d.message_id, d.rev],
                )?;
            }
            if let Some(t) = super::v11::topic_of_message(tx, &d.message_id)? {
                out.message_topics.insert(t);
            }
            out.other_events.push(ev.clone());
        }
        EventData::CardRetracted(d) => {
            super::v11::retract_card_answer(tx, ev, d)?;
            if ev.actor_id == me {
                tx.execute(
                    "DELETE FROM outbox WHERE kind = 'card_event' AND group_id = ?1 AND attempts > 0
                       AND json_extract(payload, '$.message_id') = ?2
                       AND json_extract(payload, '$.event') = 'answer.retract'",
                    params![gid, d.message_id],
                )?;
            }
            if let Some(t) = super::v11::topic_of_message(tx, &d.message_id)? {
                out.message_topics.insert(t);
            }
            out.other_events.push(ev.clone());
        }
        EventData::ArtifactPreviewReady(d) => {
            if let Some(t) = super::v11::set_version_media(tx, d)? {
                out.artifact_topics.insert(t);
            }
            out.other_events.push(ev.clone());
        }
        EventData::TopicLinked(d) => {
            super::v11::upsert_topic_link(tx, ev, d)?;
            out.topic_list_changed = true;
            out.other_events.push(ev.clone());
        }
        // 记一条: the server lists these (`GET /v1/groups/:id/decisions`); nothing to cache.
        EventData::DecisionRecorded(_) => {
            out.other_events.push(ev.clone());
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
