//! Sample v2 cards of Cards.dc.html for the v2 sample team
//! (`fake::v2::V2Samples`): every block type in every state, with stored
//! answers and answers on their way. The sample provider answers
//! `card_v2*` from a [`SampleCards`], and the asks of the 首页 point at the
//! same cards.

use std::{collections::HashMap, sync::Mutex};

use serde_json::json;

use crate::{
    data::{
        CardPressError, Millis,
        cards_v2::{
            CardV2, CardV2Answer, CardV2AnswerRequest, CardV2Message, CardV2Pending,
            CardV2SendPhase,
        },
    },
    fake::v2::{
        ALL, ART_REPORT, DEV, FORGE, GROWTH, JOJO, LEDGER, ME, OPS, POCKET, RUN_RECONNECT,
        RUN_RELEASE, RUN_VIRTUAL, SCOUT, T_INVITE, T_PROMO, T_RECONNECT, T_RELEASE, T_REPORT,
        T_STAGING, T_V4, T_VIRTUAL, T_WEEKLY, TARO, WATCH, at,
    },
};

// Message ids of the sample cards.
pub const CHOICE: &str = "cv2-choice";
pub const CHOICE_PICKED: &str = "cv2-choice-picked";
pub const CHOICE_MANY: &str = "cv2-choice-many";
/// A choice with 8 options, one long (COMPLETENESS.md §4: more than 6).
pub const CHOICE_LONG: &str = "cv2-choice-long";
pub const APPROVE: &str = "cv2-approve";
pub const SHIPPING: &str = "cv2-shipping";
pub const SHIPPED: &str = "cv2-shipped";
pub const DRAFT: &str = "cv2-draft";
pub const DRAFT_CONFIRMED: &str = "cv2-draft-confirmed";
pub const DRAFT_VOID: &str = "cv2-draft-void";
pub const FORM: &str = "cv2-form";
pub const PLAN: &str = "cv2-plan";
pub const PLAN_WAITS: &str = "cv2-plan-waits";
pub const PLAN_DONE: &str = "cv2-plan-done";
pub const METRICS: &str = "cv2-metrics";
pub const TABLE: &str = "cv2-table";
pub const METRICS_OLD: &str = "cv2-metrics-old";
pub const DIFF: &str = "cv2-diff";
pub const PR_LIST: &str = "cv2-pr-list";
pub const PR_BARE: &str = "cv2-pr-bare";
pub const EVIDENCE: &str = "cv2-evidence";
pub const EVIDENCE_NONE: &str = "cv2-evidence-none";
pub const EVIDENCE_CHIPS: &str = "cv2-evidence-chips";
pub const REFS: &str = "cv2-refs";
pub const REFS_STALE: &str = "cv2-refs-stale";
pub const FAILED: &str = "cv2-failed";
pub const EXPIRED: &str = "cv2-expired";
pub const SENDING: &str = "cv2-sending";
pub const PAGE: &str = "cv2-page";
/// A run's done card around a web 产出 (the server's `doneCard`: result text, acceptance
/// items, then the 产出 reference), as Jojo posted it for an article 解读.
pub const DONE_PAGE: &str = "cv2-done-page";
/// The message of the 轻回应 example (sample data).
pub const ACK_BODY: &str = "本地试了，没再出现重复。";

/// The 首页 asks of the sample team point at these messages; they get the
/// matching v2 card.
fn alias(message_id: &str) -> Option<&'static str> {
    Some(match message_id {
        m if m == format!("{T_RELEASE}-ask") => APPROVE,
        m if m == format!("{T_INVITE}-ask") => CHOICE,
        m if m == format!("{T_PROMO}-ask") => DRAFT,
        m if m == format!("{T_RECONNECT}-m6") => DIFF,
        _ => return None,
    })
}

struct Sample {
    msg: CardV2Message,
    answers: Vec<CardV2Answer>,
    pending: Option<CardV2Pending>,
    // --- inc-cards21: my events (absolute times) ---
    events: Vec<crate::data::cards_v2::MyCardEvent>,
}

fn card(v: serde_json::Value) -> CardV2 {
    serde_json::from_value(v).expect("sample v2 card parses")
}

// --- worst-case card data (break-ui, F65): `--worst-case` reaches every block, not
// only the title and the text ---

/// A long option, field value, ref or step label with no break for 30 characters.
const WORST_LONG: &str = "后海划船，再去旁边的胡同里找一家能坐下二十个人的小馆子吃晚饭，Aleksandra Wiśniewska-Kowalczyk 订位";
/// A right-to-left name in a mixed line.
const WORST_RTL: &str = "نور الهدى عبد الرحمن · 审核";
/// A file name that only the middle may give way in.
const WORST_FILE: &str = "IMG_20250914_183022_HDR_portrait_edited_edited_final_v3.HEIC";
/// A long address in a field.
const WORST_EMAIL: &str = "bartholomew.fitzgerald@northwind-industries-holdings.example.com";

