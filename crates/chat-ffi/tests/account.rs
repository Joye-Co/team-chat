//! The account exports against a stub server: each call sends the right request and maps the
//! answer (and the errors) the way the screens read them.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use chat_ffi::*;
use serde_json::{Value, json};

type Seen = Arc<Mutex<Vec<(String, String, Value, bool)>>>;

const MEMBER: &str =
    r#"{"id":"01JBX0ALICE0000000000000AB","kind":"human","name":"Alice","level":"member"}"#;

/// Serves canned answers by "METHOD /path"; records method, path, body and whether a token came.
fn stub(routes: Vec<(&'static str, u16, String)>) -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen: Seen = Arc::default();
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut parts = line.split_whitespace();
            let method = parts.next().unwrap_or("").to_string();
            let path = parts.next().unwrap_or("").to_string();
            let (mut len, mut authed) = (0usize, false);
            loop {
                let mut h = String::new();
                reader.read_line(&mut h).unwrap();
                if h == "\r\n" || h.is_empty() {
                    break;
                }
                let lower = h.to_lowercase();
                if let Some(v) = lower.strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
                authed |= lower.starts_with("authorization: bearer");
            }
            let mut body = vec![0; len];
            reader.read_exact(&mut body).unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            log.lock()
                .unwrap()
                .push((method.clone(), path.clone(), body, authed));
            let key = format!("{method} {path}");
            let (status, text) = routes
                .iter()
                .find(|(k, ..)| *k == key)
                .map(|(_, s, t)| (*s, t.clone()))
                .unwrap_or((
                    404,
                    r#"{"error":{"code":"not_found","message":"no"}}"#.into(),
                ));
            let reply = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
                text.len()
            );
            stream.write_all(reply.as_bytes()).unwrap();
        }
    });
    (base, seen)
}

