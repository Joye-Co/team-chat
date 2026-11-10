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
    ui::{
        self, Sym,
        native::{
            AlertLayout, NativeCheckbox, NativeTextField, NativeType as _, PushStyle, SearchField,
            SheetLayout, SheetWidth, alert_button, disabled_push, metrics, native_colors,
            native_sheet, native_sheet_keyboard, overlays::app_icon, sheet_buttons,
        },
    },
};

// --- v14/t4: the one form pattern of every sheet (COMPLETENESS 2.8, D8) ---

/// The label column of a sheet form (2.8): 112 in every sheet, so fields
/// start at one x in new-topic, rename and new-group.
pub const FORM_LABEL: gpui::Pixels = metrics::LABEL_COLUMN;
/// The gap between a label and its control.
pub const FORM_GAP: gpui::Pixels = metrics::LABEL_GAP;

/// The width a control takes in a row of a `width` sheet's form: from the
/// control column to the group's trailing inset.
pub fn form_field_width(width: SheetWidth) -> gpui::Pixels {
    width.px()
        - metrics::SHEET_PAD * 2.
        - crate::ui::native::tokens::layout::BORDER * 2.
        - metrics::FORM_ROW_INSET * 2.
        - FORM_LABEL
        - FORM_GAP
}

/// A sheet form (2.8 GroupBox): one group, radius 10, a 1 px border, the
/// group fill; its rows are separated by 1 px lines inset to the label x.
pub fn sheet_form() -> SheetForm {
    SheetForm::default()
}

/// The group of a [`sheet_form`]; children are its rows.
#[derive(IntoElement, Default)]
pub struct SheetForm {
    rows: Vec<gpui::AnyElement>,
}

impl ParentElement for SheetForm {
    fn extend(&mut self, elements: impl IntoIterator<Item = gpui::AnyElement>) {
        self.rows.extend(elements);
    }
}

impl gpui::RenderOnce for SheetForm {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        let n = self.rows.len();
        let mut group = div()
            .w_full()
            .rounded(metrics::GROUP_RADIUS)
            .bg(k.group)
            .flex()
            .flex_col();
        for (ix, row) in self.rows.into_iter().enumerate() {
            group = group.child(row);
            if ix + 1 < n {
                group = group.child(
                    div()
                        .ml(metrics::FORM_ROW_INSET)
                        .h(crate::ui::native::tokens::layout::HAIRLINE)
                        .bg(k.separator),
                );
            }
        }
        group
    }
}

/// One row of a [`sheet_form`] (2.8 FormRow, h44): the label (14
/// foreground) in the 112 column at x + 16, the control after it.
pub fn sheet_form_row(
    label: impl Into<gpui::SharedString>,
    control: impl IntoElement,
    cx: &App,
) -> gpui::Div {
    form_row(label.into(), control.into_any_element(), false, cx)
}

/// A full-width row (a textarea, a list, a grid): the label at the same x
/// on its own line, the control under it across the row.
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
    let label = div()
        .flex_none()
        .n_body()
        // An invalid field keeps a foreground label: its border, ring and
        // the footer message carry the error (§2.8 v15, review 3 K-03).
        .text_color(k.label)
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(label);
    if top {
        return div()
            .w_full()
            .px(metrics::FORM_ROW_INSET)
            .py(crate::ui::native::tokens::space::S12)
            .flex()
            .flex_col()
            .gap(crate::ui::native::tokens::space::S8)
            .child(label)
            .child(
                div()
                    .w_full()
                    .min_w(gpui::Pixels::ZERO)
                    .flex()
                    .child(control),
            );
    }
    div()
        .w_full()
        .min_h(metrics::FORM_ROW)
        .px(metrics::FORM_ROW_INSET)
        .py(crate::ui::native::tokens::space::S6)
        .flex()
        .items_center()
        .gap(FORM_GAP)
        .child(label.w(FORM_LABEL))
        .child(
            div()
                .flex_1()
                .min_w(gpui::Pixels::ZERO)
                .flex()
                .child(control),
        )
}

/// A dependent row of a [`sheet_form`] (2.8): a 12 muted hint for the row
/// above it, from the control column.
pub fn sheet_footnote(text: impl Into<gpui::SharedString>, cx: &App) -> gpui::Div {
    dependent_row(text.into(), cx)
}

