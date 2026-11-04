//! The 52 pt title bar of every page (CONCEPT2 §2): breadcrumb `群 /` in
//! `ink-3`, the title (17/600 on 首页, 产出, 成员 and 群; 15/600 on 话题),
//! the state sentence (amber only when it waits on me), and at the right a
//! group of 28 pt icon buttons: 单独问 (⌘;), 产出, 电脑, 改动 and the rail
//! toggle. The button of the open pane has the grey `sel` fill.

use std::{cell::RefCell, rc::Rc};

use gpui::{
    AnyElement, App, AppContext as _, AvailableSpace, Bounds, Element, ElementId, GlobalElementId,
    InspectorElementId, InteractiveElement as _, IntoElement, LayoutId, ParentElement, Pixels,
    SharedString, StatefulInteractiveElement as _, Style, Styled, TextLayout, Window, div, point,
    prelude::FluentBuilder as _, px, relative, size,
};

use crate::{
    app::{RightPane, Services},
    copy::topic_rail as c,
    theme::{self, StateTone, Type as _, space},
    ui::{self, Sym},
};

/// The title size: pages or a 话题.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TitleStyle {
    /// 17/600: 首页, 产出, 成员, 群.
    #[default]
    Page,
    /// 15/600 after a `群 /` breadcrumb.
    Topic,
}

/// Which title bar button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TitleButtonKind {
    SideChat,
    Outputs,
    /// Only when a member's computer is active.
    Machine,
    Changes,
    Rail,
}

impl TitleButtonKind {
    fn icon(self) -> Sym {
        match self {
            TitleButtonKind::SideChat => Sym::AskAside,
            TitleButtonKind::Outputs => Sym::Outputs,
            TitleButtonKind::Machine => Sym::Computer,
            TitleButtonKind::Changes => Sym::PullRequest,
            TitleButtonKind::Rail => Sym::Inspector,
        }
    }

    fn name(self) -> &'static str {
        match self {
            TitleButtonKind::SideChat => c::TITLE_SIDE_CHAT,
            TitleButtonKind::Outputs => c::TITLE_OUTPUTS,
            TitleButtonKind::Machine => c::TITLE_MACHINE,
            TitleButtonKind::Changes => c::TITLE_CHANGES,
            TitleButtonKind::Rail => c::TITLE_TOGGLE_RAIL,
        }
    }

    fn slug(self) -> &'static str {
        match self {
            TitleButtonKind::SideChat => "side-chat",
            TitleButtonKind::Outputs => "outputs",
            TitleButtonKind::Machine => "machine",
            TitleButtonKind::Changes => "changes",
            TitleButtonKind::Rail => "rail",
        }
    }
}

/// A title bar button and the pane it toggles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TitleButton {
    pub kind: TitleButtonKind,
    pub pane: RightPane,
}

impl TitleButton {
    pub fn new(kind: TitleButtonKind, pane: RightPane) -> Self {
        Self { kind, pane }
    }
}

/// What a page's title bar shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TitleBarSpec {
    /// `群 /` before the title (话题 pages).
    pub breadcrumb: Option<String>,
    pub title: String,
    pub style: TitleStyle,
    /// A quiet line after the title (首页: the date; 群: the purpose).
    pub subtitle: Option<String>,
    /// The state sentence and its tone (amber only for "you").
    pub state: Option<(String, StateTone)>,
    /// A quieter tail after the state sentence (`· 跑一万条消息的压测`).
    pub state_detail: Option<String>,
    /// The state is a paused run: a pause mark in place of the dot.
    pub state_paused: bool,
    /// The member's name that leads the state sentence: it is cut with `…`
    /// while the rest (`在做 3/5`) stays whole.
    pub state_name: Option<String>,
    pub buttons: Vec<TitleButton>,
}

impl TitleBarSpec {
    pub fn page(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }
}

