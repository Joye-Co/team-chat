//! Markdown rendering for message bodies.
//!
//! One place configures gpui-component's `TextView` for chat: selectable
//! text, a copy button on every code block, and links that open in the
//! browser only for safe schemes, `@name` as a mention pill, and code fences
//! read the way Slack reads them. Views call [`message_body`]; they never
//! build a `TextView` for message text themselves.

mod source;

use std::hash::{Hash, Hasher};

use gpui::{
    App, ElementId, InteractiveElement as _, IntoElement, Overflow, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, StyleRefinement, Styled as _, WhiteSpace,
    Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    clipboard::Clipboard,
    text::{
        InlineElement, InlineRenderContext, MarkdownNode, MarkdownParseContext, MarkdownPlugin,
        TextView, TextViewStyle, markdown_ast,
    },
};

pub use source::MentionTarget;
// --- inc-refs: the Markdown block counter (REFS §A2.1) ---
pub use source::{MdBlock, mark_query, top_level_blocks};
// --- end inc-refs ---
pub use source::normalize_fences;
use source::{
    AFTER_MARK, EDITED_SCHEME, MENTION_ME_SCHEME, MENTION_SCHEME, append_edited_mark, link_mentions,
};

use crate::{copy::timeline as t, theme};

/// URL schemes a link in a message may open. Anything else (file:, javascript:,
/// custom app schemes) is not opened.
const SAFE_SCHEMES: &[&str] = &["http", "https", "mailto"];

/// The URL to open for a link in a message, or `None` when the link must not
/// be opened.
pub fn safe_link(url: &str) -> Option<&str> {
    let url = url.trim();
    let (scheme, rest) = url.split_once(':')?;
    if rest.is_empty() {
        return None;
    }
    let scheme = scheme.to_ascii_lowercase();
    if !SAFE_SCHEMES.contains(&scheme.as_str()) {
        return None;
    }
    if scheme != "mailto" && !rest.starts_with("//") {
        return None;
    }
    // Control characters (and therefore newlines) never belong in a link.
    if url.chars().any(char::is_control) {
        return None;
    }
    Some(url)
}

/// `text` cut to at most `max_chars` characters, ending in `…` when cut.
///
/// The one truncation rule for text cut by length (placeholders, excerpts,
/// long logs): the ellipsis is U+2026 and gpui's `trim_before_affix` drops
/// trailing punctuation, opening brackets and a Latin word cut to under four
/// characters before it (DESIGN.md §15). Text cut to a width uses
/// `Type::one_line`, which runs the same rule on measured text.
pub fn clip(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        None => text.to_string(),
        Some((cut, _)) => format!("{}…", gpui::trim_before_affix(text, cut)),
    }
}

/// The members `@name` can mention in a body: `members`, with `me` marked.
pub fn mention_targets(members: &[crate::data::Member], me: Option<&str>) -> Vec<MentionTarget> {
    members
        .iter()
        .map(|m| MentionTarget {
            name: m.name.clone(),
            id: m.id.clone(),
            me: Some(m.id.as_str()) == me,
        })
        .collect()
}

/// A selectable Markdown view of one message body. `id` must be stable for
/// the message (its parsed state is kept under it while the row is visible).
/// `@name` of a member in `mentions` renders as a mention pill.
pub fn message_body(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    mentions: &[MentionTarget],
) -> TextView {
    message_body_marked(id, text, mentions, false, None).0
}

/// [`message_body`] with the `（已编辑）` mark at the end of the last line
/// when `edited`. The flag says whether the mark went inline; when it is
/// `false` for an edited body (it ends in a code block or a table) the
/// caller draws the mark on its own line.
pub fn message_body_marked(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    mentions: &[MentionTarget],
    edited: bool,
    diagrams: Option<&DiagramContext>,
) -> (TextView, bool) {
    message_body_refs(id, text, mentions, edited, diagrams, None)
}

