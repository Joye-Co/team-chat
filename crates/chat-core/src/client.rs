//! The UI-facing handle. Every method returns at once: writes go to the local outbox and
//! the engine thread sends them; reads are synchronous queries on the local cache.

use crate::engine::{self, Command};
use crate::http::{ApiError, HttpClient};
use crate::proto::{ErrorCode, Member, Message, TopicStatus};
use crate::store::{GroupRow, OutboxItem, OutboxOp, OutboxState, Store, StoreError, TopicRow};
use crate::transient::TransientState;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

pub const MAX_BODY_CHARS: usize = 32_768;
pub const MAX_TITLE_CHARS: usize = 200;

#[derive(Clone)]
pub struct ClientConfig {
    /// Server origin, such as `http://127.0.0.1:8787` or `https://...workers.dev`.
    pub base_url: String,
    /// Bearer token. Never logged.
    pub token: String,
    /// Directory for the cache database and logs.
    pub data_dir: PathBuf,
    /// `hello.client`.
    pub client_name: String,
    pub client_version: String,
    pub ping_interval: Duration,
    pub pong_timeout: Duration,
    /// Deadline for an `ack`, `page` or `history` answer, and for `welcome`.
    pub request_timeout: Duration,
    pub connect_timeout: Duration,
    pub backoff_min: Duration,
    pub backoff_max: Duration,
    /// `limit` of `fetch` pages.
    pub fetch_limit: u32,
    /// `limit` of `history` requests.
    pub history_limit: u32,
}

impl ClientConfig {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>, data_dir: impl Into<PathBuf>) -> Self {
        ClientConfig {
            base_url: base_url.into(),
            token: token.into(),
            data_dir: data_dir.into(),
            client_name: "chat-core".into(),
            client_version: env!("CARGO_PKG_VERSION").into(),
            ping_interval: Duration::from_secs(30),
            pong_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(20),
            connect_timeout: Duration::from_secs(15),
            backoff_min: Duration::from_millis(500),
            backoff_max: Duration::from_secs(30),
            fetch_limit: 500,
            history_limit: 100,
        }
    }

    /// The cache database file inside `data_dir`.
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("cache.sqlite3")
    }
}

impl std::fmt::Debug for ClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientConfig")
            .field("base_url", &self.base_url)
            .field("token", &"<redacted>")
            .field("data_dir", &self.data_dir)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    /// Not started, or stopped.
    Stopped,
    Connecting,
    /// `welcome` arrived; catch-up may still run.
    Connected,
    /// Waiting before the next attempt.
    Disconnected { retry_in: Duration },
    /// The server refused the token. The engine waits for `reconnect_now` or `set_token`.
    Unauthorized,
    /// The server does not support this protocol version. Needs an app update.
    Unsupported,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SendFailure {
    /// Outbox id: the `clientMsgId` of a send.
    pub id: String,
    /// `send`, `edit`, `delete` or `topic_update`.
    pub kind: &'static str,
    pub group_id: String,
    pub topic_id: Option<String>,
    pub code: ErrorCode,
    pub message: String,
}

