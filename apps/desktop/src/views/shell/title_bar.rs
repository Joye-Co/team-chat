//! The toolbar row of every page (NATIVE.md §2, §5.1, §9.2), drawn by the
//! shell from the page's [`TitleBarSpec`]: no breadcrumb and no pill.
//!
//! - Leading, 20 pt after the sidebar: the title (Title 3, 15 pt semibold)
//!   over the subtitle (Subheadline, 11 pt `secondary`). A 话题's 群 folds
//!   into the subtitle (`开发 · Forge 在做 3/5 · 跑压测`); its state is an SF
//!   Symbol plus the state words in the state color, never a capsule.
//! - Trailing, 12 pt from the edge: the pane buttons as glass capsule
//!   groups (`ToolbarGroup`, 36 pt items), at most three groups. The item of
//!   the open pane shows the toggled pill. Every item is also a 显示 command.
//! - The row is transparent over the content and has no hairline; dragging
//!   it moves the window and a double click zooms it. An inactive window
//!   draws the title and the symbols in `secondary`.

use gpui::{
    AnyElement, App, Hsla, InteractiveElement as _, IntoElement, MouseButton, ParentElement,
    Pixels, SharedString, StatefulInteractiveElement as _, Styled, StyledText, TextLayout, Window,
    div, px,
};

use crate::{
    app::{RightPane, Services},
    copy::topic_rail as c,
    ui::{
        self, Sym,
        native::{
            BadgeState, NativeColors, NativeType as _, StatusBadge, ToolbarGroup, ToolbarItemKind,
            metrics, tokens::space, toolbar_item,
        },
        sf_symbols::Sf,
    },
};

/// The title style of a page. Both draw Title 3 in the toolbar now; a 话题
/// also folds its 群 into the subtitle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TitleStyle {
    /// 首页, 产出, 成员, 群.
    #[default]
    Page,
    /// A 话题: the 群 leads the subtitle.
    Topic,
}

/// Which toolbar item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TitleButtonKind {
    SideChat,
    Outputs,
    /// Only when a member's computer is active.
    Machine,
    Changes,
    /// The inspector toggle (`sidebar.trailing`).
    Rail,
    /// 1.1.1, the 群聊: opens the 新话题 sheet for this 群 (no pane).
    NewTopic,
    /// 1.1.1, the 群聊: opens the 群 page with its 话题 lists (no pane).
    GroupTopics,
}

impl TitleButtonKind {
    /// The SF Symbol of the item (NATIVE.md §6).
    fn symbol(self) -> Sf {
        match self {
            TitleButtonKind::SideChat => Sf::SideChat,
            TitleButtonKind::Outputs => Sf::Outputs,
            TitleButtonKind::Machine => Sf::Computer,
            TitleButtonKind::Changes => Sf::Changes,
            TitleButtonKind::Rail => Sf::PaneRight,
            TitleButtonKind::NewTopic => Sf::NewTopic,
            TitleButtonKind::GroupTopics => Sf::Group,
        }
    }

    fn name(self) -> &'static str {
        match self {
            TitleButtonKind::SideChat => c::TITLE_SIDE_CHAT,
            TitleButtonKind::Outputs => c::TITLE_OUTPUTS,
            TitleButtonKind::Machine => c::TITLE_MACHINE,
            TitleButtonKind::Changes => c::TITLE_CHANGES,
            TitleButtonKind::Rail => c::TITLE_TOGGLE_RAIL,
            TitleButtonKind::NewTopic => c::TITLE_NEW_TOPIC,
            TitleButtonKind::GroupTopics => c::TITLE_GROUP_TOPICS,
        }
    }

    fn slug(self) -> &'static str {
        match self {
            TitleButtonKind::SideChat => "side-chat",
            TitleButtonKind::Outputs => "outputs",
            TitleButtonKind::Machine => "machine",
            TitleButtonKind::Changes => "changes",
            TitleButtonKind::Rail => "rail",
            TitleButtonKind::NewTopic => "new-topic",
            TitleButtonKind::GroupTopics => "group-topics",
        }
    }

    /// The capsule group of the item: the panes of the 话题 share one, the
    /// inspector toggle has its own at the trailing edge.
    fn group(self) -> usize {
        match self {
            TitleButtonKind::Rail => 1,
            _ => 0,
        }
    }
}

