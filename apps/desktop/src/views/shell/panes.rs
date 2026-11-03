//! The right pane of the shell: one entity per pane kind, the width each
//! one takes, and the container that slides a pane in (opacity + 12 pt from
//! the right, 200 ms) and floats it over the page in a narrow window.

use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, Entity, IntoElement, ParentElement,
    Pixels, Styled, Subscription, Window, div,
};

use super::PaneView;
use crate::{
    app::{Changes, RightPane, Services},
    theme, ui,
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

    /// The width `pane` takes in a window `window_w` wide; zero for none.
    pub fn width(&self, pane: &RightPane, window_w: Pixels, cx: &App) -> Pixels {
        match pane {
            RightPane::None => gpui::px(0.),
            RightPane::TopicRail => self.topic_rail.read(cx).width(window_w),
            RightPane::Changes => self.changes.read(cx).width(window_w),
            RightPane::SideChat(_) => self.side_chat.read(cx).width(window_w),
            RightPane::Live(_) => self.live.read(cx).width(window_w),
            RightPane::Artifact { .. } => self.artifact.read(cx).width(window_w),
        }
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

    /// The width `pane` gets beside the page: the 实时查看 pane gives up
    /// room down to its minimum so the page keeps [`theme::PAGE_MIN_WIDTH`].
    pub fn fitted_width(
        &self,
        pane: &RightPane,
        window_w: Pixels,
        room: Pixels,
        cx: &App,
    ) -> Pixels {
        let want = self.width(pane, window_w, cx);
        match pane {
            RightPane::Live(_) => want
                .min(room - theme::PAGE_MIN_WIDTH)
                .max(theme::LIVE_PANE_MIN_WIDTH),
            _ => want,
        }
    }

    /// The pane in its column, `width` wide: a hairline at its start, the
    /// entrance, and a float over the page (level 2) when the window has no
    /// room for it.
    pub fn render(
        &self,
        pane: &RightPane,
        width: Pixels,
        floats: bool,
        cx: &App,
    ) -> Option<AnyElement> {
        let view = self.view(pane)?;
        let k = theme::colors(cx);
        let kind = match pane {
            RightPane::None => "none",
            RightPane::TopicRail => "rail",
            RightPane::Changes => "changes",
            RightPane::SideChat(_) => "side-chat",
            RightPane::Live(_) => "live",
            RightPane::Artifact { .. } => "artifact",
        };
        let body = ui::motion::pane_enter(
            ElementId::Name(format!("pane-enter-{kind}").into()),
            div().size_full().child(view),
            cx,
        );
        let column = div()
            .w(width)
            .h_full()
            .flex_none()
            .bg(k.rail)
            .relative()
            .overflow_hidden()
            .child(body)
            .child(ui::column_start_edge(cx));
        Some(if floats {
            column
                .absolute()
                .top_0()
                .right_0()
                .shadow(k.shadow(theme::Elevation::Overlay))
                .into_any_element()
        } else {
            column.into_any_element()
        })
    }
}
