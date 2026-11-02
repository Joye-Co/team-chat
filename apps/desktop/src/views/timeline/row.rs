//! One message row: avatar, name, 岗位, time, states, extras and the
//! Markdown body. Consecutive messages of one author within five minutes
//! share one header.

use gpui::{
    AnyElement, App, Context, ElementId, FontWeight, Hsla, InteractiveElement as _, IntoElement,
    ParentElement, Pixels, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    input::Textarea,
    menu::ContextMenuExt as _,
};

use super::{TimelineView, extras, model};
use crate::{
    copy::{common, timeline as t},
    data::{Member, Message, SendState},
    markdown, theme,
    views::time_text,
};

impl TimelineView {
    pub(super) fn render_row(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(message) = self.messages.get(ix).cloned() else {
            return div().into_any_element();
        };
        let colors = theme::colors(cx);
        let key = model::key(&message).to_string();
        let divider = self.divider_key.as_deref() == Some(key.as_str()) && ix > 0;
        let grouped =
            !divider && ix > 0 && model::continues_group(&self.messages[ix - 1], &message);
        let author = self
            .members
            .iter()
            .find(|m| m.id == message.author_id)
            .cloned();
        let deleted = message.deleted_at.is_some();
        let editing = self
            .editing
            .as_ref()
            .filter(|e| e.key == key)
            .map(|e| e.input.clone());

        let ctx = extras::ExtrasContext {
            provider: &self.services.provider,
            members: &self.members,
            window_messages: &self.messages,
            me: self.me.as_deref(),
        };
        let above = extras::render(extras::Placement::AboveBody, &message, &ctx, window, cx);
        let below = extras::render(extras::Placement::BelowBody, &message, &ctx, window, cx);

        let body: AnyElement = if deleted {
            div()
                .text_sm()
                .italic()
                .text_color(colors.muted_foreground)
                .child(t::DELETED)
                .into_any_element()
        } else if let Some(input) = editing {
            self.render_editor(&input, cx)
        } else if message.body.is_empty() && message.streaming {
            div()
                .text_sm()
                .text_color(colors.muted_foreground)
                .child(t::STREAMING)
                .into_any_element()
        } else {
            markdown::message_body("body", message.body.clone())
                .w_full()
                .into_any_element()
        };

        let failed = message.send_state == SendState::Failed;
        let state_line = match message.send_state {
            SendState::Sent => None,
            SendState::Pending => Some(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(t::PENDING)
                    .into_any_element(),
            ),
            SendState::Failed => Some(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_xs()
                    .text_color(colors.danger)
                    .child(t::FAILED)
                    .child(
                        Button::new("retry")
                            .xsmall()
                            .ghost()
                            .label(t::RETRY)
                            .on_click(cx.listener({
                                let message = message.clone();
                                move |this, _, _, _| this.retry(&message)
                            })),
                    )
                    .into_any_element(),
            ),
        };

        let header = (!grouped).then(|| {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_xs()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(author.as_ref().map(|m| m.name.clone()).unwrap_or_default()),
                )
                .when_some(author.as_ref().and_then(|a| a.role.clone()), |el, role| {
                    el.child(div().text_color(colors.muted_foreground).child(role))
                })
                .child(
                    div()
                        .text_color(colors.muted_foreground)
                        .child(time_text(message.created_at)),
                )
                .when(message.edited_at.is_some() && !deleted, |el| {
                    el.child(div().text_color(colors.muted_foreground).child(t::EDITED))
                })
        });
        // A grouped row shows only its marks (edited) next to the body.
        let grouped_marks = (grouped && message.edited_at.is_some() && !deleted).then(|| {
            div()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(t::EDITED)
        });

        let gutter = if grouped {
            div().w(theme::AVATAR_SIZE).flex_none().into_any_element()
        } else {
            member_avatar(author.as_ref(), theme::AVATAR_SIZE, cx).into_any_element()
        };

