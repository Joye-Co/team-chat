//! Push for the phone: register this sign-in's device token, read the 等你 badge, read the data a
//! push carries, and answer a card from a notification over plain HTTP. None of these needs a
//! socket or the cache, so the notification extension and a background action can call them.
//! They wait for the server: call them off the main thread.

use chat_core::HttpClient;
use chat_core::proto::{
    AnswerCardRequest, NotifyReason, PushData, PushEnvironment, PushPlatform,
    RegisterPushDeviceRequest,
};

use crate::CoreError;

fn authed(base_url: &str, token: String) -> Result<HttpClient, CoreError> {
    Ok(HttpClient::new(base_url, Some(token))?)
}

fn run<T>(
    f: impl std::future::Future<Output = Result<T, chat_core::ApiError>>,
) -> Result<T, CoreError> {
    Ok(chat_core::client::background().block_on(f)?)
}

/// Registers the APNs device token (raw bytes as lower-case hex) for this sign-in.
/// `sandbox` is true for development builds.
#[uniffi::export]
pub fn register_push_device(
    base_url: String,
    token: String,
    push_token: String,
    app_id: String,
    sandbox: bool,
) -> Result<(), CoreError> {
    let http = authed(&base_url, token)?;
    let req = RegisterPushDeviceRequest {
        platform: PushPlatform::Apns,
        token: push_token.to_ascii_lowercase(),
        app_id,
        environment: if sandbox {
            PushEnvironment::Sandbox
        } else {
            PushEnvironment::Production
        },
    };
    req.validate().map_err(|_| CoreError::Invalid)?;
    run(async move { http.register_push_device(&req).await })?;
    Ok(())
}

/// Stops pushes to this sign-in's phone (通知 off, or before signing out). Returns how many
/// devices were removed.
#[uniffi::export]
pub fn unregister_push_device(base_url: String, token: String) -> Result<u64, CoreError> {
    let http = authed(&base_url, token)?;
    run(async move { http.remove_push_devices().await })
}

/// Topics that wait on me (等你): the number on the app icon.
#[uniffi::export]
pub fn push_badge(base_url: String, token: String) -> Result<u64, CoreError> {
    let http = authed(&base_url, token)?;
    run(async move { http.badge().await })
}

/// Answers a card from a notification action. True when the answer was stored now or before
/// (a repeated tap with the same `client_event_id`).
#[uniffi::export]
pub fn answer_from_notification(
    base_url: String,
    token: String,
    group_id: String,
    message_id: String,
    rev: u32,
    action_id: String,
    client_event_id: String,
) -> Result<bool, CoreError> {
    let http = authed(&base_url, token)?;
    let req = AnswerCardRequest {
        rev,
        action_id,
        client_event_id: Some(client_event_id),
    };
    req.validate().map_err(|_| CoreError::Invalid)?;
    let ack = run(async move { http.answer_card(&group_id, &message_id, &req).await })?;
    Ok(ack.stored || ack.duplicate == Some(true))
}

/// One answer a notification offers.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PushActionItem {
    pub id: String,
    pub label: String,
}

/// What a push is about, from its `tc` data.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PushInfo {
    /// `mention`, `waiting`, `result` or `message`.
    pub reason: String,
    /// `waiting`, `mention`, `result`, `blocked`, or empty.
    pub event: String,
    pub group_id: String,
    pub topic_id: Option<String>,
    pub message_id: Option<String>,
    pub seq: u64,
    /// The card that can be answered from the notification, with its rev and answers.
    pub ask_message_id: Option<String>,
    pub ask_rev: u32,
    pub actions: Vec<PushActionItem>,
}

