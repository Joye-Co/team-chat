//! The component gallery (DESIGN.md §16): every shared component of §7 in
//! its states, on the real surfaces, in one window. The self-test captures it
//! as the `components` scene in both themes, so a change to the foundation
//! shows up next to the mockups before a screen uses it.

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _,
    IntoElement, ParentElement, Render, SharedString, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Icon, Sizable as _,
    input::{InputState, Textarea, TextareaState},
    scroll::ScrollableElement as _,
};

mod v3;

use crate::ui::NamedButton as _;
use crate::{
    characters::{CharacterId, CharacterStatus},
    copy::common,
    data::{Member, MemberKind, MemberLevel, Topic, TopicStatus},
    theme::{self, ChipKind, Type as _, size, space},
    ui::{
        self, BadgeKind, ButtonKind, ButtonSize, ButtonSpec as _, ChipStyle, ComposerShell,
        CountBadge, FieldKind, GroupGlyph, Kbd, Lucide, MemberAvatar, RoleTag, RowSurface,
        SectionHeader, StatusChip, TextField,
    },
};

pub struct Gallery {
    search: Entity<InputState>,
    search_text: Entity<InputState>,
    form: Entity<InputState>,
    form_error: Entity<InputState>,
    form_disabled: Entity<InputState>,
    composer_empty: Entity<TextareaState>,
    composer_focused: Entity<TextareaState>,
    focused_once: bool,
    /// 0: buttons, fields, chips, avatars and rows; 1: the composer,
    /// overlays and surfaces.
    page: u8,
}

impl Gallery {
    pub fn new(page: u8, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = |placeholder: &'static str,
                     value: &'static str,
                     window: &mut Window,
                     cx: &mut Context<Self>| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .default_value(value)
            })
        };
        let search = input(common::SEARCH_PLACEHOLDER, "", window, cx);
        let search_text = input(common::SEARCH_PLACEHOLDER, "staging 验收", window, cx);
        let form = input("例如：产品研发", "qqbot 研发", window, cx);
        let form_error = input("", "", window, cx);
        let form_disabled = input("", "team.joye.co", window, cx);
        let composer_empty = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 8)
                .placeholder("回复「夜间告警降噪：合并重复的 5xx」")
        });
        let composer_focused = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 8)
                .placeholder("回复")
                .default_value("验收结果出来后直接发在这里，有问题先改规则，不用再问我")
        });
        Self {
            search,
            search_text,
            form,
            form_error,
            form_disabled,
            composer_empty,
            composer_focused,
            focused_once: false,
            page,
        }
    }
}

fn person(id: &str, name: &str, online: bool) -> Member {
    Member {
        id: id.into(),
        kind: MemberKind::Human,
        name: name.into(),
        role: None,
        level: MemberLevel::Member,
        avatar_url: None,
        deactivated: false,
        online,
        character: None,
    }
}

fn bot(id: &str, name: &str, character: &str) -> Member {
    Member {
        id: id.into(),
        kind: MemberKind::Bot,
        name: name.into(),
        role: None,
        level: MemberLevel::Member,
        avatar_url: None,
        deactivated: false,
        online: true,
        character: CharacterId::parse(character),
    }
}

fn topic(status: TopicStatus, waiting_on: &[&str]) -> Topic {
    Topic {
        id: String::new(),
        group_id: String::new(),
        title: String::new(),
        status,
        waiting_on: waiting_on.iter().map(|s| s.to_string()).collect(),
        owner_id: None,
        created_by: String::new(),
        created_at: 0,
        root_message_id: String::new(),
        last_seq: 0,
        last_activity_at: 0,
        message_count: 0,
        unread: 0,
        mentions: 0,
    }
}

fn section(title: &'static str, cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    div()
        .flex()
        .flex_col()
        .gap(space::S12)
        .child(div().t_section().text_color(k.text_3).child(title))
}

