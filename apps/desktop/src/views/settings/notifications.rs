//! 通知 (NATIVE.md §8): group 提醒我 with a small switch per event and
//! 每日摘要 with its time; group 勿扰 with 从 / 到; `按群设置…` opens a sheet
//! with a table of 群 and their 全部 / 只有 @我 / 不提醒 pop-ups.

use gpui::{
    App, Context, ElementId, Entity, InteractiveElement as _, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    WindowExt as _,
    menu::{DropdownMenu as _, PopupMenuItem},
};

use super::widgets::{self, Status, answer};
use crate::{
    app::{Changes, Services},
    copy::settings as s,
    data::{AdminReply, Change, ClockTime, GroupKind, NotifyLevel, NotifyPreferences},
    ui::{
        self,
        native::{
            FormRow, GroupedForm, NativeSwitch, NativeType as _, PushSize, PushStyle, SheetLayout,
            SheetWidth, metrics, native_colors, push_button,
        },
    },
};

const LEVELS: [NotifyLevel; 3] = [NotifyLevel::All, NotifyLevel::Mentions, NotifyLevel::None];

fn level_text(level: NotifyLevel) -> &'static str {
    match level {
        NotifyLevel::All => s::LEVEL_ALL,
        NotifyLevel::Mentions => s::LEVEL_MENTIONS,
        NotifyLevel::None => s::LEVEL_NONE,
    }
}

pub struct NotificationsPage {
    services: Services,
    prefs: Option<NotifyPreferences>,
    load: Status,
    /// The last save of the switches, or a group pref.
    status: Status,
    _subscriptions: Vec<Subscription>,
}

impl NotificationsPage {
    pub fn new(services: Services, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.subscribe(&services.hub, |_, _, changes: &Changes, cx| {
            if changes.any(|c| matches!(c, Change::Groups | Change::Topics { .. } | Change::Todo)) {
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

    fn run(&mut self, reply: AdminReply<()>, cx: &mut Context<Self>) {
        self.status = Status::Idle;
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                this.status = match result {
                    Ok(()) => Status::Idle,
                    Err(e) => Status::from_error(e),
                };
                cx.notify();
            });
        })
        .detach();
    }

    pub fn set_muted(&mut self, group_id: &str, muted: bool, cx: &mut Context<Self>) {
        let reply = self
            .services
            .provider
            .set_group_prefs(group_id, Some(muted), None);
        self.run(reply, cx);
    }

    pub fn set_level(&mut self, group_id: &str, level: NotifyLevel, cx: &mut Context<Self>) {
        let reply = self
            .services
            .provider
            .set_group_prefs(group_id, None, Some(level));
        self.run(reply, cx);
    }

    /// The pop-up of the 按群设置 table: a muted 群 reads 不提醒, and any
    /// other choice unmutes it.
    pub fn pick_level(&mut self, group_id: &str, level: NotifyLevel, cx: &mut Context<Self>) {
        let prefs = self.services.provider.group_prefs(group_id);
        let reply = if prefs.muted && level != NotifyLevel::None {
            self.services
                .provider
                .set_group_prefs(group_id, Some(false), Some(level))
        } else {
            self.services
                .provider
                .set_group_prefs(group_id, None, Some(level))
        };
        self.run(reply, cx);
    }

    pub fn unfollow(&mut self, group_id: &str, topic_id: &str, cx: &mut Context<Self>) {
        self.services
            .provider
            .set_topic_following(group_id, topic_id, false);
        self.status = Status::Idle;
        cx.notify();
    }

