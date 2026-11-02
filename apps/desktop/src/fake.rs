//! A fake provider with rich sample data, for development, self-tests and
//! Storybook-like work on views. Everything lives in memory.
//!
//! Sample data: 5 people and 5 bots, groups and DMs, topics in every status,
//! Chinese and English Markdown messages, mentions, edits, deletes, a streaming
//! bot reply, pending and failed sends, 产出, and one topic with 10,000 messages.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

use crate::{data::*, drafts::DraftStore};

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
}

fn now_ms() -> Millis {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

const MIN: i64 = 60_000;
const HOUR: i64 = 60 * MIN;
const DAY: i64 = 24 * HOUR;

impl FakeProvider {
    /// Signed in as Joye with the sample data.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State::sample(now_ms(), true)),
            subscribers: Mutex::new(Vec::new()),
            drafts: DraftStore::new(),
        })
    }

    /// Starts at the login view; any server URL and non-empty code signs in,
    /// except the code `WRONG-CODE-0000`.
    pub fn signed_out() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State::sample(now_ms(), false)),
            subscribers: Mutex::new(Vec::new()),
            drafts: DraftStore::new(),
        })
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
        };
        s.fill_sample();
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
        let everyone = [
            ME, XIAOWANG, LIXUE, CHENCHEN, ALEX, CODER, REVIEWER, SENTRY, DOCS, DEPLOY,
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
        self.add_group("d-coder", GroupKind::Dm, "小码", &[ME, CODER], Some(CODER));
        self.add_group(
            "d-alex",
            GroupKind::Dm,
            "Alex Chen",
            &[ME, ALEX],
            Some(ALEX),
        );

        self.add_topic(
            "g-dev",
            "t-login",
            "登录页支持邀请码和粘贴凭证",
            InProgress,
            &[],
            Some(CODER),
            &[
                (ME, "@小码 登录页需要支持两种方式：\n\n1. 邀请码 `XXXX-XXXX-XXXX`\n2. 粘贴已有的登录凭证\n\n服务器地址默认填 staging。", 3 * HOUR),
                (CODER, "收到，我来做。预计 30 分钟内给出第一版。", 3 * HOUR - MIN),
                (CODER, "进展：表单和校验已完成，正在接 Keychain。\n\n```rust\nlet entry = keyring::Entry::new(service, account)?;\nentry.set_password(&token)?;\n```", 2 * HOUR),
                (XIAOWANG, "错误提示要区分「邀请码无效」和「连不上服务器」吗？", 90 * MIN),
                (ME, "要区分，文案用目录里的。", 80 * MIN),
                (CODER, "好的，已经区分。", 10 * MIN),
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
            Some(CODER),
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
                (ME, "@文档助手 根据这周合并的 PR 写一份发布说明。", 15 * MIN),
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
    };
    use MemberKind::*;
    use MemberLevel::{Member as Regular, Owner};
    let mut list = vec![
        m(ME, Human, "Joye", "负责人", Owner, true),
        m("m-xiaowang", Human, "小王", "前端实习生", Regular, true),
        m("m-lixue", Human, "李雪", "产品实习生", Regular, false),
        m("m-chenchen", Human, "陈晨", "设计实习生", Regular, true),
        m(
            "m-alex",
            Human,
            "Alex Chen",
            "Backend intern",
            Regular,
            true,
        ),
        m("m-coder", Bot, "小码", "写代码和修 bug", Owner, true),
        m("m-reviewer", Bot, "审阅员", "代码审查", Owner, true),
        m("m-sentry", Bot, "运维哨兵", "监控和告警", Owner, true),
        m("m-docs", Bot, "文档助手", "写文档和总结", Regular, true),
        m(
            "m-deploy",
            Bot,
            "Deploy Helper",
            "Builds and releases",
            Regular,
            false,
        ),
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
        .filter(|t| t.group_id == group_id)
        .cloned()
        .collect();
    topics.sort_by(|a, b| b.last_activity_at.cmp(&a.last_activity_at));
    topics
}

impl ChatProvider for FakeProvider {
    fn session(&self) -> SessionState {
        self.with(|s| s.session.clone())
    }

    fn login(&self, request: LoginRequest) {
        let ok = match &request.credential {
            Credential::InviteCode(code) => code != "WRONG-CODE-0000",
            Credential::Token(token) => token.len() > 3,
        };
        self.with(|s| {
            if ok && !request.server_url.trim().is_empty() {
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
            groups.sort_by(|a, b| b.last_activity_at.cmp(&a.last_activity_at));
            groups
        })
    }

    fn topics(&self, group_id: &str) -> Vec<Topic> {
        self.with(|s| sorted_topics(s, group_id))
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
            items.sort_by(|a, b| b.topic.last_activity_at.cmp(&a.topic.last_activity_at));
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
            list.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            list
        })
    }

    fn send(&self, request: SendRequest) -> ClientMsgId {
        let id = self.with(|s| {
            if !s
                .topics
                .iter()
                .any(|t| t.id == request.topic_id && t.group_id == request.group_id)
            {
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

    fn create_group(&self, request: CreateGroupRequest) {
        let name = request.name.trim().to_string();
        if name.is_empty() {
            return;
        }
        self.with(|s| {
            let id = s.id("g");
            let mut member_ids = vec![ME.to_string()];
            for m in request.member_ids {
                if !member_ids.contains(&m) && s.members.iter().any(|x| x.id == m) {
                    member_ids.push(m);
                }
            }
            s.groups.push(GroupSummary {
                id,
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
        });
        self.emit(vec![Change::Groups]);
    }

    fn open_dm(&self, member_id: &str) {
        self.with(|s| {
            if member_id == ME
                || s.groups
                    .iter()
                    .any(|g| g.kind == GroupKind::Dm && g.dm_peer.as_deref() == Some(member_id))
            {
                return;
            }
            let Some(peer) = s.members.iter().find(|m| m.id == member_id).cloned() else {
                return;
            };
            let id = s.id("dm");
            s.groups.push(GroupSummary {
                id,
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
        });
        // The real provider answers with the group whether it is new or not.
        self.emit(vec![Change::Groups]);
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

    #[test]
    fn commands_emit_changes() {
        let p = FakeProvider::new();
        let mut rx = p.subscribe();
        let id = p.start_topic(StartTopicRequest {
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