/// A toolbar item and the pane it toggles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TitleButton {
    pub kind: TitleButtonKind,
    pub pane: RightPane,
}

impl TitleButton {
    pub fn of(kind: TitleButtonKind, pane: RightPane) -> Self {
        Self { kind, pane }
    }
}

/// What a page's toolbar row shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TitleBarSpec {
    /// The 群 of a 话题: it leads the subtitle (`开发 · …`).
    pub breadcrumb: Option<String>,
    pub title: String,
    pub style: TitleStyle,
    /// A quiet line (首页: the date; 群: the purpose): the subtitle, or its
    /// end after the state.
    pub subtitle: Option<String>,
    /// The state words and their §2.12 state (`topic_page::topic_state`).
    pub state: Option<(String, BadgeState)>,
    /// A quieter tail after the state (`跑一万条消息的压测`).
    pub state_detail: Option<String>,
    /// The member's name that leads the state words.
    pub state_name: Option<String>,
    /// The peer of a 私聊: its 16 avatar leads the subtitle's name (§2.2,
    /// R2-13).
    pub peer: Option<Peer>,
    pub buttons: Vec<TitleButton>,
}

/// A 私聊 peer in the title bar.
#[derive(Clone, Debug, PartialEq)]
pub struct Peer(pub crate::data::Member);

impl Eq for Peer {}

impl TitleBarSpec {
    pub fn page(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }
}

/// The parts of the subtitle line: one text run, the range of the state
/// words with their color, and the state symbol before the line.
#[derive(Clone, Debug, PartialEq)]
pub struct Subtitle {
    /// The muted text: breadcrumb, detail, subtitle (`·` between).
    pub text: String,
    /// The state words and their §2.12 state: drawn as a StatusBadge before
    /// the text (§2.2), never as colored text.
    pub state: Option<(String, BadgeState)>,
    /// A muted symbol before the text (the dropped connection).
    pub symbol: Option<(Sym, Hsla)>,
}

/// The separator between the parts of a subtitle.
const DOT: &str = " · ";

/// The state color of a tone (§4): amber, blue, red, green; quiet words
/// stay `secondary`.
pub fn subtitle(spec: &TitleBarSpec, _k: &NativeColors) -> Option<Subtitle> {
    let mut text = String::new();
    let push = |part: &str, text: &mut String| {
        if !text.is_empty() {
            text.push_str(DOT);
        }
        text.push_str(part);
    };
    if let Some(crumb) = spec.breadcrumb.as_deref().filter(|c| !c.is_empty()) {
        push(crumb, &mut text);
    }
    let state = spec
        .state
        .as_ref()
        .filter(|(w, _)| !w.is_empty())
        .map(|(words, state)| (words.clone(), *state));
    if let Some(detail) = spec.state_detail.as_deref().filter(|d| !d.is_empty()) {
        push(detail, &mut text);
    }
    if let Some(sub) = spec.subtitle.as_deref().filter(|s| !s.is_empty()) {
        push(sub, &mut text);
    }
    (!text.is_empty() || state.is_some()).then_some(Subtitle {
        text,
        state,
        symbol: None,
    })
}

pub fn groups(spec: &TitleBarSpec) -> Vec<Vec<TitleButtonKind>> {
    let mut out: Vec<Vec<TitleButtonKind>> = vec![Vec::new(), Vec::new()];
    for b in &spec.buttons {
        out[b.kind.group()].push(b.kind);
    }
    out.retain(|g| !g.is_empty());
    out.truncate(3);
    out
}

/// The title over the subtitle. `title` and `subtitle` receive the text
/// layouts of the last frame (the title's tooltip and the tests read them).
fn heading(
    spec: &TitleBarSpec,
    active: bool,
    window: &mut Window,
    cx: &App,
) -> (AnyElement, TextLayout, Option<TextLayout>) {
    heading_with(spec, None, active, window, cx)
}

