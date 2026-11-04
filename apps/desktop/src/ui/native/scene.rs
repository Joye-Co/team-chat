//! The `native-kit` self-test scenes (NATIVE.md §11): every component of
//! `ui::native` in its states (rest, pressed, focused, disabled, selected,
//! inactive window), on the real surfaces, in one window per page.
//!
//! Page 0 (`native-kit`): the window anatomy (floating sidebar, toolbar row
//! active and inactive), push buttons, pop-ups, switches, checkboxes,
//! radios, segmented controls, fields and a grouped form.
//! Page 1 (`native-kit-2`): menus, a popover, the inspector, list rows,
//! progress, a sheet over a dimmed window and an alert.

use gpui::{
    AnyElement, AnyWindowHandle, App, AppContext as _, Context, Entity, IntoElement, ParentElement,
    Pixels, Render, Size, Styled, Window, WindowBounds, div, px,
};
use gpui_component::input::InputState;

use super::{
    buttons::{
        PushSize, PushStyle, borderless_button, borderless_text, disabled_push, popup_button,
        pull_down, push_button,
    },
    fields::{FieldSize, NativeTextField, SearchField},
    form::{FormRow, GroupedForm, InlineValueField},
    inspector::{CompactRow, InspectorSection, inspector_column, label_value},
    menu::{MenuRow, menu_separator, menu_surface, popover_surface},
    segmented::{Segment, Segmented},
    sheet::{
        AlertLayout, SheetLayout, SheetWidth, alert_button, alert_surface, sheet_buttons,
        sheet_surface,
    },
    sidebar::SidebarRow,
    toggles::{NativeCheckbox, NativeRadio, NativeSwitch, SwitchSize},
    tokens::{NativeColors, NativeType as _, metrics, native_colors},
    toolbar::ToolbarRow,
};
use crate::{
    app::{Services, main_window_options},
    copy::native_kit as t,
    selftest::scenes::{Scene, Start, capture_bounds},
    ui::sf_symbols::{Sf, Weight, symbol},
};

/// The scenes' slice of the self-test (`selftest::scenes::all`).
pub const SCENES: &[Scene] = &[
    Scene {
        name: "native-kit",
        about: "Native kit (NATIVE.md): window anatomy, toolbar active and inactive, buttons, pop-ups, switches, checkboxes, radios, segmented, fields, grouped form",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| open(s, c, 0, cx),
    },
    Scene {
        name: "native-kit-2",
        about: "Native kit (NATIVE.md): menus, popover, inspector, list selection (focused, unfocused, inactive), progress, sheet over a dimmed window, alert",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| open(s, c, 1, cx),
    },
    Scene {
        name: "native-kit-3",
        about: "Completeness kit (COMPLETENESS.md §1.8, §2.3–§2.16): badges, status badges, count pills, key caps, card, list rows, table, empty states, skeletons, alerts, toast",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| open(s, c, 2, cx),
    },
];

/// A device row's value and its "⋯" action (§2.19).
fn device_value(
    id: &'static str,
    value: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(div().n_body().text_color(k.secondary).child(value))
        .child(super::icon_button(
            id,
            Sf::More,
            t::DEVICE_MORE,
            super::IconSize::Small,
            window,
            cx,
        ))
}

fn open(
    _services: Services,
    content: Size<Pixels>,
    page: u8,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let mut options = main_window_options(false, cx);
    options.window_bounds = Some(WindowBounds::Windowed(capture_bounds(content, cx)));
    options.window_min_size = None;
    gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| NativeKit::new(page, window, cx))
    })
    .map(|(handle, _)| handle)
    .map_err(|e| e.to_string())
}

pub struct NativeKit {
    page: u8,
    search: Entity<InputState>,
    search_text: Entity<InputState>,
    name: Entity<InputState>,
    name_empty: Entity<InputState>,
    code: Entity<InputState>,
    inline_name: Entity<InputState>,
    inline_role: Entity<InputState>,
    sheet_name: Entity<InputState>,
    sheet_role: Entity<InputState>,
}

impl NativeKit {
    fn new(page: u8, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut input = |placeholder: &'static str, value: &'static str| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .default_value(value)
            })
        };
        Self {
            page,
            search: input(crate::copy::common::SEARCH_PLACEHOLDER, ""),
            search_text: input(crate::copy::common::SEARCH_PLACEHOLDER, t::TOPIC_B),
            name: input(t::NAME_PLACEHOLDER, t::NAME_VALUE),
            name_empty: input(t::NAME_PLACEHOLDER, ""),
            code: input(t::CODE_PLACEHOLDER, ""),
            inline_name: input(t::NAME_PLACEHOLDER, t::NAME_VALUE),
            inline_role: input(t::ROLE, t::ROLE_VALUE),
            sheet_name: input(t::NAME_PLACEHOLDER, t::NAME_VALUE),
            sheet_role: input(t::ROLE, t::ROLE_VALUE),
        }
    }
}