/// Field errors go to the sheet footer's status slot; this row is a hint.
fn dependent_row(text: gpui::SharedString, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .w_full()
        .px(metrics::FORM_ROW_INSET)
        .py(crate::ui::native::tokens::space::S8)
        .pl(metrics::FORM_ROW_INSET + FORM_LABEL + FORM_GAP)
        .n_subheadline()
        .text_color(k.secondary)
        .child(text)
}

gpui::actions!(
    member_picker,
    [PickPrevious, PickNext, PickToggle, PickSubmit]
);

/// The key context of a [`MemberPicker`]'s search field: ↓ moves into the
/// list, ⌘↩ runs the default button.
pub const PICK_FILTER_CONTEXT: &str = "MemberPickerFilter";
/// The key context of a [`MemberPicker`]'s list (a Tab stop): ↑↓ move the
/// highlight, Space toggles it, ↩ and ⌘↩ run the default button.
pub const PICK_LIST_CONTEXT: &str = "MemberPickerList";

/// The member picker's keys (call once at start-up).
pub fn bind_keys(cx: &mut App) {
    use gpui::KeyBinding;
    cx.bind_keys([
        KeyBinding::new("up", PickPrevious, Some(PICK_FILTER_CONTEXT)),
        KeyBinding::new("down", PickNext, Some(PICK_FILTER_CONTEXT)),
        KeyBinding::new(
            crate::secondary!("enter"),
            PickSubmit,
            Some(PICK_FILTER_CONTEXT),
        ),
        KeyBinding::new("up", PickPrevious, Some(PICK_LIST_CONTEXT)),
        KeyBinding::new("down", PickNext, Some(PICK_LIST_CONTEXT)),
        KeyBinding::new("space", PickToggle, Some(PICK_LIST_CONTEXT)),
        KeyBinding::new("enter", PickSubmit, Some(PICK_LIST_CONTEXT)),
        KeyBinding::new(
            crate::secondary!("enter"),
            PickSubmit,
            Some(PICK_LIST_CONTEXT),
        ),
    ]);
}

/// The keyboard highlight of a multi-select [`MemberPicker`] after a move
/// by `delta` over `n` rows: the first ↓ lands on the first row, then it
/// stops at the ends (a Mac table does not wrap).
pub fn pick_step(active: Option<usize>, delta: isize, n: usize) -> Option<usize> {
    if n == 0 {
        return None;
    }
    Some(match active {
        None => 0,
        Some(ix) => (ix as isize + delta).clamp(0, n as isize - 1) as usize,
    })
}

/// How a [`MemberPicker`] chooses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickMode {
    /// A checkbox on every row (new 群, add members).
    Many,
    /// One row, marked with a check on a muted row (start a DM).
    One,
}

/// The rows of a member picker (2.9): h40.
const PICK_ROW: gpui::Pixels = crate::ui::native::tokens::height::TABLE_ROW_COMPACT;
/// Rows a picker shows: 7.5, so the cut row says the list scrolls.
const PICK_VISIBLE: f32 = 7.5;

/// The one member picker (2.9) of 新建群, 添加成员 and 发起私聊: a search
/// field across the content column, 12 below it a list box (radius 10,
/// border) of h40 rows that shows 7.5 rows and a scrollbar, then one 12
/// the sheet footer shows the count or the error (2.9).
#[derive(IntoElement)]
pub struct MemberPicker {
    id: &'static str,
    filter: Entity<InputState>,
    scroll: gpui::ScrollHandle,
    rows: Vec<gpui::AnyElement>,
    empty: gpui::SharedString,
    /// The list's focus (a Tab stop with the 3 px ring), when it takes keys.
    list_focus: Option<gpui::FocusHandle>,
}

impl MemberPicker {
    pub fn new(
        id: &'static str,
        filter: &Entity<InputState>,
        scroll: &gpui::ScrollHandle,
        rows: Vec<gpui::AnyElement>,
        empty: impl Into<gpui::SharedString>,
    ) -> Self {
        Self {
            id,
            filter: filter.clone(),
            scroll: scroll.clone(),
            rows,
            empty: empty.into(),
            list_focus: None,
        }
    }

    /// Makes the list a Tab stop that takes the picker's keys
    /// ([`PICK_LIST_CONTEXT`]); it draws the focus ring while focused.
    pub fn list_focus(mut self, focus: &gpui::FocusHandle) -> Self {
        self.list_focus = Some(focus.clone());
        self
    }

