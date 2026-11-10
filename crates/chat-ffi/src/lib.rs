//! The mobile bridge: a small, flat surface over [`chat_core::Client`] that UniFFI turns into
//! Swift. Reads come from the local cache and never wait for the network; writes go to the
//! outbox. The app re-reads after [`UpdateListener::changed`].
//!
//! This is the first slice (sign in with a token, list 群 and 话题, read and send messages).
//! It grows with the mobile client; the protocol types stay in chat-protocol.

use std::sync::Arc;

use chat_core::{Client, ClientConfig, ClientUpdate, ConnectionState};
use chat_protocol::{GroupKind, Member};

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CoreError {
    #[error("{message}")]
    Failed { message: String },
}

impl From<chat_core::ClientError> for CoreError {
    fn from(e: chat_core::ClientError) -> Self {
        CoreError::Failed {
            message: e.to_string(),
        }
    }
}

/// A 群 or a 私聊 in the list.
#[derive(Debug, Clone, uniffi::Record)]
pub struct GroupItem {
    pub id: String,
    /// The group's name; for a 私聊 the other member's name.
    pub name: String,
    pub is_dm: bool,
    pub unread: u64,
    pub mentions: u64,
    pub last_activity_at: u64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct TopicItem {
    pub id: String,
    pub group_id: String,
    pub title: String,
    /// The protocol's `TopicStatus` in its wire form (`open`, `in_progress`, ...).
    pub status: String,
    pub unread: u64,
    pub message_count: u64,
    pub last_activity_at: u64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct MessageItem {
    pub id: String,
    pub author_id: String,
    pub author_name: String,
    pub author_is_bot: bool,
    pub body: String,
    pub created_at: u64,
    pub is_mine: bool,
}

/// What changed, so the app knows what to read again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum UpdateKind {
    Connection,
    Data,
}

/// Called on a background thread; hop to the main thread before touching the UI.
#[uniffi::export(callback_interface)]
pub trait UpdateListener: Send + Sync {
    fn changed(&self, kind: UpdateKind);
}

#[derive(uniffi::Object)]
pub struct ChatClient {
    inner: Client,
}

fn wire<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
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

    pub fn groups(&self) -> Result<Vec<GroupItem>, CoreError> {
        let me = self.inner.me()?.map(|m| m.id);
        let members = self.inner.members()?;
        let name_of = |id: &str| -> Option<String> {
            members
                .iter()
                .find(|m: &&Member| m.id == id)
                .map(|m| m.name.clone())
        };
        let mut out: Vec<GroupItem> = self
            .inner
            .groups()?
            .into_iter()
            .map(|row| {
                let is_dm = row.group.kind == GroupKind::Dm;
                let name = if is_dm || row.group.name.is_empty() {
                    row.group
                        .member_ids
                        .iter()
                        .filter(|id| Some(*id) != me.as_ref())
                        .filter_map(|id| name_of(id))
                        .collect::<Vec<_>>()
                        .join("、")
                } else {
                    row.group.name.clone()
                };
                GroupItem {
                    id: row.group.id,
                    name,
                    is_dm,
                    unread: row.unread,
                    mentions: row.mentions,
                    last_activity_at: row.last_activity_at,
                }
            })
            .collect();
        out.sort_by_key(|g| std::cmp::Reverse(g.last_activity_at));
        Ok(out)
    }

    pub fn topics(&self, group_id: String, limit: u32) -> Result<Vec<TopicItem>, CoreError> {
        Ok(self
            .inner
            .topics(&group_id, limit as usize)?
            .into_iter()
            .map(|row| TopicItem {
                status: wire(&row.topic.status),
                id: row.topic.id,
                group_id: row.topic.group_id,
                title: row.topic.title,
                unread: row.unread,
                message_count: row.topic.message_count,
                last_activity_at: row.last_activity_at,
            })
            .collect())
    }

    /// The newest `limit` messages of a topic, oldest first.
    pub fn messages(&self, topic_id: String, limit: u32) -> Result<Vec<MessageItem>, CoreError> {
        let me = self.inner.me()?.map(|m| m.id);
        let members = self.inner.members()?;
        Ok(self
            .inner
            .messages(&topic_id, None, limit as usize)?
            .into_iter()
            .map(|m| {
                let author = members.iter().find(|x| x.id == m.author_id);
                MessageItem {
                    author_name: author.map(|a| a.name.clone()).unwrap_or_default(),
                    author_is_bot: author.map(|a| wire(&a.kind) == "bot").unwrap_or(false),
                    is_mine: Some(&m.author_id) == me.as_ref(),
                    id: m.id,
                    author_id: m.author_id,
                    body: m.body,
                    created_at: m.created_at,
                }
            })
            .collect())
    }

    /// Queues a text message; returns its client id at once.
    pub fn send_message(
        &self,
        group_id: String,
        topic_id: String,
        body: String,
    ) -> Result<String, CoreError> {
        Ok(self
            .inner
            .send_message(&group_id, &topic_id, &body, vec![])?)
    }

    /// Reports cache and connection changes until the client is dropped.
    pub fn listen(&self, listener: Box<dyn UpdateListener>) {
        let mut rx = self.inner.subscribe();
        std::thread::spawn(move || {
            while let Some(update) = rx.blocking_recv() {
                listener.changed(match update {
                    ClientUpdate::Connection(_) => UpdateKind::Connection,
                    _ => UpdateKind::Data,
                });
            }
        });
    }
}
