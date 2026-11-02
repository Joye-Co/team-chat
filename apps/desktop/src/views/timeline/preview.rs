//! Screenshot scenes of the topic view (`selftest/scenes.rs`): the topics of
//! the v2.1 mockups (`docs/design/mockups-v2`), drawn by the real views.
//!
//! [`TopicSamples`] wraps the scene's provider and adds three topics to the
//! first 群: the long topic of `main-*` (people and bots, a date divider,
//! grouping, a steps row, a 产出, an ask card, a bot at work and a draft with
//! the @ list), the short topic of `short-topic-*` (the start block, a draft
//! card and the hover toolbar) and the worst case of `worst-case-*` (a long
//! title, a URL, jumbo emoji, a big download, a failed send, many typing).
//! Everything else goes to the wrapped provider.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use chrono::{Local, TimeZone as _};
use futures::channel::mpsc::UnboundedReceiver;
use gpui::{AnyWindowHandle, App, AppContext as _, Pixels, Size};

use crate::{
    app::{RootView, Services},
    data::*,
};

pub const GROUP: &str = "g-dev";
pub const MAIN: &str = "t-v21-alerts";
pub const SHORT: &str = "t-v21-launch";
pub const WORST: &str = "t-v21-worst";
const ARTIFACT: &str = "art-v21-rules";
const RUN_DONE: &str = "run-v21-forge";
const RUN_LIVE: &str = "run-v21-scout";
const BIG_FILE_SHA: &str = "v21worstcasetrace";

const MIN: i64 = 60_000;

/// Which state a scene shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sample {
    /// `main-*`: the long topic with a focused draft and the @ list.
    Main,
    /// `short-topic-*`: the start block and the hover toolbar.
    Short,
    /// `worst-case-*`: long names and titles, counts, a failed send.
    Worst,
    /// The composer's states: offline slab, quote chip, format bar, emoji.
    Compose,
    /// The short topic with the hover toolbar's emoji picker open.
    React,
}

struct Topics {
    topics: Vec<Topic>,
    messages: HashMap<String, Vec<Message>>,
    drafts: Mutex<HashMap<DraftKey, Draft>>,
    runs: Vec<BotRunSummary>,
    cards: HashMap<String, Arc<card::Card>>,
}

/// The scene provider: the wrapped one plus the sample topics.
pub struct TopicSamples {
    inner: Arc<dyn ChatProvider>,
    /// Shown as reconnecting (the composer's blocking slab).
    offline: bool,
    data: Topics,
    now: Millis,
}

fn today_at(hour: u32, minute: u32, days_ago: i64) -> Millis {
    let date = Local::now().date_naive() - chrono::Duration::days(days_ago);
    let at = date.and_hms_opt(hour, minute, 0).unwrap_or_default();
    Local
        .from_local_datetime(&at)
        .single()
        .map(|t| t.timestamp_millis())
        .unwrap_or_default()
}

fn message(topic: &str, seq: u64, author: &str, at: Millis, body: &str) -> Message {
    Message {
        id: format!("{topic}-m{seq}"),
        client_msg_id: String::new(),
        group_id: GROUP.into(),
        topic_id: topic.into(),
        author_id: author.into(),
        body: body.into(),
        mentions: Vec::new(),
        created_seq: seq,
        created_at: at,
        edited_at: None,
        deleted_at: None,
        streaming: false,
        send_state: SendState::Sent,
        quote_of: None,
        attachments: Vec::new(),
        artifact_refs: Vec::new(),
        reactions: Vec::new(),
    }
}

