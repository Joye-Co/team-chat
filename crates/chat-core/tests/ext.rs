//! V1 client extensions: 待办, 产出 events, card presses, notifications, files,
//! 产出 folders, members admin, prefs, avatars, audit, hooks, updates and diagnostics,
//! against the in-process mocks.

mod common;
mod mock;
mod mock_http;

use chat_core::proto::machine::{RunEvent, RunLogEntry};
use chat_core::proto::*;
use chat_core::{
    ApiError, Client, ClientConfig, ClientError, ClientUpdate, LocalArtifactFile, NewVersion,
    Phase, Progress, ProgressFn, Store, TodoReason, UploadRequest, no_progress,
};
use common::*;
use mock_http::{MockHttp, TOKEN, sha};
use serde_json::json;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedReceiver;

const WAIT: Duration = Duration::from_secs(10);

fn store_with_group(dir: &Path) -> Store {
    let store = Store::open(&dir.join("cache.sqlite3")).unwrap();
    store
        .apply_welcome(
            &member(ME, "me"),
            &[member(ME, "me"), member(ALICE, "alice")],
            &[summary(group(G1, &[ME, ALICE]), 0, 0, 0)],
        )
        .unwrap();
    store
}

fn recorder() -> (ProgressFn, Arc<Mutex<Vec<Progress>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    (Arc::new(move |p| s.lock().unwrap().push(p)), seen)
}

fn http_client(http: &MockHttp, dir: &Path, tweak: impl FnOnce(&mut ClientConfig)) -> Client {
    let mut cfg = ClientConfig::new(http.base_url(), TOKEN, dir);
    tweak(&mut cfg);
    Client::new(cfg).unwrap()
}

/// A PNG header (enough for the size reader) followed by `pad` bytes.
fn png(w: u32, h: u32, pad: usize) -> Vec<u8> {
    let mut v = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
    v.extend_from_slice(&w.to_be_bytes());
    v.extend_from_slice(&h.to_be_bytes());
    v.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    v.extend((0..pad).map(|i| (i % 251) as u8));
    v
}

fn noise(n: usize, seed: u8) -> Vec<u8> {
    (0..n).map(|i| (i as u8).wrapping_mul(31) ^ seed).collect()
}

fn artifact(id: &str, topic: &str, v: u32) -> Artifact {
    Artifact {
        id: id.into(),
        group_id: G1.into(),
        topic_id: topic.into(),
        title: format!("page v{v}"),
        kind: ArtifactKind::Html,
        current_version: v,
        created_by: ALICE.into(),
        created_at: 1,
        updated_at: 1 + v as u64,
    }
}

fn version(id: &str, v: u32) -> ArtifactVersion {
    ArtifactVersion {
        events: None,
        run_id: None,
        media: None,
        data_rev: None,
        artifact_id: id.into(),
        version: v,
        entry: "index.html".into(),
        files: vec![ArtifactFile {
            path: "index.html".into(),
            file_id: format!("F{v}"),
            size: 3,
            mime: "text/html".into(),
        }],
        note: None,
        created_by: ALICE.into(),
        created_at: 1,
    }
}

#[test]
fn reaction_events_keep_the_message_reactions_in_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_with_group(dir.path());
    let react = |seq: u64, actor: &str, emoji: &str, on: bool| {
        ev(
            seq,
            G1,
            actor,
            EventData::ReactionChanged(ReactionChangedData {
                message_id: "M1".into(),
                emoji: emoji.into(),
                on,
            }),
        )
    };
    let events = vec![
        topic_created(1, G1, topic("T1", G1, "M1", 1)),
        msg_created(2, message("M1", G1, "T1", ALICE, 2, "ship?")),
        react(3, ALICE, "👍", true),
        react(4, ME, "👍", true),
        react(5, ME, "✅", true),
        react(6, ALICE, "👍", false),
        react(7, ME, "✅", false),
    ];
    let out = store.apply_events(G1, &events).unwrap();
    assert_eq!(out.applied, 7);
    assert!(out.message_topics.contains("T1"));
    let m = store.message("M1").unwrap().unwrap();
    assert_eq!(
        m.reactions(),
        &[Reaction {
            emoji: "👍".into(),
            member_ids: vec![ME.into()],
        }]
    );
}