/// Card JSON with worst-case data in every block: long and right-to-left options
/// (and more than 6 of them), long field values and labels, long table names and
/// 7-digit numbers, long refs (12 in a ref list), long file names, a 2,000-character
/// draft, long plan steps and huge metrics. Ids and the structure stay valid.
pub(crate) fn worsen(v: &mut serde_json::Value) {
    use serde_json::Value;
    let draft: String = "改过的内容会整体替换原稿。".repeat(160);
    fn walk(v: &mut Value, draft: &str) {
        match v {
            Value::Array(items) => {
                // A list of ref blocks grows to 12.
                let refs: Vec<Value> = items
                    .iter()
                    .filter(|b| b.get("type").and_then(Value::as_str) == Some("ref"))
                    .cloned()
                    .collect();
                if let Some(first) = refs.first().filter(|_| refs.len() >= 2) {
                    for n in refs.len()..12 {
                        let mut extra = first.clone();
                        if let Some(id) = extra.get("id").and_then(Value::as_str) {
                            extra["id"] = Value::from(format!("{id}-w{n}"));
                        }
                        extra["label"] = Value::from(format!("#{} {WORST_LONG}", 600 + n));
                        items.push(extra);
                    }
                }
                for item in items.iter_mut() {
                    walk(item, draft);
                }
            }
            Value::Object(o) => {
                let kind = o.get("type").and_then(Value::as_str).map(str::to_string);
                match kind.as_deref() {
                    Some("decision") => {
                        if let Some(Value::Array(opts)) = o.get_mut("options") {
                            if let Some(x) = opts.get_mut(1) {
                                x["label"] = Value::from(WORST_LONG);
                            }
                            if let Some(x) = opts.get_mut(2) {
                                x["label"] = Value::from(WORST_RTL);
                            }
                            for n in opts.len()..7 {
                                opts.push(serde_json::json!({
                                    "id": format!("w{n}"),
                                    // The count leads, so 7 long options stay apart.
                                    "label": format!("{n} · {WORST_RTL}"),
                                    "detail": "1,284 人"
                                }));
                            }
                        }
                    }
                    Some("fields") => {
                        if let Some(Value::Array(fields)) = o.get_mut("fields") {
                            for (i, f) in fields.iter_mut().enumerate() {
                                f["value"] =
                                    Value::from(if i % 2 == 0 { WORST_LONG } else { WORST_EMAIL });
                            }
                        }
                    }
                    Some("table") => {
                        if let Some(Value::Array(rows)) = o.get_mut("rows") {
                            for (i, r) in rows.iter_mut().enumerate() {
                                if let Value::Array(cells) = r {
                                    if i == 0
                                        && let Some(c) = cells.first_mut()
                                    {
                                        *c = Value::from(WORST_LONG);
                                    }
                                    if let Some(c) = cells.get_mut(1)
                                        && c.as_str().is_some_and(|t| {
                                            t.chars().all(|ch| ch.is_ascii_digit())
                                        })
                                    {
                                        *c = Value::from("1,284,000");
                                    }
                                }
                            }
                        }
                    }
                    Some("ref") => {
                        o.insert("label".into(), Value::from(WORST_LONG));
                    }
                    Some("image") => {
                        o.insert("alt".into(), Value::from(WORST_FILE));
                    }
                    Some("draft") => {
                        o.insert("text".into(), Value::from(draft));
                    }
                    Some("metric") => {
                        o.insert("value".into(), Value::from(1_284_567));
                        o.insert("label".into(), Value::from(WORST_LONG));
                    }
                    Some("input") => {
                        o.insert("label".into(), Value::from(WORST_LONG));
                    }
                    _ => {}
                }
                if kind.as_deref() == Some("plan")
                    && let Some(Value::Array(items)) = o.get_mut("items")
                {
                    // The worst steps of the runs (C-07): a card's plan reads like the
                    // inspector's plan of the same run.
                    for (j, it) in items.iter_mut().enumerate() {
                        if let Some(t) = crate::selftest::worst_case::worst_plan_step(j) {
                            it["text"] = Value::from(t);
                        }
                    }
                }
                for (_, x) in o.iter_mut() {
                    walk(x, draft);
                }
            }
            _ => {}
        }
    }
    walk(v, &draft);
}

/// The sample's JSON, with worst-case data under `--worst-case`.
pub(crate) fn sample_json(mut v: serde_json::Value) -> serde_json::Value {
    if crate::selftest::worst_case() {
        worsen(&mut v);
    }
    v
}

fn sample(
    id: &str,
    group: &str,
    topic: &str,
    author: &str,
    created: Millis,
    edited: Option<Millis>,
    v: serde_json::Value,
) -> Sample {
    let c = card(sample_json(v));
    debug_assert!(
        c.validate().is_ok(),
        "sample {id} is valid: {:?}",
        c.validate()
    );
    Sample {
        msg: CardV2Message {
            group_id: group.into(),
            topic_id: topic.into(),
            message_id: id.into(),
            author_id: author.into(),
            created_at: created,
            edited_at: edited,
            card: std::sync::Arc::new(c),
            card_state: None,
            cause: None,
        },
        answers: Vec::new(),
        pending: None,
        events: Vec::new(),
    }
}

fn answer(by: &str, rev: u32, action: &str, inputs: serde_json::Value, at: Millis) -> CardV2Answer {
    CardV2Answer {
        actor_id: by.into(),
        rev,
        action_id: action.into(),
        inputs: serde_json::from_value(inputs)
            .ok()
            .filter(|m: &crate::data::cards_v2::AnswerInputs| !m.is_empty()),
        edited_values: None,
        at,
    }
}

fn choice_card(rev: u32) -> serde_json::Value {
    json!({
        "v": 2, "rev": rev, "intent": "ask", "title": "邀请码输入框用哪种？",
        "fallback": "Forge 在等你选邀请码输入框的样式",
        "ask": { "from": [ME] },
        "blocks": [
            { "type": "decision", "id": "style", "default": ["one"], "options": [
                { "id": "one", "label": "一个输入框，粘贴自动识别" },
                { "id": "eight", "label": "8 个分格，逐位输入" }
            ]},
            { "type": "text", "id": "why", "text": "Forge 推荐一个输入框：实习生大多从微信复制邀请码。" }
        ],
        "actions": [{ "id": "pick", "label": "选这个", "style": "primary" }]
    })
}

fn approve_card() -> serde_json::Value {
    json!({
        "v": 2, "rev": 1, "intent": "ask", "title": "把 orderbot v2.14 发布到生产吗？",
        "fallback": "3 个 PR，Scout 09:10 已在 staging 验收通过。",
        "ask": { "from": [ME], "irreversible": {
            "verb": "发布", "target": "orderbot v2.14 到生产",
            "consequence": "所有群会重启约 2 分钟。出问题可以一键回到 v2.13。",
            "typeToConfirm": "orderbot-prod"
        }},
        "blocks": [{ "type": "fields", "id": "what", "fields": [
            { "label": "包含", "value": "群相册分享、撤回同步、告警降噪，共 3 个 PR" },
            { "label": "验收", "value": "Scout 今天 09:10 在 staging 验收通过" },
            { "label": "影响", "value": "所有群重启约 2 分钟；出问题可以一键回到 v2.13" }
        ]}],
        "actions": [
            { "id": "ship", "label": "发布到生产", "style": "primary" },
            { "id": "later", "label": "先不发", "kind": "cancel" },
            { "id": "report", "label": "验收报告", "kind": "link",
              "target": { "kind": "url", "url": "https://staging.example.com/qa/v2.14" } }
        ]
    })
}

