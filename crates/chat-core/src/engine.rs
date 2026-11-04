//! Connection and sync engine. Follows the Rules section of docs/protocol.md:
//! strict seq order per group with gap fetches, catch-up after `welcome`, outbox resend
//! with the same `clientMsgId`, keepalive, and reconnect with backoff.

use crate::backoff::Backoff;
use crate::client::{
    ClientConfig, ClientUpdate, ConnectionState, SearchRequest, SendFailure, Shared,
};
use crate::proto::{
    AckFrame, AckOutcome, ArtifactEventFrame, CardActionFrame, CardAnswerFrame, ClientFrame,
    ClientInfo, DeleteFrame, DeltaFrame, DirectoryFrame, EditFrame, ErrorBody, ErrorCode, Event,
    FetchFrame, HelloFrame, HistoryFrame, HistoryRequestFrame, Message, MessageDeltaEvent,
    NotifyReason, PROTOCOL_VERSION, PresenceEvent, ReactFrame, ReadFrame, SearchFrame, SendFrame,
    SendResult, ServerFrame, TopicFollowFrame, TopicState, TopicUpdateFrame, TransientEvent,
    TypingEvent, TypingFrame, WelcomeFrame,
};
use crate::proto::{CARD_CATALOGS, CLIENT_FEATURES, CardEventFrame};
use crate::store::{ApplyOutcome, LogEntry, OutboxOp, OutboxState, PendingRead};
use futures_util::{SinkExt, StreamExt};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::{self, Message as WsMessage};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tracing::{debug, info, warn};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

const TYPING_MIN_GAP: Duration = Duration::from_secs(3);
const TICK: Duration = Duration::from_millis(250);
/// Outbox writes in flight at once. The server handles the frames of one socket in order.
const MAX_INFLIGHT_WRITES: usize = 32;
/// Client-side write pace, below the server's per-member write limit (20 per second, burst 20),
/// so a reconnect backlog is not refused with `rate_limited` and reordered by the retries.
const WRITE_RATE_PER_SEC: f64 = 16.0;
const WRITE_BURST: f64 = 16.0;
/// `internal` answers to one write before it is marked failed.
const MAX_INTERNAL_FAILURES: u32 = 5;
/// `internal` error frames on one connection before it reconnects.
const MAX_INTERNAL_ERROR_FRAMES: u32 = 5;

/// Token bucket for outbox writes.
struct WritePace {
    tokens: f64,
    at: Instant,
}

impl WritePace {
    fn new(now: Instant) -> WritePace {
        WritePace {
            tokens: WRITE_BURST,
            at: now,
        }
    }

    fn take(&mut self, now: Instant) -> bool {
        let dt = now.duration_since(self.at).as_secs_f64();
        self.at = now;
        self.tokens = (self.tokens + dt * WRITE_RATE_PER_SEC).min(WRITE_BURST);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[derive(Debug)]
pub(crate) enum Command {
    /// The outbox has new or retried rows.
    Flush,
    Typing {
        group_id: String,
        topic_id: String,
    },
    Read(PendingRead),
    Search {
        rid: String,
        req: SearchRequest,
    },
    LoadOlder {
        group_id: String,
        topic_id: String,
    },
    Delta {
        group_id: String,
        message_id: String,
        append: String,
    },
    /// 1.1: a live write (card event, page event) that is never queued: it is sent now or
    /// fails at once with reason `client.offline`.
    Live {
        frame: Box<ClientFrame>,
        what: LiveWrite,
    },
    ReconnectNow,
    Stop,
}

/// What a live write was, so its ack reaches the right `ClientUpdate`.
#[derive(Debug, Clone)]
pub(crate) enum LiveWrite {
    Card {
        client_event_id: String,
        message_id: String,
    },
    Artifact {
        client_event_id: String,
        artifact_id: String,
        version: u32,
    },
}

/// The ack target of a card answer or card event outbox row (`CardEventAcked`).
fn card_live_write(id: &str, op: &OutboxOp) -> Option<LiveWrite> {
    match op {
        OutboxOp::CardAnswer { message_id, .. } => Some(LiveWrite::Card {
            client_event_id: id.to_string(),
            message_id: message_id.clone(),
        }),
        OutboxOp::CardEvent {
            message_id,
            client_event_id,
            ..
        } => Some(LiveWrite::Card {
            client_event_id: client_event_id.clone(),
            message_id: message_id.clone(),
        }),
        _ => None,
    }
}

/// The error a live write gets when it never left this client.
pub(crate) fn offline_error() -> ErrorBody {
    ErrorBody {
        retry_after_ms: None,
        code: ErrorCode::Internal,
        message: "not connected".into(),
        reason: Some("client.offline".into()),
    }
}

/// Emits the ack of a live write or of a durable card event.
pub(crate) fn emit_live_ack(
    shared: &Shared,
    what: LiveWrite,
    result: Result<Option<serde_json::Value>, ErrorBody>,
) {
    let bad_result = || ErrorBody {
        retry_after_ms: None,
        code: ErrorCode::Internal,
        message: "ack without a valid result".into(),
        reason: None,
    };
    match what {
        LiveWrite::Card {
            client_event_id,
            message_id,
        } => {
            let result = result.and_then(|v| {
                v.and_then(|v| serde_json::from_value(v).ok())
                    .ok_or_else(bad_result)
            });
            shared.emit(ClientUpdate::CardEventAcked {
                client_event_id,
                message_id,
                result,
            });
        }
        LiveWrite::Artifact {
            client_event_id,
            artifact_id,
            version,
        } => {
            let result = result.and_then(|v| {
                v.and_then(|v| serde_json::from_value(v).ok())
                    .ok_or_else(bad_result)
            });
            shared.emit(ClientUpdate::ArtifactEventAcked {
                client_event_id,
                artifact_id,
                version,
                result,
            });
        }
    }
}

enum ConnectError {
    Unauthorized,
    Other(String),
}

enum SessionEnd {
    Stop,
    /// Reconnect; `immediate` skips the backoff wait.
    Reconnect {
        reason: String,
        immediate: bool,
    },
    Unauthorized,
    Unsupported,
}

pub(crate) async fn run(
    shared: Arc<Shared>,
    config: ClientConfig,
    mut cmds: UnboundedReceiver<Command>,
) {
    let mut backoff = Backoff::new(config.backoff_min, config.backoff_max);
    let mut skip_wait = true;
    // Page sizes for the next session. A page that is too large to read is asked for
    // again in smaller pages, so a reconnect does not repeat the same failing fetch.
    let mut fetch_limit = config.fetch_limit.clamp(1, crate::proto::MAX_PAGE_LIMIT);
    let mut history_limit = config.history_limit.clamp(1, crate::proto::MAX_PAGE_LIMIT);
    loop {
        if !skip_wait {
            let delay = backoff.next_delay();
            shared.set_state(ConnectionState::Disconnected { retry_in: delay });
            info!(
                delay_ms = delay.as_millis() as u64,
                attempt = backoff.attempt(),
                "reconnect wait"
            );
            match wait(&shared, &mut cmds, Some(delay)).await {
                WaitEnd::Stop => break,
                WaitEnd::Now => backoff.reset(),
                WaitEnd::Elapsed => {}
            }
        }
        skip_wait = false;
        shared.set_state(ConnectionState::Connecting);
        let token = shared
            .token
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let ws = match tokio::time::timeout(
            config.connect_timeout,
            connect(&config.base_url, &token, config.max_frame_bytes),
        )
        .await
        {
            Ok(Ok(ws)) => ws,
            Ok(Err(ConnectError::Unauthorized)) => {
                warn!("token refused");
                shared.set_state(ConnectionState::Unauthorized);
                match wait(&shared, &mut cmds, None).await {
                    WaitEnd::Stop => break,
                    _ => {
                        skip_wait = true;
                        backoff.reset();
                        continue;
                    }
                }
            }
            Ok(Err(ConnectError::Other(e))) => {
                info!(error = %e, "connect failed");
                continue;
            }
            Err(_) => {
                info!("connect timed out");
                continue;
            }
        };
        let mut session = Session::new(shared.clone(), &config, fetch_limit, history_limit);
        let end = session.run(ws, &mut cmds, &mut backoff).await;
        session.fail_live();
        if session.too_large {
            fetch_limit = (fetch_limit / 4).max(1);
            history_limit = (history_limit / 4).max(1);
            warn!(fetch_limit, history_limit, "frame too large: smaller pages");
        }
        shared.transient().clear_presence();
        shared.transient().clear_streams();
        match end {
            SessionEnd::Stop => break,
            SessionEnd::Reconnect { reason, immediate } => {
                info!(%reason, "connection closed");
                if immediate {
                    backoff.reset();
                    skip_wait = true;
                }
            }
            SessionEnd::Unauthorized => {
                shared.set_state(ConnectionState::Unauthorized);
                match wait(&shared, &mut cmds, None).await {
                    WaitEnd::Stop => break,
                    _ => {
                        skip_wait = true;
                        backoff.reset();
                    }
                }
            }
            SessionEnd::Unsupported => {
                shared.set_state(ConnectionState::Unsupported);
                if let WaitEnd::Stop = wait(&shared, &mut cmds, None).await {
                    break;
                }
                skip_wait = true;
            }
        }
    }
    shared.set_state(ConnectionState::Stopped);
    info!("engine stopped");
}

enum WaitEnd {
    Stop,
    Now,
    Elapsed,
}

/// Waits for the delay (or forever), ending early on `ReconnectNow` or `Stop`. Other
/// commands are dropped: their state is in the store and is sent after the reconnect.
async fn wait(
    shared: &Shared,
    cmds: &mut UnboundedReceiver<Command>,
    delay: Option<Duration>,
) -> WaitEnd {
    let sleep = async {
        match delay {
            Some(d) => tokio::time::sleep(d).await,
            None => std::future::pending().await,
        }
    };
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            _ = &mut sleep => return WaitEnd::Elapsed,
            c = cmds.recv() => match c {
                None | Some(Command::Stop) => return WaitEnd::Stop,
                Some(Command::ReconnectNow) => return WaitEnd::Now,
                Some(Command::Search { rid, .. }) => fail_search(shared, rid),
                Some(Command::Live { what, .. }) => emit_live_ack(shared, what, Err(offline_error())),
                Some(_) => {}
            }
        }
    }
}

