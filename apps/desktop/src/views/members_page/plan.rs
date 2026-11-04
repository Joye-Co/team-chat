//! The 汇报和限额 sheet (NATIVE.md §7.1) of the 成员 page, owners only: who the member
//! reports to (`Member.reportsTo`, a pop-up of the team) and, for an AI member, the weekly
//! limit (`Member.weeklyBudgetCents`, US dollars in the field). 保存 sends both fields with
//! `PATCH /v1/members/:id`; it is off while nothing changed or the amount is invalid, and
//! a refusal opens an alert while the sheet stays open.

use std::cell::Cell;

use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement, Render,
    SharedString, Styled as _, Subscription, Window,
};
use gpui_component::{
    WindowExt as _,
    input::{InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
};

use crate::{
    app::Services,
    copy::{common, fill, members_page as c},
    data::{AdminError, Member, MemberId, pages::MemberPlan},
    ui::native::{
        NativeTextField, PushSize, SheetLayout, SheetWidth, disabled_push, native_sheet,
        popup_button, sheet_buttons,
    },
    views::nav::dialogs::{
        error_alert, form_field_width, sheet_footnote, sheet_form, sheet_form_row,
    },
};

/// The largest weekly limit the server takes, in US cents.
const MAX_CENTS: u64 = chat_core::proto::MAX_WEEKLY_BUDGET_CENTS;

/// The typed weekly limit in US cents: `Ok(None)` for an empty field (no limit), an
/// error for anything that is not 0..=1,000,000 dollars with at most two decimals.
pub fn parse_limit(text: &str) -> Result<Option<u64>, ()> {
    let t = text
        .trim()
        .trim_start_matches('$')
        .trim()
        .replace([',', '，'], "");
    if t.is_empty() {
        return Ok(None);
    }
    let (whole, frac) = match t.split_once('.') {
        Some((w, f)) => (w, f),
        None => (t.as_str(), ""),
    };
    let digits = |s: &str| s.chars().all(|ch| ch.is_ascii_digit());
    if whole.is_empty() && frac.is_empty() || !digits(whole) || !digits(frac) || frac.len() > 2 {
        return Err(());
    }
    let dollars: u64 = if whole.is_empty() {
        0
    } else {
        whole.parse().map_err(|_| ())?
    };
    let cents: u64 = format!("{frac:0<2}").parse().map_err(|_| ())?;
    let total = dollars.checked_mul(100).and_then(|d| d.checked_add(cents));
    match total {
        Some(v) if v <= MAX_CENTS => Ok(Some(v)),
        _ => Err(()),
    }
}

/// US cents as the field shows them: `300`, `12.5`.
pub fn limit_text(cents: Option<u64>) -> String {
    match cents {
        None => String::new(),
        Some(c) if c % 100 == 0 => (c / 100).to_string(),
        Some(c) if c % 10 == 0 => format!("{}.{}", c / 100, (c % 100) / 10),
        Some(c) => format!("{}.{:02}", c / 100, c % 100),
    }
}

struct PlanForm {
    services: Services,
    member: Member,
    current: MemberPlan,
    reports_to: Option<MemberId>,
    limit: Entity<InputState>,
    /// Who the member may report to: the other active members, by name.
    team: Vec<(MemberId, String)>,
    busy: bool,
    _subscriptions: Vec<Subscription>,
}

impl PlanForm {
    /// The plan the form holds, when the amount is valid.
    fn plan(&self, cx: &App) -> Option<MemberPlan> {
        let weekly_budget_cents = if self.member.is_bot() {
            parse_limit(&self.limit.read(cx).value()).ok()?
        } else {
            self.current.weekly_budget_cents
        };
        Some(MemberPlan {
            reports_to: self.reports_to.clone(),
            weekly_budget_cents,
        })
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(plan) = self.plan(cx).filter(|p| *p != self.current) else {
            return;
        };
        let reply = self
            .services
            .provider
            .set_member_plan(&self.member.id, plan);
        self.busy = true;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = reply.await.unwrap_or(Err(AdminError::Other));
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(_) => window.close_dialog(cx),
                    Err(e) => {
                        error_alert(
                            c::MEMBERS_PLAN_REFUSED,
                            crate::views::settings::widgets::error_text(e),
                            |_, _| {},
                            window,
                            cx,
                        );
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }
}

impl Render for PlanForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let typed = self.limit.read(cx).value().to_string();
        let invalid = self.member.is_bot() && parse_limit(&typed).is_err();
        let ready = !self.busy && self.plan(cx).is_some_and(|p| p != self.current);
        let field_w = form_field_width(SheetWidth::Form);
        let chosen: SharedString = self
            .reports_to
            .as_ref()
            .map(|id| {
                self.team
                    .iter()
                    .find(|(m, _)| m == id)
                    .map(|(_, n)| n.clone())
                    .unwrap_or_else(|| id.clone())
            })
            .unwrap_or_else(|| c::MEMBERS_PLAN_NOBODY.to_string())
            .into();
        let options: Vec<(Option<MemberId>, String)> =
            std::iter::once((None, c::MEMBERS_PLAN_NOBODY.to_string()))
                .chain(
                    self.team
                        .iter()
                        .map(|(id, n)| (Some(id.clone()), n.clone())),
                )
                .collect();
        let current = self.reports_to.clone();
        let this = cx.entity().downgrade();
        let reports = popup_button("member-plan-reports", chosen, PushSize::Regular, window, cx)
            .max_w(field_w)
            .dropdown_menu(move |menu, _, _| {
                options.iter().fold(menu, |menu, (id, name)| {
                    let this = this.clone();
                    let pick = id.clone();
                    menu.item(
                        PopupMenuItem::new(name.clone())
                            .checked(current == *id)
                            .on_click(move |_, _, cx| {
                                let pick = pick.clone();
                                let _ = this.update(cx, |this, cx| {
                                    this.reports_to = pick;
                                    cx.notify();
                                });
                            }),
                    )
                })
            });
        let mut form = sheet_form().child(sheet_form_row(c::MEMBERS_PLAN_REPORTS, reports, cx));
        if self.member.is_bot() {
            form = form.child(sheet_form_row(
                c::MEMBERS_PLAN_LIMIT,
                // The limit is in US dollars: the unit sits in the field, so a cost
                // in another currency is never read against it.
                NativeTextField::new("member-plan-limit", &self.limit)
                    .width(field_w)
                    .trailing({
                        use crate::ui::native::NativeType as _;
                        gpui::div()
                            .n_callout()
                            .text_color(crate::ui::native::native_colors(cx).secondary)
                            .child(c::MEMBERS_PLAN_LIMIT_UNIT)
                    }),
                cx,
            ));
            if invalid {
                form = form.child(sheet_footnote(c::MEMBERS_PLAN_LIMIT_INVALID, cx));
            }
        }
        let [cancel, save] = sheet_buttons("member-plan", common::CANCEL, common::SAVE, cx);
        SheetLayout::new(c::MEMBERS_PLAN)
            .explanation(fill(
                c::MEMBERS_PLAN_EXPLANATION,
                &[("name", &self.member.name)],
            ))
            .body(form)
            .buttons([
                cancel.on_click(|_, window, cx| window.close_dialog(cx)),
                disabled_push(save, !ready, cx)
                    .loading(self.busy)
                    .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
            ])
    }
}

