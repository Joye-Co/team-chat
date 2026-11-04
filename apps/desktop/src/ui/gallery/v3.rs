//! Gallery page 3 (`components-v3`): the v3 tokens of HELMET2 and the v3
//! components, so the foundation can be reviewed in light and dark before
//! the screens use it.

use gpui::{
    AnyElement, App, Hsla, IntoElement, ParentElement, SharedString, Styled, div,
    prelude::FluentBuilder as _, px,
};

use crate::{
    fake::v2,
    theme::{self, StateTone, Type as _, space},
    ui::{self, JobTag, MemberAvatar, PushSize, PushStyle, Ring, RowSurface, Sym},
};

fn section(title: &'static str, cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    div()
        .flex()
        .flex_col()
        .gap(space::S10)
        .child(div().t_meta_medium().text_color(k.ink_3).child(title))
}

fn swatch(name: &'static str, color: Hsla, cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    div()
        .flex()
        .flex_col()
        .gap(space::S4)
        .child(
            div()
                .size(px(52.))
                .rounded(theme::radius::MD)
                .bg(color)
                .border_1()
                .border_color(k.line),
        )
        .child(div().t_mono().text_color(k.ink_2).child(name))
}

fn surfaces(cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    div().flex().flex_wrap().gap(space::S12).children([
        swatch("bg", k.bg, cx),
        swatch("rail", k.rail, cx),
        swatch("sunken", k.sunken, cx),
        swatch("raised", k.raised, cx),
        swatch("hover", k.hover, cx),
        swatch("sel", k.sel, cx),
        swatch("line", k.line, cx),
        swatch("line-2", k.line_2, cx),
        swatch("btn", k.btn, cx),
        swatch("screen", k.screen, cx),
        swatch("person", k.person, cx),
    ])
}

fn tints(cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    div().flex().flex_wrap().gap(space::S16).children(
        v2::members().into_iter().filter(|m| m.is_bot()).map(|m| {
            div()
                .flex()
                .items_center()
                .gap(space::S8)
                .child(MemberAvatar::new(Some(&m), px(28.)))
                .child(
                    div()
                        .t_small()
                        .medium()
                        .text_color(k.ink)
                        .child(m.name.clone()),
                )
                .children(m.role.clone().map(JobTag::new))
        }),
    )
}

fn states(cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    let row = |tone: StateTone, label: &'static str| {
        let (mark, ink) = k.state(tone);
        div()
            .flex()
            .items_center()
            .gap(space::S8)
            .child(div().size(px(7.)).rounded_full().bg(mark))
            .child(div().t_small().text_color(ink).child(label))
    };
    div()
        .flex()
        .flex_col()
        .gap(space::S8)
        .child(row(StateTone::You, "需要你审 PR #61"))
        .child(row(StateTone::Live, "Forge 在做 · 3/5"))
        .child(row(StateTone::Problem, "CI 超时"))
        .child(row(StateTone::Done, "做完了 · 09:55"))
        .child(row(StateTone::Quiet, "等小王"))
        .child(
            div()
                .t_mono()
                .text_color(k.ink)
                .child("09:42 · ¥6.20 · +214 −63 · 3/5 · 118 fps"),
        )
}

fn buttons(cx: &App) -> gpui::Div {
    let b = |id: &'static str, label: &'static str, kind, size| {
        ui::native::push_button(id, label, kind, size, cx).into_any_element()
    };
    div()
        .flex()
        .flex_col()
        .gap(space::S10)
        .child(div().flex().gap(space::S8).children([
            b("v3-p", "批准合并", PushStyle::Default, PushSize::Regular),
            b("v3-s", "要求修改", PushStyle::Outline, PushSize::Regular),
            b("v3-g", "看改动", PushStyle::Ghost, PushSize::Regular),
        ]))
        .child(div().flex().gap(space::S8).children([
            b("v3-p-sm", "发布到生产", PushStyle::Default, PushSize::Small),
            b("v3-s-sm", "先不发", PushStyle::Outline, PushSize::Small),
            b("v3-d-sm", "停止", PushStyle::Destructive, PushSize::Small),
        ]))
}

fn rings() -> gpui::Div {
    let members = v2::members();
    let forge = members.iter().find(|m| m.id == v2::FORGE).cloned();
    let watch = members.iter().find(|m| m.id == v2::WATCH).cloned();
    let me = members.first().cloned();
    let xw = members.iter().find(|m| m.id == v2::XIAOWANG).cloned();
    div()
        .flex()
        .items_center()
        .gap(space::S20)
        .children([16., 20., 28., 48.].map(|s| {
            MemberAvatar::new(forge.as_ref(), px(s))
                .ring(Some(Ring::Live))
                .into_any_element()
        }))
        .child(MemberAvatar::new(watch.as_ref(), px(28.)).ring(Some(Ring::You)))
        .child(MemberAvatar::new(me.as_ref(), px(28.)).ring(Some(Ring::You)))
        .child(MemberAvatar::new(xw.as_ref(), px(28.)).ring(Some(Ring::Live)))
}

