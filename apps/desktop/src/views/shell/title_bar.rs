//! The 52 pt title bar of every page (CONCEPT2 §2): breadcrumb `群 /` in
//! `ink-3`, the title (17/600 on 首页, 产出, 成员 and 群; 15/600 on 话题),
//! the state sentence (amber only when it waits on me), and at the right a
//! group of 28 pt icon buttons: 单独问 (⌘;), 产出, 电脑, 改动 and the rail
//! toggle. The button of the open pane has the grey `sel` fill.

use gpui::{
    AnyElement, App, InteractiveElement as _, IntoElement, ParentElement, Pixels, SharedString,
    StatefulInteractiveElement as _, Styled, div, prelude::FluentBuilder as _, px,
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
    let title = div()
        .min_w_0()
        .one_line()
        .text_color(k.ink)
        .map(|el| match spec.style {
            TitleStyle::Page => el.t_page_title(),
            TitleStyle::Topic => el.t_bar_title(),
        })
        .child(spec.title.clone());
    let state = spec.state.as_ref().map(|(text, tone)| {
        let (mark, ink) = k.state(*tone);
        let you = *tone == StateTone::You;
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(space::S6)
            .h(px(22.))
            .px(space::S8)
            .rounded(theme::radius::SM)
            .when(you, |el| el.bg(k.you_bg))
            .t_meta_medium()
            .text_color(ink)
            .child(div().size(px(6.)).rounded_full().bg(mark))
            .child(text.clone())
    });
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
        // A long 群 name gives way first: it shrinks to an ellipsis (at
        // most 40 % of the bar) while the 话题 title keeps its room.
        .children(spec.breadcrumb.as_ref().map(|b| {
            div()
                .flex_shrink(1.)
                .min_w(px(48.))
                .max_w(gpui::relative(0.4))
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
