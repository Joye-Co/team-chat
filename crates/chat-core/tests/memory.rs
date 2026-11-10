//! 沉淀 (memory) over HTTP against the in-process mock: list, create, edit, 停用, versions,
//! restore, a day's 记忆更新 and undoing its lines.

mod common;
mod mock;
mod mock_http;

use chat_core::proto::*;
use chat_core::{ApiError, Client, ClientConfig, MemoryStatusFilter};
use mock_http::{MockHttp, TOKEN};

fn item(id: &str, scope: &str, kind: MemoryKind, status: MemoryStatus) -> MemoryItem {
    MemoryItem {
        id: id.into(),
        scope: scope.into(),
        kind,
        text: format!("text of {id}"),
        when: None,
        source: MemorySource {
            kind: MemorySourceKind::Run,
            ids: vec!["R1".into()],
        },
        note: None,
        author_id: "b1".into(),
        human: false,
        status,
        confidence: 0.6,
        uses: 2,
        last_used_at: Some(3),
        version: 1,
        created_at: 1,
        updated_at: 2,
    }
}

fn update(id: &str, scope: &str, change: MemoryChange, after: &MemoryItem) -> MemoryUpdate {
    MemoryUpdate {
        id: id.into(),
        day: "2026-11-08".into(),
        scope: scope.into(),
        memory_id: after.id.clone(),
        change,
        before: None,
        after: Some(after.clone()),
        note: None,
        undone_at: None,
        at: 4,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn memories_updates_and_undo_over_http() {
    let http = MockHttp::start().await;
    let a = item("MA", "member:b1", MemoryKind::Lesson, MemoryStatus::Active);
    let r = item(
        "MR",
        "member:b1",
        MemoryKind::Playbook,
        MemoryStatus::Retired,
    );
    let t = item("MT", "team", MemoryKind::Convention, MemoryStatus::Active);
    http.with(|s| {
        s.memories = vec![a.clone(), r.clone(), t.clone()];
        s.memory_updates = vec![
            update("U1", "member:b1", MemoryChange::Added, &a),
            update("U2", "member:b1", MemoryChange::Flagged, &a),
        ];
    });
    let dir = tempfile::tempdir().unwrap();
    let client = Client::new(ClientConfig::new(http.base_url(), TOKEN, dir.path())).unwrap();
    let api = client.http();

    // The scope and status go in the query, encoded.
    let active = api
        .memories("member:b1", MemoryStatusFilter::Active, Some("先发"), 900)
        .await
        .unwrap();
    assert_eq!(active.memories, vec![a.clone()]);
    let all = api
        .memories("member:b1", MemoryStatusFilter::All, None, 200)
        .await
        .unwrap();
    assert_eq!(all.memories.len(), 2);
    let sent = http.with(|s| s.memory_queries[0].clone());
    assert!(sent.contains("scope=member%3Ab1"), "{sent}");
    assert!(sent.contains("limit=500"), "{sent}");
    assert!(sent.contains("q=%E5%85%88%E5%8F%91"), "{sent}");

    // A bad scope never leaves the client.
    let before = http.with(|s| s.requests.len());
    assert!(matches!(
        api.memories("everyone", MemoryStatusFilter::Active, None, 10)
            .await,
        Err(ApiError::Invalid(_))
    ));
    assert!(matches!(
        api.memory_updates("member:", None).await,
        Err(ApiError::Invalid(_))
    ));
    // Nor does an empty text.
    let empty = CreateMemoryRequest {
        scope: "team".into(),
        kind: MemoryKind::Convention,
        text: "  ".into(),
        when: None,
        source: None,
    };
    assert!(matches!(
        api.create_memory(&empty).await,
        Err(ApiError::Invalid(_))
    ));
    assert_eq!(http.with(|s| s.requests.len()), before);

    // 记一条: a person's memory is active at once.
    let made = api
        .create_memory(&CreateMemoryRequest {
            scope: "team".into(),
            kind: MemoryKind::Convention,
            text: " PR 标题用英文 ".into(),
            when: Some("开 PR 时".into()),
            source: None,
        })
        .await
        .unwrap();
    assert_eq!(made.text, "PR 标题用英文");
    assert_eq!(made.status, MemoryStatus::Active);
    assert!(made.human);
    assert_eq!(api.memory(&made.id).await.unwrap(), made);

    // Edit, then 停用: each a new version.
    let edited = api
        .update_memory(
            "MA",
            &UpdateMemoryRequest {
                text: Some("改过的经验".into()),
                when: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!((edited.text.as_str(), edited.version), ("改过的经验", 2));
    let retired = api
        .update_memory(
            "MA",
            &UpdateMemoryRequest {
                status: Some(MemoryStatus::Retired),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(retired.status, MemoryStatus::Retired);
    // A person never makes a memory pending.
    assert!(matches!(
        api.update_memory(
            "MA",
            &UpdateMemoryRequest {
                status: Some(MemoryStatus::Pending),
                ..Default::default()
            },
        )
        .await,
        Err(ApiError::Invalid(_))
    ));
    let versions = api.memory_versions("MA").await.unwrap();
    assert_eq!(versions.versions[0].version, 3);
    assert_eq!(
        api.restore_memory_version("MA", 1).await.unwrap().version,
        4
    );

    // The day's lines; undo once, a second undo and a flagged line are refused.
    let day = api.memory_updates("member:b1", None).await.unwrap();
    assert_eq!(day.day.as_deref(), Some("2026-11-08"));
    assert_eq!(day.updates.len(), 2);
    let undone = api.undo_memory_update("U1").await.unwrap();
    assert!(undone.undone_at.is_some());
    assert!(matches!(
        api.undo_memory_update("U1").await,
        Err(ApiError::Server {
            code: ErrorCode::Conflict,
            ..
        })
    ));
    assert!(matches!(
        api.undo_memory_update("U2").await,
        Err(ApiError::Server {
            code: ErrorCode::Invalid,
            ..
        })
    ));
    assert!(
        http.with(|s| s.count("POST", "/v1/memories/updates/U1/undo")) == 2,
        "both undos reached the server"
    );
}
