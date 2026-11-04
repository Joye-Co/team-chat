//! Unit tests for the diagram service helpers (REFS §C3): Markdown fences, the 4-per-message
//! limit, streaming bodies, alt text, source order and theming.

use super::*;

fn node(id: &str, label: &str) -> renderer::DiagramNode {
    renderer::DiagramNode {
        id: id.into(),
        label: label.into(),
        kind: "node".into(),
        x: 0.,
        y: 0.,
        w: 10.,
        h: 10.,
    }
}

fn fence(src: &str) -> String {
    format!("```mermaid\n{src}\n```\n")
}

#[test]
fn finds_closed_mermaid_fences_case_insensitively() {
    let body = format!(
        "intro\n\n```Mermaid \ngraph TD\nA-->B\n```\n\n```rust\nfn x() {{}}\n```\n\n~~~ mermaid\nsequenceDiagram\nA->>B: hi\n~~~\n"
    );
    let f = fences(&body);
    assert_eq!(f.len(), 3);
    assert!(f.iter().all(|f| f.closed));
    assert_eq!(f[0].info, "Mermaid");
    let d = diagram_fences(&body, false);
    assert_eq!(d.len(), 2);
    assert_eq!(&body[d[0]..d[0] + 3], "```");
    assert!(is_diagram_info(" MERMAID "));
    assert!(!is_diagram_info("mermaid js"));
}

#[test]
fn at_most_four_diagrams_per_message() {
    let body: String = (0..6)
        .map(|i| fence(&format!("graph TD\nA{i}-->B{i}")))
        .collect();
    let d = diagram_fences(&body, false);
    assert_eq!(d.len(), MAX_PER_MESSAGE);
    let all = fences(&body);
    assert_eq!(d, all.iter().take(4).map(|f| f.offset).collect::<Vec<_>>());
}

#[test]
fn streaming_bodies_and_unclosed_fences_show_code() {
    let body = fence("graph TD\nA-->B");
    assert!(diagram_fences(&body, true).is_empty());
    let open = "text\n\n```mermaid\ngraph TD\nA-->B\n";
    assert_eq!(fences(open).len(), 1);
    assert!(!fences(open)[0].closed);
    assert!(diagram_fences(open, false).is_empty());
    // A longer closing fence closes; a shorter one does not.
    assert_eq!(
        diagram_fences("````mermaid\ngraph TD\n```\n````\n", false).len(),
        1
    );
    // Indented four spaces is not a fence.
    assert!(fences("    ```mermaid\n    graph TD\n    ```\n").is_empty());
}

#[test]
fn alt_text_prefers_given_text_then_generates() {
    let labels: Vec<String> = ["构建", "验收", "生产"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        alt_text(
            Some("发布流程"),
            Some("c"),
            Some("t"),
            DiagramType::Flowchart,
            &labels
        ),
        "发布流程"
    );
    assert_eq!(
        alt_text(
            None,
            Some("说明"),
            Some("t"),
            DiagramType::Flowchart,
            &labels
        ),
        "说明"
    );
    assert_eq!(
        alt_text(None, None, Some("标题"), DiagramType::Flowchart, &labels),
        "标题"
    );
    assert_eq!(
        alt_text(None, None, None, DiagramType::Flowchart, &labels),
        "流程图：3 个节点。构建 → 验收 → 生产"
    );
    assert_eq!(
        alt_text(None, None, None, DiagramType::Sequence, &labels),
        "时序图：3 个节点。构建、验收、生产"
    );
    let many: Vec<String> = (0..200).map(|i| format!("第{i}步")).collect();
    let long = alt_text(None, None, None, DiagramType::Gantt, &many);
    assert_eq!(long.chars().count(), 301);
    assert!(long.ends_with('…'));
}

#[test]
fn nodes_sort_in_source_order() {
    let src = "flowchart LR\n  b[乙] --> a[甲]\n  a --> ab[甲乙]";
    let sorted = source_order(src, &[node("a", "甲"), node("ab", "甲乙"), node("b", "乙")]);
    let ids: Vec<&str> = sorted.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, ["b", "a", "ab"]);
}

#[test]
fn error_kinds_map_reasons_and_refusals() {
    assert_eq!(ErrorKind::from_reason(FailReason::Parse), ErrorKind::Failed);
    assert_eq!(
        ErrorKind::from_reason(FailReason::Timeout),
        ErrorKind::Failed
    );
    assert_eq!(
        ErrorKind::from_reason(FailReason::Internal),
        ErrorKind::Failed
    );
    assert_eq!(
        ErrorKind::from_reason(FailReason::TooBig),
        ErrorKind::TooBig
    );
    assert_eq!(
        ErrorKind::from_reason(FailReason::Unsupported),
        ErrorKind::Unsupported
    );
    assert_eq!(
        ErrorKind::from_refusal(&Refusal::TooLong),
        ErrorKind::TooBig
    );
    assert_eq!(
        ErrorKind::from_refusal(&Refusal::Forbidden("click ")),
        ErrorKind::Forbidden
    );
    assert_eq!(
        ErrorKind::from_refusal(&Refusal::Unsupported("pie".into())),
        ErrorKind::Unsupported
    );
}

