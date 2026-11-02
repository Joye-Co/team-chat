//! V1 cache tables (migration `ext-1`): 产出 and their versions, card presses, the
//! 待办 query and the index of the downloaded-file cache.

use super::rows::{self, json_err};
use super::{Store, StoreResult};
use crate::proto::{Artifact, ArtifactResponse, ArtifactVersion, BotRun, CardActionedData, Event};
use rusqlite::{OptionalExtension, Transaction, params};

/// A 产出 with its cached versions, newest first (may be empty when only the list
/// was fetched).
#[derive(Debug, Clone, PartialEq)]
pub struct ArtifactRow {
    pub artifact: Artifact,
    pub versions: Vec<ArtifactVersion>,
}

/// One press of a card button (`card.actioned`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardActionRow {
    pub seq: u64,
    pub message_id: String,
    pub actor_id: String,
    pub action_id: String,
    pub at: u64,
}

/// Why a topic is in 待办.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoReason {
    /// The topic's `waitingOn` holds me and it is not done.
    WaitingOnMe,
    /// The topic has unread messages that mention me.
    Mentions,
}

/// A topic of 待办. `reasons` has one or both reasons.
#[derive(Debug, Clone, PartialEq)]
pub struct TodoRow {
    pub topic: rows::TopicRow,
    pub reasons: Vec<TodoReason>,
}

pub(crate) fn insert_card_action(
    tx: &Transaction<'_>,
    ev: &Event,
    d: &CardActionedData,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT OR IGNORE INTO card_actions (group_id, seq, message_id, actor_id, action_id, at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            ev.group_id,
            ev.seq,
            d.message_id,
            ev.actor_id,
            d.action_id,
            ev.at
        ],
    )?;
    Ok(())
}

/// Stores a 产出 and some of its versions. An older copy (lower `currentVersion`)
/// never replaces a newer one, so HTTP answers and events may arrive in any order.
pub(crate) fn upsert_artifact(
    tx: &Transaction<'_>,
    a: &Artifact,
    versions: &[ArtifactVersion],
) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO artifacts (id, group_id, topic_id, current_version, updated_at, raw)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET current_version = excluded.current_version,
           updated_at = excluded.updated_at, raw = excluded.raw
         WHERE excluded.current_version > artifacts.current_version
            OR (excluded.current_version = artifacts.current_version
                AND excluded.updated_at >= artifacts.updated_at)",
        params![
            a.id,
            a.group_id,
            a.topic_id,
            a.current_version,
            a.updated_at,
            serde_json::to_string(a)?
        ],
    )?;
    for v in versions {
        // Versions never change once made.
        tx.execute(
            "INSERT OR IGNORE INTO artifact_versions (artifact_id, version, raw) VALUES (?1, ?2, ?3)",
            params![v.artifact_id, v.version, serde_json::to_string(v)?],
        )?;
    }
    Ok(())
}

/// Stores a bot run. An older copy (lower `updatedAt`) never replaces a newer one.
pub(crate) fn upsert_run(tx: &Transaction<'_>, r: &BotRun) -> StoreResult<()> {
    tx.execute(
        "INSERT INTO runs (id, group_id, topic_id, updated_at, created_at, raw)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET updated_at = excluded.updated_at, raw = excluded.raw
         WHERE excluded.updated_at >= runs.updated_at",
        params![
            r.id,
            r.group_id,
            r.topic_id,
            r.updated_at,
            r.created_at,
            serde_json::to_string(r)?
        ],
    )?;
    Ok(())
}

fn artifact_from_raw(raw: String) -> rusqlite::Result<Artifact> {
    serde_json::from_str(&raw).map_err(json_err)
}

impl Store {
    /// Stores 产出 from an HTTP list answer.
    pub fn store_artifacts(&self, list: &[Artifact]) -> StoreResult<()> {
        self.with_writer(|tx| {
            for a in list {
                upsert_artifact(tx, a, &[])?;
            }
            Ok(())
        })
    }

    /// Stores a 产出 with its versions (`GET /v1/artifacts/:id`).
    pub fn store_artifact_response(&self, r: &ArtifactResponse) -> StoreResult<()> {
        self.with_writer(|tx| upsert_artifact(tx, &r.artifact, &r.versions))
    }

