//! `--selftest-live <url> --token-file <path> --report <path>`: signs in to a
//! real server with the chat-core provider, opens the first group, starts a
//! topic, sends a message into it, waits for each ack and for the message's
//! event in the cache, then restores the session in a second provider from
//! the saved session and cache. Writes a JSON report and exits. The window
//! opens without a Dock icon and without taking focus. The cache lives in a
//! temporary folder that is removed at the end; the token is kept in memory
//! only and never written to the report.
//!
//! With `--interact <dir>` it runs the 1.1 interaction and 引用提问 check of
//! `selftest_interact` instead of the topic and message steps.
//!
//! With `--read-only` it only signs in with the token, connects, waits for the catch-up
//! of every group and reports counts: no window, no topic, no message, no read
//! position, so it is safe against production.
//!
//! The file may hold a token (`tc_...`) or an invite code. With an invite
//! code the token is this test's own, so the test also signs out with
//! "wipe": it checks that the server revoked the token and the cache is gone.

use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use gpui::{ActivationPolicy, App, AsyncApp};

use super::{CoreProvider, ProviderConfig, convert::pending_topic_id, session::TokenStorage};
use crate::{
    app::{self, Services},
    data::{
        ChatProvider, ConnectionState, Credential, LoginRequest, SendRequest, SendState,
        SessionState, StartTopicRequest,
    },
};

const TIMEOUT: Duration = Duration::from_secs(45);
const POLL: Duration = Duration::from_millis(20);

pub struct LiveArgs {
    pub url: String,
    pub token_file: PathBuf,
    pub report: PathBuf,
    pub read_only: bool,
    /// `--interact <dir>`: the INTERACT 7.1 and 引用提问 live check
    /// ([`super::selftest_interact`]), with its captures in `dir`.
    pub interact: Option<PathBuf>,
}

/// Runs inside the app. Calls `finish(report_json, ok, cx)` at the end.
pub fn run(
    args: LiveArgs,
    started: Instant,
    finish: fn(&PathBuf, serde_json::Value, bool, &mut App),
    cx: &mut App,
) {
    cx.set_activation_policy(ActivationPolicy::Accessory);
    let credential = match std::fs::read_to_string(&args.token_file) {
        Ok(t) if Credential::from_input(&t).is_some() => Credential::from_input(&t).unwrap(),
        _ => {
            let json = serde_json::json!({ "ok": false, "error": "cannot read the token file" });
            finish(&args.report, json, false, cx);
            return;
        }
    };
    let data_root = std::env::temp_dir().join(format!(
        "{}-selftest-live-{}",
        env!("PRODUCT_SLUG"),
        std::process::id()
    ));
    let tokens = TokenStorage::memory();
    let provider = CoreProvider::new(ProviderConfig {
        data_root: data_root.clone(),
        tokens: tokens.clone(),
        restore: false,
    });
    if args.read_only {
        if matches!(credential, Credential::InviteCode(_)) {
            // Redeeming an invite is a write.
            let json = serde_json::json!({ "ok": false, "error": "--read-only needs a token" });
            finish(&args.report, json, false, cx);
            return;
        }
        let login_at = Instant::now();
        provider.login(LoginRequest {
            server_url: args.url.clone(),
            credential,
            device_name: "selftest".into(),
        });
        let report = args.report.clone();
        cx.spawn(async move |cx: &mut AsyncApp| {
            let (ok, json) = match drive_read_only(&provider, login_at, started, cx).await {
                Ok(json) => (true, json),
                Err((step, mut json)) => {
                    json["ok"] = false.into();
                    json["error"] = format!("timed out or failed at: {step}").into();
                    (false, json)
                }
            };
            cx.update(|cx| {
                provider.sign_out_local_for_selftest();
                finish(&report, json, ok, cx);
            });
            let _ = std::fs::remove_dir_all(&data_root);
        })
        .detach();
        return;
    }
    let dyn_provider: Arc<dyn ChatProvider> = provider.clone();
    let services = Services::new(dyn_provider, cx);
    let root = match app::open_main_window(services.clone(), false, None, cx) {
        Ok(root) => root,
        Err(error) => {
            let json = serde_json::json!({ "ok": false, "error": error });
            finish(&args.report, json, false, cx);
            return;
        }
    };
    let own_token = matches!(credential, Credential::InviteCode(_));
    let login_at = Instant::now();
    provider.login(LoginRequest {
        server_url: args.url.clone(),
        credential,
        device_name: "selftest".into(),
    });
    let report = args.report.clone();
    let interact = args.interact.clone();
    cx.spawn(async move |cx: &mut AsyncApp| {
        if let Some(shots) = interact {
            let result =
                super::selftest_interact::drive(&provider, &services, &root, &shots, cx).await;
            let (ok, json) = match result {
                Ok(json) => (true, json),
                Err((step, mut json)) => {
                    json["ok"] = false.into();
                    json["error"] = format!("timed out or failed at: {step}").into();
                    (false, json)
                }
            };
            cx.update(|cx| {
                provider.sign_out_local_for_selftest();
                finish(&report, json, ok, cx);
            });
            let _ = std::fs::remove_dir_all(&data_root);
            return;
        }
        let mut result = drive(&provider, &services, &root, login_at, started, cx).await;
        if let Ok(json) = result {
            provider.sign_out_local_for_selftest();
            let config = ProviderConfig {
                data_root: data_root.clone(),
                tokens: tokens.clone(),
                restore: true,
            };
            result = restore_and_sign_out(config, tokens, own_token, json, cx).await;
        }
        let (ok, json) = match result {
            Ok(json) => (true, json),
            Err((step, mut json)) => {
                json["ok"] = false.into();
                json["error"] = format!("timed out or failed at: {step}").into();
                (false, json)
            }
        };
        cx.update(|cx| {
            provider.sign_out_local_for_selftest();
            finish(&report, json, ok, cx);
        });
        let _ = std::fs::remove_dir_all(&data_root);
    })
    .detach();
}