/// Reads the `tc` value of a push (as JSON text). `None` for anything else.
#[uniffi::export]
pub fn parse_push(json: String) -> Option<PushInfo> {
    let data: PushData = serde_json::from_str(&json).ok()?;
    data.validate().ok()?;
    let reason = match data.reason {
        NotifyReason::Mention => "mention",
        NotifyReason::Waiting => "waiting",
        NotifyReason::Result => "result",
        NotifyReason::Message => "message",
    };
    let event = data
        .notify_event
        .and_then(|e| serde_json::to_value(e).ok())
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    let (ask_message_id, ask_rev, actions) = match data.ask {
        Some(a) => (
            Some(a.message_id),
            a.rev,
            a.actions
                .into_iter()
                .map(|x| PushActionItem {
                    id: x.id,
                    label: x.label,
                })
                .collect(),
        ),
        None => (None, 0, Vec::new()),
    };
    Some(PushInfo {
        reason: reason.into(),
        event,
        group_id: data.group_id,
        topic_id: data.topic_id,
        message_id: data.message_id,
        seq: data.seq,
        ask_message_id,
        ask_rev,
        actions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    #[test]
    fn parses_push_data() {
        let json = r#"{"v":1,"reason":"waiting","notifyEvent":"waiting","groupId":"g","topicId":"t","messageId":"m","seq":4,
            "ask":{"messageId":"m","rev":2,"actions":[{"id":"ok","label":"可以"}]}}"#;
        let p = parse_push(json.into()).expect("parses");
        assert_eq!(p.reason, "waiting");
        assert_eq!(p.event, "waiting");
        assert_eq!(p.topic_id.as_deref(), Some("t"));
        assert_eq!(p.ask_message_id.as_deref(), Some("m"));
        assert_eq!(p.ask_rev, 2);
        assert_eq!(
            p.actions,
            vec![PushActionItem {
                id: "ok".into(),
                label: "可以".into()
            }]
        );
        let plain =
            parse_push(r#"{"v":1,"reason":"mention","groupId":"g","seq":1}"#.into()).unwrap();
        assert_eq!(plain.event, "");
        assert!(plain.actions.is_empty());
        assert!(parse_push(r#"{"v":2,"reason":"mention","groupId":"g","seq":1}"#.into()).is_none());
        assert!(parse_push("not json".into()).is_none());
    }

    #[test]
    fn rejects_a_bad_token_before_the_network() {
        let r = register_push_device(
            "http://127.0.0.1:9".into(),
            "tok".into(),
            "xyz".into(),
            "dev.example.chat".into(),
            true,
        );
        assert!(matches!(r, Err(CoreError::Invalid)));
    }

    /// Serves one request and answers `body`; sends back the request line, headers and body.
    fn one_shot(status: &str, body: &'static str) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let status = status.to_string();
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(s.try_clone().unwrap());
            let mut head = String::new();
            let mut len = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
                if line == "\r\n" {
                    break;
                }
                head.push_str(&line);
            }
            let mut buf = vec![0; len];
            reader.read_exact(&mut buf).unwrap();
            head.push_str(&String::from_utf8_lossy(&buf));
            tx.send(head).unwrap();
            let resp = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            s.write_all(resp.as_bytes()).unwrap();
        });
        (format!("http://{addr}"), rx)
    }

    #[test]
    fn registers_answers_and_reads_the_badge_over_http() {
        let (base, rx) = one_shot(
            "200 OK",
            r#"{"id":"d","platform":"apns","appId":"dev.example.chat","environment":"sandbox","createdAt":1,"updatedAt":1,"current":true}"#,
        );
        register_push_device(
            base,
            "tok".into(),
            "AB".repeat(32),
            "dev.example.chat".into(),
            true,
        )
        .unwrap();
        let req = rx.recv().unwrap();
        assert!(req.starts_with("POST /v1/me/push-devices "));
        assert!(
            req.to_ascii_lowercase()
                .contains("authorization: bearer tok")
        );
        assert!(req.contains(&format!(r#""token":"{}""#, "ab".repeat(32))));
        assert!(req.contains(r#""environment":"sandbox""#));

        let (base, rx) = one_shot(
            "200 OK",
            r#"{"eventId":"e","stored":true,"answerId":"a","expect":{"by":"b","mode":"none"}}"#,
        );
        let ok = answer_from_notification(
            base,
            "tok".into(),
            "g".into(),
            "m".into(),
            2,
            "ok".into(),
            "push-1".into(),
        )
        .unwrap();
        assert!(ok);
        let req = rx.recv().unwrap();
        assert!(req.starts_with("POST /v1/groups/g/messages/m/answer "));
        assert!(req.contains(r#""actionId":"ok""#));

        let (base, _rx) = one_shot("200 OK", r#"{"waiting":3}"#);
        assert_eq!(push_badge(base, "tok".into()).unwrap(), 3);

        let (base, _rx) = one_shot("200 OK", r#"{"removed":1}"#);
        assert_eq!(unregister_push_device(base, "tok".into()).unwrap(), 1);

        let (base, _rx) = one_shot(
            "409 Conflict",
            r#"{"error":{"code":"conflict","message":"answered","reason":"card.already_answered"}}"#,
        );
        assert!(matches!(
            answer_from_notification(
                base,
                "tok".into(),
                "g".into(),
                "m".into(),
                2,
                "ok".into(),
                "push-2".into()
            ),
            Err(CoreError::Failed { .. })
        ));
    }
}
