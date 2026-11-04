//! Navigation sheets and alerts (NATIVE.md §7.1, §7.2): 新建群 (name and
//! members), 添加成员, 发起私聊 (a member list) are sheets attached under the
//! toolbar, with 取消 and the default button at the bottom trailing edge;
//! 离开群 is a Tahoe alert whose destructive choice has red text.

use std::{cell::Cell, collections::HashSet};

use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    ParentElement, Render, StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    WindowExt as _,
    input::{InputEvent, InputState},
};

use crate::{
    app::{Place, Services},
    copy::{common, fill, nav, overlays as o},
    data::{AdminError, CreateGroupRequest, GroupId, Member, MemberId},
    theme::Type as _,
    ui::{
        self,
        native::{
            AlertLayout, NativeCheckbox, NativeTextField, NativeType as _, PushStyle, SearchField,
            SheetLayout, SheetWidth, alert_button, disabled_push, metrics, native_colors,
            native_sheet,
            overlays::{app_icon, sheet_list, sheet_list_row},
            sheet_buttons,
        },
    },
};

// --- v12/fx-overlays: the one form pattern of every sheet ---

/// The label column of a sheet form: fits 第一条消息 (five characters).
pub const FORM_LABEL: gpui::Pixels = px(72.);
/// The gap between a label and its control.
pub const FORM_GAP: gpui::Pixels = px(8.);
/// The space between the rows of a sheet form.
pub const FORM_ROW_GAP: gpui::Pixels = px(12.);

/// The width a control takes in the form of a `width` sheet: from the
/// control column to the sheet's trailing inset.
pub fn form_field_width(width: SheetWidth) -> gpui::Pixels {
    width.px() - metrics::SHEET_PAD * 2. - FORM_LABEL - FORM_GAP
}

/// A sheet form (the Messages Name and Photo sheet): right-aligned labels
/// in one column, standalone controls on the sheet background, no group
/// fill under them.
pub fn sheet_form() -> gpui::Div {
    div().w_full().flex().flex_col().gap(FORM_ROW_GAP)
}

/// One row of a [`sheet_form`]: the label right-aligned in the label column,
/// the control after it, centred on the control's first line.
pub fn sheet_form_row(
    label: impl Into<gpui::SharedString>,
    control: impl IntoElement,
    cx: &App,
) -> gpui::Div {
    form_row(label.into(), control.into_any_element(), false, cx)
}

/// A row whose control is taller than one line (a multi-line field, a
/// grid): the label sits on the control's first line.
pub fn sheet_form_row_top(
    label: impl Into<gpui::SharedString>,
    control: impl IntoElement,
    cx: &App,
) -> gpui::Div {
    form_row(label.into(), control.into_any_element(), true, cx)
}

fn form_row(
    label: gpui::SharedString,
    control: gpui::AnyElement,
    top: bool,
    cx: &App,
) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .w_full()
        .flex()
        .gap(FORM_GAP)
        .map(|d| {
            if top {
                d.items_start()
            } else {
                d.items_center()
            }
        })
        .child(
            div()
                .flex_none()
                .w(FORM_LABEL)
                .min_h(metrics::FIELD)
                .flex()
                .items_center()
                .justify_end()
                .n_body()
                .text_color(k.label)
                .child(label),
        )
        .child(div().flex_1().min_w(px(0.)).flex().child(control))
}

/// The one quiet footnote of a sheet form (a field problem, a hint):
/// Subheadline in `secondary`, on the control column. Never red.
pub fn sheet_footnote(text: impl Into<gpui::SharedString>, cx: &App) -> gpui::Div {
    div()
        .w_full()
        .pl(FORM_LABEL + FORM_GAP)
        .n_subheadline()
        .text_color(native_colors(cx).secondary)
        .child(text.into())
}

