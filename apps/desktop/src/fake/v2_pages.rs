//! The sample data of the 群, 成员 and 产出 pages (pages track), as the
//! artboards Group.dc.html, Members.dc.html and Artifact.dc.html show it.

use std::sync::Mutex;

use super::*;
use crate::data::pages::*;

/// The artboards' "now": 已 3 分钟 into the stress test.
#[allow(non_snake_case)]
pub fn NOW() -> Millis {
    at(9, 49)
}

pub const ART_SEARCH: &str = "art-v2-search-check";
pub const ART_RELEASE_NOTES: &str = "art-v2-release-notes";

static RECORDED: Mutex<Vec<Decision>> = Mutex::new(Vec::new());

/// Decisions added with 记一条, newest first.
pub fn recorded(group_id: &str) -> Vec<Decision> {
    RECORDED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .filter(|d| d.group_id == group_id)
        .cloned()
        .collect()
}

pub fn record(group_id: &str, text: String, now: Millis) {
    let mut all = RECORDED.lock().unwrap_or_else(|e| e.into_inner());
    let id = format!("d-recorded-{}", all.len());
    all.insert(
        0,
        Decision {
            id,
            group_id: group_id.into(),
            topic_id: None,
            text,
            by: ME.into(),
            at: now,
        },
    );
}

pub fn topic_glance(_group_id: &str, topic_id: &str) -> TopicGlance {
    match topic_id {
        T_RECONNECT => TopicGlance {
            preview: Some(TopicPreview::Diff {
                path: "src/sync/reconnect.ts".into(),
                additions: 5,
                deletions: 2,
                lines: vec![
                    DiffLine {
                        added: false,
                        text: "const from = conn.lastSeq;".into(),
                    },
                    DiffLine {
                        added: true,
                        text: "const from = conn.lastAckedSeq;".into(),
                    },
                ],
            }),
            tests_passed: Some(148),
            ..TopicGlance::default()
        },
        T_INVITE => TopicGlance {
            state_note: Some("输入框用 A 还是 B".into()),
            footer_note: Some("Forge 推荐 A：实习生大多从微信复制".into()),
            preview: Some(TopicPreview::Choice {
                options: vec![
                    ChoiceOption {
                        key: "A".into(),
                        label: "一个输入框".into(),
                        sketch: Some(ChoiceSketch::Field("TC8F3K29".into())),
                    },
                    ChoiceOption {
                        key: "B".into(),
                        label: "8 个分格".into(),
                        sketch: Some(ChoiceSketch::Cells {
                            total: 8,
                            filled: "TC8".into(),
                        }),
                    },
                ],
            }),
            ..TopicGlance::default()
        },
        T_VIRTUAL => TopicGlance {
            preview: Some(TopicPreview::Screen {
                run_id: RUN_VIRTUAL.into(),
            }),
            ..TopicGlance::default()
        },
        T_SEARCH => TopicGlance {
            state_note: Some("分词已验收".into()),
            footer_note: Some("高亮样式改好了".into()),
            preview: Some(TopicPreview::Matches {
                query: "重连".into(),
                count: 2,
                lines: vec!["断线重连后消息重复".into(), "重连间隔改成了指数退避".into()],
            }),
            ..TopicGlance::default()
        },
        _ => TopicGlance::default(),
    }
}

pub fn recent_done(group_id: &str) -> Vec<RecentDone> {
    if group_id != DEV {
        return Vec::new();
    }
    let done = |id: &str, title: &str, note: &str, at: Millis| RecentDone {
        group_id: DEV.into(),
        topic_id: id.into(),
        member_id: FORGE.into(),
        title: title.into(),
        note: Some(note.into()),
        at,
    };
    vec![
        done(
            "t-v2-stop",
            "TeamChat 1.0.1：话题里能停止成员的工作",
            "已发布",
            at(8, 30),
        ),
        done(
            "t-v2-update",
            "TeamChat 应用内更新",
            "启动时检查新版本，下载后重启生效",
            days_ago_at(1, 17, 20),
        ),
        done(
            "t-v2-album",
            "qqbot 群相册分享",
            "PR #412 已合并，等 v2.14 一起上线",
            days_ago_at(1, 11, 5),
        ),
    ]
}