/// A small `secondary` caption over a sample.
fn caption(text: &'static str, k: &NativeColors) -> gpui::Div {
    div().n_subheadline().text_color(k.secondary).child(text)
}

/// A gallery section: a Section-style title and its samples.
fn section(title: &'static str, k: &NativeColors) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(div().n_section().text_color(k.secondary).child(title))
}

/// A labeled sample: the caption above the element.
fn sample(label: &'static str, el: impl IntoElement, k: &NativeColors) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(caption(label, k))
        .child(el)
}

fn hstack() -> gpui::Div {
    div().flex().items_center().gap(px(12.))
}

impl NativeKit {
    fn buttons(&self, window: &mut Window, cx: &mut App) -> AnyElement {
        let k = native_colors(cx);
        let reg = PushSize::Regular;
        section(t::SEC_BUTTONS, &k)
            .child(
                hstack()
                    .child(sample(
                        t::STATE_REST,
                        push_button("kb-1", t::EDIT, PushStyle::Outline, reg, cx),
                        &k,
                    ))
                    .child(sample(
                        t::STATE_PRESSED,
                        push_button("kb-2", t::EDIT, PushStyle::Outline, reg, cx).bg(k.fill_hover),
                        &k,
                    ))
                    .child(sample(
                        t::STATE_DISABLED,
                        disabled_push(
                            push_button("kb-3", t::EDIT, PushStyle::Outline, reg, cx),
                            true,
                            cx,
                        ),
                        &k,
                    ))
                    .child(sample(
                        t::STATE_REST,
                        push_button("kb-4", t::APPROVE, PushStyle::Default, reg, cx),
                        &k,
                    ))
                    .child(sample(
                        t::STATE_PRESSED,
                        push_button("kb-5", t::APPROVE, PushStyle::Default, reg, cx)
                            .bg(k.primary.opacity(0.82)),
                        &k,
                    ))
                    .child(sample(
                        t::STATE_REST,
                        push_button("kb-6", t::SIGN_OUT, PushStyle::Destructive, reg, cx),
                        &k,
                    )),
            )
            .child(
                hstack()
                    .child(push_button(
                        "kb-s1",
                        t::SEE_CHANGES,
                        PushStyle::Outline,
                        PushSize::Small,
                        cx,
                    ))
                    .child(push_button(
                        "kb-s2",
                        t::APPROVE,
                        PushStyle::Default,
                        PushSize::Small,
                        cx,
                    ))
                    .child(push_button(
                        "kb-l1",
                        t::CANCEL,
                        PushStyle::Outline,
                        PushSize::Large,
                        cx,
                    ))
                    .child(push_button(
                        "kb-l2",
                        t::CONTINUE,
                        PushStyle::Default,
                        PushSize::Large,
                        cx,
                    )),
            )
            .child(
                hstack()
                    .child(popup_button(
                        "kb-pop",
                        t::ON_LAUNCH_HOME,
                        PushSize::Regular,
                        window,
                        cx,
                    ))
                    .child(pull_down(
                        "kb-pull",
                        t::INVITE,
                        PushSize::Regular,
                        window,
                        cx,
                    ))
                    .child(borderless_button("kb-open", Sf::Open, t::OPEN, window, cx))
                    .child(
                        borderless_button("kb-more", Sf::More, t::OPEN, window, cx)
                            .bg(k.fill_hover),
                    )
                    .child(borderless_text("kb-edit", t::EDIT, cx)),
            )
            .into_any_element()
    }

