//! 1.1 operations on [`Client`]: Card v2 answers, 产出 page events, side chats (单独问),
//! pause and resume of a run (接管 / 交还), the live terminal, and cache reads for the desk
//! (decisions, active runs, topic links). See docs/protocol.md, "1.1 extensions".

use crate::client::{Client, ClientError, ClientUpdate, SendRequest, Shared, enqueue_with};
use crate::proto::machine::RunLogEntry;
use crate::proto::{
    AnswerEdits, AnswerInputs, BotRun, GroupDecision, GroupKind, MemberKind, TopicContext,
    TopicLink,
};
use crate::runs::{TerminalLine, terminal_lines};
use crate::store::{ArtifactEventRow, CardAnswerRow, DecisionRow, OutboxOp, OutboxState, TopicRow};
use tokio::task::JoinHandle;

/// A side chat to start: my question to `bot_id` about a topic, read up to `upto_seq`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SideChatRequest {
    pub bot_id: String,
    /// The topic the side chat is about.
    pub context: TopicContext,
    pub body: String,
    /// Title of the side chat topic; `None` uses the first line.
    pub title: Option<String>,
}

/// A side chat that was queued: the DM it lives in and the `clientMsgId` of its first message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideChatStarted {
    pub dm_group_id: String,
    pub client_msg_id: String,
}

impl Client {
    // ------------------------------------------------------------------
    // Card v2
    // ------------------------------------------------------------------

    /// Queues my answer to a v2 card (`card.answer`). `rev` is the card's rev that I saw;
    /// `inputs` and `edited_values` are keyed by block id. The cache gets the answer from the
    /// server's `card.answered`; until then [`Client::pending_card_answers`] lists it.
    pub fn answer_card(
        &self,
        message_id: &str,
        rev: u32,
        action_id: &str,
        inputs: Option<AnswerInputs>,
        edited_values: Option<AnswerEdits>,
    ) -> Result<String, ClientError> {
        crate::proto::check_answer(rev, action_id, inputs.as_ref(), edited_values.as_ref())
            .map_err(|_| ClientError::Invalid("bad card answer"))?;
        let m = self
            .store()
            .message(message_id)?
            .ok_or(ClientError::NotFound)?;
        if m.card_v2.is_none() {
            return Err(ClientError::Invalid("not a v2 card"));
        }
        self.enqueue(
            crate::new_id(),
            OutboxOp::CardAnswer {
                group_id: m.group_id,
                topic_id: Some(m.topic_id),
                message_id: message_id.into(),
                rev,
                action_id: action_id.into(),
                inputs,
                edited_values,
            },
        )
    }

    /// My answers the server has not confirmed yet, in a topic: `(message id, rev, action id,
    /// failed)`. A failed one (for example `conflict`: the card changed) can be discarded.
    pub fn pending_card_answers(
        &self,
        topic_id: &str,
    ) -> Result<Vec<(String, u32, String, bool)>, ClientError> {
        Ok(self
            .store()
            .outbox_all()?
            .into_iter()
            .filter_map(|item| match item.op {
                OutboxOp::CardAnswer {
                    topic_id: Some(t),
                    message_id,
                    rev,
                    action_id,
                    ..
                } if t == topic_id => Some((
                    message_id,
                    rev,
                    action_id,
                    item.state == OutboxState::Failed,
                )),
                _ => None,
            })
            .collect())
    }

    /// Answers to a v2 card message from the cache, oldest first.
    pub fn card_answers(&self, message_id: &str) -> Result<Vec<CardAnswerRow>, ClientError> {
        Ok(self.store().card_answers(message_id)?)
    }

