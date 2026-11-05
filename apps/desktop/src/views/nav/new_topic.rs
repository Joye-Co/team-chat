//! The 新话题 sheet (NATIVE.md §7.1, ⌘N): the 群 (a pop-up) and the first
//! message, then 取消 and 开始. Nobody types a title: the server names the
//! 话题 with AI after it starts (server/src/autotitle). An `@成员` in the
//! first message hands the 话题 to that member; ⌘Return or 开始 starts the
//! 话题 and shows it.

use std::cell::Cell;

use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, ParentElement,
    Render, SharedString, Styled, Subscription, Window, div, px,
};
use gpui_component::{
    WindowExt as _,
    input::{InputEvent, TextareaState},
    menu::{DropdownMenu as _, PopupMenuItem},
};

use super::dialogs::{form_field_width, sheet_form, sheet_form_row, sheet_form_row_top};
use crate::{
    app::Services,
    copy::{common, overlays as o},
    data::{GroupId, StartTopicRequest},
    ui::native::{
        NativeTextArea, PushSize, PushStyle, SheetLayout, SheetWidth, disabled_push,
        native_sheet_keyboard, popup_button, push_button, sheet_buttons,
    },
    views::rail::model as rail_model,
};

gpui::actions!(new_topic, [StartTopic]);

const CONTEXT: &str = "NewTopicSheet";

/// The sheet's keys (call once at start-up): ⌘Return starts the 话题 from
/// the multi-line message field.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([gpui::KeyBinding::new(
        "cmd-enter",
        StartTopic,
        Some(CONTEXT),
    )]);
}

/// The members of `group_id` that `body` mentions (`@名字`).
pub fn mentioned(services: &Services, group_id: &str, body: &str) -> Vec<String> {
    let ids = services
        .provider
        .groups()
        .into_iter()
        .find(|g| g.id == group_id)
        .map(|g| g.member_ids)
        .unwrap_or_default();
    services
        .provider
        .members()
        .into_iter()
        .filter(|m| ids.contains(&m.id) && body.contains(&format!("@{}", m.name)))
        .map(|m| m.id)
        .collect()
}

/// At most this many members show in the 交给 row.
const HAND_TO_MAX: usize = 5;

/// Who the 交给 row offers in `group_id` (onboarding: a new member does not know the
/// names to @): the members of the 群 but me, bots first (they take work), each once.
pub fn hand_to(services: &Services, group_id: &str) -> Vec<(String, String)> {
    let me = services.me();
    let ids = services
        .provider
        .groups()
        .into_iter()
        .find(|g| g.id == group_id)
        .map(|g| g.member_ids)
        .unwrap_or_default();
    let mut members: Vec<_> = services
        .provider
        .members()
        .into_iter()
        .filter(|m| ids.contains(&m.id) && Some(&m.id) != me.as_ref() && !m.deactivated)
        .collect();
    members.sort_by_key(|m| !m.is_bot());
    members
        .into_iter()
        .take(HAND_TO_MAX)
        .map(|m| (m.id, m.name))
        .collect()
}

/// 开个话题: the 群聊 message the new 话题 starts from.
#[derive(Clone, Debug)]
struct QuoteFrom {
    message_id: String,
    author: Option<crate::data::Member>,
    /// The author's name, or empty when unknown.
    name: String,
    /// The message text (Markdown as written), shown at most three lines.
    text: String,
    /// The message time, `HH:MM`.
    time: String,
}

pub struct NewTopicSheet {
    services: Services,
    groups: Vec<(GroupId, String)>,
    group: Option<GroupId>,
    body: Entity<TextareaState>,
    /// Set when the sheet opened from a 群聊 message: the 群 is fixed and
    /// the first message quotes it.
    quote: Option<QuoteFrom>,
    _subscriptions: Vec<Subscription>,
}

impl NewTopicSheet {
    fn new(
        services: Services,
        group: Option<GroupId>,
        body_placeholder: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let body = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(4, 4)
                .placeholder(body_placeholder)
        });
        let groups = rail_model::groups(services.provider.as_ref());
        let group = group
            .filter(|g| groups.iter().any(|(id, _)| id == g))
            .or_else(|| groups.first().map(|(id, _)| id.clone()));
        let subscriptions = vec![cx.subscribe(&body, |_, _, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                cx.notify();
            }
        })];
        Self {
            services,
            groups,
            group,
            body,
            quote: None,
            _subscriptions: subscriptions,
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(group_id) = self.group.clone() else {
            return;
        };
        let body = self.body.read(cx).value().trim().to_string();
        // 开个话题: the quote is the content, so the note may stay empty.
        // Elsewhere an empty message only turns 开始 off (NATIVE.md §7.4).
        if body.is_empty() && self.quote.is_none() {
            return;
        }
        let mentions = mentioned(&self.services, &group_id, &body);
        let provider = self.services.provider.clone();
        provider.start_topic(StartTopicRequest {
            group_id: group_id.clone(),
            title: None,
            body,
            mentions,
            quote_of: self.quote.as_ref().map(|q| q.message_id.clone()),
        });
        window.close_dialog(cx);
        // The new 话题 is the newest one in the 群.
        if let Some(topic) = provider.topics(&group_id).into_iter().next() {
            self.services
                .nav
                .update(cx, |nav, cx| nav.open_topic(group_id, topic.id, cx));
        }
    }
}