    /// One row: the checkbox (many) or the trailing check (one), the 24
    /// avatar (a green dot only when the person is online), the name (14
    /// medium, it keeps its room) and the 岗位 (14 muted, it truncates
    /// first). The full name is the tooltip.
    pub fn row(
        id: gpui::SharedString,
        m: &Member,
        mode: PickMode,
        chosen: bool,
        cx: &App,
    ) -> gpui::Stateful<gpui::Div> {
        Self::row_highlighted(id, m, mode, chosen, false, cx)
    }

    /// [`MemberPicker::row`] with the keyboard highlight of a multi-select
    /// list: the muted row fill (the checkbox shows what is chosen).
    pub fn row_highlighted(
        id: gpui::SharedString,
        m: &Member,
        mode: PickMode,
        chosen: bool,
        highlighted: bool,
        cx: &App,
    ) -> gpui::Stateful<gpui::Div> {
        use crate::ui::native::tokens::{icon, radius, space};
        let k = native_colors(cx);
        let filled = highlighted || (chosen && mode == PickMode::One);
        let surface = if filled { k.selection } else { k.content };
        // Beside the avatar a leading emoji is left out (X-01).
        let name: gpui::SharedString = ui::avatar::name_beside_avatar(&m.name).to_string().into();
        let tip = name.clone();
        div()
            .id(id.clone())
            .flex_none()
            .h(PICK_ROW)
            .px(space::S8)
            .rounded(radius::ITEM)
            .flex()
            .items_center()
            .gap(space::S8)
            .map(|d| {
                if filled {
                    d.bg(k.selection)
                } else {
                    d.hover(|s| s.bg(k.fill.opacity(0.5)))
                }
            })
            .when(mode == PickMode::Many, |d| {
                d.child(NativeCheckbox::new(
                    gpui::SharedString::from(format!("{id}-check")),
                    chosen,
                ))
            })
            .child(
                div().flex_none().child(
                    ui::MemberAvatar::new(Some(m), crate::ui::native::tokens::avatar::SM)
                        .when(!m.is_bot(), |a| a.presence(Some(m)))
                        .on(surface),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(gpui::Pixels::ZERO)
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .child(
                        div()
                            .id(gpui::SharedString::from(format!("{id}-name")))
                            .flex_none()
                            .max_w(gpui::relative(1.))
                            .n_headline()
                            .text_color(k.label)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .tooltip(move |window, cx| {
                                gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                            })
                            .child(name),
                    )
                    .children(m.role.clone().filter(|r| !r.trim().is_empty()).map(|r| {
                        div()
                            .flex_1()
                            .min_w(gpui::Pixels::ZERO)
                            .n_body()
                            .text_color(k.secondary)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(r)
                    })),
            )
            .when(mode == PickMode::One && chosen, |d| {
                d.child(div().flex_none().child(crate::ui::icons::icon(
                    Sym::Check,
                    icon::MD,
                    k.label,
                )))
            })
    }
}

impl gpui::RenderOnce for MemberPicker {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        use crate::ui::native::tokens::{layout, space};
        let k = native_colors(cx);
        let empty = self.rows.is_empty();
        let list_h = PICK_ROW * PICK_VISIBLE + space::S4 * 2. + layout::BORDER * 2.;
        // While the list scrolls its scrollbar shows at rest: the rows end
        // before its 12 track, so the thumb never covers a highlighted row
        // (review 3 K-08).
        let scrolls = self.rows.len() as f32 > PICK_VISIBLE;
        let focused = self
            .list_focus
            .as_ref()
            .is_some_and(|f| f.is_focused(window));
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(space::S12)
            .child(
                div()
                    .w_full()
                    .key_context(PICK_FILTER_CONTEXT)
                    .child(SearchField::new(
                        gpui::SharedString::from(format!("{}-filter", self.id)),
                        &self.filter,
                    )),
            )
            .child(
                div()
                    .id(gpui::SharedString::from(format!("{}-list", self.id)))
                    .relative()
                    .flex_none()
                    .w_full()
                    .h(list_h)
                    .rounded(metrics::GROUP_RADIUS)
                    .bg(k.content)
                    .border_1()
                    .border_color(k.separator)
                    .when_some(self.list_focus.as_ref(), |d, f| {
                        d.key_context(PICK_LIST_CONTEXT).track_focus(f)
                    })
                    .when(focused, |d| d.shadow(k.focus_ring()))
                    .overflow_hidden()
                    .child(
                        div()
                            .id(gpui::SharedString::from(format!("{}-members", self.id)))
                            .size_full()
                            .p(space::S4)
                            .when(scrolls, |d| d.pr(space::S12))
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .flex()
                            .flex_col()
                            .children(self.rows)
                            .when(empty, |el| {
                                el.child(div().px(space::S8).py(space::S8).child(
                                    crate::ui::native::page::compact_empty(
                                        Sym::Search,
                                        self.empty.clone(),
                                        None,
                                        cx,
                                    ),
                                ))
                            }),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .right_0()
                            .bottom_0()
                            .w(space::S12)
                            // Shown at rest while the list overflows (2.9).
                            .child(
                                gpui_component::scroll::Scrollbar::vertical(&self.scroll)
                                    .mode(gpui_component::scroll::ScrollbarMode::Always),
                            ),
                    ),
            )
    }
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
        .p(gpui::Pixels::ZERO)
        .gap(gpui::Pixels::ZERO)
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

// --- end v14/t4 sheet form ---

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
    /// The server's last refusal, shown under the list until the next edit.
    refusal: Option<AdminError>,
    /// The list's focus: Tab or ↓ from the search field reach it.
    list_focus: gpui::FocusHandle,
    /// The keyboard highlight, an index into the shown rows.
    active: Option<usize>,
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
                    InputEvent::Change => {
                        this.refusal = None;
                        cx.notify()
                    }
                    _ => {}
                },
            ),
            cx.subscribe_in(
                &filter,
                window,
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        this.active = None;
                        cx.notify()
                    }
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
            refusal: None,
            list_focus: cx.focus_handle().tab_stop(true),
            active: None,
            _subscriptions: subscriptions,
        }
    }

    /// The members the search field lets through, in list order.
    fn shown(&self, cx: &App) -> Vec<&Member> {
        let query = self.filter.read(cx).value().trim().to_lowercase();
        self.members
            .iter()
            .filter(|m| query.is_empty() || m.name.to_lowercase().contains(&query))
            .collect()
    }

    /// ↑ / ↓: moves the highlight; from the search field ↓ moves into the
    /// list, and ↑ on the first row goes back to the field.
    fn step(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.shown(cx).len();
        let in_list = self.list_focus.is_focused(window);
        if !in_list {
            if delta < 0 {
                return;
            }
            self.list_focus.focus(window, cx);
            self.active = pick_step(None, delta, n);
        } else if delta < 0 && self.active == Some(0) {
            self.filter.read(cx).focus_handle(cx).focus(window, cx);
            self.active = None;
        } else {
            self.active = pick_step(self.active, delta, n);
        }
        if let Some(ix) = self.active {
            self.scroll.scroll_to_item(ix);
        }
        cx.notify();
    }

    /// Space: checks or unchecks the highlighted row.
    fn toggle_active(&mut self, cx: &mut Context<Self>) {
        let id = self
            .active
            .and_then(|ix| self.shown(cx).get(ix).map(|m| m.id.clone()));
        if let Some(id) = id {
            let on = !self.selected.contains(&id);
            self.toggle(&id, on, cx);
        }
    }

    fn toggle(&mut self, id: &str, on: bool, cx: &mut Context<Self>) {
        self.selected.retain(|m| m != id);
        if on {
            self.selected.push(id.to_string());
        }
        self.refusal = None;
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
                        if this.mode == FormMode::NewGroup {
                            super::clear_pending(cx);
                        }
                        // A refusal shows under the list; the sheet stays
                        // open so the user can try again.
                        this.refusal = Some(e);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// Shows a refusal as if the server had sent it (a screenshot scene).
    pub fn preview_refusal(
        &mut self,
        name: &str,
        e: Option<AdminError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.name
            .update(cx, |st, cx| st.set_value(name.to_string(), window, cx));
        self.refusal = e;
        cx.notify();
    }
}

