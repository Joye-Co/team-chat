#![allow(dead_code, clippy::too_many_arguments, clippy::result_large_err)]
//! In-process mock of the server WebSocket: the subset of docs/protocol.md that the
//! sync engine uses, plus knobs to drop acks, hide live events, refuse connections
//! and stop answering pings.

use chat_core::proto::*;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio_tungstenite::tungstenite::Message as Ws;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};

pub const TOKEN: &str = "tc_mock_token_for_tests";

#[derive(Default)]
pub struct GroupState {
    pub group: Option<Group>,
    pub log: Vec<Event>,
    pub read_seq: u64,
    pub topics: HashMap<String, Topic>,
    pub messages: BTreeMap<String, Message>,
    pub dedupe: HashMap<(String, String), String>,
    pub topic_reads: HashMap<String, u64>,
}

const G: &str = "G";

enum Ctrl {
    Frame(String),
    Close,
}

#[derive(Default)]
pub struct State {
    pub me: Option<Member>,
    pub members: Vec<Member>,
    pub groups: BTreeMap<String, GroupState>,
    conns: Vec<(usize, UnboundedSender<Ctrl>)>,
    next_conn: usize,
    next_id: u64,
    // Knobs.
    /// Process this many sends without acking them, then close the connection.
    pub drop_acks: usize,
    /// Seqs that are stored but not broadcast live (the client sees a gap).
    pub hide_live: HashSet<u64>,
    pub ignore_pings: bool,
    /// Accept the TCP connection and fail the upgrade (server unreachable).
    pub refuse: bool,
    /// Ack every `topic.update` with this error.
    pub topic_update_error: Option<ErrorCode>,
    /// Ack every `search` with this error.
    pub search_error: Option<ErrorCode>,
    /// Ack every `send` with this body with this error, without storing it.
    pub fail_send_body: Option<(String, ErrorCode)>,
    /// Answer every `typing` with a connection-level error frame (a frame without rid).
    pub typing_error: Option<ErrorCode>,
    // Observations.
    pub client_frames: Vec<Value>,
    pub accepted: usize,
    pub hellos: Vec<Value>,
}

impl State {
    fn id(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{prefix}{:020}", self.next_id)
    }

    fn me_id(&self) -> String {
        self.me.as_ref().map(|m| m.id.clone()).unwrap_or_default()
    }

    fn summary(&self, gid: &str) -> GroupSummary {
        let g = &self.groups[gid];
        let me = self.me_id();
        let unread = g
            .log
            .iter()
            .filter(|e| e.seq > g.read_seq)
            .filter(
                |e| matches!(&e.data, EventData::MessageCreated(d) if d.message.author_id != me),
            )
            .count() as u64;
        GroupSummary {
            group: g.group.clone().unwrap(),
            last_seq: g.log.len() as u64,
            read_seq: g.read_seq,
            unread,
            mentions: 0,
            ..Default::default()
        }
    }

    /// Appends an event to a group log and broadcasts it unless hidden.
    pub fn append(&mut self, gid: &str, actor: &str, data: EventData) -> u64 {
        let g = self.groups.get_mut(gid).unwrap();
        let seq = g.log.len() as u64 + 1;
        let ev = Event {
            seq,
            group_id: gid.into(),
            at: 1_000 + seq,
            actor_id: actor.into(),
            data,
        };
        g.log.push(ev.clone());
        if !self.hide_live.contains(&seq) {
            self.broadcast(&json!({"t": "event", "event": ev}), None);
        }
        seq
    }

    pub fn broadcast(&mut self, v: &Value, except: Option<usize>) {
        let text = v.to_string();
        self.conns
            .retain(|(id, tx)| Some(*id) == except || tx.send(Ctrl::Frame(text.clone())).is_ok());
    }

