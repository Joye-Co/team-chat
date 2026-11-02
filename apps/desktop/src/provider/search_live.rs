//! Live check of search and the process view through the chat-core provider
//! against a real server with a runner-backed bot and a connected machine.
//! `apps/desktop/e2e/search-process-e2e.ts` sets that up and runs:
//!
//! ```sh
//! TC_LIVE_URL=http://127.0.0.1:8804 TC_LIVE_TOKEN=<owner token file> \
//! TC_LIVE_BOT=<bot member id> TC_LIVE_OUT=<folder> \
//! cargo test -p desktop search_live -- --ignored --nocapture
//! ```
//!
//! The owner mentions the bot in a new 话题 of a new 群; the test waits for the
//! run (`Change::Runs` data), follows its log to the end (steps, raw entries),
//! downloads the raw session, then searches a Chinese two-character word in
//! the 群 and in all 群 and checks the hit and its snippet highlight.

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use super::{CoreProvider, ProviderConfig, session::TokenStorage};
use crate::data::{
    ChatProvider, ConnectionState, CreateGroupRequest, Credential, LoginRequest, ProcessStep,
    RunStatus, SearchQuery, SessionState, StartTopicRequest,
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
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
#[ignore = "needs a server: run apps/desktop/e2e/search-process-e2e.ts"]
fn search_live() {
    let (Ok(url), Ok(token_file), Ok(bot), Ok(out)) = (
        std::env::var("TC_LIVE_URL"),
        std::env::var("TC_LIVE_TOKEN"),
        std::env::var("TC_LIVE_BOT"),
        std::env::var("TC_LIVE_OUT"),
    ) else {
        panic!("set TC_LIVE_URL, TC_LIVE_TOKEN, TC_LIVE_BOT and TC_LIVE_OUT");
    };
    let token = std::fs::read_to_string(&token_file).expect("token file");
    let root = PathBuf::from(&out).join("desktop-cache");
    let p = CoreProvider::new(ProviderConfig {
        data_root: root,
        tokens: TokenStorage::memory(),
        restore: false,
    });
    p.login(LoginRequest {
        server_url: url,
        credential: Credential::Token(token.trim().to_string()),
        device_name: "search-live".into(),
    });
    wait("signed in", || {
        matches!(p.session(), SessionState::SignedIn { .. }).then_some(())
    });
    wait("online", || {
        (p.connection() == ConnectionState::Online).then_some(())
    });

    let name = format!("search-{}", chat_core::now_ms());
    p.create_group(CreateGroupRequest {
        name: name.clone(),
        member_ids: vec![bot.clone()],
    });
    let g = wait("group made", || {
        p.groups()
            .into_iter()
            .find(|g| g.name == name)
            .map(|g| g.id)
    });
    let bot_name = p.member(&bot).map(|m| m.name).unwrap_or_default();
    let body = format!("@{bot_name} 请检查部署流水线 slow");
    let cid = p.start_topic(StartTopicRequest {
        group_id: g.clone(),
        title: Some("检查部署流水线".into()),
        body,
        mentions: vec![bot.clone()],
    });
    let root_id = wait("topic acked", || p.acked_message(&cid));
    let t = wait("topic listed", || {
        p.topics(&g)
            .into_iter()
            .find(|t| t.root_message_id == root_id)
            .map(|t| t.id)
    });

    // The run appears from run.updated events, then ends.
    let run = wait("run listed", || p.runs(&g, &t).into_iter().next());
    assert_eq!(run.bot_id, bot);
    eprintln!("run {} status {:?}", run.id, run.status);
    let run = wait("run done", || {
        p.runs(&g, &t)
            .into_iter()
            .find(|r| r.id == run.id && !r.status.is_active())
    });
    assert_eq!(run.status, RunStatus::Done, "run ended {:?}", run.status);
    assert!(run.machine_id.is_some());
    wait("machine name", || p.runs(&g, &t)[0].machine_name.clone());

    // The log, folded into steps, until the done step.
    let process = wait("process has the done step", || {
        p.follow_run(&run.id);
        std::thread::sleep(Duration::from_millis(300));
        let process = p.run_process(&run.id);
        assert_eq!(process.error, None);
        process
            .steps
            .iter()
            .any(|s| matches!(s, ProcessStep::Done { .. }))
            .then_some(process)
    });
    let kinds: Vec<&str> = process
        .steps
        .iter()
        .map(|s| match s {
            ProcessStep::Message { .. } => "message",
            ProcessStep::Reasoning { .. } => "reasoning",
            ProcessStep::Tool { .. } => "tool",
            ProcessStep::Files { .. } => "files",
            ProcessStep::Approval { .. } => "approval",
            ProcessStep::TurnStarted => "turn-started",
            ProcessStep::TurnCompleted { .. } => "turn-completed",
            ProcessStep::Status { .. } => "status",
            ProcessStep::Done { .. } => "done",
        })
        .collect();
    eprintln!("steps: {kinds:?}; raw entries {}", process.raw.len());
    assert!(kinds.contains(&"message"), "{kinds:?}");
    assert!(!process.raw.is_empty());
    assert!(process.last_seq > 0);
    // Following again adds nothing.
    let before = process.steps.len();
    p.follow_run(&run.id);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(p.run_process(&run.id).steps.len(), before);

    // The raw session (owner only), saved as gzip.
    let session = wait("raw session saved", || {
        let reply = p.download_run_session(&run.id, PathBuf::from(&out));
        futures::executor::block_on(reply).ok().and_then(|r| r.ok())
    });
    let bytes = std::fs::read(&session).unwrap();
    assert_eq!(&bytes[..2], &[0x1f, 0x8b], "gzip");

    // Search a Chinese two-character word in the group and in all groups.
    for group_id in [Some(g.clone()), None] {
        let hits = wait("search hit", || {
            let reply = p.search(SearchQuery {
                text: "流水".into(),
                group_id: group_id.clone(),
                limit: 20,
            });
            let hits = futures::executor::block_on(reply).ok()?.ok()?;
            hits.into_iter()
                .find(|h| h.message_id == root_id)
                .map(|h| (h, ()))
        });
        let (hit, ()) = hits;
        assert_eq!(hit.topic_id, t);
        assert_eq!(hit.group_id, g);
        assert!(!hit.highlights.is_empty());
        let marked: String = hit
            .snippet
            .chars()
            .skip(hit.highlights[0].start)
            .take(hit.highlights[0].len())
            .collect();
        assert!(
            marked.contains("流水"),
            "highlight {marked:?} in {:?}",
            hit.snippet
        );
        eprintln!("hit: {:?}", hit.snippet);
    }
    let none = futures::executor::block_on(p.search(SearchQuery {
        text: "不存在的词语组合".into(),
        group_id: Some(g.clone()),
        limit: 20,
    }))
    .unwrap()
    .unwrap();
    assert!(none.is_empty());
}