pub(crate) fn ws_url(base: &str) -> String {
    let base = base.trim_end_matches('/');
    let rest = if let Some(r) = base.strip_prefix("https://") {
        format!("wss://{r}")
    } else if let Some(r) = base.strip_prefix("http://") {
        format!("ws://{r}")
    } else {
        base.to_string()
    };
    format!("{rest}/v1/ws")
}

async fn connect(base: &str, token: &str, max_frame_bytes: usize) -> Result<Ws, ConnectError> {
    let mut req = ws_url(base)
        .into_client_request()
        .map_err(|e| ConnectError::Other(e.to_string()))?;
    let mut auth = HeaderValue::from_str(&format!("Bearer {token}"))
        .map_err(|_| ConnectError::Unauthorized)?;
    auth.set_sensitive(true);
    req.headers_mut().insert("Authorization", auth);
    let mut cfg = WebSocketConfig::default();
    cfg.max_message_size = Some(max_frame_bytes);
    cfg.max_frame_size = Some(max_frame_bytes);
    match tokio_tungstenite::connect_async_with_config(req, Some(cfg), true).await {
        Ok((ws, _)) => Ok(ws),
        Err(tungstenite::Error::Http(resp))
            if resp.status().as_u16() == 401 || resp.status().as_u16() == 403 =>
        {
            Err(ConnectError::Unauthorized)
        }
        Err(tungstenite::Error::Http(resp)) => {
            Err(ConnectError::Other(format!("http {}", resp.status())))
        }
        Err(e) => Err(ConnectError::Other(e.to_string())),
    }
}

enum Pending {
    Outbox {
        id: String,
    },
    Fetch {
        group_id: String,
    },
    /// A page of the 1.1 cache recovery (`log_refetch`).
    Refetch {
        group_id: String,
    },
    History {
        group_id: String,
        topic_id: String,
    },
    Search {
        rid: String,
    },
    Live(LiveWrite),
}

#[derive(Default)]
struct GroupSync {
    fetch_rid: Option<String>,
    buffer: BTreeMap<u64, LogEntry>,
}

struct Session {
    shared: Arc<Shared>,
    client: ClientInfo,
    ping_interval: Duration,
    pong_timeout: Duration,
    request_timeout: Duration,
    fetch_limit: u32,
    history_limit: u32,
    write_retry_base: Duration,
    me: String,
    welcomed: bool,
    next_rid: u64,
    pending: HashMap<String, (Pending, Instant)>,
    inflight_outbox: HashSet<String>,
    /// Card answers and events in flight by outbox id: their ack still reaches subscribers
    /// when my `card.answered` applied first and removed the row.
    inflight_cards: HashMap<String, LiveWrite>,
    write_pace: WritePace,
    retry_at: HashMap<String, Instant>,
    /// `internal` answers per outbox row on this session.
    internal_failures: HashMap<String, u32>,
    /// Sends reported as `Sent` when their event applied before the ack.
    sent_emitted: HashSet<String>,
    /// `internal` error frames on this connection.
    internal_errors: u32,
    groups: HashMap<String, GroupSync>,
    history_inflight: HashSet<String>,
    typing_sent: HashMap<(String, String), Instant>,
    /// Groups that were behind after `welcome` and are not caught up yet.
    catching_up: BTreeSet<String>,
    next_ping: Instant,
    pong_deadline: Option<Instant>,
    /// Pings sent and pongs received on this connection; pongs answer pings in order.
    pings_sent: u64,
    pongs: u64,
    /// Reads sent but not confirmed, with the number of the pong that confirms each.
    unconfirmed_reads: Vec<(u64, PendingRead)>,
    welcome_deadline: Instant,
    out: Vec<ClientFrame>,
    drop_live_event: Option<crate::client::DropLiveEvent>,
    /// The session ended on a frame larger than `max_frame_bytes`.
    too_large: bool,
    /// The session made sync progress (caught up), so the next reconnect may start at the
    /// shortest delay. A session that ends before that keeps the backoff growing.
    progressed: bool,
}

impl Session {
    fn new(shared: Arc<Shared>, c: &ClientConfig, fetch_limit: u32, history_limit: u32) -> Session {
        let now = Instant::now();
        Session {
            shared,
            client: ClientInfo {
                name: c.client_name.clone(),
                version: c.client_version.clone(),
            },
            ping_interval: c.ping_interval,
            pong_timeout: c.pong_timeout,
            request_timeout: c.request_timeout,
            fetch_limit,
            history_limit,
            write_retry_base: c.write_retry_base,
            drop_live_event: c.drop_live_event.clone(),
            me: String::new(),
            welcomed: false,
            next_rid: 1,
            pending: HashMap::new(),
            inflight_outbox: HashSet::new(),
            inflight_cards: HashMap::new(),
            write_pace: WritePace::new(now),
            retry_at: HashMap::new(),
            internal_failures: HashMap::new(),
            internal_errors: 0,
            sent_emitted: HashSet::new(),
            groups: HashMap::new(),
            history_inflight: HashSet::new(),
            typing_sent: HashMap::new(),
            catching_up: BTreeSet::new(),
            next_ping: now + c.ping_interval,
            pong_deadline: None,
            pings_sent: 0,
            pongs: 0,
            unconfirmed_reads: Vec::new(),
            welcome_deadline: now + c.request_timeout,
            out: Vec::new(),
            too_large: false,
            progressed: false,
        }
    }

    fn rid(&mut self) -> String {
        let r = format!("r{}", self.next_rid);
        self.next_rid += 1;
        r
    }

