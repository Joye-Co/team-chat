//! The chat-core provider: implements [`ChatProvider`] on `chat_core::Client`.
//!
//! - Getters read the chat-core SQLite cache (and its in-memory transient
//!   state) synchronously. They never wait for the network.
//! - Commands go to chat-core's outbox or engine and return at once. Sign-in
//!   and sign-out run on chat-core's background runtime.
//! - A pump thread per client turns `ClientUpdate`s into coalesced [`Change`]
//!   batches, so only the views whose data changed refresh.
//! - A topic started here is shown at once as a pending topic
//!   (`pending:<clientMsgId>`) until the server acks its first message; after
//!   the ack the pending id is an alias of the real topic id.
//! - `ClientUpdate::Notify` becomes a [`Notice`] on [`CoreProvider::notices`];
//!   the app shows it when it is in the background (see `macos.rs`).

pub mod app_glue;
mod cards;
pub mod convert;
pub mod files;
#[cfg(test)]
mod files_live;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod persist;
mod process;
mod realtime;
#[cfg(test)]
mod realtime_live;
#[cfg(test)]
mod search_live;
pub mod selftest_live;
pub mod session;
mod settings;

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use chat_core::{Client, ClientConfig, ClientUpdate, TopicPatch};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

use crate::data::{
    AdminReply, BotCreated, GroupPrefs, InviteCreated, InviteRequest, MachineCreated, MachineInfo,
    MemberLevel, NotifyLevel, RunnerSetup,
};
use crate::{
    data::{
        ArtifactDetail, ArtifactFolder, ArtifactSummary, Change, ChatProvider, ClientMsgId,
        ConnectionState, CreateGroupRequest, CreatedArtifact, Draft, DraftKey, FileAttachment,
        GroupId, GroupSummary, LoginError, LoginRequest, Member, MemberId, MessageId, MessageWindow,
        NewArtifactRequest, NewFile, PendingFileId, RejectedFile, Reply, SavedNavigation,
        SendRequest, SessionState, StartTopicRequest, TodoItem, Topic, TopicFile, TopicStatus,
        TransferState,
    },
    drafts::DraftStore,
};

use convert::{pending_cid, pending_topic_id};
use session::{SavedSession, TokenStorage};

/// Messages added to a topic's window by each `load_older`.
const PAGE: usize = 100;
/// Most topics read per group for the topic list and 待办.
const TOPIC_LIMIT: usize = 500;

/// Where the provider keeps its data and tokens.
#[derive(Clone)]
pub struct ProviderConfig {
    pub data_root: PathBuf,
    pub tokens: TokenStorage,
    /// Restore the saved session at start.
    pub restore: bool,
}

impl ProviderConfig {
    /// The app: Application Support and the Keychain.
    pub fn app() -> Self {
        Self {
            data_root: session::default_data_root(),
            tokens: TokenStorage::Keychain,
            restore: true,
        }
    }
}

/// A notification the app may show (mentions, DMs, topics waiting on me,
/// results of topics I follow). The server decides which events notify.
#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub title: String,
    pub body: String,
    pub group_id: String,
    pub topic_id: Option<String>,
}

struct Account {
    client: Arc<Client>,
    server_url: String,
    me: chat_core::proto::Member,
    dir: PathBuf,
}

#[derive(Default)]
struct State {
    session: Option<SessionState>,
    account: Option<Arc<Account>>,
    /// Bumped on every sign-in and sign-out; stale background work checks it.
    generation: u64,
    ever_connected: bool,
    attempt: u32,
    connection: Option<ConnectionState>,
    /// Extra messages per topic beyond the view's limit (older pages).
    extra: HashMap<String, usize>,
    loading_older: HashSet<String>,
    /// Pending topic id -> real topic id, after the ack.
    aliases: HashMap<String, String>,
    /// Sends into a pending topic whose first message still uploads its files (an upload job,
    /// not an outbox row yet), held until its ack: (pending topic id, request). Sends into a
    /// pending topic whose first message is queued go to the chat-core outbox at once.
    deferred: Vec<(String, ClientMsgId, SendRequest)>,
    /// Acks seen: clientMsgId -> message id (self-tests read it).
    acked: HashMap<String, String>,
    /// The place of the main window, kept in `ui-state.json`.
    navigation: Option<SavedNavigation>,
}

struct Shared {
    config: ProviderConfig,
    state: Mutex<State>,
    subscribers: Mutex<Vec<UnboundedSender<Vec<Change>>>>,
    notices: Mutex<Option<UnboundedSender<Notice>>>,
    drafts: DraftStore,
    writer: persist::Writer,
    files: files::FilesState,
    /// Run logs and machine names for the process view (`process.rs`).
    runs: process::RunCache,
    cards: cards::CardCache,
}