/// What a title bar leaves out when it is narrow, most first: the step
/// goes whole, then the title is cut with `…`, then the member's name goes
/// whole. The title keeps more room than the step; `在做 3/5` (or `已暂停`)
/// never goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The step after the state (`· 跑一万条消息的压测`).
    pub step: Step,
    /// The title may be cut with `…`.
    pub title_cut: bool,
    /// The member's name leads the state sentence.
    pub name: bool,
}

/// How much of the step shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// All of it.
    Whole,
    /// At least [`STEP_MIN`], cut with `…`.
    Cut,
    Hidden,
}

impl Plan {
    /// The plans to try, the fullest first; the last always shows.
    const ORDER: [Plan; 5] = [
        Plan {
            step: Step::Whole,
            title_cut: false,
            name: true,
        },
        Plan {
            step: Step::Cut,
            title_cut: false,
            name: true,
        },
        Plan {
            step: Step::Hidden,
            title_cut: false,
            name: true,
        },
        Plan {
            step: Step::Hidden,
            title_cut: true,
            name: true,
        },
        Plan {
            step: Step::Hidden,
            title_cut: true,
            name: false,
        },
    ];
}

/// What the last frame of a title bar showed (for its tests).
#[derive(Clone, Default)]
pub struct Shown {
    pub plan: Option<Plan>,
    pub title: Option<TextLayout>,
    pub step: Option<TextLayout>,
    /// The member's name as drawn, when it shows.
    pub name: Option<String>,
}

/// The narrowest a cut step gets: `· ` and about six CJK characters.
const STEP_MIN: Pixels = px(13. * 7.6);
/// The narrowest a cut 话题 title gets before the member's name goes:
/// about six characters at 15 pt.
const TITLE_MIN: Pixels = px(15. * 6.6);
/// The narrowest a cut `群 /` breadcrumb gets.
const CRUMB_MIN: Pixels = px(48.);
/// A member's name longer than this (CJK and emoji count two) is cut at a
/// grapheme boundary with `…`; it then shows or goes as one piece.
const NAME_MAX: usize = 24;