pub fn extra_artifacts(group_id: &str) -> Vec<ArtifactSummary> {
    if group_id != DEV {
        return Vec::new();
    }
    vec![
        artifact(
            ART_SEARCH,
            T_SEARCH,
            ArtifactKind::Page,
            "中文分词验收记录",
            1,
            SCOUT,
            at(10, 12),
        ),
        artifact(
            ART_RELEASE_NOTES,
            T_SEARCH,
            ArtifactKind::File,
            "qqbot v2.14 改动说明",
            1,
            FORGE,
            days_ago_at(1, 16, 0),
        ),
    ]
    .into_iter()
    .map(|mut a| {
        if a.id == ART_RELEASE_NOTES {
            a.topic_id = "t-v2-album".into();
        }
        a
    })
    .collect()
}

pub fn thumb(artifact_id: &str) -> Option<ArtifactThumb> {
    let row = |label: &str, value: Option<&str>, ok: bool| ThumbRow {
        label: label.into(),
        value: value.map(Into::into),
        ok,
    };
    Some(match artifact_id {
        ART_RECONNECT => ArtifactThumb {
            title: "重连测试报告".into(),
            rows: vec![
                row("断网 30 秒", None, true),
                row("切换网络", None, true),
                row("连续重连", None, true),
            ],
        },
        ART_SEARCH => ArtifactThumb {
            title: "分词验收".into(),
            rows: vec![
                row("两字词", Some("12/12"), false),
                row("人名", Some("8/8"), false),
                row("高亮", None, true),
            ],
        },
        ART_RELEASE_NOTES => ArtifactThumb {
            title: "v2.14 改动".into(),
            rows: vec![
                row("#412 群相册", None, false),
                row("#415 撤回同步", None, false),
                row("#418 告警降噪", None, false),
            ],
        },
        ART_REPORT => ArtifactThumb {
            title: "10 月数据月报".into(),
            rows: vec![
                row("新增群", Some("38"), false),
                row("日活", Some("2,340"), false),
                row("平均回复", Some("1.2 秒"), false),
            ],
        },
        _ => return None,
    })
}

pub fn member_extras(p: &V2Samples, member_id: &str) -> MemberExtras {
    let line = |s: &str| Some(s.to_string());
    let mut extras = MemberExtras {
        line: match member_id {
            WATCH => line("批准 qqbot v2.14 发布到生产"),
            TARO => line("确认小红书草稿，周四 20:00 发"),
            JOJO => line("分给了 Pocket 和 Ledger"),
            FORGE => line("在跑压测"),
            TATA => line("写第二版"),
            LEDGER => line("空闲 · 09:40 交了 10 月数据月报"),
            POCKET => line("空闲 · 明早 08:00 发晨报"),
            SCOUT => line("空闲 · 09:12 巡检 staging，24 项都通过"),
            XIAOWANG => line("在线 · 在试断线重连的修复"),
            LIXUE => line("在线 · 跟进夹夹 3.0 宣传"),
            ALEX => line("离开 · 08:50 后没回来"),
            CHENCHEN => line("离线 · 周三回来"),
            _ => None,
        },
        ..MemberExtras::default()
    };
    if member_id != FORGE {
        return extras;
    }
    extras.avg_topic_ms = Some(38 * 60_000);
    extras.prs_this_week = 3;
    extras.pages_this_week = 1;
    extras.budget_cents = Some(30_000);
    extras.deliveries = vec![
        Delivery {
            kind: DeliveryKind::Pr {
                number: 61,
                additions: 214,
                deletions: 63,
            },
            title: "按 seq 去重".into(),
            at: at(9, 55),
            topic: Some((DEV.into(), T_RECONNECT.into())),
            state: Some(DeliveryState::WaitsOnMe),
        },
        Delivery {
            kind: DeliveryKind::Artifact {
                artifact_id: ART_RECONNECT.into(),
                version: 2,
            },
            title: "重连测试报告".into(),
            at: at(9, 55),
            topic: Some((DEV.into(), T_RECONNECT.into())),
            state: None,
        },
        Delivery {
            kind: DeliveryKind::Pr {
                number: 58,
                additions: 96,
                deletions: 41,
            },
            title: "话题加载更快".into(),
            at: days_ago_at(1, 15, 30),
            topic: None,
            state: Some(DeliveryState::Merged),
        },
    ];
    extras.tools = vec![
        MemberTool {
            name: "GitHub".into(),
            access: Some("可以提 PR".into()),
            detail: Some("qqbot、夹夹、TeamChat 三个仓库".into()),
        },
        MemberTool {
            name: "Cloudflare".into(),
            access: Some("只读".into()),
            detail: None,
        },
        MemberTool {
            name: "staging".into(),
            access: Some("可以部署和回滚".into()),
            detail: None,
        },
    ];
    extras.queued = vec![QueuedWork {
        topic: Some((DEV.into(), T_INVITE.into())),
        title: "登录页支持邀请码".into(),
        note: Some("你选好输入框后开始".into()),
    }];
    let mut decisions = p.topic_decisions(DEV, T_RECONNECT);
    decisions.extend(
        p.group_decisions(DEV)
            .into_iter()
            .filter(|d| d.id == "d-invite"),
    );
    extras.decisions = decisions;
    extras
}