fn draft_blocks() -> serde_json::Value {
    json!([{ "type": "group", "id": "post", "blocks": [
        { "type": "image", "id": "img1", "fileId": "f-promo-1" },
        { "type": "image", "id": "img2", "fileId": "f-promo-2" },
        { "type": "image", "id": "img3", "fileId": "f-promo-3" },
        { "type": "draft", "id": "title", "label": "标题", "editable": true,
          "text": "拾光 3.0｜群里的照片，一键分享回群" },
        { "type": "draft", "id": "body", "label": "正文", "editable": true,
          "text": "以前在群里找一张合照要翻半天。3.0 把群里的照片按活动自动整理好，点一下就能分享回群。" }
    ]}])
}

fn draft_card(rev: u32, title: &str) -> serde_json::Value {
    json!({
        "v": 2, "rev": rev, "intent": "ask", "title": title,
        "fallback": "Taro 的微博草稿等你确认",
        "ask": { "from": [ME], "irreversible": {
            "verb": "发布", "target": "这条微博", "consequence": "发出后不能撤回。"
        }},
        "blocks": draft_blocks(),
        "actions": [
            { "id": "publish", "label": "确认发布", "style": "primary" },
            { "id": "drop", "label": "不发了", "kind": "cancel" }
        ],
        "data": { "limits": { "title": 20, "body": 1000 } }
    })
}

fn form_card() -> serde_json::Value {
    json!({
        "v": 2, "rev": 1, "intent": "ask", "title": "新实习生入职，要开哪些权限？",
        "fallback": "Pocket 开账号要用",
        "ask": { "from": [ME] },
        "blocks": [
            { "type": "input", "id": "name", "kind": "text", "label": "名字", "required": true,
              "placeholder": "真实姓名", "maxLength": 20 },
            { "type": "input", "id": "role", "kind": "select", "label": "岗位", "required": true,
              "placeholder": "选岗位", "options": [
                { "id": "fe", "label": "前端实习" }, { "id": "data", "label": "数据实习" },
                { "id": "design", "label": "设计实习" }
            ]},
            { "type": "input", "id": "start", "kind": "date", "label": "入职日期", "required": true },
            { "type": "input", "id": "groups", "kind": "chips", "label": "要进的群", "options": [
                { "id": "dev", "label": "开发" }, { "id": "ops", "label": "运维" },
                { "id": "growth", "label": "拾光增长" }, { "id": "all", "label": "全员" }
            ], "default": ["all"] }
        ],
        "actions": [{ "id": "submit", "label": "提交", "style": "primary" }]
    })
}

fn plan_items(current: &str) -> serde_json::Value {
    json!([
        { "text": "读现有列表实现", "state": "done" },
        { "text": "换成虚拟列表", "state": "done" },
        { "text": "跑一万条消息的压测", "state": current },
        { "text": "修滚动锚点", "state": "pending" },
        { "text": "提 PR", "state": "pending" }
    ])
}