fn topic(id: &str, title: &str, status: TopicStatus, owner: &str, msgs: &[Message]) -> Topic {
    let last = msgs.last();
    Topic {
        id: id.into(),
        group_id: GROUP.into(),
        title: title.into(),
        status,
        waiting_on: vec![crate::fake::ME.into()],
        owner_id: Some(owner.into()),
        created_by: crate::fake::ME.into(),
        created_at: msgs.first().map_or(0, |m| m.created_at),
        root_message_id: msgs.first().map(|m| m.id.clone()).unwrap_or_default(),
        last_seq: last.map_or(0, |m| m.created_seq),
        last_activity_at: last.map_or(0, |m| m.created_at) + 10 * 24 * 60 * MIN,
        message_count: msgs.len() as u32,
        unread: 0,
        mentions: 0,
    }
}

fn ask_card() -> card::Card {
    serde_json::from_value(serde_json::json!({
        "v": 1,
        "title": "要在 staging 先打开合并规则吗？",
        "tone": "warning",
        "blocks": [
            { "type": "text", "text": "你选之前我不会改任何环境。生产要等 Scout 验收后再单独确认。" }
        ],
        "actions": [
            { "id": "a", "label": "现在在 staging 打开" },
            { "id": "b", "label": "等 Scout 验收完再开" },
            { "id": "c", "label": "先不改，回到讨论" }
        ]
    }))
    .expect("sample card parses")
}

fn draft_card() -> card::Card {
    serde_json::from_value(serde_json::json!({
        "v": 1,
        "title": "发布预览",
        "tone": "warning",
        "blocks": [
            { "type": "fields", "fields": [
                { "label": "平台", "value": "小红书" },
                { "label": "时间", "value": "周四 20:00" },
                { "label": "标题", "value": "夹夹 3.0：粉丝群的照片，一键分享回群里" }
            ]},
            { "type": "text", "text": "以前在群里找一张合照要翻半天。3.0 把群里的照片按活动自动整理好，点一下就能分享回群…" }
        ],
        "actions": [
            { "id": "publish", "label": "发布", "style": "primary" },
            { "id": "edit", "label": "修改" },
            { "id": "drop", "label": "放弃" }
        ]
    }))
    .expect("sample card parses")
}

