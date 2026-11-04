//! Sample Card v2.1 cards of CardsCompose.dc.html and Interact.dc.html for the v2 sample
//! team, with stored answers, server card state and my events (relative to the clock, so
//! the pending stages hold in the screenshot self-test). `v2::samples::SampleCards`
//! serves them next to the 2.0 samples.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::{
    data::{
        CardPressError, Millis,
        cards_v2::{
            CardEventAck, CardEventFailure, CardEventPhase, CardEventRequest, CardV2, CardV2Answer,
            CardV2Message, CardV2Pending, CardV2SendPhase, Cause, Expect, ExpectMode, MyCardEvent,
            WireCardState,
        },
    },
    fake::v2::{
        ALEX, ALL, ART_EMBED_BROKEN, ART_EMBED_FILE, ART_REPORT, CHENCHEN, DEV, GROWTH, JOJO,
        LEDGER, LIXUE, ME, OPS, POCKET, SCOUT, T_PROMO, T_RELEASE, T_REPORT, T_STAGING, T_V4,
        T_WEEKLY, TARO, TATA, WATCH, XIAOWANG, at,
    },
};

// Message ids.
pub const QUESTIONNAIRE: &str = "c21-questionnaire";
pub const COMPARE: &str = "c21-compare";
pub const GROUPS: &str = "c21-groups";
pub const HEALTH: &str = "c21-health";
pub const VOID: &str = "c21-void";
pub const SINGLE_OTHER: &str = "c21-single-other";
pub const MULTIPLE: &str = "c21-multiple";
pub const RANK: &str = "c21-rank";
pub const RATING: &str = "c21-rating";
pub const CONFIRM: &str = "c21-confirm";
pub const IMAGE: &str = "c21-image";
pub const FAILED: &str = "c21-failed";
pub const MATRIX: &str = "c21-matrix";
pub const SLOTS: &str = "c21-slots";
pub const POLL: &str = "c21-poll";
pub const EMBED: &str = "c21-embed";
/// The embed of a 产出 whose folder does not open.
pub const EMBED_BROKEN: &str = "c21-embed-broken";
/// The embed of a 产出 that is a file, not a web page.
pub const EMBED_FILE: &str = "c21-embed-file";
pub const UNKNOWN: &str = "c21-unknown";
pub const BUTTONS: &str = "c21-buttons";
pub const DIAGRAM: &str = "c21-diagram";
/// INTERACT 7.1 rev 1.
pub const REPORT: &str = "c21-report";
/// INTERACT 7.2: rev 1, my event waiting at three stages.
pub const PEND_WAIT: &str = "c21-pend-wait";
pub const PEND_SLOW: &str = "c21-pend-slow";
pub const PEND_HARD: &str = "c21-pend-hard";
/// INTERACT 7.2 rev 2 after my bar click.
pub const DRILL: &str = "c21-drill";
/// INTERACT 7.1 rev 3: my submit was refused, the card changed.
pub const CONFLICT: &str = "c21-conflict";
// v11/pb-cards: the notes of the loop (INTERACT §4.4, §6.2).
/// My bar click came back offline: the bar goes back.
pub const NOTE_OFFLINE: &str = "c21-note-offline";
/// My bar click came back `expect.mode: none`.
pub const NOTE_BUSY: &str = "c21-note-busy";
/// Rev 2 changed the block I typed in.
pub const NOTE_DRAFT: &str = "c21-note-draft";
/// Ledger answered my click with a new message off screen.
pub const NOTE_REPLY: &str = "c21-note-reply";

/// One sample: the card message, stored answers, my answer on its way and my events
/// (event times are ms before now).
pub struct Sample21 {
    pub msg: CardV2Message,
    pub answers: Vec<CardV2Answer>,
    pub pending: Option<CardV2Pending>,
    pub events: Vec<(Millis, MyCardEvent)>,
    /// The rev the viewer saw before (drill-down scenes).
    pub prev: Option<Arc<CardV2>>,
}

fn card(v: Value) -> CardV2 {
    serde_json::from_value(v).expect("sample 2.1 card parses")
}

fn sample(id: &str, group: &str, topic: &str, author: &str, created: Millis, v: Value) -> Sample21 {
    let c = card(v);
    Sample21 {
        msg: CardV2Message {
            group_id: group.into(),
            topic_id: topic.into(),
            message_id: id.into(),
            author_id: author.into(),
            created_at: created,
            edited_at: None,
            card: Arc::new(c),
            card_state: None,
            cause: None,
        },
        answers: Vec::new(),
        pending: None,
        events: Vec::new(),
        prev: None,
    }
}

fn answer(by: &str, rev: u32, action: &str, inputs: Value, at: Millis) -> CardV2Answer {
    CardV2Answer {
        actor_id: by.into(),
        rev,
        action_id: action.into(),
        inputs: serde_json::from_value(inputs).ok(),
        edited_values: None,
        at,
    }
}

