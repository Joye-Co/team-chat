//! The Card v2.1 galleries: CardsCompose.dc.html section by section (`compose*` scenes)
//! and the interaction loop of Interact.dc.html (`interact_*` scenes). Every card is drawn
//! through `v2::render` from the v2 sample team.

use gpui::{
    AnyElement, App, Context, FontWeight, InteractiveElement as _, IntoElement, ParentElement,
    Render, SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};

use super::samples::*;
use crate::{
    app::Services,
    copy::cards21 as c,
    fake::v2::{ME, XIAOWANG},
    theme::{self, Type as _, space},
    views::cards::v2::{
        CardEnv, CardMode,
        form::{Preset, Val},
        meta, render,
    },
};

/// What a gallery window shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Page {
    /// CardsCompose 01 组合.
    Compose,
    /// 02 选择题: single + other, multiple, rank, rating + scale.
    Choice,
    /// 02 选择题: confirm, image choice, failed, matrix.
    Choice2,
    /// 02 选择题: slots and poll.
    Choice3,
    /// 03 扩展方式: embed, unknown, buttons and tabs, diagram.
    Extend,
    /// Interact: the report with a chart beside a question, wide and narrow.
    Report,
    /// Interact: my bar click waiting at three stages.
    Pending,
    /// Interact: rev 2 after my click, as I see it and as others see it.
    Drill,
    /// Interact: my answer refused because the card changed.
    Conflict,
    /// Interact: the report as someone the card does not ask.
    Readonly,
}

pub struct Gallery21 {
    services: Services,
    page: Page,
}

impl Gallery21 {
    pub fn new(services: Services, page: Page) -> Self {
        Self { services, page }
    }

    #[allow(clippy::too_many_arguments)]
    fn card(
        &self,
        id: &str,
        viewer: &str,
        preset: Option<Preset>,
        instance: &str,
        mode: CardMode,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let env = CardEnv {
            viewer: Some(viewer.into()),
            preset,
            instance: Some(SharedString::from(instance.to_string())),
            ..CardEnv::from_services(&self.services)
        };
        let group = self.services.provider.card_v2("", id).map(|m| m.group_id)?;
        render(&env, &group, id, mode, window, cx)
    }
}

/// A captioned example.
fn item(
    caption: &str,
    note: &str,
    el: Option<AnyElement>,
    width: Option<f32>,
    cx: &App,
) -> AnyElement {
    let k = theme::colors(cx);
    div()
        .min_w(px(0.))
        .when_some(width, |el, w| el.w(px(w)).flex_none())
        .child(
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
                        .child(caption.to_string()),
                )
                .child(div().text_color(k.ink_3).child(note.to_string())),
        )
        .children(el)
        .into_any_element()
}

fn column(items: Vec<AnyElement>) -> AnyElement {
    div()
        .flex_1()
        .min_w(px(0.))
        .max_w(px(600.))
        .flex()
        .flex_col()
        .gap(space::S24)
        .children(items)
        .into_any_element()
}

