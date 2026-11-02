//! The `files` screenshot scene: the sample data plus messages with an image,
//! files (one downloading, one uploading) and 产出 cards, and the side panel
//! with the topic's 产出 and files. It wraps the fake provider and adds file
//! data for one topic, so the shared fake data stays as it is.

use std::{path::PathBuf, sync::Arc};

use futures::channel::mpsc::UnboundedReceiver;
use gpui::{AnyWindowHandle, App, Pixels, Size};

use crate::{
    app::Services,
    data::*,
    selftest::scenes::{open_first_topic, open_main},
};

pub struct FilesPreview {
    inner: Arc<dyn ChatProvider>,
    group_id: String,
    topic_id: String,
    image: Option<PathBuf>,
}

const ARTIFACT: &str = "a-preview";

fn photo() -> FileAttachment {
    FileAttachment {
        file_id: "f-photo".into(),
        name: "首页设计稿.png".into(),
        size: 2_400_000,
        mime: "image/png".into(),
        width: Some(1600),
        height: Some(1000),
        sha256: "1".repeat(64),
    }
}

fn pdf() -> FileAttachment {
    FileAttachment {
        file_id: "f-pdf".into(),
        name: "第三季度运营数据与下季度计划（终版）.pdf".into(),
        size: 18_300_000,
        mime: "application/pdf".into(),
        width: None,
        height: None,
        sha256: "2".repeat(64),
    }
}

fn sheet() -> FileAttachment {
    FileAttachment {
        file_id: "f-sheet".into(),
        name: "用户反馈.csv".into(),
        size: 48_200,
        mime: "text/csv".into(),
        width: None,
        height: None,
        sha256: "3".repeat(64),
    }
}

impl FilesPreview {
    /// Wraps `inner`; file data goes into its first ordinary topic.
    pub fn new(inner: Arc<dyn ChatProvider>) -> Arc<Self> {
        let group = inner.groups().first().cloned();
        let topic = group.as_ref().and_then(|g| {
            inner
                .topics(&g.id)
                .into_iter()
                .find(|t| t.id != crate::fake::LONG_TOPIC)
        });
        let image = std::env::temp_dir().join("team-chat-files-preview.png");
        if !image.is_file() {
            let img = image::RgbImage::from_fn(640, 400, |x, y| {
                image::Rgb([(40 + x / 8) as u8, (90 + y / 6) as u8, 200])
            });
            let _ = img.save(&image);
        }
        Arc::new(Self {
            inner,
            group_id: group.map(|g| g.id).unwrap_or_default(),
            topic_id: topic.map(|t| t.id).unwrap_or_default(),
            image: image.is_file().then_some(image),
        })
    }

    fn mine(&self, g: &str, t: &str) -> bool {
        g == self.group_id && t == self.topic_id
    }

    fn extra_messages(&self) -> Vec<Message> {
        let members = self.inner.members();
        let me = match self.inner.session() {
            SessionState::SignedIn { me, .. } => me.id,
            _ => String::new(),
        };
        let other = members
            .iter()
            .find(|m| m.id != me && !m.is_bot())
            .map(|m| m.id.clone())
            .unwrap_or_default();
        let bot = members
            .iter()
            .find(|m| m.is_bot())
            .map(|m| m.id.clone())
            .unwrap_or_default();
        let now = chrono::Utc::now().timestamp_millis();
        let base = |id: &str, author: &str, body: &str, at: i64| Message {
            id: id.into(),
            client_msg_id: format!("c-{id}"),
            group_id: self.group_id.clone(),
            topic_id: self.topic_id.clone(),
            author_id: author.into(),
            body: body.into(),
            mentions: Vec::new(),
            created_seq: 900_000 + at as u64 % 1000,
            created_at: at,
            edited_at: None,
            deleted_at: None,
            streaming: false,
            send_state: SendState::Sent,
            quote_of: None,
            attachments: Vec::new(),
            artifact_refs: Vec::new(),
            reactions: Vec::new(),
        };
        let mut a = base("pv-1", &other, "首页的新稿子和数据都在这里，看看有没有问题。", now - 600_000);
        a.attachments = vec![photo(), pdf()];
        let mut b = base("pv-2", &bot, "整理好了，页面放在产出里：", now - 300_000);
        b.artifact_refs = vec![ArtifactRef {
            artifact_id: ARTIFACT.into(),
            version: 2,
        }];
        let mut c = base("", &me, "", now - 10_000);
        c.client_msg_id = "pv-up".into();
        c.send_state = SendState::Pending;
        c.created_seq = 0;
        c.attachments = vec![FileAttachment {
            file_id: String::new(),
            sha256: String::new(),
            ..sheet()
        }];
        vec![a, b, c]
    }
}

