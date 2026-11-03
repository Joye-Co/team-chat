//! 1.1 cache tables (migration `v11-1`): Card v2 answers, topic links, 产出 page events, and
//! the one-time log re-fetch that recovers what 1.0.x clients skipped (docs/protocol.md,
//! "Client cache recovery"). Queries for the desk: decisions, active runs, side chats.

use super::rows::{self, json_err};
use super::{ApplyOutcome, LogEntry, Store, StoreResult};
use crate::proto::{
    AnswerEdits, AnswerInputs, ArtifactEventedData, ArtifactVersion, BotRun, CardAnsweredData,
    CardV2, Event, EventData, Message, TopicContext, TopicLink, TopicLinkedData,
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

/// One event an interactive 产出 page sent (`artifact.evented`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactEventRow {
    pub seq: u64,
    pub artifact_id: String,
    pub version: u32,
    pub actor_id: String,
    pub name: String,
    pub payload: Option<serde_json::Value>,
    pub at: u64,
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
        "INSERT OR IGNORE INTO card_answers (group_id, seq, message_id, rev, actor_id, action_id, inputs, edited_values, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            ev.group_id,
            ev.seq,
            d.message_id,
            d.rev,
            ev.actor_id,
            d.action_id,
            json_opt(&d.inputs)?,
            json_opt(&d.edited_values)?,
            ev.at
        ],
    )?;
    Ok(())
}

pub(super) fn insert_artifact_event(
    tx: &Transaction<'_>,
    ev: &Event,
    d: &ArtifactEventedData,
) -> StoreResult<()> {
    tx.execute(
        "INSERT OR IGNORE INTO artifact_events (group_id, seq, artifact_id, version, actor_id, name, payload, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            ev.group_id,
            ev.seq,
            d.artifact_id,
            d.version,
            ev.actor_id,
            d.name,
            json_opt(&d.payload)?,
            ev.at
        ],
    )?;
    Ok(())
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
        EventData::ArtifactEvented(d) => insert_artifact_event(tx, ev, d)?,
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
    })
}

const ANSWER_COLS: &str = "a.group_id, a.seq, a.message_id, a.rev, a.actor_id, a.action_id, a.inputs, a.edited_values, a.at";

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
                 WHERE (?1 IS NULL OR a.group_id = ?1) AND m.deleted_at IS NULL
                   AND json_extract(m.raw, '$.cardV2.ask.record') = 1
                 ORDER BY a.at DESC, a.seq DESC LIMIT ?2"
            ))?;
            let rows = st.query_map(params![group_id, limit as u64], |r| {
                let answer = answer_from_row(r)?;
                let topic_id: String = r.get(9)?;
                let raw: String = r.get(10)?;
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

    /// Page events of a 产出, oldest first.
    pub fn artifact_events(&self, artifact_id: &str) -> StoreResult<Vec<ArtifactEventRow>> {
        self.with_reader(|c| {
            let mut st = c.prepare_cached(
                "SELECT seq, artifact_id, version, actor_id, name, payload, at FROM artifact_events
                 WHERE artifact_id = ?1 ORDER BY seq",
            )?;
            st.query_map([artifact_id], |r| {
                Ok(ArtifactEventRow {
                    seq: r.get(0)?,
                    artifact_id: r.get(1)?,
                    version: r.get(2)?,
                    actor_id: r.get(3)?,
                    name: r.get(4)?,
                    payload: parse_opt(r.get(5)?)?,
                    at: r.get(6)?,
                })
            })?
            .collect()
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