/// An event of mine on `block`, acked `ago` ms before now with `mode`.
fn acked(
    id: &str,
    block: &str,
    event: &str,
    value: Value,
    mode: ExpectMode,
    author: &str,
) -> MyCardEvent {
    MyCardEvent {
        client_event_id: format!("ce-{id}"),
        request: CardEventRequest {
            rev: 1,
            block_id: Some(block.into()),
            event: event.into(),
            value: Some(value),
            ..Default::default()
        },
        durable: false,
        sent_at: 0,
        phase: CardEventPhase::Acked {
            ack: CardEventAck {
                event_id: format!("ev-{id}"),
                stored: false,
                answer_id: None,
                duplicate: None,
                expect: Expect {
                    by: author.into(),
                    mode,
                    eta_ms: Some(2000),
                },
            },
            at: 0,
        },
        dropped: false,
        reply: None,
    }
}

/// Moves an event to `now - ago`.
pub fn at_ago(e: &MyCardEvent, ago: Millis, now: Millis) -> MyCardEvent {
    let mut e = e.clone();
    e.sent_at = now - ago - 80;
    match &mut e.phase {
        CardEventPhase::Acked { at, .. }
        | CardEventPhase::Failed { at, .. }
        | CardEventPhase::Resolved { at, .. } => *at = now - ago,
        CardEventPhase::Sending => {}
    }
    e
}

fn base(title: &str, intent: &str, fallback: &str) -> Value {
    json!({ "v": 2, "catalog": "tc.card/2.1", "rev": 1, "intent": intent, "title": title,
            "fallback": fallback })
}

fn with(mut v: Value, extra: Value) -> Value {
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        for (k, x) in e {
            o.insert(k.clone(), x.clone());
        }
    }
    v
}

fn questionnaire() -> Value {
    with(
        base(
            "周宁下周一入职，这三件事要你定",
            "ask",
            "Pocket 问了 3 个问题：周宁下周一入职",
        ),
        json!({
            "summary": "周宁是新来的数据实习，跟着 Ledger。我按李雪入职时的设置先填好了，你改完一起交。",
            "ask": { "from": [ME], "expiresAt": at(12, 0) },
            "blocks": [
                { "id": "qs", "type": "questionnaire", "children": ["perm", "groups", "hello"], "submitLabel": "一起提交" },
                { "id": "perm", "type": "decision", "label": "给她开哪一级权限？", "required": true, "fallbackText": "权限",
                  "options": [
                    { "id": "same", "label": "和其他实习生一样：能看所有群，只在自己的话题里交代", "recommended": true },
                    { "id": "staging", "label": "再加 staging 只读，方便她看数据看板" },
                    { "id": "growth", "label": "先只开「夹夹增长」一个群" } ],
                  "help": "Pocket 推荐和其他实习生一样：李雪入职时也是这样，两周后再按需要加。" },
                { "id": "groups", "type": "input", "kind": "chips", "label": "要进的群", "required": true,
                  "options": [ { "id": "all", "label": "全员" }, { "id": "growth", "label": "夹夹增长" },
                               { "id": "dev", "label": "开发" }, { "id": "ops", "label": "运维" } ],
                  "default": ["all", "growth"] },
                { "id": "hello", "type": "input", "kind": "textarea", "label": "想对她说的第一句话", "required": false,
                  "placeholder": "Pocket 会放进第一天的欢迎消息里", "maxLength": 200 }
            ],
            "actions": [
                { "id": "send", "label": "一起提交", "style": "primary", "kind": "submit" },
                { "id": "later", "label": "先存着", "kind": "cancel" }
            ]
        }),
    )
}

fn compare(rev: u32) -> Value {
    with(
        base(
            "夹夹 4.0 首页用哪个方案？",
            "ask",
            "Tata 在等你选夹夹 4.0 首页的方案",
        ),
        json!({
            "rev": rev,
            "summary": "两个都做成了能点的原型。访谈了 6 位群主，5 位说在群里找照片最难。",
            "ask": { "from": [ME], "expiresAt": at(12, 0) },
            "blocks": [
                { "id": "cmp", "type": "compare", "fallbackText": "按活动整理 / 按时间排",
                  "options": [
                    { "id": "A", "label": "按活动整理", "recommended": true, "preview": { "fileId": "f-proto-a" },
                      "pros": ["找照片最快，5 位群主都提到"], "cons": ["每个活动要先归类，第一次打开要等几秒"] },
                    { "id": "B", "label": "按时间排", "preview": { "fileId": "f-proto-b" },
                      "pros": ["不用整理，做起来最快"], "cons": ["照片一多就难找，和 3.0 差别不大"] } ],
                  "note": { "label": "说说为什么，Tata 会写进需求（可选）", "maxLength": 300 } }
            ],
            "actions": [
                { "id": "ok", "label": "确定", "style": "primary", "kind": "submit" },
                { "id": "open", "label": "打开原型", "kind": "link", "target": { "kind": "url", "url": "https://example.com/proto" } }
            ]
        }),
    )
}

