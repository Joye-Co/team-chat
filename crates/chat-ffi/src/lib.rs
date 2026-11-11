//! The mobile bridge: a small, flat surface over [`chat_core::Client`] that UniFFI turns into
//! Swift. Reads come from the local cache and never wait for the network; writes go to the
//! outbox. The app re-reads the part that [`UpdateListener::changed`] names.
//!
//! The records here are what the screens show, not the protocol: the protocol types stay in
//! chat-protocol. Cards cross as JSON (`MessageItem::card_json`) and are decoded in Swift.

use std::sync::Arc;

use chat_core::proto::{FileRef, Member, MemberKind, Message, MessageKind};
use chat_core::{
    ApiError, Client, ClientConfig, ClientUpdate, ConnectionState, HttpClient, OutboxOp,
    OutboxState, SendRequest, TopicRow,
};
use chat_protocol::{ErrorCode, GroupKind, main_topic_id};

mod auth;
mod devices;
mod pages;
mod token;

pub use auth::{
    SignInCode, SignInPoll, SignInRequest, SignedIn, email_sign_in, parse_sign_in_code,
    password_sign_in, qr_claim, qr_poll, qr_request, redeem_invite, request_email_code,
    sign_in_qr_text,
};
pub use devices::{DeviceItem, OfferState, PasswordState, SignInOffer, device_kind};
pub use pages::*;
pub use token::{clear_token, load_token, save_token};

uniffi::setup_scaffolding!();

/// Why a call failed, as far as the screens tell it apart.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CoreError {
    /// The request did not reach the server.
    #[error("network")]
    Network,
    /// A wrong or expired code, or a sign-in that is no longer valid.
    #[error("unauthorized")]
    Unauthorized,
    /// Too many tries; wait before the next one.
    #[error("rate limited")]
    RateLimited,
    /// The input is not acceptable (a malformed email, an empty body).
    #[error("invalid")]
    Invalid,
    /// The thing asked for is not there (a wrong or used sign-in code).
    #[error("not found")]
    NotFound,
    #[error("{message}")]
    Failed { message: String },
}

impl From<ApiError> for CoreError {
    fn from(e: ApiError) -> Self {
        match e {
            ApiError::Network(_) => CoreError::Network,
            ApiError::Server { code, .. } => match code {
                ErrorCode::Unauthorized => CoreError::Unauthorized,
                ErrorCode::RateLimited => CoreError::RateLimited,
                ErrorCode::Invalid => CoreError::Invalid,
                ErrorCode::NotFound => CoreError::NotFound,
                _ => CoreError::Failed {
                    message: e.to_string(),
                },
            },
            ApiError::BadUrl | ApiError::Invalid(_) => CoreError::Invalid,
            _ => CoreError::Failed {
                message: e.to_string(),
            },
        }
    }
}

impl From<chat_core::ClientError> for CoreError {
    fn from(e: chat_core::ClientError) -> Self {
        match e {
            chat_core::ClientError::Api(api) => api.into(),
            chat_core::ClientError::Invalid(_) => CoreError::Invalid,
            _ => CoreError::Failed {
                message: e.to_string(),
            },
        }
    }
}

impl From<chat_core::TokenStoreError> for CoreError {
    fn from(e: chat_core::TokenStoreError) -> Self {
        CoreError::Failed {
            message: e.to_string(),
        }
    }
}

/// A 成员, for avatars, names and the mention list.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MemberItem {
    pub id: String,
    pub name: String,
    pub is_bot: bool,
    /// What the member does (开发, 运营, ...); empty when not set.
    pub role: String,
    pub online: bool,
    pub is_me: bool,
}

