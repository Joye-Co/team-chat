//! The right pane of the shell: one entity per pane kind, the width each
//! one takes, and the container that shows a pane (no entrance motion) and
//! floats it over the page in a narrow window.

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, IntoElement, ParentElement, Pixels, Styled,
    Subscription, Window, div,
};

use crate::{
    app::{Changes, RightPane, Services},
    ui::native::metrics,
    views::{
        artifact_page::pane::ArtifactPane,
        group_page::info::GroupInfoPane,
        live::LivePane,
        side_chat::SideChat,
        topic_rail::{ChangesPane, TopicRail},
    },
};

/// Re-renders a page or pane entity when the provider or navigation
/// changes. Feature tracks narrow this to what they read.
pub fn watch<T: 'static>(services: &Services, cx: &mut Context<T>) -> Vec<Subscription> {
    vec![
        cx.subscribe(&services.hub, |_, _, _: &Changes, cx| cx.notify()),
        cx.observe(&services.nav, |_, _, cx| cx.notify()),
    ]
}

/// The pane entities of one main window.
pub struct Panes {
    nav: Entity<crate::app::Navigation>,
    pub topic_rail: Entity<TopicRail>,
    pub changes: Entity<ChangesPane>,
    pub side_chat: Entity<SideChat>,
    pub live: Entity<LivePane>,
    pub artifact: Entity<ArtifactPane>,
    pub group_info: Entity<GroupInfoPane>,
}

impl Panes {
    pub fn new(services: &Services, window: &mut Window, cx: &mut App) -> Self {
        Self {
            nav: services.nav.clone(),
            topic_rail: cx.new(|cx| TopicRail::new(services.clone(), cx)),
            changes: cx.new(|cx| ChangesPane::new(services.clone(), cx)),
            side_chat: cx.new(|cx| SideChat::new(services.clone(), window, cx)),
            live: cx.new(|cx| LivePane::new(services.clone(), cx)),
            artifact: cx.new(|cx| ArtifactPane::new(services.clone(), cx)),
            group_info: cx.new(|cx| GroupInfoPane::new(services.clone(), cx)),
        }
    }

    /// The width `pane` takes; zero for none. One width per kind on every
    /// screen (coordinator decision): the inspector and 改动 are 280 pt,
    /// 单独问, the 产出 pane and 实时查看 are 400 pt. The panes' own
    /// `PaneView::width` is not asked, so the trailing column never jumps
    /// between screens.
    pub fn width(&self, pane: &RightPane, _window_w: Pixels, _cx: &App) -> Pixels {
        pane_width(pane)
    }

    fn view(&self, pane: &RightPane) -> Option<gpui::AnyView> {
        Some(match pane {
            RightPane::None => return None,
            RightPane::TopicRail => self.topic_rail.clone().into(),
            RightPane::Changes => self.changes.clone().into(),
            RightPane::SideChat(_) => self.side_chat.clone().into(),
            RightPane::Live(_) => self.live.clone().into(),
            RightPane::Artifact { .. } => self.artifact.clone().into(),
            RightPane::GroupInfo => self.group_info.clone().into(),
        })
    }

    /// The width `pane` gets beside the page: always its one width; a
    /// window without room floats the pane over the page instead.
    pub fn fitted_width(
        &self,
        pane: &RightPane,
        window_w: Pixels,
        _room: Pixels,
        cx: &App,
    ) -> Pixels {
        self.width(pane, window_w, cx)
    }

