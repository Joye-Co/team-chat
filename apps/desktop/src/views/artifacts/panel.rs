//! The 产出 and files of the open topic, in the side panel. A 产出 opens on
//! click; 上传产出 picks an HTML file or a folder and uploads it as a new
//! 产出, and each 产出 can get a new version the same way.

use std::path::PathBuf;

use gpui::{
    AsyncApp, Context, ElementId, InteractiveElement as _, IntoElement,
    ParentElement, PathPromptOptions, Render, SharedString, StatefulInteractiveElement as _,
    Styled, Subscription, WeakEntity, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    notification::Notification,
    progress::Progress,
};

use super::window;
use crate::{
    app::{Changes, Services},
    copy::{artifacts as ca, fill, panel as p},
    data::{
        ArtifactKind, ArtifactSummary, Change, FileError, GroupId, Member, NewArtifactRequest, TopicFile,
        TopicId, TransferState, create_key,
    },
    theme::{self, Type as _, space},
    ui::{self, Lucide},
    views::{
        files::{
            PANEL_ROW_H, error_text,
            extra::{TileKind, file_row_in, tile},
            notify, notify_error,
        },
        time_text,
    },
};

/// Most files listed; older ones stay in the timeline.
const MAX_FILES: usize = 50;

pub struct TopicOutputs {
    services: Services,
    open: Option<(GroupId, TopicId)>,
    artifacts: Vec<ArtifactSummary>,
    files: Vec<TopicFile>,
    members: Vec<Member>,
    _subscriptions: Vec<Subscription>,
}

impl TopicOutputs {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.subscribe(&services.hub, |this, _, changes: &Changes, cx| {
                let Some((g, t)) = this.open.clone() else {
                    return;
                };
                if changes.touches_topic(&g, &t) || changes.any(|c| matches!(c, Change::Members)) {
                    this.refresh();
                    cx.notify();
                }
            }),
            cx.observe(&services.nav, |this, nav, cx| {
                let open = nav.read(cx).topic.clone();
                if open != this.open {
                    this.open = open;
                    this.on_open();
                    cx.notify();
                }
            }),
        ];
        let open = services.nav.read(cx).topic.clone();
        let mut this = Self {
            services,
            open,
            artifacts: Vec::new(),
            files: Vec::new(),
            members: Vec::new(),
            _subscriptions: subscriptions,
        };
        this.on_open();
        this
    }

    fn on_open(&mut self) {
        if let Some((g, t)) = &self.open {
            self.services.provider.refresh_artifacts(g, t);
        }
        self.refresh();
    }

    fn refresh(&mut self) {
        let provider = &self.services.provider;
        self.members = provider.members();
        match &self.open {
            Some((g, t)) => {
                self.artifacts = provider.artifacts(g, t);
                self.files = provider.topic_files(g, t);
                self.files.truncate(MAX_FILES);
            }
            None => {
                self.artifacts.clear();
                self.files.clear();
            }
        }
    }

    /// Picks a file or folder and uploads it as a new 产出 (`artifact_id:
    /// None`) or a new version.
    fn pick_and_upload(&mut self, artifact_id: Option<String>, cx: &mut Context<Self>) {
        let Some((group_id, topic_id)) = self.open.clone() else {
            return;
        };
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: false,
            prompt: Some(ca::PICK.into()),
        });
        let provider = self.services.provider.clone();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let picked: Option<PathBuf> = match paths.await {
                Ok(Ok(Some(mut list))) if !list.is_empty() => Some(list.remove(0)),
                _ => None,
            };
            let Some(path) = picked else { return };
            let rx = provider.create_artifact(NewArtifactRequest {
                group_id,
                topic_id,
                path,
                artifact_id,
            });
            let result = rx.await.unwrap_or(Err(FileError::Other));
            cx.update(|cx| match result {
                Ok(created) => notify(
                    Notification::success(fill(
                        ca::CREATED,
                        &[("title", &created.title), ("n", &created.version.to_string())],
                    )),
                    cx,
                ),
                Err(e) => notify_error(fill(ca::CREATE_FAILED, &[("reason", error_text(e))]), cx),
            })
        })
        .detach();
    }
}

impl TopicOutputs {
    /// 产出 and files listed, for the tab label.
    pub fn count(&self) -> usize {
        self.artifacts.len() + self.files.len()
    }
}

