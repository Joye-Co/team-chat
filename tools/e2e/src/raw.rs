//! A raw WebSocket client that speaks JSON frames. Scenarios use it to read the server's
//! group log directly and to send frames that chat-core would never send.

use anyhow::{Result, anyhow, bail};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::{self, Message as WsMessage};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub const TIMEOUT: Duration = Duration::from_secs(15);

fn ws_url(base: &str) -> String {
    ws_url_at(base, "/v1/ws")
}

fn ws_url_at(base: &str, path: &str) -> String {
    let b = base.trim_end_matches('/');
    let b = if let Some(r) = b.strip_prefix("https://") {
        format!("wss://{r}")
    } else if let Some(r) = b.strip_prefix("http://") {
        format!("ws://{r}")
    } else {
        b.to_string()
    };
    format!("{b}{path}")
}

/// The HTTP status of a failed upgrade, or `None` when the upgrade worked.
pub async fn upgrade_status(base: &str, token: Option<&str>) -> Result<Option<u16>> {
    let mut req = ws_url(base).into_client_request()?;
    if let Some(t) = token {
        req.headers_mut().insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {t}"))?,
        );
    }
    match tokio_tungstenite::connect_async(req).await {
        Ok(_) => Ok(None),
        Err(tungstenite::Error::Http(res)) => Ok(Some(res.status().as_u16())),
        Err(e) => Err(anyhow!("connect: {e}")),
    }
}

pub struct RawWs {
    ws: Ws,
    /// Frames read while waiting for something else, oldest first.
    stash: VecDeque<Value>,
    next_rid: u64,
    pub welcome: Value,
}

impl RawWs {
    /// Connects, sends `hello` without cursors and waits for `welcome`.
    pub async fn connect(base: &str, token: &str) -> Result<RawWs> {
        let mut req = ws_url(base).into_client_request()?;
        req.headers_mut().insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {token}"))?,
        );
        let (ws, _) = tokio::time::timeout(TIMEOUT, tokio_tungstenite::connect_async(req))
            .await
            .map_err(|_| anyhow!("raw connect timed out"))?
            .map_err(|e| anyhow!("raw connect: {e}"))?;
        let mut r = RawWs {
            ws,
            stash: VecDeque::new(),
            next_rid: 1,
            welcome: Value::Null,
        };
        r.send(
            json!({ "t": "hello", "protocol": 1, "client": { "name": "e2e-raw", "version": "0" } }),
        )
        .await?;
        let w = r.read(TIMEOUT).await?;
        if w["t"] != "welcome" {
            bail!("expected welcome, got {}", w["t"]);
        }
        r.welcome = w;
        Ok(r)
    }

    /// A machine (docs/machine-protocol.md): connects to `/v1/machine/ws`, sends `hello` and
    /// waits for `welcome`. Machine frames carry no `rid`; read them with `next_matching`.
    pub async fn machine(base: &str, token: &str, hello: Value) -> Result<RawWs> {
        let mut req = ws_url_at(base, "/v1/machine/ws").into_client_request()?;
        req.headers_mut().insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {token}"))?,
        );
        let (ws, _) = tokio::time::timeout(TIMEOUT, tokio_tungstenite::connect_async(req))
            .await
            .map_err(|_| anyhow!("machine connect timed out"))?
            .map_err(|e| anyhow!("machine connect: {e}"))?;
        let mut r = RawWs {
            ws,
            stash: VecDeque::new(),
            next_rid: 1,
            welcome: Value::Null,
        };
        r.send(hello).await?;
        let w = r
            .next_matching(TIMEOUT, |f| f["t"] == "welcome" || f["t"] == "error")
            .await?;
        if w["t"] != "welcome" {
            bail!("expected welcome, got {w}");
        }
        r.welcome = w;
        Ok(r)
    }

    pub async fn send(&mut self, v: Value) -> Result<()> {
        self.ws
            .send(WsMessage::Text(v.to_string().into()))
            .await
            .map_err(|e| anyhow!("raw send: {e}"))
    }

    /// Reads the next JSON frame from the socket.
    async fn read(&mut self, timeout: Duration) -> Result<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let msg = tokio::time::timeout(left, self.ws.next())
                .await
                .map_err(|_| anyhow!("raw read timed out"))?;
            match msg {
                None => bail!("raw socket closed"),
                Some(Err(e)) => bail!("raw read: {e}"),
                Some(Ok(WsMessage::Text(t))) => return Ok(serde_json::from_str(t.as_str())?),
                Some(Ok(WsMessage::Close(c))) => bail!("raw socket closed: {c:?}"),
                Some(Ok(_)) => {}
            }
        }
    }

    pub fn rid(&mut self) -> String {
        let r = format!("e{}", self.next_rid);
        self.next_rid += 1;
        r
    }

    /// Sends a frame with a new `rid` and returns the answer that carries it. A connection
    /// `error` frame also ends the wait. Other frames are kept for `next_matching`.
    pub async fn request(&mut self, mut v: Value) -> Result<Value> {
        let rid = self.rid();
        v["rid"] = Value::String(rid.clone());
        self.send(v).await?;
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let f = self
                .read(deadline.saturating_duration_since(Instant::now()))
                .await?;
            if f["rid"] == rid.as_str() || f["t"] == "error" {
                return Ok(f);
            }
            self.stash.push_back(f);
        }
    }

    /// The first frame (kept or new) for which `pred` holds; frames before it are kept.
    pub async fn next_matching(
        &mut self,
        timeout: Duration,
        mut pred: impl FnMut(&Value) -> bool,
    ) -> Result<Value> {
        if let Some(i) = self.stash.iter().position(&mut pred) {
            return Ok(self.stash.remove(i).expect("index from position"));
        }
        let deadline = Instant::now() + timeout;
        loop {
            let f = self
                .read(deadline.saturating_duration_since(Instant::now()))
                .await?;
            if pred(&f) {
                return Ok(f);
            }
            self.stash.push_back(f);
        }
    }

    /// Every stored event of a group, in seq order, read page by page with `fetch`.
    pub async fn fetch_all(&mut self, group_id: &str) -> Result<(Vec<Value>, u64)> {
        let mut out = Vec::new();
        let mut after = 0u64;
        loop {
            let p = self
                .request(
                    json!({ "t": "fetch", "groupId": group_id, "afterSeq": after, "limit": 500 }),
                )
                .await?;
            if p["t"] != "page" {
                bail!("fetch {group_id}: {p}");
            }
            let events = p["events"].as_array().cloned().unwrap_or_default();
            for e in events {
                let seq = e["seq"]
                    .as_u64()
                    .ok_or_else(|| anyhow!("event without seq"))?;
                if seq != after + 1 {
                    bail!("fetch {group_id}: seq {seq} after {after}");
                }
                after = seq;
                out.push(e);
            }
            if p["hasMore"] != true {
                let last = p["lastSeq"].as_u64().unwrap_or(after);
                return Ok((out, last));
            }
        }
    }
}

/// `message.created` events of a log: (seq, message).
pub fn created(events: &[Value]) -> Vec<&Value> {
    events
        .iter()
        .filter(|e| e["type"] == "message.created")
        .map(|e| &e["data"]["message"])
        .collect()
}
