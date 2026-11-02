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
    Disableable as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    notification::Notification,
    progress::Progress,
};

use super::window;
use crate::{
    app::{Changes, Services},
    copy::{artifacts as ca, files as f, fill, panel as p},
    data::{
        ArtifactKind, ArtifactSummary, Change, FileError, GroupId, Member, NewArtifactRequest, TopicFile,
        TopicId, TransferState, create_key,
    },
    theme,
    views::{
        files::{error_text, extra::file_row_in, notify, notify_error},
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

impl Render for TopicOutputs {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let Some((g, t)) = self.open.clone() else {
            return div();
        };
        let section = |title: &'static str| {
            div()
                .pt(px(12.))
                .pb(px(4.))
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(title)
        };
        let provider = self.services.provider.clone();
        let creating = provider.file_transfer(&create_key(&t));
        let busy = matches!(creating, Some(TransferState::Running { .. }));

        let header = div()
            .flex()
            .items_center()
            .child(section(p::ARTIFACTS).flex_1())
            .child(
                Button::new("artifact-new")
                    .xsmall()
                    .ghost()
                    .icon(IconName::Plus)
                    .label(ca::NEW)
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, _, cx| this.pick_and_upload(None, cx))),
            );

        let rows: Vec<_> = self
            .artifacts
            .iter()
            .map(|a| {
                let kind = match a.kind {
                    ArtifactKind::Page => ca::KIND_PAGE,
                    _ => ca::KIND_FILE,
                };
                let name = self
                    .members
                    .iter()
                    .find(|m| m.id == a.created_by)
                    .map(|m| m.name.clone())
                    .unwrap_or_default();
                let icon = match a.kind {
                    ArtifactKind::Page => IconName::Globe,
                    _ => IconName::FileText,
                };
                let (id, id2) = (a.id.clone(), a.id.clone());
                let provider = provider.clone();
                div()
                    .id(ElementId::Name(SharedString::from(format!("panel-artifact-{}", a.id))))
                    .py(px(4.))
                    .px(px(4.))
                    .rounded(px(6.))
                    .hover(|s| s.bg(colors.row_hover))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(Icon::new(icon).text_color(colors.accent).flex_none())
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(div().text_sm().truncate().child(a.title.clone()))
                            .child(
                                div()
                                    .flex()
                                    .gap(px(6.))
                                    .text_xs()
                                    .text_color(colors.muted_foreground)
                                    .child(kind)
                                    .child(fill(p::VERSION, &[("n", &a.version.to_string())]))
                                    .when(!name.is_empty(), |el| el.child(name))
                                    .child(time_text(a.updated_at)),
                            ),
                    )
                    .child(
                        Button::new(ElementId::Name(SharedString::from(format!(
                            "panel-artifact-version-{}",
                            a.id
                        ))))
                        .xsmall()
                        .ghost()
                        .icon(IconName::Plus)
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
                div()
                    .py(px(2.))
                    .flex()
                    .flex_col()
                    .child(file_row_in(
                        &g,
                        &t,
                        &tf.file,
                        &provider,
                        true,
                        |part| ElementId::Name(SharedString::from(format!("{key}-{part}"))),
                        cx,
                    ))
                    .into_any_element()
            })
            .collect();

        div()
            .flex()
            .flex_col()
            .child(header)
            .when_some(creating, |el, state| match state {
                TransferState::Running { .. } => el.child(
                    div()
                        .py(px(4.))
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child(fill(ca::CREATING, &[("p", &format!("{:.0}", state.percent()))])),
                        )
                        .child(Progress::new("artifact-create-progress").value(state.percent())),
                ),
                _ => el,
            })
            .when(rows.is_empty(), |el| {
                el.child(
                    div()
                        .text_color(colors.muted_foreground)
                        .child(p::ARTIFACTS_EMPTY),
                )
            })
            .children(rows)
            .child(section(f::PANEL_FILES))
            .when(files.is_empty(), |el| {
                el.child(
                    div()
                        .text_color(colors.muted_foreground)
                        .child(f::PANEL_FILES_EMPTY),
                )
            })
            .children(files)
    }
}
