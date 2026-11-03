//! The 产出 pane beside a 话题 (Topic.dc.html right pane): the 产出 page
//! rendered inline with its version and 在窗口打开.
//!
//! Foundation stub: names the 产出 and its version; the artifact track shows
//! the page itself.

use gpui::{Context, IntoElement, Pixels, Render, Subscription, Window};

use crate::{
    app::{RightPane, Services},
    copy::artifact_page as c,
    theme,
    ui::Lucide,
    views::shell::{PaneView, clock, panes::watch, stub_body},
};

pub struct ArtifactPane {
    services: Services,
    _subscriptions: Vec<Subscription>,
}

impl ArtifactPane {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        Self {
            _subscriptions: watch(&services, cx),
            services,
        }
    }
}

impl PaneView for ArtifactPane {
    fn width(&self, _window_w: Pixels) -> Pixels {
        theme::ARTIFACT_PANE_WIDTH
    }
}

impl Render for ArtifactPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let RightPane::Artifact { id, .. } = self.services.nav.read(cx).visible_pane() else {
            return stub_body(Lucide::Layers, c::ARTIFACTS_EMPTY, None, cx);
        };
        match self.services.provider.artifact(&id) {
            Some(a) => stub_body(
                Lucide::FileText,
                &a.summary.title,
                Some(format!(
                    "v{} · {}",
                    a.summary.version,
                    clock(a.summary.updated_at)
                )),
                cx,
            ),
            None => stub_body(Lucide::Layers, c::ARTIFACTS_EMPTY, None, cx),
        }
    }
}
