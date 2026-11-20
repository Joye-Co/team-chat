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
    ui::native::tokens::space,
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
    &ThreadExtra,
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
        let alone = message.body.trim().is_empty();
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
        // A 话题 started from a message has no words of its own: the quote is
        // the whole first message, so it is drawn as one: the body recipe
        // (14/22, Markdown), all of it, behind the quote bar. The author is
        // named only when it is not the sender, whose name is one line above.
        if alone && let Some(q) = quoted.as_ref().filter(|q| !q.deleted) {
            let name = (q.author_id != message.author_id)
                .then(|| ctx.member(&q.author_id).map(|m| m.name.clone()))
                .flatten();
            let body = crate::markdown::message_body(
                gpui::ElementId::Name(format!("quote-body-{}", message.id).into()),
                q.body.clone(),
                &crate::markdown::mention_targets(ctx.members, ctx.me),
            )
            .w_full()
            .text_color(colors.text_1);
            return Some(
                div()
                    .mb(space::S4)
                    .pl(space::S12)
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
                    .child(body)
                    .into_any_element(),
            );
        }
        let (name, text) = match quoted {
            Some(q) if q.deleted => (
                ctx.member(&q.author_id).map(|m| m.name.clone()),
                t::RECALLED.to_string(),
            ),
            Some(q) => (
                ctx.member(&q.author_id).map(|m| m.name.clone()),
                quote_preview(&q.body),
            ),
            None => (None, t::QUOTE_UNAVAILABLE.to_string()),
        };
        Some(
            div()
                .mb(space::S4)
                .pl(space::S12)
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

/// Most 话题 chips under one 群聊 message; the rest is one line of words.
const THREAD_CHIPS: usize = 3;

/// Under a 群聊 message that a 话题 was opened from (开个话题): one chip per 话题 with its title
/// and message count, a click opens it. Reads the cache only.
pub struct ThreadExtra;

impl MessageExtra for ThreadExtra {
    fn render(
        &self,
        message: &Message,
        ctx: &ExtrasContext,
        _window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        use gpui::{ElementId, InteractiveElement as _, StatefulInteractiveElement as _};

        use crate::ui::{self, native::NativeType as _, sf_symbols::Sym};
        if message.id.is_empty() || !crate::data::is_main_topic(&message.topic_id) {
            return None;
        }
        let topics = ctx
            .provider
            .topics_from_message(&message.group_id, &message.id);
        if topics.is_empty() {
            return None;
        }
        let k = theme::colors(cx);
        let n = ui::native::native_colors(cx);
        let mut list = div().mt(space::S8).flex().flex_col().items_start().gap(space::S4);
        for topic in topics.iter().take(THREAD_CHIPS) {
            let (group, id) = (topic.group_id.clone(), topic.id.clone());
            let meta = crate::copy::fill(
                t::THREAD_MESSAGES,
                &[("n", &topic.message_count.max(1).to_string())],
            );
            list = list.child(
                div()
                    .id(ElementId::Name(format!("thread-{}-{}", message.id, topic.id).into()))
                    .max_w_full()
                    .h(px(32.))
                    .px(space::S8)
                    .rounded(ui::native::tokens::radius::CONTAINER)
                    .border_1()
                    .border_color(n.separator)
                    .bg(n.content)
                    .cursor_pointer()
                    .hover(|d| d.bg(k.hover))
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .child(ui::icon(Sym::OpenTopic, theme::icon::HEADER, k.text_2))
                    .child(
                        div()
                            .min_w_0()
                            .n_subheadline()
                            .text_color(n.label)
                            .one_line()
                            .child(topic.title.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .n_subheadline()
                            .text_color(n.secondary)
                            .child(meta),
                    )
                    .child(ui::icon(Sym::ChevronRight, theme::icon::HEADER, k.text_2))
                    .on_click(move |_, _, cx| {
                        crate::views::nav::open_topic(group.clone(), id.clone(), cx)
                    }),
            );
        }
        if topics.len() > THREAD_CHIPS {
            list = list.child(
                div()
                    .n_subheadline()
                    .text_color(n.secondary)
                    .child(crate::copy::fill(
                        t::THREAD_MORE,
                        &[("n", &(topics.len() - THREAD_CHIPS).to_string())],
                    )),
            );
        }
        Some(list.into_any_element())
    }
}

/// The text lines of a body without their leading Markdown marks.
fn text_lines(body: &str) -> impl Iterator<Item = &str> {
    body.lines()
        .map(|l| l.trim().trim_start_matches(['#', '>', '-', '*', '`', ' ']))
        .filter(|l| !l.is_empty())
}

/// The first non-empty line of a body without leading Markdown marks, cut to
/// [`QUOTE_CHARS`]. It ends with `…` when the body goes on, so a cut quote never reads
/// as the whole message.
pub fn quote_preview(body: &str) -> String {
    let mut lines = text_lines(body);
    let line = lines.next().unwrap_or_default();
    let mut text = crate::markdown::clip(line, QUOTE_CHARS);
    if lines.next().is_some() && !text.ends_with('…') {
        text.push('…');
    }
    text
}

/// The first `max` text lines of a body in plain words, one per line; `…` ends the last
/// one when the body goes on.
pub fn quote_lines(body: &str, max: usize) -> String {
    // Plain words: inline Markdown marks (`**`, backticks) are not drawn here.
    let mut lines = text_lines(body)
        .map(|l| crate::views::search::plain::plain(l).trim().to_string())
        .filter(|l| !l.is_empty());
    let mut shown: Vec<String> = lines
        .by_ref()
        .take(max)
        .map(|l| crate::markdown::clip(&l, QUOTE_CHARS))
        .collect();
    if lines.next().is_some()
        && let Some(last) = shown.last_mut()
        && !last.ends_with('…')
    {
        last.push('…');
    }
    shown.join("\n")
}

#[cfg(test)]
mod tests {
    use super::{quote_lines, quote_preview};

    #[test]
    fn preview_takes_the_first_text_line() {
        // A body that goes on says so.
        assert_eq!(quote_preview("\n## 标题\n正文"), "标题…");
        assert_eq!(quote_preview("只有一行"), "只有一行");
        assert_eq!(quote_lines("# 一\n\n二\n- 三", 2), "一\n二…");
        assert_eq!(quote_lines("一\n二", 6), "一\n二");
        assert_eq!(quote_preview("```\ncargo test\n```"), "cargo test");
        let long = "字".repeat(200);
        assert_eq!(quote_preview(&long).chars().count(), 121);
    }
}