fn all() -> Vec<Sample> {
    let mut out = Vec::new();

    // 01 选择
    out.push(sample(
        CHOICE,
        DEV,
        T_INVITE,
        FORGE,
        at(9, 58),
        None,
        choice_card(1),
    ));
    let mut picked = sample(
        CHOICE_PICKED,
        DEV,
        T_INVITE,
        FORGE,
        at(9, 58),
        None,
        choice_card(1),
    );
    picked.answers.push(answer(
        ME,
        1,
        "pick",
        json!({ "style": "eight" }),
        at(10, 16),
    ));
    out.push(picked);
    out.push(sample(
        CHOICE_MANY,
        ALL,
        T_WEEKLY,
        POCKET,
        at(10, 12),
        None,
        json!({
            "v": 2, "rev": 1, "intent": "ask", "title": "周会挪到哪天？",
            "fallback": "Pocket 在等你选周会的时间",
            "ask": { "from": [ME] },
            "blocks": [
                { "type": "decision", "id": "day", "default": ["tue"], "options": [
                    { "id": "tue", "label": "周二 15:00" },
                    { "id": "wed", "label": "周三 15:00" },
                    { "id": "thu", "label": "周四 10:00" }
                ]},
                { "type": "text", "id": "why", "text": "Pocket 推荐周二：人最齐。" },
                { "type": "input", "id": "note", "kind": "text", "label": "补充一句",
                  "placeholder": "补充一句（可选）" }
            ],
            "actions": [{ "id": "submit", "label": "提交", "style": "primary" }]
        }),
    ));

    out.push(sample(
        CHOICE_LONG,
        ALL,
        T_WEEKLY,
        POCKET,
        at(10, 13),
        None,
        json!({
            "v": 2, "rev": 1, "intent": "ask", "title": "这周的团建去哪儿？",
            "fallback": "Pocket 在等你选团建的地方",
            "ask": { "from": [ME] },
            "blocks": [
                { "type": "decision", "id": "place", "default": ["park"], "options": [
                    { "id": "park", "label": "奥森公园野餐", "detail": "人均 60" },
                    { "id": "bowl", "label": "保龄球", "detail": "人均 90" },
                    { "id": "escape", "label": "密室逃脱", "detail": "人均 120" },
                    { "id": "hotpot", "label": "火锅", "detail": "人均 110" },
                    { "id": "museum", "label": "国家博物馆", "detail": "免费" },
                    { "id": "climb", "label": "室内攀岩", "detail": "人均 150" },
                    { "id": "boat", "label": "后海划船，再去旁边的胡同里找一家能坐下二十个人的小馆子吃晚饭，吃完沿着什刹海走回地铁站", "detail": "人均 80" },
                    { "id": "office", "label": "就在公司点外卖" }
                ]}
            ],
            "actions": [{ "id": "submit", "label": "提交", "style": "primary" }]
        }),
    ));

    // 02 批准
    out.push(sample(
        APPROVE,
        OPS,
        T_RELEASE,
        WATCH,
        at(10, 14),
        None,
        approve_card(),
    ));
    let mut shipping = sample(
        SHIPPING,
        OPS,
        T_RELEASE,
        WATCH,
        at(10, 14),
        Some(at(10, 17)),
        json!({
            "v": 2, "rev": 2, "intent": "working", "title": "正在发布 orderbot v2.14",
            "fallback": "正在发布 orderbot v2.14",
            "blocks": [
                { "type": "plan", "id": "steps", "runId": RUN_RELEASE, "items": [
                    { "text": "构建镜像", "state": "done" },
                    { "text": "逐个重启各群", "state": "current" },
                    { "text": "盯 15 分钟告警", "state": "pending" }
                ]},
                { "type": "progress", "id": "restart", "value": 0.375, "label": "已重启 9/24" }
            ]
        }),
    );
    shipping
        .answers
        .push(answer(ME, 1, "ship", json!({}), at(10, 16)));
    out.push(shipping);
    let mut shipped = sample(
        SHIPPED,
        OPS,
        T_RELEASE,
        WATCH,
        at(10, 14),
        Some(at(10, 19)),
        json!({
            "v": 2, "rev": 3, "intent": "done", "title": "已发布 orderbot v2.14",
            "fallback": "10:19 上线",
            "blocks": [
                { "type": "evidence", "id": "online", "label": "24/24 个群在线", "state": "pass" },
                { "type": "evidence", "id": "alerts", "label": "新告警 0", "state": "pass" }
            ],
            "actions": [{ "id": "rollback", "label": "回滚到 v2.13",
              "confirm": { "title": "回滚到 v2.13？", "body": "回滚约 2 分钟，随时可以" } }]
        }),
    );
    shipped
        .answers
        .push(answer(ME, 1, "ship", json!({}), at(10, 16)));
    out.push(shipped);

    // 03 草稿
    out.push(sample(
        DRAFT,
        GROWTH,
        T_PROMO,
        TARO,
        at(10, 10),
        None,
        draft_card(1, "这条微博确认后，周四 20:00 发出"),
    ));
    let mut confirmed = sample(
        DRAFT_CONFIRMED,
        GROWTH,
        T_PROMO,
        TARO,
        at(10, 10),
        Some(at(10, 18)),
        draft_card(1, "周四 20:00 发出这条微博"),
    );
    let mut a = answer(ME, 1, "publish", json!({}), at(10, 18));
    a.edited_values = Some(
        [
            ("title".to_string(), "拾光 3.0｜群里的照片一键分享回群".to_string()),
            (
                "body".to_string(),
                "在群里找一张合照，以前要翻半天。3.0 把群里的照片按活动自动整理好，点一下就能分享回群。周四晚上 8 点，群里见。".to_string(),
            ),
        ]
        .into_iter()
        .collect(),
    );
    confirmed.answers.push(a);
    out.push(confirmed);
    let mut void = sample(
        DRAFT_VOID,
        GROWTH,
        T_PROMO,
        TARO,
        at(10, 10),
        Some(at(10, 16)),
        draft_card(2, "这条微博确认后，周四 20:00 发出"),
    );
    void.pending = Some(CardV2Pending {
        rev: 1,
        action_id: "publish".into(),
        inputs: None,
        edited_values: None,
        phase: CardV2SendPhase::Failed(CardPressError::Gone),
        at: at(10, 15),
    });
    out.push(void);

    // 04 表单
    out.push(sample(
        FORM,
        ALL,
        T_WEEKLY,
        POCKET,
        at(10, 12),
        None,
        form_card(),
    ));

    // 05 计划
    out.push(sample(
        PLAN,
        DEV,
        T_VIRTUAL,
        FORGE,
        at(10, 2),
        Some(at(10, 21)),
        json!({
            "v": 2, "rev": 4, "intent": "working", "title": "话题列表虚拟滚动",
            "fallback": "Forge 在做：跑一万条消息的压测",
            "blocks": [{ "type": "plan", "id": "plan", "runId": RUN_VIRTUAL, "items": plan_items("current") }]
        }),
    ));
    out.push(sample(
        PLAN_WAITS,
        DEV,
        T_INVITE,
        FORGE,
        at(9, 30),
        Some(at(9, 58)),
        json!({
            "v": 2, "rev": 3, "intent": "working", "title": "登录页支持邀请码",
            "fallback": "等 Joye 拍板",
            "ask": { "from": [ME] },
            "blocks": [{ "type": "plan", "id": "plan", "items": [
                { "text": "读现有登录流程", "state": "done" },
                { "text": "加邀请码校验接口", "state": "done" },
                { "text": "定输入框样式", "state": "waiting" },
                { "text": "做输入框", "state": "pending" },
                { "text": "提 PR", "state": "pending" }
            ]}]
        }),
    ));
    out.push(sample(
        PLAN_DONE,
        DEV,
        T_RECONNECT,
        FORGE,
        at(9, 14),
        Some(at(9, 55)),
        // One plan source (C-07): the steps and times of Forge's run RUN_RECONNECT, which
        // the 话题 inspector shows.
        json!({
            "v": 2, "rev": 6, "intent": "done", "title": "断线重连后消息重复",
            "fallback": "5 步都做完了",
            "blocks": [{ "type": "plan", "id": "plan", "runId": RUN_RECONNECT, "items": [
                { "text": "复现重复消息", "state": "done" },
                { "text": "找到重复的原因", "state": "done" },
                { "text": "按 seq 去重并从确认处续传", "state": "done" },
                { "text": "改成指数退避", "state": "done" },
                { "text": "提 PR #61", "state": "done" }
            ]}]
        }),
    ));

    // 06 数据
    out.push(sample(
        METRICS,
        GROWTH,
        T_REPORT,
        LEDGER,
        at(9, 40),
        None,
        json!({
            "v": 2, "rev": 1, "intent": "done", "title": "10 月的三个数",
            "fallback": "10 月数据月报 v3",
            "blocks": [
                { "type": "metric", "id": "groups", "label": "新增群", "value": 38, "delta": 12,
                  "better": "up", "note": "% 比 9 月" },
                { "type": "metric", "id": "dau", "label": "日活", "value": 2340, "delta": 4,
                  "better": "up", "note": "% 比 9 月" },
                { "type": "metric", "id": "reply", "label": "平均回复", "value": 1.2, "unit": "秒",
                  "delta": -0.3, "better": "down", "note": "秒" },
                { "type": "chart", "id": "trend", "kind": "line", "label": "日活 · 近 4 周",
                  "x": ["10-06", "10-13", "10-20", "10-27"],
                  "series": [
                    { "name": "10 月", "values": [1900, 2060, 2210, 2340] },
                    { "name": "9 月", "values": [1640, 1760, 1680, 1820] }
                  ]}
            ]
        }),
    ));
    out.push(sample(
        TABLE,
        GROWTH,
        T_REPORT,
        LEDGER,
        at(9, 40),
        None,
        json!({
            "v": 2, "rev": 1, "intent": "done", "title": "新增群从哪来",
            "fallback": "按渠道拆开",
            "blocks": [{ "type": "table", "id": "by", "columns": ["渠道", "新增群", "占比", "比 9 月"], "rows": [
                ["微博", "14", "37%", "+6"], ["群内邀请", "10", "26%", "+1"],
                ["B 站", "9", "24%", "−1"], ["抖音", "5", "13%", "−2"], ["合计", "38", "100%", "+4"]
            ]}],
            "actions": [{ "id": "full", "label": "在产出里看完整月报", "kind": "link",
              "target": { "kind": "artifact", "artifactId": ART_REPORT, "version": 3 } }]
        }),
    ));
    out.push(sample(
        METRICS_OLD,
        GROWTH,
        T_REPORT,
        LEDGER,
        at(9, 40),
        None,
        json!({
            "v": 2, "rev": 1, "intent": "done", "title": "留存还没有数",
            "fallback": "留存还没有数",
            "blocks": [
                { "type": "metric", "id": "groups", "label": "新增群", "value": 38, "delta": 12,
                  "better": "up", "note": "% 比 9 月" },
                { "type": "metric", "id": "dau", "label": "日活", "value": 2340, "note": "截至 10-31" },
                { "type": "text", "id": "why", "text": "留存的埋点 11-05 才上线，下个月的月报再补。" }
            ]
        }),
    ));

    // 07 对比
    out.push(sample(
        DIFF,
        DEV,
        T_RECONNECT,
        FORGE,
        at(9, 55),
        None,
        json!({
            "v": 2, "rev": 1, "intent": "ask", "title": "PR #61 重连后按 seq 去重",
            "fallback": "Forge 请你审 · 12 个文件",
            "ask": { "from": [ME] },
            "blocks": [
                { "type": "evidence", "id": "checks", "label": "3 项检查通过", "state": "pass" },
                { "type": "evidence", "id": "lines", "label": "+214 −63" },
                { "type": "text", "id": "why", "text": "原因是客户端重连后从旧的 seq 重新拉取，服务器把已经收到的消息又推了一遍。现在按 seq 去重，重连时从最后一个确认过的 seq 继续；重连间隔也改成了指数退避。" },
                { "type": "diff", "id": "d1", "path": "src/sync/reconnect.ts", "additions": 5, "deletions": 2,
                  "text": "@@ -42,7 +42,10 @@ export async function resume(conn: Conn)\n-  const from = conn.lastSeq;\n+  const from = conn.lastAckedSeq;\n   const batch = await fetchSince(from);\n-  for (const msg of batch) store.push(msg);\n+  for (const msg of batch) {\n+    if (store.has(msg.seq)) continue;\n+    store.push(msg);\n+  }\n   conn.markResumed(from);" }
            ],
            "actions": [
                { "id": "approve", "label": "通过", "style": "primary" },
                { "id": "changes", "label": "要改", "kind": "cancel" },
                { "id": "github", "label": "在 GitHub 打开", "kind": "link",
                  "target": { "kind": "pr", "url": "https://github.com/example/orderbot/pull/61", "number": 61 } }
            ]
        }),
    ));
    out.push(sample(
        PR_LIST,
        DEV,
        T_RECONNECT,
        FORGE,
        at(10, 10),
        None,
        json!({
            "v": 2, "rev": 1, "intent": "info", "title": "这周的 PR",
            "fallback": "这周的 PR",
            "blocks": [
                { "type": "ref", "id": "p61", "label": "重连后按 seq 去重", "target": { "kind": "pr",
                  "url": "https://github.com/example/orderbot/pull/61", "number": 61, "repo": "开发 · Forge · 3/3 项检查通过", "state": "等你审" } },
                { "type": "ref", "id": "p62", "label": "话题列表虚拟滚动", "target": { "kind": "pr",
                  "url": "https://github.com/example/orderbot/pull/62", "number": 62, "repo": "开发 · Forge", "state": "检查 2/3" } },
                { "type": "ref", "id": "p59", "label": "话题加载提速", "target": { "kind": "pr",
                  "url": "https://github.com/example/orderbot/pull/59", "number": 59, "repo": "开发 · Forge", "state": "1 项检查没过" } },
                { "type": "ref", "id": "p412", "label": "群相册分享", "target": { "kind": "pr",
                  "url": "https://github.com/example/orderbot/pull/412", "number": 412, "repo": "运维 · Forge", "state": "已合并 11-02" } }
            ]
        }),
    ));
    out.push(sample(
        PR_BARE,
        DEV,
        T_RECONNECT,
        FORGE,
        at(10, 12),
        None,
        json!({
            "v": 2, "rev": 1, "intent": "ask", "title": "PR #63 撤回时同步删除通知",
            "fallback": "Forge 请你审 · 没附证据",
            "ask": { "from": [ME] },
            "blocks": [
                { "type": "evidence", "id": "checks", "label": "3 项检查通过", "state": "pass" },
                { "type": "evidence", "id": "lines", "label": "+38 −6 · 3 个文件" },
                { "type": "ref", "id": "pr", "label": "撤回时同步删除通知", "target": { "kind": "pr",
                  "url": "https://github.com/example/orderbot/pull/63", "number": 63, "repo": "fix/recall-notify → main" } }
            ],
            "actions": [
                { "id": "approve", "label": "通过", "style": "primary" },
                { "id": "changes", "label": "要改", "kind": "cancel" }
            ]
        }),
    ));

    // 08 证据
    out.push(sample(
        EVIDENCE,
        DEV,
        T_RECONNECT,
        FORGE,
        at(9, 55),
        None,
        json!({
            "v": 2, "rev": 1, "intent": "done", "title": "改好了：重连后不再重复",
            "fallback": "截图、录屏和测试结果都在",
            "blocks": [
                { "type": "text", "id": "why", "text": "原因是客户端重连后从**旧的 seq** 重新拉取，服务器把已经收到的消息又推了一遍。\n\n改法：\n- **去重**：按 seq 去掉重复的消息\n- **续传**：重连时从最后一个确认过的 seq 继续" },
                { "type": "evidence", "id": "s1", "label": "断网前", "fileId": "f-shot-1",
                  "detail": "#1201 小王：能复现了\n#1202 Forge：收到\n#1203 李雪：周四发" },
                { "type": "evidence", "id": "s2", "label": "重连后 · 修复前", "fileId": "f-shot-2",
                  "detail": "#1203 李雪：周四发\n#1203 李雪：周四发\n#1204 小王：还在吗" },
                { "type": "evidence", "id": "s3", "label": "重连后 · 修复后", "fileId": "f-shot-3",
                  "detail": "#1203 李雪：周四发\n#1204 小王：还在吗\n#1205 Forge：好了" },
                { "type": "evidence", "id": "s4", "label": "压测", "fileId": "f-shot-4" },
                { "type": "evidence", "id": "s5", "label": "压测", "fileId": "f-shot-5" },
                { "type": "evidence", "id": "tests", "label": "测试 148 项通过 · 0 失败", "state": "pass",
                  "detail": "录屏：断网 60 秒后恢复，消息没有重复 · 截图来自 Forge 的电脑 09:52" }
            ]
        }),
    ));
    out.push(sample(
        EVIDENCE_NONE,
        OPS,
        T_STAGING,
        WATCH,
        at(10, 8),
        None,
        json!({
            "v": 2, "rev": 1, "intent": "ask", "title": "告警合并，请你审",
            "fallback": "Watch 请你审",
            "ask": { "from": [ME] },
            "blocks": [
                { "type": "text", "id": "what", "text": "同一个群 5 分钟内的重复告警，合并成一条再发。" },
                { "type": "ref", "id": "p70", "label": "告警合并", "target": { "kind": "pr",
                  "url": "https://github.com/example/orderbot/pull/70", "repo": "example/orderbot", "number": 70 } }
            ],
            "actions": [
                { "id": "approve", "label": "通过", "style": "primary" },
                { "id": "changes", "label": "要改", "kind": "cancel" }
            ]
        }),
    ));
    out.push(sample(
        EVIDENCE_CHIPS,
        OPS,
        T_STAGING,
        SCOUT,
        at(9, 12),
        None,
        json!({
            "v": 2, "rev": 1, "intent": "done", "title": "staging 巡检：24 项都通过",
            "fallback": "每日巡检",
            "blocks": [
                { "type": "evidence", "id": "pass", "label": "24 项通过", "state": "pass" },
                { "type": "evidence", "id": "shots", "label": "截图 6", "state": "info" },
                { "type": "evidence", "id": "log", "label": "日志", "state": "info",
                  "url": "https://staging.example.com/qa/daily" }
            ]
        }),
    ));

    // 09 链接
    out.push(sample(
        REFS,
        GROWTH,
        T_REPORT,
        JOJO,
        at(10, 19),
        None,
        json!({
            "v": 2, "rev": 1, "intent": "info", "title": "今天要看的四样",
            "fallback": "今天要看的四样",
            "blocks": [
                { "type": "ref", "id": "pr", "label": "重连后按 seq 去重", "target": { "kind": "pr",
                  "url": "https://github.com/example/orderbot/pull/61", "number": 61, "repo": "开发 · Forge · 3 项检查通过", "state": "等你审" } },
                { "type": "ref", "id": "art", "label": "10 月数据月报", "target": { "kind": "artifact",
                  "artifactId": ART_REPORT, "version": 2 } },
                { "type": "ref", "id": "topic", "label": "拾光 4.0 需求", "target": { "kind": "topic",
                  "groupId": GROWTH, "topicId": T_V4 } },
                { "type": "ref", "id": "member", "label": "Ledger", "target": { "kind": "member", "memberId": LEDGER } }
            ]
        }),
    ));
    out.push(sample(
        REFS_STALE,
        OPS,
        T_STAGING,
        WATCH,
        at(10, 1),
        None,
        json!({
            "v": 2, "rev": 1, "intent": "info", "title": "上周的告警改动",
            "fallback": "上周的告警改动",
            "blocks": [
                { "type": "ref", "id": "pr", "label": "告警降噪", "target": { "kind": "pr",
                  "url": "https://github.com/example/orderbot/pull/418", "number": 418, "repo": "运维 · Watch", "state": "已合并" } },
                { "type": "ref", "id": "gone", "label": "orderbot 告警汇总", "target": { "kind": "artifact",
                  "artifactId": "art-v2-gone" } }
            ]
        }),
    ));

    // A run's done card around a web 产出 (server `doneCard`).
    out.push(sample(
        DONE_PAGE,
        GROWTH,
        T_REPORT,
        JOJO,
        at(10, 18),
        None,
        json!({
            "v": 2, "rev": 1, "intent": "done",
            "title": "解读页面已经发到群里，标题是“解读 | 10 月数据月报”。",
            "fallback": "解读页面已经发到群里。",
            "blocks": [
                { "type": "text", "id": "result",
                  "text": "**文章核心**：日活按去重设备算是 2340，比 9 月多 4%。新增群主要来自 B 站教程视频。\n\n**页面有 9 节**：\n\n- 先讲口径的变化和度量方式。\n- “一次增长的来源”是可以点的流程图，点任意一步能看这一步的细节。\n- **渠道模拟器**：选一种渠道，再勾选几种情况，页面会显示预计新增群。\n- **落地清单**：15 条，可以勾选，带进度条。\n\n页面里的数字都和原表核对过。我在本机 Chrome 里截图看过排版，但没有在群里的产出预览中打开过。" },
                { "type": "evidence", "id": "acceptance-1", "label": "阅读原表", "state": "pass",
                  "detail": "抓取了原表全部 12 列，数字逐条和原表核对" },
                { "type": "evidence", "id": "acceptance-2", "label": "写出一个 HTML 解读页面，内容和原表一致", "state": "pass",
                  "detail": "共 9 节：流程图、渠道模拟器、4 张前后对比图、15 条清单；脚本语法检查通过" },
                { "type": "evidence", "id": "acceptance-3", "label": "排版正常", "state": "pass",
                  "detail": "用本机 Chrome 无头模式截了桌面宽度的图，逐段查看，没有重叠或溢出" },
                { "type": "evidence", "id": "acceptance-4", "label": "在群里的产出预览中，模拟器和清单能点", "state": "info",
                  "detail": "没在群里的产出预览中打开过，需要你试一下" },
                { "type": "evidence", "id": "acceptance-5", "label": "作为产出发到群里", "state": "pass" },
                { "type": "ref", "id": "artifact-1", "label": "10 月数据月报 · v3",
                  "target": { "kind": "artifact", "artifactId": ART_REPORT, "version": 3 } }
            ]
        }),
    ));

    // States: failed, expired, an answer on its way.
    out.push(sample(
        FAILED,
        DEV,
        T_RECONNECT,
        FORGE,
        at(10, 5),
        None,
        json!({
            "v": 2, "rev": 1, "intent": "done", "title": "PR #59 话题加载提速",
            "fallback": "PR #59 · 1 项",
            "blocks": [
                { "type": "evidence", "id": "lint", "label": "PR #59 · 1 项", "state": "fail" }
            ],
            "actions": [{ "id": "rerun", "label": "重新跑检查" }]
        }),
    ));
    out.push(sample(
        EXPIRED,
        OPS,
        T_STAGING,
        WATCH,
        at(10, 0),
        None,
        json!({
            "v": 2, "rev": 1, "intent": "ask", "title": "PR #418 告警降噪",
            "fallback": "Watch 请你审 PR #418",
            "ask": { "from": [ME], "expiresAt": at(10, 20) },
            "blocks": [{ "type": "text", "id": "t", "text": "合并后同一个群 5 分钟内的告警只发一条。" }],
            "actions": [{ "id": "approve", "label": "通过", "style": "primary" }]
        }),
    ));
    let mut sending = sample(
        SENDING,
        DEV,
        T_INVITE,
        FORGE,
        at(9, 58),
        None,
        choice_card(1),
    );
    sending.pending = Some(CardV2Pending {
        rev: 1,
        action_id: "pick".into(),
        inputs: serde_json::from_value(json!({ "style": "one" })).ok(),
        edited_values: None,
        phase: CardV2SendPhase::Sending,
        at: at(10, 20),
    });
    out.push(sending);

    // 11 交互产出: the snapshot of an interactive 产出 with its answer.
    let mut page = sample(
        PAGE,
        GROWTH,
        T_REPORT,
        LEDGER,
        at(9, 40),
        None,
        json!({
            "v": 2, "rev": 1, "intent": "ask", "title": "下个月加预算投哪个渠道？",
            "fallback": "做好了，渠道拆分在第二屏。",
            "ask": { "from": [ME], "record": true },
            "blocks": [
                { "type": "metric", "id": "groups", "label": "新增群", "value": 38 },
                { "type": "metric", "id": "dau", "label": "日活", "value": 2340 },
                { "type": "metric", "id": "reply", "label": "平均回复", "value": 1.2, "unit": "秒" },
                { "type": "decision", "id": "channel", "default": ["weibo"], "options": [
                    { "id": "weibo", "label": "微博" }, { "id": "bili", "label": "B 站" },
                    { "id": "douyin", "label": "抖音" }, { "id": "none", "label": "先不加，维持现状" }
                ]}
            ],
            "actions": [
                { "id": "submit", "label": "提交", "style": "primary" },
                { "id": "open", "label": "打开 10 月数据月报", "kind": "link",
                  "target": { "kind": "artifact", "artifactId": ART_REPORT, "version": 3 } }
            ]
        }),
    );
    page.answers.push(answer(
        ME,
        1,
        "submit",
        json!({ "channel": "bili" }),
        at(10, 12),
    ));
    out.push(page);
    // --- inc-cards21: the Card v2.1 samples ---
    let now = crate::selftest::clock::now_ms();
    out.extend(
        crate::views::cards::v21::samples::all()
            .into_iter()
            .map(|s| Sample {
                msg: s.msg,
                answers: s.answers,
                pending: s.pending,
                events: s
                    .events
                    .iter()
                    .map(|(ago, e)| crate::views::cards::v21::samples::at_ago(e, *ago, now))
                    .collect(),
            }),
    );
    // --- end inc-cards21 ---
    out
}

