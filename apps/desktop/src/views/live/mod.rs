//! 实时查看 (Live.dc.html): a member's computer for one run. MVP: a terminal
//! rebuilt from the run's tool events, 回放 by step (the plan's step times),
//! 验收标准 the run reported, the plan with its progress, and 接管 / 交还 /
//! 停止 / 开发者视图. While someone holds the computer, the run is paused, a
//! banner says so and 交还 is the primary action; the machine's screen
//! sharing address, when it reports one, opens from the banner.
//!
//! Layout (half the window, `raised`): a 48 pt header (name, 实时 / 你在操作,
//! machine, 已用, 全屏查看, close), the screen frame (16:10, a menu bar and a
//! terminal window over the backdrop), the 回放 track, the 验收标准 and 计划
//! cards, and the footer with the actions.

pub mod scenes;

use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, Entity, InteractiveElement as _,
    IntoElement, ParentElement, Pixels, Render, SharedString, StatefulInteractiveElement as _,
    Styled, Subscription, Task, Window, div, prelude::FluentBuilder as _, px, relative,
};

use crate::{
    app::{RightPane, Services},
    copy::{fill, live as c, nav as n},
    data::{
        AdminError, BotRunSummary, Member, MemberLevel, Millis, PlanState,
        v2::{AcceptanceItem, MachineScreen, ScreenLine, ScreenLineKind},
    },
    theme::{self, Type as _, motion, space},
    ui::{
        self, Sym,
        native::{
            NativeType as _, PushSize, PushStyle, ToolbarGroup, borderless_text, disabled_push,
            metrics, native_colors,
            overlays::{decision_bar, sym_toolbar_item},
            push_button,
        },
        sf_symbols::SymSize,
    },
    views::{
        process::{ProcessView, stop_run},
        shell::{PaneView, clock, member_name, panes::watch, stub_body},
        timeline::plan,
    },
};

/// The pane header: 48 pt.
const HEADER: Pixels = px(48.);
/// The side padding of the pane body.
const SIDE: Pixels = px(24.);
/// The screen frame is 16:10.
const SCREEN_RATIO: f32 = 1.6;
/// The menu bar of the screen and the title bar of its terminal window.
const MENU_BAR: Pixels = px(24.);
const WINDOW_BAR: Pixels = px(28.);
const TRAFFIC: Pixels = px(10.);
/// The terminal cursor: 7 x 14.
const CURSOR_W: Pixels = px(7.);
const CURSOR_H: Pixels = px(14.);
/// The 回放 track: 44 pt tall, the line at 11, marks 24 pt targets.
const TRACK_HEIGHT: Pixels = px(44.);
const TRACK_LINE_TOP: Pixels = px(11.);
const MARK: Pixels = px(24.);
const MARK_DOT: Pixels = px(10.);
const MARK_NOW: Pixels = px(12.);
/// A step label under the track, centered on its mark.
const MARK_LABEL_W: Pixels = px(150.);
/// Rows of the 验收标准 card: at least 40 pt.
const CRITERION_MIN: Pixels = px(40.);
/// Rows of the plan card: 34 pt.
const PLAN_ROW: Pixels = px(34.);
/// The held banner: 48 pt.
const BANNER: Pixels = px(48.);
/// The running run's screen is read again this often while the pane shows.
const POLL: std::time::Duration = std::time::Duration::from_millis(1500);

pub struct LivePane {
    services: Services,
    /// The step 回放 shows; `None` follows the run.
    replay: Option<(String, usize)>,
    /// 全屏查看: the pane takes all the room the page can give.
    expanded: bool,
    /// 开发者视图: the run's process view in place of the screen.
    developer: Option<Entity<ProcessView>>,
    poll: Option<(String, Task<()>)>,
    _subscriptions: Vec<Subscription>,
}