impl TopicSamples {
    pub fn new(inner: Arc<dyn ChatProvider>, offline: bool) -> Arc<Self> {
        let now = chrono::Utc::now().timestamp_millis();
        let mut messages = HashMap::new();
        let mut cards = HashMap::new();

        // main-*: 夜间告警降噪
        let t = MAIN;
        let mut main = vec![
            message(
                t,
                1,
                "m-sentry",
                today_at(23, 58, 1),
                "**夜间巡检**：staging 共 37 条 5xx，34 条来自 `/api/share`，原因都是上游超时。生产正常。",
            ),
            message(
                t,
                2,
                "m-tata",
                today_at(9, 12, 0),
                "昨晚 staging 有 37 条 5xx 告警，其实都是同一个上游超时。@Forge 能不能把同一原因的告警合并成一条？",
            ),
            message(
                t,
                3,
                "m-lixue",
                today_at(9, 40, 0),
                "合并后要保留第一条的时间，复盘要用。",
            ),
            message(
                t,
                4,
                "m-lixue",
                today_at(9, 41, 0),
                "另外值班卡片也要跟着改，@Watch 记一下。",
            ),
            message(
                t,
                5,
                "m-coder",
                today_at(10, 2, 0),
                "**改好了**：同一路由、同一错误码，10 分钟内只发一条，后面的次数累加到这一条上。第一条的时间保留。\n\nPR 已开，等 @Scout 在 staging 验收。",
            ),
            message(
                t,
                6,
                "m-sentry",
                today_at(10, 41, 0),
                "要在 staging 先打开合并规则吗？",
            ),
            message(
                t,
                7,
                "m-lixue",
                today_at(10, 44, 0),
                "我倾向 B，staging 上周刚因为配置出过一次事。",
            ),
        ];
        main[3].reactions = vec![
            Reaction {
                emoji: "👍".into(),
                member_ids: vec![crate::fake::ME.into(), "m-tata".into()],
            },
            Reaction {
                emoji: "👀".into(),
                member_ids: vec!["m-sentry".into()],
            },
        ];
        main[4].artifact_refs = vec![ArtifactRef {
            artifact_id: ARTIFACT.into(),
            version: 2,
        }];
        cards.insert(main[5].id.clone(), Arc::new(ask_card()));
        let main_topic = topic(
            MAIN,
            "夜间告警降噪：合并重复的 5xx",
            TopicStatus::WaitingAnswer,
            "m-sentry",
            &main,
        );

        // short-topic-*: 周四小红书
        let t = SHORT;
        let short = vec![
            message(
                t,
                1,
                crate::fake::ME,
                today_at(14, 5, 0),
                "@Taro 把夹夹 3.0 上线写成一条小红书，周四晚上发。重点讲“一键分享到群”。",
            ),
            message(
                t,
                2,
                "m-taro",
                today_at(14, 6, 0),
                "收到。先写两版标题，再配图。",
            ),
            message(
                t,
                3,
                "m-taro",
                today_at(14, 31, 0),
                "**草稿好了**，按你上次喜欢的口吻写的。确认后我才会发出。",
            ),
        ];
        cards.insert(short[2].id.clone(), Arc::new(draft_card()));
        let mut short_topic = topic(
            SHORT,
            "周四小红书：夹夹 3.0 上线",
            TopicStatus::WaitingReview,
            "m-taro",
            &short,
        );
        short_topic.last_activity_at -= MIN;

        // worst-case-*
        let t = WORST;
        let mut worst = vec![
            message(
                t,
                1,
                "m-alex",
                today_at(23, 59, 1),
                "https://staging.example.com/api/v1/organizations/9f8e7d6c-5b4a-4321-9876-0123456789ab/websocket/reconnect?attempt=17&reason=network_change_while_suspended",
            ),
            message(t, 2, "m-chenchen", today_at(0, 1, 0), "👍👍👍"),
            message(
                t,
                3,
                "m-sentry",
                today_at(0, 3, 0),
                "**3 个例外**：重连第 17 次时 seq 回退；已读位置写入了旧值；推送重复 2 次。\n\n日志超过 2000 行，放进产出了。",
            ),
            message(t, 4, crate::fake::ME, today_at(0, 5, 0), "我来看。"),
        ];
        // Past two names a pill shows the count: `👀 99+`, `✅ 14`.
        let many = |n: usize, mine: bool| {
            let mut ids: Vec<String> = (0..n).map(|i| format!("m-reader-{i}")).collect();
            if mine {
                ids[0] = crate::fake::ME.into();
            }
            ids
        };
        worst[1].reactions = vec![
            Reaction {
                emoji: "👀".into(),
                member_ids: many(120, false),
            },
            Reaction {
                emoji: "✅".into(),
                member_ids: many(14, true),
            },
        ];
        worst[2].attachments = vec![FileAttachment {
            file_id: "f-v21-trace".into(),
            name:
                "websocket-reconnect-trace-2025-11-04T23-59-59-production-ios-17.4.1-build-4412.log"
                    .into(),
            size: 112_400_000,
            mime: "text/plain".into(),
            sha256: BIG_FILE_SHA.into(),
            ..Default::default()
        }];
        worst[3].send_state = SendState::Failed;
        worst[3].client_msg_id = "c-v21-failed".into();
        let mut worst_topic = topic(
            WORST,
            "【紧急】生产环境 v0.9.2 发布后 iOS 端 WebSocket 在弱网下频繁断线重连并导致消息重复与已读位置错乱的问题排查",
            TopicStatus::WaitingAnswer,
            "m-sentry",
            &worst,
        );
        worst_topic.waiting_on = vec!["m-alex".into()];
        worst_topic.last_activity_at -= 2 * MIN;

        let runs = vec![
            BotRunSummary {
                id: RUN_DONE.into(),
                bot_id: "m-coder".into(),
                group_id: GROUP.into(),
                topic_id: MAIN.into(),
                machine_id: None,
                machine_name: None,
                status: RunStatus::Done,
                requested_by: Some("m-tata".into()),
                created_at: main[4].created_at - 252_000 - 1000,
                started_at: Some(main[4].created_at - 252_000),
                ended_at: Some(main[4].created_at),
                updated_at: main[4].created_at,
                summary: None,
            },
            BotRunSummary {
                id: RUN_LIVE.into(),
                bot_id: "m-reviewer".into(),
                group_id: GROUP.into(),
                topic_id: MAIN.into(),
                machine_id: None,
                machine_name: None,
                status: RunStatus::Running,
                requested_by: Some(crate::fake::ME.into()),
                created_at: now - 135_000,
                started_at: Some(now - 134_000),
                ended_at: None,
                updated_at: now - 2_000,
                summary: None,
            },
        ];

        let mut drafts = HashMap::new();
        // short-topic-* shows the empty composer (the mockup); the compose
        // scene shows the quote chip and a draft.
        if offline {
            drafts.insert(
                DraftKey::topic(GROUP, SHORT),
                Draft {
                    body: "标题可以再短一点，**一键分享**放到开头".into(),
                    quote_of: Some(short[2].id.clone()),
                    ..Default::default()
                },
            );
        }
        drafts.insert(
            DraftKey::topic(GROUP, MAIN),
            Draft {
                body: "验收结果出来后直接发在这里，有问题先改规则，不用再问我 @Sc".into(),
                ..Default::default()
            },
        );

        messages.insert(MAIN.to_string(), main);
        messages.insert(SHORT.to_string(), short);
        messages.insert(WORST.to_string(), worst);
        Arc::new(Self {
            inner,
            offline,
            data: Topics {
                topics: vec![main_topic, short_topic, worst_topic],
                messages,
                drafts: Mutex::new(drafts),
                runs,
                cards,
            },
            now,
        })
    }