/// Configures a dialog as a window-modal alert (§7.2): the Tahoe alert,
/// attached at the top centre of its window under the toolbar row, as a
/// sheet is, not floating in the middle.
pub fn window_alert(
    dialog: gpui_component::dialog::Dialog,
    cx: &App,
) -> gpui_component::dialog::Dialog {
    let k = native_colors(cx);
    dialog
        .entrance(crate::ui::native::sheet::SHEET_ENTRANCE)
        .margin_top(metrics::TOOLBAR_ROW)
        .width(metrics::ALERT_WIDTH)
        .close_button(false)
        .keyboard(true)
        .overlay_closable(false)
        .bg(k.sheet)
        .border_color(k.glass_edge)
        .rounded(metrics::SHEET_RADIUS)
        .p(px(0.))
        .gap(px(0.))
}

/// A blocking error (§7.4): an alert with the app icon, the title, the
/// message and one default `好`. `then` runs after it closes (a sheet the
/// error ends closes with it).
pub fn error_alert(
    title: impl Into<gpui::SharedString>,
    message: impl Into<gpui::SharedString>,
    then: impl Fn(&mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let title = title.into();
    let message = message.into();
    let then = std::rc::Rc::new(then);
    window.open_dialog(cx, move |dialog, _, cx| {
        let on_ok = then.clone();
        let on_click = then.clone();
        window_alert(dialog, cx)
            .on_ok(move |_, window, cx| {
                let on_ok = on_ok.clone();
                window.defer(cx, move |window, cx| on_ok(window, cx));
                true
            })
            .child(
                AlertLayout::new(title.clone())
                    .icon(app_icon(metrics::ALERT_ICON))
                    .message(message.clone())
                    .buttons([alert_button(
                        "error-alert-ok",
                        o::ALERT_OK,
                        PushStyle::Default,
                        false,
                        cx,
                    )
                    .on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        on_click(window, cx);
                    })]),
            )
    });
}

// --- end v12/fx-overlays ---

/// Members I can add to a group or DM: everyone but me and deactivated members.
pub fn pickable_members(members: &[Member], me: Option<&str>) -> Vec<Member> {
    members
        .iter()
        .filter(|m| Some(m.id.as_str()) != me && !m.deactivated)
        .cloned()
        .collect()
}

/// Longest 群 name, in characters (docs/protocol.md).
pub const MAX_GROUP_NAME_CHARS: usize = 64;

/// A 群 name as the server stores it: trimmed, every run of whitespace (also
/// full-width spaces) one space, 1 to 64 characters. New 群 and rename use it.
pub fn validate_group_name(name: &str) -> Result<String, String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(nav::GROUP_NAME_REQUIRED.to_string());
    }
    if name.chars().count() > MAX_GROUP_NAME_CHARS {
        return Err(fill(
            nav::GROUP_NAME_TOO_LONG,
            &[("n", &MAX_GROUP_NAME_CHARS.to_string())],
        ));
    }
    Ok(name)
}