impl LivePane {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        Self {
            _subscriptions: watch(&services, cx),
            services,
            replay: None,
            expanded: false,
            developer: None,
            poll: None,
        }
    }

    fn target(&self, cx: &App) -> Option<(String, String, String)> {
        let nav = self.services.nav.read(cx);
        let (g, t) = nav.topic.clone()?;
        match nav.visible_pane() {
            RightPane::Live(run) => Some((g, t, run)),
            _ => None,
        }
    }

    /// Re-renders a running run's screen every [`POLL`] while it shows.
    fn ensure_polling(&mut self, run_id: &str, cx: &mut Context<Self>) {
        if self.poll.as_ref().is_some_and(|(id, _)| id == run_id) {
            return;
        }
        let id = run_id.to_string();
        let task = cx.spawn({
            let id = id.clone();
            async move |this, cx| {
                loop {
                    cx.background_executor().timer(POLL).await;
                    let go_on = this.update(cx, |this, cx| {
                        let showing = this.target(cx).is_some_and(|(_, _, r)| r == id);
                        if showing {
                            cx.notify();
                        } else {
                            this.poll = None;
                        }
                        showing
                    });
                    if !matches!(go_on, Ok(true)) {
                        break;
                    }
                }
            }
        });
        self.poll = Some((id, task));
    }

    fn pause(&self, run: &BotRunSummary, take: bool, cx: &mut App) {
        let p = &self.services.provider;
        let reply = if take {
            p.take_over_run(&run.group_id, &run.topic_id, &run.id)
        } else {
            p.hand_back_run(&run.group_id, &run.topic_id, &run.id)
        };
        let title = if take {
            crate::copy::overlays::TAKE_OVER_REFUSED
        } else {
            crate::copy::overlays::HAND_BACK_REFUSED
        };
        cx.spawn(async move |cx: &mut gpui::AsyncApp| {
            let result = reply.await.unwrap_or(Err(AdminError::Other));
            if let Err(e) = result {
                // A refusal blocks: an alert, no toast (NATIVE.md §7.4).
                cx.update(|cx| alert_in_active_window(title, e, cx));
            }
        })
        .detach();
    }

    /// The pane's toolbar row (NATIVE.md §2): `Forge 的电脑` (Title 3) over
    /// the state, the machine and the time it has run (Subheadline,
    /// `secondary`, the state as a symbol, no pill); 放大 and 关闭 as
    /// toolbar items in one capsule.
    fn header(
        &self,
        bot: &str,
        screen: &MachineScreen,
        run: &BotRunSummary,
        held_by_me: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let n = native_colors(cx);
        let state: Option<(Sym, gpui::Hsla, String)> = match &screen.paused_by {
            Some(_) if held_by_me => Some((Sym::Pointer, n.label, c::LIVE_YOU_HOLD.to_string())),
            Some(who) => Some((
                Sym::Pointer,
                n.secondary,
                fill(
                    c::LIVE_HELD_BY_OTHER,
                    &[("who", &member_name(&self.services, who))],
                ),
            )),
            None if run.status.is_active() => Some((Sym::Live, n.blue, c::LIVE_NOW.to_string())),
            None => None,
        };
        let end = run.ended_at.unwrap_or_else(now_ms);
        let elapsed = screen
            .started_at
            .or(run.started_at)
            .map(|s| elapsed_text((end - s).max(0), cx));
        let close = self.services.clone();
        let dot = || div().flex_none().child("·");
        let mut subtitle = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .min_w(px(0.))
            .n_subheadline()
            .n_figures()
            .text_color(n.secondary);
        let mut first = true;
        if let Some((sym, color, text)) = state {
            subtitle = subtitle
                .child(sym.icon(SymSize::INLINE_SMALL).text_color(color))
                .child(div().flex_none().text_color(n.label).child(text));
            first = false;
        }
        if let Some(m) = screen.machine_name.clone() {
            if !first {
                subtitle = subtitle.child(dot());
            }
            subtitle = subtitle.child(div().min_w_0().one_line().child(m));
            first = false;
        }
        if let Some(e) = elapsed {
            if !first {
                subtitle = subtitle.child(dot());
            }
            subtitle = subtitle.child(div().flex_none().child(e));
        }
        div()
            .flex_none()
            .h(metrics::TOOLBAR_ROW)
            .pl(metrics::TOOLBAR_LEADING)
            .pr(metrics::TOOLBAR_TRAILING)
            .flex()
            .items_center()
            .gap(metrics::TOOLBAR_GROUP_GAP)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .n_title3()
                            .text_color(n.label)
                            .one_line()
                            .child(fill(c::LIVE_COMPUTER, &[("name", bot)])),
                    )
                    .child(subtitle),
            )
            .child(ToolbarGroup::new([
                sym_toolbar_item(
                    "live-expand",
                    if self.expanded {
                        Sym::Collapse
                    } else {
                        Sym::Expand
                    },
                    if self.expanded {
                        c::LIVE_COLLAPSE
                    } else {
                        c::LIVE_EXPAND
                    },
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.expanded = !this.expanded;
                    cx.notify();
                })),
                sym_toolbar_item("live-close", Sym::Close, c::LIVE_CLOSE, cx).on_click(
                    move |_, _, cx| {
                        close
                            .nav
                            .update(cx, |nav, cx| nav.set_right_pane(RightPane::None, cx))
                    },
                ),
            ]))
    }

    /// 你在操作 Forge 的电脑 · Forge 已暂停 [在屏幕共享里打开] [交还].
    fn held_banner(
        &self,
        bot: &str,
        screen: &MachineScreen,
        run: &BotRunSummary,
        held_by_me: bool,
        can_control: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let who = screen
            .paused_by
            .as_deref()
            .map(|w| member_name(&self.services, w))
            .unwrap_or_default();
        let title = if held_by_me {
            fill(c::LIVE_HELD_TITLE, &[("name", bot)])
        } else {
            fill(c::LIVE_HELD_OTHER, &[("who", &who), ("name", bot)])
        };
        let n = native_colors(cx);
        let share = screen_share_url(screen).map(|url| {
            let name = bot.to_string();
            push_button(
                "live-screen-share",
                c::LIVE_OPEN_SCREEN_SHARE,
                PushStyle::Standard,
                PushSize::Regular,
                cx,
            )
            .on_click(move |_, _, _| {
                crate::views::artifacts::window::ask_before_screen_share(&name, url.clone())
            })
        });
        let hand_back = {
            let run = run.clone();
            push_button(
                "live-hand-back",
                c::LIVE_HAND_BACK,
                PushStyle::Default,
                PushSize::Regular,
                cx,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.pause(&run, false, cx)))
        };
        // A decision bar (NATIVE.md §9.2): on `fill`, no border, no shadow.
        decision_bar(cx)
            .mb(px(12.))
            .child(Sym::Pointer.icon(SymSize::INLINE).text_color(n.label))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .n_headline()
                            .text_color(n.label)
                            .one_line()
                            .child(title),
                    )
                    .child(
                        div()
                            .n_subheadline()
                            .text_color(n.secondary)
                            .one_line()
                            // The copy starts with a separator for an inline
                            // run; on its own line it goes.
                            .child(
                                fill(c::LIVE_HELD_PAUSED, &[("name", bot)])
                                    .trim_start_matches(['·', ' '])
                                    .to_string(),
                            ),
                    ),
            )
            .children(share)
            .children(can_control.then_some(hand_back))
            .into_any_element()
    }

    /// The screen: a menu bar and the terminal window over the backdrop.
    #[allow(clippy::too_many_arguments)]
    fn screen(
        &self,
        screen: &MachineScreen,
        lines: &[&ScreenLine],
        title: String,
        cursor: bool,
        pointer: Option<String>,
        cx: &App,
    ) -> AnyElement {
        let k = theme::colors(cx);
        let menu = div()
            .flex_none()
            .h(MENU_BAR)
            .px(space::S12)
            .flex()
            .items_center()
            .gap(space::S16)
            .bg(k.rail)
            .border_b_1()
            .border_color(k.line)
            .t_meta()
            .text_color(k.ink_2)
            .child(
                div()
                    .semibold()
                    .text_color(k.ink)
                    .child(c::LIVE_MENU_TERMINAL),
            )
            .child(c::LIVE_MENU_FILE)
            .child(c::LIVE_MENU_EDIT)
            .child(c::LIVE_MENU_WINDOW)
            .child(div().flex_1())
            .child(div().mono().child(menu_clock()));
        let dot = |color: gpui::Hsla| div().size(TRAFFIC).rounded_full().bg(color);
        let bar = div()
            .flex_none()
            .h(WINDOW_BAR)
            .px(space::S10)
            .flex()
            .items_center()
            .gap(space::S6)
            .border_b_1()
            .border_color(k.line)
            .child(dot(gpui::rgb(0xFF5F57).into()))
            .child(dot(gpui::rgb(0xFEBC2E).into()))
            .child(dot(gpui::rgb(0x28C840).into()))
            .child(
                div()
                    .flex_1()
                    .mr(px(42.))
                    .min_w_0()
                    .one_line()
                    .text_center()
                    .t_meta()
                    .text_color(k.ink_3)
                    .child(title),
            );
        let rows: Vec<AnyElement> = if lines.is_empty() {
            vec![
                div()
                    .text_color(k.ink_3)
                    .child(c::LIVE_NO_LINES)
                    .into_any_element(),
            ]
        } else {
            lines
                .iter()
                .enumerate()
                .map(|(i, l)| {
                    let row = div()
                        .when(i > 0 && l.kind == ScreenLineKind::Command, |d| {
                            d.mt(space::S8)
                        })
                        .one_line();
                    let row = match l.kind {
                        ScreenLineKind::Command => row
                            .child(div().flex_none().text_color(k.ink_3).child("$ "))
                            .flex()
                            .child(
                                div()
                                    .min_w_0()
                                    .one_line()
                                    .text_color(k.ink)
                                    .child(l.text.clone()),
                            ),
                        ScreenLineKind::Output => row.text_color(k.ink_2).child(l.text.clone()),
                        ScreenLineKind::Result => row.text_color(k.ink).child(l.text.clone()),
                        ScreenLineKind::Error => row.text_color(k.bad_ink).child(l.text.clone()),
                    };
                    ui::motion::rise(
                        SharedString::from(format!("live-line-{}-{i}", screen.run_id)),
                        i,
                        row,
                        cx,
                    )
                })
                .collect()
        };
        let cursor = cursor.then(|| blinking_cursor(&screen.run_id, cx));
        let terminal = div()
            .absolute()
            .left(relative(0.04))
            .right(relative(0.04))
            .top(relative(0.07))
            .bottom(relative(0.09))
            .rounded(theme::radius::MD)
            .border_1()
            .border_color(k.line_2)
            .bg(k.bg)
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(bar)
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .px(space::S14)
                    .py(space::S12)
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .text_size(px(12.))
                    .line_height(px(20.))
                    .code_font()
                    .children(rows)
                    .children(cursor),
            );
        let pointer = pointer.map(|name| {
            div()
                .absolute()
                .left(relative(0.78))
                .top(relative(0.56))
                .child(ui::motion::rise(
                    "live-pointer",
                    0,
                    div()
                        .flex()
                        .items_start()
                        .gap(space::S2)
                        .child(ui::icon(Sym::Pointer, theme::icon::HEADER, k.ink))
                        .child(
                            div()
                                .mt(px(14.))
                                .h(px(18.))
                                .px(space::S6)
                                .rounded(theme::TAG_RADIUS)
                                .bg(k.btn)
                                .text_size(px(11.))
                                .line_height(px(18.))
                                .medium()
                                .text_color(k.btn_ink)
                                .max_w(px(120.))
                                .one_line()
                                .child(name),
                        ),
                    cx,
                ))
        });
        div()
            .flex_none()
            .w_full()
            .aspect_ratio(SCREEN_RATIO)
            .rounded(theme::radius::FIELD)
            .border_1()
            .border_color(k.line_2)
            .shadow(k.lift())
            .overflow_hidden()
            .flex()
            .flex_col()
            .bg(k.sunken)
            .child(menu)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .bg(k.ph[1])
                    .child(terminal)
                    .children(pointer),
            )
            .into_any_element()
    }

    /// 回放: a mark at the end of each finished step, the current step at the end.
    fn replay(
        &self,
        run: &BotRunSummary,
        times: &[Millis],
        current: Option<usize>,
        shown: Option<usize>,
        end: Millis,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let k = theme::colors(cx);
        let start = *times.first()?;
        let span = (end - start).max(1) as f32;
        let active = run.status.is_active();
        let last = current.unwrap_or(times.len().saturating_sub(1));
        let mut marks: Vec<AnyElement> = Vec::new();
        let mut labels: Vec<AnyElement> = Vec::new();
        for i in 0..times.len() {
            let is_last = i == last;
            let pos = if is_last {
                1.0
            } else {
                match times.get(i + 1) {
                    Some(t) => ((t - start) as f32 / span).clamp(0., 1.),
                    None => continue,
                }
            };
            let selected = shown == Some(i);
            let step_text = run.plan.get(i).map(|p| p.text.clone()).unwrap_or_default();
            let dot = if is_last && active {
                div()
                    .relative()
                    .size(MARK_NOW)
                    .rounded_full()
                    .bg(k.live)
                    .child(ui::motion::breathe(
                        "live-replay-now",
                        div()
                            .absolute()
                            .top(px(-5.))
                            .left(px(-5.))
                            .size(px(22.))
                            .rounded_full()
                            .border(px(1.5))
                            .border_color(k.live),
                        cx,
                    ))
                    .into_any_element()
            } else {
                div()
                    .size(MARK_DOT)
                    .rounded_full()
                    .bg(if selected { k.ink } else { k.raised })
                    .border_2()
                    .border_color(if selected { k.ink } else { k.ink_3 })
                    .into_any_element()
            };
            let id = ElementId::Name(format!("live-mark-{}-{i}", run.id).into());
            marks.push(
                div()
                    .id(id)
                    .absolute()
                    .top_0()
                    .left(relative(pos))
                    .ml(-MARK / 2.)
                    .size(MARK)
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(dot)
                    .tooltip({
                        let tip: SharedString = if is_last {
                            c::LIVE_BACK_TO_NOW.into()
                        } else {
                            fill(c::LIVE_REPLAY_STEP, &[("n", &(i + 1).to_string())]).into()
                        };
                        move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                        }
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let rid = this.target(cx).map(|(_, _, r)| r).unwrap_or_default();
                        this.replay = (!is_last).then_some((rid, i));
                        cx.notify();
                    }))
                    .into_any_element(),
            );
            if is_last {
                let word = if active {
                    c::LIVE_NOW_AT
                } else {
                    c::LIVE_ENDED_AT
                };
                labels.push(
                    div()
                        .absolute()
                        .top(px(26.))
                        .right(-MARK / 2.)
                        .flex()
                        .items_center()
                        .gap(space::S6)
                        .t_meta()
                        .medium()
                        .text_color(k.ink)
                        .child(word)
                        .child(
                            div()
                                .mono()
                                .font_weight(gpui::FontWeight::NORMAL)
                                .text_color(k.ink_3)
                                .child(clock(if active { now_ms() } else { end })),
                        )
                        .into_any_element(),
                );
            } else {
                labels.push(
                    div()
                        .absolute()
                        .top(px(26.))
                        .left(relative(pos))
                        .ml(-MARK_LABEL_W / 2.)
                        .w(MARK_LABEL_W)
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(space::S4)
                                .t_meta()
                                .text_color(if selected { k.ink } else { k.ink_2 })
                                .max_w(MARK_LABEL_W)
                                .min_w_0()
                                .child(div().flex_none().child(ui::icon(
                                    Sym::Check,
                                    theme::icon::TAG,
                                    k.ink_3,
                                )))
                                .child(div().min_w_0().one_line().child(step_text)),
                        )
                        .into_any_element(),
                );
            }
        }
        let fill_line = div()
            .absolute()
            .left_0()
            .top(TRACK_LINE_TOP)
            .h(px(2.))
            .rounded(px(1.))
            .bg(k.ink_3);
        let fill_line = if cx.reduce_motion() {
            fill_line.w_full().into_any_element()
        } else {
            use gpui::{Animation, AnimationExt as _};
            fill_line
                .with_animation(
                    ElementId::Name(format!("live-track-{}", run.id).into()),
                    Animation::new(motion::LIVE_TRACK_FILL).with_easing(motion::out_fn()),
                    |el, t| el.w(relative(t)),
                )
                .into_any_element()
        };
        Some(ui::motion::rise(
            "live-replay",
            1,
            div()
                .flex_none()
                .mt(space::S16)
                .flex()
                .items_start()
                .gap(space::S12)
                .child(
                    div()
                        .flex_none()
                        .t_meta()
                        .line_height(px(24.))
                        .text_color(k.ink_3)
                        .child(c::LIVE_REPLAY),
                )
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .min_w_0()
                        .h(TRACK_HEIGHT)
                        .mx(space::S12)
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .top(TRACK_LINE_TOP)
                                .h(px(2.))
                                .rounded(px(1.))
                                .bg(k.line_2),
                        )
                        .child(fill_line)
                        .children(labels)
                        .children(marks),
                ),
            cx,
        ))
    }

    /// The file, link and 产出 a criterion's proof points at; each opens it.
    fn evidence_chips(
        &self,
        run: &BotRunSummary,
        index: usize,
        item: &AcceptanceItem,
        cx: &App,
    ) -> Vec<AnyElement> {
        let k = theme::colors(cx);
        let chip = |id: String, icon: Sym, label: String| {
            div()
                .id(SharedString::from(id))
                .flex_none()
                .max_w(px(160.))
                .flex()
                .items_center()
                .gap(space::S4)
                .px(space::S6)
                .rounded(theme::radius::SM)
                .bg(k.sunken)
                .hover(|d| d.text_color(k.ink))
                .child(ui::icon(icon, theme::icon::CHIP, k.ink_3))
                .child(div().min_w_0().one_line().child(label))
        };
        let mut out = Vec::new();
        if let Some(file) = item.evidence_file.clone() {
            let provider = self.services.provider.clone();
            let (g, t) = (run.group_id.clone(), run.topic_id.clone());
            out.push(
                chip(
                    format!("live-evidence-file-{index}"),
                    Sym::Attach,
                    file.name.clone(),
                )
                .on_click(move |_, _, cx| {
                    crate::views::files::open_file(
                        &provider,
                        &g,
                        &t,
                        &file,
                        crate::views::files::OpenAction::Open,
                        cx,
                    )
                })
                .into_any_element(),
            );
        }
        if let Some(url) = item.evidence_url.clone() {
            out.push(
                chip(
                    format!("live-evidence-url-{index}"),
                    Sym::OpenExternal,
                    c::LIVE_EVIDENCE_LINK.to_string(),
                )
                .on_click(move |_, _, cx| cx.open_url(&url))
                .into_any_element(),
            );
        }
        if let Some((id, version, title)) = item.evidence_artifact.clone() {
            let nav = self.services.nav.clone();
            let label = if title.is_empty() {
                c::LIVE_EVIDENCE_ARTIFACT.to_string()
            } else {
                title
            };
            out.push(
                chip(
                    format!("live-evidence-artifact-{index}"),
                    Sym::Document,
                    label,
                )
                .on_click(move |_, _, cx| {
                    let id = id.clone();
                    nav.update(cx, |nav, cx| nav.open_artifact(id, Some(version), cx))
                })
                .into_any_element(),
            );
        }
        out
    }

    fn acceptance(
        &self,
        run: &BotRunSummary,
        items: &[AcceptanceItem],
        cx: &App,
    ) -> Option<AnyElement> {
        if items.is_empty() {
            return None;
        }
        let k = theme::colors(cx);
        let met = items.iter().filter(|i| i.met == Some(true)).count();
        let count = [("met", met.to_string()), ("total", items.len().to_string())];
        let summary = match run.requested_by.as_deref() {
            Some(who) => mono_fill(
                c::LIVE_ACCEPTANCE_FROM,
                &[
                    ("name", member_name(&self.services, who), false),
                    ("met", count[0].1.clone(), true),
                    ("total", count[1].1.clone(), true),
                ],
            ),
            None => mono_fill(
                c::LIVE_ACCEPTANCE_COUNT,
                &[
                    ("met", count[0].1.clone(), true),
                    ("total", count[1].1.clone(), true),
                ],
            ),
        };
        let active = run.status.is_active();
        let rows = items.iter().enumerate().map(|(i, item)| {
            let status: AnyElement = match item.met {
                Some(true) => ui::icon(Sym::Check, theme::icon::CHIP, k.ok).into_any_element(),
                Some(false) => div()
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .child(div().text_color(k.bad_ink).child(c::LIVE_NOT_MET))
                    .child(ui::icon(Sym::Close, theme::icon::CHIP, k.bad))
                    .into_any_element(),
                None if active => div()
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .child(div().text_color(k.live_ink).child(c::LIVE_CHECKING))
                    .child(
                        div()
                            .size(px(12.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(ui::motion::breathe(
                                SharedString::from(format!("live-check-{i}")),
                                div().size(px(8.)).rounded_full().bg(k.live),
                                cx,
                            )),
                    )
                    .into_any_element(),
                None => div()
                    .text_color(k.ink_3)
                    .child(c::LIVE_UNCHECKED)
                    .into_any_element(),
            };
            div()
                .min_h(CRITERION_MIN)
                .py(space::S8)
                .flex()
                .items_center()
                .gap(space::S12)
                .when(i > 0, |d| d.border_t_1().border_color(k.line))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .t_small()
                        .text_color(k.ink)
                        .child(item.text.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(space::S8)
                        .t_meta()
                        .text_color(k.ink_2)
                        .children(
                            item.evidence
                                .clone()
                                .map(|e| div().max_w(px(220.)).one_line().child(e)),
                        )
                        .children(self.evidence_chips(run, i, item, cx))
                        .child(status),
                )
        });
        Some(ui::motion::rise(
            "live-acceptance",
            2,
            div()
                .flex_none()
                .mt(space::S16)
                .pt(space::S12)
                .px(space::S14)
                .pb(space::S4)
                // A grouped section (§8): `group` fill, no border, no shadow.
                .rounded(metrics::GROUP_RADIUS)
                .bg(native_colors(cx).group)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S8)
                        .child(
                            div()
                                .t_meta_medium()
                                .text_color(k.ink_3)
                                .child(c::LIVE_ACCEPTANCE),
                        )
                        .child(div().flex_1())
                        .child(div().t_meta().text_color(k.ink_3).child(summary)),
                )
                .child(div().mt(space::S6).children(rows)),
            cx,
        ))
    }

    /// 计划 n/m with its bar and steps (the plan card of the artboard).
    fn plan(
        &self,
        run: &BotRunSummary,
        times: &[Millis],
        bot: &str,
        held: bool,
        cx: &App,
    ) -> Option<AnyElement> {
        if run.plan.is_empty() {
            return None;
        }
        let k = theme::colors(cx);
        let done = run
            .plan
            .iter()
            .filter(|p| p.state == PlanState::Done)
            .count();
        let total = run.plan.len();
        // 3/5: the step it is on, or the steps done when none is on.
        let done = run
            .plan
            .iter()
            .position(|p| matches!(p.state, PlanState::Current | PlanState::Waiting))
            .map_or(done, |i| i + 1);
        let bar = div()
            .flex()
            .gap(space::S4)
            .children(run.plan.iter().map(|p| {
                div().flex_1().h(px(3.)).rounded(px(2.)).bg(match p.state {
                    PlanState::Done => k.ink_3,
                    PlanState::Current => k.live,
                    PlanState::Waiting => k.you,
                    PlanState::Pending => k.line_2,
                })
            }));
        let now = run.ended_at.unwrap_or_else(now_ms);
        let rows = run.plan.iter().enumerate().map(|(i, p)| {
            let started = times.get(i).copied();
            let next = times.get(i + 1).copied();
            match p.state {
                PlanState::Current | PlanState::Waiting => {
                    let mark: AnyElement = if held {
                        ui::icon(Sym::Pause, theme::icon::TAG, k.ink_3).into_any_element()
                    } else if p.state == PlanState::Waiting {
                        div()
                            .size(px(8.))
                            .rounded_full()
                            .bg(k.you)
                            .into_any_element()
                    } else {
                        ui::motion::breathe(
                            SharedString::from(format!("live-plan-{i}")),
                            div().size(px(8.)).rounded_full().bg(k.live),
                            cx,
                        )
                    };
                    div()
                        .flex()
                        .flex_col()
                        .gap(space::S2)
                        .pt(px(7.))
                        .pb(space::S8)
                        .px(space::S8)
                        .rounded(theme::radius::MD)
                        .bg(k.sunken)
                        .child(
                            div()
                                .h(px(20.))
                                .flex()
                                .items_center()
                                .gap(space::S10)
                                .child(
                                    div()
                                        .flex_none()
                                        .size(px(16.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(mark),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .one_line()
                                        .t_small()
                                        .medium()
                                        .text_color(k.ink)
                                        .child(p.text.clone()),
                                )
                                .children(started.map(|s| {
                                    div()
                                        .t_meta()
                                        .text_color(k.ink_2)
                                        .child(span_fill(plan::FOR, now - s))
                                })),
                        )
                        .when(held, |d| {
                            d.child(
                                div()
                                    .pl(px(26.))
                                    .t_meta()
                                    .text_color(k.ink_2)
                                    .child(fill(c::LIVE_PLAN_HELD, &[("name", bot)])),
                            )
                        })
                        .into_any_element()
                }
                state => {
                    let done = state == PlanState::Done;
                    let mark: AnyElement = if done {
                        ui::icon(Sym::Check, theme::icon::CHIP, k.ink_3).into_any_element()
                    } else {
                        div()
                            .size(px(10.))
                            .rounded_full()
                            .border(px(1.5))
                            .border_color(k.line_2)
                            .into_any_element()
                    };
                    let took = done
                        .then(|| started.zip(next).map(|(a, b)| b - a))
                        .flatten();
                    div()
                        .h(PLAN_ROW)
                        .px(space::S8)
                        .flex()
                        .items_center()
                        .gap(space::S10)
                        .child(
                            div()
                                .flex_none()
                                .size(px(16.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(mark),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .one_line()
                                .t_small()
                                .text_color(if done { k.ink_3 } else { k.ink_2 })
                                .child(p.text.clone()),
                        )
                        .children(took.map(|n| {
                            div()
                                .t_meta()
                                .text_color(k.ink_3)
                                .child(span_fill(plan::TOOK, n))
                        }))
                        .into_any_element()
                }
            }
        });
        Some(ui::motion::rise(
            "live-plan",
            3,
            div()
                .flex_none()
                .mt(space::S16)
                .rounded(metrics::GROUP_RADIUS)
                .bg(native_colors(cx).group)
                .overflow_hidden()
                .child(
                    div()
                        .pt(space::S12)
                        .px(space::S14)
                        .flex()
                        .flex_col()
                        .gap(space::S10)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(space::S8)
                                .t_small()
                                .medium()
                                .text_color(k.ink)
                                .child(c::LIVE_PLAN)
                                .child(
                                    div()
                                        .t_mono()
                                        .text_color(k.ink_3)
                                        .child(format!("{done}/{total}")),
                                ),
                        )
                        .child(bar),
                )
                .child(
                    div()
                        .pt(space::S8)
                        .px(space::S6)
                        .pb(space::S6)
                        .flex()
                        .flex_col()
                        .gap(space::S2)
                        .children(rows),
                ),
            cx,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn footer(
        &self,
        run: &BotRunSummary,
        bot: &str,
        step: &str,
        held: bool,
        held_by_me: bool,
        can_control: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = run.status.is_active();
        let stopping = self.services.provider.run_stopping(&run.id);
        let n = native_colors(cx);
        let take = (!held && active && can_control).then(|| {
            let run = run.clone();
            push_button(
                "live-take-over",
                c::LIVE_TAKE_OVER,
                PushStyle::Default,
                PushSize::Regular,
                cx,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.pause(&run, true, cx)))
        });
        // 停止 asks in an alert first (停止成员的工作, NATIVE.md §7.2).
        let stop = active.then(|| {
            let (services, run) = (self.services.clone(), run.clone());
            disabled_push(
                push_button(
                    "live-stop",
                    if stopping {
                        c::LIVE_STOPPING
                    } else {
                        c::LIVE_STOP
                    },
                    PushStyle::Destructive,
                    PushSize::Regular,
                    cx,
                ),
                stopping,
                cx,
            )
            .on_click(move |_, _, cx| stop_run(&services, &run, cx))
        });
        let hint = if held && held_by_me {
            Some(fill(c::LIVE_HELD_HINT, &[("name", bot), ("step", step)]))
        } else if active && !held && can_control {
            Some(fill(c::LIVE_TAKE_OVER_HINT, &[("name", bot)]))
        } else {
            None
        };
        let developer_on = self.developer.is_some();
        // The bottom bar: the hint at the leading edge, the developer view
        // as a borderless button, then 停止 and the default 接管 at the
        // trailing edge.
        div()
            .flex_none()
            .px(px(20.))
            .py(px(12.))
            .border_t_1()
            .border_color(n.separator)
            .flex()
            .items_center()
            .gap(px(12.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .one_line()
                    .n_subheadline()
                    .text_color(n.secondary)
                    .children(hint),
            )
            .child(
                borderless_text(
                    "live-developer",
                    if developer_on {
                        c::LIVE_BACK_TO_SCREEN
                    } else {
                        c::LIVE_DEVELOPER
                    },
                    cx,
                )
                .text_color(n.secondary)
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.developer.take().is_none() {
                        let services = this.services.clone();
                        let view = cx.new(|cx| ProcessView::new(services, cx));
                        view.update(cx, |v, cx| v.set_visible(true, cx));
                        this.developer = Some(view);
                    }
                    cx.notify();
                })),
            )
            .children(stop)
            .children(take)
    }
}

impl LivePane {
    /// The run's steps (the process view) and a bottom bar with 停止.
    fn steps_only(&mut self, run: &BotRunSummary, cx: &mut Context<Self>) -> gpui::Div {
        let n = native_colors(cx);
        let view = match &self.developer {
            Some(v) => v.clone(),
            None => {
                let services = self.services.clone();
                let view = cx.new(|cx| ProcessView::new(services, cx));
                view.update(cx, |v, cx| {
                    v.set_titled(false);
                    v.set_visible(true, cx)
                });
                self.developer = Some(view.clone());
                view
            }
        };
        let stopping = self.services.provider.run_stopping(&run.id);
        let stop = run.status.is_active().then(|| {
            let (services, run) = (self.services.clone(), run.clone());
            disabled_push(
                push_button(
                    "live-stop",
                    if stopping {
                        c::LIVE_STOPPING
                    } else {
                        c::LIVE_STOP
                    },
                    PushStyle::Destructive,
                    PushSize::Regular,
                    cx,
                ),
                stopping,
                cx,
            )
            .on_click(move |_, _, cx| stop_run(&services, &run, cx))
        });
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(n.content)
            // The pane's toolbar row (52 pt): its title, Title 3.
            .child(
                div()
                    .flex_none()
                    .h(metrics::TOOLBAR_ROW)
                    .px(space::S16)
                    .flex()
                    .items_center()
                    .n_title3()
                    .text_color(n.label)
                    .child(crate::copy::process::RUNS),
            )
            .child(div().flex_1().min_h(px(0.)).child(view))
            .children(stop.map(|stop| {
                div()
                    .flex_none()
                    .px(px(20.))
                    .py(px(12.))
                    .border_t_1()
                    .border_color(n.separator)
                    .flex()
                    .items_center()
                    .child(div().flex_1())
                    .child(stop)
            }))
    }
}

impl PaneView for LivePane {
    fn width(&self, window_w: Pixels) -> Pixels {
        if self.expanded {
            // The shell gives the page its minimum and the pane the rest.
            window_w
        } else {
            theme::live_pane_width(window_w)
        }
    }
}

/// Opens the error alert of a refused request in the active window.
fn alert_in_active_window(title: &'static str, e: AdminError, cx: &mut App) {
    if let Some(window) = cx.active_window() {
        let _ = window.update(cx, |_, window, cx| {
            crate::views::nav::dialogs::error_alert(
                title,
                crate::views::settings::widgets::error_text(e),
                |_, _| {},
                window,
                cx,
            )
        });
    }
}

/// Who may 接管 or 交还 a run, as the server decides: a person who asked
/// for the run, or an owner.
fn may_control(me: Option<&Member>, run: &BotRunSummary) -> bool {
    me.is_some_and(|m| {
        !m.is_bot()
            && !m.deactivated
            && (run.requested_by.as_deref() == Some(m.id.as_str()) || m.level == MemberLevel::Owner)
    })
}

/// The screen sharing address to offer, when it is one the app may open
/// (`vnc`, `http` or `https`; never `file:`, `smb:` or another app's scheme).
fn screen_share_url(screen: &MachineScreen) -> Option<String> {
    screen
        .takeover_url
        .as_deref()
        .map(str::trim)
        .filter(|u| artifact_view::is_screen_share_url(u))
        .map(str::to_string)
}

fn now_ms() -> Millis {
    crate::selftest::clock::utc_now().timestamp_millis()
}

/// `周一 10:21`, the menu bar clock.
fn menu_clock() -> String {
    use chrono::Datelike as _;
    let now = crate::selftest::clock::local_now();
    let day = match now.weekday() {
        chrono::Weekday::Mon => n::WEEKDAY_MON,
        chrono::Weekday::Tue => n::WEEKDAY_TUE,
        chrono::Weekday::Wed => n::WEEKDAY_WED,
        chrono::Weekday::Thu => n::WEEKDAY_THU,
        chrono::Weekday::Fri => n::WEEKDAY_FRI,
        chrono::Weekday::Sat => n::WEEKDAY_SAT,
        chrono::Weekday::Sun => n::WEEKDAY_SUN,
    };
    format!("{day} {}", now.format("%H:%M"))
}

/// A filled template whose values marked `true` are set in mono (`.mono`).
fn mono_fill(template: &str, values: &[(&str, String, bool)]) -> gpui::Div {
    let mut parts: Vec<AnyElement> = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else {
            break;
        };
        let key = &rest[open + 1..open + close];
        if open > 0 {
            parts.push(rest[..open].to_string().into_any_element());
        }
        match values.iter().find(|(k, _, _)| *k == key) {
            Some((_, v, true)) => parts.push(div().mono().child(v.clone()).into_any_element()),
            Some((_, v, false)) => parts.push(v.clone().into_any_element()),
            None => parts.push(rest[open..open + close + 1].to_string().into_any_element()),
        }
        rest = &rest[open + close + 1..];
    }
    if !rest.is_empty() {
        parts.push(rest.to_string().into_any_element());
    }
    div()
        .flex()
        .items_center()
        .whitespace_nowrap()
        .children(parts)
}

/// A span in `copy` with its numbers in mono (the timeline's formatter).
fn span_fill(copy: plan::SpanCopy, ms: i64) -> gpui::Div {
    let (template, values) = plan::Span::of(ms).parts(copy);
    let values: Vec<(&str, String, bool)> = values.into_iter().map(|(k, v)| (k, v, true)).collect();
    mono_fill(template, &values)
}

/// 已用 19 分钟, or 已用 2 小时 5 分钟.
fn elapsed_text(ms: i64, cx: &App) -> impl IntoElement {
    let k = theme::colors(cx);
    let copy = plan::SpanCopy {
        minutes: c::LIVE_ELAPSED,
        hours: c::LIVE_ELAPSED_HOURS,
    };
    div()
        .flex_none()
        .t_meta()
        .text_color(k.ink_3)
        .child(span_fill(copy, ms))
}

/// The terminal cursor: a block that blinks while the run works (still with
/// Reduce motion).
fn blinking_cursor(run_id: &str, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let block = div().w(CURSOR_W).h(CURSOR_H).bg(k.ink_2);
    let row = div().h(px(20.)).flex().items_center();
    if cx.reduce_motion() {
        return row.child(block).into_any_element();
    }
    use gpui::{Animation, AnimationExt as _};
    row.child(block.with_animation(
        ElementId::Name(format!("live-cursor-{run_id}").into()),
        Animation::new(motion::LIVE_CURSOR_BLINK).repeat(),
        |el, t| el.opacity(if t < 0.5 { 1. } else { 0. }),
    ))
    .into_any_element()
}

impl Render for LivePane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some((g, t, run_id)) = self.target(cx) else {
            return stub_body(Sym::Computer, c::LIVE_EMPTY, None, cx).into_any_element();
        };
        let p = self.services.provider.clone();
        let Some(run) = p.runs(&g, &t).into_iter().find(|r| r.id == run_id) else {
            return stub_body(Sym::Computer, c::LIVE_EMPTY, None, cx).into_any_element();
        };
        // v12/fx-overlays: a run without a screen (a cloud runner, an older
        // server) shows its steps, with 停止 while it runs.
        let Some(screen) = p.machine_screen(&run_id) else {
            return self.steps_only(&run, cx).into_any_element();
        };
        if run.status.is_active() {
            self.ensure_polling(&run_id, cx);
        }
        if self.replay.as_ref().is_some_and(|(r, _)| *r != run_id) {
            self.replay = None;
        }
        let bot = member_name(&self.services, &run.bot_id);
        crate::selftest::drawn::mark("live", format!("{bot} {}", run.id));
        let me = self.services.me();
        let held = screen.paused_by.is_some();
        let held_by_me = held && screen.paused_by == me;
        let can_control = may_control(me.as_deref().and_then(|id| p.member(id)).as_ref(), &run);
        let times = p.run_step_times(&run_id);
        let current = run
            .plan
            .iter()
            .position(|s| matches!(s.state, PlanState::Current | PlanState::Waiting))
            .or_else(|| run.plan.iter().rposition(|s| s.state == PlanState::Done));
        let replay_step = self.replay.as_ref().map(|(_, s)| *s);
        let shown_step = replay_step.or(current);
        let step_text = current
            .and_then(|i| run.plan.get(i))
            .map(|s| s.text.clone())
            .unwrap_or_default();
        let shown_text = shown_step
            .and_then(|i| run.plan.get(i))
            .map(|s| s.text.clone())
            .unwrap_or_default();

        // One screen per step; a run without step info shows every line.
        let stepped = screen.lines.iter().any(|l| l.step.is_some());
        let lines: Vec<&ScreenLine> = screen
            .lines
            .iter()
            .filter(|l| !stepped || shown_step.is_none() || l.step == shown_step)
            .collect();
        let title = match (&screen.machine_name, shown_text.is_empty()) {
            (Some(m), false) => fill(
                c::LIVE_TERMINAL_TITLE,
                &[("machine", m), ("step", &shown_text)],
            ),
            (Some(m), true) => m.clone(),
            (None, _) => shown_text.clone(),
        };
        let live_now = run.status.is_active() && !held && replay_step.is_none();
        // Show the newest lines that fit the terminal window, from its top.
        let window_w = window.viewport_size().width;
        let pane_w = self
            .width(window_w)
            .min(window_w - theme::RAIL_WIDTH - theme::PAGE_MIN_WIDTH)
            .max(theme::LIVE_PANE_MIN_WIDTH);
        let screen_h = (pane_w - SIDE * 2.) / SCREEN_RATIO;
        let room = (screen_h - MENU_BAR) * 0.84 - WINDOW_BAR - space::S12 * 2.;
        let mut budget = f32::from(room) - if live_now { 20. } else { 0. };
        let mut keep = 0;
        for (i, l) in lines.iter().enumerate().rev() {
            let gap = if i > 0 && l.kind == ScreenLineKind::Command {
                8.
            } else {
                0.
            };
            if budget < 20. + gap {
                break;
            }
            budget -= 20. + gap;
            keep += 1;
        }
        let lines: Vec<&ScreenLine> = lines[lines.len() - keep..].to_vec();
        let pointer = held_by_me
            .then(|| me.as_deref().map(|m| member_name(&self.services, m)))
            .flatten();
        let end = run.ended_at.unwrap_or_else(|| {
            let last_line = screen.lines.last().map(|l| l.at).unwrap_or(0);
            let last_step = times.last().copied().unwrap_or(0);
            run.updated_at.max(last_line).max(last_step)
        });
        let criteria = p.acceptance_criteria(&run_id);

        let body: AnyElement = match &self.developer {
            Some(view) => div()
                .flex_1()
                .min_h(px(0.))
                .child(view.clone())
                .into_any_element(),
            None => div()
                .id("live-body")
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .child(
                    div()
                        .pt(space::S4)
                        .px(SIDE)
                        .pb(space::S20)
                        .flex()
                        .flex_col()
                        .children(held.then(|| {
                            self.held_banner(&bot, &screen, &run, held_by_me, can_control, cx)
                        }))
                        .child(self.screen(&screen, &lines, title, live_now, pointer, cx))
                        .children(self.replay(&run, &times, current, replay_step, end, cx))
                        .children(self.acceptance(&run, &criteria, cx))
                        .children(self.plan(&run, &times, &bot, held, cx)),
                )
                .into_any_element(),
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(native_colors(cx).content)
            .child(self.header(&bot, &screen, &run, held_by_me, cx))
            .child(body)
            .child(self.footer(&run, &bot, &step_text, held, held_by_me, can_control, cx))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{MemberKind, RunStatus};

    fn person(id: &str, level: MemberLevel) -> Member {
        Member {
            id: id.into(),
            kind: MemberKind::Human,
            name: id.into(),
            role: None,
            level,
            avatar_url: None,
            deactivated: false,
            online: true,
            character: None,
        }
    }

    fn run(requested_by: Option<&str>) -> BotRunSummary {
        BotRunSummary {
            id: "r1".into(),
            bot_id: "b1".into(),
            group_id: "g1".into(),
            topic_id: "t1".into(),
            machine_id: Some("m1".into()),
            machine_name: None,
            status: RunStatus::Running,
            requested_by: requested_by.map(Into::into),
            created_at: 0,
            started_at: None,
            ended_at: None,
            updated_at: 0,
            summary: None,
            plan: Vec::new(),
        }
    }

    #[test]
    fn only_the_requester_or_an_owner_controls_a_run() {
        let r = run(Some("alice"));
        assert!(may_control(Some(&person("alice", MemberLevel::Member)), &r));
        assert!(may_control(Some(&person("olga", MemberLevel::Owner)), &r));
        assert!(!may_control(Some(&person("bob", MemberLevel::Member)), &r));
        assert!(!may_control(None, &r));
        let mut bot = person("alice", MemberLevel::Owner);
        bot.kind = MemberKind::Bot;
        assert!(!may_control(Some(&bot), &r));
        // A run nobody asked for: owners only.
        assert!(!may_control(
            Some(&person("alice", MemberLevel::Member)),
            &run(None)
        ));
        assert!(may_control(
            Some(&person("olga", MemberLevel::Owner)),
            &run(None)
        ));
    }

    #[test]
    fn only_screen_sharing_addresses_open() {
        let screen = |url: &str| MachineScreen {
            takeover_url: Some(url.into()),
            ..MachineScreen::default()
        };
        assert_eq!(
            screen_share_url(&screen(" vnc://mac-mini.tailnet ")),
            Some("vnc://mac-mini.tailnet".into())
        );
        for bad in [
            "file:///Users/me/Downloads/x.command",
            "smb://attacker.example/share",
            "x-apple-systempreferences:com.apple.preference.security",
        ] {
            assert_eq!(screen_share_url(&screen(bad)), None, "{bad}");
        }
    }
}