/// [`ChatProvider`] on chat-core. Cheap to clone.
#[derive(Clone)]
pub struct CoreProvider {
    shared: Arc<Shared>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl CoreProvider {
    /// Makes the provider and, with `config.restore`, restores the saved
    /// session from the cache at once (the token is read in the background).
    pub fn new(config: ProviderConfig) -> Arc<Self> {
        let _ = std::fs::create_dir_all(&config.data_root);
        if let Some(guard) = chat_core::logging::init(&config.data_root, "app") {
            // Logs flush until the process ends.
            std::mem::forget(guard);
        }
        let provider = Arc::new(CoreProvider {
            shared: Arc::new(Shared {
                config,
                state: Mutex::new(State::default()),
                subscribers: Mutex::new(Vec::new()),
                notices: Mutex::new(None),
                drafts: DraftStore::new(),
                writer: persist::Writer::new(),
                files: files::FilesState::default(),
                runs: process::RunCache::default(),
                cards: cards::CardCache::default(),
            }),
        });
        if provider.shared.config.restore {
            provider.restore();
        }
        provider
    }

    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.shared.state)
    }

    fn account(&self) -> Option<Arc<Account>> {
        self.state().account.clone()
    }

    fn emit(&self, mut changes: Vec<Change>) {
        if changes.is_empty() {
            return;
        }
        // A pending topic id stays open in the views after its ack: name it too.
        let aliases: Vec<(String, String)> = {
            let s = self.state();
            s.aliases
                .keys()
                .map(|p| (p.clone(), resolve_alias(&s.aliases, p)))
                .collect()
        };
        if !aliases.is_empty() {
            let mut extra = Vec::new();
            for c in &changes {
                if let Change::Messages { group_id, topic_id }
                | Change::Typing { group_id, topic_id } = c
                {
                    for (pending, real) in &aliases {
                        if real == topic_id {
                            extra.push(match c {
                                Change::Messages { .. } => Change::Messages {
                                    group_id: group_id.clone(),
                                    topic_id: pending.clone(),
                                },
                                _ => Change::Typing {
                                    group_id: group_id.clone(),
                                    topic_id: pending.clone(),
                                },
                            });
                        }
                    }
                }
            }
            changes.extend(extra);
        }
        let mut subs = lock(&self.shared.subscribers);
        subs.retain(|s| s.unbounded_send(changes.clone()).is_ok());
    }

    /// Notifications to show; the app drains this on the main thread. A new
    /// call replaces the previous receiver.
    pub fn notices(&self) -> UnboundedReceiver<Notice> {
        let (tx, rx) = unbounded();
        *lock(&self.shared.notices) = Some(tx);
        rx
    }

    /// Reconnects at once (wake from sleep, network change).
    pub fn reconnect_now_inner(&self) {
        let (generation, unreadable) = {
            let s = self.state();
            (s.generation, s.connection == Some(ConnectionState::TokenUnreadable))
        };
        if unreadable && let Some(a) = self.account() {
            tracing::info!("reading the saved token again");
            self.load_token(generation, a.server_url.clone(), a.client.clone());
            return;
        }
        if let Some(a) = self.account() {
            tracing::info!("reconnect requested");
            a.client.reconnect_now();
        }
    }

    /// The message id the server gave a send, once acked.
    pub fn acked_message(&self, client_msg_id: &str) -> Option<String> {
        self.state().acked.get(client_msg_id).cloned()
    }

    /// The real topic id of a pending topic id (or the id itself).
    pub fn resolve_topic(&self, topic_id: &str) -> String {
        resolve_alias(&self.state().aliases, topic_id)
    }

    /// The chat-core client of the signed-in account (self-tests).
    pub fn client(&self) -> Option<Arc<Client>> {
        self.account().map(|a| a.client.clone())
    }

    // ------------------------------------------------------------------
    // Session
    // ------------------------------------------------------------------

    fn restore(&self) {
        let root = self.shared.config.data_root.clone();
        let Some(saved) = session::load_session(&root) else {
            return;
        };
        let dir = session::account_dir(&root, &saved.server_url, &saved.member.id);
        // The token comes from the Keychain in the background; the cache can
        // be read at once.
        let client = match open_client(&saved.server_url, String::new(), &dir) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = %e, "cannot open the cache; signing out");
                return;
            }
        };
        let generation = self.install(saved.clone(), client.clone(), dir);
        self.load_token(generation, saved.server_url, client);
    }

    /// Reads the saved token from the Keychain and starts `client` with it.
    /// A Keychain error keeps the session and the token: it is often a denied
    /// access prompt after an update, or a keychain that is still locked.
    fn load_token(&self, generation: u64, server_url: String, client: Arc<Client>) {
        let this = self.clone();
        let tokens = self.shared.config.tokens.clone();
        chat_core::background().spawn(async move {
            let loaded = tokio_blocking(move || tokens.store(&server_url).load()).await;
            match loaded {
                Ok(Some(token)) => {
                    let mut s = this.state();
                    if s.generation == generation {
                        if s.connection == Some(ConnectionState::TokenUnreadable) {
                            s.connection = None;
                        }
                        drop(s);
                        client.set_token(token);
                        client.start();
                        this.emit(vec![Change::Connection]);
                    }
                }
                Ok(None) => {
                    tracing::info!("no saved token; signing out");
                    this.drop_session(generation, None);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "cannot read the saved token; keeping the session");
                    let mut s = this.state();
                    if s.generation == generation {
                        s.connection = Some(ConnectionState::TokenUnreadable);
                        drop(s);
                        this.emit(vec![Change::Connection]);
                    }
                }
            }
        });
    }

    /// Makes `client` the signed-in account and starts its pump. Returns the
    /// new generation.
    fn install(&self, saved: SavedSession, client: Arc<Client>, dir: PathBuf) -> u64 {
        let me = client
            .me()
            .ok()
            .flatten()
            .filter(|m| m.id == saved.member.id)
            .unwrap_or_else(|| saved.member.clone());
        let updates = client.subscribe();
        self.load_ui_state(&dir);
        let generation = {
            let mut s = self.state();
            s.generation += 1;
            s.account = Some(Arc::new(Account {
                client: client.clone(),
                server_url: saved.server_url.clone(),
                me,
                dir,
            }));
            s.session = None;
            s.connection = None;
            s.ever_connected = false;
            s.attempt = 0;
            s.extra.clear();
            s.loading_older.clear();
            s.aliases.clear();
            s.deferred.clear();
            s.generation
        };
        self.shared.files.reset();
        self.shared.cards.clear();
        self.spawn_pump(generation, updates);
        self.emit(vec![
            Change::Session,
            Change::Connection,
            Change::Members,
            Change::Groups,
            Change::Todo,
        ]);
        generation
    }

    /// Signs out locally: forgets the account and shows the login view.
    /// Returns the account that was signed in.
    fn take_account(&self, error: Option<LoginError>) -> Option<Arc<Account>> {
        let account = {
            let mut s = self.state();
            s.generation += 1;
            s.session = Some(SessionState::SignedOut { last_error: error });
            s.connection = None;
            s.extra.clear();
            s.loading_older.clear();
            s.aliases.clear();
            s.deferred.clear();
            s.navigation = None;
            s.account.take()
        };
        self.shared.drafts.clear_all();
        self.shared.files.reset();
        self.shared.cards.clear();
        if let Some(a) = &account {
            a.client.stop();
        }
        self.emit(vec![
            Change::Session,
            Change::Connection,
            Change::Members,
            Change::Groups,
            Change::Todo,
        ]);
        account
    }

    /// The server refused the token, or it is gone: sign out, keep the cache.
    fn drop_session(&self, generation: u64, error: Option<LoginError>) {
        if self.state().generation != generation {
            return;
        }
        let Some(account) = self.take_account(error) else {
            return;
        };
        let root = self.shared.config.data_root.clone();
        let tokens = self.shared.config.tokens.clone();
        std::thread::spawn(move || {
            let _ = tokens.store(&account.server_url).clear();
            session::clear_session(&root);
            drop(account);
        });
    }

    /// Signs out: revokes this token on the server (best effort), deletes it
    /// from the Keychain, and keeps or wipes the local cache.
    pub fn sign_out_inner(&self, wipe_cache: bool) {
        let Some(account) = self.take_account(None) else {
            return;
        };
        let root = self.shared.config.data_root.clone();
        let tokens = self.shared.config.tokens.clone();
        chat_core::background().spawn(async move {
            // Forget locally first, so a crash during the revoke cannot keep the session.
            session::clear_session(&root);
            let server = account.server_url.clone();
            let token_store = tokens.clone();
            let _ = tokio_blocking(move || token_store.store(&server).clear()).await;
            let http = account.client.http();
            match tokio::time::timeout(Duration::from_secs(5), http.revoke(None, false)).await {
                Ok(Ok(_)) => tracing::info!("token revoked"),
                Ok(Err(e)) => tracing::warn!(error = %e, "revoke failed"),
                Err(_) => tracing::warn!("revoke timed out"),
            }
            let dir = account.dir.clone();
            drop(account);
            if wipe_cache {
                // The engine closes its database as its runtime shuts down.
                tokio::time::sleep(Duration::from_millis(300)).await;
                if let Err(e) = std::fs::remove_dir_all(&dir) {
                    tracing::warn!(error = %e, "cannot remove the cache");
                }
            }
        });
    }

    fn login_inner(&self, request: LoginRequest) {
        let Some(server_url) = session::normalize_server_url(&request.server_url) else {
            let mut s = self.state();
            s.session = Some(SessionState::SignedOut {
                last_error: Some(LoginError::ServerUnreachable),
            });
            drop(s);
            self.emit(vec![Change::Session]);
            return;
        };
        let generation = {
            let mut s = self.state();
            if s.account.is_some() || matches!(s.session, Some(SessionState::SigningIn)) {
                return;
            }
            s.generation += 1;
            s.session = Some(SessionState::SigningIn);
            s.generation
        };
        self.emit(vec![Change::Session]);
        let this = self.clone();
        chat_core::background().spawn(async move {
            let result = this.finish_login(&server_url, &request, generation).await;
            if let Err(error) = result {
                let mut s = this.state();
                if s.generation == generation {
                    s.session = Some(SessionState::SignedOut {
                        last_error: Some(error),
                    });
                    drop(s);
                    this.emit(vec![Change::Session]);
                }
            }
        });
    }

    async fn finish_login(
        &self,
        server_url: &str,
        request: &LoginRequest,
        generation: u64,
    ) -> Result<(), LoginError> {
        let (token, me) =
            session::sign_in(server_url, &request.credential, &request.device_name).await?;
        if self.state().generation != generation {
            return Ok(());
        }
        let tokens = self.shared.config.tokens.clone();
        let root = self.shared.config.data_root.clone();
        let saved = SavedSession {
            server_url: server_url.to_string(),
            member: me.clone(),
        };
        let dir = session::account_dir(&root, server_url, &me.id);
        let (server, t) = (server_url.to_string(), token.clone());
        let saved2 = saved.clone();
        let dir2 = dir.clone();
        let client = tokio_blocking(move || -> Result<Arc<Client>, LoginError> {
            tokens.store(&server).save(&t).map_err(|e| {
                tracing::warn!(error = %e, "cannot save the token");
                LoginError::Other
            })?;
            session::save_session(&root, &saved2).map_err(|e| {
                tracing::warn!(error = %e, "cannot save the session");
                LoginError::Other
            })?;
            open_client(&server, t, &dir2).map_err(|e| {
                tracing::warn!(error = %e, "cannot open the cache");
                LoginError::Other
            })
        })
        .await?;
        if self.state().generation != generation {
            return Ok(());
        }
        tracing::info!(member = %me.id, "signed in");
        self.install(saved, client.clone(), dir);
        client.start();
        Ok(())
    }

    // ------------------------------------------------------------------
    // Updates
    // ------------------------------------------------------------------

    fn spawn_pump(
        &self,
        generation: u64,
        mut updates: tokio::sync::mpsc::UnboundedReceiver<ClientUpdate>,
    ) {
        let this = self.clone();
        let spawned = std::thread::Builder::new()
            .name("provider-pump".into())
            .spawn(move || {
                while let Some(first) = updates.blocking_recv() {
                    if this.state().generation != generation {
                        break;
                    }
                    let mut batch = vec![first];
                    while let Ok(u) = updates.try_recv() {
                        batch.push(u);
                        if batch.len() >= 256 {
                            break;
                        }
                    }
                    let mut changes = Vec::new();
                    for u in batch {
                        this.on_update(generation, u, &mut changes);
                    }
                    if this.state().generation != generation {
                        break;
                    }
                    let mut seen = HashSet::new();
                    changes.retain(|c| seen.insert(c.clone()));
                    this.emit(changes);
                }
            });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "cannot start the update thread");
        }
    }

    fn on_update(&self, generation: u64, u: ClientUpdate, out: &mut Vec<Change>) {
        match u {
            ClientUpdate::Connection(c) => {
                use chat_core::ConnectionState as C;
                match c {
                    C::Unauthorized => {
                        tracing::info!("the server refused the token");
                        let this = self.clone();
                        // Not on the pump thread: it ends with this generation.
                        std::thread::spawn(move || {
                            this.drop_session(generation, Some(LoginError::BadCredential))
                        });
                        return;
                    }
                    other => {
                        let mut s = self.state();
                        let state = match other {
                            C::Connected => {
                                s.ever_connected = true;
                                s.attempt = 0;
                                ConnectionState::Online
                            }
                            C::Connecting if !s.ever_connected && s.attempt == 0 => {
                                ConnectionState::Connecting
                            }
                            C::Connecting => ConnectionState::Reconnecting {
                                attempt: s.attempt.max(1),
                                retry_in_ms: 0,
                            },
                            C::Disconnected { retry_in } => {
                                s.attempt = s.attempt.saturating_add(1);
                                ConnectionState::Reconnecting {
                                    attempt: s.attempt,
                                    retry_in_ms: retry_in.as_millis() as u64,
                                }
                            }
                            C::Stopped | C::Unsupported | C::Unauthorized => {
                                ConnectionState::Offline
                            }
                        };
                        s.connection = Some(state);
                    }
                }
                out.push(Change::Connection);
            }
            ClientUpdate::DirectoryChanged => {
                out.extend([
                    Change::Members,
                    Change::Groups,
                    Change::Todo,
                    Change::Session,
                ]);
            }
            ClientUpdate::GroupChanged { group_id } => {
                out.extend([Change::Groups, Change::Todo, Change::Topics { group_id }]);
            }
            ClientUpdate::TopicsChanged { group_id } => {
                out.extend([Change::Topics { group_id }, Change::Todo]);
            }
            ClientUpdate::MessagesChanged { group_id, topic_id } => {
                out.push(Change::Messages { group_id, topic_id });
            }
            ClientUpdate::HistoryLoaded {
                group_id, topic_id, ..
            } => {
                let mut s = self.state();
                if s.loading_older.remove(&topic_id) {
                    *s.extra.entry(topic_id.clone()).or_default() += PAGE;
                }
                out.push(Change::Messages { group_id, topic_id });
            }
            ClientUpdate::Typing { group_id, topic_id } => {
                out.push(Change::Typing { group_id, topic_id });
            }
            ClientUpdate::Presence { .. } => {
                out.extend([Change::Members, Change::Groups]);
            }
            ClientUpdate::Sent {
                client_msg_id,
                message_id,
                topic_id,
                ..
            } => {
                let pending = pending_topic_id(&client_msg_id);
                // The first message of a topic I started: the pending id becomes an alias.
                let is_root = self.account().is_some_and(|a| {
                    a.client
                        .store()
                        .topic(&topic_id)
                        .ok()
                        .flatten()
                        .is_some_and(|t| t.topic.root_message_id == message_id)
                });
                let flush = {
                    let mut s = self.state();
                    s.acked.insert(client_msg_id, message_id);
                    if s.acked.len() > 1000 {
                        s.acked.clear();
                    }
                    if is_root {
                        s.aliases.insert(pending.clone(), topic_id.clone());
                    }
                    let (ready, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut s.deferred)
                        .into_iter()
                        .partition(|(p, _, _)| {
                            *p == pending || resolve_alias(&s.aliases, p) == topic_id
                        });
                    s.deferred = rest;
                    ready
                };
                if let Some(group_id) = self.group_of_topic(&topic_id) {
                    out.extend([
                        Change::Messages {
                            group_id: group_id.clone(),
                            topic_id: topic_id.clone(),
                        },
                        Change::Messages {
                            group_id: group_id.clone(),
                            topic_id: pending,
                        },
                        Change::Topics { group_id },
                    ]);
                }
                for (_, _, mut req) in flush {
                    req.topic_id = topic_id.clone();
                    self.send_now(req);
                }
            }
            ClientUpdate::SendFailed(f) => {
                tracing::warn!(kind = f.kind, code = ?f.code, "write refused");
                if f.kind == "card_action"
                    && let Some(a) = self.account()
                {
                    self.shared.cards.on_refused(&a.client, &f.id, f.code);
                }
                match f.topic_id {
                    Some(topic_id) => out.push(Change::Messages {
                        group_id: f.group_id,
                        topic_id,
                    }),
                    None => out.push(Change::Topics {
                        group_id: f.group_id,
                    }),
                }
            }
            ClientUpdate::CaughtUp => out.extend([Change::Groups, Change::Todo]),
            ClientUpdate::Notify(n) => {
                let notice = self.notice_from(&n);
                if let Some(tx) = lock(&self.shared.notices).as_ref() {
                    let _ = tx.unbounded_send(notice);
                }
            }
            ClientUpdate::ArtifactsChanged { group_id, topic_id } => {
                // Cards in the timeline show the 产出 title and version too.
                out.extend([
                    Change::Artifacts {
                        group_id: group_id.clone(),
                        topic_id: topic_id.clone(),
                    },
                    Change::Messages { group_id, topic_id },
                ]);
            }
            ClientUpdate::RunsChanged { group_id, topic_id } => {
                out.push(Change::Runs { group_id, topic_id });
            }
            ClientUpdate::GroupEvent(ev) => {
                if let chat_core::proto::EventData::CardActioned(d) = &ev.data
                    && let Some(a) = self.account()
                {
                    let mine = ev.actor_id == a.me.id;
                    self.shared
                        .cards
                        .on_actioned(&d.message_id, mine, &d.action_id);
                    if let Some(m) = a.client.store().message(&d.message_id).ok().flatten() {
                        out.push(Change::Messages {
                            group_id: m.group_id,
                            topic_id: m.topic_id,
                        });
                    }
                }
            }
            ClientUpdate::SearchResults { .. } | ClientUpdate::SearchFailed { .. } => {}
        }
    }

    fn group_of_topic(&self, topic_id: &str) -> Option<String> {
        let a = self.account()?;
        a.client.store().topic_group(topic_id).ok().flatten()
    }

    fn send_now(&self, req: SendRequest) -> ClientMsgId {
        let Some(a) = self.account() else {
            return String::new();
        };
        let result = a.client.send(chat_core::SendRequest {
            group_id: req.group_id,
            topic_id: Some(req.topic_id),
            body: req.body,
            mentions: req.mentions,
            reply_to: req.quote_of,
            ..Default::default()
        });
        result.unwrap_or_else(|e| {
            tracing::warn!(error = %e, "send refused");
            String::new()
        })
    }

    fn topics_of(&self, a: &Account, group_id: &str) -> Vec<Topic> {
        let me = a.me.id.as_str();
        let mut out: Vec<Topic> = a
            .client
            .pending_new_topics(group_id)
            .unwrap_or_default()
            .iter()
            .filter_map(|item| convert::pending_topic(item, me))
            .collect();
        out.reverse();
        let mut jobs = self.job_topics(group_id, me);
        jobs.append(&mut out);
        let mut out = jobs;
        match a.client.topics(group_id, TOPIC_LIMIT) {
            Ok(rows) => out.extend(rows.iter().map(convert::topic)),
            Err(e) => tracing::warn!(error = %e, "topics read failed"),
        }
        out
    }
}

