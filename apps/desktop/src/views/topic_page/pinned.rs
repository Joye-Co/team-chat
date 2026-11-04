//! The banner above the composer (§2.15, §2.16): the newest open ask for
//! me in this 话题, in the Alert anatomy. A review of the 话题's change
//! shows 看改动, 要求修改… and the default 批准合并; another ask offers 查看,
//! which scrolls to its card (the card holds its buttons). Once I
//! answer, the controls give way to a one-line record that rises in with its
//! check drawn, and 撤回 while my approval is still queued on this computer
//! (撤回 can take back only a queued answer).

use gpui::{
    AnyElement, ClickEvent, Context, ElementId, IntoElement, ParentElement, SharedString, Styled,
    Window, div,
};

use super::TopicPage;
use crate::{
    app::RightPane,
    copy::{common, fill, topic_rail as c},
    data::{
        Millis,
        v2::{AskItem, AskKind, ChangeState},
    },
    ui::{
        Sym,
        native::{Alert, AlertTone, PushSize, PushStyle, native_colors, push_button},
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
    /// Another ask: its card in the thread holds the buttons.
    Ask { ask: AskItem },
    /// My answer, or someone's: the one-line record. `merging` while the
    /// approved change merges (never a done check before it is merged).
    Record {
        text: String,
        at: Millis,
        withdraw: bool,
        merging: bool,
    },
}

/// 撤回 shows on my approval only while it is queued here and the change has
/// not merged: once the server stored it, it cannot be taken back.
fn can_withdraw(mine: bool, state: ChangeState, queued: bool) -> bool {
    mine && queued && state != ChangeState::Merged
}

/// `刚刚` within a minute, else the clock; `None` for a missing time
/// (never `01-01`).
fn when(at: Millis) -> Option<String> {
    crate::views::shell::known(at)?;
    let now = crate::selftest::clock::utc_now().timestamp_millis();
    Some(if (now - at).abs() < 60_000 {
        common::JUST_NOW.to_string()
    } else {
        clock(at)
    })
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
                merging: ch.state == ChangeState::Merging,
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
                    merging: false,
                });
            }
        }
        Some(Pinned::Ask { ask })
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

    /// The banner above the composer (§2.15, §2.16), or `None` when
    /// nothing waits on me here: the Alert anatomy at x 288–1096 (radius 10,
    /// a 1 px border, px 16, py 12, a 16 state icon, title 14 medium,
    /// description 14 muted, sm actions at the right). An ask whose card is
    /// in the thread offers 查看 only: the card stays the one surface of
    /// its actions.
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
        let sm = |id: &str, label: &str, style: PushStyle, cx: &mut Context<Self>| {
            push_button(
                ElementId::Name(SharedString::from(id.to_string())),
                label.to_string(),
                style,
                PushSize::Small,
                cx,
            )
        };
        let body: AnyElement = match state {
            Pinned::Record {
                text,
                at,
                withdraw,
                merging,
            } => {
                let mut alert = Alert::new(if merging { Sym::Merge } else { Sym::Done }, text)
                    .icon_color(if merging { k.blue } else { k.green });
                if let Some(when) = when(at) {
                    alert = alert.description(when);
                }
                if withdraw {
                    alert = alert.action(sm("pinned-withdraw", c::WITHDRAW, PushStyle::Outline, cx).on_click(
                        cx.listener({
                            let (g, t) = (g.clone(), t.clone());
                            move |this, _: &ClickEvent, _, cx| this.withdraw(g.clone(), t.clone(), cx)
                        }),
                    ));
                }
                alert.into_any_element()
            }
            Pinned::Review {
                number,
                files,
                merger,
            } => Alert::new(
                Sym::NeedsYou,
                fill(c::REVIEW_TITLE, &[("n", &number.to_string())]),
            )
            .tone(AlertTone::Warning)
            .description(fill(
                c::REVIEW_LINE,
                &[("files", &files.to_string()), ("name", &merger)],
            ))
            .action(
                sm("pinned-see-changes", c::SEE_CHANGES, PushStyle::Outline, cx).on_click(
                    cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.services
                            .nav
                            .update(cx, |nav, cx| nav.set_right_pane(RightPane::Changes, cx));
                    }),
                ),
            )
            .action(
                sm("pinned-request", c::REQUEST_CHANGES_MORE, PushStyle::Outline, cx).on_click(
                    cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.request_changes(window, cx)
                    }),
                ),
            )
            .action(
                sm("pinned-approve", c::APPROVE_MERGE, PushStyle::Default, cx).on_click(
                    cx.listener({
                        let (g, t) = (g.clone(), t.clone());
                        move |this, _: &ClickEvent, _, cx| this.approve(g.clone(), t.clone(), cx)
                    }),
                ),
            )
            .into_any_element(),
            Pinned::Ask { ask } => {
                let from = fill(
                    c::ASK_FROM,
                    &[("name", &member_name(&self.services, &ask.from))],
                );
                let line = match when(ask.at) {
                    Some(at) => format!("{from} · {at}"),
                    None => from,
                };
                let mut alert = Alert::new(Sym::NeedsYou, ask.title.clone())
                    .tone(AlertTone::Warning)
                    .description(line);
                if let Some(id) = ask.message_id.clone() {
                    let (g, t) = (g.clone(), t.clone());
                    alert = alert.action(
                        sm("pinned-see", c::SEE_QUESTION_SHORT, PushStyle::Outline, cx).on_click(
                            cx.listener(move |this, _: &ClickEvent, _, cx| {
                                let (g, t, id) = (g.clone(), t.clone(), id.clone());
                                this.services
                                    .nav
                                    .update(cx, |nav, cx| nav.open_message(g, t, id, 0, cx));
                            }),
                        ),
                    );
                }
                alert.into_any_element()
            }
        };
        let _ = window;
        Some(div().w_full().child(body).into_any_element())
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
