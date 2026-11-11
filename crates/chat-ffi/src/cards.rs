//! 拍板 (M2): answering cards, the 首页 lists and 交代.
//!
//! Cards cross as JSON ([`crate::MessageItem::card_json`]) and Swift draws them; this file
//! sends the answers and says where each card stands ([`CardRecord`]), so the card can turn
//! into a record in place. Answers go to the outbox: they are resent after a reconnect and the
//! server's verdict comes back as [`crate::Update::CardAnswered`].

use std::collections::{HashMap, HashSet};

use chat_core::proto::{
    AnswerEdits, AnswerInputs, AskStatus, CARD_CATALOG_21, CardIntent, MemberKind, MemberLevel,
    Message, TopicStatus,
};
use chat_core::{CardEventRequest, Client, OutboxOp, OutboxState, SendRequest, TodoReason};
use serde_json::Value;

use crate::{ChatClient, CoreError, Directory, MessageItem, message_item};

/// Where a card stands for me, read from the cache and the outbox.
#[derive(Debug, Clone, Default, PartialEq, Eq, uniffi::Record)]
pub struct CardRecord {
    /// `open`, `answered`, `expired`, `closed`, `superseded`, or `none` for a card that asks
    /// nothing.
    pub state: String,
    /// I may answer it now (I am asked, it is open and nothing of mine is on its way).
    pub can_answer: bool,
    /// Who answered, empty when nobody did.
    pub answered_by: String,
    pub answered_by_me: bool,
    pub answered_at: u64,
    /// The action of the answer (or of my answer on its way).
    pub action_id: String,
    /// Its label, as the card names it.
    pub action_label: String,
    /// The answer is a cancel or a reject (拒绝, 不了).
    pub declined: bool,
    /// What was chosen or filled in: option labels, `label：value` of inputs.
    pub details: Vec<String>,
    /// The answer changed the card's draft before sending it.
    pub edited: bool,
    /// My answer is on its way.
    pub sending: bool,
    /// The server refused my answer; [`crate::Update::CardAnswered`] carried the reason.
    pub refused: bool,
}

/// One row of the 首页.
#[derive(Debug, Clone, uniffi::Record)]
pub struct HomeItem {
    pub group_id: String,
    pub group_name: String,
    pub topic_id: String,
    /// The topic's title, or the card's title for an ask.
    pub title: String,
    /// The topic's status in its wire form.
    pub status: String,
    /// Who it is about: who asks, who is doing it, who finished it.
    pub member_id: String,
    pub member_name: String,
    pub member_is_bot: bool,
    pub at: u64,
    /// For 等你的事: the card to answer right on the 首页.
    pub card: Option<MessageItem>,
    /// For 今天做完的: the result card's title when it says more than the topic's.
    pub note: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct HomeData {
    /// 等你的事: asks I may answer, then topics that wait on me without a card.
    pub waiting: Vec<HomeItem>,
    /// 正在做: topics with an active run or in progress.
    pub working: Vec<HomeItem>,
    /// 今天做完的: topics done since `today_start`.
    pub done_today: Vec<HomeItem>,
}

/// A 群 or 私聊 where a member can be handed something.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct HandOffPlace {
    pub group_id: String,
    pub name: String,
    pub is_dm: bool,
}

/// How many asks the 首页 looks at.
const ASKS_SCANNED: usize = 200;
/// How many topics per group the 首页 looks at.
const TOPICS_SCANNED: usize = 60;
/// A topic's title is at most this many characters when 交代 makes it from the text.
const TITLE_CHARS: usize = 40;

fn is_owner(client: &Client) -> bool {
    client
        .me()
        .ok()
        .flatten()
        .is_some_and(|m| m.level == MemberLevel::Owner)
}

/// The values of an answer input: a bare value (2.0), `{value}`, `{values}` (2.1), or an array.
fn input_values(v: &Value) -> Vec<Value> {
    let inner = v
        .as_object()
        .and_then(|o| o.get("values").or_else(|| o.get("value")))
        .unwrap_or(v);
    match inner {
        Value::Array(a) => a.clone(),
        other => vec![other.clone()],
    }
}