/// The sample cards with their answers; answering adds a stored answer at
/// once (the sample team has no server).
pub struct SampleCards {
    inner: Mutex<HashMap<String, Sample>>,
}

impl Default for SampleCards {
    fn default() -> Self {
        Self::new()
    }
}

impl SampleCards {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(
                all()
                    .into_iter()
                    .map(|s| (s.msg.message_id.clone(), s))
                    .collect(),
            ),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Sample>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn key(message_id: &str) -> String {
        alias(message_id).unwrap_or(message_id).to_string()
    }

    pub fn card(&self, message_id: &str) -> Option<CardV2Message> {
        let mut msg = self.lock().get(&Self::key(message_id))?.msg.clone();
        msg.message_id = message_id.to_string();
        Some(msg)
    }

    pub fn answers(&self, message_id: &str) -> Vec<CardV2Answer> {
        self.lock()
            .get(&Self::key(message_id))
            .map(|s| s.answers.clone())
            .unwrap_or_default()
    }

    pub fn pending(&self, message_id: &str) -> Option<CardV2Pending> {
        self.lock()
            .get(&Self::key(message_id))
            .and_then(|s| s.pending.clone())
    }

    /// Stores `me`'s answer at once. Returns the topic for the change.
    pub fn answer(
        &self,
        message_id: &str,
        me: &str,
        request: CardV2AnswerRequest,
        now: Millis,
    ) -> Option<(String, String)> {
        let mut map = self.lock();
        let s = map.get_mut(&Self::key(message_id))?;
        if request.rev != s.msg.card.rev || s.answers.iter().any(|a| a.rev == request.rev) {
            return None;
        }
        s.pending = None;
        s.answers.push(CardV2Answer {
            actor_id: me.into(),
            rev: request.rev,
            action_id: request.action_id,
            inputs: request.inputs,
            edited_values: request.edited_values,
            at: now,
        });
        Some((s.msg.group_id.clone(), s.msg.topic_id.clone()))
    }