        let menu_message = message.clone();
        let view = cx.entity().downgrade();
        let can_edit = self.can_edit(&message);
        let can_delete = self.can_delete(&message);
        let row = div()
            .id(ElementId::Name(key.clone().into()))
            .w_full()
            .px(px(16.))
            .pt(if grouped { px(2.) } else { px(8.) })
            .pb(px(2.))
            .flex()
            .gap(px(10.))
            .hover(|s| s.bg(colors.row_hover))
            .when(failed, |el| el.bg(colors.danger.opacity(0.06)))
            .child(gutter)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children(header)
                    .children(above)
                    .child(body)
                    .children(grouped_marks)
                    .when(message.streaming && !message.body.is_empty(), |el| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child(t::STREAMING),
                        )
                    })
                    .children(below)
                    .children(state_line),
            )
            .context_menu(move |menu, window, cx| {
                super::menu::build(
                    menu,
                    view.clone(),
                    &menu_message,
                    can_edit,
                    can_delete,
                    window,
                    cx,
                )
            });

        if divider {
            div()
                .w_full()
                .flex()
                .flex_col()
                .child(new_messages_divider(colors.danger))
                .child(row)
                .into_any_element()
        } else {
            row.into_any_element()
        }
    }

    fn render_editor(
        &self,
        input: &gpui::Entity<gpui_component::input::TextareaState>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.cancel_edit(cx);
                }
            }))
            .child(Textarea::new(input))
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        Button::new("edit-save")
                            .xsmall()
                            .primary()
                            .label(common::SAVE)
                            .on_click(cx.listener(|this, _, _, cx| this.save_edit(cx))),
                    )
                    .child(
                        Button::new("edit-cancel")
                            .xsmall()
                            .ghost()
                            .label(common::CANCEL)
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_edit(cx))),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn can_edit(&self, message: &Message) -> bool {
        !message.id.is_empty()
            && message.deleted_at.is_none()
            && message.send_state == SendState::Sent
            && !message.streaming
            && Some(message.author_id.as_str()) == self.me.as_deref()
    }

    pub(super) fn can_delete(&self, message: &Message) -> bool {
        !message.id.is_empty()
            && message.deleted_at.is_none()
            && (Some(message.author_id.as_str()) == self.me.as_deref() || self.i_am_owner)
    }

    pub(super) fn retry(&mut self, message: &Message) {
        self.services
            .provider
            .retry_send(&message.group_id, &message.client_msg_id);
    }
}

/// The line above the first unread message.
fn new_messages_divider(color: Hsla) -> impl IntoElement {
    div()
        .px(px(16.))
        .py(px(4.))
        .flex()
        .items_center()
        .gap(px(8.))
        .text_xs()
        .text_color(color)
        .child(div().flex_1().h(px(1.)).bg(color))
        .child(t::NEW_MESSAGES)
        .child(div().flex_1().h(px(1.)).bg(color))
}

/// Round avatar with the initial on a color picked from the member id; bots
/// get a small mark.
pub fn member_avatar(member: Option<&Member>, size: Pixels, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let palette = [
        theme.blue,
        theme.green,
        theme.magenta,
        theme.cyan,
        theme.yellow,
        theme.red,
    ];
    let color = member
        .map(|m| palette[color_index(&m.id, palette.len())])
        .unwrap_or(theme.muted_foreground);
    let initial: String = member
        .and_then(|m| m.name.chars().next())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    let is_bot = member.is_some_and(|m| m.is_bot());
    let colors = theme::colors(cx);
    div()
        .relative()
        .flex_none()
        .size(size)
        .rounded_full()
        .bg(color.opacity(0.85))
        .text_color(gpui::white())
        .flex()
        .items_center()
        .justify_center()
        .text_size(size * 0.45)
        .font_weight(FontWeight::SEMIBOLD)
        .child(initial)
        .when(is_bot, |el| {
            el.child(
                div()
                    .absolute()
                    .bottom(px(-2.))
                    .right(px(-4.))
                    .px(px(2.))
                    .rounded(px(3.))
                    .bg(colors.foreground)
                    .text_color(colors.background)
                    .text_size(px(8.))
                    .child(common::BOT_MARK),
            )
        })
}

/// A stable index into a palette of `n` colors for a member id.
pub fn color_index(id: &str, n: usize) -> usize {
    // FNV-1a: stable across runs and platforms (unlike the std hasher).
    let mut hash: u32 = 0x811c_9dc5;
    for byte in id.bytes() {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash as usize % n.max(1)
}

#[cfg(test)]
mod tests {
    use super::color_index;

    #[test]
    fn avatar_color_is_stable() {
        assert_eq!(color_index("m-joye", 6), color_index("m-joye", 6));
        assert!(color_index("anything", 6) < 6);
        assert_eq!(color_index("x", 0), 0);
    }
}