    async fn run(
        &mut self,
        ws: Ws,
        cmds: &mut UnboundedReceiver<Command>,
        backoff: &mut Backoff,
    ) -> SessionEnd {
        let (mut sink, mut stream) = ws.split();
        let cursors = match self.shared.store.cursors() {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "read cursors");
                BTreeMap::new()
            }
        };
        self.out.push(ClientFrame::Hello(HelloFrame {
            protocol: PROTOCOL_VERSION,
            client: self.client.clone(),
            cursors: Some(cursors),
            features: Some(
                CLIENT_FEATURES
                    .iter()
                    .chain([&crate::proto::FEATURE_SETTINGS])
                    .map(|f| f.to_string())
                    .collect(),
            ),
            supported_catalogs: Some(CARD_CATALOGS.iter().map(|c| c.to_string()).collect()),
        }));
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let end = loop {
            // Write queued frames first.
            let frames = std::mem::take(&mut self.out);
            let mut write_err = None;
            for f in frames {
                let text = match serde_json::to_string(&f) {
                    Ok(t) => t,
                    Err(e) => {
                        warn!(error = %e, "encode frame");
                        continue;
                    }
                };
                if let Err(e) = sink.feed(WsMessage::Text(text.into())).await {
                    write_err = Some(e.to_string());
                    break;
                }
            }
            if write_err.is_none()
                && let Err(e) = sink.flush().await
            {
                write_err = Some(e.to_string());
            }
            if let Some(e) = write_err {
                break SessionEnd::Reconnect {
                    reason: format!("write: {e}"),
                    immediate: false,
                };
            }

            tokio::select! {
                msg = stream.next() => {
                    let msg = match msg {
                        None => break SessionEnd::Reconnect { reason: "closed by server".into(), immediate: false },
                        Some(Err(e)) => {
                            self.too_large = matches!(e, tungstenite::Error::Capacity(_));
                            break SessionEnd::Reconnect { reason: format!("read: {e}"), immediate: false };
                        }
                        Some(Ok(m)) => m,
                    };
                    let text = match msg {
                        WsMessage::Text(t) => t,
                        WsMessage::Close(_) => break SessionEnd::Reconnect { reason: "close frame".into(), immediate: false },
                        _ => continue,
                    };
                    let frame = match parse_incoming(text.as_str()) {
                        Ok(f) => f,
                        Err(e) => {
                            warn!(error = %e, "unparsable server frame");
                            continue;
                        }
                    };
                    let end = self.on_frame(frame);
                    if std::mem::take(&mut self.progressed) {
                        backoff.reset();
                    }
                    if let Some(end) = end {
                        break end;
                    }
                }
                c = cmds.recv() => {
                    match c {
                        None | Some(Command::Stop) => break SessionEnd::Stop,
                        Some(Command::ReconnectNow) => break SessionEnd::Reconnect { reason: "requested".into(), immediate: true },
                        Some(c) => self.on_command(c),
                    }
                }
                _ = tick.tick() => {
                    if let Some(end) = self.on_tick() {
                        break end;
                    }
                }
            }
        };
        let _ = sink.send(WsMessage::Close(None)).await;
        end
    }

    fn on_tick(&mut self) -> Option<SessionEnd> {
        let now = Instant::now();
        if !self.welcomed {
            if now >= self.welcome_deadline {
                return Some(SessionEnd::Reconnect {
                    reason: "no welcome".into(),
                    immediate: false,
                });
            }
            return None;
        }
        if let Some(d) = self.pong_deadline
            && now >= d
        {
            return Some(SessionEnd::Reconnect {
                reason: "pong timeout".into(),
                immediate: false,
            });
        }
        if now >= self.next_ping {
            self.out.push(ClientFrame::Ping);
            self.pings_sent += 1;
            self.next_ping = now + self.ping_interval;
            if self.pong_deadline.is_none() {
                self.pong_deadline = Some(now + self.pong_timeout);
            }
        }
        if self.pending.values().any(|(_, deadline)| now >= *deadline) {
            return Some(SessionEnd::Reconnect {
                reason: "request timeout".into(),
                immediate: false,
            });
        }
        let changed = self.shared.transient().expire_typing(now);
        for (group_id, topic_id) in changed {
            self.shared
                .emit(ClientUpdate::Typing { group_id, topic_id });
        }
        self.typing_sent
            .retain(|_, t| now.duration_since(*t) < TYPING_MIN_GAP);
        self.flush_outbox();
        None
    }

    fn on_command(&mut self, c: Command) {
        if !self.welcomed {
            // Outbox rows and read cursors are in the store and go out after welcome.
            match c {
                Command::Search { rid, .. } => fail_search(&self.shared, rid),
                Command::Live { what, .. } => {
                    emit_live_ack(&self.shared, what, Err(offline_error()))
                }
                _ => {}
            }
            return;
        }
        match c {
            Command::Flush => self.flush_outbox(),
            Command::Typing { group_id, topic_id } => {
                let key = (group_id.clone(), topic_id.clone());
                let now = Instant::now();
                if self
                    .typing_sent
                    .get(&key)
                    .is_some_and(|t| now.duration_since(*t) < TYPING_MIN_GAP)
                {
                    return;
                }
                self.typing_sent.insert(key, now);
                self.out
                    .push(ClientFrame::Typing(TypingFrame { group_id, topic_id }));
            }
            Command::Read(read) => self.send_read(read),
            Command::Search { rid, req } => {
                self.out.push(ClientFrame::Search(SearchFrame {
                    rid: rid.clone(),
                    query: req.query,
                    group_id: req.group_id,
                    topic_id: req.topic_id,
                    limit: req.limit,
                }));
                self.track(rid.clone(), Pending::Search { rid });
            }
            Command::LoadOlder { group_id, topic_id } => {
                if self.history_inflight.contains(&topic_id) {
                    return;
                }
                let before = self
                    .shared
                    .store
                    .oldest_cached_seq(&topic_id)
                    .ok()
                    .flatten();
                let rid = self.rid();
                self.history_inflight.insert(topic_id.clone());
                self.out.push(ClientFrame::History(HistoryRequestFrame {
                    rid: rid.clone(),
                    group_id: group_id.clone(),
                    topic_id: topic_id.clone(),
                    before_seq: before,
                    limit: Some(self.history_limit),
                }));
                self.track(rid, Pending::History { group_id, topic_id });
            }
            Command::Delta {
                group_id,
                message_id,
                append,
            } => {
                self.out.push(ClientFrame::Delta(DeltaFrame {
                    group_id,
                    message_id,
                    append,
                }));
            }
            Command::Live { mut frame, what } => {
                let rid = self.rid();
                match frame.as_mut() {
                    ClientFrame::CardEvent(f) => f.rid = rid.clone(),
                    ClientFrame::ArtifactEvent(f) => f.rid = rid.clone(),
                    _ => {}
                }
                self.out.push(*frame);
                self.track(rid, Pending::Live(what));
            }
            Command::ReconnectNow | Command::Stop => {}
        }
    }

    /// Fails the live writes that have no ack yet (the session ends): they are not resent.
    fn fail_live(&mut self) {
        let live: Vec<String> = self
            .pending
            .iter()
            .filter(|(_, (p, _))| matches!(p, Pending::Live(_)))
            .map(|(rid, _)| rid.clone())
            .collect();
        for rid in live {
            if let Some((Pending::Live(what), _)) = self.pending.remove(&rid) {
                emit_live_ack(&self.shared, what, Err(offline_error()));
            }
        }
    }

    /// Sends a read cursor. A `read` has no ack, so the cursor is marked synced only when the
    /// pong of a ping sent after it arrives: the server then has the frame. A cursor that a dead
    /// socket lost stays unsynced and is sent again after the next `welcome`.
    fn send_read(&mut self, read: PendingRead) {
        self.out.push(ClientFrame::Read(ReadFrame {
            group_id: read.group_id.clone(),
            seq: read.seq,
            topic_id: read.topic_id.clone(),
        }));
        self.unconfirmed_reads.push((self.pings_sent + 1, read));
        // Ping on the next tick, so the read is confirmed soon.
        self.next_ping = self.next_ping.min(Instant::now());
    }

    fn on_pong(&mut self) {
        self.pong_deadline = None;
        self.pongs += 1;
        let pongs = self.pongs;
        let (done, wait): (Vec<_>, Vec<_>) = std::mem::take(&mut self.unconfirmed_reads)
            .into_iter()
            .partition(|(ping, _)| *ping <= pongs);
        self.unconfirmed_reads = wait;
        for (_, read) in done {
            if let Err(e) = self.shared.store.mark_read_synced(&read) {
                warn!(error = %e, "mark read synced");
            }
        }
    }

    fn track(&mut self, rid: String, p: Pending) {
        self.pending
            .insert(rid, (p, Instant::now() + self.request_timeout));
    }

    /// Sends due outbox rows oldest first, pipelined: up to `MAX_INFLIGHT_WRITES` in flight, paced
    /// below the server's write limit. Writes of one group reach the server in outbox order: a row
    /// that waits for its retry holds back the later rows of its group, not of other groups.
    fn flush_outbox(&mut self) {
        if !self.welcomed || self.inflight_outbox.len() >= MAX_INFLIGHT_WRITES {
            return;
        }
        let items = match self.shared.store.outbox_all() {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, "read outbox");
                return;
            }
        };
        let now = Instant::now();
        // One reaction change per message and emoji on the wire at a time, so
        // a quick add then remove reaches the server in that order.
        let mut reacting: std::collections::HashSet<(String, String)> = Default::default();
        let mut held: HashSet<String> = HashSet::new();
        for item in items {
            if let OutboxOp::React {
                message_id, emoji, ..
            } = &item.op
                && !reacting.insert((message_id.clone(), emoji.clone()))
            {
                continue;
            }
            if item.state != OutboxState::Pending || self.inflight_outbox.contains(&item.id) {
                continue;
            }
            // A card press that was sent before waits until its group caught up: if the server
            // applied it, its `card.actioned` event removes the row instead of a second press.
            if item.attempts > 0
                && matches!(
                    item.op,
                    OutboxOp::CardAction { .. } | OutboxOp::CardAnswer { .. }
                )
                && self.catching_up.contains(item.op.group_id())
            {
                continue;
            }
            if held.contains(item.op.group_id()) {
                continue;
            }
            if self.retry_at.get(&item.id).is_some_and(|t| *t > now) {
                held.insert(item.op.group_id().to_string());
                continue;
            }
            if self.inflight_outbox.len() >= MAX_INFLIGHT_WRITES || !self.write_pace.take(now) {
                return;
            }
            let mut op = item.op.clone();
            if let OutboxOp::Send {
                after_root: Some(root),
                topic_id,
                ..
            } = &mut op
            {
                // A follow-up waits until the first message of its topic is acked or applied;
                // that gives the row its topic id.
                if !matches!(self.shared.store.outbox_get(root), Ok(None)) {
                    continue;
                }
                match self.shared.store.my_topic_of_client_msg(root) {
                    Ok(Some(t)) => {
                        let _ = self.shared.store.resolve_followups(root, &t);
                        *topic_id = Some(t);
                    }
                    _ => {
                        warn!(id = %item.id, "follow-up of a topic that was not started");
                        self.fail_row(&item, ErrorCode::NotFound, String::new());
                        continue;
                    }
                }
            }
            let rid = self.rid();
            let frame = match op {
                OutboxOp::Send {
                    group_id,
                    topic_id,
                    title,
                    body,
                    mentions,
                    streaming,
                    reply_to,
                    attachments,
                    artifact_refs,
                    card,
                    card_v2,
                    context,
                    cause_event_id,
                    block_refs,
                    from_aside,
                    ..
                } => ClientFrame::Send(SendFrame {
                    from_aside,
                    rid: rid.clone(),
                    group_id,
                    topic_id,
                    title,
                    client_msg_id: item.id.clone(),
                    body,
                    mentions: (!mentions.is_empty()).then_some(mentions),
                    streaming: streaming.then_some(true),
                    reply_to: OutboxOp::reply_to_ref(&reply_to),
                    attachments,
                    artifact_refs,
                    card,
                    card_v2,
                    context,
                    cause_event_id,
                    block_refs,
                }),
                OutboxOp::Edit {
                    group_id,
                    message_id,
                    body,
                    streaming,
                    attachments,
                    artifact_refs,
                    card,
                    card_v2,
                    ..
                } => ClientFrame::Edit(EditFrame {
                    rid: rid.clone(),
                    group_id,
                    message_id,
                    body,
                    streaming: Some(streaming),
                    attachments,
                    artifact_refs,
                    card,
                    card_v2,
                    base_rev: None,
                    cause_event_id: None,
                }),
                OutboxOp::Delete {
                    group_id,
                    message_id,
                    ..
                } => ClientFrame::Delete(DeleteFrame {
                    rid: rid.clone(),
                    group_id,
                    message_id,
                }),
                OutboxOp::TopicUpdate {
                    group_id,
                    topic_id,
                    title,
                    status,
                    waiting_on,
                    owner_id,
                } => ClientFrame::TopicUpdate(TopicUpdateFrame {
                    rid: rid.clone(),
                    group_id,
                    topic_id,
                    title,
                    status,
                    waiting_on,
                    owner_id,
                }),
                OutboxOp::TopicFollow {
                    group_id,
                    topic_id,
                    follow,
                } => ClientFrame::TopicFollow(TopicFollowFrame {
                    rid: rid.clone(),
                    group_id,
                    topic_id,
                    follow,
                }),
                OutboxOp::CardAction {
                    group_id,
                    message_id,
                    action_id,
                    ..
                } => ClientFrame::CardAction(CardActionFrame {
                    rid: rid.clone(),
                    group_id,
                    message_id,
                    action_id,
                    value: None,
                }),
                OutboxOp::React {
                    group_id,
                    message_id,
                    emoji,
                    on,
                    ..
                } => ClientFrame::React(ReactFrame {
                    rid: rid.clone(),
                    group_id,
                    message_id,
                    emoji,
                    on,
                }),
                OutboxOp::CardAnswer {
                    group_id,
                    message_id,
                    rev,
                    action_id,
                    inputs,
                    edited_values,
                    content_hash,
                    ..
                } => ClientFrame::CardAnswer(CardAnswerFrame {
                    rid: rid.clone(),
                    group_id,
                    message_id,
                    rev,
                    action_id,
                    inputs,
                    edited_values,
                    client_event_id: Some(item.id.clone()),
                    content_hash,
                }),
                OutboxOp::ArtifactEvent {
                    group_id,
                    artifact_id,
                    version,
                    name,
                    payload,
                    ..
                } => ClientFrame::ArtifactEvent(ArtifactEventFrame {
                    rid: rid.clone(),
                    group_id,
                    artifact_id,
                    version,
                    client_event_id: None,
                    name,
                    kind: None,
                    payload,
                }),
                OutboxOp::CardEvent {
                    group_id,
                    message_id,
                    rev,
                    client_event_id,
                    block_id,
                    action_id,
                    event,
                    value,
                    inputs,
                    part,
                    ..
                } => ClientFrame::CardEvent(CardEventFrame {
                    rid: rid.clone(),
                    group_id,
                    message_id,
                    rev,
                    client_event_id,
                    block_id,
                    action_id,
                    event,
                    value,
                    inputs,
                    part,
                    content_hash: None,
                }),
            };
            if let Err(e) = self.shared.store.outbox_attempt(&item.id) {
                warn!(error = %e, "outbox attempt");
            }
            debug!(id = %item.id, kind = item.op.kind(), attempts = item.attempts + 1, "outbox send");
            self.inflight_outbox.insert(item.id.clone());
            if let Some(w) = card_live_write(&item.id, &item.op) {
                self.inflight_cards.insert(item.id.clone(), w);
            }
            self.out.push(frame);
            self.track(rid, Pending::Outbox { id: item.id });
        }
    }

    fn on_frame(&mut self, frame: Incoming) -> Option<SessionEnd> {
        let frame = match frame {
            Incoming::Unsupported => return Some(SessionEnd::Unsupported),
            Incoming::Event(entry, notify) => {
                if let Some(drop) = &self.drop_live_event
                    && drop(entry.group_id(), entry.seq())
                {
                    debug!(group = %entry.group_id(), seq = entry.seq(), "test hook dropped event");
                    return None;
                }
                if self.welcomed {
                    let gid = entry.group_id().to_string();
                    let note = notify.map(|reason| (reason, entry.clone()));
                    self.groups
                        .entry(gid.clone())
                        .or_default()
                        .buffer
                        .insert(entry.seq(), entry);
                    self.drain(&gid);
                    // After the drain, so the cache knows the topic when the event applied.
                    if let Some(n) =
                        note.and_then(|((r, ev), e)| notify_update(&self.shared, r, ev, &e))
                    {
                        self.shared.emit(n);
                    }
                }
                return None;
            }
            Incoming::Page {
                rid,
                group_id,
                entries,
                has_more,
                last_seq,
            } => {
                self.on_page(rid, group_id, entries, has_more, last_seq);
                return None;
            }
            Incoming::Frame(f) => f,
        };
        match frame {
            ServerFrame::Welcome(WelcomeFrame {
                me,
                members,
                groups,
                features,
                ..
            }) => {
                self.shared
                    .set_server_features(features.unwrap_or_default());
                let out = match self.shared.store.apply_welcome(&me, &members, &groups) {
                    Ok(o) => o,
                    Err(e) => {
                        warn!(error = %e, "apply welcome");
                        return Some(SessionEnd::Reconnect {
                            reason: "store".into(),
                            immediate: false,
                        });
                    }
                };
                info!(groups = groups.len(), members = members.len(), "welcome");
                self.me = me.id.clone();
                self.welcomed = true;
                self.shared.set_state(ConnectionState::Connected);
                self.shared.emit(ClientUpdate::DirectoryChanged);
                for g in &out.groups_changed {
                    self.shared.emit(ClientUpdate::GroupChanged {
                        group_id: g.clone(),
                    });
                }
                for g in &groups {
                    let last = self.shared.store.last_applied(&g.group.id).unwrap_or(0);
                    if g.last_seq > last {
                        self.catching_up.insert(g.group.id.clone());
                        self.start_fetch(&g.group.id);
                    }
                }
                if self.catching_up.is_empty() {
                    self.caught_up();
                }
                // 1.1 cache recovery: re-read logs that 1.0.x applied, once.
                let known: BTreeSet<&str> = groups.iter().map(|g| g.group.id.as_str()).collect();
                for (group_id, r) in self.shared.store.refetches().unwrap_or_default() {
                    if known.contains(group_id.as_str()) {
                        self.start_refetch(&group_id, r.after);
                    }
                }
                if let Ok(reads) = self.shared.store.unsynced_reads() {
                    for r in reads {
                        self.send_read(r);
                    }
                }
                self.flush_outbox();
            }
            // Parsed by `parse_incoming` into `Incoming::Event` and `Incoming::Page`.
            ServerFrame::Event(_) | ServerFrame::Page(_) => {}
            ServerFrame::History(HistoryFrame {
                rid,
                group_id,
                topic_id,
                messages,
                has_more,
            }) => {
                self.pending.remove(&rid)?;
                self.history_inflight.remove(&topic_id);
                let msgs: Vec<Message> = messages
                    .into_iter()
                    .filter(|m| m.topic_id == topic_id)
                    .collect();
                if let Err(e) = self.shared.store.insert_history(&topic_id, &msgs, has_more) {
                    warn!(error = %e, "store history");
                }
                self.shared.emit(ClientUpdate::MessagesChanged {
                    group_id: group_id.clone(),
                    topic_id: topic_id.clone(),
                });
                self.shared.emit(ClientUpdate::HistoryLoaded {
                    group_id,
                    topic_id,
                    has_more,
                });
            }
            ServerFrame::Ack(AckFrame { rid, outcome }) => {
                let (p, _) = self.pending.remove(&rid)?;
                let (ok, result, error) = match outcome {
                    AckOutcome::Ok { result } => (true, result, None),
                    AckOutcome::Err { error } => (false, None, Some(error)),
                };
                match p {
                    Pending::Outbox { id } => {
                        let end = self.on_outbox_ack(id, ok, result, error);
                        if end.is_none() {
                            self.flush_outbox();
                        }
                        return end;
                    }
                    Pending::Fetch { group_id } => {
                        // A fetch answered with an error ack instead of a page.
                        warn!(group = %group_id, code = ?error.as_ref().map(|e| e.code), "fetch refused");
                        if let Some(g) = self.groups.get_mut(&group_id) {
                            g.fetch_rid = None;
                        }
                        if self.catching_up.remove(&group_id) && self.catching_up.is_empty() {
                            self.caught_up();
                        }
                    }
                    Pending::History { group_id, topic_id } => {
                        self.history_inflight.remove(&topic_id);
                        warn!(group = %group_id, code = ?error.as_ref().map(|e| e.code), "history refused");
                    }
                    Pending::Refetch { group_id } => {
                        // Tried again on the next connection.
                        warn!(group = %group_id, code = ?error.as_ref().map(|e| e.code), "recovery fetch refused");
                    }
                    Pending::Search { rid } => {
                        let (code, message) = match error {
                            Some(e) => (Some(e.code), e.message),
                            None => (None, String::new()),
                        };
                        self.shared
                            .emit(ClientUpdate::SearchFailed { rid, code, message });
                    }
                    Pending::Live(what) => {
                        let r = if ok {
                            Ok(result)
                        } else {
                            Err(error.unwrap_or_else(offline_error))
                        };
                        emit_live_ack(&self.shared, what, r);
                    }
                }
            }
            ServerFrame::Transient(t) => self.on_transient(t.event),
            ServerFrame::Directory(DirectoryFrame {
                members,
                groups,
                removed_group_ids,
            }) => {
                let out = match self.shared.store.apply_directory(
                    members.as_deref(),
                    groups.as_deref(),
                    removed_group_ids.as_deref(),
                ) {
                    Ok(o) => o,
                    Err(e) => {
                        warn!(error = %e, "apply directory");
                        return None;
                    }
                };
                for g in &out.groups_removed {
                    self.forget_group(g);
                }
                self.shared.emit(ClientUpdate::DirectoryChanged);
                for g in &out.groups_changed {
                    self.shared.emit(ClientUpdate::GroupChanged {
                        group_id: g.clone(),
                    });
                }
                for g in groups.unwrap_or_default() {
                    let last = self.shared.store.last_applied(&g.group.id).unwrap_or(0);
                    if g.last_seq > last {
                        self.start_fetch(&g.group.id);
                    }
                }
            }
            ServerFrame::Error(e) => {
                // The gateway also answers a refused frame without `rid` (read, typing, delta)
                // with an error frame. Only an auth or protocol error, or a run of internal
                // errors, ends the connection; the others are about one frame.
                let code = e.error.code;
                warn!(?code, "error frame");
                match code {
                    ErrorCode::Unauthorized => return Some(SessionEnd::Unauthorized),
                    ErrorCode::UnsupportedProtocol => return Some(SessionEnd::Unsupported),
                    ErrorCode::Internal => {
                        self.internal_errors += 1;
                        if self.internal_errors >= MAX_INTERNAL_ERROR_FRAMES {
                            return Some(SessionEnd::Reconnect {
                                reason: "repeated internal error frames".into(),
                                immediate: false,
                            });
                        }
                    }
                    _ => {}
                }
            }
            ServerFrame::Reads(f) => {
                let received = self.received_seq(&f.group_id);
                if let Err(e) = self.shared.store.apply_reads(&f, received) {
                    warn!(error = %e, "apply reads");
                }
                self.shared.emit(ClientUpdate::GroupChanged {
                    group_id: f.group_id.clone(),
                });
                self.shared.emit(ClientUpdate::TopicsChanged {
                    group_id: f.group_id,
                });
            }
            ServerFrame::SearchResults(f) => {
                if self.pending.remove(&f.rid).is_some() {
                    self.shared.emit(ClientUpdate::SearchResults {
                        rid: f.rid,
                        hits: f.hits,
                    });
                }
            }
            ServerFrame::Pong => self.on_pong(),
        }
        None
    }

    /// Drops the sync state of a group I left: its pending fetches and history requests are
    /// forgotten, so a page that arrives later does not bring back rows of the removed group.
    fn forget_group(&mut self, group_id: &str) {
        self.groups.remove(group_id);
        let mut topics = Vec::new();
        self.pending.retain(|_, (p, _)| match p {
            Pending::Fetch { group_id: g } | Pending::Refetch { group_id: g } => g != group_id,
            Pending::History {
                group_id: g,
                topic_id,
            } if g == group_id => {
                topics.push(topic_id.clone());
                false
            }
            _ => true,
        });
        for t in topics {
            self.history_inflight.remove(&t);
        }
        if self.catching_up.remove(group_id) && self.catching_up.is_empty() {
            self.caught_up();
        }
    }

    /// Catch-up after `welcome` is done: the connection is healthy, so the backoff resets.
    fn caught_up(&mut self) {
        self.progressed = true;
        self.shared.emit(ClientUpdate::CaughtUp);
    }

    /// Newest seq of a group received on this connection, applied or buffered.
    fn received_seq(&self, group_id: &str) -> u64 {
        let applied = self.shared.store.last_applied(group_id).unwrap_or(0);
        let buffered = self
            .groups
            .get(group_id)
            .and_then(|g| g.buffer.keys().next_back().copied())
            .unwrap_or(0);
        applied.max(buffered)
    }

    fn on_page(
        &mut self,
        rid: String,
        group_id: String,
        entries: Vec<LogEntry>,
        has_more: bool,
        last_seq: u64,
    ) {
        let pending = match self.pending.remove(&rid) {
            Some((Pending::Refetch { group_id: g }, _)) => {
                self.on_refetch_page(g, entries, has_more);
                return;
            }
            Some(p) => p,
            None => return,
        };
        let (Pending::Fetch { .. }, _) = pending else {
            return;
        };
        let empty = entries.is_empty();
        {
            let gs = self.groups.entry(group_id.clone()).or_default();
            if gs.fetch_rid.as_deref() == Some(rid.as_str()) {
                gs.fetch_rid = None;
            }
            for e in entries.into_iter().filter(|e| e.group_id() == group_id) {
                gs.buffer.insert(e.seq(), e);
            }
        }
        let last = self.drain(&group_id);
        let gap = self
            .groups
            .get(&group_id)
            .is_some_and(|g| !g.buffer.is_empty());
        if (has_more || gap || last < last_seq) && !empty {
            self.start_fetch(&group_id);
        } else {
            if last < last_seq || gap {
                warn!(group = %group_id, last, last_seq, "fetch ended behind the server");
            }
            if self.catching_up.remove(&group_id) && self.catching_up.is_empty() {
                self.caught_up();
            }
        }
    }

    fn on_outbox_ack(
        &mut self,
        id: String,
        ok: bool,
        result: Option<serde_json::Value>,
        error: Option<ErrorBody>,
    ) -> Option<SessionEnd> {
        self.inflight_outbox.remove(&id);
        let inflight_card = self.inflight_cards.remove(&id);
        let item = match self.shared.store.outbox_get(&id) {
            Ok(Some(i)) => i,
            // Already removed: its event arrived first, or the UI discarded it.
            _ => {
                if ok
                    && result.is_some()
                    && let Some(what) = inflight_card
                {
                    emit_live_ack(&self.shared, what, Ok(result));
                    return None;
                }
                if ok
                    && let Some(r) =
                        result.and_then(|v| serde_json::from_value::<SendResult>(v).ok())
                    && r.message.client_msg_id == id
                {
                    let _ = self.shared.store.ack_send(&id, &r);
                    if !self.sent_emitted.remove(&id) {
                        self.emit_sent(id, r);
                    }
                }
                return None;
            }
        };
        let group_id = item.op.group_id().to_string();
        let card_ack = card_live_write(&id, &item.op);
        if ok {
            if let Some(what) = card_ack.clone() {
                // A 1.0 server acks card.answer without a result: no CardEventAcked then.
                if result.is_some() {
                    emit_live_ack(&self.shared, what, Ok(result.clone()));
                }
            }
        } else if let Some(what) = card_ack.clone()
            && let Some(e) = &error
            && !matches!(e.code, ErrorCode::Internal | ErrorCode::RateLimited)
        {
            emit_live_ack(&self.shared, what, Err(e.clone()));
        }
        if ok {
            self.retry_at.remove(&id);
            match &item.op {
                OutboxOp::Send { .. } => {
                    let parsed = result.and_then(|v| serde_json::from_value::<SendResult>(v).ok());
                    match parsed {
                        Some(r) => {
                            if let Err(e) = self.shared.store.ack_send(&id, &r) {
                                warn!(error = %e, "store ack");
                            }
                            self.emit_sent(id, r);
                        }
                        None => {
                            warn!(id = %id, "send ack without a valid result");
                            let _ = self.shared.store.outbox_remove(&id);
                        }
                    }
                }
                OutboxOp::TopicFollow { group_id, .. } => {
                    if let Some(state) =
                        result.and_then(|v| serde_json::from_value::<TopicState>(v).ok())
                        && let Err(e) = self.shared.store.apply_topic_state(group_id, &state)
                    {
                        warn!(error = %e, "store topic state");
                    }
                    let _ = self.shared.store.outbox_remove(&id);
                    self.shared.emit(ClientUpdate::TopicsChanged {
                        group_id: group_id.clone(),
                    });
                }
                OutboxOp::CardAction {
                    group_id,
                    topic_id: Some(topic_id),
                    ..
                }
                | OutboxOp::CardAnswer {
                    group_id,
                    topic_id: Some(topic_id),
                    ..
                }
                | OutboxOp::CardEvent {
                    group_id,
                    topic_id: Some(topic_id),
                    ..
                }
                | OutboxOp::React {
                    group_id,
                    topic_id: Some(topic_id),
                    ..
                } => {
                    // The UI shows a press as sending until its row is gone.
                    let _ = self.shared.store.outbox_remove(&id);
                    self.shared.emit(ClientUpdate::MessagesChanged {
                        group_id: group_id.clone(),
                        topic_id: topic_id.clone(),
                    });
                }
                _ => {
                    let _ = self.shared.store.outbox_remove(&id);
                }
            }
            return None;
        }
        let err = error.unwrap_or(ErrorBody {
            retry_after_ms: None,
            code: ErrorCode::Internal,
            message: String::new(),
            reason: None,
        });
        let internal_failures = if err.code == ErrorCode::Internal {
            let n = self.internal_failures.entry(id.clone()).or_default();
            *n += 1;
            *n
        } else {
            0
        };
        match err.code {
            // A write that keeps failing with `internal` becomes failed below, so it does not
            // hold back its group forever and the member can retry or discard it.
            ErrorCode::RateLimited | ErrorCode::Internal
                if internal_failures < MAX_INTERNAL_FAILURES =>
            {
                let attempts = item.attempts.clamp(0, 6) as u32;
                let delay = self
                    .write_retry_base
                    .saturating_mul(1u32 << attempts)
                    .min(Duration::from_secs(60));
                self.retry_at.insert(id.clone(), Instant::now() + delay);
                let _ = self.shared.store.outbox_set_state(
                    &id,
                    OutboxState::Pending,
                    Some(code_str(err.code)),
                );
                None
            }
            ErrorCode::Unauthorized => Some(SessionEnd::Unauthorized),
            ErrorCode::NotFound if matches!(item.op, OutboxOp::Delete { .. }) => {
                // Already gone.
                let _ = self.shared.store.outbox_remove(&id);
                None
            }
            code if matches!(item.op, OutboxOp::ArtifactEvent { .. }) => {
                // A refused page event (undeclared name, rate limit after retries) is dropped.
                warn!(id = %id, ?code, "artifact event refused");
                let _ = self.shared.store.outbox_remove(&id);
                None
            }
            code if matches!(item.op, OutboxOp::React { .. }) => {
                // A refused reaction (message deleted, bad emoji) is dropped: the
                // timeline shows the server's list again.
                warn!(id = %id, ?code, "reaction refused");
                let _ = self.shared.store.outbox_remove(&id);
                if let Some(t) = item.op.topic_id() {
                    self.shared.emit(ClientUpdate::MessagesChanged {
                        group_id: group_id.clone(),
                        topic_id: t.to_string(),
                    });
                }
                None
            }
            code => {
                warn!(id = %id, kind = item.op.kind(), ?code, "write refused");
                self.fail_row(&item, code, err.message);
                None
            }
        }
    }

    /// Marks an outbox row failed (retry and discard become available) and tells the UI.
    fn fail_row(&mut self, item: &crate::store::OutboxItem, code: ErrorCode, message: String) {
        let id = item.id.clone();
        let group_id = item.op.group_id().to_string();
        self.internal_failures.remove(&id);
        self.retry_at.remove(&id);
        let _ = self
            .shared
            .store
            .outbox_set_state(&id, OutboxState::Failed, Some(code_str(code)));
        let topic_id = item.op.topic_id().map(str::to_string);
        match &topic_id {
            Some(t) => self.shared.emit(ClientUpdate::MessagesChanged {
                group_id: group_id.clone(),
                topic_id: t.clone(),
            }),
            None => self.shared.emit(ClientUpdate::TopicsChanged {
                group_id: group_id.clone(),
            }),
        }
        self.shared.emit(ClientUpdate::SendFailed(SendFailure {
            id,
            kind: item.op.kind(),
            group_id,
            topic_id,
            code,
            message,
        }));
    }

    fn emit_sent(&self, client_msg_id: String, r: SendResult) {
        let group_id = r.message.group_id.clone();
        let topic_id = r.message.topic_id.clone();
        self.shared.emit(ClientUpdate::TopicsChanged {
            group_id: group_id.clone(),
        });
        self.shared.emit(ClientUpdate::MessagesChanged {
            group_id,
            topic_id: topic_id.clone(),
        });
        self.shared.emit(ClientUpdate::Sent {
            client_msg_id,
            message_id: r.message.id,
            topic_id,
            duplicate: r.duplicate.unwrap_or(false),
        });
    }

    fn on_transient(&mut self, ev: TransientEvent) {
        match ev {
            TransientEvent::Typing(TypingEvent {
                group_id,
                topic_id,
                member_id,
                ..
            }) => {
                if member_id == self.me {
                    return;
                }
                self.shared
                    .transient()
                    .on_typing(&group_id, &topic_id, &member_id, Instant::now());
                self.shared
                    .emit(ClientUpdate::Typing { group_id, topic_id });
            }
            TransientEvent::Presence(PresenceEvent {
                member_id, online, ..
            }) => {
                if self.shared.transient().set_presence(&member_id, online) {
                    self.shared
                        .emit(ClientUpdate::Presence { member_id, online });
                }
            }
            TransientEvent::MessageDelta(MessageDeltaEvent {
                group_id,
                topic_id,
                message_id,
                append,
            }) => {
                self.shared.transient().append_delta(&message_id, &append);
                self.shared
                    .emit(ClientUpdate::MessagesChanged { group_id, topic_id });
            }
            TransientEvent::CardEvent(e) => self.shared.emit(ClientUpdate::CardEvent(Box::new(e))),
            TransientEvent::InteractionResolved(e) => {
                self.shared.emit(ClientUpdate::InteractionResolved(e))
            }
            TransientEvent::ArtifactEvent(e) => {
                self.shared.emit(ClientUpdate::ArtifactEvent(Box::new(e)))
            }
            TransientEvent::ArtifactData(e) => {
                self.shared.emit(ClientUpdate::ArtifactData(Box::new(e)))
            }
            TransientEvent::ArtifactReply(e) => {
                self.shared.emit(ClientUpdate::ArtifactReply(Box::new(e)))
            }
            // settings-backend: the daily digest (feature "settings/1").
            TransientEvent::Digest(d) => self.shared.emit(ClientUpdate::Digest(Box::new(d))),
        }
    }

    fn start_refetch(&mut self, group_id: &str, after: u64) {
        let rid = self.rid();
        debug!(group = %group_id, after, "recovery fetch");
        self.out.push(ClientFrame::Fetch(FetchFrame {
            rid: rid.clone(),
            group_id: group_id.to_string(),
            after_seq: after,
            limit: Some(self.fetch_limit),
        }));
        self.track(
            rid,
            Pending::Refetch {
                group_id: group_id.to_string(),
            },
        );
    }

    /// One page of the 1.1 cache recovery: re-applies what 1.0.x dropped, then asks for the
    /// next page until the recorded seq is reached.
    fn on_refetch_page(&mut self, group_id: String, entries: Vec<LogEntry>, has_more: bool) {
        let Ok(Some(r)) = self.shared.store.refetch(&group_id) else {
            return;
        };
        let last = entries.iter().map(LogEntry::seq).max().unwrap_or(r.after);
        let done = !has_more || entries.is_empty() || last >= r.upto || last <= r.after;
        let mut out = match self
            .shared
            .store
            .apply_recovered(&group_id, &entries, last, done)
        {
            Ok(o) => o,
            Err(e) => {
                warn!(group = %group_id, error = %e, "apply recovery page");
                return;
            }
        };
        // Recovered rows are not new events: report the changes, never as notifications.
        out.other_events.clear();
        out.applied = out.applied.max(1);
        self.emit_outcome(&group_id, &out);
        if !done {
            self.start_refetch(&group_id, last);
        } else {
            info!(group = %group_id, "cache recovery done");
        }
    }

    fn start_fetch(&mut self, group_id: &str) {
        let after = self.shared.store.last_applied(group_id).unwrap_or(0);
        let rid = self.rid();
        let gs = self.groups.entry(group_id.to_string()).or_default();
        if gs.fetch_rid.is_some() {
            return;
        }
        gs.fetch_rid = Some(rid.clone());
        debug!(group = %group_id, after, "fetch");
        self.out.push(ClientFrame::Fetch(FetchFrame {
            rid: rid.clone(),
            group_id: group_id.to_string(),
            after_seq: after,
            limit: Some(self.fetch_limit),
        }));
        self.track(
            rid,
            Pending::Fetch {
                group_id: group_id.to_string(),
            },
        );
    }

    /// Applies the contiguous buffered events of a group; fetches when a gap remains.
    /// Returns `lastApplied`.
    fn drain(&mut self, group_id: &str) -> u64 {
        let events: Vec<LogEntry> = match self.groups.get(group_id) {
            Some(g) if !g.buffer.is_empty() => g.buffer.values().cloned().collect(),
            _ => return self.shared.store.last_applied(group_id).unwrap_or(0),
        };
        let out = match self.shared.store.apply_log(group_id, &events) {
            Ok(o) => o,
            Err(e) => {
                warn!(group = %group_id, error = %e, "apply events");
                return self.shared.store.last_applied(group_id).unwrap_or(0);
            }
        };
        let last = out.last_applied;
        let gs = self.groups.entry(group_id.to_string()).or_default();
        gs.buffer = gs.buffer.split_off(&(last + 1));
        let gap = !gs.buffer.is_empty();
        let fetching = gs.fetch_rid.is_some();
        self.emit_outcome(group_id, &out);
        if gap && !fetching {
            debug!(group = %group_id, last, "gap");
            self.start_fetch(group_id);
        }
        last
    }

    fn emit_outcome(&mut self, group_id: &str, out: &ApplyOutcome) {
        if out.applied == 0 {
            return;
        }
        {
            let mut t = self.shared.transient();
            for id in &out.replaced_bodies {
                t.clear_stream(id);
            }
        }
        for (topic_id, author) in &out.created {
            if self
                .shared
                .transient()
                .clear_typing(group_id, topic_id, author)
            {
                self.shared.emit(ClientUpdate::Typing {
                    group_id: group_id.into(),
                    topic_id: topic_id.clone(),
                });
            }
        }
        for id in &out.my_client_msg_ids {
            self.retry_at.remove(id);
        }
        // My sends applied before their ack: report them now, since the ack can be lost.
        for r in &out.sent {
            let id = r.message.client_msg_id.clone();
            if self.sent_emitted.len() > 1000 {
                self.sent_emitted.clear();
            }
            self.sent_emitted.insert(id.clone());
            self.emit_sent(id, r.clone());
        }
        for ev in &out.other_events {
            self.shared
                .emit(ClientUpdate::GroupEvent(Box::new(ev.clone())));
        }
        for t in &out.run_topics {
            self.shared.emit(ClientUpdate::RunsChanged {
                group_id: group_id.into(),
                topic_id: t.clone(),
            });
        }
        for t in &out.artifact_topics {
            self.shared.emit(ClientUpdate::ArtifactsChanged {
                group_id: group_id.into(),
                topic_id: t.clone(),
            });
        }
        if out.group_changed {
            self.shared.emit(ClientUpdate::GroupChanged {
                group_id: group_id.into(),
            });
        }
        if out.topic_list_changed {
            self.shared.emit(ClientUpdate::TopicsChanged {
                group_id: group_id.into(),
            });
        }
        for t in &out.message_topics {
            self.shared.emit(ClientUpdate::MessagesChanged {
                group_id: group_id.into(),
                topic_id: t.clone(),
            });
        }
    }
}

