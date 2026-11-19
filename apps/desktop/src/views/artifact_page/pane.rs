//! The 产出 pane beside a 话题: the page live in the pane (the same child
//! web view as the 产出 page) or a Markdown document read in it, with its
//! version and 在新窗口打开.

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

/// The one right pane width (D13).
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
        // draws as the Topic artboard's 产出 pane; a web page always shows live
        // (its scraped preview is only its text), and so do the rest.
        if detail.summary.kind != crate::data::ArtifactKind::Page
            && self
                .services
                .provider
                .artifact_preview(&id, shown)
                .is_some()
            && let Some(pane) =
                crate::views::topic_page::output_pane::render(&self.services, &id, version, cx)
        {
            return pane;
        }
        // The page fills the pane under its header, on the pane's
        // background (§2.1), with the pane's 16 padding (§2.5).
        let folder = self.folders.get(&self.services, &id, shown, cx);
        // A Markdown document reads in the pane itself, with the same padding.
        let reader = match &folder {
            Some(Ok(f)) => crate::views::media::in_place::markdown(
                "artifact-pane",
                f,
                &detail.summary.title,
                crate::ui::native::tokens::layout::IN_THREAD,
                space::S16,
                cx,
            ),
            _ => None,
        };
        let body = match (folder, reader) {
            (_, Some(doc)) => doc,
            (Some(Ok(folder)), None) if folder.is_page => embed(
                &self.services,
                "artifact-pane",
                &detail,
                &folder,
                crate::ui::native::metrics::TOOLBAR_ROW,
                px(0.),
                cx,
            )
            .fill()
            .pad(space::S16)
            .into_any_element(),
            (Some(_), None) => {
                // Not a web page: a designed Empty with 打开 (§2.13), not a
                // thumbnail floating in the middle of the pane.
                let provider = self.services.provider.clone();
                let open = id.clone();
                crate::ui::native::page::page_empty(
                    super::kind_sym(detail.summary.kind),
                    detail.summary.title.clone(),
                    c::ARTIFACT_PAGE_SNAPSHOT,
                    vec![
                        crate::ui::native::push_button(
                            "artifact-pane-open",
                            c::ARTIFACT_OPEN,
                            crate::ui::native::PushStyle::Default,
                            crate::ui::native::PushSize::Small,
                            cx,
                        )
                        .on_click(move |_, _, cx| {
                            crate::views::artifacts::window::open_artifact(
                                &provider,
                                &open,
                                Some(shown),
                                cx,
                            )
                        })
                        .into_any_element(),
                    ],
                    cx,
                )
                .into_any_element()
            }
            (None, None) => div().size_full().into_any_element(),
        };
        let provider = self.services.provider.clone();
        let nav = self.services.nav.clone();
        let open = id.clone();
        let _ = window;
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(crate::ui::native::native_colors(cx).content)
            // The pane header (§2.2): the pane's name "产出" over the 产出's
            // title and version (the page under it shows its own title),
            // then 在新窗口打开, 在产出里打开 and close at the far right.
            .child({
                use crate::ui::{native as nk, sf_symbols::Sym as S};
                let open_window = nk::icon_button_sym(
                    "artifact-pane-window",
                    S::OpenExternal,
                    c::ARTIFACT_OPEN_WINDOW,
                    nk::IconSize::Regular,
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
                let page = nk::icon_button_sym(
                    "artifact-pane-page",
                    S::Outputs,
                    c::ARTIFACTS_TITLE,
                    nk::IconSize::Regular,
                    cx,
                )
                .on_click({
                    let nav = nav.clone();
                    move |_, _, cx| {
                        let id = id.clone();
                        nav.update(cx, |n, cx| n.open_artifact(id, version, cx))
                    }
                });
                let close = nk::icon_button_sym(
                    "artifact-pane-close",
                    S::Close,
                    crate::copy::common::CLOSE,
                    nk::IconSize::Regular,
                    cx,
                )
                .on_click(move |_, _, cx| {
                    nav.update(cx, |n, cx| {
                        let back = if n.page() == crate::app::Page::Topic {
                            RightPane::TopicRail
                        } else {
                            RightPane::None
                        };
                        n.set_right_pane(back, cx)
                    })
                });
                kit::pane_header(
                    c::ARTIFACTS_TITLE,
                    format!(
                        "{} · {} · v{shown}",
                        detail.summary.title,
                        kind_label(detail.summary.kind)
                    ),
                    vec![
                        open_window.into_any_element(),
                        page.into_any_element(),
                        close.into_any_element(),
                    ],
                    cx,
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .child(body),
            )
            .into_any_element()
    }
}