/// [`message_body_marked`] whose tables and code fences can be referenced (引用提问): `refs`
/// names the message. Without it, or when the server does not take references, the body
/// renders as before.
pub fn message_body_refs(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    mentions: &[MentionTarget],
    edited: bool,
    diagrams: Option<&DiagramContext>,
    refs: Option<&crate::views::refs::markdown::RefsBody>,
) -> (TextView, bool) {
    let text: SharedString = text.into();
    let source = link_mentions(&normalize_fences(&text), mentions);
    let marked = edited
        .then(|| append_edited_mark(&source, t::EDITED))
        .flatten();
    let inline = marked.is_some();
    let source = marked.unwrap_or(source);
    let diagram_plugin = diagrams.map(|d| DiagramPlugin {
        allowed: crate::diagrams::diagram_fences(&source, d.streaming),
        message_id: d.message_id.clone(),
        topic_id: d.topic_id.clone(),
        // inc-refs: `md:<index>` of each fence (REFS §A2.1), counted in the stored body.
        blocks: top_level_blocks(&source),
        block_offset: refs.map_or(0, |r| r.block_offset),
    });
    // --- inc-refs: tables and code fences can be referenced ---
    let refs = refs.filter(|r| !r.message_id.is_empty());
    let table_plugin =
        refs.map(|r| crate::views::refs::markdown::TablePlugin::new(r.clone(), &source));
    let code_refs = refs.map(|r| (r.clone(), source.clone()));
    // --- end inc-refs ---
    let view = TextView::markdown(id, source)
        .selectable(true)
        .text_size(crate::theme::type_scale::MESSAGE.0)
        .line_height(crate::theme::type_scale::MESSAGE.1)
        .style(body_style())
        .plugin(MentionPlugin)
        .plugin(EditedPlugin)
        .when_some(diagram_plugin, |view, p| view.plugin(p))
        .when_some(table_plugin, |view, p| view.plugin(p))
        .code_block_actions(move |code, _window, cx| {
            let text = code.code();
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            text.hash(&mut hasher);
            let hash = hasher.finish();
            let copy = Clipboard::new(ElementId::Name(format!("copy-code-{hash:x}").into()))
                .value(text.clone())
                .tooltip(t::COPY_CODE);
            // inc-refs: 问这里 on a code fence references the whole fence.
            let ask = code_refs
                .as_ref()
                .filter(|_| crate::views::refs::enabled(cx))
                .and_then(|(body, source)| {
                    crate::views::refs::markdown::code_target(body, source, &text)
                })
                .map(|target| {
                    crate::views::refs::affordance::hover_button(
                        &format!("code-{}-{hash:x}", target.block_id),
                        std::rc::Rc::new(move |_: &gpui::App| target.clone()),
                        cx,
                    )
                });
            div()
                .h(CODE_ACTIONS)
                .flex()
                .items_center()
                .gap(theme::space::S4)
                .children(ask)
                .child(copy)
                .child(code_edge_fade(&text, cx))
        })
        .on_link_click(|url, _event, _window, cx| {
            if let Some(url) = safe_link(url) {
                cx.open_url(url);
            }
        });
    (view, inline)
}

/// Tables and code blocks in a body (§15): table columns take their content
/// width and the table scrolls sideways once it is wider than the message;
/// code never wraps and scrolls sideways; the code block's copy button sits in
/// its own top strip, clear of the first line.
fn body_style() -> TextViewStyle {
    let mut table = StyleRefinement::default();
    table.overflow.x = Some(Overflow::Scroll);
    TextViewStyle::default()
        .table(table)
        .code_block(code_block_style())
}

/// The top strip of a code block that holds 问这里 and the copy button: the
/// actions sit 8 from the top (gpui-base) and are at most 24 tall, so the
/// strip is 32 and the first line's ascenders are never under the actions'
/// fill (review 3 K-05).
const CODE_STRIP: gpui::Pixels = px(32.);
/// The tallest action in the strip.
const CODE_ACTIONS: gpui::Pixels = px(24.);
/// The bottom padding of a code block.
const CODE_PAD_BOTTOM: gpui::Pixels = px(14.);