fn plain(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(_) | Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// `label：value` lines (and the chosen option labels) of an answer, in card order.
fn answer_details(card: &Value, inputs: Option<&AnswerInputs>) -> Vec<String> {
    let mut out = Vec::new();
    let empty = Vec::new();
    for b in card["blocks"].as_array().unwrap_or(&empty) {
        let Some(id) = b["id"].as_str() else { continue };
        let Some(v) = inputs.and_then(|i| i.get(id)) else {
            continue;
        };
        let options = b["options"].as_array();
        let label_of = |x: &Value| {
            let key = plain(x);
            options
                .and_then(|o| o.iter().find(|o| o["id"].as_str() == Some(key.as_str())))
                .and_then(|o| o["label"].as_str())
                .map(str::to_owned)
                .unwrap_or(key)
        };
        let values: Vec<String> = input_values(v)
            .iter()
            .map(label_of)
            .filter(|s| !s.is_empty())
            .collect();
        if values.is_empty() {
            continue;
        }
        let joined = values.join("、");
        match (b["type"].as_str(), b["label"].as_str()) {
            (Some("input"), Some(label)) => out.push(format!("{label}：{joined}")),
            _ => out.push(joined),
        }
    }
    out
}

/// My answers to a card that the server has not confirmed: `(action id, failed)`.
fn my_pending(client: &Client, message_id: &str) -> Vec<(String, bool)> {
    client
        .store()
        .outbox_all()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            let failed = item.state == OutboxState::Failed;
            match item.op {
                OutboxOp::CardAnswer {
                    message_id: m,
                    action_id,
                    ..
                }
                | OutboxOp::CardAction {
                    message_id: m,
                    action_id,
                    ..
                } if m == message_id => Some((action_id, failed)),
                OutboxOp::CardEvent {
                    message_id: m,
                    action_id,
                    block_id,
                    event,
                    ..
                } if m == message_id && event != "answer.retract" => {
                    Some((action_id.or(block_id).unwrap_or_default(), failed))
                }
                _ => None,
            }
        })
        .collect()
}

fn record_of(client: &Client, dir: &Directory, m: &Message, now: u64) -> CardRecord {
    let mut r = CardRecord {
        state: "none".into(),
        ..Default::default()
    };
    let pending = my_pending(client, &m.id);
    let mine = |id: &str| dir.is_me(id);
    if let Some(card) = &m.card_v2 {
        let raw = serde_json::to_value(card).unwrap_or(Value::Null);
        let label = |action: &str| {
            card.actions()
                .iter()
                .find(|a| a.id == action)
                .map(|a| a.label.clone())
                .unwrap_or_default()
        };
        let declined = |action: &str| {
            card.actions()
                .iter()
                .find(|a| a.id == action)
                .is_some_and(|a| {
                    matches!(
                        a.kind(),
                        chat_core::proto::CardActionKind::Cancel
                            | chat_core::proto::CardActionKind::Reject
                    )
                })
        };
        let Some(ask) = &card.ask else {
            return r;
        };
        r.state = "open".into();
        let answer = client
            .card_answers(&m.id)
            .unwrap_or_default()
            .into_iter()
            .rfind(|a| a.rev == card.rev && a.retracted_at.is_none());
        if let Some(a) = &answer {
            let action = a.action_id.clone();
            r.state = "answered".into();
            r.answered_by = dir.name(&a.actor_id);
            r.answered_by_me = mine(&a.actor_id);
            r.answered_at = a.at;
            r.action_label = label(&action);
            r.declined = declined(&action);
            r.action_id = action;
            r.details = answer_details(&raw, a.inputs.as_ref());
            r.edited = a.edited_values.as_ref().is_some_and(|e| !e.is_empty());
            if r.details.is_empty()
                && let (Some(block), Some(value)) = (&a.block_id, &a.value)
            {
                let inputs: AnswerInputs = [(block.clone(), value.clone())].into();
                r.details = answer_details(&raw, Some(&inputs));
            }
        }
        match m.card_state.as_ref().and_then(|s| s.ask.as_ref()) {
            Some(s) if answer.is_none() => {
                r.state = match s.status {
                    AskStatus::Answered => "answered",
                    AskStatus::Expired => "expired",
                    AskStatus::Closed => "closed",
                    AskStatus::Superseded => "superseded",
                    _ => "open",
                }
                .into();
                if let Some(by) = &s.answered_by {
                    r.answered_by = dir.name(by);
                    r.answered_by_me = mine(by);
                    r.answered_at = s.answered_at.unwrap_or(0);
                }
            }
            _ => {}
        }
        if r.state == "open" && ask.expires_at.is_some_and(|e| e < now) {
            r.state = "expired".into();
        }
        let requester = client
            .store()
            .topic(&m.topic_id)
            .ok()
            .flatten()
            .map(|t| t.topic.created_by);
        let asked = dir.me.as_deref().is_some_and(|me| {
            m.author_id != me && ask.allows(me, is_owner(client), requester.as_deref())
        });
        if let Some((action, failed)) = pending.last() {
            r.sending = !failed;
            r.refused = *failed;
            if r.state == "open" {
                r.action_label = label(action);
                r.declined = declined(action);
                r.action_id = action.clone();
            }
        }
        r.can_answer = asked && r.state == "open" && !r.sending;
        return r;
    }
    if let Some(card) = &m.card {
        let actions = card.actions.clone().unwrap_or_default();
        if actions.is_empty() {
            return r;
        }
        r.state = "open".into();
        let label = |id: &str| {
            actions
                .iter()
                .find(|a| a.id == id)
                .map(|a| a.label.clone())
                .unwrap_or_default()
        };
        let presses = client.card_actions(&m.id).unwrap_or_default();
        let press = presses
            .iter()
            .rfind(|p| mine(&p.actor_id))
            .or(presses.last());
        if let Some(p) = press {
            r.state = "answered".into();
            r.answered_by = dir.name(&p.actor_id);
            r.answered_by_me = mine(&p.actor_id);
            r.answered_at = p.at;
            r.action_label = label(&p.action_id);
            r.action_id = p.action_id.clone();
        }
        if let Some((action, failed)) = pending.last() {
            r.sending = !failed;
            r.refused = *failed;
            if r.state == "open" {
                r.action_label = label(action);
                r.action_id = action.clone();
            }
        }
        r.can_answer = r.state == "open" && !r.sending && !dir.is_me(&m.author_id);
    }
    r
}

