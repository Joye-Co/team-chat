//! A fake model for the real Claude Code CLI: an HTTP server that speaks enough of the
//! Anthropic Messages API (`POST /v1/messages`, streamed or not, and `count_tokens`) for the
//! CLI to run a turn without a login. The CLI is the real one; only the model is scripted.
//!
//! The script is the prompt. A conversation with a line `TOOL <name> <command>` in a user text
//! answers with a call of that tool (`{"command": ...}`); the request that brings the tool's
//! result back is answered with the text `saw: <the result>`; anything else with `ok`. Every
//! request body is kept for the test to read.

#![allow(dead_code)]

use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub struct Model {
    /// `http://127.0.0.1:<port>`, for `ANTHROPIC_BASE_URL`.
    pub url: String,
    requests: Arc<Mutex<Vec<Value>>>,
}

impl Model {
    pub async fn start() -> Model {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(vec![]));
        let seen = requests.clone();
        tokio::spawn(async move {
            while let Ok((s, _)) = listener.accept().await {
                tokio::spawn(serve(s, seen.clone()));
            }
        });
        Model { url, requests }
    }

    /// The environment that points the CLI at this model, with its other traffic off.
    pub fn env(&self) -> Vec<(&'static str, String)> {
        vec![
            ("ANTHROPIC_BASE_URL", self.url.clone()),
            ("ANTHROPIC_API_KEY", "sk-ant-fake-for-tests".into()),
            ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1".into()),
            ("DISABLE_AUTOUPDATER", "1".into()),
            ("DISABLE_TELEMETRY", "1".into()),
            ("DISABLE_ERROR_REPORTING", "1".into()),
        ]
    }

    /// Every `/v1/messages` body so far.
    pub fn requests(&self) -> Vec<Value> {
        self.requests.lock().unwrap().clone()
    }

    /// The tool results the CLI sent back to the model, as (is_error, text).
    pub fn tool_results(&self) -> Vec<(bool, String)> {
        let mut out: Vec<(bool, String)> = vec![];
        for r in self.requests() {
            for m in r["messages"].as_array().into_iter().flatten() {
                for b in m["content"].as_array().into_iter().flatten() {
                    if b["type"] == "tool_result" {
                        let found = (b["is_error"] == json!(true), text_of(&b["content"]));
                        if !out.contains(&found) {
                            out.push(found);
                        }
                    }
                }
            }
        }
        out
    }

    /// The names of the tools the CLI offered the model in its latest main request.
    pub fn tools(&self) -> Vec<String> {
        self.requests()
            .iter()
            .rev()
            .find_map(|r| r["tools"].as_array().filter(|t| !t.is_empty()).cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|t| t["name"].as_str().map(str::to_string))
            .collect()
    }
}

/// The text of a message's content: a string, or the text blocks of a list.
fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

enum Reply {
    Text(String),
    Tool { name: String, command: String },
}

fn reply_to(body: &Value) -> Reply {
    let messages = body["messages"].as_array().cloned().unwrap_or_default();
    let blocks = |m: &Value| m["content"].as_array().cloned().unwrap_or_default();
    // The tool was called and its result is back: say what it was.
    if let Some(result) = messages
        .iter()
        .flat_map(blocks)
        .find(|b| b["type"] == "tool_result")
    {
        let text = text_of(&result["content"]);
        return Reply::Text(format!(
            "saw: {}",
            text.chars().take(600).collect::<String>()
        ));
    }
    // Only the main conversation has tools; the CLI's side requests (titles and the like) get
    // a plain answer. The prompt is one of the user messages (the CLI adds its own around it).
    let has_tools = body["tools"].as_array().is_some_and(|t| !t.is_empty());
    let call = messages
        .iter()
        .filter(|m| m["role"] == "user")
        .map(|m| text_of(&m["content"]))
        .find_map(|text| {
            text.lines().find_map(|l| {
                let rest = l.trim().strip_prefix("TOOL ")?;
                let (name, command) = rest.split_once(' ')?;
                Some((name.to_string(), command.to_string()))
            })
        });
    match call {
        Some((name, command)) if has_tools => Reply::Tool { name, command },
        _ => Reply::Text("ok".into()),
    }
}