/// A 群 or a 私聊 in the list.
#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct GroupItem {
    pub id: String,
    /// The group's name; for a 私聊 the other member's name.
    pub name: String,
    pub is_dm: bool,
    /// The group's own chat (群聊): what opens when the row is tapped.
    pub main_topic_id: String,
    pub member_count: u32,
    /// For a 私聊: the other member.
    pub peer_id: Option<String>,
    pub peer_is_bot: bool,
    pub unread: u64,
    pub mentions: u64,
    pub muted: bool,
    pub last_activity_at: u64,
    /// The newest message of the group's own chat: `name：text`, or just the text in a 私聊.
    pub preview: String,
    /// The newest 话题 of the group (not its own chat), newest first.
    pub topics: Vec<TopicItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, uniffi::Record)]
pub struct TopicItem {
    pub id: String,
    pub group_id: String,
    pub title: String,
    /// The protocol's `TopicStatus` in its wire form (`open`, `in_progress`, ...).
    pub status: String,
    pub is_main: bool,
    /// The topic waits on me (an answer or a review).
    pub waiting_on_me: bool,
    /// Names of the other members it waits on.
    pub waiting_names: Vec<String>,
    pub unread: u64,
    pub mentions: u64,
    pub message_count: u64,
    pub last_activity_at: u64,
    /// Where I have read to (a message `seq`).
    pub read_seq: u64,
    /// Every older message is in the cache: nothing more to load.
    pub history_complete: bool,
}

/// The message another message quotes.
#[derive(Debug, Clone, uniffi::Record)]
pub struct QuoteItem {
    pub message_id: String,
    pub author_name: String,
    pub excerpt: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ReactionItem {
    pub emoji: String,
    pub count: u32,
    pub mine: bool,
    /// Names of the members who reacted, for the accessibility label.
    pub names: Vec<String>,
}

/// A file or an image of a message.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FileItem {
    pub file_id: String,
    pub name: String,
    pub size: u64,
    pub mime: String,
    pub is_image: bool,
    /// Pixel size, images only; 0 when unknown.
    pub width: u32,
    pub height: u32,
    /// Where the file is on disk when it is already in the cache.
    pub cached_path: Option<String>,
    /// The protocol's file reference; give it back to [`ChatClient::file_path`].
    pub ref_json: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct MessageItem {
    pub id: String,
    pub seq: u64,
    pub author_id: String,
    pub author_name: String,
    pub author_role: String,
    pub author_is_bot: bool,
    pub is_mine: bool,
    /// `text`, `card` or `system`.
    pub kind: String,
    /// Markdown; empty for a deleted message.
    pub body: String,
    pub created_at: u64,
    pub edited: bool,
    pub deleted: bool,
    /// The author is still writing it.
    pub streaming: bool,
    /// Ids of the members the message mentions.
    pub mentions: Vec<String>,
    pub mentions_me: bool,
    pub quote: Option<QuoteItem>,
    pub reactions: Vec<ReactionItem>,
    pub files: Vec<FileItem>,
    /// The card of the message as protocol JSON (Card v2 when present, else v1).
    pub card_json: Option<String>,
}

/// A message of mine that the server has not confirmed.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PendingItem {
    /// Give it to [`ChatClient::retry`] or [`ChatClient::discard`].
    pub id: String,
    pub body: String,
    pub created_at: u64,
    /// The server refused it; it stays until I retry or discard it.
    pub failed: bool,
    pub quote_id: Option<String>,
}

/// What changed, so the app reads only that part again.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum Update {
    Connection,
    /// The members or the set of groups.
    Directory,
    /// One group's name, members or counts.
    Group {
        group_id: String,
    },
    /// The 话题 list of one group.
    Topics {
        group_id: String,
    },
    /// The messages of one 话题 (also my pending and failed sends).
    Messages {
        group_id: String,
        topic_id: String,
    },
    /// Older messages arrived after [`ChatClient::load_older`].
    History {
        topic_id: String,
        has_more: bool,
    },
    Presence {
        member_id: String,
    },
    /// The 产出 of a topic changed.
    Artifacts {
        group_id: String,
        topic_id: String,
    },
    /// Runs of a topic changed (progress, done, failed).
    Runs {
        group_id: String,
        topic_id: String,
    },
    /// The server answered one of my card answers; `reason` names a refusal.
    CardAnswered {
        message_id: String,
        ok: bool,
        reason: Option<String>,
    },
    /// Something the screens of this version do not show.
    Other,
}