/// Checks the new-group form. Returns the request or the error text.
pub fn validate_new_group(name: &str, selected: &[MemberId]) -> Result<CreateGroupRequest, String> {
    Ok(CreateGroupRequest {
        name: validate_group_name(name)?,
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
    scroll: gpui::ScrollHandle,
    /// The sheet title of 添加成员 (with the 群 name).
    title: gpui::SharedString,
    _subscriptions: Vec<Subscription>,
}

impl NewGroupForm {
    fn new(
        services: Services,
        mode: FormMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
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
                    InputEvent::Change => cx.notify(),
                    _ => {}
                },
            ),
            cx.subscribe_in(
                &filter,
                window,
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => cx.notify(),
                    // Return runs the default button from the search field too.
                    InputEvent::PressEnter { .. } => this.submit(window, cx),
                    _ => {}
                },
            ),
        ];
        Self {
            services,
            mode,
            busy: false,
            name,
            filter,
            members,
            selected: Vec::new(),
            scroll: gpui::ScrollHandle::new(),
            title: gpui::SharedString::default(),
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
                    // The default button is off while the name has a problem.
                    Err(_) => return,
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
                futures::FutureExt::boxed_local(
                    async move { r.await.unwrap_or(Err(AdminError::Other)) },
                )
            }
        };
        self.busy = true;
        cx.notify();
        // The dialog stays open until the server answers, so a refusal can be shown.
        cx.spawn_in(window, async move |this, cx| {
            let result = reply.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(()) => window.close_dialog(cx),
                    Err(e) => {
                        let title = if this.mode == FormMode::NewGroup {
                            super::clear_pending(cx);
                            o::NEW_GROUP_REFUSED
                        } else {
                            o::ADD_MEMBERS_REFUSED
                        };
                        // A refusal blocks: an alert over the sheet, which
                        // stays open so the user can try again.
                        error_alert(
                            title,
                            crate::views::settings::widgets::error_text(e),
                            |_, _| {},
                            window,
                            cx,
                        );
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }
}

impl Render for NewGroupForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        let query = self.filter.read(cx).value().trim().to_lowercase();
        let new_group = self.mode == FormMode::NewGroup;
        let typed = self.name.read(cx).value().to_string();
        // A field problem (a name that is too long) turns the default button
        // off and shows one quiet footnote; an empty name only turns it off.
        let name_problem = (new_group && !typed.trim().is_empty())
            .then(|| validate_group_name(&typed).err())
            .flatten();
        let ready = !self.busy
            && if new_group {
                !typed.trim().is_empty() && name_problem.is_none()
            } else {
                !self.selected.is_empty()
            };
        let selected: HashSet<&str> = self.selected.iter().map(|s| s.as_str()).collect();
        let rows: Vec<_> = self
            .members
            .iter()
            .filter(|m| query.is_empty() || m.name.to_lowercase().contains(&query))
            .map(|m| {
                let id = m.id.clone();
                let checked = selected.contains(m.id.as_str());
                // The whole row toggles the checkbox (a Mac table row).
                sheet_list_row(
                    gpui::SharedString::from(format!("pick-row-{}", m.id)),
                    false,
                    true,
                    cx,
                )
                .child(NativeCheckbox::new(
                    gpui::SharedString::from(format!("pick-{}", m.id)),
                    checked,
                ))
                .child(member_line(m, k.content, k.secondary))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let on = !this.selected.contains(&id);
                    this.toggle(&id, on, cx)
                }))
                .into_any_element()
            })
            .collect();
        let empty = rows.is_empty();
        let title: gpui::SharedString = match &self.mode {
            FormMode::NewGroup => nav::NEW_GROUP.into(),
            FormMode::AddMembers { .. } => self.title.clone(),
        };
        let [cancel, create] = sheet_buttons(
            "new-group",
            common::CANCEL,
            if new_group { nav::CREATE } else { nav::ADD },
            cx,
        );
        let cancel = cancel.on_click(|_, window, cx| window.close_dialog(cx));
        let create = disabled_push(create, !ready, cx)
            .loading(self.busy)
            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx)));
        let field_w = form_field_width(SheetWidth::List);
        let members = div()
            .w(field_w)
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(SearchField::new("new-group-filter", &self.filter).width(px(220.)))
                    .child(div().flex_1())
                    .child(
                        div()
                            .flex_none()
                            .n_subheadline()
                            .n_figures()
                            .text_color(k.secondary)
                            .child(fill(
                                nav::SELECTED_COUNT,
                                &[("n", &self.selected.len().to_string())],
                            )),
                    ),
            )
            .child(
                sheet_list(7, cx).child(
                    div()
                        .id("new-group-members")
                        .size_full()
                        .py(px(5.))
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .flex()
                        .flex_col()
                        .children(rows)
                        .when(empty, |el| {
                            el.child(
                                div()
                                    .size_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .n_body()
                                    .text_color(k.secondary)
                                    .child(if self.members.is_empty() {
                                        nav::ALL_MEMBERS_IN
                                    } else {
                                        nav::NO_MEMBERS_MATCH
                                    }),
                            )
                        }),
                ),
            );
        SheetLayout::new(title)
            .explanation(if new_group {
                o::NEW_GROUP_EXPLANATION
            } else {
                o::ADD_MEMBERS_EXPLANATION
            })
            .body(
                sheet_form()
                    .when(new_group, |f| {
                        f.child(sheet_form_row(
                            o::GROUP_NAME,
                            NativeTextField::new("new-group-name", &self.name).width(px(280.)),
                            cx,
                        ))
                        .when_some(name_problem, |f, p| f.child(sheet_footnote(p, cx)))
                    })
                    .child(sheet_form_row_top(o::MEMBERS, members, cx)),
            )
            .buttons([cancel, create])
    }
}

