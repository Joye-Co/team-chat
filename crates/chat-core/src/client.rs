//! The UI-facing handle. Every method returns at once: writes go to the local outbox and
//! the engine thread sends them; reads are synchronous queries on the local cache.

use crate::engine::{self, Command};
use crate::http::{ApiError, HttpClient};
use crate::proto::{
    ArtifactRef, Card, ErrorCode, Event, FileRef, Member, Message, NotifyReason, SearchHit,
    TopicStatus,
};
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
    /// Largest WebSocket message the client reads. A page of 500 events of the longest
    /// bodies is about 50 MB, so the default is 64 MiB. A larger frame ends the session; the
    /// engine then asks for smaller pages.
    pub max_frame_bytes: usize,
    /// First retry delay of a write refused with `rate_limited` or `internal`; it doubles per
    /// attempt up to 60 s.
    pub write_retry_base: Duration,
    /// Size limit of the downloaded-file cache (`data_dir/files`); least recently used
    /// files are removed past it. Default 2 GiB.
    pub file_cache_max_bytes: u64,
    /// Test hook for end-to-end tests (tools/e2e): a live event for which it returns true
    /// is dropped as if it never arrived. Always `None` in the app.
    pub drop_live_event: Option<DropLiveEvent>,
}

/// `(group_id, seq) -> drop?`; see [`ClientConfig::drop_live_event`].
pub type DropLiveEvent = Arc<dyn Fn(&str, u64) -> bool + Send + Sync>;

impl ClientConfig {
    pub fn new(
        base_url: impl Into<String>,
        token: impl Into<String>,
        data_dir: impl Into<PathBuf>,
    ) -> Self {
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
            max_frame_bytes: 64 << 20,
            write_retry_base: Duration::from_secs(1),
            file_cache_max_bytes: 2 * 1024 * 1024 * 1024,
            drop_live_event: None,
        }
    }

    /// The cache database file inside `data_dir`.
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("cache.sqlite3")
    }

    /// Downloaded files: `files/<sha256>/<name>`.
    pub fn files_dir(&self) -> PathBuf {
        self.data_dir.join("files")
    }

    /// 产出 versions: `artifacts/<artifactId>/<version>/`.
    pub fn artifacts_dir(&self) -> PathBuf {
        self.data_dir.join("artifacts")
    }

    /// Avatar images by URL.
    pub fn avatars_dir(&self) -> PathBuf {
        self.data_dir.join("avatars")
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
    Disconnected {
        retry_in: Duration,
    },
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
    /// The server's specific reason, e.g. `relay.member_unavailable`.
    pub reason: Option<String>,
    /// The members the reason is about (`ErrorBody.memberIds`).
    pub member_ids: Vec<String>,
}

