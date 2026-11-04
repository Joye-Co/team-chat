//! 1.1 cache tables (migration `v11-1`): Card v2 answers, topic links, 产出 page events, and
//! the one-time log re-fetch that recovers what 1.0.x clients skipped (docs/protocol.md,
//! "Client cache recovery"). Queries for the desk: decisions, active runs, side chats.

use super::rows::{self, json_err};
use super::{ApplyOutcome, LogEntry, Store, StoreResult};
use crate::proto::{
    AnswerEdits, AnswerInputs, ArtifactPreviewReadyData, ArtifactVersion, BotRun, CardAnsweredData,
    CardRetractedData, CardV2, CardV2Block, Event, EventData, Message, RefTarget, TopicContext,
    TopicLink, TopicLinkedData,
};
use rusqlite::{OptionalExtension, Transaction, params};

pub(super) const MIGRATION_SQL: &str = r#"
CREATE TABLE card_answers (
  group_id TEXT NOT NULL,
  seq INTEGER NOT NULL,
  message_id TEXT NOT NULL,
  rev INTEGER NOT NULL,
  actor_id TEXT NOT NULL,
  action_id TEXT NOT NULL,
  inputs TEXT,
  edited_values TEXT,
  at INTEGER NOT NULL,
  PRIMARY KEY (group_id, seq)
);
CREATE INDEX card_answers_by_message ON card_answers(message_id, seq);
CREATE TABLE topic_links (
  topic_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  url TEXT NOT NULL,
  group_id TEXT NOT NULL,
  seq INTEGER NOT NULL,
  at INTEGER NOT NULL,
  raw TEXT NOT NULL,
  PRIMARY KEY (topic_id, kind, url)
);
CREATE TABLE artifact_events (
  group_id TEXT NOT NULL,
  seq INTEGER NOT NULL,
  artifact_id TEXT NOT NULL,
  version INTEGER NOT NULL,
  actor_id TEXT NOT NULL,
  name TEXT NOT NULL,
  payload TEXT,
  at INTEGER NOT NULL,
  PRIMARY KEY (group_id, seq)
);
CREATE INDEX artifact_events_by_artifact ON artifact_events(artifact_id, seq);
CREATE INDEX runs_by_updated ON runs(updated_at DESC);
CREATE INDEX topics_by_context ON topics(json_extract(raw, '$.contextOf.topicId'));
CREATE TABLE log_refetch (
  group_id TEXT PRIMARY KEY,
  upto INTEGER NOT NULL,
  after INTEGER NOT NULL DEFAULT 0
);
INSERT INTO log_refetch (group_id, upto) SELECT group_id, last_applied FROM sync_state WHERE last_applied > 0;
"#;

/// 1.1 step `v11-2`: answer ids and retracts of the interaction loop; page events are no
/// longer stored (they are transient), so their table goes.
pub(super) const MIGRATION_SQL_2: &str = r#"
ALTER TABLE card_answers ADD COLUMN answer_id TEXT;
ALTER TABLE card_answers ADD COLUMN ask_id TEXT;
ALTER TABLE card_answers ADD COLUMN retracted_at INTEGER;
CREATE INDEX card_answers_by_answer ON card_answers(message_id, answer_id);
DROP TABLE IF EXISTS artifact_events;
"#;

/// 1.1 step `v11-3`: the event, block and value of a 2.1 answer (an immediate commit has a
/// block and a value, no action and no inputs). A separate step, so caches that already ran
/// `v11-2` get the columns too.
pub(super) const MIGRATION_SQL_3: &str = r#"
ALTER TABLE card_answers ADD COLUMN event TEXT;
ALTER TABLE card_answers ADD COLUMN block_id TEXT;
ALTER TABLE card_answers ADD COLUMN value TEXT;
"#;

/// 1.1 server-files gaps: group purpose lines and desk notes (首页), synced from the log.
pub(super) const MIGRATION_SQL_4: &str = r#"
ALTER TABLE groups ADD COLUMN purpose TEXT;
CREATE TABLE desk_notes (
  group_id TEXT NOT NULL,
  member_id TEXT NOT NULL,
  text TEXT NOT NULL,
  at INTEGER NOT NULL,
  seq INTEGER NOT NULL,
  PRIMARY KEY (group_id, member_id)
);
"#;