/// The code block (§2.4, §2.17; review 2 R1-04):
/// - one width: the in-thread width (640), or the column when it is
///   narrower. A definite 640 capped at 100 % makes a short block as wide as
///   a long one, also when its message has nothing else to set the width;
/// - a long line never wraps: the block scrolls sideways (trackpad, or ⇧ and
///   the wheel) and the line ends 16 before the right edge when scrolled to
///   its end;
/// - the actions (问这里, copy) sit in a 28 strip at the top and stay pinned
///   while the code scrolls: `overflow.y = Scroll` makes the block a scroll
///   viewport with its actions outside the scrolled content (the block has
///   no max height, so it never scrolls vertically and a vertical wheel
///   passes to the thread);
/// - mono 13/20, so the edge fade ([`code_edge_fade`]) knows its height.
fn code_block_style() -> StyleRefinement {
    use crate::ui::native::tokens::{layout, text};
    let mut code = StyleRefinement::default()
        .pt(CODE_STRIP)
        .pb(CODE_PAD_BOTTOM)
        .pr(theme::space::S16)
        .w(layout::IN_THREAD)
        .max_w_full()
        .text_size(text::MONO.0)
        .line_height(text::MONO.1)
        .flex()
        .flex_row()
        .items_start();
    code.overflow.x = Some(Overflow::Scroll);
    code.overflow.y = Some(Overflow::Scroll);
    code.text.white_space = Some(WhiteSpace::Nowrap);
    code
}

/// The scroll cue of a code block (§2.17 "never a hard clip"): a 16 fade
/// from transparent to the block fill over the block's right padding, from
/// under the action strip to the bottom padding. A line that fits ends
/// before it (the 16 padding), so the fade shows only on a line that runs
/// past the edge, where it softens the cut into a cue that the block
/// scrolls. It is drawn from the pinned actions, so it stays at the edge
/// while the code scrolls.
fn code_edge_fade(code: &str, cx: &App) -> impl IntoElement {
    use crate::ui::native::tokens::text;
    let lines = code.trim_end_matches('\n').lines().count().max(1) as f32;
    let fill = gpui_component::ActiveTheme::theme(cx).muted;
    // The actions sit 8 in from the block's top and right edges (gpui-base);
    // the fade starts at the end of the strip and stops at the last line.
    let inset = px(8.);
    div()
        .absolute()
        .top(CODE_STRIP - inset)
        // Inside the block's 1 px border.
        .right(px(1.) - inset)
        .w(theme::space::S16)
        .h(text::MONO.1 * lines)
        .bg(gpui::linear_gradient(
            90.,
            gpui::linear_color_stop(fill.opacity(0.), 0.),
            gpui::linear_color_stop(fill, 1.),
        ))
}

// --- v11/inc-diagrams: ```mermaid fences drawn as diagrams (REFS §B1) ---
/// The message a body belongs to, for diagrams: refs need its id, and a streaming body
/// shows its fences as code until the final text arrives.
#[derive(Clone, Debug)]
pub struct DiagramContext {
    pub message_id: String,
    pub topic_id: String,
    pub streaming: bool,
}

/// Draws the first [`crate::diagrams::MAX_PER_MESSAGE`] closed ```mermaid fences of a
/// finished body as diagrams; every other fence stays a code block.
struct DiagramPlugin {
    /// Byte offsets of the opening lines of the fences to draw.
    allowed: Vec<usize>,
    message_id: String,
    topic_id: String,
    blocks: Vec<(MdBlock, std::ops::Range<usize>)>,
    block_offset: usize,
}

struct DiagramNode {
    source: String,
    offset: usize,
    index: Option<usize>,
    /// Diagrams drawn in this body (the `{block}` segment is omitted when there is one).
    count: usize,
    message_id: String,
    topic_id: String,
}

