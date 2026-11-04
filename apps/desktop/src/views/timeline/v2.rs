//! The v2 timeline style of the 话题 page (NATIVE.md §9.2): the full content
//! width with 24 pt side insets and message text at most 680 pt; 28 pt
//! avatars; the sender name in Headline with 岗位 and time as one
//! `secondary` run; centered day separators without rules; file rows on
//! `fill`; and three rows built from the messages themselves:
//!
//! - a quiet one-line routing row (`Jojo 转给了 Forge`) for a hand-off;
//! - a quiet one-line decision row (`Joye 选了「…」`) for a message that
//!   made a decision of the 话题;
//! - the done card of a result message (radius 12 on `fill`, no border):
//!   title, why, facts as symbol + text runs, and rows to the change and
//!   the 产出 with a trailing `arrow.up.forward`.
//!
//! The topic track owns this file; `row.rs` calls it only when the view is
//! in the v2 style.

use chrono::Datelike as _;
use gpui::{
    AnyElement, App, ClickEvent, Context, ElementId, Hsla, InteractiveElement as _, IntoElement,
    ParentElement, Pixels, SharedString, StatefulInteractiveElement as _, Styled, div,
    prelude::FluentBuilder as _, px,
};

use super::{TimelineView, model};
use crate::{
    app::RightPane,
    copy::{fill, topic_rail as c},
    data::{FileAttachment, Member, Message, RunStatus, v2::ChangeState, v2::Proof},
    markdown,
    theme::{self, Type as _, space},
    ui::{
        self, Sym,
        native::{NativeType as _, native_colors},
    },
    views::{
        files::{OpenAction, open_file, size_text},
        topic_rail::{kind_text, media_time},
    },
};

/// The v2 gutter: avatars and tiles are 28 pt.
pub const GUTTER: Pixels = px(28.);
/// The column of a v2 timeline (and of the pinned card and composer).
pub const COLUMN: Pixels = px(720.);
/// Trailing padding of the v2 column (NATIVE.md §9.2: 24 pt side insets).
pub const SIDE: Pixels = px(24.);
// --- v12/fx-topic: one leading content inset (coordinator decision) ---
/// The shared leading inset: the toolbar title, transcript avatars, the
/// decision bar and the composer start 20 pt after the sidebar panel. The
/// content view starts after the panel's 8 pt inset, so the inset inside
/// it is 12 pt. Use the shell's token once it lands.
pub const LEADING: Pixels = px(12.);
// --- end v12/fx-topic ---
/// The widest message text and card (NATIVE.md §9.2).
pub const TEXT_MAX: Pixels = px(680.);
/// Rows drawn this soon after a 话题 opens rise in (first paint).
pub const FIRST_PAINT: std::time::Duration = std::time::Duration::from_millis(600);

/// The date divider label: `11 月 3 日 周一`.
pub fn date_label(at: crate::data::Millis) -> String {
    let Some(day) = model::local_day(at) else {
        return String::new();
    };
    let w = match day.weekday() {
        chrono::Weekday::Mon => c::WEEKDAY_1,
        chrono::Weekday::Tue => c::WEEKDAY_2,
        chrono::Weekday::Wed => c::WEEKDAY_3,
        chrono::Weekday::Thu => c::WEEKDAY_4,
        chrono::Weekday::Fri => c::WEEKDAY_5,
        chrono::Weekday::Sat => c::WEEKDAY_6,
        chrono::Weekday::Sun => c::WEEKDAY_7,
    };
    fill(
        c::DATE_FULL,
        &[
            ("m", &day.month().to_string()),
            ("d", &day.day().to_string()),
            ("w", w),
        ],
    )
}

/// The day separator (NATIVE.md §9.2): centered `secondary` Subheadline
/// text, no rules.
pub fn date_divider(at: crate::data::Millis, first: bool, cx: &App) -> AnyElement {
    let k = native_colors(cx);
    div()
        .when(!first, |el| el.mt(space::S16))
        .mb(space::S4)
        .h(px(18.))
        .flex()
        .items_center()
        .justify_center()
        .n_subheadline()
        .text_color(k.secondary)
        .child(date_label(at))
        .into_any_element()
}

