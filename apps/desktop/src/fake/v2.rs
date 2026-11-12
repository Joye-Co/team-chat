//! The v2 sample team (1.1.0 artboards, CONCEPT.md §4): Joye, four interns
//! and the eight bots with their characters, the 群 开发 / 运维 / 拾光增长 /
//! 全员, their 话题 with runs, plans, cards, 产出, decisions, a 单独问 aside
//! and a live screen. Every v2 screenshot scene starts from it, so all
//! screens show the same consistent data.
//!
//! It wraps a [`FakeProvider`] for the plumbing (session, drafts, settings)
//! and answers everything a v2 screen reads itself. `fake.rs` and its tests
//! stay as they are.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use chrono::{Local, TimeZone as _};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};

use super::FakeProvider;
// pages track: the sample data of the 群, 成员 and 产出 pages.
#[path = "v2_pages.rs"]
mod pages_samples;
// --- inc-refs: the 引用提问 samples (Refs.dc.html §01, §03).
#[path = "v2_refs.rs"]
pub mod refs_samples;
// --- end inc-refs
use crate::data::{pages, v2::*, *};

pub const ME: &str = "m-joye";
pub const JOJO: &str = "b-jojo";
pub const TATA: &str = "b-tata";
pub const POCKET: &str = "b-pocket";
pub const FORGE: &str = "b-forge";
pub const WATCH: &str = "b-watch";
pub const SCOUT: &str = "b-scout";
pub const LEDGER: &str = "b-ledger";
pub const TARO: &str = "b-taro";
pub const XIAOWANG: &str = "m-xiaowang";
pub const LIXUE: &str = "m-lixue";
pub const ALEX: &str = "m-alex";
pub const CHENCHEN: &str = "m-chenchen";

pub const DEV: &str = "g-v2-dev";
pub const OPS: &str = "g-v2-ops";
pub const GROWTH: &str = "g-v2-growth";
pub const ALL: &str = "g-v2-all";
pub const DM_JOJO: &str = "g-v2-dm-jojo";
pub const DM_XIAOWANG: &str = "g-v2-dm-xiaowang";

/// 开发 / 断线重连后消息重复: done, PR #61 waits on my review.
pub const T_RECONNECT: &str = "t-v2-reconnect";
/// 开发 / 话题列表虚拟滚动: Forge works on it (3/5), its computer is live.
pub const T_VIRTUAL: &str = "t-v2-virtual";
/// 开发 / 登录页支持邀请码: Forge asks me to choose.
pub const T_INVITE: &str = "t-v2-invite";
/// 开发 / 搜索支持中文两字词: waits on 小王.
pub const T_SEARCH: &str = "t-v2-search";
/// 运维 / orderbot v2.14 上线: Watch asks me to approve the release.
pub const T_RELEASE: &str = "t-v2-release";
/// 运维 / staging 每日巡检: Scout, done 09:12.
pub const T_STAGING: &str = "t-v2-staging";
/// 拾光增长 / 拾光 3.0 宣传: Taro's draft waits on me.
pub const T_PROMO: &str = "t-v2-promo";
/// 拾光增长 / 10 月数据月报: Ledger, done 09:40.
pub const T_REPORT: &str = "t-v2-report";
/// 拾光增长 / 拾光 4.0 需求: Tata writes the second draft (2/4).
pub const T_V4: &str = "t-v2-v4";
/// 全员 / 本周周报: Jojo split it to Pocket and Ledger (1/3).
pub const T_WEEKLY: &str = "t-v2-weekly";

pub const RUN_RECONNECT: &str = "run-v2-reconnect";
pub const RUN_VIRTUAL: &str = "run-v2-virtual";
/// Scout's check of the virtual list, beside Forge's run (`with_second_run`).
pub const RUN_VIRTUAL_CHECK: &str = "run-v2-virtual-check";
pub const RUN_V4: &str = "run-v2-v4";
/// Watch's release of orderbot v2.14 that I approved (the Card v2 gallery only,
/// `with_release_run`).
pub const RUN_RELEASE: &str = "run-v2-release";
pub const RUN_WEEKLY: &str = "run-v2-weekly";

pub const ART_RECONNECT: &str = "art-v2-reconnect-report";
pub const ART_REPORT: &str = "art-v2-oct-report";
pub const ART_V4: &str = "art-v2-v4-draft";
/// Embedded in a card but its folder never opens (the 页面没有打开 · 重试 scene).
pub const ART_EMBED_BROKEN: &str = "art-v2-embed-broken";
/// Embedded in a card but a single file, not a web page (这个产出不是网页).
pub const ART_EMBED_FILE: &str = "art-v2-embed-file";

const MIN: i64 = 60_000;

/// Today at `hh:mm` local time. The sample day runs from 09:00 to the
/// pinned 10:21: nothing that already happened is later than now.
pub fn at(hour: u32, minute: u32) -> Millis {
    days_ago_at(0, hour, minute)
}

/// Today at `hh:mm:ss`: events between 10:20 and 10:21 keep their order
/// and still show a time no later than now.
fn at_s(hour: u32, minute: u32, second: u32) -> Millis {
    days_ago_at_s(0, hour, minute, second)
}

fn days_ago_at(days: i64, hour: u32, minute: u32) -> Millis {
    days_ago_at_s(days, hour, minute, 0)
}

fn days_ago_at_s(days: i64, hour: u32, minute: u32, second: u32) -> Millis {
    let date = crate::selftest::clock::local_now().date_naive() - chrono::Duration::days(days);
    let t = date.and_hms_opt(hour, minute, second).unwrap_or_default();
    Local
        .from_local_datetime(&t)
        .single()
        .map(|t| t.timestamp_millis())
        .unwrap_or_default()
}

fn person(id: &str, name: &str, role: &str, online: bool, owner: bool) -> Member {
    Member {
        id: id.into(),
        kind: MemberKind::Human,
        name: name.into(),
        role: Some(role.into()),
        level: if owner {
            MemberLevel::Owner
        } else {
            MemberLevel::Member
        },
        avatar_url: None,
        deactivated: false,
        online,
        character: None,
        team_owner: owner,
    }
}

fn bot(id: &str, name: &str, role: &str, character: &str) -> Member {
    Member {
        id: id.into(),
        kind: MemberKind::Bot,
        name: name.into(),
        role: Some(role.into()),
        level: MemberLevel::Member,
        avatar_url: None,
        deactivated: false,
        online: true,
        character: CharacterId::parse(character),
        team_owner: false,
    }
}

/// The team of the artboards: Joye first, then the bots, then the interns.
pub fn members() -> Vec<Member> {
    vec![
        person(ME, "Joye", "负责人", true, true),
        bot(JOJO, "Jojo", "主助手", "jojo"),
        bot(TATA, "Tata", "产品负责人", "o03"),
        bot(POCKET, "Pocket", "幕僚长", "o05"),
        bot(FORGE, "Forge", "开发", "o08"),
        bot(WATCH, "Watch", "运维", "o11"),
        bot(SCOUT, "Scout", "测试验收", "o07"),
        bot(LEDGER, "Ledger", "数据", "h02"),
        bot(TARO, "Taro", "社媒发布", "h03"),
        person(XIAOWANG, "小王", "前端实习", true, false),
        person(LIXUE, "李雪", "运营实习", true, false),
        person(ALEX, "Alex Chen", "后端实习", false, false),
        person(CHENCHEN, "陈晨", "设计实习", false, false),
    ]
}

struct Data {
    groups: Vec<GroupSummary>,
    topics: Vec<Topic>,
    messages: HashMap<String, Vec<Message>>,
    runs: Vec<BotRunSummary>,
    cards: HashMap<String, Arc<card::Card>>,
    artifacts: Vec<(GroupId, ArtifactSummary)>,
    side_chat: Vec<SideChatMessage>,
    /// Usable (T6, T7): the sample live run's phase fields and its held commands.
    hold: run_phase::RunHold,
    hand: Vec<run_phase::HandCommand>,
    // topic track (v11/topic): who approved PR #61 and when.
    approval: Option<(MemberId, Millis)>,
    /// My own notify settings, so the 通知 sections and menus can be tried
    /// in `--fake`: muted 群 / 私聊, their 提醒 levels, the 话题 I unfollowed
    /// and the 话题 I muted.
    muted_groups: std::collections::HashSet<GroupId>,
    levels: HashMap<GroupId, NotifyLevel>,
    unfollowed: std::collections::HashSet<TopicId>,
    muted_topics: std::collections::HashSet<TopicId>,
    /// Usable: why the server refused my last order in a 话题 (`relay_refusal`).
    relay_refusal: Option<(TopicId, RelayRefusal)>,
}

/// Usable: the state a sample 按顺序做 is in (`V2Samples::with_relay`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelayShape {
    /// Two members at work in one 话题, no order (Forge and Scout).
    TwoWorking,
    /// Step 2 of 3 at work.
    Working,
    /// Step 2 waits for its member's other work here.
    Queued,
    /// Stopped at step 2: Scout got stuck; 重试 and 停止.
    Paused,
    /// I stopped it at step 2.
    Stopped,
    /// Every step done.
    Done,
    /// The server refused my order: Scout cannot take it.
    Refused,
}

/// The v2 sample provider.
pub struct V2Samples {
    inner: Arc<FakeProvider>,
    members: Vec<Member>,
    data: Mutex<Data>,
    subscribers: Mutex<Vec<UnboundedSender<Vec<Change>>>>,
    // Desk track (1.1.0 首页): a quiet day, and the answers made on the desk.
    quiet: bool,
    desk: Mutex<super::v2_desk::DeskSamples>,
    // --- cards v2 track ---
    cards_v2: crate::views::cards::v2::samples::SampleCards,
    // --- end cards v2 track ---
    // --- inc-refs
    refs: super::refs::FakeRefs,
}

fn group(id: &str, kind: GroupKind, name: &str, members: &[&str], last: Millis) -> GroupSummary {
    GroupSummary {
        id: id.into(),
        kind,
        name: name.into(),
        member_ids: members.iter().map(|m| m.to_string()).collect(),
        dm_peer: (kind == GroupKind::Dm)
            .then(|| members.iter().find(|m| **m != ME).map(|m| m.to_string()))
            .flatten(),
        last_seq: 0,
        read_seq: 0,
        unread: 0,
        mentions: 0,
        last_activity_at: last,
    }
}

struct TopicSpec<'a> {
    id: &'a str,
    group: &'a str,
    title: &'a str,
    status: TopicStatus,
    waiting_on: &'a [&'a str],
    owner: &'a str,
    by: &'a str,
    created: Millis,
    last: Millis,
}

fn topic(s: TopicSpec) -> Topic {
    Topic {
        id: s.id.into(),
        group_id: s.group.into(),
        title: s.title.into(),
        status: s.status,
        waiting_on: s.waiting_on.iter().map(|m| m.to_string()).collect(),
        owner_id: Some(s.owner.into()),
        created_by: s.by.into(),
        created_at: s.created,
        root_message_id: format!("{}-m1", s.id),
        last_seq: 0,
        last_activity_at: s.last,
        message_count: 0,
        unread: 0,
        mentions: 0,
        participants: Vec::new(),
        relay: None,
    }
}