/// A bot member's desk note in one group (`desk.noted`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeskNoteRow {
    pub group_id: String,
    pub member_id: String,
    pub text: String,
    pub at: u64,
    pub seq: u64,
}

/// Applies `desk.noted`: the newest note of the bot in the group; `""` removes it.
pub(super) fn apply_desk_note(
    tx: &Transaction<'_>,
    gid: &str,
    member_id: &str,
    text: &str,
    at: u64,
    seq: u64,
) -> rusqlite::Result<()> {
    if text.is_empty() {
        tx.execute(
            "DELETE FROM desk_notes WHERE group_id = ?1 AND member_id = ?2 AND seq <= ?3",
            params![gid, member_id, seq],
        )?;
    } else {
        tx.execute(
            "INSERT INTO desk_notes (group_id, member_id, text, at, seq) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(group_id, member_id) DO UPDATE SET text = excluded.text, at = excluded.at, seq = excluded.seq
             WHERE excluded.seq > desk_notes.seq",
            params![gid, member_id, text, at, seq],
        )?;
    }
    Ok(())
}

/// 1.1 step `asks-1`: the card messages that ask someone, newest first (需要你拍板 reads
/// every open ask, not only the newest messages of a waiting topic).
pub(super) const MIGRATION_SQL_ASKS: &str = r#"
CREATE INDEX messages_asks ON messages(created_at) WHERE json_extract(raw, '$.cardV2.ask') IS NOT NULL;
"#;

/// One answer to a v2 card (`card.answered`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardAnswerRow {
    pub group_id: String,
    pub seq: u64,
    pub message_id: String,
    pub rev: u32,
    pub actor_id: String,
    pub action_id: String,
    pub inputs: Option<AnswerInputs>,
    pub edited_values: Option<AnswerEdits>,
    pub at: u64,
    /// 2.1: the answer id and ask round (absent for 2.0 answers).
    pub answer_id: Option<String>,
    pub ask_id: Option<String>,
    /// 2.1: when the answerer took it back (`card.retracted`).
    pub retracted_at: Option<u64>,
    /// 2.1: the event that answered (`choice.submit`, `action.press`, …).
    pub event: Option<String>,
    /// 2.1: the block of an immediate commit, and its value (`{value}`, `{values}`, …).
    pub block_id: Option<String>,
    pub value: Option<serde_json::Value>,
}

/// A group decision (群里的决定): an answer to a v2 card whose `ask.record` is true.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionRow {
    pub group_id: String,
    pub topic_id: String,
    pub message_id: String,
    /// The card as it is now (title = the question).
    pub card: CardV2,
    pub answer: CardAnswerRow,
}

/// A group whose log the 1.1 cache recovery still has to re-read: `(after, upto]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refetch {
    pub after: u64,
    pub upto: u64,
}

fn json_opt<T: serde::Serialize>(v: &Option<T>) -> Result<Option<String>, serde_json::Error> {
    v.as_ref().map(serde_json::to_string).transpose()
}

fn parse_opt<T: serde::de::DeserializeOwned>(s: Option<String>) -> rusqlite::Result<Option<T>> {
    s.map(|s| serde_json::from_str(&s).map_err(json_err))
        .transpose()
}

pub(super) fn insert_card_answer(
    tx: &Transaction<'_>,
    ev: &Event,
    d: &CardAnsweredData,
) -> StoreResult<()> {
    tx.execute(
        "INSERT OR IGNORE INTO card_answers (group_id, seq, message_id, rev, actor_id, action_id, inputs, edited_values, at, answer_id, ask_id, event, block_id, value)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            ev.group_id,
            ev.seq,
            d.message_id,
            d.rev,
            ev.actor_id,
            d.action_id,
            json_opt(&d.inputs)?,
            json_opt(&d.edited_values)?,
            ev.at,
            d.answer_id,
            d.ask_id,
            d.event,
            d.block_id,
            json_opt(&d.value)?
        ],
    )?;
    Ok(())
}

/// Marks a retracted answer (`card.retracted`).
pub(super) fn retract_card_answer(
    tx: &Transaction<'_>,
    ev: &Event,
    d: &CardRetractedData,
) -> StoreResult<()> {
    tx.execute(
        "UPDATE card_answers SET retracted_at = ?3 WHERE message_id = ?1 AND answer_id = ?2",
        params![d.message_id, d.answer_id, ev.at],
    )?;
    Ok(())
}