/// Follows pending-topic aliases (a job's topic, then the queued send's
/// topic, then the real topic).
fn resolve_alias(aliases: &HashMap<String, String>, topic_id: &str) -> String {
    let mut id = topic_id;
    for _ in 0..4 {
        match aliases.get(id) {
            Some(next) => id = next,
            None => break,
        }
    }
    id.to_string()
}

/// Runs blocking work (Keychain, files, SQLite open) off the async threads.
async fn tokio_blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.await.expect("blocking task ended")
}

fn open_client(
    server_url: &str,
    token: String,
    dir: &std::path::Path,
) -> Result<Arc<Client>, chat_core::ClientError> {
    std::fs::create_dir_all(dir).map_err(|e| chat_core::ClientError::Runtime(e.to_string()))?;
    let mut cfg = ClientConfig::new(server_url, token, dir);
    cfg.client_name = env!("PRODUCT_SLUG").into();
    cfg.client_version = env!("CARGO_PKG_VERSION").into();
    Ok(Arc::new(Client::new(cfg)?))
}

impl ChatProvider for CoreProvider {
    fn session(&self) -> SessionState {
        let s = self.state();
        if let Some(a) = &s.account {
            // The cached row has my latest name, 岗位 and avatar.
            let me = a.client.member(&a.me.id).ok().flatten();
            return SessionState::SignedIn {
                me: convert::member(me.as_ref().unwrap_or(&a.me), true),
                server_url: a.server_url.clone(),
            };
        }
        s.session
            .clone()
            .unwrap_or(SessionState::SignedOut { last_error: None })
    }

