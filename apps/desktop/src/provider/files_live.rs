//! Live check of files and 产出 through two chat-core providers against a
//! real server (`wrangler dev`):
//!
//! ```sh
//! TC_LIVE_URL=http://127.0.0.1:8802 \
//! TC_LIVE_TOKEN_A=~/.config/team-chat/<env>/owner-token \
//! TC_LIVE_TOKEN_B=~/.config/team-chat/<env>/<member>-token \
//! cargo test -p desktop files_live -- --ignored --nocapture
//! ```
//!
//! A makes a new 群 with B, starts a topic with an image and a text file
//! (upload job, pending topic, alias to the real topic), sends a file into
//! the topic, fails an upload on purpose and retries it, and uploads a
//! folder as a 产出 and then a second version. B downloads the file, gets
//! the thumbnail, sees the 产出 card and opens both versions. Caches go to a
//! temporary folder that is removed at the end.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use super::{CoreProvider, ProviderConfig, session::TokenStorage};
use crate::data::{
    ChatProvider, CreateGroupRequest, Credential, DraftKey, FileError, LoginRequest, Message,
    NewArtifactRequest, NewFile, SendRequest, SendState, SessionState, StartTopicRequest,
    TransferState, upload_key,
};

const TIMEOUT: Duration = Duration::from_secs(30);