pub(crate) enum Incoming {
    Frame(ServerFrame),
    /// The event, its notify reason and (settings) its switchable `notifyEvent`.
    Event(
        LogEntry,
        Option<(NotifyReason, Option<crate::proto::NotifyEvent>)>,
    ),
    Page {
        rid: String,
        group_id: String,
        entries: Vec<LogEntry>,
        has_more: bool,
        last_seq: u64,
    },
    /// A `welcome` for another protocol version.
    Unsupported,
}

/// Parses a server frame. Events are parsed one by one, so an event type added by a
/// newer server keeps its place in the seq order instead of failing the whole frame.
pub(crate) fn parse_incoming(text: &str) -> Result<Incoming, String> {
    let mut v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    match v.get("t").and_then(|t| t.as_str()) {
        Some("event") => {
            // An unknown reason reads as `message` (docs/protocol.md, Notifications).
            let notify = v.get("notify").and_then(|n| n.as_str()).map(|n| {
                serde_json::from_value(serde_json::Value::String(n.into()))
                    .unwrap_or(NotifyReason::Message)
            });
            // settings: an unknown event reads as `Unknown`.
            let notify_event = v.get("notifyEvent").and_then(|n| n.as_str()).map(|n| {
                serde_json::from_value(serde_json::Value::String(n.into()))
                    .unwrap_or(crate::proto::NotifyEvent::Unknown)
            });
            let notify = notify.map(|r| (r, notify_event));
            let ev = v
                .get_mut("event")
                .map(serde_json::Value::take)
                .ok_or("event frame without event")?;
            Ok(Incoming::Event(log_entry(ev)?, notify))
        }
        Some("page") => {
            let rid = v
                .get("rid")
                .and_then(|x| x.as_str())
                .ok_or("page without rid")?
                .to_string();
            let group_id = v
                .get("groupId")
                .and_then(|x| x.as_str())
                .ok_or("page without groupId")?
                .to_string();
            let has_more = v
                .get("hasMore")
                .and_then(|x| x.as_bool())
                .ok_or("page without hasMore")?;
            let last_seq = v
                .get("lastSeq")
                .and_then(|x| x.as_u64())
                .ok_or("page without lastSeq")?;
            let events = match v.get_mut("events").map(serde_json::Value::take) {
                Some(serde_json::Value::Array(a)) => a,
                _ => return Err("page without events".into()),
            };
            let entries = events
                .into_iter()
                .map(log_entry)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Incoming::Page {
                rid,
                group_id,
                entries,
                has_more,
                last_seq,
            })
        }
        Some("welcome")
            if v.get("protocol").and_then(|p| p.as_u64()) != Some(PROTOCOL_VERSION as u64) =>
        {
            Ok(Incoming::Unsupported)
        }
        _ => crate::proto::parse_server_frame(text)
            .map(Incoming::Frame)
            .map_err(|e| e.to_string()),
    }
}