/// INTERACT 7.1 as Interact.dc.html 01 draws it: Ledger asks Joye, before the weekly
/// meeting, whether the data is right (an ask card: the amber band for Joye, 等 Joye for
/// everyone else), with the chart and the question side by side (2 : 1).
fn report_rev1() -> Value {
    with(
        base(
            "10 月日活 2,340，比 9 月涨了 4%",
            "ask",
            "Ledger 想在周会前确认 10 月数据：日活 2,340，比 9 月涨了 4%。这个数据对不对？",
        ),
        json!({
            "summary": "Ledger 想在周会前确认数据",
            "ask": { "id": "verify", "from": [ME], "expiresAt": at(14, 0),
                     "promise": "你确认之前，我不会把月报发到全员群。" },
            "blocks": [
                { "id": "basis", "type": "text", "text": "按去重设备算，用的是 Joye 11-01 定的口径。发到周会之前，想请你看一眼。" },
                { "id": "main", "type": "row", "weights": [2, 1], "children": ["trend", "check_col"] },
                { "id": "trend", "type": "chart", "kind": "line", "label": "日活 · 近 4 周", "emphasis": "high",
                  "x": ["10-06", "10-13", "10-20", "10-27"],
                  "series": [ { "id": "oct", "name": "10 月", "values": [2210, 2268, 2301, 2340] },
                              { "id": "sep", "name": "9 月", "values": [2150, 2190, 2236, 2250] } ],
                  "summary": "10 月日活从 2,210 涨到 2,340，每周都比 9 月高。", "events": ["chart.select"] },
                { "id": "check_col", "type": "stack", "children": ["check", "note"] },
                { "id": "check", "type": "decision", "label": "这个数据对不对？", "required": true, "fallbackText": "对 / 有问题",
                  "help": "有问题就写一句，Ledger 会先查再发。",
                  "options": [ { "id": "ok", "label": "对，可以发周会" }, { "id": "wrong", "label": "有问题" } ],
                  "deliver": "onSubmit", "events": ["choice.change"] },
                { "id": "note", "type": "input", "kind": "textarea", "label": "哪里不对？", "required": false, "maxLength": 300 }
            ],
            "actions": [ { "id": "send", "label": "发给 Ledger", "style": "primary", "kind": "submit", "inputs": ["check", "note"] } ]
        }),
    )
}

// Interact.dc.html 02 draws the drill-down bars as rows with no title; the pending and
// notes cards keep the chart label `新增群 · 按渠道`.
fn bars_block(labeled: bool) -> Value {
    let mut v = json!({ "id": "bars", "type": "chart", "kind": "bar",
            "x": ["小红书", "群内邀请", "B 站", "抖音"],
            "series": [ { "id": "groups", "name": "新增群", "values": [14, 10, 9, 5] } ],
            "summary": "小红书 14、群内邀请 10、B 站 9、抖音 5。",
            "events": ["chart.select", "chart.clear"], "emitBy": "members" });
    if labeled {
        v["label"] = json!("新增群 · 按渠道");
    }
    v
}

fn bars_rev1() -> Value {
    bars_rev1_with(true)
}

fn bars_rev1_with(labeled: bool) -> Value {
    with(
        base(
            "新增群从哪来",
            "info",
            "Ledger：10 月新增群 38 个，小红书最多（14 个）。",
        ),
        json!({
            "blocks": [
                bars_block(labeled),
                { "id": "hint", "type": "text", "text": "点一个渠道，Ledger 会拆开给你看。" }
            ]
        }),
    )
}

// Interact.dc.html 02 step 3: Ledger answers the click on B 站 with two blocks, the
// breakdown by source and a follow-up question.
fn bars_rev2() -> Value {
    with(
        base(
            "新增群从哪来",
            "info",
            "Ledger：10 月新增群 38 个，B 站 9 个。",
        ),
        json!({
            "rev": 2,
            "ask": { "id": "follow", "from": [ME] },
            "blocks": [
                bars_block(false),
                { "id": "detail", "type": "group", "title": "B 站 · 按来源", "collapsible": false, "children": ["d_posts"] },
                { "id": "d_posts", "type": "table", "columns": ["来源", "新增群", "比 9 月"],
                  "rows": [ ["夹夹教程视频", "5", "+1"], ["UP 主合作", "3", "−1"], ["直播间", "1", "−1"], ["合计", "9", "−1"] ] },
                { "id": "follow", "type": "decision", "label": "UP 主合作少了 1 个群。要我查一下是哪位 UP 主停更了吗？",
                  "fallbackText": "好，去查 / 先不用", "deliver": "immediate",
                  "options": [ { "id": "yes", "label": "好，去查" }, { "id": "no", "label": "先不用" } ] }
            ]
        }),
    )
}

fn conflict_rev3() -> Value {
    with(
        report_rev1(),
        json!({
            "rev": 3,
            "summary": "新增群 38 个；抖音 10-14 到 10-16 埋点缺了 3 天。",
            "blocks": [
                { "id": "kpis", "type": "row", "children": ["m_groups", "m_dau"] },
                { "id": "m_groups", "type": "metric", "label": "新增群", "value": 38, "delta": 12, "unit": "%", "better": "up", "note": "比 9 月" },
                { "id": "m_dau", "type": "metric", "label": "日活", "value": 2340, "delta": 4, "unit": "%", "better": "up", "note": "比 9 月" },
                { "id": "check", "type": "decision", "label": "这个数据对不对？", "required": true, "fallbackText": "对 / 有问题",
                  "options": [ { "id": "ok", "label": "对，可以发周会" }, { "id": "wrong", "label": "有问题" } ],
                  "deliver": "onSubmit", "events": ["choice.change"] },
                { "id": "what", "type": "input", "kind": "text", "label": "哪一项不对？", "required": false, "maxLength": 300,
                  "help": "抖音 10-14 到 10-16 埋点缺了 3 天，这 3 天我按 0 算的。" }
            ],
            "actions": [ { "id": "send", "label": "发给 Ledger", "style": "primary", "kind": "submit", "inputs": ["check", "what"] } ]
        }),
    )
}

