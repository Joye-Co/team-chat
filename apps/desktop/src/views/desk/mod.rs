//! 首页 (COMPLETENESS.md §2.18): the toolbar row (title 首页, subtitle
//! `3 件事等你拍板`, from the same needs-you query as the sidebar count),
//! then Jojo's priority as an Alert, a KPI row (等你, 在做, 今天完成, 排队)
//! and one section per queue (需要你拍板, 正在做, 做完了), each a framed
//! list with fixed columns. The right pane holds only what the list does
//! not show (排队, 在哪干活). Every state is drawn: loading (skeletons),
//! failed (Alert with 重试), empty (Empty state) and long lists (看全部).
//!
//! The page keeps a snapshot of what it shows and rebuilds it when the
//! provider reports a change, so the frame path reads memory only.
//!
//! An answer is held for [`UNDO_WINDOW`] before it is sent: the protocol has
//! no way to take back a sent answer, so 撤回 shows only in that window. The
//! card collapses into its record at once and stays on the desk for this
//! session, also after the ask leaves the provider's list.

pub mod ask;
pub mod done;
pub mod list;
pub mod model;
pub mod scenes;
pub mod working;

use std::{collections::HashMap, rc::Rc, time::Duration};

use gpui::{
    AnyElement, App, Context, InteractiveElement as _, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _,
};

use crate::{
    app::{Changes, NewTopic, Services},
    copy::{desk as c, fill},
    data::Millis,
    ui::{
        native::{
            PushSize, PushStyle, ToolbarGroup, ToolbarItemKind, ToolbarRow, emphasized,
            native_colors,
            page::{self, PageLoad, SkeletonKind, m},
            push_button, toolbar_item,
        },
        sf_symbols::{Sf, Sym},
    },
    views::{
        group_page::rows,
        shell::{PageView, TitleBarSpec, clock},
    },
};

/// The rows a section shows before its `看全部` footer.
const SECTION_ROWS: usize = 6;

pub use model::{Answer, AskCard, Snapshot};

/// How long an answer waits before it is sent; 撤回 shows meanwhile.
pub const UNDO_WINDOW: Duration = Duration::from_secs(5);

/// An answer I made on the desk this session.
#[derive(Clone, Debug)]
pub struct LocalAnswer {
    /// The record line: `你选了「批准合并」`.
    pub record: String,
    pub at: Millis,
    /// Still inside [`UNDO_WINDOW`]: not sent, 撤回 shows.
    pub undoable: bool,
    /// Which press this is, so a timer of a press that was taken back never
    /// sends a later one early.
    token: u64,
}

pub struct DeskPage {
    services: Services,
    snapshot: Option<Rc<Snapshot>>,
    /// Answers by ask key (message id, else topic id).
    answers: HashMap<String, LocalAnswer>,
    /// Asks I answered this session, so they stay in place as records.
    kept: Vec<AskCard>,
    /// The right pane (排队, 在哪干活) is hidden.
    side_hidden: bool,
    /// Sections opened past [`SECTION_ROWS`] with `看全部`.
    expanded: std::collections::HashSet<&'static str>,
    next_token: u64,
    _subscriptions: Vec<Subscription>,
}