/// Change notices. The UI re-reads the cache for the named scope.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientUpdate {
    Connection(ConnectionState),
    /// Members or the set of groups changed (welcome, directory frame).
    DirectoryChanged,
    /// A group's name, members, unread or mention counts changed.
    GroupChanged {
        group_id: String,
    },
    /// The topic list of a group changed (new topic, title, status, activity, pending new topics).
    TopicsChanged {
        group_id: String,
    },
    /// Messages of a topic changed (new, edited, deleted, streaming text, pending sends).
    MessagesChanged {
        group_id: String,
        topic_id: String,
    },
    /// A `load_older` answer arrived.
    HistoryLoaded {
        group_id: String,
        topic_id: String,
        has_more: bool,
    },
    Typing {
        group_id: String,
        topic_id: String,
    },
    Presence {
        member_id: String,
        online: bool,
    },
    /// A send was acked.
    Sent {
        client_msg_id: String,
        message_id: String,
        topic_id: String,
        duplicate: bool,
    },
    /// The server refused a write; the outbox row is now `Failed`.
    SendFailed(SendFailure),
    /// Every group that was behind after the last `welcome` is caught up.
    CaughtUp,
    /// The server marked a live event for a notification (docs/protocol.md,
    /// Notifications). Sent only for events the server marked, never for my own
    /// events or a muted group. The UI may skip it while the topic is open and focused.
    Notify(Box<Notification>),
    /// Settings transient: the daily digest (每日摘要), counted by event since the last one.
    Digest(Box<crate::proto::DigestTransient>),
    /// The 产出 of a topic changed (event or HTTP answer). Re-read `artifacts(topic)`.
    ArtifactsChanged {
        group_id: String,
        topic_id: String,
    },
    /// Bot runs of a topic changed (`run.updated`). Re-read `runs(topic)`.
    RunsChanged {
        group_id: String,
        topic_id: String,
    },
    /// An applied event that the core cache does not model (`card.actioned`,
    /// `artifact.created`, `artifact.versionAdded`).
    GroupEvent(Box<Event>),
    SearchResults {
        rid: String,
        hits: Vec<SearchHit>,
    },
    /// `code = None`: not connected or no answer.
    SearchFailed {
        rid: String,
        code: Option<ErrorCode>,
        message: String,
    },
    /// 2.1: the server answered one of my card events (`card.event`, `card.answer`, retract).
    /// `Err` with `reason` `client.offline` when it never left this client (live events are
    /// not queued). `ack.error.reason` names the rule (`card.stale_rev`, ...).
    CardEventAcked {
        client_event_id: String,
        message_id: String,
        result: Result<crate::proto::CardEventAck, crate::proto::ErrorBody>,
    },
    /// 1.1: the server answered one of my 产出 page events (`artifact.event`).
    ArtifactEventAcked {
        client_event_id: String,
        artifact_id: String,
        version: u32,
        result: Result<crate::proto::ArtifactEventAck, crate::proto::ErrorBody>,
    },
    /// 2.1 transient: a live event of a card I made (bots; a person's client rarely gets it).
    CardEvent(Box<crate::proto::CardEventTransient>),
    /// 2.1 transient: the author answered one of my events (pending clears).
    InteractionResolved(crate::proto::InteractionResolvedTransient),
    /// 1.1 transient: a page event of a 产出 version I made.
    ArtifactEvent(Box<crate::proto::ArtifactEventTransient>),
    /// 1.1 transient: a new data snapshot of a 产出 version; `data` is absent over 64 KiB
    /// (read it with [`Client::artifact_data`]).
    ArtifactData(Box<crate::proto::ArtifactDataTransient>),
    /// 1.1 transient: the bot's answer to my page `ask` (private to me).
    ArtifactReply(Box<crate::proto::ArtifactReplyTransient>),
}

/// The content of a notification. Texts are data from the cache (names, titles, the
/// message text); the UI adds the words for `reason` from its copy catalog.
#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    pub reason: NotifyReason,
    /// Settings: which switchable event it is (需要我拍板, 成员卡住或出错, ...); `None` from
    /// older servers and for plain messages.
    pub event: Option<crate::proto::NotifyEvent>,
    pub group_id: String,
    pub topic_id: Option<String>,
    pub message_id: Option<String>,
    /// The group name (for a DM the other member's name).
    pub title: String,
    /// The topic title, when known.
    pub subtitle: Option<String>,
    /// Plain text: the message text (at most 200 characters; Markdown left as is),
    /// or the 产出 title; empty for a topic change.
    pub body: String,
    /// Who did it, and the name to show.
    pub actor_id: String,
    pub actor_name: Option<String>,
}

/// A message to queue. `topic_id = None` starts a new topic (`title` optional).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SendRequest {
    pub group_id: String,
    pub topic_id: Option<String>,
    pub title: Option<String>,
    pub body: String,
    pub mentions: Vec<String>,
    /// A bot reply to finish with `send_delta` and an edit with `streaming: false`.
    pub streaming: bool,
    /// Id of the quoted message.
    pub reply_to: Option<String>,
    pub attachments: Option<Vec<FileRef>>,
    pub artifact_refs: Option<Vec<ArtifactRef>>,
    pub card: Option<Card>,
    /// 1.1: a Card v2; the server derives the v1 card when `card` is `None`.
    pub card_v2: Option<crate::proto::CardV2>,
    /// 1.1: a side chat's context (a send in a DM with a bot); see [`Client::start_side_chat`].
    pub context: Option<crate::proto::TopicContext>,
    /// 2.1: the event (`eventId`) this message answers.
    pub cause_event_id: Option<String>,
    /// 1.1: refs to parts (引用提问), at most 5; the server writes their quote lines.
    pub block_refs: Option<Vec<crate::proto::BlockRefInput>>,
    /// 1.1: the side chat answer this send posts to its source topic (`send.fromAside`).
    pub from_aside: Option<crate::proto::AsideOrigin>,
}

/// An edit of my message. `None` fields keep their value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EditRequest {
    pub message_id: String,
    pub body: String,
    pub streaming: bool,
    pub attachments: Option<Vec<FileRef>>,
    pub artifact_refs: Option<Vec<ArtifactRef>>,
    pub card: Option<Card>,
    /// 1.1: a card message only; its `rev` must be higher than the stored one.
    pub card_v2: Option<crate::proto::CardV2>,
}

