//! Markdown rendering for message bodies.
//!
//! One place configures gpui-component's `TextView` for chat: selectable
//! text, a copy button on every code block, and links that open in the
//! browser only for safe schemes. Views call [`message_body`]; they never
//! build a `TextView` for message text themselves.

use std::hash::{Hash, Hasher};

use gpui::{ElementId, SharedString, Styled as _};
use gpui_component::{clipboard::Clipboard, text::TextView};

use crate::copy::timeline as t;

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

/// A selectable Markdown view of one message body. `id` must be stable for
/// the message (its parsed state is kept under it while the row is visible).
pub fn message_body(id: impl Into<ElementId>, text: impl Into<SharedString>) -> TextView {
    TextView::markdown(id, text)
        .selectable(true)
        .text_size(crate::theme::type_scale::MESSAGE.0)
        .line_height(crate::theme::type_scale::MESSAGE.1)
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