/// A state forced on a control, so the screenshot shows it without a
/// pointer or a keyboard (§16: every component in all its states).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Forced {
    Rest,
    Hover,
    Pressed,
    Focus,
    Loading,
}

const FORCED: [(Forced, &str); 5] = [
    (Forced::Rest, "rest"),
    (Forced::Hover, "hover"),
    (Forced::Pressed, "pressed"),
    (Forced::Focus, "focus"),
    (Forced::Loading, "loading"),
];

/// The focus-visible ring of a button (§7.1): 2 pt `focus` outside a 2 pt
/// gap. A border on a wrapper, not a shadow: a shadow shows through the
/// clear fill of a ghost button.
fn focus_ring(k: &theme::Colors, child: impl IntoElement) -> AnyElement {
    div()
        .flex_none()
        .p(px(2.))
        .border_2()
        .border_color(k.focus)
        .rounded(theme::radius::MD + px(4.))
        .child(child)
        .into_any_element()
}

/// The STATES section (§7.1): each button kind, the icon button and a list
/// row at rest, hover, pressed, focus-visible and loading, drawn with the
/// state's tokens instead of a pointer.
fn forced_states(cx: &App) -> gpui::Div {
    use gpui_component::ActiveTheme as _;
    let k = theme::colors(cx);
    let t = cx.theme();
    let label = |text: &'static str| {
        div()
            .w(px(64.))
            .flex_none()
            .t_meta()
            .text_color(k.text_3)
            .child(text)
            .into_any_element()
    };
    let mut lines: Vec<AnyElement> = vec![
        row_of(
            std::iter::once(label(""))
                .chain(FORCED.iter().map(|(_, n)| {
                    div()
                        .w(px(84.))
                        .t_meta()
                        .text_color(k.text_3)
                        .child(*n)
                        .into_any_element()
                }))
                .collect(),
        )
        .into_any_element(),
    ];
    for (kind, name, text) in [
        (ButtonKind::Primary, "primary", "发布"),
        (ButtonKind::Secondary, "secondary", "修改"),
        (ButtonKind::Ghost, "ghost", "放弃"),
        (ButtonKind::Danger, "danger", "删除"),
    ] {
        let (hover, pressed) = match kind {
            ButtonKind::Primary => (k.accent_hover, k.accent_pressed),
            ButtonKind::Danger => (t.danger_hover, t.danger_active),
            _ => (k.hover, k.pressed),
        };
        let mut line = vec![label(name)];
        for (state, sname) in FORCED {
            let b = ui::button(
                SharedString::from(format!("st-{name}-{sname}")),
                text,
                kind,
                ButtonSize::Md,
                cx,
            );
            let b = match state {
                Forced::Rest => b.into_any_element(),
                Forced::Hover => b.bg(hover).into_any_element(),
                Forced::Pressed => b.bg(pressed).into_any_element(),
                Forced::Focus => focus_ring(&k, b),
                Forced::Loading => b.loading(true).into_any_element(),
            };
            line.push(div().w(px(84.)).flex().child(b).into_any_element());
        }
        lines.push(row_of(line).into_any_element());
    }
    let mut icons = vec![label("icon")];
    for (state, sname) in FORCED {
        let b = ui::icon_button(SharedString::from(format!("st-icon-{sname}")), Lucide::Bell)
            .named("关注");
        let b = match state {
            Forced::Rest => b.into_any_element(),
            Forced::Hover => b.bg(k.hover).into_any_element(),
            Forced::Pressed => b.bg(k.pressed).into_any_element(),
            Forced::Focus => focus_ring(&k, b),
            Forced::Loading => b.loading(true).into_any_element(),
        };
        icons.push(div().w(px(84.)).flex().child(b).into_any_element());
    }
    lines.push(row_of(icons).into_any_element());
    let mut rows = vec![label("row")];
    for (state, sname) in FORCED {
        let r = ui::list_row(
            SharedString::from(format!("st-row-{sname}")),
            RowSurface::Content,
            false,
            cx,
        )
        .w(px(70.))
        .h(px(32.))
        .px(space::S8)
        .t_ui()
        .text_color(k.text_1)
        .child("话题");
        let r = match state {
            Forced::Rest => r.into_any_element(),
            Forced::Hover => r.bg(k.hover).into_any_element(),
            Forced::Pressed => r.bg(k.pressed).into_any_element(),
            // Rows draw the ring inset 1 pt (§7.1).
            Forced::Focus => r.border_2().border_color(k.focus).into_any_element(),
            Forced::Loading => div()
                .w(px(70.))
                .h(px(32.))
                .flex()
                .items_center()
                .px(space::S8)
                .child(
                    div()
                        .w_full()
                        .h(px(12.))
                        .rounded(theme::radius::XS)
                        .bg(k.field),
                )
                .into_any_element(),
        };
        rows.push(div().w(px(84.)).flex().child(r).into_any_element());
    }
    lines.push(row_of(rows).into_any_element());
    div().flex().flex_col().gap(space::S12).children(lines)
}

