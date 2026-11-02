//! Navigation dialogs (DESIGN.md §7.12): new 群 (name and members), start a
//! DM (member picker), and the alert before leaving a 群. Dialogs are 440
//! wide with 20 pt padding, a `title` 16/24, 40 pt form fields with labels
//! above, and `md` buttons at the bottom right.

use std::{cell::Cell, collections::HashSet};

use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    ParentElement, Render, StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    WindowExt as _,
    button::ButtonVariant,
    checkbox::Checkbox,
    input::{InputEvent, InputState},
    tooltip::Tooltip,
};

use crate::{
    app::{Place, Services},
    copy::{common, fill, nav},
    data::{AdminError, CreateGroupRequest, GroupId, Member, MemberId},
    theme::{self, Type as _, size, space},
    ui::{self, ButtonSpec as _, FieldKind, Lucide},
    views::sidebar::{edge_fade, scroll_edges, watch_scroll_edges},
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

/// What the member form does: start a new 群, or add members to one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormMode {
    NewGroup,
    AddMembers { group_id: GroupId },
}

/// The body of the new-group and add-members dialogs. It keeps the inputs across dialog
/// rebuilds, and stays open until the server accepts the request.
pub struct NewGroupForm {
    services: Services,
    mode: FormMode,
    busy: bool,
    name: Entity<InputState>,
    filter: Entity<InputState>,
    members: Vec<Member>,
    /// In pick order.
    selected: Vec<MemberId>,
    error: Option<&'static str>,
    scroll: gpui::ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl NewGroupForm {
    fn new(services: Services, mode: FormMode, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name =
            cx.new(|cx| InputState::new(window, cx).placeholder(nav::GROUP_NAME_PLACEHOLDER));
        let filter =
            cx.new(|cx| InputState::new(window, cx).placeholder(nav::MEMBER_FILTER_PLACEHOLDER));
        let me = services.me();
        let mut members = pickable_members(&services.provider.members(), me.as_deref());
        if let FormMode::AddMembers { group_id } = &mode {
            // Only members who are not in the 群 yet.
            let inside: Vec<MemberId> = services
                .provider
                .groups()
                .into_iter()
                .find(|g| &g.id == group_id)
                .map(|g| g.member_ids)
                .unwrap_or_default();
            members.retain(|m| !inside.contains(&m.id));
        }
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
            mode,
            busy: false,
            name,
            filter,
            members,
            selected: Vec::new(),
            error: None,
            scroll: gpui::ScrollHandle::new(),
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
        if self.busy {
            return;
        }
        let reply = match &self.mode {
            FormMode::NewGroup => {
                let name = self.name.read(cx).value().to_string();
                match validate_new_group(&name, &self.selected) {
                    Ok(request) => {
                        let r = super::create_group(request, cx);
                        futures::FutureExt::boxed_local(async move {
                            r.await.unwrap_or(Err(AdminError::Other)).map(|_| ())
                        })
                    }
                    Err(error) => {
                        self.error = Some(error);
                        cx.notify();
                        return;
                    }
                }
            }
            FormMode::AddMembers { group_id } => {
                if self.selected.is_empty() {
                    return;
                }
                let r = self
                    .services
                    .provider
                    .add_group_members(group_id, self.selected.clone());
                futures::FutureExt::boxed_local(async move {
                    r.await.unwrap_or(Err(AdminError::Other))
                })
            }
        };
        self.busy = true;
        self.error = None;
        cx.notify();
        // The dialog stays open until the server answers, so a refusal can be shown.
        cx.spawn_in(window, async move |this, cx| {
            let result = reply.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(()) => window.close_dialog(cx),
                    Err(e) => {
                        if this.mode == FormMode::NewGroup {
                            super::clear_pending(cx);
                        }
                        this.error = Some(crate::views::settings::widgets::error_text(e));
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }
}

impl Render for NewGroupForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let edges = scroll_edges(&self.scroll);
        watch_scroll_edges(&self.scroll, edges, window, cx);
        let query = self.filter.read(cx).value().trim().to_lowercase();
        let new_group = self.mode == FormMode::NewGroup;
        let ready = !self.busy
            && if new_group {
                !self.name.read(cx).value().trim().is_empty()
            } else {
                !self.selected.is_empty()
            };
        let selected: HashSet<&str> = self.selected.iter().map(|s| s.as_str()).collect();
        let rows: Vec<_> =
            self.members
                .iter()
                .filter(|m| query.is_empty() || m.name.to_lowercase().contains(&query))
                .map(|m| {
                    let id = m.id.clone();
                    let id2 = m.id.clone();
                    let checked = selected.contains(m.id.as_str());
                    // A 40 pt row (§7.12): the whole row toggles the checkbox.
                    ui::list_row(
                        gpui::SharedString::from(format!("pick-row-{}", m.id)),
                        ui::RowSurface::Overlay,
                        false,
                        cx,
                    )
                    .h(size::POPUP_ROW)
                    .px(space::S8)
                    .gap(space::S10)
                    .tooltip({
                        let tip = member_tip(m);
                        move |window, cx| Tooltip::new(tip.clone()).build(window, cx)
                    })
                    .child(member_row(m, k.overlay, cx))
                    .child(
                        div().flex_none().child(
                            Checkbox::new(gpui::SharedString::from(format!("pick-{}", m.id)))
                                .checked(checked)
                                .on_click(cx.listener(move |this, on: &bool, _, cx| {
                                    this.toggle(&id, *on, cx)
                                })),
                        ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let on = !this.selected.contains(&id2);
                        this.toggle(&id2, on, cx)
                    }))
                    .into_any_element()
                })
                .collect();
        let empty = rows.is_empty();
        div()
            .flex()
            .flex_col()
            .t_ui()
            .when(new_group, |el| {
                el.child(ui::field_label(nav::GROUP_NAME_LABEL, cx)).child(
                    ui::TextField::new("new-group-name", &self.name, FieldKind::Form)
                        .error(self.error.is_some()),
                )
            })
            .when_some(self.error.filter(|_| new_group), |el, e| {
                el.child(ui::field_help(e, true, cx))
            })
            .child(
                div()
                    .when(new_group, |el| el.pt(space::S16))
                    .pb(space::S8)
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .t_small()
                            .medium()
                            .text_color(k.text_2)
                            .child(nav::PICK_MEMBERS),
                    )
                    .child(div().t_meta().tnum().text_color(k.text_3).child(fill(
                        nav::SELECTED_COUNT,
                        &[("n", &self.selected.len().to_string())],
                    ))),
            )
            .child(ui::TextField::new(
                "new-group-filter",
                &self.filter,
                FieldKind::SearchBordered,
            ))
            .child(
                // Rows cut by an edge fade from the dialog surface (§7.7
                // scroll edges), as in the sidebar and the switcher.
                div()
                    .relative()
                    .mt(space::S8)
                    .mx(px(-8.))
                    // Five and a half 40 pt rows: the half row and the fade show that
                    // more members follow.
                    .h(px(220.))
                    .child(
                        div()
                            .id("new-group-members")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .children(rows)
                            .when(empty, |el| {
                                el.child(
                                    div()
                                        .h(size::POPUP_ROW)
                                        .px(space::S8)
                                        .flex()
                                        .items_center()
                                        .t_small()
                                        .text_color(k.text_2)
                                        .child(if self.members.is_empty() {
                                            nav::ALL_MEMBERS_IN
                                        } else {
                                            nav::NO_MEMBERS_MATCH
                                        }),
                                )
                            }),
                    )
                    .when(edges.0, |el| el.child(edge_fade(true, k.overlay)))
                    .when(edges.1, |el| el.child(edge_fade(false, k.overlay))),
            )
            .when_some(self.error.filter(|_| !new_group), |el, e| {
                el.child(div().pt(space::S8).child(ui::field_help(e, true, cx)))
            })
            .child(
                div()
                    .pt(space::S20)
                    .flex()
                    .justify_end()
                    .gap(space::S8)
                    .child(
                        ui::secondary_button("new-group-cancel", common::CANCEL, cx)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        ui::primary_button(
                            "new-group-create",
                            if new_group { nav::CREATE } else { nav::ADD },
                            cx,
                        )
                        .loading(self.busy)
                        .disabled_fill(!ready, cx)
                            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                    ),
            )
    }
}

