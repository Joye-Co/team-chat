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
    AnyElement, App, Context, InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::ActiveTheme as _;

use crate::{
    app::{Changes, Services},
    copy::{fill, overlays as o, process as p},
    data::{
        AdminError, BotRunSummary, Change, GroupId, Member, Millis, RunLoadError, RunProcess,
        RunStatus, TopicId,
    },
    theme::{self, Type as _, space},
    ui::{
        self, Sym,
        native::{
            PushSize, PushStyle, StatusKind, disabled_push, label_value, native_colors, push_button,
        },
    },
    views::shell::{clock, clock_range},
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

/// "42 秒", "3 分钟", "1 小时 2 分钟": the one duration format of the
/// run log, the live pane and the inspector.
pub fn duration_text(ms: u64) -> String {
    let secs = ms / 1000;
    if secs < 60 {
        fill(p::DURATION_SECONDS, &[("n", &secs.to_string())])
    } else if secs < 3600 {
        fill(p::DURATION_MINUTES, &[("m", &(secs / 60).to_string())])
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

/// `停止` (NATIVE.md §7.2): asks in a Tahoe alert in the active window
/// (停止 in red, a Standard 取消), then stops the run with [`stop_run_now`].
pub fn stop_run(services: &Services, run: &BotRunSummary, cx: &mut App) {
    let Some(window) = cx.active_window() else {
        stop_run_now(services, run, cx);
        return;
    };
    let (s, r) = (services.clone(), run.clone());
    let _ = window.update(cx, |_, window, cx| confirm_stop(&s, &r, window, cx));
}

/// The 停止 alert in `window`.
pub fn confirm_stop(services: &Services, run: &BotRunSummary, window: &mut Window, cx: &mut App) {
    let name = services
        .provider
        .member(&run.bot_id)
        .map(|m| m.name)
        .unwrap_or_default();
    let (s, r) = (services.clone(), run.clone());
    crate::views::nav::dialogs::destructive_alert(
        "stop-run",
        fill(o::STOP_TITLE, &[("name", &name)]),
        fill(o::STOP_MESSAGE, &[("name", &name)]),
        o::STOP_OK,
        move |_, cx| stop_run_now(&s, &r, cx),
        window,
        cx,
    )
}

/// Asks the provider to stop a run. The views show `正在停止…` from
/// `ChatProvider::run_stopping` until the run ends. A run that already ended
/// answers `Ok`; any other failure blocks and opens an alert (no toast).
pub fn stop_run_now(services: &Services, run: &BotRunSummary, cx: &mut App) {
    let reply = services
        .provider
        .cancel_run(&run.group_id, &run.topic_id, &run.id);
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        let result = reply.await.unwrap_or(Err(AdminError::Other));
        if let Err(e) = result {
            cx.update(|cx| {
                if let Some(window) = cx.active_window() {
                    let _ = window.update(cx, |_, window, cx| {
                        crate::views::nav::dialogs::error_alert(
                            o::STOP_REFUSED,
                            stop_error_text(e),
                            |_, _| {},
                            window,
                            cx,
                        )
                    });
                }
            });
        }
    })
    .detach();
}

/// Why a stop failed, for its alert.
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
    /// Shows its own 工作记录 header (off when a pane's toolbar row names it).
    titled: bool,
    _subscriptions: Vec<Subscription>,
}