#[test]
fn todo_unread_artifact_events_and_card_presses_in_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_with_group(dir.path());
    let mut waiting = topic("T1", G1, "M1", 1);
    waiting.status = TopicStatus::WaitingAnswer;
    waiting.waiting_on = Some(vec![ME.into()]);
    let mut done = topic("T4", G1, "M4", 4);
    done.status = TopicStatus::Done;
    done.waiting_on = Some(vec![ME.into()]);
    let mut mention = message("M2", G1, "T2", ALICE, 3, "@me please");
    mention.mentions = vec![ME.into()];
    let events = vec![
        topic_created(1, G1, waiting),
        topic_created(2, G1, topic("T2", G1, "M2", 2)),
        msg_created(3, mention),
        topic_created(4, G1, done),
        topic_created(5, G1, topic("T3", G1, "M3", 5)),
        msg_created(6, message("M3", G1, "T3", ALICE, 6, "plain")),
        ev(
            7,
            G1,
            ALICE,
            EventData::ArtifactCreated(ArtifactVersionResponse {
                artifact: artifact("A1", "T3", 1),
                version: version("A1", 1),
            }),
        ),
        ev(
            8,
            G1,
            ALICE,
            EventData::ArtifactVersionAdded(ArtifactVersionResponse {
                artifact: artifact("A1", "T3", 2),
                version: version("A1", 2),
            }),
        ),
        ev(
            9,
            G1,
            ME,
            EventData::CardActioned(CardActionedData {
                message_id: "M3".into(),
                action_id: "approve".into(),
                value: Some(json!({"ok": true})),
            }),
        ),
    ];
    let out = store.apply_events(G1, &events).unwrap();
    assert_eq!(out.applied, 9);
    assert_eq!(
        out.artifact_topics.iter().cloned().collect::<Vec<_>>(),
        vec!["T3".to_string()]
    );

    // 待办: T1 waits on me, T2 has an unread mention; T4 is done, T3 has no mention.
    let todo = store.todo().unwrap();
    let got: Vec<(&str, &[TodoReason])> = todo
        .iter()
        .map(|t| (t.topic.topic.id.as_str(), t.reasons.as_slice()))
        .collect();
    assert_eq!(
        got,
        vec![
            ("T2", &[TodoReason::Mentions][..]),
            ("T1", &[TodoReason::WaitingOnMe][..])
        ]
    );
    // Reading T2 removes it from 待办 and leaves T3 unread.
    store.mark_read(G1, Some("T2"), 3).unwrap();
    let todo = store.todo().unwrap();
    assert_eq!(todo.len(), 1);
    assert_eq!(todo[0].topic.topic.id, "T1");
    let t3 = store.topic("T3").unwrap().unwrap();
    assert_eq!((t3.unread, t3.mentions), (1, 0));

    // 产出 from events: newest version wins, versions kept.
    let list = store.artifacts("T3").unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].current_version, 2);
    let row = store.artifact("A1").unwrap().unwrap();
    assert_eq!(
        row.versions.iter().map(|v| v.version).collect::<Vec<_>>(),
        vec![2, 1]
    );
    // An older list answer does not replace the newer copy.
    store.store_artifacts(&[artifact("A1", "T3", 1)]).unwrap();
    assert_eq!(store.artifacts("T3").unwrap()[0].current_version, 2);

    let presses = store.card_actions("M3").unwrap();
    assert_eq!(presses.len(), 1);
    assert_eq!(
        (presses[0].actor_id.as_str(), presses[0].action_id.as_str()),
        (ME, "approve")
    );
    // Re-applying is a no-op (seq order, idempotent).
    assert_eq!(store.apply_events(G1, &events).unwrap().applied, 0);
    assert_eq!(store.card_actions("M3").unwrap().len(), 1);
}

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn notifications_skip_mine_and_muted_and_artifact_events_update_the_panel() {
    let server = mock::MockServer::start(
        member(ME, "me"),
        vec![member(ME, "me"), member(ALICE, "alice")],
    )
    .await;
    server.add_group(group(G1, &[ME, ALICE]));
    let tid = server.with(|s| {
        s.post(G1, ALICE, None, Some("plan"), "a0", "root", false)
            .1
            .unwrap()
            .id
    });
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = ClientConfig::new(server.base_url(), mock::TOKEN, dir.path());
    cfg.backoff_min = Duration::from_millis(50);
    let client = Client::new(cfg).unwrap();
    let mut rx = client.subscribe();
    client.start();
    next_update(&mut rx, "caught up", |u| {
        matches!(u, ClientUpdate::CaughtUp)
    })
    .await;

    let marked_as = |s: &mut mock::State, author: &str, cmid: &str, reason: &str| {
        let seq = s.groups[G1].log.len() as u64 + 1;
        s.hide_live.insert(seq);
        let (m, _) = s.post(G1, author, Some(&tid), None, cmid, "hello", false);
        let event = s.groups[G1].log.last().unwrap().clone();
        s.broadcast(
            &json!({"t": "event", "event": event, "notify": reason}),
            None,
        );
        m.id
    };
    let marked =
        |s: &mut mock::State, author: &str, cmid: &str| marked_as(s, author, cmid, "message");
    let muted = |s: &mut mock::State, muted: bool| {
        let mut g = summary(group(G1, &[ME, ALICE]), s.groups[G1].log.len() as u64, 0, 0);
        g.muted = Some(muted);
        s.broadcast(&json!({"t": "directory", "groups": [g]}), None);
    };
    server.with(|s| {
        marked(s, ME, "m1"); // my own: no notification
        muted(s, true);
    });
    next_update(&mut rx, "muted", |u| {
        matches!(u, ClientUpdate::GroupChanged { .. })
    })
    .await;
    // A muted group notifies only mentions (docs/protocol.md, Notifications).
    let mention = server.with(|s| {
        marked(s, ALICE, "a1"); // muted group: no notification
        marked_as(s, ALICE, "a1m", "mention")
    });
    let u = next_update(&mut rx, "mention", |u| matches!(u, ClientUpdate::Notify(_))).await;
    let ClientUpdate::Notify(n) = u else {
        unreachable!()
    };
    assert_eq!(
        (n.message_id.as_deref(), n.reason),
        (Some(mention.as_str()), NotifyReason::Mention)
    );
    server.with(|s| muted(s, false));
    next_update(&mut rx, "unmuted", |u| {
        matches!(u, ClientUpdate::GroupChanged { .. })
    })
    .await;
    let want = server.with(|s| marked(s, ALICE, "a2"));
    let u = next_update(&mut rx, "notify", |u| matches!(u, ClientUpdate::Notify(_))).await;
    let ClientUpdate::Notify(n) = u else {
        unreachable!()
    };
    assert_eq!(n.message_id.as_deref(), Some(want.as_str()));
    assert_eq!(
        (n.reason, n.subtitle.as_deref()),
        (NotifyReason::Message, Some("plan"))
    );

    server.with(|s| {
        s.append(
            G1,
            ALICE,
            EventData::ArtifactCreated(ArtifactVersionResponse {
                artifact: artifact("A9", &tid, 1),
                version: version("A9", 1),
            }),
        );
    });
    let u = next_update(&mut rx, "artifacts", |u| {
        matches!(u, ClientUpdate::ArtifactsChanged { .. })
    })
    .await;
    assert_eq!(
        u,
        ClientUpdate::ArtifactsChanged {
            group_id: G1.into(),
            topic_id: tid.clone()
        }
    );
    assert_eq!(client.artifacts(&tid).unwrap()[0].id, "A9");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_streams_with_hash_and_download_is_verified_and_cached() {
    let http = MockHttp::start().await;
    let dir = tempfile::tempdir().unwrap();
    let client = http_client(&http, dir.path(), |_| {});
    let src = tempfile::tempdir().unwrap();

    // An image: size from the header, streamed in several chunks.
    let img = png(640, 480, 700_000);
    let img_path = src.path().join("shot.png");
    std::fs::write(&img_path, &img).unwrap();
    let (progress, seen) = recorder();
    let meta = client
        .upload_file(
            UploadRequest {
                group_id: G1.into(),
                topic_id: Some("T1".into()),
                path: img_path.clone(),
                ..Default::default()
            },
            progress,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(meta.status, FileStatus::Ready);
    assert_eq!((meta.width, meta.height), (Some(640), Some(480)));
    assert_eq!(
        (meta.mime.as_str(), meta.name.as_str()),
        ("image/png", "shot.png")
    );
    assert_eq!(meta.sha256, sha(&img));
    assert_eq!(
        http.with(|s| s.upload_lengths.clone()),
        vec![Some(img.len() as u64)]
    );
    {
        let seen = seen.lock().unwrap();
        let last = |ph| seen.iter().rev().find(|p| p.phase == ph).copied();
        assert_eq!(last(Phase::Hashing).unwrap().done, img.len() as u64);
        assert_eq!(last(Phase::Uploading).unwrap().done, img.len() as u64);
        assert!(seen.iter().filter(|p| p.phase == Phase::Uploading).count() > 2);
    }
    // My own image is in the cache at once: no download for the preview.
    let fref = meta.to_file_ref();
    let cached = client.cached_file(&fref).unwrap();
    assert_eq!(std::fs::read(&cached).unwrap(), img);
    assert_eq!(chat_core::files::image_size(&cached), Some((640, 480)));

    // A file of someone else: downloaded once, then served from the cache.
    let data = noise(300_000, 7);
    let (fid, fsha) = http.with(|s| seed_file(s, "F0001", "report.pdf", &data));
    let fref = FileRef {
        file_id: fid.clone(),
        name: "report.pdf".into(),
        size: data.len() as u64,
        mime: "application/pdf".into(),
        width: None,
        height: None,
        sha256: fsha,
        media: None,
        source: None,
    };
    let (progress, seen) = recorder();
    let p = client
        .download_file(fref.clone(), progress)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), data);
    assert_eq!(p.file_name().unwrap(), "report.pdf");
    assert!(
        seen.lock()
            .unwrap()
            .iter()
            .any(|p| p.phase == Phase::Verifying)
    );
    let p2 = client
        .download_file(fref.clone(), no_progress())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(p, p2);
    assert_eq!(
        http.with(|s| s.count("GET", &format!("/v1/files/{fid}/content"))),
        1
    );
    // The same bytes sent again under another name open under that name, never under the
    // first one (a safe name must not open a copy named `a.terminal`).
    let renamed = FileRef {
        name: "notes.txt".into(),
        ..fref.clone()
    };
    let p3 = client
        .download_file(renamed.clone(), no_progress())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(p3.file_name().unwrap(), "notes.txt");
    assert_eq!(std::fs::read(&p3).unwrap(), data);
    assert_eq!(client.cached_file(&renamed), Some(p3.clone()));
    assert_eq!(client.cached_file(&fref), Some(p.clone()));
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("xattr")
            .args(["-p", "com.apple.quarantine"])
            .arg(&p3)
            .output()
            .unwrap();
        assert!(out.status.success(), "downloaded files are quarantined");
    }

    // Corrupt bytes are refused and leave nothing behind.
    let data2 = noise(50_000, 9);
    let (fid2, fsha2) = http.with(|s| {
        s.corrupt_downloads = true;
        seed_file(s, "F0002", "x.bin", &data2)
    });
    let bad = FileRef {
        file_id: fid2,
        name: "x.bin".into(),
        size: data2.len() as u64,
        mime: "application/octet-stream".into(),
        width: None,
        height: None,
        sha256: fsha2.clone(),
        media: None,
        source: None,
    };
    let e = client
        .download_file(bad.clone(), no_progress())
        .await
        .unwrap()
        .unwrap_err();
    assert!(matches!(e, ClientError::Api(ApiError::Checksum)), "{e:?}");
    assert!(client.cached_file(&bad).is_none());
    let left: Vec<_> = std::fs::read_dir(dir.path().join("files").join(&fsha2))
        .unwrap()
        .collect();
    assert!(left.is_empty(), "partial download left behind");

    // A file over the limit is refused before any request.
    let before = http.with(|s| s.requests.len());
    let big = src.path().join("big.bin");
    std::fs::File::create(&big)
        .unwrap()
        .set_len(MAX_FILE_BYTES + 1)
        .unwrap();
    let e = client
        .upload_file(
            UploadRequest {
                group_id: G1.into(),
                path: big,
                ..Default::default()
            },
            no_progress(),
        )
        .await
        .unwrap()
        .unwrap_err();
    assert!(matches!(e, ClientError::Api(ApiError::Invalid(_))), "{e:?}");
    assert_eq!(http.with(|s| s.requests.len()), before);
}