/// Called on a background thread; hop to the main thread before touching the UI.
#[uniffi::export(callback_interface)]
pub trait UpdateListener: Send + Sync {
    fn changed(&self, update: Update);
}

#[derive(uniffi::Object)]
pub struct ChatClient {
    inner: Client,
}

/// The wire form of a protocol enum (`in_progress`, `system`, ...).
fn wire<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Names, kinds and who I am, read once per call.
struct Directory {
    me: Option<String>,
    members: Vec<Member>,
}

impl Directory {
    fn read(client: &Client) -> Result<Self, CoreError> {
        Ok(Directory {
            me: client.me()?.map(|m| m.id),
            members: client.members()?,
        })
    }

    fn member(&self, id: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.id == id)
    }

    fn name(&self, id: &str) -> String {
        self.member(id).map(|m| m.name.clone()).unwrap_or_default()
    }

    fn is_me(&self, id: &str) -> bool {
        self.me.as_deref() == Some(id)
    }
}

fn role_of(m: &Member) -> String {
    m.role.clone().flatten().unwrap_or_default()
}

fn topic_item(row: TopicRow, dir: &Directory) -> TopicItem {
    let waiting = row.topic.waiting_on.clone().unwrap_or_default();
    TopicItem {
        status: wire(&row.topic.status),
        is_main: row.topic.is_main(),
        waiting_on_me: waiting.iter().any(|id| dir.is_me(id)),
        waiting_names: waiting
            .iter()
            .filter(|id| !dir.is_me(id))
            .map(|id| dir.name(id))
            .filter(|n| !n.is_empty())
            .collect(),
        id: row.topic.id,
        group_id: row.topic.group_id,
        title: row.topic.title,
        unread: row.unread,
        mentions: row.mentions,
        message_count: row.topic.message_count,
        last_activity_at: row.last_activity_at,
        read_seq: row.read_seq,
        history_complete: row.history_complete,
    }
}

fn file_item(client: &Client, f: &FileRef) -> FileItem {
    FileItem {
        file_id: f.file_id.clone(),
        name: f.name.clone(),
        size: f.size,
        mime: f.mime.clone(),
        is_image: f.mime.starts_with("image/"),
        width: f.width.unwrap_or(0),
        height: f.height.unwrap_or(0),
        cached_path: client
            .cached_file(f)
            .map(|p| p.to_string_lossy().into_owned()),
        ref_json: serde_json::to_string(f).unwrap_or_default(),
    }
}

/// One line for a list: the text without line breaks, or what stands for an empty body.
fn one_line(m: &Message) -> String {
    let text = m.body.split_whitespace().collect::<Vec<_>>().join(" ");
    if !text.is_empty() {
        return text;
    }
    m.attachments
        .as_ref()
        .and_then(|a| a.first())
        .map(|f| f.name.clone())
        .unwrap_or_default()
}