    fn controls(&self, cx: &mut App) -> AnyElement {
        let k = native_colors(cx);
        section(t::SEC_CONTROLS, &k)
            .child(
                hstack()
                    .gap(px(16.))
                    .child(sample(
                        t::STATE_SELECTED,
                        NativeSwitch::new("ks-1", true).name(t::SOUND),
                        &k,
                    ))
                    .child(sample(
                        t::STATE_REST,
                        NativeSwitch::new("ks-2", false).name(t::SOUND),
                        &k,
                    ))
                    .child(sample(
                        t::STATE_FOCUSED,
                        NativeSwitch::new("ks-3", true)
                            .show_focus(true)
                            .name(t::SOUND),
                        &k,
                    ))
                    .child(sample(
                        t::STATE_DISABLED,
                        NativeSwitch::new("ks-4", true)
                            .disabled(true)
                            .name(t::SOUND),
                        &k,
                    ))
                    .child(sample(
                        t::STATE_SELECTED,
                        NativeSwitch::new("ks-5", true)
                            .size(SwitchSize::Regular)
                            .name(t::SOUND),
                        &k,
                    ))
                    .child(sample(
                        t::STATE_REST,
                        NativeSwitch::new("ks-6", false)
                            .size(SwitchSize::Regular)
                            .name(t::SOUND),
                        &k,
                    )),
            )
            .child(
                hstack()
                    .gap(px(16.))
                    .child(NativeCheckbox::new("kc-1", true).label(t::LAUNCH_AT_LOGIN))
                    .child(NativeCheckbox::new("kc-2", false).label(t::MENU_BAR))
                    .child(NativeCheckbox::new("kc-3", true).show_focus(true))
                    .child(NativeCheckbox::new("kc-4", false).show_focus(true))
                    .child(
                        NativeCheckbox::new("kc-5", true)
                            .disabled(true)
                            .label(t::SOUND),
                    ),
            )
            .child(
                hstack()
                    .gap(px(16.))
                    .child(NativeRadio::new("kr-1", true).label(t::ALL_MESSAGES))
                    .child(NativeRadio::new("kr-2", false).label(t::ONLY_MENTIONS))
                    .child(
                        NativeRadio::new("kr-3", true)
                            .disabled(true)
                            .label(t::SOUND),
                    ),
            )
            .child(
                hstack()
                    .child(Segmented::new(
                        "kseg-1",
                        [t::SEG_ALL, t::SEG_WORKING, t::SEG_WAITING]
                            .map(|l| Segment::Label(l.into())),
                    ))
                    .child(
                        Segmented::new(
                            "kseg-2",
                            [
                                Segment::Symbol(Sf::Image, t::GRID.into()),
                                Segment::Symbol(Sf::Document, t::LIST.into()),
                            ],
                        )
                        .selected(1)
                        .toolbar(),
                    ),
            )
            .into_any_element()
    }

    fn fields(&self, cx: &mut App) -> AnyElement {
        let k = native_colors(cx);
        section(t::SEC_FIELDS, &k)
            .child(
                hstack()
                    .items_start()
                    .child(sample(
                        t::STATE_REST,
                        NativeTextField::new("kf-1", &self.name),
                        &k,
                    ))
                    .child(sample(
                        t::STATE_FOCUSED,
                        NativeTextField::new("kf-2", &self.name_empty).show_focus(true),
                        &k,
                    )),
            )
            .child(
                hstack()
                    .items_start()
                    .child(sample(
                        t::STATE_DISABLED,
                        NativeTextField::new("kf-3", &self.name).disabled(true),
                        &k,
                    ))
                    .child(sample(
                        t::STATE_REST,
                        NativeTextField::new("kf-4", &self.code)
                            .size(FieldSize::Large)
                            .width(metrics::FIELD_CODE_WIDTH),
                        &k,
                    )),
            )
            .child(
                hstack()
                    .child(SearchField::new("kf-s1", &self.search).width(px(200.)))
                    .child(
                        SearchField::new("kf-s2", &self.search_text)
                            .width(px(200.))
                            .show_focus(true),
                    ),
            )
            .into_any_element()
    }

    fn form(&self, window: &mut Window, cx: &mut App) -> AnyElement {
        let k = native_colors(cx);
        let device = |sf: Sf, window: &Window, cx: &App| {
            symbol(
                sf,
                px(18.),
                Weight::Regular,
                native_colors(cx).secondary,
                window,
            )
        };
        section(t::SEC_FORM, &k)
            .w(px(440.))
            .child(
                GroupedForm::new()
                    .header(t::FORM_GENERAL)
                    .row(FormRow::new(t::FORM_ON_LAUNCH).control(popup_button(
                        "kform-pop",
                        t::ON_LAUNCH_HOME,
                        PushSize::Regular,
                        window,
                        cx,
                    )))
                    .row(
                        FormRow::new(t::LAUNCH_AT_LOGIN)
                            .control(NativeSwitch::new("kform-s1", true).name(t::LAUNCH_AT_LOGIN)),
                    )
                    .row(
                        FormRow::new(t::SOUND)
                            .control(NativeSwitch::new("kform-s2", false).name(t::SOUND)),
                    ),
            )
            .child(
                GroupedForm::new()
                    .row(FormRow::new(t::NAME).control(InlineValueField::new(
                        "kform-name",
                        &self.inline_name,
                        t::NAME_VALUE,
                    )))
                    .row(
                        FormRow::new(t::ROLE).control(
                            InlineValueField::new("kform-role", &self.inline_role, t::ROLE_VALUE)
                                .show_focus(true),
                        ),
                    ),
            )
            .child(
                GroupedForm::new()
                    .header(t::FORM_DEVICES)
                    // Row actions are visible in the row: a ghost 28 "⋯"
                    // (§2.19), never select-then-"−".
                    .row(
                        FormRow::new(t::DEVICE_A)
                            .leading(device(Sf::Computer, window, cx))
                            .control(device_value("kform-more-a", t::THIS_MAC, window, cx)),
                    )
                    .row(
                        FormRow::new(t::DEVICE_B)
                            .leading(device(Sf::Machines, window, cx))
                            .control(device_value("kform-more-b", t::LAST_SEEN, window, cx)),
                    ),
            )
            .into_any_element()
    }

