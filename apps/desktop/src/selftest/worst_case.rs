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

use std::{path::PathBuf, sync::Arc};

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

const ATTACHMENT_NAMES: &[&str] = &[
    "Q3-Board-Deck-FINAL-revised-v12-approved-by-legal-and-finance-2026-11-03.pdf",
    "a",
    "产品设计与前端体验改进讨论组第四季度冲刺复盘纪要（定稿）.docx",
    "IMG_20250914_183022_HDR_portrait_edited_edited.HEIC",
];

const PLAN_TEXTS: &[&str] = &[
    "把登录页、设置窗口和侧边栏的所有颜色、间距、圆角与动效统一到主题令牌里，并在浅色和深色模式下逐屏截图核对，最后提交评审",
    "1",
    "Verify https://example.com/workspaces/acme/projects/q3-launch/docs/9f8e7d6c5b4a?tab=comments",
];

const MACHINE_NAMES: &[&str] = &[
    "joye-macbook-pro-m4-max-16-inch-2026.local",
    "m",
    "构建机 · 北京机房 · 第三排",
];

/// Eight reactions: every member, one, a ZWJ emoji, and counts past 99.
fn worst_reactions(ids: &[MemberId]) -> Vec<Reaction> {
    let take = |n: usize| ids.iter().take(n).cloned().collect::<Vec<_>>();
    let many: Vec<MemberId> = (0..120).map(|i| format!("m-many-{i}")).collect();
    [
        ("👍", ids.to_vec()),
        ("👩🏽‍💻", take(1)),
        ("🎉", take(3)),
        ("✅", take(2)),
        ("🚀", many),
        ("❤️", ids.to_vec()),
        ("😂", ids.to_vec()),
        ("👀", ids.to_vec()),
    ]
    .into_iter()
    .map(|(emoji, member_ids)| Reaction {
        emoji: emoji.into(),
        member_ids,
    })
    .collect()
}

fn pick<T: Copy>(list: &[T], i: usize) -> T {
    list[i % list.len()]
}

