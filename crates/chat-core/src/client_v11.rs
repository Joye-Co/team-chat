//! 1.1 operations on [`Client`]: Card v2 answers, 产出 page events, side chats (单独问),
//! pause and resume of a run (接管 / 交还), the live terminal, and cache reads for the desk
//! (decisions, active runs, topic links). See docs/protocol.md, "1.1 extensions".

use crate::client::{Client, ClientError, ClientUpdate, SendRequest, Shared, enqueue_with};
use crate::engine::{Command, LiveWrite};
use crate::proto::machine::RunLogEntry;
use crate::proto::{
    AnswerEdits, AnswerInputs, ArtifactDataResponse, ArtifactEventFrame, ArtifactEventKind, BotRun,
    CardActionKind, CardEventFrame, ClientFrame, EVENT_ACTION_PRESS, EVENT_ANSWER_RETRACT,
    GroupDecision, GroupKind, MemberKind, PartRef, TopicContext, TopicLink,
};
use crate::runs::{TerminalLine, terminal_lines};
use crate::store::{CardAnswerRow, DecisionRow, OutboxOp, OutboxState, TopicRow};
use tokio::task::JoinHandle;

/// One interaction with a v2 card (2.1, `card.event`). Exactly one of `block_id` /
/// `action_id`, except a retract (use [`Client::retract_card_answer`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CardEventRequest {
    pub message_id: String,
    /// The rev I saw.
    pub rev: u32,
    pub block_id: Option<String>,
    pub action_id: Option<String>,
    /// A card event name (`choice.change`, `chart.select`, `action.press`, ...).
    pub event: String,
    pub value: Option<serde_json::Value>,
    /// Only with `action.press` of a submit or reject action.
    pub inputs: Option<AnswerInputs>,
    /// The part the event targets (`chart.select`, `metric.press`).
    pub part: Option<PartRef>,
}

/// A card event that left (or was queued by) this client. `durable`: an answer, kept in the
/// outbox and resent with the same id after a reconnect; else a live event that is dropped
/// when the connection goes (its ack is `ClientUpdate::CardEventAcked`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardEventSent {
    pub client_event_id: String,
    pub durable: bool,
}

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

