//! Connection and sync engine. Follows the Rules section of docs/protocol.md:
//! strict seq order per group with gap fetches, catch-up after `welcome`, outbox resend
//! with the same `clientMsgId`, keepalive, and reconnect with backoff.

use crate::backoff::Backoff;
use crate::client::{ClientConfig, ClientUpdate, ConnectionState, SendFailure, Shared};
use crate::proto::{
    ClientFrame, ClientInfo, ErrorBody, ErrorCode, Event, Message, PROTOCOL_VERSION, SendResult, ServerFrame,
    TransientEvent,
};
use crate::store::{ApplyOutcome, OutboxOp, OutboxState};
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

#[derive(Debug)]
pub(crate) enum Command {
    /// The outbox has new or retried rows.
    Flush,
    Typing { group_id: String, topic_id: String },
    Read { group_id: String, seq: i64 },
    LoadOlder { group_id: String, topic_id: String },
    Delta { group_id: String, message_id: String, append: String },
    ReconnectNow,
    Stop,
}

enum ConnectError {
    Unauthorized,
    Other(String),
}

enum SessionEnd {
    Stop,
    /// Reconnect; `immediate` skips the backoff wait.
    Reconnect { reason: String, immediate: bool },
    Unauthorized,
    Unsupported,
}

pub(crate) async fn run(shared: Arc<Shared>, config: ClientConfig, mut cmds: UnboundedReceiver<Command>) {
    let mut backoff = Backoff::new(config.backoff_min, config.backoff_max);
    let mut skip_wait = true;
    loop {
        if !skip_wait {
            let delay = backoff.next_delay();
            shared.set_state(ConnectionState::Disconnected { retry_in: delay });
            info!(delay_ms = delay.as_millis() as u64, attempt = backoff.attempt(), "reconnect wait");
            match wait(&mut cmds, Some(delay)).await {
                WaitEnd::Stop => break,
                WaitEnd::Now => backoff.reset(),
                WaitEnd::Elapsed => {}
            }
        }
        skip_wait = false;
        shared.set_state(ConnectionState::Connecting);
        let token = shared.token.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let ws = match tokio::time::timeout(config.connect_timeout, connect(&config.base_url, &token)).await {
            Ok(Ok(ws)) => ws,
            Ok(Err(ConnectError::Unauthorized)) => {
                warn!("token refused");
                shared.set_state(ConnectionState::Unauthorized);
                match wait(&mut cmds, None).await {
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
        let mut session = Session::new(shared.clone(), &config);
        let end = session.run(ws, &mut cmds, &mut backoff).await;
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
                match wait(&mut cmds, None).await {
                    WaitEnd::Stop => break,
                    _ => {
                        skip_wait = true;
                        backoff.reset();
                    }
                }
            }
            SessionEnd::Unsupported => {
                shared.set_state(ConnectionState::Unsupported);
                if let WaitEnd::Stop = wait(&mut cmds, None).await {
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
async fn wait(cmds: &mut UnboundedReceiver<Command>, delay: Option<Duration>) -> WaitEnd {
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

async fn connect(base: &str, token: &str) -> Result<Ws, ConnectError> {
    let mut req = ws_url(base)
        .into_client_request()
        .map_err(|e| ConnectError::Other(e.to_string()))?;
    let mut auth = HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| ConnectError::Unauthorized)?;
    auth.set_sensitive(true);
    req.headers_mut().insert("Authorization", auth);
    let mut cfg = WebSocketConfig::default();
    cfg.max_message_size = Some(2 << 20);
    cfg.max_frame_size = Some(2 << 20);
    match tokio_tungstenite::connect_async_with_config(req, Some(cfg), true).await {
        Ok((ws, _)) => Ok(ws),
        Err(tungstenite::Error::Http(resp)) if resp.status().as_u16() == 401 || resp.status().as_u16() == 403 => {
            Err(ConnectError::Unauthorized)
        }
        Err(tungstenite::Error::Http(resp)) => Err(ConnectError::Other(format!("http {}", resp.status()))),
        Err(e) => Err(ConnectError::Other(e.to_string())),
    }
}

enum Pending {
    Outbox { id: String },
    Fetch { group_id: String },
    History { group_id: String, topic_id: String },
}

#[derive(Default)]
struct GroupSync {
    fetch_rid: Option<String>,
    buffer: BTreeMap<i64, Event>,
}

struct Session {
    shared: Arc<Shared>,
    client: ClientInfo,
    ping_interval: Duration,
    pong_timeout: Duration,
    request_timeout: Duration,
    fetch_limit: u32,
    history_limit: u32,
    me: String,
    welcomed: bool,
    next_rid: u64,
    pending: HashMap<String, (Pending, Instant)>,
    inflight_outbox: HashSet<String>,
    retry_at: HashMap<String, Instant>,
    groups: HashMap<String, GroupSync>,
    history_inflight: HashSet<String>,
    typing_sent: HashMap<(String, String), Instant>,
    /// Groups that were behind after `welcome` and are not caught up yet.
    catching_up: BTreeSet<String>,
    next_ping: Instant,
    pong_deadline: Option<Instant>,
    welcome_deadline: Instant,
    out: Vec<ClientFrame>,
}

impl Session {
    fn new(shared: Arc<Shared>, c: &ClientConfig) -> Session {
        let now = Instant::now();
        Session {
            shared,
            client: ClientInfo { name: c.client_name.clone(), version: c.client_version.clone() },
            ping_interval: c.ping_interval,
            pong_timeout: c.pong_timeout,
            request_timeout: c.request_timeout,
            fetch_limit: c.fetch_limit.clamp(1, crate::proto::MAX_PAGE_LIMIT),
            history_limit: c.history_limit.clamp(1, crate::proto::MAX_PAGE_LIMIT),
            me: String::new(),
            welcomed: false,
            next_rid: 1,
            pending: HashMap::new(),
            inflight_outbox: HashSet::new(),
            retry_at: HashMap::new(),
            groups: HashMap::new(),
            history_inflight: HashSet::new(),
            typing_sent: HashMap::new(),
            catching_up: BTreeSet::new(),
            next_ping: now + c.ping_interval,
            pong_deadline: None,
            welcome_deadline: now + c.request_timeout,
            out: Vec::new(),
        }
    }

    fn rid(&mut self) -> String {
        let r = format!("r{}", self.next_rid);
        self.next_rid += 1;
        r
    }

    async fn run(&mut self, ws: Ws, cmds: &mut UnboundedReceiver<Command>, backoff: &mut Backoff) -> SessionEnd {
        let (mut sink, mut stream) = ws.split();
        let cursors = match self.shared.store.cursors() {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "read cursors");
                BTreeMap::new()
            }
        };
        self.out.push(ClientFrame::Hello {
            protocol: PROTOCOL_VERSION,
            client: self.client.clone(),
            cursors: Some(cursors),
        });
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
            if write_err.is_none() {
                if let Err(e) = sink.flush().await {
                    write_err = Some(e.to_string());
                }
            }
            if let Some(e) = write_err {
                break SessionEnd::Reconnect { reason: format!("write: {e}"), immediate: false };
            }

            tokio::select! {
                msg = stream.next() => {
                    let msg = match msg {
                        None => break SessionEnd::Reconnect { reason: "closed by server".into(), immediate: false },
                        Some(Err(e)) => break SessionEnd::Reconnect { reason: format!("read: {e}"), immediate: false },
                        Some(Ok(m)) => m,
                    };
                    let text = match msg {
                        WsMessage::Text(t) => t,
                        WsMessage::Close(_) => break SessionEnd::Reconnect { reason: "close frame".into(), immediate: false },
                        _ => continue,
                    };
                    let frame: ServerFrame = match serde_json::from_str(text.as_str()) {
                        Ok(f) => f,
                        Err(e) => {
                            warn!(error = %e, "unparsable server frame");
                            continue;
                        }
                    };
                    if let Some(end) = self.on_frame(frame, backoff) {
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
                return Some(SessionEnd::Reconnect { reason: "no welcome".into(), immediate: false });
            }
            return None;
        }
        if let Some(d) = self.pong_deadline {
            if now >= d {
                return Some(SessionEnd::Reconnect { reason: "pong timeout".into(), immediate: false });
            }
        }
        if now >= self.next_ping {
            self.out.push(ClientFrame::Ping);
            self.next_ping = now + self.ping_interval;
            if self.pong_deadline.is_none() {
                self.pong_deadline = Some(now + self.pong_timeout);
            }
        }
        if self.pending.values().any(|(_, deadline)| now >= *deadline) {
            return Some(SessionEnd::Reconnect { reason: "request timeout".into(), immediate: false });
        }
        let changed = self.shared.transient().expire_typing(now);
        for (group_id, topic_id) in changed {
            self.shared.emit(ClientUpdate::Typing { group_id, topic_id });
        }
        self.typing_sent.retain(|_, t| now.duration_since(*t) < TYPING_MIN_GAP);
        self.flush_outbox();
        None
    }

    fn on_command(&mut self, c: Command) {
        if !self.welcomed {
            // Outbox rows and read cursors are in the store and go out after welcome.
            return;
        }
        match c {
            Command::Flush => self.flush_outbox(),
            Command::Typing { group_id, topic_id } => {
                let key = (group_id.clone(), topic_id.clone());
                let now = Instant::now();
                if self.typing_sent.get(&key).is_some_and(|t| now.duration_since(*t) < TYPING_MIN_GAP) {
                    return;
                }
                self.typing_sent.insert(key, now);
                self.out.push(ClientFrame::Typing { group_id, topic_id });
            }
            Command::Read { group_id, seq } => self.send_read(group_id, seq),
            Command::LoadOlder { group_id, topic_id } => {
                if self.history_inflight.contains(&topic_id) {
                    return;
                }
                let before = self.shared.store.oldest_cached_seq(&topic_id).ok().flatten();
                let rid = self.rid();
                self.history_inflight.insert(topic_id.clone());
                self.out.push(ClientFrame::History {
                    rid: rid.clone(),
                    group_id: group_id.clone(),
                    topic_id: topic_id.clone(),
                    before_seq: before,
                    limit: Some(self.history_limit),
                });
                self.track(rid, Pending::History { group_id, topic_id });
            }
            Command::Delta { group_id, message_id, append } => {
                self.out.push(ClientFrame::Delta { group_id, message_id, append });
            }
            Command::ReconnectNow | Command::Stop => {}
        }
    }

    fn send_read(&mut self, group_id: String, seq: i64) {
        self.out.push(ClientFrame::Read { group_id: group_id.clone(), seq });
        if let Err(e) = self.shared.store.mark_read_synced(&group_id, seq) {
            warn!(error = %e, "mark read synced");
        }
    }

    fn track(&mut self, rid: String, p: Pending) {
        self.pending.insert(rid, (p, Instant::now() + self.request_timeout));
    }

    /// Sends every due outbox row that is not in flight, oldest first.
    fn flush_outbox(&mut self) {
        if !self.welcomed {
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
        for item in items {
            if item.state != OutboxState::Pending || self.inflight_outbox.contains(&item.id) {
                continue;
            }
            if self.retry_at.get(&item.id).is_some_and(|t| *t > now) {
                continue;
            }
            let rid = self.rid();
            let frame = match item.op.clone() {
                OutboxOp::Send { group_id, topic_id, title, body, mentions, streaming } => ClientFrame::Send {
                    rid: rid.clone(),
                    group_id,
                    topic_id,
                    title,
                    client_msg_id: item.id.clone(),
                    body,
                    mentions: (!mentions.is_empty()).then_some(mentions),
                    streaming: streaming.then_some(true),
                },
                OutboxOp::Edit { group_id, message_id, body, streaming, .. } => ClientFrame::Edit {
                    rid: rid.clone(),
                    group_id,
                    message_id,
                    body,
                    streaming: Some(streaming),
                },
                OutboxOp::Delete { group_id, message_id, .. } => {
                    ClientFrame::Delete { rid: rid.clone(), group_id, message_id }
                }
                OutboxOp::TopicUpdate { group_id, topic_id, title, status } => {
                    ClientFrame::TopicUpdate { rid: rid.clone(), group_id, topic_id, title, status }
                }
            };
            if let Err(e) = self.shared.store.outbox_attempt(&item.id) {
                warn!(error = %e, "outbox attempt");
            }
            debug!(id = %item.id, kind = item.op.kind(), attempts = item.attempts + 1, "outbox send");
            self.inflight_outbox.insert(item.id.clone());
            self.out.push(frame);
            self.track(rid, Pending::Outbox { id: item.id });
        }
    }

    fn on_frame(&mut self, frame: ServerFrame, backoff: &mut Backoff) -> Option<SessionEnd> {
        match frame {
            ServerFrame::Welcome { protocol, me, members, groups, .. } => {
                if protocol != PROTOCOL_VERSION {
                    return Some(SessionEnd::Unsupported);
                }
                let out = match self.shared.store.apply_welcome(&me, &members, &groups) {
                    Ok(o) => o,
                    Err(e) => {
                        warn!(error = %e, "apply welcome");
                        return Some(SessionEnd::Reconnect { reason: "store".into(), immediate: false });
                    }
                };
                info!(groups = groups.len(), members = members.len(), "welcome");
                self.me = me.id.clone();
                self.welcomed = true;
                backoff.reset();
                self.shared.set_state(ConnectionState::Connected);
                self.shared.emit(ClientUpdate::DirectoryChanged);
                for g in &out.groups_changed {
                    self.shared.emit(ClientUpdate::GroupChanged { group_id: g.clone() });
                }
                for g in &groups {
                    let last = self.shared.store.last_applied(&g.group.id).unwrap_or(0);
                    if g.last_seq > last {
                        self.catching_up.insert(g.group.id.clone());
                        self.start_fetch(&g.group.id);
                    }
                }
                if self.catching_up.is_empty() {
                    self.shared.emit(ClientUpdate::CaughtUp);
                }
                if let Ok(reads) = self.shared.store.unsynced_group_reads() {
                    for (g, seq) in reads {
                        self.send_read(g, seq);
                    }
                }
                self.flush_outbox();
            }
            ServerFrame::Event { event } => {
                if !self.welcomed {
                    return None;
                }
                let gid = event.group_id.clone();
                self.groups.entry(gid.clone()).or_default().buffer.insert(event.seq, event);
                self.drain(&gid);
            }
            ServerFrame::Page { rid, group_id, events, has_more, last_seq } => {
                let Some((Pending::Fetch { .. }, _)) = self.pending.remove(&rid) else {
                    return None;
                };
                let empty = events.is_empty();
                {
                    let gs = self.groups.entry(group_id.clone()).or_default();
                    if gs.fetch_rid.as_deref() == Some(rid.as_str()) {
                        gs.fetch_rid = None;
                    }
                    for e in events.into_iter().filter(|e| e.group_id == group_id) {
                        gs.buffer.insert(e.seq, e);
                    }
                }
                let last = self.drain(&group_id);
                let gap = self.groups.get(&group_id).is_some_and(|g| !g.buffer.is_empty());
                if (has_more || gap || last < last_seq) && !empty {
                    self.start_fetch(&group_id);
                } else {
                    if last < last_seq || gap {
                        warn!(group = %group_id, last, last_seq, "fetch ended behind the server");
                    }
                    if self.catching_up.remove(&group_id) && self.catching_up.is_empty() {
                        self.shared.emit(ClientUpdate::CaughtUp);
                    }
                }
            }
            ServerFrame::History { rid, group_id, topic_id, messages, has_more } => {
                if self.pending.remove(&rid).is_none() {
                    return None;
                }
                self.history_inflight.remove(&topic_id);
                let msgs: Vec<Message> = messages.into_iter().filter(|m| m.topic_id == topic_id).collect();
                if let Err(e) = self.shared.store.insert_history(&topic_id, &msgs, has_more) {
                    warn!(error = %e, "store history");
                }
                self.shared.emit(ClientUpdate::MessagesChanged { group_id: group_id.clone(), topic_id: topic_id.clone() });
                self.shared.emit(ClientUpdate::HistoryLoaded { group_id, topic_id, has_more });
            }
            ServerFrame::Ack { rid, ok, result, error } => {
                let Some((p, _)) = self.pending.remove(&rid) else {
                    return None;
                };
                match p {
                    Pending::Outbox { id } => return self.on_outbox_ack(id, ok, result, error),
                    Pending::Fetch { group_id } => {
                        // A fetch answered with an error ack instead of a page.
                        warn!(group = %group_id, code = ?error.as_ref().map(|e| e.code), "fetch refused");
                        if let Some(g) = self.groups.get_mut(&group_id) {
                            g.fetch_rid = None;
                        }
                        self.catching_up.remove(&group_id);
                    }
                    Pending::History { group_id, topic_id } => {
                        self.history_inflight.remove(&topic_id);
                        warn!(group = %group_id, code = ?error.as_ref().map(|e| e.code), "history refused");
                    }
                }
            }
            ServerFrame::Transient { event } => self.on_transient(event),
            ServerFrame::Directory { members, groups, removed_group_ids } => {
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
                    self.groups.remove(g);
                }
                self.shared.emit(ClientUpdate::DirectoryChanged);
                for g in &out.groups_changed {
                    self.shared.emit(ClientUpdate::GroupChanged { group_id: g.clone() });
                }
                for g in groups.unwrap_or_default() {
                    let last = self.shared.store.last_applied(&g.group.id).unwrap_or(0);
                    if g.last_seq > last {
                        self.start_fetch(&g.group.id);
                    }
                }
            }
            ServerFrame::Error { error } => {
                warn!(code = ?error.code, "connection error frame");
                return Some(match error.code {
                    ErrorCode::Unauthorized => SessionEnd::Unauthorized,
                    ErrorCode::UnsupportedProtocol => SessionEnd::Unsupported,
                    _ => SessionEnd::Reconnect { reason: format!("error frame {:?}", error.code), immediate: false },
                });
            }
            ServerFrame::Pong => self.pong_deadline = None,
        }
        None
    }

    fn on_outbox_ack(
        &mut self,
        id: String,
        ok: bool,
        result: Option<serde_json::Value>,
        error: Option<ErrorBody>,
    ) -> Option<SessionEnd> {
        self.inflight_outbox.remove(&id);
        let item = match self.shared.store.outbox_get(&id) {
            Ok(Some(i)) => i,
            // Already removed: its event arrived first, or the UI discarded it.
            _ => {
                if ok {
                    if let Some(r) = result.and_then(|v| serde_json::from_value::<SendResult>(v).ok()) {
                        let _ = self.shared.store.ack_send(&id, &r);
                    }
                }
                return None;
            }
        };
        let group_id = item.op.group_id().to_string();
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
                            let topic_id = r.message.topic_id.clone();
                            if r.topic.is_some() || item.op.topic_id().is_none() {
                                self.shared.emit(ClientUpdate::TopicsChanged { group_id: group_id.clone() });
                            }
                            self.shared.emit(ClientUpdate::MessagesChanged {
                                group_id: group_id.clone(),
                                topic_id: topic_id.clone(),
                            });
                            self.shared.emit(ClientUpdate::Sent {
                                client_msg_id: id,
                                message_id: r.message.id,
                                topic_id,
                                duplicate: r.duplicate.unwrap_or(false),
                            });
                        }
                        None => {
                            warn!(id = %id, "send ack without a valid result");
                            let _ = self.shared.store.outbox_remove(&id);
                        }
                    }
                }
                _ => {
                    let _ = self.shared.store.outbox_remove(&id);
                }
            }
            return None;
        }
        let err = error.unwrap_or(ErrorBody { code: ErrorCode::Internal, message: String::new() });
        match err.code {
            ErrorCode::RateLimited | ErrorCode::Internal => {
                let attempts = item.attempts.clamp(0, 6) as u32;
                let delay = Duration::from_secs(1u64 << attempts).min(Duration::from_secs(60));
                self.retry_at.insert(id.clone(), Instant::now() + delay);
                let _ = self.shared.store.outbox_set_state(&id, OutboxState::Pending, Some(code_str(err.code)));
                None
            }
            ErrorCode::Unauthorized => Some(SessionEnd::Unauthorized),
            ErrorCode::NotFound if matches!(item.op, OutboxOp::Delete { .. }) => {
                // Already gone.
                let _ = self.shared.store.outbox_remove(&id);
                None
            }
            code => {
                warn!(id = %id, kind = item.op.kind(), ?code, "write refused");
                let _ = self.shared.store.outbox_set_state(&id, OutboxState::Failed, Some(code_str(code)));
                let topic_id = item.op.topic_id().map(str::to_string);
                match &topic_id {
                    Some(t) => self.shared.emit(ClientUpdate::MessagesChanged { group_id: group_id.clone(), topic_id: t.clone() }),
                    None => self.shared.emit(ClientUpdate::TopicsChanged { group_id: group_id.clone() }),
                }
                self.shared.emit(ClientUpdate::SendFailed(SendFailure {
                    id,
                    kind: item.op.kind(),
                    group_id,
                    topic_id,
                    code,
                    message: err.message,
                }));
                None
            }
        }
    }

    fn on_transient(&mut self, ev: TransientEvent) {
        match ev {
            TransientEvent::Typing { group_id, topic_id, member_id, .. } => {
                if member_id == self.me {
                    return;
                }
                self.shared.transient().on_typing(&group_id, &topic_id, &member_id, Instant::now());
                self.shared.emit(ClientUpdate::Typing { group_id, topic_id });
            }
            TransientEvent::Presence { member_id, online, .. } => {
                if self.shared.transient().set_presence(&member_id, online) {
                    self.shared.emit(ClientUpdate::Presence { member_id, online });
                }
            }
            TransientEvent::MessageDelta { group_id, topic_id, message_id, append } => {
                self.shared.transient().append_delta(&message_id, &append);
                self.shared.emit(ClientUpdate::MessagesChanged { group_id, topic_id });
            }
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
        self.out.push(ClientFrame::Fetch {
            rid: rid.clone(),
            group_id: group_id.to_string(),
            after_seq: after,
            limit: Some(self.fetch_limit),
        });
        self.track(rid, Pending::Fetch { group_id: group_id.to_string() });
    }

    /// Applies the contiguous buffered events of a group; fetches when a gap remains.
    /// Returns `lastApplied`.
    fn drain(&mut self, group_id: &str) -> i64 {
        let events: Vec<Event> = match self.groups.get(group_id) {
            Some(g) if !g.buffer.is_empty() => g.buffer.values().cloned().collect(),
            _ => return self.shared.store.last_applied(group_id).unwrap_or(0),
        };
        let out = match self.shared.store.apply_events(group_id, &events) {
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
            if self.shared.transient().clear_typing(group_id, topic_id, author) {
                self.shared.emit(ClientUpdate::Typing { group_id: group_id.into(), topic_id: topic_id.clone() });
            }
        }
        for id in &out.my_client_msg_ids {
            self.retry_at.remove(id);
        }
        if out.group_changed {
            self.shared.emit(ClientUpdate::GroupChanged { group_id: group_id.into() });
        }
        if out.topic_list_changed {
            self.shared.emit(ClientUpdate::TopicsChanged { group_id: group_id.into() });
        }
        for t in &out.message_topics {
            self.shared.emit(ClientUpdate::MessagesChanged { group_id: group_id.into(), topic_id: t.clone() });
        }
    }
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
        assert_eq!(super::ws_url("http://127.0.0.1:8787"), "ws://127.0.0.1:8787/v1/ws");
    }
}