/// One member in a picker row: a 20 pt avatar, the name and the 岗位 in
/// `role_color` (it shrinks first). `surface` is what the avatar sits on.
fn member_line(m: &Member, surface: gpui::Hsla, role_color: gpui::Hsla) -> gpui::Div {
    div()
        .flex_1()
        .min_w(px(0.))
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            div().flex_none().child(
                ui::MemberAvatar::new(Some(m), px(20.))
                    .presence(Some(m))
                    .on(surface),
            ),
        )
        .child(
            div()
                .flex_shrink_0()
                .max_w(gpui::relative(0.6))
                .one_line()
                .child(m.name.clone()),
        )
        .children(m.role.clone().map(|r| {
            div()
                .flex_1()
                .min_w(px(0.))
                .text_color(role_color)
                .one_line()
                .child(r)
        }))
}

fn open_sheet<V: gpui::Render>(
    view: Entity<V>,
    focus: gpui::FocusHandle,
    width: SheetWidth,
    window: &mut Window,
    cx: &mut App,
) {
    let first = Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            focus.focus(window, cx);
        }
        native_sheet(dialog, width, cx)
            .px(px(0.))
            .child(view.clone())
    });
}

/// Opens the sheet that adds members to a 群 (from the 群 menu).
pub fn open_add_members(
    services: Services,
    group_id: GroupId,
    name: String,
    window: &mut Window,
    cx: &mut App,
) {
    let title = fill(nav::ADD_MEMBERS_TITLE, &[("name", &name)]);
    let form = cx.new(|cx| {
        let mut f = NewGroupForm::new(services, FormMode::AddMembers { group_id }, window, cx);
        f.title = title.into();
        f
    });
    let focus = form.read(cx).filter.read(cx).focus_handle(cx);
    open_sheet(form, focus, SheetWidth::List, window, cx);
}

/// Opens the new-group sheet.
pub fn open_new_group(services: Services, window: &mut Window, cx: &mut App) {
    let form = cx.new(|cx| NewGroupForm::new(services, FormMode::NewGroup, window, cx));
    let focus = form.read(cx).name.read(cx).focus_handle(cx);
    open_sheet(form, focus, SheetWidth::List, window, cx);
}