/// The other member a hand-off message routes to: the message is only
/// `转给 @名字` (with punctuation).
pub fn routed_to<'a>(message: &Message, members: &'a [Member]) -> Option<&'a Member> {
    let mut rest = message.body.trim().to_string();
    let target = members
        .iter()
        .filter(|m| m.id != message.author_id && !m.name.is_empty())
        .find(|m| rest.contains(&format!("@{}", m.name)))?;
    rest = rest.replace(&format!("@{}", target.name), "");
    let rest: String = rest
        .chars()
        .filter(|ch| !ch.is_whitespace() && !"。.，,！!".contains(*ch))
        .collect();
    (rest == "转给" || rest == "交给").then_some(target)
}

/// The title line and the rest of a result message whose first line is
/// bold (`**改好了：重连后不再重复**`).
pub fn result_parts(body: &str) -> Option<(String, String)> {
    let body = body.trim();
    let (first, rest) = body.split_once('\n').unwrap_or((body, ""));
    let first = first.trim();
    let title = first.strip_prefix("**")?.strip_suffix("**")?.trim();
    (!title.is_empty()).then(|| (title.to_string(), rest.trim().to_string()))
}

/// A v2 row that replaces a whole message: it carries its own header, so it
/// never joins or continues an author group (§7.6).
pub(super) enum Special {
    /// `Jojo 转给了 Forge`.
    Routed(String),
    /// `Joye 选了「…」`.
    Decision(String),
    /// A member's result with its proof.
    Done {
        title: String,
        rest: String,
        proof: Vec<Proof>,
    },
}

impl TimelineView {
    /// Scrolls to a message of the open 话题 and highlights it (看问题).
    fn show_message(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(m) = self.messages.iter().find(|m| m.id == id).cloned() else {
            return;
        };
        self.services.nav.update(cx, |nav, cx| {
            nav.open_message(m.group_id, m.topic_id, m.id, m.created_seq, cx)
        });
    }

    /// The question a decision answered: the message it quotes, or the
    /// latest card a member asked before it in the loaded 话题.
    fn decision_question(&self, decision: &Message) -> Option<String> {
        if let Some(q) = &decision.quote_of {
            return Some(q.clone());
        }
        let p = &self.services.provider;
        self.messages
            .iter()
            .rev()
            .skip_while(|m| m.id != decision.id)
            .skip(1)
            .find(|m| {
                m.deleted_at.is_none()
                    && self.v2_member(&m.author_id).is_some_and(Member::is_bot)
                    && (p.card(&m.group_id, &m.id).is_some()
                        || p.card_v2(&m.group_id, &m.id).is_some())
            })
            .map(|m| m.id.clone())
    }