    // --- inc-cards21: card events on the sample team (acked at once, live author) ---

    pub fn events(&self, message_id: &str) -> Vec<crate::data::cards_v2::MyCardEvent> {
        self.lock()
            .get(&Self::key(message_id))
            .map(|s| s.events.clone())
            .unwrap_or_default()
    }

    /// Records my event as acked by a connected author. A submit, reject or commit is
    /// stored as my answer. Returns the topic for the change.
    pub fn send_event(
        &self,
        message_id: &str,
        me: &str,
        request: crate::data::cards_v2::CardEventRequest,
        now: Millis,
    ) -> Option<(String, String, String)> {
        use crate::data::cards_v2::{
            CardEventAck, CardEventPhase, Expect, ExpectMode, MyCardEvent,
        };
        let mut map = self.lock();
        let s = map.get_mut(&Self::key(message_id))?;
        let card = s.msg.card.clone();
        let answer_action = request.action_id.as_ref().and_then(|a| {
            card.actions()
                .iter()
                .find(|x| &x.id == a)
                .filter(|x| {
                    matches!(
                        x.kind(),
                        crate::data::cards_v2::CardActionKind::Submit
                            | crate::data::cards_v2::CardActionKind::Reject
                    )
                })
                .map(|x| x.id.clone())
        });
        let commit = request
            .block_id
            .as_ref()
            .is_some_and(|b| card.is_answer_event(b, &request.event));
        let durable = answer_action.is_some() || commit;
        if durable {
            let inputs = match (&request.inputs, &request.block_id, &request.value) {
                (Some(i), _, _) => Some(i.clone()),
                (None, Some(b), Some(v)) => Some([(b.clone(), v.clone())].into_iter().collect()),
                _ => None,
            };
            let action_id = answer_action
                .or(request.block_id.clone())
                .unwrap_or_default();
            // A changeable vote replaces my earlier one.
            s.answers
                .retain(|a| !(a.actor_id == me && a.rev == request.rev && commit));
            s.answers.push(CardV2Answer {
                actor_id: me.into(),
                rev: request.rev,
                action_id,
                inputs,
                edited_values: None,
                at: now,
            });
            s.pending = None;
        }
        let id = format!("sample-{}-{now}", s.events.len());
        if let Some(b) = &request.block_id {
            for e in s.events.iter_mut().filter(|e| {
                !e.durable
                    && e.request.block_id.as_deref() == Some(b)
                    && e.request.event == request.event
            }) {
                e.dropped = true;
            }
        }
        s.events.push(MyCardEvent {
            client_event_id: id.clone(),
            request,
            durable,
            sent_at: now,
            phase: CardEventPhase::Acked {
                ack: CardEventAck {
                    event_id: format!("ev-{id}"),
                    stored: durable,
                    answer_id: None,
                    duplicate: None,
                    expect: Expect {
                        by: s.msg.author_id.clone(),
                        mode: ExpectMode::Live,
                        eta_ms: Some(2000),
                    },
                },
                at: now,
            },
            dropped: false,
            reply: None,
        });
        if s.events.len() > 20 {
            s.events.remove(0);
        }
        Some((s.msg.group_id.clone(), s.msg.topic_id.clone(), id))
    }

    /// 不等了.
    pub fn drop_event(&self, message_id: &str, client_event_id: &str) -> Option<(String, String)> {
        let mut map = self.lock();
        let s = map.get_mut(&Self::key(message_id))?;
        for e in s
            .events
            .iter_mut()
            .filter(|e| e.client_event_id == client_event_id)
        {
            e.dropped = true;
        }
        Some((s.msg.group_id.clone(), s.msg.topic_id.clone()))
    }
    // --- end inc-cards21 ---

    /// Takes back `me`'s answer (the sample stands in for an answer that is
    /// still queued) or a refused one.
    pub fn withdraw(&self, message_id: &str, me: &str) -> Option<(String, String)> {
        let mut map = self.lock();
        let s = map.get_mut(&Self::key(message_id))?;
        s.pending = None;
        if let Some(i) = s.answers.iter().rposition(|a| a.actor_id == me) {
            s.answers.remove(i);
        }
        Some((s.msg.group_id.clone(), s.msg.topic_id.clone()))
    }
}