impl MarkdownPlugin for DiagramPlugin {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "diagram"
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        cx: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let markdown_ast::Node::Code(code) = node else {
            return None;
        };
        if code.meta.is_some() || !crate::diagrams::is_diagram_info(code.lang.as_deref()?) {
            return None;
        }
        let at = cx.offset() + node.position()?.start.offset;
        let offset = *self.allowed.iter().find(|a| at >= **a && at <= **a + 3)?;
        Some(
            MarkdownNode::new(
                "diagram",
                DiagramNode {
                    source: code.value.clone(),
                    offset,
                    index: self
                        .blocks
                        .iter()
                        .position(|(_, r)| r.start <= at && at < r.end.max(r.start + 1))
                        .map(|i| i + self.block_offset),
                    count: self.allowed.len(),
                    message_id: self.message_id.clone(),
                    topic_id: self.topic_id.clone(),
                },
            )
            .text(code.value.clone()),
        )
    }

    fn render(&self, node: &MarkdownNode, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(d) = node.data::<DiagramNode>() else {
            return div().into_any_element();
        };
        let spec = crate::diagrams::view::InlineSpec {
            id: format!("{}-{}", d.message_id, d.offset).into(),
            source: d.source.clone(),
            title: None,
            caption: None,
            alt: None,
            origin: crate::diagrams::DiagramSource {
                message_id: Some(d.message_id.clone()),
                topic_id: Some(d.topic_id.clone()),
                artifact: None,
                block: crate::diagrams::DiagramBlock::MarkdownFence {
                    offset: d.offset,
                    index: d.index,
                },
                block_name: crate::views::refs::parts::block_name(
                    None,
                    crate::views::refs::parts::BlockKind::Diagram,
                    d.count,
                ),
            },
        };
        div()
            .w_full()
            .py(theme::space::S4)
            .child(crate::diagrams::view::inline(spec, window, cx))
            .into_any_element()
    }
}
// --- end v11/inc-diagrams ---

/// Renders the `（已编辑）` link that [`append_edited_mark`] puts after the
/// last line: `meta` 12/16 `text-3`, 4 pt after the text, on the line's
/// baseline. It is not a link.
struct EditedPlugin;

impl MarkdownPlugin for EditedPlugin {
    fn name(&self) -> &str {
        "edited"
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        _: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let markdown_ast::Node::Link(link) = node else {
            return None;
        };
        if link.url != EDITED_SCHEME {
            return None;
        }
        Some(MarkdownNode::new("edited", ()).text(t::EDITED))
    }

    fn render_inline(
        &self,
        node: &MarkdownNode,
        context: &InlineRenderContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<InlineElement> {
        node.data::<()>()?;
        let k = theme::colors(cx);
        let size = theme::type_scale::META.0;
        let text_system = window.text_system();
        let font_id = text_system.resolve_font(&context.text_style().font());
        let ascent = text_system.ascent(font_id, size);
        let descent = text_system.descent(font_id, size).abs();
        let mark = div()
            .ml(theme::space::S4)
            .text_size(size)
            .line_height(ascent + descent)
            .text_color(k.text_3)
            .child(node.as_text().to_string());
        Some(InlineElement::new(mark).with_baseline(ascent))
    }
}

/// A mention in a body.
struct Mention {
    id: String,
    me: bool,
    /// The full-width mark right before the mention, if any.
    after: Option<char>,
}

/// Renders mention links (`[@name](<mention:id>)`, made by
/// [`link_mentions`]) as foreground 500 text; a mention of me also sits on
/// the muted fill, radius 4, padding 0 2. Clicking one opens the member.
struct MentionPlugin;

impl MarkdownPlugin for MentionPlugin {
    fn name(&self) -> &str {
        "mention"
    }

    fn parse(
        &self,
        node: &markdown_ast::Node,
        _: &MarkdownParseContext<'_>,
    ) -> Option<MarkdownNode> {
        let markdown_ast::Node::Link(link) = node else {
            return None;
        };
        let (target, me) = if let Some(rest) = link.url.strip_prefix(MENTION_ME_SCHEME) {
            (rest, true)
        } else {
            (link.url.strip_prefix(MENTION_SCHEME)?, false)
        };
        let (id, after) = match target.split_once(AFTER_MARK) {
            Some((id, mark)) => (id, mark.chars().next()),
            None => (target, None),
        };
        let label: String = link
            .children
            .iter()
            .map(|child| match child {
                markdown_ast::Node::Text(text) => text.value.as_str(),
                _ => "",
            })
            .collect();
        Some(
            MarkdownNode::new(
                "mention",
                Mention {
                    id: id.to_string(),
                    me,
                    after,
                },
            )
            .text(label),
        )
    }