impl Render for NewGroupForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        let new_group = self.mode == FormMode::NewGroup;
        // The highlight shows only while the list has the keyboard.
        let highlight = self.active.filter(|_| self.list_focus.is_focused(window));
        let typed = self.name.read(cx).value().to_string();
        // A field problem (a name that is too long) turns the default button
        // off and marks the field; an empty name only turns it off.
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
            .shown(cx)
            .into_iter()
            .enumerate()
            .map(|(ix, m)| {
                let id = m.id.clone();
                let checked = selected.contains(m.id.as_str());
                // The whole row toggles the checkbox (a Mac table row).
                MemberPicker::row_highlighted(
                    gpui::SharedString::from(format!("pick-row-{}", m.id)),
                    m,
                    PickMode::Many,
                    checked,
                    highlight == Some(ix),
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.active = Some(ix);
                    let on = !this.selected.contains(&id);
                    this.toggle(&id, on, cx)
                }))
                .into_any_element()
            })
            .collect();
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
        // The line under the list: the name's problem or the server's
        // refusal in destructive, else the count.
        let refusal = self.refusal.map(|e| {
            let title = if new_group {
                o::NEW_GROUP_REFUSED
            } else {
                o::ADD_MEMBERS_REFUSED
            };
            format!(
                "{title}：{}",
                crate::views::settings::widgets::error_text(e)
            )
        });
        let (line, error) = match name_problem.clone().or(refusal) {
            Some(e) => (e, true),
            None => (
                fill(
                    nav::SELECTED_COUNT,
                    &[("n", &self.selected.len().to_string())],
                ),
                false,
            ),
        };
        let picker = MemberPicker::new(
            "new-group",
            &self.filter,
            &self.scroll,
            rows,
            if self.members.is_empty() {
                nav::ALL_MEMBERS_IN
            } else {
                nav::NO_MEMBERS_MATCH
            },
        )
        .list_focus(&self.list_focus);
        // The footer's left slot (2.9): the count, or the error.
        let status = if error {
            crate::ui::native::sheet::sheet_status_error(line, cx).into_any_element()
        } else {
            div()
                .n_footnote()
                .n_figures()
                .text_color(k.secondary)
                .child(line)
                .into_any_element()
        };
        let mut layout = SheetLayout::new(title)
            .explanation(if new_group {
                o::NEW_GROUP_EXPLANATION
            } else {
                o::ADD_MEMBERS_EXPLANATION
            })
            .status(status);
        if new_group {
            layout = layout.body(
                sheet_form().child(sheet_form_row(
                    o::GROUP_NAME,
                    NativeTextField::new("new-group-name", &self.name)
                        .full_width()
                        .error(name_problem.is_some()),
                    cx,
                )),
            );
        }
        div()
            .w_full()
            .on_action(cx.listener(|this, _: &PickPrevious, window, cx| this.step(-1, window, cx)))
            .on_action(cx.listener(|this, _: &PickNext, window, cx| this.step(1, window, cx)))
            .on_action(cx.listener(|this, _: &PickToggle, _, cx| this.toggle_active(cx)))
            .on_action(cx.listener(|this, _: &PickSubmit, window, cx| this.submit(window, cx)))
            .child(layout.body(picker).buttons([cancel, create]))
    }
}