/// The tooltip of a picker row: the full name and 岗位.
fn member_tip(m: &Member) -> gpui::SharedString {
    match &m.role {
        Some(r) => format!("{} · {}", m.name, r).into(),
        None => m.name.clone().into(),
    }
}

/// One member in a picker: a 24 avatar, the name `ui` 14 and the 岗位 13
/// `text-3`. The 岗位 shrinks first; `surface` is what the avatar sits on.
fn member_row(m: &Member, surface: gpui::Hsla, cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    div()
        .flex_1()
        .min_w(px(0.))
        .flex()
        .items_center()
        .gap(space::S10)
        .child(
            div().flex_none().child(
                ui::MemberAvatar::new(Some(m), size::AVATAR_ROW)
                    .presence(Some(m))
                    .on(surface),
            ),
        )
        .child(
            // The name has priority: it keeps its width up to 60 % of the
            // row, and the 岗位 takes what is left (max 160).
            div()
                .flex_shrink_0()
                .max_w(gpui::relative(0.6))
                .t_ui()
                .text_color(k.text_1)
                .one_line()
                .child(m.name.clone()),
        )
        .children(m.role.clone().map(|r| {
            div()
                .flex_1()
                .min_w(px(0.))
                .max_w(px(160.))
                .t_small()
                .text_color(k.text_3)
                .one_line()
                .child(r)
        }))
}