    fn login(&self, request: LoginRequest) {
        self.login_inner(request);
    }

    fn logout(&self) {
        self.sign_out_inner(false);
    }

    fn sign_out(&self, wipe_cache: bool) {
        self.sign_out_inner(wipe_cache);
    }

    fn reconnect_now(&self) {
        self.reconnect_now_inner();
    }

    fn connection(&self) -> ConnectionState {
        let s = self.state();
        match (&s.account, s.connection) {
            (None, _) => ConnectionState::Offline,
            (Some(_), Some(c)) => c,
            (Some(_), None) => ConnectionState::Connecting,
        }
    }

    fn members(&self) -> Vec<Member> {
        let Some(a) = self.account() else {
            return Vec::new();
        };
        let online: HashSet<String> = a.client.online_members().into_iter().collect();
        a.client
            .members()
            .unwrap_or_default()
            .iter()
            .map(|m| convert::member(m, online.contains(&m.id) || m.id == a.me.id))
            .collect()
    }

    fn member(&self, id: &str) -> Option<Member> {
        let a = self.account()?;
        let m = a.client.member(id).ok().flatten()?;
        Some(convert::member(
            &m,
            m.id == a.me.id || a.client.is_online(id),
        ))
    }

    fn groups(&self) -> Vec<GroupSummary> {
        let Some(a) = self.account() else {
            return Vec::new();
        };
        let names: HashMap<String, String> = a
            .client
            .members()
            .unwrap_or_default()
            .into_iter()
            .map(|m| (m.id, m.name))
            .collect();
        let mut out: Vec<GroupSummary> = a
            .client
            .groups()
            .unwrap_or_default()
            .iter()
            .map(|row| convert::group(row, &a.me.id, |id| names.get(id).cloned()))
            .collect();
        // A topic I just started counts as activity.
        for g in out.iter_mut() {
            if let Some(newest) = a
                .client
                .pending_new_topics(&g.id)
                .unwrap_or_default()
                .last()
            {
                g.last_activity_at = g.last_activity_at.max(newest.created_at as i64);
            }
        }
        out.sort_by_key(|x| std::cmp::Reverse(x.last_activity_at));
        out
    }