fn rail_rows(cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    let members = v2::members();
    let forge = members.iter().find(|m| m.id == v2::FORGE).cloned();
    let row = |id: &'static str,
               selected: bool,
               lead: AnyElement,
               text: &'static str,
               meta: Option<&'static str>| {
        ui::list_row(id, RowSurface::Rail, selected, cx)
            .h(theme::RAIL_ROW)
            .px(space::S10)
            .gap(space::S8)
            .child(
                div()
                    .flex_none()
                    .size(theme::RAIL_SLOT)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(lead),
            )
            .child(
                div()
                    .flex_1()
                    .t_small()
                    .text_color(k.ink)
                    .when(selected, |el| el.medium())
                    .child(text),
            )
            .children(meta.map(|m| div().t_mono().text_color(k.ink_3).child(m)))
    };
    div()
        .w(theme::RAIL_WIDTH)
        .p(space::S10)
        .rounded(theme::radius::LG)
        .bg(k.rail)
        .border_1()
        .border_color(k.line)
        .flex()
        .flex_col()
        .gap(px(1.))
        .child(row(
            "g-r1",
            true,
            div()
                .size(px(7.))
                .rounded_full()
                .bg(k.you)
                .into_any_element(),
            "断线重连后消息重复",
            None,
        ))
        .child(row(
            "g-r2",
            false,
            MemberAvatar::new(forge.as_ref(), px(16.))
                .ring(Some(Ring::Live))
                .into_any_element(),
            "话题列表虚拟滚动",
            Some("3/5"),
        ))
        .child(row(
            "g-r3",
            false,
            ui::icon(Sym::Check, theme::icon::TAG, k.ok).into_any_element(),
            "staging 每日巡检",
            Some("09:12"),
        ))
        .child(row(
            "g-r4",
            false,
            div()
                .size(px(7.))
                .rounded_full()
                .border(px(1.5))
                .border_color(k.ink_3)
                .into_any_element(),
            "搜索支持中文两字词",
            Some("等小王"),
        ))
}

fn cards(cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    div()
        .flex()
        .gap(space::S16)
        .child(
            ui::card(cx)
                .w(px(300.))
                .p(space::S16)
                .flex()
                .flex_col()
                .gap(space::S8)
                .child(
                    div()
                        .t_card_title()
                        .text_color(k.ink)
                        .child("把 qqbot v2.14 发布到生产吗？"),
                )
                .child(
                    div()
                        .t_small()
                        .text_color(k.ink_2)
                        .child("所有群重启约 2 分钟；出问题可以一键回到 v2.13"),
                ),
        )
        .child(
            div()
                .p(space::S12)
                .bg(k.rail)
                .rounded(theme::radius::LG)
                .child(
                    ui::rail_card(cx)
                        .w(px(260.))
                        .child(
                            div()
                                .t_meta_medium()
                                .text_color(k.ink_3)
                                .child("用时和成本"),
                        )
                        .child(
                            div()
                                .t_mono()
                                .text_color(k.ink)
                                .child("用时 41 分钟 · ¥6.20"),
                        ),
                ),
        )
}

fn motion(cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    let bars = [0.4f32, 0.7, 0.55, 0.9, 0.65];
    div()
        .flex()
        .items_end()
        .gap(space::S24)
        .child(div().flex().gap(space::S6).children((0..5).map(|i| {
            ui::motion::rise(
                SharedString::from(format!("g-rise-{i}")),
                i,
                div().size(px(20.)).rounded(theme::radius::SM).bg(k.sel),
                cx,
            )
        })))
        .child(ui::motion::breathe(
            "g-breathe",
            div().size(px(10.)).rounded_full().bg(k.live),
            cx,
        ))
        // A sample answered now: its check draws.
        .child(ui::motion::check_mark(
            "g-check",
            ui::icon(Sym::Check, theme::icon::ROW, k.ok),
            px(16.),
            ui::motion::just_answered(crate::selftest::clock::now_ms()),
            cx,
        ))
        .child(div().h(px(48.)).flex().items_end().gap(space::S4).children(
            bars.iter().enumerate().map(|(i, h)| {
                ui::motion::bar_grow(
                    SharedString::from(format!("g-bar-{i}")),
                    div().w(px(10.)).rounded_t(px(2.)).bg(k.ink),
                    px(48. * h),
                    ui::motion::stagger_delay(i),
                    cx,
                )
            }),
        ))
}

/// The page body.
pub fn page(cx: &App) -> gpui::Div {
    div()
        .p(space::S32)
        .pt(theme::TOOLBAR_HEIGHT)
        .flex()
        .gap(space::S44)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(space::S24)
                .child(section("SURFACES v3", cx).child(surfaces(cx)))
                .child(section("MEMBER TINTS · 岗位 TAGS", cx).child(tints(cx)))
                .child(section("BUTTONS v3 (btn-hi, sec-hi)", cx).child(buttons(cx)))
                .child(section("CARDS (lift) · RAIL CARD", cx).child(cards(cx))),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(space::S24)
                .child(section("STATES · MONO NUMERALS", cx).child(states(cx)))
                .child(section("RINGS (live breathes, you static)", cx).child(rings()))
                .child(section("RAIL ROWS", cx).child(rail_rows(cx)))
                .child(section("MOTION (rise, breathe, check, bars)", cx).child(motion(cx))),
        )
}