/// `name`, cut to [`NAME_MAX`] at a grapheme boundary: an emoji with
/// skin tone and ZWJ parts (`👩🏽‍💻`) stays one piece.
fn short_name(name: &str) -> String {
    use unicode_segmentation::UnicodeSegmentation as _;
    let weight = |g: &str| if g.is_ascii() { 1 } else { 2 };
    let total: usize = name.graphemes(true).map(weight).sum();
    if total <= NAME_MAX {
        return name.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for g in name.graphemes(true) {
        if used + weight(g) > NAME_MAX - 1 {
            break;
        }
        used += weight(g);
        out.push_str(g);
    }
    format!("{}…", out.trim_end())
}

/// The parts of the state sentence: the member's name (cut by
/// [`short_name`]), the rest (`在做 3/5`) and the whole sentence.
struct Sentence {
    name: Option<String>,
    rest: String,
    full: SharedString,
}

fn sentence(spec: &TitleBarSpec) -> Option<Sentence> {
    let (text, _) = spec.state.as_ref()?;
    let is_sentence = spec.state_detail.is_some() || spec.state_paused;
    let (name, rest) = match spec.state_name.as_deref() {
        Some(n) if is_sentence && !n.is_empty() && text.starts_with(n) => (
            Some(short_name(n)),
            text[n.len()..].trim_start().to_string(),
        ),
        _ => (None, text.clone()),
    };
    let full = match &spec.state_detail {
        Some(d) => format!("{text} · {d}").into(),
        None => text.clone().into(),
    };
    Some(Sentence { name, rest, full })
}

/// The state of the bar: a short state stays whole; a member's run reads as
/// a sentence (Live.dc.html), `ink-2` text, the step in `ink-3`, a live dot
/// or, while paused, a pause mark. `plan` says what shows; when it leaves
/// something out, or the step is cut, the full sentence shows in a tooltip
/// (§15). `measure` draws the narrowest form of `plan`, without hover.
fn state(
    spec: &TitleBarSpec,
    plan: Plan,
    measure: bool,
    shown: &mut Shown,
    cx: &App,
) -> Option<AnyElement> {
    let (_, tone) = spec.state.as_ref()?;
    let parts = sentence(spec)?;
    let k = theme::colors(cx);
    let (mark, ink) = k.state(*tone);
    let you = *tone == StateTone::You;
    let is_sentence = spec.state_detail.is_some() || spec.state_paused;
    let glyph = if spec.state_paused {
        div()
            .flex_none()
            .child(ui::icon(Sym::Pause, theme::icon::TAG, k.ink_3))
            .into_any_element()
    } else {
        div()
            .flex_none()
            .size(px(6.))
            .rounded_full()
            .bg(mark)
            .into_any_element()
    };
    let name = parts.name.clone().filter(|_| plan.name);
    shown.name = name.clone();
    let step = spec
        .state_detail
        .as_ref()
        .filter(|_| plan.step != Step::Hidden)
        .map(|d| ui::cut_line(format!("· {d}")));
    shown.step = step.as_ref().map(|(_, l)| l.clone());
    let left_out = (parts.name.is_some() && !plan.name)
        || (spec.state_detail.is_some() && plan.step == Step::Hidden);
    let cut: Vec<TextLayout> = shown.step.iter().cloned().collect();
    let full = parts.full.clone();
    let words = match name {
        Some(n) => format!("{n} {}", parts.rest),
        None => parts.rest.clone(),
    };
    let el = div()
        .id("title-state")
        .flex_none()
        .when(plan.step == Step::Cut, |el| el.flex_shrink(1.).min_w_0())
        .flex()
        .items_center()
        .gap(space::S6)
        .h(px(22.))
        .px(space::S8)
        .rounded(theme::radius::SM)
        .when(you, |el| el.bg(k.you_bg))
        .map(|el| {
            if is_sentence && !you {
                el.t_small().text_color(k.ink_2)
            } else {
                el.t_meta_medium().text_color(ink)
            }
        })
        .child(glyph)
        .child(div().flex_none().whitespace_nowrap().child(words))
        .children(step.map(|(styled, _)| {
            div()
                .flex_shrink(1.)
                .min_w_0()
                .when(measure && plan.step == Step::Cut, |el| el.max_w(STEP_MIN))
                .one_line()
                .text_color(k.ink_3)
                .child(styled)
        }))
        .when(is_sentence && !measure, |el| {
            el.tooltip(move |window, cx| {
                if left_out || ui::truncate::any_cut(&cut) {
                    ui::truncate::text_tip(full.clone()).build(window, cx)
                } else {
                    cx.new(|_| gpui::Empty).into()
                }
            })
        })
        .into_any_element();
    Some(el)
}

/// The left part of the bar: `群 /`, the title, the subtitle and the state.
/// `title_min` is the narrowest the title gets while the name still shows;
/// `crumb_max` the widest the breadcrumb gets. `width` is the room it
/// is drawn in (none while it is measured).
fn heading(
    spec: &TitleBarSpec,
    plan: Plan,
    width: Option<Pixels>,
    title_min: Pixels,
    crumb_max: Pixels,
    cx: &App,
) -> (AnyElement, Shown) {
    let measure = width.is_none();
    let k = theme::colors(cx);
    let mut shown = Shown {
        plan: Some(plan),
        ..Shown::default()
    };
    let (title_text, title_layout) = ui::cut_line(spec.title.clone());
    shown.title = Some(title_layout.clone());
    let full_title: SharedString = spec.title.clone().into();
    let title = div()
        .id("title-text")
        .flex_none()
        .when(plan.title_cut, |el| {
            el.flex_shrink(1.)
                .min_w(if plan.name { title_min } else { px(0.) })
        })
        .when(measure && plan.title_cut, |el| el.max_w(title_min))
        .one_line()
        .text_color(k.ink)
        .map(|el| match spec.style {
            TitleStyle::Page => el.t_page_title(),
            TitleStyle::Topic => el.t_bar_title(),
        })
        .child(title_text)
        .when(!measure, |el| {
            el.tooltip(ui::tip_when_cut(vec![title_layout], move || {
                ui::truncate::text_tip(full_title.clone())
            }))
        });
    let crumb = spec.breadcrumb.as_ref().map(|b| {
        div()
            .flex_shrink(1.)
            .min_w(CRUMB_MIN)
            .max_w(if measure && plan.title_cut {
                CRUMB_MIN
            } else {
                crumb_max
            })
            .flex()
            .items_center()
            .gap(space::S6)
            .t_bar_title()
            .font_weight(gpui::FontWeight::NORMAL)
            .text_color(k.ink_3)
            .child(div().min_w_0().one_line().child(b.clone()))
            .child(div().flex_none().child("/"))
    });
    let state = state(spec, plan, measure, &mut shown, cx);
    let el = div()
        .when_some(width, |el, w| el.w(w))
        .flex()
        .items_center()
        .gap(space::S8)
        .children(crumb)
        .child(title)
        .children(spec.subtitle.as_ref().map(|s| {
            div()
                .flex_none()
                .one_line()
                .t_small()
                .text_color(k.ink_3)
                .child(s.clone())
        }))
        .children(state)
        .into_any_element();
    (el, shown)
}

/// Lays out the left part of the bar in the room the buttons leave: it
/// tries each [`Plan`] in [`Plan::ORDER`] at its narrowest and draws the
/// first that fits, so a part is shown well or not at all.
pub struct Heading {
    spec: TitleBarSpec,
    shown: Rc<RefCell<Shown>>,
}

impl Heading {
    pub fn new(spec: TitleBarSpec) -> Self {
        Self {
            spec,
            shown: Rc::default(),
        }
    }

    /// What the last frame showed.
    pub fn shown(&self) -> Rc<RefCell<Shown>> {
        self.shown.clone()
    }

    /// The plans that differ for this spec.
    fn plans(&self) -> impl Iterator<Item = Plan> + '_ {
        let has_step = self.spec.state_detail.is_some();
        let has_name = sentence(&self.spec).is_some_and(|s| s.name.is_some());
        Plan::ORDER
            .into_iter()
            .filter(move |p| has_step || p.step == Step::Hidden)
            .filter(move |p| has_name || p.name)
    }
}