/// Change notices. The UI re-reads the cache for the named scope.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientUpdate {
    Connection(ConnectionState),
    /// Members or the set of groups changed (welcome, directory frame).
    DirectoryChanged,
    /// A group's name, members, unread or mention counts changed.
    GroupChanged { group_id: String },
    /// The topic list of a group changed (new topic, title, status, activity, pending new topics).
    TopicsChanged { group_id: String },
    /// Messages of a topic changed (new, edited, deleted, streaming text, pending sends).
    MessagesChanged { group_id: String, topic_id: String },
    /// A `load_older` answer arrived.
    HistoryLoaded { group_id: String, topic_id: String, has_more: bool },
    Typing { group_id: String, topic_id: String },
    Presence { member_id: String, online: bool },
    /// A send was acked.
    Sent { client_msg_id: String, message_id: String, topic_id: String, duplicate: bool },
    /// The server refused a write; the outbox row is now `Failed`.
    SendFailed(SendFailure),
    /// Every group that was behind after the last `welcome` is caught up.
    CaughtUp,
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("invalid input: {0}")]
    Invalid(&'static str),
    #[error("not found")]
    NotFound,
    #[error("runtime: {0}")]
    Runtime(String),
    #[error(transparent)]
    Api(#[from] ApiError),
}

pub(crate) struct Shared {
    pub store: Store,
    pub transient: Mutex<TransientState>,
    subscribers: Mutex<Vec<UnboundedSender<ClientUpdate>>>,
    state: Mutex<ConnectionState>,
    pub token: Mutex<String>,
}

impl Shared {
    pub fn emit(&self, u: ClientUpdate) {
        let mut subs = self.subscribers.lock().unwrap_or_else(|e| e.into_inner());
        subs.retain(|s| s.send(u.clone()).is_ok());
    }

    pub fn set_state(&self, s: ConnectionState) {
        let mut cur = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if *cur != s {
            *cur = s.clone();
            drop(cur);
            self.emit(ClientUpdate::Connection(s));
        }
    }

    pub fn transient(&self) -> std::sync::MutexGuard<'_, TransientState> {
        self.transient.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub struct Client {
    shared: Arc<Shared>,
    config: ClientConfig,
    http: HttpClient,
    cmd_tx: UnboundedSender<Command>,
    cmd_rx: Mutex<Option<UnboundedReceiver<Command>>>,
    runtime: Mutex<Option<tokio::runtime::Runtime>>,
    handle: tokio::runtime::Handle,
}

impl Client {
    /// Opens the cache in `config.data_dir` and builds the runtime. Does not connect.
    pub fn new(config: ClientConfig) -> Result<Client, ClientError> {
        let store = Store::open(&config.db_path())?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("chat-core")
            .enable_all()
            .build()
            .map_err(|e| ClientError::Runtime(e.to_string()))?;
        let handle = runtime.handle().clone();
        let http = HttpClient::new(&config.base_url, Some(config.token.clone()))?;
        let (cmd_tx, cmd_rx) = unbounded_channel();
        Ok(Client {
            shared: Arc::new(Shared {
                store,
                transient: Mutex::new(TransientState::default()),
                subscribers: Mutex::new(Vec::new()),
                state: Mutex::new(ConnectionState::Stopped),
                token: Mutex::new(config.token.clone()),
            }),
            config,
            http,
            cmd_tx,
            cmd_rx: Mutex::new(Some(cmd_rx)),
            runtime: Mutex::new(Some(runtime)),
            handle,
        })
    }

    /// Starts the connection and sync engine. A second call does nothing.
    pub fn start(&self) {
        let Some(rx) = self.cmd_rx.lock().unwrap_or_else(|e| e.into_inner()).take() else {
            return;
        };
        let shared = self.shared.clone();
        let config = self.config.clone();
        self.handle.spawn(engine::run(shared, config, rx));
    }

    /// Asks the engine to close the connection and end. Returns at once.
    pub fn stop(&self) {
        let _ = self.cmd_tx.send(Command::Stop);
    }

    /// A receiver of change notices. Works from any executor (`recv().await`) or by
    /// polling with `try_recv`.
    pub fn subscribe(&self) -> UnboundedReceiver<ClientUpdate> {
        let (tx, rx) = unbounded_channel();
        self.shared.subscribers.lock().unwrap_or_else(|e| e.into_inner()).push(tx);
        rx
    }

    /// Runs a future on the core runtime (for HTTP calls). The handle can be awaited
    /// from any executor.
    pub fn spawn<F>(&self, fut: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: std::future::Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.handle.spawn(fut)
    }

    /// HTTP client with this client's token.
    pub fn http(&self) -> HttpClient {
        self.http.clone()
    }

    pub fn store(&self) -> &Store {
        &self.shared.store
    }

    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    pub fn connection_state(&self) -> ConnectionState {
        self.shared.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Reconnects at once, without waiting for the backoff (wake from sleep, network change).
    pub fn reconnect_now(&self) {
        let _ = self.cmd_tx.send(Command::ReconnectNow);
    }

    /// Replaces the token (after a new sign-in) and reconnects.
    pub fn set_token(&self, token: String) {
        *self.shared.token.lock().unwrap_or_else(|e| e.into_inner()) = token;
        let _ = self.cmd_tx.send(Command::ReconnectNow);
    }

    // ------------------------------------------------------------------
    // Reads
    // ------------------------------------------------------------------

    pub fn me(&self) -> Result<Option<Member>, ClientError> {
        Ok(self.shared.store.me()?)
    }

    pub fn members(&self) -> Result<Vec<Member>, ClientError> {
        Ok(self.shared.store.members()?)
    }

    pub fn member(&self, id: &str) -> Result<Option<Member>, ClientError> {
        Ok(self.shared.store.member(id)?)
    }

    /// My groups by last activity, with unread and mention counts.
    pub fn groups(&self) -> Result<Vec<GroupRow>, ClientError> {
        Ok(self.shared.store.groups()?)
    }

    /// Topics of a group by last activity, with per-topic unread counts.
    pub fn topics(&self, group_id: &str, limit: usize) -> Result<Vec<TopicRow>, ClientError> {
        Ok(self.shared.store.topics(group_id, limit)?)
    }

    /// A window of messages of a topic, oldest first: up to `limit` messages created
    /// before `before_seq` (the newest when `None`). Streaming text is merged in.
    pub fn messages(&self, topic_id: &str, before_seq: Option<i64>, limit: usize) -> Result<Vec<Message>, ClientError> {
        let mut v = self.shared.store.messages_before(topic_id, before_seq, limit)?;
        self.merge_streams(&mut v);
        Ok(v)
    }

    /// Up to `limit` messages created after `after_seq`, oldest first.
    pub fn messages_after(&self, topic_id: &str, after_seq: i64, limit: usize) -> Result<Vec<Message>, ClientError> {
        let mut v = self.shared.store.messages_after(topic_id, after_seq, limit)?;
        self.merge_streams(&mut v);
        Ok(v)
    }

    fn merge_streams(&self, v: &mut [Message]) {
        let t = self.shared.transient();
        for m in v.iter_mut() {
            if let Some(s) = t.stream_suffix(&m.id) {
                m.body.push_str(s);
            }
        }
    }

    /// Unacked and failed sends in a topic, oldest first. Show them after the messages.
    pub fn pending_sends(&self, topic_id: &str) -> Result<Vec<OutboxItem>, ClientError> {
        Ok(self.shared.store.pending_sends_in_topic(topic_id)?)
    }

    /// Unacked and failed sends that start new topics in a group.
    pub fn pending_new_topics(&self, group_id: &str) -> Result<Vec<OutboxItem>, ClientError> {
        Ok(self.shared.store.pending_new_topics(group_id)?)
    }

    /// Members typing in a topic now (not me).
    pub fn typing(&self, group_id: &str, topic_id: &str) -> Vec<String> {
        self.shared.transient().typing_in(group_id, topic_id, Instant::now())
    }

    pub fn is_online(&self, member_id: &str) -> bool {
        self.shared.transient().is_online(member_id)
    }

    pub fn online_members(&self) -> Vec<String> {
        self.shared.transient().online_members()
    }

    // ------------------------------------------------------------------
    // Writes (queued; return at once)
    // ------------------------------------------------------------------

    fn enqueue(&self, id: String, op: OutboxOp) -> Result<String, ClientError> {
        let group_id = op.group_id().to_string();
        let topic_id = op.topic_id().map(str::to_string);
        self.shared.store.outbox_push(&OutboxItem {
            id: id.clone(),
            op,
            attempts: 0,
            last_error: None,
            state: OutboxState::Pending,
            created_at: crate::now_ms(),
        })?;
        match topic_id {
            Some(t) => self.shared.emit(ClientUpdate::MessagesChanged { group_id, topic_id: t }),
            None => self.shared.emit(ClientUpdate::TopicsChanged { group_id }),
        }
        let _ = self.cmd_tx.send(Command::Flush);
        Ok(id)
    }

    fn check_body(body: &str) -> Result<(), ClientError> {
        if body.chars().count() > MAX_BODY_CHARS {
            return Err(ClientError::Invalid("body too long"));
        }
        Ok(())
    }

    /// Sends a message into a topic. Returns its `clientMsgId`.
    pub fn send_message(&self, group_id: &str, topic_id: &str, body: &str, mentions: Vec<String>) -> Result<String, ClientError> {
        self.send_message_ext(group_id, topic_id, body, mentions, false)
    }

    /// Like `send_message`; `streaming = true` starts a bot reply to finish with
    /// `send_delta` and `edit_message_ext(.., streaming = false)`.
    pub fn send_message_ext(
        &self,
        group_id: &str,
        topic_id: &str,
        body: &str,
        mentions: Vec<String>,
        streaming: bool,
    ) -> Result<String, ClientError> {
        Self::check_body(body)?;
        if body.trim().is_empty() && !streaming {
            return Err(ClientError::Invalid("empty body"));
        }
        self.enqueue(
            crate::new_id(),
            OutboxOp::Send {
                group_id: group_id.into(),
                topic_id: Some(topic_id.into()),
                title: None,
                body: body.into(),
                mentions,
                streaming,
            },
        )
    }

    /// Starts a new topic whose root is this message. Returns its `clientMsgId`; the
    /// topic appears when the send is acked. `title = None` uses the first line.
    pub fn start_topic(&self, group_id: &str, title: Option<&str>, body: &str, mentions: Vec<String>) -> Result<String, ClientError> {
        Self::check_body(body)?;
        if body.trim().is_empty() {
            return Err(ClientError::Invalid("empty body"));
        }
        if let Some(t) = title {
            let n = t.chars().count();
            if n == 0 || n > MAX_TITLE_CHARS {
                return Err(ClientError::Invalid("bad title"));
            }
        }
        self.enqueue(
            crate::new_id(),
            OutboxOp::Send {
                group_id: group_id.into(),
                topic_id: None,
                title: title.map(str::to_string),
                body: body.into(),
                mentions,
                streaming: false,
            },
        )
    }

    pub fn edit_message(&self, message_id: &str, body: &str) -> Result<String, ClientError> {
        self.edit_message_ext(message_id, body, false)
    }

    pub fn edit_message_ext(&self, message_id: &str, body: &str, streaming: bool) -> Result<String, ClientError> {
        Self::check_body(body)?;
        let m = self.shared.store.message(message_id)?.ok_or(ClientError::NotFound)?;
        self.enqueue(
            crate::new_id(),
            OutboxOp::Edit {
                group_id: m.group_id,
                topic_id: Some(m.topic_id),
                message_id: message_id.into(),
                body: body.into(),
                streaming,
            },
        )
    }

    pub fn delete_message(&self, message_id: &str) -> Result<String, ClientError> {
        let m = self.shared.store.message(message_id)?.ok_or(ClientError::NotFound)?;
        self.enqueue(
            crate::new_id(),
            OutboxOp::Delete { group_id: m.group_id, topic_id: Some(m.topic_id), message_id: message_id.into() },
        )
    }

    pub fn update_topic(&self, topic_id: &str, title: Option<&str>, status: Option<TopicStatus>) -> Result<String, ClientError> {
        if let Some(t) = title {
            let n = t.chars().count();
            if n == 0 || n > MAX_TITLE_CHARS {
                return Err(ClientError::Invalid("bad title"));
            }
        }
        if title.is_none() && status.is_none() {
            return Err(ClientError::Invalid("nothing to change"));
        }
        let group_id = self.shared.store.topic_group(topic_id)?.ok_or(ClientError::NotFound)?;
        self.enqueue(
            crate::new_id(),
            OutboxOp::TopicUpdate {
                group_id,
                topic_id: topic_id.into(),
                title: title.map(str::to_string),
                status,
            },
        )
    }

    /// Appends streaming text to my streaming message (no ack, not queued offline).
    pub fn send_delta(&self, group_id: &str, message_id: &str, append: &str) {
        let _ = self.cmd_tx.send(Command::Delta {
            group_id: group_id.into(),
            message_id: message_id.into(),
            append: append.into(),
        });
    }

    /// Marks a topic read up to `seq` (a message `createdSeq`). When no topic of the
    /// group is unread any more, the group cursor moves and is sent to the server.
    pub fn mark_topic_read(&self, group_id: &str, topic_id: &str, seq: i64) -> Result<(), ClientError> {
        self.mark_read_inner(group_id, Some(topic_id), seq)
    }

    /// Moves the group read cursor to `seq` and sends it to the server.
    pub fn mark_group_read(&self, group_id: &str, seq: i64) -> Result<(), ClientError> {
        self.mark_read_inner(group_id, None, seq)
    }

    fn mark_read_inner(&self, group_id: &str, topic_id: Option<&str>, seq: i64) -> Result<(), ClientError> {
        let sent = self.shared.store.mark_read(group_id, topic_id, seq)?;
        self.shared.emit(ClientUpdate::GroupChanged { group_id: group_id.into() });
        self.shared.emit(ClientUpdate::TopicsChanged { group_id: group_id.into() });
        if let Some(seq) = sent {
            let _ = self.cmd_tx.send(Command::Read { group_id: group_id.into(), seq });
        }
        Ok(())
    }

    /// Asks the server for older messages of a topic. `HistoryLoaded` follows.
    pub fn load_older(&self, topic_id: &str) -> Result<(), ClientError> {
        let group_id = self.shared.store.topic_group(topic_id)?.ok_or(ClientError::NotFound)?;
        let _ = self.cmd_tx.send(Command::LoadOlder { group_id, topic_id: topic_id.into() });
        Ok(())
    }

    /// Tells the others that I type in a topic (the engine sends at most one per 3 s).
    pub fn typing_in(&self, group_id: &str, topic_id: &str) {
        let _ = self.cmd_tx.send(Command::Typing { group_id: group_id.into(), topic_id: topic_id.into() });
    }

    /// Puts a failed outbox row back in the queue.
    pub fn retry_failed(&self, id: &str) -> Result<(), ClientError> {
        let item = self.shared.store.outbox_get(id)?.ok_or(ClientError::NotFound)?;
        self.shared.store.outbox_set_state(id, OutboxState::Pending, None)?;
        self.emit_outbox_scope(&item);
        let _ = self.cmd_tx.send(Command::Flush);
        Ok(())
    }

    /// Removes a failed (or pending) outbox row. A pending send that is in flight may
    /// still arrive.
    pub fn discard(&self, id: &str) -> Result<(), ClientError> {
        let item = self.shared.store.outbox_get(id)?.ok_or(ClientError::NotFound)?;
        self.shared.store.outbox_remove(id)?;
        self.emit_outbox_scope(&item);
        Ok(())
    }

    fn emit_outbox_scope(&self, item: &OutboxItem) {
        let group_id = item.op.group_id().to_string();
        match item.op.topic_id() {
            Some(t) => self.shared.emit(ClientUpdate::MessagesChanged { group_id, topic_id: t.into() }),
            None => self.shared.emit(ClientUpdate::TopicsChanged { group_id }),
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(Command::Stop);
        if let Some(rt) = self.runtime.lock().unwrap_or_else(|e| e.into_inner()).take() {
            rt.shutdown_background();
        }
    }
}

/// A small process-wide runtime for HTTP calls made before a [`Client`] exists
/// (redeem on the sign-in screen).
pub fn background() -> &'static tokio::runtime::Runtime {
    static RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("chat-core-bg")
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}