impl Render for TopicOutputs {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let Some((g, t)) = self.open.clone() else {
            return div();
        };
        let provider = self.services.provider.clone();
        let creating = provider.file_transfer(&create_key(&t));
        let busy = matches!(creating, Some(TransferState::Running { .. }));

        // "产出" with a trailing + (上传产出), §5.2 section header.
        let header = ui::SectionHeader::new(p::ARTIFACTS).trailing(
            ui::icon_button("artifact-new", Lucide::Plus)
                .xsmall()
                .tooltip(ca::NEW)
                .disabled(busy)
                .on_click(cx.listener(|this, _, _, cx| this.pick_and_upload(None, cx))),
        );

        let rows: Vec<_> = self
            .artifacts
            .iter()
            .map(|a| {
                let (kind, kind_word) = match a.kind {
                    ArtifactKind::Page => (TileKind::Page, ca::KIND_PAGE),
                    _ => (TileKind::Text, ca::KIND_FILE),
                };
                let meta = format!(
                    "{kind_word} · {} · {}",
                    fill(p::VERSION, &[("n", &a.version.to_string())]),
                    time_text(a.updated_at)
                );
                let (id, id2) = (a.id.clone(), a.id.clone());
                let provider = provider.clone();
                let group = format!("panel-artifact-{}", a.id);
                div()
                    .id(ElementId::Name(SharedString::from(group.clone())))
                    .group(SharedString::from(group.clone()))
                    .h(PANEL_ROW_H)
                    .mx(-space::S6)
                    .px(space::S6)
                    .rounded(theme::radius::MD)
                    .hover(|s| s.bg(colors.fill_hover))
                    .active(|s| s.bg(colors.fill_pressed))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(space::S10)
                    .child(tile(kind, px(32.), cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(1.))
                            .child(
                                div()
                                    .t_ui()
                                    .medium()
                                    .text_color(colors.text_1)
                                    .one_line()
                                    .child(a.title.clone()),
                            )
                            .child(
                                div()
                                    .t_caption()
                                    .font_weight(gpui::FontWeight::NORMAL)
                                    .tnum()
                                    .text_color(colors.text_2)
                                    .one_line()
                                    .child(meta),
                            ),
                    )
                    .child(
                        Button::new(ElementId::Name(SharedString::from(format!(
                            "panel-artifact-version-{}",
                            a.id
                        ))))
                        .xsmall()
                        .ghost()
                        .icon(Lucide::Plus)
                        .tooltip(ca::NEW_VERSION)
                        .disabled(busy)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.pick_and_upload(Some(id2.clone()), cx)
                        })),
                    )
                    .on_click(move |_, _, cx| window::open_artifact(&provider, &id, None, cx))
                    .into_any_element()
            })
            .collect();

        let files: Vec<_> = self
            .files
            .iter()
            .enumerate()
            .map(|(i, tf)| {
                let key = format!("panel-file-{}-{i}", tf.message_id);
                file_row_in(
                    &g,
                    &t,
                    &tf.file,
                    &provider,
                    true,
                    |part| ElementId::Name(SharedString::from(format!("{key}-{part}"))),
                    cx,
                )
            })
            .collect();
        let empty = rows.is_empty() && files.is_empty() && creating.is_none();

        div()
            .flex()
            .flex_col()
            .child(div().pb(space::S6).child(header))
            .when_some(creating, |el, state| match state {
                TransferState::Running { .. } => el.child(
                    div()
                        .h(PANEL_ROW_H)
                        .flex()
                        .flex_col()
                        .justify_center()
                        .gap(space::S6)
                        .child(
                            div()
                                .t_caption()
                                .font_weight(gpui::FontWeight::NORMAL)
                                .tnum()
                                .text_color(colors.text_2)
                                .child(fill(ca::CREATING, &[("p", &format!("{:.0}", state.percent()))])),
                        )
                        .child(
                            Progress::new("artifact-create-progress")
                                .with_size(gpui_component::Size::Size(px(3.)))
                                .color(colors.accent)
                                .value(state.percent()),
                        ),
                ),
                _ => el,
            })
            .when(empty, |el| {
                el.child(
                    div().h(px(220.)).child(
                        ui::EmptyState::new(Lucide::Layers, p::EMPTY_TITLE)
                            .description(p::EMPTY_BODY),
                    ),
                )
            })
            .children(rows)
            .children(files)
    }
}