impl IntoElement for Heading {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Heading {
    type RequestLayoutState = ();
    type PrepaintState = Option<AnyElement>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style {
            flex_grow: 1.,
            flex_shrink: 1.,
            flex_basis: px(0.).into(),
            ..Style::default()
        };
        style.min_size.width = px(0.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let room = bounds.size.width;
        let free = size(AvailableSpace::MaxContent, AvailableSpace::MaxContent);
        let crumb_max = (room * 0.25).max(CRUMB_MIN);
        // The title alone, to keep a short title whole while the name shows.
        let full_title = Plan::ORDER[2];
        let mut title_only = TitleBarSpec {
            state: None,
            breadcrumb: None,
            subtitle: None,
            ..self.spec.clone()
        };
        title_only.buttons.clear();
        let title_w = heading(&title_only, full_title, None, TITLE_MIN, crumb_max, cx)
            .0
            .layout_as_root(free, window, cx)
            .width;
        let title_min = title_w.min(TITLE_MIN);
        let plans: Vec<Plan> = self.plans().collect();
        let last = plans.len() - 1;
        let plan = plans
            .iter()
            .enumerate()
            .find(|(i, p)| {
                *i == last
                    || heading(&self.spec, **p, None, title_min, crumb_max, cx)
                        .0
                        .layout_as_root(free, window, cx)
                        .width
                        <= room
            })
            .map(|(_, p)| *p)
            .unwrap_or(plans[last]);
        let (mut el, shown) = heading(&self.spec, plan, Some(room), title_min, crumb_max, cx);
        *self.shown.borrow_mut() = shown;
        let space = size(
            AvailableSpace::Definite(room),
            AvailableSpace::Definite(bounds.size.height),
        );
        let h = el.layout_as_root(space, window, cx).height;
        let origin = point(
            bounds.origin.x,
            bounds.origin.y + (bounds.size.height - h) / 2.,
        );
        el.prepaint_as_root(origin, space, window, cx);
        Some(el)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        el: &mut Option<AnyElement>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(el) = el {
            el.paint(window, cx);
        }
    }
}

/// Draws a title bar. `open` is the pane that shows now; `leading` is the
/// inset at the start (the traffic lights when the rail is hidden).
pub fn render(
    spec: &TitleBarSpec,
    open: &RightPane,
    leading: Pixels,
    services: &Services,
    cx: &App,
) -> AnyElement {
    let k = theme::colors(cx);
    let buttons = spec.buttons.iter().map(|b| {
        let active = open.same_kind(&b.pane) && *open != RightPane::None;
        let pane = b.pane.clone();
        let nav = services.nav.clone();
        div()
            .id(SharedString::from(format!("title-{}", b.kind.slug())))
            .size(theme::TITLE_BUTTON)
            .rounded(px(7.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .when(active, |el| el.bg(k.sel))
            .when(!active, |el| el.hover(|s| s.bg(k.hover)))
            .child(ui::sym(
                b.kind.icon(),
                ui::SymSize::TOOLBAR,
                if active { k.ink } else { k.ink_2 },
            ))
            .tooltip({
                let name = b.kind.name();
                move |window, cx| gpui_component::tooltip::Tooltip::new(name).build(window, cx)
            })
            .on_click(move |_, _, cx| {
                let pane = pane.clone();
                nav.update(cx, |nav, cx| nav.toggle_pane(pane, cx));
            })
    });
    div()
        .flex_none()
        .h(theme::TITLE_BAR)
        .w_full()
        .pl(leading.max(space::S24))
        .pr(space::S12)
        .flex()
        .items_center()
        .gap(space::S8)
        .child(Heading::new(spec.clone()))
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(space::S4)
                .children(buttons),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use gpui::{Context, Render, TestAppContext, Window, size};
    use unicode_segmentation::UnicodeSegmentation as _;

    use super::*;

    const PRIYA: &str = "👩🏽‍💻 Priya";

    fn live(name: &str) -> TitleBarSpec {
        TitleBarSpec {
            breadcrumb: Some("开发".into()),
            title: "话题列表虚拟滚动和消息分页的重写".into(),
            style: TitleStyle::Topic,
            state: Some((format!("{name} 在做 3/5"), StateTone::Live)),
            state_detail: Some("跑一万条消息的压测，再看内存和滚动锚点".into()),
            state_name: Some(name.into()),
            ..TitleBarSpec::default()
        }
    }

    /// The left part of a bar `width` wide.
    struct Bar {
        width: f32,
        spec: TitleBarSpec,
        shown: Option<Rc<RefCell<Shown>>>,
    }

    impl Render for Bar {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let heading = Heading::new(self.spec.clone());
            self.shown = Some(heading.shown());
            div()
                .w(px(self.width))
                .h(theme::TITLE_BAR)
                .flex()
                .items_center()
                .child(heading)
        }
    }

    /// (plan, title cut, step cut, name drawn) for each width.
    fn sweep(
        cx: &mut TestAppContext,
        spec: TitleBarSpec,
        widths: impl Iterator<Item = f32>,
    ) -> Vec<(f32, Plan, bool, bool, Option<String>)> {
        cx.update(gpui_component::init);
        let window = cx.open_window(size(px(1600.), px(60.)), |_, _| Bar {
            width: 1500.,
            spec,
            shown: None,
        });
        let mut out = Vec::new();
        for w in widths {
            window
                .update(cx, |bar, _, cx| {
                    bar.width = w;
                    cx.notify();
                })
                .unwrap();
            cx.run_until_parked();
            let row = window
                .update(cx, |bar, _, _| {
                    let s = bar.shown.as_ref().expect("drawn").borrow().clone();
                    (
                        w,
                        s.plan.expect("a plan"),
                        s.title.as_ref().is_some_and(TextLayout::truncated),
                        s.step.as_ref().is_some_and(TextLayout::truncated),
                        s.name.clone(),
                    )
                })
                .unwrap();
            out.push(row);
        }
        out
    }

    fn rank(p: Plan) -> usize {
        Plan::ORDER
            .iter()
            .position(|x| *x == p)
            .expect("a known plan")
    }

    #[gpui::test]
    fn the_title_wins_over_the_step_and_the_name_goes_last(cx: &mut TestAppContext) {
        let rows = sweep(cx, live("Forge"), (0..=275).map(|i| 1200. - i as f32 * 4.));
        // Wide: all. Medium: no step, the whole title. Narrow: the title
        // is cut. Very narrow: no name. Each shows at some width.
        for want in Plan::ORDER {
            assert!(
                rows.iter().any(|r| r.1 == want),
                "{want:?} never shows: {:?}",
                rows.iter().map(|r| (r.0, r.1)).collect::<Vec<_>>()
            );
        }
        let wide = &rows[0];
        assert_eq!(wide.1.step, Step::Whole);
        assert!(!wide.2 && !wide.3, "a wide bar cuts nothing");
        let mut last = 0;
        for (w, plan, title_cut, step_cut, name) in &rows {
            assert!(rank(*plan) >= last, "a narrower bar shows more at {w}");
            last = rank(*plan);
            if plan.step != Step::Hidden {
                assert!(!title_cut, "the step shows while the title is cut at {w}");
            }
            if *step_cut {
                assert_eq!(plan.step, Step::Cut, "the whole step is cut at {w}");
            }
            if !plan.title_cut {
                assert!(!title_cut, "the title is cut before its turn at {w}");
            }
            assert_eq!(name.is_some(), plan.name, "at {w}");
            if let Some(n) = name {
                assert_eq!(n, "Forge", "the name shows whole at {w}");
            }
        }
    }

    #[gpui::test]
    fn a_zwj_name_shows_or_goes_whole(cx: &mut TestAppContext) {
        let rows = sweep(cx, live(PRIYA), (0..=225).map(|i| 1000. - i as f32 * 4.));
        assert!(rows.iter().any(|r| r.4.is_some()) && rows.iter().any(|r| r.4.is_none()));
        for (w, _, _, _, name) in &rows {
            if let Some(n) = name {
                assert_eq!(n, PRIYA, "the name is split at {w}");
            }
        }
    }

    #[test]
    fn a_long_name_is_cut_between_graphemes() {
        assert_eq!(short_name(PRIYA), PRIYA);
        assert_eq!(short_name("Forge"), "Forge");
        let long = format!("{PRIYA} 👩🏽‍💻👩🏽‍💻👩🏽‍💻👩🏽‍💻👩🏽‍💻👩🏽‍💻👩🏽‍💻👩🏽‍💻 Kowalczyk-Montgomery");
        let cut = short_name(&long);
        let kept = cut.strip_suffix('…').expect("cut with …");
        let whole: Vec<&str> = long.graphemes(true).collect();
        let part: Vec<&str> = kept.graphemes(true).collect();
        assert!(!part.is_empty() && part.len() < whole.len());
        assert_eq!(&whole[..part.len()], part.as_slice(), "an emoji is split");
        assert!(
            part.iter().all(|g| whole.contains(g)),
            "a grapheme was made that is not in the name"
        );
    }
}