    fn topics(&self, group_id: &str) -> Vec<Topic> {
        match self.account() {
            Some(a) => self.topics_of(&a, group_id),
            None => Vec::new(),
        }
    }

    fn topic(&self, group_id: &str, topic_id: &str) -> Option<Topic> {
        let a = self.account()?;
        let real = self.resolve_topic(topic_id);
        if let Some(cid) = pending_cid(&real) {
            if let Some(t) = self.job_topic_of(cid, &a.me.id) {
                return Some(t);
            }
            let item = a.client.store().outbox_get(cid).ok().flatten()?;
            return convert::pending_topic(&item, &a.me.id);
        }
        let row = a.client.store().topic(&real).ok().flatten()?;
        (row.topic.group_id == group_id).then(|| convert::topic(&row))
    }

    fn todo(&self) -> Vec<TodoItem> {
        self.todo_items()
    }

    fn messages(&self, group_id: &str, topic_id: &str, limit: usize) -> MessageWindow {
        let Some(a) = self.account() else {
            return MessageWindow::default();
        };
        let me = a.me.id.as_str();
        let real = self.resolve_topic(topic_id);
        let (extra, loading, deferred) = {
            let s = self.state();
            let deferred: Vec<_> = s
                .deferred
                .iter()
                .filter(|(p, _, _)| *p == topic_id || *p == real)
                .map(|(_, cid, req)| (cid.clone(), req.clone()))
                .collect();
            (
                s.extra.get(&real).copied().unwrap_or(0),
                s.loading_older.contains(&real),
                deferred,
            )
        };
        let mut window = MessageWindow {
            loading_older: loading,
            ..Default::default()
        };
        if let Some(cid) = pending_cid(&real) {
            // Only the queued first message exists.
            if self.job_topic_of(cid, me).is_some() {
                // Its files still upload: the message comes from the job below.
            } else if let Some(item) = a.client.store().outbox_get(cid).ok().flatten() {
                window
                    .messages
                    .extend(convert::outbox_message(&item, topic_id, me));
            }
            // Follow-ups queued behind the first message.
            for item in a.client.pending_followups(cid).unwrap_or_default() {
                window
                    .messages
                    .extend(convert::outbox_message(&item, topic_id, me));
            }
        } else {
            let n = limit.max(1) + extra;
            let mut msgs = a.client.messages(&real, None, n + 1).unwrap_or_default();
            self.shared.cards.remember(&msgs);
            let cached_more = msgs.len() > n;
            if cached_more {
                msgs.remove(0);
            }
            let complete = a
                .client
                .store()
                .topic(&real)
                .ok()
                .flatten()
                .is_none_or(|t| t.history_complete);
            window.has_older = cached_more || (!complete && !msgs.is_empty());
            let known: HashSet<String> = msgs.iter().map(|m| m.client_msg_id.clone()).collect();
            window
                .messages
                .extend(msgs.iter().map(convert::message).map(|mut m| {
                    m.topic_id = topic_id.to_string();
                    m
                }));
            // My reactions that the server has not confirmed yet show at once.
            for (message_id, emoji, on) in a.client.pending_reactions(&real).unwrap_or_default() {
                if let Some(m) = window.messages.iter_mut().find(|m| m.id == message_id) {
                    crate::data::apply_reaction(&mut m.reactions, &emoji, me, on);
                }
            }
            for item in a.client.pending_sends(&real).unwrap_or_default() {
                if known.contains(&item.id) {
                    continue;
                }
                window
                    .messages
                    .extend(convert::outbox_message(&item, topic_id, me));
            }
        }
        window
            .messages
            .extend(self.job_messages(group_id, topic_id, &real, me));
        for (cid, req) in deferred {
            window.messages.push(crate::data::Message {
                id: String::new(),
                client_msg_id: cid,
                group_id: group_id.to_string(),
                topic_id: topic_id.to_string(),
                author_id: me.to_string(),
                body: req.body,
                mentions: req.mentions,
                created_seq: 0,
                created_at: chat_core::now_ms() as i64,
                edited_at: None,
                deleted_at: None,
                streaming: false,
                send_state: crate::data::SendState::Pending,
                quote_of: req.quote_of,
                attachments: Vec::new(),
                artifact_refs: Vec::new(),
                reactions: Vec::new(),
            });
        }
        window
    }

