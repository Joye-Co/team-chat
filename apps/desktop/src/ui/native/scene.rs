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
    Pixels, Render, Size, Styled, Window, WindowBounds, div, prelude::FluentBuilder as _, px,
};
use gpui_component::input::InputState;

use super::{
    buttons::{
        PushSize, PushStyle, ToolbarItemKind, borderless_button, borderless_text, disabled_push,
        popup_button, pull_down, push_button, square_button, square_pair, toolbar_item,
    },
    fields::{FieldSize, NativeTextField, SearchField},
    form::{FormRow, GroupedForm, InlineValueField},
    inspector::{InspectorSection, ListRow, determinate_bar, inspector_column, label_value},
    menu::{MenuRow, menu_separator, menu_surface, popover_surface},
    segmented::{Segment, Segmented},
    sheet::{
        AlertLayout, SheetLayout, SheetWidth, alert_button, alert_surface, sheet_buttons,
        sheet_surface,
    },
    sidebar::{SidebarRow, SidebarSection, floating_sidebar},
    toggles::{NativeCheckbox, NativeRadio, NativeSwitch, SwitchSize},
    tokens::{NativeColors, NativeType as _, metrics, native_colors},
    toolbar::{ToolbarGroup, ToolbarRow},
};
use crate::{
    app::{Services, main_window_options},
    copy::native_kit as t,
    selftest::scenes::{Scene, Start, capture_bounds},
    ui::{
        MemberAvatar,
        sf_symbols::{Sf, Weight, symbol},
    },
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
];

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
    fn sidebar(&self, window: &mut Window, cx: &mut App) -> AnyElement {
        let k = native_colors(cx);
        let avatar = |cx: &App| MemberAvatar::new(None, px(16.)).on(native_colors(cx).sidebar);
        floating_sidebar(cx)
            .h(px(452.))
            .child(
                // The traffic lights row: the compose item at its trailing edge.
                div()
                    .h(metrics::TOOLBAR_ROW)
                    .pl(px(16.))
                    .pr(px(8.))
                    .flex()
                    .items_center()
                    .child(div().flex_1())
                    .child(toolbar_item(
                        "kit-compose",
                        Sf::NewTopic,
                        t::NEW_TOPIC,
                        ToolbarItemKind::Plain,
                        true,
                        window,
                        cx,
                    )),
            )
            .child(
                div()
                    .px(metrics::SIDEBAR_ROW_INSET)
                    .pb(px(8.))
                    .child(SearchField::new("kit-sb-search", &self.search).sidebar()),
            )
            .child(
                SidebarRow::new("kit-home", t::HOME)
                    .symbol(Sf::Desk)
                    .selected(true),
            )
            .child(SidebarRow::new("kit-outputs", t::OUTPUTS).symbol(Sf::Outputs))
            .child(SidebarRow::new("kit-members", t::MEMBERS).symbol(Sf::Members))
            .child(SidebarSection::new("kit-sec-dev", t::GROUP_DEV).show_chevron(true))
            .child(
                SidebarRow::new("kit-t-a", t::TOPIC_A)
                    .state_symbol(Sf::Alert, k.amber)
                    .unread(true)
                    .trailing(t::TIME_A),
            )
            .child(
                SidebarRow::new("kit-t-b", t::TOPIC_B)
                    .leading(avatar(cx))
                    .count(3)
                    .unread(true),
            )
            .child(
                SidebarRow::new("kit-t-c", t::TOPIC_C)
                    .state_symbol(Sf::Done, k.secondary)
                    .trailing(t::TIME_B),
            )
            .child(SidebarSection::new("kit-sec-dm", t::GROUP_DM))
            .child(SidebarRow::new("kit-dm-a", t::INS_MEMBER_VALUE).leading(avatar(cx)))
            .into_any_element()
    }

    fn toolbar(&self, inactive: bool, window: &mut Window, cx: &mut App) -> AnyElement {
        let k = native_colors(cx);
        let active = !inactive;
        let item = |id: &'static str, sf, name, kind, window: &mut Window, cx: &mut App| {
            toolbar_item(id, sf, name, kind, active, window, cx)
        };
        let mut row = ToolbarRow::new(t::TOOLBAR_TITLE)
            .subtitle(t::TOOLBAR_SUBTITLE)
            .state(Sf::Alert, k.amber)
            .inactive(inactive)
            .group(ToolbarGroup::new([
                item(
                    "kit-tb-ask",
                    Sf::SideChat,
                    t::ASK_ALONE,
                    ToolbarItemKind::Plain,
                    window,
                    cx,
                ),
                // Hovered, as the pointer would show it.
                item(
                    "kit-tb-out",
                    Sf::Outputs,
                    t::OUTPUTS,
                    ToolbarItemKind::Plain,
                    window,
                    cx,
                )
                .when(!inactive, |b| b.bg(k.fill_hover)),
                item(
                    "kit-tb-pr",
                    Sf::Changes,
                    t::PR,
                    ToolbarItemKind::Plain,
                    window,
                    cx,
                ),
            ]))
            .group(ToolbarGroup::new([item(
                "kit-tb-insp",
                Sf::PaneRight,
                t::INSPECTOR,
                ToolbarItemKind::Toggled,
                window,
                cx,
            )]));
        if !inactive {
            row = row.group(
                ToolbarGroup::new([item(
                    "kit-tb-new",
                    Sf::NewTopic,
                    t::NEW_TOPIC,
                    ToolbarItemKind::Prominent,
                    window,
                    cx,
                )])
                .bare(),
            );
        }
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                caption(
                    if inactive {
                        t::STATE_INACTIVE
                    } else {
                        t::STATE_REST
                    },
                    &k,
                )
                .pl(px(20.)),
            )
            .child(row)
            .into_any_element()
    }

    fn buttons(&self, window: &mut Window, cx: &mut App) -> AnyElement {
        let k = native_colors(cx);
        let reg = PushSize::Regular;
        section(t::SEC_BUTTONS, &k)
            .child(
                hstack()
                    .child(sample(
                        t::STATE_REST,
                        push_button("kb-1", t::EDIT, PushStyle::Standard, reg, cx),
                        &k,
                    ))
                    .child(sample(
                        t::STATE_PRESSED,
                        push_button("kb-2", t::EDIT, PushStyle::Standard, reg, cx).bg(k.fill_hover),
                        &k,
                    ))
                    .child(sample(
                        t::STATE_DISABLED,
                        disabled_push(
                            push_button("kb-3", t::EDIT, PushStyle::Standard, reg, cx),
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
                            .bg(k.accent.opacity(0.82)),
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
                        PushStyle::Standard,
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
                        PushStyle::Standard,
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
                    .child(square_pair(
                        square_button("kb-add", true, t::ADD, window, cx),
                        square_button("kb-rm", false, t::REMOVE, window, cx),
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
                    .footer(t::FORM_DEVICES_FOOTER)
                    .row(
                        FormRow::new(t::DEVICE_A)
                            .leading(device(Sf::Computer, window, cx))
                            .value(t::THIS_MAC, cx),
                    )
                    .row(
                        FormRow::new(t::DEVICE_B)
                            .leading(device(Sf::Machines, window, cx))
                            .value(t::LAST_SEEN, cx),
                    )
                    .below(square_pair(
                        square_button("kform-add", true, t::ADD, window, cx),
                        square_button("kform-rm", false, t::REMOVE, window, cx),
                        cx,
                    )),
            )
            .into_any_element()
    }

    fn page0(&self, window: &mut Window, cx: &mut App) -> AnyElement {
        let k = native_colors(cx);
        div()
            .size_full()
            .bg(k.content)
            .flex()
            .child(self.sidebar(window, cx))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    // v12/shell: the first caption clears the window's top edge.
                    .pt(px(12.))
                    .child(self.toolbar(false, window, cx))
                    .child(self.toolbar(true, window, cx))
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
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(div().size(px(6.)).rounded_full().bg(k.green))
                                .child(t::INS_COMPUTER_VALUE),
                            cx,
                        )),
                )
                .child(
                    InspectorSection::new("ki-cost", t::INS_COST)
                        .row(label_value(t::INS_TIME, t::INS_TIME_VALUE, cx))
                        .row(label_value(t::INS_COST, t::INS_COST_VALUE, cx))
                        .row(div().pt(px(6.)).child(determinate_bar(0.44, None, cx))),
                ),
        );
        let row = |id: &'static str, title: &'static str, cx: &App| {
            let _ = cx;
            ListRow::new(id)
                .child(div().flex_1().child(title))
                .child(div().n_subheadline().child(t::TIME_A))
        };
        let list = section(t::SEC_LIST, &k).w(px(320.)).child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(caption(t::STATE_FOCUSED, &k).pl(px(16.)))
                .child(row("kl-1", t::TOPIC_A, cx).selected(true))
                .child(row("kl-2", t::TOPIC_B, cx))
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
                .child(determinate_bar(0.62, None, cx))
                .child(determinate_bar(0.3, Some(k.blue), cx))
                .child(determinate_bar(1.0, Some(k.green), cx))
                .child(hstack().child(crate::ui::motion::spinner())),
        );
        // A sheet attached under the toolbar of a dimmed window.
        let app_icon = div()
            .size(metrics::ALERT_ICON)
            .rounded(px(11.))
            .bg(k.accent)
            .flex()
            .items_center()
            .justify_center()
            .child(symbol(
                Sf::SideChat,
                px(26.),
                Weight::Medium,
                k.on_accent,
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
                        alert_button("ka-ok", t::ALERT_CONFIRM, PushStyle::Standard, true, cx),
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

impl Render for NativeKit {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.page {
            0 => self.page0(window, cx),
            _ => self.page1(window, cx),
        }
    }
}
