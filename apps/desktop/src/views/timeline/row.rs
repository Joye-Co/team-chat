//! One timeline row (DESIGN.md §7.5, §8.1): what sits above the message (the
//! topic start block on the first row, a date divider when the day changes,
//! the new-messages divider), then the message: a 36 pt gutter (the avatar,
//! or the hover time of a continuation), the meta line (name, bot role tag,
//! time), the steps row, the body, extras, the send state and the hover
//! toolbar. Consecutive messages of one author within five minutes share one
//! meta line.

use gpui::{
    AnyElement, App, ClipboardItem, Context, ElementId, InteractiveElement as _, IntoElement,
    ParentElement, Pixels, SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _,
    button::{Button, ButtonVariants as _},
    input::Textarea,
    menu::{ContextMenuExt as _, DropdownMenu as _},
};

use super::{TimelineView, extras, model};
use crate::{
    copy::{common, fill, timeline as t},
    data::{Member, Message, SendState, Topic},
    markdown,
    theme::{self, Type as _, space},
    ui::{self, Lucide},
};

/// The row's inset inside the content box: hover fills reach 8 pt past the
/// text (§7.5).
const ROW_INSET: Pixels = px(8.);

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
        let key = model::key(&message).to_string();
        let prev = ix.checked_sub(1).and_then(|i| self.messages.get(i));
        let start = ix == 0 && !self.has_older;
        let new_day = model::starts_day(prev, &message) && !start;
        let divider = self.divider_key.as_deref() == Some(key.as_str()) && ix > 0;
        let grouped =
            !divider && !new_day && prev.is_some_and(|prev| model::continues_group(prev, &message));
        let last = ix + 1 == self.messages.len();

        let mut item = div()
            .w_full()
            .px(theme::CONTENT_GUTTER - ROW_INSET)
            .flex()
            .flex_col()
            .when(last, |el| el.pb(space::S8));
        if ix == 0 {
            // The first message sits 16 pt under the header (§7.5).
            item = item.pt(if start { space::S16 } else { space::S10 });
        }
        if start && let Some(topic) = self.topic.clone() {
            item = item.child(self.render_start(&topic, cx));
        }
        if new_day {
            item = item.child(date_divider(message.created_at, cx));
        }
        if divider {
            item = item.child(new_messages_divider(cx));
        }
        let gap = if grouped {
            None
        } else if start {
            Some(space::S12)
        } else {
            Some(space::S6)
        };
        let message_row = self.render_message(&message, &key, grouped, window, cx);
        item.children(gap.map(|h| div().flex_none().h(h)))
            .child(message_row)
            .into_any_element()
    }

    fn render_message(
        &mut self,
        message: &Message,
        key: &str,
        grouped: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme::colors(cx);
        let author = self
            .members
            .iter()
            .find(|m| m.id == message.author_id)
            .cloned();
        let deleted = message.deleted_at.is_some();
        let hovered = self.hovered.as_deref() == Some(key);
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
        let above = extras::render(extras::Placement::AboveBody, message, &ctx, window, cx);
        let below = extras::render(extras::Placement::BelowBody, message, &ctx, window, cx);
        let body_hidden = extras::hides_body(message, &ctx);
        if !message.id.is_empty()
            && self
                .services
                .provider
                .card(&message.group_id, &message.id)
                .is_some()
        {
            self.card_seen = true;
        }
        let steps = (!deleted && author.as_ref().is_some_and(Member::is_bot))
            .then(|| self.render_steps(message, cx))
            .flatten();

        let body: Option<AnyElement> = if deleted {
            Some(
                div()
                    .t_body()
                    .text_color(colors.text_3)
                    .child(t::DELETED)
                    .into_any_element(),
            )
        } else if let Some(input) = editing {
            Some(self.render_editor(&input, cx))
        } else if body_hidden {
            None
        } else if message.body.is_empty() && message.streaming {
            Some(
                div()
                    .t_body()
                    .text_color(colors.text_3)
                    .child(t::STREAMING)
                    .into_any_element(),
            )
        } else if message.body.is_empty() {
            None
        } else if model::is_jumbo(&message.body) {
            Some(
                div()
                    .type_role(theme::type_scale::EMOJI)
                    .child(message.body.trim().to_string())
                    .into_any_element(),
            )
        } else {
            Some(
                markdown::message_body(
                    ElementId::Name(format!("body-{key}").into()),
                    message.body.clone(),
                )
                .w_full()
                .text_color(colors.text_1)
                .into_any_element(),
            )
        };
        // Sending dims the body (§7.5).
        let body = body.map(|b| {
            div()
                .w_full()
                .when(message.send_state == SendState::Pending, |el| {
                    el.opacity(0.6)
                })
                .child(b)
                .into_any_element()
        });

        let marks = (message.edited_at.is_some() && !deleted).then(|| {
            div()
                .t_meta()
                .text_color(colors.text_3)
                .child(t::EDITED)
                .into_any_element()
        });

        let failed = (message.send_state == SendState::Failed).then(|| {
            div()
                .h(px(22.))
                .flex()
                .items_center()
                .gap(space::S8)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S4)
                        .t_meta()
                        .text_color(colors.danger_text)
                        .child(ui::icon(
                            Lucide::CircleAlert,
                            theme::icon::CHIP,
                            colors.danger_text,
                        ))
                        .child(t::FAILED),
                )
                .child(
                    ui::text_button(ElementId::Name(format!("retry-{key}").into()), t::RETRY, cx)
                        .on_click(cx.listener({
                            let message = message.clone();
                            move |this, _, _, _| this.retry(&message)
                        })),
                )
                .into_any_element()
        });

        let meta = (!grouped).then(|| {
            let name = author.as_ref().map(|m| m.name.clone()).unwrap_or_default();
            div()
                .h(px(22.))
                .min_w(px(0.))
                .flex()
                .items_center()
                .gap(space::S8)
                .child(
                    div()
                        .flex_shrink(1.)
                        .min_w(px(0.))
                        .max_w(px(160.))
                        .t_body()
                        .semibold()
                        .text_color(colors.text_1)
                        .one_line()
                        .child(name),
                )
                .when_some(
                    author
                        .as_ref()
                        .filter(|a| a.is_bot())
                        .and_then(|a| a.role.clone())
                        .filter(|r| !r.trim().is_empty()),
                    |el, role| {
                        el.child(
                            div()
                                .flex_shrink(1.)
                                .min_w(px(40.))
                                .child(ui::RoleTag::new(role)),
                        )
                    },
                )
                .child(
                    div()
                        .flex_none()
                        .t_meta()
                        .tnum()
                        .text_color(colors.text_3)
                        .child(model::clock(message.created_at)),
                )
        });

        let gutter = if grouped {
            div()
                .w(theme::AVATAR_SIZE)
                .flex_none()
                .flex()
                .justify_center()
                .when(hovered, |el| {
                    el.child(
                        div()
                            .t_meta()
                            .line_height(px(22.))
                            .tnum()
                            .text_color(colors.text_3)
                            .child(model::clock(message.created_at)),
                    )
                })
                .into_any_element()
        } else {
            div()
                .w(theme::AVATAR_SIZE)
                .flex_none()
                .child(
                    // People show presence; bot tiles carry no pip in the
                    // timeline (§7.5: no status dot on every bot avatar).
                    ui::MemberAvatar::new(author.as_ref(), theme::AVATAR_SIZE)
                        .presence(author.as_ref().filter(|a| !a.is_bot())),
                )
                .into_any_element()
        };

        let toolbar = (hovered && !deleted && editing_is_none(self, key))
            .then(|| self.render_hover_toolbar(message, key, cx));

        let menu_message = message.clone();
        let view = cx.entity().downgrade();
        let can_edit = self.can_edit(message);
        let can_delete = self.can_delete(message);
        let hover_key: SharedString = key.to_string().into();
        div()
            .id(ElementId::Name(key.to_string().into()))
            .relative()
            .w_full()
            .px(ROW_INSET)
            .pt(if grouped { space::S2 } else { space::S6 })
            .pb(space::S6)
            .rounded(theme::radius::MD)
            .flex()
            .gap(space::S12)
            .when(hovered, |el| el.bg(colors.hover))
            .when(self.is_highlighted(key), |el| el.bg(colors.me_bg))
            .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                let key = hover_key.to_string();
                if *hovering {
                    if this.hovered.as_deref() != Some(key.as_str()) {
                        this.hovered = Some(key);
                        cx.notify();
                    }
                } else if this.hovered.as_deref() == Some(key.as_str()) {
                    this.hovered = None;
                    cx.notify();
                }
            }))
            .child(gutter)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .max_w(theme::MEASURE)
                    .flex()
                    .flex_col()
                    .children(meta)
                    .children(steps)
                    .children(above)
                    .children(body)
                    .children(marks)
                    .children(below)
                    .children(failed),
            )
            .children(toolbar)
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
            })
            .into_any_element()
    }

    /// The hover toolbar (§7.5): a level-1 capsule 18 pt above the row, at
    /// the right edge of the content box: reply (quotes into the composer),
    /// copy as quote and more (the row menu).
    fn render_hover_toolbar(
        &self,
        message: &Message,
        key: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let sent = !message.id.is_empty();
        let menu_message = message.clone();
        let view = cx.entity().downgrade();
        let can_edit = self.can_edit(message);
        let can_delete = self.can_delete(message);
        let tool = |id: &str, glyph: Lucide| {
            ui::icon_button(ElementId::Name(format!("{id}-{key}").into()), glyph)
                .rounded(theme::radius::SM)
        };
        ui::capsule(cx)
            .id(ElementId::Name(format!("hover-bar-{key}").into()))
            .absolute()
            .top(px(-18.))
            .right(ROW_INSET)
            .gap(space::S2)
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .when(sent, |el| {
                el.child(
                    tool("reply", Lucide::Reply)
                        .tooltip(t::REPLY)
                        .on_click(cx.listener({
                            let m = message.clone();
                            move |this, _, _, cx| this.quote(&m, cx)
                        })),
                )
            })
            .child({
                let quoted = quote_markdown(&message.body);
                tool("copy-quote", Lucide::TextQuote)
                    .tooltip(t::COPY_QUOTE)
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(quoted.clone()))
                    })
            })
            .child(
                tool("more", Lucide::Ellipsis)
                    .tooltip(t::MORE)
                    .dropdown_menu(move |menu, window, cx| {
                        super::menu::build(
                            menu,
                            view.clone(),
                            &menu_message,
                            can_edit,
                            can_delete,
                            window,
                            cx,
                        )
                    }),
            )
            .into_any_element()
    }

    /// The topic start block (§8.1): the responsible member at 48 pt, the
    /// title in `display` (two lines at most), `Joye 发起 · Taro 负责 · 今天
    /// 14:05`, and a 1 pt rule under it.
    fn render_start(&self, topic: &Topic, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme::colors(cx);
        let name = |id: &str| {
            self.members
                .iter()
                .find(|m| m.id == id)
                .map(|m| m.name.clone())
                .unwrap_or_default()
        };
        let starter = name(&topic.created_by);
        let people = match &topic.owner_id {
            Some(owner) => fill(
                t::STARTED_LINE,
                &[("starter", &starter), ("owner", &name(owner))],
            ),
            None => fill(t::STARTED_LINE_NO_OWNER, &[("starter", &starter)]),
        };
        let when = match model::local_day(topic.created_at) {
            Some(day) => format!(
                "{} {}",
                model::day_label(day, chrono::Local::now().date_naive()),
                model::clock(topic.created_at)
            ),
            None => String::new(),
        };
        let lead_id = topic.owner_id.as_deref().unwrap_or(&topic.created_by);
        let lead = self.members.iter().find(|m| m.id == lead_id);
        let title = crate::views::sidebar::topic_line(&topic.title);
        div()
            .mx(ROW_INSET)
            .pt(space::S8)
            .pb(space::S16)
            .border_b_1()
            .border_color(colors.border)
            .flex()
            .flex_col()
            .child(
                div()
                    .mb(space::S12)
                    .child(ui::MemberAvatar::new(lead, theme::size::AVATAR_START)),
            )
            .child(
                div()
                    .t_display()
                    .text_color(colors.text_1)
                    .line_clamp(2)
                    .child(title),
            )
            .child(
                div()
                    .mt(space::S8)
                    .min_w(px(0.))
                    .flex()
                    .t_small()
                    .text_color(colors.text_2)
                    .child(div().min_w(px(0.)).one_line().child(people))
                    .when(!when.is_empty(), |el| {
                        el.child(div().flex_none().tnum().child(format!(" · {when}")))
                    }),
            )
            .into_any_element()
    }

    fn render_editor(
        &self,
        input: &gpui::Entity<gpui_component::input::TextareaState>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme::colors(cx);
        div()
            .flex()
            .flex_col()
            .gap(space::S8)
            .py(space::S2)
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.cancel_edit(cx);
                }
            }))
            .child(
                div()
                    .rounded(theme::radius::FIELD)
                    .border_1()
                    .border_color(colors.focus_border)
                    .shadow(colors.focus_halo())
                    .bg(colors.content)
                    .px(space::S12 - crate::ui::composer::EDITOR_INSET)
                    .py(space::S8)
                    .t_body()
                    .child(
                        Textarea::new(input)
                            .appearance(false)
                            .xsmall()
                            .p_0()
                            .t_body(),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(space::S8)
                    .child(
                        Button::new("edit-save")
                            .primary()
                            .label(common::SAVE)
                            .h(theme::size::BUTTON_SM)
                            .on_click(cx.listener(|this, _, _, cx| this.save_edit(cx))),
                    )
                    .child(
                        ui::secondary_button("edit-cancel", common::CANCEL, cx)
                            .h(theme::size::BUTTON_SM)
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

    /// Shows the hover toolbar on row `ix` (screenshot scenes).
    pub fn preview_hover(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.hovered = self.messages.get(ix).map(|m| model::key(m).to_string());
        cx.notify();
    }
}

fn editing_is_none(view: &TimelineView, key: &str) -> bool {
    view.editing.as_ref().is_none_or(|e| e.key != key)
}

/// A message as a Markdown quote, for 复制为引用.
fn quote_markdown(body: &str) -> String {
    body.lines()
        .map(|l| {
            if l.is_empty() {
                ">".to_string()
            } else {
                format!("> {l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The date divider (§7.5): `12/16 500 text-3` centered between two 0.5 pt
/// `sep` lines across the content box; 16 pt above, 4 below.
fn date_divider(at: crate::data::Millis, cx: &App) -> impl IntoElement {
    let colors = theme::colors(cx);
    let label = model::local_day(at)
        .map(|day| model::day_label(day, chrono::Local::now().date_naive()))
        .unwrap_or_default();
    div()
        .mx(ROW_INSET)
        .mt(space::S16)
        .mb(space::S4)
        .h(px(20.))
        .flex()
        .items_center()
        .gap(space::S12)
        .t_meta_medium()
        .text_color(colors.text_3)
        .child(div().flex_1().h(theme::HAIRLINE).bg(colors.sep))
        .child(label)
        .child(div().flex_1().h(theme::HAIRLINE).bg(colors.sep))
}

/// The new-messages divider (§7.5): a `danger` hairline with `新消息`
/// 12/500 `danger-text` at the right.
fn new_messages_divider(cx: &App) -> impl IntoElement {
    let colors = theme::colors(cx);
    div()
        .mx(ROW_INSET)
        .mt(space::S16)
        .mb(space::S4)
        .h(px(20.))
        .flex()
        .items_center()
        .gap(space::S8)
        .t_meta_medium()
        .text_color(colors.danger_text)
        .child(div().flex_1().h(theme::BORDER).bg(colors.danger))
        .child(t::NEW_MESSAGES)
}

#[cfg(test)]
mod tests {
    use super::quote_markdown;

    #[test]
    fn quotes_every_line() {
        assert_eq!(quote_markdown("a\n\nb"), "> a\n>\n> b");
    }
}
