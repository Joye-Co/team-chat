//! The v2 sample team (1.1.0 artboards, CONCEPT.md §4): Joye, four interns
//! and the eight bots with their characters, the 群 开发 / 运维 / 夹夹增长 /
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
use crate::data::{v2::*, *};

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
/// 运维 / qqbot v2.14 上线: Watch asks me to approve the release.
pub const T_RELEASE: &str = "t-v2-release";
/// 运维 / staging 每日巡检: Scout, done 09:12.
pub const T_STAGING: &str = "t-v2-staging";
/// 夹夹增长 / 夹夹 3.0 宣传: Taro's draft waits on me.
pub const T_PROMO: &str = "t-v2-promo";
/// 夹夹增长 / 10 月数据月报: Ledger, done 09:40.
pub const T_REPORT: &str = "t-v2-report";
/// 夹夹增长 / 夹夹 4.0 需求: Tata writes the second draft (2/4).
pub const T_V4: &str = "t-v2-v4";
/// 全员 / 本周周报: Jojo split it to Pocket and Ledger (1/3).
pub const T_WEEKLY: &str = "t-v2-weekly";

pub const RUN_RECONNECT: &str = "run-v2-reconnect";
pub const RUN_VIRTUAL: &str = "run-v2-virtual";
pub const RUN_V4: &str = "run-v2-v4";
pub const RUN_WEEKLY: &str = "run-v2-weekly";

pub const ART_RECONNECT: &str = "art-v2-reconnect-report";
pub const ART_REPORT: &str = "art-v2-oct-report";
pub const ART_V4: &str = "art-v2-v4-draft";

const MIN: i64 = 60_000;

/// Today at `hh:mm` local time (the artboards' day runs 09:00 to 10:45).
pub fn at(hour: u32, minute: u32) -> Millis {
    days_ago_at(0, hour, minute)
}