impl ChatProvider for FilesPreview {
    fn session(&self) -> SessionState {
        self.inner.session()
    }
    fn login(&self, request: LoginRequest) {
        self.inner.login(request)
    }
    fn logout(&self) {
        self.inner.logout()
    }
    fn connection(&self) -> ConnectionState {
        self.inner.connection()
    }
    fn members(&self) -> Vec<Member> {
        self.inner.members()
    }
    fn member(&self, id: &str) -> Option<Member> {
        self.inner.member(id)
    }
    fn groups(&self) -> Vec<GroupSummary> {
        self.inner.groups()
    }
    fn topics(&self, group_id: &str) -> Vec<Topic> {
        self.inner.topics(group_id)
    }
    fn topic(&self, group_id: &str, topic_id: &str) -> Option<Topic> {
        self.inner.topic(group_id, topic_id)
    }
    fn todo(&self) -> Vec<TodoItem> {
        self.inner.todo()
    }
    fn messages(&self, group_id: &str, topic_id: &str, limit: usize) -> MessageWindow {
        let mut w = self.inner.messages(group_id, topic_id, limit);
        if self.mine(group_id, topic_id) {
            w.messages.extend(self.extra_messages());
        }
        w
    }
    fn load_older(&self, group_id: &str, topic_id: &str) {
        self.inner.load_older(group_id, topic_id)
    }
    fn typing(&self, group_id: &str, topic_id: &str) -> Vec<MemberId> {
        self.inner.typing(group_id, topic_id)
    }
    fn artifacts(&self, group_id: &str, topic_id: &str) -> Vec<ArtifactSummary> {
        if self.mine(group_id, topic_id) {
            self.artifact(ARTIFACT).map(|d| vec![d.summary]).unwrap_or_default()
        } else {
            self.inner.artifacts(group_id, topic_id)
        }
    }
    fn send(&self, request: SendRequest) -> ClientMsgId {
        self.inner.send(request)
    }
    fn start_topic(&self, request: StartTopicRequest) -> ClientMsgId {
        self.inner.start_topic(request)
    }
    fn edit(&self, group_id: &str, message_id: &str, body: String) {
        self.inner.edit(group_id, message_id, body)
    }
    fn delete(&self, group_id: &str, message_id: &str) {
        self.inner.delete(group_id, message_id)
    }
    fn set_topic_status(&self, g: &str, t: &str, status: TopicStatus, waiting_on: Vec<MemberId>) {
        self.inner.set_topic_status(g, t, status, waiting_on)
    }
    fn mark_read(&self, group_id: &str, topic_id: &str, seq: u64) {
        self.inner.mark_read(group_id, topic_id, seq)
    }
    fn set_typing(&self, group_id: &str, topic_id: &str) {
        self.inner.set_typing(group_id, topic_id)
    }
    fn draft(&self, key: &DraftKey) -> Draft {
        self.inner.draft(key)
    }
    fn file_transfer(&self, key: &str) -> Option<TransferState> {
        if key == download_key(&pdf()) {
            Some(TransferState::Running {
                done: 7_000_000,
                total: 18_300_000,
            })
        } else if key == upload_key("pv-up", 0) {
            Some(TransferState::Running {
                done: 21_000,
                total: 48_200,
            })
        } else {
            None
        }
    }
    fn thumbnail(&self, _g: &str, _t: &str, file: &FileAttachment) -> Option<PathBuf> {
        (file.file_id == "f-photo").then(|| self.image.clone()).flatten()
    }
    fn cached_file(&self, file: &FileAttachment) -> Option<PathBuf> {
        self.thumbnail("", "", file)
    }
    fn topic_files(&self, group_id: &str, topic_id: &str) -> Vec<TopicFile> {
        if !self.mine(group_id, topic_id) {
            return Vec::new();
        }
        self.extra_messages()
            .into_iter()
            .flat_map(|m| {
                m.attachments
                    .iter()
                    .filter(|f| f.is_uploaded())
                    .map(|f| TopicFile {
                        file: f.clone(),
                        message_id: m.id.clone(),
                        author_id: m.author_id.clone(),
                        created_at: m.created_at,
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }
    fn artifact(&self, artifact_id: &str) -> Option<ArtifactDetail> {
        if artifact_id != ARTIFACT {
            return None;
        }
        let bot = self.inner.members().into_iter().find(|m| m.is_bot())?.id;
        let now = chrono::Utc::now().timestamp_millis();
        Some(ArtifactDetail {
            summary: ArtifactSummary {
                id: ARTIFACT.into(),
                topic_id: self.topic_id.clone(),
                kind: ArtifactKind::Page,
                title: "第三季度运营复盘".into(),
                version: 3,
                size_bytes: None,
                created_by: bot.clone(),
                updated_at: now - 120_000,
            },
            group_id: self.group_id.clone(),
            versions: (1..=3)
                .rev()
                .map(|v| ArtifactVersionInfo {
                    version: v,
                    note: None,
                    created_by: bot.clone(),
                    created_at: now - 1_000_000 + v as i64,
                })
                .collect(),
        })
    }
    fn subscribe(&self) -> UnboundedReceiver<Vec<Change>> {
        self.inner.subscribe()
    }
}

/// The scene opener: the main window on the preview provider.
pub fn open(services: Services, content: Size<Pixels>, cx: &mut App) -> Result<AnyWindowHandle, String> {
    let provider: Arc<dyn ChatProvider> = FilesPreview::new(services.provider.clone());
    let services = Services::new(provider, cx);
    open_first_topic(&services, cx);
    open_main(services, content, cx)
}