impl Render for NewTopicSheet {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.quote.is_some() {
            return self.render_from(cx).into_any_element();
        }
        let ready = self.group.is_some() && !self.body.read(cx).value().trim().is_empty();
        let group_name: SharedString = self
            .groups
            .iter()
            .find(|(id, _)| Some(id) == self.group.as_ref())
            .map(|(_, n)| n.clone())
            .unwrap_or_default()
            .into();
        // 开个话题 stays in the quoted message's 群: the pop-up offers only it.
        let groups: Vec<_> = self
            .groups
            .iter()
            .filter(|(id, _)| self.quote.is_none() || Some(id) == self.group.as_ref())
            .cloned()
            .collect();
        let current = self.group.clone();
        let this = cx.entity().downgrade();
        let group_tip = group_name.clone();
        let group_popup =
            popup_button("new-topic-group", group_name, PushSize::Regular, window, cx)
                .w(form_field_width(SheetWidth::Form))
                .tooltip(group_tip)
                .dropdown_menu(move |menu, _, _| {
                    groups.iter().fold(menu, |menu, (id, name)| {
                        let this = this.clone();
                        let id2 = id.clone();
                        menu.item(
                            PopupMenuItem::new(name.clone())
                                .checked(current.as_ref() == Some(id))
                                .on_click(move |_, _, cx| {
                                    let id = id2.clone();
                                    let _ = this.update(cx, |this, cx| {
                                        this.group = Some(id);
                                        cx.notify();
                                    });
                                }),
                        )
                    })
                });
        // The §2.8 textarea: the 标题 field's edge, fill and ring over four
        // lines (review 3 K-02).
        let body_field = NativeTextArea::new("new-topic-body", &self.body).rows(4);
        // 交给: one small button per member of the 群; a click leads the
        // first message with @名字 (once).
        let hand_to = self
            .group
            .as_deref()
            .map(|g| hand_to(&self.services, g))
            .unwrap_or_default();
        let hand_row = (!hand_to.is_empty()).then(|| {
            let buttons = hand_to.into_iter().map(|(id, name)| {
                let body = self.body.clone();
                let mention = format!("@{name}");
                push_button(
                    SharedString::from(format!("new-topic-hand-{id}")),
                    mention.clone(),
                    PushStyle::Outline,
                    PushSize::Small,
                    cx,
                )
                .on_click(move |_, window, cx| {
                    let mention = mention.clone();
                    body.update(cx, |b, cx| {
                        let text = b.value().to_string();
                        let next = if text.contains(&mention) {
                            text
                        } else {
                            format!("{mention} {}", text.trim_start())
                        };
                        let end = next.len();
                        b.set_value(next, window, cx);
                        b.set_selected_range(end..end, cx);
                        b.focus(window, cx);
                    });
                })
            });
            sheet_form_row(
                o::NEW_TOPIC_HAND_TO,
                div().flex().flex_wrap().gap(px(6.)).children(buttons),
                cx,
            )
        });
        let [cancel, start] = sheet_buttons("new-topic", common::CANCEL, o::NEW_TOPIC_CREATE, cx);
        let (sheet_title, explanation) = (o::NEW_TOPIC_TITLE, o::NEW_TOPIC_EXPLANATION);
        div()
            .key_context(CONTEXT)
            .on_action(cx.listener(|this, _: &StartTopic, window, cx| this.submit(window, cx)))
            .w_full()
            .child(
                SheetLayout::new(sheet_title)
                    .explanation(explanation)
                    .body(
                        sheet_form()
                            .child(sheet_form_row(o::NEW_TOPIC_GROUP, group_popup, cx))
                            .child(sheet_form_row_top(o::NEW_TOPIC_BODY, body_field, cx))
                            .children(hand_row),
                    )
                    .buttons([
                        cancel.on_click(|_, window, cx| window.close_dialog(cx)),
                        disabled_push(start, !ready, cx)
                            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                    ]),
            )
            .into_any_element()
    }
}