impl ProcessView {
    /// A pane's toolbar row names the view: no 工作记录 header inside.
    pub fn set_titled(&mut self, titled: bool) {
        self.titled = titled;
    }

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
            titled: true,
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
        // The one date format of the pane (`10:21`, `11-01`), as the facts.
        let when = clock(run.started_at.unwrap_or(run.created_at));
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
        .child(run_badge(
            run,
            self.services.provider.run_stopping(&run.id),
            cx,
        ))
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
        // The facts as DescriptionList rows (§2.5): one label column, one
        // value per row, so a fact never wraps onto a `·` of its own.
        // F3: a cut value shows the whole value in its tooltip.
        let value = |text: String| {
            let id = SharedString::from(format!("run-fact-{}-{}", run.id, text.len()));
            crate::ui::truncate::TruncText::new(text).id(id).tnum()
        };
        let ends_with_summary = self.process.steps.iter().any(|s| {
            matches!(
                s,
                crate::data::ProcessStep::Done {
                    summary: Some(_),
                    ..
                }
            )
        });
        let mut facts: Vec<AnyElement> = Vec::new();
        if let Some(name) = run.machine_name.clone().or_else(|| run.machine_id.clone()) {
            facts.push(label_value(p::FACT_MACHINE, value(name), cx).into_any_element());
        }
        if let Some(by) = run.requested_by.as_deref().and_then(|id| self.member(id)) {
            facts.push(
                label_value(p::FACT_REQUESTED_BY, value(by.name.clone()), cx).into_any_element(),
            );
        }
        if let Some(at) = run.started_at {
            // One line for the span (`11-01 10:22–10:28`), the start alone
            // while it runs.
            let (label, text) = match run.ended_at {
                Some(end) => (p::FACT_TIME, clock_range(at, end)),
                None => (p::FACT_STARTED, clock(at)),
            };
            facts.push(label_value(label, value(text), cx).into_any_element());
        }
        if let Some(ms) = run_duration(run, now) {
            facts.push(
                label_value(p::FACT_DURATION, value(duration_text(ms)), cx).into_any_element(),
            );
        }
        div()
            .pt(space::S8)
            .flex()
            .flex_col()
            .gap(space::S6)
            .child(div().flex().flex_col().children(facts))
            // The end step below repeats the summary once the log has it.
            .when_some(
                run.summary.clone().filter(|_| !ends_with_summary),
                |el, summary| el.child(div().t_body().text_color(colors.text_1).child(summary)),
            )
            .when(developer, |el| {
                let time = |t: Option<Millis>| match t.and_then(crate::views::shell::known) {
                    Some(t) => clock(t),
                    None => p::DEV_NONE.to_string(),
                };
                let mut rows: Vec<(&'static str, String)> = vec![
                    (p::DEV_RUN_ID, run.id.clone()),
                    (p::DEV_MEMBER_ID, run.bot_id.clone()),
                    (
                        p::DEV_MACHINE_ID,
                        run.machine_id.clone().unwrap_or_else(|| p::DEV_NONE.into()),
                    ),
                    (p::DEV_STATUS, status_text(run.status).to_string()),
                    (p::DEV_CREATED, time(Some(run.created_at))),
                    (p::DEV_STARTED, time(run.started_at)),
                    (p::DEV_ENDED, time(run.ended_at)),
                    (p::DEV_UPDATED, time(Some(run.updated_at))),
                    (p::DEV_LOG_SEQ, self.process.last_seq.to_string()),
                ];
                if let Some((id, runtime)) = self.process.session.clone() {
                    rows.push((p::DEV_RECORD, format!("{id} ({runtime})")));
                }
                el.child(developer_panel(rows, cx))
            })
    }

    /// A run that ended failed (§2.15, R2-07): one Alert in the pane, in
    /// place of the end step: the cause once (the time is the 用时 fact),
    /// 重试 (primary sm) and 查看日志 (outline sm, the developer view with
    /// the raw log).
    fn failure_alert(
        &self,
        run: &BotRunSummary,
        developer: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !matches!(run.status, RunStatus::Failed | RunStatus::Blocked) {
            return None;
        }
        // The thread's alert reads the same cause (`timeline/plan.rs`).
        let cause = run
            .summary
            .clone()
            .or_else(|| steps::error_line(&self.process.steps))
            .map(|c| c.trim().trim_end_matches(['。', '.']).to_string())
            .filter(|c| !c.is_empty());
        let title = fill(p::STEP_DONE, &[("status", status_text(run.status))]);
        let retry = {
            let (bot, id) = (run.bot_id.clone(), run.id.clone());
            push_button(
                "process-run-retry",
                p::RETRY,
                PushStyle::Default,
                PushSize::Small,
                cx,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                let rx = this.services.provider.retry_member_problem(&bot, &id);
                cx.spawn(async move |_, _| {
                    let _ = rx.await;
                })
                .detach();
                cx.notify();
            }))
        };
        let log = (!developer).then(|| {
            push_button(
                "process-run-log",
                crate::copy::timeline::RUN_VIEW_LOG,
                PushStyle::Outline,
                PushSize::Small,
                cx,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.services
                    .nav
                    .update(cx, |nav, cx| nav.set_developer_view(true, cx));
            }))
        });
        let mut alert = ui::native::Alert::new(Sym::Failed, cause.unwrap_or(title))
            .icon_color(native_colors(cx).red)
            .description(p::FAILED_HINT)
            .action(retry);
        if let Some(log) = log {
            alert = alert.action(log);
        }
        Some(div().pt(space::S12).child(alert).into_any_element())
    }

    fn render_developer_tail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let mono = cx.theme().mono_font_family.clone();
        let download_line = match &self.download {
            Download::Idle => None,
            Download::Running => Some(p::DEV_DOWNLOADING.to_string()),
            Download::Saved(path) => Some(fill(
                p::DEV_DOWNLOADED,
                &[("path", &path.display().to_string())],
            )),
            Download::Failed(e) => Some(fill(
                p::DEV_DOWNLOAD_FAILED,
                &[("error", load_error_text(*e))],
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
                // One line per event, cut with `…` at the frame: raw JSON
                // has no break points and would run past the pane.
                div()
                    .py(space::S2)
                    .min_w_0()
                    .one_line()
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
                disabled_push(
                    push_button(
                        "download-session",
                        p::DEV_DOWNLOAD,
                        PushStyle::Outline,
                        PushSize::Small,
                        cx,
                    ),
                    matches!(self.download, Download::Running),
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| this.download_session(cx))),
            )
            .when_some(download_line, |el, line| {
                el.child(div().text_color(colors.text_2).child(line))
            })
            .child(div().pt(space::S6).text_color(colors.text_3).child(fill(
                p::DEV_RAW,
                &[("n", &self.process.raw.len().to_string())],
            )))
            .child(
                div()
                    .min_w_0()
                    .font_family(mono)
                    .text_color(colors.text_2)
                    .children(raw),
            )
    }
}