    fn page0(&self, window: &mut Window, cx: &mut App) -> AnyElement {
        let k = native_colors(cx);
        // The controls on their own: the window anatomy (sidebar, toolbar)
        // is the real main window's, so the kit no longer draws an older
        // copy of it (review 2 R1-14).
        div()
            .size_full()
            .bg(k.content)
            .flex()
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    // The first caption clears the traffic lights.
                    .pt(px(40.))
                    .child(
                        div()
                            .px(px(20.))
                            .pt(px(16.))
                            .flex()
                            .gap(px(32.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(24.))
                                    .child(self.buttons(window, cx))
                                    .child(self.controls(cx))
                                    .child(self.fields(cx)),
                            )
                            .child(self.form(window, cx)),
                    ),
            )
            .into_any_element()
    }

    fn page1(&self, window: &mut Window, cx: &mut App) -> AnyElement {
        let k = native_colors(cx);
        let menu = section(t::SEC_MENU, &k).child(
            hstack()
                .items_start()
                .gap(px(24.))
                .child(
                    menu_surface(cx)
                        .child(MenuRow::new(t::MENU_RENAME).symbol(Sf::Document))
                        .child(
                            MenuRow::new(t::MENU_COPY_LINK)
                                .symbol(Sf::Copy)
                                .highlighted(true),
                        )
                        .child(
                            MenuRow::new(t::MENU_OPEN_WINDOW)
                                .symbol(Sf::Open)
                                .disabled(true),
                        )
                        .child(menu_separator(true, cx))
                        .child(
                            MenuRow::new(t::MENU_LEAVE)
                                .symbol(Sf::Close)
                                .destructive(true),
                        ),
                )
                .child(
                    menu_surface(cx)
                        .child(MenuRow::new(t::ON_LAUNCH_HOME).checked(true))
                        .child(
                            MenuRow::new(t::ON_LAUNCH_LAST)
                                .checked(false)
                                .highlighted(true),
                        )
                        .child(menu_separator(true, cx))
                        .child(
                            MenuRow::new(t::OPEN)
                                .checked(false)
                                .shortcut(t::SHORTCUT_OPEN),
                        )
                        .child(
                            MenuRow::new(t::MENU_COPY_LINK)
                                .checked(false)
                                .shortcut(t::SHORTCUT_COPY),
                        ),
                )
                .child(
                    div().pt(px(8.)).child(
                        popover_surface(px(40.), cx).w(px(240.)).p(px(12.)).child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .child(label_value(t::INS_MEMBER, t::INS_MEMBER_VALUE, cx))
                                .child(label_value(t::INS_TIME, t::INS_TIME_VALUE, cx)),
                        ),
                    ),
                ),
        );
        let inspector = section(t::SEC_INSPECTOR, &k).child(
            inspector_column(cx)
                .h(px(330.))
                .child(
                    InspectorSection::new("ki-owner", t::INS_OWNER).row(label_value(
                        t::INS_MEMBER,
                        t::INS_MEMBER_VALUE,
                        cx,
                    )),
                )
                .child(InspectorSection::new("ki-plan", t::INS_PLAN).collapsed(true))
                .child(
                    InspectorSection::new("ki-where", t::INS_WHERE)
                        .action(borderless_button("ki-open", Sf::Open, t::OPEN, window, cx))
                        .row(label_value(
                            t::INS_COMPUTER,
                            // Online is a StatusBadge, never a loose dot
                            // (§2.12, R1-14).
                            super::StatusBadge::new(
                                super::BadgeState::Online,
                                t::INS_COMPUTER_VALUE,
                            ),
                            cx,
                        )),
                )
                .child(
                    InspectorSection::new("ki-cost", t::INS_COST)
                        .row(label_value(t::INS_TIME, t::INS_TIME_VALUE, cx))
                        .row(label_value(t::INS_COST, t::INS_COST_VALUE, cx))
                        .row(div().pt(px(6.)).child(super::progress(0.44, cx))),
                ),
        );
        let row = |id: &'static str, title: &'static str, cx: &App| {
            let _ = cx;
            CompactRow::new(id)
                .child(div().flex_1().child(title))
                .child(div().n_subheadline().child(t::TIME_A))
        };
        let list = section(t::SEC_LIST, &k).w(px(320.)).child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(caption(t::STATE_SELECTED, &k).pl(px(16.)))
                .child(row("kl-1", t::TOPIC_A, cx).selected(true))
                .child(caption(t::STATE_FOCUSED, &k).pl(px(16.)).pt(px(6.)))
                // Keyboard focus is the ring, never the selected fill
                // (E3, R1-01); the scene draws it without a key press.
                .child(
                    row("kl-2", t::TOPIC_B, cx)
                        .on_click(|_, _, _| {})
                        .show_focus(true),
                )
                .child(caption(t::STATE_SELECTED, &k).pl(px(16.)).pt(px(6.)))
                .child(
                    row("kl-3", t::TOPIC_C, cx)
                        .selected(true)
                        .list_focused(false),
                )
                .child(caption(t::STATE_INACTIVE, &k).pl(px(16.)).pt(px(6.)))
                .child(row("kl-4", t::TOPIC_D, cx).selected(true).inactive(true))
                .child(
                    SidebarRow::new("kl-sb", t::HOME)
                        .symbol(Sf::Desk)
                        .selected(true)
                        .inactive(true),
                ),
        );
        let progress = section(t::SEC_PROGRESS, &k).w(px(320.)).child(
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(caption(t::PROGRESS_LABEL, &k))
                // D12: one Progress (and its steps variant), no colored bars.
                .child(super::progress(0.62, cx))
                .child(super::progress_steps(3, 5, cx))
                .child(hstack().child(crate::ui::motion::spinner())),
        );
        // A sheet attached under the toolbar of a dimmed window.
        let app_icon = div()
            .size(metrics::ALERT_ICON)
            .rounded(px(11.))
            .bg(k.primary)
            .flex()
            .items_center()
            .justify_center()
            .child(symbol(
                Sf::SideChat,
                px(26.),
                Weight::Medium,
                k.on_primary,
                window,
            ));
        let sheet = section(t::SEC_SHEET, &k).child(
            div()
                .w(px(560.))
                .h(px(380.))
                .rounded(px(12.))
                .overflow_hidden()
                .bg(k.content)
                .border_1()
                .border_color(k.separator)
                .relative()
                .child(ToolbarRow::new(t::TOOLBAR_TITLE).subtitle(t::TOOLBAR_SUBTITLE))
                .child(div().absolute().inset_0().bg(k.dim))
                .child(
                    div()
                        .absolute()
                        .top(metrics::TOOLBAR_ROW)
                        .left(px(50.))
                        .child(
                            sheet_surface(SheetWidth::Form, cx).child(
                                SheetLayout::new(t::SHEET_TITLE)
                                    .explanation(t::SHEET_EXPLANATION)
                                    .body(
                                        GroupedForm::new()
                                            .row(
                                                FormRow::new(t::NAME).control(
                                                    NativeTextField::new(
                                                        "ksh-name",
                                                        &self.sheet_name,
                                                    )
                                                    .show_focus(true),
                                                ),
                                            )
                                            .row(FormRow::new(t::ROLE).control(
                                                NativeTextField::new("ksh-role", &self.sheet_role),
                                            )),
                                    )
                                    .buttons(sheet_buttons("ksh", t::CANCEL, t::CONTINUE, cx)),
                            ),
                        ),
                ),
        );
        let alert = section(t::SEC_ALERT, &k).child(
            alert_surface(cx).child(
                AlertLayout::new(t::ALERT_TITLE)
                    .icon(app_icon)
                    .message(t::ALERT_MESSAGE)
                    .checkbox(NativeCheckbox::new("ka-check", false).label(t::DELETE_LOCAL))
                    .buttons([
                        alert_button("ka-ok", t::ALERT_CONFIRM, PushStyle::Outline, true, cx),
                        alert_button("ka-cancel", t::CANCEL, PushStyle::Default, false, cx),
                    ]),
            ),
        );
        div()
            .size_full()
            .bg(k.content)
            .p(px(24.))
            .pt(metrics::TOOLBAR_ROW)
            .flex()
            .flex_col()
            .gap(px(28.))
            .child(
                hstack()
                    .items_start()
                    .gap(px(40.))
                    .child(menu)
                    .child(inspector)
                    .child(list),
            )
            .child(
                hstack()
                    .items_start()
                    .gap(px(40.))
                    .child(sheet)
                    .child(alert)
                    .child(progress),
            )
            .into_any_element()
    }
}

