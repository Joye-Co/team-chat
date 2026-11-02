//! Providers for the screenshot self-test that change the data, not the views
//! (break-ui, Hard Rule 2):
//!
//! - [`WorstCase`] wraps the fake provider and rewrites every value a view
//!   renders with realistic worst cases from
//!   `docs/design/skills/emilkowalski/skills/break-ui/CATALOG.md`: long and
//!   one-character names, mixed scripts, RTL, emoji with ZWJ, unbroken
//!   e-mails and URLs, 999+ counts, file names, empty and multi-line titles,
//!   huge sizes, old and zero timestamps. Different failures sit in different
//!   rows, as real data does. Ids stay the same, so navigation still works.
//! - [`Empty`] is signed in with no 群, 话题 or messages.

use std::sync::Arc;

use futures::channel::mpsc::{UnboundedReceiver, unbounded};

use crate::data::*;

const NAMES: &[&str] = &[
    "Aleksandra Wiśniewska-Kowalczyk",
    "王",
    "欧阳慕容·司马长空",
    "bartholomew.fitzgerald@northwind-industries-holdings.example.com",
    "👩🏽‍💻 Priya",
    "نور الهدى عبد الرحمن",
    "Jo",
    "  Sam   Lee ",
    "Đặng Thị Ngọc Hân",
    "🦊 代码审查与发布值守助手",
    "Christopher Alexander Montgomery III",
];

const ROLES: &[Option<&str>] = &[
    Some("高级产品设计工程师，平台基础设施与开发者体验"),
    None,
    Some("Senior Product Design Engineer, Platform Infrastructure"),
    Some(""),
    Some("实习"),
];

const GROUP_NAMES: &[&str] = &[
    "产品设计与前端体验改进讨论组（2026 第四季度冲刺）",
    "群",
    "Benachrichtigungseinstellungen",
    "https://example.com/workspaces/acme/projects/q3-launch/docs/9f8e7d6c5b4a?tab=comments",
    "🚀🚀🚀",
    "运营 · Ops · العمليات",
];

const TITLES: &[&str] = &[
    "把登录页、设置窗口和侧边栏的所有颜色、间距、圆角与动效统一到主题令牌里，并在浅色和深色模式下逐屏截图核对",
    "1",
    "IMG_20250914_183022_HDR_portrait_edited_edited.HEIC",
    "<script>alert(1)</script> &amp; **粗体** `代码`",
    "",
    "第一行\n第二行",
    "Q3 Board Deck — FINAL (revised) v12 [approved by legal].pdf",
    "修复 9f8e7d6c-5b4a-4c3d-8e2f-1a0b9c8d7e6f 在重连后重复投递的问题",
];

const COUNTS: &[u32] = &[0, 1, 99, 100, 999, 1_000, 12_345, 1_000_000];

const BODIES: &[&str] = &[
    "https://example.com/workspaces/acme/projects/q3-launch/docs/9f8e7d6c5b4a?tab=comments&filter=unresolved&highlight=a-very-long-anchor-that-keeps-going",
    "1",
    "👍",
    "```rust\nlet result = provider.messages(group_id, topic_id, limit).messages.into_iter().filter(|m| m.deleted_at.is_none()).map(|m| m.body).collect::<Vec<_>>().join(\"\\n\");\n```",
    "这是一段很长的说明，用来检查正文的行长和换行：我们在周一的评审里决定把发布流程拆成三步，第一步先在预发环境验证数据迁移，第二步只对内部成员开放，第三步再全量发布。每一步都需要有人确认，并把结论记录在话题里，同时把长内容放进产出，话题里只保留结论和链接。Mixed English inside Chinese text, with numbers like 1,284 and 99.9% and dates like 2026-11-03, should keep a steady rhythm without odd gaps.",
    "| 名称 | 状态 | 负责人 | 截止 | 备注 |\n| --- | --- | --- | --- | --- |\n| bartholomew.fitzgerald@northwind-industries-holdings.example.com | 卡住了 | Aleksandra Wiśniewska-Kowalczyk | 2026-12-31 | 等审 |",
    "BenachrichtigungseinstellungenDatenschutzeinstellungenSicherheitseinstellungen",
    "مرحبا بالجميع، هذا اختبار للنص من اليمين إلى اليسار مع English و中文。",
];