/// Opens the new-group sheet with `name` typed and the server's refusal
/// `e` shown (a screenshot scene).
pub fn open_new_group_with(
    services: Services,
    name: &str,
    e: Option<AdminError>,
    window: &mut Window,
    cx: &mut App,
) {
    let form = cx.new(|cx| {
        let mut f = NewGroupForm::new(services, FormMode::NewGroup, window, cx);
        f.preview_refusal(name, e, window, cx);
        f
    });
    let focus = form.read(cx).name.read(cx).focus_handle(cx);
    open_sheet(form, focus, SheetWidth::List, window, cx);
}

fn open_sheet<V: gpui::Render>(
    view: Entity<V>,
    focus: gpui::FocusHandle,
    width: SheetWidth,
    window: &mut Window,
    cx: &mut App,
) {
    open_sheet_as(view, focus, width, false, window, cx);
}

/// [`open_sheet`]; `keyboard` when a key binding (⌘⇧N) opens it: the sheet
/// shows at once, with no slide, as ⌘N's 新话题 does (review-animations 9,
/// review 3 K-08).
fn open_sheet_as<V: gpui::Render>(
    view: Entity<V>,
    focus: gpui::FocusHandle,
    width: SheetWidth,
    keyboard: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let first = Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            focus.focus(window, cx);
        }
        let dialog = if keyboard {
            native_sheet_keyboard(dialog, width, cx)
        } else {
            native_sheet(dialog, width, cx)
        };
        dialog.px(px(0.)).child(view.clone())
    });
}

