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

        let is_bot = author.as_ref().is_some_and(|a| a.is_bot());
        let pending = message.send_state == SendState::Pending && !deleted;
        let edited = message.edited_at.is_some() && !deleted;
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
            div().into_any_element()
        } else if model::emoji_only(&message.body) {
            // 1 to 3 emoji only: 32 / 40 (§4).
            div()
                .type_role(theme::type_scale::EMOJI)
                .child(message.body.trim().to_string())
                .into_any_element()
        } else {
            // Code blocks: `code-bg`, radius 8, no border (§13.5).
            markdown::message_body("body", message.body.clone())
                .style(gpui_component::text::TextViewStyle::default().code_block({
                    let mut s = gpui::StyleRefinement::default()
                        .bg(colors.code_bg)
                        .rounded(theme::radius::ROW)
                        .border_0()
                        .border_color(gpui::transparent_black());
                    s.max_size.width = Some(theme::MEASURE.into());
                    s
                }))
                .into_any_element()
        };

        // Marks that follow the text on its last line when they fit (§11):
        // （已编辑）, the pending clock and 发送中, and the streaming caret.
        let mut marks: Vec<AnyElement> = Vec::new();
        if message.streaming {
            marks.push(self.stream_caret(&key, &message, colors.accent, cx));
        }
        if edited && self.editing.as_ref().is_none_or(|e| e.key != key) {
            marks.push(
                div()
                    .flex_none()
                    .type_role(theme::type_scale::CAPTION)
                    .text_color(colors.text_3)
                    .child(t::EDITED)
                    .into_any_element(),
            );
        }
        if pending {
            marks.push(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(space::S4)
                    .t_caption()
                    .text_color(colors.text_3)
                    .child(ui::icon(Lucide::Clock, theme::icon::CHIP, colors.text_3))
                    .child(t::PENDING)
                    .into_any_element(),
            );
        }
        let has_marks = !marks.is_empty();
        let body_line = div()
            .w_full()
            .min_w(px(0.))
            .flex()
            .flex_wrap()
            .items_end()
            .gap_x(space::S6)
            .child(
                div()
                    .min_w(px(0.))
                    .max_w_full()
                    .when(!has_marks, |el| el.w_full())
                    // The text of a message that is still sending is at 60 %.
                    .when(pending, |el| el.opacity(0.6))
                    .child(body),
            )
            .children(marks.into_iter().map(|mark| {
                // The marks sit on the text's last line box (24 pt).
                div()
                    .flex_none()
                    .h(theme::type_scale::MESSAGE.1)
                    .flex()
                    .items_center()
                    .child(mark)
            }));

        let state_line = (message.send_state == SendState::Failed && !deleted).then(|| {
            div()
                .flex()
                .items_center()
                .gap(space::S6)
                .t_callout()
                .text_color(colors.danger_text)
                .child(ui::icon(
                    Lucide::CircleAlert,
                    theme::icon::SMALL,
                    colors.danger_text,
                ))
                .child(t::FAILED)
                .child(
                    div()
                        .ml(space::S4)
                        .flex()
                        .items_center()
                        .gap(space::S10)
                        .child(text_action("retry", t::RETRY, colors.accent_text).on_click(cx.listener({
                            let message = message.clone();
                            move |this, _, _, _| this.retry(&message)
                        })))
                        .child(
                            text_action("discard", t::DISCARD, colors.text_2)
                                .on_click(cx.listener({
                                    let message = message.clone();
                                    move |this, _, _, _| this.discard(&message)
                                })),
                        ),
                )
                .into_any_element()
        });

        // Streaming: `loader` + 正在输出 (bots) or 正在输入 (people), 12 pt
        // `text-3`; the label shimmers (1.6 s), static with reduce motion.
        let streaming_line = message.streaming.then(|| {
            let label = if is_bot { t::STREAMING_BOT } else { t::STREAMING };
            div()
                .h(px(20.))
                .flex()
                .items_center()
                .gap(space::S6)
                .t_callout()
                .text_color(colors.text_3)
                .child(
                    gpui_component::spinner::Spinner::new()
                        .icon(gpui_component::Icon::new(Lucide::Loader))
                        .ease(|t| t)
                        .with_size(theme::icon::SMALL)
                        .color(colors.text_3),
                )
                .child(
                    gpui_component::shimmer::ShimmerText::new(label)
                        .id(ElementId::Name(format!("stream-label-{key}").into()))
                        .duration(theme::motion::SHIMMER),
                )
        });

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
                // 岗位 shrinks before the name (§12).
                .when_some(
                    author
                        .as_ref()
                        .and_then(|a| a.role.clone())
                        .filter(|r| !r.trim().is_empty()),
                    |el, role| {
                        el.child(
                            div()
                                .flex_shrink(2.)
                                .min_w(px(0.))
                                .max_w(px(200.))
                                .t_callout()
                                .text_color(colors.text_2)
                                .one_line()
                                .child(role),
                        )
                    },
                )
                .child(
                    div()
                        .flex_none()
                        .t_caption()
                        .font_weight(FontWeight::NORMAL)
                        .tnum()
                        .text_color(colors.text_3)
                        .child(time_text(message.created_at)),
                )
        });

        let gutter = if grouped {
            div().w(theme::AVATAR_SIZE).flex_none().into_any_element()
        } else {
            // People show an online dot; bots carry the bot mark (§14).
            crate::ui::MemberAvatar::new(author.as_ref(), theme::AVATAR_SIZE)
                .presence(author.as_ref())
                .into_any_element()
        };

        let menu_message = message.clone();
        let view = cx.entity().downgrade();
        let can_edit = self.can_edit(&message);
        let can_delete = self.can_delete(&message);
        // The hover toolbar (§11): quote, copy and more, level 2, 30 pt, at
        // the top right −14 pt. It appears at once (no fade, §10).
        let toolbar = (!deleted && !message.id.is_empty() && self.editing.as_ref().is_none_or(|e| e.key != key))
            .then(|| {
                let forced = self.preview_hover.as_deref() == Some(key.as_str());
                self.render_hover_toolbar(&message, can_edit, can_delete, forced, cx)
            });
        let row = div()
            .id(ElementId::Name(key.clone().into()))
            .group(ROW_GROUP)
            .relative()
            .w_full()
            .pl(space::S20)
            .pr(space::S24)
            .pt(if grouped { space::S2 } else { space::S12 })
            .pb(space::S2)
            .flex()
            .gap(space::S12)
            .hover(|s| s.bg(colors.fill_hover))
            .when(self.preview_hover.as_deref() == Some(key.as_str()), |el| {
                el.bg(colors.fill_hover)
            })
            .when(self.is_highlighted(&key), |el| {
                el.bg(colors.search_highlight)
            })
            .child(gutter)
            .children(toolbar)
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
                    .child(body_line)
                    .children(below)
                    .children(streaming_line)
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

        // A message from someone else fades in over 150 ms in place; the
        // timeline stays pinned to the bottom (§10). Opacity only, so it stays
        // with reduce motion.
        let fresh = self
            .arrived
            .iter()
            .any(|(k, at)| *k == key && at.elapsed() < theme::motion::FADE);
        let row: AnyElement = if fresh {
            crate::ui::motion::fade_in(
                ElementId::Name(format!("arrive-{key}").into()),
                theme::motion::FADE,
                row,
            )
            .into_any_element()
        } else {
            row.into_any_element()
        };

        if divider {
            div()
                .w_full()
                .flex()
                .flex_col()
                .child(new_messages_divider(colors.new_divider, colors.danger_text))
                .child(row)
                .into_any_element()
        } else {
            row
        }
    }

    /// A message edited in place: a field like the composer's (radius 8,
    /// `control-border-focus`, no blue ring; §11), then 取消 and 保存 with
    /// their keys.
    fn render_editor(
        &self,
        input: &gpui::Entity<gpui_component::input::TextareaState>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme::colors(cx);
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(space::S6)
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.cancel_edit(cx);
                }
            }))
            .child(
                div()
                    .w_full()
                    .px(space::S8)
                    .py(space::S4)
                    .rounded(theme::radius::ROW)
                    .border_1()
                    .border_color(colors.control_border_focus)
                    .bg(colors.content)
                    .child(
                        Textarea::new(input)
                            .appearance(false)
                            .xsmall()
                            .t_message(),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(space::S8)
                    .child(
                        Button::new("edit-cancel")
                            .small()
                            .label(common::CANCEL)
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_edit(cx))),
                    )
                    .child(
                        Button::new("edit-save")
                            .small()
                            .primary()
                            .label(common::SAVE)
                            .on_click(cx.listener(|this, _, _, cx| this.save_edit(cx))),
                    ),
            )
            .into_any_element()
    }

    /// Quote, copy and more (the row's menu) over the hovered row.
    fn render_hover_toolbar(
        &self,
        message: &Message,
        can_edit: bool,
        can_delete: bool,
        forced: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use gpui_component::menu::DropdownMenu as _;
        let colors = theme::colors(cx);
        let tool = |id: &'static str, icon: Lucide| {
            ui::icon_button(id, icon)
                .w(px(26.))
                .h(px(26.))
                .rounded(theme::radius::SM)
        };
        let view = cx.entity().downgrade();
        let menu_message = message.clone();
        let quote_message = message.clone();
        let body = message.body.clone();
        div()
            .absolute()
            .top(px(-14.))
            .right(space::S24)
            .when(!forced, |el| el.invisible())
            .group_hover(ROW_GROUP, |s| s.visible())
            .child(
                ui::floating(cx)
                    .h(px(30.))
                    .px(space::S2)
                    .rounded(theme::radius::ROW)
                    .flex()
                    .items_center()
                    .gap(space::S2)
                    .child(
                        tool("hover-quote", Lucide::TextQuote)
                            .tooltip(t::MENU_QUOTE)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.quote(&quote_message, cx)
                            })),
                    )
                    .child(
                        tool("hover-copy", Lucide::Copy)
                            .tooltip(t::MENU_COPY)
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                    body.clone(),
                                ))
                            }),
                    )
                    .child(tool("hover-more", Lucide::Ellipsis).tooltip(t::MORE).dropdown_menu(
                        move |menu, window, cx| {
                            super::menu::build(
                                menu,
                                view.clone(),
                                &menu_message,
                                can_edit,
                                can_delete,
                                window,
                                cx,
                            )
                        },
                    )),
            )
            .text_color(colors.text_2)
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

    /// 删除 on a failed send: drops it before the server has it.
    pub(super) fn discard(&mut self, message: &Message) {
        self.services
            .provider
            .discard_send(&message.group_id, &message.client_msg_id);
    }

    /// The 2 × 16 pt accent caret after streaming text (§13.5): solid while
    /// text arrives, blinking 530 / 530 ms after 1 s without new text, solid
    /// with reduce motion (§10).
    fn stream_caret(
        &mut self,
        key: &str,
        message: &Message,
        color: Hsla,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let len = message.body.len();
        let now = std::time::Instant::now();
        let since = match &self.caret {
            Some((k, l, at)) if k == key && *l == len => now.duration_since(*at),
            _ => {
                self.caret = Some((key.to_string(), len, now));
                std::time::Duration::ZERO
            }
        };
        let caret = div().w(px(2.)).h(px(16.)).rounded(px(1.)).bg(color);
        if cx.reduce_motion() {
            return caret.into_any_element();
        }
        if since < theme::motion::CARET_IDLE {
            // Render again when the caret should start blinking.
            if !self.caret_wake {
                self.caret_wake = true;
                let wait = theme::motion::CARET_IDLE - since;
                let key = key.to_string();
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(wait).await;
                    let _ = this.update(cx, |this, cx| {
                        this.caret_wake = false;
                        this.remeasure_key(&key);
                        cx.notify();
                    });
                })
                .detach();
            }
            return caret.into_any_element();
        }
        use gpui::{Animation, AnimationExt as _};
        caret
            .with_animation(
                ElementId::Name(format!("caret-{key}-{len}").into()),
                Animation::new(theme::motion::CARET_BLINK * 2).repeat(),
                |el, t| el.opacity(if t < 0.5 { 1. } else { 0. }),
            )
            .into_any_element()
    }
}

/// The hover group of a message row (its toolbar shows on hover).
const ROW_GROUP: &str = "message-row";

/// A quiet text action (重试, 删除; §11): the label in its color, no
/// underline, the ghost hover and pressed fills.
pub(crate) fn text_action(
    id: impl Into<ElementId>,
    label: &'static str,
    color: Hsla,
) -> Button {
    // The shared text button, as a ghost: no underline at rest.
    ui::text_button(id, label).ghost().text_color(color)
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
