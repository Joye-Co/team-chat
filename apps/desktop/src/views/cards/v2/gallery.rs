//! The Card v2 gallery of Cards.dc.html: every block type in every state,
//! drawn through [`super::render`] from the v2 sample team, section by
//! section. The `cards-v2*` screenshot scenes open it at one section.

use gpui::{
    AnyElement, App, Context, FontWeight, InteractiveElement as _, IntoElement, ParentElement,
    Render, SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};

use super::{
    CardEnv, CardMode, confirm,
    form::{Preset, Val},
    meta, render,
    samples::*,
};
use crate::{
    app::Services,
    copy::{cards_v2 as c, fill},
    fake::v2::{ME, OPS, XIAOWANG},
    theme::{self, Type as _, card_v2 as m, card_v2::mix, space},
    ui,
};

/// The sections of the gallery, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Section {
    States,
    Choice,
    Approve,
    Draft,
    Form,
    Plan,
    Data,
    Diff,
    Evidence,
    Refs,
    Ack,
    Page,
    Notify,
    /// Loading, a card that cannot be read, a long choice (§2.14, §2.15).
    Load,
}

impl Section {
    pub const ALL: [Section; 14] = [
        Section::States,
        Section::Choice,
        Section::Approve,
        Section::Draft,
        Section::Form,
        Section::Plan,
        Section::Data,
        Section::Diff,
        Section::Evidence,
        Section::Refs,
        Section::Ack,
        Section::Page,
        Section::Notify,
        Section::Load,
    ];

    fn number(self) -> usize {
        self as usize
    }

    fn title(self) -> &'static str {
        match self {
            Section::States => c::GALLERY_STATES,
            Section::Choice => c::GALLERY_S_CHOICE,
            Section::Approve => c::GALLERY_S_APPROVE,
            Section::Draft => c::GALLERY_S_DRAFT,
            Section::Form => c::GALLERY_S_FORM,
            Section::Plan => c::GALLERY_S_PLAN,
            Section::Data => c::GALLERY_S_DATA,
            Section::Diff => c::GALLERY_S_DIFF,
            Section::Evidence => c::GALLERY_S_EVIDENCE,
            Section::Refs => c::GALLERY_S_REFS,
            Section::Ack => c::GALLERY_S_ACK,
            Section::Page => c::GALLERY_S_PAGE,
            Section::Notify => c::GALLERY_S_NOTIFY,
            Section::Load => c::GALLERY_S_LOAD,
        }
    }

    fn note(self) -> &'static str {
        match self {
            Section::States => c::GALLERY_STATES_NOTE,
            Section::Choice => c::GALLERY_S_CHOICE_NOTE,
            Section::Approve => c::GALLERY_S_APPROVE_NOTE,
            Section::Draft => c::GALLERY_S_DRAFT_NOTE,
            Section::Form => c::GALLERY_S_FORM_NOTE,
            Section::Plan => c::GALLERY_S_PLAN_NOTE,
            Section::Data => c::GALLERY_S_DATA_NOTE,
            Section::Diff => c::GALLERY_S_DIFF_NOTE,
            Section::Evidence => c::GALLERY_S_EVIDENCE_NOTE,
            Section::Refs => c::GALLERY_S_REFS_NOTE,
            Section::Ack => c::GALLERY_S_ACK_NOTE,
            Section::Page => c::GALLERY_S_PAGE_NOTE,
            Section::Notify => c::GALLERY_S_NOTIFY_NOTE,
            Section::Load => c::GALLERY_S_LOAD_NOTE,
        }
    }
}

pub struct CardsGallery {
    services: Services,
    start: Section,
}

impl CardsGallery {
    pub fn new(services: Services, start: Section) -> Self {
        Self { services, start }
    }

    fn env(&self) -> CardEnv {
        CardEnv {
            viewer: Some(ME.into()),
            ..CardEnv::from_services(&self.services)
        }
    }
}

/// One example: its caption, its note and the element.
struct Item {
    caption: &'static str,
    note: &'static str,
    el: AnyElement,
}

fn item(caption: &'static str, note: &'static str, el: Option<AnyElement>) -> Option<Item> {
    el.map(|el| Item { caption, note, el })
}

