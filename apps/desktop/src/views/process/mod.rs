//! The process view (过程) of the side panel: the runs of bot members in the
//! open topic (status, where they run, times) and the steps of the selected
//! run: messages, commands and their results, file changes and approvals.
//! While the panel shows an active run it asks the provider for new log
//! entries every [`POLL`]. The developer view adds ids, the raw log entries
//! and the raw session download.

pub mod preview;
mod steps;

pub use steps::{error_line, glyph as step_glyph, step_line};

use std::{path::PathBuf, time::Duration};

use gpui::{
    App, Context, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{ActiveTheme as _, Disableable as _};

use crate::{
    app::{Changes, Services},
    copy::{dev, fill, process as p},
    data::{
        AdminError, BotRunSummary, Change, GroupId, Member, Millis, RunLoadError, RunProcess,
        RunStatus, TopicId,
    },
    theme::{self, Type as _, space},
    ui,
    views::time_text,
};

/// How often an active run's log is fetched while the view shows it.
pub const POLL: Duration = Duration::from_secs(2);

/// The words of a run status.
pub fn status_text(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Created => p::STATUS_CREATED,
        RunStatus::Running => p::STATUS_RUNNING,
        RunStatus::Waiting => p::STATUS_WAITING,
        RunStatus::Blocked => p::STATUS_BLOCKED,
        RunStatus::Done => p::STATUS_DONE,
        RunStatus::Failed => p::STATUS_FAILED,
        RunStatus::Cancelled => p::STATUS_CANCELLED,
    }
}

/// "42 秒", "3 分 5 秒", "1 小时 2 分".
pub fn duration_text(ms: u64) -> String {
    let secs = ms / 1000;
    if secs < 60 {
        fill(p::DURATION_SECONDS, &[("n", &secs.to_string())])
    } else if secs < 3600 {
        fill(
            p::DURATION_MINUTES,
            &[
                ("m", &(secs / 60).to_string()),
                ("s", &(secs % 60).to_string()),
            ],
        )
    } else {
        fill(
            p::DURATION_HOURS,
            &[
                ("h", &(secs / 3600).to_string()),
                ("m", &(secs / 60 % 60).to_string()),
            ],
        )
    }
}

/// How long a run took, or has run so far.
pub fn run_duration(run: &BotRunSummary, now: Millis) -> Option<u64> {
    let start = run.started_at?;
    let end = run.ended_at.unwrap_or(if run.status.is_active() {
        now
    } else {
        run.updated_at
    });
    Some(end.saturating_sub(start).max(0) as u64)
}

/// Asks the provider to stop a run (`停止`, DESIGN.md §9.2). No confirmation:
/// a stop loses nothing. The views show `正在停止…` from
/// `ChatProvider::run_stopping` until the run ends. A run that already ended
/// answers `Ok`; any other failure shows the standard error toast.
pub fn stop_run(services: &Services, run: &BotRunSummary, cx: &mut App) {
    let reply = services
        .provider
        .cancel_run(&run.group_id, &run.topic_id, &run.id);
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        let result = reply.await.unwrap_or(Err(AdminError::Other));
        if let Err(e) = result {
            cx.update(|cx| {
                crate::views::files::notify_error(
                    fill(p::STOP_FAILED, &[("reason", stop_error_text(e))]),
                    cx,
                )
            });
        }
    })
    .detach();
}

/// Why a stop failed, for the toast.
pub fn stop_error_text(e: AdminError) -> &'static str {
    match e {
        // Only people in the run's 群 stop runs; the server says forbidden.
        AdminError::Forbidden => p::STOP_FORBIDDEN,
        other => crate::views::settings::widgets::error_text(other),
    }
}

/// The member may stop this run from its pending row: the person who asked.
pub fn asked_by(run: &BotRunSummary, me: Option<&str>) -> bool {
    run.requested_by.is_some() && run.requested_by.as_deref() == me
}

fn load_error_text(error: RunLoadError) -> &'static str {
    match error {
        RunLoadError::Offline => p::OFFLINE,
        RunLoadError::NotFound => p::NOT_FOUND,
        RunLoadError::Forbidden => p::FORBIDDEN,
        RunLoadError::Failed | RunLoadError::Unavailable => p::LOAD_FAILED,
    }
}

enum Download {
    Idle,
    Running,
    Saved(PathBuf),
    Failed(RunLoadError),
}

