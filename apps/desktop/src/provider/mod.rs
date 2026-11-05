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
// --- cards v2 track ---
mod cards_v2;
// --- inc-cards21 ---
mod card_events;
// --- end inc-cards21 ---
// --- end cards v2 track ---
pub mod convert;
// Desk track (1.1.0 首页).
mod desk;
mod email;
pub mod files;
#[cfg(test)]
mod files_live;
#[cfg(target_os = "macos")]
pub mod macos;
mod panes;
pub mod persist;
// Proof chips from done cards (1.1.0).
mod proof;
// pages track: the 群, 成员 and 产出 pages.
mod pages;
// --- inc-host track ---
mod page_host;
// --- end inc-host track ---
mod process;
mod realtime;
#[cfg(test)]
mod realtime_live;
#[cfg(test)]
mod search_live;
// v13/wire: the pages' backend data against a live server.
pub mod selftest_interact;
pub mod selftest_live;
pub mod session;
mod settings;
#[cfg(test)]
mod wire_live;
// --- settings-backend track ---
mod settings_backend;
// --- end settings-backend track ---
// desktop-data: cached aggregates of the 首页, 成员, 群 and 产出 pages.
mod snapshot;
// provider track: v2 topic surfaces and the seeded cache of the `topic-core` scene.
pub mod seeded;
mod topic_v2;
// --- inc-refs: 引用提问 (references, chips, sends with blockRefs).
mod refs;
// --- end inc-refs

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
        GroupId, GroupSummary, LoginError, LoginRequest, Member, MemberId, MessageId,
        MessageWindow, NewArtifactRequest, NewFile, PendingFileId, RejectedFile, Reply,
        SavedNavigation, SendRequest, SessionState, StartTopicRequest, TodoItem, Topic, TopicFile,
        TopicStatus, TransferState,
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
    /// Avatar URLs whose download started (once per URL and session).
    avatars_requested: HashSet<String>,
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
    // panes track (1.1.0): 单独问 and 实时查看.
    panes: panes::PanesCache,
    // --- cards v2 track ---
    cards_v2: cards_v2::CardV2Cache,
    // --- end cards v2 track ---
    // --- inc-cards21: my card events (card.event) ---
    card_events: card_events::CardEventLog,
    // --- end inc-cards21 ---
    /// 群里的决定 from the server (`provider/pages.rs`).
    decisions: pages::DecisionsCache,
    /// R5: page events this window saw (they are not in the group log).
    page_events: pages::PageEventLog,
    // --- provider track: v2 topic surfaces (provider/topic_v2.rs) ---
    topic_v2: topic_v2::TopicV2Cache,
    // --- end provider track ---
    // --- inc-host track: acks, replies and data pushes for open 产出 pages ---
    page_signals: page_host::PageSignals,
    // --- end inc-host track ---
    // --- inc-refs ---
    refs: refs::RefsCache,
    // --- end inc-refs ---
    /// Aggregates of the pages, rebuilt on a change (`provider/snapshot.rs`).
    snapshots: snapshot::SnapshotCache,
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
                panes: panes::PanesCache::default(),
                cards_v2: cards_v2::CardV2Cache::default(),
                card_events: card_events::CardEventLog::default(),
                decisions: pages::DecisionsCache::default(),
                page_events: pages::PageEventLog::default(),
                topic_v2: topic_v2::TopicV2Cache::default(),
                page_signals: page_host::PageSignals::default(),
                refs: refs::RefsCache::default(),
                snapshots: snapshot::SnapshotCache::default(),
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
        self.shared.snapshots.note(&changes);
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
            (
                s.generation,
                s.connection == Some(ConnectionState::TokenUnreadable),
            )
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
        self.shared.cards_v2.clear();
        self.shared.card_events.clear();
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
        self.shared.cards_v2.clear();
        self.shared.card_events.clear();
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
            session::forget_session(&root, &account.server_url);
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
            session::forget_session(&root, &account.server_url);
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

    /// Uses another server: keeps this server's session and token, then signs
    /// in with the saved session of `server_url`, or shows the login window
    /// for it.
    fn switch_server_inner(&self, server_url: &str) {
        let Some(server_url) = session::normalize_server_url(server_url) else {
            return;
        };
        let same = |a: &str| a.trim_end_matches('/') == server_url.trim_end_matches('/');
        if self.account().is_some_and(|a| same(&a.server_url)) {
            return;
        }
        let root = self.shared.config.data_root.clone();
        let saved = session::set_current_server(&root, &server_url);
        tracing::info!(signed_in = saved.is_some(), "switching servers");
        // Not a sign-out: the old token stays in the Keychain.
        drop(self.take_account(None));
        if saved.is_some() {
            self.restore();
        }
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
        // inc-host track: page acks, replies and data pushes go to the page host.
        self.emit_page_signal(&u);
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
                // --- cards v2 track: an edit replaces a card in place ---
                self.shared.cards_v2.forget_topic(&topic_id);
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
                // --- cards v2 track ---
                if f.kind == "card_answer"
                    && let Some(a) = self.account()
                {
                    self.shared.cards_v2.on_refused(&a.client, &f.id, f.code);
                }
                // --- end cards v2 track ---
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
                // 记一条 on any device: the 群 page fetches its decisions again.
                if let chat_core::proto::EventData::DecisionRecorded(_) = &ev.data {
                    self.forget_decisions(&ev.group_id);
                    out.push(Change::Groups);
                }
                // --- cards v2 track ---
                if let chat_core::proto::EventData::CardAnswered(d) = &ev.data
                    && let Some(a) = self.account()
                {
                    let mine = ev.actor_id == a.me.id;
                    self.shared.cards_v2.on_answered(&d.message_id, mine, d.rev);
                    if let Some(m) = a.client.store().message(&d.message_id).ok().flatten() {
                        out.extend([
                            Change::Messages {
                                group_id: m.group_id,
                                topic_id: m.topic_id,
                            },
                            Change::Todo,
                        ]);
                    }
                }
                // --- end cards v2 track ---
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
            ClientUpdate::ArtifactEvent(t) => {
                self.shared.page_events.record_transient(&t);
                out.push(Change::Artifacts {
                    group_id: t.group_id.clone(),
                    topic_id: t.topic_id.clone(),
                });
            }
            // --- inc-cards21: acks and responses of my card events ---
            ClientUpdate::CardEventAcked {
                client_event_id,
                message_id,
                result,
            } => {
                let now = crate::selftest::clock::now_ms();
                if self
                    .shared
                    .card_events
                    .acked(&message_id, &client_event_id, &result, now)
                    && let Some(a) = self.account()
                    && let Some(m) = a.client.store().message(&message_id).ok().flatten()
                {
                    out.push(Change::Messages {
                        group_id: m.group_id,
                        topic_id: m.topic_id,
                    });
                }
            }
            ClientUpdate::InteractionResolved(t) => {
                let now = crate::selftest::clock::now_ms();
                if let Some(message_id) = self.shared.card_events.resolved(&t, now)
                    && let Some(a) = self.account()
                    && let Some(m) = a.client.store().message(&message_id).ok().flatten()
                {
                    out.push(Change::Messages {
                        group_id: m.group_id,
                        topic_id: m.topic_id,
                    });
                }
            }
            // --- end inc-cards21 ---
            // 1.1 interaction loop: the 产出 host track handles these.
            ClientUpdate::ArtifactEventAcked { .. }
            | ClientUpdate::CardEvent(_)
            | ClientUpdate::ArtifactData(_)
            | ClientUpdate::ArtifactReply(_) => {}
            // --- settings-backend track: the daily digest becomes one notice ---
            ClientUpdate::Digest(d) => {
                let notice = self.digest_notice(&d);
                if let Some(tx) = lock(&self.shared.notices).as_ref() {
                    let _ = tx.unbounded_send(notice);
                }
            } // --- end settings-backend track ---
        }
    }

    /// The local file of a member's avatar (the server route needs the token, so views draw
    /// the downloaded copy). Starts the download once per URL and emits `Change::Members` when
    /// it is on disk; until then the avatar shows the monogram.
    fn avatar_file(&self, a: &Account, m: &chat_core::proto::Member) -> Option<String> {
        let url = m.avatar_url()?;
        if let Some(p) = a.client.cached_avatar(m) {
            return Some(p.to_string_lossy().into_owned());
        }
        if self.state().avatars_requested.insert(url.to_string()) {
            let handle = a.client.avatar(m);
            let this = self.clone();
            chat_core::background().spawn(async move {
                match handle.await {
                    Ok(Ok(Some(_))) => this.emit(vec![Change::Members, Change::Groups]),
                    Ok(Ok(None)) => {}
                    Ok(Err(e)) => tracing::warn!(error = %e, "avatar download failed"),
                    Err(e) => tracing::warn!(error = %e, "avatar task failed"),
                }
            });
        }
        None
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
        // `avatar_file` takes the state lock, so read the account first.
        if let Some(a) = self.account() {
            // The cached row has my latest name, 岗位 and avatar.
            let me = a.client.member(&a.me.id).ok().flatten();
            let row = me.as_ref().unwrap_or(&a.me);
            // Views draw only the downloaded file, like `members()`.
            let mut me = convert::member(row, true);
            me.avatar_url = self.avatar_file(&a, row);
            return SessionState::SignedIn {
                me,
                server_url: a.server_url.clone(),
            };
        }
        self.state()
            .session
            .clone()
            .unwrap_or(SessionState::SignedOut { last_error: None })
    }

    fn login(&self, request: LoginRequest) {
        self.login_inner(request);
    }

    fn logout(&self) {
        self.sign_out_inner(false);
    }

    fn current_server(&self) -> Option<String> {
        if let Some(a) = self.account() {
            return Some(a.server_url.clone());
        }
        session::current_server(&self.shared.config.data_root)
    }

    fn signed_in_servers(&self) -> Vec<String> {
        session::signed_in_servers(&self.shared.config.data_root)
    }

    fn switch_server(&self, server_url: &str) {
        self.switch_server_inner(server_url);
    }

    fn request_sign_in_code(&self, server_url: &str, email: &str) -> crate::data::EmailReply<()> {
        self.request_sign_in_code_inner(server_url, email)
    }

    fn my_email(&self) -> crate::data::EmailReply<Option<String>> {
        self.my_email_inner()
    }

    fn add_email_code(&self, email: &str) -> crate::data::EmailReply<()> {
        self.add_email_code_inner(email)
    }

    fn verify_email(&self, email: &str, code: &str) -> crate::data::EmailReply<String> {
        self.verify_email_inner(email, code)
    }

    fn remove_email(&self) -> crate::data::EmailReply<()> {
        self.remove_email_inner()
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
            .map(|m| {
                let mut out = convert::member(m, online.contains(&m.id) || m.id == a.me.id);
                out.avatar_url = self.avatar_file(&a, m);
                out
            })
            .collect()
    }

    fn member(&self, id: &str) -> Option<Member> {
        let a = self.account()?;
        let m = a.client.member(id).ok().flatten()?;
        let mut out = convert::member(&m, m.id == a.me.id || a.client.is_online(id));
        out.avatar_url = self.avatar_file(&a, &m);
        Some(out)
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
            self.shared.cards_v2.remember(&msgs);
            // --- inc-cards21: a new message that names my event ends its pending state ---
            let now = crate::selftest::clock::now_ms();
            for m in msgs.iter().filter(|m| m.cause.is_some()) {
                if let Some(c) = &m.cause {
                    self.shared.card_events.caused_anywhere(c, now, &m.id);
                }
            }
            // --- end inc-cards21 ---
            self.shared.refs.remember(&msgs);
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

    fn quoted_message(
        &self,
        _group_id: &str,
        quoting_id: &str,
        quoted_id: &str,
    ) -> Option<crate::data::QuotedMessage> {
        let a = self.account()?;
        let store = a.client.store();
        if let Some(q) = store.message(quoted_id).ok().flatten() {
            return Some(convert::quoted(&q));
        }
        let reply = store.message(quoting_id).ok().flatten()?.reply_to?;
        (reply.message_id == quoted_id).then_some(crate::data::QuotedMessage {
            author_id: reply.author_id,
            body: reply.excerpt,
            deleted: false,
        })
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

    fn rename_group(&self, group_id: &str, name: String) -> AdminReply<()> {
        self.rename_group_inner(group_id, name)
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

    fn topic_muted(&self, _group_id: &str, topic_id: &str) -> bool {
        self.topic_muted_inner(topic_id)
    }

    fn set_topic_muted(&self, _group_id: &str, topic_id: &str, muted: bool) {
        self.set_topic_muted_inner(topic_id, muted);
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

    fn cancel_run(&self, group_id: &str, topic_id: &str, run_id: &str) -> AdminReply<()> {
        self.cancel_run_inner(group_id, topic_id, run_id)
    }

    fn run_stopping(&self, run_id: &str) -> bool {
        self.run_stopping_inner(run_id)
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
        character: Option<crate::data::CharacterId>,
        runner: Option<RunnerSetup>,
    ) -> AdminReply<BotCreated> {
        self.create_bot_member_inner(name, role, character, runner)
    }

    fn set_member_character(
        &self,
        member_id: &str,
        character: Option<crate::data::CharacterId>,
    ) -> AdminReply<Member> {
        self.set_member_character_inner(member_id, character)
    }

    fn set_member_plan(
        &self,
        member_id: &str,
        plan: crate::data::pages::MemberPlan,
    ) -> AdminReply<Member> {
        self.set_member_plan_inner(member_id, plan)
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

    // --- settings-backend track ---
    fn notify_preferences(&self) -> AdminReply<crate::data::NotifyPreferences> {
        self.notify_preferences_inner()
    }
    fn set_notify_preferences(
        &self,
        prefs: crate::data::NotifyPreferences,
    ) -> AdminReply<crate::data::NotifyPreferences> {
        self.set_notify_preferences_inner(prefs)
    }
    fn approval_defaults(&self) -> AdminReply<crate::data::ApprovalDefaults> {
        self.approval_defaults_inner()
    }
    fn set_approval_defaults(
        &self,
        defaults: crate::data::ApprovalDefaults,
    ) -> AdminReply<crate::data::ApprovalDefaults> {
        self.set_approval_defaults_inner(defaults)
    }
    fn devices(&self) -> AdminReply<Vec<crate::data::DeviceInfo>> {
        self.devices_inner()
    }
    fn sign_out_device(&self, device_id: &str) -> AdminReply<()> {
        self.sign_out_device_inner(device_id)
    }
    // --- end settings-backend track ---

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

    // --- cards v2 track ---
    fn card_v2(
        &self,
        _group_id: &str,
        message_id: &str,
    ) -> Option<crate::data::cards_v2::CardV2Message> {
        let a = self.account()?;
        let card = self.shared.cards_v2.card(&a.client, message_id);
        // --- inc-cards21: a rev that names my event ends its pending state ---
        if let Some(c) = card.as_ref().and_then(|c| c.cause.as_ref()) {
            self.shared
                .card_events
                .caused(message_id, c, crate::selftest::clock::now_ms());
        }
        // --- end inc-cards21 ---
        card
    }

    fn card_v2_answers(
        &self,
        _group_id: &str,
        message_id: &str,
    ) -> Vec<crate::data::cards_v2::CardV2Answer> {
        match self.account() {
            Some(a) => self.shared.cards_v2.answers(&a.client, message_id),
            None => Vec::new(),
        }
    }

    fn card_v2_pending(
        &self,
        _group_id: &str,
        message_id: &str,
    ) -> Option<crate::data::cards_v2::CardV2Pending> {
        let a = self.account()?;
        self.shared
            .cards_v2
            .pending(&a.client, message_id)
            // --- inc-cards21: a 2.1 answer is a durable card event ---
            .or_else(|| card_events::pending_answer(&self.shared.card_events.events(message_id)))
    }

    fn answer_card_v2(
        &self,
        group_id: &str,
        topic_id: &str,
        message_id: &str,
        answer: crate::data::cards_v2::CardV2AnswerRequest,
    ) {
        if let Some(a) = self.account() {
            self.shared.cards_v2.answer(&a.client, message_id, answer);
            self.emit(vec![Change::Messages {
                group_id: group_id.to_string(),
                topic_id: topic_id.to_string(),
            }]);
        }
    }

    fn withdraw_card_v2_answer(&self, group_id: &str, topic_id: &str, message_id: &str) {
        if let Some(a) = self.account() {
            self.shared.cards_v2.withdraw(&a.client, message_id);
            self.emit(vec![Change::Messages {
                group_id: group_id.to_string(),
                topic_id: topic_id.to_string(),
            }]);
        }
    }

    // --- inc-cards21: the interaction loop ---
    fn send_card_event(
        &self,
        group_id: &str,
        topic_id: &str,
        message_id: &str,
        request: crate::data::cards_v2::CardEventRequest,
    ) -> Option<String> {
        let a = self.account()?;
        let now = crate::selftest::clock::now_ms();
        let req = chat_core::CardEventRequest {
            message_id: message_id.to_string(),
            rev: request.rev,
            block_id: request.block_id.clone(),
            action_id: request.action_id.clone(),
            event: request.event.clone(),
            value: request.value.clone(),
            inputs: request.inputs.clone(),
            part: request.part.clone(),
        };
        let out = match a.client.send_card_event(req) {
            Ok(sent) => {
                self.shared.card_events.sent(
                    message_id,
                    sent.client_event_id.clone(),
                    request,
                    sent.durable,
                    now,
                );
                Some(sent.client_event_id)
            }
            Err(e) => {
                tracing::warn!(error = %e, "card event refused");
                self.shared.card_events.failed_now(
                    message_id,
                    request,
                    crate::data::cards_v2::CardEventFailure::Other,
                    now,
                );
                None
            }
        };
        self.emit(vec![Change::Messages {
            group_id: group_id.to_string(),
            topic_id: topic_id.to_string(),
        }]);
        out
    }

    fn card_v2_events(
        &self,
        _group_id: &str,
        message_id: &str,
    ) -> Vec<crate::data::cards_v2::MyCardEvent> {
        self.shared.card_events.events(message_id)
    }

    fn drop_card_event(
        &self,
        group_id: &str,
        topic_id: &str,
        message_id: &str,
        client_event_id: &str,
    ) {
        self.shared
            .card_events
            .drop_pending(message_id, client_event_id);
        self.emit(vec![Change::Messages {
            group_id: group_id.to_string(),
            topic_id: topic_id.to_string(),
        }]);
    }

    fn retry_card_event(
        &self,
        group_id: &str,
        topic_id: &str,
        message_id: &str,
        client_event_id: &str,
    ) -> bool {
        let Some(a) = self.account() else {
            return false;
        };
        let Some(e) = self.shared.card_events.get(message_id, client_event_id) else {
            return false;
        };
        // Only a queued answer keeps its id; a refused live event is sent again as new.
        if !e.durable || a.client.retry_failed(client_event_id).is_err() {
            return false;
        }
        self.shared.card_events.resending(
            message_id,
            client_event_id,
            crate::selftest::clock::now_ms(),
        );
        self.emit(vec![Change::Messages {
            group_id: group_id.to_string(),
            topic_id: topic_id.to_string(),
        }]);
        true
    }
    // --- end inc-cards21 ---
    // --- end cards v2 track ---

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

    // --- desk track (1.1.0 首页): provider/desk.rs ---

    fn asks_for_me(&self) -> Vec<crate::data::v2::AskItem> {
        self.snapshot().asks.clone()
    }

    fn done_today(&self) -> Vec<crate::data::v2::DoneItem> {
        self.snapshot().done_today.clone()
    }

    fn active_runs(&self) -> Vec<crate::data::BotRunSummary> {
        self.snapshot().active_runs.clone()
    }

    // Card v2 reads and answers come from the cards v2 track above.

    fn message_proof(&self, _group_id: &str, message_id: &str) -> Vec<crate::data::v2::Proof> {
        self.message_proof_inner(message_id)
    }

    fn topic_result_proof(&self, _group_id: &str, topic_id: &str) -> Vec<crate::data::v2::Proof> {
        self.topic_result_proof_inner(&self.resolve_topic(topic_id))
    }

    // --- end of the desk track block ---

    // --- panes track (1.1.0): 单独问 and 实时查看 (provider/panes.rs) ---

    fn side_chat(
        &self,
        group_id: &str,
        topic_id: &str,
        member_id: &str,
    ) -> crate::data::v2::SideChatThread {
        self.side_chat_inner(group_id, topic_id, member_id)
    }

    fn send_side_chat(
        &self,
        group_id: &str,
        topic_id: &str,
        member_id: &str,
        body: String,
        refs: Vec<crate::data::DraftRef>,
    ) {
        self.send_side_chat_inner(group_id, topic_id, member_id, body, refs)
    }

    fn post_side_chat_suggestion(
        &self,
        group_id: &str,
        topic_id: &str,
        member_id: &str,
        message_id: &str,
    ) {
        self.post_side_chat_suggestion_inner(group_id, topic_id, member_id, message_id)
    }

    fn machine_screen(&self, run_id: &str) -> Option<crate::data::v2::MachineScreen> {
        self.machine_screen_inner(run_id)
    }

    fn acceptance_criteria(&self, run_id: &str) -> Vec<crate::data::v2::AcceptanceItem> {
        self.acceptance_inner(run_id)
    }

    fn run_step_times(&self, run_id: &str) -> Vec<crate::data::Millis> {
        self.run_step_times_inner(run_id)
    }

    fn take_over_run(&self, group_id: &str, topic_id: &str, run_id: &str) -> AdminReply<()> {
        self.pause_inner(group_id, topic_id, run_id, true)
    }

    fn hand_back_run(&self, group_id: &str, topic_id: &str, run_id: &str) -> AdminReply<()> {
        self.pause_inner(group_id, topic_id, run_id, false)
    }

    // --- end of the panes track ---

    // --- pages track: the 群, 成员 and 产出 pages (provider/pages.rs) ---

    fn group_decisions(&self, group_id: &str) -> Vec<crate::data::v2::Decision> {
        self.group_decisions_inner(group_id)
    }
    fn can_record_decision(&self, _group_id: &str) -> bool {
        self.account().is_some()
    }
    fn record_decision(&self, group_id: &str, text: String) -> AdminReply<()> {
        self.record_decision_inner(group_id, text)
    }

    fn artifact_page_events(&self, artifact_id: &str, version: u32) -> Vec<String> {
        self.artifact_page_events_inner(artifact_id, version)
    }

    fn send_artifact_event(
        &self,
        artifact_id: &str,
        version: u32,
        name: &str,
        payload: Option<serde_json::Value>,
    ) {
        self.send_artifact_event_inner(artifact_id, version, name, payload)
    }

    fn artifact_responses(&self, artifact_id: &str) -> Vec<crate::data::pages::ArtifactResponse> {
        self.artifact_responses_inner(artifact_id)
    }

    fn artifact_responses_load(&self, artifact_id: &str) -> crate::data::pages::ResponsesLoad {
        self.shared.page_events.load(artifact_id)
    }

    fn reload_artifact_responses(&self, artifact_id: &str) {
        self.shared.page_events.forget_fetch(artifact_id);
        self.refresh_page_log(artifact_id);
        self.emit_artifact_change(artifact_id);
    }

    fn retry_member_problem(&self, member_id: &str, run_id: &str) -> AdminReply<()> {
        self.retry_member_problem_inner(member_id, run_id)
    }

    // desktop-data: the page aggregates come from one cached snapshot (provider/snapshot.rs).

    fn member_stats(&self, member_id: &str) -> Option<crate::data::v2::MemberStats> {
        self.member_stats_inner(member_id)
    }

    fn member_extras(&self, member_id: &str) -> crate::data::pages::MemberExtras {
        self.member_extras_inner(member_id)
    }

    fn all_artifacts(&self) -> Vec<(GroupId, ArtifactSummary)> {
        self.snapshot().artifacts.clone()
    }

    fn group_recent_done(&self, group_id: &str) -> Vec<crate::data::pages::RecentDone> {
        self.group_recent_done_inner(group_id)
    }

    // --- end pages track ---

    // --- provider track: v2 topic surfaces (provider/topic_v2.rs) ---

    fn topic_changes(
        &self,
        group_id: &str,
        topic_id: &str,
    ) -> Option<crate::data::v2::TopicChanges> {
        self.topic_changes_inner(group_id, topic_id)
    }

    fn pinned_ask(&self, group_id: &str, topic_id: &str) -> Option<crate::data::v2::AskItem> {
        self.pinned_ask_inner(group_id, topic_id)
    }

    fn changes_approval(
        &self,
        group_id: &str,
        topic_id: &str,
    ) -> Option<(crate::data::MemberId, crate::data::Millis)> {
        self.changes_approval_inner(group_id, topic_id)
    }

    fn approve_changes(&self, group_id: &str, topic_id: &str) -> AdminReply<()> {
        self.approve_changes_inner(group_id, topic_id)
    }

    fn withdraw_changes_approval(&self, group_id: &str, topic_id: &str) -> AdminReply<()> {
        self.withdraw_changes_approval_inner(group_id, topic_id)
    }

    fn changes_approval_queued(&self, group_id: &str, topic_id: &str) -> bool {
        self.changes_approval_queued_inner(group_id, topic_id)
    }

    fn run_usage(&self, run_id: &str) -> Option<crate::data::v2::RunUsage> {
        self.run_usage_inner(run_id)
    }

    fn topic_decisions(&self, group_id: &str, topic_id: &str) -> Vec<crate::data::v2::Decision> {
        self.topic_decisions_inner(group_id, topic_id)
    }

    fn artifact_story(&self, artifact_id: &str) -> Option<crate::data::v2::ArtifactStory> {
        self.artifact_story_inner(artifact_id)
    }

    fn artifact_preview(
        &self,
        artifact_id: &str,
        version: u32,
    ) -> Option<crate::data::v2::ArtifactPreview> {
        self.artifact_preview_inner(artifact_id, version, true)
    }

    fn artifact_thumb(&self, artifact_id: &str) -> Option<crate::data::pages::ArtifactThumb> {
        self.artifact_thumb_inner(artifact_id)
    }

    fn media_duration(&self, file: &FileAttachment) -> Option<u64> {
        self.media_duration_inner(file)
    }

    fn desk_note(&self) -> Option<crate::data::v2::DeskNote> {
        self.desk_note_inner()
    }

    fn queue(&self) -> Vec<crate::data::v2::QueuedItem> {
        self.queue_inner()
    }

    fn group_purpose(&self, group_id: &str) -> Option<String> {
        self.account()?
            .client
            .group_purpose(group_id)
            .ok()
            .flatten()
            .filter(|p| !p.trim().is_empty())
    }

    fn set_group_purpose(&self, group_id: &str, purpose: String) -> AdminReply<()> {
        self.set_group_purpose_inner(group_id, purpose)
    }

    // --- end provider track ---

    // --- dfix-motion track ---

    fn side_chat_topics(&self) -> std::collections::HashSet<(GroupId, crate::data::TopicId)> {
        let Some(a) = self.account() else {
            return Default::default();
        };
        // Side chats live in my DMs only.
        a.client
            .groups()
            .unwrap_or_default()
            .iter()
            .map(|row| convert::group(row, &a.me.id, |_| None))
            .filter(|g| g.kind == crate::data::GroupKind::Dm)
            .flat_map(|g| a.client.topics(&g.id, TOPIC_LIMIT).unwrap_or_default())
            .filter(|row| row.topic.context_of.is_some())
            .map(|row| (row.topic.group_id.clone(), row.topic.id.clone()))
            .collect()
    }

    // --- end dfix-motion track ---

    // --- inc-host track: the 产出 page host (provider/page_host.rs) ---

    fn page_bridge(
        &self,
        artifact_id: &str,
        version: u32,
    ) -> Option<crate::data::page_host::PageBridge> {
        self.page_bridge_inner(artifact_id, version)
    }

    fn send_page_event(
        &self,
        artifact_id: &str,
        version: u32,
        name: &str,
        kind: crate::data::page_host::PageEventKind,
        payload: Option<serde_json::Value>,
    ) -> Result<String, crate::data::page_host::PageSendError> {
        self.send_page_event_inner(artifact_id, version, name, kind, payload)
    }

    fn fetch_page_data(
        &self,
        artifact_id: &str,
        version: u32,
    ) -> futures::channel::oneshot::Receiver<Option<(serde_json::Value, u64)>> {
        self.fetch_page_data_inner(artifact_id, version)
    }

    fn page_signals(&self) -> Option<UnboundedReceiver<crate::data::page_host::PageSignal>> {
        Some(self.shared.page_signals.subscribe())
    }

    fn file_media(&self, file: &FileAttachment) -> Option<chat_core::proto::MediaInfo> {
        self.file_media_inner(file)
    }

    fn artifact_media(
        &self,
        artifact_id: &str,
        version: u32,
    ) -> Option<chat_core::proto::MediaInfo> {
        self.artifact_media_inner(artifact_id, version)
    }

    fn file_poster(
        &self,
        group_id: &str,
        topic_id: &str,
        file: &FileAttachment,
    ) -> Option<PathBuf> {
        self.file_poster_inner(group_id, topic_id, file)
    }

    fn artifact_poster(&self, artifact_id: &str, version: u32) -> Option<PathBuf> {
        self.artifact_poster_inner(artifact_id, version)
    }

    fn upload_generating(&self, key: &str) -> bool {
        page_host::is_generating(key)
    }

    // --- end inc-host track ---
    // --- inc-refs track ---

    fn refs_supported(&self) -> bool {
        self.refs_supported_inner()
    }

    fn add_draft_ref(&self, key: &DraftKey, r: crate::data::DraftRef) -> crate::data::AddRef {
        self.add_draft_ref_inner(key, r)
    }

    fn remove_draft_ref(&self, key: &DraftKey, index: usize) {
        self.remove_draft_ref_inner(key, index)
    }

    fn message_block_refs(
        &self,
        message: &crate::data::Message,
    ) -> Vec<chat_core::proto::BlockRef> {
        self.message_block_refs_inner(message)
    }

    fn send_with_refs(
        &self,
        request: SendRequest,
        key: &DraftKey,
        files: Vec<PendingFileId>,
        refs: Vec<chat_core::proto::BlockRefInput>,
    ) -> ClientMsgId {
        self.send_with_refs_inner(request, key, files, refs)
    }

    // --- end inc-refs track ---

    fn subscribe(&self) -> UnboundedReceiver<Vec<Change>> {
        let (tx, rx) = unbounded();
        lock(&self.shared.subscribers).push(tx);
        rx
    }
}
