//! 首页: the desk (Main.dc.html). Jojo's one-line priority, 需要你拍板 (asks
//! for me as real cards I answer in place), 做完了 today with proof chips,
//! and at the right 正在做 (live terminals and plan progress) and 排队.
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
pub mod model;
pub mod scenes;
pub mod working;

use std::{collections::HashMap, rc::Rc, time::Duration};

use gpui::{
    App, Context, InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};

use crate::{
    app::{Changes, Services},
    copy::desk as c,
    data::Millis,
    theme::{self, Type as _, space},
    ui::{self, Sym},
    views::shell::{PageView, TitleBarSpec},
};

pub use model::{Answer, AskCard, Snapshot};

/// How long an answer waits before it is sent; 撤回 shows meanwhile.
pub const UNDO_WINDOW: Duration = Duration::from_secs(5);
/// The main column: cards and rows (Main.dc.html `max-width: 680px`).
pub const MAIN_MEASURE: gpui::Pixels = px(680.);
/// The 正在做 column, padding included (`flex: 1 1 320px`).
pub const SIDE_WIDTH: gpui::Pixels = px(320.);

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
    /// The 正在做 column is hidden.
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
    /// The 首页 draws its own header inside its centered column, and the
    /// 正在做 column runs up to the window top (Main.dc.html), so the shell
    /// draws no title bar here.
    fn title_bar(&self, _cx: &App) -> TitleBarSpec {
        TitleBarSpec::default()
    }
}

/// A section header: an optional 6 pt dot, the name 13/600 and a quiet
/// trailing part (a mono count or `今天`).
fn section_header(
    label: &str,
    dot: Option<gpui::Hsla>,
    trailing: Option<(String, bool)>,
    cx: &App,
) -> gpui::Div {
    let k = theme::colors(cx);
    div()
        .flex()
        .items_center()
        .gap(space::S8)
        .h(px(24.))
        .children(dot.map(|d| div().flex_none().size(px(6.)).rounded_full().bg(d)))
        .child(
            div()
                .t_small()
                .semibold()
                .text_color(k.ink)
                .child(label.to_string()),
        )
        .children(trailing.map(|(text, mono)| {
            div()
                .t_meta()
                .text_color(k.ink_3)
                .when(mono, |el| el.mono())
                .child(text)
        }))
}

impl Render for DeskPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let snap = self.snapshot();
        let quiet = snap.asks.is_empty() && snap.done.is_empty();
        let side_has_data = !snap.runs.is_empty() || !snap.queue.is_empty();
        let show_side = side_has_data && !self.side_hidden;

        let header = div()
            .flex_none()
            .h(theme::TITLE_BAR)
            .flex()
            .items_center()
            .gap(space::S10)
            .child(
                div()
                    .flex_none()
                    .t_page_title()
                    .text_color(k.ink)
                    .child(c::DESK_TITLE),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .one_line()
                    .t_small()
                    .text_color(k.ink_3)
                    .child(today_line()),
            )
            .when(side_has_data && self.side_hidden, |el| {
                el.child(side_toggle("desk-show-side", false, cx))
            });

        let note = snap.note.as_ref().map(|n| {
            let member = snap.member(&n.member_id);
            let name = member.as_ref().map(|m| m.name.clone()).unwrap_or_default();
            let from = crate::copy::fill(c::DESK_NOTE_FROM, &[("name", &name)]);
            let text = gpui::StyledText::new(format!("{from}{}", n.text)).with_highlights([(
                0..from.len(),
                gpui::HighlightStyle {
                    color: Some(k.ink),
                    font_weight: Some(theme::MEDIUM_WEIGHT),
                    ..Default::default()
                },
            )]);
            let full = SharedString::from(format!("{from}{}", n.text));
            let row = div()
                .id("desk-note")
                .mt(space::S6)
                .flex()
                .items_start()
                .gap(space::S10)
                .t_small()
                .text_color(k.ink_2)
                .child(
                    div()
                        .flex_none()
                        .mt(px(1.))
                        .child(ui::MemberAvatar::new(member.as_ref(), px(18.))),
                )
                // Two lines, then an ellipsis; the whole note is the tooltip.
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .line_clamp(2)
                        .text_ellipsis()
                        .child(text),
                )
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(full.clone()).build(window, cx)
                });
            ui::motion::rise("desk-note", 0, row, cx)
        });

        let ask_count = snap.asks.len();
        let asks = (!snap.asks.is_empty()).then(|| {
            div()
                .mt(space::S24)
                .flex()
                .flex_col()
                .child(section_header(
                    c::DESK_NEEDS_YOU,
                    Some(k.you),
                    Some((ask_count.to_string(), true)),
                    cx,
                ))
                .child(
                    div()
                        .mt(space::S10)
                        .flex()
                        .flex_col()
                        .gap(space::S12)
                        .children(snap.asks.iter().enumerate().map(|(i, a)| {
                            let card = ask::render(a, self, window, cx);
                            ui::motion::rise(
                                SharedString::from(format!("desk-ask-rise-{}", a.key())),
                                i + 1,
                                card,
                                cx,
                            )
                        })),
                )
        });

        let done = (!snap.done.is_empty()).then(|| {
            div()
                .mt(px(36.))
                .flex()
                .flex_col()
                .child(section_header(
                    c::DESK_DONE_TODAY,
                    None,
                    Some((c::DESK_TODAY.to_string(), false)),
                    cx,
                ))
                .child(div().mt(space::S6).flex().flex_col().children(
                    snap.done.iter().enumerate().map(|(i, d)| {
                        done::render(d, ask_count + 1 + i, &snap, &self.services, cx)
                    }),
                ))
        });

        let empty = quiet.then(|| {
            div().mt(px(96.)).child(
                ui::EmptyState::new(Sym::Done, c::DESK_EMPTY)
                    .description(c::DESK_EMPTY_LINE),
            )
        });

        let main = div()
            .id("desk-main")
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .overflow_y_scroll()
            .px(px(40.))
            .pb(px(56.))
            .child(
                div()
                    .w_full()
                    .max_w(MAIN_MEASURE)
                    .mx_auto()
                    .flex()
                    .flex_col()
                    .child(header)
                    .children(note)
                    .children(asks)
                    .children(empty)
                    .children(done),
            );

        div()
            .size_full()
            .flex()
            .bg(k.bg)
            .child(main)
            .when(show_side, |el| {
                el.child(working::render(&snap, &self.services, cx))
            })
    }
}

/// The 28 pt button that hides or shows the 正在做 column.
pub fn side_toggle(id: &'static str, hide: bool, cx: &mut Context<DeskPage>) -> impl IntoElement {
    let k = theme::colors(cx);
    let name = if hide {
        c::DESK_HIDE_SIDE
    } else {
        c::DESK_SHOW_SIDE
    };
    div()
        .id(id)
        .flex_none()
        .size(theme::TITLE_BUTTON)
        .rounded(px(7.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|s| s.bg(k.hover))
        .child(ui::icon(Sym::Inspector, theme::icon::ROW, k.ink_3))
        .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(name).build(window, cx))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.side_hidden = hide;
            cx.notify();
        }))
}
