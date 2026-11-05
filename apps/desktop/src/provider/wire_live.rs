//! Live check of the backend data the 首页, 成员, 群 and 产出 pages read through the chat-core
//! provider (v13/wire), against `wrangler dev` with a connected machine.
//! `apps/desktop/e2e/wire-e2e.ts` sets up the owner, a runner-backed bot `Jojo` (one run at a
//! time on the machine), an SDK bot `Pixel` and the machine, then runs:
//!
//! ```sh
//! TC_LIVE_URL=http://127.0.0.1:8806 TC_LIVE_TOKEN=<owner token file> \
//! TC_LIVE_BOT=<Jojo id> TC_LIVE_BOT_TOKEN=<Jojo token file> \
//! TC_LIVE_PAGE_BOT=<Pixel id> TC_LIVE_PAGE_BOT_TOKEN=<Pixel token file> TC_LIVE_OUT=<folder> \
//! cargo test -p desktop wire_live -- --ignored --nocapture
//! ```
//!
//! Each step calls the provider the app uses and waits for the page getter to show the server's
//! data: 群说明 (`PATCH /v1/groups/:id`), 记一条 (`POST` + `GET /v1/groups/:id/decisions`),
//! Jojo's desk note (`PUT /v1/desk-note`, synced `desk.noted`), 汇报给 and 限额
//! (`PATCH /v1/members/:id`), 需要你批准的事 (`GET` + `PATCH /v1/settings/approvals`), 排队
//! (`queuedAt` of a run that waits for the busy machine), 重试 (`POST /v1/runs/:id/retry`) and
//! 页面里的回应 (`GET /v1/artifacts/:id/events`, read by a second provider with an empty cache).
//! It writes `wire-live.json` with what each step saw.

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use chat_core::{HttpClient, UploadRequest, no_progress, proto};

use super::{CoreProvider, ProviderConfig, session::TokenStorage};
use crate::data::{
    ApprovalDefaults, ApproverRole, ChatProvider, ConnectionState, CreateGroupRequest, Credential,
    LoginRequest, RunStatus, SessionState, StartTopicRequest,
    pages::{MemberPlan, ResponsesLoad},
};

const TIMEOUT: Duration = Duration::from_secs(90);

fn wait<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(v) = check() {
            eprintln!("ok: {what}");
            return v;
        }
        assert!(Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("set {name}"))
}

fn secret(name: &str) -> String {
    std::fs::read_to_string(env(name))
        .expect("token file")
        .trim()
        .to_string()
}

/// Runs an HTTP call or waits for a provider reply (the chat-core runtime drives both).
fn block<T>(f: impl std::future::Future<Output = T>) -> T {
    chat_core::background().block_on(f)
}

/// A provider signed in with `token`, its cache under `out/<name>`.
fn signed_in(url: &str, token: &str, out: &str, name: &str) -> std::sync::Arc<CoreProvider> {
    let p = CoreProvider::new(ProviderConfig {
        data_root: PathBuf::from(out).join(name),
        tokens: TokenStorage::memory(),
        restore: false,
    });
    p.login(LoginRequest {
        server_url: url.to_string(),
        credential: Credential::Token(token.to_string()),
        device_name: format!("wire-live-{name}"),
    });
    wait(&format!("{name} signed in"), || {
        matches!(p.session(), SessionState::SignedIn { .. }).then_some(())
    });
    wait(&format!("{name} online"), || {
        (p.connection() == ConnectionState::Online).then_some(())
    });
    p
}

/// Starts a 话题 that mentions Jojo and waits for its run.
fn mention(p: &CoreProvider, g: &str, jojo: &str, title: &str, body: &str) -> (String, String) {
    p.start_topic(StartTopicRequest {
        quote_of: None,
        group_id: g.to_string(),
        title: Some(title.to_string()),
        body: format!("@Jojo {body}"),
        mentions: vec![jojo.to_string()],
    });
    let t = wait(&format!("topic {title}"), || {
        p.topics(g)
            .into_iter()
            .find(|t| t.title == title && !t.id.starts_with("pending:"))
            .map(|t| t.id)
    });
    let run = wait(&format!("run of {title}"), || {
        p.runs(g, &t).into_iter().next().map(|r| r.id)
    });
    (t, run)
}