    fn mine(&self, group_id: &str, topic_id: &str) -> bool {
        group_id == GROUP && self.data.messages.contains_key(topic_id)
    }

    fn mine_key(&self, key: &DraftKey) -> bool {
        key.topic_id
            .as_deref()
            .is_some_and(|t| self.mine(&key.group_id, t))
    }
}

impl ChatProvider for TopicSamples {
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
        if self.offline {
            return ConnectionState::Reconnecting {
                attempt: 3,
                retry_in_ms: 8_000,
            };
        }
        self.inner.connection()
    }
    fn members(&self) -> Vec<Member> {
        self.inner.members()
    }
    fn groups(&self) -> Vec<GroupSummary> {
        self.inner.groups()
    }
    fn topics(&self, group_id: &str) -> Vec<Topic> {
        let mut topics = self.inner.topics(group_id);
        if group_id == GROUP {
            topics.extend(self.data.topics.iter().cloned());
            topics.sort_by_key(|t| std::cmp::Reverse(t.last_activity_at));
        }
        topics
    }
    fn todo(&self) -> Vec<TodoItem> {
        self.inner.todo()
    }
    fn messages(&self, group_id: &str, topic_id: &str, limit: usize) -> MessageWindow {
        if !self.mine(group_id, topic_id) {
            return self.inner.messages(group_id, topic_id, limit);
        }
        let all = &self.data.messages[topic_id];
        MessageWindow {
            messages: all[all.len().saturating_sub(limit)..].to_vec(),
            has_older: false,
            loading_older: false,
        }
    }
    fn load_older(&self, group_id: &str, topic_id: &str) {
        self.inner.load_older(group_id, topic_id)
    }
    fn typing(&self, group_id: &str, topic_id: &str) -> Vec<MemberId> {
        if topic_id == WORST {
            return ["m-lixue", "m-chenchen", "m-xiaowang", "m-alex", "m-old"]
                .map(String::from)
                .to_vec();
        }
        self.inner.typing(group_id, topic_id)
    }
    fn artifacts(&self, group_id: &str, topic_id: &str) -> Vec<ArtifactSummary> {
        self.inner.artifacts(group_id, topic_id)
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
    fn draft(&self, key: &DraftKey) -> Draft {
        if self.mine_key(key) {
            let drafts = self.data.drafts.lock().unwrap_or_else(|e| e.into_inner());
            return drafts.get(key).cloned().unwrap_or_default();
        }
        self.inner.draft(key)
    }
    fn save_draft_text(&self, key: &DraftKey, body: String, mentions: Vec<MemberId>) {
        if self.mine_key(key) {
            let mut drafts = self.data.drafts.lock().unwrap_or_else(|e| e.into_inner());
            let draft = drafts.entry(key.clone()).or_default();
            draft.body = body;
            draft.mentions = mentions;
            return;
        }
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
    fn file_transfer(&self, key: &str) -> Option<TransferState> {
        if key.ends_with(BIG_FILE_SHA) {
            return Some(TransferState::Running {
                done: 71_936_000,
                total: 112_400_000,
            });
        }
        self.inner.file_transfer(key)
    }
    fn cached_file(&self, file: &FileAttachment) -> Option<PathBuf> {
        self.inner.cached_file(file)
    }
    fn thumbnail(&self, group_id: &str, topic_id: &str, file: &FileAttachment) -> Option<PathBuf> {
        self.inner.thumbnail(group_id, topic_id, file)
    }
    fn topic_files(&self, group_id: &str, topic_id: &str) -> Vec<TopicFile> {
        self.inner.topic_files(group_id, topic_id)
    }
    fn artifact(&self, artifact_id: &str) -> Option<ArtifactDetail> {
        if artifact_id == ARTIFACT {
            return Some(ArtifactDetail {
                summary: ArtifactSummary {
                    id: ARTIFACT.into(),
                    topic_id: MAIN.into(),
                    kind: ArtifactKind::Page,
                    title: "告警合并规则说明".into(),
                    version: 2,
                    size_bytes: Some(18_000),
                    created_by: "m-coder".into(),
                    updated_at: self.now,
                },
                group_id: GROUP.into(),
                versions: Vec::new(),
            });
        }
        self.inner.artifact(artifact_id)
    }
    fn group_muted(&self, group_id: &str) -> bool {
        self.inner.group_muted(group_id)
    }
    fn topic_read_seq(&self, group_id: &str, topic_id: &str) -> Option<u64> {
        self.inner.topic_read_seq(group_id, topic_id)
    }
    fn topic_following(&self, group_id: &str, topic_id: &str) -> bool {
        self.inner.topic_following(group_id, topic_id)
    }
    fn runs(&self, group_id: &str, topic_id: &str) -> Vec<BotRunSummary> {
        if self.mine(group_id, topic_id) {
            return self
                .data
                .runs
                .iter()
                .filter(|r| r.topic_id == topic_id)
                .cloned()
                .collect();
        }
        self.inner.runs(group_id, topic_id)
    }
    fn run_process(&self, run_id: &str) -> RunProcess {
        let tool = |title: &str, ok: bool| ProcessStep::Tool {
            title: title.into(),
            input: None,
            ok: Some(ok),
            output: None,
            exit_code: Some(if ok { 0 } else { 1 }),
        };
        let files = |n: usize| ProcessStep::Files {
            changes: (0..n)
                .map(|i| FileChangeView {
                    path: format!("server/src/alerts/rule_{i}.rs"),
                    kind: FileChangeKind::Modified,
                })
                .collect(),
        };
        let steps = match run_id {
            RUN_DONE => vec![
                ProcessStep::TurnStarted,
                files(6),
                tool("cargo test -p alerts", true),
                tool("cargo clippy", true),
                tool("gh pr create", true),
            ],
            RUN_LIVE => vec![
                ProcessStep::TurnStarted,
                tool("deploy staging", true),
                ProcessStep::Status {
                    message: "正在 staging 上验收".into(),
                },
            ],
            _ => return self.inner.run_process(run_id),
        };
        RunProcess {
            steps,
            loaded: true,
            ..Default::default()
        }
    }
    fn follow_run(&self, run_id: &str) {
        self.inner.follow_run(run_id)
    }
    fn card(&self, group_id: &str, message_id: &str) -> Option<Arc<card::Card>> {
        if let Some(card) = self.data.cards.get(message_id) {
            return Some(card.clone());
        }
        self.inner.card(group_id, message_id)
    }
    fn card_presses(&self, group_id: &str, message_id: &str) -> Vec<CardPress> {
        self.inner.card_presses(group_id, message_id)
    }
    fn card_press_state(&self, group_id: &str, message_id: &str) -> Option<CardPressState> {
        self.inner.card_press_state(group_id, message_id)
    }
    fn subscribe(&self) -> UnboundedReceiver<Vec<Change>> {
        self.inner.subscribe()
    }
}

/// Opens the main window on one sample topic. `focus` puts the caret in the
/// composer (its draft then shows the @ list); `hover` shows the hover
/// toolbar on the second message.
fn open(
    services: Services,
    content: Size<Pixels>,
    sample: Sample,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let provider: Arc<dyn ChatProvider> =
        TopicSamples::new(services.provider.clone(), sample == Sample::Compose);
    let services = Services::new(provider, cx);
    let topic = match sample {
        Sample::Main => MAIN,
        Sample::Short => SHORT,
        Sample::Worst => WORST,
        Sample::Compose | Sample::React => SHORT,
    };
    services.nav.update(cx, |nav, cx| {
        nav.open_group(GROUP.into(), cx);
        nav.open_topic(GROUP.into(), topic.into(), cx);
        // main-* and worst-case-* show the topic view without the side panel.
        if nav.side_panel_open != matches!(sample, Sample::Short | Sample::React) {
            nav.toggle_side_panel(cx);
        }
    });
    let mut options = crate::app::main_window_options(false, cx);
    options.window_bounds = Some(gpui::WindowBounds::Windowed(gpui::Bounds::centered(
        None, content, cx,
    )));
    options.window_min_size = None;
    let (handle, root) = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| RootView::new(services, None, window, cx))
    })
    .map_err(|e| e.to_string())?;
    handle
        .update(cx, |_, window, cx| {
            let root = root.read(cx);
            let (timeline, composer) = (root.timeline().clone(), root.composer().clone());
            match sample {
                Sample::Main => {
                    composer.update(cx, |c, cx| c.preview_mention(window, cx));
                }
                Sample::Short => {
                    timeline.update(cx, |t, cx| t.preview_hover(1, cx));
                }
                Sample::Worst => {}
                Sample::React => {
                    timeline.update(cx, |t, cx| t.preview_reacting(1, cx));
                }
                Sample::Compose => {
                    composer.update(cx, |c, cx| c.preview_tools(cx));
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(handle)
}

pub fn open_main(s: Services, c: Size<Pixels>, cx: &mut App) -> Result<AnyWindowHandle, String> {
    open(s, c, Sample::Main, cx)
}

pub fn open_short(s: Services, c: Size<Pixels>, cx: &mut App) -> Result<AnyWindowHandle, String> {
    open(s, c, Sample::Short, cx)
}

pub fn open_compose(s: Services, c: Size<Pixels>, cx: &mut App) -> Result<AnyWindowHandle, String> {
    open(s, c, Sample::Compose, cx)
}

pub fn open_worst(s: Services, c: Size<Pixels>, cx: &mut App) -> Result<AnyWindowHandle, String> {
    open(s, c, Sample::Worst, cx)
}

pub fn open_react(s: Services, c: Size<Pixels>, cx: &mut App) -> Result<AnyWindowHandle, String> {
    open(s, c, Sample::React, cx)
}