fn message(group: &str, topic: &str, seq: u64, author: &str, at: Millis, body: &str) -> Message {
    Message {
        id: format!("{topic}-m{seq}"),
        client_msg_id: String::new(),
        group_id: group.into(),
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

fn plan(items: &[(&str, PlanState)]) -> Vec<PlanItem> {
    items.iter().map(|(t, s)| PlanItem::new(*t, *s)).collect()
}

struct RunSpec<'a> {
    id: &'a str,
    bot: &'a str,
    group: &'a str,
    topic: &'a str,
    status: RunStatus,
    by: &'a str,
    started: Millis,
    ended: Option<Millis>,
    machine: &'a str,
    plan: Vec<PlanItem>,
}

fn run(s: RunSpec) -> BotRunSummary {
    BotRunSummary {
        hold: Default::default(),
        id: s.id.into(),
        bot_id: s.bot.into(),
        group_id: s.group.into(),
        topic_id: s.topic.into(),
        machine_id: Some(format!("mc-{}", s.machine)),
        machine_name: Some(s.machine.into()),
        status: s.status,
        requested_by: Some(s.by.into()),
        created_at: s.started,
        started_at: Some(s.started),
        ended_at: s.ended,
        updated_at: s.ended.unwrap_or(s.started + 20 * MIN),
        model: None,
        effort: None,
        summary: None,
        plan: s.plan,
    }
}

fn card(value: serde_json::Value) -> Arc<card::Card> {
    Arc::new(serde_json::from_value(value).expect("v2 sample card parses"))
}

fn artifact(
    id: &str,
    topic: &str,
    kind: ArtifactKind,
    title: &str,
    version: u32,
    by: &str,
    at: Millis,
) -> ArtifactSummary {
    ArtifactSummary {
        id: id.into(),
        topic_id: topic.into(),
        kind,
        title: title.into(),
        version,
        size_bytes: Some(48_000),
        created_by: by.into(),
        updated_at: at,
    }
}

impl V2Samples {
    pub fn new() -> Arc<Self> {
        Self::wrap(FakeProvider::new())
    }

    /// The sample team whose 记得的事 and 团队约定 answer as `shape` (memory scenes).
    pub fn with_memory(shape: super::memory::MemoryShape) -> Arc<Self> {
        let inner = FakeProvider::new();
        inner.set_memory_shape(shape);
        Self::wrap(inner)
    }

    /// [`Self::with_memory`] on a server that `team` describes: one person with the AI
    /// members (盯 4.12), or no AI 服务 (盯 4.6).
    pub fn with_memory_team(
        shape: super::memory::MemoryShape,
        team: super::memory::MemoryTeam,
    ) -> Arc<Self> {
        let inner = FakeProvider::new();
        inner.set_memory_shape(shape);
        if !team.ai {
            inner.clear_ai_service();
        }
        let mut this = Self::wrap(inner);
        if team.solo {
            let me_alone = members()
                .into_iter()
                .filter(|m| m.is_bot() || m.id == ME)
                .collect();
            if let Some(s) = Arc::get_mut(&mut this) {
                s.members = me_alone;
            }
        }
        this
    }

    /// The sample team over `inner` (for its session, drafts and settings).
    pub fn wrap(inner: Arc<FakeProvider>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            members: members(),
            data: Mutex::new(Self::data()),
            subscribers: Mutex::new(Vec::new()),
            quiet: false,
            desk: Mutex::default(),
            cards_v2: Default::default(),
            refs: Default::default(),
        })
    }

    /// The sample team on a quiet day (`desk-empty`): nothing waits on me,
    /// nothing runs, nothing is done yet today.
    pub fn quiet() -> Arc<Self> {
        Arc::new(Self {
            inner: FakeProvider::new(),
            members: members(),
            data: Mutex::new(Self::data()),
            subscribers: Mutex::new(Vec::new()),
            quiet: true,
            desk: Mutex::default(),
            // --- cards v2 track ---
            cards_v2: Default::default(),
            // --- end cards v2 track ---
            refs: Default::default(),
        })
    }

    /// Onboarding (`desk-first-run`, `rail-first-run`): a member who just joined. Only in
    /// 全员 (its 话题 are Jojo's, none mine, none followed) and the 私聊 with Jojo the
    /// server opened on joining; a quiet day.
    pub fn newcomer() -> Arc<Self> {
        let mut data = Self::data();
        data.groups.retain(|g| g.id == ALL || g.id == DM_JOJO);
        data.topics.retain(|t| t.group_id == ALL);
        for t in &mut data.topics {
            t.created_by = JOJO.into();
            t.owner_id = Some(JOJO.into());
            t.waiting_on.clear();
            data.unfollowed.insert(t.id.clone());
        }
        let topics: std::collections::HashSet<_> =
            data.topics.iter().map(|t| t.id.clone()).collect();
        data.runs.retain(|r| topics.contains(&r.topic_id));
        Arc::new(Self {
            inner: FakeProvider::new(),
            members: members(),
            data: Mutex::new(data),
            subscribers: Mutex::new(Vec::new()),
            quiet: true,
            desk: Mutex::default(),
            cards_v2: Default::default(),
            refs: Default::default(),
        })
    }

    fn data() -> Data {
        use TopicStatus as S;
        let dev_members = [
            ME, JOJO, FORGE, SCOUT, TATA, XIAOWANG, ALEX, CHENCHEN, WATCH,
        ];
        let groups = vec![
            group(DEV, GroupKind::Group, "开发", &dev_members, at(10, 20)),
            group(
                OPS,
                GroupKind::Group,
                "运维",
                &[ME, JOJO, WATCH, SCOUT, FORGE, ALEX],
                at_s(10, 20, 45),
            ),
            group(
                GROWTH,
                GroupKind::Group,
                "拾光增长",
                &[ME, JOJO, TATA, LEDGER, TARO, LIXUE, CHENCHEN],
                at(10, 20),
            ),
            group(
                ALL,
                GroupKind::Group,
                "全员",
                &[
                    ME, JOJO, TATA, POCKET, FORGE, WATCH, SCOUT, LEDGER, TARO, XIAOWANG, LIXUE,
                    ALEX, CHENCHEN,
                ],
                at(10, 5),
            ),
            group(DM_JOJO, GroupKind::Dm, "Jojo", &[ME, JOJO], at(8, 50)),
            group(
                DM_XIAOWANG,
                GroupKind::Dm,
                "小王",
                &[ME, XIAOWANG],
                days_ago_at(1, 18, 30),
            ),
        ];
        let topics = vec![
            topic(TopicSpec {
                id: T_RECONNECT,
                group: DEV,
                title: "断线重连后消息重复",
                status: S::WaitingReview,
                waiting_on: &[ME],
                owner: FORGE,
                by: XIAOWANG,
                created: at(9, 12),
                last: at(10, 20),
            }),
            topic(TopicSpec {
                id: T_VIRTUAL,
                group: DEV,
                title: "话题列表虚拟滚动",
                status: S::InProgress,
                waiting_on: &[],
                owner: FORGE,
                by: ME,
                created: at(9, 30),
                last: at_s(10, 20, 30),
            }),
            topic(TopicSpec {
                id: T_INVITE,
                group: DEV,
                title: "登录页支持邀请码",
                status: S::WaitingAnswer,
                waiting_on: &[ME],
                owner: FORGE,
                by: ME,
                created: at(9, 40),
                last: at(9, 58),
            }),
            topic(TopicSpec {
                id: T_SEARCH,
                group: DEV,
                title: "搜索支持中文两字词",
                status: S::WaitingAnswer,
                waiting_on: &[XIAOWANG],
                owner: FORGE,
                by: XIAOWANG,
                created: at(9, 5),
                last: at(9, 50),
            }),
            topic(TopicSpec {
                id: T_RELEASE,
                group: OPS,
                title: "orderbot v2.14 上线",
                status: S::WaitingAnswer,
                waiting_on: &[ME],
                owner: WATCH,
                by: JOJO,
                created: at(9, 20),
                last: at_s(10, 20, 45),
            }),
            topic(TopicSpec {
                id: T_STAGING,
                group: OPS,
                title: "staging 每日巡检",
                status: S::Done,
                waiting_on: &[],
                owner: SCOUT,
                by: JOJO,
                created: at(9, 0),
                last: at(9, 12),
            }),
            topic(TopicSpec {
                id: T_PROMO,
                group: GROWTH,
                title: "拾光 3.0 宣传",
                status: S::WaitingReview,
                waiting_on: &[ME],
                owner: TARO,
                by: TATA,
                created: at(9, 15),
                last: at(10, 20),
            }),
            topic(TopicSpec {
                id: T_REPORT,
                group: GROWTH,
                title: "10 月数据月报",
                status: S::Done,
                waiting_on: &[],
                owner: LEDGER,
                by: TATA,
                created: at(9, 2),
                last: at(9, 40),
            }),
            topic(TopicSpec {
                id: T_V4,
                group: GROWTH,
                title: "拾光 4.0 需求",
                status: S::InProgress,
                waiting_on: &[],
                owner: TATA,
                by: ME,
                created: at(9, 25),
                last: at(10, 10),
            }),
            topic(TopicSpec {
                id: T_WEEKLY,
                group: ALL,
                title: "本周周报",
                status: S::InProgress,
                waiting_on: &[],
                owner: JOJO,
                by: ME,
                created: at(9, 0),
                last: at(10, 5),
            }),
        ];

        let mut messages: HashMap<String, Vec<Message>> = HashMap::new();
        let mut cards = HashMap::new();
        // 开发 / 断线重连后消息重复 (Topic.dc.html).
        let t = T_RECONNECT;
        let mut video = message(
            DEV,
            t,
            1,
            XIAOWANG,
            at(9, 12),
            "重连之后同一条消息会出现两次，录屏在这里。",
        );
        video.attachments.push(FileAttachment {
            file_id: "f-v2-video".into(),
            name: "重连后重复.mov".into(),
            size: 4_400_000,
            mime: "video/quicktime".into(),
            width: None,
            height: None,
            sha256: "v2video".into(),
        });
        let mut done = message(
            DEV,
            t,
            6,
            FORGE,
            at(9, 55),
            "**改好了：重连后不再重复**\n\n原因是客户端重连后从旧的 seq 重新拉取，服务器把已经收到的消息又推了一遍。现在按 seq 去重，重连时从最后一个确认过的 seq 继续；重连间隔也改成了指数退避。",
        );
        done.artifact_refs.push(ArtifactRef {
            artifact_id: ART_RECONNECT.into(),
            version: 2,
        });
        // The question my decision answers (看问题 on the decision record).
        let question = message(
            DEV,
            t,
            4,
            FORGE,
            at(9, 30),
            "重连间隔用固定 5 秒，还是改成指数退避？要不要放进同一个 PR？",
        );
        let mut decision = message(DEV, t, 5, ME, at(9, 31), "改成指数退避，放进同一个 PR。");
        decision.quote_of = Some(question.id.clone());
        messages.insert(
            t.into(),
            vec![
                video,
                message(DEV, t, 2, JOJO, at(9, 13), "转给 @Forge。"),
                message(DEV, t, 3, FORGE, at(9, 14), "收到，先复现，大约 30 分钟。"),
                question,
                decision,
                done,
                message(DEV, t, 7, XIAOWANG, at(10, 20), "本地试了，没再出现重复。"),
            ],
        );
        // 开发 / 话题列表虚拟滚动 (Live.dc.html).
        let t = T_VIRTUAL;
        messages.insert(
            t.into(),
            vec![
                message(
                    DEV,
                    t,
                    1,
                    ME,
                    at(9, 30),
                    "@Forge 话题列表到一万条会卡，换成虚拟列表，滚动要稳在 60 fps 以上。",
                ),
                message(
                    DEV,
                    t,
                    2,
                    FORGE,
                    at(9, 31),
                    "收到。先读现有实现，再换虚拟列表，最后用一万条消息压测。",
                ),
            ],
        );
        // 开发 / 登录页支持邀请码: Forge asks me to choose.
        let t = T_INVITE;
        let mut ask = message(DEV, t, 2, FORGE, at(9, 58), "邀请码输入框用哪种？");
        ask.id = format!("{t}-ask");
        cards.insert(
            ask.id.clone(),
            card(serde_json::json!({
                "v": 1,
                "title": "邀请码输入框用哪种？",
                "blocks": [
                    { "type": "text", "text": "Forge 推荐 A：实习生大多从微信复制邀请码。" }
                ],
                "actions": [
                    { "id": "a", "label": "A 一个输入框，粘贴自动识别" },
                    { "id": "b", "label": "B 8 个分格，逐位输入" }
                ]
            })),
        );
        messages.insert(
            t.into(),
            vec![
                message(
                    DEV,
                    t,
                    1,
                    ME,
                    at(9, 40),
                    "@Forge 登录页加上邀请码登录，不做手机号。",
                ),
                ask,
            ],
        );
        let t = T_SEARCH;
        messages.insert(
            t.into(),
            vec![
                message(
                    DEV,
                    t,
                    1,
                    XIAOWANG,
                    at(9, 5),
                    "搜「重连」搜不到「断线重连」那条，两个字的词好像都不行。",
                ),
                message(
                    DEV,
                    t,
                    2,
                    FORGE,
                    at(9, 50),
                    "加了两字词的分词，@小王 你在本地试一下？",
                ),
            ],
        );
        // 运维 / orderbot v2.14 上线: the approval.
        let t = T_RELEASE;
        let mut approval = message(
            OPS,
            t,
            2,
            WATCH,
            at_s(10, 20, 45),
            "把 orderbot v2.14 发布到生产吗？",
        );
        approval.id = format!("{t}-ask");
        cards.insert(
            approval.id.clone(),
            card(serde_json::json!({
                "v": 1,
                "title": "把 orderbot v2.14 发布到生产吗？",
                "tone": "warning",
                "blocks": [
                    { "type": "fields", "fields": [
                        { "label": "包含", "value": "群相册分享、撤回同步、告警降噪，共 3 个 PR" },
                        { "label": "验收", "value": "Scout 今天 09:10 在 staging 验收通过" },
                        { "label": "影响", "value": "所有群重启约 2 分钟；出问题可以一键回到 v2.13" }
                    ]}
                ],
                "actions": [
                    { "id": "release", "label": "发布到生产", "style": "primary" },
                    { "id": "hold", "label": "先不发" }
                ]
            })),
        );
        messages.insert(
            t.into(),
            vec![
                message(
                    OPS,
                    t,
                    1,
                    JOJO,
                    at(9, 20),
                    "@Watch 准备 orderbot v2.14：#412 群相册分享、#415 撤回同步、#418 告警降噪。",
                ),
                approval,
            ],
        );
        let t = T_STAGING;
        messages.insert(
            t.into(),
            vec![message(
                OPS,
                t,
                1,
                SCOUT,
                at(9, 12),
                "staging 每日巡检做完了，24 项都通过。",
            )],
        );
        // 拾光增长 / 拾光 3.0 宣传: the draft.
        let t = T_PROMO;
        let mut draft = message(
            GROWTH,
            t,
            2,
            TARO,
            at(10, 20),
            "这条微博确认后，周四 20:00 发出。",
        );
        draft.id = format!("{t}-ask");
        cards.insert(
            draft.id.clone(),
            card(serde_json::json!({
                "v": 1,
                "title": "这条微博确认后，周四 20:00 发出",
                "blocks": [
                    { "type": "fields", "fields": [
                        { "label": "标题", "value": "拾光 3.0｜群里的照片，一键分享回群" }
                    ]},
                    { "type": "text", "text": "以前在群里找一张合照要翻半天。3.0 把群里的照片按活动自动整理好，点一下就能分享回群。" }
                ],
                "actions": [
                    { "id": "confirm", "label": "确认发布", "style": "primary" },
                    { "id": "edit", "label": "改一下" },
                    { "id": "drop", "label": "不发了" }
                ]
            })),
        );
        messages.insert(
            t.into(),
            vec![
                message(
                    GROWTH,
                    t,
                    1,
                    TATA,
                    at(9, 15),
                    "@Taro 写一条 3.0 的微博，主打群相册。",
                ),
                draft,
            ],
        );
        let t = T_REPORT;
        let mut report = message(
            GROWTH,
            t,
            2,
            LEDGER,
            at(9, 40),
            "做好了 10 月数据月报，加了渠道拆分。",
        );
        report.artifact_refs.push(ArtifactRef {
            artifact_id: ART_REPORT.into(),
            version: 3,
        });
        messages.insert(
            t.into(),
            vec![
                message(
                    GROWTH,
                    t,
                    1,
                    TATA,
                    at(9, 2),
                    "@Ledger 出 10 月数据月报，日活按去重设备算。",
                ),
                report,
                message(
                    GROWTH,
                    t,
                    3,
                    ME,
                    at(10, 12),
                    "@Jojo 看看这份月报，然后产出一个 html 的解读",
                ),
                {
                    let mut done =
                        message(GROWTH, t, 4, JOJO, at(10, 18), "解读页面已经发到群里。");
                    done.id = crate::views::cards::v2::samples::DONE_PAGE.into();
                    done
                },
            ],
        );
        let t = T_V4;
        messages.insert(
            t.into(),
            vec![
                message(
                    GROWTH,
                    t,
                    1,
                    ME,
                    at(9, 25),
                    "@Tata 整理拾光 4.0 的需求，参考上周的群反馈。",
                ),
                message(
                    GROWTH,
                    t,
                    2,
                    TATA,
                    at(10, 10),
                    "在写第二版，参考了上周的群反馈。",
                ),
            ],
        );
        let t = T_WEEKLY;
        messages.insert(
            t.into(),
            vec![
                message(ALL, t, 1, ME, at(9, 0), "@Jojo 汇总本周周报。"),
                message(
                    ALL,
                    t,
                    2,
                    JOJO,
                    at(10, 5),
                    "分给了 Pocket 和 Ledger，等它们交稿。",
                ),
            ],
        );
        // The 群聊 of 开发: plain chat to reply to (a quote, its @, the keys).
        let chat = crate::data::main_topic_id(DEV);
        messages.insert(
            chat.clone(),
            vec![
                message(
                    DEV,
                    &chat,
                    1,
                    XIAOWANG,
                    at(8, 50),
                    "大家早，今天先把 PR #61 合了？",
                ),
                message(DEV, &chat, 2, ME, at(8, 55), "@Forge 你先简单看下这个 repo"),
                message(
                    DEV,
                    &chat,
                    3,
                    FORGE,
                    at(8, 58),
                    "看了：结构清楚，重连那块可以先从 seq 去重入手。",
                ),
                message(DEV, &chat, 4, ALEX, at(9, 5), "我下午有空，可以帮忙验收。"),
            ],
        );

        let runs = vec![
            run(RunSpec {
                id: RUN_RECONNECT,
                bot: FORGE,
                group: DEV,
                topic: T_RECONNECT,
                status: RunStatus::Done,
                by: XIAOWANG,
                started: at(9, 14),
                ended: Some(at(9, 55)),
                machine: "Mac mini",
                plan: plan(&[
                    ("复现重复消息", PlanState::Done),
                    ("找到重复的原因", PlanState::Done),
                    ("按 seq 去重并从确认处续传", PlanState::Done),
                    ("改成指数退避", PlanState::Done),
                    ("提 PR #61", PlanState::Done),
                ]),
            }),
            run(RunSpec {
                id: RUN_VIRTUAL,
                bot: FORGE,
                group: DEV,
                topic: T_VIRTUAL,
                status: RunStatus::Running,
                by: ME,
                started: at(9, 31),
                ended: None,
                machine: "Mac mini",
                plan: plan(&[
                    ("读现有列表实现", PlanState::Done),
                    ("换成虚拟列表", PlanState::Done),
                    ("跑一万条消息的压测", PlanState::Current),
                    ("修滚动锚点", PlanState::Pending),
                    ("提 PR", PlanState::Pending),
                ]),
            }),
            run(RunSpec {
                id: RUN_V4,
                bot: TATA,
                group: GROWTH,
                topic: T_V4,
                status: RunStatus::Running,
                by: ME,
                started: at(9, 26),
                ended: None,
                machine: "云端",
                plan: plan(&[
                    ("整理上周的群反馈", PlanState::Done),
                    ("写第二版需求", PlanState::Current),
                    ("请 Joye 看", PlanState::Pending),
                    ("定稿", PlanState::Pending),
                ]),
            }),
            run(RunSpec {
                id: RUN_WEEKLY,
                bot: JOJO,
                group: ALL,
                topic: T_WEEKLY,
                status: RunStatus::Waiting,
                by: ME,
                started: at(9, 1),
                ended: None,
                machine: "云端",
                plan: plan(&[
                    ("分给 Pocket 和 Ledger", PlanState::Done),
                    ("等它们交稿", PlanState::Waiting),
                    ("汇总发到全员", PlanState::Pending),
                ]),
            }),
        ];

        let artifacts = vec![
            (
                DEV.to_string(),
                artifact(
                    ART_RECONNECT,
                    T_RECONNECT,
                    ArtifactKind::Page,
                    "重连测试报告",
                    2,
                    FORGE,
                    at(9, 55),
                ),
            ),
            (
                GROWTH.to_string(),
                artifact(
                    ART_REPORT,
                    T_REPORT,
                    ArtifactKind::Page,
                    "10 月数据月报",
                    3,
                    LEDGER,
                    at(9, 40),
                ),
            ),
            (
                GROWTH.to_string(),
                artifact(
                    ART_V4,
                    T_V4,
                    ArtifactKind::Page,
                    "拾光 4.0 需求（第二版）",
                    1,
                    TATA,
                    at(10, 10),
                ),
            ),
        ];

        let side_chat = vec![
            SideChatMessage {
                id: "sc-1".into(),
                author_id: ME.into(),
                body: "这个 PR 合进去有什么风险？".into(),
                at: at_s(10, 20, 15),
                refs: Vec::new(),
                sources: Vec::new(),
                suggestion: None,
                streaming: false,
            },
            SideChatMessage {
                id: "sc-2".into(),
                author_id: JOJO.into(),
                // panes track: the 单独问 artboard's answer.
                body: "风险不大，有三处值得留意：\n1. 去重靠客户端记住的 seq。缓存清空后会从头拉一次，不会重复，只是首次打开慢一点。\n2. 断网久了，重试间隔会拉长到 30 秒，网恢复后最多等半分钟才连上。\n3. 旧版客户端不带 seq，服务器照旧推送。\n\nForge 的测试覆盖了 6 个场景，没覆盖多端同时在线。".into(),
                at: at_s(10, 20, 15),
                refs: vec![
                    "reconnect.ts seq 去重".into(),
                    "指数退避上限 30 秒".into(),
                    "旧客户端兼容".into(),
                ],
                sources: vec![
                    SideChatSource::Pr {
                        number: 61,
                        files: Some(12),
                    },
                    SideChatSource::Artifact {
                        title: "重连测试报告".into(),
                        version: 2,
                    },
                ],
                suggestion: Some(SideChatSuggestion {
                    text: "让 Forge 补一个多端测试".into(),
                    posted_at: None,
                }),
                streaming: false,
            },
        ];

        Data {
            groups,
            topics,
            messages,
            runs,
            cards,
            artifacts,
            side_chat,
            hold: run_phase::RunHold {
                // A personal Mac of mine, which takes commands.
                takeover: Some("commands".into()),
                machine_owner: Some(ME.into()),
                ..Default::default()
            },
            hand: Vec::new(),
            approval: None,
            muted_groups: Default::default(),
            levels: HashMap::new(),
            unfollowed: Default::default(),
            muted_topics: Default::default(),
            relay_refusal: None,
        }
    }

    /// The sample team as the Card v2 gallery shows it: Watch's release that I approved
    /// runs, so its 发布中 card offers 停止… (C-04). Only the gallery uses it: in the
    /// other scenes the release still waits for my approval.
    pub fn with_release_run() -> Arc<Self> {
        let s = Self::new();
        s.with(|d| {
            d.runs.push(run(RunSpec {
                id: RUN_RELEASE,
                bot: WATCH,
                group: OPS,
                topic: T_RELEASE,
                status: RunStatus::Running,
                by: ME,
                started: at(10, 16),
                ended: None,
                machine: "云端",
                plan: plan(&[
                    ("构建镜像", PlanState::Done),
                    ("逐个重启各群", PlanState::Current),
                    ("盯 15 分钟告警", PlanState::Pending),
                ]),
            }))
        });
        s
    }

    /// The sample team with a second member at work in 开发 / 虚拟列表: Scout checks the
    /// list while Forge builds it (two plan cards in one 话题, `topic-two-runs`).
    pub fn with_second_run() -> Arc<Self> {
        let s = Self::new();
        s.with(|d| {
            d.runs.push(run(RunSpec {
                id: RUN_VIRTUAL_CHECK,
                bot: SCOUT,
                group: DEV,
                topic: T_VIRTUAL,
                status: RunStatus::Running,
                by: ME,
                started: at(9, 40),
                ended: None,
                machine: "云端",
                plan: plan(&[
                    ("读压测脚本", PlanState::Done),
                    ("在三台电脑上跑一遍", PlanState::Current),
                    ("写验收记录", PlanState::Pending),
                ]),
            }))
        });
        s
    }

    /// Usable: 开发 / 话题列表虚拟滚动 as an order I wrote (`@Forge 写 → @Scout 审 → @Forge
    /// 按意见改`), at one state of [`RelayShape`]; or, for [`RelayShape::Refused`], no relay
    /// and the server's refusal of my order (Scout cannot take it).
    pub fn with_relay(shape: RelayShape) -> Arc<Self> {
        let s = Self::new();
        s.with(|d| {
            let step = |member: &str, note: &str, state: &str, run: Option<&str>| TopicRelayStep {
                member_id: member.into(),
                note: Some(note.into()),
                state: state.into(),
                run_id: run.map(Into::into),
            };
            // Forge wrote it; Scout's check is the second step.
            let forge_done = matches!(
                shape,
                RelayShape::Working
                    | RelayShape::Queued
                    | RelayShape::Paused
                    | RelayShape::Stopped
                    | RelayShape::Done
            );
            if forge_done && let Some(r) = d.runs.iter_mut().find(|r| r.id == RUN_VIRTUAL) {
                r.status = RunStatus::Done;
                r.ended_at = Some(at(9, 38));
                for p in &mut r.plan {
                    p.state = PlanState::Done;
                }
            }
            let scout = |status: RunStatus, ended: Option<Millis>| {
                run(RunSpec {
                    id: RUN_VIRTUAL_CHECK,
                    bot: SCOUT,
                    group: DEV,
                    topic: T_VIRTUAL,
                    status,
                    by: ME,
                    started: at(9, 40),
                    ended,
                    machine: "云端",
                    plan: plan(&[
                        ("读 Forge 的改动", PlanState::Done),
                        ("在三台电脑上跑一遍", PlanState::Current),
                        ("写审查意见", PlanState::Pending),
                    ]),
                })
            };
            let (state, current, steps, reason, stopped_by) = match shape {
                RelayShape::Working | RelayShape::TwoWorking => {
                    d.runs.push(scout(RunStatus::Running, None));
                    let first = if shape == RelayShape::TwoWorking {
                        "working"
                    } else {
                        "done"
                    };
                    ("running", 1, [first, "working", "pending"], None, None)
                }
                RelayShape::Queued => ("running", 1, ["done", "queued", "pending"], None, None),
                RelayShape::Paused => {
                    d.runs.push(scout(RunStatus::Failed, Some(at(9, 52))));
                    (
                        "paused",
                        1,
                        ["done", "failed", "pending"],
                        Some("step_blocked"),
                        None,
                    )
                }
                RelayShape::Stopped => {
                    d.runs.push(scout(RunStatus::Cancelled, Some(at(9, 47))));
                    (
                        "stopped",
                        1,
                        ["done", "stopped", "pending"],
                        Some("step_cancelled"),
                        Some(ME),
                    )
                }
                RelayShape::Done => {
                    d.runs.push(scout(RunStatus::Done, Some(at(9, 50))));
                    ("done", 2, ["done", "done", "done"], None, None)
                }
                RelayShape::Refused => {
                    d.relay_refusal = Some((
                        T_VIRTUAL.into(),
                        RelayRefusal {
                            reason: "relay.member_unavailable".into(),
                            member_ids: vec![SCOUT.into()],
                        },
                    ));
                    return;
                }
            };
            if shape == RelayShape::TwoWorking {
                // Two members at work, no order: the second run beside Forge's.
                return;
            }
            let runs = [Some(RUN_VIRTUAL), Some(RUN_VIRTUAL_CHECK), None];
            let notes = ["写", "审", "按意见改"];
            let members = [FORGE, SCOUT, FORGE];
            let relay = TopicRelay {
                id: "relay-v2-virtual".into(),
                started_by: ME.into(),
                goal: "话题列表改成虚拟滚动，一千条也不卡".into(),
                current,
                state: state.into(),
                reason: reason.map(Into::into),
                stopped_by: stopped_by.map(Into::into),
                updated_at: at(9, 52) as u64,
                steps: (0..3)
                    .map(|i| {
                        let run = (steps[i] != "pending" && steps[i] != "queued")
                            .then_some(runs[i])
                            .flatten();
                        step(members[i], notes[i], steps[i], run)
                    })
                    .collect(),
            };
            if let Some(t) = d.topics.iter_mut().find(|t| t.id == T_VIRTUAL) {
                t.relay = Some(relay);
                t.status = match state {
                    "paused" => TopicStatus::Blocked,
                    "done" => TopicStatus::Done,
                    "stopped" => TopicStatus::Open,
                    _ => TopicStatus::InProgress,
                };
                if state == "paused" {
                    t.waiting_on = vec![ME.into()];
                }
            }
        });
        s
    }

    fn with<R>(&self, f: impl FnOnce(&mut Data) -> R) -> R {
        f(&mut self.data.lock().unwrap_or_else(|e| e.into_inner()))
    }

    fn emit(&self, changes: Vec<Change>) {
        let mut subs = self.subscribers.lock().unwrap_or_else(|e| e.into_inner());
        subs.retain(|tx| tx.unbounded_send(changes.clone()).is_ok());
    }

    /// `--fake`: 停止 and 重试 of a sample 按顺序做 move it as the server would.
    fn relay_to(
        &self,
        group_id: &str,
        topic_id: &str,
        relay_id: &str,
        state: &str,
    ) -> AdminReply<()> {
        let done = self.with(|d| {
            let r = d
                .topics
                .iter_mut()
                .find(|t| t.id == topic_id)
                .and_then(|t| t.relay.as_mut())
                .filter(|r| r.id == relay_id)?;
            let cur = r.current as usize;
            if state == "stopped" {
                r.stopped_by = Some(ME.into());
                if let Some(s) = r.steps.get_mut(cur) {
                    s.state = "stopped".into();
                }
            } else {
                r.reason = None;
                if let Some(s) = r.steps.get_mut(cur) {
                    s.state = "working".into();
                }
            }
            r.state = state.into();
            Some(())
        });
        self.emit(vec![Change::Topics {
            group_id: group_id.into(),
        }]);
        reply_now(done.ok_or(AdminError::Conflict))
    }

    /// Takes over the live run in a scene (`live-takeover`).
    pub fn set_paused_by(&self, member: Option<MemberId>) {
        self.update_hold(|h| {
            h.paused_by = member.clone();
            h.hold_kind = member.as_ref().map(|_| "takeover".into());
            h.held_at = member.as_ref().map(|_| at(9, 50));
            h.phase = member.as_ref().map(|_| "taken_over".into());
        });
    }

    /// Usable (T6, T7 scenes): changes the live run's phase fields.
    pub fn update_hold(&self, f: impl FnOnce(&mut run_phase::RunHold)) {
        self.with(|d| f(&mut d.hold));
        self.emit(vec![Change::Runs {
            group_id: DEV.into(),
            topic_id: T_VIRTUAL.into(),
        }]);
    }

    /// Usable (T7 scenes): the commands of the person holding the live run.
    pub fn set_hand(&self, hand: Vec<run_phase::HandCommand>) {
        self.with(|d| d.hand = hand);
        self.emit(vec![Change::RunLog {
            run_id: RUN_VIRTUAL.into(),
        }]);
    }

    /// 继续 / 交还: the hold ends; a hand-back with commands leaves its summary.
    fn release_hold(&self) {
        self.with(|d| {
            let h = &mut d.hold;
            if let Some(by) = h.paused_by.take() {
                let took_over = h.hold_kind.as_deref() == Some("takeover");
                let commands = d.hand.iter().filter(|c| c.by == by).count() as u64;
                if took_over {
                    h.last_handback = Some(run_phase::Handback {
                        by,
                        held_ms: h
                            .held_at
                            .map_or(0, |t| (crate::selftest::clock::now_ms() - t).max(0) as u64),
                        commands,
                        files: Vec::new(),
                        head_from: None,
                        head_to: None,
                    });
                }
            }
            h.hold_kind = None;
            h.held_at = None;
            h.phase = None;
            d.hand.clear();
        });
        self.emit(vec![Change::Runs {
            group_id: DEV.into(),
            topic_id: T_VIRTUAL.into(),
        }]);
    }
}