const FILE_TITLES: &[&str] = &[
    "Q3 Board Deck — FINAL (revised) v12 [approved by legal].pdf",
    "IMG_20250914_183022_HDR_portrait_edited_edited.HEIC",
    "设计评审结论",
    "a.md",
];

fn pick<T: Copy>(list: &[T], i: usize) -> T {
    list[i % list.len()]
}

fn member(mut m: Member, i: usize) -> Member {
    m.name = pick(NAMES, i).to_string();
    m.role = pick(ROLES, i).map(str::to_string);
    m
}

fn group(mut g: GroupSummary, i: usize) -> GroupSummary {
    // DMs keep the peer's (worst-case) name; groups get worst-case titles.
    if g.kind == GroupKind::Group {
        g.name = pick(GROUP_NAMES, i).to_string();
    }
    g.unread = pick(COUNTS, i + 3);
    g.mentions = if i.is_multiple_of(3) {
        pick(COUNTS, i + 4)
    } else {
        0
    };
    g
}

fn topic(mut t: Topic, i: usize, now: Millis) -> Topic {
    t.title = pick(TITLES, i).to_string();
    t.unread = pick(COUNTS, i + 2);
    t.mentions = if i.is_multiple_of(2) {
        pick(COUNTS, i + 1)
    } else {
        0
    };
    t.message_count = pick(COUNTS, i + 5);
    const DAY: i64 = 24 * 3_600_000;
    t.last_activity_at = match i % 5 {
        0 => now,
        1 => now - 12 * DAY,
        2 => now - 3 * 365 * DAY,
        3 => 0,
        _ => t.last_activity_at,
    };
    t
}

fn now_ms() -> Millis {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// The fake provider with every rendered value replaced by a worst case.
pub struct WorstCase {
    inner: Arc<dyn ChatProvider>,
    now: Millis,
}

impl WorstCase {
    pub fn new(inner: Arc<dyn ChatProvider>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            now: now_ms(),
        })
    }

    fn member_index(&self, id: &str) -> usize {
        self.inner
            .members()
            .iter()
            .position(|m| m.id == id)
            .unwrap_or(0)
    }

    fn group_index(&self, id: &str) -> usize {
        self.inner
            .groups()
            .iter()
            .position(|g| g.id == id)
            .unwrap_or(0)
    }

    fn topics_of(&self, group_id: &str) -> Vec<Topic> {
        let offset = self.group_index(group_id);
        self.inner
            .topics(group_id)
            .into_iter()
            .enumerate()
            .map(|(i, t)| topic(t, i + offset, self.now))
            .collect()
    }
}