/// The developer panel of a run (§2.19): a framed DescriptionList with a
/// 40 pt header (its title and a 28 copy button); localized labels, values
/// in 13 pt monospace that truncate at the frame.
fn developer_panel(rows: Vec<(&'static str, String)>, cx: &App) -> AnyElement {
    let colors = theme::colors(cx);
    let n = native_colors(cx);
    let text = rows
        .iter()
        .map(|(l, v)| format!("{l}: {v}"))
        .collect::<Vec<_>>()
        .join("\n");
    // R2-07, F4: no inner inset, so its values share the one value x of
    // the facts above; a rule above its header sets it apart.
    div()
        .mt(space::S4)
        .border_t_1()
        .border_color(n.separator)
        .child(
            div()
                .h(ui::native::tokens::height::FOOTER_ROW)
                .flex()
                .items_center()
                .border_b_1()
                .border_color(n.separator)
                .child(
                    div()
                        .flex_1()
                        .t_meta_medium()
                        .text_color(n.secondary)
                        .child(p::DEV_TITLE),
                )
                .child(
                    ui::native::icon_button_sym(
                        "run-details-copy",
                        Sym::Copy,
                        p::DEV_COPY,
                        ui::native::IconSize::Small,
                        cx,
                    )
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()))
                    }),
                ),
        )
        .child(
            div()
                .py(space::S4)
                .children(rows.into_iter().map(|(label, v)| {
                    label_value(
                        label,
                        div()
                            .min_w_0()
                            .one_line()
                            .t_code()
                            .text_color(colors.text_1)
                            .child(v),
                        cx,
                    )
                })),
        )
        .into_any_element()
}