fn message_item(
    client: &Client,
    dir: &Directory,
    pending: &[(String, String, bool)],
    m: Message,
) -> MessageItem {
    let author = dir.member(&m.author_id);
    let mut reactions: Vec<ReactionItem> = m
        .reactions()
        .iter()
        .map(|r| ReactionItem {
            emoji: r.emoji.clone(),
            count: r.member_ids.len() as u32,
            mine: r.member_ids.iter().any(|id| dir.is_me(id)),
            names: r.member_ids.iter().map(|id| dir.name(id)).collect(),
        })
        .collect();
    for (_, emoji, on) in pending.iter().filter(|(id, ..)| *id == m.id) {
        match reactions.iter_mut().find(|r| r.emoji == *emoji) {
            Some(r) if *on && !r.mine => {
                r.mine = true;
                r.count += 1;
            }
            Some(r) if !*on && r.mine => {
                r.mine = false;
                r.count = r.count.saturating_sub(1);
            }
            None if *on => reactions.push(ReactionItem {
                emoji: emoji.clone(),
                count: 1,
                mine: true,
                names: vec![],
            }),
            _ => {}
        }
    }
    reactions.retain(|r| r.count > 0);
    let card_json = match (&m.card_v2, &m.card) {
        (Some(c), _) => serde_json::to_string(c).ok(),
        (None, Some(c)) => serde_json::to_string(c).ok(),
        _ => None,
    };
    MessageItem {
        seq: m.created_seq,
        author_name: author.map(|a| a.name.clone()).unwrap_or_default(),
        author_role: author.map(role_of).unwrap_or_default(),
        author_is_bot: author.is_some_and(|a| a.kind == MemberKind::Bot),
        is_mine: dir.is_me(&m.author_id),
        kind: wire(&m.kind()),
        created_at: m.created_at,
        edited: matches!(m.edited_at, Some(Some(_))),
        deleted: matches!(m.deleted_at, Some(Some(_))),
        streaming: m.streaming == Some(true),
        mentions_me: m.mentions.iter().any(|id| dir.is_me(id)),
        quote: m.reply_to.as_ref().map(|q| QuoteItem {
            message_id: q.message_id.clone(),
            author_name: dir.name(&q.author_id),
            excerpt: q.excerpt.clone(),
        }),
        reactions,
        files: m
            .attachments
            .iter()
            .flatten()
            .map(|f| file_item(client, f))
            .collect(),
        card_json,
        mentions: m.mentions,
        id: m.id,
        author_id: m.author_id,
        body: m.body,
    }
}

#[uniffi::export]
impl ChatClient {
    /// Opens the cache in `data_dir` and builds the runtime. Does not connect yet.
    #[uniffi::constructor]
    pub fn new(base_url: String, token: String, data_dir: String) -> Result<Arc<Self>, CoreError> {
        let mut cfg = ClientConfig::new(base_url, token, data_dir);
        cfg.client_name = "mobile".into();
        Ok(Arc::new(ChatClient {
            inner: Client::new(cfg)?,
        }))
    }

    /// Connects in the background and keeps the connection.
    pub fn start(&self) {
        self.inner.start();
    }

    pub fn stop(&self) {
        self.inner.stop();
    }

    /// Call when the app returns to the foreground or the network changes.
    pub fn reconnect_now(&self) {
        self.inner.reconnect_now();
    }

    /// `stopped`, `connecting`, `connected`, `disconnected`, `unauthorized` or `unsupported`.
    pub fn connection_state(&self) -> String {
        match self.inner.connection_state() {
            ConnectionState::Stopped => "stopped",
            ConnectionState::Connecting => "connecting",
            ConnectionState::Connected => "connected",
            ConnectionState::Disconnected { .. } => "disconnected",
            ConnectionState::Unauthorized => "unauthorized",
            ConnectionState::Unsupported => "unsupported",
        }
        .into()
    }

    pub fn my_name(&self) -> Option<String> {
        self.inner.me().ok().flatten().map(|m| m.name)
    }

    /// Everyone on the team, people first, then by name.
    pub fn members(&self) -> Result<Vec<MemberItem>, CoreError> {
        let dir = Directory::read(&self.inner)?;
        let mut out: Vec<MemberItem> = dir
            .members
            .iter()
            .map(|m| MemberItem {
                id: m.id.clone(),
                name: m.name.clone(),
                is_bot: m.kind == MemberKind::Bot,
                role: role_of(m),
                online: self.inner.is_online(&m.id),
                is_me: dir.is_me(&m.id),
            })
            .collect();
        out.sort_by(|a, b| (a.is_bot, &a.name).cmp(&(b.is_bot, &b.name)));
        Ok(out)
    }