/// Opens the dialog that adds members to a 群 (from the 群 menu).
pub fn open_add_members(services: Services, group_id: GroupId, name: String, window: &mut Window, cx: &mut App) {
    let form = cx.new(|cx| {
        NewGroupForm::new(services, FormMode::AddMembers { group_id }, window, cx)
    });
    let first = Cell::new(true);
    let title = fill(nav::ADD_MEMBERS_TITLE, &[("name", &name)]);
    window.open_dialog(cx, move |dialog, window, cx| {
        let dialog = crate::ui::SheetExt::sheet(dialog, cx);
        if first.replace(false) {
            let handle = form.read(cx).filter.read(cx).focus_handle(cx);
            handle.focus(window, cx);
        }
        dialog
            .close_button(false)
            .p(space::S20)
            .title(div().t_title().pb(space::S8).one_line().child(title.clone()))
            .child(form.clone())
    });
}

/// Opens the new-group dialog.
pub fn open_new_group(services: Services, window: &mut Window, cx: &mut App) {
    let form = cx.new(|cx| NewGroupForm::new(services, FormMode::NewGroup, window, cx));
    let first = Cell::new(true);
    // ⌘⇧N opens it at once; a click on `+` fades it in (§12).
    let keyed = window.last_input_was_keyboard();
    window.open_dialog(cx, move |dialog, window, cx| {
        let dialog = crate::ui::SheetExt::sheet(dialog, cx);
        let dialog = crate::ui::SheetExt::keyed(dialog, keyed);
        if first.replace(false) {
            let handle = form.read(cx).name.read(cx).focus_handle(cx);
            handle.focus(window, cx);
        }
        dialog
            .close_button(false)
            .p(space::S20)
            .title(div().t_title().pb(space::S8).child(nav::NEW_GROUP))
            .child(form.clone())
    });
}