/// The `contentHash` of an answer by `action_id`: sha256 (hex) of the draft text it approves.
fn draft_hash(
    card: &crate::proto::CardV2,
    action_id: &str,
    edited: Option<&AnswerEdits>,
) -> Option<String> {
    use sha2::{Digest, Sha256};
    let action = card.actions.iter().flatten().find(|a| a.id == action_id);
    let ids: std::collections::BTreeSet<String> = match action.and_then(|a| a.inputs.as_ref()) {
        Some(ids) => ids.iter().cloned().collect(),
        None => card
            .all_blocks()
            .into_iter()
            .filter_map(|b| match b {
                crate::proto::CardV2Block::Draft { id, .. } => Some(id.clone()),
                _ => None,
            })
            .collect(),
    };
    let text = crate::proto::draft_content_text(card, &ids, edited)?;
    let digest = Sha256::digest(text.as_bytes());
    Some(digest.iter().map(|b| format!("{b:02x}")).collect())
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
        let Some(card) = m.card_v2.as_ref() else {
            return Err(ClientError::Invalid("not a v2 card"));
        };
        let content_hash = draft_hash(card, action_id, edited_values.as_ref());
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
                content_hash,
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

    /// Sends one interaction with a v2 card (2.1, `card.event`). An answer (a submit or
    /// reject press, or a commit on the decision surface while the card asks:
    /// [`crate::proto::CardV2::is_answer_event`]) goes through
    /// the outbox and is resent after a reconnect; every other event is live: sent now or
    /// failed at once (reason `client.offline`). The ack is `ClientUpdate::CardEventAcked`.
    pub fn send_card_event(&self, req: CardEventRequest) -> Result<CardEventSent, ClientError> {
        let m = self
            .store()
            .message(&req.message_id)?
            .ok_or(ClientError::NotFound)?;
        let card = m
            .card_v2
            .as_ref()
            .ok_or(ClientError::Invalid("not a v2 card"))?;
        let client_event_id = crate::new_id();
        let frame = CardEventFrame {
            rid: "r".into(),
            group_id: m.group_id.clone(),
            message_id: req.message_id.clone(),
            rev: req.rev,
            client_event_id: client_event_id.clone(),
            block_id: req.block_id.clone(),
            action_id: req.action_id.clone(),
            event: req.event.clone(),
            value: req.value.clone(),
            inputs: req.inputs.clone(),
            part: req.part.clone(),
            content_hash: None,
        };
        frame
            .validate()
            .map_err(|_| ClientError::Invalid("bad card event"))?;
        let durable = match (&req.action_id, &req.block_id) {
            (Some(a), _) if req.event == EVENT_ACTION_PRESS => {
                card.actions().iter().find(|x| x.id == *a).is_some_and(|x| {
                    !matches!(x.kind(), CardActionKind::Press | CardActionKind::Link)
                })
            }
            // The server's rule for a stored answer (a commit on the decision surface).
            (_, Some(b)) => card.is_answer_event(b, &req.event),
            _ => false,
        };
        if durable {
            self.enqueue(
                client_event_id.clone(),
                OutboxOp::CardEvent {
                    group_id: m.group_id,
                    topic_id: Some(m.topic_id),
                    message_id: req.message_id,
                    rev: req.rev,
                    client_event_id: client_event_id.clone(),
                    block_id: req.block_id,
                    action_id: req.action_id,
                    event: req.event,
                    value: req.value,
                    inputs: req.inputs,
                    part: req.part,
                },
            )?;
        } else {
            self.command(Command::Live {
                frame: Box::new(ClientFrame::CardEvent(frame)),
                what: LiveWrite::Card {
                    client_event_id: client_event_id.clone(),
                    message_id: req.message_id,
                },
            });
        }
        Ok(CardEventSent {
            client_event_id,
            durable,
        })
    }

    /// Takes back my answer (撤回, `card.event answer.retract`); queued like an answer.
    pub fn retract_card_answer(
        &self,
        message_id: &str,
        answer_id: &str,
    ) -> Result<String, ClientError> {
        let m = self
            .store()
            .message(message_id)?
            .ok_or(ClientError::NotFound)?;
        let rev = m
            .card_v2
            .as_ref()
            .map(|c| c.rev)
            .ok_or(ClientError::Invalid("not a v2 card"))?;
        let client_event_id = crate::new_id();
        self.enqueue(
            client_event_id.clone(),
            OutboxOp::CardEvent {
                group_id: m.group_id,
                topic_id: Some(m.topic_id),
                message_id: message_id.into(),
                rev,
                client_event_id: client_event_id.clone(),
                block_id: None,
                action_id: None,
                event: EVENT_ANSWER_RETRACT.into(),
                value: Some(serde_json::json!({ "answerId": answer_id })),
                inputs: None,
                part: None,
            },
        )?;
        Ok(client_event_id)
    }

    /// My queued card answers and retracts (2.1) in a topic: `(clientEventId, message id,
    /// event, failed)`. A failed one can be discarded with [`Client::discard`].
    pub fn pending_card_events(
        &self,
        topic_id: &str,
    ) -> Result<Vec<(String, String, String, bool)>, ClientError> {
        Ok(self
            .store()
            .outbox_all()?
            .into_iter()
            .filter_map(|item| match item.op {
                OutboxOp::CardEvent {
                    topic_id: Some(t),
                    message_id,
                    client_event_id,
                    event,
                    ..
                } if t == topic_id => Some((
                    client_event_id,
                    message_id,
                    event,
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

    /// Sends an event from an interactive 产出 page (`artifact.event`) now. Page events are
    /// never queued: when not connected the ack fails at once (reason `client.offline`). The
    /// answer is `ClientUpdate::ArtifactEventAcked` with this `clientEventId`; an `ask` gets
    /// its reply as `ClientUpdate::ArtifactReply` with the ack's `eventId`.
    pub fn send_artifact_event(
        &self,
        artifact_id: &str,
        version: u32,
        name: &str,
        kind: ArtifactEventKind,
        payload: Option<serde_json::Value>,
    ) -> Result<String, ClientError> {
        let row = self
            .store()
            .artifact(artifact_id)?
            .ok_or(ClientError::NotFound)?;
        let client_event_id = crate::new_id();
        let frame = ArtifactEventFrame {
            rid: "r".into(),
            group_id: row.artifact.group_id.clone(),
            artifact_id: artifact_id.into(),
            version,
            client_event_id: Some(client_event_id.clone()),
            name: name.into(),
            kind: Some(kind),
            payload,
        };
        frame
            .validate()
            .map_err(|_| ClientError::Invalid("bad artifact event"))?;
        self.command(Command::Live {
            frame: Box::new(ClientFrame::ArtifactEvent(frame)),
            what: LiveWrite::Artifact {
                client_event_id: client_event_id.clone(),
                artifact_id: artifact_id.into(),
                version,
            },
        });
        Ok(client_event_id)
    }

    /// The latest data snapshot of a 产出 version (`GET /v1/artifacts/:id/versions/:v/data`).
    /// Newer snapshots arrive as `ClientUpdate::ArtifactData`.
    pub fn artifact_data(
        &self,
        artifact_id: &str,
        version: u32,
    ) -> JoinHandle<Result<ArtifactDataResponse, ClientError>> {
        let http = self.http();
        let id = artifact_id.to_string();
        self.spawn(async move { Ok(http.artifact_data(&id, version).await?) })
    }

    /// `welcome.features` of the server (empty before the first welcome or from a 1.0 server).
    pub fn server_features(&self) -> Vec<String> {
        self.shared().server_features()
    }

    /// True when the server listed `feature` (`interact/1`, `refs/1`, `diagram/1`).
    pub fn server_has(&self, feature: &str) -> bool {
        self.server_features().iter().any(|f| f == feature)
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
                    && g.group.member_ids.contains(&me)
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
                    cause_event_id: None,
                    block_refs: None,
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