    /// 群 and 私聊, newest activity first, each with its newest 话题.
    pub fn groups(&self) -> Result<Vec<GroupItem>, CoreError> {
        let dir = Directory::read(&self.inner)?;
        let mut out = Vec::new();
        for row in self.inner.groups()? {
            let is_dm = row.group.kind == GroupKind::Dm;
            let others: Vec<&String> = row
                .group
                .member_ids
                .iter()
                .filter(|id| !dir.is_me(id))
                .collect();
            let name = if is_dm || row.group.name.is_empty() {
                others
                    .iter()
                    .map(|id| dir.name(id))
                    .filter(|n| !n.is_empty())
                    .collect::<Vec<_>>()
                    .join("、")
            } else {
                row.group.name.clone()
            };
            let peer = if is_dm { others.first().copied() } else { None };
            let main = main_topic_id(&row.group.id);
            let preview = self
                .inner
                .messages(&main, None, 1)?
                .last()
                .filter(|m| m.kind() != MessageKind::System)
                .map(|m| {
                    let text = one_line(m);
                    if is_dm || dir.is_me(&m.author_id) {
                        text
                    } else {
                        format!("{}：{}", dir.name(&m.author_id), text)
                    }
                })
                .unwrap_or_default();
            let topics = self
                .inner
                .topics(&row.group.id, 8)?
                .into_iter()
                .filter(|t| !t.topic.is_main())
                .take(4)
                .map(|t| topic_item(t, &dir))
                .collect();
            out.push(GroupItem {
                name,
                is_dm,
                main_topic_id: main,
                member_count: row.group.member_ids.len() as u32,
                peer_is_bot: peer
                    .and_then(|id| dir.member(id))
                    .is_some_and(|m| m.kind == MemberKind::Bot),
                peer_id: peer.cloned(),
                unread: row.unread,
                mentions: row.mentions,
                muted: row.muted,
                last_activity_at: row.last_activity_at,
                preview,
                topics,
                id: row.group.id,
            });
        }
        out.sort_by_key(|g| std::cmp::Reverse(g.last_activity_at));
        Ok(out)
    }

    /// The 话题 of a group by newest activity, without the group's own chat.
    pub fn topics(&self, group_id: String, limit: u32) -> Result<Vec<TopicItem>, CoreError> {
        let dir = Directory::read(&self.inner)?;
        Ok(self
            .inner
            .topics(&group_id, limit as usize + 1)?
            .into_iter()
            .filter(|t| !t.topic.is_main())
            .take(limit as usize)
            .map(|t| topic_item(t, &dir))
            .collect())
    }

    /// One 话题 (also a group's own chat), or `None` when it is not in the cache yet.
    pub fn topic(
        &self,
        group_id: String,
        topic_id: String,
    ) -> Result<Option<TopicItem>, CoreError> {
        let dir = Directory::read(&self.inner)?;
        Ok(self
            .inner
            .topics(&group_id, 500)?
            .into_iter()
            .find(|t| t.topic.id == topic_id)
            .map(|t| topic_item(t, &dir)))
    }

    /// The newest `limit` messages of a topic, oldest first. My reaction changes that the
    /// server has not confirmed are already applied.
    pub fn messages(&self, topic_id: String, limit: u32) -> Result<Vec<MessageItem>, CoreError> {
        let dir = Directory::read(&self.inner)?;
        let pending = self.inner.pending_reactions(&topic_id)?;
        Ok(self
            .inner
            .messages(&topic_id, None, limit as usize)?
            .into_iter()
            .map(|m| message_item(&self.inner, &dir, &pending, m))
            .collect())
    }

    /// My messages in a topic that the server has not confirmed, oldest first. Show them
    /// after the messages.
    pub fn pending(&self, topic_id: String) -> Result<Vec<PendingItem>, CoreError> {
        Ok(self
            .inner
            .pending_sends(&topic_id)?
            .into_iter()
            .filter_map(|item| match item.op {
                OutboxOp::Send { body, reply_to, .. } => Some(PendingItem {
                    id: item.id,
                    body,
                    created_at: item.created_at,
                    failed: item.state == OutboxState::Failed,
                    quote_id: reply_to,
                }),
                _ => None,
            })
            .collect())
    }