// --- v12/fx-shell: the connection state in the subtitle ---
/// The subtitle while the connection is not up (coordinator decision: no
/// row at the sidebar bottom): `wifi.slash` and `连接已断开 · 8 秒后重连`, in
/// `secondary`. `None` while online. 立即重连 is a 显示 menu command.
pub fn connection_subtitle(
    state: crate::data::ConnectionState,
    k: &NativeColors,
) -> Option<Subtitle> {
    use crate::{copy::connection as cc, data::ConnectionState as S};
    let (text, symbol) = match state {
        S::Online => return None,
        // Starting up is not a problem: no symbol.
        S::Connecting => (cc::CONNECTING.to_string(), None),
        S::Reconnecting { retry_in_ms, .. } if retry_in_ms >= 1000 => {
            let s = retry_in_ms.div_ceil(1000).to_string();
            (
                format!(
                    "{}{DOT}{}",
                    cc::BANNER_DISCONNECTED,
                    crate::copy::fill(cc::BANNER_RETRY_IN, &[("s", &s)])
                ),
                Some(Sym::Offline),
            )
        }
        S::Reconnecting { .. } => (cc::RECONNECTING_NOW.to_string(), Some(Sym::Offline)),
        S::Offline => (cc::OFFLINE.to_string(), Some(Sym::Offline)),
        S::TokenUnreadable => (cc::TOKEN_UNREADABLE.to_string(), Some(Sym::Offline)),
    };
    Some(Subtitle {
        text,
        state: None,
        symbol: symbol.map(|s| (s, k.secondary)),
    })
}
// --- end v12/fx-shell ---

/// The widest a state badge in the subtitle gets before it truncates.
const BADGE_MAX: Pixels = px(240.);

/// [`heading`] with a subtitle that replaces the spec's (the connection
/// state).
fn heading_with(
    spec: &TitleBarSpec,
    over: Option<Subtitle>,
    active: bool,
    window: &mut Window,
    cx: &App,
) -> (AnyElement, TextLayout, Option<TextLayout>) {
    let k = crate::views::shell::chrome::colors(cx);
    let (title_text, title_layout) = ui::cut_line(spec.title.clone());
    let full_title: SharedString = spec.title.clone().into();
    let title = div()
        .id("toolbar-title")
        .min_w(px(0.))
        .n_title3()
        .text_color(if active { k.label } else { k.secondary })
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(title_text)
        .tooltip(ui::tip_when_cut(vec![title_layout.clone()], move || {
            ui::truncate::text_tip(full_title.clone())
        }));
    // The connection line replaces the subtitle, and the peer with it.
    let peer = over.is_none().then(|| spec.peer.clone()).flatten();
    let sub = over.or_else(|| subtitle(spec, &k)).map(|s| {
        let full: SharedString = s.text.clone().into();
        let styled = StyledText::new(s.text.clone());
        let layout = styled.layout().clone();
        let line = div()
            .id("toolbar-subtitle")
            .min_w(px(0.))
            .flex()
            .items_center()
            .gap(space::S6)
            .children(s.symbol.map(|(sym, color)| {
                div()
                    .flex_none()
                    .child(ui::sym(sym, ui::SymSize::INLINE_SMALL, color))
            }))
            .children(s.state.clone().map(|(words, state)| {
                div()
                    .flex_none()
                    .child(StatusBadge::new(state, words).max_w(BADGE_MAX))
            }))
            .children(peer.clone().map(|Peer(m)| {
                div().flex_none().child(ui::MemberAvatar::new(
                    Some(&m),
                    crate::ui::native::tokens::avatar::XS,
                ))
            }))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .n_subheadline()
                    .text_color(k.secondary)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(styled),
            )
            .tooltip(ui::tip_when_cut(vec![layout.clone()], move || {
                ui::truncate::text_tip(full.clone())
            }));
        (line, layout)
    });
    let _ = window;
    let (sub_el, sub_layout) = match sub {
        Some((el, layout)) => (Some(el), Some(layout)),
        None => (None, None),
    };
    // One line on the traffic-light line: the title, then the subtitle
    // after it, never under it (owner decision, v18). The title keeps up to
    // 70 % of the row; the subtitle is cut first.
    let el = div()
        .flex_1()
        .min_w(px(0.))
        .flex()
        .items_center()
        .gap(space::S12)
        .child(
            div()
                .flex_shrink_0()
                .max_w(gpui::relative(0.7))
                .child(title),
        )
        .children(sub_el.map(|s| div().flex_1().min_w(px(0.)).child(s)))
        .into_any_element();
    (el, title_layout, sub_layout)
}