fn parse<T: serde::de::DeserializeOwned>(json: Option<String>) -> Result<Option<T>, CoreError> {
    json.filter(|s| !s.trim().is_empty())
        .map(|s| serde_json::from_str(&s).map_err(|_| CoreError::Invalid))
        .transpose()
}

/// The title 交代 gives the new topic: the text's first line, at most [`TITLE_CHARS`].
fn hand_off_title(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default();
    let mut title: String = line.chars().take(TITLE_CHARS).collect();
    if line.chars().count() > TITLE_CHARS {
        title.push('…');
    }
    title
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[uniffi::export]
impl ChatClient {
    /// Where a card message stands for me; `None` when the message is not in the cache.
    pub fn card_record(&self, message_id: String) -> Result<Option<CardRecord>, CoreError> {
        let dir = Directory::read(&self.inner)?;
        Ok(self
            .inner
            .store()
            .message(&message_id)
            .map_err(chat_core::ClientError::from)?
            .map(|m| record_of(&self.inner, &dir, &m, now_ms())))
    }

    /// Answers a v2 card with one of its actions. `rev` is the card's rev I saw; `inputs_json`
    /// and `edits_json` are objects keyed by block id (2.0 bare values, 2.1 `{value}` shapes).
    /// A 2.1 card gets `card.event action.press`, a 2.0 card `card.answer`. Returns the
    /// outbox id.
    pub fn answer_card(
        &self,
        message_id: String,
        rev: u32,
        action_id: String,
        inputs_json: Option<String>,
        edits_json: Option<String>,
    ) -> Result<String, CoreError> {
        let inputs: Option<AnswerInputs> = parse(inputs_json)?;
        let edits: Option<AnswerEdits> = parse(edits_json)?;
        let m = self
            .inner
            .store()
            .message(&message_id)
            .map_err(chat_core::ClientError::from)?
            .ok_or(CoreError::Invalid)?;
        let card = m.card_v2.as_ref().ok_or(CoreError::Invalid)?;
        if card.catalog.as_deref() == Some(CARD_CATALOG_21) {
            let sent = self.inner.send_card_event(CardEventRequest {
                message_id,
                rev,
                action_id: Some(action_id),
                event: "action.press".into(),
                inputs,
                ..Default::default()
            })?;
            return Ok(sent.client_event_id);
        }
        Ok(self
            .inner
            .answer_card(&message_id, rev, &action_id, inputs, edits)?)
    }

    /// Commits one block of a 2.1 card on its own (a `confirm`, an immediate choice):
    /// `event` is `choice.submit`, `input.submit` or `compare.pick`, `value_json` the block's
    /// value. Returns the event id.
    pub fn commit_card_block(
        &self,
        message_id: String,
        rev: u32,
        block_id: String,
        event: String,
        value_json: String,
    ) -> Result<String, CoreError> {
        let value: Value = serde_json::from_str(&value_json).map_err(|_| CoreError::Invalid)?;
        Ok(self
            .inner
            .send_card_event(CardEventRequest {
                message_id,
                rev,
                block_id: Some(block_id),
                event,
                value: Some(value),
                ..Default::default()
            })?
            .client_event_id)
    }

    /// Presses a button of a v1 card (`card.action`). Returns the outbox id.
    pub fn press_card(&self, message_id: String, action_id: String) -> Result<String, CoreError> {
        Ok(self.inner.card_action(&message_id, &action_id)?)
    }

    /// Forgets my refused answer to a card, so I can answer again.
    pub fn forget_card_answer(&self, message_id: String) -> Result<(), CoreError> {
        let failed: Vec<String> = self
            .inner
            .store()
            .outbox_all()
            .map_err(chat_core::ClientError::from)?
            .into_iter()
            .filter(|item| item.state == OutboxState::Failed)
            .filter(|item| match &item.op {
                OutboxOp::CardAnswer { message_id: m, .. }
                | OutboxOp::CardAction { message_id: m, .. }
                | OutboxOp::CardEvent { message_id: m, .. } => *m == message_id,
                _ => false,
            })
            .map(|item| item.id)
            .collect();
        for id in failed {
            self.inner.discard(&id)?;
        }
        Ok(())
    }

    /// The 首页: what waits on me, what is being done, what got done since `today_start` (ms).
    pub fn home(&self, today_start: u64) -> Result<HomeData, CoreError> {
        let client = &self.inner;
        let dir = Directory::read(client)?;
        let me = dir.me.clone().unwrap_or_default();
        let now = now_ms();
        let groups: HashMap<String, (String, bool)> = client
            .groups()?
            .into_iter()
            .map(|g| {
                let is_dm = g.group.kind == chat_protocol::GroupKind::Dm;
                let name = if is_dm || g.group.name.is_empty() {
                    g.group
                        .member_ids
                        .iter()
                        .filter(|id| !dir.is_me(id))
                        .map(|id| dir.name(id))
                        .collect::<Vec<_>>()
                        .join("、")
                } else {
                    g.group.name.clone()
                };
                (g.group.id, (name, is_dm))
            })
            .collect();
        let group_name = |id: &str| groups.get(id).map(|g| g.0.clone()).unwrap_or_default();
        let item = |t: &chat_core::TopicRow, member: &str, at: u64| HomeItem {
            group_id: t.topic.group_id.clone(),
            group_name: group_name(&t.topic.group_id),
            topic_id: t.topic.id.clone(),
            title: t.topic.title.clone(),
            status: crate::wire(&t.topic.status),
            member_id: member.to_string(),
            member_name: dir.name(member),
            member_is_bot: dir
                .member(member)
                .is_some_and(|m| m.kind == MemberKind::Bot),
            at,
            card: None,
            note: String::new(),
        };
        let topic = |id: &str| client.store().topic(id).ok().flatten();

        // 等你的事: open asks of every group that I may answer, newest first.
        let mut waiting = Vec::new();
        let mut asked: HashSet<String> = HashSet::new();
        for m in client.ask_messages(ASKS_SCANNED)? {
            if !groups.contains_key(&m.group_id) || m.author_id == me {
                continue;
            }
            let record = record_of(client, &dir, &m, now);
            if !(record.can_answer || record.sending || record.refused) {
                continue;
            }
            let Some(t) = topic(&m.topic_id) else {
                continue;
            };
            if !asked.insert(t.topic.id.clone()) {
                continue;
            }
            let mut row = item(&t, &m.author_id, m.created_at);
            row.title = m
                .card_v2
                .as_ref()
                .map(|c| c.title.clone())
                .unwrap_or(row.title);
            row.card = Some(message_item(client, &dir, &[], m));
            waiting.push(row);
        }
        for todo in client.todo()? {
            let t = &todo.topic;
            if !todo.reasons.contains(&TodoReason::WaitingOnMe) || asked.contains(&t.topic.id) {
                continue;
            }
            // A v1 card with buttons that I have not pressed is the thing to answer.
            let card = client
                .messages(&t.topic.id, None, 30)?
                .into_iter()
                .rev()
                .find(|m| {
                    m.card_v2.is_none()
                        && m.author_id != me
                        && !m.is_deleted()
                        && m.card
                            .as_ref()
                            .is_some_and(|c| c.actions.as_ref().is_some_and(|a| !a.is_empty()))
                        && record_of(client, &dir, m, now).can_answer
                });
            let from = t
                .topic
                .owner_id
                .clone()
                .flatten()
                .unwrap_or_else(|| t.topic.created_by.clone());
            let mut row = item(t, &from, t.last_activity_at);
            if let Some(m) = card {
                row.member_id = m.author_id.clone();
                row.member_name = dir.name(&m.author_id);
                row.member_is_bot = dir
                    .member(&m.author_id)
                    .is_some_and(|x| x.kind == MemberKind::Bot);
                row.card = Some(message_item(client, &dir, &[], m));
            }
            asked.insert(t.topic.id.clone());
            waiting.push(row);
        }
        waiting.sort_by_key(|r| std::cmp::Reverse(r.at));

        // 正在做: active runs first (who runs it), then other topics in progress.
        let mut working = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for r in client.active_runs()? {
            if !groups.contains_key(&r.group_id)
                || r.is_aside()
                || asked.contains(&r.topic_id)
                || !seen.insert(r.topic_id.clone())
            {
                continue;
            }
            if let Some(t) = topic(&r.topic_id).filter(|t| !t.topic.is_main()) {
                working.push(item(&t, &r.bot_id, r.updated_at));
            }
        }
        let mut done_today = Vec::new();
        let mut done_seen: HashSet<String> = HashSet::new();
        for gid in groups.keys() {
            for t in client.topics(gid, TOPICS_SCANNED)? {
                if t.topic.is_main() || t.topic.context_of.is_some() {
                    continue;
                }
                let doer = t
                    .topic
                    .owner_id
                    .clone()
                    .flatten()
                    .unwrap_or_else(|| t.topic.created_by.clone());
                match t.topic.status {
                    TopicStatus::InProgress
                        if !asked.contains(&t.topic.id) && seen.insert(t.topic.id.clone()) =>
                    {
                        working.push(item(&t, &doer, t.last_activity_at));
                    }
                    TopicStatus::Done
                        if t.last_activity_at >= today_start
                            && done_seen.insert(t.topic.id.clone()) =>
                    {
                        let result = client
                            .messages(&t.topic.id, None, 30)?
                            .into_iter()
                            .rev()
                            .find(|m| {
                                !m.is_deleted()
                                    && m.card_v2
                                        .as_ref()
                                        .is_some_and(|c| c.intent == CardIntent::Done)
                            });
                        let mut row = item(&t, &doer, t.last_activity_at);
                        if let Some(m) = result {
                            row.member_id = m.author_id.clone();
                            row.member_name = dir.name(&m.author_id);
                            row.member_is_bot = dir
                                .member(&m.author_id)
                                .is_some_and(|x| x.kind == MemberKind::Bot);
                            let title = m.card_v2.map(|c| c.title).unwrap_or_default();
                            if title != t.topic.title {
                                row.note = title;
                            }
                        }
                        done_today.push(row);
                    }
                    _ => {}
                }
            }
        }
        working.sort_by_key(|r| std::cmp::Reverse(r.at));
        done_today.sort_by_key(|r| std::cmp::Reverse(r.at));
        Ok(HomeData {
            waiting,
            working,
            done_today,
        })
    }

    /// The 群 and 私聊 where I can hand `member_id` something: groups we are both in, the
    /// 私聊 with them first, then by newest activity.
    pub fn hand_off_places(&self, member_id: String) -> Result<Vec<HandOffPlace>, CoreError> {
        let dir = Directory::read(&self.inner)?;
        let mut rows: Vec<_> = self
            .inner
            .groups()?
            .into_iter()
            .filter(|g| g.group.member_ids.contains(&member_id))
            .collect();
        rows.sort_by_key(|g| {
            (
                g.group.kind != chat_protocol::GroupKind::Dm,
                std::cmp::Reverse(g.last_activity_at),
            )
        });
        Ok(rows
            .into_iter()
            .map(|g| {
                let is_dm = g.group.kind == chat_protocol::GroupKind::Dm;
                HandOffPlace {
                    name: if is_dm || g.group.name.is_empty() {
                        dir.name(&member_id)
                    } else {
                        g.group.name.clone()
                    },
                    group_id: g.group.id,
                    is_dm,
                }
            })
            .collect())
    }

    /// 交代: starts a new 话题 in `group_id` that mentions `member_id`, with `text` as the
    /// request. The title is the text's first line. Returns the message's client id.
    pub fn hand_off(
        &self,
        group_id: String,
        member_id: String,
        text: String,
    ) -> Result<String, CoreError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(CoreError::Invalid);
        }
        let dir = Directory::read(&self.inner)?;
        let name = dir
            .member(&member_id)
            .map(|m| m.name.clone())
            .ok_or(CoreError::Invalid)?;
        Ok(self.inner.send(SendRequest {
            group_id,
            topic_id: None,
            title: Some(hand_off_title(text)),
            body: format!("@{name} {text}"),
            mentions: vec![member_id],
            ..Default::default()
        })?)
    }
}

#[cfg(test)]
mod tests;