/// Sets the media of a cached 产出 version (`artifact.previewReady`). Returns its topic.
pub(super) fn set_version_media(
    tx: &Transaction<'_>,
    d: &ArtifactPreviewReadyData,
) -> StoreResult<Option<String>> {
    tx.execute(
        "UPDATE artifact_versions SET raw = json_set(raw, '$.media', json(?3)) WHERE artifact_id = ?1 AND version = ?2",
        params![d.artifact_id, d.version, serde_json::to_string(&d.media)?],
    )?;
    Ok(tx
        .query_row(
            "SELECT topic_id FROM artifacts WHERE id = ?1",
            [&d.artifact_id],
            |r| r.get(0),
        )
        .optional()?)
}

/// Keeps the newest state per (topic, kind, url). Unknown link kinds are skipped.
pub(super) fn upsert_topic_link(
    tx: &Transaction<'_>,
    ev: &Event,
    d: &TopicLinkedData,
) -> StoreResult<()> {
    let Some((kind, url)) = d.link.key() else {
        return Ok(());
    };
    tx.execute(
        "INSERT INTO topic_links (topic_id, kind, url, group_id, seq, at, raw) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(topic_id, kind, url) DO UPDATE SET seq = excluded.seq, at = excluded.at, raw = excluded.raw
         WHERE excluded.seq > topic_links.seq",
        params![
            d.topic_id,
            kind,
            url,
            ev.group_id,
            ev.seq,
            ev.at,
            serde_json::to_string(&d.link)?
        ],
    )?;
    Ok(())
}

/// Sets `contextOf` in a cached topic's raw JSON.
pub(super) fn set_topic_context(
    tx: &Transaction<'_>,
    topic_id: &str,
    c: &TopicContext,
) -> StoreResult<()> {
    tx.execute(
        "UPDATE topics SET raw = json_set(raw, '$.contextOf', json(?2)) WHERE id = ?1",
        params![topic_id, serde_json::to_string(c)?],
    )?;
    Ok(())
}

/// Recovery only: sets `contextOf` unless the cached topic has one at the same or a newer
/// `uptoSeq` (applied by catch-up or a live event after `upto`).
fn recover_topic_context(
    tx: &Transaction<'_>,
    topic_id: &str,
    c: &TopicContext,
) -> StoreResult<bool> {
    let n = tx.execute(
        "UPDATE topics SET raw = json_set(raw, '$.contextOf', json(?2)) WHERE id = ?1
           AND (json_extract(raw, '$.contextOf') IS NULL
                OR json_extract(raw, '$.contextOf.uptoSeq') < ?3)",
        params![topic_id, serde_json::to_string(c)?, c.upto_seq],
    )?;
    Ok(n > 0)
}