pub struct ProcessView {
    services: Services,
    open: Option<(GroupId, TopicId)>,
    runs: Vec<BotRunSummary>,
    selected: Option<String>,
    process: RunProcess,
    members: Vec<Member>,
    /// The side panel shows this view.
    visible: bool,
    /// The run the poll loop follows.
    polling: Option<String>,
    download: Download,
    _subscriptions: Vec<Subscription>,
}

impl ProcessView {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.subscribe(&services.hub, |this, _, changes: &Changes, cx| {
                this.on_changes(changes, cx)
            }),
            cx.observe(&services.nav, |this, nav, cx| {
                let open = nav.read(cx).topic.clone();
                if open != this.open {
                    this.open_topic(open, cx);
                }
                cx.notify();
            }),
        ];
        let open = services.nav.read(cx).topic.clone();
        let mut this = Self {
            members: services.provider.members(),
            services,
            open: None,
            runs: Vec::new(),
            selected: None,
            process: RunProcess::default(),
            visible: false,
            polling: None,
            download: Download::Idle,
            _subscriptions: subscriptions,
        };
        this.open_topic(open, cx);
        this
    }

    /// Runs of the open topic (for tests and the side panel).
    pub fn runs(&self) -> &[BotRunSummary] {
        &self.runs
    }

    pub fn selected_process(&self) -> &RunProcess {
        &self.process
    }

    /// The side panel shows (or hides) this view. Hidden, it stops polling.
    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        if visible && let Some(id) = self.selected.clone() {
            self.services.provider.follow_run(&id);
            self.ensure_polling(cx);
        }
    }

    fn open_topic(&mut self, open: Option<(GroupId, TopicId)>, cx: &mut Context<Self>) {
        self.open = open;
        self.selected = None;
        self.process = RunProcess::default();
        self.download = Download::Idle;
        self.refresh_runs(cx);
    }

    fn on_changes(&mut self, changes: &Changes, cx: &mut Context<Self>) {
        let mut dirty = false;
        if changes.any(|c| matches!(c, Change::Members)) {
            self.members = self.services.provider.members();
            dirty = true;
        }
        if let Some((g, t)) = self.open.clone()
            && changes.any(|c| {
                matches!(c, Change::Runs { group_id, topic_id } if *group_id == g && *topic_id == t)
            })
        {
            self.refresh_runs(cx);
            dirty = true;
        }
        if let Some(id) = self.selected.clone()
            && changes.any(|c| matches!(c, Change::RunLog { run_id } if *run_id == id))
        {
            self.process = self.services.provider.run_process(&id);
            dirty = true;
        }
        if dirty {
            cx.notify();
        }
    }

    fn refresh_runs(&mut self, cx: &mut Context<Self>) {
        let old_status = self.selected_run().map(|r| r.status);
        self.runs = match &self.open {
            Some((g, t)) => self.services.provider.runs(g, t),
            None => Vec::new(),
        };
        let keep = self
            .selected
            .as_ref()
            .is_some_and(|id| self.runs.iter().any(|r| &r.id == id));
        if !keep {
            // Newest run first.
            let first = self.runs.first().map(|r| r.id.clone());
            self.select(first, cx);
        } else if let (Some(run), Some(old)) = (self.selected_run(), old_status)
            && run.status != old
        {
            // Status changed (for example the run ended): fetch the last entries.
            let id = run.id.clone();
            self.services.provider.follow_run(&id);
            self.ensure_polling(cx);
        }
    }

    /// The run the view shows.
    pub fn selected_run(&self) -> Option<&BotRunSummary> {
        let id = self.selected.as_ref()?;
        self.runs.iter().find(|r| &r.id == id)
    }

    fn select(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        if self.selected == id {
            return;
        }
        self.selected = id.clone();
        self.download = Download::Idle;
        self.process = match &id {
            Some(id) => {
                self.services.provider.follow_run(id);
                self.services.provider.run_process(id)
            }
            None => RunProcess::default(),
        };
        self.ensure_polling(cx);
        cx.notify();
    }

    fn should_poll(&self, id: &str) -> bool {
        self.visible
            && self.selected.as_deref() == Some(id)
            && self.selected_run().is_some_and(|r| r.status.is_active())
    }

    /// Starts the poll loop for the selected run when it is active and shown.
    fn ensure_polling(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        if self.polling.as_deref() == Some(id.as_str()) || !self.should_poll(&id) {
            return;
        }
        self.polling = Some(id.clone());
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let go_on = this.update(cx, |this, _| {
                    if this.polling.as_deref() != Some(id.as_str()) {
                        return false;
                    }
                    if this.should_poll(&id) {
                        this.services.provider.follow_run(&id);
                        true
                    } else {
                        this.polling = None;
                        false
                    }
                });
                if !matches!(go_on, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    fn download_session(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        let dir = std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join("Downloads"))
            .unwrap_or_else(std::env::temp_dir);
        let reply = self.services.provider.download_run_session(&id, dir);
        self.download = Download::Running;
        cx.spawn(async move |this, cx| {
            let result = reply.await.unwrap_or(Err(RunLoadError::Failed));
            let _ = this.update(cx, |this, cx| {
                if this.selected.as_deref() == Some(id.as_str()) {
                    this.download = match result {
                        Ok(path) => Download::Saved(path),
                        Err(e) => Download::Failed(e),
                    };
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn member(&self, id: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.id == id)
    }

    /// One run: a 40 pt row (§8.1 rows), the 24 pt bot, its name `ui` 14,
    /// the status label and the start time 12 `text-3`.
    fn render_run_row(&self, run: &BotRunSummary, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let bot = self.member(&run.bot_id).cloned();
        let selected = self.selected.as_deref() == Some(run.id.as_str());
        let id = run.id.clone();
        let when = time_text(run.started_at.unwrap_or(run.created_at));
        ui::list_row(
            gpui::ElementId::Name(format!("run-{}", run.id).into()),
            ui::RowSurface::Content,
            selected,
            cx,
        )
        .h(theme::size::PANEL_ROW)
        .px(space::S8)
        .gap(space::S10)
        .on_click(cx.listener(move |this, _, _, cx| this.select(Some(id.clone()), cx)))
        .child(
            ui::MemberAvatar::new(bot.as_ref(), theme::size::AVATAR_ROW)
                .selected(selected)
                .on(if selected { k.selected } else { k.content }),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .t_ui()
                .text_color(k.text_1)
                .when(selected, |el| el.semibold())
                .one_line()
                .child(bot.map(|b| b.name).unwrap_or_default()),
        )
        .child(status_badge(run.status, cx))
        .child(
            div()
                .flex_none()
                .t_meta()
                .tnum()
                .text_color(k.text_3)
                .child(when),
        )
    }

    fn render_run_details(
        &self,
        run: &BotRunSummary,
        developer: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = theme::colors(cx);
        let now = crate::selftest::clock::utc_now().timestamp_millis();
        let mut facts: Vec<String> = Vec::new();
        if let Some(name) = run.machine_name.clone().or_else(|| run.machine_id.clone()) {
            facts.push(fill(p::RUN_ON, &[("name", &name)]));
        }
        if let Some(by) = run.requested_by.as_deref().and_then(|id| self.member(id)) {
            facts.push(fill(p::REQUESTED_BY, &[("name", &by.name)]));
        }
        if let Some(at) = run.started_at {
            facts.push(fill(p::STARTED_AT, &[("time", &time_text(at))]));
        }
        if let Some(at) = run.ended_at {
            facts.push(fill(p::ENDED_AT, &[("time", &time_text(at))]));
        }
        if let Some(ms) = run_duration(run, now) {
            facts.push(fill(p::DURATION, &[("d", &duration_text(ms))]));
        }
        let live = run.status.is_active() && self.visible;
        // One `small` line of facts split by `·`, the summary in `body`.
        div()
            .pt(space::S8)
            .flex()
            .flex_col()
            .gap(space::S6)
            .child(
                div()
                    .t_small()
                    .tnum()
                    .text_color(colors.text_2)
                    .child(facts.join(" · ")),
            )
            .when(live, |el| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S6)
                        .t_meta()
                        .text_color(colors.accent_text)
                        .child(div().size(px(6.)).rounded_full().bg(colors.accent))
                        .child(p::LIVE),
                )
            })
            .when_some(run.summary.clone(), |el, summary| {
                el.child(div().t_body().text_color(colors.text_1).child(summary))
            })
            .when(developer, |el| {
                let opt =
                    |t: Option<Millis>| t.map(|t| t.to_string()).unwrap_or_else(|| "-".into());
                el.child(
                    div()
                        .p(space::S8)
                        .rounded(theme::radius::MD)
                        .bg(colors.code_bg)
                        .t_code()
                        .text_color(colors.text_2)
                        .child(div().child(fill(dev::RUN_ID, &[("id", &run.id)])))
                        .child(div().child(fill(dev::RUN_BOT, &[("id", &run.bot_id)])))
                        .child(div().child(fill(
                            dev::RUN_MACHINE,
                            &[("id", run.machine_id.as_deref().unwrap_or("-"))],
                        )))
                        .child(
                            div().child(fill(
                                dev::RUN_STATUS,
                                &[("status", run.status.wire_name())],
                            )),
                        )
                        .child(div().child(fill(
                            dev::RUN_TIMES,
                            &[
                                ("created", &run.created_at.to_string()),
                                ("started", &opt(run.started_at)),
                                ("ended", &opt(run.ended_at)),
                                ("updated", &run.updated_at.to_string()),
                            ],
                        )))
                        .child(div().child(fill(
                            dev::RUN_LAST_SEQ,
                            &[("seq", &self.process.last_seq.to_string())],
                        )))
                        .when_some(self.process.session.clone(), |el, (id, runtime)| {
                            el.child(div().child(fill(
                                dev::RUN_SESSION,
                                &[("id", &id), ("runtime", &runtime)],
                            )))
                        }),
                )
            })
    }

    fn render_developer_tail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let mono = cx.theme().mono_font_family.clone();
        let download_line = match &self.download {
            Download::Idle => None,
            Download::Running => Some(dev::RUN_DOWNLOADING.to_string()),
            Download::Saved(path) => Some(fill(
                dev::RUN_DOWNLOADED,
                &[("path", &path.display().to_string())],
            )),
            Download::Failed(e) => Some(fill(
                dev::RUN_DOWNLOAD_FAILED,
                &[("error", &format!("{e:?}"))],
            )),
        };
        let raw: Vec<_> = self
            .process
            .raw
            .iter()
            .rev()
            .take(100)
            .rev()
            .map(|e| {
                let mut text: String = e.json.chars().take(600).collect();
                if text.len() < e.json.len() {
                    text.push('…');
                }
                div()
                    .py(space::S2)
                    .border_b(theme::HAIRLINE)
                    .border_color(colors.sep)
                    .child(format!("#{} {}", e.seq, text))
            })
            .collect();
        div()
            .pt(space::S12)
            .flex()
            .flex_col()
            .gap(space::S6)
            .t_meta()
            .child(
                ui::button(
                    "download-session",
                    dev::RUN_DOWNLOAD_SESSION,
                    ui::ButtonKind::Secondary,
                    ui::ButtonSize::Sm,
                    cx,
                )
                .disabled(matches!(self.download, Download::Running))
                .on_click(cx.listener(|this, _, _, cx| this.download_session(cx))),
            )
            .when_some(download_line, |el, line| {
                el.child(div().text_color(colors.text_2).child(line))
            })
            .child(div().pt(space::S6).text_color(colors.text_3).child(fill(
                dev::RUN_RAW,
                &[("n", &self.process.raw.len().to_string())],
            )))
            .child(
                div()
                    .font_family(mono)
                    .text_color(colors.text_2)
                    .children(raw),
            )
    }
}

/// The status label of a run (§7.8): a glyph and the word, no fill.
pub fn status_badge(status: RunStatus, _cx: &App) -> impl IntoElement {
    use crate::{data::TopicStatus as S, theme::ChipKind as K};
    let (topic_status, kind) = match status {
        RunStatus::Created | RunStatus::Cancelled => (S::Open, K::Quiet),
        RunStatus::Running => (S::InProgress, K::Quiet),
        RunStatus::Waiting => (S::WaitingAnswer, K::Quiet),
        RunStatus::Blocked | RunStatus::Failed => (S::Blocked, K::Blocked),
        RunStatus::Done => (S::Done, K::Quiet),
    };
    ui::StatusChip::labeled(status_text(status), topic_status, kind).style(ui::ChipStyle::Label)
}

impl Render for ProcessView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let developer = self.services.nav.read(cx).developer_view;
        let section = |title: &'static str, cx: &App| {
            div()
                .pt(space::S16)
                .pb(space::S8)
                .child(ui::panel_label(title, None, cx))
        };
        let body = div()
            .id("process-view")
            .size_full()
            .overflow_y_scroll()
            .px(space::S16)
            .pb(space::S24);
        if self.runs.is_empty() {
            return body
                .child(
                    div()
                        .pt(space::S16)
                        .t_meta()
                        .text_color(colors.text_3)
                        .child(p::EMPTY),
                )
                .into_any_element();
        }
        let rows: Vec<_> = self
            .runs
            .iter()
            .map(|r| self.render_run_row(r, cx).into_any_element())
            .collect();
        let run = self.selected_run().cloned();
        let bot_name = run
            .as_ref()
            .and_then(|r| self.member(&r.bot_id))
            .map(|m| m.name.clone())
            .unwrap_or_default();
        let steps = steps::render_steps(&self.process, &bot_name, developer, window, cx);
        let state_line = match (
            &self.process.error,
            self.process.loaded,
            self.process.steps.is_empty(),
        ) {
            (Some(e), _, _) => Some((load_error_text(*e), true)),
            (None, false, _) => Some((p::LOADING, false)),
            (None, true, true) => Some((p::NO_STEPS, false)),
            _ => None,
        };
        body.child(section(p::RUNS, cx))
            .child(div().mx(-space::S8).flex().flex_col().children(rows))
            .when_some(run, |el, run| {
                el.child(self.render_run_details(&run, developer, cx))
                    .child(section(p::STEPS, cx))
                    .children(steps)
                    .when_some(state_line, |el, (text, retry)| {
                        el.child(
                            div()
                                .py(space::S6)
                                .flex()
                                .items_center()
                                .gap(space::S8)
                                .t_small()
                                .text_color(if retry {
                                    colors.danger_text
                                } else {
                                    colors.text_3
                                })
                                .when(retry, |el| {
                                    el.child(ui::icon(
                                        ui::Lucide::CircleAlert,
                                        theme::icon::CHIP,
                                        colors.danger,
                                    ))
                                })
                                .child(text)
                                .when(retry, |el| {
                                    let id = run.id.clone();
                                    el.child(
                                        ui::text_button("process-retry", p::RETRY, cx).on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.services.provider.follow_run(&id);
                                                cx.notify();
                                            }),
                                        ),
                                    )
                                }),
                        )
                    })
                    .when(developer, |el| el.child(self.render_developer_tail(cx)))
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_naturally() {
        assert_eq!(duration_text(0), "0 秒");
        assert_eq!(duration_text(42_900), "42 秒");
        assert_eq!(duration_text(185_000), "3 分 5 秒");
        assert_eq!(duration_text(3_720_000), "1 小时 2 分");
    }

    #[test]
    fn run_duration_uses_now_while_active() {
        let mut run = BotRunSummary {
            id: "r".into(),
            bot_id: "b".into(),
            group_id: "g".into(),
            topic_id: "t".into(),
            machine_id: None,
            machine_name: None,
            status: RunStatus::Running,
            requested_by: None,
            created_at: 0,
            started_at: Some(1_000),
            ended_at: None,
            updated_at: 2_000,
            summary: None,
            plan: Vec::new(),
        };
        assert_eq!(run_duration(&run, 11_000), Some(10_000));
        run.status = RunStatus::Failed;
        assert_eq!(run_duration(&run, 11_000), Some(1_000));
        run.ended_at = Some(5_000);
        assert_eq!(run_duration(&run, 11_000), Some(4_000));
        run.started_at = None;
        assert_eq!(run_duration(&run, 11_000), None);
    }

    #[test]
    fn only_the_person_who_asked_gets_stop_on_the_pending_row() {
        let mut run = BotRunSummary {
            id: "r".into(),
            bot_id: "b".into(),
            group_id: "g".into(),
            topic_id: "t".into(),
            machine_id: None,
            machine_name: None,
            status: RunStatus::Running,
            requested_by: Some("me".into()),
            created_at: 0,
            started_at: None,
            ended_at: None,
            updated_at: 0,
            summary: None,
            plan: Vec::new(),
        };
        assert!(asked_by(&run, Some("me")));
        assert!(!asked_by(&run, Some("other")));
        assert!(!asked_by(&run, None));
        run.requested_by = None;
        assert!(!asked_by(&run, None));
        assert_eq!(stop_error_text(AdminError::Forbidden), p::STOP_FORBIDDEN);
    }

    #[test]
    fn every_status_has_words() {
        for s in [
            RunStatus::Created,
            RunStatus::Running,
            RunStatus::Waiting,
            RunStatus::Blocked,
            RunStatus::Done,
            RunStatus::Failed,
            RunStatus::Cancelled,
        ] {
            assert!(!status_text(s).is_empty());
            assert_eq!(
                s.is_active(),
                matches!(
                    s,
                    RunStatus::Created
                        | RunStatus::Running
                        | RunStatus::Waiting
                        | RunStatus::Blocked
                )
            );
        }
    }
}