/// Draws a toolbar row. `open` is the pane that shows now; `leading` is
/// the inset at the start (20 pt after the sidebar, or past the traffic
/// lights when the sidebar is hidden); `active` is false in an inactive
/// window.
pub fn render(
    spec: &TitleBarSpec,
    open: &RightPane,
    leading: Pixels,
    active: bool,
    services: &Services,
    window: &mut Window,
    cx: &App,
) -> AnyElement {
    let k = crate::views::shell::chrome::colors(cx);
    let signed_in = matches!(
        services.provider.session(),
        crate::data::SessionState::SignedIn { .. }
    );
    // §2.2, §2.15: a 话题 keeps its state badge offline; the Alert docked
    // above its composer says the connection is down, the subtitle does not
    // repeat it. Pages without a state show the connection in the subtitle.
    let over = (signed_in && spec.state.is_none())
        .then(|| connection_subtitle(services.provider.connection(), &k))
        .flatten();
    let (heading, _, _) = heading_with(spec, over, active, window, cx);
    let mut row = div()
        .id("toolbar-row")
        .flex_none()
        .w_full()
        .h(metrics::TOOLBAR_ROW)
        // §2.2: one header band h52, a 1 px border-b; content scrolls under
        // it with no fade.
        .bg(k.content)
        .border_b_1()
        .border_color(k.separator)
        .pl(leading)
        .pr(metrics::TOOLBAR_TRAILING)
        .flex()
        .items_center()
        .gap(metrics::TOOLBAR_GROUP_GAP)
        .on_mouse_down(MouseButton::Left, |event, window, _| {
            if event.click_count >= 2 {
                window.titlebar_double_click();
            } else {
                window.start_window_move();
            }
        })
        .child(heading);
    for (gix, group) in groups(spec).into_iter().enumerate() {
        // §2.2: a vertical separator h16, mx 8, before the right-pane toggle.
        if gix > 0 {
            row = row.child(
                div()
                    .flex_none()
                    .mx(space::S8)
                    .w(px(1.))
                    .h(ui::native::tokens::icon::MD)
                    .bg(k.separator),
            );
        }
        let items: Vec<gpui_component::button::Button> = group
            .into_iter()
            .filter_map(|kind| spec.buttons.iter().find(|b| b.kind == kind))
            .map(|b| {
                let shown = open.same_kind(&b.pane) && *open != RightPane::None;
                let pane = b.pane.clone();
                let kind = b.kind;
                let nav = services.nav.clone();
                toolbar_item(
                    gpui::SharedString::from(format!("toolbar-{}", b.kind.slug())),
                    b.kind.symbol(),
                    b.kind.name(),
                    if shown {
                        ToolbarItemKind::Toggled
                    } else {
                        ToolbarItemKind::Plain
                    },
                    active,
                    window,
                    cx,
                )
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    match kind {
                        TitleButtonKind::NewTopic => {
                            window.dispatch_action(Box::new(crate::app::NewTopic), cx)
                        }
                        TitleButtonKind::GroupTopics => nav.update(cx, |nav, cx| {
                            if let Some(g) = nav.current_group() {
                                nav.open_group_page(g, cx);
                            }
                        }),
                        _ => {
                            let pane = pane.clone();
                            nav.update(cx, |nav, cx| nav.toggle_pane(pane, cx));
                        }
                    }
                })
            })
            .collect();
        row = row.child(
            div()
                .flex_none()
                // A press on an item is not a window drag.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(ToolbarGroup::new(items)),
        );
    }
    row.into_any_element()
}

#[cfg(test)]
mod tests {
    use gpui::{Context, Render, TestAppContext, Window, size};

    use super::*;

    fn live() -> TitleBarSpec {
        TitleBarSpec {
            breadcrumb: Some("开发".into()),
            title: "话题列表虚拟滚动和消息分页的重写".into(),
            style: TitleStyle::Topic,
            state: Some(("Forge 在做 3/5".into(), BadgeState::Working)),
            state_detail: Some("跑一万条消息的压测".into()),
            state_name: Some("Forge".into()),
            ..TitleBarSpec::default()
        }
    }

