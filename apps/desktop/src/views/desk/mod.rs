//! 首页 (NATIVE.md §9.4): a Mail-like list under the toolbar row (title
//! 首页, subtitle `3 件事等你拍板`), grouped by 需要你拍板, 正在做 and 做完了,
//! with Jojo's one-line priority above it; the inspector at the right shows
//! 正在做 (plan bars) and 排队 as inspector rows.
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
    App, Context, InteractiveElement as _, IntoElement, ParentElement, Render,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};

use crate::{
    app::{Changes, Services},
    copy::{desk as c, fill},
    data::Millis,
    ui::{
        self,
        native::{
            NativeType as _, ToolbarGroup, ToolbarItemKind, ToolbarRow, emphasized, native_colors,
            toolbar_item,
        },
        sf_symbols::Sf,
    },
    views::{
        group_page::rows,
        shell::{PageView, TitleBarSpec},
    },
};

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
    /// The inspector (正在做, 排队) is hidden.
    side_hidden: bool,
    next_token: u64,
    _subscriptions: Vec<Subscription>,
}

impl DeskPage {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
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
            cx.notify();
        }
    }

    pub fn local_answer(&self, key: &str) -> Option<&LocalAnswer> {
        self.answers.get(key)
    }

    pub fn services(&self) -> &Services {
        &self.services
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

impl Render for DeskPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        let snap = self.snapshot();
        let waiting = snap
            .asks
            .iter()
            .filter(|a| !list::is_answered(a, self))
            .count();
        let quiet = snap.asks.is_empty() && snap.done.is_empty() && snap.runs.is_empty();
        let side_has_data = !snap.runs.is_empty() || !snap.queue.is_empty();
        let show_side = side_has_data && !self.side_hidden;

        let subtitle = if waiting > 0 {
            fill(c::DESK_SUBTITLE_ASKS, &[("n", &waiting.to_string())])
        } else {
            today_line()
        };
        let mut toolbar = ToolbarRow::new(c::DESK_TITLE)
            .subtitle(subtitle)
            .leading_inset(rows::toolbar_leading(&self.services, cx));
        if side_has_data {
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

        let note = snap.note.as_ref().map(|n| {
            let member = snap.member(&n.member_id);
            let from = fill(c::DESK_NOTE_FROM, &[("name", &snap.name(&n.member_id))]);
            let text = gpui::StyledText::new(format!("{from}{}", n.text)).with_highlights([(
                0..from.len(),
                gpui::HighlightStyle {
                    color: Some(k.label),
                    ..Default::default()
                },
            )]);
            div()
                .flex_none()
                .px(rows::TEXT_START)
                .pb(px(4.))
                .flex()
                .items_center()
                .gap(px(8.))
                .n_body()
                .text_color(k.secondary)
                .child(ui::MemberAvatar::new(member.as_ref(), px(16.)))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(text),
                )
        });

        let mut body = div()
            .id("desk-list")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .pb(px(20.))
            .flex()
            .flex_col()
            .children(note);
        if !snap.asks.is_empty() {
            let n = snap.asks.len();
            body = body
                .child(rows::section_header(
                    c::DESK_NEEDS_YOU,
                    (waiting > 0).then(|| waiting.to_string()),
                    cx,
                ))
                .children(
                    snap.asks
                        .iter()
                        .enumerate()
                        .map(|(i, a)| list::ask_row(a, i + 1 == n, self, &snap, cx))
                        .collect::<Vec<_>>(),
                );
        }
        if !snap.runs.is_empty() {
            let n = snap.runs.len();
            body = body
                .child(rows::section_header(
                    c::DESK_WORKING,
                    Some(n.to_string()),
                    cx,
                ))
                .children(snap.runs.iter().enumerate().map(|(i, r)| {
                    list::run_row(i, r, i + 1 == n, &snap, &self.services, cx)
                }));
        }
        if !snap.done.is_empty() {
            let n = snap.done.len();
            body = body
                .child(rows::section_header(
                    c::DESK_DONE_TODAY,
                    Some(c::DESK_TODAY.to_string()),
                    cx,
                ))
                .children(snap.done.iter().enumerate().map(|(i, d)| {
                    list::done_row(i, d, i + 1 == n, &snap, &self.services, cx)
                }));
        }
        if quiet {
            body = body.child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .n_body()
                    .text_color(k.secondary)
                    .child(c::DESK_NOTHING),
            );
        }

        div()
            .size_full()
            .flex()
            .bg(k.content)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(toolbar)
                    .child(body),
            )
            .when(show_side, |el| {
                el.child(working::render(&snap, &self.services, cx))
            })
    }
}