    /// The pane in its column, `width` wide (NATIVE.md §2 inspector): edge
    /// to edge with the `sidebar` fill and a 1 px `separator` on its leading
    /// edge, the entrance, and a float over the page when the window has no
    /// room for it.
    pub fn render(
        &self,
        pane: &RightPane,
        width: Pixels,
        floats: bool,
        cx: &App,
    ) -> Option<AnyElement> {
        let view = self.view(pane)?;
        let kind = match pane {
            RightPane::None => "none",
            RightPane::TopicRail => "rail",
            RightPane::Changes => "changes",
            RightPane::SideChat(_) => "side-chat",
            RightPane::Live(_) => "live",
            RightPane::Artifact { .. } => "artifact",
            RightPane::GroupInfo => "group-info",
        };
        // Content starts below the toolbar row (coordinator decision). A
        // pane with its own header (单独问, 实时查看, the 产出 pane) draws it
        // in a 52 pt row at its top; the inspector panes have none, so the
        // shell starts them below the row.
        // §2.2, §2.5: every right pane starts with the h52 header band and
        // its border-b. The inspector panes get theirs here (pane name 16
        // medium, close at the far right); the others draw their own 52
        // row, and the shell lays the border line at y 52 over it.
        let k = super::chrome::colors(cx);
        let body = if let Some(title) = header_less(pane).then(|| pane_title(pane)) {
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(pane_header(title, self.nav.clone(), cx))
                .child(div().flex_1().min_h(gpui::px(0.)).child(view))
        } else {
            div().size_full().child(view)
        };
        // review-animations 2: a pane toggled from the keyboard (⌘⌥I, ⌘;)
        // or the toolbar appears at once; a slide that replays on every
        // toggle reads as lag.
        let _ = kind;
        let column = super::chrome::inspector_frame(width, cx).child(body).child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(metrics::TOOLBAR_ROW - gpui::px(1.))
                .h(gpui::px(1.))
                .bg(k.separator),
        );
        Some(if floats {
            column
                .absolute()
                .top_0()
                .right_0()
                .shadow(super::chrome::colors(cx).float())
                .into_any_element()
        } else {
            column.into_any_element()
        })
    }
}

/// The one width of a pane kind (D13, owner decision): 320 for the
/// inspector, 改动 and 单独问; 400 for 实时查看 and the 产出 pane, which
/// show a screen or a page.
pub fn pane_width(pane: &RightPane) -> Pixels {
    match pane {
        RightPane::None => gpui::px(0.),
        RightPane::TopicRail
        | RightPane::Changes
        | RightPane::SideChat(_)
        | RightPane::GroupInfo => metrics::INSPECTOR_WIDTH,
        RightPane::Live(_) | RightPane::Artifact { .. } => metrics::WIDE_PANE_WIDTH,
    }
}

/// Whether `pane` has no header row of its own (the inspector panes).
/// The name of a pane the shell draws the header for.
fn pane_title(pane: &RightPane) -> &'static str {
    match pane {
        RightPane::Changes => crate::copy::topic_rail::TITLE_CHANGES,
        RightPane::GroupInfo => crate::copy::group_page::GROUP_PAGE_INFO,
        _ => crate::copy::common::PANE_TOPIC,
    }
}

/// The header band of a right pane (§2.2): h52, px 16, the pane name 16
/// medium, a 32 ghost close button at the far right.
fn pane_header(title: &'static str, nav: Entity<crate::app::Navigation>, cx: &App) -> AnyElement {
    use crate::ui::native::{IconSize, NativeType as _, icon_button_sym};
    let k = super::chrome::colors(cx);
    div()
        .flex_none()
        .h(metrics::TOOLBAR_ROW)
        .pl(metrics::INSPECTOR_PAD)
        .pr(metrics::TOOLBAR_TRAILING)
        .flex()
        .items_center()
        .gap(crate::ui::native::tokens::space::S8)
        .child(
            div()
                .flex_1()
                .min_w(gpui::px(0.))
                .n_title3()
                .text_color(k.label)
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(title),
        )
        .child(
            icon_button_sym(
                "pane-close",
                crate::ui::Sym::Close,
                crate::copy::common::CLOSE,
                IconSize::Regular,
                cx,
            )
            .on_click(move |_, _, cx| {
                nav.update(cx, |nav, cx| nav.set_right_pane(RightPane::None, cx))
            }),
        )
        .into_any_element()
}

fn header_less(pane: &RightPane) -> bool {
    matches!(
        pane,
        RightPane::TopicRail | RightPane::Changes | RightPane::GroupInfo
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pane_kind_has_one_of_two_widths() {
        // D13: the inspector is 320 wide.
        assert_eq!(pane_width(&RightPane::TopicRail), gpui::px(320.));
        assert_eq!(pane_width(&RightPane::Changes), gpui::px(320.));
        assert_eq!(
            pane_width(&RightPane::SideChat("m-ledger".into())),
            gpui::px(320.)
        );
        assert_eq!(pane_width(&RightPane::Live("r-1".into())), gpui::px(400.));
        assert_eq!(pane_width(&RightPane::None), gpui::px(0.));
        assert!(header_less(&RightPane::TopicRail));
        // 群资料 is the 群聊's 详情: the inspector width, the shell's header.
        assert_eq!(pane_width(&RightPane::GroupInfo), gpui::px(320.));
        assert!(header_less(&RightPane::GroupInfo));
        assert_eq!(pane_title(&RightPane::GroupInfo), "群资料");
    }
}