impl NativeKit {
    /// The completeness kit: every new component in its states.
    fn page2(&self, window: &mut Window, cx: &mut App) -> AnyElement {
        use super::{
            Alert, AlertTone, Badge, BadgeState, BadgeVariant, Card, Column, CountPill, Empty,
            EmptyLine, IconSize, KeyCap, LIST_TITLE_INSET, ListRow, PushSize, PushStyle,
            SkeletonKind, StatusBadge, icon_button, list_frame, media_box, push_button,
            skeleton_rows, table, tokens::space,
        };
        use crate::ui::Sym;
        let k = native_colors(cx);
        let badges = section(t::SEC_BADGES, &k).child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(space::S8)
                .child(Badge::new(t::BADGE_DEFAULT).variant(BadgeVariant::Default))
                .child(Badge::new(t::BADGE_SECONDARY).secondary())
                .child(Badge::new(t::BADGE_OUTLINE))
                .child(Badge::new(t::BADGE_DESTRUCTIVE).variant(BadgeVariant::Destructive))
                .child(CountPill::new(3))
                .child(CountPill::new(128))
                .child(KeyCap::new(t::KEY_CMD))
                .child(KeyCap::new(t::KEY_K)),
        );
        let states = [
            (BadgeState::NeedsYou, t::ST_NEEDS_YOU),
            (BadgeState::Working, t::ST_WORKING),
            (BadgeState::Stopping, t::ST_STOPPING),
            (BadgeState::Queued, t::ST_QUEUED),
            (BadgeState::Done, t::ST_DONE),
            (BadgeState::Failed, t::ST_FAILED),
            (BadgeState::Abnormal, t::ST_ABNORMAL),
            (BadgeState::Online, t::ST_ONLINE),
            (BadgeState::Offline, t::ST_OFFLINE),
            (BadgeState::Deactivated, t::ST_DEACTIVATED),
            (BadgeState::Paused, t::ST_PAUSED),
            (BadgeState::Merging, t::ST_MERGING),
            (BadgeState::Quiet, t::ST_WAITING),
        ];
        let status = section(t::SEC_STATUS, &k).child(
            div()
                .w_full()
                .flex()
                .flex_wrap()
                .gap(space::S8)
                .children(states.into_iter().map(|(s, l)| StatusBadge::new(s, l))),
        );
        let buttons = section(t::SEC_BUTTONS, &k).child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(space::S8)
                .child(push_button(
                    "k3-p",
                    t::APPROVE,
                    PushStyle::Default,
                    PushSize::Regular,
                    cx,
                ))
                .child(push_button(
                    "k3-o",
                    t::CANCEL,
                    PushStyle::Outline,
                    PushSize::Regular,
                    cx,
                ))
                .child(push_button(
                    "k3-s",
                    t::SEE_CHANGES,
                    PushStyle::Secondary,
                    PushSize::Regular,
                    cx,
                ))
                .child(push_button(
                    "k3-g",
                    t::EDIT,
                    PushStyle::Ghost,
                    PushSize::Regular,
                    cx,
                ))
                .child(push_button(
                    "k3-d",
                    t::REMOVE,
                    PushStyle::Destructive,
                    PushSize::Small,
                    cx,
                ))
                .child(push_button(
                    "k3-l",
                    t::CONTINUE,
                    PushStyle::Default,
                    PushSize::Large,
                    cx,
                ))
                .child(icon_button(
                    "k3-i",
                    Sf::More,
                    t::OPEN,
                    IconSize::Regular,
                    window,
                    cx,
                ))
                .child(icon_button(
                    "k3-i2",
                    Sf::Plus,
                    t::ADD,
                    IconSize::Small,
                    window,
                    cx,
                )),
        );
        let card = section(t::SEC_CARD, &k).child(
            Card::new()
                .width(px(440.))
                .title(t::CARD_TITLE)
                .description(t::CARD_DESCRIPTION)
                .action(Badge::new(t::VERSION_3))
                .child(div().n_body().text_color(k.label).child(t::CARD_BODY))
                .footer(push_button(
                    "k3-card",
                    t::CARD_MORE,
                    PushStyle::Outline,
                    PushSize::Small,
                    cx,
                )),
        );
        let rows = vec![
            ListRow::new("k3-r1", t::TOPIC_A)
                .media(media_box(Sym::Topic, cx))
                .meta(t::GROUP_DEV)
                .status(StatusBadge::new(BadgeState::NeedsYou, t::ST_NEEDS_YOU))
                .time(t::TIME_A)
                .time_width(px(40.))
                .actions(push_button(
                    "k3-r1-a",
                    t::OPEN,
                    PushStyle::Outline,
                    PushSize::Small,
                    cx,
                ))
                .into_any_element(),
            ListRow::new("k3-r2", t::TOPIC_B)
                .media(media_box(Sym::Topic, cx))
                .meta(t::GROUP_DEV)
                .status(StatusBadge::new(BadgeState::Working, t::ST_WORKING))
                .time(t::TIME_B)
                .time_width(px(40.))
                .actions(push_button(
                    "k3-r2-a",
                    t::OPEN,
                    PushStyle::Outline,
                    PushSize::Small,
                    cx,
                ))
                .selected(true)
                .into_any_element(),
        ];
        let list = section(t::SEC_LIST, &k).child(div().w_full().child(list_frame(
            rows,
            LIST_TITLE_INSET,
            Some(div().child(t::LIST_FOOTER).into_any_element()),
            cx,
        )));
        let cols = [
            Column::flex(t::COL_NAME),
            Column::fixed(t::COL_KIND, px(80.)),
            Column::fixed(t::COL_VERSION, px(64.)).right(),
            Column::fixed(t::COL_UPDATED, px(80.)).right(),
        ];
        // §2.21: a cell cuts with "…" and shows the whole value in a
        // tooltip (T1-19).
        let cell = |s: &'static str| crate::ui::truncate::TruncText::new(s).into_any_element();
        let data = vec![
            vec![
                cell(t::TOPIC_A),
                Badge::new(t::KIND_PAGE).dense().into_any_element(),
                cell(t::VERSION_3),
                cell(t::TIME_A),
            ],
            vec![
                cell(t::TOPIC_B),
                Badge::new(t::KIND_DOC).dense().into_any_element(),
                cell(t::VERSION_1),
                cell(t::TIME_B),
            ],
        ];
        let tables = section(t::SEC_TABLE, &k).child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(space::S16)
                .child(table(
                    &cols,
                    data,
                    false,
                    t::TABLE_EMPTY,
                    Some(div().child(t::TABLE_COUNT).into_any_element()),
                    cx,
                ))
                .child(table(&cols, Vec::new(), true, t::TABLE_EMPTY, None, cx))
                .child(
                    EmptyLine::new(t::EMPTY_LINE)
                        .icon(Sym::Checklist)
                        .action(push_button(
                            "k3-e3",
                            t::ASSIGN,
                            PushStyle::Ghost,
                            PushSize::Small,
                            cx,
                        )),
                ),
        );
        // A page table with its footer: the count, 第 2 / 3 页 and the four
        // page buttons (§2.7).
        let page_table = section(t::SEC_TABLE, &k).child({
            use super::page::{Col, Pager, frame, table_footer};
            let page_cols = || {
                vec![
                    Col {
                        header: t::COL_NAME.into(),
                        width: None,
                        right: false,
                    },
                    Col {
                        header: t::COL_VERSION.into(),
                        width: Some(px(64.)),
                        right: true,
                    },
                    Col {
                        header: t::COL_UPDATED.into(),
                        width: Some(px(80.)),
                        right: true,
                    },
                ]
            };
            let page_rows = vec![(
                vec![cell(t::TOPIC_A), cell(t::VERSION_3), cell(t::TIME_A)],
                Some(
                    Box::new(|_: &gpui::ClickEvent, _: &mut Window, _: &mut App| {})
                        as Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)>,
                ),
            )];
            div().flex().flex_col().gap(space::S16).child(
                frame(cx)
                    .child(super::page::table("k3-page", &page_cols(), page_rows, cx))
                    .child(table_footer(
                        "k3-page-footer",
                        t::TABLE_COUNT,
                        Pager {
                            page: 2,
                            pages: 3,
                            go: Some(std::rc::Rc::new(|_, _, _| {})),
                        },
                        cx,
                    )),
            )
        });
        let empty = section(t::SEC_EMPTY, &k).child(
            div().w_full().flex().flex_col().gap(space::S16).child(
                div().h(px(260.)).child(
                    Empty::new(Sym::Done, t::EMPTY_TITLE)
                        .description(t::EMPTY_DESCRIPTION)
                        .action(push_button(
                            "k3-e1",
                            t::EMPTY_START,
                            PushStyle::Default,
                            PushSize::Small,
                            cx,
                        ))
                        .action(push_button(
                            "k3-e2",
                            t::EMPTY_OUTPUTS,
                            PushStyle::Outline,
                            PushSize::Small,
                            cx,
                        ))
                        .dashed(),
                ),
            ),
        );
        let loading = section(t::SEC_LOADING, &k).child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(space::S16)
                .child(skeleton_rows(SkeletonKind::Message, 2))
                .child(skeleton_rows(SkeletonKind::ListRow, 2)),
        );
        let feedback = section(t::SEC_FEEDBACK, &k).child(
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(space::S12)
                .child(
                    Alert::new(Sym::Offline, t::ALERT_OFFLINE)
                        .tone(AlertTone::Warning)
                        .description(t::ALERT_OFFLINE_DETAIL)
                        .action(push_button(
                            "k3-a1",
                            t::ALERT_RECONNECT,
                            PushStyle::Outline,
                            PushSize::Small,
                            cx,
                        )),
                )
                .child(
                    Alert::new(Sym::Failed, t::ALERT_FAILED)
                        .tone(AlertTone::Destructive)
                        .description(t::ALERT_FAILED_DETAIL)
                        .action(push_button(
                            "k3-a2",
                            t::ALERT_RETRY,
                            PushStyle::Outline,
                            PushSize::Small,
                            cx,
                        )),
                )
                .child(
                    crate::ui::overlay::Toast::new(Sym::Done, t::TOAST_TEXT)
                        .tone(crate::ui::overlay::Tone::Success)
                        .description(t::TOAST_DETAIL)
                        .action(push_button(
                            "k3-t1",
                            t::TOAST_UNDO,
                            PushStyle::Outline,
                            PushSize::Small,
                            cx,
                        )),
                ),
        );
        // The state matrix of the current kit (T1-18, §1.8): rest, hover,
        // pressed, focus (3 px ring), loading and disabled, for the
        // default and the outline button.
        // One line per state (§1.8): its name, then the default and the
        // outline button in it. The buttons are wide enough for the spinner
        // beside the label: the width stays while loading (R1-14).
        let button = |style: PushStyle, label: &'static str, id: String| {
            push_button(id, label, style, PushSize::Small, cx).w(px(88.))
        };
        let state = |n: &'static str, style: PushStyle, label: &'static str| {
            let look = super::buttons::look(style, &k);
            let b = button(style, label, format!("k3-m-{n}-{label}"));
            match n {
                "hover" => b.bg(look.hover).into_any_element(),
                "pressed" => b.bg(look.pressed).into_any_element(),
                "focus" => div()
                    .rounded(super::tokens::radius::CONTROL)
                    .shadow(k.focus_ring())
                    .child(b)
                    .into_any_element(),
                "loading" => super::loading_push(b, true, PushSize::Small).into_any_element(),
                "disabled" => super::disabled_push(b, true, cx).into_any_element(),
                _ => b.into_any_element(),
            }
        };
        let states = [
            ("rest", t::STATE_REST),
            ("hover", t::STATE_HOVER),
            ("pressed", t::STATE_PRESSED),
            ("focus", t::STATE_FOCUSED),
            ("loading", t::STATE_LOADING),
            ("disabled", t::STATE_DISABLED),
        ];
        let matrix =
            section(t::SEC_STATES, &k).child(div().flex().flex_col().gap(space::S8).children(
                states.map(|(n, name)| {
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S12)
                        .child(
                            div()
                                .w(px(48.))
                                .n_footnote()
                                .text_color(k.secondary)
                                .child(name),
                        )
                        .child(state(n, PushStyle::Default, t::CONTINUE))
                        .child(state(n, PushStyle::Outline, t::CANCEL))
                }),
            ));
        let col = || div().w(px(440.)).flex().flex_col().gap(space::S24);
        div()
            .size_full()
            .bg(k.content)
            .p(space::S24)
            // The traffic lights sit in the top 52 band; content starts
            // under it (T1-19).
            .pt(metrics::TOOLBAR_ROW)
            .flex()
            .gap(space::S24)
            .child(
                col()
                    .child(badges)
                    .child(status)
                    .child(buttons)
                    .child(card)
                    .child(matrix),
            )
            .child(col().child(list).child(tables).child(page_table))
            .child(col().child(feedback).child(empty).child(loading))
            .into_any_element()
    }
}

impl Render for NativeKit {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.page {
            0 => self.page0(window, cx),
            1 => self.page1(window, cx),
            _ => self.page2(window, cx),
        }
    }
}
