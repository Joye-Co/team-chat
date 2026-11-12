//! The process view (过程) of the side panel: the runs of bot members in the
//! open topic (status, where they run, times) and the steps of the selected
//! run: messages, commands and their results, file changes and approvals.
//! While the panel shows an active run it asks the provider for new log
//! entries every [`POLL`]. The developer view adds ids, the raw log entries
//! and the raw session download.

pub mod preview;
mod steps;

pub use steps::{error_line, glyph as step_glyph, live_action, step_line};

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
            NativeType as _, PushSize, PushStyle, StatusKind, disabled_push, label_value,
            native_colors, push_button,
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

/// `text` with a break opportunity after every character: raw JSON has none of its own.
fn break_anywhere(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for c in text.chars() {
        out.push(c);
        if !c.is_whitespace() {
            out.push('\u{200B}');
        }
    }
    out
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

/// The model and 思考强度 the run really used (`claude-opus-5-5 · 思考强度高`); `None` until
/// the CLI said which model it runs. With no 思考强度 the CLI ran at its default: the model alone.
pub fn model_line(run: &BotRunSummary) -> Option<String> {
    let model = run.model.as_deref().filter(|m| !m.trim().is_empty())?;
    Some(
        match run.effort.as_deref().filter(|e| !e.trim().is_empty()) {
            Some(e) => fill(
                p::FACT_MODEL_EFFORT,
                &[
                    ("name", model),
                    ("effort", &crate::views::settings::members::effort_text(e)),
                ],
            ),
            None => model.to_string(),
        },
    )
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
    /// Raw events opened in the developer view (by seq): shown whole instead of one line.
    raw_open: std::collections::HashSet<u64>,
    /// Shows its own 工作记录 header (off when a pane's toolbar row names it).
    titled: bool,
    /// 全部记录 shows every run, not only the newest [`RUNS_FOLDED`].
    runs_expanded: bool,
    _subscriptions: Vec<Subscription>,
}

/// Rows 全部记录 shows while folded; more runs fold behind 看全部 n 条记录.
const RUNS_FOLDED: usize = 3;

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
            raw_open: Default::default(),
            titled: true,
            runs_expanded: false,
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
        self.runs_expanded = false;
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
        self.raw_open.clear();
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
        self.save_run_file(false, cx);
    }

    fn download_context(&mut self, cx: &mut Context<Self>) {
        self.save_run_file(true, cx);
    }

    /// Saves the run's raw session, or what its prompt carried, into ~/Downloads.
    fn save_run_file(&mut self, context: bool, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        let dir = crate::paths::downloads().unwrap_or_else(std::env::temp_dir);
        let reply = if context {
            self.services.provider.download_run_context(&id, dir)
        } else {
            self.services.provider.download_run_session(&id, dir)
        };
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

    /// One row of 全部记录 (§2.6): h40 inside the one framed list, a Tab stop with the focus
    /// ring: the 20 member, its name 14 and what came of the run (12 muted, one line), the
    /// StatusBadge and the start time 12 muted. Selected: the muted fill and the name medium
    /// (D10). Rows part with a hairline, none after the last.
    fn render_run_row(
        &self,
        run: &BotRunSummary,
        last: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        use crate::ui::native::{FocusRow as _, NativeType as _, tokens};
        let n = native_colors(cx);
        let bot = self.member(&run.bot_id).cloned();
        let selected = self.selected.as_deref() == Some(run.id.as_str());
        let id = run.id.clone();
        let when = clock(run.started_at.unwrap_or(run.created_at));
        let about = run_about(run);
        div()
            .id(gpui::ElementId::Name(format!("run-{}", run.id).into()))
            .focus_row(selected, cx)
            .min_h(space::S48)
            .py(space::S8)
            .px(space::S12)
            .when(!last, |el| el.border_b_1().border_color(n.separator))
            .when(selected, |el| el.bg(n.fill))
            .flex()
            .items_center()
            .gap(space::S12)
            .on_click(cx.listener(move |this, _, _, cx| this.select(Some(id.clone()), cx)))
            .child(
                div()
                    .flex_none()
                    .child(ui::MemberAvatar::new(bot.as_ref(), tokens::avatar::SM)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .n_body()
                            .text_color(n.label)
                            .when(selected, |el| el.font_weight(gpui::FontWeight::MEDIUM))
                            .one_line()
                            .child(bot.map(|b| b.name).unwrap_or_default()),
                    )
                    .when_some(about, |el, about| {
                        el.child(
                            div()
                                .n_footnote()
                                .text_color(n.secondary)
                                .one_line()
                                .child(about),
                        )
                    }),
            )
            .child(run_badge(
                run,
                self.services.provider.run_stopping(&run.id),
                cx,
            ))
            .child(
                div()
                    .flex_none()
                    .n_footnote()
                    .n_figures()
                    .text_color(n.secondary)
                    .child(when),
            )
    }

    /// The selected run as one card (§2.5): who did it and its StatusBadge, what it is on or
    /// what came of it, the plan's Progress while it has one, then its facts in a two-column
    /// grid (label 12 muted over value 14), so five facts take three short rows instead of
    /// five long ones.
    fn render_run_card(
        &self,
        run: &BotRunSummary,
        developer: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        use crate::ui::native::{NativeType as _, tokens};
        let n = native_colors(cx);
        let now = crate::selftest::clock::utc_now().timestamp_millis();
        let bot = self.member(&run.bot_id).cloned();
        let value = |key: &str, text: String| {
            let id = SharedString::from(format!("run-fact-{}-{key}", run.id));
            crate::ui::truncate::TruncText::new(text).id(id).tnum()
        };
        let fact = |label: &'static str, v: AnyElement, cx: &App| {
            let n = native_colors(cx);
            div()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(space::S2)
                .child(div().n_footnote().text_color(n.secondary).child(label))
                .child(div().n_body().text_color(n.label).min_w(px(0.)).child(v))
                .into_any_element()
        };
        let mut cells: Vec<AnyElement> = Vec::new();
        if let Some(at) = run.started_at {
            let (label, text) = match run.ended_at {
                Some(end) => (p::FACT_TIME, clock_range(at, end)),
                None => (p::FACT_STARTED, clock(at)),
            };
            cells.push(fact(label, value("time", text).into_any_element(), cx));
        }
        if let Some(ms) = run_duration(run, now) {
            cells.push(fact(
                p::FACT_DURATION,
                value("took", duration_text(ms)).into_any_element(),
                cx,
            ));
        }
        if let Some(name) = run.machine_name.clone().or_else(|| run.machine_id.clone()) {
            cells.push(fact(
                p::FACT_MACHINE,
                value("machine", name).into_any_element(),
                cx,
            ));
        }
        if let Some(by) = run.requested_by.as_deref().and_then(|id| self.member(id)) {
            cells.push(fact(
                p::FACT_REQUESTED_BY,
                value("by", by.name.clone()).into_any_element(),
                cx,
            ));
        }
        // The model line is the longest fact: it takes the whole width.
        let model = model_line(run)
            .map(|line| fact(p::FACT_MODEL, value("model", line).into_any_element(), cx));
        let grid = div()
            .flex()
            .flex_col()
            .gap(space::S12)
            .children(cells.chunks_mut(2).map(|pair| {
                let mut row = div().flex().gap(space::S16);
                for cell in pair.iter_mut() {
                    let cell = std::mem::replace(cell, div().into_any_element());
                    row = row.child(div().flex_1().min_w(px(0.)).child(cell));
                }
                if pair.len() == 1 {
                    row = row.child(div().flex_1());
                }
                row
            }))
            .children(model);
        let ends_with_summary = self.process.steps.iter().any(|s| {
            matches!(
                s,
                crate::data::ProcessStep::Done {
                    summary: Some(_),
                    ..
                }
            )
        });
        let progress = run
            .status
            .is_active()
            .then(|| crate::views::topic_page::plan_progress(&run.plan))
            .flatten();
        let about = if progress.is_some() {
            None
        } else if run.status.is_active() {
            steps::live_action(&self.process.steps)
                .map(|step| fill(crate::copy::timeline::PLAN_DOING, &[("step", &step)]))
        } else {
            run.summary
                .as_deref()
                .filter(|_| !ends_with_summary)
                .and_then(|s| s.lines().map(str::trim).find(|l| !l.is_empty()))
                .map(|l| crate::markdown::clip(l, 160))
        };
        let card = div()
            .rounded(tokens::radius::CONTAINER)
            .border_1()
            .border_color(n.separator)
            .flex()
            .flex_col()
            .child(
                div()
                    .p(space::S16)
                    .flex()
                    .flex_col()
                    .gap(space::S12)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space::S12)
                            .child(
                                div()
                                    .flex_none()
                                    .child(ui::MemberAvatar::new(bot.as_ref(), tokens::avatar::MD)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .n_headline()
                                    .text_color(n.label)
                                    .one_line()
                                    .child(bot.map(|b| b.name).unwrap_or_default()),
                            )
                            .child(div().flex_none().child(run_badge(
                                run,
                                self.services.provider.run_stopping(&run.id),
                                cx,
                            ))),
                    )
                    .when_some(progress, |el, (done, total, current)| {
                        el.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(space::S6)
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(space::S8)
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .n_body()
                                                .text_color(n.label)
                                                .one_line()
                                                .children(current),
                                        )
                                        .child(
                                            div()
                                                .flex_none()
                                                .n_footnote()
                                                .n_figures()
                                                .text_color(n.secondary)
                                                .child(format!("{done}/{total}")),
                                        ),
                                )
                                .child(ui::native::steps_bar(done, total, None, cx)),
                        )
                    })
                    .when_some(about, |el, about| {
                        el.child(
                            div()
                                .n_body()
                                .text_color(n.label)
                                .line_clamp(3)
                                .child(about),
                        )
                    }),
            )
            .child(
                div()
                    .px(space::S16)
                    .py(space::S12)
                    .border_t_1()
                    .border_color(n.separator)
                    .child(grid),
            );
        div()
            .flex()
            .flex_col()
            .gap(space::S12)
            .child(card)
            .when(developer, |el| el.child(self.developer_rows(run, cx)))
    }

    fn developer_rows(&self, run: &BotRunSummary, cx: &mut Context<Self>) -> AnyElement {
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
            // A2-19: the state and the start are the facts above;
            // the box adds only what they do not say.
            (p::DEV_CREATED, time(Some(run.created_at))),
            (
                p::DEV_ENDED,
                match run.ended_at {
                    None if run.status.is_active() => p::DEV_NOT_ENDED.to_string(),
                    t => time(t),
                },
            ),
            (p::DEV_UPDATED, time(Some(run.updated_at))),
            (p::DEV_LOG_SEQ, self.process.last_seq.to_string()),
        ];
        if let Some((id, runtime)) = self.process.session.clone() {
            rows.push((p::DEV_RECORD, format!("{id} ({runtime})")));
        }
        developer_panel(rows, cx)
    }

    /// A run that ended failed (§2.15, R2-07): one Alert in the pane, in
    /// place of the end step: the cause once (the time is the 用时 fact),
    /// 重试 (primary sm) and 查看日志 (outline sm, the developer view with
    /// the raw log).
    fn failure_alert(&self, run: &BotRunSummary, cx: &mut Context<Self>) -> Option<AnyElement> {
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
        // The Alert anatomy (§2.15); its actions are the pane's footer
        // (`failure_footer`). The pane is narrow, so the title and its
        // cause keep the full width.
        let n = native_colors(cx);
        // T-20, §2.15: the destructive variant tints icon and border.
        let (_, _, edge) =
            ui::native::feedback::alert_colors(ui::native::AlertTone::Destructive, &n);
        let alert = div()
            .px(space::S16)
            .py(space::S12)
            .rounded(ui::native::tokens::radius::CONTAINER)
            .border_1()
            .border_color(edge)
            .flex()
            .items_start()
            .gap(space::S12)
            .child(
                div()
                    .flex_none()
                    .h(ui::native::tokens::text::UI.1)
                    .flex()
                    .items_center()
                    .child(ui::sym(Sym::Failed, ui::SymSize::INLINE, n.red)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(space::S4)
                    .child(
                        div()
                            .n_headline()
                            .text_color(n.label)
                            .line_clamp(2)
                            .child(cause.unwrap_or(title)),
                    )
                    .child(div().n_body().text_color(n.secondary).child(p::FAILED_HINT)),
            );
        Some(div().pt(space::S12).child(alert).into_any_element())
    }

    /// The pane's footer for a run that ended failed (§2.5, A2-06): border-t,
    /// p 16, 重试 (primary) over 查看日志 (outline), full width, so the pane
    /// ends with its actions and never in a blank band.
    fn failure_footer(
        &self,
        run: &BotRunSummary,
        developer: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !matches!(run.status, RunStatus::Failed | RunStatus::Blocked) {
            return None;
        }
        let retry = {
            let (bot, id) = (run.bot_id.clone(), run.id.clone());
            push_button(
                "process-run-retry",
                p::RETRY,
                PushStyle::Default,
                PushSize::Regular,
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
                PushSize::Regular,
                cx,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.services
                    .nav
                    .update(cx, |nav, cx| nav.set_developer_view(true, cx));
            }))
        });
        let n = native_colors(cx);
        Some(
            div()
                .flex_none()
                .w_full()
                .p(space::S16)
                .border_t_1()
                .border_color(n.separator)
                .flex()
                .flex_col()
                .gap(space::S8)
                .child(retry.w_full())
                .children(log.map(|b| b.w_full()))
                .into_any_element(),
        )
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
                // One line per event, cut with `…` at the frame: raw JSON has no break
                // points and would run past the pane. A click opens the event whole (it
                // wraps anywhere) and puts it on the clipboard.
                let open = self.raw_open.contains(&e.seq);
                let limit = if open { 20_000 } else { 600 };
                let mut text: String = e.json.chars().take(limit).collect();
                if text.len() < e.json.len() {
                    text.push('…');
                }
                let (seq, json) = (e.seq, e.json.clone());
                div()
                    .id(gpui::ElementId::NamedInteger("raw-event".into(), e.seq))
                    .py(space::S2)
                    .min_w_0()
                    .cursor_pointer()
                    .border_b(theme::HAIRLINE)
                    .border_color(colors.sep)
                    .map(|el| {
                        if open {
                            el.text_color(colors.text_1)
                                .whitespace_normal()
                                .overflow_hidden()
                                .child(break_anywhere(&format!("#{} {}", e.seq, text)))
                        } else {
                            el.one_line().child(format!("#{} {}", e.seq, text))
                        }
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.raw_open.remove(&seq) {
                            this.raw_open.insert(seq);
                            cx.write_to_clipboard(gpui::ClipboardItem::new_string(json.clone()));
                        }
                        cx.notify();
                    }))
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
            .child(
                disabled_push(
                    push_button(
                        "download-context",
                        p::DEV_CONTEXT,
                        PushStyle::Outline,
                        PushSize::Small,
                        cx,
                    ),
                    matches!(self.download, Download::Running),
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| this.download_context(cx))),
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