/// Every 2.1 sample.
pub fn all() -> Vec<Sample21> {
    let mut out = Vec::new();

    // 01 组合
    out.push(sample(
        QUESTIONNAIRE,
        ALL,
        T_WEEKLY,
        POCKET,
        at(10, 5),
        questionnaire(),
    ));
    out.push(sample(COMPARE, DEV, T_V4, TATA, at(10, 22), compare(1)));
    out.push(sample(
        GROUPS,
        ALL,
        T_WEEKLY,
        JOJO,
        at(10, 40),
        with(
            base("本周周报，开发和运维写好了", "working", "Jojo 在写本周周报"),
            json!({
                "summary": "夹夹增长那部分等 Ledger 的数，补完一起发给你。",
                "blocks": [
                    { "id": "dev", "type": "group", "title": "开发", "summary": "3 个话题", "children": ["dev_list"] },
                    { "id": "dev_list", "type": "list", "items": [
                        { "text": "断线重连后消息重复 · 改好了，PR #61 等合并", "status": "done" },
                        { "text": "话题列表虚拟滚动 · Forge 在做 3/5", "status": "in_progress" },
                        { "text": "登录页支持邀请码 · 等 Joye 选输入框", "status": "todo" } ] },
                    { "id": "ops", "type": "group", "title": "运维", "summary": "v2.14 等发布 · 巡检 24 项都通过", "collapsed": true, "children": ["ops_text"] },
                    { "id": "ops_text", "type": "text", "text": "qqbot v2.14 已在 staging 跑满 24 小时。" },
                    { "id": "growth", "type": "group", "title": "夹夹增长", "summary": "Ledger 在补 10 月的数", "collapsed": true, "children": ["growth_text"] },
                    { "id": "growth_text", "type": "text", "text": "10 月新增群和日活的数今天下午补齐。" },
                    { "id": "plan", "type": "plan", "items": [
                        { "text": "开发和运维写好了", "state": "done" },
                        { "text": "Ledger 在补夹夹增长的数", "state": "current" },
                        { "text": "一起发给 Joye", "state": "pending" } ] }
                ],
                "actions": [ { "id": "first", "label": "先看写好的两部分", "kind": "press" } ]
            }),
        ),
    ));
    out.push(sample(
        HEALTH,
        OPS,
        T_RELEASE,
        WATCH,
        at(10, 42),
        with(
            base("staging 跑了 24 小时，可以发到生产吗？", "ask", "Watch 在等你批准 qqbot v2.14 上线"),
            json!({
                "summary": "发布前看一眼健康度",
                "ask": { "from": [ME], "expiresAt": at(20, 0),
                         "irreversible": { "verb": "发布", "target": "生产", "consequence": "所有群重启约 2 分钟" } },
                "blocks": [
                    { "id": "kpi", "type": "row", "children": ["err", "p95", "mem"] },
                    { "id": "err", "type": "metric", "label": "错误率", "value": 0.12, "unit": "%", "delta": -0.05, "better": "down" },
                    { "id": "p95", "type": "metric", "label": "回复耗时 P95", "value": 1.4, "unit": "秒", "delta": -0.2, "better": "down" },
                    { "id": "mem", "type": "metric", "label": "内存", "value": 318, "unit": "MB", "delta": 6, "note": "正常范围" },
                    { "id": "trend", "type": "chart", "kind": "area", "label": "错误率 · 每小时",
                      "x": ["11:00", "14:00", "17:00", "20:00", "23:00", "02:00", "05:00", "08:00", "10:00"],
                      "series": [ { "name": "错误率", "values": [0.14, 0.12, 0.11, 0.12, 0.15, 0.13, 0.16, 0.12, 0.12] } ] },
                    { "id": "facts", "type": "fields", "fields": [
                        { "label": "包含", "value": "群相册分享、撤回同步、告警降噪，共 3 个 PR" },
                        { "label": "验收", "value": "Scout 今天 09:10 在 staging 验收通过" },
                        { "label": "影响", "value": "所有群重启约 2 分钟；出问题可以一键回到 v2.13" } ] }
                ],
                "actions": [
                    { "id": "ship", "label": "发布到生产", "style": "primary", "kind": "submit" },
                    { "id": "hold", "label": "先不发", "kind": "reject" },
                    { "id": "report", "label": "验收报告", "kind": "link", "target": { "kind": "url", "url": "https://example.com/r" } }
                ]
            }),
        ),
    ));
    let mut void = sample(VOID, DEV, T_V4, TATA, at(9, 40), compare(1));
    void.msg.card_state = Some(Arc::new(WireCardState {
        ask: Some(chat_core::proto::CardStateAsk {
            id: "a1".into(),
            status: chat_core::proto::AskStatus::Superseded,
            from: vec![ME.into()],
            blocking: true,
            ..Default::default()
        }),
        ..Default::default()
    }));
    out.push(void);

    // 02 选择题
    out.push(sample(
        SINGLE_OTHER,
        OPS,
        T_STAGING,
        SCOUT,
        at(9, 14),
        with(
            base("staging 巡检没过时，先告诉谁？", "ask", "Scout 在问巡检没过时先告诉谁"),
            json!({
                "ask": { "from": [ME], "promise": "选「其他」时，写的字会一起发给 Scout" },
                "blocks": [
                    { "id": "who", "type": "decision", "fallbackText": "先告诉谁",
                      "options": [ { "id": "watch", "label": "Watch，它这周值班", "recommended": true },
                                   { "id": "wang", "label": "小王" },
                                   { "id": "ops", "label": "在「运维」群里提醒所有人" } ],
                      "allowOther": { "label": "其他", "placeholder": "写一个人或一个办法" },
                      "help": "Scout 推荐 Watch：它能马上回滚，别人要等它。" }
                ],
                "actions": [ { "id": "send", "label": "提交", "style": "primary", "kind": "submit", "inputs": ["who"] } ]
            }),
        ),
    ));
    out.push(sample(
        MULTIPLE,
        GROWTH,
        T_PROMO,
        TARO,
        at(10, 26),
        with(
            base("夹夹 3.0 这周发哪几个平台？", "ask", "Taro 在问夹夹 3.0 发哪几个平台"),
            json!({
                "ask": { "from": [ME], "promise": "每个平台 Taro 都会先发草稿给你" },
                "blocks": [
                    { "id": "where", "type": "decision", "multiple": true, "minSelect": 2, "maxSelect": 3, "fallbackText": "平台",
                      "options": [ { "id": "xhs", "label": "小红书", "detail": "草稿已经写好" },
                                   { "id": "bili", "label": "B 站", "detail": "10 月新增群最多" },
                                   { "id": "douyin", "label": "抖音" },
                                   { "id": "wx", "label": "微信视频号" },
                                   { "id": "jike", "label": "即刻" } ] }
                ],
                "actions": [ { "id": "send", "label": "提交", "style": "primary", "kind": "submit", "inputs": ["where"] } ]
            }),
        ),
    ));
    out.push(sample(
        RANK,
        DEV,
        T_V4,
        TATA,
        at(10, 15),
        with(
            base("夹夹 4.0 先做哪几件？排个顺序", "ask", "Tata 在问夹夹 4.0 的顺序"),
            json!({
                "ask": { "from": [ME], "promise": "只排前 3 个也行" },
                "blocks": [
                    { "id": "order", "type": "rank", "label": "排个顺序", "fallbackText": "排序",
                      "items": [ { "id": "photos", "label": "按活动整理照片", "meta": "约 2 周" },
                                 { "id": "share", "label": "群相册分享到小红书", "meta": "约 4 天" },
                                 { "id": "birthday", "label": "生日提醒", "meta": "约 3 天" },
                                 { "id": "night", "label": "夜间消息汇总", "meta": "约 1 周" },
                                 { "id": "weekly", "label": "群主周报", "meta": "约 1 周" } ],
                      "cut": { "after": 3, "label": "前 3 个进 4.0" } }
                ],
                "actions": [ { "id": "send", "label": "按这个顺序", "style": "primary", "kind": "submit", "inputs": ["order"] } ]
            }),
        ),
    ));
    out.push(sample(
        RATING,
        ALL,
        T_WEEKLY,
        POCKET,
        at(10, 35),
        with(
            base("实习第一周，帮我打两个分", "ask", "Pocket 请你给实习第一周打分"),
            json!({
                "ask": { "from": [ME] },
                "blocks": [
                    { "id": "qs", "type": "questionnaire", "children": ["clear", "nps"], "anonymous": true },
                    { "id": "clear", "type": "rating", "label": "交代给成员的事，结果清楚吗？", "max": 5, "fallbackText": "评分",
                      "labels": { "1": "很乱", "4": "清楚", "5": "很清楚" } },
                    { "id": "nps", "type": "scale", "label": "你会推荐朋友来这样的团队实习吗？", "min": 0, "max": 10,
                      "ends": ["不会", "一定会"], "fallbackText": "量表" }
                ],
                "actions": [ { "id": "send", "label": "提交", "style": "primary", "kind": "submit" } ]
            }),
        ),
    ));
    out.push(sample(
        CONFIRM,
        OPS,
        T_RELEASE,
        WATCH,
        at(10, 31),
        with(
            base("夜里 0–7 点只发一条汇总，可以吗？", "ask", "Watch 问夜里只发一条汇总可以吗"),
            json!({
                "ask": { "from": [ME] },
                "blocks": [
                    { "id": "ok", "type": "confirm", "label": "夜里只发一条汇总", "yes": "可以", "no": "不行",
                      "detail": "生产宕机这类紧急告警照常马上提醒", "fallbackText": "可以 / 不行" }
                ]
            }),
        ),
    ));
    out.push(sample(
        IMAGE,
        GROWTH,
        T_PROMO,
        TARO,
        at(10, 24),
        with(
            base("小红书封面用哪张？", "ask", "Taro 在问小红书封面用哪张"),
            json!({
                "ask": { "from": [ME], "promise": "选好后草稿里的封面会换掉" },
                "blocks": [
                    { "id": "cover", "type": "imageChoice", "label": "封面", "fallbackText": "封面",
                      "options": [ { "id": "A", "label": "合照拼图", "fileId": "f-cover-a" },
                                   { "id": "B", "label": "大字标题", "fileId": "f-cover-b", "recommended": true },
                                   { "id": "C", "label": "手机截图", "fileId": "f-cover-c" } ],
                      "help": "Taro 推荐大字标题：上一条大字封面的点击率高了 40%。" }
                ],
                "actions": [ { "id": "use", "label": "用这张", "style": "primary", "kind": "submit", "inputs": ["cover"] } ]
            }),
        ),
    ));
    let mut failed = sample(
        FAILED,
        OPS,
        T_STAGING,
        SCOUT,
        at(9, 20),
        with(
            base("staging 巡检改到每天 08:30？", "ask", "Scout 问巡检时间"),
            json!({
                "ask": { "from": [ME] },
                "blocks": [
                    { "id": "time", "type": "confirm", "label": "改到每天 08:30", "yes": "可以", "no": "不行", "fallbackText": "可以 / 不行" }
                ]
            }),
        ),
    );
    failed.pending = Some(CardV2Pending {
        rev: 1,
        action_id: "time".into(),
        inputs: serde_json::from_value(json!({ "time": { "value": true } })).ok(),
        edited_values: None,
        phase: CardV2SendPhase::Failed(CardPressError::Other),
        at: at(9, 21),
    });
    out.push(failed);
    out.push(sample(
        MATRIX,
        DEV,
        T_V4,
        TATA,
        at(10, 18),
        with(
            base("这四件事各放进哪个版本？", "ask", "Tata 问四件事放进哪个版本"),
            json!({
                "ask": { "from": [ME], "promise": "最多 8 行 × 5 列，再多就做成产出" },
                "blocks": [
                    { "id": "plan", "type": "matrix", "label": "放进哪个版本", "fallbackText": "矩阵",
                      "rows": [ { "id": "photos", "label": "按活动整理照片", "required": true },
                                { "id": "birthday", "label": "生日提醒" },
                                { "id": "weekly", "label": "群主周报" },
                                { "id": "night", "label": "夜间消息汇总" } ],
                      "columns": [ { "id": "v40", "label": "4.0" }, { "id": "v41", "label": "4.1" }, { "id": "later", "label": "先不做" } ] }
                ],
                "actions": [ { "id": "send", "label": "提交", "style": "primary", "kind": "submit", "inputs": ["plan"] } ]
            }),
        ),
    ));
    let day = 24 * 3600 * 1000;
    let slot = |id: &str, d: i64, h: u32, conflicts: Value| json!({ "id": id, "start": at(h, 0) + d * day, "end": at(h, 0) + d * day + 3600 * 1000, "conflicts": conflicts });
    out.push(sample(
        SLOTS,
        ALL,
        T_WEEKLY,
        POCKET,
        at(10, 12),
        with(
            base("下周周会定在哪个时间？", "ask", "Pocket 问下周周会的时间"),
            json!({
                "summary": "我看了每个人的日历。三个时间都能来，周二 14:00 离周一的计划会最近。",
                "ask": { "from": [ME], "promise": "定了以后 Pocket 发日历邀请" },
                "blocks": [
                    { "id": "when", "type": "slots", "label": "选一个时间", "tz": "Asia/Shanghai", "fallbackText": "时间段",
                      "slots": [
                        slot("t1", 7, 10, json!([ALEX])), slot("t2", 8, 10, json!([ALEX, LIXUE])), slot("t3", 9, 10, json!([])),
                        slot("t4", 7, 14, json!([])), slot("t5", 8, 14, json!([LIXUE])), slot("t6", 9, 14, json!([ALEX, LIXUE])),
                        slot("t7", 7, 16, json!([XIAOWANG])), slot("t8", 8, 16, json!([])), slot("t9", 9, 16, json!([ALEX, LIXUE, CHENCHEN])) ] }
                ],
                "actions": [ { "id": "send", "label": "定这个时间", "style": "primary", "kind": "submit", "inputs": ["when"] } ]
            }),
        ),
    ));
    let mut poll = sample(
        POLL,
        ALL,
        T_WEEKLY,
        JOJO,
        at(10, 8),
        with(
            base(
                "qqbot 下个版本先做哪个？每人一票",
                "ask",
                "Jojo 发起投票：qqbot 下个版本先做哪个",
            ),
            json!({
                "ask": { "from": [ME, XIAOWANG, LIXUE, ALEX, CHENCHEN] },
                "blocks": [
                    { "id": "vote", "type": "poll", "label": "先做哪个", "fallbackText": "投票",
                      "options": [ { "id": "A", "label": "群相册按人物整理" }, { "id": "B", "label": "定时提醒" }, { "id": "C", "label": "入群欢迎卡片" } ],
                      "voters": [ME, XIAOWANG, LIXUE, ALEX, CHENCHEN], "closesAt": at(18, 0), "public": true, "changeable": true }
                ]
            }),
        ),
    );
    poll.answers = vec![
        answer(
            XIAOWANG,
            1,
            "vote",
            json!({ "vote": { "value": "A" } }),
            at(9, 30),
        ),
        answer(
            ME,
            1,
            "vote",
            json!({ "vote": { "value": "B" } }),
            at(9, 40),
        ),
        answer(
            LIXUE,
            1,
            "vote",
            json!({ "vote": { "value": "B" } }),
            at(9, 50),
        ),
    ];
    out.push(poll);

    // 03 扩展方式
    out.push(sample(
        EMBED,
        GROWTH,
        T_REPORT,
        LEDGER,
        at(10, 38),
        with(
            base("11 月的 2 万块预算怎么分？", "ask", "Ledger 做了一个预算分配页面，请在 1.1 里打开。"),
            json!({
                "summary": "拖动滑块，页面会按 10 月的数估新增群。分好后按页面里的「发给 Ledger」。",
                "ask": { "from": [ME] },
                "blocks": [
                    { "id": "page", "type": "embed", "artifactId": ART_REPORT, "version": 3, "height": 280,
                      "title": "渠道预算分配", "events": ["budget.submit"],
                      "fallbackText": "Ledger 做了一个预算分配页面，请在新版本里打开。" },
                    { "id": "budget", "type": "input", "kind": "slider", "label": "小红书", "min": 0, "max": 20000, "step": 1000, "unit": "元", "default": 6000 }
                ]
            }),
        ),
    ));
    for (id, artifact) in [
        (EMBED_BROKEN, ART_EMBED_BROKEN),
        (EMBED_FILE, ART_EMBED_FILE),
    ] {
        out.push(sample(
            id,
            GROWTH,
            T_REPORT,
            LEDGER,
            at(10, 38),
            with(
                base("11 月的 2 万块预算怎么分？", "ask", "Ledger 做了一个预算分配页面，请在 1.1 里打开。"),
                json!({
                    "summary": "拖动滑块，页面会按 10 月的数估新增群。分好后按页面里的「发给 Ledger」。",
                    "ask": { "from": [ME] },
                    "blocks": [
                        { "id": "page", "type": "embed", "artifactId": artifact, "version": 1, "height": 280,
                          "title": "渠道预算分配", "events": ["budget.submit"],
                          "fallbackText": "Ledger 做了一个预算分配页面，请在新版本里打开。" }
                    ]
                }),
            ),
        ));
    }
    out.push(sample(
        UNKNOWN,
        ALL,
        T_WEEKLY,
        POCKET,
        at(10, 44),
        with(
            base("周五团建订在这里", "done", "Pocket 订好了周五团建"),
            json!({
                "catalog": "tc.card/2.1",
                "blocks": [
                    { "id": "map", "type": "map", "lat": 39.99, "lng": 116.48,
                      "fallbackText": "望京 SOHO T3 12 层。地铁 15 号线望京东站 A 口，步行 6 分钟。" },
                    { "id": "facts", "type": "fields", "fields": [ { "label": "时间", "value": "周五 18:30" },
                                                             { "label": "人数", "value": "9 人，Alex Chen 远程" } ] }
                ]
            }),
        ),
    ));
    out.push(sample(
        BUTTONS,
        GROWTH,
        T_REPORT,
        LEDGER,
        at(10, 30),
        with(
            base("10 月新增群 · 按周看", "info", "Ledger：10 月新增群按周看"),
            json!({
                "blocks": [
                    { "id": "tabs", "type": "tabs", "events": ["tab.open"], "tabs": [
                        { "id": "all", "label": "全部", "children": ["t_all"] },
                        { "id": "xhs", "label": "小红书", "children": ["t_xhs"] },
                        { "id": "bili", "label": "B 站", "children": ["t_bili"] } ] },
                    { "id": "t_all", "type": "chart", "kind": "bar", "label": "全部 · 每周新增群", "x": ["10-06", "10-13", "10-20", "10-27"],
                      "series": [ { "name": "新增群", "values": [9, 8, 10, 11] } ] },
                    { "id": "t_xhs", "type": "chart", "kind": "bar", "label": "小红书 · 每周", "x": ["10-06", "10-13", "10-20", "10-27"],
                      "series": [ { "name": "新增群", "values": [2, 3, 4, 5] } ] },
                    { "id": "t_bili", "type": "chart", "kind": "bar", "label": "B 站 · 每周", "x": ["10-06", "10-13", "10-20", "10-27"],
                      "series": [ { "name": "新增群", "values": [3, 2, 2, 2] } ] },
                    { "id": "more", "type": "buttons", "events": ["button.press"], "fallbackText": "按渠道拆开 / 换成按月看",
                      "buttons": [ { "id": "split", "label": "按渠道拆开" }, { "id": "month", "label": "换成按月看" },
                                   { "id": "reset", "label": "清掉这个月的数", "style": "danger",
                                     "confirm": { "title": "确定清掉？" } } ] }
                ]
            }),
        ),
    ));
    out.push(sample(
        DIAGRAM,
        DEV,
        T_V4,
        TATA,
        at(10, 20),
        with(
            base("4.0 首页打开的流程", "info", "Tata 画了 4.0 首页打开的流程"),
            json!({
                "blocks": [
                    { "id": "flow", "type": "diagram", "syntax": "mermaid", "title": "首页打开",
                      "source": "flowchart LR\n  open[打开首页] --> cache{有缓存?}\n  cache -->|有| show[先显示缓存]\n  cache -->|没有| load[加载照片]\n  show --> load\n  load --> done[按活动分组]",
                      "caption": "有缓存时先显示缓存，再在后台加载。", "fallbackText": "打开首页 → 有缓存先显示 → 加载照片 → 按活动分组" }
                ]
            }),
        ),
    ));

    // Interact.dc.html
    out.push(sample(
        REPORT,
        GROWTH,
        T_REPORT,
        LEDGER,
        at(9, 40),
        report_rev1(),
    ));
    for (id, ago) in [(PEND_WAIT, 2_000), (PEND_SLOW, 14_000), (PEND_HARD, 52_000)] {
        let mut s = sample(id, GROWTH, T_REPORT, LEDGER, at(9, 40), bars_rev1());
        s.events.push((
            ago,
            acked(
                id,
                "bars",
                "chart.select",
                json!({ "series": "groups", "x": "B 站", "y": 9, "index": 2 }),
                ExpectMode::Live,
                LEDGER,
            ),
        ));
        out.push(s);
    }
    let mut drill = sample(DRILL, GROWTH, T_REPORT, LEDGER, at(9, 40), bars_rev2());
    drill.msg.edited_at = Some(at(10, 21));
    drill.msg.cause = Some(Cause {
        event_id: "ev-drill".into(),
        actor_id: ME.into(),
        at: at(10, 21) as u64,
        event: "chart.select".into(),
        block_id: Some("bars".into()),
        action_id: None,
    });
    drill.prev = Some(Arc::new(card(bars_rev1_with(false))));
    // My click on B 站 that caused rev 2 (its value names the bar).
    let mut click = acked(
        DRILL,
        "bars",
        "chart.select",
        json!({ "series": "groups", "x": "B 站", "y": 9, "index": 2 }),
        ExpectMode::Live,
        LEDGER,
    );
    click.phase = CardEventPhase::Resolved {
        result: crate::data::cards_v2::InteractionResult::Patched,
        note: None,
        at: 0,
    };
    drill.events.push((1_000, click));
    out.push(drill);
    let mut conflict = sample(
        CONFLICT,
        GROWTH,
        T_REPORT,
        LEDGER,
        at(9, 40),
        conflict_rev3(),
    );
    conflict.msg.edited_at = Some(at(10, 20));
    conflict.events.push((
        1_000,
        MyCardEvent {
            client_event_id: "ce-conflict".into(),
            request: CardEventRequest {
                rev: 1,
                action_id: Some("send".into()),
                event: "action.press".into(),
                inputs: serde_json::from_value(json!({ "check": { "value": "wrong" } })).ok(),
                ..Default::default()
            },
            durable: false,
            sent_at: 0,
            phase: CardEventPhase::Failed {
                failure: CardEventFailure::Conflict,
                at: 0,
            },
            dropped: false,
            reply: None,
        },
    ));
    conflict.prev = Some(Arc::new(card(report_rev1())));
    out.push(conflict);
    // v11/pb-cards: the notes of the loop.
    let click = json!({ "series": "groups", "x": "B 站", "y": 9, "index": 2 });
    for (id, mode) in [
        (NOTE_OFFLINE, ExpectMode::Offline),
        (NOTE_BUSY, ExpectMode::None),
    ] {
        let mut s = sample(id, GROWTH, T_REPORT, LEDGER, at(9, 40), bars_rev1());
        s.events.push((
            1_000,
            acked(id, "bars", "chart.select", click.clone(), mode, LEDGER),
        ));
        out.push(s);
    }
    let mut draft = sample(
        NOTE_DRAFT,
        GROWTH,
        T_REPORT,
        LEDGER,
        at(9, 40),
        retro(2, "B 站停更了两周，新增群少了 1 个"),
    );
    draft.msg.edited_at = Some(at(10, 2));
    draft.prev = Some(Arc::new(card(retro(1, "B 站和上月差不多"))));
    out.push(draft);
    let mut reply = sample(NOTE_REPLY, GROWTH, T_REPORT, LEDGER, at(9, 40), bars_rev1());
    let mut e = acked(
        NOTE_REPLY,
        "bars",
        "chart.select",
        click,
        ExpectMode::Live,
        LEDGER,
    );
    e.phase = CardEventPhase::Resolved {
        result: crate::data::cards_v2::InteractionResult::Patched,
        note: None,
        at: 0,
    };
    e.reply = Some("c21-note-reply-answer".into());
    reply.events.push((2_000, e));
    out.push(reply);
    out
}