    fn load_older(&self, group_id: &str, topic_id: &str) {
        let Some(a) = self.account() else {
            return;
        };
        let real = self.resolve_topic(topic_id);
        if pending_cid(&real).is_some() {
            return;
        }
        let cached = a.client.store().message_count_cached(&real).unwrap_or(0) as usize;
        let mut s = self.state();
        if s.loading_older.contains(&real) {
            return;
        }
        let shown = s.extra.get(&real).copied().unwrap_or(0) + PAGE;
        if cached > shown {
            // The cache has more: grow the window without the network.
            *s.extra.entry(real.clone()).or_default() += PAGE;
            drop(s);
            self.emit(vec![Change::Messages {
                group_id: group_id.into(),
                topic_id: real,
            }]);
            return;
        }
        if a.client.load_older(&real).is_ok() {
            s.loading_older.insert(real.clone());
            drop(s);
            self.emit(vec![Change::Messages {
                group_id: group_id.into(),
                topic_id: real,
            }]);
        }
    }

    fn typing(&self, group_id: &str, topic_id: &str) -> Vec<MemberId> {
        match self.account() {
            Some(a) => a.client.typing(group_id, &self.resolve_topic(topic_id)),
            None => Vec::new(),
        }
    }

    fn artifacts(&self, _group_id: &str, topic_id: &str) -> Vec<ArtifactSummary> {
        self.artifacts_inner(topic_id)
    }

    fn send(&self, request: SendRequest) -> ClientMsgId {
        let real = self.resolve_topic(&request.topic_id);
        if let Some(root) = pending_cid(&real)
            && let Some(a) = self.account()
        {
            // The topic has no server id yet: queue a follow-up in the outbox. It is stored at
            // once and goes out after the first message.
            let queued = a.client.send_after(
                root,
                chat_core::SendRequest {
                    group_id: request.group_id.clone(),
                    body: request.body.clone(),
                    mentions: request.mentions.clone(),
                    reply_to: request.quote_of.clone(),
                    ..Default::default()
                },
            );
            match queued {
                Ok(cid) => {
                    self.emit(vec![Change::Messages {
                        group_id: request.group_id,
                        topic_id: request.topic_id,
                    }]);
                    return cid;
                }
                Err(chat_core::ClientError::NotFound) => {}
                Err(e) => {
                    tracing::warn!(error = %e, "send refused");
                    return String::new();
                }
            }
        }
        if pending_cid(&real).is_some() {
            // The first message still uploads its files: hold the message until its ack.
            let cid = chat_core::new_id();
            let group_id = request.group_id.clone();
            let topic_id = request.topic_id.clone();
            self.state().deferred.push((real, cid.clone(), request));
            self.emit(vec![Change::Messages { group_id, topic_id }]);
            return cid;
        }
        self.send_now(SendRequest {
            topic_id: real,
            ..request
        })
    }

    fn start_topic(&self, request: StartTopicRequest) -> ClientMsgId {
        let Some(a) = self.account() else {
            return String::new();
        };
        let title = Some(convert::topic_title(
            request.title.as_deref(),
            &request.body,
        ))
        .filter(|t| !t.is_empty());
        let result = a.client.send(chat_core::SendRequest {
            group_id: request.group_id.clone(),
            topic_id: None,
            title,
            body: request.body,
            mentions: request.mentions,
            ..Default::default()
        });
        match result {
            Ok(cid) => {
                self.emit(vec![
                    Change::Topics {
                        group_id: request.group_id,
                    },
                    Change::Groups,
                ]);
                cid
            }
            Err(e) => {
                tracing::warn!(error = %e, "start topic refused");
                String::new()
            }
        }
    }

    fn edit(&self, _group_id: &str, message_id: &str, body: String) {
        if let Some(a) = self.account()
            && let Err(e) = a.client.edit_message(message_id, &body)
        {
            tracing::warn!(error = %e, "edit refused");
        }
    }

    fn delete(&self, _group_id: &str, message_id: &str) {
        if let Some(a) = self.account()
            && let Err(e) = a.client.delete_message(message_id)
        {
            tracing::warn!(error = %e, "delete refused");
        }
    }

