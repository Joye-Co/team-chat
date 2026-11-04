//! The 产出 pane beside a 话题: the page live in the pane (the same child
//! web view as the 产出 page), with its version and 在新窗口打开.

use gpui::{
    Context, IntoElement, ParentElement, Pixels, Render, Styled, Subscription, Window, div, px,
};

use super::{Folders, embed};
use crate::{
    app::{RightPane, Services},
    copy::artifact_page as c,
    theme::space,
    ui::Sym,
    views::{
        group_page::{kind_label, kit},
        shell::{PaneView, panes::watch, stub_body},
    },
};

/// The wider trailing panes (单独问, 产出, 实时查看) are all 400 pt; the inspector is 280.
const PANE_WIDTH: Pixels = crate::ui::native::metrics::WIDE_PANE_WIDTH;

pub struct ArtifactPane {
    services: Services,
    folders: Folders,
    _subscriptions: Vec<Subscription>,
}

impl ArtifactPane {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        crate::views::artifacts::host::track_pane(services.nav.clone(), cx);
        Self {
            _subscriptions: watch(&services, cx),
            services,
            folders: Folders::default(),
        }
    }
}

impl PaneView for ArtifactPane {
    fn width(&self, _window_w: Pixels) -> Pixels {
        PANE_WIDTH
    }
}

impl Render for ArtifactPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let RightPane::Artifact { id, version } = self.services.nav.read(cx).visible_pane() else {
            return stub_body(Sym::Outputs, c::ARTIFACTS_EMPTY, None, cx).into_any_element();
        };
        let Some(detail) = self.services.provider.artifact(&id) else {
            return stub_body(Sym::Outputs, c::ARTIFACTS_EMPTY, None, cx).into_any_element();
        };
        let shown = version.unwrap_or(detail.summary.version);
        // topic track: a 产出 with a provider preview (metrics, table, chart)
        // draws as the Topic artboard's 产出 pane; the rest show the live page.
        if self
            .services
            .provider
            .artifact_preview(&id, shown)
            .is_some()
            && let Some(pane) =
                crate::views::topic_page::output_pane::render(&self.services, &id, version, cx)
        {
            return pane;
        }
        // The page fills the pane edge to edge under its toolbar row.
        let height = window.viewport_size().height - crate::ui::native::metrics::TOOLBAR_ROW;
        let body = match self.folders.get(&self.services, &id, shown, cx) {
            Some(Ok(folder)) if folder.is_page => embed(
                &self.services,
                "artifact-pane",
                &detail,
                &folder,
                height,
                px(0.),
                cx,
            )
            .into_any_element(),
            Some(_) => {
                let provider = self.services.provider.clone();
                let open = id.clone();
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(space::S12)
                    .child(kit::artifact_thumb(
                        &self.services,
                        &detail.summary,
                        px(192.),
                        px(144.),
                        cx,
                    ))
                    .child(
                        crate::ui::native::push_button(
                            "artifact-pane-open",
                            c::ARTIFACT_OPEN,
                            crate::ui::native::PushStyle::Default,
                            crate::ui::native::PushSize::Regular,
                            cx,
                        )
                        .on_click(move |_, _, cx| {
                            crate::views::artifacts::window::open_artifact(
                                &provider,
                                &open,
                                Some(shown),
                                cx,
                            )
                        }),
                    )
                    .into_any_element()
            }
            None => div().size_full().into_any_element(),
        };
        let provider = self.services.provider.clone();
        let nav = self.services.nav.clone();
        let open = id.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            // The pane's toolbar row (NATIVE.md §5.1): the 产出's name over
            // `类型 · v3`, one glass group.
            .child({
                use crate::ui::{native as nk, sf_symbols::Sf};
                let active = nk::emphasized(window);
                let open_window = nk::toolbar_item(
                    "artifact-pane-window",
                    Sf::Open,
                    c::ARTIFACT_OPEN_WINDOW,
                    nk::ToolbarItemKind::Plain,
                    active,
                    window,
                    cx,
                )
                .on_click(move |_, _, cx| {
                    crate::views::artifacts::window::open_artifact(
                        &provider,
                        &open,
                        Some(shown),
                        cx,
                    )
                });
                let page = nk::toolbar_item(
                    "artifact-pane-page",
                    Sf::Outputs,
                    c::ARTIFACTS_TITLE,
                    nk::ToolbarItemKind::Plain,
                    active,
                    window,
                    cx,
                )
                .on_click(move |_, _, cx| {
                    let id = id.clone();
                    nav.update(cx, |n, cx| n.open_artifact(id, version, cx))
                });
                nk::ToolbarRow::new(detail.summary.title.clone())
                    .subtitle(format!("{} · v{shown}", kind_label(detail.summary.kind)))
                    .leading_inset(crate::ui::native::metrics::TOOLBAR_LEADING)
                    .group(nk::ToolbarGroup::new([open_window, page]))
            })
            .child(div().flex_1().min_h_0().overflow_hidden().child(body))
            .into_any_element()
    }
}