/// A weekly review with Ledger's note and my view (rev `rev` changes the note).
fn retro(rev: u32, note: &str) -> Value {
    with(
        base("10 月复盘", "info", "Ledger：10 月复盘"),
        json!({
            "rev": rev,
            "blocks": [
                { "id": "why", "type": "text", "text": note },
                { "id": "view", "type": "input", "kind": "text", "label": "你的看法",
                  "default": note, "events": ["input.change"] }
            ]
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_parse_and_known_ones_are_valid() {
        for s in all() {
            let c = &s.msg.card;
            if s.msg.message_id == UNKNOWN {
                assert!(
                    c.blocks
                        .iter()
                        .any(|b| matches!(b, crate::data::cards_v2::CardV2Block::Unknown))
                );
                continue;
            }
            assert!(
                c.validate().is_ok(),
                "{}: {:?}",
                s.msg.message_id,
                c.validate()
            );
            assert!(super::super::is_21(c), "{} is a 2.1 card", s.msg.message_id);
        }
    }
}

/// The rev the viewer saw before, for a sample that changed (the drill-down scenes).
pub fn prev_of(message_id: &str) -> Option<Arc<CardV2>> {
    all()
        .into_iter()
        .find(|s| s.msg.message_id == message_id)
        .and_then(|s| s.prev)
}
