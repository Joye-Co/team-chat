//! 团队管理 › 活动记录 (docs/protocol.md "Activity"): who changed what in the team, newest first,
//! grouped by day, filtered by 谁 / 类型 / 机器, kept 90 days on the server.

use gpui::{
    AnyElement, Context, IntoElement, ParentElement as _, Styled, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};
use serde_json::Value;

use chat_core::proto::activity::{ActivityEntry, kind};

use super::AdminView;
use crate::{
    copy::{admin as m, fill},
    data::MemberKind,
    ui::{
        Sym,
        icons::sym as sym_icon,
        native::{
            Alert, AlertTone, Badge, Empty, NativeType as _, PushSize, PushStyle, TruncText,
            native_colors, push_button, skeleton,
            tokens::{height, radius, space, text},
        },
        sf_symbols::SymSize,
    },
    views::settings::widgets::answer,
};

/// The page's state: what is loaded and the filters.
#[derive(Default)]
pub struct Activity {
    pub entries: Vec<ActivityEntry>,
    pub has_more: bool,
    pub loaded: bool,
    pub loading: bool,
    pub error: Option<String>,
    pub actor: Option<String>,
    pub kind: Option<&'static str>,
    pub machine: Option<String>,
}

/// The words of a kind.
pub fn kind_label(k: &str) -> &'static str {
    match k {
        kind::LEVEL => m::KIND_LEVEL,
        kind::INVITE => m::KIND_INVITE,
        kind::KEY => m::KIND_KEY,
        kind::APPROVAL => m::KIND_APPROVAL,
        kind::BUDGET => m::KIND_BUDGET,
        kind::LOGIN => m::KIND_LOGIN,
        kind::MACHINE => m::KIND_MACHINE,
        _ => m::KIND_MEMBER,
    }
}

fn kind_icon(k: &str) -> Sym {
    match k {
        kind::LEVEL | kind::MEMBER => Sym::Members,
        kind::INVITE => Sym::Invite,
        kind::KEY => Sym::Key,
        kind::APPROVAL => Sym::Check,
        kind::BUDGET => Sym::BarChart,
        kind::LOGIN => Sym::Account,
        kind::MACHINE => Sym::Server,
        _ => Sym::Info,
    }
}

fn level_word(v: &Value) -> &'static str {
    if v.as_str() == Some("owner") {
        m::LEVEL_OWNER
    } else {
        m::LEVEL_MEMBER
    }
}

fn money(cents: &Value) -> String {
    match cents.as_u64() {
        Some(c) => format!("${}", (c as f64 / 100.).round() as u64),
        None => m::NO_LIMIT.to_string(),
    }
}

/// `[before, after]` of a change in `details`.
fn change<'a>(e: &'a ActivityEntry, key: &str) -> Option<(&'a Value, &'a Value)> {
    let v = e.details.as_ref()?.get(key)?.as_array()?;
    Some((v.first()?, v.get(1)?))
}

fn detail_str<'a>(e: &'a ActivityEntry, key: &str) -> Option<&'a str> {
    e.details.as_ref()?.get(key)?.as_str()
}