    /// 产出 of a topic, newest `updatedAt` first, without versions.
    pub fn artifacts(&self, topic_id: &str) -> StoreResult<Vec<Artifact>> {
        self.with_reader(|c| {
            let mut st = c.prepare_cached(
                "SELECT raw FROM artifacts WHERE topic_id = ?1 ORDER BY updated_at DESC, id",
            )?;
            st.query_map([topic_id], |r| artifact_from_raw(r.get(0)?))?
                .collect()
        })
    }

    /// 产出 of a group, newest `updatedAt` first, without versions.
    pub fn group_artifacts(&self, group_id: &str) -> StoreResult<Vec<Artifact>> {
        self.with_reader(|c| {
            let mut st = c.prepare_cached(
                "SELECT raw FROM artifacts WHERE group_id = ?1 ORDER BY updated_at DESC, id",
            )?;
            st.query_map([group_id], |r| artifact_from_raw(r.get(0)?))?
                .collect()
        })
    }

    /// One 产出 with its cached versions, newest first.
    pub fn artifact(&self, id: &str) -> StoreResult<Option<ArtifactRow>> {
        self.with_reader(|c| {
            let Some(raw) = c
                .query_row("SELECT raw FROM artifacts WHERE id = ?1", [id], |r| {
                    r.get::<_, String>(0)
                })
                .optional()?
            else {
                return Ok(None);
            };
            let artifact = artifact_from_raw(raw)?;
            let mut st = c.prepare_cached(
                "SELECT raw FROM artifact_versions WHERE artifact_id = ?1 ORDER BY version DESC",
            )?;
            let versions = st
                .query_map([id], |r| {
                    serde_json::from_str::<ArtifactVersion>(&r.get::<_, String>(0)?)
                        .map_err(json_err)
                })?
                .collect::<Result<_, _>>()?;
            Ok(Some(ArtifactRow { artifact, versions }))
        })
    }

    /// One cached version of a 产出.
    pub fn artifact_version(
        &self,
        artifact_id: &str,
        version: u32,
    ) -> StoreResult<Option<ArtifactVersion>> {
        self.with_reader(|c| {
            c.query_row(
                "SELECT raw FROM artifact_versions WHERE artifact_id = ?1 AND version = ?2",
                params![artifact_id, version],
                |r| {
                    serde_json::from_str::<ArtifactVersion>(&r.get::<_, String>(0)?)
                        .map_err(json_err)
                },
            )
            .optional()
        })
    }

    /// Stores a bot run from an HTTP answer.
    pub fn store_run(&self, r: &BotRun) -> StoreResult<()> {
        self.with_writer(|tx| upsert_run(tx, r))
    }

    /// Bot runs of a topic, newest first.
    pub fn runs(&self, topic_id: &str) -> StoreResult<Vec<BotRun>> {
        self.with_reader(|c| {
            let mut st = c.prepare_cached(
                "SELECT raw FROM runs WHERE topic_id = ?1 ORDER BY created_at DESC, id DESC",
            )?;
            st.query_map([topic_id], |r| {
                serde_json::from_str::<BotRun>(&r.get::<_, String>(0)?).map_err(json_err)
            })?
            .collect()
        })
    }

    pub fn run(&self, id: &str) -> StoreResult<Option<BotRun>> {
        self.with_reader(|c| {
            c.query_row("SELECT raw FROM runs WHERE id = ?1", [id], |r| {
                serde_json::from_str::<BotRun>(&r.get::<_, String>(0)?).map_err(json_err)
            })
            .optional()
        })
    }

    /// Presses of a card message's buttons, oldest first.
    pub fn card_actions(&self, message_id: &str) -> StoreResult<Vec<CardActionRow>> {
        self.with_reader(|c| {
            let mut st = c.prepare_cached(
                "SELECT seq, message_id, actor_id, action_id, at FROM card_actions
                 WHERE message_id = ?1 ORDER BY seq",
            )?;
            st.query_map([message_id], |r| {
                Ok(CardActionRow {
                    seq: r.get(0)?,
                    message_id: r.get(1)?,
                    actor_id: r.get(2)?,
                    action_id: r.get(3)?,
                    at: r.get(4)?,
                })
            })?
            .collect()
        })
    }