    fn render_inline(
        &self,
        node: &MarkdownNode,
        context: &InlineRenderContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<InlineElement> {
        let mention = node.data::<Mention>()?;
        // A mention is foreground 500 (COMPLETENESS.md D3, §1.3: no state
        // color on text, review 2 R1-05); a mention of me also gets the
        // muted fill, so it still stands out without a color.
        let k = crate::ui::native::native_colors(cx);
        // The text covers the font's ascent and descent and sits on the
        // line's baseline.
        let text_system = window.text_system();
        let font_id = text_system.resolve_font(&context.text_style().font());
        let ascent = text_system.ascent(font_id, context.font_size());
        // CoreText reports the descent below the baseline as a negative number.
        let descent = text_system.descent(font_id, context.font_size()).abs();
        // A full-width mark right before the pill ends its text run, and
        // CoreText sets a run-final mark half wide: take the rest back.
        let gap = mention.after.map_or(gpui::Pixels::ZERO, |mark| {
            let style = context.text_style();
            let width = |t: String| {
                let run = style.to_run(t.len());
                text_system
                    .shape_line(t.into(), context.font_size(), &[run], None)
                    .width
            };
            let x = width("x".into());
            let inside = width(format!("{mark}x")) - x;
            let at_end = width(mark.to_string());
            (inside - at_end).max(gpui::Pixels::ZERO)
        });
        let member = mention.id.clone();
        let at = node.source_range().map_or(0, |r| r.start);
        let pill = div()
            .id(ElementId::Name(format!("mention-{member}-{at}").into()))
            .ml(gap)
            .text_color(k.label)
            .font_weight(theme::MEDIUM_WEIGHT)
            .line_height(ascent + descent)
            .when(mention.me, |el| {
                el.bg(k.fill)
                    .rounded(crate::ui::native::tokens::radius::CHECKBOX)
                    .px(theme::space::S2)
            })
            .cursor_pointer()
            .when(!mention.me, move |el| {
                el.on_click(move |_, _, cx| crate::views::nav::open_dm_with(member.clone(), cx))
            })
            .child(node.as_text().to_string());
        Some(InlineElement::new(pill).with_baseline(ascent))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_search_hit_marks_its_words_outside_code() {
        assert_eq!(
            super::mark_query("压测消息：`压测` 和压测", "压测"),
            "<b><u>压测</u></b>消息：`压测` 和<b><u>压测</u></b>"
        );
        assert_eq!(
            super::mark_query("```\n压测\n```\n", "压测"),
            "```\n压测\n```\n"
        );
        assert_eq!(
            super::mark_query("see https://x/压测", "压测"),
            "see https://x/压测"
        );
    }

    use super::safe_link;

    #[test]
    fn opens_only_web_and_mail_links() {
        assert_eq!(
            safe_link("https://example.com/a?b=1"),
            Some("https://example.com/a?b=1")
        );
        assert_eq!(safe_link("HTTP://example.com"), Some("HTTP://example.com"));
        assert_eq!(
            safe_link("mailto:a@example.com"),
            Some("mailto:a@example.com")
        );
        assert_eq!(safe_link("javascript:alert(1)"), None);
        assert_eq!(safe_link("file:///etc/passwd"), None);
        assert_eq!(safe_link("someapp://open"), None);
        assert_eq!(safe_link("https:example.com"), None);
        assert_eq!(safe_link("https://a\nb"), None);
        assert_eq!(safe_link("example.com"), None);
        assert_eq!(safe_link("http:"), None);
    }
}

#[cfg(test)]
mod text_tests;

/// Whether the first strong letter of `text` is right to left (Hebrew,
/// Arabic and their presentation forms): the paragraph's direction under
/// `dir="auto"` (F15).
pub fn starts_rtl(text: &str) -> bool {
    text.chars().find(|c| c.is_alphabetic()).is_some_and(|c| {
        matches!(c as u32,
                0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF)
    })
}

#[cfg(test)]
mod rtl_tests {
    #[test]
    fn the_first_strong_letter_sets_the_direction() {
        assert!(super::starts_rtl("مرحبا بالعالم 123"));
        assert!(super::starts_rtl("12 שלום"));
        assert!(!super::starts_rtl("你好 مرحبا"));
        assert!(!super::starts_rtl("hello"));
    }
}