/// Polls `check` until it gives a value or the deadline passes.
async fn wait_for<T>(
    cx: &mut AsyncApp,
    deadline: Instant,
    mut check: impl FnMut(&mut AsyncApp) -> Option<T>,
) -> Option<T> {
    loop {
        if let Some(v) = check(cx) {
            return Some(v);
        }
        if Instant::now() > deadline {
            return None;
        }
        cx.background_executor().timer(POLL).await;
    }
}

type Step = (&'static str, serde_json::Value);

/// Signed in, online, and every group caught up (its cache has applied events).
async fn drive_read_only(
    provider: &Arc<CoreProvider>,
    login_at: Instant,
    started: Instant,
    cx: &mut AsyncApp,
) -> Result<serde_json::Value, Step> {
    let deadline = Instant::now() + TIMEOUT;
    let ms = |t: Instant| (t.elapsed().as_secs_f64() * 1000.0).round();
    let mut json = serde_json::json!({ "ok": false, "read_only": true });
    wait_for(cx, deadline, |_| {
        let signed_in = matches!(provider.session(), SessionState::SignedIn { .. });
        (signed_in && provider.connection() == ConnectionState::Online).then_some(())
    })
    .await
    .ok_or(("sign in and connect", json.clone()))?;
    json["online_ms"] = ms(login_at).into();
    let client = provider.client().ok_or(("client", json.clone()))?;
    let synced = wait_for(cx, deadline, |_| {
        let groups = provider.groups();
        if groups.is_empty() {
            return None;
        }
        let seqs: Option<Vec<u64>> = groups
            .iter()
            .map(|g| client.store().last_applied(&g.id).ok().filter(|s| *s > 0))
            .collect();
        seqs.map(|s| (groups.len(), s.iter().sum::<u64>()))
    })
    .await
    .ok_or(("catch-up of every group", json.clone()))?;
    json["synced_ms"] = ms(login_at).into();
    json["groups"] = synced.0.into();
    json["applied_events"] = synced.1.into();
    json["members"] = provider.members().len().into();
    json["total_ms"] = ms(started).into();
    json["ok"] = true.into();
    Ok(json)
}

async fn drive(
    provider: &Arc<CoreProvider>,
    services: &Services,
    root: &gpui::Entity<app::RootView>,
    login_at: Instant,
    started: Instant,
    cx: &mut AsyncApp,
) -> Result<serde_json::Value, Step> {
    let deadline = Instant::now() + TIMEOUT;
    let t0 = login_at;
    let ms = |t: Instant| (t.elapsed().as_secs_f64() * 1000.0).round();
    let mut json = serde_json::json!({ "ok": false });

    // 1. Signed in, online, with a group.
    let group = wait_for(cx, deadline, |_| {
        let signed_in = matches!(provider.session(), SessionState::SignedIn { .. });
        let online = provider.connection() == ConnectionState::Online;
        if !(signed_in && online) {
            return None;
        }
        provider.groups().into_iter().next()
    })
    .await
    .ok_or(("sign in and first group", json.clone()))?;
    json["signed_in_ms"] = ms(t0).into();
    json["groups"] = provider.groups().len().into();
    json["group_id"] = group.id.clone().into();
    let gid = group.id.clone();
    cx.update(|cx| services.nav.update(cx, |nav, cx| nav.open_group(gid, cx)));

    // 2. Start a topic: it shows at once as a pending topic.
    let stamp = chat_core::new_id();
    let t_topic = Instant::now();
    let cid1 = provider.start_topic(StartTopicRequest {
        group_id: group.id.clone(),
        title: None,
        body: format!("selftest-live topic {stamp}"),
        mentions: Vec::new(),
    });
    if cid1.is_empty() {
        return Err(("start topic", json));
    }
    let pending = pending_topic_id(&cid1);
    let shown_pending = provider
        .topics(&group.id)
        .first()
        .is_some_and(|t| t.id == pending);
    json["pending_topic_shown"] = shown_pending.into();
    let (g, p) = (group.id.clone(), pending.clone());
    cx.update(|cx| services.nav.update(cx, |nav, cx| nav.open_topic(g, p, cx)));
    wait_for(cx, deadline, |_| provider.acked_message(&cid1))
        .await
        .ok_or(("topic ack", json.clone()))?;
    let topic_id = provider.resolve_topic(&pending);
    json["topic_ack_ms"] = ms(t_topic).into();
    json["topic_id"] = topic_id.clone().into();
    let alias_works = provider
        .messages(&group.id, &pending, 50)
        .messages
        .iter()
        .any(|m| m.client_msg_id == cid1 && m.send_state == SendState::Sent);
    json["pending_alias_resolves"] = alias_works.into();

    // 3. Send a message into the topic: pending at once, then acked, then its event.
    let t_send = Instant::now();
    let body = format!("selftest-live message {stamp}");
    let cid2 = provider.send(SendRequest {
        group_id: group.id.clone(),
        topic_id: topic_id.clone(),
        body: body.clone(),
        mentions: Vec::new(),
        quote_of: None,
    });
    let shown_pending_message = provider
        .messages(&group.id, &topic_id, 50)
        .messages
        .iter()
        .any(|m| m.client_msg_id == cid2 && m.send_state == SendState::Pending);
    json["pending_message_shown"] = shown_pending_message.into();
    let message_id = wait_for(cx, deadline, |_| provider.acked_message(&cid2))
        .await
        .ok_or(("message ack", json.clone()))?;
    json["ack_ms"] = ms(t_send).into();
    json["message_id"] = message_id.clone().into();
    let client = provider.client().ok_or(("client", json.clone()))?;
    let seq = wait_for(cx, deadline, |_| {
        let m = client.store().message(&message_id).ok().flatten()?;
        let applied = client.store().last_applied(&group.id).ok()?;
        (m.created_seq > 0 && applied >= m.created_seq && m.body == body).then_some(m.created_seq)
    })
    .await
    .ok_or(("message event", json.clone()))?;
    json["event_ms"] = ms(t_send).into();
    json["created_seq"] = seq.into();

    // 4. The open timeline shows it, and marking read works.
    let (g, t) = (group.id.clone(), topic_id.clone());
    cx.update(|cx| services.nav.update(cx, |nav, cx| nav.open_topic(g, t, cx)));
    let shown = wait_for(cx, deadline, |cx| {
        let n = cx.update(|cx| root.read(cx).timeline().read(cx).message_count());
        (n >= 2).then_some(n)
    })
    .await
    .ok_or(("timeline shows the messages", json.clone()))?;
    json["messages_in_timeline"] = shown.into();
    provider.mark_read(&group.id, &topic_id, seq);
    json["total_ms"] = (started.elapsed().as_secs_f64() * 1000.0).round().into();
    let ok = shown_pending && alias_works && shown_pending_message;
    json["ok"] = ok.into();
    if ok {
        Ok(json)
    } else {
        Err(("pending states", json))
    }
}

/// A second provider restores the session from `session.json`, the cache
/// and the token store: signed in at once with cached groups, then online.
/// With its own token, signs out with "wipe" and checks the revoke.
async fn restore_and_sign_out(
    config: ProviderConfig,
    tokens: TokenStorage,
    own_token: bool,
    mut json: serde_json::Value,
    cx: &mut AsyncApp,
) -> Result<serde_json::Value, Step> {
    let deadline = Instant::now() + TIMEOUT;
    let t = Instant::now();
    let root = config.data_root.clone();
    let p2 = CoreProvider::new(config);
    let signed_in = matches!(p2.session(), SessionState::SignedIn { .. });
    let cached_groups = p2.groups().len();
    json["restore_signed_in_at_once"] = signed_in.into();
    json["restore_cached_groups"] = cached_groups.into();
    if !signed_in || cached_groups == 0 {
        return Err(("restore from cache", json));
    }
    wait_for(cx, deadline, |_| {
        (p2.connection() == ConnectionState::Online).then_some(())
    })
    .await
    .ok_or(("restore online", json.clone()))?;
    json["restore_online_ms"] = (t.elapsed().as_secs_f64() * 1000.0).round().into();
    if !own_token {
        p2.sign_out_local_for_selftest();
        json["sign_out_checked"] = false.into();
        return Ok(json);
    }
    let TokenStorage::Memory(slot) = &tokens else {
        return Err(("token storage", json));
    };
    let token = slot.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let (Some(token), Some(client)) = (token, p2.client()) else {
        return Err(("saved token", json));
    };
    let base = client.config().base_url.clone();
    let account = client.config().data_dir.clone();
    drop(client);
    p2.sign_out(true);
    let signed_out = matches!(p2.session(), SessionState::SignedOut { .. });
    wait_for(cx, deadline, |_| {
        (!account.exists() && !super::session::session_path(&root).exists()).then_some(())
    })
    .await
    .ok_or(("sign out wipes the cache", json.clone()))?;
    let check = chat_core::background().spawn(async move {
        let http = chat_core::HttpClient::new(&base, Some(token)).ok()?;
        Some(matches!(http.me().await, Err(e) if e.is_unauthorized()))
    });
    let revoked = check.await.ok().flatten().unwrap_or(false);
    let cleared = slot.lock().unwrap_or_else(|e| e.into_inner()).is_none();
    json["sign_out_checked"] = true.into();
    json["sign_out_revoked"] = revoked.into();
    json["sign_out_token_cleared"] = cleared.into();
    if signed_out && revoked && cleared {
        Ok(json)
    } else {
        Err(("sign out", json))
    }
}

impl CoreProvider {
    /// Ends the self-test session without revoking the token (it belongs to
    /// the caller).
    pub fn sign_out_local_for_selftest(&self) {
        let _ = self.take_account(None);
    }
}
