//! The v2 timeline style of the 话题 page (Topic.dc.html, 1.1.0): a 720 pt
//! column with a 28 pt gutter, the full date divider, 岗位 tags, real file
//! cards, and three compact rows built from the messages themselves:
//!
//! - a routing line (`Jojo 转给了 Forge`) for a member's hand-off message;
//! - the decision record (`Joye 选了「…」`) for a message that made a
//!   decision of the 话题;
//! - the done card of a result message: title, why, proof chips, and links
//!   to the change and the 产出 (in the right pane).
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
    ui::{self, Lucide},
    views::{
        files::{OpenAction, open_file, size_text},
        topic_rail::{kind_text, media_time},
    },
};

/// The v2 gutter: avatars and tiles are 28 pt.
pub const GUTTER: Pixels = px(28.);
/// The column of a v2 timeline (and of the pinned card and composer).
pub const COLUMN: Pixels = px(720.);
/// Side padding of the v2 column.
pub const SIDE: Pixels = px(32.);
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

/// The v2 date divider: 12/18 `ink-3` between two `line` hairlines.
pub fn date_divider(at: crate::data::Millis, first: bool, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let row = div()
        .when(!first, |el| el.mt(space::S16))
        .mb(space::S4)
        .px(space::S8)
        .h(px(18.))
        .flex()
        .items_center()
        .gap(space::S12)
        .t_meta()
        .line_height(px(18.))
        .text_color(k.ink_3)
        .child(div().flex_1().h(theme::BORDER).bg(k.line))
        .child(date_label(at))
        .child(div().flex_1().h(theme::BORDER).bg(k.line));
    row.into_any_element()
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

fn chip(cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    div()
        .flex()
        .items_center()
        .gap(space::S6)
        .h(px(26.))
        .px(px(9.))
        .rounded(px(7.))
        .border_1()
        .border_color(k.line)
        .t_meta()
        .text_color(k.ink_2)
}

/// A decision made this recently draws its check (CONCEPT2 motion: once,
/// when answered); older records show it drawn.
pub fn decision_is_fresh(at: crate::data::Millis, now: crate::data::Millis) -> bool {
    (0..FRESH_DECISION_MS).contains(&(now - at))
}

const FRESH_DECISION_MS: i64 = 5_000;

fn check(size: Pixels, color: Hsla) -> AnyElement {
    ui::icon(Lucide::Check, size, color).into_any_element()
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

    /// The meta line of a v2 message: name 13/500, the 岗位 (a tag for a
    /// member that works for the team, plain `ink-3` text for a person),
    /// and the mono time.
    pub(super) fn v2_meta(
        &self,
        author: Option<&Member>,
        at: crate::data::Millis,
        cx: &App,
    ) -> AnyElement {
        let k = theme::colors(cx);
        let role = author
            .and_then(|a| a.role.clone())
            .filter(|r| !r.trim().is_empty());
        let bot = author.is_some_and(Member::is_bot);
        div()
            .h(px(20.))
            .min_w_0()
            .flex()
            .items_center()
            .gap(space::S8)
            .child(
                ui::truncating(
                    "sender-name",
                    author.map(|m| m.name.clone()).unwrap_or_default(),
                )
                .flex_shrink(1.)
                .max_w(px(160.))
                .t_small()
                .medium()
                .text_color(k.ink),
            )
            .when_some(role, |el, role| {
                if bot {
                    el.child(ui::JobTag::new(role))
                } else {
                    el.child(
                        div()
                            .flex_shrink(1.)
                            .min_w(px(40.))
                            .max_w(px(200.))
                            .one_line()
                            .t_meta()
                            .text_color(k.ink_3)
                            .child(role),
                    )
                }
            })
            .child(
                div()
                    .flex_none()
                    .t_mono()
                    .text_color(k.ink_3)
                    .child(model::clock(at)),
            )
            .into_any_element()
    }

    /// A row that replaces the whole message in the v2 style, or `None` for
    /// a normal message.
    pub(super) fn v2_special_row(
        &mut self,
        message: &Message,
        key: &str,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if message.deleted_at.is_some() || message.id.is_empty() {
            return None;
        }
        let author = self.v2_member(&message.author_id).cloned();
        let bot = author.as_ref().is_some_and(Member::is_bot);
        let has_card = self
            .services
            .provider
            .card(&message.group_id, &message.id)
            .is_some();
        if has_card {
            return None;
        }
        if bot && let Some(to) = routed_to(message, &self.members) {
            return Some(self.v2_routing_row(message, author.as_ref(), &to.name.clone(), cx));
        }
        if !bot
            && let Some(d) = self
                .services
                .provider
                .topic_decisions(&message.group_id, &message.topic_id)
                .into_iter()
                .find(|d| d.by == message.author_id && (d.at - message.created_at).abs() < 60_000)
        {
            return Some(self.v2_decision_row(message, author.as_ref(), &d.text, cx));
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
                return Some(self.v2_done_card(
                    message,
                    key,
                    author.as_ref(),
                    title,
                    rest,
                    proof,
                    cx,
                ));
            }
        }
        None
    }

    fn v2_routing_row(
        &self,
        message: &Message,
        author: Option<&Member>,
        to: &str,
        cx: &App,
    ) -> AnyElement {
        let k = theme::colors(cx);
        div()
            .mt(px(-6.))
            .px(space::S8)
            .flex()
            .items_center()
            .gap(space::S12)
            .t_meta()
            .line_height(px(20.))
            .text_color(k.ink_3)
            .child(
                div()
                    .flex_none()
                    .w(GUTTER)
                    .flex()
                    .justify_center()
                    .child(ui::MemberAvatar::new(author, px(16.))),
            )
            .child(fill(
                c::ROUTED,
                &[
                    ("from", &author.map(|a| a.name.clone()).unwrap_or_default()),
                    ("to", to),
                ],
            ))
            .child(div().mono().child(model::clock(message.created_at)))
            .into_any_element()
    }

    fn v2_decision_row(
        &self,
        message: &Message,
        author: Option<&Member>,
        text: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = theme::colors(cx);
        let see = self.decision_question(message).map(|q| {
            div()
                .id(ElementId::Name(format!("v2-see-q-{}", message.id).into()))
                .flex_none()
                .h(px(26.))
                .px(space::S8)
                .rounded(theme::radius::SM)
                .flex()
                .items_center()
                .t_meta()
                .text_color(k.ink_2)
                .cursor_pointer()
                .hover(|s| s.bg(k.hover))
                .child(c::SEE_QUESTION)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.show_message(&q, cx);
                }))
        });
        div()
            .px(space::S8)
            .flex()
            .items_center()
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
                    .h(px(40.))
                    .pl(space::S12)
                    .pr(space::S8)
                    .rounded(theme::radius::FIELD)
                    .bg(k.sunken)
                    .flex()
                    .items_center()
                    .gap(space::S10)
                    .t_small()
                    .text_color(k.ink)
                    // The green check draws once, when the answer is new;
                    // a record read later shows it whole.
                    .child(div().flex_none().size(px(14.)).child(
                        if decision_is_fresh(message.created_at, crate::selftest::clock::now_ms()) {
                            ui::motion::check_draw(
                                ElementId::Name(format!("v2-dec-check-{}", message.id).into()),
                                check(px(14.), k.ok),
                                px(14.),
                                cx,
                            )
                        } else {
                            check(px(14.), k.ok)
                        },
                    ))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .gap(space::S4)
                            .child(
                                div()
                                    .flex_none()
                                    .medium()
                                    .child(author.map(|a| a.name.clone()).unwrap_or_default()),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .one_line()
                                    .child(fill(c::PICKED, &[("text", text)])),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .t_mono()
                            .text_color(k.ink_3)
                            .child(model::clock(message.created_at)),
                    )
                    .children(see),
            )
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
        let k = theme::colors(cx);
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
        let chips: Vec<AnyElement> = proof
            .iter()
            .filter_map(|x| match x {
                Proof::Pr {
                    number,
                    additions,
                    deletions,
                } => Some(
                    chip(cx)
                        .mono()
                        .child(ui::icon(Lucide::GitBranch, px(12.), k.ink_2))
                        .child(format!("#{number}"))
                        .child(div().text_color(k.ok_ink).child(format!("+{additions}")))
                        .child(div().text_color(k.bad_ink).child(format!("−{deletions}")))
                        .into_any_element(),
                ),
                Proof::Tests { passed, failed } => Some(
                    chip(cx)
                        .child(if *failed == 0 {
                            check(px(12.), k.ok)
                        } else {
                            ui::icon(Lucide::CircleAlert, px(12.), k.bad).into_any_element()
                        })
                        .child(if *failed == 0 {
                            fill(c::TESTS_PASSED, &[("n", &passed.to_string())])
                        } else {
                            fill(c::TESTS_FAILED, &[("failed", &failed.to_string())])
                        })
                        .into_any_element(),
                ),
                Proof::Recording { duration_ms } => Some(
                    chip(cx)
                        .child(ui::icon(Lucide::Play, px(11.), k.ink_2))
                        .child(fill(c::RECORDING, &[("time", &media_time(*duration_ms))]))
                        .into_any_element(),
                ),
                Proof::Checks { passed, total } => Some(
                    chip(cx)
                        .child(check(px(12.), k.ok))
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
            links.push(
                link_row(ElementId::Name(format!("v2-pr-{key}").into()), cx)
                    .child(ui::icon(Lucide::GitPullRequest, px(15.), k.ink_3))
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_baseline()
                            .gap(space::S4)
                            .medium()
                            .child(c::PR_LABEL)
                            .child(div().t_mono().child(format!("#{}", ch.number))),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .one_line()
                            .text_color(k.ink_2)
                            .child(ch.title.clone()),
                    )
                    .when(ch.checks_total > 0, |el| {
                        el.child(div().flex_none().t_meta().text_color(k.ink_3).child(fill(
                            c::PR_ROW_META,
                            &[("n", &ch.checks_passed.to_string()), ("state", state)],
                        )))
                    })
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.services
                            .nav
                            .update(cx, |nav, cx| nav.set_right_pane(RightPane::Changes, cx));
                    }))
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
                .when(!links.is_empty(), |el| el.border_t_1().border_color(k.line))
                .child(ui::icon(Lucide::FileText, px(15.), k.ink_3))
                .child(
                    div()
                        .min_w_0()
                        .flex_shrink(1.)
                        .one_line()
                        .medium()
                        .child(fill(c::OUTPUT_ROW, &[("title", title)])),
                )
                .child(div().flex_none().t_mono().text_color(k.ink_3).child(fill(
                    c::OUTPUT_META,
                    &[("v", &version.to_string()), ("kind", kind)],
                )))
                .child(div().flex_1())
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(space::S4)
                        .t_meta()
                        .text_color(k.ink_2)
                        .child(c::OPEN_RIGHT)
                        .child(ui::icon(Lucide::ChevronRight, px(12.), k.ink_2)),
                )
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
            .text_color(k.ink_2)
        });
        let header = div()
            .flex()
            .items_center()
            .gap(space::S8)
            .t_meta()
            .line_height(px(18.))
            .text_color(k.ink_3)
            .child(
                div()
                    .t_small()
                    .medium()
                    .text_color(k.ink)
                    .child(author.map(|a| a.name.clone()).unwrap_or_default()),
            )
            .children(
                author
                    .and_then(|a| a.role.clone())
                    .filter(|r| !r.trim().is_empty())
                    .map(ui::JobTag::new),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S4)
                    .text_color(k.ink_2)
                    .child(check(px(12.), k.ok))
                    .child(c::DONE_MARK),
            )
            .child(div().flex_1())
            .child(div().mono().child(model::clock(message.created_at)));
        let card = ui::card(cx).flex_1().min_w_0().overflow_hidden().child(
            div()
                .pt(space::S14)
                .px(space::S16)
                .pb(space::S16)
                .child(header)
                .child(
                    div()
                        .mt(space::S10)
                        .t_title()
                        .semibold()
                        .text_color(k.ink)
                        .child(title),
                )
                .children(body.map(|b| div().mt(space::S6).child(b)))
                .when(!chips.is_empty(), |el| {
                    el.child(
                        div()
                            .mt(space::S12)
                            .flex()
                            .flex_wrap()
                            .gap(space::S6)
                            .children(chips),
                    )
                })
                .when(!links.is_empty(), |el| {
                    el.child(
                        div()
                            .mt(space::S12)
                            .rounded(theme::radius::FIELD)
                            .border_1()
                            .border_color(k.line)
                            .overflow_hidden()
                            .children(links),
                    )
                }),
        );
        div()
            .px(space::S8)
            .flex()
            .gap(space::S12)
            .child(
                div()
                    .flex_none()
                    .child(ui::MemberAvatar::new(author, GUTTER)),
            )
            .child(card)
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
        let p = self.services.provider.clone();
        Some(
            div()
                .mt(space::S10)
                .flex()
                .flex_col()
                .gap(space::S8)
                .children(message.attachments.iter().enumerate().map(|(i, file)| {
                    let video = file.mime.starts_with("video/") || file.mime.starts_with("audio/");
                    let meta = match p.media_duration(file) {
                        Some(ms) => format!("{} · {}", media_time(ms), size_text(file.size)),
                        None => size_text(file.size),
                    };
                    let thumb = if video {
                        div()
                            .flex_none()
                            .relative()
                            .w(px(72.))
                            .h(px(44.))
                            .rounded(theme::radius::SM)
                            .bg(k.screen)
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
                                    .child(ui::icon(Lucide::Play, px(8.), k.screen)),
                            )
                    } else {
                        div()
                            .flex_none()
                            .w(px(72.))
                            .h(px(44.))
                            .rounded(theme::radius::SM)
                            .bg(k.sunken)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(ui::icon(Lucide::File, px(18.), k.ink_3))
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
                        .py(space::S6)
                        .pl(space::S6)
                        .pr(space::S12)
                        .rounded(theme::radius::FIELD)
                        .border_1()
                        .border_color(k.line)
                        .bg(k.bg)
                        .shadow(k.lift())
                        .flex()
                        .items_center()
                        .gap(space::S12)
                        .cursor_pointer()
                        .hover(|s| s.bg(k.hover))
                        .child(thumb)
                        .child(
                            div()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .t_small()
                                        .medium()
                                        .text_color(k.ink)
                                        .one_line()
                                        .child(name),
                                )
                                .child(
                                    div()
                                        .t_mono()
                                        .line_height(px(18.))
                                        .text_color(k.ink_3)
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
                })),
        )
        .map(IntoElement::into_any_element)
    }
}

/// A 38 pt link row inside a done card.
fn link_row(id: ElementId, cx: &App) -> gpui::Stateful<gpui::Div> {
    let k = theme::colors(cx);
    div()
        .id(id)
        .w_full()
        .h(px(38.))
        .px(space::S12)
        .flex()
        .items_center()
        .gap(space::S10)
        .t_small()
        .text_color(k.ink)
        .cursor_pointer()
        .hover(|s| s.bg(k.hover))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_new_decision_draws_its_check() {
        assert!(decision_is_fresh(1_000, 2_000));
        assert!(!decision_is_fresh(1_000, 1_000 + 60_000));
        assert!(!decision_is_fresh(5_000, 1_000));
    }

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