/// The status of a run as a status badge (COMPLETENESS §2.12): one
/// mapping in the run list, the pane headers and the inspector. A run that
/// is being stopped says 停止中.
pub fn status_badge(status: RunStatus, cx: &App) -> impl IntoElement {
    ui::native::status_badge(status_kind(status), status_text(status), cx)
}

/// The badge kind of a run status.
pub fn status_kind(status: RunStatus) -> StatusKind {
    match status {
        RunStatus::Created => StatusKind::Queued,
        RunStatus::Running => StatusKind::Working,
        RunStatus::Waiting => StatusKind::NeedsYou,
        RunStatus::Blocked | RunStatus::Failed => StatusKind::Failed,
        RunStatus::Done => StatusKind::Done,
        RunStatus::Cancelled => StatusKind::Idle,
    }
}

/// The badge of a run, 停止中 while a stop is on its way.
pub fn run_badge(run: &BotRunSummary, stopping: bool, cx: &App) -> AnyElement {
    if stopping && run.status.is_active() {
        ui::native::status_badge(StatusKind::Stopping, p::STATUS_STOPPING, cx).into_any_element()
    } else {
        status_badge(run.status, cx).into_any_element()
    }
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
            // A compact Empty (§2.13): icon and one 14 muted line.
            return body
                .pt(space::S16)
                .child(ui::native::EmptyLine::new(p::EMPTY).icon(Sym::Terminal))
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
            // Loading draws skeleton rows below, not a line of text.
            (None, false, _) => None,
            (None, true, true) => Some((p::NO_STEPS, false)),
            _ => None,
        };
        let titled = self.titled;
        let loading = self.process.error.is_none() && !self.process.loaded;
        body.when(titled, |el| el.child(section(p::RUNS, cx)))
            .when(!titled, |el| el.pt(space::S16))
            // R2-07: the run rows sit inside the pane padding (x 1056), so
            // the selected band lines up with the facts below.
            .child(div().flex().flex_col().children(rows))
            .when_some(run, |el, run| {
                el.child(self.render_run_details(&run, developer, cx))
                    .child(section(p::STEPS, cx))
                    .children(steps)
                    .children(self.failure_alert(&run, developer, cx))
                    .when(loading, |el| {
                        el.child(ui::native::skeleton_rows(
                            ui::native::SkeletonKind::ListRow,
                            3,
                        ))
                    })
                    .when_some(state_line, |el, (text, retry)| {
                        el.child(
                            div()
                                .py(space::S6)
                                .flex()
                                .items_center()
                                .gap(space::S8)
                                .t_small()
                                .text_color(if retry { colors.text_2 } else { colors.text_3 })
                                .when(retry, |el| {
                                    el.child(ui::icon(
                                        ui::Sym::Alert,
                                        theme::icon::CHIP,
                                        colors.danger,
                                    ))
                                })
                                .child(text)
                                .when(retry, |el| {
                                    let id = run.id.clone();
                                    el.child(
                                        push_button(
                                            "process-retry",
                                            p::RETRY,
                                            PushStyle::Outline,
                                            PushSize::Small,
                                            cx,
                                        )
                                        .on_click(
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
        assert_eq!(duration_text(185_000), "3 分钟");
        assert_eq!(duration_text(3_720_000), "1 小时 2 分钟");
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
    fn failed_and_stuck_runs_badge_as_failed() {
        assert_eq!(status_kind(RunStatus::Failed), StatusKind::Failed);
        assert_eq!(status_kind(RunStatus::Blocked), StatusKind::Failed);
        assert_eq!(status_kind(RunStatus::Running), StatusKind::Working);
        assert_eq!(status_kind(RunStatus::Done), StatusKind::Done);
        assert_eq!(status_kind(RunStatus::Cancelled), StatusKind::Idle);
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