    /// 按群设置… : the sheet with every 群 and 私聊 and its pop-up.
    pub fn open_groups(this: Entity<Self>, window: &mut Window, cx: &mut App) {
        widgets::open_sheet(
            window,
            cx,
            SheetWidth::List,
            px(640.),
            move |dialog, window, cx| {
                let k = native_colors(cx);
                let page = this.read(cx);
                let provider = page.services.provider.clone();
                let members = provider.members();
                let (dms, groups): (Vec<_>, Vec<_>) = provider
                    .groups()
                    .into_iter()
                    .partition(|g| g.kind == GroupKind::Dm);
                let rows: Vec<gpui::AnyElement> = groups
                    .iter()
                    .chain(dms.iter())
                    .map(|g| {
                        let prefs = provider.group_prefs(&g.id);
                        let shown = if prefs.muted {
                            NotifyLevel::None
                        } else {
                            prefs.notify
                        };
                        let peer = (g.kind == GroupKind::Dm)
                            .then(|| {
                                g.dm_peer
                                    .as_ref()
                                    .and_then(|p| members.iter().find(|m| &m.id == p))
                            })
                            .flatten();
                        let page = this.clone();
                        let gid = g.id.clone();
                        let name: SharedString = g
                            .name
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ")
                            .into();
                        FormRow::new(name)
                            .when_some(peer.cloned(), |row, m| {
                                row.leading(ui::MemberAvatar::new(Some(&m), px(18.)).on(k.sheet))
                            })
                            .control(
                                widgets::popup_fixed(
                                    ElementId::Name(format!("notify-{}", g.id).into()),
                                    level_text(shown),
                                    px(112.),
                                    window,
                                    cx,
                                )
                                .dropdown_menu(
                                    move |menu, _, _| {
                                        LEVELS.into_iter().fold(menu, |menu, level| {
                                            let (page, gid) = (page.clone(), gid.clone());
                                            menu.item(
                                                PopupMenuItem::new(level_text(level))
                                                    .checked(level == shown)
                                                    .on_click(move |_, _, cx| {
                                                        page.update(cx, |p, cx| {
                                                            p.pick_level(&gid, level, cx)
                                                        })
                                                    }),
                                            )
                                        })
                                    },
                                ),
                            )
                            .into_any_element()
                    })
                    .collect();
                let table = if rows.is_empty() {
                    div()
                        .n_body()
                        .text_color(k.secondary)
                        .child(s::NO_GROUPS)
                        .into_any_element()
                } else {
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        // Column names over the table, as a Mac table header.
                        .child(
                            div()
                                .px(metrics::FORM_ROW_INSET + px(2.))
                                .pb(px(4.))
                                .flex()
                                .n_subheadline()
                                .text_color(k.secondary)
                                .child(div().flex_1().child(s::COL_GROUP))
                                .child(div().w(px(112.)).child(s::COL_NOTIFY)),
                        )
                        .child(
                            div()
                                .id("notify-groups")
                                .max_h(px(296.))
                                .overflow_y_scroll()
                                .child(GroupedForm::new().rows(rows)),
                        )
                        .into_any_element()
                };
                dialog.on_ok(|_, _, _| true).child(
                    SheetLayout::new(s::PER_GROUP_TITLE)
                        .explanation(s::PER_GROUP_HINT)
                        .body(table)
                        .when_some(widgets::native_status(&page.status, cx), |l, st| l.body(st))
                        .buttons([push_button(
                            "notify-groups-done",
                            s::DONE,
                            PushStyle::Default,
                            PushSize::Large,
                            cx,
                        )
                        .on_click(|_, window, cx| window.close_dialog(cx))]),
                )
            },
        );
    }
}

impl Render for NotificationsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        let page = cx.entity();
        let Some(p) = self.prefs.clone() else {
            let body = match &self.load {
                Status::Failed(t) => div()
                    .n_body()
                    .text_color(k.secondary)
                    .child(t.clone())
                    .into_any_element(),
                _ => ui::native::spinner().into_any_element(),
            };
            return widgets::pane().child(
                div()
                    .h(px(120.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(body),
            );
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
        let digest = {
            let (pick, toggle) = (page.clone(), page.clone());
            FormRow::new(s::NOTIFY_DIGEST).control(
                div()
                    .flex()
                    .items_center()
                    .gap(crate::ui::native::tokens::space::S12)
                    .child(widgets::time_popup(
                        "notify-digest-time",
                        digest_time,
                        p.digest_at.is_none(),
                        move |t, _, cx| {
                            pick.update(cx, |this, cx| this.change(|p| p.digest_at = Some(t), cx))
                        },
                        window,
                        cx,
                    ))
                    .child(
                        NativeSwitch::new("notify-digest", p.digest_at.is_some())
                            .name(s::NOTIFY_DIGEST)
                            .on_change(move |on, _, cx| {
                                toggle.update(cx, |this, cx| {
                                    this.change(|p| p.digest_at = on.then_some(digest_time), cx)
                                })
                            }),
                    ),
            )
        };
        let remind = GroupedForm::new()
            .header(s::REMIND_ME)
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
            .row(digest);
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
        let quiet = GroupedForm::new()
            .row(switch("notify-quiet", s::QUIET, p.quiet_enabled, |p, v| {
                p.quiet_enabled = v
            }))
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
            ))
            .footer(s::QUIET_FOOTER);
        let groups_page = page.clone();
        widgets::pane().child(remind).child(quiet).child(
            div()
                .flex()
                .flex_col()
                .gap(crate::ui::native::tokens::space::S6)
                .child(
                    div().flex().child(
                        push_button(
                            "notify-groups-open",
                            s::PER_GROUP_OPEN,
                            PushStyle::Outline,
                            PushSize::Regular,
                            cx,
                        )
                        .on_click(move |_, window, cx| {
                            Self::open_groups(groups_page.clone(), window, cx)
                        }),
                    ),
                )
                .children(widgets::native_status(&self.status, cx)),
        )
    }
}