fn message(reply: &Reply, model: &str) -> Value {
    let (content, stop) = match reply {
        Reply::Text(t) => (json!([{ "type": "text", "text": t }]), "end_turn"),
        Reply::Tool { name, command } => (
            json!([{ "type": "tool_use", "id": "toolu_fake_1", "name": name, "input": { "command": command } }]),
            "tool_use",
        ),
    };
    json!({
        "id": "msg_fake_1", "type": "message", "role": "assistant", "model": model,
        "content": content, "stop_reason": stop, "stop_sequence": null,
        "usage": { "input_tokens": 10, "output_tokens": 5 },
    })
}

fn sse(reply: &Reply, model: &str) -> String {
    let mut out = String::new();
    let mut ev = |name: &str, data: Value| {
        out.push_str(&format!("event: {name}\ndata: {data}\n\n"));
    };
    let mut start = message(reply, model);
    start["content"] = json!([]);
    start["stop_reason"] = Value::Null;
    ev(
        "message_start",
        json!({ "type": "message_start", "message": start }),
    );
    let stop = match reply {
        Reply::Text(t) => {
            ev(
                "content_block_start",
                json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "text", "text": "" } }),
            );
            ev(
                "content_block_delta",
                json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": t } }),
            );
            "end_turn"
        }
        Reply::Tool { name, command } => {
            ev(
                "content_block_start",
                json!({ "type": "content_block_start", "index": 0,
                    "content_block": { "type": "tool_use", "id": "toolu_fake_1", "name": name, "input": {} } }),
            );
            ev(
                "content_block_delta",
                json!({ "type": "content_block_delta", "index": 0,
                    "delta": { "type": "input_json_delta", "partial_json": json!({ "command": command }).to_string() } }),
            );
            "tool_use"
        }
    };
    ev(
        "content_block_stop",
        json!({ "type": "content_block_stop", "index": 0 }),
    );
    ev(
        "message_delta",
        json!({ "type": "message_delta", "delta": { "stop_reason": stop, "stop_sequence": null }, "usage": { "output_tokens": 5 } }),
    );
    ev("message_stop", json!({ "type": "message_stop" }));
    out
}

async fn serve(mut s: TcpStream, seen: Arc<Mutex<Vec<Value>>>) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        match s.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or_default().to_string();
    let length = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while buf.len() < head_end + length {
        match s.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let body: Value = serde_json::from_slice(&buf[head_end..head_end + length]).unwrap_or_default();
    let path = request_line.split(' ').nth(1).unwrap_or_default();
    let (status, kind, text) = if !request_line.starts_with("POST ") {
        (
            "404 Not Found",
            "application/json",
            json!({ "type": "error", "error": { "type": "not_found_error", "message": "no such page" } }).to_string(),
        )
    } else if path.contains("/count_tokens") {
        (
            "200 OK",
            "application/json",
            json!({ "input_tokens": 10 }).to_string(),
        )
    } else if path.starts_with("/v1/messages") {
        let model = body["model"].as_str().unwrap_or("fake-model").to_string();
        let reply = reply_to(&body);
        let stream = body["stream"] == json!(true);
        seen.lock().unwrap().push(body);
        if stream {
            ("200 OK", "text/event-stream", sse(&reply, &model))
        } else {
            (
                "200 OK",
                "application/json",
                message(&reply, &model).to_string(),
            )
        }
    } else {
        (
            "404 Not Found",
            "application/json",
            json!({ "type": "error", "error": { "type": "not_found_error", "message": "no such page" } }).to_string(),
        )
    };
    let head = format!(
        "HTTP/1.1 {status}\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        text.len()
    );
    let _ = s.write_all(head.as_bytes()).await;
    let _ = s.write_all(text.as_bytes()).await;
    let _ = s.shutdown().await;
}
