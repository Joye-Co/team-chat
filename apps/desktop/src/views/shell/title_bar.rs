//! The 52 pt title bar of every page (CONCEPT2 §2): breadcrumb `群 /` in
//! `ink-3`, the title (17/600 on 首页, 产出, 成员 and 群; 15/600 on 话题),
//! the state sentence (amber only when it waits on me), and at the right a
//! group of 28 pt icon buttons: 单独问 (⌘;), 产出, 电脑, 改动 and the rail
//! toggle. The button of the open pane has the grey `sel` fill.

use gpui::{
    AnyElement, App, InteractiveElement as _, IntoElement, ParentElement, Pixels, SharedString,
    StatefulInteractiveElement as _, Styled, TextLayout, div, prelude::FluentBuilder as _, px,
};

use crate::{
    app::{RightPane, Services},
    copy::topic_rail as c,
    theme::{self, StateTone, Type as _, space},
    ui::{self, Lucide},
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
    fn icon(self) -> Lucide {
        match self {
            TitleButtonKind::SideChat => Lucide::MessageCircle,
            TitleButtonKind::Outputs => Lucide::Layers,
            TitleButtonKind::Machine => Lucide::Monitor,
            TitleButtonKind::Changes => Lucide::GitPullRequest,
            TitleButtonKind::Rail => Lucide::PanelRight,
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

/// The text layouts of the state sentence, to tell what was cut.
pub struct StateLines {
    pub name: Option<TextLayout>,
    pub rest: TextLayout,
    pub detail: Option<TextLayout>,
}

/// The state of the bar: a short state stays whole; a member's run reads as
/// a sentence (Live.dc.html), `ink-2` text, the step in `ink-3`, a live dot
/// or, while paused, a pause mark. In a narrow bar the step gives way first,
/// then the member's name, each cut with `…`; `在做 3/5` stays whole, and a
/// cut sentence shows in full in a tooltip (§15).
pub fn state(spec: &TitleBarSpec, cx: &App) -> Option<(AnyElement, StateLines)> {
    let (text, tone) = spec.state.as_ref()?;
    let k = theme::colors(cx);
    let (mark, ink) = k.state(*tone);
    let you = *tone == StateTone::You;
    let sentence = spec.state_detail.is_some() || spec.state_paused;
    let glyph = if spec.state_paused {
        div()
            .flex_none()
            .child(ui::icon(Lucide::Pause, theme::icon::TAG, k.ink_3))
            .into_any_element()
    } else {
        div()
            .flex_none()
            .size(px(6.))
            .rounded_full()
            .bg(mark)
            .into_any_element()
    };
    let (name, rest) = match spec.state_name.as_deref() {
        Some(n) if sentence && !n.is_empty() && text.starts_with(n) => {
            (Some(n.to_string()), text[n.len()..].to_string())
        }
        _ => (None, text.clone()),
    };
    let name = name.map(ui::cut_line);
    let rest_text_str = rest.clone();
    let (rest_text, rest_layout) = ui::cut_line(rest);
    let detail = spec
        .state_detail
        .as_ref()
        .map(|d| ui::cut_line(format!("· {d}")));
    let lines = StateLines {
        name: name.as_ref().map(|(_, l)| l.clone()),
        rest: rest_layout,
        detail: detail.as_ref().map(|(_, l)| l.clone()),
    };
    let full: SharedString = match &spec.state_detail {
        Some(d) => format!("{text} · {d}").into(),
        None => text.clone().into(),
    };
    let cut: Vec<TextLayout> = lines
        .name
        .iter()
        .chain(lines.detail.iter())
        .cloned()
        .collect();
    let el = div()
        .id("title-state")
        // A short state keeps its width; a sentence gives way before the
        // 话题 title does, down to the dot, a cut name and `在做 3/5`.
        .when(!sentence, |el| el.flex_none())
        .when(sentence, |el| {
            el.flex_shrink(16.)
                .min_w(sentence_min(name.is_some(), &rest_text_str))
        })
        .flex()
        .items_center()
        .gap(space::S6)
        .h(px(22.))
        .px(space::S8)
        .rounded(theme::radius::SM)
        .when(you, |el| el.bg(k.you_bg))
        .map(|el| {
            if sentence && !you {
                el.t_small().text_color(k.ink_2)
            } else {
                el.t_meta_medium().text_color(ink)
            }
        })
        .child(glyph)
        .child(
            div()
                .flex()
                .items_center()
                .flex_shrink(1.)
                .min_w_0()
                .children(name.map(|(styled, _)| {
                    div()
                        .flex_shrink(1.)
                        .min_w(NAME_MIN)
                        .one_line()
                        .child(styled)
                }))
                .child(div().flex_none().whitespace_nowrap().child(rest_text)),
        )
        .children(detail.map(|(styled, _)| {
            div()
                .flex_shrink(1000.)
                .min_w_0()
                .one_line()
                .text_color(k.ink_3)
                .child(styled)
        }))
        .when(sentence, |el| {
            el.tooltip(ui::tip_when_cut(cut, move || {
                ui::truncate::text_tip(full.clone())
            }))
        })
        .into_any_element();
    Some((el, lines))
}

/// The narrowest a cut member name gets in the state: a glyph and `…`.
const NAME_MIN: Pixels = px(28.);

/// The narrowest a state sentence gets: padding, the mark, a cut name and
/// the whole `在做 3/5` (the step may go). The bar has no window to shape
/// text with, so the width of `rest` is a generous estimate at the 13 pt
/// type: a full em for CJK, 0.62 em for the rest. Without this floor the
/// state's own text (its step) holds it wide and pushes the buttons out,
/// or it shrinks to nothing and `在做 3/5` runs into them.
fn sentence_min(named: bool, rest: &str) -> Pixels {
    let em = theme::type_scale::SMALL.0;
    let rest_w: f32 = rest
        .chars()
        .map(|c| if c.is_ascii() { 0.62 } else { 1. })
        .sum();
    let name = if named { NAME_MIN } else { px(0.) };
    space::S8 * 2. + px(6.) + space::S6 * 2. + name + em * rest_w
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
    // The title keeps its width (up to 35 % of the bar); the state's step,
    // the member's name and the 群 name give way first, the title last.
    let title = div()
        .flex_shrink(1.)
        .max_w(gpui::relative(0.35))
        .min_w_0()
        .one_line()
        .text_color(k.ink)
        .map(|el| match spec.style {
            TitleStyle::Page => el.t_page_title(),
            TitleStyle::Topic => el.t_bar_title(),
        })
        .child(spec.title.clone());
    let state = state(spec, cx).map(|(el, _)| el);
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
            .child(ui::icon(
                b.kind.icon(),
                theme::icon::ROW,
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
        // A long 群 name gives way: it shrinks to an ellipsis (at most 25 %
        // of the bar) while the 话题 title keeps its room.
        .children(spec.breadcrumb.as_ref().map(|b| {
            div()
                .flex_shrink(1.)
                .min_w(px(48.))
                .max_w(gpui::relative(0.25))
                .flex()
                .items_center()
                .gap(space::S6)
                .t_bar_title()
                .font_weight(gpui::FontWeight::NORMAL)
                .text_color(k.ink_3)
                .child(div().min_w_0().one_line().child(b.clone()))
                .child(div().flex_none().child("/"))
        }))
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
        .child(div().flex_1())
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

    use super::*;

    const LONG: &str = "Aleksandra Wiśniewska-Kowalczyk-Montgomery";

    /// The state of a bar `width` wide, after a 200 pt title that may shrink.
    struct Bar {
        width: f32,
        lines: Option<StateLines>,
    }

    impl Render for Bar {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let spec = TitleBarSpec {
                state: Some((format!("{LONG} 在做 3/5"), StateTone::Live)),
                state_detail: Some("跑一万条消息的压测，再看内存和滚动锚点".into()),
                state_name: Some(LONG.into()),
                ..TitleBarSpec::default()
            };
            let (el, lines) = state(&spec, cx).expect("a state");
            self.lines = Some(lines);
            div()
                .w(px(self.width))
                .flex()
                .child(div().flex_shrink(1.).min_w(px(40.)).w(px(200.)))
                .child(el)
                .child(div().flex_1())
        }
    }

    /// (name cut, rest cut, step cut) in a bar `width` wide.
    fn cuts(cx: &mut TestAppContext, width: f32) -> (bool, bool, bool) {
        cx.update(gpui_component::init);
        let window = cx.open_window(size(px(1600.), px(60.)), |_, _| Bar { width, lines: None });
        cx.run_until_parked();
        window
            .update(cx, |bar, _, _| {
                let l = bar.lines.as_ref().expect("drawn");
                (
                    l.name.as_ref().is_some_and(TextLayout::truncated),
                    l.rest.truncated(),
                    l.detail.as_ref().is_some_and(TextLayout::truncated),
                )
            })
            .unwrap()
    }

    #[gpui::test]
    fn a_wide_bar_shows_the_whole_state(cx: &mut TestAppContext) {
        assert_eq!(cuts(cx, 1500.), (false, false, false));
    }

    #[gpui::test]
    fn a_narrow_bar_cuts_the_step_and_the_name_never_the_progress(cx: &mut TestAppContext) {
        let (name, rest, step) = cuts(cx, 420.);
        assert!(step, "the step gives way");
        assert!(name, "a long name is cut with …");
        assert!(!rest, "在做 3/5 stays whole");
    }

    #[test]
    fn the_sentence_keeps_room_for_its_progress() {
        let progress = sentence_min(true, " 在做 3/5");
        assert!(progress > NAME_MIN + theme::type_scale::SMALL.0 * 5.);
        assert!(sentence_min(false, " 在做 3/5") < progress);
    }
}