impl NewTopicSheet {
    /// 开个话题: the quoted message as a card (who, when, up to three
    /// lines), then the 标题 and an optional note. The 群 is the message's,
    /// named in the explanation; there is nothing to pick.
    fn render_from(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        use crate::ui::native::tokens::{NativeType as _, avatar, metrics, space};
        let Some(q) = self.quote.clone() else {
            return div();
        };
        let k = crate::ui::native::native_colors(cx);
        let group_name = self
            .groups
            .iter()
            .find(|(id, _)| Some(id) == self.group.as_ref())
            .map(|(_, n)| n.clone())
            .unwrap_or_default();
        let ready = self.group.is_some();
        let card = div()
            .id("new-topic-quote")
            .w_full()
            .p(space::S12)
            .rounded(metrics::GROUP_RADIUS)
            .border_1()
            .border_color(k.separator)
            .bg(k.group)
            .flex()
            .gap(space::S12)
            .child(
                div()
                    .flex_none()
                    .child(crate::ui::MemberAvatar::new(q.author.as_ref(), avatar::SM)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(gpui::Pixels::ZERO)
                    .flex()
                    .flex_col()
                    .gap(space::S4)
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(space::S8)
                            .child(
                                div()
                                    .n_headline()
                                    .text_color(k.label)
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(q.name.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .n_subheadline()
                                    .text_color(k.secondary)
                                    .child(q.time.clone()),
                            ),
                    )
                    .child(
                        div()
                            .n_body()
                            .text_color(k.label)
                            .line_clamp(3)
                            .child(q.text.clone()),
                    ),
            );
        let body_field = NativeTextArea::new("new-topic-body", &self.body).rows(3);
        let [cancel, start] =
            sheet_buttons("new-topic", common::CANCEL, o::NEW_TOPIC_FROM_CREATE, cx);
        div()
            .key_context(CONTEXT)
            .on_action(cx.listener(|this, _: &StartTopic, window, cx| this.submit(window, cx)))
            .w_full()
            .child(
                SheetLayout::new(o::NEW_TOPIC_FROM_TITLE)
                    .explanation(crate::copy::fill(
                        o::NEW_TOPIC_FROM_EXPLANATION,
                        &[("group", &group_name)],
                    ))
                    .body(
                        div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .gap(space::S12)
                            .child(card)
                            .child(sheet_form().child(sheet_form_row_top(
                                o::NEW_TOPIC_FROM_NOTE,
                                body_field,
                                cx,
                            ))),
                    )
                    .buttons([
                        cancel.on_click(|_, window, cx| window.close_dialog(cx)),
                        disabled_push(start, !ready, cx)
                            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                    ]),
            )
    }
}

/// Opens the 新话题 sheet, the 群 preset to `group` (the open one), focus in
/// the first message.
pub fn open_new_topic(
    services: Services,
    group: Option<GroupId>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<NewTopicSheet> {
    let sheet =
        cx.new(|cx| NewTopicSheet::new(services, group, o::NEW_TOPIC_BODY_PLACEHOLDER, window, cx));
    present(sheet, window, cx)
}

/// 开个话题 (1.1.1): the sheet for a 话题 that starts from a 群聊 message.
/// The 群 is the message's and the first message quotes it; the server
/// names the 话题 with AI.
pub fn open_new_topic_from(
    services: Services,
    message: &crate::data::Message,
    window: &mut Window,
    cx: &mut App,
) -> Entity<NewTopicSheet> {
    let author = services.provider.member(&message.author_id);
    let quote = QuoteFrom {
        message_id: message.id.clone(),
        name: author.as_ref().map(|m| m.name.clone()).unwrap_or_default(),
        author,
        text: message.body.trim().to_string(),
        time: crate::views::timeline::model::clock(message.created_at),
    };
    let group_id = message.group_id.clone();
    // A 私聊 is not in the sheet's 群 list; its 话题 still start there.
    let dm_name = services
        .provider
        .groups()
        .into_iter()
        .find(|g| g.id == group_id)
        .map(|g| g.name);
    let sheet = cx.new(|cx| {
        let mut sheet = NewTopicSheet::new(
            services,
            Some(group_id.clone()),
            o::NEW_TOPIC_FROM_NOTE_PLACEHOLDER,
            window,
            cx,
        );
        if !sheet.groups.iter().any(|(id, _)| *id == group_id) {
            sheet
                .groups
                .push((group_id.clone(), dm_name.unwrap_or_default()));
        }
        sheet.group = Some(group_id);
        sheet.quote = Some(quote);
        sheet
    });
    present(sheet, window, cx)
}

fn present(
    sheet: Entity<NewTopicSheet>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<NewTopicSheet> {
    let first = Cell::new(true);
    let view = sheet.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            let body = view.read(cx).body.clone();
            body.update(cx, |b, cx| b.focus(window, cx));
        }
        // ⌘N and the rail's 新话题 button send one action, many times a
        // day: the sheet opens at once, with no slide (review-animations 9).
        native_sheet_keyboard(dialog, SheetWidth::Form, cx)
            .px(px(0.))
            .child(view.clone())
    });
    sheet
}