fn wait<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(v) = check() {
            eprintln!("ok: {what}");
            return v;
        }
        assert!(Instant::now() < deadline, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn read_token(var: &str) -> Option<String> {
    let path = std::env::var(var).ok()?;
    let path = match path.strip_prefix("~/") {
        Some(rest) => PathBuf::from(std::env::var("HOME").ok()?).join(rest),
        None => PathBuf::from(path),
    };
    Some(std::fs::read_to_string(path).ok()?.trim().to_string())
}

fn sign_in(url: &str, token: String, root: PathBuf) -> (Arc<CoreProvider>, String) {
    let p = CoreProvider::new(ProviderConfig {
        data_root: root,
        tokens: TokenStorage::memory(),
        restore: false,
    });
    p.login(LoginRequest {
        server_url: url.to_string(),
        credential: Credential::Token(token),
        device_name: "files-live".into(),
    });
    let me = wait("signed in", || match p.session() {
        SessionState::SignedIn { me, .. } => Some(me.id),
        _ => None,
    });
    (p, me)
}

fn block<T>(rx: crate::data::Reply<T>) -> Result<T, FileError> {
    futures::executor::block_on(rx).unwrap_or(Err(FileError::Other))
}

fn find(p: &CoreProvider, g: &str, t: &str, f: impl Fn(&Message) -> bool) -> Option<Message> {
    p.messages(g, t, 200).messages.into_iter().find(|m| f(m))
}

fn png(path: &Path, w: u32, h: u32) {
    image::RgbImage::from_pixel(w, h, image::Rgb([30, 120, 200]))
        .save(path)
        .unwrap();
}

#[test]
#[ignore = "needs a server: set TC_LIVE_URL, TC_LIVE_TOKEN_A and TC_LIVE_TOKEN_B"]
fn files_live() {
    let (Ok(url), Some(token_a), Some(token_b)) = (
        std::env::var("TC_LIVE_URL"),
        read_token("TC_LIVE_TOKEN_A"),
        read_token("TC_LIVE_TOKEN_B"),
    ) else {
        panic!("set TC_LIVE_URL, TC_LIVE_TOKEN_A and TC_LIVE_TOKEN_B");
    };
    let base = std::env::temp_dir().join(format!("files-live-{}", std::process::id()));
    let local = base.join("local");
    std::fs::create_dir_all(&local).unwrap();
    let (a, me_a) = sign_in(&url, token_a, base.join("a"));
    let (b, me_b) = sign_in(&url, token_b, base.join("b"));
    assert_ne!(me_a, me_b);

    // A new 群 with B.
    let name = format!("文件 {}", chat_core::new_id());
    let _ = a.create_group(CreateGroupRequest {
        name: name.clone(),
        member_ids: vec![me_b.clone()],
    });
    let g = wait("group made", || a.groups().into_iter().find(|x| x.name == name)).id;
    wait("B sees the group", || b.groups().into_iter().find(|x| x.id == g));

    // A folder is refused at attach time with a reason.
    let new_key = DraftKey::new_topic(g.clone());
    a.attach_files(&new_key, vec![NewFile::Path(local.clone())]);
    let rejected = wait("folder rejected", || {
        let r = a.take_rejected_files(&new_key);
        (!r.is_empty()).then_some(r)
    });
    assert_eq!(rejected[0].error, FileError::NotAFile);

    // A starts a topic with an image and a text file.
    let photo = local.join("photo.png");
    png(&photo, 1600, 800);
    let notes = local.join("说明.txt");
    let text = "第一行\nsecond line\n".repeat(2000);
    std::fs::write(&notes, &text).unwrap();
    a.attach_files(
        &new_key,
        vec![NewFile::Path(photo.clone()), NewFile::Path(notes.clone())],
    );
    let draft = wait("files in the draft", || {
        let d = a.draft(&new_key);
        (d.files.len() == 2).then_some(d)
    });
    let ids: Vec<_> = draft.files.iter().map(|f| f.id.clone()).collect();
    let job = a.start_topic_with_files(
        StartTopicRequest {
            group_id: g.clone(),
            title: None,
            body: "两份文件".into(),
            mentions: vec![],
        },
        &new_key,
        ids,
    );
    a.clear_draft(&new_key);
    let pending_topic = format!("pending:{job}");
    // The pending topic shows at once with my message and its uploads.
    let first = a.topics(&g).into_iter().next().unwrap();
    assert_eq!(first.id, pending_topic);
    let shown = find(&a, &g, &pending_topic, |m| m.client_msg_id == job).unwrap();
    assert_eq!(shown.attachments.len(), 2);
    assert_ne!(shown.send_state, SendState::Sent);
    // The pending id follows the send to the real topic.
    let sent = wait("topic message acked", || {
        find(&a, &g, &pending_topic, |m| {
            !m.id.is_empty() && m.attachments.len() == 2 && m.attachments.iter().all(|f| f.is_uploaded())
        })
    });
    let t = a.resolve_topic(&pending_topic);
    assert!(!t.starts_with("pending:"), "real topic id after the ack");
    let image = sent.attachments.iter().find(|f| f.is_image()).unwrap().clone();
    assert_eq!((image.width, image.height), (Some(1600), Some(800)));
    let doc = sent.attachments.iter().find(|f| !f.is_image()).unwrap().clone();
    assert_eq!(doc.name, "说明.txt");
    assert_eq!(doc.size, text.len() as u64);
    assert!(a.file_transfer(&upload_key(&job, 0)).is_none(), "job forgotten after the send");

    // B downloads the text file and gets a thumbnail of the image.
    let on_b = wait("B sees the files", || find(&b, &g, &t, |m| m.id == sent.id));
    assert_eq!(on_b.attachments, sent.attachments);
    assert!(b.cached_file(&doc).is_none());
    let path = block(b.fetch_file(&g, &t, &doc)).expect("download");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    assert_eq!(b.cached_file(&doc).as_deref(), Some(path.as_path()));
    block(b.fetch_file(&g, &t, &image)).expect("image download");
    let thumb = wait("thumbnail", || b.thumbnail(&g, &t, &image));
    assert_eq!(chat_core::files::image_size(&thumb), Some((640, 320)));
    assert_eq!(b.topic_files(&g, &t).len(), 2);

    // An upload that fails can be retried.
    let key = DraftKey::topic(g.clone(), t.clone());
    let late = local.join("late.txt");
    std::fs::write(&late, "later").unwrap();
    a.attach_files(&key, vec![NewFile::Path(late.clone())]);
    let draft = wait("file in the topic draft", || {
        let d = a.draft(&key);
        (d.files.len() == 1).then_some(d)
    });
    std::fs::remove_file(&late).unwrap();
    let cid = a.send_with_files(
        SendRequest {
            group_id: g.clone(),
            topic_id: t.clone(),
            body: String::new(),
            mentions: vec![],
            quote_of: None,
        },
        &key,
        vec![draft.files[0].id.clone()],
    );
    a.clear_draft(&key);
    wait("upload failed", || {
        find(&a, &g, &t, |m| m.client_msg_id == cid && m.send_state == SendState::Failed)
    });
    assert_eq!(
        a.file_transfer(&upload_key(&cid, 0)),
        Some(TransferState::Failed(FileError::Unreadable))
    );
    std::fs::write(&late, "later").unwrap();
    a.retry_send(&g, &cid);
    let retried = wait("retried send acked", || {
        find(&a, &g, &t, |m| {
            !m.id.is_empty() && m.attachments.iter().any(|f| f.name == "late.txt")
        })
    });
    assert!(retried.body.is_empty());

    // A uploads a folder as a 产出, then a second version.
    let site = local.join("site");
    std::fs::create_dir_all(site.join("css")).unwrap();
    std::fs::write(site.join("index.html"), "<title>周报</title><p>v1</p>").unwrap();
    std::fs::write(site.join("css/app.css"), "p{color:red}").unwrap();
    let created = block(a.create_artifact(NewArtifactRequest {
        group_id: g.clone(),
        topic_id: t.clone(),
        path: site.clone(),
        artifact_id: None,
    }))
    .expect("产出 created");
    assert_eq!((created.title.as_str(), created.version), ("周报", 1));
    std::fs::write(site.join("index.html"), "<title>周报</title><p>v2</p>").unwrap();
    let v2 = block(a.create_artifact(NewArtifactRequest {
        group_id: g.clone(),
        topic_id: t.clone(),
        path: site.clone(),
        artifact_id: Some(created.artifact_id.clone()),
    }))
    .expect("version 2");
    assert_eq!(v2.version, 2);

    // B sees both cards, the list, and opens each version.
    let card = wait("B sees the 产出 message", || {
        find(&b, &g, &t, |m| {
            m.artifact_refs.iter().any(|r| r.artifact_id == created.artifact_id && r.version == 2)
        })
    });
    assert!(!card.body.is_empty());
    b.request_artifact(&g, &t, &created.artifact_id);
    let detail = wait("B has the 产出", || {
        b.artifact(&created.artifact_id).filter(|d| d.versions.len() == 2)
    });
    assert_eq!(detail.summary.version, 2);
    wait("B lists it", || {
        b.artifacts(&g, &t).into_iter().find(|x| x.id == created.artifact_id)
    });
    let current = block(b.open_artifact_version(&created.artifact_id, None)).expect("open current");
    assert!(current.is_page);
    assert_eq!(current.entry, "index.html");
    assert!(std::fs::read_to_string(current.dir.join("index.html")).unwrap().contains("v2"));
    assert_eq!(
        std::fs::read_to_string(current.dir.join("css/app.css")).unwrap(),
        "p{color:red}"
    );
    let first = block(b.open_artifact_version(&created.artifact_id, Some(1))).expect("open v1");
    assert_eq!(first.version, 1);
    assert!(std::fs::read_to_string(first.dir.join("index.html")).unwrap().contains("v1"));

    let _ = std::fs::remove_dir_all(&base);
}
