//! The process view (过程) of the side panel: the runs of bot members in the
//! open topic (status, where they run, times) and the steps of the selected
//! run: messages, commands and their results, file changes and approvals.
//! While the panel shows an active run it asks the provider for new log
//! entries every [`POLL`]. The developer view adds ids, the raw log entries
//! and the raw session download.

pub mod preview;
mod steps;

use std::{path::PathBuf, time::Duration};

use gpui::{
    App, Context, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::Button,
    list::ListItem,
};

use crate::{
    app::{Changes, Services},
    copy::{dev, fill, process as p},
    data::{
        BotRunSummary, Change, GroupId, Member, Millis, RunLoadError, RunProcess, RunStatus,
        TopicId,
    },
    theme::{self, Type as _, space},
    ui::{self, Lucide},
    views::{avatar, time_text},
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

    fn selected_run(&self) -> Option<&BotRunSummary> {
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

    /// A row of the 工作记录 list (shown when the topic has more than one
    /// run): 20 pt avatar, name, status and time.
    fn render_run_row(&self, run: &BotRunSummary, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let bot = self.member(&run.bot_id).cloned();
        let selected = self.selected.as_deref() == Some(run.id.as_str());
        let id = run.id.clone();
        let when = time_text(run.started_at.unwrap_or(run.created_at));
        ListItem::new(gpui::ElementId::Name(format!("run-{}", run.id).into()))
            .selected(selected)
            .on_click(cx.listener(move |this, _, _, cx| this.select(Some(id.clone()), cx)))
            .child(
                div()
                    .w_full()
                    .h(px(28.))
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .child(avatar(bot.as_ref(), px(20.), cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .t_ui()
                            .text_color(colors.text_1)
                            .one_line()
                            .child(bot.map(|b| b.name).unwrap_or_default()),
                    )
                    .child(status_badge(run.status, cx))
                    .child(
                        div()
                            .flex_none()
                            .t_caption()
                            .tnum()
                            .text_color(colors.text_3)
                            .child(when),
                    ),
            )
    }

    /// The working member (§13.7): 28 pt avatar, name 13 / 600 and
    /// `正在工作 · 用时 18 分` with a failed or stuck status in `danger-text`;
    /// then who asked, the summary and, in the developer view, the ids.
    fn render_run_details(
        &self,
        run: &BotRunSummary,
        developer: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = theme::colors(cx);
        let now = chrono::Utc::now().timestamp_millis();
        let bot = self.member(&run.bot_id).cloned();
        let alarm = matches!(run.status, RunStatus::Failed | RunStatus::Blocked);
        let status = div()
            .flex_none()
            .text_color(if alarm { colors.danger_text } else { colors.text_2 })
            .child(status_text(run.status));
        let duration = run_duration(run, now).map(|ms| fill(p::DURATION, &[("d", &duration_text(ms))]));
        let mut facts: Vec<String> = Vec::new();
        if let Some(by) = run.requested_by.as_deref().and_then(|id| self.member(id)) {
            facts.push(fill(p::REQUESTED_BY, &[("name", &by.name)]));
        }
        if let Some(at) = run.started_at {
            facts.push(fill(p::STARTED_AT, &[("time", &time_text(at))]));
        }
        if let Some(at) = run.ended_at {
            facts.push(fill(p::ENDED_AT, &[("time", &time_text(at))]));
        }
        let live = run.status.is_active() && self.visible;
        div()
            .flex()
            .flex_col()
            .gap(space::S6)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S10)
                    .child(avatar(bot.as_ref(), px(28.), cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .t_ui()
                                    .semibold()
                                    .text_color(colors.text_1)
                                    .one_line()
                                    .child(bot.map(|b| b.name).unwrap_or_default()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(space::S4)
                                    .t_caption()
                                    .font_weight(gpui::FontWeight::NORMAL)
                                    .tnum()
                                    .text_color(colors.text_2)
                                    .child(status)
                                    .when_some(duration, |el, d| {
                                        el.child("·").child(div().min_w(px(0.)).one_line().child(d))
                                    }),
                            ),
                    ),
            )
            .when(!facts.is_empty() || live, |el| {
                el.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_x(space::S8)
                        .t_caption()
                        .font_weight(gpui::FontWeight::NORMAL)
                        .tnum()
                        .text_color(colors.text_2)
                        .children(facts.into_iter().map(|f| div().child(f)))
                        .when(live, |el| {
                            el.child(div().text_color(colors.accent_text).child(p::LIVE))
                        }),
                )
            })
            .when_some(run.summary.clone(), |el, summary| {
                el.child(
                    div()
                        .t_ui()
                        .text_color(colors.text_1)
                        .child(summary),
                )
            })
            .when(developer, |el| {
                let opt =
                    |t: Option<Millis>| t.map(|t| t.to_string()).unwrap_or_else(|| "-".into());
                el.child(
                    div()
                        .pt(space::S4)
                        .t_caption()
                        .font_weight(gpui::FontWeight::NORMAL)
                        .font_family(cx.theme().mono_font_family.clone())
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

    /// The machine box (§13.7): `fill-field`, radius 8, `monitor`, where it
    /// runs and the elapsed time.
    fn render_machine(&self, run: &BotRunSummary, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let colors = theme::colors(cx);
        let name = run.machine_name.clone().or_else(|| run.machine_id.clone())?;
        let now = chrono::Utc::now().timestamp_millis();
        Some(
            div()
                .mt(space::S12)
                .h(px(36.))
                .px(space::S10)
                .rounded(theme::radius::ROW)
                .bg(colors.fill_field)
                .flex()
                .items_center()
                .gap(space::S8)
                .t_callout()
                .text_color(colors.text_2)
                .child(ui::icon(Lucide::Monitor, theme::icon::BUTTON, colors.text_2))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .one_line()
                        .child(fill(p::RUN_ON, &[("name", &name)])),
                )
                .children(run_duration(run, now).map(|ms| {
                    div().flex_none().tnum().child(duration_text(ms))
                }))
                .into_any_element(),
        )
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
                    .py(px(2.))
                    .border_b_1()
                    .border_color(colors.border)
                    .child(format!("#{} {}", e.seq, text))
            })
            .collect();
        div()
            .pt(px(12.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .text_xs()
            .child(
                Button::new("download-session")
                    .small()
                    .outline()
                    .label(dev::RUN_DOWNLOAD_SESSION)
                    .disabled(matches!(self.download, Download::Running))
                    .on_click(cx.listener(|this, _, _, cx| this.download_session(cx))),
            )
            .when_some(download_line, |el, line| {
                el.child(div().text_color(colors.muted_foreground).child(line))
            })
            .child(
                div()
                    .pt(px(6.))
                    .text_color(colors.muted_foreground)
                    .child(fill(
                        dev::RUN_RAW,
                        &[("n", &self.process.raw.len().to_string())],
                    )),
            )
            .child(div().font_family(mono).children(raw))
    }
}

/// The status of a run as a chip: the three chip weights of §2.5 (a quiet
/// chip with a colored glyph, the `you` fill for waiting, red for stuck).
pub fn status_badge(status: RunStatus, _cx: &App) -> impl IntoElement {
    use crate::{data::TopicStatus as S, theme::ChipKind as K};
    let (topic, kind) = match status {
        RunStatus::Created => (S::Open, K::Quiet),
        RunStatus::Running => (S::InProgress, K::Quiet),
        RunStatus::Waiting => (S::WaitingAnswer, K::Quiet),
        RunStatus::Blocked | RunStatus::Failed => (S::Blocked, K::Blocked),
        RunStatus::Done => (S::Done, K::Quiet),
        RunStatus::Cancelled => (S::Open, K::Quiet),
    };
    ui::StatusChip::labeled(status_text(status), topic, kind)
}

impl Render for ProcessView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let developer = self.services.nav.read(cx).developer_view;
        let section = |title: &'static str| {
            div()
                .pt(space::S16)
                .pb(space::S6)
                .child(ui::SectionHeader::new(title))
        };
        let body = div()
            .id("process-view")
            .size_full()
            .overflow_y_scroll()
            .px(space::S16)
            .pt(space::S12)
            .pb(space::S16);
        if self.runs.is_empty() {
            return div()
                .size_full()
                .child(
                    ui::EmptyState::new(Lucide::Activity, p::EMPTY_TITLE)
                        .description(p::EMPTY_BODY),
                )
                .into_any_element();
        }
        let many = self.runs.len() > 1;
        let rows: Vec<_> = if many {
            self.runs
                .iter()
                .map(|r| self.render_run_row(r, cx).into_any_element())
                .collect()
        } else {
            Vec::new()
        };
        let run = self.selected_run().cloned();
        let bot_name = run
            .as_ref()
            .and_then(|r| self.member(&r.bot_id))
            .map(|m| m.name.clone())
            .unwrap_or_default();
        let steps = steps::render_steps(&self.process, &bot_name, developer, cx);
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
        let machine = run.as_ref().and_then(|r| self.render_machine(r, cx));
        body.when(many, |el| {
            el.child(div().pb(space::S6).child(ui::SectionHeader::new(p::RUNS)))
                .children(rows)
                .child(div().h(space::S12))
        })
        .when_some(run, |el, run| {
            el.child(self.render_run_details(&run, developer, cx))
                .child(section(p::STEPS))
                .children(steps)
                .when_some(state_line, |el, (text, retry)| {
                    el.child(
                        div()
                            .py(space::S6)
                            .flex()
                            .items_center()
                            .gap(space::S8)
                            .t_callout()
                            .text_color(colors.text_2)
                            .when(retry, |el| {
                                el.child(ui::icon(
                                    Lucide::CircleAlert,
                                    theme::icon::SMALL,
                                    colors.danger,
                                ))
                            })
                            .child(div().flex_1().min_w(px(0.)).child(text))
                            .when(retry, |el| {
                                let id = run.id.clone();
                                el.child(
                                    Button::new("process-retry")
                                        .small()
                                        .label(p::RETRY)
                                        .shadow(colors.shadow(theme::Elevation::Control))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.services.provider.follow_run(&id);
                                            cx.notify();
                                        })),
                                )
                            }),
                    )
                })
                .children(machine)
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