    /// Group decisions (群里的决定), newest first: answers to v2 asks with `record`.
    /// `group_id = None` lists every group.
    pub fn decisions(
        &self,
        group_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<DecisionRow>, ClientError> {
        Ok(self.store().decisions(group_id, limit)?)
    }

    /// The newest group decisions from the server (`GET /v1/groups/:id/decisions`). The
    /// server keeps the card as it was answered, so a decision stays listed after its author
    /// edits the card; [`Client::decisions`] reads the cache and is the offline fallback.
    pub fn fetch_decisions(
        &self,
        group_id: &str,
        limit: u32,
    ) -> JoinHandle<Result<Vec<GroupDecision>, ClientError>> {
        let http = self.http();
        let id = group_id.to_string();
        self.spawn(async move { Ok(http.group_decisions(&id, None, limit).await?.decisions) })
    }

    // ------------------------------------------------------------------
    // 产出 page events and topic links
    // ------------------------------------------------------------------

    /// Queues an event from an interactive 产出 page (`artifact.event`). The page's version
    /// must declare `name`; a refused event is dropped.
    pub fn send_artifact_event(
        &self,
        artifact_id: &str,
        version: u32,
        name: &str,
        payload: Option<serde_json::Value>,
    ) -> Result<String, ClientError> {
        let row = self
            .store()
            .artifact(artifact_id)?
            .ok_or(ClientError::NotFound)?;
        let frame = crate::proto::ArtifactEventFrame {
            rid: "r".into(),
            group_id: row.artifact.group_id.clone(),
            artifact_id: artifact_id.into(),
            version,
            name: name.into(),
            payload: payload.clone(),
        };
        frame
            .validate()
            .map_err(|_| ClientError::Invalid("bad artifact event"))?;
        self.enqueue(
            crate::new_id(),
            OutboxOp::ArtifactEvent {
                group_id: row.artifact.group_id,
                topic_id: Some(row.artifact.topic_id),
                artifact_id: artifact_id.into(),
                version,
                name: name.into(),
                payload,
            },
        )
    }

    /// Page events of a 产出 from the cache, oldest first.
    pub fn artifact_events(&self, artifact_id: &str) -> Result<Vec<ArtifactEventRow>, ClientError> {
        Ok(self.store().artifact_events(artifact_id)?)
    }

    /// Topics that point at a 产出 (Card v2 `ref` blocks), newest first, from the cache.
    pub fn artifact_citations(
        &self,
        artifact_id: &str,
        limit: usize,
    ) -> Result<Vec<(String, String)>, ClientError> {
        Ok(self.store().artifact_citations(artifact_id, limit)?)
    }

    /// The newest state of each link (pull request) of a topic, newest first.
    pub fn topic_links(&self, topic_id: &str) -> Result<Vec<TopicLink>, ClientError> {
        Ok(self.store().topic_links(topic_id)?)
    }

    // ------------------------------------------------------------------
    // Side chats (单独问)
    // ------------------------------------------------------------------

    /// My DM group with a member, from the cache.
    pub fn dm_with(&self, member_id: &str) -> Result<Option<String>, ClientError> {
        let me = self.store().me_id()?.unwrap_or_default();
        Ok(self
            .store()
            .groups()?
            .into_iter()
            .find(|g| {
                g.group.kind == GroupKind::Dm
                    && g.group.member_ids.iter().any(|m| m == member_id)
                    && g.group.member_ids.iter().any(|m| *m == me)
            })
            .map(|g| g.group.id))
    }

    /// Starts a side chat: finds or makes my DM with the bot (`POST /v1/dms`), then queues
    /// a new topic there with `send.context`. The answer stays private to me and the bot.
    pub fn start_side_chat(
        &self,
        req: SideChatRequest,
    ) -> JoinHandle<Result<SideChatStarted, ClientError>> {
        let http = self.http();
        let (shared, cmd_tx) = self.queue();
        let existing = self.dm_with(&req.bot_id);
        let bot = self.store().member(&req.bot_id);
        self.spawn(async move {
            let bot = bot?.ok_or(ClientError::NotFound)?;
            if bot.kind != MemberKind::Bot {
                return Err(ClientError::Invalid("side chats are with bots"));
            }
            if req.body.trim().is_empty() || req.body.chars().count() > crate::proto::MAX_BODY_CHARS
            {
                return Err(ClientError::Invalid("bad body"));
            }
            req.context
                .validate()
                .map_err(|_| ClientError::Invalid("bad context"))?;
            let dm_group_id = match existing? {
                Some(id) => id,
                None => {
                    let summary = http.dm(&req.bot_id).await?;
                    shared.store.apply_directory(
                        None,
                        Some(std::slice::from_ref(&summary)),
                        None,
                    )?;
                    shared.emit(ClientUpdate::DirectoryChanged);
                    summary.group.id
                }
            };
            let client_msg_id = enqueue_with(
                &shared,
                &cmd_tx,
                crate::new_id(),
                OutboxOp::Send {
                    group_id: dm_group_id.clone(),
                    topic_id: None,
                    title: req.title,
                    body: req.body,
                    mentions: Vec::new(),
                    streaming: false,
                    reply_to: None,
                    attachments: None,
                    artifact_refs: None,
                    card: None,
                    card_v2: None,
                    context: Some(req.context),
                    after_root: None,
                },
            )?;
            Ok(SideChatStarted {
                dm_group_id,
                client_msg_id,
            })
        })
    }

    /// A follow-up in a side chat topic. `upto_seq`, when set, lets the bot read the source
    /// topic up to that seq now (`send.context` with the topic's own `contextOf`).
    pub fn reply_in_side_chat(
        &self,
        side_topic_id: &str,
        body: &str,
        upto_seq: Option<u64>,
    ) -> Result<String, ClientError> {
        let row = self
            .store()
            .topic(side_topic_id)?
            .ok_or(ClientError::NotFound)?;
        let context = match (upto_seq, row.topic.context_of.clone()) {
            (Some(seq), Some(c)) if seq > c.upto_seq => Some(TopicContext { upto_seq: seq, ..c }),
            (_, None) => return Err(ClientError::Invalid("not a side chat")),
            _ => None,
        };
        self.send(SendRequest {
            group_id: row.topic.group_id.clone(),
            topic_id: Some(side_topic_id.into()),
            body: body.into(),
            context,
            ..Default::default()
        })
    }

    /// My side chats about a topic, newest first.
    pub fn side_chats(&self, group_id: &str, topic_id: &str) -> Result<Vec<TopicRow>, ClientError> {
        Ok(self.store().side_chats(group_id, topic_id)?)
    }

    // ------------------------------------------------------------------
    // Runs: desk, pause and resume, live terminal
    // ------------------------------------------------------------------

    /// Active runs of every group (正在做), newest change first.
    pub fn active_runs(&self) -> Result<Vec<BotRun>, ClientError> {
        Ok(self.store().active_runs()?)
    }

    /// Runs of every group created at or after `since` (ms): member stats (这周).
    pub fn runs_since(&self, since: u64) -> Result<Vec<BotRun>, ClientError> {
        Ok(self.store().runs_since(since)?)
    }

    /// Pauses a run for 接管 (`POST /v1/runs/:id/pause`) and stores the answer.
    pub fn pause_run(&self, run_id: &str) -> JoinHandle<Result<BotRun, ClientError>> {
        let http = self.http();
        let shared = self.shared().clone();
        let id = run_id.to_string();
        self.spawn(async move {
            let run = http.pause_run(&id).await?;
            store_run_and_emit(&shared, &run)?;
            Ok(run)
        })
    }

    /// Resumes a paused run (交还, `POST /v1/runs/:id/resume`) and stores the answer.
    pub fn resume_run(
        &self,
        run_id: &str,
        note: Option<String>,
    ) -> JoinHandle<Result<BotRun, ClientError>> {
        let http = self.http();
        let shared = self.shared().clone();
        let id = run_id.to_string();
        self.spawn(async move {
            let run = http.resume_run(&id, note.as_deref()).await?;
            store_run_and_emit(&shared, &run)?;
            Ok(run)
        })
    }

    /// Terminal lines (commands and their output) of a run after `after_seq`, for the live
    /// view; call again with the returned seq to follow a running run.
    pub fn run_terminal(
        &self,
        run_id: &str,
        after_seq: u64,
    ) -> JoinHandle<Result<(Vec<TerminalLine>, u64), ClientError>> {
        let log = self.run_log(run_id, after_seq);
        self.spawn(async move {
            let (entries, last): (Vec<RunLogEntry>, u64) = log
                .await
                .map_err(|e| ClientError::Runtime(e.to_string()))??;
            Ok((terminal_lines(&entries), last))
        })
    }
}

fn store_run_and_emit(shared: &Shared, run: &BotRun) -> Result<(), ClientError> {
    shared.store.store_run(run)?;
    shared.emit(ClientUpdate::RunsChanged {
        group_id: run.group_id.clone(),
        topic_id: run.topic_id.clone(),
    });
    Ok(())
}