fn days_ago_at(days: i64, hour: u32, minute: u32) -> Millis {
    let date = Local::now().date_naive() - chrono::Duration::days(days);
    let t = date.and_hms_opt(hour, minute, 0).unwrap_or_default();
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
    paused_by: Option<MemberId>,
    // topic track (v11/topic): who approved PR #61 and when.
    approval: Option<(MemberId, Millis)>,
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

    /// The sample team over `inner` (for its session, drafts and settings).
    pub fn wrap(inner: Arc<FakeProvider>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            members: members(),
            data: Mutex::new(Self::data()),
            subscribers: Mutex::new(Vec::new()),
            quiet: false,
            desk: Mutex::default(),
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
                &[ME, JOJO, WATCH, SCOUT, ALEX],
                at(10, 42),
            ),
            group(
                GROWTH,
                GroupKind::Group,
                "夹夹增长",
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
                last: at(10, 38),
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
                title: "qqbot v2.14 上线",
                status: S::WaitingAnswer,
                waiting_on: &[ME],
                owner: WATCH,
                by: JOJO,
                created: at(9, 20),
                last: at(10, 42),
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
                title: "夹夹 3.0 宣传",
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
                title: "夹夹 4.0 需求",
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
            5,
            FORGE,
            at(9, 55),
            "**改好了：重连后不再重复**\n\n原因是客户端重连后从旧的 seq 重新拉取，服务器把已经收到的消息又推了一遍。现在按 seq 去重，重连时从最后一个确认过的 seq 继续；重连间隔也改成了指数退避。",
        );
        done.artifact_refs.push(ArtifactRef {
            artifact_id: ART_RECONNECT.into(),
            version: 2,
        });
        messages.insert(
            t.into(),
            vec![
                video,
                message(DEV, t, 2, JOJO, at(9, 13), "转给 @Forge。"),
                message(DEV, t, 3, FORGE, at(9, 14), "收到，先复现，大约 30 分钟。"),
                message(DEV, t, 4, ME, at(9, 31), "改成指数退避，放进同一个 PR。"),
                done,
                message(DEV, t, 6, XIAOWANG, at(10, 20), "本地试了，没再出现重复。"),
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
        // 运维 / qqbot v2.14 上线: the approval.
        let t = T_RELEASE;
        let mut approval = message(
            OPS,
            t,
            2,
            WATCH,
            at(10, 42),
            "把 qqbot v2.14 发布到生产吗？",
        );
        approval.id = format!("{t}-ask");
        cards.insert(
            approval.id.clone(),
            card(serde_json::json!({
                "v": 1,
                "title": "把 qqbot v2.14 发布到生产吗？",
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
                    "@Watch 准备 qqbot v2.14：#412 群相册分享、#415 撤回同步、#418 告警降噪。",
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
        // 夹夹增长 / 夹夹 3.0 宣传: the draft.
        let t = T_PROMO;
        let mut draft = message(
            GROWTH,
            t,
            2,
            TARO,
            at(10, 20),
            "这条小红书确认后，周四 20:00 发出。",
        );
        draft.id = format!("{t}-ask");
        cards.insert(
            draft.id.clone(),
            card(serde_json::json!({
                "v": 1,
                "title": "这条小红书确认后，周四 20:00 发出",
                "blocks": [
                    { "type": "fields", "fields": [
                        { "label": "标题", "value": "夹夹 3.0｜群里的照片，一键分享回群" }
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
                    "@Taro 写一条 3.0 的小红书，主打群相册。",
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
                    "@Tata 整理夹夹 4.0 的需求，参考上周的群反馈。",
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
                    "夹夹 4.0 需求（第二版）",
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
                at: at(10, 31),
                refs: Vec::new(),
                suggestion: None,
                streaming: false,
            },
            SideChatMessage {
                id: "sc-2".into(),
                author_id: JOJO.into(),
                // panes track: the 单独问 artboard's answer.
                body: "风险不大，有三处值得留意：\n1. 去重靠客户端记住的 seq。缓存清空后会从头拉一次，不会重复，只是首次打开慢一点。\n2. 断网久了，重试间隔会拉长到 30 秒，网恢复后最多等半分钟才连上。\n3. 旧版客户端不带 seq，服务器照旧推送。\n\nForge 的测试覆盖了 6 个场景，没覆盖多端同时在线。".into(),
                at: at(10, 31),
                refs: vec![
                    "reconnect.ts seq 去重".into(),
                    "指数退避上限 30 秒".into(),
                    "旧客户端兼容".into(),
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
            paused_by: None,
            approval: None,
        }
    }

    fn with<R>(&self, f: impl FnOnce(&mut Data) -> R) -> R {
        f(&mut self.data.lock().unwrap_or_else(|e| e.into_inner()))
    }

    fn emit(&self, changes: Vec<Change>) {
        let mut subs = self.subscribers.lock().unwrap_or_else(|e| e.into_inner());
        subs.retain(|tx| tx.unbounded_send(changes.clone()).is_ok());
    }

    /// Takes over the live run in a scene (`live-takeover`).
    pub fn set_paused_by(&self, member: Option<MemberId>) {
        self.with(|d| d.paused_by = member);
        self.emit(vec![Change::Runs {
            group_id: DEV.into(),
            topic_id: T_VIRTUAL.into(),
        }]);
    }
}

impl ChatProvider for V2Samples {
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
                chrono::Utc::now().timestamp_millis(),
                &request.body,
            );
            m.client_msg_id = format!("v2-c{seq}");
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
    fn start_topic(&self, request: StartTopicRequest) -> ClientMsgId {
        self.inner.start_topic(request)
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
    fn group_muted(&self, _group_id: &str) -> bool {
        false
    }
    // --- rail track: I follow every 话题 of the 群 I am in, so the rail's
    // 我参与的 filter lists what the artboards show.
    fn topic_following(&self, group_id: &str, _topic_id: &str) -> bool {
        self.with(|d| {
            d.groups
                .iter()
                .any(|g| g.id == group_id && g.member_ids.iter().any(|m| m == ME))
        })
    }
    // --- end rail track
    fn runs(&self, _group_id: &str, topic_id: &str) -> Vec<BotRunSummary> {
        let mut runs: Vec<BotRunSummary> = self.with(|d| {
            d.runs
                .iter()
                .filter(|r| r.topic_id == topic_id)
                .filter(|r| !(self.quiet && r.status.is_active()))
                .cloned()
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
            at: at(10, 43),
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
                "把 qqbot v2.14 发布到生产吗？",
                at(10, 42),
            ),
            ask(
                DEV,
                T_RECONNECT,
                Some(format!("{T_RECONNECT}-m5")),
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
                "这条小红书确认后，周四 20:00 发出",
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
        vec![
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
                at: Some(days_ago_at(-1, 8, 0)),
            },
            QueuedItem {
                member_id: SCOUT.into(),
                title: "巡检 staging".into(),
                at: Some(days_ago_at(-1, 9, 0)),
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
                cost_cents: Some(620),
                input_tokens: None,
                output_tokens: None,
            }),
            RUN_VIRTUAL => Some(RunUsage {
                duration_ms: 20 * 60_000,
                cost_cents: Some(310),
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
        match group_id {
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
                    "qqbot 发布前必须 Scout 在 staging 验收",
                    days_ago_at(6, 10, 0),
                ),
            ],
            _ => Vec::new(),
        }
    }
    fn group_purpose(&self, group_id: &str) -> Option<String> {
        match group_id {
            DEV => Some("qqbot、夹夹和 TeamChat 的开发".into()),
            OPS => Some("qqbot 和夹夹的上线、巡检和告警".into()),
            GROWTH => Some("夹夹的增长、数据和社媒".into()),
            ALL => Some("全员的周报和通知".into()),
            _ => None,
        }
    }
    fn side_chat_default_member(&self) -> Option<MemberId> {
        Some(JOJO.into())
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
    fn send_side_chat(&self, group_id: &str, topic_id: &str, _member_id: &str, body: String) {
        self.with(|d| {
            let n = d.side_chat.len() + 1;
            d.side_chat.push(SideChatMessage {
                id: format!("sc-{n}"),
                author_id: ME.into(),
                body,
                at: chrono::Utc::now().timestamp_millis(),
                refs: Vec::new(),
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
            s.posted_at = Some(chrono::Utc::now().timestamp_millis());
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
            paused_by: self.with(|d| d.paused_by.clone()),
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
            },
            AcceptanceItem {
                text: "内存不涨".into(),
                met: Some(true),
                evidence: Some("现在 212 MB".into()),
            },
            AcceptanceItem {
                text: "跳到第 8,000 条后位置不跳".into(),
                met: None,
                evidence: None,
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
        self.set_paused_by(Some(ME.into()));
        reply_now(Ok(()))
    }
    fn hand_back_run(&self, _group_id: &str, _topic_id: &str, _run_id: &str) -> AdminReply<()> {
        self.set_paused_by(None);
        reply_now(Ok(()))
    }
    fn member_stats(&self, member_id: &str) -> Option<MemberStats> {
        let mut stats = MemberStats::default();
        for r in self.with(|d| d.runs.clone()) {
            if r.bot_id == member_id && r.status.is_active() {
                stats.active.push((r.group_id.clone(), r.topic_id.clone()));
            }
        }
        if member_id == FORGE {
            stats.queued = 1;
            stats.topics_this_week = 6;
            stats.done_this_week = 6;
            stats.artifacts_this_week = 4;
            stats.cost_cents_this_week = Some(13_200);
            stats.reports_to = Some(TATA.into());
            stats.needs_approval = vec![
                "合并到 main".into(),
                "发布到生产".into(),
                "改生产环境配置".into(),
                "单次花费超过 ¥50".into(),
            ];
            stats.tools = vec!["GitHub".into(), "Cloudflare 只读".into(), "staging".into()];
            stats.machine = Some(MemberMachine {
                name: "Mac mini".into(),
                online: true,
                busy: true,
                disk_used_pct: Some(61),
            });
            stats.problems = vec![MemberProblem {
                text: "CI 超时".into(),
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
                    (GROWTH.into(), T_V4.into(), "夹夹 4.0 需求".into()),
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

    fn card_v2(&self, _group_id: &str, message_id: &str) -> Option<Arc<chat_core::proto::CardV2>> {
        super::v2_desk::card_v2(message_id)
    }
    fn card_v2_answers(&self, _group_id: &str, message_id: &str) -> Vec<CardAnswer> {
        let desk = self.desk.lock().unwrap_or_else(|e| e.into_inner());
        desk.answers.get(message_id).cloned().unwrap_or_default()
    }
    fn answer_card_v2(
        &self,
        group_id: &str,
        topic_id: &str,
        message_id: &str,
        _rev: u32,
        action_id: &str,
        inputs: std::collections::BTreeMap<String, serde_json::Value>,
    ) {
        let now = Local::now().timestamp_millis();
        self.desk
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .answer(message_id, action_id, inputs, now);
        self.emit(vec![Change::Messages {
            group_id: group_id.into(),
            topic_id: topic_id.into(),
        }]);
    }
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
        let now = Local::now().timestamp_millis();
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
        if message_id != format!("{T_RECONNECT}-m5") {
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
        self.with(|d| d.approval = Some((ME.into(), chrono::Utc::now().timestamp_millis())));
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

    fn subscribe(&self) -> UnboundedReceiver<Vec<Change>> {
        let (tx, rx) = unbounded();
        self.subscribers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(tx);
        rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sample_team_is_consistent() {
        let p = V2Samples::new();
        let members = p.members();
        assert_eq!(members.iter().filter(|m| m.is_bot()).count(), 8);
        assert!(
            members
                .iter()
                .filter(|m| m.is_bot())
                .all(|m| m.character.is_some())
        );
        // Every ask, result, run and 产出 points at a known 话题 and member.
        let topic_ok = |g: &str, t: &str| p.topic(g, t).is_some();
        let member_ok = |id: &str| p.member(id).is_some();
        for a in p.asks_for_me() {
            assert!(topic_ok(&a.group_id, &a.topic_id), "{a:?}");
            assert!(member_ok(&a.from));
            if let Some(m) = &a.message_id {
                let msgs = p.messages(&a.group_id, &a.topic_id, 50).messages;
                assert!(msgs.iter().any(|x| &x.id == m), "{m}");
            }
        }
        for d in p.done_today() {
            assert!(topic_ok(&d.group_id, &d.topic_id));
        }
        for r in p.active_runs() {
            assert!(member_ok(&r.bot_id));
            assert!(!r.plan.is_empty());
        }
        assert_eq!(p.active_runs().len(), 3);
        for (g, a) in p.all_artifacts() {
            assert!(topic_ok(&g, &a.topic_id));
        }
        // The four asks of the desk: 需要你拍板 4.
        assert_eq!(p.asks_for_me().len(), 4);
        assert_eq!(p.todo().len(), 4);
        assert!(p.card(OPS, &format!("{T_RELEASE}-ask")).is_some());
        assert_eq!(p.side_chat_default_member().as_deref(), Some(JOJO));
        let cost = p.topic_cost(DEV, T_RECONNECT).unwrap();
        assert_eq!(cost.cost_cents, Some(620));
        assert_eq!(cost.duration_ms, 41 * 60_000);
    }

    #[test]
    fn the_side_chat_suggestion_posts_once() {
        let p = V2Samples::new();
        let before = p.messages(DEV, T_RECONNECT, 50).messages.len();
        p.post_side_chat_suggestion(DEV, T_RECONNECT, JOJO, "sc-2");
        p.post_side_chat_suggestion(DEV, T_RECONNECT, JOJO, "sc-2");
        assert_eq!(p.messages(DEV, T_RECONNECT, 50).messages.len(), before + 1);
        let thread = p.side_chat(DEV, T_RECONNECT, JOJO);
        assert!(
            thread.messages[1]
                .suggestion
                .as_ref()
                .unwrap()
                .posted_at
                .is_some()
        );
    }
}