/// The sheet that starts a DM: a search field over a list of members;
/// ↑/↓ move, Return opens or starts the DM.
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
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder(o::SEARCH_MEMBERS));
        let me = services.me();
        let members = pickable_members(&services.provider.members(), me.as_deref());
        let subscriptions = vec![cx.subscribe_in(
            &filter,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    this.active = 0;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.confirm(window, cx),
                _ => {}
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
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
                let surface = if active { k.accent } else { k.content };
                sheet_list_row(
                    gpui::SharedString::from(format!("dm-row-{}", m.id)),
                    active,
                    true,
                    cx,
                )
                .child(member_line(
                    m,
                    surface,
                    if active {
                        k.on_accent.opacity(0.72)
                    } else {
                        k.secondary
                    },
                ))
                .on_click(cx.listener(move |this, e: &gpui::ClickEvent, window, cx| {
                    this.active = ix;
                    if e.click_count() > 1 {
                        this.confirm(window, cx)
                    } else {
                        cx.notify()
                    }
                }))
                .into_any_element()
            })
            .collect();
        let empty = rows.is_empty();
        let [cancel, open] = sheet_buttons("start-dm", common::CANCEL, o::START_DM_OPEN, cx);
        let cancel = cancel.on_click(|_, window, cx| window.close_dialog(cx));
        let open = disabled_push(open, empty, cx)
            .on_click(cx.listener(|this, _, window, cx| this.confirm(window, cx)));
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
            .w_full()
            .child(
                SheetLayout::new(nav::START_DM)
                    .explanation(o::START_DM_EXPLANATION)
                    .body(SearchField::new("start-dm-filter", &self.filter))
                    .body(
                        sheet_list(8, cx).child(
                            div()
                                .id("start-dm-members")
                                .size_full()
                                .py(px(5.))
                                .overflow_y_scroll()
                                .track_scroll(&self.scroll)
                                .flex()
                                .flex_col()
                                .children(rows)
                                .when(empty, |el| {
                                    el.child(
                                        div()
                                            .size_full()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .n_body()
                                            .text_color(k.secondary)
                                            .child(nav::NO_MEMBERS_MATCH),
                                    )
                                }),
                        ),
                    )
                    .buttons([cancel, open]),
            )
    }
}

/// Opens the member sheet; confirming opens or starts the DM.
pub fn open_start_dm(services: Services, window: &mut Window, cx: &mut App) {
    let picker = cx.new(|cx| DmPicker::new(services, window, cx));
    let focus = picker.read(cx).filter.read(cx).focus_handle(cx);
    open_sheet(picker, focus, SheetWidth::List, window, cx);
}

/// Opens a Tahoe alert (§7.2) with a destructive choice, attached at the
/// top centre of the window: the app icon, the title and message, the red
/// `destructive` button on top and a Standard `取消` below (Return and
/// Escape cancel; no button is filled). `on_confirm` runs when the red
/// button is chosen.
pub fn destructive_alert(
    id: &'static str,
    title: String,
    message: String,
    destructive: &'static str,
    on_confirm: impl Fn(&mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let on_confirm = std::rc::Rc::new(on_confirm);
    window.open_dialog(cx, move |dialog, _, cx| {
        let on_confirm = on_confirm.clone();
        window_alert(dialog, cx)
            // Return cancels: a destructive choice is never the default.
            .on_ok(|_, _, _| true)
            .child(
                AlertLayout::new(title.clone())
                    .icon(app_icon(metrics::ALERT_ICON))
                    .message(message.clone())
                    .buttons([
                        alert_button((id, 0usize), destructive, PushStyle::Standard, true, cx)
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                on_confirm(window, cx);
                            }),
                        alert_button((id, 1usize), common::CANCEL, PushStyle::Standard, false, cx)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ]),
            )
    });
}

/// Asks before leaving a group; on 离开 leaves it and moves away from it.
pub fn confirm_leave(
    services: Services,
    group_id: GroupId,
    name: String,
    window: &mut Window,
    cx: &mut App,
) {
    destructive_alert(
        "leave-group",
        fill(nav::LEAVE_CONFIRM_TITLE, &[("name", &name)]),
        nav::LEAVE_CONFIRM_BODY.to_string(),
        nav::LEAVE_OK,
        move |_, cx| leave(&services, &group_id, cx),
        window,
        cx,
    );
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
        let long: String = "长".repeat(65);
        assert_eq!(
            validate_new_group(&long, &[]).unwrap_err(),
            "群名称最多 64 个字"
        );
        assert_eq!(
            validate_group_name(&"长".repeat(64))
                .unwrap()
                .chars()
                .count(),
            64
        );
    }

    #[test]
    fn group_names_collapse_whitespace_like_the_server() {
        assert_eq!(
            validate_group_name(" 前端\u{3000}\t 小组 \n").unwrap(),
            "前端 小组"
        );
        assert_eq!(
            validate_group_name("\u{3000}\n").unwrap_err(),
            nav::GROUP_NAME_REQUIRED
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
