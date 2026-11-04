//! Extension point: extra content attached to a message row.
//!
//! A row renders, in order: header, the extras placed [`Placement::AboveBody`]
//! (a quote), the Markdown body, then the extras placed
//! [`Placement::BelowBody`] (attachments, image previews, 产出 references,
//! cards). Each extra is one [`MessageExtra`] in [`REGISTRY`].
//!
//! To add one: write a unit struct in your own module that implements
//! [`MessageExtra`], and add `&your_module::YourExtra` to [`REGISTRY`]. The
//! renderer runs for every visible row on every frame, so it must be cheap and
//! must never block: read snapshots from the provider (which never waits) and
//! return `None` when the message has nothing for you. A row's height is
//! measured with its extras, so content that changes size later (an image
//! that finishes loading) must keep a fixed box or ask the timeline to
//! remeasure through a provider change for that topic.

use std::sync::Arc;

use gpui::{AnyElement, App, IntoElement, ParentElement, Styled, Window, div, px};

use crate::{
    copy::timeline as t,
    data::{ChatProvider, Member, Message},
    theme::{self, Type as _},
};

/// What an extra can read while it renders.
pub struct ExtrasContext<'a> {
    pub provider: &'a Arc<dyn ChatProvider>,
    pub members: &'a [Member],
    /// The messages in the timeline window, oldest first (to find a quoted
    /// message without asking the provider).
    pub window_messages: &'a [Message],
    /// My member id.
    pub me: Option<&'a str>,
}

impl ExtrasContext<'_> {
    pub fn member(&self, id: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.id == id)
    }

    pub fn message(&self, id: &str) -> Option<&Message> {
        self.window_messages.iter().find(|m| m.id == id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    AboveBody,
    BelowBody,
}

/// Extra content for a message row.
pub trait MessageExtra: Sync {
    fn placement(&self) -> Placement {
        Placement::BelowBody
    }

    /// True when this extra shows the message in place of its Markdown body
    /// (a card message, whose body is only the plain-text fallback).
    fn hides_body(&self, _message: &Message, _ctx: &ExtrasContext) -> bool {
        false
    }

    /// This extra draws the message's files (the v2 timeline can draw its
    /// own file cards instead).
    fn draws_files(&self) -> bool {
        false
    }

    /// The element to show for `message`, or `None`. Deleted messages are
    /// never passed in.
    fn render(
        &self,
        message: &Message,
        ctx: &ExtrasContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement>;
}

/// Every extra, in display order within its placement.
pub static REGISTRY: &[&dyn MessageExtra] = &[
    &QuoteExtra,
    // --- inc-refs: the references of a message above its body (引用提问).
    &crate::views::refs::chips::RefsExtra,
    // --- end inc-refs
    &crate::views::cards::CardExtra,
    &crate::views::files::extra::AttachmentsExtra,
    &crate::views::artifacts::extra::ArtifactRefsExtra,
];

/// The extras of `message` for one placement.
pub fn render(
    placement: Placement,
    message: &Message,
    ctx: &ExtrasContext,
    window: &mut Window,
    cx: &mut App,
) -> Vec<AnyElement> {
    if message.deleted_at.is_some() {
        return Vec::new();
    }
    REGISTRY
        .iter()
        .filter(|extra| extra.placement() == placement)
        .filter_map(|extra| extra.render(message, ctx, window, cx))
        .collect()
}

/// [`render`] without the extra that draws files (the v2 timeline draws its
/// own file cards).
pub fn render_without_files(
    placement: Placement,
    message: &Message,
    ctx: &ExtrasContext,
    window: &mut Window,
    cx: &mut App,
) -> Vec<AnyElement> {
    if message.deleted_at.is_some() {
        return Vec::new();
    }
    REGISTRY
        .iter()
        .filter(|extra| extra.placement() == placement && !extra.draws_files())
        .filter_map(|extra| extra.render(message, ctx, window, cx))
        .collect()
}

/// True when an extra shows `message` in place of its body.
pub fn hides_body(message: &Message, ctx: &ExtrasContext) -> bool {
    message.deleted_at.is_none() && REGISTRY.iter().any(|extra| extra.hides_body(message, ctx))
}

/// The quoted message above the body: author and the first line.
pub struct QuoteExtra;

/// Longest quote preview, in characters.
const QUOTE_CHARS: usize = 120;

impl MessageExtra for QuoteExtra {
    fn placement(&self) -> Placement {
        Placement::AboveBody
    }

    fn render(
        &self,
        message: &Message,
        ctx: &ExtrasContext,
        _window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let quoted_id = message.quote_of.as_deref()?;
        let colors = theme::colors(cx);
        let quoted = match ctx.message(quoted_id) {
            Some(q) => Some(crate::data::QuotedMessage {
                author_id: q.author_id.clone(),
                body: q.body.clone(),
                deleted: q.deleted_at.is_some(),
            }),
            // Older than the window: the cache, or the server's excerpt.
            None => ctx
                .provider
                .quoted_message(&message.group_id, &message.id, quoted_id),
        };
        let (name, text) = match quoted {
            Some(q) if q.deleted => (
                ctx.member(&q.author_id).map(|m| m.name.clone()),
                t::DELETED.to_string(),
            ),
            Some(q) => (
                ctx.member(&q.author_id).map(|m| m.name.clone()),
                quote_preview(&q.body),
            ),
            None => (None, t::QUOTE_UNAVAILABLE.to_string()),
        };
        Some(
            div()
                .mb(px(4.))
                .pl(px(10.))
                .border_l_2()
                .border_color(colors.quote_bar)
                .flex()
                .flex_col()
                .min_w(px(0.))
                .children(name.map(|n| {
                    div()
                        .t_callout()
                        .semibold()
                        .text_color(colors.text_1)
                        .one_line()
                        .child(n)
                }))
                .child(
                    div()
                        .min_w(px(0.))
                        .type_role(theme::type_scale::QUOTE)
                        .text_color(colors.text_2)
                        .line_clamp(2)
                        .child(text),
                )
                .into_any_element(),
        )
    }
}

/// The first non-empty line of a body without leading Markdown marks, cut to
/// [`QUOTE_CHARS`].
pub fn quote_preview(body: &str) -> String {
    let line = body
        .lines()
        .map(|l| l.trim().trim_start_matches(['#', '>', '-', '*', '`', ' ']))
        .find(|l| !l.is_empty())
        .unwrap_or_default();
    crate::markdown::clip(line, QUOTE_CHARS)
}

#[cfg(test)]
mod tests {
    use super::quote_preview;

    #[test]
    fn preview_takes_the_first_text_line() {
        assert_eq!(quote_preview("\n## 标题\n正文"), "标题");
        assert_eq!(quote_preview("```\ncargo test\n```"), "cargo test");
        let long = "字".repeat(200);
        assert_eq!(quote_preview(&long).chars().count(), 121);
    }
}
