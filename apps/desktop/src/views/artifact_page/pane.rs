//! The 产出 pane beside a 话题: the page live in the pane (the same child
//! web view as the 产出 page), with its version and 在新窗口打开.

use gpui::{
    Context, IntoElement, ParentElement, Pixels, Render, Styled, Subscription, Window, div, px,
};

use super::{Folders, embed};
use crate::{
    app::{RightPane, Services},
    copy::artifact_page as c,
    theme::{self, Type as _, space},
    ui::{self, Lucide},
    views::{
        group_page::{kind_label, kit},
        shell::{PaneView, panes::watch, stub_body},
    },
};

pub struct ArtifactPane {
    services: Services,
    folders: Folders,
    _subscriptions: Vec<Subscription>,
}

impl ArtifactPane {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        Self {
            _subscriptions: watch(&services, cx),
            services,
            folders: Folders::default(),
        }
    }
}

impl PaneView for ArtifactPane {
    fn width(&self, _window_w: Pixels) -> Pixels {
        theme::ARTIFACT_PANE_WIDTH
    }
}

impl Render for ArtifactPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let RightPane::Artifact { id, version } = self.services.nav.read(cx).visible_pane() else {
            return stub_body(Lucide::Layers, c::ARTIFACTS_EMPTY, None, cx).into_any_element();
        };
        let Some(detail) = self.services.provider.artifact(&id) else {
            return stub_body(Lucide::Layers, c::ARTIFACTS_EMPTY, None, cx).into_any_element();
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
        let height = window.viewport_size().height - theme::TITLE_BAR - space::S12 - px(2.);
        let body = match self.folders.get(&self.services, &id, shown, cx) {
            Some(Ok(folder)) if folder.is_page => embed(
                &self.services,
                "artifact-pane",
                &detail,
                &folder,
                height,
                px(0.),
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
                        ui::button(
                            "artifact-pane-open",
                            c::ARTIFACT_OPEN,
                            ui::ButtonKind::Primary,
                            ui::ButtonSize::Md,
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
            .px(space::S12)
            .pb(space::S12)
            .child(
                kit::bar()
                    .px(space::S4)
                    .child(
                        div()
                            .min_w_0()
                            .one_line()
                            .t_small()
                            .semibold()
                            .text_color(k.ink)
                            .child(detail.summary.title.clone()),
                    )
                    .child(ui::JobTag::new(kind_label(detail.summary.kind)))
                    .child(
                        div()
                            .mono()
                            .type_role(kit::META)
                            .text_color(k.ink_3)
                            .child(format!("v{shown}")),
                    )
                    .child(div().flex_1())
                    .child(
                        kit::bar_icon(
                            "artifact-pane-window",
                            Lucide::ExternalLink,
                            c::ARTIFACT_OPEN_WINDOW,
                            false,
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
                    .child(
                        kit::bar_icon(
                            "artifact-pane-page",
                            Lucide::Layers,
                            c::ARTIFACTS_TITLE,
                            false,
                            cx,
                        )
                        .on_click(move |_, _, cx| {
                            let id = id.clone();
                            nav.update(cx, |n, cx| n.open_artifact(id, version, cx))
                        }),
                    ),
            )
            .child(
                ui::card(cx)
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_hidden()
                    .child(body),
            )
            .into_any_element()
    }
}
