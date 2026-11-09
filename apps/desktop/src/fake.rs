//! A fake provider with rich sample data, for development, self-tests and
//! Storybook-like work on views. Everything lives in memory.
//!
//! Sample data: 5 people and 5 bots, groups and DMs, topics in every status,
//! Chinese and English Markdown messages, mentions, edits, deletes, a streaming
//! bot reply, pending and failed sends, 产出, and one topic with 10,000 messages.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

use crate::{data::*, drafts::DraftStore};

pub mod memory;
mod settings;
/// The v2 sample team of the 1.1.0 artboards.
pub mod v2;
// Desk track (1.1.0 首页).
pub mod v2_desk;
// --- inc-refs: what the server does with references.
pub mod refs;
// --- end inc-refs
// --- settings-backend track
mod settings_backend;
// --- end settings-backend track

pub const ME: &str = "m-joye";
/// The topic with 10,000 messages.
pub const LONG_TOPIC: &str = "t-perf";
pub const LONG_TOPIC_GROUP: &str = "g-dev";
pub const LONG_TOPIC_MESSAGES: usize = 10_000;
const PAGE: usize = 200;

pub struct FakeProvider {
    state: Mutex<State>,
    subscribers: Mutex<Vec<UnboundedSender<Vec<Change>>>>,
    drafts: DraftStore,
    settings: Mutex<settings::FakeSettings>,
    // --- inc-refs
    refs: refs::FakeRefs,
    // --- settings-backend track
    settings_backend: Mutex<settings_backend::FakeSettingsBackend>,
    /// 用这台电脑 was clicked in the demo.
    this_mac_used: std::sync::atomic::AtomicBool,
    /// 记得的事 and 团队约定.
    memory: memory::FakeMemory,
}

struct State {
    session: SessionState,
    connection: ConnectionState,
    members: Vec<Member>,
    groups: Vec<GroupSummary>,
    topics: Vec<Topic>,
    /// (group, topic) -> messages oldest first.
    messages: HashMap<(GroupId, TopicId), Vec<Message>>,
    /// How many messages of each topic the window shows beyond `limit`.
    extra_loaded: HashMap<(GroupId, TopicId), usize>,
    typing: HashMap<(GroupId, TopicId), Vec<MemberId>>,
    artifacts: Vec<ArtifactSummary>,
    next_id: u64,
    now: Millis,
    muted: HashSet<GroupId>,
    saved_navigation: Option<SavedNavigation>,
    cards: crate::views::cards::samples::FakeCards,
    /// Sample runs that were stopped, and when.
    stopped_runs: HashMap<String, Millis>,
    /// Sample runs whose stop is held unanswered (screenshot scenes).
    stopping_runs: HashSet<String>,
}

fn now_ms() -> Millis {
    crate::selftest::clock::now_ms()
}

const MIN: i64 = 60_000;
const HOUR: i64 = 60 * MIN;
const DAY: i64 = 24 * HOUR;

impl FakeProvider {
    /// The asks of the cards in one 话题, oldest first: a card whose state word is 等你审 or
    /// 等你回答, from its author. `None` when the 话题 has no cards: the 话题 itself then
    /// waits on me. Whether my answer reached the card is the reader's call
    /// (`topic_page::ask_answered`).
    fn card_asks(&self, group_id: &str, topic_id: &str) -> Option<Vec<crate::data::v2::AskItem>> {
        use crate::{copy::common as cc, data::v2::AskKind};
        let cards: Vec<_> = self
            .messages(group_id, topic_id, 200)
            .messages
            .into_iter()
            .filter(|m| m.deleted_at.is_none() && m.author_id != ME)
            .filter_map(|m| Some((self.card(group_id, &m.id)?, m)))
            .collect();
        if cards.is_empty() {
            return None;
        }
        Some(
            cards
                .into_iter()
                .filter_map(|(card, m)| {
                    let kind = match card.status.as_ref()?.label.as_str() {
                        l if l == cc::STATUS_WAITING_REVIEW_ME => AskKind::Review,
                        l if l == cc::STATUS_WAITING_ANSWER_ME => AskKind::Choice,
                        _ => return None,
                    };
                    Some(crate::data::v2::AskItem {
                        group_id: group_id.into(),
                        topic_id: topic_id.into(),
                        message_id: Some(m.id.clone()),
                        from: m.author_id.clone(),
                        kind,
                        title: card.title.clone(),
                        at: m.created_at,
                    })
                })
                .collect(),
        )
    }

    /// Signed in as Joye with the sample data.
    pub fn new() -> Arc<Self> {
        let provider = Arc::new(Self {
            state: Mutex::new(State::sample(now_ms(), true)),
            subscribers: Mutex::new(Vec::new()),
            drafts: DraftStore::new(),
            settings: Mutex::new(settings::FakeSettings::new()),
            refs: refs::FakeRefs::default(),
            // settings-backend track
            settings_backend: Mutex::new(settings_backend::FakeSettingsBackend::new(now_ms())),
            this_mac_used: std::sync::atomic::AtomicBool::new(false),
            memory: memory::FakeMemory::default(),
        });
        provider.seed_followed();
        provider
    }

    /// Starts at the login view; any server URL and non-empty code signs in,
    /// except the codes `0000-0000` and `WRONG-CODE-0000`.
    /// Sets the connection state, for self-test scenes of the offline states.
    pub fn set_connection(&self, state: ConnectionState) {
        self.with(|s| s.connection = state);
        self.emit(vec![Change::Connection]);
    }

    pub fn signed_out() -> Arc<Self> {
        let provider = Arc::new(Self {
            state: Mutex::new(State::sample(now_ms(), false)),
            subscribers: Mutex::new(Vec::new()),
            drafts: DraftStore::new(),
            settings: Mutex::new(settings::FakeSettings::new()),
            refs: refs::FakeRefs::default(),
            // settings-backend track
            settings_backend: Mutex::new(settings_backend::FakeSettingsBackend::new(now_ms())),
            this_mac_used: std::sync::atomic::AtomicBool::new(false),
            memory: memory::FakeMemory::default(),
        });
        provider.seed_followed();
        provider
    }

    /// Holds a stop of a sample run unanswered, so `run_stopping` stays true
    /// (screenshot scenes of `正在停止…`).
    pub fn hold_stop(&self, run_id: &str) {
        self.with(|s| s.stopping_runs.insert(run_id.to_string()));
    }

    fn emit(&self, changes: Vec<Change>) {
        let mut subs = self.subscribers.lock().unwrap();
        subs.retain(|tx| tx.unbounded_send(changes.clone()).is_ok());
    }

    fn with<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        f(&mut self.state.lock().unwrap())
    }
}

impl State {
    fn sample(now: Millis, signed_in: bool) -> State {
        let mut s = State {
            session: SessionState::SignedOut { last_error: None },
            connection: ConnectionState::Offline,
            members: members(),
            groups: Vec::new(),
            topics: Vec::new(),
            messages: HashMap::new(),
            extra_loaded: HashMap::new(),
            typing: HashMap::new(),
            artifacts: Vec::new(),
            next_id: 1,
            now,
            muted: HashSet::new(),
            saved_navigation: None,
            cards: Default::default(),
            stopped_runs: HashMap::new(),
            stopping_runs: HashSet::new(),
        };
        s.fill_sample();
        s.fill_cards();
        // One muted 群, as in the mockups (the sidebar's bell-off row).
        s.muted.insert("g-all".into());
        if signed_in {
            s.sign_in("https://chat.example.invalid".into());
        }
        s
    }

    fn sign_in(&mut self, server_url: String) {
        let me = self.members.iter().find(|m| m.id == ME).cloned().unwrap();
        self.session = SessionState::SignedIn { me, server_url };
        self.connection = ConnectionState::Online;
    }

    fn id(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{prefix}-{:06}", self.next_id)
    }

    fn group_mut(&mut self, group_id: &str) -> Option<&mut GroupSummary> {
        self.groups.iter_mut().find(|g| g.id == group_id)
    }

