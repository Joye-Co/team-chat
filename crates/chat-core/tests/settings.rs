//! Settings over HTTP (notification settings, approval rules, devices) against the in-process
//! mock, and the settings fields of the live stream (`notifyEvent`, the `digest` transient).

mod common;
mod mock;
mod mock_http;

use chat_core::proto::*;
use chat_core::{ApiError, Client, ClientConfig, ClientUpdate};
use common::*;
use mock_http::{MockHttp, TOKEN};
use serde_json::json;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedReceiver;

const WAIT: Duration = Duration::from_secs(10);

async fn next_update(
    rx: &mut UnboundedReceiver<ClientUpdate>,
    what: &str,
    mut pred: impl FnMut(&ClientUpdate) -> bool,
) -> ClientUpdate {
    let t0 = Instant::now();
    loop {
        let left = WAIT.saturating_sub(t0.elapsed());
        match tokio::time::timeout(left, rx.recv()).await {
            Ok(Some(u)) if pred(&u) => return u,
            Ok(Some(_)) => {}
            _ => panic!("timed out waiting for update: {what}"),
        }
    }
}

fn device(id: &str, name: &str, current: bool) -> Device {
    Device {
        id: id.into(),
        name: name.into(),
        created_at: 1,
        last_used_at: current.then_some(2),
        current,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn notify_settings_approval_rules_and_devices_over_http() {
    let http = MockHttp::start().await;
    let dir = tempfile::tempdir().unwrap();
    let client = Client::new(ClientConfig::new(http.base_url(), TOKEN, dir.path())).unwrap();
    let api = client.http();

    // 通知: defaults, then a partial change; a bad time never leaves the client.
    assert_eq!(
        api.notify_settings().await.unwrap(),
        NotifySettings::default()
    );
    let req = UpdateNotifySettingsRequest {
        events: Some(NotifyEventSwitchesPatch {
            result: Some(false),
            ..Default::default()
        }),
        digest_at: Some(Some("09:00".into())),
        quiet_hours: Some(QuietHoursPatch {
            enabled: Some(true),
            start: Some("23:00".into()),
            ..Default::default()
        }),
        time_zone: Some("Australia/Sydney".into()),
    };
    let s = client.update_notify_settings(req).await.unwrap().unwrap();
    assert!(!s.events.result && s.events.waiting);
    assert_eq!(s.digest_at.as_deref(), Some("09:00"));
    assert_eq!(
        (s.quiet_hours.enabled, s.quiet_hours.start.as_str()),
        (true, "23:00")
    );
    assert_eq!(s.time_zone, "Australia/Sydney");
    let off = UpdateNotifySettingsRequest {
        digest_at: Some(None),
        ..Default::default()
    };
    assert_eq!(
        api.update_notify_settings(&off).await.unwrap().digest_at,
        None
    );
    let bad = UpdateNotifySettingsRequest {
        digest_at: Some(Some("9:00".into())),
        ..Default::default()
    };
    let before = http.with(|s| s.count("PATCH", "/v1/me/notify-settings"));
    assert!(matches!(
        api.update_notify_settings(&bad).await,
        Err(ApiError::Invalid(_))
    ));
    assert_eq!(
        http.with(|s| s.count("PATCH", "/v1/me/notify-settings")),
        before
    );

    // 成员 › 默认规则: read, change as an owner, refused for a member.
    assert_eq!(
        api.approval_policy().await.unwrap(),
        ApprovalPolicy::default()
    );
    let up = UpdateApprovalPolicyRequest {
        ask_before: Some(ApprovalActionsPatch {
            merge_main: Some(false),
            ..Default::default()
        }),
        max_spend_cny: Some(None),
        approver: Some(Approver::Owners),
        ..Default::default()
    };
    let p = client
        .update_approval_policy(up.clone())
        .await
        .unwrap()
        .unwrap();
    assert!(!p.ask_before.merge_main && p.ask_before.deploy_prod);
    assert_eq!((p.max_spend_cny, p.approver), (None, Approver::Owners));
    let zero = UpdateApprovalPolicyRequest {
        max_spend_cny: Some(Some(0)),
        ..Default::default()
    };
    assert!(matches!(
        api.update_approval_policy(&zero).await,
        Err(ApiError::Invalid(_))
    ));
    http.with(|s| s.not_owner = true);
    assert!(matches!(
        api.update_approval_policy(&up).await,
        Err(ApiError::Server { status: 403, .. })
    ));

    // 账号 › 登录的设备: list, sign out another device, an unknown one is not found.
    http.with(|s| {
        s.devices = vec![
            device("TK1", "Joye 的 MacBook Pro", true),
            device("TK2", "Joye 的 iPhone", false),
        ]
    });
    let list = client.devices().await.unwrap().unwrap();
    assert_eq!(list.devices.len(), 2);
    assert!(list.devices[0].current);
    assert_eq!(client.sign_out_device("TK2").await.unwrap().unwrap(), 1);
    assert_eq!(api.devices().await.unwrap().devices.len(), 1);
    assert!(matches!(
        api.sign_out_device("TK2").await,
        Err(ApiError::Server { status: 404, .. })
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn notify_event_and_digest_reach_the_app() {
    let server = mock::MockServer::start(
        member(ME, "me"),
        vec![member(ME, "me"), member(ALICE, "alice")],
    )
    .await;
    server.add_group(group(G1, &[ME, ALICE]));
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
            .contains(&json!(FEATURE_SETTINGS))
    );

    // A followed topic gets blocked: a plain reason with the `blocked` event.
    let tid = server.with(|s| {
        s.post(G1, ALICE, None, None, "a0", "root", false)
            .1
            .unwrap()
            .id
    });
    server.with(|s| {
        s.hide_live.insert(s.groups[G1].log.len() as u64 + 1);
        let seq = s.append(
            G1,
            ALICE,
            EventData::TopicUpdated(TopicUpdatedData {
                topic_id: tid.clone(),
                status: Some(TopicStatus::Blocked),
                ..Default::default()
            }),
        );
        let event = s.groups[G1].log[seq as usize - 1].clone();
        s.broadcast(
            &json!({"t": "event", "event": event, "notify": "message", "notifyEvent": "blocked"}),
            None,
        );
    });
    let ClientUpdate::Notify(n) =
        next_update(&mut rx, "notify", |u| matches!(u, ClientUpdate::Notify(_))).await
    else {
        unreachable!()
    };
    assert_eq!(
        (n.reason, n.event, n.topic_id.as_deref()),
        (
            NotifyReason::Message,
            Some(NotifyEvent::Blocked),
            Some(tid.as_str())
        )
    );

    server.with(|s| {
        s.transient(json!({
            "type": "digest", "at": 2, "since": 1,
            "counts": {"waiting": 2, "mention": 0, "result": 0, "blocked": 1},
            "items": [{"groupId": G1, "topicId": tid, "event": "waiting"}],
        }))
    });
    let ClientUpdate::Digest(d) =
        next_update(&mut rx, "digest", |u| matches!(u, ClientUpdate::Digest(_))).await
    else {
        unreachable!()
    };
    assert_eq!((d.counts.waiting, d.counts.blocked), (2, 1));
    assert_eq!(d.items[0].event, NotifyEvent::Waiting);
}