fn caption(item: &Item, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    meta(div())
        .mb(space::S8)
        .flex()
        .items_baseline()
        .gap(space::S8)
        .child(
            div()
                .flex_none()
                .font_weight(theme::MEDIUM_WEIGHT)
                .text_color(k.ink_2)
                .child(item.caption),
        )
        .child(div().text_color(k.ink_3).child(item.note))
        .into_any_element()
}

/// One gallery column at the one card width (§2.4); two sit 16 apart.
fn column(items: Vec<Option<Item>>, cx: &App) -> AnyElement {
    div()
        .flex_none()
        .w(m::MAX_WIDTH)
        .flex()
        .flex_col()
        .gap(space::S24)
        .children(items.into_iter().flatten().map(|i| {
            div()
                .min_w(px(0.))
                .child(caption(&i, cx))
                .child(i.el)
                .into_any_element()
        }))
        .into_any_element()
}

impl CardsGallery {
    #[allow(clippy::too_many_arguments)]
    fn card(
        &self,
        id: &str,
        mode: CardMode,
        viewer: Option<&str>,
        preset: Option<Preset>,
        instance: &str,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let env = CardEnv {
            viewer: viewer.map(str::to_string).or(Some(ME.into())),
            preset,
            instance: Some(SharedString::from(instance.to_string())),
            ..self.env()
        };
        let group = self.services.provider.card_v2("", id).map(|m| m.group_id)?;
        render(&env, &group, id, mode, window, cx)
    }

