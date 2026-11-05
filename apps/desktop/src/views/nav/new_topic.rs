//! The 新话题 sheet (NATIVE.md §7.1, ⌘N): the 群 (a pop-up), an optional
//! 标题 and the first message, then 取消 and 开始. An `@成员` in the first
//! message hands the 话题 to that member. Return in the title field moves
//! on; ⌘Return or 开始 starts the 话题 and shows it.

use std::cell::Cell;

use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, ParentElement,
    Render, SharedString, StatefulInteractiveElement as _, Styled, Subscription, Window, div, px,
};
use gpui_component::{
    WindowExt as _,
    input::{InputEvent, InputState, TextareaState},
    menu::{DropdownMenu as _, PopupMenuItem},
};

use super::dialogs::{form_field_width, sheet_form, sheet_form_row, sheet_form_row_top};
use crate::{
    app::Services,
    copy::{common, overlays as o},
    data::{GroupId, StartTopicRequest},
    ui::native::{
        NativeTextArea, NativeTextField, PushSize, SheetLayout, SheetWidth, disabled_push,
        native_sheet_keyboard, popup_button, sheet_buttons,
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

/// 开个话题: the 群聊 message the new 话题 starts from.
#[derive(Clone, Debug)]
struct QuoteFrom {
    message_id: String,
    /// The author and the first line, as the sheet shows them.
    line: String,
}

pub struct NewTopicSheet {
    services: Services,
    groups: Vec<(GroupId, String)>,
    group: Option<GroupId>,
    title: Entity<InputState>,
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let title =
            cx.new(|cx| InputState::new(window, cx).placeholder(o::NEW_TOPIC_NAME_PLACEHOLDER));
        let body = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(4, 4)
                .placeholder(o::NEW_TOPIC_BODY_PLACEHOLDER)
        });
        let groups = rail_model::groups(services.provider.as_ref());
        let group = group
            .filter(|g| groups.iter().any(|(id, _)| id == g))
            .or_else(|| groups.first().map(|(id, _)| id.clone()));
        let body2 = body.clone();
        let subscriptions = vec![
            cx.subscribe_in(
                &title,
                window,
                move |_, _, event: &InputEvent, window, cx| {
                    if let InputEvent::PressEnter { .. } = event {
                        body2.update(cx, |b, cx| b.focus(window, cx));
                    }
                },
            ),
            cx.subscribe(&body, |_, _, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    cx.notify();
                }
            }),
        ];
        Self {
            services,
            groups,
            group,
            title,
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
        // An empty message only turns 开始 off (NATIVE.md §7.4).
        if body.is_empty() {
            return;
        }
        let title = crate::views::topic_menu::validate_title(&self.title.read(cx).value()).ok();
        let mentions = mentioned(&self.services, &group_id, &body);
        let provider = self.services.provider.clone();
        provider.start_topic(StartTopicRequest {
            group_id: group_id.clone(),
            title,
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
        let [cancel, start] = sheet_buttons("new-topic", common::CANCEL, o::NEW_TOPIC_CREATE, cx);
        // 开个话题: the quoted 群聊 message, one muted line under its label.
        let quote_row = self.quote.as_ref().map(|q| {
            let k = crate::ui::native::native_colors(cx);
            let line: SharedString = q.line.clone().into();
            sheet_form_row(
                o::NEW_TOPIC_QUOTE,
                div()
                    .id("new-topic-quote")
                    .w(form_field_width(SheetWidth::Form))
                    .pl(crate::theme::space::S8)
                    .border_l_2()
                    .border_color(k.separator)
                    .text_color(k.secondary)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(line.clone())
                    .tooltip(move |window, cx| {
                        gpui_component::tooltip::Tooltip::new(line.clone()).build(window, cx)
                    }),
                cx,
            )
        });
        let (sheet_title, explanation) = if self.quote.is_some() {
            (o::NEW_TOPIC_FROM_TITLE, o::NEW_TOPIC_FROM_EXPLANATION)
        } else {
            (o::NEW_TOPIC_TITLE, o::NEW_TOPIC_EXPLANATION)
        };
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
                            .children(quote_row)
                            .child(sheet_form_row(
                                o::NEW_TOPIC_NAME,
                                NativeTextField::new("new-topic-title", &self.title).full_width(),
                                cx,
                            ))
                            .child(sheet_form_row_top(o::NEW_TOPIC_BODY, body_field, cx)),
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
    let sheet = cx.new(|cx| NewTopicSheet::new(services, group, window, cx));
    present(sheet, window, cx)
}

/// 开个话题 (1.1.1): the sheet for a 话题 that starts from a 群聊 message.
/// The 群 is the message's, the 标题 defaults to its first line, and the
/// first message quotes it.
pub fn open_new_topic_from(
    services: Services,
    message: &crate::data::Message,
    window: &mut Window,
    cx: &mut App,
) -> Entity<NewTopicSheet> {
    let author = services
        .provider
        .member(&message.author_id)
        .map(|m| m.name)
        .unwrap_or_default();
    let first = message
        .body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default()
        .to_string();
    let title: String = first
        .chars()
        .take(chat_core::proto::DEFAULT_TITLE_CHARS)
        .collect();
    let quote = QuoteFrom {
        message_id: message.id.clone(),
        line: if author.is_empty() {
            first.clone()
        } else {
            format!("{author}：{first}")
        },
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
        let mut sheet = NewTopicSheet::new(services, Some(group_id.clone()), window, cx);
        if !sheet.groups.iter().any(|(id, _)| *id == group_id) {
            sheet
                .groups
                .push((group_id.clone(), dm_name.unwrap_or_default()));
        }
        sheet.group = Some(group_id);
        sheet.quote = Some(quote);
        sheet
            .title
            .update(cx, |t, cx| t.set_value(title, window, cx));
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