    /// Creates a message (and a topic when `topic_id` is None). Returns (message, new topic).
    pub fn post(
        &mut self,
        gid: &str,
        author: &str,
        topic_id: Option<&str>,
        title: Option<&str>,
        client_msg_id: &str,
        body: &str,
        streaming: bool,
    ) -> (Message, Option<Topic>) {
        let msg_id = self.id("M");
        let mut new_topic = None;
        let topic_id = match topic_id {
            Some(t) => t.to_string(),
            None => {
                let tid = self.id("T");
                let seq = self.groups[gid].log.len() as u64 + 1;
                let topic = Topic {
                    id: tid.clone(),
                    group_id: gid.into(),
                    title: title
                        .map(str::to_string)
                        .unwrap_or_else(|| default_title(body)),
                    status: TopicStatus::Open,
                    created_by: author.into(),
                    created_at: 1_000 + seq,
                    root_message_id: msg_id.clone(),
                    last_seq: seq,
                    message_count: 1,
                    owner_id: Some(Some(author.into())),
                    ..Default::default()
                };
                self.groups
                    .get_mut(gid)
                    .unwrap()
                    .topics
                    .insert(tid.clone(), topic.clone());
                self.append(
                    gid,
                    author,
                    EventData::TopicCreated(TopicCreatedData {
                        topic: topic.clone(),
                    }),
                );
                new_topic = Some(topic);
                tid
            }
        };
        let seq = self.groups[gid].log.len() as u64 + 1;
        let m = Message {
            id: msg_id.clone(),
            client_msg_id: client_msg_id.into(),
            group_id: gid.into(),
            topic_id,
            author_id: author.into(),
            body: body.into(),
            mentions: vec![],
            created_seq: seq,
            created_at: 1_000 + seq,
            streaming: streaming.then_some(true),
            ..Default::default()
        };
        let g = self.groups.get_mut(gid).unwrap();
        g.messages.insert(msg_id.clone(), m.clone());
        g.dedupe
            .insert((author.into(), client_msg_id.into()), msg_id);
        self.append(
            gid,
            author,
            EventData::MessageCreated(Box::new(MessageCreatedData { message: m.clone() })),
        );
        (m, new_topic)
    }

    pub fn transient(&mut self, ev: Value) {
        self.broadcast(&json!({"t": "transient", "event": ev}), None);
    }

    pub fn close_all(&mut self) {
        for (_, tx) in self.conns.drain(..) {
            let _ = tx.send(Ctrl::Close);
        }
    }

    pub fn frames_of(&self, t: &str) -> Vec<Value> {
        self.client_frames
            .iter()
            .filter(|f| f["t"] == t)
            .cloned()
            .collect()
    }

    pub fn message_count(&self, gid: &str) -> usize {
        self.groups[gid].messages.len()
    }
}

pub struct MockServer {
    pub addr: SocketAddr,
    pub state: Arc<Mutex<State>>,
}

impl MockServer {
    pub async fn start(me: Member, members: Vec<Member>) -> MockServer {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let state = Arc::new(Mutex::new(State {
            me: Some(me),
            members,
            ..Default::default()
        }));
        let st = state.clone();
        tokio::spawn(async move {
            loop {
                let Ok((tcp, _)) = listener.accept().await else {
                    break;
                };
                let st = st.clone();
                tokio::spawn(async move { serve(tcp, st).await });
            }
        });
        MockServer { addr, state }
    }

    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn with<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        f(&mut self.state.lock().unwrap())
    }

    pub fn add_group(&self, g: Group) {
        self.with(|s| {
            s.groups.insert(
                g.id.clone(),
                GroupState {
                    group: Some(g),
                    ..Default::default()
                },
            );
        });
    }
}

async fn serve(tcp: tokio::net::TcpStream, st: Arc<Mutex<State>>) {
    let refuse = st.lock().unwrap().refuse;
    let check = move |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
        let ok = req
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v == format!("Bearer {TOKEN}"));
        if refuse {
            let mut e = ErrorResponse::new(None);
            *e.status_mut() = tokio_tungstenite::tungstenite::http::StatusCode::SERVICE_UNAVAILABLE;
            return Err(e);
        }
        if !ok {
            let mut e = ErrorResponse::new(None);
            *e.status_mut() = tokio_tungstenite::tungstenite::http::StatusCode::UNAUTHORIZED;
            return Err(e);
        }
        Ok(resp)
    };
    let Ok(ws) = tokio_tungstenite::accept_hdr_async(tcp, check).await else {
        return;
    };
    let (mut sink, mut stream) = ws.split();
    let (tx, mut rx) = unbounded_channel::<Ctrl>();
    let conn_id = {
        let mut s = st.lock().unwrap();
        s.accepted += 1;
        s.next_conn += 1;
        s.next_conn
    };
    tokio::spawn(async move {
        while let Some(c) = rx.recv().await {
            match c {
                Ctrl::Frame(t) => {
                    if sink.send(Ws::Text(t.into())).await.is_err() {
                        break;
                    }
                }
                Ctrl::Close => {
                    let _ = sink.close().await;
                    break;
                }
            }
        }
    });
    while let Some(Ok(msg)) = stream.next().await {
        let Ws::Text(text) = msg else { continue };
        let v: Value = serde_json::from_str(text.as_str()).unwrap();
        let mut s = st.lock().unwrap();
        s.client_frames.push(v.clone());
        if !handle(&mut s, conn_id, &tx, v) {
            let _ = tx.send(Ctrl::Close);
            break;
        }
    }
    st.lock().unwrap().conns.retain(|(id, _)| *id != conn_id);
}