    fn header(&self, cx: &App) -> AnyElement {
        let k = theme::colors(cx);
        let list = |title: &'static str, rows: Vec<AnyElement>| {
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(space::S8)
                .child(
                    meta(div())
                        .font_weight(theme::MEDIUM_WEIGHT)
                        .text_color(k.ink_3)
                        .child(title),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(space::S4)
                        .text_size(crate::ui::native::tokens::text::UI.0)
                        .line_height(px(20.))
                        .text_color(k.ink_2)
                        .children(rows),
                )
        };
        let numbered = |i: usize, t: &'static str| {
            div()
                .flex()
                .gap(space::S8)
                .child(
                    div()
                        .w(px(10.))
                        .text_size(crate::ui::native::tokens::text::META.0)
                        .mono()
                        .text_color(k.ink_3)
                        .child(i.to_string()),
                )
                .child(t)
                .into_any_element()
        };
        // The legend: each state as its StatusBadge (§2.12), never a free dot.
        let dot = |state: ui::native::BadgeState, t: &'static str| {
            div()
                .flex()
                .child(ui::native::StatusBadge::new(state, t))
                .into_any_element()
        };
        let chips = Section::ALL[1..].iter().map(|s| {
            div()
                .flex()
                .items_center()
                .gap(space::S6)
                .h(px(24.))
                .text_size(crate::ui::native::tokens::text::UI.0)
                .text_color(k.ink_2)
                .child(
                    div()
                        .text_size(crate::ui::native::tokens::text::META.0)
                        .mono()
                        .text_color(k.ink_3)
                        .child(format!("{:02}", s.number())),
                )
                .child(s.title())
        });
        div()
            .pt(px(56.))
            .pb(px(40.))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(space::S12)
                    .child(
                        div()
                            .text_size(px(17.))
                            .line_height(px(24.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(k.ink)
                            .child(c::GALLERY_TITLE),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::native::tokens::text::META.0)
                            .mono()
                            .text_color(k.ink_3)
                            .child(c::GALLERY_COUNT),
                    ),
            )
            .child(
                div()
                    .mt(space::S6)
                    .text_size(crate::ui::native::tokens::text::UI.0)
                    .line_height(px(22.))
                    .text_color(k.ink_2)
                    .child(fill(
                        c::GALLERY_SUBTITLE,
                        &[("product", env!("PRODUCT_NAME"))],
                    )),
            )
            .child(
                div()
                    .mt(px(28.))
                    .flex()
                    .gap(space::S32)
                    .child(list(
                        c::GALLERY_ANATOMY,
                        vec![
                            numbered(1, c::GALLERY_ANATOMY_1),
                            numbered(2, c::GALLERY_ANATOMY_2),
                            numbered(3, c::GALLERY_ANATOMY_3),
                            numbered(4, c::GALLERY_ANATOMY_4),
                            numbered(5, c::GALLERY_ANATOMY_5),
                        ],
                    ))
                    .child(list(
                        c::GALLERY_COLORS,
                        vec![
                            dot(ui::native::BadgeState::NeedsYou, c::GALLERY_COLOR_YOU),
                            dot(ui::native::BadgeState::Working, c::GALLERY_COLOR_LIVE),
                            dot(ui::native::BadgeState::Done, c::GALLERY_COLOR_OK),
                            dot(ui::native::BadgeState::Failed, c::GALLERY_COLOR_BAD),
                            dot(ui::native::BadgeState::Queued, c::GALLERY_COLOR_REST),
                        ],
                    ))
                    .child(list(
                        c::GALLERY_DONTS,
                        [
                            c::GALLERY_DONT_1,
                            c::GALLERY_DONT_2,
                            c::GALLERY_DONT_3,
                            c::GALLERY_DONT_4,
                        ]
                        .into_iter()
                        .map(|t| div().child(t).into_any_element())
                        .collect(),
                    )),
            )
            .child(
                div()
                    .mt(space::S32)
                    .flex()
                    .flex_wrap()
                    .gap_x(space::S20)
                    .gap_y(space::S6)
                    .children(chips),
            )
            .into_any_element()
    }

    fn section_head(&self, s: Section, cx: &App) -> AnyElement {
        let k = theme::colors(cx);
        div()
            .flex()
            .items_baseline()
            .gap(space::S12)
            .when(s != Section::States, |el| {
                el.child(
                    div()
                        .text_size(crate::ui::native::tokens::text::META.0)
                        .mono()
                        .text_color(k.ink_3)
                        .child(format!("{:02}", s.number())),
                )
            })
            .child(
                div()
                    .text_size(crate::ui::native::tokens::text::UI.0)
                    .line_height(px(20.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(k.ink)
                    .child(s.title()),
            )
            .child(meta(div()).text_color(k.ink_3).child(s.note()))
            .into_any_element()
    }

    fn states(&self, window: &mut Window, cx: &mut App) -> AnyElement {
        let line =
            |this: &Self, id: &str, viewer: Option<&str>, window: &mut Window, cx: &mut App| {
                this.card(id, CardMode::Line, viewer, None, "line", window, cx)
            };
        let cells = vec![
            item(
                c::GALLERY_PENDING,
                "",
                line(self, APPROVE, None, window, cx),
            ),
            item(
                c::GALLERY_WAITING,
                "",
                line(self, CHOICE, Some(XIAOWANG), window, cx),
            ),
            item(
                c::GALLERY_WORKING,
                "",
                line(self, SHIPPING, None, window, cx),
            ),
            item(c::GALLERY_DONE, "", line(self, SHIPPED, None, window, cx)),
            item(c::GALLERY_FAILED, "", line(self, FAILED, None, window, cx)),
            item(
                c::GALLERY_EXPIRED,
                "",
                line(self, EXPIRED, None, window, cx),
            ),
            item(
                c::GALLERY_SUPERSEDED,
                "",
                line(self, DRAFT_VOID, None, window, cx),
            ),
        ];
        let cells: Vec<Item> = cells.into_iter().flatten().collect();
        let mut rows = Vec::new();
        let mut it = cells.into_iter().peekable();
        while it.peek().is_some() {
            // Two cards per row at the card width, 16 apart (§2.4, F66).
            let row: Vec<Item> = it.by_ref().take(2).collect();
            rows.push(
                div()
                    .flex()
                    .gap(space::S16)
                    .children(row.into_iter().map(|i| {
                        div()
                            .flex_none()
                            .w(m::MAX_WIDTH)
                            .child(caption(&i, cx))
                            .child(i.el)
                    }))
                    .into_any_element(),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(space::S24)
            .children(rows)
            .into_any_element()
    }

    fn grid(&self, s: Section, window: &mut Window, cx: &mut App) -> AnyElement {
        let me = None;
        let wang = Some(XIAOWANG);
        let d = CardMode::Desk;
        let (left, right): (Vec<Option<Item>>, Vec<Option<Item>>) = match s {
            Section::States => return self.states(window, cx),
            Section::Choice => (
                vec![
                    item(
                        c::GALLERY_QUICK,
                        "",
                        self.card(CHOICE, d, me, None, "a", window, cx),
                    ),
                    item(
                        c::GALLERY_MANY,
                        "",
                        self.card(CHOICE_MANY, d, me, None, "a", window, cx),
                    ),
                ],
                vec![
                    item(
                        c::GALLERY_PICKED,
                        "",
                        self.card(CHOICE_PICKED, d, wang, None, "a", window, cx),
                    ),
                    item(
                        c::GALLERY_OTHERS,
                        c::STRIP_WAITING_ON_DETAIL,
                        self.card(CHOICE, d, wang, None, "b", window, cx),
                    ),
                ],
            ),
            Section::Approve => (
                vec![
                    item(
                        c::GALLERY_ASK,
                        "",
                        self.card(APPROVE, d, me, None, "a", window, cx),
                    ),
                    item(
                        c::GALLERY_RUNNING,
                        c::PLAN_IN_PLACE,
                        self.card(SHIPPING, d, me, None, "a", window, cx),
                    ),
                ],
                vec![
                    item(c::GALLERY_CONFIRM, "", Some(self.sheet_preview(cx))),
                    item(
                        c::GALLERY_SHIPPED,
                        "",
                        self.card(SHIPPED, d, me, None, "a", window, cx),
                    ),
                ],
            ),
            Section::Draft => (
                vec![
                    item(
                        c::GALLERY_TO_CONFIRM,
                        "",
                        self.card(DRAFT, d, me, None, "a", window, cx),
                    ),
                    item(
                        c::GALLERY_CONFIRMED,
                        "",
                        self.card(DRAFT_CONFIRMED, d, me, None, "a", window, cx),
                    ),
                ],
                vec![
                    item(
                        c::GALLERY_EDIT,
                        "",
                        self.card(
                            DRAFT,
                            d,
                            me,
                            Some(Preset {
                                editing: true,
                                values: vec![(
                                    "title",
                                    Val::Text("夹夹 3.0｜群里的照片一键分享回群".into()),
                                )],
                                ..Default::default()
                            }),
                            "edit",
                            window,
                            cx,
                        ),
                    ),
                    item(
                        c::GALLERY_VOID,
                        "",
                        self.card(DRAFT_VOID, d, me, None, "a", window, cx),
                    ),
                ],
            ),
            Section::Form => (
                vec![item(
                    c::GALLERY_UNFILLED,
                    "",
                    self.card(FORM, d, me, None, "a", window, cx),
                )],
                vec![item(
                    c::GALLERY_FILLED,
                    "",
                    self.card(
                        FORM,
                        d,
                        me,
                        Some(Preset {
                            values: vec![
                                ("name", Val::Text("周可".into())),
                                ("role", Val::Choice(vec!["data".into()])),
                                ("start", Val::Text("2026-11-10".into())),
                                ("groups", Val::Choice(vec!["growth".into(), "all".into()])),
                            ],
                            ..Default::default()
                        }),
                        "filled",
                        window,
                        cx,
                    ),
                )],
            ),
            Section::Plan => {
                let env = CardEnv {
                    viewer: Some(ME.into()),
                    ..self.env()
                };
                let rows = [PLAN_DONE, PLAN]
                    .iter()
                    .filter_map(|id| {
                        let g = self.services.provider.card_v2("", id)?.group_id;
                        super::plan_row(&env, &g, id, window, cx)
                    })
                    .collect::<Vec<_>>();
                (
                    vec![item(
                        c::GALLERY_DOING,
                        "",
                        self.card(PLAN, d, me, None, "a", window, cx),
                    )],
                    vec![
                        item(
                            c::GALLERY_WAITS,
                            "",
                            self.card(PLAN_WAITS, d, wang, None, "a", window, cx),
                        ),
                        item(
                            c::GALLERY_IN_TOPIC,
                            "",
                            Some(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(space::S8)
                                    .children(rows)
                                    .into_any_element(),
                            ),
                        ),
                    ],
                )
            }
            Section::Data => (
                vec![item(
                    c::GALLERY_METRICS,
                    "",
                    self.card(METRICS, d, me, None, "a", window, cx),
                )],
                vec![
                    item(
                        c::GALLERY_TABLE,
                        "",
                        self.card(TABLE, d, me, None, "a", window, cx),
                    ),
                    item(
                        c::GALLERY_MISSING,
                        "",
                        self.card(METRICS_OLD, d, me, None, "a", window, cx),
                    ),
                ],
            ),
            Section::Diff => (
                vec![item(
                    c::GALLERY_HUNK,
                    "",
                    self.card(DIFF, d, me, None, "a", window, cx),
                )],
                vec![
                    item(
                        c::GALLERY_FOUR_REFS,
                        "",
                        self.card(PR_LIST, d, me, None, "a", window, cx),
                    ),
                    item(
                        c::GALLERY_NO_EVIDENCE,
                        "",
                        self.card(PR_BARE, d, me, None, "a", window, cx),
                    ),
                ],
            ),
            Section::Evidence => (
                vec![item(
                    c::GALLERY_WITH_EVIDENCE,
                    "",
                    self.card(EVIDENCE, d, me, None, "a", window, cx),
                )],
                vec![
                    item(
                        c::GALLERY_NO_EVIDENCE,
                        "",
                        self.card(EVIDENCE_NONE, d, me, None, "a", window, cx),
                    ),
                    item(
                        c::GALLERY_RESULT_ONLY,
                        "",
                        self.card(EVIDENCE_CHIPS, d, me, None, "a", window, cx),
                    ),
                ],
            ),
            Section::Refs => (
                vec![item(
                    c::GALLERY_FOUR_REFS,
                    "",
                    self.card(REFS, d, me, None, "a", window, cx),
                )],
                vec![
                    item(
                        c::GALLERY_STALE_REFS,
                        "",
                        self.card(REFS_STALE, d, me, None, "a", window, cx),
                    ),
                    item(
                        c::STRIP_SENDING,
                        "",
                        self.card(SENDING, d, me, None, "a", window, cx),
                    ),
                ],
            ),
            Section::Ack => (vec![item(c::GALLERY_SEEN, "", Some(self.ack(cx)))], vec![]),
            Section::Page => (
                vec![item(
                    c::GALLERY_SNAPSHOT,
                    "",
                    self.card(PAGE, d, me, None, "a", window, cx),
                )],
                vec![],
            ),
            Section::Notify => (
                vec![item(
                    c::GALLERY_BANNER,
                    "",
                    self.notify_box(false, window, cx),
                )],
                vec![item(
                    c::GALLERY_HOVER,
                    "",
                    self.notify_box(true, window, cx),
                )],
            ),
            Section::Load => (
                vec![
                    item(c::GALLERY_LOADING, "", Some(super::loading_card(cx))),
                    item(
                        c::GALLERY_DECODE,
                        "",
                        Some(super::failed_card("gallery-decode".into(), cx)),
                    ),
                ],
                vec![item(
                    c::GALLERY_LONG_CHOICE,
                    "",
                    self.card(CHOICE_LONG, d, me, None, "a", window, cx),
                )],
            ),
        };
        div()
            .flex()
            .gap(space::S16)
            .child(column(left, cx))
            .child(column(right, cx))
            .into_any_element()
    }

    /// The confirm sheet as it opens, with the name half typed.
    fn sheet_preview(&self, cx: &App) -> AnyElement {
        let k = theme::colors(cx);
        let Some(msg) = self.services.provider.card_v2(OPS, APPROVE) else {
            return div().into_any_element();
        };
        let action = msg.card.actions()[0].clone();
        let text = confirm::sheet_text(
            &action,
            msg.card.ask.as_ref().and_then(|a| a.irreversible.as_ref()),
        );
        let field = confirm::field_box(div().child("qqbot-pr").into_any_element(), cx);
        div()
            .rounded(theme::radius::LG)
            .bg(theme::card_v2::alpha(k.ink, 0.18))
            .py(px(40.))
            .px(px(48.))
            .flex()
            .justify_center()
            .child(
                crate::ui::native::sheet_surface(crate::ui::native::SheetWidth::Form, cx)
                    .overflow_hidden()
                    .child(confirm::sheet(
                        &text,
                        Some(field),
                        false,
                        "gallery-sheet",
                        None,
                        None,
                        cx,
                    )),
            )
            .into_any_element()
    }

    fn notify_box(&self, hovered: bool, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
        let k = theme::colors(cx);
        let env = CardEnv {
            viewer: Some(ME.into()),
            hovered,
            instance: Some(if hovered { "hover" } else { "rest" }.into()),
            ..self.env()
        };
        let el = render(&env, OPS, APPROVE, CardMode::Notification, window, cx)?;
        Some(
            div()
                .h(px(168.))
                .rounded(theme::radius::LG)
                .bg(mix(k.sunken, k.bg, 0.6))
                // A fill step alone is not an edge (§1.7, D5).
                .border_1()
                .border_color(k.line)
                .pt(space::S24)
                .pr(space::S24)
                .flex()
                .justify_end()
                .items_start()
                .child(el)
                .into_any_element(),
        )
    }

    /// 轻回应: a message that got an emoji instead of a reply line.
    fn ack(&self, cx: &App) -> AnyElement {
        let k = theme::colors(cx);
        let p = &self.services.provider;
        let wang = p.member(XIAOWANG);
        let jojo = p.member(crate::fake::v2::JOJO);
        crate::views::cards::v2::frame(false, cx)
            .p(space::S16)
            .flex()
            .flex_col()
            .gap(space::S12)
            .child(
                div()
                    .flex()
                    .gap(space::S12)
                    .child(ui::MemberAvatar::new(wang.as_ref(), px(28.)))
                    .child(
                        div()
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
                                            .text_size(crate::ui::native::tokens::text::UI.0)
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(k.ink)
                                            .child(
                                                wang.as_ref()
                                                    .map(|m| m.name.clone())
                                                    .unwrap_or_default(),
                                            ),
                                    )
                                    .child(meta(div()).mono().text_color(k.ink_3).child(
                                        crate::views::shell::clock(crate::fake::v2::at(10, 20)),
                                    )),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::native::tokens::text::UI.0)
                                    .line_height(px(22.))
                                    .text_color(k.ink)
                                    .child(super::samples::ACK_BODY),
                            )
                            .child(
                                div()
                                    .id("ack-pill")
                                    .mt(space::S4)
                                    .flex()
                                    .items_center()
                                    .gap(space::S6)
                                    .h(px(26.))
                                    .px(space::S8)
                                    .w(px(52.))
                                    .rounded(px(13.))
                                    .bg(crate::ui::native::native_colors(cx).fill)
                                    .child(
                                        div()
                                            .text_size(crate::ui::native::tokens::text::UI.0)
                                            .child("👀"),
                                    )
                                    .child(ui::MemberAvatar::new(jojo.as_ref(), px(16.))),
                            ),
                    ),
            )
            .child(
                div()
                    .h(crate::ui::native::tokens::layout::BORDER)
                    .bg(k.line),
            )
            .child(meta(div()).text_color(k.ink_3).child(fill(
                c::GALLERY_ACK_NOTE,
                &[(
                    "name",
                    &jojo.as_ref().map(|m| m.name.clone()).unwrap_or_default(),
                )],
            )))
            .into_any_element()
    }
}

