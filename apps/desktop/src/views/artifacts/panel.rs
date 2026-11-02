//! The 产出 and files of the open topic, in the side panel. A 产出 opens on
//! click; 上传产出 picks an HTML file or a folder and uploads it as a new
//! 产出, and each 产出 can get a new version the same way.

use std::path::PathBuf;

use gpui::{
    AsyncApp, Context, ElementId, InteractiveElement as _, IntoElement, ParentElement,
    PathPromptOptions, Render, SharedString, StatefulInteractiveElement as _, Styled, Subscription,
    WeakEntity, Window, div, prelude::FluentBuilder as _, px, relative,
};
use gpui_component::{Disableable as _, notification::Notification};

use super::window;
use crate::ui::NamedButton as _;
use crate::{
    app::{Changes, Services},
    copy::{artifacts as ca, files as f, fill, panel as p},
    data::{
        ArtifactKind, ArtifactSummary, Change, FileError, GroupId, Member, NewArtifactRequest,
        TopicFile, TopicId, TransferState, create_key,
    },
    theme::{self, Type as _, space},
    ui::{self, Lucide},
    views::{
        files::{PANEL_ROW_H, error_text, extra::file_row_in, notify, notify_error, size_text},
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
                        &[
                            ("title", &created.title),
                            ("n", &created.version.to_string()),
                        ],
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
        let k = theme::colors(cx);
        let Some((g, t)) = self.open.clone() else {
            return div();
        };
        let provider = self.services.provider.clone();
        let creating = provider.file_transfer(&create_key(&t));
        let busy = matches!(creating, Some(TransferState::Running { .. }));

        // `产出 · 2` with the upload button at the right (§8.1 sections).
        let header = div()
            .h(px(20.))
            .mb(space::S4)
            .flex()
            .items_center()
            .child(div().flex_1().min_w(px(0.)).child(ui::panel_label(
                p::ARTIFACTS,
                Some(self.artifacts.len() as u32),
                cx,
            )))
            .child(
                ui::icon_button_sm("artifact-new", Lucide::Plus)
                    .named(ca::NEW)
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, _, cx| this.pick_and_upload(None, cx))),
            );

        // 48 pt rows (§7.11): a 32 pt `accent-soft` tile, the title 14/500,
        // `网页 · v3 · 今天 14:31` 12/16 `text-2`; a new version on hover.
        let rows: Vec<_> = self
            .artifacts
            .iter()
            .map(|a| {
                let (kind, glyph) = match a.kind {
                    ArtifactKind::Page => (ca::PANEL_KIND_PAGE, Lucide::FileCode),
                    ArtifactKind::Image => (f::KIND_IMAGE, Lucide::FileImage),
                    _ => (ca::KIND_FILE, Lucide::FileText),
                };
                // A page shows its version and time (`网页 · v3 · 今天 14:31`);
                // a first-version image its size (`图片 · 2.4 MB`).
                let meta = match a.size_bytes {
                    Some(bytes) if a.kind == ArtifactKind::Image && a.version <= 1 => {
                        format!("{kind} · {}", size_text(bytes))
                    }
                    _ => format!(
                        "{kind} · {} · {}",
                        fill(ca::VERSION_SHORT, &[("n", &a.version.to_string())]),
                        day_time_text(a.updated_at)
                    ),
                };
                let author = self
                    .members
                    .iter()
                    .find(|m| m.id == a.created_by)
                    .map(|m| m.name.clone())
                    .unwrap_or_default();
                let (id, id2) = (a.id.clone(), a.id.clone());
                let group = SharedString::from(format!("panel-artifact-{}", a.id));
                let provider = provider.clone();
                div()
                    .id(ElementId::Name(group.clone()))
                    .group(group.clone())
                    .h(PANEL_ROW_H)
                    .mx(-space::S8)
                    .px(space::S8)
                    .rounded(theme::radius::MD)
                    .hover(|s| s.bg(k.hover))
                    .active(|s| s.bg(k.pressed))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(space::S12)
                    .child(
                        div()
                            .flex_none()
                            .size(px(32.))
                            .rounded(theme::radius::MD)
                            .bg(k.accent_soft)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(ui::icon(glyph, theme::icon::ROW, k.accent_text)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .t_ui()
                                    .medium()
                                    .text_color(k.text_1)
                                    .one_line()
                                    .child(a.title.clone()),
                            )
                            .child(
                                div()
                                    .t_meta()
                                    .tnum()
                                    .text_color(k.text_2)
                                    .one_line()
                                    .child(meta),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .invisible()
                            .group_hover(group, |s| s.visible())
                            .child(
                                ui::icon_button_sm(
                                    ElementId::Name(SharedString::from(format!(
                                        "panel-artifact-version-{}",
                                        a.id
                                    ))),
                                    Lucide::Plus,
                                )
                                .named(ca::NEW_VERSION)
                                .disabled(busy)
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.pick_and_upload(Some(id2.clone()), cx)
                                    },
                                )),
                            ),
                    )
                    .tooltip(move |window, cx| {
                        gpui_component::tooltip::Tooltip::new(author.clone()).build(window, cx)
                    })
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
        let n_files = files.len() as u32;

        // An empty 产出 section is left out (the side panel shows no filler).
        let show_artifacts = !rows.is_empty() || busy;
        div()
            .flex()
            .flex_col()
            .gap(space::S16)
            .when(show_artifacts, |el| {
                el.child(
                    div()
                        .flex()
                        .flex_col()
                        .child(header)
                        .when_some(creating, |el, state| match state {
                            TransferState::Running { .. } => el.child(
                                div()
                                    .py(space::S4)
                                    .flex()
                                    .flex_col()
                                    .gap(space::S6)
                                    .child(div().t_meta().tnum().text_color(k.text_2).child(fill(
                                        ca::CREATING,
                                        &[("p", &format!("{:.0}", state.percent()))],
                                    )))
                                    .child(
                                        div().h(px(3.)).rounded(px(1.5)).bg(k.field).child(
                                            div()
                                                .h_full()
                                                .w(relative((state.percent() / 100.).clamp(0., 1.)))
                                                .rounded(px(1.5))
                                                .bg(k.accent),
                                        ),
                                    ),
                            ),
                            _ => el,
                        })
                        .children(rows),
                )
            })
            .when(n_files > 0, |el| {
                el.child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .h(px(20.))
                                .mb(space::S4)
                                .flex()
                                .items_center()
                                .child(ui::panel_label(f::PANEL_FILES, Some(n_files), cx)),
                        )
                        .children(files),
                )
            })
    }
}

/// A 产出's time (§8.1 `网页 · v3 · 今天 14:31`): today spells `今天` and the
/// time, so it reads the same as the message that posted it; other days as
/// in lists.
fn day_time_text(at: crate::data::Millis) -> String {
    use chrono::{Local, TimeZone as _};
    let Some(then) = Local.timestamp_millis_opt(at).single() else {
        return String::new();
    };
    if then.date_naive() == Local::now().date_naive() {
        format!("{} {}", crate::copy::timeline::TODAY, then.format("%H:%M"))
    } else {
        time_text(at)
    }
}