impl ChatProvider for V2Samples {
    // v11/pb2-cards: the preview images of the sample compare card.
    fn card_image(
        &self,
        _group_id: &str,
        _topic_id: &str,
        file_id: &str,
    ) -> Option<std::path::PathBuf> {
        pages_samples::sample_image(file_id)
    }

    fn session(&self) -> SessionState {
        match self.inner.session() {
            SessionState::SignedIn { server_url, .. } => SessionState::SignedIn {
                me: self.members[0].clone(),
                server_url,
            },
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
        self.members.clone()
    }
    fn groups(&self) -> Vec<GroupSummary> {
        self.with(|d| d.groups.clone())
    }
    fn topics(&self, group_id: &str) -> Vec<Topic> {
        let mut out: Vec<Topic> = self.with(|d| {
            d.topics
                .iter()
                .filter(|t| t.group_id == group_id)
                .cloned()
                .map(|mut t| {
                    let msgs = d.messages.get(&t.id).map(|m| m.len()).unwrap_or(0);
                    t.message_count = msgs as u32;
                    t.last_seq = msgs as u64;
                    if self.quiet {
                        t.waiting_on.retain(|m| m != ME);
                    }
                    if t.participants.is_empty() {
                        let is_bot =
                            |id: &str| self.members.iter().any(|m| m.id == id && m.is_bot());
                        let runs: Vec<BotRunSummary> = d
                            .runs
                            .iter()
                            .filter(|r| !(self.quiet && r.status.is_active()))
                            .cloned()
                            .collect();
                        t.participants = crate::fake::sample_participants(&t, &runs, &is_bot);
                    }
                    t
                })
                .collect()
        });
        out.sort_by_key(|t| std::cmp::Reverse(t.last_activity_at));
        out
    }
    fn todo(&self) -> Vec<TodoItem> {
        if self.quiet {
            return Vec::new();
        }
        let groups = self.groups();
        let mut out: Vec<TodoItem> = groups
            .iter()
            .flat_map(|g| {
                self.topics(&g.id)
                    .into_iter()
                    .filter(|t| t.waiting_on.iter().any(|m| m == ME))
                    .map(|t| TodoItem {
                        group_id: g.id.clone(),
                        group_name: g.name.clone(),
                        topic: t,
                        reason: TodoReason::WaitingOnMe,
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        out.sort_by_key(|t| std::cmp::Reverse(t.topic.last_activity_at));
        out
    }
    fn messages(&self, _group_id: &str, topic_id: &str, limit: usize) -> MessageWindow {
        let all = self.with(|d| d.messages.get(topic_id).cloned().unwrap_or_default());
        let start = all.len().saturating_sub(limit);
        MessageWindow {
            messages: all[start..].to_vec(),
            has_older: start > 0,
            loading_older: false,
        }
    }
    fn load_older(&self, _group_id: &str, _topic_id: &str) {}
    fn typing(&self, _group_id: &str, _topic_id: &str) -> Vec<MemberId> {
        Vec::new()
    }
    fn artifacts(&self, _group_id: &str, topic_id: &str) -> Vec<ArtifactSummary> {
        self.with(|d| {
            d.artifacts
                .iter()
                .filter(|(_, a)| a.topic_id == topic_id)
                .map(|(_, a)| a.clone())
                .collect()
        })
    }
    fn artifact(&self, artifact_id: &str) -> Option<ArtifactDetail> {
        let (group_id, summary) = self.with(|d| {
            d.artifacts
                .iter()
                .find(|(_, a)| a.id == artifact_id)
                .cloned()
        })?;
        let versions = (1..=summary.version)
            .rev()
            .map(|v| ArtifactVersionInfo {
                version: v,
                note: None,
                created_by: summary.created_by.clone(),
                created_at: summary.updated_at - (summary.version - v) as i64 * 24 * 60 * MIN,
                // --- v11/media ---
                entry: "index.html".into(),
                files: vec![ArtifactVersionFile {
                    path: "index.html".into(),
                    size: 18_400,
                    mime: "text/html".into(),
                }],
                // --- end v11/media ---
            })
            .collect();
        Some(ArtifactDetail {
            summary,
            group_id,
            versions,
        })
    }
    fn send(&self, request: SendRequest) -> ClientMsgId {
        let id = self.with(|d| {
            let list = d.messages.entry(request.topic_id.clone()).or_default();
            let seq = list.len() as u64 + 1;
            let mut m = message(
                &request.group_id,
                &request.topic_id,
                seq,
                ME,
                crate::selftest::clock::utc_now().timestamp_millis(),
                &request.body,
            );
            m.client_msg_id = format!("v2-c{seq}");
            m.mentions = request.mentions.clone();
            m.quote_of = request.quote_of.clone();
            let id = m.client_msg_id.clone();
            list.push(m);
            id
        });
        self.emit(vec![Change::Messages {
            group_id: request.group_id,
            topic_id: request.topic_id,
        }]);
        id
    }
    /// A quote from another 话题 (开个话题 quotes the 群聊): any message of the
    /// sample team.
    fn quoted_message(
        &self,
        _group_id: &str,
        _quoting_id: &str,
        quoted_id: &str,
    ) -> Option<QuotedMessage> {
        self.with(|d| {
            d.messages
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
    /// A 话题 of the sample team: new or 开个话题 (its first message quotes a
    /// 群聊 message, and an empty note takes the title from the quote).
    fn start_topic(&self, request: StartTopicRequest) -> ClientMsgId {
        let now = crate::selftest::clock::utc_now().timestamp_millis();
        let started = self.with(|d| {
            if !d.groups.iter().any(|g| g.id == request.group_id) {
                return None;
            }
            let quoted = request.quote_of.as_ref().and_then(|q| {
                d.messages
                    .values()
                    .flatten()
                    .find(|m| m.id == *q)
                    .map(|m| m.body.clone())
            });
            let first_line = |text: &str| -> String {
                let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
                line.trim().chars().take(80).collect()
            };
            let title = request
                .title
                .clone()
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| {
                    let own = first_line(&request.body);
                    if own.is_empty() {
                        quoted.as_deref().map(first_line).unwrap_or_default()
                    } else {
                        own
                    }
                });
            let n = d.topics.len() + 1;
            let id = format!("t-v2-new-{n}");
            let mut m = message(&request.group_id, &id, 1, ME, now, &request.body);
            m.client_msg_id = format!("v2-c-{id}");
            m.quote_of = request.quote_of.clone();
            m.mentions = request.mentions.clone();
            let cid = m.client_msg_id.clone();
            d.messages.insert(id.clone(), vec![m]);
            d.topics.push(Topic {
                id: id.clone(),
                group_id: request.group_id.clone(),
                title,
                status: TopicStatus::Open,
                waiting_on: Vec::new(),
                owner_id: Some(ME.into()),
                created_by: ME.into(),
                created_at: now,
                root_message_id: format!("{id}-m1"),
                last_seq: 1,
                last_activity_at: now,
                message_count: 1,
                participants: Vec::new(),
                relay: None,
                unread: 0,
                mentions: 0,
            });
            Some((id, cid))
        });
        let Some((topic_id, cid)) = started else {
            return self.inner.start_topic(request);
        };
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
        cid
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
    fn draft(&self, key: &DraftKey) -> Draft {
        self.inner.draft(key)
    }
    fn save_draft_text(&self, key: &DraftKey, body: String, mentions: Vec<MemberId>) {
        self.inner.save_draft_text(key, body, mentions)
    }
    // The quote and the pending files live in the wrapped draft, like the text.
    fn set_quote(&self, key: &DraftKey, quote_of: Option<MessageId>) {
        self.inner.set_quote(key, quote_of);
        self.emit(vec![Change::Draft { key: key.clone() }]);
    }
    fn attach_files(&self, key: &DraftKey, files: Vec<NewFile>) {
        self.inner.attach_files(key, files);
        self.emit(vec![Change::Draft { key: key.clone() }]);
    }
    fn remove_file(&self, key: &DraftKey, file_id: &str) {
        self.inner.remove_file(key, file_id);
        self.emit(vec![Change::Draft { key: key.clone() }]);
    }
    fn take_rejected_files(&self, key: &DraftKey) -> Vec<RejectedFile> {
        self.inner.take_rejected_files(key)
    }
    fn group_muted(&self, group_id: &str) -> bool {
        self.with(|d| d.muted_groups.contains(group_id))
    }
    fn set_group_muted(&self, group_id: &str, muted: bool) {
        self.with(|d| {
            if muted {
                d.muted_groups.insert(group_id.to_string());
            } else {
                d.muted_groups.remove(group_id);
            }
        });
        self.emit(vec![Change::Groups]);
    }
    fn group_prefs(&self, group_id: &str) -> GroupPrefs {
        self.with(|d| {
            let dm = d
                .groups
                .iter()
                .any(|g| g.id == group_id && g.kind == GroupKind::Dm);
            GroupPrefs {
                muted: d.muted_groups.contains(group_id),
                notify: d.levels.get(group_id).copied().unwrap_or(if dm {
                    NotifyLevel::All
                } else {
                    NotifyLevel::Mentions
                }),
            }
        })
    }
    fn set_group_prefs(
        &self,
        group_id: &str,
        muted: Option<bool>,
        notify: Option<NotifyLevel>,
    ) -> AdminReply<()> {
        self.with(|d| {
            match muted {
                Some(true) => {
                    d.muted_groups.insert(group_id.to_string());
                }
                Some(false) => {
                    d.muted_groups.remove(group_id);
                }
                None => {}
            }
            if let Some(level) = notify {
                d.levels.insert(group_id.to_string(), level);
            }
        });
        self.emit(vec![Change::Groups]);
        reply_now(Ok(()))
    }
    // --- rail track: I follow every 话题 of the 群 I am in (until I
    // unfollow one), so the rail's 我参与的 filter lists what the artboards
    // show.
    fn topic_following(&self, group_id: &str, topic_id: &str) -> bool {
        self.with(|d| {
            !d.unfollowed.contains(topic_id)
                && d.groups
                    .iter()
                    .any(|g| g.id == group_id && g.member_ids.iter().any(|m| m == ME))
        })
    }
    fn set_topic_following(&self, group_id: &str, topic_id: &str, follow: bool) {
        self.with(|d| {
            if follow {
                d.unfollowed.remove(topic_id);
            } else {
                d.unfollowed.insert(topic_id.to_string());
            }
        });
        self.emit(vec![Change::Topics {
            group_id: group_id.to_string(),
        }]);
    }
    fn topic_muted(&self, _group_id: &str, topic_id: &str) -> bool {
        self.with(|d| d.muted_topics.contains(topic_id))
    }
    fn set_topic_muted(&self, group_id: &str, topic_id: &str, muted: bool) {
        self.with(|d| {
            if muted {
                d.muted_topics.insert(topic_id.to_string());
            } else {
                d.muted_topics.remove(topic_id);
            }
        });
        self.emit(vec![Change::Topics {
            group_id: group_id.to_string(),
        }]);
    }
    // --- end rail track
    fn relay_refusal(&self, _group_id: &str, topic_id: &str) -> Option<RelayRefusal> {
        self.with(|d| {
            d.relay_refusal
                .as_ref()
                .filter(|(t, _)| t == topic_id)
                .map(|(_, r)| r.clone())
        })
    }
    fn stop_relay(&self, group_id: &str, topic_id: &str, relay_id: &str) -> AdminReply<()> {
        self.relay_to(group_id, topic_id, relay_id, "stopped")
    }
    fn retry_relay(&self, group_id: &str, topic_id: &str, relay_id: &str) -> AdminReply<()> {
        self.relay_to(group_id, topic_id, relay_id, "running")
    }
    fn runs(&self, _group_id: &str, topic_id: &str) -> Vec<BotRunSummary> {
        let mut runs: Vec<BotRunSummary> = self.with(|d| {
            d.runs
                .iter()
                .filter(|r| r.topic_id == topic_id)
                .filter(|r| !(self.quiet && r.status.is_active()))
                .cloned()
                .map(|mut r| {
                    if r.id == RUN_VIRTUAL {
                        r.hold = d.hold.clone();
                    }
                    r
                })
                .collect()
        });
        runs.sort_by_key(|r| std::cmp::Reverse(r.created_at));
        runs
    }
    fn card(&self, _group_id: &str, message_id: &str) -> Option<Arc<card::Card>> {
        self.with(|d| d.cards.get(message_id).cloned())
    }

    // --- v2 ---

    fn desk_note(&self) -> Option<DeskNote> {
        if self.quiet {
            return None;
        }
        Some(DeskNote {
            member_id: JOJO.into(),
            text: "先批 Watch 的发布。晚上 8 点是群里的高峰，越早发越稳。".into(),
            at: at(10, 21),
        })
    }
    fn asks_for_me(&self) -> Vec<AskItem> {
        if self.quiet {
            return Vec::new();
        }
        let ask =
            |g: &str, t: &str, msg: Option<String>, from: &str, kind, title: &str, when| AskItem {
                group_id: g.into(),
                topic_id: t.into(),
                message_id: msg,
                from: from.into(),
                kind,
                title: title.into(),
                at: when,
            };
        let approved = self.with(|d| d.approval.is_some());
        let mut asks = vec![
            ask(
                OPS,
                T_RELEASE,
                Some(format!("{T_RELEASE}-ask")),
                WATCH,
                AskKind::Approval,
                "把 orderbot v2.14 发布到生产吗？",
                at_s(10, 20, 45),
            ),
            ask(
                DEV,
                T_RECONNECT,
                Some(format!("{T_RECONNECT}-m6")),
                FORGE,
                AskKind::Review,
                "改好了，请你审 PR #61",
                at(9, 55),
            ),
            ask(
                GROWTH,
                T_PROMO,
                Some(format!("{T_PROMO}-ask")),
                TARO,
                AskKind::Draft,
                "这条微博确认后，周四 20:00 发出",
                at(10, 20),
            ),
            ask(
                DEV,
                T_INVITE,
                Some(format!("{T_INVITE}-ask")),
                FORGE,
                AskKind::Choice,
                "邀请码输入框用哪种？",
                at(9, 58),
            ),
        ];
        if approved {
            asks.retain(|a| a.topic_id != T_RECONNECT);
        }
        asks
    }
    fn done_today(&self) -> Vec<DoneItem> {
        if self.quiet {
            return Vec::new();
        }
        // Every result delivered today, newest first: the 首页 KPI, the
        // rail and 成员 count the same three (review 5 C-18).
        vec![
            DoneItem {
                group_id: DEV.into(),
                topic_id: T_RECONNECT.into(),
                member_id: FORGE.into(),
                title: "改好了重连后的重复消息".into(),
                at: at(9, 55),
                proof: vec![
                    Proof::Pr {
                        number: 61,
                        additions: 214,
                        deletions: 63,
                    },
                    Proof::Artifact {
                        artifact_id: ART_RECONNECT.into(),
                        title: "重连测试报告".into(),
                        version: 2,
                    },
                ],
            },
            DoneItem {
                group_id: GROWTH.into(),
                topic_id: T_REPORT.into(),
                member_id: LEDGER.into(),
                title: "做好了 10 月数据月报".into(),
                at: at(9, 40),
                proof: vec![Proof::Artifact {
                    artifact_id: ART_REPORT.into(),
                    title: "10 月数据月报".into(),
                    version: 3,
                }],
            },
            DoneItem {
                group_id: OPS.into(),
                topic_id: T_STAGING.into(),
                member_id: SCOUT.into(),
                title: "巡检了 staging".into(),
                at: at(9, 12),
                proof: vec![Proof::Checks {
                    passed: 24,
                    total: 24,
                }],
            },
        ]
    }
    fn queue(&self) -> Vec<QueuedItem> {
        if self.quiet {
            return Vec::new();
        }
        vec![
            QueuedItem {
                member_id: POCKET.into(),
                title: "发晨报".into(),
                at: Some(days_ago_at(0, 9, 12)),
            },
            QueuedItem {
                member_id: SCOUT.into(),
                title: "巡检 staging".into(),
                at: Some(days_ago_at(0, 9, 40)),
            },
        ]
    }
    fn topic_changes(&self, _group_id: &str, topic_id: &str) -> Option<TopicChanges> {
        (topic_id == T_RECONNECT).then(|| TopicChanges {
            number: 61,
            title: "按 seq 去重，从最后确认的 seq 续传".into(),
            url: None,
            additions: 214,
            deletions: 63,
            files: 12,
            checks_passed: 3,
            checks_total: 3,
            state: if self.with(|d| d.approval.is_some()) {
                ChangeState::Merging
            } else {
                ChangeState::Open
            },
        })
    }
    fn run_usage(&self, run_id: &str) -> Option<RunUsage> {
        match run_id {
            RUN_RECONNECT => Some(RunUsage {
                duration_ms: 41 * 60_000,
                // The sample team's costs are in yuan (the artboards' ¥6.20).
                cost_cents: Some(620),
                currency: crate::data::v2::Currency::Cny,
                input_tokens: None,
                output_tokens: None,
            }),
            RUN_VIRTUAL => Some(RunUsage {
                duration_ms: 20 * 60_000,
                // The sample team's costs are in yuan (the artboards' ¥6.20).
                cost_cents: Some(310),
                currency: crate::data::v2::Currency::Cny,
                input_tokens: None,
                output_tokens: None,
            }),
            _ => None,
        }
    }
    fn topic_decisions(&self, _group_id: &str, topic_id: &str) -> Vec<Decision> {
        if topic_id != T_RECONNECT {
            return Vec::new();
        }
        vec![Decision {
            id: "d-backoff".into(),
            group_id: DEV.into(),
            topic_id: Some(T_RECONNECT.into()),
            text: "改成指数退避，放进同一个 PR".into(),
            by: ME.into(),
            at: at(9, 31),
        }]
    }
    fn group_decisions(&self, group_id: &str) -> Vec<Decision> {
        let d = |id: &str, by: &str, text: &str, when: Millis| Decision {
            id: id.into(),
            group_id: group_id.into(),
            topic_id: None,
            text: text.into(),
            by: by.into(),
            at: when,
        };
        // pages track: decisions added with 记一条 come first.
        let mut out = pages_samples::recorded(group_id);
        out.extend(match group_id {
            DEV | GROWTH | OPS => vec![
                d("d-dau", ME, "日活按去重设备算", days_ago_at(2, 11, 0)),
                d(
                    "d-invite",
                    ME,
                    "登录只用邀请码，不做手机号",
                    days_ago_at(4, 15, 0),
                ),
                d(
                    "d-scout",
                    TATA,
                    "orderbot 发布前必须 Scout 在 staging 验收",
                    days_ago_at(6, 10, 0),
                ),
            ],
            _ => Vec::new(),
        });
        out
    }
    fn group_purpose(&self, group_id: &str) -> Option<String> {
        match group_id {
            DEV => Some("orderbot、拾光和 TeamChat 的开发".into()),
            OPS => Some("orderbot 和拾光的上线、巡检和告警".into()),
            GROWTH => Some("拾光的增长、数据和社媒".into()),
            ALL => Some("全员的周报和通知".into()),
            _ => None,
        }
    }
    fn side_chat_default_member(&self) -> Option<MemberId> {
        Some(JOJO.into())
    }
    /// The memory screens ask an owner whether the server has an AI 服务 (盯 4.6).
    fn ai_service(&self) -> crate::data::AdminReply<crate::data::AiServiceSettings> {
        self.inner.ai_service()
    }
    fn memories(
        &self,
        scope: &crate::data::memory::MemoryScope,
    ) -> crate::data::AdminReply<Vec<crate::data::memory::Memory>> {
        self.inner.memories(scope)
    }
    fn memory_updates(
        &self,
        scope: &crate::data::memory::MemoryScope,
    ) -> crate::data::AdminReply<crate::data::memory::MemoryDay> {
        self.inner.memory_updates(scope)
    }
    fn create_memory(
        &self,
        new: crate::data::memory::NewMemory,
    ) -> crate::data::AdminReply<crate::data::memory::Memory> {
        self.inner.create_memory(new)
    }
    fn update_memory(
        &self,
        memory_id: &str,
        edit: crate::data::memory::MemoryEdit,
    ) -> crate::data::AdminReply<crate::data::memory::Memory> {
        self.inner.update_memory(memory_id, edit)
    }
    fn undo_memory_update(
        &self,
        update_id: &str,
    ) -> crate::data::AdminReply<crate::data::memory::MemoryUpdate> {
        self.inner.undo_memory_update(update_id)
    }
    /// 需要你批准的事 of the samples: the team's rules (`GET /v1/settings/approvals`).
    fn approval_defaults(&self) -> crate::data::AdminReply<crate::data::ApprovalDefaults> {
        crate::data::reply_now(Ok(crate::data::ApprovalDefaults {
            delete_data: false,
            external_publish: false,
            ..crate::data::ApprovalDefaults::default()
        }))
    }
    fn side_chat(&self, _group_id: &str, topic_id: &str, member_id: &str) -> SideChatThread {
        SideChatThread {
            member_id: member_id.into(),
            messages: if topic_id == T_RECONNECT && member_id == JOJO {
                self.with(|d| d.side_chat.clone())
            } else {
                Vec::new()
            },
        }
    }
    fn send_side_chat(
        &self,
        group_id: &str,
        topic_id: &str,
        _member_id: &str,
        body: String,
        refs: Vec<crate::data::DraftRef>,
    ) {
        // The server writes the chips' quote lines before the body; the fake does the same.
        let body = crate::views::refs::side_chat_body(body.trim(), &refs);
        self.with(|d| {
            let n = d.side_chat.len() + 1;
            d.side_chat.push(SideChatMessage {
                id: format!("sc-{n}"),
                author_id: ME.into(),
                body,
                at: crate::selftest::clock::utc_now().timestamp_millis(),
                refs: Vec::new(),
                sources: Vec::new(),
                suggestion: None,
                streaming: false,
            })
        });
        self.emit(vec![Change::Messages {
            group_id: group_id.into(),
            topic_id: topic_id.into(),
        }]);
    }
    fn post_side_chat_suggestion(
        &self,
        group_id: &str,
        topic_id: &str,
        _member_id: &str,
        message_id: &str,
    ) {
        let text = self.with(|d| {
            let m = d.side_chat.iter_mut().find(|m| m.id == message_id)?;
            let s = m.suggestion.as_mut().filter(|s| s.posted_at.is_none())?;
            s.posted_at = Some(crate::selftest::clock::utc_now().timestamp_millis());
            Some(s.text.clone())
        });
        if let Some(text) = text {
            self.send(SendRequest {
                group_id: group_id.into(),
                topic_id: topic_id.into(),
                body: text,
                mentions: Vec::new(),
                quote_of: None,
            });
        }
    }
    fn machine_screen(&self, run_id: &str) -> Option<MachineScreen> {
        if run_id != RUN_VIRTUAL {
            return None;
        }
        let line = |kind, text: &str, m: u32, step| ScreenLine {
            kind,
            text: text.into(),
            at: at(9, m),
            step: Some(step),
        };
        use ScreenLineKind as K;
        Some(MachineScreen {
            run_id: run_id.into(),
            machine_name: Some("Mac mini".into()),
            lines: vec![
                // panes track: the 实时查看 artboard's terminal, one screen per step.
                line(K::Command, "rg -n \"uniform_list\" apps/desktop/src", 32, 0),
                line(K::Output, "views/topic_list.rs:412  uniform_list(", 32, 0),
                line(K::Command, "cargo build -p desktop", 40, 1),
                line(K::Output, "Finished `dev` profile in 38.2s", 41, 1),
                line(K::Command, "cargo run -p stress -- -n 10000", 46, 2),
                line(K::Output, "生成 10,000 条消息… 完成", 47, 2),
                line(K::Output, "打开话题列表，自动滚动 60 秒", 47, 2),
                line(K::Result, "滚动 60 秒：平均 118 fps，最低 57 fps", 48, 2),
                line(K::Result, "内存 212 MB", 48, 2),
                line(K::Output, "跳到第 8,000 条，检查滚动位置…", 49, 2),
            ],
            started_at: Some(at(9, 31)),
            takeover_url: None,
            paused_by: self.with(|d| d.hold.paused_by.clone()),
        })
    }
    fn acceptance_criteria(&self, run_id: &str) -> Vec<AcceptanceItem> {
        if run_id != RUN_VIRTUAL {
            return Vec::new();
        }
        vec![
            // panes track: the 实时查看 artboard's 验收标准.
            AcceptanceItem {
                text: "一万条消息滚动不卡".into(),
                met: Some(true),
                evidence: Some("现在 57 fps".into()),
                ..Default::default()
            },
            AcceptanceItem {
                text: "内存不涨".into(),
                met: Some(true),
                evidence: Some("现在 212 MB".into()),
                ..Default::default()
            },
            AcceptanceItem {
                text: "跳到第 8,000 条后位置不跳".into(),
                ..Default::default()
            },
        ]
    }
    fn run_step_times(&self, run_id: &str) -> Vec<Millis> {
        match run_id {
            RUN_VIRTUAL => vec![at(9, 31), at(9, 35), at(9, 46)],
            RUN_RECONNECT => vec![at(9, 14), at(9, 22), at(9, 30), at(9, 41), at(9, 52)],
            _ => Vec::new(),
        }
    }
    fn take_over_run(&self, _group_id: &str, _topic_id: &str, _run_id: &str) -> AdminReply<()> {
        self.update_hold(|h| {
            h.paused_by = Some(ME.into());
            h.hold_kind = Some("takeover".into());
            h.held_at = Some(crate::selftest::clock::now_ms());
            h.phase = Some("taken_over".into());
        });
        reply_now(Ok(()))
    }
    fn hand_back_run(&self, _group_id: &str, _topic_id: &str, _run_id: &str) -> AdminReply<()> {
        self.release_hold();
        reply_now(Ok(()))
    }
    fn pause_run(&self, _group_id: &str, _topic_id: &str, _run_id: &str) -> AdminReply<()> {
        self.update_hold(|h| {
            h.paused_by = Some(ME.into());
            h.hold_kind = Some("pause".into());
            h.held_at = Some(crate::selftest::clock::now_ms());
            h.phase = Some("paused".into());
        });
        reply_now(Ok(()))
    }
    fn resume_run(
        &self,
        _g: &str,
        _t: &str,
        _run_id: &str,
        _note: Option<String>,
    ) -> AdminReply<()> {
        self.release_hold();
        reply_now(Ok(()))
    }
    /// A sample command ends at once: its first word picks the output.
    fn run_command(&self, run_id: &str, command: String) -> AdminReply<String> {
        if run_id != RUN_VIRTUAL {
            return reply_now(Err(AdminError::Unsupported));
        }
        let held = self.with(|d| d.hold.paused_by.as_deref() == Some(ME));
        if !held {
            return reply_now(Err(AdminError::Conflict));
        }
        let id = self.with(|d| format!("hc-{}", d.hand.len() + 1));
        let (ok, output) = match command.split_whitespace().next() {
            Some("git") => (
                true,
                "On branch bot/forge-virtual\nnothing to commit, working tree clean\n",
            ),
            Some("ls") => (true, "Cargo.toml\napps\ncrates\ndocs\n"),
            _ => (false, "command not found\n"),
        };
        self.with(|d| {
            d.hand.push(run_phase::HandCommand {
                id: id.clone(),
                command,
                by: ME.into(),
                output: output.into(),
                result: Some(run_phase::HandResult {
                    ok,
                    exit_code: Some(if ok { 0 } else { 127 }),
                    ms: 40,
                    ..Default::default()
                }),
                at: crate::selftest::clock::now_ms(),
            })
        });
        self.emit(vec![Change::RunLog {
            run_id: RUN_VIRTUAL.into(),
        }]);
        reply_now(Ok(id))
    }
    fn cancel_run_command(&self, _run_id: &str, command_id: &str) -> AdminReply<()> {
        self.with(|d| {
            if let Some(c) = d
                .hand
                .iter_mut()
                .find(|c| c.id == command_id && c.result.is_none())
            {
                c.result = Some(run_phase::HandResult::default());
            }
        });
        self.emit(vec![Change::RunLog {
            run_id: RUN_VIRTUAL.into(),
        }]);
        reply_now(Ok(()))
    }
    fn hand_commands(&self, run_id: &str) -> Vec<run_phase::HandCommand> {
        if run_id != RUN_VIRTUAL {
            return Vec::new();
        }
        self.with(|d| d.hand.clone())
    }
    /// The sample run is on this computer (a made-up folder; 在访达中打开 shows it).
    fn run_work_folder(&self, run_id: &str) -> Option<std::path::PathBuf> {
        (run_id == RUN_VIRTUAL).then(|| {
            std::path::PathBuf::from(format!(
                "~/{}-runs/lines/forge-virtual/work",
                env!("PRODUCT_SLUG")
            ))
        })
    }
    fn member_stats(&self, member_id: &str) -> Option<MemberStats> {
        let mut stats = MemberStats::default();
        for r in self.with(|d| d.runs.clone()) {
            if r.bot_id == member_id && r.status.is_active() {
                stats.active.push((r.group_id.clone(), r.topic_id.clone()));
            }
        }
        // pages track: this week's 话题 of everyone (Members.dc.html).
        stats.topics_this_week = match member_id {
            WATCH => 5,
            TARO => 3,
            JOJO => 14,
            TATA => 4,
            LEDGER => 4,
            POCKET => 7,
            SCOUT => 9,
            XIAOWANG => 3,
            LIXUE => 2,
            ALEX => 1,
            ME => 9,
            _ => 0,
        };
        // This week's deliveries include today's (`done_today`): Ledger's
        // 10 月数据月报 and Scout's staging check (review 5 C-18).
        stats.done_this_week = match member_id {
            LEDGER | SCOUT => 1,
            _ => 0,
        };
        if member_id == FORGE {
            stats.queued = 1;
            stats.topics_this_week = 6;
            stats.done_this_week = 6;
            stats.artifacts_this_week = 4;
            stats.cost_cents_this_week = Some(13_200);
            // `costUsd` and `weeklyBudgetCents` are US dollars.
            stats.cost_currency = crate::data::v2::Currency::Usd;
            stats.reports_to = Some(TATA.into());
            stats.tools = vec!["GitHub".into(), "Cloudflare 只读".into(), "staging".into()];
            stats.machine = Some(MemberMachine {
                name: "Mac mini".into(),
                online: true,
                busy: true,
                disk_used_pct: Some(61),
            });
            stats.problems = vec![MemberProblem {
                text: "CI 超时，PR #59 的检查没跑完".into(),
                at: days_ago_at(1, 23, 10),
                run_id: Some("run-v2-ci".into()),
            }];
        }
        Some(stats)
    }
    fn all_artifacts(&self) -> Vec<(GroupId, ArtifactSummary)> {
        let mut out = self.with(|d| d.artifacts.clone());
        out.sort_by_key(|(_, a)| std::cmp::Reverse(a.updated_at));
        out
    }
    fn artifact_story(&self, artifact_id: &str) -> Option<ArtifactStory> {
        match artifact_id {
            ART_REPORT => Some(ArtifactStory {
                topic: Some((GROWTH.into(), T_REPORT.into())),
                made_by: Some(LEDGER.into()),
                asked_by: Some(TATA.into()),
                decisions: self
                    .group_decisions(GROWTH)
                    .into_iter()
                    .filter(|d| d.id == "d-dau")
                    .collect(),
                version_notes: vec![
                    (3, "加了渠道拆分".into()),
                    (2, "修正日活口径".into()),
                    (1, "第一版".into()),
                ],
                cited_by: vec![
                    (ALL.into(), T_WEEKLY.into(), "本周周报".into()),
                    (GROWTH.into(), T_V4.into(), "拾光 4.0 需求".into()),
                ],
            }),
            ART_RECONNECT => Some(ArtifactStory {
                topic: Some((DEV.into(), T_RECONNECT.into())),
                made_by: Some(FORGE.into()),
                asked_by: Some(XIAOWANG.into()),
                decisions: self.topic_decisions(DEV, T_RECONNECT),
                version_notes: vec![(2, "补了弱网场景".into()), (1, "第一版".into())],
                cited_by: Vec::new(),
            }),
            _ => None,
        }
    }

    // --- desk track (1.1.0 首页): fake/v2_desk.rs ---

    fn card_presses(&self, _group_id: &str, message_id: &str) -> Vec<CardPress> {
        let desk = self.desk.lock().unwrap_or_else(|e| e.into_inner());
        desk.presses.get(message_id).cloned().unwrap_or_default()
    }
    fn press_card_action(&self, group_id: &str, topic_id: &str, message_id: &str, action_id: &str) {
        let label = self.with(|d| {
            d.cards.get(message_id).and_then(|c| {
                c.actions
                    .as_deref()
                    .unwrap_or(&[])
                    .iter()
                    .find(|a| a.id == action_id)
                    .map(|a| a.label.clone())
            })
        });
        let now = crate::selftest::clock::local_now().timestamp_millis();
        self.desk
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .press(message_id, action_id, label, now);
        self.emit(vec![Change::Messages {
            group_id: group_id.into(),
            topic_id: topic_id.into(),
        }]);
    }

    // --- end of the desk track block ---

    // --- topic track (v11/topic) ---

    fn message_proof(&self, _group_id: &str, message_id: &str) -> Vec<Proof> {
        if message_id != format!("{T_RECONNECT}-m6") {
            return Vec::new();
        }
        vec![
            Proof::Pr {
                number: 61,
                additions: 214,
                deletions: 63,
            },
            Proof::Tests {
                passed: 148,
                failed: 0,
            },
            Proof::Recording {
                duration_ms: 42_000,
            },
            Proof::Artifact {
                artifact_id: ART_RECONNECT.into(),
                title: "重连测试报告".into(),
                version: 2,
            },
        ]
    }
    fn media_duration(&self, file: &FileAttachment) -> Option<u64> {
        (file.file_id == "f-v2-video").then_some(18_000)
    }
    fn artifact_preview(&self, artifact_id: &str, _version: u32) -> Option<ArtifactPreview> {
        if artifact_id != ART_RECONNECT {
            return None;
        }
        let row = |label: &str| PreviewRow {
            label: label.into(),
            ok: Some(true),
            value: "0".into(),
        };
        let bar = |label: &str, share: f32, value: &str, highlight: bool| PreviewBar {
            label: label.into(),
            share,
            value: value.into(),
            highlight,
        };
        Some(ArtifactPreview {
            eyebrow: Some("测试报告 · staging".into()),
            title: "重连测试报告".into(),
            summary: Some(
                "修复后的客户端，每个场景跑 20 次，记录重连耗时和重复消息的条数。".into(),
            ),
            metrics: vec![
                PreviewMetric {
                    label: "场景".into(),
                    value: "6".into(),
                    unit: Some("/ 6".into()),
                    note: Some("全部通过".into()),
                },
                PreviewMetric {
                    label: "最长重连".into(),
                    value: "1.8".into(),
                    unit: Some("秒".into()),
                    note: Some("合盖休眠 10 分钟后".into()),
                },
            ],
            table_title: Some("逐项结果".into()),
            columns: vec!["场景".into(), "结果".into(), "重复".into()],
            rows: vec![
                row("断网 5 秒"),
                row("断网 60 秒"),
                row("合盖休眠 10 分钟"),
                row("切换 Wi-Fi"),
                row("服务器重启"),
                row("弱网（丢包 20%）"),
            ],
            chart_title: Some("重连耗时".into()),
            chart_note: Some("20 次的中位数，单位秒".into()),
            bars: vec![
                bar("断网 5 秒", 0.3, "0.6", false),
                bar("断网 60 秒", 0.6, "1.2", false),
                bar("合盖休眠 10 分钟", 0.9, "1.8", true),
                bar("切换 Wi-Fi", 0.45, "0.9", false),
                bar("服务器重启", 0.7, "1.4", false),
                bar("弱网", 0.55, "1.1", false),
            ],
            axis: vec!["0".into(), "1".into(), "2".into()],
        })
    }
    fn changes_approval(&self, _group_id: &str, topic_id: &str) -> Option<(MemberId, Millis)> {
        (topic_id == T_RECONNECT)
            .then(|| self.with(|d| d.approval.clone()))
            .flatten()
    }
    fn approve_changes(&self, group_id: &str, topic_id: &str) -> AdminReply<()> {
        if topic_id != T_RECONNECT {
            return reply_now(Err(AdminError::NotFound));
        }
        self.with(|d| {
            d.approval = Some((
                ME.into(),
                crate::selftest::clock::utc_now().timestamp_millis(),
            ))
        });
        self.emit(vec![
            Change::Topics {
                group_id: group_id.into(),
            },
            Change::Messages {
                group_id: group_id.into(),
                topic_id: topic_id.into(),
            },
            Change::Todo,
        ]);
        reply_now(Ok(()))
    }
    /// r2-topic: the samples never send, so my approval stays queued.
    fn changes_approval_queued(&self, group_id: &str, topic_id: &str) -> bool {
        self.changes_approval(group_id, topic_id)
            .is_some_and(|(by, _)| by == ME)
    }
    fn withdraw_changes_approval(&self, group_id: &str, topic_id: &str) -> AdminReply<()> {
        self.with(|d| d.approval = None);
        self.emit(vec![
            Change::Topics {
                group_id: group_id.into(),
            },
            Change::Messages {
                group_id: group_id.into(),
                topic_id: topic_id.into(),
            },
            Change::Todo,
        ]);
        reply_now(Ok(()))
    }

    // --- pages track: the 群, 成员 and 产出 pages ---

    fn clock_now(&self) -> Millis {
        // One clock (T, P-01): the self-test's pinned now, so a working card, its step
        // times and the pages agree.
        crate::selftest::clock::now_ms()
    }
    fn topic_glance(&self, group_id: &str, topic_id: &str) -> pages::TopicGlance {
        pages_samples::topic_glance(group_id, topic_id)
    }
    fn group_recent_done(&self, group_id: &str) -> Vec<pages::RecentDone> {
        pages_samples::recent_done(group_id)
    }
    fn group_artifacts(&self, group_id: &str) -> Vec<ArtifactSummary> {
        let mut out: Vec<ArtifactSummary> = self
            .with(|d| d.artifacts.clone())
            .into_iter()
            .filter(|(g, _)| g == group_id)
            .map(|(_, a)| a)
            .collect();
        out.extend(pages_samples::extra_artifacts(group_id));
        out.sort_by_key(|a| std::cmp::Reverse(a.updated_at));
        out
    }
    fn artifact_thumb(&self, artifact_id: &str) -> Option<pages::ArtifactThumb> {
        pages_samples::thumb(artifact_id)
    }
    fn can_record_decision(&self, _group_id: &str) -> bool {
        true
    }
    fn record_decision(&self, group_id: &str, text: String) -> AdminReply<()> {
        pages_samples::record(group_id, text, self.clock_now());
        self.emit(vec![Change::Topics {
            group_id: group_id.into(),
        }]);
        reply_now(Ok(()))
    }
    fn member_extras(&self, member_id: &str) -> pages::MemberExtras {
        pages_samples::member_extras(self, member_id)
    }
    fn retry_member_problem(&self, _member_id: &str, _run_id: &str) -> AdminReply<()> {
        reply_now(Ok(()))
    }
    fn artifact_responses(&self, artifact_id: &str) -> Vec<pages::ArtifactResponse> {
        pages_samples::responses(artifact_id)
    }
    fn artifact_page_events(&self, artifact_id: &str, _version: u32) -> Vec<String> {
        if artifact_id == ART_REPORT {
            vec!["choose".into()]
        } else {
            Vec::new()
        }
    }
    // --- r2-trust ---
    fn send_artifact_event(
        &self,
        artifact_id: &str,
        version: u32,
        name: &str,
        payload: Option<serde_json::Value>,
    ) {
        sent_page_events::record(artifact_id, version, name, payload);
    }
    // --- end r2-trust ---
    // --- inc-host track: the 产出 page host on the samples ---
    fn page_bridge(
        &self,
        artifact_id: &str,
        version: u32,
    ) -> Option<crate::data::page_host::PageBridge> {
        use crate::data::page_host::{PageEventDecl, PageEventKind};
        let mut b = crate::data::page_host::default_bridge(self, artifact_id, version)?;
        if artifact_id == ART_REPORT {
            // 10 月数据月报: Ledger made it in a run; the page filters and asks.
            b.runner = true;
            b.events.extend([
                PageEventDecl {
                    name: "filter".into(),
                    kind: PageEventKind::Emit,
                    label: Some("按渠道看".into()),
                },
                PageEventDecl {
                    name: "question".into(),
                    kind: PageEventKind::Ask,
                    label: Some("数据有问题".into()),
                },
            ]);
        }
        Some(b)
    }
    fn send_page_event(
        &self,
        artifact_id: &str,
        version: u32,
        name: &str,
        _kind: crate::data::page_host::PageEventKind,
        payload: Option<serde_json::Value>,
    ) -> Result<String, crate::data::page_host::PageSendError> {
        sent_page_events::record(artifact_id, version, name, payload);
        Ok(format!("ce-{}", sent_page_events::snapshot().len()))
    }
    fn fetch_page_data(
        &self,
        artifact_id: &str,
        _version: u32,
    ) -> futures::channel::oneshot::Receiver<Option<(serde_json::Value, u64)>> {
        let (tx, rx) = futures::channel::oneshot::channel();
        let data = (artifact_id == ART_REPORT).then(|| {
            (
                serde_json::json!({
                    "channel": "全部",
                    "weeks": ["10-06", "10-13", "10-20", "10-27"],
                    "values": [9, 8, 10, 11],
                    "total": 38,
                    "delta": "+4"
                }),
                1,
            )
        });
        let _ = tx.send(data);
        rx
    }
    fn file_poster(
        &self,
        _group_id: &str,
        _topic_id: &str,
        file: &FileAttachment,
    ) -> Option<std::path::PathBuf> {
        // The sample recording's poster: a frame of the bundled sample clip, made once.
        static POSTER: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
        if file.file_id != "f-v2-video" {
            return None;
        }
        POSTER
            .get_or_init(|| {
                let clip = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("src/views/artifacts/preview/clip.mp4");
                crate::views::media::poster::make(&clip, crate::views::media::MediaKind::Video)
            })
            .clone()
    }
    // --- end inc-host track ---
    fn open_artifact_version(
        &self,
        artifact_id: &str,
        version: Option<u32>,
    ) -> Reply<ArtifactFolder> {
        // The embed states of a card: a folder that does not open, and a file.
        if artifact_id == ART_EMBED_BROKEN {
            return reply_err(FileError::Network);
        }
        if artifact_id == ART_EMBED_FILE {
            let dir = std::env::temp_dir()
                .join("team-chat-v2-samples")
                .join(artifact_id);
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(dir.join("预算.pdf"), "%PDF-1.4\n%%EOF\n");
            let (tx, rx) = futures::channel::oneshot::channel();
            let _ = tx.send(Ok(ArtifactFolder {
                artifact_id: artifact_id.into(),
                version: version.unwrap_or(1),
                dir,
                entry: "预算.pdf".into(),
                is_page: false,
            }));
            return rx;
        }
        let Some(detail) = self.artifact(artifact_id) else {
            return reply_err(FileError::NotFound);
        };
        let version = version.unwrap_or(detail.summary.version);
        let (tx, rx) = futures::channel::oneshot::channel();
        let _ = tx.send(pages_samples::write_page(
            artifact_id,
            version,
            &detail.summary.title,
        ));
        rx
    }

    // --- end pages track ---

    // --- cards v2 track ---
    // The 首页 asks keep the desk track's cards of Main.dc.html (with the
    // message context of the matching cards v2 sample); the choice ask stays
    // a v1 card there. Every other card comes from the cards v2 samples.
    fn card_v2(&self, _group_id: &str, message_id: &str) -> Option<cards_v2::CardV2Message> {
        if message_id == super::v2_desk::choice_ask() {
            return None;
        }
        if let Some(card) = super::v2_desk::card_v2(message_id) {
            let mut msg = self.cards_v2.card(message_id)?;
            msg.card = card;
            return Some(msg);
        }
        self.cards_v2.card(message_id)
    }
    fn card_v2_answers(&self, _group_id: &str, message_id: &str) -> Vec<cards_v2::CardV2Answer> {
        if super::v2_desk::card_v2(message_id).is_some() {
            let desk = self.desk.lock().unwrap_or_else(|e| e.into_inner());
            return desk.answers.get(message_id).cloned().unwrap_or_default();
        }
        self.cards_v2.answers(message_id)
    }
    fn card_v2_pending(
        &self,
        _group_id: &str,
        message_id: &str,
    ) -> Option<cards_v2::CardV2Pending> {
        if super::v2_desk::card_v2(message_id).is_some() {
            return None;
        }
        self.cards_v2.pending(message_id)
    }
    fn answer_card_v2(
        &self,
        group_id: &str,
        topic_id: &str,
        message_id: &str,
        answer: cards_v2::CardV2AnswerRequest,
    ) {
        let now = crate::selftest::clock::utc_now().timestamp_millis();
        if super::v2_desk::card_v2(message_id).is_some() {
            self.desk
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .answer(message_id, answer, now);
            self.emit(vec![
                Change::Messages {
                    group_id: group_id.into(),
                    topic_id: topic_id.into(),
                },
                Change::Todo,
            ]);
            return;
        }
        if let Some((group_id, topic_id)) = self.cards_v2.answer(message_id, ME, answer, now) {
            self.emit(vec![Change::Messages { group_id, topic_id }, Change::Todo]);
        }
    }
    // --- inc-cards21: the interaction loop on the sample team ---
    fn send_card_event(
        &self,
        _group_id: &str,
        _topic_id: &str,
        message_id: &str,
        request: cards_v2::CardEventRequest,
    ) -> Option<String> {
        let now = crate::selftest::clock::now_ms();
        let (group_id, topic_id, id) = self.cards_v2.send_event(message_id, ME, request, now)?;
        self.emit(vec![Change::Messages { group_id, topic_id }, Change::Todo]);
        Some(id)
    }
    fn card_v2_events(&self, _group_id: &str, message_id: &str) -> Vec<cards_v2::MyCardEvent> {
        self.cards_v2.events(message_id)
    }
    fn drop_card_event(
        &self,
        _group_id: &str,
        _topic_id: &str,
        message_id: &str,
        client_event_id: &str,
    ) {
        if let Some((group_id, topic_id)) = self.cards_v2.drop_event(message_id, client_event_id) {
            self.emit(vec![Change::Messages { group_id, topic_id }]);
        }
    }
    // --- end inc-cards21 ---
    fn withdraw_card_v2_answer(&self, group_id: &str, topic_id: &str, message_id: &str) {
        if super::v2_desk::card_v2(message_id).is_some() {
            self.desk
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .withdraw(message_id);
            self.emit(vec![
                Change::Messages {
                    group_id: group_id.into(),
                    topic_id: topic_id.into(),
                },
                Change::Todo,
            ]);
            return;
        }
        if let Some((group_id, topic_id)) = self.cards_v2.withdraw(message_id, ME) {
            self.emit(vec![Change::Messages { group_id, topic_id }, Change::Todo]);
        }
    }
    // --- end cards v2 track ---

    // --- inc-refs track ---

    fn refs_supported(&self) -> bool {
        true
    }

    fn add_draft_ref(&self, key: &DraftKey, r: DraftRef) -> AddRef {
        let added = self.inner.add_draft_ref(key, r);
        if added == AddRef::Added {
            self.emit(vec![Change::Draft { key: key.clone() }]);
        }
        added
    }

    fn remove_draft_ref(&self, key: &DraftKey, index: usize) {
        self.inner.remove_draft_ref(key, index);
        self.emit(vec![Change::Draft { key: key.clone() }]);
    }

    fn clear_draft(&self, key: &DraftKey) {
        self.inner.clear_draft(key);
        self.emit(vec![Change::Draft { key: key.clone() }]);
    }

    fn message_block_refs(&self, message: &Message) -> Vec<chat_core::proto::BlockRef> {
        self.refs.of(message)
    }

    fn send_with_refs(
        &self,
        request: SendRequest,
        _key: &DraftKey,
        _files: Vec<PendingFileId>,
        refs: Vec<chat_core::proto::BlockRefInput>,
    ) -> ClientMsgId {
        refs_samples::send(self, request, refs)
    }

    // --- end inc-refs track ---

    fn subscribe(&self) -> UnboundedReceiver<Vec<Change>> {
        let (tx, rx) = unbounded();
        self.subscribers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(tx);
        rx
    }
}

// --- r2-trust ---
/// The page events (`artifact.event`) the sample provider was asked to send, in order, for
/// the page-event self-tests.
pub mod sent_page_events {
    use std::sync::Mutex;

    /// One sent event: 产出, version, name and payload.
    pub type Sent = (String, u32, String, Option<serde_json::Value>);

    static SENT: Mutex<Vec<Sent>> = Mutex::new(Vec::new());

    pub(super) fn record(
        artifact_id: &str,
        version: u32,
        name: &str,
        payload: Option<serde_json::Value>,
    ) {
        if let Ok(mut s) = SENT.lock() {
            s.push((artifact_id.into(), version, name.into(), payload));
        }
    }

    pub fn take() -> Vec<Sent> {
        SENT.lock()
            .map(|mut s| std::mem::take(&mut *s))
            .unwrap_or_default()
    }

    pub fn snapshot() -> Vec<Sent> {
        SENT.lock().map(|s| s.clone()).unwrap_or_default()
    }
}
// --- end r2-trust ---