pub fn responses(artifact_id: &str) -> Vec<ArtifactResponse> {
    if artifact_id != ART_REPORT {
        return Vec::new();
    }
    vec![
        ArtifactResponse {
            member_id: ME.into(),
            text: "Joye 选了「B 站」".into(),
            at: Some(at(10, 12)),
            topic: None,
        },
        ArtifactResponse {
            member_id: LEDGER.into(),
            text: "Ledger 在话题里接着算预算".into(),
            at: None,
            topic: Some((GROWTH.into(), T_REPORT.into())),
        },
    ]
}

/// Writes the page of a sample 产出 version to a temporary folder.
pub fn write_page(artifact_id: &str, version: u32, title: &str) -> Result<ArtifactFolder, FileError> {
    let html = match artifact_id {
        ART_REPORT => report_html(version),
        _ => plain_page(title),
    };
    let dir = std::env::temp_dir()
        .join("team-chat-v2-samples")
        .join(artifact_id)
        .join(version.to_string());
    std::fs::create_dir_all(&dir).map_err(|_| FileError::Other)?;
    std::fs::write(dir.join("index.html"), html).map_err(|_| FileError::Other)?;
    Ok(ArtifactFolder {
        artifact_id: artifact_id.into(),
        version,
        dir,
        entry: "index.html".into(),
        is_page: true,
    })
}

/// 10 月数据月报: v3 as the artboard; v2 has no 渠道拆分 table and its
/// question takes no answers (it was answered in v3).
fn report_html(version: u32) -> String {
    let page = include_str!("v2_report.html");
    if version >= 3 {
        return page.to_string();
    }
    let mut out = String::new();
    match (page.find("<!--v3-->"), page.find("<!--/v3-->")) {
        (Some(a), Some(b)) => {
            out.push_str(&page[..a]);
            out.push_str(&page[b..]);
        }
        _ => out.push_str(page),
    }
    out.replace("class=\"opt on\"", "class=\"opt\"")
        .replace("type=\"button\" class=\"opt", "type=\"button\" disabled class=\"opt")
        .replace(
            "你选了「B 站」· <span class=\"mono\">10:12</span> · 已发给 Ledger",
            "在 v3 里回应过",
        )
}

fn plain_page(title: &str) -> String {
    format!(
        "<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\"><style>body{{margin:0;padding:32px 40px;font-family:-apple-system,\"PingFang SC\",sans-serif;color:#111113;background:#fff}}@media (prefers-color-scheme:dark){{body{{color:#F1F1F3;background:#111113}}}}</style></head><body><h2>{title}</h2></body></html>"
    )
}
