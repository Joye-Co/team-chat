//! The decision bar above the composer (NATIVE.md §9.2): the newest open
//! ask for me in this 话题, a row on `fill` with radius 12. A review of the
//! 话题's change shows 看改动, 要求修改… and the default 批准合并; another
//! ask shows its card's buttons as push buttons. Once I
//! answer, the controls give way to a one-line record that rises in with its
//! check drawn, and 撤回 while my approval is still queued on this computer
//! (撤回 can take back only a queued answer).

use gpui::{
    AnyElement, ClickEvent, Context, ElementId, IntoElement, ParentElement, SharedString, Styled,
    Window, div, px,
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
    theme::{Type as _, space},
    ui::{
        self, Sym,
        native::{NativeType as _, PushSize, PushStyle, native_colors, push_button},
    },
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

/// 撤回 shows on my approval only while it is queued here and the change has
/// not merged: once the server stored it, it cannot be taken back.
fn can_withdraw(mine: bool, state: ChangeState, queued: bool) -> bool {
    mine && queued && state != ChangeState::Merged
}

/// `刚刚` within a minute, else the mono clock.
fn when(at: Millis) -> String {
    let now = crate::selftest::clock::utc_now().timestamp_millis();
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
                withdraw: can_withdraw(mine, ch.state, p.changes_approval_queued(g, t)),
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

    /// The decision bar above the composer (NATIVE.md §9.2), or `None`
    /// when nothing waits on me here: a row on `fill` with radius 12, the
    /// state symbol, a Headline title over one `secondary` line, and
    /// trailing native push buttons with the default one last.
    pub(super) fn render_pinned(
        &mut self,
        g: &str,
        t: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let k = native_colors(cx);
        let state = self.pinned_state(g, t)?;
        let (g, t) = (g.to_string(), t.to_string());
        let needs_you = || ui::sym(Sym::NeedsYou, ui::SymSize::INLINE, k.amber).into_any_element();
        let bar =
            |symbol: AnyElement, title: String, line: Option<String>, buttons: Vec<AnyElement>| {
                div()
                    .w_full()
                    .min_h(px(52.))
                    .pl(space::S12)
                    .pr(space::S10)
                    .py(space::S8)
                    .rounded(px(12.))
                    .bg(k.fill)
                    .flex()
                    .items_center()
                    .gap(space::S10)
                    .child(div().flex_none().self_start().pt(px(1.)).child(symbol))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .n_headline()
                                    .text_color(k.label)
                                    .one_line()
                                    .child(title),
                            )
                            .children(line.map(|l| {
                                div()
                                    .n_subheadline()
                                    .n_figures()
                                    .text_color(k.secondary)
                                    .one_line()
                                    .child(l)
                            })),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(space::S8)
                            .children(buttons),
                    )
            };
        let body: AnyElement = match state {
            Pinned::Record { text, at, withdraw } => {
                let id = format!("pinned-record-{t}-{at}");
                let mark = ui::motion::check_mark(
                    ElementId::Name(format!("{id}-check").into()),
                    ui::sym(Sym::Done, ui::SymSize::INLINE, k.green).into_any_element(),
                    px(16.),
                    ui::motion::just_answered(at),
                    cx,
                )
                .into_any_element();
                let mut buttons = Vec::new();
                if withdraw {
                    buttons.push(
                        push_button(
                            "pinned-withdraw",
                            c::WITHDRAW,
                            PushStyle::Standard,
                            PushSize::Regular,
                            cx,
                        )
                        .on_click(cx.listener({
                            let (g, t) = (g.clone(), t.clone());
                            move |this, _: &ClickEvent, _, cx| {
                                this.withdraw(g.clone(), t.clone(), cx)
                            }
                        }))
                        .into_any_element(),
                    );
                }
                let row = bar(mark, text, Some(when(at)), buttons);
                ui::motion::rise(SharedString::from(id), 0, row, cx)
            }
            Pinned::Review {
                number,
                files,
                merger,
            } => bar(
                needs_you(),
                fill(c::REVIEW_TITLE, &[("n", &number.to_string())]),
                Some(fill(
                    c::REVIEW_LINE,
                    &[("files", &files.to_string()), ("name", &merger)],
                )),
                vec![
                    push_button(
                        "pinned-see-changes",
                        c::SEE_CHANGES,
                        PushStyle::Standard,
                        PushSize::Regular,
                        cx,
                    )
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.services
                            .nav
                            .update(cx, |nav, cx| nav.set_right_pane(RightPane::Changes, cx));
                    }))
                    .into_any_element(),
                    push_button(
                        "pinned-request",
                        c::REQUEST_CHANGES_MORE,
                        PushStyle::Standard,
                        PushSize::Regular,
                        cx,
                    )
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.request_changes(window, cx)
                    }))
                    .into_any_element(),
                    push_button(
                        "pinned-approve",
                        c::APPROVE_MERGE,
                        PushStyle::Default,
                        PushSize::Regular,
                        cx,
                    )
                    .on_click(cx.listener({
                        let (g, t) = (g.clone(), t.clone());
                        move |this, _: &ClickEvent, _, cx| this.approve(g.clone(), t.clone(), cx)
                    }))
                    .into_any_element(),
                ],
            )
            .into_any_element(),
            Pinned::Ask { ask, actions } => {
                let from = fill(
                    c::ASK_FROM,
                    &[("name", &member_name(&self.services, &ask.from))],
                );
                let message_id = ask.message_id.clone().unwrap_or_default();
                // At most one default button: the card's first primary.
                let default = actions.iter().position(|(_, _, primary)| *primary);
                let buttons = actions
                    .into_iter()
                    .enumerate()
                    .map(|(i, (id, label, _))| {
                        let style = if Some(i) == default {
                            PushStyle::Default
                        } else {
                            PushStyle::Standard
                        };
                        let (g, t, m) = (g.clone(), t.clone(), message_id.clone());
                        push_button(
                            ElementId::Name(format!("pinned-action-{i}").into()),
                            label,
                            style,
                            PushSize::Regular,
                            cx,
                        )
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.services.provider.press_card_action(&g, &t, &m, &id);
                            cx.notify();
                        }))
                        .into_any_element()
                    })
                    .collect();
                bar(
                    needs_you(),
                    ask.title.clone(),
                    Some(format!("{from} · {}", when(ask.at))),
                    buttons,
                )
                .into_any_element()
            }
        };
        let _ = window;
        // First paint: the bar rises in after the timeline rows.
        Some(ui::motion::rise(
            SharedString::from(format!("pinned-{t}")),
            6,
            div().w_full().child(body),
            cx,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_my_queued_approval_can_be_withdrawn() {
        assert!(can_withdraw(true, ChangeState::Merging, true));
        assert!(!can_withdraw(true, ChangeState::Merging, false));
        assert!(!can_withdraw(false, ChangeState::Merging, true));
        assert!(!can_withdraw(true, ChangeState::Merged, true));
    }
}
