//! The 按顺序做 bar (docs/plans/usable/design-relay.zh.md §3.6) at the top of a 话题 whose
//! work goes from member to member in an order a person wrote (`@A → @B → @A`): the steps
//! with their state words, where it is (第 k/n 步), why it stopped, and 停止 / 重试.
//!
//! It shows while the relay runs or is paused, and for a day after it ended. The server
//! moves the relay on; this bar only reads `Topic.relay` and calls the two routes.

use gpui::{
    AnyElement, App, Context, InteractiveElement as _, IntoElement, ParentElement, Render,
    SharedString, Styled, Subscription, Window, div, prelude::FluentBuilder as _, px,
};

use gpui_component::Disableable as _;

use crate::{
    app::Services,
    copy::{fill, relay as c},
    data::{AdminError, Millis, TopicRelay},
    ui::native::{
        BadgeState, NativeType as _, PushSize, PushStyle, StatusBadge, native_colors, push_button,
        tokens::{radius, space},
    },
    views::{shell::panes::watch, timeline::v2 as timeline_v2},
};

/// An ended relay stays on the bar this long (`updatedAt`).
pub const SHOW_ENDED_MS: Millis = 24 * 60 * 60 * 1000;

/// One step as the bar draws it: the member's name and its state word (none while it has
/// not had its turn: the name alone, muted).
#[derive(Clone, Debug, PartialEq)]
pub struct StepView {
    pub name: String,
    pub word: Option<(&'static str, BadgeState)>,
}

/// What the bar shows of one relay.
#[derive(Clone, Debug, PartialEq)]
pub struct StripView {
    pub steps: Vec<StepView>,
    /// 第 k/n 步, or 都做完了.
    pub position: String,
    /// Why it stopped, or what the current step waits for.
    pub line: Option<String>,
    pub can_stop: bool,
    pub can_retry: bool,
}

/// The state word of a step; `None` for one that has not had its turn. An unknown state
/// reads as stopped.
pub fn step_word(state: &str) -> Option<(&'static str, BadgeState)> {
    match state {
        "pending" => None,
        "queued" => Some((c::RELAY_STEP_QUEUED, BadgeState::Queued)),
        "working" => Some((c::RELAY_STEP_WORKING, BadgeState::Working)),
        "done" => Some((c::RELAY_STEP_DONE, BadgeState::Done)),
        "failed" => Some((c::RELAY_STEP_BLOCKED, BadgeState::Failed)),
        _ => Some((c::RELAY_STEP_STOPPED, BadgeState::Deactivated)),
    }
}

/// Why a relay stopped, in words. `at` is the member of the step it stopped at, `before`
/// the member of the step before it.
pub fn reason_text(
    reason: Option<&str>,
    at: &str,
    before: &str,
    stopped_by: Option<&str>,
) -> String {
    let one = |t: &str, name: &str| fill(t, &[("name", name)]);
    match reason {
        Some("member_unavailable") => one(c::RELAY_WHY_UNAVAILABLE, at),
        Some("step_failed" | "step_blocked") => c::RELAY_WHY_BLOCKED.to_string(),
        Some("step_cancelled") => match stopped_by {
            Some(who) if !who.is_empty() => one(c::RELAY_WHY_STOPPED_BY, who),
            _ => c::RELAY_STEP_STOPPED.to_string(),
        },
        Some("limit") => c::RELAY_WHY_LIMIT.to_string(),
        Some("not_pushed") => one(c::RELAY_WHY_NOT_PUSHED, before),
        Some("requester_left") => c::RELAY_WHY_REQUESTER_LEFT.to_string(),
        Some("undelivered") => one(c::RELAY_WHY_UNDELIVERED, at),
        _ => c::RELAY_WHY_UNKNOWN.to_string(),
    }
}

/// The bar of `relay` at `now`, or `None` when it does not show (ended more than a day ago,
/// or no steps). `name` turns a member id into the name to show.
pub fn strip_view(
    relay: &TopicRelay,
    now: Millis,
    name: &dyn Fn(&str) -> String,
) -> Option<StripView> {
    let n = relay.steps.len();
    if n == 0 {
        return None;
    }
    let live = matches!(relay.state.as_str(), "running" | "paused");
    if !live && now.saturating_sub(relay.updated_at as Millis) > SHOW_ENDED_MS {
        return None;
    }
    let current = (relay.current as usize).min(n - 1);
    let at = name(&relay.steps[current].member_id);
    let before = current
        .checked_sub(1)
        .map(|i| name(&relay.steps[i].member_id))
        .unwrap_or_default();
    let stopped_by = relay.stopped_by.as_deref().map(name);
    let steps = relay
        .steps
        .iter()
        .map(|s| StepView {
            name: name(&s.member_id),
            word: step_word(&s.state),
        })
        .collect();
    let step_of = fill(
        c::RELAY_STEP_OF,
        &[("k", &(current + 1).to_string()), ("n", &n.to_string())],
    );
    let (position, line, can_stop, can_retry) = match relay.state.as_str() {
        "running" => {
            // The step's member still works on something else in this 话题.
            let line = (relay.steps[current].state == "queued")
                .then(|| fill(c::RELAY_WHY_QUEUED, &[("name", &at)]));
            (step_of, line, true, false)
        }
        "paused" => {
            let why = reason_text(relay.reason.as_deref(), &at, &before, stopped_by.as_deref());
            let line = fill(c::RELAY_STOPPED_AT, &[("name", &at), ("reason", &why)]);
            (step_of, Some(line), true, true)
        }
        "done" => (c::RELAY_ALL_DONE.to_string(), None, false, false),
        // `stopped`, or a state this version does not know: it stopped.
        _ => {
            let line = match stopped_by.as_deref() {
                Some(who) if !who.is_empty() => fill(c::RELAY_STOPPED_BY, &[("name", who)]),
                _ => {
                    let why = reason_text(relay.reason.as_deref(), &at, &before, None);
                    fill(c::RELAY_STOPPED_AT, &[("name", &at), ("reason", &why)])
                }
            };
            (step_of, Some(line), false, false)
        }
    };
    Some(StripView {
        steps,
        position,
        line,
        can_stop,
        can_retry,
    })
}

/// The bar above the open 话题's timeline; it draws nothing when the 话题 has no relay to show.
pub struct RelayStrip {
    services: Services,
    /// A 停止 or 重试 of this relay is in flight: its buttons wait.
    busy: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl RelayStrip {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        Self {
            _subscriptions: watch(&services, cx),
            services,
            busy: None,
        }
    }