impl DeskPage {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        cx.default_global::<model::DeskAnswers>();
        let subscriptions = vec![
            cx.subscribe(&services.hub, |this, _, _: &Changes, cx| {
                this.snapshot = None;
                cx.notify();
            }),
            cx.observe(&services.nav, |_, _, cx| cx.notify()),
        ];
        Self {
            services,
            snapshot: None,
            answers: HashMap::new(),
            kept: Vec::new(),
            side_hidden: false,
            expanded: Default::default(),
            next_token: 0,
            _subscriptions: subscriptions,
        }
    }

    /// What the page shows: read once per change (a breathing dot renders
    /// the page on every frame it asks for, so frames read only this).
    fn snapshot(&mut self) -> Rc<Snapshot> {
        let snap = self
            .snapshot
            .get_or_insert_with(|| Rc::new(Snapshot::read(&self.services)));
        // Answered asks stay where the provider dropped them.
        let missing: Vec<&AskCard> = self
            .kept
            .iter()
            .filter(|kept| !snap.asks.iter().any(|a| a.key() == kept.key()))
            .collect();
        if !missing.is_empty() {
            let mut merged = (**snap).clone();
            merged.asks.extend(missing.into_iter().cloned());
            *snap = Rc::new(merged);
        }
        snap.clone()
    }

    /// The snapshot being drawn.
    pub fn snap(&self) -> Rc<Snapshot> {
        self.snapshot.clone().unwrap_or_default()
    }

    /// Answers an ask: the card collapses into `record` now, and the answer
    /// goes out after [`UNDO_WINDOW`] unless 撤回 is pressed.
    pub fn answer(&mut self, ask: AskCard, answer: Answer, record: String, cx: &mut Context<Self>) {
        let key = ask.key();
        if self.answers.contains_key(&key) {
            return;
        }
        self.next_token += 1;
        let token = self.next_token;
        self.answers.insert(
            key.clone(),
            LocalAnswer {
                record,
                at: crate::selftest::clock::local_now().timestamp_millis(),
                undoable: true,
                token,
            },
        );
        if !self.kept.iter().any(|k| k.key() == key) {
            self.kept.push(ask.clone());
        }
        cx.default_global::<model::DeskAnswers>()
            .keys
            .insert(key.clone());
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(UNDO_WINDOW).await;
            let _ = this.update(cx, |this, cx| this.send(&ask, &answer, token, cx));
        })
        .detach();
    }

    fn send(&mut self, ask: &AskCard, answer: &Answer, token: u64, cx: &mut Context<Self>) {
        let key = ask.key();
        let Some(local) = self.answers.get_mut(&key) else {
            return; // 撤回 in time.
        };
        if !local.undoable || local.token != token {
            return;
        }
        local.undoable = false;
        let Some(message_id) = ask.item.message_id.clone() else {
            cx.notify();
            return;
        };
        let p = &self.services.provider;
        let (g, t) = (&ask.item.group_id, &ask.item.topic_id);
        match answer {
            Answer::V1 { action_id } => p.press_card_action(g, t, &message_id, action_id),
            Answer::V2 {
                rev,
                action_id,
                inputs,
            } => p.answer_card_v2(
                g,
                t,
                &message_id,
                crate::data::cards_v2::CardV2AnswerRequest {
                    rev: *rev,
                    action_id: action_id.clone(),
                    inputs: (!inputs.is_empty()).then(|| inputs.clone()),
                    edited_values: None,
                },
            ),
        }
        cx.notify();
    }

    /// 撤回: drops an answer that was not sent yet.
    pub fn undo(&mut self, key: &str, cx: &mut Context<Self>) {
        if self.answers.get(key).is_some_and(|a| a.undoable) {
            self.answers.remove(key);
            self.kept.retain(|k| k.key() != key);
            cx.default_global::<model::DeskAnswers>().keys.remove(key);
            cx.notify();
        }
    }

    pub fn local_answer(&self, key: &str) -> Option<&LocalAnswer> {
        self.answers.get(key)
    }

    pub fn services(&self) -> &Services {
        &self.services
    }

    /// The rail's close button.
    pub fn hide_side(&mut self, cx: &mut Context<Self>) {
        self.side_hidden = true;
        cx.notify();
    }
}

/// `11 月 3 日 周一`.
pub fn today_line() -> String {
    use chrono::Datelike as _;
    let now = crate::selftest::clock::local_now();
    const DAYS: [&str; 7] = [
        c::WEEKDAY_1,
        c::WEEKDAY_2,
        c::WEEKDAY_3,
        c::WEEKDAY_4,
        c::WEEKDAY_5,
        c::WEEKDAY_6,
        c::WEEKDAY_7,
    ];
    crate::copy::fill(
        c::DESK_DATE,
        &[
            ("m", &now.month().to_string()),
            ("d", &now.day().to_string()),
            (
                "weekday",
                DAYS[now.weekday().num_days_from_monday() as usize],
            ),
        ],
    )
}