impl Render for CardsGallery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let mut body = div()
            .w(m::MAX_WIDTH * 2. + space::S16)
            .mx_auto()
            .flex()
            .flex_col();
        if self.start == Section::States {
            body = body.child(self.header(cx));
        }
        for s in Section::ALL.into_iter().filter(|s| *s >= self.start) {
            let first = s == self.start && s != Section::States;
            body = body.child(
                div()
                    .when(s == Section::States, |el| el.pb(px(56.)))
                    .when(s != Section::States, |el| {
                        el.pt(if first { px(40.) } else { px(48.) })
                            .pb(px(56.))
                            .border_t_1()
                            .border_color(k.line)
                    })
                    .child(self.section_head(s, cx))
                    .child(div().mt(space::S20).child(self.grid(s, window, cx))),
            );
        }
        if self.start <= Section::Notify {
            body = body.child(
                meta(div())
                    .pt(space::S24)
                    .pb(px(40.))
                    .border_t_1()
                    .border_color(k.line)
                    .text_color(k.ink_3)
                    .child(c::GALLERY_FOOT),
            );
        }
        div()
            .id("cards-v2-gallery")
            .size_full()
            .bg(k.bg)
            .text_color(k.ink)
            .overflow_y_scroll()
            .px(px(40.))
            .child(body)
    }
}
