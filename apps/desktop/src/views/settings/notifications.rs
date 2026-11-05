//! 通知 (NATIVE.md §8): the global settings only. Group 提醒我 with a small
//! switch per event and 每日摘要 with its time; group 勿扰 with 从 / 到.
//! How one 群 or 私聊 reminds me is set on that conversation (its context
//! menu: 静音 and 提醒, `views::nav::notify`), not here.

use gpui::{Context, IntoElement, ParentElement, Render, Subscription, Window};

use super::widgets::{self, Status, answer};
use crate::{
    app::{Changes, Services},
    copy::settings as s,
    data::{Change, ClockTime, NotifyPreferences},
    ui::native::{FormRow, GroupedForm, NativeSwitch, SectionTitle},
};

pub struct NotificationsPage {
    services: Services,
    prefs: Option<NotifyPreferences>,
    load: Status,
    /// The last save of the switches.
    status: Status,
    _subscriptions: Vec<Subscription>,
}

impl NotificationsPage {
    pub fn new(services: Services, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.subscribe(&services.hub, |_, _, changes: &Changes, cx| {
            if changes.any(|c| matches!(c, Change::Groups)) {
                cx.notify();
            }
        })];
        Self {
            services,
            prefs: None,
            load: Status::Idle,
            status: Status::Idle,
            _subscriptions: subscriptions,
        }
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn preferences(&self) -> Option<&NotifyPreferences> {
        self.prefs.as_ref()
    }

    /// Loads my notification settings.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.load.busy() {
            return;
        }
        self.load = Status::Busy(s::LOADING.into());
        let reply = self.services.provider.notify_preferences();
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(p) => {
                        this.prefs = Some(p);
                        this.load = Status::Idle;
                    }
                    Err(e) => this.load = Status::from_error(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Changes the settings at once and saves them; the server's answer
    /// wins, a failure puts the old ones back.
    pub fn change(&mut self, f: impl FnOnce(&mut NotifyPreferences), cx: &mut Context<Self>) {
        let Some(before) = self.prefs.clone() else {
            return;
        };
        let mut next = before.clone();
        f(&mut next);
        if next == before {
            return;
        }
        self.prefs = Some(next.clone());
        self.status = Status::Idle;
        cx.notify();
        let reply = self.services.provider.set_notify_preferences(next);
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(p) => this.prefs = Some(p),
                    Err(e) => {
                        this.prefs = Some(before);
                        this.status = Status::from_error(e);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for NotificationsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = cx.entity();
        let Some(p) = self.prefs.clone() else {
            // Loading: the 提醒我 group as skeleton rows; failed: an Alert
            // with 重试 in its place.
            let body = match &self.load {
                Status::Failed(t) => {
                    let retry = page.clone();
                    widgets::failed_alert(
                        "notify-load-failed",
                        s::NOTIFY_LOAD_FAILED,
                        Some(t.clone()),
                        Some(
                            widgets::retry_button("notify-retry", cx)
                                .on_click(move |_, _, cx| retry.update(cx, |p, cx| p.refresh(cx))),
                        ),
                        cx,
                    )
                }
                _ => GroupedForm::new()
                    .row(widgets::list_loading("notify-loading", 4, false, cx))
                    .into_any_element(),
            };
            return widgets::pane().child(widgets::section(
                SectionTitle::new(s::REMIND_ME).description(s::REMIND_HINT),
                body,
            ));
        };
        let switch = |id: &'static str,
                      label: &'static str,
                      on: bool,
                      set: fn(&mut NotifyPreferences, bool)| {
            let page = page.clone();
            FormRow::new(label).control(NativeSwitch::new(id, on).name(label).on_change(
                move |next, _, cx| page.update(cx, |this, cx| this.change(|p| set(p, next), cx)),
            ))
        };
        let digest_time = p.digest_at.unwrap_or(NotifyPreferences::DIGEST_DEFAULT);
        let (pick, toggle) = (page.clone(), page.clone());
        let digest = FormRow::new(s::NOTIFY_DIGEST).control(
            NativeSwitch::new("notify-digest", p.digest_at.is_some())
                .name(s::NOTIFY_DIGEST)
                .on_change(move |on, _, cx| {
                    toggle.update(cx, |this, cx| {
                        this.change(|p| p.digest_at = on.then_some(digest_time), cx)
                    })
                }),
        );
        // The dependent 时间 row under 每日摘要 (2.8: one control per row).
        let digest_at = FormRow::new(s::DIGEST_TIME).control(widgets::time_popup(
            "notify-digest-time",
            digest_time,
            p.digest_at.is_none(),
            move |t, _, cx| pick.update(cx, |this, cx| this.change(|p| p.digest_at = Some(t), cx)),
            window,
            cx,
        ));
        let remind = widgets::section(
            SectionTitle::new(s::REMIND_ME).description(s::REMIND_HINT),
            GroupedForm::new()
                .row(switch(
                    "notify-waiting",
                    s::NOTIFY_WAITING,
                    p.waiting,
                    |p, v| p.waiting = v,
                ))
                .row(switch(
                    "notify-result",
                    s::NOTIFY_RESULT,
                    p.result,
                    |p, v| p.result = v,
                ))
                .row(switch(
                    "notify-blocked",
                    s::NOTIFY_BLOCKED,
                    p.blocked,
                    |p, v| p.blocked = v,
                ))
                .row(switch(
                    "notify-mention",
                    s::NOTIFY_MENTION,
                    p.mention,
                    |p, v| p.mention = v,
                ))
                .row(digest)
                .row(digest_at),
        );
        let time_row = |label: &'static str,
                        id: &'static str,
                        at: ClockTime,
                        set: fn(&mut NotifyPreferences, ClockTime)| {
            let page = page.clone();
            FormRow::new(label).control(widgets::time_popup(
                id,
                at,
                !p.quiet_enabled,
                move |t, _, cx| page.update(cx, |this, cx| this.change(|p| set(p, t), cx)),
                window,
                cx,
            ))
        };
        let quiet = widgets::section(
            SectionTitle::new(s::QUIET).description(s::QUIET_FOOTER),
            GroupedForm::new()
                .row(switch(
                    "notify-quiet",
                    s::QUIET_ENABLE,
                    p.quiet_enabled,
                    |p, v| p.quiet_enabled = v,
                ))
                .row(time_row(
                    s::QUIET_FROM,
                    "notify-quiet-from",
                    p.quiet_start,
                    |p, t| p.quiet_start = t,
                ))
                .row(time_row(
                    s::QUIET_TO,
                    "notify-quiet-to",
                    p.quiet_end,
                    |p, t| p.quiet_end = t,
                )),
        );
        let failed = match &self.status {
            Status::Failed(t) => Some(widgets::failed_alert(
                "notify-save-failed",
                s::NOTIFY_SAVE_FAILED,
                Some(t.clone()),
                None,
                cx,
            )),
            _ => None,
        };
        widgets::pane().child(remind).child(quiet).children(failed)
    }
}
