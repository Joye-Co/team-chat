//! Screenshot self-test scenes for diagrams (Refs.dc.html §04, §06, §07): the real main
//! window, whose first 话题 ends with messages whose bodies hold ```mermaid fences, drawn
//! by the real pipeline (normalize, hidden renderer, snapshot, cache) through the real
//! thread.
//!
//! - `diagram-inline`: a sequence diagram, a flowchart and a gantt.
//! - `diagram-error`: each error kind (parse, unsupported, forbidden, too large), a fifth
//!   fence that stays code, a streaming body.
//! - `diagram-viewer`: the viewer as it opens, at fit.
//! - `diagram-loading`: a diagram while it draws (the skeleton).
//! - With `--worst-case`: a 150-node flowchart with long Chinese labels (`diagram-inline`,
//!   `diagram-viewer`); `diagram-error` keeps each error state.

use std::path::PathBuf;
use std::sync::Mutex;

use gpui::{AnyWindowHandle, App, Pixels};

use super::{INLINE_SCALE, normalize};
use crate::{
    app::Services,
    selftest::scenes::{open_first_topic, open_main},
    views::media::preview::{Extra, MediaThread},
};

/// The cache folder self-tests use (emptied once per run), never the person's cache.
pub static CACHE_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    Inline,
    Error,
    Viewer,
    Loading,
}

struct Msg {
    author: &'static str,
    body: String,
    streaming: bool,
}

const SEQUENCE: &str = "sequenceDiagram\n  participant C as 客户端\n  participant D as seq 去重\n  participant S as 服务器\n  loop 连不上时\n    C->>C: 等 1、2、4 … 秒再试，最多等 30 秒\n  end\n  C->>S: 重连 · 带上最后确认的 seq 1041\n  S->>D: 从 1042 推到 1050\n  Note over D: 已经有的 seq 直接跳过\n  D->>C: 9 条新消息放进列表\n  C-->>S: 确认到 seq 1050";

const FLOW: &str = "flowchart LR\n  build[构建<br/>2 分钟] --> staging[staging<br/>已部署]\n  staging --> check{Scout 验收<br/>12 项通过}\n  check -->|通过| approve[批准]\n  check -.->|没通过就回到构建| build\n  approve --> prod[生产]\n  prod --> watch[盯 15 分钟]\n  style approve fill:#f80,stroke:#f00";

const GANTT: &str = "gantt\n  dateFormat YYYY-MM-DD\n  axisFormat %m-%d\n  section 发布 v2.14\n  构建和单测 :done, b1, 2026-11-01, 1d\n  staging 验收 :active, b2, after b1, 2d\n  回滚演练 :crit, b3, after b2, 1d\n  灰度 10% :b4, after b3, 1d\n  全量 :b5, after b4, 1d";

fn fence(src: &str) -> String {
    format!("```mermaid\n{src}\n```")
}

/// A flowchart with `n` nodes: long Chinese labels (worst case) or short ones.
pub fn big_flowchart(n: usize, long: bool) -> String {
    let mut s = String::from("flowchart TD\n");
    for i in 0..n {
        if long {
            s.push_str(&format!(
                "  n{i}[第 {i} 步：按 seq 去重后放进话题列表并记下确认位置]\n"
            ));
        } else {
            s.push_str(&format!("  n{i}[检查 {i}]\n"));
        }
    }
    // Ten chains side by side, so the drawing stays inside the 4096 px limit.
    let per = n.div_ceil(10).max(1);
    for i in 1..n {
        if i % per != 0 {
            s.push_str(&format!("  n{} --> n{i}\n", i - 1));
        }
    }
    s
}

fn messages(scene: Scene, worst: bool) -> Vec<Msg> {
    let m = |author, _time: &str, body: String| Msg {
        author,
        body,
        streaming: false,
    };
    // The error scene keeps every error state in the worst case too (in the narrow
    // window); the others draw the 150-step flowchart.
    if worst && scene != Scene::Error {
        return vec![
            m(
                "m-xiaowang",
                "10:00",
                "整条同步链路画出来看看，每一步都写清楚。".into(),
            ),
            m(
                "m-coder",
                "10:02",
                format!(
                    "全部 150 步都在这里：\n\n{}",
                    fence(&big_flowchart(150, true))
                ),
            ),
        ];
    }
    match scene {
        Scene::Loading => vec![m(
            "m-coder",
            "10:02",
            format!("发布流程也补一张：\n\n{}", fence(FLOW)),
        )],
        Scene::Inline | Scene::Viewer => vec![
            m(
                "m-xiaowang",
                "10:00",
                "重连那块现在到底怎么走？画一下吧，我好写到文档里。".into(),
            ),
            m(
                "m-coder",
                "10:02",
                format!(
                    "重连现在这样走：连不上先按指数退避重试；连上后带着最后确认的 seq，服务器只推之后的消息，客户端再按 seq 去重一次。\n\n{}\n\n文档里可以直接贴这段原文。",
                    fence(SEQUENCE)
                ),
            ),
            m(
                "m-alex",
                "10:05",
                format!("发布流程也补一张：\n\n{}", fence(FLOW)),
            ),
            m(
                "m-sentry",
                "10:06",
                format!("这周的排期：\n\n{}", fence(GANTT)),
            ),
        ],
        Scene::Error => vec![
            m(
                "m-coder",
                "10:16",
                format!(
                    "五张图放一条消息里，第五张只显示原文：\n\n{}\n{}\n{}\n{}\n{}",
                    fence("flowchart LR\n  a1[一] --> a2[二]"),
                    fence("flowchart LR\n  b1[一] --> b2[二]"),
                    fence("flowchart LR\n  c1[一] --> c2[二]"),
                    fence("flowchart LR\n  d1[一] --> d2[二]"),
                    fence("flowchart LR\n  e1[第五张] --> e2[原文]")
                ),
            ),
            Msg {
                author: "m-coder",
                body: "正在写：\n\n```mermaid\nflowchart LR\n  x[收到] --> y[处理".to_string(),
                streaming: true,
            },
            m(
                "m-coder",
                "10:12",
                format!(
                    "发布前后的顺序也画一下：\n\n{}",
                    fence(
                        "sequenceDiagram\n  participant 客户端\n  客户端 -> > 服务器: 重连\n  服务器 -->> 客户端: 从 1042 推到 1050\n  客户端 -->> 服务器: 确认到 1050"
                    )
                ),
            ),
            m(
                "m-alex",
                "10:13",
                format!(
                    "夹夹 4.0 的结构：\n\n{}",
                    fence("mindmap\n  root((夹夹 4.0))\n    首页\n    群相册\n    分享")
                ),
            ),
            m(
                "m-coder",
                "10:13",
                format!(
                    "点一下就能打开：\n\n{}",
                    fence("flowchart LR\n  A[报告] --> B[详情]\n  click A \"https://example.com\"")
                ),
            ),
            m(
                "m-sentry",
                "10:14",
                format!("所有检查项：\n\n{}", fence(&big_flowchart(160, false))),
            ),
            m(
                "m-sentry",
                "10:14",
                format!("原始清单：\n\n{}", fence(&big_flowchart(400, true))),
            ),
        ],
    }
}

