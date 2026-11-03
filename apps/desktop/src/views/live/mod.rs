//! 实时查看 (Live.dc.html): a member's computer for one run. MVP: a terminal
//! rebuilt from the run's tool events, the plan, 验收标准, 回放 by step, and
//! 接管 / 交还 / 停止.
//!
//! Foundation stub: the screen lines, 验收标准 and the take-over state are
//! read from the provider and 接管 / 交还 call it; the live track builds the
//! artboard (screen frame, revealing lines, blinking cursor, 回放 scrubber).

pub mod scenes;

use gpui::{
    Context, IntoElement, ParentElement, Pixels, Render, SharedString, Styled, Subscription,
    Window, div, px,
};

use crate::{
    app::{RightPane, Services},
    copy::{fill, live as c},
    data::v2::ScreenLineKind,
    theme::{self, Type as _, space},
    ui::{self, ButtonKind, ButtonSize, Lucide},
    views::shell::{PaneView, member_name, panes::watch, section_label, stub_body},
};

pub struct LivePane {
    services: Services,
    _subscriptions: Vec<Subscription>,
}

impl LivePane {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        Self {
            _subscriptions: watch(&services, cx),
            services,
        }
    }
}

impl PaneView for LivePane {
    fn width(&self, window_w: Pixels) -> Pixels {
        theme::live_pane_width(window_w)
    }
}

impl Render for LivePane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let nav = self.services.nav.read(cx);
        let (Some((g, t)), RightPane::Live(run_id)) = (nav.topic.clone(), nav.visible_pane())
        else {
            return stub_body(Lucide::Monitor, c::LIVE_EMPTY, None, cx).into_any_element();
        };
        let p = self.services.provider.clone();
        let Some(screen) = p.machine_screen(&run_id) else {
            return stub_body(Lucide::Monitor, c::LIVE_EMPTY, None, cx).into_any_element();
        };
        let bot = p
            .runs(&g, &t)
            .into_iter()
            .find(|r| r.id == run_id)
            .map(|r| member_name(&self.services, &r.bot_id))
            .unwrap_or_default();
        let paused = screen.paused_by.is_some();
        let lines = screen.lines.iter().enumerate().map(|(i, l)| {
            let color = match l.kind {
                ScreenLineKind::Command => k.screen_ink,
                ScreenLineKind::Output => k.screen_dim,
                ScreenLineKind::Result => k.screen_ink,
                ScreenLineKind::Error => k.bad,
            };
            let prefix = if l.kind == ScreenLineKind::Command {
                "$ "
            } else {
                ""
            };
            ui::motion::rise(
                SharedString::from(format!("live-line-{i}")),
                i,
                div()
                    .t_mono()
                    .text_color(color)
                    .child(format!("{prefix}{}", l.text)),
                cx,
            )
        });
        let criteria = p.acceptance_criteria(&run_id);
        let (services, g2, t2, r2) = (self.services.clone(), g.clone(), t.clone(), run_id.clone());
        let action = if paused {
            ui::button(
                "live-hand-back",
                c::LIVE_HAND_BACK,
                ButtonKind::Primary,
                ButtonSize::Md,
                cx,
            )
            .on_click(move |_, _, _| {
                let _ = services.provider.hand_back_run(&g2, &t2, &r2);
            })
        } else {
            ui::button(
                "live-take-over",
                c::LIVE_TAKE_OVER,
                ButtonKind::Primary,
                ButtonSize::Md,
                cx,
            )
            .on_click(move |_, _, _| {
                let _ = services.provider.take_over_run(&g2, &t2, &r2);
            })
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(space::S12)
            .p(space::S16)
            .bg(k.rail)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .child(
                        div()
                            .t_ui()
                            .semibold()
                            .text_color(k.ink)
                            .child(fill(c::LIVE_COMPUTER, &[("name", &bot)])),
                    )
                    .child(div().t_meta().text_color(k.live_ink).child(c::LIVE_NOW))
                    .children(
                        screen
                            .machine_name
                            .clone()
                            .map(|m| div().t_mono().text_color(k.ink_3).child(m)),
                    ),
            )
            .children(paused.then(|| {
                div()
                    .p(space::S10)
                    .rounded(theme::radius::MD)
                    .bg(k.you_bg)
                    .t_small()
                    .text_color(k.you_ink)
                    .child(fill(c::LIVE_TAKEN_OVER, &[("name", &bot)]))
            }))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .rounded(px(10.))
                    .bg(k.screen)
                    .p(space::S12)
                    .flex()
                    .flex_col()
                    .gap(space::S4)
                    .overflow_hidden()
                    .children(lines),
            )
            .children((!criteria.is_empty()).then(|| {
                ui::rail_card(cx)
                    .child(section_label(c::LIVE_ACCEPTANCE, None, cx))
                    .children(criteria.iter().map(|a| {
                        let (glyph, color) = match a.met {
                            Some(true) => ("✓", k.ok),
                            Some(false) => ("✕", k.bad),
                            None => ("○", k.ink_3),
                        };
                        div()
                            .flex()
                            .gap(space::S8)
                            .t_small()
                            .child(div().w(px(12.)).text_color(color).child(glyph))
                            .child(div().text_color(k.ink).child(a.text.clone()))
                    }))
            }))
            .child(div().flex().gap(space::S8).child(action).child(ui::button(
                "live-stop",
                c::LIVE_STOP,
                ButtonKind::Secondary,
                ButtonSize::Md,
                cx,
            )))
            .into_any_element()
    }
}