/// A worst-case member as a provider gives it: the raw name goes through
/// the same `clean_name` as server data (`provider::convert`).
fn member(mut m: Member, i: usize) -> Member {
    m.name = clean_name(pick(NAMES, i));
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
    /// Host names that differ only at the end, and a machine with seven
    /// tags (break-ui: twelve tags on one item).
    fn machines(&self) -> AdminReply<Vec<MachineInfo>> {
        let tags = [
            "mac",
            "office",
            "gpu",
            "ios-simulator",
            "xcode-16",
            "arm64",
            "nightly",
        ];
        let base = MachineInfo {
            connected: true,
            healthy: true,
            last_heartbeat_at: Some(self.now),
            version: Some("0.4.0-beta.12+build.20261103".into()),
            disk_free_bytes: Some(3 * 1024 * 1024 * 1024),
            disk_total_bytes: Some(8 * 1024 * 1024 * 1024 * 1024),
            active_runs: 12,
            max_parallel_runs: Some(16),
            ..MachineInfo::default()
        };
        reply_now(Ok(vec![
            MachineInfo {
                id: "wc-m1".into(),
                name: "joye-macbook-pro-m4-max-16-inch-office-2026-a".into(),
                labels: tags.iter().map(|t| t.to_string()).collect(),
                ..base.clone()
            },
            MachineInfo {
                id: "wc-m2".into(),
                name: "joye-macbook-pro-m4-max-16-inch-office-2026-b".into(),
                labels: vec!["mac".into()],
                connected: false,
                healthy: false,
                last_heartbeat_at: Some(0),
                ..base.clone()
            },
            MachineInfo {
                id: "wc-m3".into(),
                name: "构建服务器".into(),
                labels: vec!["linux".into(), "gpu".into(), "home".into()],
                healthy: false,
                ..base
            },
        ]))
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
        let ids: Vec<MemberId> = self.inner.members().into_iter().map(|m| m.id).collect();
        let len = window.messages.len();
        for (i, m) in window.messages.iter_mut().enumerate() {
            // Every other message from the newest one up, so the visible end
            // of the timeline mixes worst cases with ordinary messages.
            let from_end = len - 1 - i;
            if from_end.is_multiple_of(2) && m.deleted_at.is_none() && !m.streaming {
                m.body = pick(BODIES, from_end / 2).to_string();
            }
            for (j, a) in m.attachments.iter_mut().enumerate() {
                a.name = pick(ATTACHMENT_NAMES, from_end + j).to_string();
            }
            if from_end == 1 && m.deleted_at.is_none() {
                m.reactions = worst_reactions(&ids);
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

    // Everything else goes to the fake provider, so cards, runs, files,
    // drafts, search and machines are on screen under --worst-case too;
    // the values a view renders are rewritten as above.
    fn topic(&self, group_id: &str, topic_id: &str) -> Option<Topic> {
        self.topics_of(group_id)
            .into_iter()
            .find(|t| t.id == topic_id)
    }
    fn topic_files(&self, group_id: &str, topic_id: &str) -> Vec<TopicFile> {
        self.inner
            .topic_files(group_id, topic_id)
            .into_iter()
            .enumerate()
            .map(|(i, mut f)| {
                f.file.name = pick(ATTACHMENT_NAMES, i).to_string();
                f
            })
            .collect()
    }
    fn runs(&self, group_id: &str, topic_id: &str) -> Vec<BotRunSummary> {
        self.inner
            .runs(group_id, topic_id)
            .into_iter()
            .enumerate()
            .map(|(i, mut r)| {
                r.machine_name = r.machine_name.map(|_| pick(MACHINE_NAMES, i).to_string());
                for (j, item) in r.plan.iter_mut().enumerate() {
                    if j.is_multiple_of(2) {
                        item.text = pick(PLAN_TEXTS, j / 2).to_string();
                    }
                }
                r
            })
            .collect()
    }
    fn sign_out(&self, wipe_cache: bool) {
        self.inner.sign_out(wipe_cache)
    }
    fn reconnect_now(&self) {
        self.inner.reconnect_now()
    }
    fn retry_send(&self, group_id: &str, client_msg_id: &str) {
        self.inner.retry_send(group_id, client_msg_id)
    }
    fn react(&self, group_id: &str, topic_id: &str, message_id: &str, emoji: &str, on: bool) {
        self.inner.react(group_id, topic_id, message_id, emoji, on)
    }
    fn discard_send(&self, group_id: &str, client_msg_id: &str) {
        self.inner.discard_send(group_id, client_msg_id)
    }
    fn draft(&self, key: &DraftKey) -> Draft {
        self.inner.draft(key)
    }
    fn save_draft_text(&self, key: &DraftKey, body: String, mentions: Vec<MemberId>) {
        self.inner.save_draft_text(key, body, mentions)
    }
    fn set_quote(&self, key: &DraftKey, quote_of: Option<MessageId>) {
        self.inner.set_quote(key, quote_of)
    }
    fn attach_files(&self, key: &DraftKey, files: Vec<NewFile>) {
        self.inner.attach_files(key, files)
    }
    fn remove_file(&self, key: &DraftKey, file_id: &str) {
        self.inner.remove_file(key, file_id)
    }
    fn clear_draft(&self, key: &DraftKey) {
        self.inner.clear_draft(key)
    }
    fn send_with_files(
        &self,
        request: SendRequest,
        key: &DraftKey,
        files: Vec<PendingFileId>,
    ) -> ClientMsgId {
        self.inner.send_with_files(request, key, files)
    }
    fn start_topic_with_files(
        &self,
        request: StartTopicRequest,
        key: &DraftKey,
        files: Vec<PendingFileId>,
    ) -> ClientMsgId {
        self.inner.start_topic_with_files(request, key, files)
    }
    fn take_rejected_files(&self, key: &DraftKey) -> Vec<RejectedFile> {
        self.inner.take_rejected_files(key)
    }
    fn file_transfer(&self, key: &str) -> Option<TransferState> {
        self.inner.file_transfer(key)
    }
    fn cached_file(&self, file: &FileAttachment) -> Option<PathBuf> {
        self.inner.cached_file(file)
    }
    fn thumbnail(&self, group_id: &str, topic_id: &str, file: &FileAttachment) -> Option<PathBuf> {
        self.inner.thumbnail(group_id, topic_id, file)
    }
    fn fetch_file(&self, group_id: &str, topic_id: &str, file: &FileAttachment) -> Reply<PathBuf> {
        self.inner.fetch_file(group_id, topic_id, file)
    }
    fn cancel_download(&self, file: &FileAttachment) {
        self.inner.cancel_download(file)
    }
    fn refresh_artifacts(&self, group_id: &str, topic_id: &str) {
        self.inner.refresh_artifacts(group_id, topic_id)
    }
    fn artifact(&self, artifact_id: &str) -> Option<ArtifactDetail> {
        self.inner.artifact(artifact_id)
    }
    fn request_artifact(&self, group_id: &str, topic_id: &str, artifact_id: &str) {
        self.inner.request_artifact(group_id, topic_id, artifact_id)
    }
    fn open_artifact_version(
        &self,
        artifact_id: &str,
        version: Option<u32>,
    ) -> Reply<ArtifactFolder> {
        self.inner.open_artifact_version(artifact_id, version)
    }
    fn create_artifact(&self, request: NewArtifactRequest) -> Reply<CreatedArtifact> {
        self.inner.create_artifact(request)
    }
    fn create_group(&self, request: CreateGroupRequest) -> AdminReply<GroupId> {
        self.inner.create_group(request)
    }
    fn open_dm(&self, member_id: &str) -> AdminReply<GroupId> {
        self.inner.open_dm(member_id)
    }
    fn leave_group(&self, group_id: &str) {
        self.inner.leave_group(group_id)
    }
    fn group_muted(&self, group_id: &str) -> bool {
        self.inner.group_muted(group_id)
    }
    fn set_group_muted(&self, group_id: &str, muted: bool) {
        self.inner.set_group_muted(group_id, muted)
    }
    fn saved_navigation(&self) -> Option<SavedNavigation> {
        self.inner.saved_navigation()
    }
    fn save_navigation(&self, navigation: SavedNavigation) {
        self.inner.save_navigation(navigation)
    }
    fn topic_read_seq(&self, group_id: &str, topic_id: &str) -> Option<u64> {
        self.inner.topic_read_seq(group_id, topic_id)
    }
    fn topic_following(&self, group_id: &str, topic_id: &str) -> bool {
        self.inner.topic_following(group_id, topic_id)
    }
    fn set_topic_following(&self, group_id: &str, topic_id: &str, follow: bool) {
        self.inner.set_topic_following(group_id, topic_id, follow)
    }
    fn set_topic_owner(&self, group_id: &str, topic_id: &str, owner: Option<MemberId>) {
        self.inner.set_topic_owner(group_id, topic_id, owner)
    }
    fn search(&self, query: SearchQuery) -> SearchReply {
        self.inner.search(query)
    }
    fn run_process(&self, run_id: &str) -> RunProcess {
        self.inner.run_process(run_id)
    }
    fn follow_run(&self, run_id: &str) {
        self.inner.follow_run(run_id)
    }
    fn download_run_session(&self, run_id: &str, dir: PathBuf) -> SessionDownload {
        self.inner.download_run_session(run_id, dir)
    }
    fn update_profile(&self, name: String, role: Option<String>) -> AdminReply<Member> {
        self.inner.update_profile(name, role)
    }
    fn upload_avatar(&self, path: PathBuf) -> AdminReply<Member> {
        self.inner.upload_avatar(path)
    }
    fn create_invite(&self, request: InviteRequest) -> AdminReply<InviteCreated> {
        self.inner.create_invite(request)
    }
    fn set_member_level(&self, member_id: &str, level: MemberLevel) -> AdminReply<Member> {
        self.inner.set_member_level(member_id, level)
    }
    fn set_member_deactivated(&self, member_id: &str, deactivated: bool) -> AdminReply<Member> {
        self.inner.set_member_deactivated(member_id, deactivated)
    }
    fn create_bot_member(
        &self,
        name: String,
        role: Option<String>,
        runner: Option<RunnerSetup>,
    ) -> AdminReply<BotCreated> {
        self.inner.create_bot_member(name, role, runner)
    }
    fn create_machine(&self, name: String, labels: Vec<String>) -> AdminReply<MachineCreated> {
        self.inner.create_machine(name, labels)
    }
    fn group_prefs(&self, group_id: &str) -> GroupPrefs {
        self.inner.group_prefs(group_id)
    }
    fn set_group_prefs(
        &self,
        group_id: &str,
        muted: Option<bool>,
        notify: Option<NotifyLevel>,
    ) -> AdminReply<()> {
        self.inner.set_group_prefs(group_id, muted, notify)
    }
    fn send_diagnostics(&self, description: String) -> AdminReply<String> {
        self.inner.send_diagnostics(description)
    }
    fn card(&self, group_id: &str, message_id: &str) -> Option<Arc<card::Card>> {
        self.inner.card(group_id, message_id)
    }
    fn card_presses(&self, group_id: &str, message_id: &str) -> Vec<CardPress> {
        self.inner.card_presses(group_id, message_id)
    }
    fn card_press_state(&self, group_id: &str, message_id: &str) -> Option<CardPressState> {
        self.inner.card_press_state(group_id, message_id)
    }
    fn press_card_action(&self, group_id: &str, topic_id: &str, message_id: &str, action_id: &str) {
        self.inner
            .press_card_action(group_id, topic_id, message_id, action_id)
    }
    fn card_image(&self, group_id: &str, topic_id: &str, file_id: &str) -> Option<PathBuf> {
        self.inner.card_image(group_id, topic_id, file_id)
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
    // The account and settings stay those of the fake provider.
    fn machines(&self) -> AdminReply<Vec<MachineInfo>> {
        self.inner.machines()
    }
    fn sign_out(&self, wipe_cache: bool) {
        self.inner.sign_out(wipe_cache)
    }
    fn reconnect_now(&self) {
        self.inner.reconnect_now()
    }
    fn create_group(&self, request: CreateGroupRequest) -> AdminReply<GroupId> {
        self.inner.create_group(request)
    }
    fn open_dm(&self, member_id: &str) -> AdminReply<GroupId> {
        self.inner.open_dm(member_id)
    }
    fn update_profile(&self, name: String, role: Option<String>) -> AdminReply<Member> {
        self.inner.update_profile(name, role)
    }
    fn upload_avatar(&self, path: PathBuf) -> AdminReply<Member> {
        self.inner.upload_avatar(path)
    }
    fn create_invite(&self, request: InviteRequest) -> AdminReply<InviteCreated> {
        self.inner.create_invite(request)
    }
    fn set_member_level(&self, member_id: &str, level: MemberLevel) -> AdminReply<Member> {
        self.inner.set_member_level(member_id, level)
    }
    fn set_member_deactivated(&self, member_id: &str, deactivated: bool) -> AdminReply<Member> {
        self.inner.set_member_deactivated(member_id, deactivated)
    }
    fn create_bot_member(
        &self,
        name: String,
        role: Option<String>,
        runner: Option<RunnerSetup>,
    ) -> AdminReply<BotCreated> {
        self.inner.create_bot_member(name, role, runner)
    }
    fn create_machine(&self, name: String, labels: Vec<String>) -> AdminReply<MachineCreated> {
        self.inner.create_machine(name, labels)
    }
    fn send_diagnostics(&self, description: String) -> AdminReply<String> {
        self.inner.send_diagnostics(description)
    }
}
