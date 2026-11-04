//! The 新话题 sheet (NATIVE.md §7.1, ⌘N): the 群 (a pop-up), an optional
//! 标题 and the first message, then 取消 and 开始. An `@成员` in the first
//! message hands the 话题 to that member. Return in the title field moves
//! on; ⌘Return or 开始 starts the 话题 and shows it.

use std::cell::Cell;

use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    ParentElement, Render, SharedString, Styled, Subscription, Window, div, px,
};
use gpui_component::{
    Sizable as _, WindowExt as _,
    input::{InputEvent, InputState, Textarea, TextareaState},
    menu::{DropdownMenu as _, PopupMenuItem},
};

use crate::{
    app::Services,
    copy::{common, overlays as o},
    data::{GroupId, StartTopicRequest},
    ui::{
        controls::placeholder_input,
        native::overlays::field_error,
        native::{
            FormRow, GroupedForm, NativeTextField, NativeType as _, PushSize, SheetLayout,
            SheetWidth, disabled_push, metrics, native_colors, native_sheet, popup_button,
            sheet_buttons,
        },
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

pub struct NewTopicSheet {
    services: Services,
    groups: Vec<(GroupId, String)>,
    group: Option<GroupId>,
    title: Entity<InputState>,
    body: Entity<TextareaState>,
    error: Option<&'static str>,
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
            cx.subscribe(&body, |this, _, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    this.error = None;
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
            error: None,
            _subscriptions: subscriptions,
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(group_id) = self.group.clone() else {
            return;
        };
        let body = self.body.read(cx).value().trim().to_string();
        if body.is_empty() {
            self.error = Some(o::NEW_TOPIC_BODY_REQUIRED);
            cx.notify();
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
        let k = native_colors(cx);
        let ready = self.group.is_some() && !self.body.read(cx).value().trim().is_empty();
        let group_name: SharedString = self
            .groups
            .iter()
            .find(|(id, _)| Some(id) == self.group.as_ref())
            .map(|(_, n)| n.clone())
            .unwrap_or_default()
            .into();
        let groups = self.groups.clone();
        let current = self.group.clone();
        let this = cx.entity().downgrade();
        let group_popup =
            popup_button("new-topic-group", group_name, PushSize::Regular, window, cx)
                .max_w(px(280.))
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
        let body_focused = self.body.read(cx).focus_handle(cx).is_focused(window);
        let body_field = div()
            .id("new-topic-body")
            .w_full()
            .h(px(96.))
            .px(metrics::FIELD_PAD)
            .py(px(4.))
            .rounded(metrics::FIELD_RADIUS)
            .bg(k.content)
            .border_1()
            .border_color(if self.error.is_some() {
                k.red
            } else {
                k.separator
            })
            .when_focus(body_focused, k.focus_ring())
            .cursor_text()
            .child(placeholder_input(
                Textarea::new(&self.body)
                    .appearance(false)
                    .xsmall()
                    .p_0()
                    .n_body()
                    .text_color(k.label),
                window,
                cx,
            ));
        let [cancel, start] = sheet_buttons("new-topic", common::CANCEL, o::NEW_TOPIC_CREATE, cx);
        div()
            .key_context(CONTEXT)
            .on_action(cx.listener(|this, _: &StartTopic, window, cx| this.submit(window, cx)))
            .w_full()
            .child(
                SheetLayout::new(o::NEW_TOPIC_TITLE)
                    .explanation(o::NEW_TOPIC_EXPLANATION)
                    .body(
                        GroupedForm::new()
                            .row(FormRow::new(o::NEW_TOPIC_GROUP).control(group_popup))
                            .row(
                                FormRow::new(o::NEW_TOPIC_NAME).control(
                                    NativeTextField::new("new-topic-title", &self.title)
                                        .width(px(280.)),
                                ),
                            ),
                    )
                    .body(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .px(px(2.))
                                    .n_headline()
                                    .text_color(k.label)
                                    .child(o::NEW_TOPIC_BODY),
                            )
                            .child(body_field)
                            .children(self.error.map(|e| field_error(e, cx))),
                    )
                    .buttons([
                        cancel.on_click(|_, window, cx| window.close_dialog(cx)),
                        disabled_push(start, !ready, cx)
                            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                    ]),
            )
    }
}

trait WhenFocus {
    fn when_focus(self, focused: bool, ring: Vec<gpui::BoxShadow>) -> Self;
}

impl WhenFocus for gpui::Stateful<gpui::Div> {
    fn when_focus(self, focused: bool, ring: Vec<gpui::BoxShadow>) -> Self {
        if focused { self.shadow(ring) } else { self }
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
    let first = Cell::new(true);
    let view = sheet.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            let body = view.read(cx).body.clone();
            body.update(cx, |b, cx| b.focus(window, cx));
        }
        native_sheet(dialog, SheetWidth::Form, cx).child(view.clone())
    });
    sheet
}