impl ChatProvider for WorstCase {
    fn session(&self) -> SessionState {
        match self.inner.session() {
            SessionState::SignedIn { me, server_url } => {
                let i = self.member_index(&me.id);
                SessionState::SignedIn {
                    me: member(me, i),
                    server_url,
                }
            }
            other => other,
        }
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
        self.inner
            .members()
            .into_iter()
            .enumerate()
            .map(|(i, m)| member(m, i))
            .collect()
    }
    fn groups(&self) -> Vec<GroupSummary> {
        let members = self.members();
        self.inner
            .groups()
            .into_iter()
            .enumerate()
            .map(|(i, g)| {
                let mut g = group(g, i);
                if let Some(peer) = g
                    .dm_peer
                    .as_ref()
                    .and_then(|p| members.iter().find(|m| &m.id == p))
                {
                    g.name = peer.name.clone();
                }
                g
            })
            .collect()
    }
    fn topics(&self, group_id: &str) -> Vec<Topic> {
        self.topics_of(group_id)
    }
    fn todo(&self) -> Vec<TodoItem> {
        let groups = self.groups();
        self.inner
            .todo()
            .into_iter()
            .map(|mut item| {
                if let Some(t) = self
                    .topics_of(&item.group_id)
                    .into_iter()
                    .find(|t| t.id == item.topic.id)
                {
                    item.topic = t;
                }
                if let Some(g) = groups.iter().find(|g| g.id == item.group_id) {
                    item.group_name = g.name.clone();
                }
                item
            })
            .collect()
    }
    fn messages(&self, group_id: &str, topic_id: &str, limit: usize) -> MessageWindow {
        let mut window = self.inner.messages(group_id, topic_id, limit);
        let len = window.messages.len();
        for (i, m) in window.messages.iter_mut().enumerate() {
            // Every other message from the newest one up, so the visible end
            // of the timeline mixes worst cases with ordinary messages.
            let from_end = len - 1 - i;
            if from_end.is_multiple_of(2) && m.deleted_at.is_none() && !m.streaming {
                m.body = pick(BODIES, from_end / 2).to_string();
            }
        }
        window
    }
    fn load_older(&self, group_id: &str, topic_id: &str) {
        self.inner.load_older(group_id, topic_id)
    }
    fn typing(&self, group_id: &str, topic_id: &str) -> Vec<MemberId> {
        self.inner.typing(group_id, topic_id)
    }
    fn artifacts(&self, group_id: &str, topic_id: &str) -> Vec<ArtifactSummary> {
        self.inner
            .artifacts(group_id, topic_id)
            .into_iter()
            .enumerate()
            .map(|(i, mut a)| {
                a.title = pick(FILE_TITLES, i).to_string();
                a.version = [999, 1, 12][i % 3];
                a.size_bytes = a.size_bytes.map(|_| [104_857_600, 0, 1_284][i % 3]);
                a
            })
            .collect()
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
    fn set_topic_status(
        &self,
        group_id: &str,
        topic_id: &str,
        status: TopicStatus,
        waiting_on: Vec<MemberId>,
    ) {
        self.inner
            .set_topic_status(group_id, topic_id, status, waiting_on)
    }
    fn mark_read(&self, group_id: &str, topic_id: &str, seq: u64) {
        self.inner.mark_read(group_id, topic_id, seq)
    }
    fn set_typing(&self, group_id: &str, topic_id: &str) {
        self.inner.set_typing(group_id, topic_id)
    }
    fn subscribe(&self) -> UnboundedReceiver<Vec<Change>> {
        self.inner.subscribe()
    }
}

/// Signed in as the fake provider's member, with nothing else.
pub struct Empty {
    inner: Arc<dyn ChatProvider>,
}

impl Empty {
    pub fn new(inner: Arc<dyn ChatProvider>) -> Arc<Self> {
        Arc::new(Self { inner })
    }
}

impl ChatProvider for Empty {
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
        match self.inner.session() {
            SessionState::SignedIn { me, .. } => vec![me],
            _ => Vec::new(),
        }
    }
    fn groups(&self) -> Vec<GroupSummary> {
        Vec::new()
    }
    fn topics(&self, _group_id: &str) -> Vec<Topic> {
        Vec::new()
    }
    fn todo(&self) -> Vec<TodoItem> {
        Vec::new()
    }
    fn messages(&self, _group_id: &str, _topic_id: &str, _limit: usize) -> MessageWindow {
        MessageWindow::default()
    }
    fn load_older(&self, _group_id: &str, _topic_id: &str) {}
    fn typing(&self, _group_id: &str, _topic_id: &str) -> Vec<MemberId> {
        Vec::new()
    }
    fn artifacts(&self, _group_id: &str, _topic_id: &str) -> Vec<ArtifactSummary> {
        Vec::new()
    }
    fn send(&self, request: SendRequest) -> ClientMsgId {
        request.body.len().to_string()
    }
    fn start_topic(&self, request: StartTopicRequest) -> ClientMsgId {
        request.body.len().to_string()
    }
    fn edit(&self, _group_id: &str, _message_id: &str, _body: String) {}
    fn delete(&self, _group_id: &str, _message_id: &str) {}
    fn set_topic_status(
        &self,
        _group_id: &str,
        _topic_id: &str,
        _status: TopicStatus,
        _waiting_on: Vec<MemberId>,
    ) {
    }
    fn mark_read(&self, _group_id: &str, _topic_id: &str, _seq: u64) {}
    fn set_typing(&self, _group_id: &str, _topic_id: &str) {}
    fn subscribe(&self) -> UnboundedReceiver<Vec<Change>> {
        unbounded().1
    }
}