    /// 待办: topics of all my groups that wait on me (not done) or have unread
    /// messages that mention me, newest activity first.
    pub fn todo(&self) -> StoreResult<Vec<TodoRow>> {
        let Some(me) = self.me_id()? else {
            return Ok(Vec::new());
        };
        let candidates = self.with_reader(|c| {
            let mut st = c.prepare_cached(&rows::topic_select(&format!(
                "WHERE ?1 = ?1 AND (
                   (t.status != 'done' AND t.waiting_on LIKE '%\"' || ?2 || '\"%')
                   OR EXISTS (SELECT 1 FROM messages m WHERE m.topic_id = t.id
                        AND m.mentions LIKE '%\"' || ?2 || '\"%' AND {cond}
                        AND m.created_seq > MAX(COALESCE((SELECT g.read_seq FROM groups g WHERE g.id = t.group_id), 0),
                                                COALESCE(rc.seq, 0))))
                 LIMIT ?3",
                cond = rows::UNREAD_COND
            )))?;
            st.query_map(params!["", me, 10_000u64], rows::topic_from_row)?
                .collect::<Result<Vec<_>, _>>()
        })?;
        let mut out: Vec<TodoRow> = candidates
            .into_iter()
            .filter_map(|t| {
                let mut reasons = Vec::new();
                if t.topic.status != crate::proto::TopicStatus::Done
                    && t.topic.waiting_on().contains(&me)
                {
                    reasons.push(TodoReason::WaitingOnMe);
                }
                if t.mentions > 0 {
                    reasons.push(TodoReason::Mentions);
                }
                (!reasons.is_empty()).then_some(TodoRow { topic: t, reasons })
            })
            .collect();
        out.sort_by(|a, b| {
            b.topic
                .last_activity_at
                .cmp(&a.topic.last_activity_at)
                .then_with(|| b.topic.topic.last_seq.cmp(&a.topic.topic.last_seq))
        });
        Ok(out)
    }

    // ------------------------------------------------------------------
    // Downloaded-file cache index (files/ in the data directory)
    // ------------------------------------------------------------------

    /// The cached path of a file content, and marks it used now.
    pub fn file_cache_get(&self, sha256: &str) -> StoreResult<Option<String>> {
        self.with_writer(|tx| {
            let p: Option<String> = tx
                .query_row(
                    "SELECT path FROM file_cache WHERE sha256 = ?1",
                    [sha256],
                    |r| r.get(0),
                )
                .optional()?;
            if p.is_some() {
                tx.execute(
                    "UPDATE file_cache SET last_used = ?2 WHERE sha256 = ?1",
                    params![sha256, crate::now_ms()],
                )?;
            }
            Ok(p)
        })
    }

    pub fn file_cache_put(&self, sha256: &str, path: &str, size: u64) -> StoreResult<()> {
        self.with_writer(|tx| {
            tx.execute(
                "INSERT INTO file_cache (sha256, path, size, last_used) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(sha256) DO UPDATE SET path = excluded.path, size = excluded.size,
                   last_used = excluded.last_used",
                params![sha256, path, size, crate::now_ms()],
            )?;
            Ok(())
        })
    }

    pub fn file_cache_remove(&self, sha256: &str) -> StoreResult<()> {
        self.with_writer(|tx| {
            tx.execute("DELETE FROM file_cache WHERE sha256 = ?1", [sha256])?;
            Ok(())
        })
    }

    /// Total bytes of the cached files.
    pub fn file_cache_size(&self) -> StoreResult<u64> {
        self.with_reader(|c| {
            c.query_row("SELECT COALESCE(SUM(size), 0) FROM file_cache", [], |r| {
                r.get(0)
            })
        })
    }

    /// Removes least recently used entries until the total is at most `max_bytes`,
    /// never `keep`. Returns the (sha256, path) of the removed entries; the caller
    /// deletes the files.
    pub fn file_cache_evict(
        &self,
        max_bytes: u64,
        keep: &str,
    ) -> StoreResult<Vec<(String, String)>> {
        self.with_writer(|tx| {
            let mut total: u64 =
                tx.query_row("SELECT COALESCE(SUM(size), 0) FROM file_cache", [], |r| {
                    r.get(0)
                })?;
            let mut removed = Vec::new();
            if total <= max_bytes {
                return Ok(removed);
            }
            let rows: Vec<(String, String, u64)> = {
                let mut st = tx.prepare(
                    "SELECT sha256, path, size FROM file_cache WHERE sha256 != ?1 ORDER BY last_used, sha256",
                )?;
                st.query_map([keep], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                    .collect::<Result<_, _>>()?
            };
            for (sha, path, size) in rows {
                if total <= max_bytes {
                    break;
                }
                tx.execute("DELETE FROM file_cache WHERE sha256 = ?1", [&sha])?;
                total = total.saturating_sub(size);
                removed.push((sha, path));
            }
            Ok(removed)
        })
    }
}