/// An entry as a sentence and an optional second line. `name` gives a member's name, `machine` a
/// machine's.
pub fn sentence(
    e: &ActivityEntry,
    name: &dyn Fn(&str) -> String,
    machine: &dyn Fn(&str) -> String,
) -> (String, Option<String>) {
    let actor = e
        .actor_id
        .as_deref()
        .map(name)
        .unwrap_or_else(|| m::SOMEONE.into());
    let target = e.target.as_deref().map(name).unwrap_or_default();
    let mach = e.machine_id.as_deref().map(machine).unwrap_or_default();
    let f = |t: &str, extra: &[(&str, &str)]| {
        // The entry's own words win over the defaults.
        let mut args: Vec<(&str, &str)> = extra.to_vec();
        args.extend_from_slice(&[("actor", &actor), ("target", &target), ("machine", &mach)]);
        fill(t, &args)
    };
    match e.action.as_str() {
        "member.update" => {
            if let Some((_, after)) = change(e, "level") {
                return (f(m::ACT_LEVEL, &[("level", level_word(after))]), None);
            }
            if let Some((before, after)) = change(e, "weeklyBudgetCents") {
                return (
                    f(m::ACT_BUDGET, &[("budget", &money(after))]),
                    Some(fill(m::ACT_BUDGET_WAS, &[("budget", &money(before))])),
                );
            }
            if let Some((_, after)) = change(e, "deactivated") {
                let t = if after.as_bool() == Some(true) {
                    m::ACT_DEACTIVATE
                } else {
                    m::ACT_REACTIVATE
                };
                return (f(t, &[]), None);
            }
            (f(m::ACT_MEMBER_UPDATE, &[]), None)
        }
        "member.deactivate" => (f(m::ACT_DEACTIVATE, &[]), None),
        "bot.create" => (
            f(
                m::ACT_BOT_CREATE,
                &[("name", detail_str(e, "name").unwrap_or(&target))],
            ),
            None,
        ),
        "bot.runner.set" => (f(m::ACT_RUNNER_SET, &[]), None),
        "bot.runner.delete" => (f(m::ACT_RUNNER_DELETE, &[]), None),
        "token.revoke" => (f(m::ACT_TOKEN_REVOKE, &[]), None),
        "invite.create" => {
            let who = detail_str(e, "name").unwrap_or_default().to_string();
            let level = e
                .details
                .as_ref()
                .and_then(|d| d.get("level"))
                .map(level_word)
                .unwrap_or(m::LEVEL_MEMBER);
            (
                f(m::ACT_INVITE, &[("name", &who)]),
                Some(fill(m::ACT_INVITE_META, &[("level", level)])),
            )
        }
        "invite.redeem" => (f(m::ACT_REDEEM, &[]), None),
        "machine.secret.set" | "machine.secret.delete" => {
            let key = detail_str(e, "name").unwrap_or_default().to_string();
            let t = if e.action == "machine.secret.set" {
                m::ACT_KEY_SET
            } else {
                m::ACT_KEY_DELETE
            };
            let members: Vec<String> = e
                .details
                .as_ref()
                .and_then(|d| d.get("members"))
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str()).map(name).collect())
                .unwrap_or_default();
            let meta = (e.action == "machine.secret.set").then(|| {
                let who = if members.is_empty() {
                    m::KEYS_ALL_MEMBERS.to_string()
                } else {
                    members.join("、")
                };
                fill(m::ACT_KEY_FOR, &[("who", &who), ("machine", &mach)])
            });
            (f(t, &[("name", &key)]), meta)
        }
        "run.approval" => {
            let bot = detail_str(e, "botId").map(name).unwrap_or_default();
            let t = if detail_str(e, "decision") == Some("deny") {
                m::ACT_DENY
            } else {
                m::ACT_APPROVE
            };
            (f(t, &[("target", &bot)]), None)
        }
        "settings.approvals" => (f(m::ACT_RULES, &[]), None),
        "email.sign_in" => (f(m::ACT_EMAIL_SIGN_IN, &[]), None),
        "machine.login.start" => {
            let rt = detail_str(e, "runtime").unwrap_or_default();
            let cli = match rt {
                "claude" => "Claude Code".to_string(),
                "codex" => "Codex".to_string(),
                r => r.to_string(),
            };
            (f(m::ACT_CLI_LOGIN, &[("cli", &cli)]), None)
        }
        "machine.create" => (f(m::ACT_MACHINE_CREATE, &[]), None),
        "machine.revoke" => (f(m::ACT_MACHINE_REVOKE, &[]), None),
        a => (f(m::ACT_OTHER, &[("action", a)]), None),
    }
}