fn log_entry(v: serde_json::Value) -> Result<LogEntry, String> {
    let seq = v.get("seq").and_then(|x| x.as_u64());
    let group_id = v
        .get("groupId")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    let event_type = v
        .get("type")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    match serde_json::from_value::<Event>(v) {
        Ok(e) => Ok(LogEntry::Known(e)),
        Err(err) => match (seq, group_id) {
            (Some(seq), Some(group_id)) if seq > 0 => {
                warn!(seq, event_type = %event_type, error = %err, "skipping event this client cannot read");
                Ok(LogEntry::Unknown {
                    seq,
                    group_id,
                    event_type,
                })
            }
            _ => Err(err.to_string()),
        },
    }
}

fn fail_search(shared: &Shared, rid: String) {
    shared.emit(ClientUpdate::SearchFailed {
        rid,
        code: None,
        message: "not connected".into(),
    });
}

/// Builds the notification for a server-marked event, or `None` for my own event or a
/// non-mention in a muted group (the server should not mark those; this guards stale
/// decisions). A muted group still notifies mentions (docs/protocol.md, Notifications).
fn notify_update(
    shared: &Shared,
    reason: NotifyReason,
    event: Option<crate::proto::NotifyEvent>,
    entry: &LogEntry,
) -> Option<ClientUpdate> {
    use crate::proto::EventData;
    let LogEntry::Known(e) = entry else {
        return None;
    };
    let store = &shared.store;
    let me = store.me_id().ok().flatten();
    if me.as_deref() == Some(e.actor_id.as_str()) {
        return None;
    }
    let group = store.group(&e.group_id).ok().flatten();
    if reason != NotifyReason::Mention && group.as_ref().is_some_and(|g| g.muted) {
        return None;
    }
    let (topic_id, message_id, body) = match &e.data {
        EventData::MessageCreated(d) => (
            Some(d.message.topic_id.clone()),
            Some(d.message.id.clone()),
            d.message.body.chars().take(200).collect(),
        ),
        EventData::TopicUpdated(d) => (Some(d.topic_id.clone()), None, String::new()),
        EventData::TopicCreated(d) => (Some(d.topic.id.clone()), None, String::new()),
        EventData::ArtifactCreated(d) | EventData::ArtifactVersionAdded(d) => (
            Some(d.artifact.topic_id.clone()),
            None,
            d.artifact.title.clone(),
        ),
        _ => (None, None, String::new()),
    };
    let subtitle = match (&e.data, &topic_id) {
        (EventData::TopicCreated(d), _) => Some(d.topic.title.clone()),
        (_, Some(t)) => store.topic(t).ok().flatten().map(|r| r.topic.title),
        _ => None,
    };
    let actor_name = store.member(&e.actor_id).ok().flatten().map(|m| m.name);
    Some(ClientUpdate::Notify(Box::new(
        crate::client::Notification {
            reason,
            event,
            group_id: e.group_id.clone(),
            topic_id,
            message_id,
            title: group.map(|g| g.group.name).unwrap_or_default(),
            subtitle,
            body,
            actor_id: e.actor_id.clone(),
            actor_name,
        },
    )))
}

fn code_str(c: ErrorCode) -> &'static str {
    match c {
        ErrorCode::Unauthorized => "unauthorized",
        ErrorCode::Forbidden => "forbidden",
        ErrorCode::NotFound => "not_found",
        ErrorCode::Invalid => "invalid",
        ErrorCode::Conflict => "conflict",
        ErrorCode::RateLimited => "rate_limited",
        ErrorCode::TooLarge => "too_large",
        ErrorCode::UnsupportedProtocol => "unsupported_protocol",
        ErrorCode::Internal => "internal",
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ws_url_from_base() {
        assert_eq!(super::ws_url("https://a.example/"), "wss://a.example/v1/ws");
        assert_eq!(
            super::ws_url("http://127.0.0.1:8787"),
            "ws://127.0.0.1:8787/v1/ws"
        );
    }
}