#[test]
fn theme_vars_are_greyscale_except_gantt_states() {
    for id in [
        ThemeId::Light,
        ThemeId::Dark,
        ThemeId::LightContrast,
        ThemeId::DarkContrast,
    ] {
        let v = theme_vars(id);
        for c in [
            &v.raised, &v.line, &v.line_2, &v.ink, &v.ink_3, &v.sunken, &v.bg,
        ] {
            assert!(c.starts_with('#') && (c.len() == 7 || c.len() == 9), "{c}");
        }
        assert_ne!(v.live_mix, v.ok_mix);
        assert_ne!(v.ok_mix, v.bad_mix);
    }
    assert_ne!(
        theme_vars(ThemeId::Light).ink,
        theme_vars(ThemeId::Dark).ink
    );
    assert_ne!(
        theme_vars(ThemeId::Light).ink_3,
        theme_vars(ThemeId::LightContrast).ink_3
    );
}

#[test]
fn bundle_hash_is_part_of_every_key() {
    assert_eq!(renderer::MERMAID_SHA256.len(), 64);
    let n = Normalized {
        source: "graph TD".into(),
        kind: DiagramType::Flowchart,
    };
    let k = key_for(&n, ThemeId::Light, INLINE_SCALE);
    assert_eq!(
        k,
        cache::key(
            renderer::RENDERER_VERSION,
            renderer::MERMAID_SHA256,
            ThemeId::Light,
            2,
            "graph TD"
        )
    );
}

// --- F9: the `{block}` segment of node chips (REFS §A2.2) ---

fn fence_source(block_name: Option<&str>) -> DiagramSource {
    DiagramSource {
        message_id: Some("m1".into()),
        topic_id: Some("t1".into()),
        artifact: None,
        block: DiagramBlock::MarkdownFence {
            offset: 0,
            index: Some(2),
        },
        block_name: block_name.map(str::to_string),
    }
}

#[gpui::test]
fn node_chips_omit_the_block_of_a_single_untitled_diagram(cx: &mut gpui::TestAppContext) {
    let t = cx
        .update(|cx| node_target(&fence_source(None), "gray", "灰度", cx))
        .expect("a fence with an index");
    assert_eq!(t.block_id, "md:2");
    assert!(!t.label.contains("图"), "{}", t.label);
    assert!(t.label.ends_with("灰度"), "{}", t.label);
}

#[gpui::test]
fn node_chips_use_the_diagram_title(cx: &mut gpui::TestAppContext) {
    let t = cx
        .update(|cx| node_target(&fence_source(Some("上线流程")), "gray", "灰度", cx))
        .expect("a fence with an index");
    assert!(t.label.ends_with("上线流程 · 灰度"), "{}", t.label);
}

#[test]
fn several_untitled_diagrams_are_named_by_their_type() {
    use crate::views::refs::parts::{BlockKind, block_name};
    assert_eq!(block_name(None, BlockKind::Diagram, 1), None);
    assert_eq!(
        block_name(None, BlockKind::Diagram, 2).as_deref(),
        Some(crate::copy::refs::BLOCK_DIAGRAM)
    );
    assert_eq!(
        block_name(Some("上线流程"), BlockKind::Diagram, 3).as_deref(),
        Some("上线流程")
    );
}

/// The diagram-inline scene's sequence diagram (REFS §B2), as a member posts it.
const SCENE_SEQUENCE: &str = "sequenceDiagram\n  participant C as 客户端\n  participant D as seq 去重\n  participant S as 服务器\n  C->>S: 重连 · 带上最后确认的 seq 1041\n  S->>D: 从 1042 推到 1050\n  D->>C: 9 条新消息放进列表";

