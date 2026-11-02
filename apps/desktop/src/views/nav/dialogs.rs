//! Navigation dialogs: new 群 (name and members), start a DM (member picker),
//! and the confirmation before leaving a 群.

use std::{cell::Cell, collections::HashSet, rc::Rc};

use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    ParentElement, Render, StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _, WindowExt as _,
    button::{Button, ButtonVariant, ButtonVariants as _},
    checkbox::Checkbox,
    command::{Command, CommandItem, CommandState},
    input::{Input, InputEvent, InputState},
};

use crate::{
    app::{Place, Services},
    copy::{common, fill, nav},
    data::{CreateGroupRequest, GroupId, Member, MemberId},
    theme,
    views::avatar,
};

/// Members I can add to a group or DM: everyone but me and deactivated members.
pub fn pickable_members(members: &[Member], me: Option<&str>) -> Vec<Member> {
    members
        .iter()
        .filter(|m| Some(m.id.as_str()) != me && !m.deactivated)
        .cloned()
        .collect()
}

/// Checks the new-group form. Returns the request or the error text.
pub fn validate_new_group(
    name: &str,
    selected: &[MemberId],
) -> Result<CreateGroupRequest, &'static str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(nav::GROUP_NAME_REQUIRED);
    }
    Ok(CreateGroupRequest {
        name: name.chars().take(80).collect(),
        member_ids: selected.to_vec(),
    })
}

/// The body of the new-group dialog. It keeps the inputs across dialog rebuilds.
pub struct NewGroupForm {
    services: Services,
    name: Entity<InputState>,
    filter: Entity<InputState>,
    members: Vec<Member>,
    /// In pick order.
    selected: Vec<MemberId>,
    error: Option<&'static str>,
    _subscriptions: Vec<Subscription>,
}

impl NewGroupForm {
    fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name =
            cx.new(|cx| InputState::new(window, cx).placeholder(nav::GROUP_NAME_PLACEHOLDER));
        let filter =
            cx.new(|cx| InputState::new(window, cx).placeholder(nav::MEMBER_FILTER_PLACEHOLDER));
        let me = services.me();
        let members = pickable_members(&services.provider.members(), me.as_deref());
        let subscriptions = vec![
            cx.subscribe_in(
                &name,
                window,
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::PressEnter { .. } => this.submit(window, cx),
                    InputEvent::Change => {
                        this.error = None;
                        cx.notify();
                    }
                    _ => {}
                },
            ),
            cx.subscribe(&filter, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
        ];
        Self {
            services,
            name,
            filter,
            members,
            selected: Vec::new(),
            error: None,
            _subscriptions: subscriptions,
        }
    }

    fn toggle(&mut self, id: &str, on: bool, cx: &mut Context<Self>) {
        self.selected.retain(|m| m != id);
        if on {
            self.selected.push(id.to_string());
        }
        cx.notify();
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name.read(cx).value().to_string();
        match validate_new_group(&name, &self.selected) {
            Ok(request) => {
                window.close_dialog(cx);
                super::create_group(request, cx);
            }
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }
}

impl Render for NewGroupForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let query = self.filter.read(cx).value().trim().to_lowercase();
        let selected: HashSet<&str> = self.selected.iter().map(|s| s.as_str()).collect();
        let rows: Vec<_> =
            self.members
                .iter()
                .filter(|m| query.is_empty() || m.name.to_lowercase().contains(&query))
                .map(|m| {
                    let id = m.id.clone();
                    let checked = selected.contains(m.id.as_str());
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .py(px(4.))
                        .child(
                            Checkbox::new(gpui::SharedString::from(format!("pick-{}", m.id)))
                                .checked(checked)
                                .on_click(cx.listener(move |this, on: &bool, _, cx| {
                                    this.toggle(&id, *on, cx)
                                })),
                        )
                        .child(avatar(Some(m), px(20.), cx))
                        .child(div().text_sm().child(m.name.clone()))
                        .children(
                            m.role.clone().map(|r| {
                                div().text_xs().text_color(colors.muted_foreground).child(r)
                            }),
                        )
                        .into_any_element()
                })
                .collect();
        let empty = rows.is_empty();
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(div().text_sm().child(nav::GROUP_NAME_LABEL))
            .child(Input::new(&self.name))
            .when_some(self.error, |el, e| {
                el.child(div().text_xs().text_color(colors.danger).child(e))
            })
            .child(
                div()
                    .pt(px(8.))
                    .flex()
                    .justify_between()
                    .text_sm()
                    .child(nav::PICK_MEMBERS)
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(fill(
                                nav::SELECTED_COUNT,
                                &[("n", &self.selected.len().to_string())],
                            )),
                    ),
            )
            .child(Input::new(&self.filter).small())
            .child(
                div()
                    .id("new-group-members")
                    .h(px(220.))
                    .overflow_y_scroll()
                    .children(rows)
                    .when(empty, |el| {
                        el.child(
                            div()
                                .p(px(8.))
                                .text_sm()
                                .text_color(colors.muted_foreground)
                                .child(nav::NO_MEMBERS_MATCH),
                        )
                    }),
            )
            .child(
                div()
                    .pt(px(8.))
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        Button::new("new-group-cancel")
                            .label(common::CANCEL)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("new-group-create")
                            .primary()
                            .label(nav::CREATE)
                            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                    ),
            )
    }
}