/// Opens the 汇报和限额 sheet of `member` with its current values.
pub fn open_plan_sheet(
    services: Services,
    member: Member,
    current: MemberPlan,
    window: &mut Window,
    cx: &mut App,
) {
    if window.has_active_dialog(cx) {
        return;
    }
    let mut team: Vec<(MemberId, String)> = services
        .provider
        .members()
        .into_iter()
        .filter(|m| !m.deactivated && m.id != member.id)
        .map(|m| (m.id, m.name))
        .collect();
    team.sort_by(|a, b| a.1.cmp(&b.1));
    let form = cx.new(|cx| {
        let limit = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(c::MEMBERS_PLAN_LIMIT_PLACEHOLDER)
                .default_value(limit_text(current.weekly_budget_cents))
        });
        let subscriptions = vec![cx.subscribe_in(
            &limit,
            window,
            |this: &mut PlanForm, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            },
        )];
        PlanForm {
            services,
            reports_to: current.reports_to.clone(),
            member,
            current,
            limit,
            team,
            busy: false,
            _subscriptions: subscriptions,
        }
    });
    let first = Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) && form.read(cx).member.is_bot() {
            let limit = form.read(cx).limit.clone();
            limit.read(cx).focus_handle(cx).focus(window, cx);
        }
        native_sheet(dialog, SheetWidth::Form, cx)
            .px_0()
            .child(form.clone())
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_weekly_limit_reads_dollars_and_cents() {
        assert_eq!(parse_limit(""), Ok(None));
        assert_eq!(parse_limit("  "), Ok(None));
        assert_eq!(parse_limit("300"), Ok(Some(30_000)));
        assert_eq!(parse_limit("$12.5"), Ok(Some(1_250)));
        assert_eq!(parse_limit("0.05"), Ok(Some(5)));
        assert_eq!(parse_limit("1,000"), Ok(Some(100_000)));
        assert_eq!(parse_limit("0"), Ok(Some(0)));
        assert_eq!(parse_limit("1000000"), Ok(Some(MAX_CENTS)));
        assert_eq!(parse_limit("1000000.01"), Err(()));
        assert_eq!(parse_limit("1.234"), Err(()));
        assert_eq!(parse_limit("-5"), Err(()));
        assert_eq!(parse_limit("abc"), Err(()));
        assert_eq!(parse_limit("."), Err(()));
        assert_eq!(parse_limit("99999999999999999999"), Err(()));
    }

    #[test]
    fn the_field_shows_the_stored_limit_back() {
        for cents in [0, 5, 50, 1_250, 30_000, 12_345, MAX_CENTS] {
            assert_eq!(parse_limit(&limit_text(Some(cents))), Ok(Some(cents)));
        }
        assert_eq!(limit_text(None), "");
        assert_eq!(limit_text(Some(1_250)), "12.5");
    }
}