impl PageView for DeskPage {
    /// The 首页 draws its own toolbar row (the inspector runs up to the
    /// window top, §2), so the shell draws no title bar here.
    fn title_bar(&self, _cx: &App) -> TitleBarSpec {
        TitleBarSpec::default()
    }
}

/// A section of the page (§2.3): its header, 12 above its list.
fn section(header: gpui::Div, body: impl IntoElement) -> gpui::Div {
    div()
        .flex_none()
        .flex()
        .flex_col()
        .gap(m::HEADER_GAP)
        .child(header)
        .child(body)
}

impl DeskPage {
    /// Whether row `i` of `n` is the last one a section shows (no line
    /// under it).
    fn last_shown(&self, id: &'static str, i: usize, n: usize) -> bool {
        let shown = if self.expanded.contains(id) {
            n
        } else {
            n.min(SECTION_ROWS)
        };
        i + 1 == shown
    }

    /// A framed list of `rows`; past [`SECTION_ROWS`] it shows the first
    /// ones and a footer `看全部 N 件` that opens the rest (and `收起`).
    fn list(&self, id: &'static str, rows: Vec<AnyElement>, cx: &mut Context<Self>) -> gpui::Div {
        let total = rows.len();
        let open = self.expanded.contains(id);
        let long = total > SECTION_ROWS;
        let shown: Vec<AnyElement> = if long && !open {
            rows.into_iter().take(SECTION_ROWS).collect()
        } else {
            rows
        };
        let footer = long.then(|| {
            let text = if open {
                c::DESK_SHOW_LESS.to_string()
            } else {
                fill(c::DESK_SHOW_ALL, &[("n", &total.to_string())])
            };
            page::list_footer(SharedString::from(format!("{id}-more")), text, None, cx)
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.expanded.remove(id) {
                        this.expanded.insert(id);
                    }
                    cx.notify();
                }))
        });
        page::frame(cx).children(shown).children(footer)
    }

    /// The KPI row (§2.4): 等你, 在做, 今天完成, 排队.
    fn kpis(&self, snap: &Snapshot, waiting: usize, cx: &App) -> gpui::Div {
        let k = native_colors(cx);
        let earliest = snap
            .asks
            .iter()
            .filter(|a| !list::is_answered(a, self))
            .map(|a| a.item.at)
            .min();
        let mut workers: Vec<&str> = snap.runs.iter().map(|r| r.run.bot_id.as_str()).collect();
        workers.sort_unstable();
        workers.dedup();
        let latest = snap.done.iter().map(|d| d.at).max();
        let next = snap.queue.iter().filter_map(|q| q.at).min();
        let foot = |v: Option<String>, key: &str, none: &str| match v {
            Some(v) => fill(key, &[("time", &v)]),
            None => none.to_string(),
        };
        div()
            .flex_none()
            .flex()
            .gap(m::GRID_GAP)
            .child(page::kpi_card(
                "desk-kpi-waiting",
                c::DESK_KPI_WAITING,
                waiting.to_string(),
                foot(
                    earliest.filter(|_| waiting > 0).map(clock),
                    c::DESK_KPI_WAITING_FOOT,
                    c::DESK_KPI_WAITING_NONE,
                ),
                Some((Sym::NeedsYou, k.secondary)),
                cx,
            ))
            .child(page::kpi_card(
                "desk-kpi-working",
                c::DESK_KPI_WORKING,
                snap.runs.len().to_string(),
                if workers.is_empty() {
                    c::DESK_KPI_WORKING_NONE.to_string()
                } else {
                    fill(
                        c::DESK_KPI_WORKING_FOOT,
                        &[("n", &workers.len().to_string())],
                    )
                },
                Some((Sym::Spinner, k.secondary)),
                cx,
            ))
            .child(page::kpi_card(
                "desk-kpi-done",
                c::DESK_KPI_DONE,
                snap.done.len().to_string(),
                foot(
                    latest.map(clock),
                    c::DESK_KPI_DONE_FOOT,
                    c::DESK_KPI_DONE_NONE,
                ),
                Some((Sym::Done, k.secondary)),
                cx,
            ))
            .child(page::kpi_card(
                "desk-kpi-queue",
                c::DESK_KPI_QUEUE,
                snap.queue.len().to_string(),
                foot(
                    next.map(clock),
                    c::DESK_KPI_QUEUE_FOOT,
                    c::DESK_KPI_QUEUE_NONE,
                ),
                Some((Sym::Waiting, k.secondary)),
                cx,
            ))
    }
}

