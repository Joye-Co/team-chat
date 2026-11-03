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