    fn react(&self, group_id: &str, topic_id: &str, message_id: &str, emoji: &str, on: bool) {
        if let Some(a) = self.account() {
            if let Err(e) = a.client.react(message_id, emoji, on) {
                tracing::warn!(error = %e, "reaction refused");
                return;
            }
            self.emit(vec![Change::Messages {
                group_id: group_id.to_string(),
                topic_id: topic_id.to_string(),
            }]);
        }
    }

    fn rename_topic(&self, _group_id: &str, topic_id: &str, title: String) {
        let Some(a) = self.account() else {
            return;
        };
        let real = self.resolve_topic(topic_id);
        if pending_cid(&real).is_some() {
            return;
        }
        let patch = TopicPatch {
            title: Some(title),
            ..Default::default()
        };
        if let Err(e) = a.client.update_topic(&real, patch) {
            tracing::warn!(error = %e, "topic rename refused");
        }
    }

    fn set_topic_status(
        &self,
        _group_id: &str,
        topic_id: &str,
        status: TopicStatus,
        waiting_on: Vec<MemberId>,
    ) {
        let Some(a) = self.account() else {
            return;
        };
        let real = self.resolve_topic(topic_id);
        if pending_cid(&real).is_some() {
            return;
        }
        let patch = TopicPatch {
            status: Some(convert::status_to_wire(status)),
            waiting_on: Some(waiting_on),
            ..Default::default()
        };
        if let Err(e) = a.client.update_topic(&real, patch) {
            tracing::warn!(error = %e, "topic update refused");
        }
    }

    fn mark_read(&self, group_id: &str, topic_id: &str, seq: u64) {
        let real = self.resolve_topic(topic_id);
        if seq == 0 || pending_cid(&real).is_some() {
            return;
        }
        // Off the UI thread: the store write waits for the writer lock, which a catch-up page
        // can hold for tens of milliseconds. The change arrives as GroupChanged/TopicsChanged.
        if let Some(a) = self.account() {
            let client = a.client.clone();
            let group_id = group_id.to_string();
            chat_core::background().spawn_blocking(move || {
                if let Err(e) = client.mark_topic_read(&group_id, &real, seq) {
                    tracing::warn!(error = %e, "mark read failed");
                }
            });
        }
    }

    fn set_typing(&self, group_id: &str, topic_id: &str) {
        let real = self.resolve_topic(topic_id);
        if pending_cid(&real).is_some() {
            return;
        }
        if let Some(a) = self.account() {
            a.client.typing_in(group_id, &real);
        }
    }

    fn retry_send(&self, _group_id: &str, client_msg_id: &str) {
        if self.retry_job(client_msg_id) {
            return;
        }
        if let Some(a) = self.account()
            && let Err(e) = a.client.retry_failed(client_msg_id)
        {
            tracing::warn!(error = %e, "retry failed");
        }
    }

    fn discard_send(&self, _group_id: &str, client_msg_id: &str) {
        if self.discard_job(client_msg_id) {
            return;
        }
        if let Some(a) = self.account()
            && let Err(e) = a.client.discard(client_msg_id)
        {
            tracing::warn!(error = %e, "discard failed");
        }
    }

    fn draft(&self, key: &DraftKey) -> Draft {
        self.shared.drafts.get(key)
    }

    fn save_draft_text(&self, key: &DraftKey, body: String, mentions: Vec<MemberId>) {
        self.shared.drafts.save_text(key, body, mentions);
        self.draft_changed(key, false);
    }

    fn set_quote(&self, key: &DraftKey, quote_of: Option<MessageId>) {
        if self.shared.drafts.set_quote(key, quote_of) {
            self.draft_changed(key, true);
        }
    }

    fn attach_files(&self, key: &DraftKey, files: Vec<NewFile>) {
        self.attach_files_inner(key, files);
    }

    fn remove_file(&self, key: &DraftKey, file_id: &str) {
        if self.shared.drafts.remove_file(key, file_id) {
            self.draft_changed(key, true);
        }
    }

    fn send_with_files(
        &self,
        request: SendRequest,
        key: &DraftKey,
        files: Vec<PendingFileId>,
    ) -> ClientMsgId {
        let job = self.start_upload_job(
            key,
            &files,
            Some(request.topic_id.clone()),
            None,
            request.body.clone(),
            request.mentions.clone(),
            request.quote_of.clone(),
        );
        job.unwrap_or_else(|| self.send(request))
    }

    fn start_topic_with_files(
        &self,
        request: StartTopicRequest,
        key: &DraftKey,
        files: Vec<PendingFileId>,
    ) -> ClientMsgId {
        let job = self.start_upload_job(
            key,
            &files,
            None,
            request.title.clone(),
            request.body.clone(),
            request.mentions.clone(),
            None,
        );
        job.unwrap_or_else(|| self.start_topic(request))
    }

    fn take_rejected_files(&self, key: &DraftKey) -> Vec<RejectedFile> {
        self.take_rejected_inner(key)
    }

    fn file_transfer(&self, key: &str) -> Option<TransferState> {
        self.file_transfer_inner(key)
    }

    fn cached_file(&self, file: &FileAttachment) -> Option<PathBuf> {
        self.cached_file_inner(file)
    }

    fn thumbnail(&self, group_id: &str, topic_id: &str, file: &FileAttachment) -> Option<PathBuf> {
        self.thumbnail_inner(group_id, topic_id, file)
    }

    fn fetch_file(&self, group_id: &str, topic_id: &str, file: &FileAttachment) -> Reply<PathBuf> {
        self.fetch_file_inner(group_id, topic_id, file)
    }

    fn cancel_download(&self, file: &FileAttachment) {
        self.cancel_download_inner(file)
    }

    fn topic_files(&self, _group_id: &str, topic_id: &str) -> Vec<TopicFile> {
        self.topic_files_inner(topic_id)
    }

    fn refresh_artifacts(&self, group_id: &str, topic_id: &str) {
        self.refresh_artifacts_inner(group_id, topic_id);
    }

    fn artifact(&self, artifact_id: &str) -> Option<ArtifactDetail> {
        self.artifact_inner(artifact_id)
    }

    fn request_artifact(&self, group_id: &str, topic_id: &str, artifact_id: &str) {
        self.request_artifact_inner(group_id, topic_id, artifact_id);
    }

