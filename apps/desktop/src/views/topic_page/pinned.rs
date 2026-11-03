//! The pinned decision card above the composer (Topic.dc.html): the newest
//! open ask for me in this 话题. A review of the 话题's change shows 看改动,
//! 要求修改 and 批准合并; another ask shows its card's buttons. Once I
//! answer, the controls give way to a one-line record that rises in with its
//! check drawn, and 撤回 while the change has not merged.

use gpui::{
    AnyElement, ClickEvent, Context, ElementId, IntoElement, ParentElement, SharedString, Styled,
    Window, div, prelude::FluentBuilder as _, px,
};

use super::TopicPage;
use crate::{
    app::RightPane,
    copy::{common, fill, topic_rail as c},
    data::{
        Millis,
        card::ActionStyle,
        v2::{AskItem, AskKind, ChangeState},
    },
    theme::{self, Type as _, space},
    ui::{self, ButtonKind, ButtonSize, Lucide},
    views::shell::{clock, member_name},
};

/// What the pinned card shows.
enum Pinned {
    /// Review of the 话题's change: PR number, files, who merges.
    Review {
        number: u32,
        files: u32,
        merger: String,
    },
    /// Another ask, with the buttons of its card.
    Ask {
        ask: AskItem,
        actions: Vec<(String, String, bool)>,
    },
    /// My answer, or someone's: the one-line record.
    Record {
        text: String,
        at: Millis,
        withdraw: bool,
    },
}

/// `刚刚` within a minute, else the mono clock.
fn when(at: Millis) -> String {
    let now = chrono::Utc::now().timestamp_millis();
    if (now - at).abs() < 60_000 {
        common::JUST_NOW.to_string()
    } else {
        clock(at)
    }
}

impl TopicPage {
    fn pinned_state(&self, g: &str, t: &str) -> Option<Pinned> {
        let p = &self.services.provider;
        let me = self.services.me();
        let changes = p.topic_changes(g, t);
        let topic = p.topic(g, t);
        let merger = topic
            .as_ref()
            .and_then(|x| x.owner_id.clone())
            .map(|id| member_name(&self.services, &id))
            .unwrap_or_default();
        if let (Some((by, at)), Some(ch)) = (p.changes_approval(g, t), changes.as_ref())
            && matches!(ch.state, ChangeState::Merging | ChangeState::Merged)
        {
            let mine = me.as_deref() == Some(by.as_str());
            let text = match ch.state {
                ChangeState::Merged => fill(c::MERGED_LINE, &[("name", &merger)]),
                _ if mine => fill(c::APPROVED_YOU, &[("name", &merger)]),
                _ => fill(
                    c::APPROVED_BY,
                    &[
                        ("who", &member_name(&self.services, &by)),
                        ("name", &merger),
                    ],
                ),
            };
            return Some(Pinned::Record {
                text,
                at,
                withdraw: mine && ch.state == ChangeState::Merging,
            });
        }
        let ask = p.pinned_ask(g, t)?;
        if ask.kind == AskKind::Review
            && let Some(ch) = changes
        {
            return Some(Pinned::Review {
                number: ch.number,
                files: ch.files,
                merger,
            });
        }
        // My press on the ask's card is the record.
        if let Some(id) = &ask.message_id {
            let mine = me.as_deref();
            if let Some(press) = p
                .card_presses(g, id)
                .into_iter()
                .rev()
                .find(|x| Some(x.actor_id.as_str()) == mine)
            {
                return Some(Pinned::Record {
                    text: fill(
                        c::CHOSE_YOU,
                        &[("label", press.label.as_deref().unwrap_or(&press.action_id))],
                    ),
                    at: press.at,
                    withdraw: false,
                });
            }
        }
        let actions = ask
            .message_id
            .as_ref()
            .and_then(|id| p.card(g, id))
            .and_then(|card| card.actions.clone())
            .unwrap_or_default()
            .into_iter()
            .map(|a| {
                let primary = a.style == Some(ActionStyle::Primary);
                (a.id, a.label, primary)
            })
            .collect();
        Some(Pinned::Ask { ask, actions })
    }

    fn approve(&mut self, g: String, t: String, cx: &mut Context<Self>) {
        let reply = self.services.provider.approve_changes(&g, &t);
        cx.spawn(async move |_, _| {
            let _ = reply.await;
        })
        .detach();
        cx.notify();
    }

    fn withdraw(&mut self, g: String, t: String, cx: &mut Context<Self>) {
        let reply = self.services.provider.withdraw_changes_approval(&g, &t);
        cx.spawn(async move |_, _| {
            let _ = reply.await;
        })
        .detach();
        cx.notify();
    }