    fn act(&mut self, relay: &TopicRelay, g: &str, t: &str, retry: bool, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(relay.id.clone());
        cx.notify();
        let p = &self.services.provider;
        let reply = if retry {
            p.retry_relay(g, t, &relay.id)
        } else {
            p.stop_relay(g, t, &relay.id)
        };
        cx.spawn(async move |this, cx| {
            let result = reply.await.unwrap_or(Err(AdminError::Other));
            let _ = this.update(cx, |this, cx| {
                this.busy = None;
                cx.notify();
                // `conflict`: it had already ended or moved on; the next `topic.updated`
                // shows where it is.
                if let Err(e) = result
                    && e != AdminError::Conflict
                    && let Some(window) = cx.active_window()
                {
                    let title = if retry {
                        c::RELAY_RETRY_REFUSED
                    } else {
                        c::RELAY_STOP_REFUSED
                    };
                    let _ = window.update(cx, |_, window, cx| {
                        crate::views::nav::dialogs::error_alert(
                            title,
                            crate::views::settings::widgets::error_text(e),
                            |_, _| {},
                            window,
                            cx,
                        )
                    });
                }
            });
        })
        .detach();
    }

    fn render_step(i: usize, step: &StepView, cx: &App) -> AnyElement {
        let k = native_colors(cx);
        div()
            .flex()
            .items_center()
            .gap(space::S6)
            .min_w(px(0.))
            .when(i > 0, |d| {
                d.child(div().text_color(k.tertiary).child(c::RELAY_ARROW))
            })
            .child(
                div()
                    .n_body()
                    .text_color(if step.word.is_some() {
                        k.label
                    } else {
                        k.tertiary
                    })
                    .child(crate::copy::isolate(crate::ui::avatar::name_beside_avatar(
                        &step.name,
                    ))),
            )
            .children(
                step.word
                    .map(|(word, state)| StatusBadge::new(state, word).dense()),
            )
            .into_any_element()
    }
}

impl Render for RelayStrip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some((g, t)) = self.services.nav.read(cx).topic.clone() else {
            return div().into_any_element();
        };
        let p = &self.services.provider;
        let Some(relay) = p.topic(&g, &t).and_then(|x| x.relay) else {
            return div().into_any_element();
        };
        let now = crate::selftest::clock::utc_now().timestamp_millis();
        let services = self.services.clone();
        let name = move |id: &str| crate::views::shell::member_name(&services, id);
        let Some(view) = strip_view(&relay, now, &name) else {
            return div().into_any_element();
        };
        let k = native_colors(cx);
        let busy = self.busy.as_deref() == Some(relay.id.as_str());
        let retry = view.can_retry.then(|| {
            let (r, g, t) = (relay.clone(), g.clone(), t.clone());
            push_button(
                "relay-retry",
                c::RELAY_RETRY,
                PushStyle::Outline,
                PushSize::Small,
                cx,
            )
            .loading(busy)
            .disabled(busy)
            .on_click(cx.listener(move |this, _, _, cx| this.act(&r, &g, &t, true, cx)))
        });
        let stop = view.can_stop.then(|| {
            let (r, g, t) = (relay.clone(), g.clone(), t.clone());
            push_button(
                "relay-stop",
                c::RELAY_STOP,
                PushStyle::Outline,
                PushSize::Small,
                cx,
            )
            .disabled(busy)
            .on_click(cx.listener(move |this, _, _, cx| this.act(&r, &g, &t, false, cx)))
        });
        let steps: Vec<AnyElement> = view
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| Self::render_step(i, s, cx))
            .collect();
        div()
            .id("relay-strip")
            .flex_none()
            .w_full()
            .pl(timeline_v2::LEADING)
            .pr(timeline_v2::SIDE)
            .pt(space::S8)
            .child(
                div()
                    .w_full()
                    .px(space::S16)
                    .py(space::S12)
                    .rounded(radius::CONTAINER)
                    .border_1()
                    .border_color(k.separator)
                    .bg(k.content)
                    .flex()
                    .flex_col()
                    .gap(space::S6)
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(space::S12)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .flex()
                                    .flex_wrap()
                                    .items_center()
                                    .gap(space::S6)
                                    .child(
                                        div()
                                            .n_headline()
                                            .text_color(k.label)
                                            .mr(space::S4)
                                            .child(c::RELAY_TITLE),
                                    )
                                    .children(steps),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .n_footnote()
                                    .text_color(k.secondary)
                                    .child(SharedString::from(view.position.clone())),
                            )
                            .when(retry.is_some() || stop.is_some(), |d| {
                                d.child(
                                    div()
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .gap(space::S8)
                                        .children(retry)
                                        .children(stop),
                                )
                            }),
                    )
                    .when_some(view.line.clone(), |d, line| {
                        d.child(div().n_body().text_color(k.secondary).child(line))
                    }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::TopicRelayStep;

    fn step(member: &str, state: &str) -> TopicRelayStep {
        TopicRelayStep {
            member_id: member.into(),
            note: None,
            state: state.into(),
            run_id: None,
        }
    }

    fn relay(state: &str, current: u32, steps: Vec<TopicRelayStep>) -> TopicRelay {
        TopicRelay {
            id: "r1".into(),
            started_by: "me".into(),
            goal: "加深色模式".into(),
            current,
            state: state.into(),
            reason: None,
            stopped_by: None,
            updated_at: 1_000,
            steps,
        }
    }

    fn name(id: &str) -> String {
        match id {
            "a" => "小码".into(),
            "b" => "小审".into(),
            "me" => "Joye".into(),
            _ => String::new(),
        }
    }

    #[test]
    fn a_running_relay_shows_each_step_and_where_it_is() {
        let r = relay(
            "running",
            1,
            vec![
                step("a", "done"),
                step("b", "working"),
                step("a", "pending"),
            ],
        );
        let v = strip_view(&r, 2_000, &name).unwrap();
        assert_eq!(v.position, "第 2/3 步");
        assert_eq!(
            v.steps,
            vec![
                StepView {
                    name: "小码".into(),
                    word: Some(("做完了", BadgeState::Done))
                },
                StepView {
                    name: "小审".into(),
                    word: Some(("在做", BadgeState::Working))
                },
                StepView {
                    name: "小码".into(),
                    word: None
                },
            ]
        );
        assert_eq!(v.line, None);
        assert!(v.can_stop && !v.can_retry);
    }

    #[test]
    fn a_queued_step_says_what_it_waits_for() {
        let r = relay("running", 1, vec![step("a", "done"), step("b", "queued")]);
        let v = strip_view(&r, 2_000, &name).unwrap();
        assert_eq!(v.steps[1].word, Some(("排队", BadgeState::Queued)));
        assert_eq!(v.line.as_deref(), Some("等 小审 手上的做完"));
    }

    #[test]
    fn a_paused_relay_says_why_and_offers_retry_and_stop() {
        let mut r = relay("paused", 1, vec![step("a", "done"), step("b", "failed")]);
        r.reason = Some("step_failed".into());
        let v = strip_view(&r, 2_000, &name).unwrap();
        assert_eq!(v.line.as_deref(), Some("停在 小审：卡住了"));
        assert_eq!(v.steps[1].word, Some(("卡住了", BadgeState::Failed)));
        assert!(v.can_stop && v.can_retry);
        r.reason = Some("not_pushed".into());
        let v = strip_view(&r, 2_000, &name).unwrap();
        assert_eq!(v.line.as_deref(), Some("停在 小审：小码 的代码还没推上去"));
        r.reason = Some("step_cancelled".into());
        r.stopped_by = Some("me".into());
        let v = strip_view(&r, 2_000, &name).unwrap();
        assert_eq!(v.line.as_deref(), Some("停在 小审：被 Joye 停止"));
        // A reason this version does not know reads as stopped.
        r.reason = Some("something_new".into());
        let v = strip_view(&r, 2_000, &name).unwrap();
        assert_eq!(v.line.as_deref(), Some("停在 小审：停住了"));
    }

    #[test]
    fn every_known_reason_has_words() {
        for (reason, words) in [
            ("member_unavailable", "小审 现在接不了这件事"),
            ("limit", "交出次数到上限了"),
            ("undelivered", "没有交到 小审 手上"),
            ("requester_left", "发起的人已经不在这个群"),
            ("step_blocked", "卡住了"),
        ] {
            assert_eq!(reason_text(Some(reason), "小审", "小码", None), words);
        }
    }

    #[test]
    fn a_stopped_relay_names_who_stopped_it_and_has_no_buttons() {
        let mut r = relay("stopped", 1, vec![step("a", "done"), step("b", "stopped")]);
        r.stopped_by = Some("me".into());
        let v = strip_view(&r, 2_000, &name).unwrap();
        assert_eq!(v.line.as_deref(), Some("Joye 停止了"));
        assert_eq!(v.steps[1].word, Some(("停止了", BadgeState::Deactivated)));
        assert!(!v.can_stop && !v.can_retry);
        // An unknown relay state reads as stopped too.
        r.state = "archived".into();
        assert!(!strip_view(&r, 2_000, &name).unwrap().can_stop);
    }

    #[test]
    fn a_done_relay_shows_for_a_day() {
        let r = relay("done", 1, vec![step("a", "done"), step("b", "done")]);
        let v = strip_view(&r, 2_000, &name).unwrap();
        assert_eq!(v.position, "都做完了");
        assert_eq!(v.line, None);
        assert!(strip_view(&r, 1_000 + SHOW_ENDED_MS, &name).is_some());
        assert!(strip_view(&r, 1_001 + SHOW_ENDED_MS, &name).is_none());
        // A running one shows however old it is.
        let r = relay(
            "running",
            0,
            vec![step("a", "working"), step("b", "pending")],
        );
        assert!(strip_view(&r, 10 * SHOW_ENDED_MS, &name).is_some());
    }
}
