//! 通知 (NATIVE.md §8): group 提醒我 with a small switch per event and
//! 每日摘要 with its time; group 勿扰 with 从 / 到; `按群设置…` opens a sheet
//! with a table of 群 and their 全部 / 只有 @我 / 不提醒 pop-ups.

use gpui::{
    App, AppContext as _, Context, ElementId, Entity, IntoElement, ParentElement, Render,
    SharedString, Styled, Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    WindowExt as _,
    input::InputState,
    menu::{DropdownMenu as _, PopupMenuItem},
};

use super::widgets::{self, Status, answer};
use crate::{
    app::{Changes, Services},
    copy::{fill, settings as s},
    data::{AdminReply, Change, ClockTime, GroupKind, NotifyLevel, NotifyPreferences},
    ui::{
        self, Sym,
        native::{
            FormRow, GroupedForm, NativeSwitch, NativeType as _, PushSize, PushStyle, SearchField,
            SectionTitle, SheetLayout, SheetWidth, metrics, native_colors, push_button,
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
    /// The filter of the 按群设置 sheet.
    filter: Entity<InputState>,
    /// The scroll of the 按群设置 list.
    groups_scroll: gpui::ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl NotificationsPage {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder(s::PER_GROUP_FILTER));
        let subscriptions = vec![
            cx.subscribe(&services.hub, |_, _, changes: &Changes, cx| {
                if changes
                    .any(|c| matches!(c, Change::Groups | Change::Topics { .. } | Change::Todo))
                {
                    cx.notify();
                }
            }),
            cx.observe(&filter, |_, _, cx| cx.notify()),
        ];
        Self {
            services,
            prefs: None,
            load: Status::Idle,
            status: Status::Idle,
            filter,
            groups_scroll: gpui::ScrollHandle::new(),
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

    /// 按群设置… : the sheet with every 群 and 私聊 and its pop-up (2.9,
    /// F55): sections 群 (24 icon box) and 私聊 (avatar 24) in one media
    /// column, a filter above more than 8 rows, an Empty when none match.
    pub fn open_groups(this: Entity<Self>, window: &mut Window, cx: &mut App) {
        let filter = this.read(cx).filter.clone();
        filter.update(cx, |f, cx| f.set_value("", window, cx));
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
                let query = page.filter.read(cx).value().trim().to_lowercase();
                let all = provider.groups();
                let total = all.len();
                let (dms, groups): (Vec<_>, Vec<_>) =
                    all.into_iter().partition(|g| g.kind == GroupKind::Dm);
                let media = crate::ui::native::tokens::avatar::SM;
                // Two 群 with one name are told apart by their size (C1-13).
                let squash = |n: &str| n.split_whitespace().collect::<Vec<_>>().join(" ");
                let mut seen = std::collections::HashMap::<String, usize>::new();
                for g in groups.iter() {
                    *seen.entry(squash(&g.name)).or_default() += 1;
                }
                let row = |g: &crate::data::GroupSummary, window: &Window, cx: &App| {
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
                    // A DM row leads with the peer's avatar: a leading
                    // emoji is left out (X-01).
                    let name: SharedString = if peer.is_some() {
                        squash(crate::ui::avatar::name_beside_avatar(&g.name)).into()
                    } else {
                        squash(&g.name).into()
                    };
                    let twin =
                        g.kind != GroupKind::Dm && seen.get(name.as_ref()).is_some_and(|n| *n > 1);
                    let lead = match peer {
                        Some(m) => ui::MemberAvatar::new(Some(m), media)
                            .on(k.group)
                            .into_any_element(),
                        None => div()
                            .flex_none()
                            .size(media)
                            .rounded(crate::ui::native::tokens::radius::ITEM)
                            .bg(k.fill)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(crate::ui::icons::icon(
                                Sym::Topic,
                                crate::ui::native::tokens::icon::SM,
                                k.label,
                            ))
                            .into_any_element(),
                    };
                    widgets::ItemRow::new(
                        ElementId::Name(format!("notify-row-{}", g.id).into()),
                        name,
                    )
                    .media(lead)
                    .when(twin, |r| {
                        r.description(crate::copy::fill(
                            crate::copy::nav::MEMBER_COUNT,
                            &[("n", &g.member_ids.len().to_string())],
                        ))
                    })
                    .value(
                        widgets::popup_fixed(
                            ElementId::Name(format!("notify-{}", g.id).into()),
                            level_text(shown),
                            LEVEL_WIDTH,
                            window,
                            cx,
                        )
                        .dropdown_menu(move |menu, _, _| {
                            LEVELS.into_iter().fold(menu, |menu, level| {
                                let (page, gid) = (page.clone(), gid.clone());
                                menu.item(
                                    PopupMenuItem::new(level_text(level))
                                        .checked(level == shown)
                                        .on_click(move |_, _, cx| {
                                            page.update(cx, |p, cx| p.pick_level(&gid, level, cx))
                                        }),
                                )
                            })
                        }),
                    )
                    .into_any_element()
                };
                let matches = |g: &&crate::data::GroupSummary| {
                    query.is_empty() || g.name.to_lowercase().contains(&query)
                };
                let group_rows: Vec<_> = groups
                    .iter()
                    .filter(matches)
                    .map(|g| row(g, window, cx))
                    .collect();
                let dm_rows: Vec<_> = dms
                    .iter()
                    .filter(matches)
                    .map(|g| row(g, window, cx))
                    .collect();
                let header = |title: &'static str| {
                    div()
                        .w_full()
                        .h(crate::ui::native::tokens::height::ROW)
                        .px(metrics::FORM_ROW_INSET)
                        .flex()
                        .items_center()
                        .justify_between()
                        .n_section()
                        .text_color(k.secondary)
                        .child(title)
                        .child(s::COL_NOTIFY)
                };
                let inset = media;
                let mut list = div().w_full().flex().flex_col();
                let empty = group_rows.is_empty() && dm_rows.is_empty();
                for (title, rows) in [(s::COL_GROUP, group_rows), (s::PER_GROUP_DMS, dm_rows)] {
                    if rows.is_empty() {
                        continue;
                    }
                    list = list
                        .child(header(title))
                        .child(widgets::item_rows(rows, inset, cx));
                }
                let body = if total == 0 {
                    widgets::list_empty(Sym::Bell, s::NO_GROUPS, None, cx).into_any_element()
                } else if empty {
                    widgets::list_empty(Sym::Search, s::PER_GROUP_NO_MATCH, None, cx)
                        .into_any_element()
                } else {
                    list.into_any_element()
                };
                let frame = div()
                    .w_full()
                    .rounded(metrics::GROUP_RADIUS)
                    .border_1()
                    .border_color(k.separator)
                    .bg(k.group)
                    .overflow_hidden()
                    .child(widgets::capped_list(
                        "notify-groups",
                        body,
                        notify_list_max(),
                        &page.groups_scroll,
                    ));
                let failed = match &page.status {
                    Status::Failed(t) => Some(widgets::failed_alert(
                        "notify-groups-failed",
                        s::NOTIFY_SAVE_FAILED,
                        Some(t.clone()),
                        None,
                        cx,
                    )),
                    _ => None,
                };
                dialog.on_ok(|_, _, _| true).child(
                    SheetLayout::new(s::PER_GROUP_TITLE)
                        .explanation(s::PER_GROUP_HINT)
                        .when(total > FILTER_FROM, |l| {
                            l.body(SearchField::new("notify-groups-filter", &page.filter))
                        })
                        .body(frame)
                        .when_some(failed, |l, f| l.body(f))
                        .buttons([push_button(
                            "notify-groups-done",
                            s::DONE,
                            PushStyle::Default,
                            PushSize::Regular,
                            cx,
                        )
                        .on_click(|_, window, cx| window.close_dialog(cx))]),
                )
            },
        );
    }
}