    #[test]
    fn the_group_leads_the_subtitle_and_the_state_is_a_status_badge() {
        let k = NativeColors::dark();
        let s = subtitle(&live(), &k).expect("a subtitle");
        // §2.2: the state is a StatusBadge, not colored text in the line.
        assert_eq!(s.text, "开发 · 跑一万条消息的压测");
        assert_eq!(
            s.state,
            Some(("Forge 在做 3/5".to_string(), BadgeState::Working))
        );
        // Needs you: the warning state.
        let you = TitleBarSpec {
            state: Some(("等你审 PR #61".into(), BadgeState::NeedsYou)),
            state_detail: None,
            ..live()
        };
        let s = subtitle(&you, &k).unwrap();
        assert_eq!(s.text, "开发");
        assert_eq!(s.state.unwrap().1, BadgeState::NeedsYou);
        // Waiting on someone else: the queued clock.
        let quiet = TitleBarSpec {
            state: Some(("等李雪回答".into(), BadgeState::Queued)),
            ..live()
        };
        assert_eq!(
            subtitle(&quiet, &k).unwrap().state.unwrap().1,
            BadgeState::Queued
        );
        // A page with a date only.
        let desk = TitleBarSpec {
            subtitle: Some("11 月 3 日 周二".into()),
            ..TitleBarSpec::page("首页")
        };
        let s = subtitle(&desk, &k).unwrap();
        assert_eq!(s.text, "11 月 3 日 周二");
        assert!(s.state.is_none());
        assert!(subtitle(&TitleBarSpec::page("成员"), &k).is_none());
    }

    #[test]
    fn a_dropped_connection_takes_the_subtitle_with_wifi_slash() {
        use crate::data::ConnectionState;
        let k = NativeColors::dark();
        assert!(connection_subtitle(ConnectionState::Online, &k).is_none());
        let s = connection_subtitle(
            ConnectionState::Reconnecting {
                attempt: 2,
                retry_in_ms: 7_200,
            },
            &k,
        )
        .expect("a subtitle");
        assert_eq!(s.text, "连接已断开 · 8 秒后重连");
        assert_eq!(s.symbol, Some((Sym::Offline, k.secondary)));
    }

    #[test]
    fn the_panes_share_a_capsule_and_the_inspector_has_its_own() {
        let b = |kind| TitleButton::of(kind, RightPane::TopicRail);
        let spec = TitleBarSpec {
            buttons: vec![
                b(TitleButtonKind::SideChat),
                b(TitleButtonKind::Outputs),
                b(TitleButtonKind::Machine),
                b(TitleButtonKind::Changes),
                b(TitleButtonKind::Rail),
            ],
            ..live()
        };
        let g = groups(&spec);
        assert!(g.len() <= 3);
        assert_eq!(g.len(), 2);
        assert_eq!(g[1], vec![TitleButtonKind::Rail]);
        assert_eq!(g[0].len(), 4);
        assert!(groups(&TitleBarSpec::page("成员")).is_empty());
    }

    /// The heading in a bar `width` wide.
    struct Bar {
        width: f32,
        spec: TitleBarSpec,
        layouts: Option<(TextLayout, Option<TextLayout>)>,
    }

    impl Render for Bar {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let (el, title, sub) = heading(&self.spec, true, window, cx);
            self.layouts = Some((title, sub));
            div()
                .w(px(self.width))
                .h(metrics::TOOLBAR_ROW)
                .flex()
                .items_center()
                .child(el)
        }
    }

    #[gpui::test]
    fn a_narrow_bar_cuts_the_subtitle_end_before_the_title(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let window = cx.open_window(size(px(1200.), px(60.)), |_, _| Bar {
            width: 1100.,
            spec: live(),
            layouts: None,
        });
        let cut = |w: f32, cx: &mut TestAppContext| {
            window
                .update(cx, |bar, _, cx| {
                    bar.width = w;
                    cx.notify();
                })
                .unwrap();
            cx.run_until_parked();
            window
                .update(cx, |bar, _, _| {
                    let (title, sub) = bar.layouts.clone().expect("drawn");
                    (title.truncated(), sub.expect("a subtitle").truncated())
                })
                .unwrap()
        };
        assert_eq!(cut(1100., cx), (false, false), "a wide bar cuts nothing");
        // Narrowing: the subtitle (the long step at its end) is cut first,
        // the title later.
        let (mut sub_at, mut title_at) = (None, None);
        for i in 0..=105 {
            let w = 1100. - i as f32 * 10.;
            let (title, sub) = cut(w, cx);
            if sub && sub_at.is_none() {
                sub_at = Some(w);
            }
            if title && title_at.is_none() {
                title_at = Some(w);
            }
        }
        let (sub_at, title_at) = (
            sub_at.expect("the subtitle is cut"),
            title_at.expect("the title is cut"),
        );
        assert!(
            sub_at > title_at,
            "subtitle cut at {sub_at}, title at {title_at}"
        );
    }
}