fn head(title: &str, note: &str, number: Option<&str>, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    div()
        .flex()
        .items_baseline()
        .gap(space::S12)
        .children(number.map(|n| {
            div()
                .text_size(px(12.))
                .mono()
                .text_color(k.ink_3)
                .child(n.to_string())
        }))
        .child(
            div()
                .text_size(px(13.))
                .line_height(px(20.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(k.ink)
                .child(title.to_string()),
        )
        .child(meta(div()).text_color(k.ink_3).child(note.to_string()))
        .into_any_element()
}

impl Gallery21 {
    fn body(&self, window: &mut Window, cx: &mut App) -> (AnyElement, AnyElement) {
        let d = CardMode::Desk;
        let me = ME;
        macro_rules! card {
            ($id:expr, $viewer:expr, $preset:expr, $inst:expr) => {
                self.card($id, $viewer, $preset, $inst, d, window, cx)
            };
        }
        let preset = |values: Vec<(&'static str, Val)>| {
            Some(Preset {
                values,
                ..Default::default()
            })
        };
        match self.page {
            Page::Compose => {
                let left = vec![
                    item(
                        c::G_QUESTIONNAIRE,
                        c::G_QUESTIONNAIRE_NOTE,
                        card!(
                            QUESTIONNAIRE,
                            me,
                            preset(vec![("perm", Val::Choice(vec!["same".into()])),]),
                            "a"
                        ),
                        None,
                        cx,
                    ),
                    item(
                        c::G_METRICS_APPROVE,
                        c::G_METRICS_APPROVE_NOTE,
                        card!(HEALTH, me, None, "a"),
                        None,
                        cx,
                    ),
                ];
                let right = vec![
                    item(
                        c::G_COMPARE,
                        c::G_COMPARE_NOTE,
                        card!(COMPARE, me, None, "a"),
                        None,
                        cx,
                    ),
                    item(
                        c::G_GROUP,
                        c::G_GROUP_NOTE,
                        card!(GROUPS, me, None, "a"),
                        None,
                        cx,
                    ),
                    item(
                        c::G_READONLY,
                        c::G_READONLY_NOTE,
                        card!(QUESTIONNAIRE, XIAOWANG, None, "ro"),
                        None,
                        cx,
                    ),
                    item(
                        c::G_VOID,
                        c::G_VOID_NOTE,
                        card!(VOID, me, None, "a"),
                        None,
                        cx,
                    ),
                ];
                (column(left), column(right))
            }
            Page::Choice => {
                let left = vec![
                    item(
                        c::G_SINGLE_OTHER,
                        c::G_SINGLE_OTHER_NOTE,
                        card!(
                            SINGLE_OTHER,
                            me,
                            preset(vec![
                                ("who", Val::Choice(vec!["other".into()])),
                                ("who#other", Val::Text("Alex Chen，下周轮到他值班".into())),
                            ]),
                            "a"
                        ),
                        None,
                        cx,
                    ),
                    item(
                        c::G_RANK,
                        c::G_RANK_NOTE,
                        card!(
                            RANK,
                            me,
                            preset(vec![(
                                "order",
                                Val::Order(vec![
                                    "photos".into(),
                                    "share".into(),
                                    "birthday".into(),
                                    "night".into(),
                                    "weekly".into()
                                ])
                            ),]),
                            "a"
                        ),
                        None,
                        cx,
                    ),
                ];
                let right = vec![
                    item(
                        c::G_MULTIPLE,
                        c::G_MULTIPLE_NOTE,
                        card!(
                            MULTIPLE,
                            me,
                            preset(vec![(
                                "where",
                                Val::Choice(vec!["xhs".into(), "bili".into()])
                            ),]),
                            "a"
                        ),
                        None,
                        cx,
                    ),
                    item(
                        c::G_RATING,
                        c::G_RATING_NOTE,
                        card!(RATING, me, preset(vec![("clear", Val::Num(4.)),]), "a"),
                        None,
                        cx,
                    ),
                ];
                (column(left), column(right))
            }
            Page::Choice2 => {
                let left = vec![
                    item(
                        c::G_IMAGE,
                        c::G_IMAGE_NOTE,
                        card!(
                            IMAGE,
                            me,
                            preset(vec![("cover", Val::Choice(vec!["B".into()])),]),
                            "a"
                        ),
                        None,
                        cx,
                    ),
                    item(
                        c::G_CONFIRM,
                        c::G_CONFIRM_NOTE,
                        card!(CONFIRM, me, None, "a"),
                        None,
                        cx,
                    ),
                ];
                let right = vec![
                    item(
                        c::G_FAILED,
                        c::G_FAILED_NOTE,
                        card!(FAILED, me, None, "a"),
                        None,
                        cx,
                    ),
                    item(
                        c::G_MATRIX,
                        c::G_MATRIX_NOTE,
                        card!(
                            MATRIX,
                            me,
                            Some(Preset {
                                values: vec![(
                                    "plan",
                                    Val::Rows(
                                        [
                                            ("photos".to_string(), "v40".to_string()),
                                            ("night".to_string(), "later".to_string())
                                        ]
                                        .into()
                                    )
                                )],
                                ..Default::default()
                            }),
                            "a"
                        ),
                        None,
                        cx,
                    ),
                ];
                (column(left), column(right))
            }
            Page::Choice3 => {
                let left = vec![item(
                    c::G_SLOTS,
                    c::G_SLOTS_NOTE,
                    card!(
                        SLOTS,
                        me,
                        preset(vec![("when", Val::Choice(vec!["t4".into()])),]),
                        "a"
                    ),
                    None,
                    cx,
                )];
                let right = vec![item(
                    c::G_POLL,
                    c::G_POLL_NOTE,
                    card!(POLL, me, None, "a"),
                    None,
                    cx,
                )];
                (column(left), column(right))
            }
            Page::Extend => {
                let left = vec![
                    item(
                        c::G_BUTTONS,
                        c::G_BUTTONS_NOTE,
                        card!(BUTTONS, me, None, "a"),
                        None,
                        cx,
                    ),
                    item(
                        c::G_DIAGRAM,
                        c::G_DIAGRAM_NOTE,
                        card!(DIAGRAM, me, None, "a"),
                        None,
                        cx,
                    ),
                ];
                let right = vec![
                    item(
                        c::G_EMBED,
                        c::G_EMBED_NOTE,
                        card!(EMBED, me, None, "a"),
                        None,
                        cx,
                    ),
                    item(
                        c::G_UNKNOWN,
                        c::G_UNKNOWN_NOTE,
                        card!(UNKNOWN, me, None, "a"),
                        None,
                        cx,
                    ),
                ];
                (column(left), column(right))
            }
            Page::Report => (
                item(
                    c::I_REPORT,
                    c::I_REPORT_NOTE,
                    self.card(REPORT, me, None, "wide", CardMode::Topic, window, cx),
                    Some(720.),
                    cx,
                ),
                item(
                    c::I_REPORT_NARROW,
                    c::I_REPORT_NARROW_NOTE,
                    self.card(
                        REPORT,
                        me,
                        preset(vec![("check", Val::Choice(vec!["wrong".into()]))]),
                        "narrow",
                        CardMode::Topic,
                        window,
                        cx,
                    ),
                    Some(480.),
                    cx,
                ),
            ),
            Page::Pending => {
                let cards = div()
                    .flex()
                    .gap(space::S24)
                    .child(item(
                        c::I_PENDING_FAST,
                        "",
                        self.card(PEND_WAIT, me, None, "a", CardMode::Topic, window, cx),
                        Some(380.),
                        cx,
                    ))
                    .child(item(
                        c::I_PENDING_SOFT,
                        "",
                        self.card(PEND_SLOW, me, None, "a", CardMode::Topic, window, cx),
                        Some(380.),
                        cx,
                    ))
                    .child(item(
                        c::I_PENDING_HARD,
                        "",
                        self.card(PEND_HARD, me, None, "a", CardMode::Topic, window, cx),
                        Some(380.),
                        cx,
                    ))
                    .into_any_element();
                (cards, div().into_any_element())
            }
            Page::Drill => {
                let prev = || {
                    Some(Preset {
                        prev: prev_of(DRILL),
                        ..Default::default()
                    })
                };
                (
                    item(
                        c::I_DRILL_SELF,
                        c::I_DRILL_NOTE,
                        self.card(DRILL, me, prev(), "self", CardMode::Topic, window, cx),
                        Some(560.),
                        cx,
                    ),
                    item(
                        c::I_DRILL_OTHER,
                        "",
                        self.card(
                            DRILL,
                            XIAOWANG,
                            prev(),
                            "other",
                            CardMode::Topic,
                            window,
                            cx,
                        ),
                        Some(560.),
                        cx,
                    ),
                )
            }
            Page::Conflict => (
                item(
                    c::I_CONFLICT,
                    c::I_CONFLICT_NOTE,
                    self.card(
                        CONFLICT,
                        me,
                        Some(Preset {
                            values: vec![("check", Val::Choice(vec!["wrong".into()]))],
                            prev: prev_of(CONFLICT),
                            ..Default::default()
                        }),
                        "a",
                        CardMode::Topic,
                        window,
                        cx,
                    ),
                    Some(640.),
                    cx,
                ),
                div().into_any_element(),
            ),
            Page::Readonly => (
                item(
                    c::I_READONLY,
                    c::I_READONLY_NOTE,
                    self.card(REPORT, XIAOWANG, None, "ro", CardMode::Topic, window, cx),
                    Some(720.),
                    cx,
                ),
                div().into_any_element(),
            ),
        }
    }
}

impl Render for Gallery21 {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let (title, note, number) = match self.page {
            Page::Compose => (c::G_S_COMPOSE, c::G_S_COMPOSE_NOTE, Some("01")),
            Page::Choice | Page::Choice2 | Page::Choice3 => {
                (c::G_S_CHOICE, c::G_S_CHOICE_NOTE, Some("02"))
            }
            Page::Extend => (c::G_S_EXTEND, c::G_S_EXTEND_NOTE, Some("03")),
            _ => (c::I_TITLE, c::I_SUBTITLE, None),
        };
        let (left, right) = self.body(window, cx);
        let top = (self.page == Page::Compose).then(|| {
            div()
                .pt(px(48.))
                .pb(px(32.))
                .flex()
                .flex_col()
                .gap(space::S6)
                .child(
                    div()
                        .text_size(px(17.))
                        .line_height(px(24.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(k.ink)
                        .child(c::G_TITLE),
                )
                .child(
                    div()
                        .max_w(px(760.))
                        .text_size(px(14.))
                        .line_height(px(22.))
                        .text_color(k.ink_2)
                        .child(c::G_SUBTITLE),
                )
        });
        div()
            .id("cards-v21-gallery")
            .size_full()
            .bg(k.bg)
            .text_color(k.ink)
            .overflow_y_scroll()
            .px(px(40.))
            .child(
                div()
                    .w_full()
                    .max_w(px(1232.))
                    .mx_auto()
                    .flex()
                    .flex_col()
                    .children(top)
                    .child(
                        div()
                            .pt(px(32.))
                            .pb(px(56.))
                            .when(self.page == Page::Compose, |el| {
                                el.border_t_1().border_color(k.line)
                            })
                            .child(head(title, note, number, cx))
                            .child(
                                div()
                                    .mt(space::S20)
                                    .flex()
                                    .items_start()
                                    .gap(space::S32)
                                    .child(left)
                                    .child(right),
                            ),
                    ),
            )
    }
}