/// The level pop-ups share one width (2.8 value column).
const LEVEL_WIDTH: gpui::Pixels = crate::ui::native::metrics::LABEL_COLUMN;
/// More rows than this get a filter above the list (F55).
const FILTER_FROM: usize = 8;
/// The list box of the 按群设置 sheet: its section header, 6 rows and half
/// of the next, so the cut row says it scrolls (2.9).
fn notify_list_max() -> gpui::Pixels {
    crate::ui::native::tokens::height::ROW
        + widgets::half_row_cap(crate::ui::native::metrics::FORM_ROW, 6)
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
        let groups_page = page.clone();
        let count = self.services.provider.groups().len();
        let per_group = widgets::section(
            SectionTitle::new(s::SECTION_PER_GROUP).description(s::SECTION_PER_GROUP_HINT),
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(crate::ui::native::tokens::space::S12)
                .child(GroupedForm::new().row(super::general::nav_row(
                    "notify-groups-open",
                    s::PER_GROUP_ROW,
                    Some(fill(s::PER_GROUP_COUNT, &[("n", &count.to_string())])),
                    window,
                    cx,
                    move |window, cx| Self::open_groups(groups_page.clone(), window, cx),
                )))
                .children(match &self.status {
                    Status::Failed(t) => Some(widgets::failed_alert(
                        "notify-save-failed",
                        s::NOTIFY_SAVE_FAILED,
                        Some(t.clone()),
                        None,
                        cx,
                    )),
                    _ => None,
                }),
        );
        widgets::pane().child(remind).child(quiet).child(per_group)
    }
}
