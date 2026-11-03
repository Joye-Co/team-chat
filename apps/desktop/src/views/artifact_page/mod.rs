//! 产出 in the main window (Artifact.dc.html): the list of every 产出, and
//! one 产出 with its page in a frame and the 来龙去脉 rail (来自话题, 做的人,
//! 用到的决定, 版本, 被引用). Also the 产出 pane beside a 话题 (`pane`).
//!
//! Foundation stub: the list, the versions and 来龙去脉 read the provider;
//! the artifact track embeds the page (WKWebView spike) or its snapshot.

pub mod pane;
pub mod scenes;

use gpui::{
    App, Context, InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div, px,
};

use crate::{
    app::{Place, Services},
    copy::artifact_page as c,
    theme::{self, Type as _, space},
    ui::{self, Lucide, RowSurface},
    views::shell::{
        PageView, TitleBarSpec, clock, member_name, panes::watch, section_label, stub_body,
    },
};

pub struct ArtifactPage {
    services: Services,
    _subscriptions: Vec<Subscription>,
}

impl ArtifactPage {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        Self {
            _subscriptions: watch(&services, cx),
            services,
        }
    }

    fn open(&self, cx: &App) -> Option<(String, Option<u32>)> {
        match &self.services.nav.read(cx).place {
            Some(Place::Artifact { id, version }) => Some((id.clone(), *version)),
            _ => None,
        }
    }
}

impl PageView for ArtifactPage {
    fn title_bar(&self, cx: &App) -> TitleBarSpec {
        match self
            .open(cx)
            .and_then(|(id, _)| self.services.provider.artifact(&id))
        {
            Some(a) => TitleBarSpec {
                subtitle: Some(format!(
                    "v{} · {}",
                    a.summary.version,
                    clock(a.summary.updated_at)
                )),
                ..TitleBarSpec::page(a.summary.title)
            },
            None => TitleBarSpec::page(c::ARTIFACTS_TITLE),
        }
    }
}

/// 来龙去脉 of one 产出, as rail cards.
pub fn story(services: &Services, id: &str, cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    let p = &services.provider;
    let Some(s) = p.artifact_story(id) else {
        return div();
    };
    let card = |label: &'static str, lines: Vec<String>| {
        (!lines.is_empty()).then(|| {
            ui::rail_card(cx)
                .child(section_label(label, None, cx))
                .children(
                    lines
                        .into_iter()
                        .map(|l| div().t_small().text_color(k.ink).child(l)),
                )
        })
    };
    let topic = s
        .topic
        .as_ref()
        .and_then(|(g, t)| p.topic(g, t))
        .map(|t| t.title);
    let people: Vec<String> = s
        .made_by
        .iter()
        .chain(s.asked_by.iter())
        .map(|m| member_name(services, m))
        .collect();
    div()
        .flex()
        .flex_col()
        .gap(space::S10)
        .children(card(c::ARTIFACT_FROM_TOPIC, topic.into_iter().collect()))
        .children(card(c::ARTIFACT_MADE_BY, vec![people.join(" · ")]))
        .children(card(
            c::ARTIFACT_DECISIONS,
            s.decisions.iter().map(|d| d.text.clone()).collect(),
        ))
        .children(card(
            c::ARTIFACT_VERSIONS,
            s.version_notes
                .iter()
                .map(|(v, n)| format!("v{v} · {n}"))
                .collect(),
        ))
        .children(card(
            c::ARTIFACT_CITED_BY,
            s.cited_by.iter().map(|(_, _, t)| t.clone()).collect(),
        ))
}

impl Render for ArtifactPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let p = self.services.provider.clone();
        if let Some((id, _version)) = self.open(cx) {
            let title = p.artifact(&id).map(|a| a.summary.title).unwrap_or_default();
            return div()
                .size_full()
                .flex()
                .child(
                    div().flex_1().min_w(px(0.)).h_full().p(space::S24).child(
                        ui::card(cx).size_full().child(stub_body(
                            Lucide::FileText,
                            &title,
                            None,
                            cx,
                        )),
                    ),
                )
                .child(
                    div()
                        .id("artifact-story")
                        .flex_none()
                        .w(theme::ARTIFACT_RAIL_WIDTH)
                        .h_full()
                        .overflow_y_scroll()
                        .bg(k.rail)
                        .p(space::S16)
                        .child(section_label(c::ARTIFACT_STORY, None, cx))
                        .child(story(&self.services, &id, cx)),
                )
                .into_any_element();
        }
        let all = p.all_artifacts();
        if all.is_empty() {
            return stub_body(Lucide::Layers, c::ARTIFACTS_EMPTY, None, cx).into_any_element();
        }
        div()
            .id("artifacts-list")
            .size_full()
            .overflow_y_scroll()
            .px(space::S32)
            .flex()
            .flex_col()
            .gap(px(1.))
            .children(all.into_iter().map(|(_, a)| {
                let nav = self.services.nav.clone();
                let id = a.id.clone();
                ui::list_row(
                    SharedString::from(format!("artifact-{}", a.id)),
                    RowSurface::Content,
                    false,
                    cx,
                )
                .h(px(44.))
                .px(space::S12)
                .gap(space::S10)
                .child(ui::icon(Lucide::FileText, theme::icon::ROW, k.ink_2))
                .child(
                    div()
                        .flex_1()
                        .t_ui()
                        .text_color(k.ink)
                        .child(a.title.clone()),
                )
                .child(div().t_mono().text_color(k.ink_3).child(format!(
                    "v{} · {}",
                    a.version,
                    clock(a.updated_at)
                )))
                .on_click(move |_, _, cx| {
                    let id = id.clone();
                    nav.update(cx, |nav, cx| nav.open_artifact(id, None, cx))
                })
            }))
            .into_any_element()
    }
}