    /// Queues a message; returns its client id at once. `quote_id` is the message it quotes.
    pub fn send(
        &self,
        group_id: String,
        topic_id: String,
        body: String,
        mentions: Vec<String>,
        quote_id: Option<String>,
    ) -> Result<String, CoreError> {
        Ok(self.inner.send(SendRequest {
            group_id,
            topic_id: Some(topic_id),
            body,
            mentions,
            reply_to: quote_id,
            ..Default::default()
        })?)
    }

    /// Sends a failed message again.
    pub fn retry(&self, id: String) -> Result<(), CoreError> {
        Ok(self.inner.retry_failed(&id)?)
    }

    /// Drops a message that was not sent.
    pub fn discard(&self, id: String) -> Result<(), CoreError> {
        Ok(self.inner.discard(&id)?)
    }

    /// Adds (`on`) or removes my reaction.
    pub fn react(&self, message_id: String, emoji: String, on: bool) -> Result<(), CoreError> {
        self.inner.react(&message_id, &emoji, on)?;
        Ok(())
    }

    /// Marks a topic read up to `seq` (a message's `seq`).
    pub fn mark_read(&self, group_id: String, topic_id: String, seq: u64) -> Result<(), CoreError> {
        Ok(self.inner.mark_topic_read(&group_id, &topic_id, seq)?)
    }

    /// Asks the server for older messages; [`Update::History`] follows.
    pub fn load_older(&self, topic_id: String) -> Result<(), CoreError> {
        Ok(self.inner.load_older(&topic_id)?)
    }

    /// The file on disk: from the cache, or downloaded. Waits for the download, so call it
    /// off the main thread. `ref_json` is [`FileItem::ref_json`].
    pub fn file_path(&self, ref_json: String) -> Result<String, CoreError> {
        let file: FileRef = serde_json::from_str(&ref_json).map_err(|_| CoreError::Invalid)?;
        let task = self.inner.download_file(file, chat_core::no_progress());
        let path = chat_core::client::background()
            .block_on(task)
            .map_err(|e| CoreError::Failed {
                message: e.to_string(),
            })??;
        Ok(path.to_string_lossy().into_owned())
    }

    /// Reports cache and connection changes until the client is dropped.
    pub fn listen(&self, listener: Box<dyn UpdateListener>) {
        let mut rx = self.inner.subscribe();
        std::thread::spawn(move || {
            while let Some(update) = rx.blocking_recv() {
                listener.changed(match update {
                    ClientUpdate::Connection(_) => Update::Connection,
                    ClientUpdate::DirectoryChanged | ClientUpdate::CaughtUp => Update::Directory,
                    ClientUpdate::GroupChanged { group_id } => Update::Group { group_id },
                    ClientUpdate::TopicsChanged { group_id } => Update::Topics { group_id },
                    ClientUpdate::MessagesChanged { group_id, topic_id } => {
                        Update::Messages { group_id, topic_id }
                    }
                    ClientUpdate::HistoryLoaded {
                        topic_id, has_more, ..
                    } => Update::History { topic_id, has_more },
                    ClientUpdate::Presence { member_id, .. } => Update::Presence { member_id },
                    ClientUpdate::ArtifactsChanged { group_id, topic_id } => {
                        Update::Artifacts { group_id, topic_id }
                    }
                    ClientUpdate::RunsChanged { group_id, topic_id } => {
                        Update::Runs { group_id, topic_id }
                    }
                    ClientUpdate::CardEventAcked {
                        message_id, result, ..
                    } => Update::CardAnswered {
                        message_id,
                        ok: result.is_ok(),
                        reason: result.err().and_then(|e| e.reason),
                    },
                    ClientUpdate::SendFailed(f) => match f.topic_id {
                        Some(topic_id) => Update::Messages {
                            group_id: f.group_id,
                            topic_id,
                        },
                        None => Update::Topics {
                            group_id: f.group_id,
                        },
                    },
                    _ => Update::Other,
                });
            }
        });
    }
}

/// A client for the calls that need no sign-in.
fn anonymous(base_url: &str) -> Result<HttpClient, CoreError> {
    Ok(HttpClient::new(base_url, None)?)
}