/// Every source a scene draws, normalized (refusals left out).
fn sources(scene: Scene, worst: bool) -> Vec<normalize::Normalized> {
    let mut out = Vec::new();
    for msg in messages(scene, worst) {
        if msg.streaming {
            continue;
        }
        let offsets = super::diagram_fences(&msg.body, false);
        for f in super::fences(&msg.body) {
            if !offsets.contains(&f.offset) {
                continue;
            }
            let start = msg.body[f.offset..]
                .find('\n')
                .map_or(msg.body.len(), |n| f.offset + n + 1);
            let rest = &msg.body[start..];
            let end = rest.find("\n```").unwrap_or(rest.len());
            if let Ok(n) = normalize::normalize(&rest[..end]) {
                out.push(n);
            }
        }
    }
    out
}

fn clear_cache_once() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let dir = std::env::temp_dir().join("tc-diagrams-selftest");
        let _ = std::fs::remove_dir_all(&dir);
        if let Ok(mut d) = CACHE_DIR.lock() {
            *d = Some(dir);
        }
    });
}

/// Opens a scene window.
pub fn open(
    services: Services,
    content: gpui::Size<Pixels>,
    scene: Scene,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    clear_cache_once();
    super::clear_memory(cx);
    if scene == Scene::Loading {
        LOADING.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    let worst = crate::selftest::worst_case();
    let extras = messages(scene, worst)
        .into_iter()
        .map(|m| Extra {
            author: m.author.into(),
            body: m.body,
            streaming: m.streaming,
            artifact: None,
        })
        .collect();
    let thread = MediaThread::with_messages(services.provider.clone(), extras);
    let topic = (thread.group_id().to_string(), thread.topic_id().to_string());
    let services = Services::new(thread, cx);
    open_first_topic(&services, cx);
    let handle = open_main(services, content, cx)?;
    if scene == Scene::Viewer {
        let source = sources(Scene::Viewer, worst).into_iter().next();
        if let Some(n) = source {
            let raw = n.source.clone();
            handle
                .update(cx, |_, window, cx| {
                    let viewer = super::viewer::open(
                        super::viewer::ViewerSpec {
                            normalized: n,
                            raw,
                            title: Some(if worst {
                                "同步链路".into()
                            } else {
                                "重连的顺序".into()
                            }),
                            origin: super::DiagramSource {
                                message_id: Some("extra-1".into()),
                                topic_id: Some(topic.1.clone()),
                                artifact: None,
                                block: super::DiagramBlock::MarkdownFence {
                                    offset: 0,
                                    index: Some(0),
                                },
                                block_name: None,
                            },
                            return_focus: None,
                        },
                        window,
                        cx,
                    );
                    // The viewer opens at fit (F64): the scene shows it as it opens.
                    let _ = viewer;
                })
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(handle)
}

/// Set by `diagram-loading`: inline diagrams stay pending, so the scene shows the
/// skeleton.
pub static LOADING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// True once every diagram of the scene has an answer (image or error).
pub fn ready(scene: Scene, cx: &App) -> bool {
    if scene == Scene::Loading {
        return true;
    }
    // The thread draws only the messages in view: every diagram it asked for has an
    // answer, and at least one did.
    let worst = crate::selftest::worst_case();
    let states: Vec<_> = sources(scene, worst)
        .iter()
        .map(|n| super::peek(n, INLINE_SCALE, cx))
        .collect();
    let all = states
        .iter()
        .all(|s| s.as_ref().is_none_or(|s| !s.is_pending()))
        && states.iter().any(|s| s.is_some());
    all && super::idle(cx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_worst_error_scene_keeps_every_error_state() {
        let bodies = |worst| -> Vec<String> {
            messages(Scene::Error, worst)
                .into_iter()
                .map(|m| m.body)
                .collect()
        };
        assert_eq!(bodies(true), bodies(false));
        assert_ne!(
            messages(Scene::Inline, true)
                .into_iter()
                .map(|m| m.body)
                .collect::<Vec<_>>(),
            bodies(true)
        );
    }
}