fn send(tx: &UnboundedSender<Ctrl>, v: Value) {
    let _ = tx.send(Ctrl::Frame(v.to_string()));
}

/// Handles one client frame. Returns false to close the connection.
fn handle(s: &mut State, conn: usize, tx: &UnboundedSender<Ctrl>, v: Value) -> bool {
    let me = s.me_id();
    let rid = v["rid"].as_str().unwrap_or("").to_string();
    let gid = v["groupId"].as_str().unwrap_or("").to_string();
    match v["t"].as_str().unwrap() {
        "hello" => {
            s.hellos.push(v.clone());
            let groups: Vec<GroupSummary> = s
                .groups
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .iter()
                .map(|g| s.summary(g))
                .collect();
            send(
                tx,
                json!({"t": "welcome", "protocol": 1, "me": s.me, "members": s.members, "groups": groups, "serverTime": 1}),
            );
            s.conns.push((conn, tx.clone()));
        }
        "ping" => {
            if !s.ignore_pings {
                send(tx, json!({"t": "pong"}));
            }
        }
        "send"
            if s.fail_send_body
                .as_ref()
                .is_some_and(|(b, _)| v["body"].as_str() == Some(b.as_str())) =>
        {
            let code = s.fail_send_body.as_ref().unwrap().1;
            send(
                tx,
                json!({"t": "ack", "rid": rid, "ok": false, "error": {"code": code, "message": "boom"}}),
            );
        }
        "send" => {
            let cmid = v["clientMsgId"].as_str().unwrap().to_string();
            let key = (me.clone(), cmid.clone());
            if let Some(mid) = s.groups[&gid].dedupe.get(&key).cloned() {
                let m = s.groups[&gid].messages[&mid].clone();
                send(
                    tx,
                    json!({"t": "ack", "rid": rid, "ok": true, "result": {"message": m, "duplicate": true}}),
                );
                return true;
            }
            let (m, topic) = s.post(
                &gid,
                &me,
                v["topicId"].as_str(),
                v["title"].as_str(),
                &cmid,
                v["body"].as_str().unwrap(),
                v["streaming"].as_bool().unwrap_or(false),
            );
            if s.drop_acks > 0 {
                s.drop_acks -= 1;
                return false;
            }
            let mut result = json!({"message": m});
            if let Some(t) = topic {
                result["topic"] = json!(t);
            }
            send(
                tx,
                json!({"t": "ack", "rid": rid, "ok": true, "result": result}),
            );
        }
        "edit" => {
            let mid = v["messageId"].as_str().unwrap().to_string();
            let body = v["body"].as_str().unwrap().to_string();
            let streaming = v["streaming"].as_bool().unwrap_or(false);
            if let Some(m) = s.groups.get_mut(&gid).unwrap().messages.get_mut(&mid) {
                m.body = body.clone();
                m.edited_at = Some(Some(5));
                m.streaming = streaming.then_some(true);
            }
            s.append(
                &gid,
                &me,
                EventData::MessageUpdated(MessageUpdatedData {
                    message_id: mid,
                    body,
                    edited_at: 5,
                    streaming,
                    ..Default::default()
                }),
            );
            send(tx, json!({"t": "ack", "rid": rid, "ok": true}));
        }
        "delete" => {
            let mid = v["messageId"].as_str().unwrap().to_string();
            if let Some(m) = s.groups.get_mut(&gid).unwrap().messages.get_mut(&mid) {
                m.body.clear();
                m.deleted_at = Some(Some(6));
            }
            s.append(
                &gid,
                &me,
                EventData::MessageDeleted(MessageDeletedData {
                    message_id: mid,
                    deleted_at: 6,
                }),
            );
            send(tx, json!({"t": "ack", "rid": rid, "ok": true}));
        }
        "topic.update" => {
            if let Some(code) = s.topic_update_error {
                send(
                    tx,
                    json!({"t": "ack", "rid": rid, "ok": false, "error": {"code": code, "message": "no"}}),
                );
                return true;
            }
            let data = TopicUpdatedData {
                topic_id: v["topicId"].as_str().unwrap().into(),
                title: v["title"].as_str().map(str::to_string),
                status: serde_json::from_value(v["status"].clone()).ok(),
                waiting_on: serde_json::from_value(v["waitingOn"].clone()).ok(),
                ..Default::default()
            };
            s.append(&gid, &me, EventData::TopicUpdated(data));
            send(tx, json!({"t": "ack", "rid": rid, "ok": true}));
        }
        "fetch" => {
            let after = v["afterSeq"].as_u64().unwrap();
            let limit = v["limit"].as_u64().unwrap_or(100) as usize;
            let g = &s.groups[&gid];
            let events: Vec<&Event> = g.log.iter().filter(|e| e.seq > after).take(limit).collect();
            let has_more = events.last().is_some_and(|e| e.seq < g.log.len() as u64);
            send(
                tx,
                json!({"t": "page", "rid": rid, "groupId": gid, "events": events, "hasMore": has_more, "lastSeq": g.log.len()}),
            );
        }
        "history" => {
            let tid = v["topicId"].as_str().unwrap();
            let before = v["beforeSeq"].as_u64().unwrap_or(u64::MAX);
            let limit = v["limit"].as_u64().unwrap_or(100) as usize;
            let mut msgs: Vec<&Message> = s.groups[&gid]
                .messages
                .values()
                .filter(|m| m.topic_id == tid && m.created_seq < before)
                .collect();
            msgs.sort_by_key(|m| std::cmp::Reverse(m.created_seq));
            let has_more = msgs.len() > limit;
            msgs.truncate(limit);
            send(
                tx,
                json!({"t": "history", "rid": rid, "groupId": gid, "topicId": tid, "messages": msgs, "hasMore": has_more}),
            );
        }
        "read" => {
            let seq = v["seq"].as_u64().unwrap();
            let g = s.groups.get_mut(&gid).unwrap();
            match v["topicId"].as_str() {
                Some(t) => {
                    let e = g.topic_reads.entry(t.to_string()).or_default();
                    *e = (*e).max(seq);
                }
                None => g.read_seq = g.read_seq.max(seq),
            }
        }
        "search" if s.search_error.is_some() => {
            let code = s.search_error.unwrap();
            send(
                tx,
                json!({"t": "ack", "rid": rid, "ok": false, "error": {"code": code, "message": "refused"}}),
            );
        }
        "search" => {
            let hits = json!([{"groupId": G, "topicId": "T", "messageId": "M", "authorId": me, "snippet": v["query"], "createdAt": 1, "seq": 1}]);
            send(tx, json!({"t": "searchResults", "rid": rid, "hits": hits}));
        }
        "typing" if s.typing_error.is_some() => {
            let code = s.typing_error.unwrap();
            send(
                tx,
                json!({"t": "error", "error": {"code": code, "message": "no"}}),
            );
        }
        "card.action" => {
            let data: CardActionedData = serde_json::from_value(json!({
                "messageId": v["messageId"], "actionId": v["actionId"]
            }))
            .unwrap();
            s.append(&gid, &me, EventData::CardActioned(data));
            if s.drop_acks > 0 {
                s.drop_acks -= 1;
                return false;
            }
            send(tx, json!({"t": "ack", "rid": rid, "ok": true}));
        }
        "typing" => {
            let ev = json!({"t": "transient", "event": {"type": "typing", "groupId": gid, "topicId": v["topicId"], "memberId": me, "at": 1}});
            s.broadcast(&ev, Some(conn));
        }
        "delta" => {
            let ev = json!({"t": "transient", "event": {"type": "message.delta", "groupId": gid, "topicId": "", "messageId": v["messageId"], "append": v["append"]}});
            s.broadcast(&ev, None);
        }
        other => panic!("mock: unexpected frame {other}"),
    }
    true
}