/// Recovery only: sets `cardV2` of a cached message that is not deleted, unless it already
/// has a card at the same or a newer `rev`.
pub(super) fn set_message_card_v2(
    tx: &Transaction<'_>,
    message_id: &str,
    card: &CardV2,
) -> StoreResult<Option<String>> {
    let row: Option<(String, Option<u64>, String)> = tx
        .query_row(
            "SELECT raw, deleted_at, topic_id FROM messages WHERE id = ?1",
            [message_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((raw, deleted, topic)) = row else {
        return Ok(None);
    };
    if deleted.is_some() {
        return Ok(None);
    }
    let mut m: Message = serde_json::from_str(&raw)?;
    if m.card_v2.as_ref().is_some_and(|c| c.rev >= card.rev) {
        return Ok(None);
    }
    m.card_v2 = Some(card.clone());
    tx.execute(
        "UPDATE messages SET raw = ?2 WHERE id = ?1",
        params![message_id, serde_json::to_string(&m)?],
    )?;
    Ok(Some(topic))
}

/// Applies one event of the 1.1 recovery pass: only what is idempotent and what 1.0.x lost.
fn recover_one(tx: &Transaction<'_>, ev: &Event, out: &mut ApplyOutcome) -> StoreResult<()> {
    match &ev.data {
        EventData::MessageCreated(d) => {
            if let Some(c) = &d.message.card_v2
                && let Some(t) = set_message_card_v2(tx, &d.message.id, c)?
            {
                out.message_topics.insert(t);
            }
        }
        EventData::MessageUpdated(d) => {
            if let Some(c) = &d.card_v2
                && let Some(t) = set_message_card_v2(tx, &d.message_id, c)?
            {
                out.message_topics.insert(t);
            }
        }
        EventData::TopicCreated(d) => {
            if let Some(c) = &d.topic.context_of
                && recover_topic_context(tx, &d.topic.id, c)?
            {
                out.topic_list_changed = true;
            }
        }
        EventData::TopicUpdated(d) => {
            if let Some(c) = &d.context_of
                && recover_topic_context(tx, &d.topic_id, c)?
            {
                out.topic_list_changed = true;
            }
        }
        EventData::RunUpdated(d) => {
            super::ext::upsert_run(tx, &d.run)?;
            out.run_topics.insert(d.run.topic_id.clone());
        }
        EventData::ArtifactCreated(d) | EventData::ArtifactVersionAdded(d) => {
            // 1.0.x stored versions without runId and events; replace them.
            super::ext::upsert_artifact(tx, &d.artifact, &[])?;
            replace_version(tx, &d.version)?;
            out.artifact_topics.insert(d.artifact.topic_id.clone());
        }
        EventData::CardAnswered(d) => {
            insert_card_answer(tx, ev, d)?;
            if let Some(t) = topic_of_message(tx, &d.message_id)? {
                out.message_topics.insert(t);
            }
        }
        EventData::CardRetracted(d) => {
            retract_card_answer(tx, ev, d)?;
            if let Some(t) = topic_of_message(tx, &d.message_id)? {
                out.message_topics.insert(t);
            }
        }
        EventData::ArtifactPreviewReady(d) => {
            if let Some(t) = set_version_media(tx, d)? {
                out.artifact_topics.insert(t);
            }
        }
        EventData::TopicLinked(d) => {
            upsert_topic_link(tx, ev, d)?;
            out.topic_list_changed = true;
        }
        _ => {}
    }
    Ok(())
}

fn replace_version(tx: &Transaction<'_>, v: &ArtifactVersion) -> StoreResult<()> {
    tx.execute(
        "INSERT OR REPLACE INTO artifact_versions (artifact_id, version, raw) VALUES (?1, ?2, ?3)",
        params![v.artifact_id, v.version, serde_json::to_string(v)?],
    )?;
    Ok(())
}

pub(super) fn topic_of_message(
    tx: &Transaction<'_>,
    message_id: &str,
) -> rusqlite::Result<Option<String>> {
    tx.query_row(
        "SELECT topic_id FROM messages WHERE id = ?1",
        [message_id],
        |r| r.get(0),
    )
    .optional()
}

fn answer_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<CardAnswerRow> {
    Ok(CardAnswerRow {
        group_id: r.get(0)?,
        seq: r.get(1)?,
        message_id: r.get(2)?,
        rev: r.get(3)?,
        actor_id: r.get(4)?,
        action_id: r.get(5)?,
        inputs: parse_opt(r.get(6)?)?,
        edited_values: parse_opt(r.get(7)?)?,
        at: r.get(8)?,
        answer_id: r.get(9)?,
        ask_id: r.get(10)?,
        retracted_at: r.get(11)?,
        event: r.get(12)?,
        block_id: r.get(13)?,
        value: parse_opt(r.get(14)?)?,
    })
}

const ANSWER_COLS: &str = "a.group_id, a.seq, a.message_id, a.rev, a.actor_id, a.action_id, a.inputs, a.edited_values, a.at, a.answer_id, a.ask_id, a.retracted_at, a.event, a.block_id, a.value";

const ACTIVE_RUN: &str =
    "json_extract(raw, '$.status') IN ('created', 'running', 'waiting', 'blocked')";

fn run_from_raw(r: &rusqlite::Row<'_>) -> rusqlite::Result<BotRun> {
    serde_json::from_str::<BotRun>(&r.get::<_, String>(0)?).map_err(json_err)
}

impl Store {
    /// Answers to a v2 card message, oldest first.
    pub fn card_answers(&self, message_id: &str) -> StoreResult<Vec<CardAnswerRow>> {
        self.with_reader(|c| {
            let mut st = c.prepare_cached(&format!(
                "SELECT {ANSWER_COLS} FROM card_answers a WHERE a.message_id = ?1 ORDER BY a.seq"
            ))?;
            st.query_map([message_id], answer_from_row)?.collect()
        })
    }

    /// Group decisions (群里的决定), newest first: answers to v2 cards with `ask.record`.
    /// `group_id = None` lists every group.
    pub fn decisions(&self, group_id: Option<&str>, limit: usize) -> StoreResult<Vec<DecisionRow>> {
        self.with_reader(|c| {
            let mut st = c.prepare_cached(&format!(
                "SELECT {ANSWER_COLS}, m.topic_id, m.raw FROM card_answers a
                 JOIN messages m ON m.id = a.message_id
                 WHERE (?1 IS NULL OR a.group_id = ?1) AND m.deleted_at IS NULL AND a.retracted_at IS NULL
                   AND json_extract(m.raw, '$.cardV2.ask.record') = 1
                 ORDER BY a.at DESC, a.seq DESC LIMIT ?2"
            ))?;
            let rows = st.query_map(params![group_id, limit as u64], |r| {
                let answer = answer_from_row(r)?;
                let topic_id: String = r.get(15)?;
                let raw: String = r.get(16)?;
                Ok((answer, topic_id, raw))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (answer, topic_id, raw) = row?;
                let m: Message = serde_json::from_str(&raw).map_err(json_err)?;
                if let Some(card) = m.card_v2 {
                    out.push(DecisionRow {
                        group_id: answer.group_id.clone(),
                        topic_id,
                        message_id: answer.message_id.clone(),
                        card,
                        answer,
                    });
                }
            }
            Ok(out)
        })
    }

    /// Card v2 messages with an `ask` of every group, newest first, at most `limit`, not
    /// deleted. Whether an ask is open for a member is the caller's check.
    pub fn ask_messages(&self, limit: usize) -> StoreResult<Vec<Message>> {
        self.with_reader(|c| {
            let mut st = c.prepare_cached(&format!(
                "SELECT {} FROM messages INDEXED BY messages_asks
                 WHERE json_extract(raw, '$.cardV2.ask') IS NOT NULL AND deleted_at IS NULL
                 ORDER BY created_at DESC LIMIT ?1",
                rows::MESSAGE_COLS
            ))?;
            st.query_map([limit as u64], rows::message_from_row)?
                .collect()
        })
    }

    /// Active runs (created, running, waiting, blocked) of every group, newest change first.
    pub fn active_runs(&self) -> StoreResult<Vec<BotRun>> {
        self.with_reader(|c| {
            let mut st = c.prepare_cached(&format!(
                "SELECT raw FROM runs WHERE {ACTIVE_RUN} ORDER BY updated_at DESC, id DESC"
            ))?;
            st.query_map([], run_from_raw)?.collect()
        })
    }

    /// Runs of every group created at or after `since` (ms), newest first: member stats.
    pub fn runs_since(&self, since: u64) -> StoreResult<Vec<BotRun>> {
        self.with_reader(|c| {
            let mut st = c.prepare_cached(
                "SELECT raw FROM runs WHERE created_at >= ?1 ORDER BY created_at DESC, id DESC",
            )?;
            st.query_map([since], run_from_raw)?.collect()
        })
    }

    /// Desk notes of every group (首页), newest first.
    pub fn desk_notes(&self) -> StoreResult<Vec<DeskNoteRow>> {
        self.with_reader(|c| {
            let mut st = c.prepare_cached(
                "SELECT group_id, member_id, text, at, seq FROM desk_notes ORDER BY at DESC, seq DESC",
            )?;
            st.query_map([], |r| {
                Ok(DeskNoteRow {
                    group_id: r.get(0)?,
                    member_id: r.get(1)?,
                    text: r.get(2)?,
                    at: r.get(3)?,
                    seq: r.get(4)?,
                })
            })?
            .collect()
        })
    }

    /// The newest state of each link of a topic, newest first.
    pub fn topic_links(&self, topic_id: &str) -> StoreResult<Vec<TopicLink>> {
        self.with_reader(|c| {
            let mut st = c.prepare_cached(
                "SELECT raw FROM topic_links WHERE topic_id = ?1 ORDER BY seq DESC",
            )?;
            st.query_map([topic_id], |r| {
                serde_json::from_str::<TopicLink>(&r.get::<_, String>(0)?).map_err(json_err)
            })?
            .collect()
        })
    }

    /// Topics whose messages point at a 产出 with a Card v2 `ref` block, newest message
    /// first: `(group id, topic id)`, each topic once, at most `limit`.
    pub fn artifact_citations(
        &self,
        artifact_id: &str,
        limit: usize,
    ) -> StoreResult<Vec<(String, String)>> {
        self.with_reader(|c| {
            let mut st = c.prepare_cached(
                "SELECT raw FROM messages
                 WHERE deleted_at IS NULL AND json_extract(raw, '$.cardV2') IS NOT NULL
                   AND instr(raw, ?1) > 0
                 ORDER BY created_at DESC LIMIT 500",
            )?;
            let rows = st.query_map([artifact_id], |r| r.get::<_, String>(0))?;
            let mut out: Vec<(String, String)> = Vec::new();
            for raw in rows {
                let m: Message = serde_json::from_str(&raw?).map_err(json_err)?;
                let Some(card) = &m.card_v2 else { continue };
                let cites = card.all_blocks().into_iter().any(|b| {
                    matches!(b, CardV2Block::Ref {
                        target: RefTarget::Artifact { artifact_id: id, .. }, ..
                    } if id == artifact_id)
                });
                let key = (m.group_id.clone(), m.topic_id.clone());
                if cites && !out.contains(&key) {
                    out.push(key);
                    if out.len() >= limit {
                        break;
                    }
                }
            }
            Ok(out)
        })
    }

    /// Side chat topics (in my DMs with bots) about a topic, newest first.
    pub fn side_chats(&self, group_id: &str, topic_id: &str) -> StoreResult<Vec<rows::TopicRow>> {
        let me = self.me_id()?.unwrap_or_default();
        self.with_reader(|c| {
            let mut st = c.prepare_cached(&rows::topic_select(
                "WHERE json_extract(t.raw, '$.contextOf.topicId') = ?1
                   AND json_extract(t.raw, '$.contextOf.groupId') = ?3",
            ))?;
            st.query_map(params![topic_id, me, group_id], rows::topic_from_row)?
                .collect()
        })
    }

    /// Groups the 1.1 recovery still has to re-read.
    pub fn refetches(&self) -> StoreResult<Vec<(String, Refetch)>> {
        self.with_reader(|c| {
            let mut st = c.prepare_cached("SELECT group_id, after, upto FROM log_refetch")?;
            st.query_map([], |r| {
                Ok((
                    r.get(0)?,
                    Refetch {
                        after: r.get(1)?,
                        upto: r.get(2)?,
                    },
                ))
            })?
            .collect()
        })
    }

    pub fn refetch(&self, group_id: &str) -> StoreResult<Option<Refetch>> {
        self.with_reader(|c| {
            c.query_row(
                "SELECT after, upto FROM log_refetch WHERE group_id = ?1",
                [group_id],
                |r| {
                    Ok(Refetch {
                        after: r.get(0)?,
                        upto: r.get(1)?,
                    })
                },
            )
            .optional()
        })
    }

    /// Applies one re-fetched page of the 1.1 recovery (entries at or below `upto` only) and
    /// moves the group's position to `after`; `done` removes the group's record.
    pub fn apply_recovered(
        &self,
        group_id: &str,
        entries: &[LogEntry],
        after: u64,
        done: bool,
    ) -> StoreResult<ApplyOutcome> {
        self.with_writer(|tx| {
            let upto: Option<u64> = tx
                .query_row(
                    "SELECT upto FROM log_refetch WHERE group_id = ?1",
                    [group_id],
                    |r| r.get(0),
                )
                .optional()?;
            let mut out = ApplyOutcome::default();
            let Some(upto) = upto else {
                return Ok(out);
            };
            for e in entries {
                if let LogEntry::Known(ev) = e
                    && ev.group_id == group_id
                    && ev.seq <= upto
                {
                    recover_one(tx, ev, &mut out)?;
                    out.applied += 1;
                }
            }
            if done || after >= upto {
                tx.execute("DELETE FROM log_refetch WHERE group_id = ?1", [group_id])?;
            } else {
                tx.execute(
                    "UPDATE log_refetch SET after = MAX(after, ?2) WHERE group_id = ?1",
                    params![group_id, after],
                )?;
            }
            Ok(out)
        })
    }
}