fn signed_in() -> String {
    format!(r#"{{"token":"tc_new","member":{MEMBER}}}"#)
}

fn last(seen: &Seen) -> (String, String, Value, bool) {
    seen.lock().unwrap().last().unwrap().clone()
}

#[test]
fn password_sign_in_sends_email_and_device() {
    let (base, seen) = stub(vec![("POST /v1/auth/password/sign-in", 200, signed_in())]);
    let s = password_sign_in(
        base,
        " Joye@Example.com ".into(),
        "hunter2hunter2".into(),
        "iPhone".into(),
    )
    .unwrap();
    assert_eq!(
        (s.token.as_str(), s.member_name.as_str()),
        ("tc_new", "Alice")
    );
    let (_, _, body, authed) = last(&seen);
    assert_eq!(
        body,
        json!({"email":"joye@example.com","password":"hunter2hunter2","deviceName":"iPhone"})
    );
    assert!(!authed);
}

#[test]
fn wrong_password_and_rate_limit_map_to_errors() {
    let (base, _) = stub(vec![(
        "POST /v1/auth/password/sign-in",
        401,
        r#"{"error":{"code":"unauthorized","message":"x"}}"#.into(),
    )]);
    let e = password_sign_in(base, "a@b.co".into(), "x".into(), "i".into()).unwrap_err();
    assert!(matches!(e, CoreError::Unauthorized));
    let (base, _) = stub(vec![(
        "POST /v1/auth/redeem",
        429,
        r#"{"error":{"code":"rate_limited","message":"x"}}"#.into(),
    )]);
    let e = redeem_invite(base, "ABCD-EFGH".into(), "i".into()).unwrap_err();
    assert!(matches!(e, CoreError::RateLimited));
    // Checked before sending.
    assert!(matches!(
        password_sign_in(
            "http://127.0.0.1:9".into(),
            "nope".into(),
            "x".into(),
            "i".into()
        ),
        Err(CoreError::Invalid)
    ));
}

#[test]
fn redeem_invite_trims_the_code() {
    let (base, seen) = stub(vec![("POST /v1/auth/redeem", 200, signed_in())]);
    redeem_invite(base, " abcd-efgh ".into(), "iPhone".into()).unwrap();
    assert_eq!(
        last(&seen).2,
        json!({"inviteCode":"abcd-efgh","deviceName":"iPhone"})
    );
}

#[test]
fn qr_request_and_poll() {
    let (base, seen) = stub(vec![
        (
            "POST /v1/auth/qr/request",
            200,
            r#"{"id":"r1","code":"H3VD-2WQ8","secret":"s3","expiresAt":5}"#.into(),
        ),
        (
            "POST /v1/auth/qr/poll",
            200,
            format!(r#"{{"state":"approved","token":"tc_q","member":{MEMBER}}}"#),
        ),
    ]);
    let r = qr_request(base.clone(), "iPhone".into()).unwrap();
    assert_eq!(r.code, "H3VD-2WQ8");
    assert_eq!(
        parse_sign_in_code(r.qr_text.clone())
            .unwrap()
            .server
            .as_deref(),
        Some(base.as_str())
    );
    match qr_poll(base, r.id, r.secret).unwrap() {
        SignInPoll::Approved { signed_in } => assert_eq!(signed_in.token, "tc_q"),
        other => panic!("{other:?}"),
    }
    assert_eq!(last(&seen).2, json!({"id":"r1","secret":"s3"}));
}

#[test]
fn qr_claim_and_codes() {
    let (base, seen) = stub(vec![("POST /v1/auth/qr/claim", 200, signed_in())]);
    qr_claim(base, "7K2M-9QPX".into(), "iPhone".into()).unwrap();
    assert_eq!(
        last(&seen).2,
        json!({"code":"7K2M-9QPX","deviceName":"iPhone"})
    );
    let text = sign_in_qr_text("7K2M-9QPX".into(), "https://x.example".into());
    assert_eq!(
        parse_sign_in_code(text),
        Some(SignInCode {
            code: "7K2M-9QPX".into(),
            server: Some("https://x.example".into())
        })
    );
    assert_eq!(
        parse_sign_in_code("7k2m9qpx".into()).unwrap().code,
        "7K2M-9QPX"
    );
    assert_eq!(parse_sign_in_code("https://example.com".into()), None);
}

fn client(base: &str) -> Arc<ChatClient> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("chat-ffi-account-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    ChatClient::new(base.into(), "tc_mine".into(), dir.to_string_lossy().into()).unwrap()
}

#[test]
fn devices_list_and_sign_out() {
    let (base, seen) = stub(vec![
        (
            "GET /v1/me/devices",
            200,
            r#"{"devices":[{"id":"t1","name":"Joye 的 iPhone","createdAt":1,"lastUsedAt":2,"current":true},{"id":"t2","name":"MacBook Pro","createdAt":1,"lastUsedAt":null,"current":false}]}"#.into(),
        ),
        ("DELETE /v1/me/devices/t2", 200, r#"{"revoked":1}"#.into()),
    ]);
    let c = client(&base);
    let d = c.devices().unwrap();
    assert_eq!(d.len(), 2);
    assert_eq!((d[0].kind.as_str(), d[0].current), ("phone", true));
    assert_eq!((d[1].kind.as_str(), d[1].last_used_at), ("laptop", None));
    c.sign_out_device("t2".into()).unwrap();
    let (method, path, _, authed) = last(&seen);
    assert_eq!(
        (method.as_str(), path.as_str(), authed),
        ("DELETE", "/v1/me/devices/t2", true)
    );
    assert_eq!(device_kind("Mac Studio"), "desktop");
}

#[test]
fn password_settings() {
    let (base, seen) = stub(vec![
        (
            "GET /v1/me/password",
            200,
            r#"{"set":false,"updatedAt":null}"#.into(),
        ),
        (
            "PUT /v1/me/password",
            200,
            r#"{"set":true,"updatedAt":7}"#.into(),
        ),
        (
            "POST /v1/me/password/remove",
            400,
            r#"{"error":{"code":"invalid","message":"x","reason":"password.current"}}"#.into(),
        ),
        ("GET /v1/me/email", 200, r#"{"email":"a@b.co"}"#.into()),
    ]);
    let c = client(&base);
    assert!(!c.password_state().unwrap().set);
    let s = c
        .set_password("new password".into(), Some(String::new()))
        .unwrap();
    assert_eq!((s.set, s.updated_at), (true, Some(7)));
    assert_eq!(last(&seen).2, json!({"password":"new password"}));
    assert!(matches!(
        c.set_password("short".into(), None),
        Err(CoreError::Invalid)
    ));
    assert!(matches!(
        c.remove_password(Some("old".into())),
        Err(CoreError::Invalid)
    ));
    assert_eq!(last(&seen).2, json!({"currentPassword":"old"}));
    assert_eq!(c.my_email().unwrap().as_deref(), Some("a@b.co"));
}

#[test]
fn offer_peek_and_approve() {
    let (base, seen) = stub(vec![
        (
            "POST /v1/me/qr/offer",
            200,
            r#"{"id":"o1","code":"7K2M-9QPX","expiresAt":9}"#.into(),
        ),
        (
            "GET /v1/me/qr/offers/o1",
            200,
            r#"{"state":"used","deviceName":"iPhone"}"#.into(),
        ),
        (
            "POST /v1/me/qr/peek",
            200,
            r#"{"deviceName":"MacBook","expiresAt":9}"#.into(),
        ),
        ("POST /v1/me/qr/approve", 200, r#"{"ok":true}"#.into()),
    ]);
    let c = client(&base);
    let o = c.offer_sign_in().unwrap();
    assert_eq!(parse_sign_in_code(o.qr_text).unwrap().code, "7K2M-9QPX");
    assert_eq!(
        c.offer_state(o.id).unwrap(),
        OfferState::Used {
            device_name: "iPhone".into()
        }
    );
    assert_eq!(c.peek_sign_in("h3vd 2wq8".into()).unwrap(), "MacBook");
    c.approve_sign_in("H3VD-2WQ8".into()).unwrap();
    assert_eq!(last(&seen).2, json!({"code":"H3VD-2WQ8"}));
}

#[test]
fn wrong_code_is_not_found() {
    let (base, _) = stub(vec![]);
    let c = client(&base);
    assert!(matches!(
        c.peek_sign_in("ZZZZ-ZZZZ".into()),
        Err(CoreError::NotFound)
    ));
}