/// The day header of a time: 今天, 昨天, or "11 月 8 日".
fn day_label(ms: u64) -> (chrono::NaiveDate, String) {
    use chrono::TimeZone as _;
    let today = crate::selftest::clock::local_now().date_naive();
    let d = chrono::Local
        .timestamp_millis_opt(ms as i64)
        .single()
        .map(|t| t.date_naive())
        .unwrap_or(today);
    let label = if d == today {
        m::ACTIVITY_TODAY.to_string()
    } else if Some(d) == today.pred_opt() {
        m::ACTIVITY_YESTERDAY.to_string()
    } else {
        use chrono::Datelike as _;
        fill(
            m::ACTIVITY_DAY,
            &[("m", &d.month().to_string()), ("d", &d.day().to_string())],
        )
    };
    (d, label)
}

impl AdminView {
    /// Loads the first page (`more` false) or the next one with the current filters.
    pub(super) fn fetch_activity(&mut self, more: bool, cx: &mut Context<Self>) {
        if self.activity.loading {
            return;
        }
        let before = if more {
            self.activity.entries.last().map(|e| e.id)
        } else {
            None
        };
        self.activity.loading = true;
        let reply = self.services.provider.team_activity(
            before,
            self.activity.actor.clone(),
            self.activity
                .kind
                .map(|k| vec![k.to_string()])
                .unwrap_or_default(),
            self.activity.machine.clone(),
        );
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                let a = &mut this.activity;
                a.loading = false;
                match result {
                    Ok(r) => {
                        if more {
                            a.entries.extend(r.entries);
                        } else {
                            a.entries = r.entries;
                        }
                        a.has_more = r.has_more;
                        a.loaded = true;
                        a.error = None;
                    }
                    Err(e) => {
                        a.error = Some(crate::views::settings::widgets::error_text(e).to_string())
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn set_activity_filter(&mut self, f: impl FnOnce(&mut Activity), cx: &mut Context<Self>) {
        f(&mut self.activity);
        self.activity.loading = false;
        self.fetch_activity(false, cx);
    }

    pub(super) fn machine_name(&self, id: &str) -> String {
        self.machines
            .iter()
            .flatten()
            .find(|x| x.id == id)
            .map(|x| x.name.clone())
            .unwrap_or_else(|| id.to_string())
    }

    pub(super) fn activity_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let k = native_colors(cx);
        let a = &self.activity;
        // Filters: three pull-downs.
        let people: Vec<(String, String)> = self
            .services
            .provider
            .members()
            .into_iter()
            .filter(|x| x.kind == MemberKind::Human)
            .map(|x| (x.id.to_string(), x.name))
            .collect();
        let machines: Vec<(String, String)> = self
            .machines
            .iter()
            .flatten()
            .map(|x| (x.id.clone(), x.name.clone()))
            .collect();
        let who_title = a
            .actor
            .as_deref()
            .map(|id| self.member_name(id))
            .unwrap_or_else(|| m::ACTIVITY_WHO.to_string());
        let kind_title = a
            .kind
            .map(kind_label)
            .unwrap_or(m::ACTIVITY_KIND)
            .to_string();
        let machine_title = a
            .machine
            .as_deref()
            .map(|id| self.machine_name(id))
            .unwrap_or_else(|| m::ACTIVITY_MACHINE.to_string());
        let view = cx.entity();
        let who =
            crate::ui::native::pull_down("activity-who", who_title, PushSize::Small, window, cx)
                .dropdown_menu(move |menu, _, _| {
                    let v = view.clone();
                    let mut menu = menu.item(PopupMenuItem::new(m::ACTIVITY_ANYONE).on_click(
                        move |_, _, cx| {
                            v.update(cx, |v, cx| v.set_activity_filter(|a| a.actor = None, cx))
                        },
                    ));
                    for (id, name) in &people {
                        let (v, id) = (view.clone(), id.clone());
                        menu = menu.item(PopupMenuItem::new(name.clone()).on_click(
                            move |_, _, cx| {
                                let id = id.clone();
                                v.update(cx, |v, cx| {
                                    v.set_activity_filter(|a| a.actor = Some(id), cx)
                                })
                            },
                        ));
                    }
                    menu
                });
        let view = cx.entity();
        let kinds =
            crate::ui::native::pull_down("activity-kind", kind_title, PushSize::Small, window, cx)
                .dropdown_menu(move |menu, _, _| {
                    let v = view.clone();
                    let mut menu = menu.item(PopupMenuItem::new(m::ACTIVITY_ANY_KIND).on_click(
                        move |_, _, cx| {
                            v.update(cx, |v, cx| v.set_activity_filter(|a| a.kind = None, cx))
                        },
                    ));
                    for k in kind::ALL {
                        let v = view.clone();
                        menu = menu.item(PopupMenuItem::new(kind_label(k)).on_click(
                            move |_, _, cx| {
                                v.update(cx, |v, cx| {
                                    v.set_activity_filter(|a| a.kind = Some(k), cx)
                                })
                            },
                        ));
                    }
                    menu
                });
        let view = cx.entity();
        let mach = crate::ui::native::pull_down(
            "activity-machine",
            machine_title,
            PushSize::Small,
            window,
            cx,
        )
        .dropdown_menu(move |menu, _, _| {
            let v = view.clone();
            let mut menu = menu.item(PopupMenuItem::new(m::ACTIVITY_ANY_MACHINE).on_click(
                move |_, _, cx| {
                    v.update(cx, |v, cx| v.set_activity_filter(|a| a.machine = None, cx))
                },
            ));
            for (id, name) in &machines {
                let (v, id) = (view.clone(), id.clone());
                menu = menu.item(PopupMenuItem::new(name.clone()).on_click(move |_, _, cx| {
                    let id = id.clone();
                    v.update(cx, |v, cx| {
                        v.set_activity_filter(|a| a.machine = Some(id), cx)
                    })
                }));
            }
            menu
        });
        let filters = div()
            .flex()
            .items_center()
            .gap(space::S8)
            .child(who)
            .child(kinds)
            .child(mach)
            .child(div().flex_1())
            .child(
                div()
                    .n_footnote()
                    .text_color(k.secondary)
                    .child(m::ACTIVITY_KEPT),
            );
        let body: AnyElement = if let Some(e) = &a.error {
            Alert::new(Sym::Failed, m::ACTIVITY_FAILED)
                .tone(AlertTone::Destructive)
                .description(e.clone())
                .into_any_element()
        } else if !a.loaded {
            div()
                .flex()
                .flex_col()
                .gap(space::S8)
                .children((0..6).map(|_| skeleton(gpui::relative(1.), height::TABLE_ROW)))
                .into_any_element()
        } else if a.entries.is_empty() {
            Empty::new(Sym::List, m::ACTIVITY_EMPTY)
                .description(m::ACTIVITY_EMPTY_DESC)
                .dashed()
                .into_any_element()
        } else {
            let name = |id: &str| self.member_name(id);
            let machine = |id: &str| self.machine_name(id);
            // Group by local day, newest first.
            let mut days: Vec<(chrono::NaiveDate, String, Vec<AnyElement>)> = vec![];
            for e in &a.entries {
                let (date, label) = day_label(e.at);
                if days.last().map(|d| d.0) != Some(date) {
                    days.push((date, label, vec![]));
                }
                let (title, meta) = sentence(e, &name, &machine);
                let row =
                    div()
                        .min_h(height::LIST_ROW)
                        .px(space::S16)
                        .py(space::S8)
                        .flex()
                        .items_center()
                        .gap(space::S16)
                        .child(
                            div()
                                .w(height::LIST_ROW)
                                .flex_none()
                                .n_style(text::MONO)
                                .text_color(k.secondary)
                                .child(super::clock_short(e.at)),
                        )
                        .child(
                            div()
                                .flex_none()
                                .size(height::DEFAULT)
                                .rounded(radius::CONTROL)
                                .bg(k.fill)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(sym_icon(kind_icon(&e.kind), SymSize::INLINE, k.secondary)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(gpui::Pixels::ZERO)
                                .flex()
                                .flex_col()
                                .child(TruncText::new(title).n_headline())
                                .children(meta.map(|t| {
                                    TruncText::new(t).n_footnote().text_color(k.secondary)
                                })),
                        )
                        .child(Badge::new(kind_label(&e.kind)));
                if let Some(d) = days.last_mut() {
                    d.2.push(row.into_any_element());
                }
            }
            let view = cx.entity();
            div()
                .flex()
                .flex_col()
                .gap(space::S16)
                .children(days.into_iter().map(|(_, label, rows)| {
                    let n = rows.len();
                    div()
                        .flex()
                        .flex_col()
                        .gap(space::S8)
                        .child(div().n_section().text_color(k.secondary).child(label))
                        .child(
                            div()
                                .rounded(radius::CONTAINER)
                                .border_1()
                                .border_color(k.separator)
                                .bg(k.admin_panel())
                                .overflow_hidden()
                                .flex()
                                .flex_col()
                                .children(rows.into_iter().enumerate().map(|(i, r)| {
                                    div()
                                        .when(i + 1 < n, |d| {
                                            d.border_b_1().border_color(k.separator)
                                        })
                                        .child(r)
                                })),
                        )
                }))
                .when(a.has_more, |d| {
                    d.child(
                        div().flex().justify_center().child(
                            crate::ui::native::loading_push(
                                push_button(
                                    "activity-more",
                                    m::ACTIVITY_MORE,
                                    PushStyle::Outline,
                                    PushSize::Small,
                                    cx,
                                ),
                                a.loading,
                                PushSize::Small,
                            )
                            .on_click(move |_, _, cx| {
                                view.update(cx, |v, cx| v.fetch_activity(true, cx))
                            }),
                        ),
                    )
                })
                .into_any_element()
        };
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(space::S16)
            .child(filters)
            .child(body)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(action: &str, kind: &str, details: serde_json::Value) -> ActivityEntry {
        ActivityEntry {
            id: 1,
            at: 0,
            kind: kind.into(),
            action: action.into(),
            actor_id: Some("joye".into()),
            target: Some("forge".into()),
            machine_id: Some("m1".into()),
            details: details.as_object().cloned(),
        }
    }

    fn name(id: &str) -> String {
        match id {
            "joye" => "Joye".into(),
            "forge" => "Forge".into(),
            "taro" => "Taro".into(),
            x => x.into(),
        }
    }

    #[test]
    fn entries_read_as_sentences() {
        let machine = |_: &str| "vm-runner-1".to_string();
        let s = |a: &str, k: &str, d: serde_json::Value| sentence(&entry(a, k, d), &name, &machine);
        assert_eq!(
            s(
                "member.update",
                "level",
                serde_json::json!({"level": ["member", "owner"]})
            )
            .0,
            "Joye 把 Forge 的级别改成负责人"
        );
        assert_eq!(
            s(
                "member.update",
                "budget",
                serde_json::json!({"weeklyBudgetCents": [10000, 5000]})
            ),
            (
                "Joye 把 Forge 的每周预算改成 $50".into(),
                Some("原来 $100".into())
            )
        );
        assert_eq!(
            s(
                "machine.secret.set",
                "key",
                serde_json::json!({"name": "FIGMA_TOKEN", "members": ["forge"]})
            ),
            (
                "Joye 添加密钥 FIGMA_TOKEN".into(),
                Some("给 Forge 用 · vm-runner-1".into())
            )
        );
        assert_eq!(
            s(
                "run.approval",
                "approval",
                serde_json::json!({"decision": "deny", "botId": "taro"})
            )
            .0,
            "Joye 拒绝了 Taro 的一步"
        );
        assert_eq!(
            s(
                "machine.login.start",
                "login",
                serde_json::json!({"runtime": "claude"})
            )
            .0,
            "Joye 在 vm-runner-1 上登录 Claude Code"
        );
        assert_eq!(kind_label("whatever"), "成员");
    }
}