    fn open_artifact_version(
        &self,
        artifact_id: &str,
        version: Option<u32>,
    ) -> Reply<ArtifactFolder> {
        self.open_artifact_version_inner(artifact_id, version)
    }

    fn create_artifact(&self, request: NewArtifactRequest) -> Reply<CreatedArtifact> {
        self.create_artifact_inner(request)
    }

    fn clear_draft(&self, key: &DraftKey) {
        if self.shared.drafts.clear(key) {
            self.draft_changed(key, true);
        }
    }

    fn create_group(&self, request: CreateGroupRequest) -> AdminReply<GroupId> {
        self.create_group_inner(request)
    }

    fn open_dm(&self, member_id: &str) -> AdminReply<GroupId> {
        self.open_dm_inner(member_id)
    }

    fn add_group_members(&self, group_id: &str, member_ids: Vec<MemberId>) -> AdminReply<()> {
        self.add_group_members_inner(group_id, member_ids)
    }

    fn leave_group(&self, group_id: &str) {
        self.leave_group_inner(group_id);
    }

    fn group_muted(&self, group_id: &str) -> bool {
        self.group_muted_inner(group_id)
    }

    fn set_group_muted(&self, group_id: &str, muted: bool) {
        self.set_group_muted_inner(group_id, muted);
    }

    fn saved_navigation(&self) -> Option<SavedNavigation> {
        self.saved_navigation_inner()
    }

    fn save_navigation(&self, navigation: SavedNavigation) {
        self.save_navigation_inner(navigation);
    }

    fn topic_read_seq(&self, _group_id: &str, topic_id: &str) -> Option<u64> {
        self.topic_read_seq_inner(topic_id)
    }

    fn topic_following(&self, _group_id: &str, topic_id: &str) -> bool {
        self.topic_following_inner(topic_id)
    }

    fn set_topic_following(&self, _group_id: &str, topic_id: &str, follow: bool) {
        self.set_topic_following_inner(topic_id, follow);
    }

    fn set_topic_owner(&self, _group_id: &str, topic_id: &str, owner: Option<MemberId>) {
        self.set_topic_owner_inner(topic_id, owner);
    }

    fn search(&self, query: crate::data::SearchQuery) -> crate::data::SearchReply {
        self.search_inner(query)
    }

    fn runs(&self, group_id: &str, topic_id: &str) -> Vec<crate::data::BotRunSummary> {
        self.runs_inner(group_id, topic_id)
    }

    fn run_process(&self, run_id: &str) -> crate::data::RunProcess {
        self.run_process_inner(run_id)
    }

    fn follow_run(&self, run_id: &str) {
        self.follow_run_inner(run_id)
    }

    fn download_run_session(
        &self,
        run_id: &str,
        dir: std::path::PathBuf,
    ) -> crate::data::SessionDownload {
        self.download_session_inner(run_id, dir)
    }

    // --- settings window (provider/settings.rs) ---

    fn update_profile(&self, name: String, role: Option<String>) -> AdminReply<Member> {
        self.update_profile_inner(name, role)
    }

    fn upload_avatar(&self, path: PathBuf) -> AdminReply<Member> {
        self.upload_avatar_inner(path)
    }

    fn create_invite(&self, request: InviteRequest) -> AdminReply<InviteCreated> {
        self.create_invite_inner(request)
    }

    fn set_member_level(&self, member_id: &str, level: MemberLevel) -> AdminReply<Member> {
        self.set_member_level_inner(member_id, level)
    }

    fn set_member_deactivated(&self, member_id: &str, deactivated: bool) -> AdminReply<Member> {
        self.set_member_deactivated_inner(member_id, deactivated)
    }

    fn create_bot_member(
        &self,
        name: String,
        role: Option<String>,
        runner: Option<RunnerSetup>,
    ) -> AdminReply<BotCreated> {
        self.create_bot_member_inner(name, role, runner)
    }

    fn machines(&self) -> AdminReply<Vec<MachineInfo>> {
        self.machines_inner()
    }

    fn create_machine(&self, name: String, labels: Vec<String>) -> AdminReply<MachineCreated> {
        self.create_machine_inner(name, labels)
    }

    fn group_prefs(&self, group_id: &str) -> GroupPrefs {
        self.group_prefs_inner(group_id)
    }

    fn set_group_prefs(
        &self,
        group_id: &str,
        muted: Option<bool>,
        notify: Option<NotifyLevel>,
    ) -> AdminReply<()> {
        self.set_group_prefs_inner(group_id, muted, notify)
    }

    fn send_diagnostics(&self, description: String) -> AdminReply<String> {
        self.send_diagnostics_inner(description)
    }

    fn card(&self, _group_id: &str, message_id: &str) -> Option<Arc<crate::data::card::Card>> {
        let a = self.account()?;
        self.shared.cards.card(&a.client, message_id)
    }

    fn card_presses(&self, _group_id: &str, message_id: &str) -> Vec<crate::data::CardPress> {
        match self.account() {
            Some(a) => self.shared.cards.presses(&a.client, message_id),
            None => Vec::new(),
        }
    }

    fn card_press_state(
        &self,
        _group_id: &str,
        message_id: &str,
    ) -> Option<crate::data::CardPressState> {
        let a = self.account()?;
        self.shared.cards.press_state(&a.client, message_id)
    }

    fn press_card_action(&self, group_id: &str, topic_id: &str, message_id: &str, action_id: &str) {
        if let Some(a) = self.account() {
            self.shared.cards.press(&a.client, message_id, action_id);
            self.emit(vec![Change::Messages {
                group_id: group_id.to_string(),
                topic_id: topic_id.to_string(),
            }]);
        }
    }

    fn card_image(&self, group_id: &str, topic_id: &str, file_id: &str) -> Option<PathBuf> {
        let a = self.account()?;
        let this = self.clone();
        let change = Change::Messages {
            group_id: group_id.to_string(),
            topic_id: topic_id.to_string(),
        };
        self.shared
            .cards
            .image(&a.client, file_id, move || this.emit(vec![change]))
    }

    fn subscribe(&self) -> UnboundedReceiver<Vec<Change>> {
        let (tx, rx) = unbounded();
        lock(&self.shared.subscribers).push(tx);
        rx
    }
}