/// Opens the new-group sheet as the keyboard leaves it: the list has the
/// focus, the second row is highlighted and checked (a screenshot scene of
/// ↓ ↓ Space).
pub fn open_new_group_keyboard(services: Services, window: &mut Window, cx: &mut App) {
    let form = cx.new(|cx| {
        let mut f = NewGroupForm::new(services, FormMode::NewGroup, window, cx);
        f.active = Some(1);
        f.toggle_active(cx);
        f
    });
    let focus = form.read(cx).list_focus.clone();
    open_sheet(form, focus, SheetWidth::List, window, cx);
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

/// Opens the new-group sheet. Every path to it is the `NewGroup` action
/// (⌘⇧N and the rail button), so it opens at once like ⌘N.
pub fn open_new_group(services: Services, window: &mut Window, cx: &mut App) {
    let form = cx.new(|cx| NewGroupForm::new(services, FormMode::NewGroup, window, cx));
    let focus = form.read(cx).name.read(cx).focus_handle(cx);
    open_sheet_as(form, focus, SheetWidth::List, true, window, cx);
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
            focus: cx.focus_handle().tab_stop(true),
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

    /// ↑/↓ stop at the ends, as in every member picker (§2.9 one
    /// component, review 3 K-08).
    fn select(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(ix) = pick_step(Some(self.active), delta, self.shown(cx).len()) {
            self.active = ix;
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
                MemberPicker::row(
                    gpui::SharedString::from(format!("dm-row-{}", m.id)),
                    m,
                    PickMode::One,
                    active,
                    cx,
                )
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
        // The picker's keys (one set for every member picker): ↑↓ move the
        // chosen row from the search field or the list, ↩ opens it.
        div()
            .on_action(cx.listener(|this, _: &PickPrevious, _, cx| this.select(-1, cx)))
            .on_action(cx.listener(|this, _: &PickNext, _, cx| this.select(1, cx)))
            .on_action(cx.listener(|this, _: &PickSubmit, window, cx| this.confirm(window, cx)))
            .w_full()
            .child(
                SheetLayout::new(nav::START_DM)
                    .explanation(o::START_DM_EXPLANATION)
                    .body(
                        MemberPicker::new(
                            "start-dm",
                            &self.filter,
                            &self.scroll,
                            rows,
                            nav::NO_MEMBERS_MATCH,
                        )
                        .list_focus(&self.focus),
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
                        alert_button((id, 0usize), destructive, PushStyle::Outline, true, cx)
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                on_confirm(window, cx);
                            }),
                        alert_button((id, 1usize), common::CANCEL, PushStyle::Outline, false, cx)
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
        fill(nav::LEAVE_CONFIRM_TITLE, &[("name", &short_name(&name))]),
        nav::LEAVE_CONFIRM_BODY.to_string(),
        nav::LEAVE_OK,
        move |_, cx| leave(&services, &group_id, cx),
        window,
        cx,
    );
}

/// Display units an alert title holds for a name: CJK and other wide
/// characters take 2, the rest 1 (2.10: the title fits one line).
const TITLE_NAME_UNITS: usize = 20;

/// `name` cut at the end with "…" to fit an alert title on one line.
pub fn short_name(name: &str) -> String {
    let units = |c: char| if c.is_ascii() { 1 } else { 2 };
    if name.chars().map(units).sum::<usize>() <= TITLE_NAME_UNITS {
        return name.to_string();
    }
    let mut used = 0;
    let mut out = String::new();
    for c in name.chars() {
        if used + units(c) > TITLE_NAME_UNITS - 1 {
            break;
        }
        used += units(c);
        out.push(c);
    }
    out.push('…');
    out
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
            Some(g) => nav.open_group_chat(g, cx),
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
    fn picker_highlight_enters_at_the_top_and_stops_at_the_ends() {
        assert_eq!(pick_step(None, 1, 0), None);
        assert_eq!(pick_step(None, 1, 3), Some(0));
        assert_eq!(pick_step(Some(0), 1, 3), Some(1));
        assert_eq!(pick_step(Some(2), 1, 3), Some(2));
        assert_eq!(pick_step(Some(0), -1, 3), Some(0));
        // A filter that shrinks the list keeps the highlight on a row.
        assert_eq!(pick_step(Some(7), 1, 3), Some(2));
    }

    #[test]
    fn alert_titles_cut_long_names() {
        assert_eq!(short_name("设计"), "设计");
        let cut = short_name(&"产品设计与前端体验改进讨论组".repeat(2));
        assert!(cut.ends_with('…'));
        assert!(cut.chars().count() <= 11);
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