    /// The member of `id` in the open 群.
    fn v2_member(&self, id: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.id == id)
    }

    /// The sender line of a message block (NATIVE.md §9.2): the name in
    /// Headline, then 岗位 and time as one `secondary` Subheadline run
    /// (`前端实习 · 09:12`). No tag pill.
    pub(super) fn v2_meta(
        &self,
        author: Option<&Member>,
        at: crate::data::Millis,
        cx: &App,
    ) -> AnyElement {
        let k = native_colors(cx);
        let role = author
            .and_then(|a| a.role.clone())
            .filter(|r| !r.trim().is_empty());
        let time = model::clock(at);
        let run = match role {
            Some(role) => fill(c::ROLE_TIME, &[("role", role.trim()), ("time", &time)]),
            None => time,
        };
        div()
            .h(px(18.))
            .min_w_0()
            .flex()
            .items_baseline()
            .gap(space::S6)
            .child(
                ui::truncating(
                    "sender-name",
                    author.map(|m| m.name.clone()).unwrap_or_default(),
                )
                .flex_shrink(1.)
                .max_w(px(200.))
                .n_headline()
                .text_color(k.label),
            )
            .child(
                div()
                    .flex_shrink(1.)
                    .min_w(px(40.))
                    .one_line()
                    .n_subheadline()
                    .n_figures()
                    .text_color(k.secondary)
                    .child(run),
            )
            .into_any_element()
    }

    /// What replaces `message` in the v2 style, or `None` for a normal
    /// message.
    pub(super) fn v2_special(&self, message: &Message) -> Option<Special> {
        if message.deleted_at.is_some() || message.id.is_empty() {
            return None;
        }
        let author = self.v2_member(&message.author_id);
        let bot = author.is_some_and(Member::is_bot);
        let has_card = self
            .services
            .provider
            .card(&message.group_id, &message.id)
            .is_some();
        if has_card {
            return None;
        }
        if bot && let Some(to) = routed_to(message, &self.members) {
            return Some(Special::Routed(to.name.clone()));
        }
        if !bot
            && let Some(d) = self
                .services
                .provider
                .topic_decisions(&message.group_id, &message.topic_id)
                .into_iter()
                .find(|d| d.by == message.author_id && (d.at - message.created_at).abs() < 60_000)
        {
            return Some(Special::Decision(d.text));
        }
        if bot && let Some((title, rest)) = result_parts(&message.body) {
            let proof = self
                .services
                .provider
                .message_proof(&message.group_id, &message.id);
            let finished_run = self.runs.iter().any(|r| {
                r.bot_id == message.author_id
                    && r.status == RunStatus::Done
                    && r.ended_at
                        .is_some_and(|end| (end - message.created_at).abs() <= 10 * 60_000)
            });
            if !proof.is_empty() || finished_run {
                return Some(Special::Done { title, rest, proof });
            }
        }
        None
    }

    /// A row that replaces the whole message in the v2 style, or `None` for
    /// a normal message.
    pub(super) fn v2_special_row(
        &mut self,
        message: &Message,
        key: &str,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let special = self.v2_special(message)?;
        let author = self.v2_member(&message.author_id).cloned();
        Some(match special {
            Special::Routed(to) => self.v2_routing_row(message, author.as_ref(), &to, cx),
            Special::Decision(text) => self.v2_decision_row(message, author.as_ref(), &text, cx),
            Special::Done { title, rest, proof } => {
                self.v2_done_card(message, key, author.as_ref(), title, rest, proof, cx)
            }
        })
    }

    /// A quiet one-line system row (NATIVE.md §9.2: 转给, 选了): Callout
    /// `secondary` text in the text column, the time after it in `tertiary`.
    fn system_row(
        lead: Option<AnyElement>,
        text: String,
        at: crate::data::Millis,
        cx: &App,
    ) -> gpui::Div {
        let k = native_colors(cx);
        div()
            .pl(GUTTER + space::S12)
            .min_h(px(20.))
            .flex()
            .items_center()
            .gap(space::S6)
            .n_callout()
            .text_color(k.secondary)
            .children(lead)
            .child(div().min_w_0().flex_shrink(1.).one_line().child(text))
            .child(
                div()
                    .flex_none()
                    .n_figures()
                    .text_color(k.tertiary)
                    .child(model::clock(at)),
            )
    }

    fn v2_routing_row(
        &self,
        message: &Message,
        author: Option<&Member>,
        to: &str,
        cx: &App,
    ) -> AnyElement {
        let text = fill(
            c::ROUTED,
            &[
                ("from", &author.map(|a| a.name.clone()).unwrap_or_default()),
                ("to", to),
            ],
        );
        div()
            .px(space::S8)
            .child(Self::system_row(None, text, message.created_at, cx))
            .into_any_element()
    }

    fn v2_decision_row(
        &self,
        message: &Message,
        author: Option<&Member>,
        text: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = native_colors(cx);
        let name = author.map(|a| a.name.clone()).unwrap_or_default();
        let line = format!("{name} {}", fill(c::PICKED, &[("text", text)]));
        // The check draws once, when the answer is new; a record read later
        // shows it whole. Rows show `checkmark` in `secondary` (§6).
        let mark = div()
            .flex_none()
            .size(px(14.))
            .child(ui::motion::check_mark(
                ElementId::Name(format!("v2-dec-check-{}", message.id).into()),
                ui::sym(Sym::Check, ui::SymSize::INLINE_SMALL, k.secondary).into_any_element(),
                px(14.),
                ui::motion::just_answered(message.created_at),
                cx,
            ))
            .into_any_element();
        let see = self.decision_question(message);
        div()
            .id(ElementId::Name(
                format!("v2-decision-{}", message.id).into(),
            ))
            .px(space::S8)
            .child(Self::system_row(Some(mark), line, message.created_at, cx))
            .when_some(see, |el, q| {
                el.tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(c::SEE_QUESTION).build(window, cx)
                })
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.show_message(&q, cx);
                }))
            })
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn v2_done_card(
        &self,
        message: &Message,
        key: &str,
        author: Option<&Member>,
        title: String,
        rest: String,
        mut proof: Vec<Proof>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.services.provider.clone();
        let changes = p.topic_changes(&message.group_id, &message.topic_id);
        // Derive what the provider did not report: the change of the 话题
        // and the 产出 the message points to.
        if !proof.iter().any(|x| matches!(x, Proof::Pr { .. }))
            && let Some(ch) = &changes
        {
            proof.insert(
                0,
                Proof::Pr {
                    number: ch.number,
                    additions: ch.additions,
                    deletions: ch.deletions,
                },
            );
        }
        for r in &message.artifact_refs {
            if !proof
                .iter()
                .any(|x| matches!(x, Proof::Artifact { artifact_id, .. } if *artifact_id == r.artifact_id))
            {
                let title = p
                    .artifacts(&message.group_id, &message.topic_id)
                    .into_iter()
                    .find(|a| a.id == r.artifact_id)
                    .map(|a| a.title)
                    .unwrap_or_default();
                proof.push(Proof::Artifact {
                    artifact_id: r.artifact_id.clone(),
                    title,
                    version: r.version,
                });
            }
        }
        let n = native_colors(cx);
        // Facts (NATIVE.md §9.2): symbol + text runs 12 pt apart, no chips.
        let fact = |glyph: AnyElement| {
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(space::S4)
                .n_callout()
                .n_figures()
                .text_color(n.secondary)
                .child(glyph)
        };
        let small = |sym: Sym, color: Hsla| {
            ui::sym(sym, ui::SymSize::INLINE_SMALL, color).into_any_element()
        };
        let facts: Vec<AnyElement> = proof
            .iter()
            .filter_map(|x| match x {
                Proof::Pr {
                    number,
                    additions,
                    deletions,
                } => Some(
                    fact(small(Sym::Branch, n.secondary))
                        .child(format!("#{number}"))
                        .child(div().text_color(n.green).child(format!("+{additions}")))
                        .child(div().text_color(n.red).child(format!("−{deletions}")))
                        .into_any_element(),
                ),
                Proof::Tests { passed, failed } => Some(
                    fact(if *failed == 0 {
                        small(Sym::Done, n.green)
                    } else {
                        small(Sym::Problem, n.red)
                    })
                    .child(if *failed == 0 {
                        fill(c::TESTS_PASSED, &[("n", &passed.to_string())])
                    } else {
                        fill(c::TESTS_FAILED, &[("failed", &failed.to_string())])
                    })
                    .into_any_element(),
                ),
                Proof::Recording { duration_ms } => Some(
                    fact(small(Sym::Play, n.secondary))
                        .child(fill(c::RECORDING, &[("time", &media_time(*duration_ms))]))
                        .into_any_element(),
                ),
                Proof::Checks { passed, total } => Some(
                    fact(small(Sym::Done, n.green))
                        .child(fill(
                            c::CHECKS_CHIP,
                            &[
                                ("passed", &passed.to_string()),
                                ("total", &total.to_string()),
                            ],
                        ))
                        .into_any_element(),
                ),
                Proof::Artifact { .. } => None,
            })
            .collect();

        let mut links: Vec<AnyElement> = Vec::new();
        if let Some(ch) = changes.filter(|_| proof.iter().any(|x| matches!(x, Proof::Pr { .. }))) {
            let state = match ch.state {
                ChangeState::Open => c::CHANGES_OPEN,
                ChangeState::Merging => c::CHANGES_MERGING,
                ChangeState::Merged => c::CHANGES_MERGED,
                ChangeState::Closed => c::CHANGES_CLOSED,
            };
            let open = cx.listener(|this, _: &ClickEvent, _, cx| {
                this.services
                    .nav
                    .update(cx, |nav, cx| nav.set_right_pane(RightPane::Changes, cx));
            });
            links.push(
                link_row(ElementId::Name(format!("v2-pr-{key}").into()), cx)
                    .child(ui::sym(Sym::PullRequest, ui::SymSize::INLINE, n.secondary))
                    .child(div().flex_none().n_figures().child(format!(
                        "{} #{}",
                        c::PR_LABEL,
                        ch.number
                    )))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .one_line()
                            .text_color(n.secondary)
                            .child(ch.title.clone()),
                    )
                    .when(ch.checks_total > 0, |el| {
                        el.child(
                            div()
                                .flex_none()
                                .n_subheadline()
                                .text_color(n.secondary)
                                .child(fill(
                                    c::PR_ROW_META,
                                    &[("n", &ch.checks_passed.to_string()), ("state", state)],
                                )),
                        )
                    })
                    .child(open_arrow(
                        ElementId::Name(format!("v2-pr-open-{key}").into()),
                        c::OPEN_CHANGES,
                        cx,
                    ))
                    .on_click(open)
                    .into_any_element(),
            );
        }
        for x in &proof {
            let Proof::Artifact {
                artifact_id,
                title,
                version,
            } = x
            else {
                continue;
            };
            let kind = p
                .artifacts(&message.group_id, &message.topic_id)
                .into_iter()
                .find(|a| a.id == *artifact_id)
                .map(|a| kind_text(a.kind))
                .unwrap_or(c::KIND_PAGE);
            let pane = RightPane::Artifact {
                id: artifact_id.clone(),
                version: Some(*version),
            };
            links.push(
                link_row(
                    ElementId::Name(format!("v2-out-{key}-{artifact_id}").into()),
                    cx,
                )
                .child(ui::sym(Sym::Document, ui::SymSize::INLINE, n.secondary))
                .child(
                    div()
                        .min_w_0()
                        .flex_shrink(1.)
                        .one_line()
                        .child(fill(c::OUTPUT_ROW, &[("title", title)])),
                )
                .child(
                    div()
                        .flex_none()
                        .n_subheadline()
                        .n_figures()
                        .text_color(n.secondary)
                        .child(fill(
                            c::OUTPUT_META,
                            &[("v", &version.to_string()), ("kind", kind)],
                        )),
                )
                .child(div().flex_1())
                .child(open_arrow(
                    ElementId::Name(format!("v2-out-open-{key}-{artifact_id}").into()),
                    c::OPEN_OUTPUT,
                    cx,
                ))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let pane = pane.clone();
                    this.services
                        .nav
                        .update(cx, |nav, cx| nav.set_right_pane(pane, cx));
                }))
                .into_any_element(),
            );
        }

        let body = (!rest.is_empty()).then(|| {
            markdown::message_body(
                ElementId::Name(format!("v2-done-body-{key}").into()),
                rest.clone(),
                &markdown::mention_targets(&self.members, self.me.as_deref()),
            )
            .w_full()
            .text_color(n.label)
        });
        // The state of the result: `checkmark.circle.fill` in `green` and
        // its word, before the title (no chip).
        let state = div()
            .flex()
            .items_center()
            .gap(space::S4)
            .n_callout()
            .text_color(n.secondary)
            .child(small(Sym::Done, n.green))
            .child(c::DONE_MARK);
        let card = div()
            .w_full()
            .max_w(TEXT_MAX)
            .rounded(px(12.))
            .bg(n.fill)
            .overflow_hidden()
            .p(px(14.))
            .flex()
            .flex_col()
            .child(state)
            .child(
                div()
                    .mt(space::S4)
                    .n_title3()
                    .text_color(n.label)
                    .child(title),
            )
            .children(body.map(|b| div().mt(space::S6).child(b)))
            .when(!facts.is_empty(), |el| {
                el.child(
                    div()
                        .mt(space::S10)
                        .flex()
                        .flex_wrap()
                        .gap_x(space::S12)
                        .gap_y(space::S4)
                        .children(facts),
                )
            })
            .when(!links.is_empty(), |el| {
                el.child(
                    div().mt(space::S10).flex().flex_col().children(
                        links
                            .into_iter()
                            .map(|l| div().border_t(px(1.)).border_color(n.separator).child(l)),
                    ),
                )
            });
        div()
            .px(space::S8)
            .pt(space::S4)
            .pb(space::S4)
            .flex()
            .gap(space::S12)
            .child(
                div()
                    .flex_none()
                    .child(ui::MemberAvatar::new(author, GUTTER)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(space::S6)
                    .child(self.v2_meta(author, message.created_at, cx))
                    .child(card),
            )
            .into_any_element()
    }

    /// The v2 file cards of a message whose files are not images: a 72x44
    /// thumbnail (a dark screen with a play mark for a video), the name and
    /// the mono length and size.
    pub(super) fn v2_files(&self, message: &Message, cx: &mut Context<Self>) -> Option<AnyElement> {
        if message.attachments.is_empty()
            || message
                .attachments
                .iter()
                .any(|f| f.is_image() || !f.is_uploaded())
        {
            return None;
        }
        let k = theme::colors(cx);
        let n = native_colors(cx);
        let p = self.services.provider.clone();
        Some(
            div()
                .mt(space::S6)
                .flex()
                .flex_col()
                .gap(space::S8)
                .children(message.attachments.iter().enumerate().map(|(i, file)| {
                    let video = file.mime.starts_with("video/") || file.mime.starts_with("audio/");
                    let meta = match p.media_duration(file) {
                        Some(ms) => format!("{} · {}", media_time(ms), size_text(file.size)),
                        None => size_text(file.size),
                    };
                    // inc-host: the poster frame (the sender's, else made here).
                    let poster = (file.mime.starts_with("video/"))
                        .then(|| p.file_poster(&message.group_id, &message.topic_id, file))
                        .flatten();
                    let thumb = if video {
                        div()
                            .flex_none()
                            .relative()
                            .w(px(72.))
                            .h(px(44.))
                            .rounded(px(6.))
                            .bg(k.screen)
                            .children(poster.map(|path| {
                                gpui::StyledImage::object_fit(
                                    gpui::img(path),
                                    gpui::ObjectFit::Cover,
                                )
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full()
                            }))
                            // The screen is the card's own color in dark.
                            .when(theme::is_dark(cx), |el| {
                                el.border_1().border_color(k.line_2)
                            })
                            .overflow_hidden()
                            .child(
                                div()
                                    .absolute()
                                    .right(px(5.))
                                    .bottom(px(5.))
                                    .size(px(16.))
                                    .rounded(px(8.))
                                    .bg(k.screen_ink)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(ui::icon(Sym::Play, px(8.), k.screen)),
                            )
                    } else {
                        div()
                            .flex_none()
                            .w(px(72.))
                            .h(px(44.))
                            .rounded(px(6.))
                            .bg(n.fill)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(ui::sym(Sym::File, ui::SymSize::SIDEBAR, n.secondary))
                    };
                    let file_c: FileAttachment = file.clone();
                    let (g, t) = (message.group_id.clone(), message.topic_id.clone());
                    let name: SharedString = file.name.clone().into();
                    div()
                        .id(ElementId::Name(
                            format!("v2-file-{}-{i}", message.id).into(),
                        ))
                        .w(px(300.))
                        .max_w_full()
                        .p(space::S6)
                        .pr(space::S12)
                        .rounded(px(10.))
                        .bg(n.fill)
                        .flex()
                        .items_center()
                        .gap(space::S10)
                        .child(thumb)
                        .child(
                            div()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(div().n_body().text_color(n.label).one_line().child(name))
                                .child(
                                    div()
                                        .n_subheadline()
                                        .n_figures()
                                        .text_color(n.secondary)
                                        .child(meta),
                                ),
                        )
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            open_file(
                                &this.services.provider,
                                &g,
                                &t,
                                &file_c,
                                OpenAction::Open,
                                cx,
                            );
                        }))
                }))
                .into_any_element(),
        )
    }
}

/// A 32 pt link row inside a done card: Body text, no hover fill.
fn link_row(id: ElementId, cx: &App) -> gpui::Stateful<gpui::Div> {
    let k = native_colors(cx);
    div()
        .id(id)
        .w_full()
        .h(px(32.))
        .flex()
        .items_center()
        .gap(space::S8)
        .n_body()
        .text_color(k.label)
}

/// The trailing `arrow.up.forward` borderless button of a row that opens
/// something in the right pane (NATIVE.md §9.3: no text links).
fn open_arrow(id: ElementId, name: &'static str, cx: &App) -> AnyElement {
    let k = native_colors(cx);
    div()
        .id(id)
        .flex_none()
        .size(px(24.))
        .mr(px(-6.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .hover(|s| s.bg(k.fill_hover))
        .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(name).build(window, cx))
        .child(ui::sym(Sym::OpenExternal, ui::SymSize::INLINE, k.secondary))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bold_first_line_is_the_result_title() {
        assert_eq!(
            result_parts("**改好了：不再重复**\n\n原因是…"),
            Some(("改好了：不再重复".into(), "原因是…".into()))
        );
        assert_eq!(result_parts("收到，先复现。"), None);
        assert_eq!(result_parts("**半截"), None);
    }
}