    fn add_group(
        &mut self,
        id: &str,
        kind: GroupKind,
        name: &str,
        members: &[&str],
        peer: Option<&str>,
    ) {
        self.groups.push(GroupSummary {
            id: id.into(),
            kind,
            name: name.into(),
            member_ids: members.iter().map(|m| m.to_string()).collect(),
            dm_peer: peer.map(Into::into),
            last_seq: 0,
            read_seq: 0,
            unread: 0,
            mentions: 0,
            last_activity_at: 0,
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn add_topic(
        &mut self,
        group_id: &str,
        id: &str,
        title: &str,
        status: TopicStatus,
        waiting_on: &[&str],
        owner: Option<&str>,
        messages: &[(&str, &str, i64)],
        unread: u32,
    ) {
        let created_by = messages.first().map(|m| m.0).unwrap_or(ME);
        let created_at = self.now - messages.first().map(|m| m.2).unwrap_or(0);
        self.topics.push(Topic {
            id: id.into(),
            group_id: group_id.into(),
            title: title.into(),
            status,
            waiting_on: waiting_on.iter().map(|m| m.to_string()).collect(),
            owner_id: owner.map(Into::into),
            created_by: created_by.into(),
            created_at,
            root_message_id: String::new(),
            last_seq: 0,
            last_activity_at: created_at,
            message_count: 0,
            unread: 0,
            mentions: 0,
        });
        for (author, body, ago) in messages {
            self.push_message(group_id, id, author, body, self.now - ago, SendState::Sent);
        }
        let mentions = self
            .messages
            .get(&(group_id.to_string(), id.to_string()))
            .map(|list| {
                list.iter()
                    .rev()
                    .take(unread as usize)
                    .filter(|m| m.mentions.iter().any(|x| x == ME))
                    .count() as u32
            })
            .unwrap_or(0);
        let topic = self.topics.iter_mut().find(|t| t.id == id).unwrap();
        topic.unread = unread;
        topic.mentions = mentions;
        let group = self.group_mut(group_id).unwrap();
        group.unread += unread;
        group.mentions += mentions;
    }

    fn push_message(
        &mut self,
        group_id: &str,
        topic_id: &str,
        author: &str,
        body: &str,
        at: Millis,
        send_state: SendState,
    ) -> Message {
        let id = self.id("msg");
        let client_msg_id = self.id("c");
        let mentions = self
            .members
            .iter()
            .filter(|m| body.contains(&format!("@{}", m.name)))
            .map(|m| m.id.clone())
            .collect();
        let group = self.group_mut(group_id).expect("group exists");
        let seq = if send_state == SendState::Sent {
            group.last_seq += 1;
            group.last_seq
        } else {
            0
        };
        group.last_activity_at = group.last_activity_at.max(at);
        if author == ME {
            group.read_seq = group.last_seq;
        }
        let message = Message {
            id: if send_state == SendState::Sent {
                id
            } else {
                String::new()
            },
            client_msg_id,
            group_id: group_id.into(),
            topic_id: topic_id.into(),
            author_id: author.into(),
            body: body.into(),
            mentions,
            created_seq: seq,
            created_at: at,
            edited_at: None,
            deleted_at: None,
            streaming: false,
            send_state,
            quote_of: None,
            attachments: Vec::new(),
            artifact_refs: Vec::new(),
            reactions: Vec::new(),
        };
        let topic = self
            .topics
            .iter_mut()
            .find(|t| t.id == topic_id && t.group_id == group_id)
            .unwrap();
        if topic.root_message_id.is_empty() {
            topic.root_message_id = message.id.clone();
        }
        topic.message_count += 1;
        topic.last_seq = topic.last_seq.max(seq);
        topic.last_activity_at = topic.last_activity_at.max(at);
        self.messages
            .entry((group_id.into(), topic_id.into()))
            .or_default()
            .push(message.clone());
        message
    }

    /// The card sample topic (`views::cards::samples`).
    fn fill_cards(&mut self) {
        use crate::views::cards::samples as cs;
        let samples = cs::samples();
        let messages: Vec<(&str, &str, i64)> = samples
            .iter()
            .map(|s| (s.author, s.card.title.as_str(), s.minutes_ago * MIN))
            .collect();
        self.add_topic(
            cs::GROUP,
            cs::TOPIC,
            cs::TOPIC_TITLE,
            // The sample cards ask me to review and to answer (T: the 话题 reads 等你…).
            TopicStatus::WaitingReview,
            &[ME],
            None,
            &messages,
            0,
        );
        let ids: Vec<MessageId> = self.messages[&(cs::GROUP.to_string(), cs::TOPIC.to_string())]
            .iter()
            .map(|m| m.id.clone())
            .collect();
        for (id, sample) in ids.iter().zip(&samples) {
            self.cards.add(id, sample, self.now);
        }
    }

    fn last_message_mut(&mut self, group_id: &str, topic_id: &str) -> &mut Message {
        self.messages
            .get_mut(&(group_id.into(), topic_id.into()))
            .unwrap()
            .last_mut()
            .unwrap()
    }

    fn fill_sample(&mut self) {
        use TopicStatus::*;
        const XIAOWANG: &str = "m-xiaowang";
        const LIXUE: &str = "m-lixue";
        const CHENCHEN: &str = "m-chenchen";
        const ALEX: &str = "m-alex";
        const CODER: &str = "m-coder";
        const REVIEWER: &str = "m-reviewer";
        const SENTRY: &str = "m-sentry";
        const DOCS: &str = "m-docs";
        const DEPLOY: &str = "m-deploy";
        const TATA: &str = "m-tata";
        const LEDGER: &str = "m-ledger";
        const TARO: &str = "m-taro";
        let everyone = [
            ME, XIAOWANG, LIXUE, CHENCHEN, ALEX, DEPLOY, TATA, DOCS, CODER, SENTRY, REVIEWER,
            LEDGER, TARO,
        ];

        self.add_group("g-dev", GroupKind::Group, "产品研发", &everyone, None);
        self.add_group(
            "g-design",
            GroupKind::Group,
            "设计评审",
            &[ME, LIXUE, CHENCHEN, DOCS],
            None,
        );
        self.add_group(
            "g-ops",
            GroupKind::Group,
            "运维告警",
            &[ME, ALEX, SENTRY, DEPLOY],
            None,
        );
        self.add_group("g-all", GroupKind::Group, "全员", &everyone, None);
        self.add_group(
            "g-weekly",
            GroupKind::Group,
            "Weekly sync",
            &[ME, XIAOWANG, LIXUE, ALEX],
            None,
        );
        self.add_group(
            "d-xiaowang",
            GroupKind::Dm,
            "小王",
            &[ME, XIAOWANG],
            Some(XIAOWANG),
        );
        self.add_group("d-coder", GroupKind::Dm, "Forge", &[ME, CODER], Some(CODER));
        self.add_group(
            "d-alex",
            GroupKind::Dm,
            "Alex Chen",
            &[ME, ALEX],
            Some(ALEX),
        );

        // The 群聊 of 产品研发 and of the 私聊 with Forge (1.1.1): plain chat;
        // a 话题 opens from a message when it turns into work.
        let dev_chat = crate::data::main_topic_id("g-dev");
        self.add_topic(
            "g-dev",
            &dev_chat,
            "产品研发",
            Open,
            &[],
            None,
            &[
                (XIAOWANG, "大家早，下午 3 点对一下 1.1.1 的发布？", 7 * HOUR),
                (
                    LIXUE,
                    "可以，我先把设计稿的改动列出来。",
                    7 * HOUR - 4 * MIN,
                ),
                (
                    ME,
                    "好。@Jojo 帮我看下 staging 昨晚的报错有没有变多",
                    6 * HOUR,
                ),
                (
                    DEPLOY,
                    "看了：昨晚 12 条，比前天少 5 条，都是重连超时，没有新的。",
                    6 * HOUR - 3 * MIN,
                ),
                (
                    ALEX,
                    "重连超时那几条我想单独查一下，可能和睡眠唤醒有关。",
                    40 * MIN,
                ),
                (CHENCHEN, "下午的会我晚 10 分钟到 🙏", 12 * MIN),
            ],
            1,
        );
        let forge_chat = crate::data::main_topic_id("d-coder");
        self.add_topic(
            "d-coder",
            &forge_chat,
            "",
            Open,
            &[],
            None,
            &[
                (ME, "今天的构建为什么慢了一倍？", 2 * HOUR),
                (
                    CODER,
                    "依赖缓存昨晚被清掉了，第一次全量编译。之后会回到 4 分钟左右。",
                    2 * HOUR - 2 * MIN,
                ),
            ],
            0,
        );

        self.add_topic(
            "g-dev",
            "t-login",
            "登录页支持邀请码和粘贴凭证",
            InProgress,
            &[],
            Some(CODER),
            &[
                (ME, "@Forge 登录页需要支持两种方式：\n\n1. 邀请码 `XXXX-XXXX`\n2. 粘贴已有的登录凭证\n\n服务器地址默认填 staging。", 3 * HOUR),
                (CODER, "收到，我来做。预计 30 分钟内给出第一版。", 3 * HOUR - MIN),
                (CODER, "进展：表单和校验已完成，正在接 Keychain。\n\n```rust\nlet entry = keyring::Entry::new(service, account)?;\nentry.set_password(&token)?;\n```", 2 * HOUR),
                (XIAOWANG, "错误提示要区分「邀请码无效」和「连不上服务器」吗？", 90 * MIN),
                (ME, "要区分，文案用目录里的。", 80 * MIN),
                (CODER, "@Joye 好的，已经区分，你看一下。", 10 * MIN),
            ],
            1,
        );
        self.add_topic(
            "g-dev",
            "t-api",
            "WebSocket 断线重连后消息重复",
            WaitingAnswer,
            &[ME],
            Some(ALEX),
            &[
                (ALEX, "Reproduced: after sleep/wake the client sends `hello` with stale cursors and receives 3 duplicated events.", 5 * HOUR),
                (ALEX, "@Joye 两个方案：\n- A：客户端按 seq 去重（简单）\n- B：服务端在 hello 时校验 cursor\n\n你倾向哪个？", 4 * HOUR),
            ],
            1,
        );
        self.add_topic(
            "g-dev",
            "t-review",
            "PR #42: 话题列表虚拟滚动",
            WaitingReview,
            &[ME],
            Some(XIAOWANG),
            &[
                (XIAOWANG, "PR 已提交：话题列表改为虚拟滚动，1 万个话题滚动保持 60 fps。@Joye 麻烦审一下。", 6 * HOUR),
                (REVIEWER, "自动检查通过 ✅\n\n| 检查 | 结果 |\n| --- | --- |\n| cargo test | 通过 |\n| clippy | 2 个提示 |", 6 * HOUR - 5 * MIN),
            ],
            2,
        );
        self.add_topic(
            "g-dev",
            "t-review-other",
            "Composer: paste images from the clipboard",
            WaitingReview,
            &[LIXUE],
            // Joye owns the review: Forge's only state is its run in t-login.
            Some(ME),
            &[
                (CODER, "Done. Pasting a PNG now uploads it and shows a preview. Waiting for @李雪 to review the UX.", DAY + 2 * HOUR),
            ],
            0,
        );
        self.add_topic(
            "g-dev",
            "t-blocked",
            "DMG 签名：缺少开发者证书",
            Blocked,
            &[],
            Some(DEPLOY),
            &[
                (
                    DEPLOY,
                    "打包失败：`codesign` 找不到证书。V1 先发未签名的 DMG？",
                    2 * DAY,
                ),
                (
                    ME,
                    "先发未签名的，安装指南里写清楚右键打开。",
                    2 * DAY - HOUR,
                ),
                (
                    DEPLOY,
                    "还卡在 notarization 的步骤，需要先去掉。",
                    DAY + 20 * HOUR,
                ),
            ],
            0,
        );
        self.add_topic(
            "g-dev",
            "t-done",
            "搜索支持中文两字词",
            Done,
            &[],
            Some(ALEX),
            &[
                (
                    ALEX,
                    "FTS5 + bigram tokenizer 已上线，`测试` 这样的两字词可以搜到了。",
                    3 * DAY,
                ),
                (
                    LIXUE,
                    "验证过了，群内和全局搜索都正常 👍",
                    3 * DAY - 2 * HOUR,
                ),
                (ME, "很好，关掉。", 3 * DAY - 3 * HOUR),
            ],
            0,
        );
        self.add_topic(
            "g-dev",
            "t-open",
            "Idea: 每天早上自动总结昨天的话题",
            Open,
            &[],
            None,
            &[(
                LIXUE,
                "大家早上来先看一遍昨天的进展太花时间了，能不能每天早上自动出一份总结？",
                4 * DAY,
            )],
            0,
        );
        self.add_topic(
            "g-dev",
            "t-stream",
            "生成 V1 发布说明",
            InProgress,
            &[],
            Some(DOCS),
            &[
                (ME, "@Pocket 根据这周合并的 PR 写一份发布说明。", 15 * MIN),
                (DOCS, "## V1 发布说明（草稿）\n\n### 新功能\n- 群和私聊\n- 话题状态与待办\n- 中文搜索\n\n### 修复\n- 断线重连后", MIN),
            ],
            0,
        );
        self.last_message_mut("g-dev", "t-stream").streaming = true;

        // Edited, deleted, pending and failed messages.
        self.add_topic(
            "g-dev",
            "t-edits",
            "周五 demo 安排",
            InProgress,
            &[],
            Some(ME),
            &[
                (XIAOWANG, "周五下午 3 点 demo，每人 10 分钟。", 26 * HOUR),
                (LIXUE, "这条我发错了", 25 * HOUR),
                (CHENCHEN, "我准备设计稿部分。", 24 * HOUR),
            ],
            0,
        );
        {
            let msgs = self
                .messages
                .get_mut(&("g-dev".into(), "t-edits".into()))
                .unwrap();
            msgs[0].body = "周五下午 4 点 demo，每人 10 分钟。".into();
            msgs[0].edited_at = Some(self.now - 25 * HOUR);
            msgs[1].body.clear();
            msgs[1].deleted_at = Some(self.now - 25 * HOUR + MIN);
        }
        self.push_message(
            "g-dev",
            "t-edits",
            ME,
            "我来讲整体进度。",
            self.now - 30_000,
            SendState::Pending,
        );
        self.push_message(
            "g-dev",
            "t-edits",
            ME,
            "附上上周的数据。",
            self.now - 20_000,
            SendState::Failed,
        );

        // The long topic.
        self.add_topic(
            LONG_TOPIC_GROUP,
            LONG_TOPIC,
            "V1 性能压测：一万条消息",
            InProgress,
            &[],
            Some(SENTRY),
            &[],
            0,
        );
        let authors = [ME, XIAOWANG, LIXUE, ALEX, CODER, SENTRY];
        let samples = [
            "压测消息：中文内容，检查换行和字体渲染。",
            "Load test message with **bold**, `code` and a [link](https://example.com).",
            "这是一条比较长的消息，用来检查虚拟列表在不同高度的行之间滚动是否平滑。这是一条比较长的消息，用来检查虚拟列表在不同高度的行之间滚动是否平滑。",
            "- 列表项一\n- 列表项二\n- 列表项三",
            "p95 delivery 212 ms, p99 280 ms ✅",
            "```\ncargo bench -p chat-core\n```",
        ];
        let start = self.now - 30 * DAY;
        for i in 0..LONG_TOPIC_MESSAGES {
            let body = format!("#{} {}", i + 1, samples[i % samples.len()]);
            let at = start + (i as i64) * (30 * DAY / LONG_TOPIC_MESSAGES as i64);
            self.push_message(
                LONG_TOPIC_GROUP,
                LONG_TOPIC,
                authors[i % authors.len()],
                &body,
                at,
                SendState::Sent,
            );
        }

        self.add_topic(
            "g-design",
            "t-sidebar",
            "侧边栏的未读和@标记",
            WaitingReview,
            &[CHENCHEN],
            Some(LIXUE),
            &[
                (
                    LIXUE,
                    "@陈晨 新的侧边栏稿子：未读用数字，@用红点。",
                    7 * HOUR,
                ),
                (
                    DOCS,
                    "已把设计说明整理成产出：《侧边栏规范 v2》。",
                    6 * HOUR,
                ),
            ],
            2,
        );
        self.add_topic(
            "g-design",
            "t-dark",
            "Dark mode colors",
            Done,
            &[],
            Some(CHENCHEN),
            &[(
                CHENCHEN,
                "Dark palette is final. Contrast checked for all text tokens.",
                5 * DAY,
            )],
            0,
        );
        self.add_topic(
            "g-ops",
            "t-alert",
            "staging 健康检查失败",
            Blocked,
            &[ALEX],
            Some(SENTRY),
            &[
                (
                    SENTRY,
                    "⚠️ `GET /v1/health` 连续 3 次超时（staging）。",
                    40 * MIN,
                ),
                (
                    SENTRY,
                    "@Alex Chen 需要你看一下 Durable Object 的日志。",
                    39 * MIN,
                ),
                (ALEX, "在看了。", 20 * MIN),
            ],
            3,
        );
        self.add_topic(
            "g-ops",
            "t-backup",
            "每日备份",
            Done,
            &[],
            Some(DEPLOY),
            &[(
                DEPLOY,
                "今天的备份已完成：群日志 128 MB，数据库 3 MB。",
                9 * HOUR,
            )],
            0,
        );
        self.add_topic(
            "g-all",
            "t-welcome",
            "欢迎新同学 🎉",
            Open,
            &[],
            None,
            &[
                (
                    ME,
                    "欢迎 @小王 @李雪 加入！安装指南在这个话题的产出里。",
                    10 * DAY,
                ),
                (XIAOWANG, "谢谢！已经装好了。", 10 * DAY - HOUR),
            ],
            0,
        );
        self.add_topic(
            "g-weekly",
            "t-weekly",
            "Week 44 notes",
            Done,
            &[],
            Some(ME),
            &[(
                ME,
                "Notes are in the 产出 of this topic. Next sync Monday 10:00.",
                2 * DAY,
            )],
            0,
        );
        self.add_topic(
            "d-xiaowang",
            "t-dm-1",
            "实习周报",
            WaitingAnswer,
            &[ME],
            None,
            &[(
                XIAOWANG,
                "这周做完了话题列表，下周想试试做时间线，可以吗？",
                50 * MIN,
            )],
            1,
        );
        self.add_topic(
            "d-coder",
            "t-dm-2",
            "清理旧的构建缓存",
            Done,
            &[],
            Some(CODER),
            &[
                (
                    ME,
                    "帮我清理一下 Mac mini 上超过 7 天的构建缓存。",
                    8 * HOUR,
                ),
                (CODER, "完成，释放了 42 GB。", 7 * HOUR),
            ],
            0,
        );
        self.add_topic(
            "d-alex",
            "t-dm-3",
            "D1 schema question",
            Open,
            &[],
            None,
            &[(
                ALEX,
                "Should `audit_log` keep the full request body or only a diff?",
                3 * HOUR,
            )],
            1,
        );

        self.typing
            .insert(("g-dev".into(), "t-login".into()), vec![CODER.into()]);

        for (topic, kind, title, version, size, by, ago) in [
            (
                "t-login",
                ArtifactKind::Page,
                "登录页交互说明",
                3,
                None,
                CODER,
                HOUR,
            ),
            (
                "t-login",
                ArtifactKind::Image,
                "login-screenshot.png",
                1,
                Some(284_000),
                CODER,
                30 * MIN,
            ),
            (
                "t-api",
                ArtifactKind::Decision,
                "去重放在客户端还是服务端",
                1,
                None,
                ALEX,
                4 * HOUR,
            ),
            (
                "t-review",
                ArtifactKind::File,
                "bench-results.csv",
                2,
                Some(18_400),
                REVIEWER,
                6 * HOUR,
            ),
            (
                "t-sidebar",
                ArtifactKind::Page,
                "侧边栏规范 v2",
                2,
                None,
                DOCS,
                6 * HOUR,
            ),
            (
                "t-welcome",
                ArtifactKind::File,
                "安装指南.pdf",
                1,
                Some(2_400_000),
                ME,
                10 * DAY,
            ),
            (
                "t-weekly",
                ArtifactKind::Page,
                "Week 44 notes",
                1,
                None,
                ME,
                2 * DAY,
            ),
        ] {
            let id = self.id("art");
            self.artifacts.push(ArtifactSummary {
                id,
                topic_id: topic.into(),
                kind,
                title: title.into(),
                version,
                size_bytes: size,
                created_by: by.into(),
                updated_at: self.now - ago,
            });
        }
    }
}

fn members() -> Vec<Member> {
    let m = |id: &str, kind, name: &str, role: &str, level, online| Member {
        id: id.into(),
        kind,
        name: name.into(),
        role: Some(role.into()),
        level,
        avatar_url: None,
        deactivated: false,
        online,
        character: None,
    };
    // Sample bots with sample names, characters and 岗位. The ids stay the
    // sample ids the other samples use.
    let bot = |id: &str, name: &str, role: &str, level, character, online| Member {
        character: Some(character),
        ..m(id, MemberKind::Bot, name, role, level, online)
    };
    use crate::characters::CharacterId as C;
    use MemberKind::*;
    use MemberLevel::{Member as Regular, Owner};
    let mut list = vec![
        m(ME, Human, "Joye", "负责人", Owner, true),
        m("m-xiaowang", Human, "小王", "前端实习生", Regular, true),
        m("m-lixue", Human, "李雪", "产品实习生", Regular, false),
        m("m-chenchen", Human, "陈晨", "设计实习生", Regular, true),
        m("m-alex", Human, "Alex Chen", "后端实习生", Regular, true),
        bot("m-deploy", "Jojo", "主助手", Owner, C::Jojo, true),
        bot("m-tata", "Tata", "产品负责人", Regular, C::O03, true),
        bot("m-docs", "Pocket", "幕僚长", Regular, C::O05, true),
        bot("m-coder", "Forge", "开发", Regular, C::O08, true),
        bot("m-sentry", "Watch", "运维", Regular, C::O11, true),
        bot("m-reviewer", "Scout", "测试验收", Regular, C::O07, true),
        bot("m-ledger", "Ledger", "数据", Regular, C::H02, false),
        bot("m-taro", "Taro", "社媒发布", Regular, C::H03, true),
    ];
    list.push(Member {
        deactivated: true,
        online: false,
        ..m("m-old", Human, "前实习生", "已离开", Regular, false)
    });
    list
}

fn sorted_topics(state: &State, group_id: &str) -> Vec<Topic> {
    let mut topics: Vec<Topic> = state
        .topics
        .iter()
        // The 群聊 is the 群's chat, not one of its 话题.
        .filter(|t| t.group_id == group_id && !crate::data::is_main_topic(&t.id))
        .cloned()
        .collect();
    topics.sort_by_key(|a| std::cmp::Reverse(a.last_activity_at));
    topics
}

impl ChatProvider for FakeProvider {
    fn session(&self) -> SessionState {
        self.with(|s| s.session.clone())
    }

    /// The signed-in account and a second one on a local server (the
    /// account menu's sample).
    fn accounts(&self) -> Vec<crate::data::AccountEntry> {
        let SessionState::SignedIn { me, server_url } = self.session() else {
            return Vec::new();
        };
        vec![
            crate::data::AccountEntry {
                id: format!("{server_url}#{}", me.id),
                name: me.name.clone(),
                role: me.role.clone().filter(|r| !r.trim().is_empty()),
                server_url,
                active: true,
            },
            crate::data::AccountEntry {
                id: format!("http://127.0.0.1:8787#{}", me.id),
                name: me.name,
                role: None,
                server_url: "http://127.0.0.1:8787".into(),
                active: false,
            },
        ]
    }

    fn login(&self, request: LoginRequest) {
        let ok = match &request.credential {
            Credential::InviteCode(code) => !matches!(
                code.as_str(),
                "WRONG-CODE-0000" | "0000-0000" | "0000-0000-0000"
            ),
            Credential::Token(token) => token.len() > 3,
            Credential::EmailCode { code, .. } => code != "000000",
        };
        // A `.invalid` host (RFC 2606) never answers: the login-unreachable scene.
        let unreachable = request.server_url.contains(".invalid");
        self.with(|s| {
            if unreachable {
                s.session = SessionState::SignedOut {
                    last_error: Some(LoginError::ServerUnreachable),
                };
            } else if ok && !request.server_url.trim().is_empty() {
                s.sign_in(request.server_url.clone());
            } else {
                s.session = SessionState::SignedOut {
                    last_error: Some(LoginError::BadCredential),
                };
            }
        });
        self.emit(vec![
            Change::Session,
            Change::Connection,
            Change::Groups,
            Change::Members,
            Change::Todo,
        ]);
    }

    fn logout(&self) {
        self.with(|s| {
            s.session = SessionState::SignedOut { last_error: None };
            s.connection = ConnectionState::Offline;
        });
        self.emit(vec![Change::Session, Change::Connection]);
    }

    fn connection(&self) -> ConnectionState {
        self.with(|s| s.connection)
    }

    fn members(&self) -> Vec<Member> {
        self.with(|s| s.members.clone())
    }

    fn groups(&self) -> Vec<GroupSummary> {
        self.with(|s| {
            let mut groups = s.groups.clone();
            groups.sort_by_key(|a| std::cmp::Reverse(a.last_activity_at));
            groups
        })
    }

    fn topics(&self, group_id: &str) -> Vec<Topic> {
        self.with(|s| sorted_topics(s, group_id))
    }

    fn topic(&self, group_id: &str, topic_id: &str) -> Option<Topic> {
        self.with(|s| {
            s.topics
                .iter()
                .find(|t| t.id == topic_id && t.group_id == group_id)
                .cloned()
        })
    }

    fn todo(&self) -> Vec<TodoItem> {
        self.with(|s| {
            let mut items: Vec<TodoItem> = s
                .topics
                .iter()
                .filter_map(|t| {
                    let reason =
                        if t.status != TopicStatus::Done && t.waiting_on.iter().any(|m| m == ME) {
                            TodoReason::WaitingOnMe
                        } else if t.mentions > 0 {
                            TodoReason::Mentioned
                        } else {
                            return None;
                        };
                    let group_name = s.groups.iter().find(|g| g.id == t.group_id)?.name.clone();
                    Some(TodoItem {
                        group_id: t.group_id.clone(),
                        group_name,
                        topic: t.clone(),
                        reason,
                    })
                })
                .collect();
            items.sort_by_key(|a| std::cmp::Reverse(a.topic.last_activity_at));
            items
        })
    }

    fn messages(&self, group_id: &str, topic_id: &str, limit: usize) -> MessageWindow {
        self.with(|s| {
            let key = (group_id.to_string(), topic_id.to_string());
            let Some(all) = s.messages.get(&key) else {
                return MessageWindow::default();
            };
            let want = limit + s.extra_loaded.get(&key).copied().unwrap_or(0);
            let start = all.len().saturating_sub(want);
            MessageWindow {
                messages: all[start..].to_vec(),
                has_older: start > 0,
                loading_older: false,
            }
        })
    }

    fn load_older(&self, group_id: &str, topic_id: &str) {
        self.with(|s| {
            *s.extra_loaded
                .entry((group_id.into(), topic_id.into()))
                .or_default() += PAGE
        });
        self.emit(vec![Change::Messages {
            group_id: group_id.into(),
            topic_id: topic_id.into(),
        }]);
    }

    fn typing(&self, group_id: &str, topic_id: &str) -> Vec<MemberId> {
        self.with(|s| {
            s.typing
                .get(&(group_id.into(), topic_id.into()))
                .cloned()
                .unwrap_or_default()
        })
    }

    fn artifacts(&self, group_id: &str, topic_id: &str) -> Vec<ArtifactSummary> {
        self.with(|s| {
            let in_group = s
                .topics
                .iter()
                .any(|t| t.id == topic_id && t.group_id == group_id);
            let mut list: Vec<_> = s
                .artifacts
                .iter()
                .filter(|a| in_group && a.topic_id == topic_id)
                .cloned()
                .collect();
            list.sort_by_key(|a| std::cmp::Reverse(a.updated_at));
            list
        })
    }

    fn send(&self, request: SendRequest) -> ClientMsgId {
        let id = self.with(|s| {
            let known = s
                .topics
                .iter()
                .any(|t| t.id == request.topic_id && t.group_id == request.group_id);
            // The 群聊 starts with its first message, as on the server.
            if !known
                && request.topic_id == crate::data::main_topic_id(&request.group_id)
                && s.groups.iter().any(|g| g.id == request.group_id)
            {
                let at = now_ms();
                s.topics.push(Topic {
                    id: request.topic_id.clone(),
                    group_id: request.group_id.clone(),
                    title: String::new(),
                    status: TopicStatus::Open,
                    waiting_on: Vec::new(),
                    owner_id: None,
                    created_by: ME.into(),
                    created_at: at,
                    root_message_id: String::new(),
                    last_seq: 0,
                    last_activity_at: at,
                    message_count: 0,
                    unread: 0,
                    mentions: 0,
                });
            } else if !known {
                return String::new();
            }
            let at = now_ms();
            let mut msg = s.push_message(
                &request.group_id,
                &request.topic_id,
                ME,
                &request.body,
                at,
                SendState::Sent,
            );
            let last = s.last_message_mut(&request.group_id, &request.topic_id);
            last.quote_of = request.quote_of.clone();
            if !request.mentions.is_empty() {
                last.mentions = request.mentions.clone();
            }
            msg.quote_of = request.quote_of;
            msg.client_msg_id
        });
        self.emit(vec![
            Change::Messages {
                group_id: request.group_id.clone(),
                topic_id: request.topic_id,
            },
            Change::Topics {
                group_id: request.group_id,
            },
            Change::Groups,
        ]);
        id
    }

    fn start_topic(&self, request: StartTopicRequest) -> ClientMsgId {
        let (id, topic_id) = self.with(|s| {
            if !s.groups.iter().any(|g| g.id == request.group_id) {
                return (String::new(), String::new());
            }
            let title = request
                .title
                .clone()
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| {
                    request
                        .body
                        .lines()
                        .next()
                        .unwrap_or("")
                        .chars()
                        .take(80)
                        .collect()
                });
            let topic_id = s.id("t");
            let at = now_ms();
            s.topics.push(Topic {
                id: topic_id.clone(),
                group_id: request.group_id.clone(),
                title,
                status: TopicStatus::Open,
                waiting_on: Vec::new(),
                owner_id: None,
                created_by: ME.into(),
                created_at: at,
                root_message_id: String::new(),
                last_seq: 0,
                last_activity_at: at,
                message_count: 0,
                unread: 0,
                mentions: 0,
            });
            let msg = s.push_message(
                &request.group_id,
                &topic_id,
                ME,
                &request.body,
                at,
                SendState::Sent,
            );
            // 开个话题: the first message quotes the 群聊 message.
            s.last_message_mut(&request.group_id, &topic_id).quote_of = request.quote_of.clone();
            (msg.client_msg_id, topic_id)
        });
        if !topic_id.is_empty() {
            self.emit(vec![
                Change::Topics {
                    group_id: request.group_id.clone(),
                },
                Change::Messages {
                    group_id: request.group_id,
                    topic_id,
                },
                Change::Groups,
            ]);
        }
        id
    }

    fn retry_send(&self, group_id: &str, client_msg_id: &str) {
        let topic = self.with(|s| {
            let key = s.messages.iter().find_map(|((g, t), list)| {
                (g == group_id
                    && list.iter().any(|m| {
                        m.client_msg_id == client_msg_id && m.send_state == SendState::Failed
                    }))
                .then(|| (g.clone(), t.clone()))
            })?;
            let id = s.id("msg");
            let group = s.group_mut(group_id)?;
            group.last_seq += 1;
            let seq = group.last_seq;
            let list = s.messages.get_mut(&key)?;
            let ix = list.iter().position(|m| m.client_msg_id == client_msg_id)?;
            let mut m = list.remove(ix);
            m.id = id;
            m.created_seq = seq;
            m.send_state = SendState::Sent;
            list.push(m);
            if let Some(t) = s
                .topics
                .iter_mut()
                .find(|t| t.id == key.1 && t.group_id == key.0)
            {
                t.last_seq = seq;
            }
            Some(key.1)
        });
        if let Some(topic_id) = topic {
            self.emit(vec![Change::Messages {
                group_id: group_id.into(),
                topic_id,
            }]);
        }
    }

    fn edit(&self, group_id: &str, message_id: &str, body: String) {
        let topic = self.with(|s| {
            for ((g, t), list) in s.messages.iter_mut() {
                if g != group_id {
                    continue;
                }
                if let Some(m) = list
                    .iter_mut()
                    .find(|m| m.id == message_id && m.author_id == ME)
                {
                    m.body = body;
                    m.edited_at = Some(now_ms());
                    return Some(t.clone());
                }
            }
            None
        });
        if let Some(topic_id) = topic {
            self.emit(vec![Change::Messages {
                group_id: group_id.into(),
                topic_id,
            }]);
        }
    }

    fn delete(&self, group_id: &str, message_id: &str) {
        let topic = self.with(|s| {
            for ((g, t), list) in s.messages.iter_mut() {
                if g != group_id {
                    continue;
                }
                if let Some(m) = list.iter_mut().find(|m| m.id == message_id) {
                    m.body.clear();
                    m.deleted_at = Some(now_ms());
                    let topic = t.clone();
                    s.cards.remove(message_id);
                    return Some(topic);
                }
            }
            None
        });
        // inc-refs: references to the deleted message show 原消息已撤回.
        if topic.is_some() {
            self.with(|s| {
                let mut all: Vec<&mut Message> =
                    s.messages.values_mut().flat_map(|l| l.iter_mut()).collect();
                self.refs.source_deleted(message_id, &mut all);
            });
        }
        if let Some(topic_id) = topic {
            self.emit(vec![Change::Messages {
                group_id: group_id.into(),
                topic_id,
            }]);
        }
    }

    fn react(&self, group_id: &str, topic_id: &str, message_id: &str, emoji: &str, on: bool) {
        let changed = self.with(|s| {
            let list = s
                .messages
                .get_mut(&(group_id.to_string(), topic_id.to_string()))?;
            let m = list
                .iter_mut()
                .find(|m| m.id == message_id && m.deleted_at.is_none())?;
            crate::data::apply_reaction(&mut m.reactions, emoji, ME, on);
            Some(())
        });
        if changed.is_some() {
            self.emit(vec![Change::Messages {
                group_id: group_id.into(),
                topic_id: topic_id.into(),
            }]);
        }
    }

    fn quoted_message(
        &self,
        _group_id: &str,
        _quoting_id: &str,
        quoted_id: &str,
    ) -> Option<QuotedMessage> {
        self.with(|s| {
            s.messages
                .values()
                .flatten()
                .find(|m| m.id == quoted_id)
                .map(|m| QuotedMessage {
                    author_id: m.author_id.clone(),
                    body: m.body.clone(),
                    deleted: m.deleted_at.is_some(),
                })
        })
    }

    fn rename_topic(&self, group_id: &str, topic_id: &str, title: String) {
        let found = self.with(|s| {
            let Some(t) = s
                .topics
                .iter_mut()
                .find(|t| t.id == topic_id && t.group_id == group_id)
            else {
                return false;
            };
            t.title = title;
            true
        });
        if found {
            self.emit(vec![
                Change::Topics {
                    group_id: group_id.into(),
                },
                Change::Todo,
            ]);
        }
    }

    fn set_topic_status(
        &self,
        group_id: &str,
        topic_id: &str,
        status: TopicStatus,
        waiting_on: Vec<MemberId>,
    ) {
        let found = self.with(|s| {
            let Some(t) = s
                .topics
                .iter_mut()
                .find(|t| t.id == topic_id && t.group_id == group_id)
            else {
                return false;
            };
            t.status = status;
            t.waiting_on = waiting_on;
            true
        });
        if found {
            self.emit(vec![
                Change::Topics {
                    group_id: group_id.into(),
                },
                Change::Todo,
            ]);
        }
    }

    fn mark_read(&self, group_id: &str, topic_id: &str, seq: u64) {
        let changed = self.with(|s| {
            let Some(t) = s
                .topics
                .iter_mut()
                .find(|t| t.id == topic_id && t.group_id == group_id)
            else {
                return false;
            };
            if t.unread == 0 || seq < t.last_seq {
                return false;
            }
            let (unread, mentions) = (t.unread, t.mentions);
            t.unread = 0;
            t.mentions = 0;
            if let Some(g) = s.group_mut(group_id) {
                g.unread = g.unread.saturating_sub(unread);
                g.mentions = g.mentions.saturating_sub(mentions);
                g.read_seq = g.read_seq.max(seq);
            }
            true
        });
        if changed {
            self.emit(vec![
                Change::Topics {
                    group_id: group_id.into(),
                },
                Change::Groups,
                Change::Todo,
            ]);
        }
    }

    fn set_typing(&self, _group_id: &str, _topic_id: &str) {}

    fn draft(&self, key: &DraftKey) -> Draft {
        self.drafts.get(key)
    }

    fn save_draft_text(&self, key: &DraftKey, body: String, mentions: Vec<MemberId>) {
        self.drafts.save_text(key, body, mentions);
    }

    fn set_quote(&self, key: &DraftKey, quote_of: Option<MessageId>) {
        if self.drafts.set_quote(key, quote_of) {
            self.emit(vec![Change::Draft { key: key.clone() }]);
        }
    }

    fn attach_files(&self, key: &DraftKey, files: Vec<NewFile>) {
        if self.drafts.attach(key, files) {
            self.emit(vec![Change::Draft { key: key.clone() }]);
        }
    }

    fn remove_file(&self, key: &DraftKey, file_id: &str) {
        if self.drafts.remove_file(key, file_id) {
            self.emit(vec![Change::Draft { key: key.clone() }]);
        }
    }

    fn clear_draft(&self, key: &DraftKey) {
        if self.drafts.clear(key) {
            self.emit(vec![Change::Draft { key: key.clone() }]);
        }
    }

    fn create_group(&self, request: CreateGroupRequest) -> AdminReply<GroupId> {
        let name = request.name.trim().to_string();
        if name.is_empty() {
            return reply_now(Err(AdminError::Invalid));
        }
        let id = self.with(|s| {
            let id = s.id("g");
            let mut member_ids = vec![ME.to_string()];
            for m in request.member_ids {
                if !member_ids.contains(&m) && s.members.iter().any(|x| x.id == m) {
                    member_ids.push(m);
                }
            }
            s.groups.push(GroupSummary {
                id: id.clone(),
                kind: GroupKind::Group,
                name,
                member_ids,
                dm_peer: None,
                last_seq: 0,
                read_seq: 0,
                unread: 0,
                mentions: 0,
                last_activity_at: now_ms(),
            });
            id
        });
        self.emit(vec![Change::Groups]);
        reply_now(Ok(id))
    }

    fn add_group_members(&self, group_id: &str, member_ids: Vec<MemberId>) -> AdminReply<()> {
        let found = self.with(|s| {
            let known: Vec<String> = s.members.iter().map(|m| m.id.clone()).collect();
            let Some(g) = s
                .groups
                .iter_mut()
                .find(|g| g.id == group_id && g.kind == GroupKind::Group)
            else {
                return false;
            };
            for m in member_ids {
                if known.contains(&m) && !g.member_ids.contains(&m) {
                    g.member_ids.push(m);
                }
            }
            true
        });
        if !found {
            return reply_now(Err(AdminError::NotFound));
        }
        self.emit(vec![Change::Groups]);
        reply_now(Ok(()))
    }

    fn rename_group(&self, group_id: &str, name: String) -> AdminReply<()> {
        let found = self.with(|s| {
            let Some(g) = s.groups.iter_mut().find(|g| g.id == group_id) else {
                return Err(AdminError::NotFound);
            };
            if g.kind == GroupKind::Dm {
                return Err(AdminError::Forbidden);
            }
            g.name = name;
            Ok(())
        });
        if found.is_ok() {
            self.emit(vec![Change::Groups, Change::Todo]);
        }
        reply_now(found)
    }

    fn open_dm(&self, member_id: &str) -> AdminReply<GroupId> {
        let id = self.with(|s| {
            if member_id == ME {
                return None;
            }
            if let Some(g) = s
                .groups
                .iter()
                .find(|g| g.kind == GroupKind::Dm && g.dm_peer.as_deref() == Some(member_id))
            {
                return Some(g.id.clone());
            }
            let peer = s.members.iter().find(|m| m.id == member_id).cloned()?;
            let id = s.id("dm");
            s.groups.push(GroupSummary {
                id: id.clone(),
                kind: GroupKind::Dm,
                name: peer.name,
                member_ids: vec![ME.to_string(), peer.id.clone()],
                dm_peer: Some(peer.id),
                last_seq: 0,
                read_seq: 0,
                unread: 0,
                mentions: 0,
                last_activity_at: now_ms(),
            });
            Some(id)
        });
        // The real provider answers with the group whether it is new or not.
        self.emit(vec![Change::Groups]);
        reply_now(id.ok_or(AdminError::NotFound))
    }

    fn leave_group(&self, group_id: &str) {
        let left = self.with(|s| {
            let before = s.groups.len();
            s.groups.retain(|g| g.id != group_id);
            s.topics.retain(|t| t.group_id != group_id);
            s.muted.remove(group_id);
            s.groups.len() != before
        });
        if left {
            self.emit(vec![
                Change::Groups,
                Change::Todo,
                Change::Topics {
                    group_id: group_id.to_string(),
                },
            ]);
        }
    }

    fn group_muted(&self, group_id: &str) -> bool {
        self.with(|s| s.muted.contains(group_id))
    }

    fn set_group_muted(&self, group_id: &str, muted: bool) {
        let changed = self.with(|s| {
            if !s.groups.iter().any(|g| g.id == group_id) {
                return false;
            }
            if muted {
                s.muted.insert(group_id.to_string())
            } else {
                s.muted.remove(group_id)
            }
        });
        if changed {
            self.emit(vec![Change::Groups]);
        }
    }

    fn saved_navigation(&self) -> Option<SavedNavigation> {
        self.with(|s| s.saved_navigation.clone())
    }

    fn save_navigation(&self, navigation: SavedNavigation) {
        self.with(|s| s.saved_navigation = Some(navigation));
    }

    fn search(&self, query: SearchQuery) -> SearchReply {
        let hits = self.with(|s| {
            let messages: Vec<&Message> = s.messages.values().flatten().collect();
            crate::fake_extra::search(
                &s.topics,
                &messages,
                &query.text,
                query.group_id.as_deref(),
                query.limit as usize,
            )
        });
        let (tx, rx) = futures::channel::oneshot::channel();
        let _ = tx.send(Ok(hits));
        rx
    }

    /// What waits on me, as the default lists it, with the question itself
    /// as the ask's title: the asker's newest message in the 话题 (the pinned
    /// banner then names the question, not the 话题 title).
    fn asks_for_me(&self) -> Vec<crate::data::v2::AskItem> {
        self.todo()
            .into_iter()
            .filter(|t| t.reason == TodoReason::WaitingOnMe)
            .flat_map(|t| {
                // A 话题 whose cards ask me lists one ask per card, from the card's author,
                // so the pinned banner gives way while the card is on screen (T-10).
                if let Some(cards) = self.card_asks(&t.group_id, &t.topic.id) {
                    return cards;
                }
                let from = t
                    .topic
                    .owner_id
                    .clone()
                    .unwrap_or_else(|| t.topic.created_by.clone());
                let question = self
                    .messages(&t.group_id, &t.topic.id, 50)
                    .messages
                    .into_iter()
                    .rev()
                    .find(|m| m.author_id == from && m.deleted_at.is_none())
                    .and_then(|m| {
                        // The line that asks (it ends in a question mark),
                        // else the first line; without a leading @mention.
                        let line = m
                            .body
                            .lines()
                            .find(|l| l.trim_end().ends_with(['？', '?']))
                            .or_else(|| m.body.lines().next())?
                            .trim();
                        let line = match line.strip_prefix('@') {
                            Some(rest) => rest.split_once(' ').map_or(rest, |(_, r)| r).trim(),
                            None => line,
                        };
                        Some(line.to_string())
                    })
                    .filter(|b| !b.trim().is_empty());
                let review = t.topic.status == TopicStatus::WaitingReview;
                vec![crate::data::v2::AskItem {
                    group_id: t.group_id,
                    topic_id: t.topic.id.clone(),
                    message_id: None,
                    from,
                    kind: if review {
                        crate::data::v2::AskKind::Review
                    } else {
                        crate::data::v2::AskKind::Topic
                    },
                    // A review keeps the 话题 title (the change under review).
                    title: question
                        .filter(|_| !review)
                        .unwrap_or_else(|| t.topic.title.clone()),
                    at: t.topic.last_activity_at,
                }]
            })
            .collect()
    }

    /// This week of a member from the same data as 首页's 今天交付 and
    /// 最近的产出: 话题 it owns that are done, and 产出 it made, in the last
    /// 7 days (fixtures agree, A5).
    fn member_stats(&self, member_id: &str) -> Option<crate::data::v2::MemberStats> {
        let week_ago = crate::selftest::clock::local_now().timestamp_millis() - 7 * 24 * 3_600_000;
        let mut stats = crate::data::v2::MemberStats::default();
        for g in self.groups() {
            for t in self.topics(&g.id) {
                for r in self.runs(&g.id, &t.id) {
                    if r.bot_id == member_id && r.status.is_active() {
                        stats.active.push((g.id.clone(), t.id.clone()));
                    }
                }
                let owner = t.owner_id.as_deref() == Some(member_id);
                if owner && t.last_activity_at >= week_ago {
                    stats.topics_this_week += 1;
                    if t.status == TopicStatus::Done {
                        stats.done_this_week += 1;
                    }
                }
                stats.artifacts_this_week += self
                    .artifacts(&g.id, &t.id)
                    .iter()
                    .filter(|a| a.created_by == member_id && a.updated_at >= week_ago)
                    .count() as u32;
            }
        }
        Some(stats)
    }

    fn runs(&self, group_id: &str, topic_id: &str) -> Vec<BotRunSummary> {
        let (now, stopped) = self.with(|s| (s.now, s.stopped_runs.clone()));
        crate::fake_extra::sample_runs(now)
            .into_iter()
            .filter(|r| r.group_id == group_id && r.topic_id == topic_id)
            .map(|mut r| {
                if let Some(at) = stopped.get(&r.id) {
                    r.status = RunStatus::Cancelled;
                    r.ended_at = Some(*at);
                    r.updated_at = *at;
                }
                r
            })
            .collect()
    }

    /// Stops a sample run at once (the sample server answers immediately).
    fn cancel_run(&self, group_id: &str, topic_id: &str, run_id: &str) -> AdminReply<()> {
        let active = self
            .runs(group_id, topic_id)
            .iter()
            .any(|r| r.id == run_id && r.status.is_active());
        if active {
            self.with(|s| {
                s.stopping_runs.remove(run_id);
                s.stopped_runs.insert(run_id.to_string(), now_ms());
            });
            self.emit(vec![Change::Runs {
                group_id: group_id.into(),
                topic_id: topic_id.into(),
            }]);
        }
        reply_now(Ok(()))
    }

    fn run_stopping(&self, run_id: &str) -> bool {
        self.with(|s| s.stopping_runs.contains(run_id))
    }

    fn run_process(&self, run_id: &str) -> RunProcess {
        crate::fake_extra::sample_process(run_id)
    }

    fn follow_run(&self, run_id: &str) {
        self.emit(vec![Change::RunLog {
            run_id: run_id.to_string(),
        }]);
    }

    // --- settings window (fake/settings.rs) ---

    fn update_profile(&self, name: String, role: Option<String>) -> AdminReply<Member> {
        self.update_profile_fake(name, role)
    }

    fn upload_avatar(&self, path: std::path::PathBuf) -> AdminReply<Member> {
        self.upload_avatar_fake(path)
    }

    fn create_invite(&self, request: InviteRequest) -> AdminReply<InviteCreated> {
        self.create_invite_fake(request)
    }

    fn set_member_level(&self, member_id: &str, level: MemberLevel) -> AdminReply<Member> {
        self.set_member_level_fake(member_id, level)
    }

    fn set_member_deactivated(&self, member_id: &str, deactivated: bool) -> AdminReply<Member> {
        self.set_member_deactivated_fake(member_id, deactivated)
    }

    fn create_bot_member(
        &self,
        name: String,
        role: Option<String>,
        character: Option<CharacterId>,
        runner: Option<RunnerSetup>,
    ) -> AdminReply<BotCreated> {
        self.create_bot_member_fake(name, role, character, runner)
    }

    fn set_member_character(
        &self,
        member_id: &str,
        character: Option<CharacterId>,
    ) -> AdminReply<Member> {
        self.set_member_character_fake(member_id, character)
    }

    fn bot_runner(&self, member_id: &str) -> AdminReply<Option<RunnerSetup>> {
        self.bot_runner_fake(member_id)
    }

    fn update_bot_member(&self, member_id: &str, edit: BotMemberEdit) -> AdminReply<()> {
        self.update_bot_member_fake(member_id, edit)
    }

    fn machines(&self) -> AdminReply<Vec<MachineInfo>> {
        self.machines_fake()
    }

    // --- settings-backend track ---
    fn notify_preferences(&self) -> AdminReply<NotifyPreferences> {
        self.notify_preferences_fake()
    }
    fn set_notify_preferences(&self, prefs: NotifyPreferences) -> AdminReply<NotifyPreferences> {
        self.set_notify_preferences_fake(prefs)
    }
    fn approval_defaults(&self) -> AdminReply<ApprovalDefaults> {
        self.approval_defaults_fake()
    }
    fn memories(
        &self,
        scope: &crate::data::memory::MemoryScope,
    ) -> AdminReply<Vec<crate::data::memory::Memory>> {
        self.memories_fake(scope)
    }
    fn memory_updates(
        &self,
        scope: &crate::data::memory::MemoryScope,
    ) -> AdminReply<crate::data::memory::MemoryDay> {
        self.memory_updates_fake(scope)
    }
    fn create_memory(
        &self,
        new: crate::data::memory::NewMemory,
    ) -> AdminReply<crate::data::memory::Memory> {
        self.create_memory_fake(new)
    }
    fn update_memory(
        &self,
        memory_id: &str,
        edit: crate::data::memory::MemoryEdit,
    ) -> AdminReply<crate::data::memory::Memory> {
        self.update_memory_fake(memory_id, edit)
    }
    fn undo_memory_update(&self, update_id: &str) -> AdminReply<crate::data::memory::MemoryUpdate> {
        self.undo_memory_update_fake(update_id)
    }
    fn set_approval_defaults(&self, defaults: ApprovalDefaults) -> AdminReply<ApprovalDefaults> {
        self.set_approval_defaults_fake(defaults)
    }
    fn ai_service(&self) -> AdminReply<AiServiceSettings> {
        self.ai_service_fake()
    }
    fn worker_models(&self) -> AdminReply<Vec<WorkerModelSetting>> {
        self.worker_models_fake()
    }
    fn set_worker_model(&self, setting: WorkerModelSetting) -> AdminReply<WorkerModelSetting> {
        self.set_worker_model_fake(setting)
    }
    fn set_ai_service(&self, draft: AiServiceDraft) -> AdminReply<AiServiceSettings> {
        self.set_ai_service_fake(draft)
    }
    fn test_ai_service(&self, draft: AiServiceDraft) -> AdminReply<AiTestOutcome> {
        self.test_ai_service_fake(draft)
    }
    fn devices(&self) -> AdminReply<Vec<DeviceInfo>> {
        self.devices_fake()
    }
    fn sign_out_device(&self, device_id: &str) -> AdminReply<()> {
        self.sign_out_device_fake(device_id)
    }
    // --- end settings-backend track ---

    // Email sign-in: every address gets a code; 000000 is the wrong one.
    fn request_sign_in_code(&self, _server_url: &str, _email: &str) -> crate::data::EmailReply<()> {
        crate::data::email_reply_now(Ok(()))
    }
    fn my_email(&self) -> crate::data::EmailReply<Option<String>> {
        crate::data::email_reply_now(Ok(Some("joye@example.com".into())))
    }
    fn add_email_code(&self, _email: &str) -> crate::data::EmailReply<()> {
        crate::data::email_reply_now(Ok(()))
    }
    fn verify_email(&self, email: &str, code: &str) -> crate::data::EmailReply<String> {
        crate::data::email_reply_now(if code == "000000" {
            Err(crate::data::EmailError::WrongCode)
        } else {
            Ok(email.to_string())
        })
    }
    fn remove_email(&self) -> crate::data::EmailReply<()> {
        crate::data::email_reply_now(Ok(()))
    }

    fn create_machine(&self, name: String, labels: Vec<String>) -> AdminReply<MachineCreated> {
        self.create_machine_fake(name, labels)
    }

    /// The demo Mac has no daemon yet, so 用这台电脑 shows; using it only pretends.
    fn this_mac(&self) -> crate::data::ThisMac {
        if self
            .this_mac_used
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            crate::data::ThisMac::Working
        } else {
            crate::data::ThisMac::NotSetUp
        }
    }

    /// After 用这台电脑 the demo Mac is the office Mac mini.
    fn this_machine_id(&self) -> Option<String> {
        self.this_mac_used
            .load(std::sync::atomic::Ordering::Relaxed)
            .then(|| "mc-mini".into())
    }

    fn restart_this_mac(&self) -> AdminReply<()> {
        reply_now(Ok(()))
    }

    fn use_this_mac(&self) -> AdminReply<()> {
        self.this_mac_used
            .store(true, std::sync::atomic::Ordering::Relaxed);
        reply_now(Ok(()))
    }

    fn group_prefs(&self, group_id: &str) -> GroupPrefs {
        self.group_prefs_fake(group_id)
    }

    fn set_group_prefs(
        &self,
        group_id: &str,
        muted: Option<bool>,
        notify: Option<NotifyLevel>,
    ) -> AdminReply<()> {
        self.set_group_prefs_fake(group_id, muted, notify)
    }

    fn topic_following(&self, group_id: &str, topic_id: &str) -> bool {
        self.topic_following_fake(group_id, topic_id)
    }

    fn set_topic_following(&self, group_id: &str, topic_id: &str, follow: bool) {
        self.set_topic_following_fake(group_id, topic_id, follow);
    }

    fn topic_muted(&self, group_id: &str, topic_id: &str) -> bool {
        self.topic_muted_fake(group_id, topic_id)
    }

    fn set_topic_muted(&self, group_id: &str, topic_id: &str, muted: bool) {
        self.set_topic_muted_fake(group_id, topic_id, muted);
    }

    fn send_diagnostics(&self, description: String) -> AdminReply<String> {
        self.send_diagnostics_fake(description)
    }

    fn card(&self, _group_id: &str, message_id: &str) -> Option<Arc<card::Card>> {
        self.with(|s| s.cards.card(message_id))
    }

    fn card_presses(&self, _group_id: &str, message_id: &str) -> Vec<CardPress> {
        self.with(|s| s.cards.presses(message_id))
    }

    fn card_press_state(&self, _group_id: &str, message_id: &str) -> Option<CardPressState> {
        self.with(|s| s.cards.state(message_id))
    }

    fn press_card_action(&self, group_id: &str, topic_id: &str, message_id: &str, action_id: &str) {
        use crate::views::cards::samples::FakeOutcome;
        let changed = self.with(|s| {
            let now = now_ms();
            match s.cards.press(message_id, action_id, ME, now) {
                FakeOutcome::Ignored => false,
                FakeOutcome::Refused => true,
                FakeOutcome::Edited => {
                    if let Some(m) = s
                        .messages
                        .get_mut(&(group_id.to_string(), topic_id.to_string()))
                        .and_then(|list| list.iter_mut().find(|m| m.id == message_id))
                    {
                        m.edited_at = Some(now);
                    }
                    true
                }
            }
        });
        if changed {
            self.emit(vec![Change::Messages {
                group_id: group_id.into(),
                topic_id: topic_id.into(),
            }]);
        }
    }

    // --- inc-refs track ---

    fn refs_supported(&self) -> bool {
        true
    }

    fn add_draft_ref(&self, key: &DraftKey, r: DraftRef) -> AddRef {
        let added = self.drafts.add_ref(key, r);
        if added == AddRef::Added {
            self.emit(vec![Change::Draft { key: key.clone() }]);
        }
        added
    }

    fn remove_draft_ref(&self, key: &DraftKey, index: usize) {
        if self.drafts.remove_ref(key, index) {
            self.emit(vec![Change::Draft { key: key.clone() }]);
        }
    }

    fn message_block_refs(&self, message: &Message) -> Vec<chat_core::proto::BlockRef> {
        self.refs.of(message)
    }

    fn send_with_refs(
        &self,
        request: SendRequest,
        key: &DraftKey,
        files: Vec<PendingFileId>,
        refs: Vec<chat_core::proto::BlockRefInput>,
    ) -> ClientMsgId {
        if refs.is_empty() {
            return self.send_with_files(request, key, files);
        }
        let stored = self.with(|s| {
            let all: Vec<&Message> = s.messages.values().flat_map(|l| l.iter()).collect();
            refs::FakeRefs::stored(&refs, &all, &request.topic_id)
        });
        let body = crate::views::refs::with_prefix(&request.body, &stored);
        let (g, t) = (request.group_id.clone(), request.topic_id.clone());
        let cid = self.send_with_files(SendRequest { body, ..request }, key, files);
        let id = self.with(|s| {
            s.messages
                .get(&(g, t))
                .and_then(|l| l.iter().rev().find(|m| m.client_msg_id == cid))
                .map(|m| m.id.clone())
        });
        let key_id = id.filter(|i| !i.is_empty()).unwrap_or_else(|| cid.clone());
        self.refs.keep(&key_id, stored, "");
        cid
    }

    // --- end inc-refs track ---

    fn subscribe(&self) -> UnboundedReceiver<Vec<Change>> {
        let (tx, rx) = unbounded();
        self.subscribers.lock().unwrap().push(tx);
        rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[test]
    fn sample_covers_the_product() {
        let p = FakeProvider::new();
        let topics: Vec<Topic> = p.groups().iter().flat_map(|g| p.topics(&g.id)).collect();
        for status in TopicStatus::ALL {
            assert!(
                topics.iter().any(|t| t.status == status),
                "no topic is {status:?}"
            );
        }
        assert!(p.members().iter().filter(|m| m.is_bot()).count() >= 5);
        assert!(p.groups().iter().any(|g| g.kind == GroupKind::Dm));
        assert!(p.groups().iter().any(|g| g.mentions > 0));
        assert!(!p.todo().is_empty());
        let window = p.messages(LONG_TOPIC_GROUP, LONG_TOPIC, 50);
        assert_eq!(window.messages.len(), 50);
        assert!(window.has_older);
        let all = p.messages(LONG_TOPIC_GROUP, LONG_TOPIC, usize::MAX / 2);
        assert_eq!(all.messages.len(), LONG_TOPIC_MESSAGES);
        assert!(
            all.messages
                .windows(2)
                .all(|w| w[0].created_seq < w[1].created_seq)
        );
    }

    /// 1.1.1: a 群's chat (群聊) is not one of its 话题, still opens by its
    /// fixed id, starts with its first message, and 开个话题 quotes it.
    #[test]
    fn the_group_chat_is_not_a_topic_and_topics_open_from_it() {
        let p = FakeProvider::new();
        let dev = crate::data::main_topic_id("g-dev");
        assert!(p.topics("g-dev").iter().all(|t| t.id != dev));
        assert!(p.topic("g-dev", &dev).is_some());
        let said = p.messages("g-dev", &dev, 50).messages;
        assert!(!said.is_empty());

        // A 群 nobody wrote in yet: a blank 群聊 opens, the first message makes it.
        let ops = crate::data::main_topic_id("g-ops");
        assert!(p.topic("g-ops", &ops).is_none());
        let blank = crate::data::topic_or_chat(p.as_ref(), "g-ops", &ops).expect("blank 群聊");
        assert_eq!(blank.message_count, 0);
        assert!(crate::data::topic_or_chat(p.as_ref(), "g-ops", "t-nope").is_none());
        let cid = p.send(SendRequest {
            group_id: "g-ops".into(),
            topic_id: ops.clone(),
            body: "早".into(),
            mentions: Vec::new(),
            quote_of: None,
        });
        assert!(!cid.is_empty());
        assert_eq!(p.messages("g-ops", &ops, 10).messages.len(), 1);
        assert!(p.topics("g-ops").iter().all(|t| t.id != ops));

        // 开个话题 from a 群聊 message: a new 话题 whose first message quotes it.
        let source = said.last().unwrap().id.clone();
        let before = p.topics("g-dev").len();
        p.start_topic(StartTopicRequest {
            group_id: "g-dev".into(),
            title: Some("查重连超时".into()),
            body: "接着群里这条查".into(),
            mentions: Vec::new(),
            quote_of: Some(source.clone()),
        });
        let topics = p.topics("g-dev");
        assert_eq!(topics.len(), before + 1);
        let opened = topics.iter().find(|t| t.title == "查重连超时").unwrap();
        let first = p.messages("g-dev", &opened.id, 10).messages;
        assert_eq!(first[0].quote_of.as_deref(), Some(source.as_str()));
    }

    /// T-10: the card sample 话题 asks through its cards, each with its message and author,
    /// so the pinned banner gives way to the card and never names the 话题's creator.
    #[test]
    fn card_asks_link_their_cards() {
        use crate::views::cards::samples as cs;
        let p = FakeProvider::new();
        let asks: Vec<_> = p
            .asks_for_me()
            .into_iter()
            .filter(|a| a.topic_id == cs::TOPIC)
            .collect();
        assert_eq!(asks.len(), 2, "{asks:?}");
        for a in &asks {
            let id = a.message_id.as_deref().expect("a card ask has its message");
            let card = p.card(cs::GROUP, id).expect("the ask's card");
            assert_eq!(a.title, card.title);
            assert_ne!(a.from, ME);
        }
        let pinned = p
            .pinned_ask(cs::GROUP, cs::TOPIC)
            .expect("an ask is pinned");
        assert_eq!(pinned.from, "m-sentry");
    }

    #[test]
    fn commands_emit_changes() {
        let p = FakeProvider::new();
        let mut rx = p.subscribe();
        let id = p.start_topic(StartTopicRequest {
            quote_of: None,
            group_id: "g-dev".into(),
            title: None,
            body: "新话题标题\n正文".into(),
            mentions: vec![],
        });
        assert!(!id.is_empty());
        let batch = futures::executor::block_on(rx.next()).unwrap();
        assert!(batch.contains(&Change::Groups));
        let topic = p.topics("g-dev").into_iter().next().unwrap();
        assert_eq!(topic.title, "新话题标题");

        p.load_older(LONG_TOPIC_GROUP, LONG_TOPIC);
        futures::executor::block_on(rx.next()).unwrap();
        assert_eq!(
            p.messages(LONG_TOPIC_GROUP, LONG_TOPIC, 50).messages.len(),
            50 + PAGE
        );
    }

    #[test]
    fn mark_read_clears_one_topic_only() {
        let p = FakeProvider::new();
        let before = p
            .groups()
            .into_iter()
            .find(|g| g.id == "g-dev")
            .unwrap()
            .unread;
        let t = p.topic("g-dev", "t-review").unwrap();
        p.mark_read("g-dev", "t-review", t.last_seq);
        let after = p
            .groups()
            .into_iter()
            .find(|g| g.id == "g-dev")
            .unwrap()
            .unread;
        assert_eq!(after, before - t.unread);
        assert!(p.topic("g-dev", "t-login").unwrap().unread > 0);
    }

    #[test]
    fn credentials_are_redacted() {
        let c = Credential::from_input("tc_secretvalue").unwrap();
        assert_eq!(format!("{c:?}"), "Token(<redacted>)");
        assert!(matches!(
            Credential::from_input("ABCD-EFGH-JKMN"),
            Some(Credential::InviteCode(_))
        ));
        assert!(Credential::from_input("  ").is_none());
    }
}