fn row_of(children: Vec<AnyElement>) -> gpui::Div {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(space::S8)
        .children(children)
}

impl Render for Gallery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focused_once && self.page == 1 {
            self.focused_once = true;
            self.composer_focused
                .read(cx)
                .focus_handle(cx)
                .focus(window, cx);
        }
        let k = theme::colors(cx);
        let focused = self
            .composer_focused
            .read(cx)
            .focus_handle(cx)
            .is_focused(window);

        // Buttons (§7.3)
        let mut buttons = Vec::new();
        for (size_, name) in [
            (ButtonSize::Sm, "sm"),
            (ButtonSize::Md, "md"),
            (ButtonSize::Page, "page"),
            (ButtonSize::Lg, "lg"),
        ] {
            let mut line: Vec<AnyElement> = vec![
                div()
                    .w(px(40.))
                    .t_meta()
                    .text_color(k.text_3)
                    .child(name)
                    .into_any_element(),
            ];
            for (kind, label) in [
                (ButtonKind::Primary, "发布"),
                (ButtonKind::Secondary, "修改"),
                (ButtonKind::Ghost, "放弃"),
                (ButtonKind::Danger, "删除"),
            ] {
                line.push(
                    ui::button(
                        SharedString::from(format!("b-{name}-{label}")),
                        label,
                        kind,
                        size_,
                        cx,
                    )
                    .into_any_element(),
                );
            }
            line.push(
                ui::button(
                    SharedString::from(format!("b-{name}-off")),
                    "加入",
                    ButtonKind::Primary,
                    size_,
                    cx,
                )
                .disabled_fill(true, cx)
                .into_any_element(),
            );
            buttons.push(row_of(line).into_any_element());
        }
        buttons.push(
            row_of(vec![
                div()
                    .w(px(40.))
                    .t_meta()
                    .text_color(k.text_3)
                    .child("icon")
                    .into_any_element(),
                ui::icon_button("ib-1", Lucide::Bell)
                    .named("关注")
                    .into_any_element(),
                ui::icon_button("ib-2", Lucide::PanelRight)
                    .named("话题详情")
                    .into_any_element(),
                ui::icon_button("ib-3", Lucide::Ellipsis)
                    .named("更多")
                    .into_any_element(),
                ui::icon_button("ib-4", Lucide::SquarePen)
                    .named("新话题")
                    .into_any_element(),
                ui::icon_button_sm("ib-5", Lucide::Plus)
                    .named("新建群")
                    .into_any_element(),
                ui::text_button("tb-1", "重试", cx).into_any_element(),
                ui::text_button("tb-2", "查看过程", cx).into_any_element(),
                ui::button(
                    "b-icon",
                    "添加成员",
                    ButtonKind::Primary,
                    ButtonSize::Page,
                    cx,
                )
                .icon(Icon::new(Lucide::UserPlus))
                .into_any_element(),
            ])
            .into_any_element(),
        );

        // Fields (§7.4)
        let fields = div()
            .flex()
            .flex_col()
            .gap(space::S12)
            .child(
                div().w(px(224.)).child(
                    TextField::new("f-search", &self.search, FieldKind::Search).shortcut("⌘K"),
                ),
            )
            .child(div().w(px(320.)).child(TextField::new(
                "f-search-b",
                &self.search_text,
                FieldKind::SearchBordered,
            )))
            .child(
                div()
                    .w(px(390.))
                    .flex()
                    .flex_col()
                    .child(ui::field_label("群名", cx))
                    .child(TextField::new("f-form", &self.form, FieldKind::Form))
                    .child(ui::field_help("成员会在侧栏看到这个名字", false, cx)),
            )
            .child(
                div()
                    .w(px(390.))
                    .flex()
                    .flex_col()
                    .child(ui::field_label("邀请码", cx))
                    .child(TextField::new("f-err", &self.form_error, FieldKind::Form).error(true))
                    .child(ui::field_help("邀请码不对，请检查后再试。", true, cx)),
            )
            .child(div().w(px(390.)).child(
                TextField::new("f-off", &self.form_disabled, FieldKind::Form).disabled(true),
            ))
            .child(row_of(vec![
                ui::select_trigger("sel-1", "成员", cx).into_any_element(),
                ui::select_trigger("sel-2", "管理员", cx).into_any_element(),
            ]));

        // Chips, badges, tags (§7.8, §7.9)
        let joye = "m-joye";
        let members = vec![person("m-ola", "Aleksandra Wiśniewska", true)];
        let chips = div()
            .flex()
            .flex_col()
            .gap(space::S12)
            .child(row_of(vec![
                StatusChip::labeled(
                    common::STATUS_WAITING_ANSWER_ME,
                    TopicStatus::WaitingAnswer,
                    ChipKind::You,
                )
                .into_any_element(),
                StatusChip::labeled(
                    common::STATUS_IN_PROGRESS,
                    TopicStatus::InProgress,
                    ChipKind::Info,
                )
                .into_any_element(),
                StatusChip::for_topic(
                    &topic(TopicStatus::WaitingAnswer, &[joye]),
                    Some(joye),
                    &members,
                )
                .map(|c| c.into_any_element())
                .unwrap_or_else(|| div().into_any_element()),
                StatusChip::for_topic(
                    &topic(TopicStatus::WaitingReview, &[joye]),
                    Some(joye),
                    &members,
                )
                .map(|c| c.into_any_element())
                .unwrap_or_else(|| div().into_any_element()),
                StatusChip::header_for_topic(
                    &topic(TopicStatus::WaitingAnswer, &[joye]),
                    Some(joye),
                    &members,
                )
                .map(|c| c.into_any_element())
                .unwrap_or_else(|| div().into_any_element()),
            ]))
            .child(row_of(vec![
                StatusChip::for_topic(&topic(TopicStatus::Blocked, &[]), Some(joye), &members)
                    .map(|c| c.into_any_element())
                    .unwrap_or_else(|| div().into_any_element()),
                StatusChip::for_topic(&topic(TopicStatus::Done, &[]), Some(joye), &members)
                    .map(|c| c.into_any_element())
                    .unwrap_or_else(|| div().into_any_element()),
                StatusChip::for_topic(
                    &topic(TopicStatus::WaitingReview, &["m-ola"]),
                    Some(joye),
                    &members,
                )
                .map(|c| c.into_any_element())
                .unwrap_or_else(|| div().into_any_element()),
                StatusChip::labeled("等待加入", TopicStatus::Open, ChipKind::Quiet)
                    .style(ChipStyle::Label)
                    .into_any_element(),
            ]))
            .child(row_of(vec![
                CountBadge::new(1, BadgeKind::Mention).into_any_element(),
                CountBadge::new(2, BadgeKind::MentionCount).into_any_element(),
                CountBadge::new(128, BadgeKind::Mention).into_any_element(),
                CountBadge::new(5, BadgeKind::Unread).into_any_element(),
                CountBadge::new(1000, BadgeKind::Unread).into_any_element(),
                CountBadge::new(12, BadgeKind::Count).into_any_element(),
                RoleTag::new("开发").into_any_element(),
                RoleTag::new("测试验收").into_any_element(),
                Kbd::new("⌘K").into_any_element(),
                Kbd::new("A").into_any_element(),
                Kbd::new("esc").into_any_element(),
            ]));

        // Avatars (§7.9)
        let people = [
            person("p1", "王一鸣", true),
            person("p2", "Alex Chen", false),
            person("p3", "🧑‍💻 Priya", true),
            person("p4", "李雪", true),
        ];
        let bots = [
            bot("b1", "Pocket", "o05"),
            bot("b2", "Forge", "o08"),
            bot("b3", "Watch", "o11"),
            bot("b4", "Scout", "o07"),
            bot("b5", "Ledger", "h02"),
            bot("b6", "Taro", "h03"),
            bot("b7", "Jojo", "jojo"),
        ];
        let statuses = [
            CharacterStatus::Working,
            CharacterStatus::Success,
            CharacterStatus::NeedsInput,
            CharacterStatus::Working,
            CharacterStatus::Idle,
            CharacterStatus::NeedsInput,
            CharacterStatus::Idle,
        ];
        let mut avatar_rows = Vec::new();
        for s in [
            size::AVATAR_START,
            size::AVATAR_MESSAGE,
            size::AVATAR_FOOTER,
            size::AVATAR_ROW,
        ] {
            let mut line: Vec<AnyElement> = people
                .iter()
                .map(|m| {
                    MemberAvatar::new(Some(m), s)
                        .presence(Some(m))
                        .into_any_element()
                })
                .collect();
            line.extend(bots.iter().zip(statuses).map(|(m, st)| {
                MemberAvatar::new(Some(m), s)
                    .presence(Some(m))
                    .status(st)
                    .into_any_element()
            }));
            avatar_rows.push(row_of(line).gap(space::S12).into_any_element());
        }

        // Sidebar rows (§7.7) on the sidebar surface.
        let sidebar = div()
            .w(px(248.))
            .p(space::S8)
            .rounded(theme::radius::LG)
            .bg(k.sidebar)
            .flex()
            .flex_col()
            .child(
                SectionHeader::new("待办")
                    .collapsible("sh-todo", false)
                    .count(3),
            )
            .child(
                SectionHeader::new("群")
                    .collapsible("sh-groups", false)
                    .trailing(ui::icon_button_sm("sh-add", Lucide::Plus).named("新建群")),
            )
            .children(
                [
                    ("qqbot 研发", true, false, 2u32),
                    ("运维告警", false, true, 0),
                    ("社媒", false, true, 0),
                    ("产品周会", false, false, 0),
                ]
                .into_iter()
                .enumerate()
                .map(|(i, (name, selected, unread, mentions))| {
                    ui::list_row(
                        SharedString::from(format!("sr-{i}")),
                        RowSurface::Sidebar,
                        selected,
                        cx,
                    )
                    .h(size::SIDEBAR_ROW)
                    .px(space::S8)
                    .gap(space::S10)
                    // The last row shows the keyboard focus ring (§7.1).
                    .when(i == 3, |el| el.shadow(ui::rows::focus_ring(k.focus)))
                    .child(GroupGlyph::new().selected(selected).unread(unread))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .t_ui()
                            .text_color(if selected || unread {
                                k.text_1
                            } else {
                                k.text_2
                            })
                            .when(selected || unread, |el| el.semibold())
                            .one_line()
                            .child(name),
                    )
                    .child(CountBadge::new(mentions, BadgeKind::MentionCount))
                }),
            )
            .child(SectionHeader::new("私聊").collapsible("sh-dm", false))
            .children(
                bots.iter()
                    .take(2)
                    .chain(people.iter().take(0))
                    .enumerate()
                    .map(|(i, m)| {
                        ui::list_row(
                            SharedString::from(format!("dm-{i}")),
                            RowSurface::Sidebar,
                            false,
                            cx,
                        )
                        .h(size::SIDEBAR_ROW)
                        .px(space::S8)
                        .gap(space::S10)
                        .child(
                            MemberAvatar::new(Some(m), size::AVATAR_ROW)
                                .presence(Some(m))
                                .on(k.sidebar),
                        )
                        .child(
                            div()
                                .flex_1()
                                .t_ui()
                                .text_color(k.text_2)
                                .child(m.name.clone()),
                        )
                        .child(CountBadge::new(
                            if i == 1 { 1 } else { 0 },
                            BadgeKind::Unread,
                        ))
                    }),
            )
            .child(
                ui::list_row("dm-person", RowSurface::Sidebar, false, cx)
                    .h(size::SIDEBAR_ROW)
                    .px(space::S8)
                    .gap(space::S10)
                    .child(
                        MemberAvatar::new(Some(&people[3]), size::AVATAR_ROW)
                            .presence(Some(&people[3]))
                            .on(k.sidebar),
                    )
                    .child(
                        div()
                            .flex_1()
                            .t_ui()
                            .semibold()
                            .text_color(k.text_1)
                            .child("李雪"),
                    )
                    .child(CountBadge::new(1, BadgeKind::Unread)),
            );

        // Topic rows (§7.6) on content.
        let topic_rows = div().w(px(320.)).flex().flex_col().children(
            [(0usize, true, false), (1, false, true), (2, false, false)].map(
                |(i, selected, unread)| {
                    let m = if i == 1 { &people[3] } else { &bots[i] };
                    let title = [
                        "夜间告警降噪：合并重复的 5xx",
                        "登录页支持粘贴邀请码",
                        "【紧急】DMG 签名：缺少开发者证书",
                    ][i];
                    let preview = [
                        "Watch：要在 staging 先打开合并规则吗？",
                        "李雪：@Joye 粘贴时要不要自动去空格",
                        "Watch：证书申请还在审核",
                    ][i];
                    let trailing: AnyElement = match i {
                        0 => StatusChip::for_topic(
                            &topic(TopicStatus::WaitingAnswer, &[joye]),
                            Some(joye),
                            &members,
                        )
                        .map(|c| c.into_any_element())
                        .unwrap_or_else(|| div().into_any_element()),
                        1 => CountBadge::new(1, BadgeKind::Mention).into_any_element(),
                        _ => StatusChip::for_topic(
                            &topic(TopicStatus::Blocked, &[]),
                            Some(joye),
                            &members,
                        )
                        .map(|c| c.into_any_element())
                        .unwrap_or_else(|| div().into_any_element()),
                    };
                    div().px(space::S8).child(
                        ui::list_row(
                            SharedString::from(format!("tr-{i}")),
                            RowSurface::Content,
                            selected,
                            cx,
                        )
                        .h(size::TOPIC_ROW)
                        .pl(px(18.))
                        .pr(space::S10)
                        .gap(space::S12)
                        .relative()
                        .when(unread, |el| {
                            el.child(
                                div()
                                    .absolute()
                                    .left(px(5.))
                                    .top(px(18.))
                                    .child(ui::unread_dot(cx)),
                            )
                        })
                        .child(
                            MemberAvatar::new(Some(m), size::AVATAR_MESSAGE)
                                .selected(selected)
                                .status(statuses[i]),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .flex_col()
                                .gap(space::S4)
                                .child(
                                    div()
                                        .flex()
                                        .gap(space::S8)
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .t_ui()
                                                .medium()
                                                .when(selected || unread, |el| el.semibold())
                                                .text_color(k.text_1)
                                                .one_line()
                                                .child(title),
                                        )
                                        .child(
                                            div()
                                                .t_meta()
                                                .tnum()
                                                .text_color(k.text_3)
                                                .child(["10:41", "10:20", "昨天"][i]),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(space::S8)
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .t_small()
                                                .text_color(k.text_2)
                                                .one_line()
                                                .child(preview),
                                        )
                                        .child(trailing),
                                ),
                        ),
                    )
                },
            ),
        );

        // Menu and @mention popup (§7.12, §7.2)
        let menu = ui::menu_surface(cx)
            .w(px(220.))
            .child(ui::menu_row("m-1", Some(Lucide::Pencil), "编辑", false, cx))
            .child(ui::menu_row(
                "m-2",
                Some(Lucide::Copy),
                "复制链接",
                true,
                cx,
            ))
            .child(ui::menu_row(
                "m-3",
                Some(Lucide::Eye),
                "标为未读",
                false,
                cx,
            ))
            .child(ui::menu_separator(cx))
            .child(
                ui::menu_row("m-4", Some(Lucide::Trash), "删除", false, cx)
                    .text_color(k.danger_text),
            );
        let mention = ui::mention_surface(cx)
            .child(
                div()
                    .h(px(28.))
                    .px(space::S10)
                    .flex()
                    .items_center()
                    .t_meta_medium()
                    .text_color(k.text_3)
                    .child("成员"),
            )
            .children(bots.iter().skip(3).take(2).enumerate().map(|(i, m)| {
                ui::popup_row(SharedString::from(format!("pm-{i}")), i == 0, cx)
                    .child(
                        MemberAvatar::new(Some(m), size::AVATAR_ROW)
                            .on(k.overlay)
                            .status(CharacterStatus::Idle),
                    )
                    .child(div().child(m.name.clone()))
                    .child(
                        div()
                            .flex_1()
                            .t_small()
                            .text_color(k.text_3)
                            .child(["测试验收", "数据"][i]),
                    )
                    .when(i == 0, |el| el.child(ui::return_hint(cx)))
            }));

        // Composer (§7.2)
        let composer = |id: &'static str,
                        state: &Entity<TextareaState>,
                        focused: bool,
                        ready: bool,
                        window: &mut Window,
                        cx: &mut Context<Self>| {
            ComposerShell::new(
                id,
                ui::placeholder_input(
                    Textarea::new(state)
                        .appearance(false)
                        .with_size(gpui_component::Size::XSmall)
                        .p_0()
                        .t_body(),
                    window,
                    cx,
                ),
            )
            .focused(focused)
            .tool(ui::composer_tool(
                SharedString::from(format!("{id}-plus")),
                Lucide::Plus,
                "添加附件",
            ))
            .tool(ui::composer_tool(
                SharedString::from(format!("{id}-at")),
                Lucide::AtSign,
                "提及",
            ))
            .tool(ui::composer_tool(
                SharedString::from(format!("{id}-emoji")),
                Lucide::FaceSlightlySmiling,
                "表情",
            ))
            .tool(ui::composer_tool(
                SharedString::from(format!("{id}-fmt")),
                Lucide::Type,
                "格式",
            ))
            .send(ui::send_button(
                SharedString::from(format!("{id}-send")),
                ready,
                "发送",
                cx,
            ))
        };
        let composers = div()
            .w(px(620.))
            .flex()
            .flex_col()
            .gap(space::S16)
            .child(composer(
                "c-empty",
                &self.composer_empty,
                false,
                false,
                window,
                cx,
            ))
            .child(composer(
                "c-focus",
                &self.composer_focused,
                focused,
                true,
                window,
                cx,
            ));

        // Surfaces: card, banner, toast, empty state, tabs
        let card = ui::card_padding(ui::card(cx))
            .w(px(420.))
            .flex()
            .flex_col()
            .child(div().t_card_title().text_color(k.text_1).child("发布预览"))
            .child(
                div()
                    .pt(space::S4)
                    .t_body()
                    .text_color(k.text_2)
                    .child("周四小红书：夹夹 3.0 上线"),
            )
            .child(
                row_of(vec![
                    ui::primary_button("cb-1", "发布", cx).into_any_element(),
                    ui::secondary_button("cb-2", "修改", cx).into_any_element(),
                    ui::button("cb-3", "放弃", ButtonKind::Ghost, ButtonSize::Md, cx)
                        .into_any_element(),
                ])
                .pt(space::S14),
            );
        let surfaces = div()
            .flex()
            .flex_col()
            .gap(space::S16)
            .child(card)
            .child(
                div().w(px(232.)).child(
                    ui::Banner::new(Lucide::Download, "新版本已下载")
                        .detail("重启后生效")
                        .action(ui::button(
                            "bn-1",
                            "重启",
                            ButtonKind::Secondary,
                            ButtonSize::Sm,
                            cx,
                        )),
                ),
            )
            .child(
                ui::Toast::new(Lucide::CircleCheck, "已复制链接")
                    .tone(ui::Tone::Success)
                    .action(ui::text_button("t-undo", "撤销", cx)),
            )
            .child(
                div()
                    .h(px(40.))
                    .flex()
                    .gap(space::S24)
                    .border_b(theme::HAIRLINE)
                    .border_color(k.sep)
                    .child(ui::tab("tab-1", "全部", 0, true, cx))
                    .child(ui::tab("tab-2", "未读", 3, false, cx))
                    .child(ui::tab("tab-3", "等我", 128, false, cx)),
            )
            .child(
                div()
                    .h(px(150.))
                    .w(px(320.))
                    .rounded(theme::radius::LG)
                    .border_1()
                    .border_color(k.sep)
                    .child(
                        ui::EmptyState::new(Lucide::SearchX, "没有找到「部署」")
                            .description("换个词，或在所有群里搜索")
                            .action(ui::text_button("e-1", "在所有群里搜索", cx)),
                    ),
            );

        div()
            .id("gallery")
            .size_full()
            .bg(k.content)
            .text_color(k.text_1)
            .overflow_y_scrollbar()
            .child(if self.page == 2 {
                v3::page(cx)
            } else if self.page == 0 {
                div()
                    .p(space::S32)
                    // Clear the traffic lights: the title bar is transparent.
                    .pt(theme::TOOLBAR_HEIGHT)
                    .flex()
                    .gap(space::S44)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(space::S32)
                            .child(section("BUTTONS §7.3", cx).children(buttons))
                            .child(section("FIELDS §7.4", cx).child(fields)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(space::S32)
                            .child(section("CHIPS AND BADGES §7.8", cx).child(chips))
                            .child(section("AVATARS §7.9", cx).children(avatar_rows))
                            .child(
                                section("ROWS §7.6 §7.7", cx).child(
                                    div()
                                        .flex()
                                        .gap(space::S24)
                                        .child(sidebar)
                                        .child(topic_rows),
                                ),
                            ),
                    )
            } else {
                div()
                    .p(space::S32)
                    // Clear the traffic lights: the title bar is transparent.
                    .pt(theme::TOOLBAR_HEIGHT)
                    .flex()
                    .gap(space::S44)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(space::S32)
                            .child(section("COMPOSER §7.2", cx).child(composers))
                            .child(section("STATES §7.1", cx).child(forced_states(cx)))
                            .child(
                                section("OVERLAYS §7.12", cx).child(
                                    div()
                                        .flex()
                                        .gap(space::S24)
                                        .items_start()
                                        .child(menu)
                                        .child(mention),
                                ),
                            ),
                    )
                    .child(section("SURFACES §7.10 §8.5 §8.6", cx).child(surfaces))
            })
    }
}