#[test]
#[ignore = "needs a server: run apps/desktop/e2e/wire-e2e.ts"]
fn wire_live() {
    let url = env("TC_LIVE_URL");
    let out = env("TC_LIVE_OUT");
    let owner_token = secret("TC_LIVE_TOKEN");
    let jojo = env("TC_LIVE_BOT");
    let jojo_http = HttpClient::new(&url, Some(secret("TC_LIVE_BOT_TOKEN"))).expect("bot client");
    let pixel = env("TC_LIVE_PAGE_BOT");
    let pixel_http =
        HttpClient::new(&url, Some(secret("TC_LIVE_PAGE_BOT_TOKEN"))).expect("page bot client");
    let mut report = serde_json::Map::new();

    let p = signed_in(&url, &owner_token, &out, "owner");
    let me = match p.session() {
        SessionState::SignedIn { me, .. } => me.id,
        _ => unreachable!(),
    };
    let name = format!("wire-{}", chat_core::now_ms());
    drop(p.create_group(CreateGroupRequest {
        name: name.clone(),
        member_ids: vec![jojo.clone(), pixel.clone()],
    }));
    let g = wait("group made", || {
        p.groups()
            .into_iter()
            .find(|g| g.name == name)
            .map(|g| g.id)
    });

    // 群说明: PATCH /v1/groups/:id with purpose, shown under the 群's name.
    let purpose = "上线、巡检和告警";
    block(p.set_group_purpose(&g, "  上线、巡检和告警  ".to_string()))
        .expect("reply")
        .expect("purpose stored");
    wait("group purpose shown", || {
        (p.group_purpose(&g).as_deref() == Some(purpose)).then_some(())
    });
    report.insert("groupPurpose".into(), purpose.into());

    // 记一条: POST /v1/groups/:id/decisions, then the page reads GET …/decisions.
    let decision = "先发 staging，再发生产";
    block(p.record_decision(&g, decision.into()))
        .expect("reply")
        .expect("decision recorded");
    let d = wait("recorded decision listed", || {
        p.group_decisions(&g)
            .into_iter()
            .find(|d| d.text == decision && d.id.starts_with("recorded-"))
    });
    assert_eq!(d.by, me);
    report.insert("recordedDecision".into(), d.id.clone().into());

    // Jojo's desk note: PUT /v1/desk-note by the bot, synced as `desk.noted`.
    let note = "今天先把 1.1 发出去";
    block(jojo_http.put_desk_note(&g, note)).expect("desk note");
    let n = wait("desk note on 首页", || {
        p.desk_note().filter(|n| n.text == note)
    });
    assert_eq!(n.member_id, jojo);
    report.insert("deskNote".into(), n.text.into());

    // 汇报给 and 限额: PATCH /v1/members/:id, shown on the 成员 page.
    let plan = MemberPlan {
        reports_to: Some(me.clone()),
        weekly_budget_cents: Some(30_000),
    };
    block(p.set_member_plan(&jojo, plan))
        .expect("reply")
        .expect("plan stored");
    wait("reportsTo and weekly limit shown", || {
        let stats = p.member_stats(&jojo)?;
        let extras = p.member_extras(&jojo);
        (stats.reports_to.as_deref() == Some(me.as_str()) && extras.budget_cents == Some(30_000))
            .then_some(())
    });
    block(p.set_member_plan(&jojo, MemberPlan::default()))
        .expect("reply")
        .expect("plan cleared");
    wait("reportsTo and weekly limit cleared", || {
        let stats = p.member_stats(&jojo)?;
        (stats.reports_to.is_none() && p.member_extras(&jojo).budget_cents.is_none()).then_some(())
    });
    report.insert("memberPlan".into(), "set 30000, cleared".into());

    // 需要你批准的事: GET /v1/settings/approvals; an owner's change comes back.
    let before = block(p.approval_defaults()).expect("reply").expect("rules");
    let changed = ApprovalDefaults {
        delete_data: false,
        max_spend_cny: Some(80),
        approver: ApproverRole::Owners,
        ..before
    };
    let stored = block(p.set_approval_defaults(changed))
        .expect("reply")
        .expect("rules saved");
    assert_eq!(stored, changed);
    let again = block(p.approval_defaults()).expect("reply").expect("rules");
    assert_eq!(again, changed);
    report.insert(
        "approvals".into(),
        format!("{:?}", crate::views::members_page::approval_items(again)).into(),
    );

    // 排队: the machine takes one run; a second mention waits with `queuedAt`.
    let (t_busy, busy) = mention(&p, &g, &jojo, "占着电脑", "hang");
    wait("first run on the machine", || {
        p.runs(&g, &t_busy)
            .into_iter()
            .find(|r| r.id == busy && r.status == RunStatus::Running)
    });
    let (t_wait, queued) = mention(&p, &g, &jojo, "排队的活", "回一句好");
    let q = wait("queued run on 首页 with its time", || {
        p.queue()
            .into_iter()
            .find(|q| q.member_id == jojo && q.title == "排队的活" && q.at.is_some())
    });
    let queued_work = wait("queued run on the member page", || {
        p.member_extras(&jojo)
            .queued
            .into_iter()
            .find(|w| w.topic == Some((g.clone(), t_wait.clone())) && w.queued_at.is_some())
    });
    assert_eq!(queued_work.queued_at, q.at);
    assert!(
        !p.active_runs().iter().any(|r| r.id == queued),
        "a queued run is not 正在做"
    );
    report.insert("queuedAt".into(), q.at.into());

    // 重试: stop the busy run, then POST /v1/runs/:id/retry makes a new run of the request.
    block(p.cancel_run(&g, &t_busy, &busy))
        .expect("reply")
        .expect("stopped");
    wait("busy run cancelled", || {
        p.runs(&g, &t_busy)
            .into_iter()
            .find(|r| r.id == busy && r.status == RunStatus::Cancelled)
    });
    wait("queued run left the queue", || {
        (!p.queue().iter().any(|x| x.title == "排队的活")).then_some(())
    });
    block(p.retry_member_problem(&jojo, &busy))
        .expect("reply")
        .expect("retried");
    let a = p.account().expect("account");
    let retry = wait("the retry run, linked by retryOf", || {
        a.client
            .runs_since(0)
            .ok()?
            .into_iter()
            .find(|r| r.retry_of.as_deref() == Some(busy.as_str()))
    });
    assert_eq!(retry.topic_id, t_busy);
    report.insert("retryRun".into(), retry.id.clone().into());
    // The retry hangs like the first run: stop it so the machine is free.
    let _ = block(p.cancel_run(&g, &t_busy, &retry.id));

    // 页面里的回应: Pixel's page declares `pick`; my event goes to the page log, and a second
    // computer (an empty cache) reads it from GET /v1/artifacts/:id/events.
    let page = PathBuf::from(&out).join("page.html");
    std::fs::write(&page, "<!doctype html><title>看板</title><p>看板</p>").expect("page");
    let file = block(chat_core::files::upload_file(
        &pixel_http,
        &UploadRequest {
            group_id: g.clone(),
            topic_id: Some(t_wait.clone()),
            path: page,
            name: Some("index.html".into()),
            mime: Some("text/html".into()),
        },
        &no_progress(),
    ))
    .expect("page uploaded");
    let made = block(pixel_http.create_artifact(&proto::CreateArtifactRequest {
        group_id: g.clone(),
        topic_id: t_wait.clone(),
        title: "看板".into(),
        kind: proto::ArtifactKind::Html,
        entry: "index.html".into(),
        files: vec![proto::VersionFileInput {
            path: "index.html".into(),
            file_id: file.id,
        }],
        note: None,
        events: Some(vec![proto::ArtifactEventDecl {
            name: "pick".into(),
            label: Some("选了".into()),
            kind: None,
            schema: None,
        }]),
        media: None,
        data: None,
        run_id: None,
        preview: None,
    }))
    .expect("artifact made");
    let art = made.artifact.id.clone();
    wait("产出 synced with its events", || {
        (p.artifact_page_events(&art, 1) == vec!["pick".to_string()]).then_some(())
    });
    p.send_artifact_event(&art, 1, "pick", Some(serde_json::json!({ "label": "A" })));
    let second = signed_in(&url, &owner_token, &out, "second");
    let shown = wait("页面里的回应 from the server's page log", || {
        let list = second.artifact_responses(&art);
        (second.artifact_responses_load(&art) == ResponsesLoad::Ready && !list.is_empty())
            .then_some(list)
    });
    assert_eq!(shown[0].member_id, me);
    assert!(shown[0].text.contains('A'), "{}", shown[0].text);
    report.insert("pageResponse".into(), shown[0].text.clone().into());

    std::fs::write(
        PathBuf::from(&out).join("wire-live.json"),
        serde_json::to_string_pretty(&report).expect("json"),
    )
    .expect("report");
    eprintln!("wire_live: {}", serde_json::Value::Object(report));
}