/// The member picker that starts a DM (§7.12): a 36 pt search field, 40 pt
/// rows, Up and Down move, Enter opens or starts the DM.
pub struct DmPicker {
    filter: Entity<InputState>,
    focus: gpui::FocusHandle,
    members: Vec<Member>,
    active: usize,
    scroll: gpui::ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl gpui::Focusable for DmPicker {
    fn focus_handle(&self, _: &App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}

impl DmPicker {
    fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter =
            cx.new(|cx| InputState::new(window, cx).placeholder(nav::START_DM_PLACEHOLDER));
        let me = services.me();
        let members = pickable_members(&services.provider.members(), me.as_deref());
        let subscriptions = vec![cx.subscribe_in(
            &filter,
            window,
            |this, _, event: &InputEvent, _window, cx| {
                if let InputEvent::Change = event {
                    this.active = 0;
                    cx.notify();
                }
            },
        )];
        Self {
            filter,
            focus: cx.focus_handle(),
            members,
            active: 0,
            scroll: gpui::ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    fn shown(&self, cx: &App) -> Vec<Member> {
        let query = self.filter.read(cx).value().trim().to_lowercase();
        self.members
            .iter()
            .filter(|m| query.is_empty() || m.name.to_lowercase().contains(&query))
            .cloned()
            .collect()
    }

    fn select(&mut self, delta: isize, cx: &mut Context<Self>) {
        let n = self.shown(cx).len() as isize;
        if n > 0 {
            self.active = (self.active as isize + delta).rem_euclid(n) as usize;
            cx.notify();
        }
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let member = self.shown(cx).get(self.active).map(|m| m.id.clone());
        if let Some(id) = member {
            window.close_dialog(cx);
            super::open_dm_with(id, cx);
        }
    }
}

impl Render for DmPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let edges = scroll_edges(&self.scroll);
        watch_scroll_edges(&self.scroll, edges, window, cx);
        let shown = self.shown(cx);
        if self.active >= shown.len() {
            self.active = 0;
        }
        if !shown.is_empty() {
            self.scroll.scroll_to_item(self.active);
        }
        let rows: Vec<_> = shown
            .iter()
            .enumerate()
            .map(|(ix, m)| {
                let active = ix == self.active;
                ui::list_row(
                    gpui::SharedString::from(format!("dm-row-{}", m.id)),
                    ui::RowSurface::Overlay,
                    active,
                    cx,
                )
                .h(size::POPUP_ROW)
                .px(space::S10)
                .gap(space::S10)
                .tooltip({
                    let tip = member_tip(m);
                    move |window, cx| Tooltip::new(tip.clone()).build(window, cx)
                })
                .child(member_row(
                    m,
                    if active { k.selected } else { k.overlay },
                    cx,
                ))
                .when(active, |el| el.child(ui::return_hint(cx)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.active = ix;
                    this.confirm(window, cx)
                }))
                .into_any_element()
            })
            .collect();
        let empty = rows.is_empty();
        div()
            .key_context(super::switcher::CONTEXT)
            .track_focus(&self.focus)
            .on_action(
                cx.listener(|this, _: &super::switcher::SelectPrevious, _, cx| this.select(-1, cx)),
            )
            .on_action(
                cx.listener(|this, _: &super::switcher::SelectNext, _, cx| this.select(1, cx)),
            )
            .on_action(
                cx.listener(|this, _: &super::switcher::OpenSelected, window, cx| {
                    this.confirm(window, cx)
                }),
            )
            .flex()
            .flex_col()
            .child(ui::TextField::new(
                "start-dm-filter",
                &self.filter,
                FieldKind::SearchBordered,
            ))
            .child(
                div()
                    .relative()
                    .mt(space::S8)
                    .mx(px(-8.))
                    // Seven and a half 40 pt rows, so the cut row shows more follow.
                    .h(px(300.))
                    .child(
                        div()
                            .id("start-dm-members")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .children(rows)
                            .when(empty, |el| {
                                el.child(div().h(px(160.)).child(ui::EmptyState::new(
                                    Lucide::SearchX,
                                    nav::NO_MEMBERS_MATCH,
                                )))
                            }),
                    )
                    .when(edges.0, |el| el.child(edge_fade(true, k.overlay)))
                    .when(edges.1, |el| el.child(edge_fade(false, k.overlay))),
            )
    }
}

/// Opens the member picker; confirming opens or starts the DM.
pub fn open_start_dm(services: Services, window: &mut Window, cx: &mut App) {
    let picker = cx.new(|cx| DmPicker::new(services, window, cx));
    let first = Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        let dialog = crate::ui::SheetExt::sheet(dialog, cx);
        if first.replace(false) {
            let handle = picker.read(cx).filter.read(cx).focus_handle(cx);
            handle.focus(window, cx);
        }
        dialog
            .close_button(false)
            .p(space::S20)
            .title(div().t_title().pb(space::S8).child(nav::START_DM))
            .child(picker.clone())
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
    window.open_alert_dialog(cx, move |alert, _, cx| {
        let alert = crate::ui::SheetExt::sheet(alert, cx);
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
