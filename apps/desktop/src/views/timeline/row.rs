//! One message row: avatar, name, 岗位, time, states, extras and the
//! Markdown body. Consecutive messages of one author within five minutes
//! share one header.

use gpui::{
    AnyElement, App, Context, ElementId, FontWeight, Hsla, InteractiveElement as _, IntoElement,
    ParentElement, Pixels, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _,
    button::{Button, ButtonVariants as _},
    input::Textarea,
    menu::ContextMenuExt as _,
};

use super::{TimelineView, extras, model};
use crate::{
    copy::{common, timeline as t},
    data::{Member, Message, SendState},
    markdown,
    theme::{self, Type as _, space},
    ui::{self, Lucide},
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
        let body_hidden = extras::hides_body(&message, &ctx);

        let body: AnyElement = if deleted {
            div()
                .t_ui()
                .text_color(colors.text_3)
                .child(t::DELETED)
                .into_any_element()
        } else if let Some(input) = editing {
            self.render_editor(&input, cx)
        } else if body_hidden {
            div().into_any_element()
        } else if message.body.is_empty() && message.streaming {
            div()
                .t_callout()
                .text_color(colors.text_3)
                .child(t::STREAMING)
                .into_any_element()
        } else {
            markdown::message_body("body", message.body.clone())
                .w_full()
                .into_any_element()
        };

        let state_line = match message.send_state {
            SendState::Sent => None,
            SendState::Pending => Some(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S4)
                    .t_caption()
                    .text_color(colors.text_3)
                    .child(ui::icon(Lucide::Clock, theme::icon::CHIP, colors.text_3))
                    .child(t::PENDING)
                    .into_any_element(),
            ),
            SendState::Failed => Some(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S6)
                    .t_callout()
                    .text_color(colors.danger_text)
                    .child(ui::icon(Lucide::CircleAlert, theme::icon::SMALL, colors.danger_text))
                    .child(t::FAILED)
                    .child(
                        ui::text_button("retry", t::RETRY)
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
                .h(px(18.))
                .min_w(px(0.))
                .flex()
                .items_center()
                .gap(space::S6)
                .child(
                    div()
                        .flex_shrink(1.)
                        .min_w(px(0.))
                        .t_ui()
                        .font_weight(FontWeight::SEMIBOLD)
                        .one_line()
                        .child(author.as_ref().map(|m| m.name.clone()).unwrap_or_default()),
                )
                .when_some(author.as_ref().and_then(|a| a.role.clone()), |el, role| {
                    el.child(
                        div()
                            .flex_shrink_0()
                            .max_w(px(160.))
                            .t_callout()
                            .text_color(colors.text_2)
                            .one_line()
                            .child(role),
                    )
                })
                .child(
                    div()
                        .flex_none()
                        .t_caption()
                        .font_weight(FontWeight::NORMAL)
                        .tnum()
                        .text_color(colors.text_3)
                        .child(time_text(message.created_at)),
                )
                .when(message.edited_at.is_some() && !deleted, |el| {
                    el.child(
                        div()
                            .flex_none()
                            .t_caption()
                            .font_weight(FontWeight::NORMAL)
                            .text_color(colors.text_3)
                            .child(t::EDITED),
                    )
                })
        });
        // A grouped row shows only its marks (edited) next to the body.
        let grouped_marks = (grouped && message.edited_at.is_some() && !deleted).then(|| {
            div()
                .t_caption()
                .font_weight(FontWeight::NORMAL)
                .text_color(colors.text_3)
                .child(t::EDITED)
        });

        let gutter = if grouped {
            div().w(theme::AVATAR_SIZE).flex_none().into_any_element()
        } else {
            // Presence dot at the bottom left (the bot mark is at the right).
            let online = author.as_ref().map(|m| m.online);
            div()
                .relative()
                .flex_none()
                .size(theme::AVATAR_SIZE)
                .child(member_avatar(author.as_ref(), theme::AVATAR_SIZE, cx))
                .when_some(online, |el, online| {
                    el.child(
                        div()
                            .absolute()
                            .bottom(px(-1.))
                            .left(px(-1.))
                            .child(crate::views::presence_dot(online, cx)),
                    )
                })
                .into_any_element()
        };

        let menu_message = message.clone();
        let view = cx.entity().downgrade();
        let can_edit = self.can_edit(&message);
        let can_delete = self.can_delete(&message);
        let row = div()
            .id(ElementId::Name(key.clone().into()))
            .w_full()
            .pl(space::S20)
            .pr(space::S24)
            .pt(if grouped { space::S2 } else { space::S12 })
            .pb(space::S2)
            .flex()
            .gap(space::S12)
            .hover(|s| s.bg(colors.fill_hover))
            .when(self.is_highlighted(&key), |el| {
                el.bg(colors.search_highlight)
            })
            .child(gutter)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .max_w(theme::MEASURE)
                    .flex()
                    .flex_col()
                    .gap(space::S2)
                    .children(header)
                    .children(above)
                    .child(body)
                    .children(grouped_marks)
                    .when(message.streaming && !message.body.is_empty(), |el| {
                        el.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(space::S6)
                                .t_callout()
                                .text_color(colors.text_3)
                                .child(ui::icon(Lucide::Loader, theme::icon::SMALL, colors.text_3))
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
                .child(new_messages_divider(colors.new_divider, colors.danger_text))
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
fn new_messages_divider(line: Hsla, label: Hsla) -> impl IntoElement {
    div()
        .pl(space::S20)
        .pr(space::S24)
        .pt(space::S14)
        .flex()
        .items_center()
        .gap(space::S8)
        .t_section()
        .text_color(label)
        .child(div().flex_1().h(theme::HAIRLINE * 2.).bg(line))
        .child(t::NEW_MESSAGES)
}

/// The timeline avatar (`ui::MemberAvatar`).
pub fn member_avatar(member: Option<&Member>, size: Pixels, _cx: &App) -> impl IntoElement {
    crate::ui::MemberAvatar::new(member, size)
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