/// What the renderer page posts for a request: `rendered` with an SVG of `svg_bytes`, or
/// `failed` with `reason`, run through a bridge with the renderer window's limits.
fn page_outcome(req_id: u64, svg_bytes: usize, reason: Option<(&str, &str)>) -> RenderOutcome {
    let payload = match reason {
        None => {
            let path = "<path d=\"M0 0L1 1\"/>";
            let svg = format!("<svg>{}</svg>", path.repeat(svg_bytes / path.len() + 1));
            serde_json::json!({"reqId": req_id, "svg": svg, "width": 520, "height": 300,
                "type": "sequence", "nodes": [
                    {"id":"S","label":"服务器","kind":"actor","x":380,"y":8,"w":132,"h":40},
                    {"id":"C","label":"客户端","kind":"actor","x":8,"y":8,"w":132,"h":40}]})
        }
        Some((r, d)) => serde_json::json!({"reqId": req_id, "reason": r, "detail": d}),
    };
    let name = if reason.is_some() {
        "failed"
    } else {
        "rendered"
    };
    let raw = serde_json::to_vec(&serde_json::json!({"id": "__ab2", "method": "emitEvent",
        "params": {"name": name, "payload": payload}}))
    .unwrap();
    let o = renderer::bridge().handle(&raw, std::time::Instant::now());
    let Some(artifact_view::bridge::Request::Emit { name, payload, .. }) = o.request else {
        panic!(
            "the renderer bridge dropped the {name} event: {:?}",
            o.reply
        );
    };
    renderer::parse_event(&name, &payload).expect("a render outcome")
}

fn service_setup(cx: &mut gpui::TestAppContext) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    if let Ok(mut d) = preview::CACHE_DIR.lock() {
        *d = Some(dir.path().to_path_buf());
    }
    cx.update(gpui_component::init);
    dir
}

#[gpui::test]
fn a_rendered_scene_diagram_becomes_ready_with_nodes_in_source_order(
    cx: &mut gpui::TestAppContext,
) {
    let _dir = service_setup(cx);
    let n = normalize::normalize(SCENE_SEQUENCE).unwrap();
    cx.update(|cx| {
        assert!(request(&n, INLINE_SCALE, true, cx).is_pending());
        let req_id = start_next_for_test(cx).expect("a queued job");
        // A real sequence diagram gives a 25 KiB SVG.
        let RenderOutcome::Rendered(r) = page_outcome(req_id, 26_000, None) else {
            panic!("not rendered");
        };
        handle(
            Msg::Png {
                req_id,
                result: Ok(b"\x89PNG\r\n\x1a\n".to_vec()),
                rendered: Box::new(r),
            },
            cx,
        );
        let Some(State::Ready(ready)) = peek(&n, INLINE_SCALE, cx) else {
            panic!("the scene diagram is not ready");
        };
        let ids: Vec<&str> = ready.meta.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["C", "S"]);
        assert_eq!(ready.meta.kind, "sequence");
        assert_eq!((ready.meta.width, ready.meta.height), (520., 300.));
    });
}

#[gpui::test]
fn page_failures_show_their_reason(cx: &mut gpui::TestAppContext) {
    let _dir = service_setup(cx);
    for (i, (reason, detail, kind, hint)) in [
        (
            "parse",
            "Parse error on line 2:\n...A -- > B",
            ErrorKind::Failed,
            Some(Hint::Line(2)),
        ),
        (
            "too_big",
            "nodes 151",
            ErrorKind::TooBig,
            Some(Hint::Nodes(151)),
        ),
        ("too_big", "size 5000x300", ErrorKind::TooBig, None),
        ("unsupported", "pie", ErrorKind::Unsupported, None),
    ]
    .into_iter()
    .enumerate()
    {
        let n = normalize::normalize(&format!("flowchart LR\n  A{i}-->B{i}")).unwrap();
        cx.update(|cx| {
            request(&n, INLINE_SCALE, true, cx);
            let req_id = start_next_for_test(cx).expect("a queued job");
            let RenderOutcome::Failed {
                reason: r, detail, ..
            } = page_outcome(req_id, 0, Some((reason, detail)))
            else {
                panic!("not failed");
            };
            let job = take_running(req_id, cx).expect("the running job");
            fail(job, r, &detail, true, cx);
            let Some(State::Failed(k, h)) = peek(&n, INLINE_SCALE, cx) else {
                panic!("{reason}: not failed");
            };
            assert_eq!(k, kind, "{reason}");
            assert_eq!(h, hint, "{reason}");
        });
    }
}

#[test]
fn hints_take_numbers_only() {
    assert_eq!(
        Hint::of(
            FailReason::Parse,
            "Parse error on line 12:\n...客户端 -> > 服务器"
        ),
        Some(Hint::Line(12))
    );
    assert_eq!(Hint::of(FailReason::Parse, "Lexical error"), None);
    assert_eq!(
        Hint::of(FailReason::TooBig, "nodes 64"),
        Some(Hint::Nodes(64))
    );
    assert_eq!(Hint::of(FailReason::TooBig, "edges"), None);
    assert_eq!(Hint::of(FailReason::Timeout, "on line 3"), None);
}
