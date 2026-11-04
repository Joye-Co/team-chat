//! The right pane of the shell: one entity per pane kind, the width each
//! one takes, and the container that slides a pane in (opacity + 12 pt from
//! the right, 200 ms) and floats it over the page in a narrow window.

use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, Entity, IntoElement, ParentElement,
    Pixels, Styled, Subscription, Window, div,
};

use crate::{
    app::{Changes, RightPane, Services},
    ui::{self, native::metrics},
    views::{
        artifact_page::pane::ArtifactPane,
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
    pub topic_rail: Entity<TopicRail>,
    pub changes: Entity<ChangesPane>,
    pub side_chat: Entity<SideChat>,
    pub live: Entity<LivePane>,
    pub artifact: Entity<ArtifactPane>,
}

impl Panes {
    pub fn new(services: &Services, window: &mut Window, cx: &mut App) -> Self {
        Self {
            topic_rail: cx.new(|cx| TopicRail::new(services.clone(), cx)),
            changes: cx.new(|cx| ChangesPane::new(services.clone(), cx)),
            side_chat: cx.new(|cx| SideChat::new(services.clone(), window, cx)),
            live: cx.new(|cx| LivePane::new(services.clone(), cx)),
            artifact: cx.new(|cx| ArtifactPane::new(services.clone(), cx)),
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
        };
        // Content starts below the toolbar row (coordinator decision). A
        // pane with its own header (单独问, 实时查看, the 产出 pane) draws it
        // in a 52 pt row at its top; the inspector panes have none, so the
        // shell starts them below the row.
        let top = if header_less(pane) {
            metrics::PANE_TOP
        } else {
            gpui::px(0.)
        };
        let body = ui::motion::pane_enter(
            ElementId::Name(format!("pane-enter-{kind}").into()),
            div().size_full().pt(top).child(view),
            cx,
        );
        let column = super::chrome::inspector_frame(width, cx).child(body);
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

/// The one width of a pane kind (coordinator decision): 280 for the
/// inspector panes, 400 for the wide panes.
pub fn pane_width(pane: &RightPane) -> Pixels {
    match pane {
        RightPane::None => gpui::px(0.),
        RightPane::TopicRail | RightPane::Changes => metrics::INSPECTOR_WIDTH,
        RightPane::SideChat(_) | RightPane::Live(_) | RightPane::Artifact { .. } => {
            metrics::WIDE_PANE_WIDTH
        }
    }
}

/// Whether `pane` has no header row of its own (the inspector panes).
fn header_less(pane: &RightPane) -> bool {
    matches!(pane, RightPane::TopicRail | RightPane::Changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pane_kind_has_one_of_two_widths() {
        // D13: the inspector is 320 wide.
        assert_eq!(pane_width(&RightPane::TopicRail), gpui::px(320.));
        assert_eq!(pane_width(&RightPane::Changes), gpui::px(320.));
        assert_eq!(pane_width(&RightPane::None), gpui::px(0.));
        assert!(header_less(&RightPane::TopicRail));
    }
}