/// Topic changes; `None` keeps a field. `owner_id: Some(None)` clears the owner.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TopicPatch {
    pub title: Option<String>,
    pub status: Option<TopicStatus>,
    pub waiting_on: Option<Vec<String>>,
    pub owner_id: Option<Option<String>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchRequest {
    pub query: String,
    pub group_id: Option<String>,
    /// Needs `group_id`.
    pub topic_id: Option<String>,
    /// 1..=50; default 20.
    pub limit: Option<u32>,
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
    /// `welcome.features` of the last welcome (1.1).
    pub server_features: Mutex<Vec<String>>,
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

    pub fn set_server_features(&self, f: Vec<String>) {
        *self
            .server_features
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = f;
    }

    pub fn server_features(&self) -> Vec<String> {
        self.server_features
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
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
                server_features: Mutex::new(Vec::new()),
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
        self.shared
            .subscribers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(tx);
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
        let token = self
            .shared
            .token
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        // Before the saved token is read (the Keychain at launch) there is none: a request
        // then fails here with `NoToken` instead of reaching the server as `Bearer ` (401).
        self.http.with_token((!token.is_empty()).then_some(token))
    }

    /// Whether the client has a token yet (it is read from the Keychain after launch).
    pub fn has_token(&self) -> bool {
        !self
            .shared
            .token
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
    }

    pub(crate) fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    pub fn store(&self) -> &Store {
        &self.shared.store
    }

    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    pub fn connection_state(&self) -> ConnectionState {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
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
    pub fn messages(
        &self,
        topic_id: &str,
        before_seq: Option<u64>,
        limit: usize,
    ) -> Result<Vec<Message>, ClientError> {
        let mut v = self
            .shared
            .store
            .messages_before(topic_id, before_seq, limit)?;
        self.merge_streams(&mut v);
        Ok(v)
    }

    /// Up to `limit` messages created after `after_seq`, oldest first.
    pub fn messages_after(
        &self,
        topic_id: &str,
        after_seq: u64,
        limit: usize,
    ) -> Result<Vec<Message>, ClientError> {
        let mut v = self
            .shared
            .store
            .messages_after(topic_id, after_seq, limit)?;
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
        self.shared
            .transient()
            .typing_in(group_id, topic_id, Instant::now())
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

    pub(crate) fn enqueue(&self, id: String, op: OutboxOp) -> Result<String, ClientError> {
        enqueue_with(&self.shared, &self.cmd_tx, id, op)
    }

    /// What a background task needs to queue writes after an await.
    pub(crate) fn command(&self, c: Command) {
        let _ = self.cmd_tx.send(c);
    }

    pub(crate) fn queue(&self) -> (Arc<Shared>, UnboundedSender<Command>) {
        (self.shared.clone(), self.cmd_tx.clone())
    }

    fn check_body(body: &str) -> Result<(), ClientError> {
        if body.chars().count() > MAX_BODY_CHARS {
            return Err(ClientError::Invalid("body too long"));
        }
        Ok(())
    }

    fn check_title(title: Option<&str>) -> Result<(), ClientError> {
        if let Some(t) = title {
            let n = t.chars().count();
            if n == 0 || n > MAX_TITLE_CHARS {
                return Err(ClientError::Invalid("bad title"));
            }
        }
        Ok(())
    }

    /// Queues a message. Without `topic_id` it starts a new topic whose root is this
    /// message; the topic appears when the send is acked. Returns the `clientMsgId`.
    pub fn send(&self, req: SendRequest) -> Result<String, ClientError> {
        self.send_inner(req, None)
    }

    /// Queues a follow-up into a topic that I am starting: `root` is the `clientMsgId` of its
    /// first message and `req.topic_id` is `None`. The row is stored at once (it survives a
    /// quit) and goes out after the first message is acked. When the first message is already
    /// in the cache, this is a plain send into its topic.
    pub fn send_after(&self, root: &str, mut req: SendRequest) -> Result<String, ClientError> {
        if let Some(topic_id) = self.shared.store.my_topic_of_client_msg(root)? {
            req.topic_id = Some(topic_id);
            return self.send_inner(req, None);
        }
        let started = self
            .shared
            .store
            .outbox_get(root)?
            .is_some_and(|i| matches!(i.op, OutboxOp::Send { topic_id: None, .. }));
        if !started {
            return Err(ClientError::NotFound);
        }
        req.topic_id = None;
        req.title = None;
        self.send_inner(req, Some(root.to_string()))
    }

    fn send_inner(
        &self,
        req: SendRequest,
        after_root: Option<String>,
    ) -> Result<String, ClientError> {
        Self::check_body(&req.body)?;
        Self::check_title(req.title.as_deref())?;
        crate::proto::check_block_refs(req.block_refs.as_deref())
            .map_err(|_| ClientError::Invalid("bad block refs"))?;
        // 开个话题: a new topic that quotes a 群聊 message may come without a
        // note; the server titles it from the quote.
        let quotes_into_new_topic = req.topic_id.is_none() && req.reply_to.is_some();
        if req.body.trim().is_empty()
            && !quotes_into_new_topic
            && req.block_refs.is_none()
            && !req.streaming
            && req.card.is_none()
            && req.card_v2.is_none()
            && req.attachments.is_none()
        {
            return Err(ClientError::Invalid("empty message"));
        }
        if req.topic_id.is_some() && req.title.is_some() {
            return Err(ClientError::Invalid("title is only for a new topic"));
        }
        self.enqueue(
            crate::new_id(),
            OutboxOp::Send {
                group_id: req.group_id,
                topic_id: req.topic_id,
                title: req.title,
                body: req.body,
                mentions: req.mentions,
                streaming: req.streaming,
                reply_to: req.reply_to,
                attachments: req.attachments,
                artifact_refs: req.artifact_refs,
                card: req.card,
                card_v2: req.card_v2,
                context: req.context,
                cause_event_id: req.cause_event_id,
                block_refs: req.block_refs,
                from_aside: req.from_aside,
                after_root,
            },
        )
    }

    /// Sends a text message into a topic. Returns its `clientMsgId`.
    pub fn send_message(
        &self,
        group_id: &str,
        topic_id: &str,
        body: &str,
        mentions: Vec<String>,
    ) -> Result<String, ClientError> {
        self.send(SendRequest {
            group_id: group_id.into(),
            topic_id: Some(topic_id.into()),
            body: body.into(),
            mentions,
            ..Default::default()
        })
    }

    /// Starts a new topic with a text message. `title = None` uses the first line.
    pub fn start_topic(
        &self,
        group_id: &str,
        title: Option<&str>,
        body: &str,
        mentions: Vec<String>,
    ) -> Result<String, ClientError> {
        self.send(SendRequest {
            group_id: group_id.into(),
            title: title.map(str::to_string),
            body: body.into(),
            mentions,
            ..Default::default()
        })
    }

    /// Queues an edit of my message. Returns the outbox id.
    pub fn edit(&self, req: EditRequest) -> Result<String, ClientError> {
        Self::check_body(&req.body)?;
        let m = self
            .shared
            .store
            .message(&req.message_id)?
            .ok_or(ClientError::NotFound)?;
        self.enqueue(
            crate::new_id(),
            OutboxOp::Edit {
                group_id: m.group_id,
                topic_id: Some(m.topic_id),
                message_id: req.message_id,
                body: req.body,
                streaming: req.streaming,
                attachments: req.attachments,
                artifact_refs: req.artifact_refs,
                card: req.card,
                card_v2: req.card_v2,
            },
        )
    }

    pub fn edit_message(&self, message_id: &str, body: &str) -> Result<String, ClientError> {
        self.edit(EditRequest {
            message_id: message_id.into(),
            body: body.into(),
            ..Default::default()
        })
    }

    pub fn delete_message(&self, message_id: &str) -> Result<String, ClientError> {
        let m = self
            .shared
            .store
            .message(message_id)?
            .ok_or(ClientError::NotFound)?;
        self.enqueue(
            crate::new_id(),
            OutboxOp::Delete {
                group_id: m.group_id,
                topic_id: Some(m.topic_id),
                message_id: message_id.into(),
            },
        )
    }

    /// Changes a topic's title, status, `waitingOn` or owner.
    pub fn update_topic(&self, topic_id: &str, patch: TopicPatch) -> Result<String, ClientError> {
        Self::check_title(patch.title.as_deref())?;
        if patch == TopicPatch::default() {
            return Err(ClientError::Invalid("nothing to change"));
        }
        if patch
            .waiting_on
            .as_ref()
            .is_some_and(|w| w.len() > crate::proto::MAX_WAITING_ON)
        {
            return Err(ClientError::Invalid("too many waitingOn members"));
        }
        let group_id = self
            .shared
            .store
            .topic_group(topic_id)?
            .ok_or(ClientError::NotFound)?;
        self.enqueue(
            crate::new_id(),
            OutboxOp::TopicUpdate {
                group_id,
                topic_id: topic_id.into(),
                title: patch.title,
                status: patch.status,
                waiting_on: patch.waiting_on,
                owner_id: patch.owner_id,
            },
        )
    }

    /// Follows or unfollows a topic.
    pub fn follow_topic(&self, topic_id: &str, follow: bool) -> Result<String, ClientError> {
        let group_id = self
            .shared
            .store
            .topic_group(topic_id)?
            .ok_or(ClientError::NotFound)?;
        self.enqueue(
            crate::new_id(),
            OutboxOp::TopicFollow {
                group_id,
                topic_id: topic_id.into(),
                follow,
            },
        )
    }

    /// Mutes or unmutes a topic for me: in a muted topic only mentions notify.
    pub fn mute_topic(&self, topic_id: &str, muted: bool) -> Result<String, ClientError> {
        let group_id = self
            .shared
            .store
            .topic_group(topic_id)?
            .ok_or(ClientError::NotFound)?;
        self.enqueue(
            crate::new_id(),
            OutboxOp::TopicMute {
                group_id,
                topic_id: topic_id.into(),
                muted,
            },
        )
    }

    /// Presses a card button. The card's author gets a `card.actioned` event.
    pub fn card_action(&self, message_id: &str, action_id: &str) -> Result<String, ClientError> {
        let m = self
            .shared
            .store
            .message(message_id)?
            .ok_or(ClientError::NotFound)?;
        self.enqueue(
            crate::new_id(),
            OutboxOp::CardAction {
                group_id: m.group_id,
                topic_id: Some(m.topic_id),
                message_id: message_id.into(),
                action_id: action_id.into(),
            },
        )
    }

    /// Adds (`on`) or removes my reaction to a message. The cache changes when
    /// the server's `reaction.changed` arrives; until then
    /// [`Client::pending_reactions`] lists the change.
    pub fn react(&self, message_id: &str, emoji: &str, on: bool) -> Result<String, ClientError> {
        crate::proto::check_emoji("emoji", emoji).map_err(|_| ClientError::Invalid("bad emoji"))?;
        let m = self
            .shared
            .store
            .message(message_id)?
            .ok_or(ClientError::NotFound)?;
        self.enqueue(
            crate::new_id(),
            OutboxOp::React {
                group_id: m.group_id,
                topic_id: Some(m.topic_id),
                message_id: message_id.into(),
                emoji: emoji.into(),
                on,
            },
        )
    }

    /// My reaction changes that the server has not confirmed yet, oldest first:
    /// `(message id, emoji, on)`.
    pub fn pending_reactions(
        &self,
        topic_id: &str,
    ) -> Result<Vec<(String, String, bool)>, ClientError> {
        Ok(self
            .shared
            .store
            .outbox_all()?
            .into_iter()
            .filter_map(|item| match item.op {
                OutboxOp::React {
                    topic_id: Some(t),
                    message_id,
                    emoji,
                    on,
                    ..
                } if t == topic_id && item.state == OutboxState::Pending => {
                    Some((message_id, emoji, on))
                }
                _ => None,
            })
            .collect())
    }

    /// Appends streaming text to my streaming message (no ack, not queued offline).
    pub fn send_delta(&self, group_id: &str, message_id: &str, append: &str) {
        let _ = self.cmd_tx.send(Command::Delta {
            group_id: group_id.into(),
            message_id: message_id.into(),
            append: append.into(),
        });
    }

    /// Searches on the server. Returns a request id; `SearchResults` or `SearchFailed`
    /// with that id follows (at once when offline).
    pub fn search(&self, req: SearchRequest) -> Result<String, ClientError> {
        let n = req.query.chars().count();
        if n == 0 || n > crate::proto::MAX_QUERY_CHARS {
            return Err(ClientError::Invalid("bad query"));
        }
        if req.topic_id.is_some() && req.group_id.is_none() {
            return Err(ClientError::Invalid("topic search needs a group"));
        }
        let rid = format!("s{}", crate::new_id());
        let _ = self.cmd_tx.send(Command::Search {
            rid: rid.clone(),
            req,
        });
        Ok(rid)
    }

    /// Marks a topic read up to `seq` (a message `createdSeq`). Other topics of the
    /// group stay unread.
    pub fn mark_topic_read(
        &self,
        group_id: &str,
        topic_id: &str,
        seq: u64,
    ) -> Result<(), ClientError> {
        self.mark_read_inner(group_id, Some(topic_id), seq)
    }

    /// Moves the group read cursor to `seq`: every topic is read up to it.
    pub fn mark_group_read(&self, group_id: &str, seq: u64) -> Result<(), ClientError> {
        self.mark_read_inner(group_id, None, seq)
    }

    fn mark_read_inner(
        &self,
        group_id: &str,
        topic_id: Option<&str>,
        seq: u64,
    ) -> Result<(), ClientError> {
        if let Some(read) = self.shared.store.mark_read(group_id, topic_id, seq)? {
            self.shared.emit(ClientUpdate::GroupChanged {
                group_id: group_id.into(),
            });
            self.shared.emit(ClientUpdate::TopicsChanged {
                group_id: group_id.into(),
            });
            let _ = self.cmd_tx.send(Command::Read(read));
        }
        Ok(())
    }

    /// Asks the server for older messages of a topic. `HistoryLoaded` follows.
    pub fn load_older(&self, topic_id: &str) -> Result<(), ClientError> {
        let group_id = self
            .shared
            .store
            .topic_group(topic_id)?
            .ok_or(ClientError::NotFound)?;
        let _ = self.cmd_tx.send(Command::LoadOlder {
            group_id,
            topic_id: topic_id.into(),
        });
        Ok(())
    }

    /// Tells the others that I type in a topic (the engine sends at most one per 3 s).
    pub fn typing_in(&self, group_id: &str, topic_id: &str) {
        let _ = self.cmd_tx.send(Command::Typing {
            group_id: group_id.into(),
            topic_id: topic_id.into(),
        });
    }

    /// Puts a failed outbox row back in the queue.
    pub fn retry_failed(&self, id: &str) -> Result<(), ClientError> {
        let item = self
            .shared
            .store
            .outbox_get(id)?
            .ok_or(ClientError::NotFound)?;
        self.shared
            .store
            .outbox_set_state(id, OutboxState::Pending, None)?;
        self.emit_outbox_scope(&item);
        let _ = self.cmd_tx.send(Command::Flush);
        Ok(())
    }

    /// Removes a failed (or pending) outbox row. A pending send that is in flight may
    /// still arrive.
    pub fn discard(&self, id: &str) -> Result<(), ClientError> {
        let item = self
            .shared
            .store
            .outbox_get(id)?
            .ok_or(ClientError::NotFound)?;
        self.shared.store.outbox_remove(id)?;
        // The follow-ups of a topic that is not started any more have nowhere to go.
        if matches!(item.op, OutboxOp::Send { topic_id: None, .. }) {
            for f in self.shared.store.followups_of(id)? {
                self.shared.store.outbox_remove(&f.id)?;
            }
        }
        self.emit_outbox_scope(&item);
        Ok(())
    }

    /// Queued follow-ups of a topic I am starting (`root` is its first message's
    /// `clientMsgId`), oldest first.
    pub fn pending_followups(&self, root: &str) -> Result<Vec<OutboxItem>, ClientError> {
        Ok(self.shared.store.followups_of(root)?)
    }

    fn emit_outbox_scope(&self, item: &OutboxItem) {
        let group_id = item.op.group_id().to_string();
        match item.op.topic_id() {
            Some(t) => self.shared.emit(ClientUpdate::MessagesChanged {
                group_id,
                topic_id: t.into(),
            }),
            None => self.shared.emit(ClientUpdate::TopicsChanged { group_id }),
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(Command::Stop);
        if let Some(rt) = self
            .runtime
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
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

/// Stores one outbox row, tells the UI and wakes the engine.
pub(crate) fn enqueue_with(
    shared: &Shared,
    cmd_tx: &UnboundedSender<Command>,
    id: String,
    op: OutboxOp,
) -> Result<String, ClientError> {
    let group_id = op.group_id().to_string();
    let topic_id = op.topic_id().map(str::to_string);
    shared.store.outbox_push(&OutboxItem {
        id: id.clone(),
        op,
        attempts: 0,
        last_error: None,
        state: OutboxState::Pending,
        created_at: crate::now_ms(),
    })?;
    match topic_id {
        Some(t) => shared.emit(ClientUpdate::MessagesChanged {
            group_id,
            topic_id: t,
        }),
        None => shared.emit(ClientUpdate::TopicsChanged { group_id }),
    }
    let _ = cmd_tx.send(Command::Flush);
    Ok(id)
}
