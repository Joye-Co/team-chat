//! Each person's GitHub sign-in (docs/protocol.md, "GitHub sign-in"): the HTTP calls against
//! the in-process mock, and the `github.status` transient of the live stream.

mod common;
mod mock;
mod mock_http;

use chat_core::proto::*;
use chat_core::{Client, ClientConfig, ClientUpdate};
use common::*;
use mock_http::{MockHttp, TOKEN};
use serde_json::json;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(10);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connect_poll_and_disconnect_over_http() {
    let http = MockHttp::start().await;
    http.with(|s| {
        s.github_pending_polls = 1;
        s.github_waits = vec![GithubWait {
            run_id: "R1".into(),
            group_id: G1.into(),
            topic_id: "T1".into(),
            bot_id: "B1".into(),
            reason: "not_connected".into(),
        }];
    });
    let dir = tempfile::tempdir().unwrap();
    let client = Client::new(ClientConfig::new(http.base_url(), TOKEN, dir.path())).unwrap();
    let api = client.http();

    let status = api.github_status().await.unwrap();
    assert!(status.available && status.account.is_none());
    assert_eq!(status.waits.len(), 1);

    let device = api.github_device().await.unwrap();
    assert_eq!(device.user_code, "WDJB-MJHT");
    assert_eq!(device.interval, 5);
    assert_eq!(
        api.github_poll(&device.device_id).await.unwrap().state,
        "pending"
    );
    let done = api.github_poll(&device.device_id).await.unwrap();
    assert_eq!(done.state, "done");
    assert_eq!(done.account.unwrap().login, "octo");

    let status = api.github_status().await.unwrap();
    assert_eq!(status.account.unwrap().name.as_deref(), Some("Octo Cat"));
    assert!(status.waits.is_empty());

    api.github_disconnect().await.unwrap();
    assert!(api.github_status().await.unwrap().account.is_none());
    assert!(api.github_poll("other").await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hello_asks_for_github_status_and_it_reaches_the_app() {
    let server = mock::MockServer::start(member(ME, "me"), vec![member(ME, "me")]).await;
    let dir = tempfile::tempdir().unwrap();
    let client = Client::new(ClientConfig::new(
        server.base_url(),
        mock::TOKEN,
        dir.path(),
    ))
    .unwrap();
    let mut rx = client.subscribe();
    client.start();
    tokio::time::timeout(WAIT, async {
        while !server.with(|s| !s.frames_of("hello").is_empty()) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("hello");
    let hello = server.with(|s| s.frames_of("hello")[0].clone());
    assert!(
        hello["features"]
            .as_array()
            .unwrap()
            .contains(&json!(FEATURE_GITHUB))
    );

    server.with(|s| {
        s.transient(json!({
            "type": "github.status",
            "account": {"login": "octo", "status": "invalid", "connectedAt": 3},
        }))
    });
    let t0 = Instant::now();
    let account = loop {
        let left = WAIT.saturating_sub(t0.elapsed());
        match tokio::time::timeout(left, rx.recv()).await {
            Ok(Some(ClientUpdate::GithubStatus(g))) => break g.account,
            Ok(Some(_)) => {}
            _ => panic!("timed out waiting for github.status"),
        }
    };
    let account = account.expect("an account");
    assert_eq!(
        (account.login.as_str(), account.status.as_str()),
        ("octo", "invalid")
    );

    // Disconnected: no account.
    server.with(|s| s.transient(json!({"type": "github.status"})));
    let t0 = Instant::now();
    loop {
        let left = WAIT.saturating_sub(t0.elapsed());
        match tokio::time::timeout(left, rx.recv()).await {
            Ok(Some(ClientUpdate::GithubStatus(g))) => {
                assert!(g.account.is_none());
                break;
            }
            Ok(Some(_)) => {}
            _ => panic!("timed out waiting for the disconnect"),
        }
    }
}