/// What tells a run from the others in 全部记录: what came of it (the result's first line),
/// or the step it is on while it works.
fn run_about(run: &BotRunSummary) -> Option<String> {
    run.summary
        .as_deref()
        .and_then(|s| s.lines().map(str::trim).find(|l| !l.is_empty()))
        .map(|l| crate::markdown::clip(l, 120))
        .or_else(|| {
            run.plan
                .iter()
                .find(|p| p.state == crate::data::PlanState::Current)
                .map(|p| crate::markdown::clip(&p.text, 120))
        })
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
            .w_full()
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .px(space::S16)
            .pb(space::S24);
        if self.runs.is_empty() {
            // A compact Empty (§2.13): icon and one 14 muted line.
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(
                    body.pt(space::S16)
                        .child(ui::native::EmptyLine::new(p::EMPTY).icon(Sym::Terminal)),
                )
                .into_any_element();
        }
        // 全部记录 only when the 话题 has more than one run: a lone run is the card alone.
        let count = self.runs.len();
        // A 话题 with many runs would push the card and its steps far down: the list folds
        // to the newest rows, and the selected run always keeps its row.
        let folds = count > RUNS_FOLDED + 1;
        let shown: Vec<&BotRunSummary> = if count < 2 {
            Vec::new()
        } else if !folds || self.runs_expanded {
            self.runs.iter().collect()
        } else {
            let selected_ix = self
                .selected
                .as_ref()
                .and_then(|id| self.runs.iter().position(|r| &r.id == id))
                .unwrap_or(0);
            let mut ix: Vec<usize> = (0..RUNS_FOLDED).collect();
            if selected_ix >= RUNS_FOLDED {
                ix[RUNS_FOLDED - 1] = selected_ix;
            }
            ix.into_iter().map(|i| &self.runs[i]).collect()
        };
        let shown_len = shown.len();
        let mut rows: Vec<_> = shown
            .into_iter()
            .enumerate()
            .map(|(i, r)| {
                self.render_run_row(r, i + 1 == shown_len && !folds, cx)
                    .into_any_element()
            })
            .collect();
        if folds {
            let text = if self.runs_expanded {
                p::RUNS_COLLAPSE.to_string()
            } else {
                fill(p::RUNS_SHOW_ALL, &[("n", &count.to_string())])
            };
            let n = native_colors(cx);
            rows.push(
                div()
                    .id("runs-fold")
                    .h(space::S32)
                    .px(space::S12)
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(space::S4)
                    .cursor_pointer()
                    .hover(|el| el.bg(n.row_hover))
                    .n_footnote()
                    .text_color(n.secondary)
                    .child(text)
                    .child(ui::sym(
                        if self.runs_expanded {
                            Sym::ChevronUp
                        } else {
                            Sym::ChevronDown
                        },
                        ui::SymSize::INLINE,
                        n.secondary,
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.runs_expanded = !this.runs_expanded;
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
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
        let footer = run
            .as_ref()
            .and_then(|run| self.failure_footer(run, developer, cx));
        let n = native_colors(cx);
        let step_count = steps.len() as u32;
        let body = body
            .when(titled, |el| el.child(section(p::RUNS, cx)))
            .when(!titled, |el| el.pt(space::S16))
            .when(!rows.is_empty(), |el| {
                el.child(div().pb(space::S8).child(ui::panel_label(
                    p::RUNS_ALL,
                    Some(count as u32),
                    cx,
                )))
                .child(
                    div()
                        .mb(space::S16)
                        .rounded(ui::native::tokens::radius::CONTAINER)
                        .border_1()
                        .border_color(n.separator)
                        .overflow_hidden()
                        .flex()
                        .flex_col()
                        .children(rows),
                )
            })
            .when_some(run, |el, run| {
                el.child(self.render_run_card(&run, developer, cx))
                    .child(div().pt(space::S24).pb(space::S8).child(ui::panel_label(
                        p::STEPS,
                        (step_count > 0).then_some(step_count),
                        cx,
                    )))
                    .children(steps)
                    .children(self.failure_alert(&run, cx))
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
            });
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(body)
            .children(footer)
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

    /// The work record names the model and 思考强度 the run really used; nothing before the
    /// CLI said which model it runs.
    #[test]
    fn the_model_line_is_what_really_ran() {
        let mut run = BotRunSummary {
            hold: Default::default(),
            id: "r".into(),
            bot_id: "b".into(),
            group_id: "g".into(),
            topic_id: "t".into(),
            machine_id: None,
            machine_name: None,
            status: RunStatus::Running,
            requested_by: None,
            created_at: 0,
            started_at: None,
            ended_at: None,
            updated_at: 0,
            model: None,
            effort: Some("high".into()),
            summary: None,
            plan: Vec::new(),
        };
        assert_eq!(model_line(&run), None);
        run.model = Some("claude-haiku-5-5".into());
        assert_eq!(
            model_line(&run).as_deref(),
            Some("claude-haiku-5-5 · 思考强度高")
        );
        run.effort = None;
        assert_eq!(model_line(&run).as_deref(), Some("claude-haiku-5-5"));
        run.effort = Some("turbo".into());
        assert_eq!(
            model_line(&run).as_deref(),
            Some("claude-haiku-5-5 · 思考强度turbo")
        );
    }

    #[test]
    fn run_duration_uses_now_while_active() {
        let mut run = BotRunSummary {
            hold: Default::default(),
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
            model: None,
            effort: None,
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
            hold: Default::default(),
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
            model: None,
            effort: None,
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
}