/// Opens the new-group dialog.
pub fn open_new_group(services: Services, window: &mut Window, cx: &mut App) {
    let form = cx.new(|cx| NewGroupForm::new(services, window, cx));
    let first = Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        let dialog = crate::ui::SheetExt::sheet(dialog);
        if first.replace(false) {
            let handle = form.read(cx).name.read(cx).focus_handle(cx);
            handle.focus(window, cx);
        }
        dialog.w(px(440.)).title(nav::NEW_GROUP).child(form.clone())
    });
}

/// Opens the member picker; confirming opens or starts the DM.
pub fn open_start_dm(services: Services, window: &mut Window, cx: &mut App) {
    let me = services.me();
    let members = Rc::new(pickable_members(
        &services.provider.members(),
        me.as_deref(),
    ));
    let state = cx.new(|cx| CommandState::new(window, cx));
    let first = Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        let dialog = crate::ui::SheetExt::sheet(dialog);
        if first.replace(false) {
            state.update(cx, |s, cx| s.focus(window, cx));
        }
        let items =
            members.iter().map(|m| {
                let m2 = m.clone();
                CommandItem::new()
                    .label(m.name.clone())
                    .child(move |_, cx| {
                        let colors = theme::colors(cx);
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(avatar(Some(&m2), px(20.), cx))
                            .child(m2.name.clone())
                            .children(m2.role.clone().map(|r| {
                                div().text_xs().text_color(colors.muted_foreground).child(r)
                            }))
                    })
            });
        let members = members.clone();
        let command = Command::new(&state)
            .placeholder(nav::START_DM_PLACEHOLDER)
            .bordered(false)
            .max_h(px(360.))
            .items(items)
            .empty(|_, _, cx| {
                div()
                    .p(px(16.))
                    .text_sm()
                    .text_color(theme::colors(cx).muted_foreground)
                    .child(nav::NO_MEMBERS_MATCH)
            })
            .on_confirm(move |ix, window, cx| {
                let member = members.get(ix.row).map(|m| m.id.clone());
                window.close_dialog(cx);
                if let Some(id) = member {
                    super::open_dm_with(id, cx);
                }
            });
        dialog
            .w(px(480.))
            .margin_top(px(96.))
            .close_button(false)
            .p_0()
            .child(command)
    });
}

/// Asks before leaving a group; on OK leaves it and moves away from it.
pub fn confirm_leave(
    services: Services,
    group_id: GroupId,
    name: String,
    window: &mut Window,
    cx: &mut App,
) {
    window.open_alert_dialog(cx, move |alert, _, _| {
        let alert = crate::ui::SheetExt::sheet(alert);
        let services = services.clone();
        let group_id = group_id.clone();
        alert
            .title(fill(nav::LEAVE_CONFIRM_TITLE, &[("name", &name)]))
            .description(nav::LEAVE_CONFIRM_BODY)
            .ok_text(nav::LEAVE_OK)
            .ok_variant(ButtonVariant::Danger)
            .cancel_text(common::CANCEL)
            .show_cancel(true)
            .on_ok(move |_, _, cx| {
                leave(&services, &group_id, cx);
                true
            })
    });
}

fn leave(services: &Services, group_id: &str, cx: &mut App) {
    let showing = {
        let nav = services.nav.read(cx);
        nav.place == Some(Place::Group(group_id.to_string()))
            || nav.topic.as_ref().is_some_and(|(g, _)| g == group_id)
    };
    if showing {
        let next = services
            .provider
            .groups()
            .into_iter()
            .find(|g| g.id != group_id)
            .map(|g| g.id);
        services.nav.update(cx, |nav, cx| match next {
            Some(g) => nav.open_group(g, cx),
            None => {
                nav.place = None;
                nav.topic = None;
                cx.notify();
            }
        });
    }
    services.provider.leave_group(group_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::ChatProvider;
    use crate::fake::{FakeProvider, ME};

    #[test]
    fn new_group_needs_a_name() {
        assert_eq!(
            validate_new_group("   ", &[]).unwrap_err(),
            nav::GROUP_NAME_REQUIRED
        );
        let req = validate_new_group("  设计  ", &["m-alex".into()]).unwrap();
        assert_eq!(req.name, "设计");
        assert_eq!(req.member_ids, vec!["m-alex".to_string()]);
        let long: String = "长".repeat(200);
        assert_eq!(
            validate_new_group(&long, &[]).unwrap().name.chars().count(),
            80
        );
    }

    #[test]
    fn picker_leaves_out_me_and_deactivated_members() {
        let p = FakeProvider::new();
        let all = p.members();
        let picked = pickable_members(&all, Some(ME));
        assert!(picked.iter().all(|m| m.id != ME && !m.deactivated));
        assert!(all.iter().any(|m| m.deactivated));
        assert_eq!(
            picked.len(),
            all.iter().filter(|m| m.id != ME && !m.deactivated).count()
        );
    }
}