fn seed_file(s: &mut mock_http::HttpState, id: &str, name: &str, data: &[u8]) -> (String, String) {
    let meta = FileMeta {
        id: id.into(),
        group_id: G1.into(),
        topic_id: None,
        name: name.into(),
        size: data.len() as u64,
        mime: chat_core::files::guess_mime(name).into(),
        sha256: sha(data),
        width: None,
        height: None,
        status: FileStatus::Ready,
        created_by: ALICE.into(),
        created_at: 1,
    };
    s.files.insert(id.into(), (meta, data.to_vec()));
    (id.into(), sha(data))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn file_cache_drops_least_recently_used_past_its_limit() {
    let http = MockHttp::start().await;
    let dir = tempfile::tempdir().unwrap();
    let client = http_client(&http, dir.path(), |c| c.file_cache_max_bytes = 700_000);
    let refs: Vec<FileRef> = (0..3u8)
        .map(|i| {
            let data = noise(300_000, i);
            let id = format!("F000{i}");
            let name = format!("f{i}.bin");
            let (_, sha256) = http.with(|s| seed_file(s, &id, &name, &data));
            FileRef {
                file_id: id,
                name,
                size: data.len() as u64,
                mime: "application/octet-stream".into(),
                width: None,
                height: None,
                sha256,
                media: None,
                source: None,
            }
        })
        .collect();
    client
        .download_file(refs[0].clone(), no_progress())
        .await
        .unwrap()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(5)).await;
    client
        .download_file(refs[1].clone(), no_progress())
        .await
        .unwrap()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(5)).await;
    // Using file 0 makes file 1 the least recently used.
    assert!(client.cached_file(&refs[0]).is_some());
    tokio::time::sleep(Duration::from_millis(5)).await;
    client
        .download_file(refs[2].clone(), no_progress())
        .await
        .unwrap()
        .unwrap();
    assert!(client.cached_file(&refs[0]).is_some());
    assert!(client.cached_file(&refs[1]).is_none());
    assert!(client.cached_file(&refs[2]).is_some());
    assert!(!dir.path().join("files").join(&refs[1].sha256).exists());
    assert_eq!(client.store().file_cache_size().unwrap(), 600_000);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn artifacts_from_local_files_open_in_a_folder_per_version() {
    let http = MockHttp::start().await;
    let dir = tempfile::tempdir().unwrap();
    let client = http_client(&http, dir.path(), |_| {});
    let mut rx = client.subscribe();
    let site = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(site.path().join("assets")).unwrap();
    std::fs::create_dir_all(site.path().join("sub dir")).unwrap();
    std::fs::write(site.path().join("index.html"), "<h1>v1</h1>").unwrap();
    std::fs::write(site.path().join("assets/app.css"), "h1{}").unwrap();
    std::fs::write(site.path().join("sub dir/a b.txt"), "spaces").unwrap();
    std::fs::write(site.path().join(".DS_Store"), "x").unwrap();
    let files = chat_core::artifacts::local_files_in(site.path()).unwrap();
    assert_eq!(
        files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
        vec!["assets/app.css", "index.html", "sub dir/a b.txt"]
    );
    let req = NewVersion {
        group_id: G1.into(),
        topic_id: "T1".into(),
        title: Some("Plan".into()),
        kind: ArtifactKind::Html,
        entry: "index.html".into(),
        files,
        note: None,
        artifact_id: None,
    };
    let r = client
        .create_artifact(req, no_progress())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(r.version.version, 1);
    let html = r
        .version
        .files
        .iter()
        .find(|f| f.path == "index.html")
        .unwrap();
    assert_eq!(html.mime, "text/html");
    let u = next_update(&mut rx, "artifacts changed", |u| {
        matches!(u, ClientUpdate::ArtifactsChanged { .. })
    })
    .await;
    assert_eq!(
        u,
        ClientUpdate::ArtifactsChanged {
            group_id: G1.into(),
            topic_id: "T1".into()
        }
    );
    let id = r.artifact.id.clone();
    assert_eq!(client.artifacts("T1").unwrap()[0].id, id);

    let f1 = client
        .open_artifact_version(&id, None, no_progress())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(f1.version, 1);
    assert_eq!(std::fs::read_to_string(&f1.entry).unwrap(), "<h1>v1</h1>");
    assert_eq!(
        std::fs::read_to_string(f1.dir.join("sub dir/a b.txt")).unwrap(),
        "spaces"
    );
    assert_eq!(
        std::fs::read_to_string(f1.dir.join("assets/app.css")).unwrap(),
        "h1{}"
    );
    let gets = http.with(|s| s.count("GET", &format!("/v1/artifacts/{id}/versions/")));
    assert_eq!(gets, 3);
    // Opening again reads only the cache.
    let again = client
        .open_artifact_version(&id, Some(1), no_progress())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(again, f1);
    assert_eq!(client.cached_artifact_version(&id, 1), Some(f1.dir.clone()));
    assert_eq!(
        http.with(|s| s.count("GET", &format!("/v1/artifacts/{id}/versions/"))),
        gets
    );

    // A new version from one changed file.
    std::fs::write(site.path().join("index.html"), "<h1>v2</h1>").unwrap();
    let r2 = client
        .create_artifact(
            NewVersion {
                group_id: G1.into(),
                topic_id: "T1".into(),
                title: None,
                kind: ArtifactKind::Html,
                entry: "index.html".into(),
                files: vec![LocalArtifactFile {
                    path: "index.html".into(),
                    local: site.path().join("index.html"),
                    mime: None,
                }],
                note: Some("fix".into()),
                artifact_id: Some(id.clone()),
            },
            no_progress(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(r2.artifact.current_version, 2);
    // The cache knows version 2 from the create answer; the list refresh agrees.
    let list = client
        .refresh_artifacts(G1, Some("T1"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(list[0].current_version, 2);
    let f2 = client
        .open_artifact_version(&id, None, no_progress())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(f2.version, 2);
    assert_eq!(std::fs::read_to_string(&f2.entry).unwrap(), "<h1>v2</h1>");
    assert_eq!(std::fs::read_to_string(&f1.entry).unwrap(), "<h1>v1</h1>");
    let row = client.load_artifact(&id).await.unwrap().unwrap();
    assert_eq!(row.versions.len(), 2);

    // A version whose path climbs out of its folder is refused.
    let mut evil = version(&id, 7);
    evil.files[0].path = "../escape.html".into();
    let e = chat_core::artifacts::fetch_version(
        &client.http(),
        &client.config().artifacts_dir(),
        &evil,
        &no_progress(),
    )
    .await
    .unwrap_err();
    assert!(matches!(e, ApiError::Invalid(_)), "{e:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn members_prefs_avatars_audit_and_hooks() {
    let http = MockHttp::start().await;
    let dir = tempfile::tempdir().unwrap();
    let client = http_client(&http, dir.path(), |_| {});
    client
        .store()
        .apply_welcome(
            &member(ME, "me"),
            &[member(ME, "me"), member(ALICE, "alice")],
            &[summary(group(G1, &[ME, ALICE]), 0, 0, 0)],
        )
        .unwrap();
    http.with(|s| {
        s.members.insert(ME.into(), member(ME, "me"));
        s.members.insert(ALICE.into(), member(ALICE, "alice"));
        s.groups
            .insert(G1.into(), summary(group(G1, &[ME, ALICE]), 0, 0, 0));
    });
    let mut rx = client.subscribe();

    let row = client
        .set_group_prefs(
            G1,
            UpdateGroupPrefsRequest {
                muted: Some(true),
                notify: Some(NotifyLevel::None),
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert!(row.muted);
    assert_eq!(row.notify_level(), NotifyLevel::None);
    assert!(client.store().group(G1).unwrap().unwrap().muted);
    next_update(&mut rx, "group changed", |u| {
        matches!(u, ClientUpdate::GroupChanged { .. })
    })
    .await;

    // Rename: the stored group takes the server's normalized name at once.
    let name = client
        .rename_group(G1, "  前端   小组 ")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(name, "前端 小组");
    assert_eq!(
        client.store().group(G1).unwrap().unwrap().group.name,
        "前端 小组"
    );
    next_update(
        &mut rx,
        "group renamed",
        |u| matches!(u, ClientUpdate::GroupChanged { group_id } if group_id == G1),
    )
    .await;
    let e = client
        .rename_group(G1, &"群".repeat(65))
        .await
        .unwrap()
        .unwrap_err();
    assert!(matches!(e, ClientError::Api(ApiError::Invalid(_))), "{e:?}");
    const DM: &str = "01DM000000000000000000000A";
    http.with(|s| {
        let mut g = group(DM, &[ME, ALICE]);
        g.kind = GroupKind::Dm;
        s.groups.insert(DM.into(), summary(g, 0, 0, 0));
    });
    let e = client
        .rename_group(DM, "我们俩")
        .await
        .unwrap()
        .unwrap_err();
    assert!(
        matches!(
            e,
            ClientError::Api(ApiError::Server {
                code: ErrorCode::Forbidden,
                ..
            })
        ),
        "{e:?}"
    );

    let m = client
        .update_member(
            ALICE,
            UpdateMemberRequest {
                role: Some(Some("设计".into())),
                level: Some(MemberLevel::Owner),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(m.level, MemberLevel::Owner);
    let cached = client.member(ALICE).unwrap().unwrap();
    assert_eq!(cached.role, Some(Some("设计".into())));
    next_update(&mut rx, "directory", |u| {
        matches!(u, ClientUpdate::DirectoryChanged)
    })
    .await;

    let img = png(64, 64, 100);
    let e = client
        .upload_avatar(ME, img.clone(), "image/gif")
        .await
        .unwrap()
        .unwrap_err();
    assert!(matches!(e, ClientError::Api(ApiError::Invalid(_))));
    let me = client
        .upload_avatar(ME, img.clone(), "image/png")
        .await
        .unwrap()
        .unwrap();
    assert!(me.avatar_url.clone().flatten().is_some());
    assert_eq!(client.me().unwrap().unwrap().avatar_url, me.avatar_url);
    assert_eq!(client.cached_avatar(&me), None, "not downloaded yet");
    let p = client.avatar(&me).await.unwrap().unwrap().unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), img);
    assert_eq!(client.cached_avatar(&me), Some(p.clone()));
    let _ = client.avatar(&me).await.unwrap().unwrap();
    assert_eq!(
        http.with(|s| s.count("GET", &format!("/v1/members/{ME}/avatar"))),
        1
    );
    // An avatar URL on another origin never gets the token.
    let mut other = me.clone();
    other.avatar_url = Some(Some("https://example.com/a.png".into()));
    let e = client.avatar(&other).await.unwrap().unwrap_err();
    assert!(matches!(e, ClientError::Api(ApiError::BadUrl)));

    let h = client.http();
    let audit = client
        .spawn(async move { h.audit(Some(500), Some("E9")).await })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(audit.entries.len(), 1);
    assert!(audit.has_more);
    assert!(http.with(|s| s.requests.iter().any(|(_, p)| p == "/v1/audit")));

    let h = client.http();
    let created = client
        .spawn(async move {
            h.create_hook(&CreateHookRequest {
                name: "ci".into(),
                group_id: G1.into(),
                topic_id: None,
                kind: HookKind::Github,
            })
            .await
        })
        .await
        .unwrap()
        .unwrap();
    assert!(created.secret.starts_with("whk_"));
    let h = client.http();
    let list = client
        .spawn(async move { h.hooks().await })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(list.len(), 1);
    let h = client.http();
    let hid = created.hook.id.clone();
    client
        .spawn(async move { h.delete_hook(&hid).await })
        .await
        .unwrap()
        .unwrap();
    assert!(http.with(|s| s.hooks.is_empty()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn update_check_download_and_sha256() {
    let http = MockHttp::start().await;
    let dir = tempfile::tempdir().unwrap();
    let client = http_client(&http, dir.path(), |_| {});
    assert_eq!(client.check_update("1.0.0").await.unwrap().unwrap(), None);

    let dmg = noise(400_000, 3);
    let info = UpdateInfo {
        version: "1.10.0".into(),
        notes: "notes".into(),
        size: dmg.len() as u64,
        sha256: sha(&dmg),
        published_at: 1,
        download_url: format!("{}/v1/updates/1.10.0/download", http.base_url()),
    };
    http.with(|s| s.release = Some((info.clone(), dmg.clone())));
    assert_eq!(
        client.check_update("1.9.9").await.unwrap().unwrap(),
        Some(info.clone())
    );
    assert_eq!(client.check_update("1.10.0").await.unwrap().unwrap(), None);
    assert_eq!(client.check_update("2.0.0").await.unwrap().unwrap(), None);
    assert!(chat_core::updates::is_newer("1.10.0", "1.9.12"));

    let dest = dir.path().join("dl/app-1.10.0.dmg");
    let (progress, seen) = recorder();
    client
        .download_update(info.clone(), &dest, progress)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), dmg);
    assert_eq!(
        seen.lock()
            .unwrap()
            .iter()
            .rev()
            .find(|p| p.phase == Phase::Downloading)
            .unwrap()
            .done,
        dmg.len() as u64
    );

    let mut wrong = info.clone();
    wrong.sha256 = sha(b"other");
    let dest2 = dir.path().join("dl/bad.dmg");
    let e = client
        .download_update(wrong, &dest2, no_progress())
        .await
        .unwrap()
        .unwrap_err();
    assert!(matches!(e, ClientError::Api(ApiError::Checksum)), "{e:?}");
    assert!(!dest2.exists());
    assert!(!dir.path().join("dl/bad.dmg.part").exists());

    let mut elsewhere = info;
    elsewhere.download_url = "https://evil.example/app.dmg".into();
    let e = client
        .download_update(elsewhere, &dest2, no_progress())
        .await
        .unwrap()
        .unwrap_err();
    assert!(matches!(e, ClientError::Api(ApiError::BadUrl)));
}

#[test]
fn redaction_removes_tokens_codes_and_secret_urls() {
    let token = format!(
        "tc_{}",
        "A1b2_-".repeat(7).chars().take(43).collect::<String>()
    );
    let text = format!(
        "token {token} used\n\
         Authorization: Bearer abc.def\n\
         invite ABCD-EFGH-JKMN redeemed\n\
         invite WXYZ-2345 redeemed\n\
         GET https://x.dev/download?code=ABCDEFGHJKMN&v=2 200\n\
         {{\"secret\":\"whk_{s}\",\"seq\":5}}\n\
         X-Admin-Secret: hunter2\n\
         https://user:pw@host/path\n\
         seq=42 group=01G1000000000000000000000A ok",
        s = "z".repeat(43)
    );
    let out = chat_core::diagnostics::redact(&text);
    for secret in [
        token.as_str(),
        "abc.def",
        "ABCD-EFGH-JKMN",
        "ABCDEFGHJKMN",
        "WXYZ-2345",
        "whk_",
        "hunter2",
        "user:pw",
    ] {
        assert!(!out.contains(secret), "{secret} left in:\n{out}");
    }
    assert!(out.contains("v=2"));
    assert!(out.contains("seq=42 group=01G1000000000000000000000A ok"));
    assert!(out.contains("\"seq\":5"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn diagnostics_collect_redact_gzip_and_upload() {
    let http = MockHttp::start().await;
    let dir = tempfile::tempdir().unwrap();
    let logs = dir.path().join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    let token = format!("tc_{}", "q".repeat(43));
    std::fs::write(logs.join("app.2026-11-02.log"), "old line\n").unwrap();
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(
        logs.join("app.2026-11-03.log"),
        format!("INFO connected\nWARN redeem code=ABCD-EFGH-JKMN token={token}\n"),
    )
    .unwrap();
    let client = http_client(&http, dir.path(), |_| {});
    let id = client
        .send_diagnostics(&format!("version 1.2.3 bearer {token}"))
        .await
        .unwrap()
        .unwrap();
    assert!(!id.is_empty());
    let gz = http.with(|s| s.diagnostics[0].clone());
    let mut text = String::new();
    flate2::read::GzDecoder::new(&gz[..])
        .read_to_string(&mut text)
        .unwrap();
    assert!(text.starts_with("version 1.2.3"));
    assert!(text.contains("INFO connected"));
    assert!(text.find("old line").unwrap() < text.find("INFO connected").unwrap());
    assert!(!text.contains(&token));
    assert!(!text.contains("ABCD-EFGH-JKMN"));

    // Very large logs keep the newest lines and fit the limit.
    let big: String = (0..2_000_000u64)
        .map(|i| format!("{i:x} {}\n", i * 7919))
        .collect();
    let gz = chat_core::diagnostics::build_report(&big).unwrap();
    assert!(gz.len() as u64 <= MAX_DIAGNOSTICS_BYTES);
    let mut text = String::new();
    flate2::read::GzDecoder::new(&gz[..])
        .read_to_string(&mut text)
        .unwrap();
    assert!(text.ends_with(&format!("{:x} {}\n", 1_999_999, 1_999_999u64 * 7919)));
}

fn bot_run(id: &str, topic: &str, status: RunStatus, updated: u64) -> BotRun {
    BotRun {
        usage: None,
        duration_ms: None,
        result_message_id: None,
        acceptance: None,
        paused_by: None,
        kind: None,
        id: id.into(),
        bot_id: ALICE.into(),
        group_id: G1.into(),
        topic_id: topic.into(),
        machine_id: None,
        status,
        requested_by: Some(ME.into()),
        created_at: 1,
        started_at: None,
        ended_at: None,
        updated_at: updated,
        summary: None,
        plan: None,
        retry_of: None,
    }
}

fn run_ev(seq: u64, event: RunEvent) -> RunLogEntry {
    RunLogEntry::RunEvent {
        run_id: "R1".into(),
        seq,
        at: seq,
        event,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bot_runs_from_events_and_the_process_view_from_the_run_log() {
    // run.updated events land in the store; an older copy never wins.
    let dir = tempfile::tempdir().unwrap();
    let store = store_with_group(dir.path());
    let out = store
        .apply_events(
            G1,
            &[
                topic_created(1, G1, topic("T1", G1, "M1", 1)),
                ev(
                    2,
                    G1,
                    ALICE,
                    EventData::RunUpdated(RunUpdatedData {
                        run: bot_run("R1", "T1", RunStatus::Running, 5),
                    }),
                ),
            ],
        )
        .unwrap();
    assert!(out.run_topics.contains("T1"));
    store
        .store_run(&bot_run("R1", "T1", RunStatus::Created, 2))
        .unwrap();
    assert_eq!(store.runs("T1").unwrap()[0].status, RunStatus::Running);

    // The run log over two pages, folded into the process view.
    let http = MockHttp::start().await;
    let client = http_client(&http, dir.path(), |_| {});
    let mut log = vec![
        run_ev(
            1,
            RunEvent::Session {
                session_id: "S1".into(),
                runtime: "codex".into(),
                model: None,
            },
        ),
        run_ev(
            2,
            RunEvent::TurnStarted {
                turn_id: "t1".into(),
            },
        ),
    ];
    // Many deltas (more than one page), then the final text replaces them.
    for i in 0..600u64 {
        log.push(run_ev(
            3 + i,
            RunEvent::MessageDelta {
                item_id: "m1".into(),
                turn_id: Some("t1".into()),
                text: "x".into(),
            },
        ));
    }
    let mut seq = 603;
    let mut push = |e: RunEvent| {
        log.push(run_ev(seq, e));
        seq += 1;
    };
    push(RunEvent::Message {
        item_id: "m1".into(),
        turn_id: Some("t1".into()),
        role: "assistant".into(),
        text: "final".into(),
    });
    push(RunEvent::ToolCall {
        item_id: "c1".into(),
        turn_id: None,
        tool: "shell".into(),
        title: "cargo test".into(),
        input: None,
    });
    push(RunEvent::ToolResult {
        item_id: "c1".into(),
        turn_id: None,
        ok: true,
        output: Some("ok".into()),
        exit_code: Some(0),
    });
    push(RunEvent::ApprovalRequest {
        approval_id: "ap1".into(),
        item_id: None,
        title: "push?".into(),
        detail: None,
    });
    push(RunEvent::ApprovalResolved {
        approval_id: "ap1".into(),
        decision: "approve".into(),
    });
    http.with(|s| {
        s.runs.insert(
            "R1".into(),
            (bot_run("R1", "T1", RunStatus::Done, 9), log.clone()),
        )
    });
    let mut rx = client.subscribe();
    let (entries, last) = client.run_log("R1", 0).await.unwrap().unwrap();
    assert_eq!((entries.len(), last), (log.len(), 607));
    assert_eq!(http.with(|s| s.count("GET", "/v1/runs/R1/events")), 2);
    next_update(&mut rx, "runs changed", |u| {
        matches!(u, ClientUpdate::RunsChanged { .. })
    })
    .await;
    assert_eq!(client.runs("T1").unwrap()[0].status, RunStatus::Done);

    let mut view = chat_core::ProcessView::default();
    view.apply(&entries[..300]);
    match &view.items[1] {
        chat_core::ProcessItem::Message {
            text, streaming, ..
        } => {
            assert_eq!((text.len(), *streaming), (298, true))
        }
        other => panic!("{other:?}"),
    }
    // Overlapping pages are fine.
    view.apply(&entries[250..]);
    assert_eq!(view.session, Some(("S1".into(), "codex".into())));
    use chat_core::ProcessItem as P;
    assert!(matches!(&view.items[0], P::TurnStarted { turn_id } if turn_id == "t1"));
    assert!(matches!(&view.items[1], P::Message { text, streaming: false, .. } if text == "final"));
    assert!(matches!(
        &view.items[2],
        P::Tool { title, ok: Some(true), exit_code: Some(0), .. } if title == "cargo test"
    ));
    assert!(matches!(&view.items[3], P::Approval { decision: Some(d), .. } if d == "approve"));
    assert_eq!(view.items.len(), 4);
    assert_eq!(view.last_seq, 607);

    // Following a running run: only newer entries come back.
    let (more, last2) = client.run_log("R1", last).await.unwrap().unwrap();
    assert_eq!((more.len(), last2), (0, 607));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_run_stops_an_active_run_and_takes_an_ended_one_quietly() {
    let dir = tempfile::tempdir().unwrap();
    let http = MockHttp::start().await;
    let ended = BotRun {
        ended_at: Some(4),
        ..bot_run("R2", "T1", RunStatus::Done, 4)
    };
    let forbidden = BotRun {
        bot_id: "forbidden-bot".into(),
        ..bot_run("R3", "T1", RunStatus::Running, 1)
    };
    http.with(|s| {
        s.runs.insert(
            "R1".into(),
            (bot_run("R1", "T1", RunStatus::Running, 3), vec![]),
        );
        s.runs.insert("R2".into(), (ended.clone(), vec![]));
        s.runs.insert("R3".into(), (forbidden, vec![]));
    });
    let client = http_client(&http, dir.path(), |_| {});

    // The HTTP call answers with the ended run.
    let run = client.http().cancel_run("R1").await.unwrap();
    assert_eq!(run.status, RunStatus::Cancelled);
    assert!(run.ended_at.is_some());
    // A second stop is a conflict on the wire.
    let again = client.http().cancel_run("R1").await.unwrap_err();
    assert!(again.is_conflict(), "{again:?}");
    assert_eq!(http.with(|s| s.count("POST", "/v1/runs/R1/cancel")), 2);

    // The client stores the end at once and tells the views.
    http.with(|s| s.runs.get_mut("R1").unwrap().0 = bot_run("R1", "T1", RunStatus::Running, 5));
    let mut rx = client.subscribe();
    let outcome = client.cancel_run("R1").await.unwrap().unwrap();
    assert_eq!(outcome, chat_core::CancelRun::Cancelled);
    next_update(
        &mut rx,
        "runs changed",
        |u| matches!(u, ClientUpdate::RunsChanged { topic_id, .. } if topic_id == "T1"),
    )
    .await;
    let cached = client.runs("T1").unwrap();
    assert_eq!(
        cached.iter().find(|r| r.id == "R1").unwrap().status,
        RunStatus::Cancelled
    );

    // A run that already ended is not an error: the client reads it and stores its end.
    let outcome = client.cancel_run("R2").await.unwrap().unwrap();
    assert_eq!(outcome, chat_core::CancelRun::AlreadyEnded);
    assert_eq!(http.with(|s| s.count("GET", "/v1/runs/R2")), 1);
    assert_eq!(client.store().run("R2").unwrap(), Some(ended));

    // Other refusals and unknown runs are errors, and nothing is stored.
    let forbidden = client.cancel_run("R3").await.unwrap().unwrap_err();
    assert!(
        matches!(
            forbidden,
            ClientError::Api(ApiError::Server { status: 403, .. })
        ),
        "{forbidden:?}"
    );
    assert_eq!(client.store().run("R3").unwrap(), None);
    let missing = client.cancel_run("R9").await.unwrap().unwrap_err();
    assert!(
        matches!(
            missing,
            ClientError::Api(ApiError::Server { status: 404, .. })
        ),
        "{missing:?}"
    );
}

fn plan_item(text: &str, state: RunPlanState) -> RunPlanItem {
    RunPlanItem::new(text, state)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_plans_sync_with_the_run_and_stay_in_the_cache() {
    use RunPlanState::*;
    let dir = tempfile::tempdir().unwrap();
    let plan1 = vec![
        plan_item("写标题和正文", Done),
        plan_item("生成 3 张配图", Current),
        plan_item("周四 20:00 发出", Pending),
    ];
    let plan2 = vec![
        plan_item("写标题和正文", Done),
        plan_item("生成 3 张配图", Done),
        plan_item("等 Joye 确认", Waiting),
        plan_item("周四 20:00 发出", Pending),
    ];
    let with_plan = |updated: u64, plan: Option<Vec<RunPlanItem>>| BotRun {
        plan,
        ..bot_run("R1", "T1", RunStatus::Running, updated)
    };
    {
        let store = store_with_group(dir.path());
        let ran = |seq: u64, run: BotRun| {
            ev(
                seq,
                G1,
                ALICE,
                EventData::RunUpdated(RunUpdatedData { run }),
            )
        };
        let out = store
            .apply_events(
                G1,
                &[
                    topic_created(1, G1, topic("T1", G1, "M1", 1)),
                    ran(2, with_plan(5, Some(plan1.clone()))),
                ],
            )
            .unwrap();
        assert!(out.run_topics.contains("T1"));
        assert_eq!(store.runs("T1").unwrap()[0].plan.as_ref(), Some(&plan1));
        // An update replaces the whole plan.
        store
            .apply_events(G1, &[ran(3, with_plan(6, Some(plan2.clone())))])
            .unwrap();
        assert_eq!(store.runs("T1").unwrap()[0].plan.as_ref(), Some(&plan2));
        // An older copy (a replayed event) never brings the old plan back.
        store.store_run(&with_plan(5, Some(plan1.clone()))).unwrap();
        assert_eq!(
            store.run("R1").unwrap().unwrap().plan.as_ref(),
            Some(&plan2)
        );
    }
    // The cache keeps the plan across a restart.
    let store = Store::open(&dir.path().join("cache.sqlite3")).unwrap();
    assert_eq!(store.runs("T1").unwrap()[0].plan.as_ref(), Some(&plan2));
    // A cleared plan (no `plan` field) clears the cached one.
    store.store_run(&with_plan(7, None)).unwrap();
    assert_eq!(store.runs("T1").unwrap()[0].plan, None);
    drop(store);

    // A bot sets the plan over HTTP; the client validates before it sends.
    let http = MockHttp::start().await;
    http.with(|s| s.runs.insert("R1".into(), (with_plan(7, None), vec![])));
    let client = http_client(&http, dir.path(), |_| {});
    let run = client
        .http()
        .set_run_plan(
            "R1",
            &SetRunPlanRequest {
                plan: plan2.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(run.plan.as_ref(), Some(&plan2));
    let too_long = SetRunPlanRequest {
        plan: vec![plan_item(&"字".repeat(201), Current)],
    };
    assert!(matches!(
        client.http().set_run_plan("R1", &too_long).await,
        Err(ApiError::Invalid(_))
    ));
    assert_eq!(http.with(|s| s.count("PUT", "/v1/runs/R1/plan")), 1);
    // Reading the run log refreshes the cached run with its plan.
    client.run_log("R1", 0).await.unwrap().unwrap();
    assert_eq!(client.runs("T1").unwrap()[0].plan.as_ref(), Some(&plan2));
}
