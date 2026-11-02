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
    App, ElementId, InteractiveElement as _, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::{
    clipboard::Clipboard,
    text::{
        InlineElement, InlineRenderContext, MarkdownNode, MarkdownParseContext, MarkdownPlugin,
        TextView, markdown_ast,
    },
};

pub use source::MentionTarget;
use source::{AFTER_MARK, MENTION_ME_SCHEME, MENTION_SCHEME, link_mentions, normalize_fences};

use crate::{
    copy::timeline as t,
    theme::{self, Type as _},
};

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
    let text: SharedString = text.into();
    let source = link_mentions(&normalize_fences(&text), mentions);
    TextView::markdown(id, source)
        .selectable(true)
        .text_size(crate::theme::type_scale::MESSAGE.0)
        .line_height(crate::theme::type_scale::MESSAGE.1)
        .plugin(MentionPlugin)
        .code_block_actions(|code, _window, _cx| {
            let text = code.code();
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            text.hash(&mut hasher);
            Clipboard::new(ElementId::Name(
                format!("copy-code-{:x}", hasher.finish()).into(),
            ))
            .value(text)
            .tooltip(t::COPY_CODE)
        })
        .on_link_click(|url, _event, _window, cx| {
            if let Some(url) = safe_link(url) {
                cx.open_url(url);
            }
        })
}

/// A mention in a body.
struct Mention {
    id: String,
    me: bool,
    /// The full-width mark right before the mention, if any.
    after: Option<char>,
}

/// Renders mention links (`[@name](<mention:id>)`, made by
/// [`link_mentions`]) as pills: `mention-bg` with `accent-text`, or `me-bg`
/// with `me-text` for the viewer; radius 4, padding 0 2 (the mark padding of §8.6), weight 500 (§2.4,
/// §4.3). Clicking one opens the member.
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
        let k = theme::colors(cx);
        let (bg, fg) = if mention.me {
            (k.me_bg, k.me_text)
        } else {
            (k.mention_bg, k.accent_text)
        };
        // The pill covers the font's ascent and descent, like an inline span
        // background, and sits on the line's baseline.
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
            .px(theme::space::S2)
            .rounded(theme::radius::MARK)
            .bg(bg)
            .text_color(fg)
            .medium()
            .line_height(ascent + descent)
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
