//! Screenshot self-test scenes for diagrams (Refs.dc.html §04, §06, §07): a 话题 with
//! messages whose bodies hold ```mermaid fences, drawn by the real pipeline (normalize,
//! hidden renderer, snapshot, cache) through the real Markdown body.
//!
//! - `diagram-inline`: a sequence diagram, a flowchart and a gantt.
//! - `diagram-error`: each error kind (parse, unsupported, forbidden, too large; the tall
//!   too-large ones last so all four fit the capture), a fifth fence that stays code, a
//!   streaming body.
//! - `diagram-viewer`: the expand viewer zoomed to 250 % (the 4× raster).
//! - With `--worst-case`: a 150-node flowchart with long Chinese labels in a narrow window
//!   (`diagram-inline`, `diagram-viewer`); `diagram-error` keeps each error state there.

use std::path::PathBuf;
use std::sync::Mutex;

use gpui::{
    AnyWindowHandle, App, AppContext as _, Context, ElementId, InteractiveElement as _,
    IntoElement, ParentElement, Pixels, Render, SharedString, StatefulInteractiveElement as _,
    Styled, Window, WindowBounds, div, prelude::FluentBuilder as _, px, size,
};

use super::{INLINE_SCALE, normalize};
use crate::{
    app::{Services, main_window_options},
    data::Member,
    fake, markdown,
    selftest::scenes::capture_bounds,
    theme::{self, Type as _, space},
    ui::MemberAvatar,
};

/// The cache folder self-tests use (emptied once per run), never the person's cache.
pub static CACHE_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    Inline,
    Error,
    Viewer,
}

struct Msg {
    author: &'static str,
    time: &'static str,
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
    let m = |author, time, body: String| Msg {
        author,
        time,
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
                time: "10:17",
                body: "正在写：\n\n```mermaid\nflowchart LR\n  x[收到] --> y[处理".to_string(),
                streaming: true,
            },
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

pub struct DiagramPreview {
    scene: Scene,
    worst: bool,
    members: Vec<Member>,
}

impl DiagramPreview {
    fn new(services: &Services, scene: Scene, worst: bool, cx: &mut Context<Self>) -> Self {
        super::clear_memory(cx);
        let members = ["m-xiaowang", "m-coder", "m-alex", "m-sentry", fake::ME]
            .iter()
            .filter_map(|id| services.provider.member(id))
            .collect();
        Self {
            scene,
            worst,
            members,
        }
    }

    fn row(&self, i: usize, msg: &Msg, cx: &App) -> impl IntoElement {
        let k = theme::colors(cx);
        let member = self
            .members
            .iter()
            .find(|m| m.id == msg.author)
            .or(self.members.first());
        let (view, _) = markdown::message_body_marked(
            ElementId::Name(format!("dprev-body-{i}").into()),
            msg.body.clone(),
            &[],
            false,
            Some(&markdown::DiagramContext {
                message_id: format!("dprev-{i}"),
                topic_id: "dprev-topic".into(),
                streaming: msg.streaming,
            }),
        );
        div()
            .flex()
            .gap(space::S12)
            .py(space::S8)
            .child(MemberAvatar::new(member, px(28.)))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(space::S4)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space::S8)
                            .child(div().t_small().semibold().text_color(k.ink).child(
                                SharedString::from(
                                    member.map(|m| m.name.clone()).unwrap_or_default(),
                                ),
                            ))
                            .child(div().t_mono().text_color(k.ink_3).child(msg.time)),
                    )
                    .child(view.w_full().text_color(k.ink)),
            )
    }
}

impl Render for DiagramPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let msgs = messages(self.scene, self.worst);
        div()
            .id("diagram-preview")
            .size_full()
            .bg(k.bg)
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(52.))
                    .flex_none()
                    .pl(px(92.))
                    .pr(space::S20)
                    .flex()
                    .items_center()
                    .gap(space::S6)
                    .border_b_1()
                    .border_color(k.line)
                    .child(div().t_small().text_color(k.ink_3).child("开发 /"))
                    .child(
                        div()
                            .t_bar_title()
                            .text_color(k.ink)
                            .child("断线重连后消息重复"),
                    ),
            )
            .child(
                div()
                    .id("diagram-preview-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .mx_auto()
                            .w_full()
                            .max_w(px(theme::PAGE_MEASURE.into()))
                            .px(space::S24)
                            .py(space::S16)
                            .flex()
                            .flex_col()
                            .children(msgs.iter().enumerate().map(|(i, m)| self.row(i, m, cx))),
                    ),
            )
            .when(self.worst, |el| el)
    }
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
    let worst = crate::selftest::worst_case();
    let content = if worst {
        size(px(760.), content.height)
    } else {
        content
    };
    let mut options = main_window_options(false, cx);
    options.window_bounds = Some(WindowBounds::Windowed(capture_bounds(content, cx)));
    options.window_min_size = None;
    let (handle, _) = gpui_kit::open_window(options, cx, move |_, cx| {
        cx.new(|cx| DiagramPreview::new(&services, scene, worst, cx))
    })
    .map_err(|e| e.to_string())?;
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
                                message_id: Some("dprev-1".into()),
                                topic_id: Some("dprev-topic".into()),
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
                    viewer.update(cx, |v, cx| v.set_zoom(if worst { 1.0 } else { 2.5 }, cx));
                })
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(handle)
}

/// True once every diagram of the scene has an answer (image or error).
pub fn ready(scene: Scene, cx: &App) -> bool {
    let worst = crate::selftest::worst_case();
    let all = sources(scene, worst).iter().all(|n| {
        let scale = INLINE_SCALE;
        matches!(super::peek(n, scale, cx), Some(s) if !s.is_pending())
    });
    let zoomed = scene != Scene::Viewer
        || worst
        || sources(scene, worst).first().is_none_or(
            |n| matches!(super::peek(n, super::ZOOM_SCALE, cx), Some(s) if !s.is_pending()),
        );
    all && zoomed && super::idle(cx)
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