impl Render for DeskPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        let snap = self.snapshot();
        // One needs-you query for this subtitle and the sidebar count.
        let waiting = model::waiting_on_me(&self.services, cx);
        crate::selftest::drawn::mark(
            "desk",
            format!(
                "asks={} done={} runs={}",
                snap.asks.len(),
                snap.done.len(),
                snap.runs.len()
            ),
        );
        // Nothing waits on me and nothing was done today: the Empty state
        // (running work still shows in 正在做).
        let quiet = snap.asks.is_empty() && snap.done.is_empty();
        let has_data =
            !quiet || !snap.runs.is_empty() || snap.note.is_some() || !snap.queue.is_empty();
        let load = PageLoad::of(&self.services, has_data);
        let show_side = !self.side_hidden;

        // The subtitle slot stays in every state, so the title never moves.
        let subtitle = match load {
            PageLoad::Ready => fill(c::DESK_SUBTITLE_ASKS, &[("n", &waiting.to_string())]),
            PageLoad::Loading => c::DESK_SUBTITLE_SYNCING.to_string(),
            PageLoad::Failed => c::DESK_SUBTITLE_OFFLINE.to_string(),
        };
        let mut toolbar = ToolbarRow::new(c::DESK_TITLE)
            .subtitle(subtitle)
            .leading_inset(rows::toolbar_leading(&self.services, cx));
        {
            let name = if show_side {
                c::DESK_HIDE_SIDE
            } else {
                c::DESK_SHOW_SIDE
            };
            let kind = if show_side {
                ToolbarItemKind::Toggled
            } else {
                ToolbarItemKind::Plain
            };
            toolbar = toolbar.group(ToolbarGroup::new([toolbar_item(
                "desk-inspector",
                Sf::PaneRight,
                name,
                kind,
                emphasized(window),
                window,
                cx,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.side_hidden = !this.side_hidden;
                cx.notify();
            }))]));
        }

        // The content fills the pane's height, so a short page ends in a
        // frame that runs to the bottom, never in a blank band (§2.21).
        let mut content = div()
            .min_h_full()
            .px(m::EDGE)
            .pt(m::GUTTER)
            .pb(m::GUTTER)
            .flex()
            .flex_col()
            .gap(m::GUTTER);
        match load {
            PageLoad::Failed => {
                // The failure takes the place of the content (§2.15): no
                // skeletons pulse under it.
                content = content.child(rows::failed_region("desk", &self.services, cx));
            }
            PageLoad::Loading => {
                content = content
                    .child(page::skeleton_cards(4, cx))
                    .child(section(
                        page::section_header(c::DESK_NEEDS_YOU, None, None, None, cx),
                        page::skeleton_rows("desk-sk-asks", SkeletonKind::Row, 3, cx),
                    ))
                    .child(section(
                        page::section_header(c::DESK_WORKING, None, None, None, cx),
                        page::skeleton_rows("desk-sk-runs", SkeletonKind::Row, 2, cx),
                    ))
                    .child(section(
                        page::section_header(c::DESK_DONE_TODAY, None, None, None, cx),
                        page::skeleton_rows("desk-sk-done", SkeletonKind::Row, 2, cx),
                    ));
            }
            PageLoad::Ready => {
                content = content
                    .children(list::note_alert(&snap, cx))
                    .child(self.kpis(&snap, waiting, cx));
                // 需要你拍板 holds what still waits; my answers of this
                // session move to 刚处理, so the count matches the rows.
                let (open, handled): (Vec<&AskCard>, Vec<&AskCard>) =
                    snap.asks.iter().partition(|a| !list::is_answered(a, self));
                for (id, title, asks) in [
                    ("desk-asks", c::DESK_NEEDS_YOU, open),
                    ("desk-handled", c::DESK_JUST_HANDLED, handled),
                ] {
                    if asks.is_empty() {
                        continue;
                    }
                    let n = asks.len();
                    let rows: Vec<AnyElement> = asks
                        .into_iter()
                        .enumerate()
                        .map(|(i, a)| list::ask_row(a, self.last_shown(id, i, n), self, &snap, cx))
                        .collect();
                    content = content.child(section(
                        page::section_header(title, Some(n), None, None, cx),
                        self.list(id, rows, cx),
                    ));
                }
                if !snap.runs.is_empty() {
                    let n = snap.runs.len();
                    let rows: Vec<AnyElement> = snap
                        .runs
                        .iter()
                        .enumerate()
                        .map(|(i, r)| {
                            list::run_row(
                                i,
                                r,
                                self.last_shown("desk-runs", i, n),
                                &snap,
                                &self.services,
                                cx,
                            )
                        })
                        .collect();
                    content = content.child(section(
                        page::section_header(c::DESK_WORKING, Some(n), None, None, cx),
                        self.list("desk-runs", rows, cx),
                    ));
                }
                if !snap.done.is_empty() {
                    let n = snap.done.len();
                    let rows: Vec<AnyElement> = snap
                        .done
                        .iter()
                        .enumerate()
                        .map(|(i, d)| {
                            list::done_row(
                                i,
                                d,
                                self.last_shown("desk-done", i, n),
                                &snap,
                                &self.services,
                                cx,
                            )
                        })
                        .collect();
                    content = content.child(section(
                        page::section_header(c::DESK_DONE_TODAY, Some(n), None, None, cx),
                        self.list("desk-done", rows, cx),
                    ));
                }
                if quiet {
                    let new_topic = push_button(
                        "desk-empty-new",
                        c::DESK_NEW_TOPIC,
                        PushStyle::Default,
                        PushSize::Small,
                        cx,
                    )
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(NewTopic), cx));
                    let nav = self.services.nav.clone();
                    let artifacts = push_button(
                        "desk-empty-artifacts",
                        c::DESK_SEE_ARTIFACTS,
                        PushStyle::Outline,
                        PushSize::Small,
                        cx,
                    )
                    .on_click(move |_, _, cx| nav.update(cx, |n, cx| n.open_artifacts(cx)));
                    // The Empty in a frame that hugs it (A1: space below the
                    // last frame is allowed).
                    content = content.child(
                        div()
                            .flex_none()
                            .flex()
                            .rounded(m::FRAME_RADIUS)
                            .border_1()
                            .border_color(k.separator)
                            .child(page::page_empty(
                                Sym::Done,
                                c::DESK_EMPTY_TITLE,
                                c::DESK_EMPTY_BODY,
                                vec![new_topic.into_any_element(), artifacts.into_any_element()],
                                cx,
                            )),
                    );
                }
            }
        }

        let body = div()
            .id("desk-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(content);

        div()
            .size_full()
            .flex()
            .bg(k.content)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    // ToolbarRow draws the one 1 px line at y 52 (§2.2).
                    .child(div().flex_none().child(toolbar))
                    .child(body),
            )
            .when(show_side, |el| {
                el.child(working::render(&snap, load, &self.services, window, cx))
            })
    }
}