    fn request_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let owner = self
            .services
            .nav
            .read(cx)
            .topic
            .clone()
            .and_then(|(g, t)| self.services.provider.topic(&g, &t))
            .and_then(|t| t.owner_id)
            .map(|id| member_name(&self.services, &id));
        self.composer.update(cx, |composer, cx| {
            if let Some(name) = owner {
                composer.insert_text(&format!("@{name} "), window, cx);
            }
        });
    }

    /// The pinned card, or `None` when nothing waits on me here.
    pub(super) fn render_pinned(
        &mut self,
        g: &str,
        t: &str,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let k = theme::colors(cx);
        let state = self.pinned_state(g, t)?;
        let (g, t) = (g.to_string(), t.to_string());
        let dot = || div().flex_none().size(px(6.)).rounded(px(3.)).bg(k.you);
        let body: AnyElement = match state {
            Pinned::Record { text, at, withdraw } => {
                let id = format!("pinned-record-{t}-{at}");
                let row = div()
                    .flex_1()
                    .min_w_0()
                    .min_h(px(60.))
                    .flex()
                    .items_center()
                    .gap(space::S10)
                    .pl(space::S16)
                    .pr(space::S12)
                    .py(space::S10)
                    .bg(k.raised)
                    .t_small()
                    .text_color(k.ink)
                    .child(ui::motion::check_draw(
                        ElementId::Name(format!("{id}-check").into()),
                        ui::icon(Lucide::Check, px(16.), k.ok),
                        px(16.),
                        cx,
                    ))
                    .child(div().min_w_0().one_line().child(text))
                    .child(
                        div()
                            .flex_none()
                            .t_mono()
                            .text_color(k.ink_3)
                            .child(when(at)),
                    )
                    .child(div().flex_1())
                    .when(withdraw, |el| {
                        el.child(
                            ui::button(
                                "pinned-withdraw",
                                c::WITHDRAW,
                                ButtonKind::Ghost,
                                ButtonSize::Sm,
                                cx,
                            )
                            .on_click(cx.listener({
                                let (g, t) = (g.clone(), t.clone());
                                move |this, _: &ClickEvent, _, cx| {
                                    this.withdraw(g.clone(), t.clone(), cx)
                                }
                            })),
                        )
                    });
                ui::motion::rise(SharedString::from(id), 0, row, cx)
            }
            Pinned::Review {
                number,
                files,
                merger,
            } => div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(space::S12)
                .pl(space::S16)
                .pr(space::S12)
                .py(space::S10)
                .child(
                    div()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(space::S8)
                                .t_ui()
                                .line_height(px(22.))
                                .semibold()
                                .text_color(k.ink)
                                .child(dot())
                                .child(fill(c::REVIEW_TITLE, &[("n", &number.to_string())])),
                        )
                        .child(
                            div()
                                .pl(space::S14)
                                .t_meta()
                                .line_height(px(18.))
                                .text_color(k.ink_3)
                                .one_line()
                                .child(fill(
                                    c::REVIEW_LINE,
                                    &[("files", &files.to_string()), ("name", &merger)],
                                )),
                        ),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(space::S8)
                        .child(
                            ui::button(
                                "pinned-see-changes",
                                c::SEE_CHANGES,
                                ButtonKind::Ghost,
                                ButtonSize::Md,
                                cx,
                            )
                            .on_click(cx.listener(
                                |this, _: &ClickEvent, _, cx| {
                                    this.services.nav.update(cx, |nav, cx| {
                                        nav.set_right_pane(RightPane::Changes, cx)
                                    });
                                },
                            )),
                        )
                        .child(
                            ui::secondary_button("pinned-request", c::REQUEST_CHANGES, cx)
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.request_changes(window, cx)
                                })),
                        )
                        .child(
                            ui::primary_button("pinned-approve", c::APPROVE_MERGE, cx).on_click(
                                cx.listener({
                                    let (g, t) = (g.clone(), t.clone());
                                    move |this, _: &ClickEvent, _, cx| {
                                        this.approve(g.clone(), t.clone(), cx)
                                    }
                                }),
                            ),
                        ),
                )
                .into_any_element(),
            Pinned::Ask { ask, actions } => {
                let from = fill(
                    c::ASK_FROM,
                    &[("name", &member_name(&self.services, &ask.from))],
                );
                let message_id = ask.message_id.clone().unwrap_or_default();
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(space::S12)
                    .pl(space::S16)
                    .pr(space::S12)
                    .py(space::S10)
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(space::S8)
                                    .t_ui()
                                    .line_height(px(22.))
                                    .semibold()
                                    .text_color(k.ink)
                                    .child(dot())
                                    .child(div().min_w_0().one_line().child(ask.title.clone())),
                            )
                            .child(
                                div()
                                    .pl(space::S14)
                                    .t_meta()
                                    .line_height(px(18.))
                                    .text_color(k.ink_3)
                                    .one_line()
                                    .child(format!("{from} · {}", when(ask.at))),
                            ),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(space::S8)
                            .children(actions.into_iter().enumerate().map(
                                |(i, (id, label, primary))| {
                                    let kind = if primary {
                                        ButtonKind::Primary
                                    } else {
                                        ButtonKind::Secondary
                                    };
                                    let (g, t, m) = (g.clone(), t.clone(), message_id.clone());
                                    ui::button(
                                        ElementId::Name(format!("pinned-action-{i}").into()),
                                        label,
                                        kind,
                                        ButtonSize::Md,
                                        cx,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _: &ClickEvent, _, cx| {
                                            this.services
                                                .provider
                                                .press_card_action(&g, &t, &m, &id);
                                            cx.notify();
                                        },
                                    ))
                                },
                            )),
                    )
                    .into_any_element()
            }
        };
        let card = ui::card(cx)
            .min_h(px(62.))
            .flex()
            .items_stretch()
            .overflow_hidden()
            .child(body);
        // First paint: the card rises in after the timeline rows.
        Some(ui::motion::rise(
            SharedString::from(format!("pinned-{t}")),
            6,
            card,
            cx,
        ))
    }
}
