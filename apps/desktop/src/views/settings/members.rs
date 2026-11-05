//! 成员 (COMPLETENESS 2.19): section 成员 (count, description, a filter,
//! `添加 AI 成员…` and `邀请成员…` in its header) lists the active members
//! with a 级别 select (owners) and "⋯"; section 已停用 holds deactivated
//! members with a badge and "⋯" › 恢复; section 默认规则 holds the
//! approval rules for AI members. Members who are not owners see it all
//! read-only.

use gpui::{
    App, AppContext as _, Context, ElementId, Entity, Focusable as _, InteractiveElement as _,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement as _, Styled,
    Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Disableable as _, WindowExt as _,
    button::Button,
    clipboard::Clipboard,
    input::{InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
};

use super::{
    i_am_owner,
    widgets::{self, Status, answer},
};
use crate::views::nav::dialogs::{sheet_footnote, sheet_form, sheet_form_row, sheet_form_row_top};
use crate::{
    app::{Changes, Services},
    characters::{self, CharacterId, CharacterStatus, status::StatusBoard},
    copy::{common, fill, settings as s},
    data::{
        ApprovalDefaults, ApproverRole, BotCreated, Change, InviteCreated, InviteRequest, Member,
        MemberKind, MemberLevel, RunnerSetup,
    },
    theme,
    ui::{
        self,
        native::{
            AlertLayout, BadgeState, FormRow, GroupedForm, NativeSwitch, NativeTextField,
            NativeType as _, PushSize, PushStyle, SearchField, SectionTitle, SheetLayout,
            SheetWidth, StatusBadge, alert_button, disabled_push, inline_commit, metrics,
            native_colors, page::outline_badge, pull_down, push_button, tokens,
        },
    },
};

/// Members in list order: me, the other owners, the other people, then bot
/// members, then deactivated members; by name inside each part.
pub fn sorted(mut members: Vec<Member>, me: Option<&str>) -> Vec<Member> {
    let rank = |m: &Member| {
        if m.deactivated {
            4
        } else if m.is_bot() {
            3
        } else if me == Some(m.id.as_str()) {
            0
        } else if m.level == MemberLevel::Owner {
            1
        } else {
            2
        }
    };
    members.sort_by_cached_key(|m| (rank(m), m.name.trim().to_lowercase()));
    members
}

/// 单次花费超过 ¥…: whole yuan, 1 to 1,000,000; empty is no limit.
pub fn parse_spend(text: &str) -> Result<Option<u32>, ()> {
    let t = text
        .trim()
        .trim_start_matches('¥')
        .trim()
        .replace([',', '，'], "");
    if t.is_empty() {
        return Ok(None);
    }
    match t.parse::<u32>() {
        Ok(n) if (1..=1_000_000).contains(&n) => Ok(Some(n)),
        _ => Err(()),
    }
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The width of every select in the 成员 tab (2.8: one value column).
const SELECT_WIDTH: gpui::Pixels = tokens::size::SELECT_WIDTH;
/// The spend limit field (2.8 number field).
const SPEND_WIDTH: gpui::Pixels = metrics::LABEL_COLUMN;

/// The spend limit as the field shows it: "1,000,000" (the "¥" is the
/// field's prefix), or empty (不限).
fn spend_text(n: Option<u32>) -> String {
    n.map(|n| widgets::grouped(n.into())).unwrap_or_default()
}

pub struct MembersPage {
    services: Services,
    /// The last row request's result (level or deactivate).
    status: Status,
    rules: Option<ApprovalDefaults>,
    rules_status: Status,
    spend: Entity<InputState>,
    filter: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl MembersPage {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let spend =
            cx.new(|cx| InputState::new(window, cx).placeholder(s::RULES_SPEND_PLACEHOLDER));
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder(s::MEMBERS_FILTER));
        let subscriptions = vec![
            cx.subscribe(&filter, |_, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe(&services.hub, |_, _, changes: &Changes, cx| {
                if changes.any(|c| matches!(c, Change::Members | Change::Session)) {
                    cx.notify();
                }
            }),
            inline_commit(&spend, window, cx, |this, text, window, cx| {
                this.commit_spend(&text, window, cx)
            }),
        ];
        Self {
            services,
            status: Status::Idle,
            rules: None,
            rules_status: Status::Idle,
            spend,
            filter,
            _subscriptions: subscriptions,
        }
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn rules(&self) -> Option<&ApprovalDefaults> {
        self.rules.as_ref()
    }

    pub fn set_level(&mut self, id: String, level: MemberLevel, cx: &mut Context<Self>) {
        let reply = self.services.provider.set_member_level(&id, level);
        self.run(reply, cx);
    }

    pub fn set_deactivated(&mut self, id: String, off: bool, cx: &mut Context<Self>) {
        let reply = self.services.provider.set_member_deactivated(&id, off);
        let reactivated = !off;
        self.status = Status::Idle;
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                this.status = match result {
                    Ok(_) if reactivated => Status::Done(s::REACTIVATE_HINT.into()),
                    Ok(_) => Status::Idle,
                    Err(e) => Status::from_error(e),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn run(&mut self, reply: crate::data::AdminReply<Member>, cx: &mut Context<Self>) {
        self.status = Status::Idle;
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                this.status = match result {
                    Ok(_) => Status::Idle,
                    Err(e) => Status::from_error(e),
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// Loads 默认规则.
    pub fn refresh_rules(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.rules_status.busy() {
            return;
        }
        self.rules_status = Status::Busy(s::LOADING.into());
        let reply = self.services.provider.approval_defaults();
        cx.spawn_in(window, async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(rules) => {
                        let text = spend_text(rules.max_spend_cny);
                        this.spend
                            .update(cx, |st, cx| st.set_value(text, window, cx));
                        this.rules = Some(rules);
                        this.rules_status = Status::Idle;
                    }
                    Err(e) => this.rules_status = Status::from_error(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Saves 默认规则 at once; the server's answer wins, a failure puts the
    /// old rules back.
    pub fn change_rules(&mut self, f: impl FnOnce(&mut ApprovalDefaults), cx: &mut Context<Self>) {
        let Some(before) = self.rules else { return };
        let mut next = before;
        f(&mut next);
        if next == before {
            return;
        }
        self.rules = Some(next);
        self.rules_status = Status::Idle;
        cx.notify();
        let reply = self.services.provider.set_approval_defaults(next);
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(r) => this.rules = Some(r),
                    Err(e) => {
                        this.rules = Some(before);
                        this.rules_status = Status::from_error(e);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn commit_spend(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rules) = self.rules else { return };
        match parse_spend(text) {
            Ok(n) => {
                // The field shows the value with its separators again.
                let shown = spend_text(n);
                if shown != text.trim() {
                    self.spend
                        .update(cx, |st, cx| st.set_value(shown, window, cx));
                }
                if n != rules.max_spend_cny {
                    self.change_rules(|r| r.max_spend_cny = n, cx);
                }
            }
            Err(()) => {
                self.rules_status = Status::Failed(s::SPEND_INVALID.into());
                let back = spend_text(rules.max_spend_cny);
                self.spend
                    .update(cx, |st, cx| st.set_value(back, window, cx));
                cx.notify();
            }
        }
    }

    fn confirm_deactivate(this: Entity<Self>, member: &Member, window: &mut Window, cx: &mut App) {
        let id = member.id.clone();
        let name = member.name.clone();
        widgets::open_alert(window, cx, move |_, cx| {
            let (page, id) = (this.clone(), id.clone());
            AlertLayout::new(fill(s::DEACTIVATE_TITLE, &[("name", &name)]))
                .icon(widgets::app_icon(cx))
                .message(fill(s::DEACTIVATE_BODY, &[("name", &name)]))
                .buttons([
                    alert_button("deactivate-ok", s::DEACTIVATE, PushStyle::Outline, true, cx)
                        .on_click(move |_, window, cx| {
                            let id = id.clone();
                            page.update(cx, |p, cx| p.set_deactivated(id, true, cx));
                            window.close_dialog(cx);
                        }),
                    alert_button(
                        "deactivate-cancel",
                        common::CANCEL,
                        PushStyle::Outline,
                        false,
                        cx,
                    )
                    .on_click(|_, window, cx| window.close_dialog(cx)),
                ])
                .into_any_element()
        });
    }

    /// One member (2.19, h64): avatar 32, the name (with "你"), 岗位 ·
    /// state (and the kind when another member has the same name), the 级别
    /// select in the value column and "⋯". A deactivated member shows the
    /// 已停用 badge instead of the select.
    fn row(
        &self,
        m: &Member,
        me: Option<&str>,
        owner: bool,
        same_name: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let k = native_colors(cx);
        let is_me = me == Some(m.id.as_str());
        let id = m.id.clone();
        let page = cx.entity();
        // An AI member's live state wins over presence (§2.12 words): 在做,
        // 等人回答, 异常.
        let live = if m.is_bot() {
            StatusBoard::status(cx, m)
        } else {
            CharacterStatus::default()
        };
        let state = if m.deactivated {
            None
        } else if live == CharacterStatus::Working {
            Some(s::MEMBER_WORKING)
        } else if live == CharacterStatus::NeedsInput {
            Some(s::MEMBER_WAITING)
        } else if live == CharacterStatus::Error {
            Some(s::MEMBER_ERROR)
        } else if m.online {
            Some(s::MEMBER_ONLINE)
        } else {
            Some(s::MEMBER_OFFLINE)
        };
        let role = m
            .role
            .as_deref()
            .map(one_line)
            .filter(|r| !r.is_empty())
            .unwrap_or_else(|| s::NO_ROLE.to_string());
        // Two members with one name: the kind tells them apart. The 岗位
        // goes last, so it is what truncates first (2.21).
        let kind = same_name.then_some(if m.is_bot() {
            s::KIND_AI
        } else {
            s::KIND_HUMAN
        });
        let detail = kind
            .into_iter()
            .chain(state)
            .chain(std::iter::once(role.as_str()))
            .collect::<Vec<_>>()
            .join(" · ");
        let value: gpui::AnyElement = if m.deactivated {
            StatusBadge::new(BadgeState::Deactivated, common::MEMBER_DEACTIVATED).into_any_element()
        } else if owner {
            // One select width down the column; my own is the same select,
            // disabled (I cannot change my own level).
            let current = m.level;
            let target = id.clone();
            let page = page.clone();
            let select = widgets::popup_fixed(
                ElementId::Name(format!("level-{id}").into()),
                widgets::level_text(m.level),
                SELECT_WIDTH,
                window,
                cx,
            );
            if is_me {
                div()
                    .id(ElementId::Name(format!("level-self-{id}").into()))
                    .tooltip(|window, cx| {
                        gpui_component::tooltip::Tooltip::new(s::LEVEL_SELF).build(window, cx)
                    })
                    .child(disabled_push(select, true, cx))
                    .into_any_element()
            } else {
                select
                    .dropdown_menu(move |menu, _, _| {
                        [MemberLevel::Owner, MemberLevel::Member].into_iter().fold(
                            menu,
                            |menu, level| {
                                let (page, target) = (page.clone(), target.clone());
                                menu.item(
                                    PopupMenuItem::new(widgets::level_text(level))
                                        .checked(level == current)
                                        .on_click(move |_, _, cx| {
                                            if level != current {
                                                let target = target.clone();
                                                page.update(cx, |p, cx| {
                                                    p.set_level(target, level, cx)
                                                });
                                            }
                                        }),
                                )
                            },
                        )
                    })
                    .into_any_element()
            }
        } else {
            outline_badge(widgets::level_text(m.level), cx)
        };
        let row = widgets::ItemRow::new(
            ElementId::Name(format!("member-{id}").into()),
            // Beside the avatar a leading emoji is left out (X-01).
            one_line(crate::ui::avatar::name_beside_avatar(&m.name)),
        )
        .media(
            ui::MemberAvatar::new(Some(m), tokens::avatar::MD)
                .status(CharacterStatus::Idle)
                .on(k.group),
        )
        .when(is_me, |r| r.title_lead(outline_badge(s::MEMBER_ME, cx)))
        .description(detail)
        .value(value)
        .muted(m.deactivated);
        if !owner {
            return row.into_any_element();
        }
        if is_me {
            // Keep the "⋯" column so the selects line up.
            return row.menu(div().size(tokens::height::SM)).into_any_element();
        }
        // "⋯": 更换形象… for an AI member, 停用…; or 恢复.
        let member = m.clone();
        let deactivated = m.deactivated;
        let more = widgets::more_button(
            ElementId::Name(format!("member-more-{id}").into()),
            window,
            cx,
        )
        .dropdown_menu(move |menu, _, _| {
            let page = page.clone();
            if deactivated {
                let target = member.id.clone();
                return menu.item(PopupMenuItem::new(s::REACTIVATE).on_click(move |_, _, cx| {
                    let target = target.clone();
                    page.update(cx, |p, cx| p.set_deactivated(target, false, cx));
                }));
            }
            let menu = if member.is_bot() {
                let (page, member) = (page.clone(), member.clone());
                menu.item(
                    PopupMenuItem::new(s::CHANGE_CHARACTER).on_click(move |_, window, cx| {
                        let services = page.read(cx).services.clone();
                        open_character(services, &member, window, cx);
                    }),
                )
            } else {
                menu
            };
            let member = member.clone();
            menu.item(
                PopupMenuItem::new(s::DEACTIVATE_MENU).on_click(move |_, window, cx| {
                    Self::confirm_deactivate(page.clone(), &member, window, cx)
                }),
            )
        });
        row.menu(more).into_any_element()
    }

    /// 默认规则 (2.19): switches, the spend limit as a number field with
    /// "¥" in it and separators, and who approves; its loading and failed
    /// states.
    fn rules_section(&self, owner: bool, window: &Window, cx: &mut Context<Self>) -> gpui::Div {
        let header = SectionTitle::new(s::RULES).description(if owner {
            s::RULES_FOOTER
        } else {
            s::RULES_READONLY
        });
        let Some(r) = self.rules else {
            let body = match &self.rules_status {
                Status::Failed(t) => widgets::failed_alert(
                    "rules-failed",
                    s::RULES_FAILED,
                    Some(t.clone()),
                    Some(widgets::retry_button("rules-retry", cx).on_click(
                        cx.listener(|this, _, window, cx| this.refresh_rules(window, cx)),
                    )),
                    cx,
                ),
                _ => GroupedForm::new()
                    .row(widgets::list_loading("rules-loading", 2, false, cx))
                    .into_any_element(),
            };
            return widgets::section(header, body);
        };
        let page = cx.entity();
        let switch = |id: &'static str,
                      label: &'static str,
                      on: bool,
                      set: fn(&mut ApprovalDefaults, bool)| {
            let page = page.clone();
            FormRow::new(label).control(
                NativeSwitch::new(id, on)
                    .name(label)
                    .disabled(!owner)
                    .on_change(move |v, _, cx| {
                        page.update(cx, |p, cx| p.change_rules(|r| set(r, v), cx))
                    }),
            )
        };
        let spend_bad =
            matches!(&self.rules_status, Status::Failed(t) if t.as_ref() == s::SPEND_INVALID);
        // 2.8 number field: "¥" at x + 12, the amount right-aligned in
        // tabular figures (review 3 C1-08). No prefix on the empty 不限.
        let has_spend = !self.spend.read(cx).value().trim().is_empty();
        let spend = NativeTextField::new("rules-spend", &self.spend)
            .width(SPEND_WIDTH)
            .align(gpui::TextAlign::Right)
            .when(has_spend, |f| {
                f.leading(
                    div()
                        .flex_none()
                        .n_body()
                        .text_color(native_colors(cx).secondary)
                        .child(s::RULES_SPEND_PREFIX),
                )
            })
            .disabled(!owner)
            .error(spend_bad);
        let approver_text = |a: ApproverRole| match a {
            ApproverRole::Requester => s::APPROVER_REQUESTER,
            ApproverRole::Owners => s::APPROVER_OWNERS,
        };
        let current = r.approver;
        let approver_page = page.clone();
        let approver = disabled_push(
            widgets::popup_fixed(
                "rules-approver",
                approver_text(current),
                SELECT_WIDTH,
                window,
                cx,
            ),
            !owner,
            cx,
        )
        .dropdown_menu(move |menu, _, _| {
            [ApproverRole::Requester, ApproverRole::Owners]
                .into_iter()
                .fold(menu, |menu, a| {
                    let page = approver_page.clone();
                    menu.item(
                        PopupMenuItem::new(approver_text(a))
                            .checked(a == current)
                            .on_click(move |_, _, cx| {
                                page.update(cx, |p, cx| p.change_rules(|r| r.approver = a, cx))
                            }),
                    )
                })
        });
        let group = GroupedForm::new()
            .row(switch(
                "rules-merge",
                s::RULES_MERGE,
                r.merge_main,
                |r, v| r.merge_main = v,
            ))
            .row(switch(
                "rules-deploy",
                s::RULES_DEPLOY,
                r.deploy_prod,
                |r, v| r.deploy_prod = v,
            ))
            // At rest the amount is right-aligned (2.8 number field).
            .row(FormRow::new(s::RULES_SPEND).control(spend))
            .row(FormRow::new(s::RULES_APPROVER).control(approver));
        // A value the server refused or a spend that is not a number: the
        // Alert under the group.
        let failed = match &self.rules_status {
            Status::Failed(t) => Some(widgets::failed_alert(
                "rules-save-failed",
                s::RULES_SAVE_FAILED,
                Some(t.clone()),
                None,
                cx,
            )),
            _ => None,
        };
        widgets::section(
            header,
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(tokens::space::S12)
                .child(group)
                .children(failed),
        )
    }
}

impl Render for MembersPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let owner = i_am_owner(&self.services);
        let me = self.services.me();
        let members = sorted(self.services.provider.members(), me.as_deref());
        // Names that more than one member has.
        let mut seen = std::collections::HashMap::<String, usize>::new();
        for m in &members {
            *seen.entry(one_line(&m.name).to_lowercase()).or_default() += 1;
        }
        let same_name = |m: &Member| seen.get(&one_line(&m.name).to_lowercase()).copied() > Some(1);
        let query = self.filter.read(cx).value().trim().to_lowercase();
        let matches = |m: &Member| {
            query.is_empty()
                || m.name.to_lowercase().contains(&query)
                || m.role
                    .as_deref()
                    .is_some_and(|r| r.to_lowercase().contains(&query))
        };
        let (active, off): (Vec<&Member>, Vec<&Member>) =
            members.iter().partition(|m| !m.deactivated);
        let active_rows: Vec<gpui::AnyElement> = active
            .iter()
            .filter(|m| matches(m))
            .map(|m| self.row(m, me.as_deref(), owner, same_name(m), window, cx))
            .collect();
        let services = self.services.clone();
        let bot_services = self.services.clone();
        let mut header = SectionTitle::new(s::PAGE_MEMBERS)
            .count(active.len() as u32)
            .description(if owner {
                s::MEMBERS_NOTE
            } else {
                s::MEMBERS_READONLY
            });
        if owner {
            header = header.action(
                div()
                    .flex()
                    .gap(tokens::space::S8)
                    .child(
                        push_button(
                            "member-add-ai",
                            s::ADD_AI_OPEN,
                            PushStyle::Outline,
                            PushSize::Small,
                            cx,
                        )
                        .on_click(move |_, window, cx| {
                            open_add_bot(bot_services.clone(), window, cx);
                        }),
                    )
                    .child(
                        push_button(
                            "member-invite",
                            s::INVITE_OPEN,
                            PushStyle::Default,
                            PushSize::Small,
                            cx,
                        )
                        .on_click(move |_, window, cx| {
                            open_invite(services.clone(), window, cx);
                        }),
                    ),
            );
        }
        let list = if active_rows.is_empty() {
            widgets::list_empty(ui::Sym::Search, s::MEMBERS_NO_MATCH, None, cx).into_any_element()
        } else {
            widgets::item_rows(active_rows, tokens::avatar::MD, cx).into_any_element()
        };
        let status = match &self.status {
            Status::Failed(t) => Some(widgets::failed_alert(
                "members-failed",
                s::MEMBERS_FAILED,
                Some(t.clone()),
                None,
                cx,
            )),
            Status::Done(t) => Some(widgets::quiet(t.clone(), cx).into_any_element()),
            _ => None,
        };
        let column = tokens::size::SETTINGS_COLUMN - metrics::GROUP_MARGIN * 2.;
        let members_section = widgets::section(
            header,
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(tokens::space::S12)
                .child(SearchField::new("members-filter", &self.filter).width(column))
                .child(GroupedForm::new().row(list))
                .children(status),
        );
        let off_rows: Vec<gpui::AnyElement> = off
            .iter()
            .filter(|m| matches(m))
            .map(|m| self.row(m, me.as_deref(), owner, same_name(m), window, cx))
            .collect();
        let off_section = (!off_rows.is_empty()).then(|| {
            widgets::section(
                SectionTitle::new(s::MEMBERS_DEACTIVATED)
                    .count(off_rows.len() as u32)
                    .description(s::MEMBERS_DEACTIVATED_HINT),
                GroupedForm::new().row(widgets::item_rows(off_rows, tokens::avatar::MD, cx)),
            )
        });
        widgets::pane()
            .child(members_section)
            .children(off_section)
            .child(self.rules_section(owner, window, cx))
    }
}

/// A one-time secret (§7.1 sheet body): its label, then the value in a
/// standalone-field box with a copy button. The value is never logged.
fn secret_row(id: &'static str, label: &'static str, value: &str, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    let value: SharedString = value.to_string().into();
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(crate::ui::native::tokens::space::S4)
        .child(div().n_body().text_color(k.label).child(label))
        .child(
            div()
                .w_full()
                .min_h(metrics::FIELD)
                .pl(metrics::FIELD_PAD)
                .pr(px(2.))
                .py(px(3.))
                .rounded(metrics::FIELD_RADIUS)
                .bg(k.content)
                .border_1()
                .border_color(k.separator)
                .flex()
                .items_start()
                .gap(crate::ui::native::tokens::space::S6)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .font_family(theme::MONO_FONT_FAMILY)
                        .text_size(crate::ui::native::tokens::text::META.0)
                        .line_height(px(18.))
                        .text_color(k.label)
                        .child(value.clone()),
                )
                .child(Clipboard::new(id).tooltip(s::COPY).value(value)),
        )
}

/// A row of a sheet form (2.8) with a full-width field; `invalid` turns
/// the field and its label destructive.
fn field_row(
    label: &'static str,
    id: &'static str,
    state: &Entity<InputState>,
    invalid: bool,
    cx: &App,
) -> gpui::Div {
    sheet_form_row(
        label,
        NativeTextField::new(id, state).full_width().error(invalid),
        cx,
    )
}

/// The field problems of a name and 岗位: whether the default button is
/// off, then the name's and the 岗位's message. An empty name only turns
/// the button off.
fn name_role_problem(name: &str, role: &str) -> (bool, Option<String>, Option<String>) {
    let name_err = (!name.is_empty())
        .then(|| widgets::check_len(name, 64).err())
        .flatten();
    let role_err = widgets::check_len(role, 64).err();
    (
        name.is_empty() || name_err.is_some() || role_err.is_some(),
        name_err,
        role_err,
    )
}

/// A server refusal in a sheet (2.15): an Alert in the sheet with 重试,
/// never a second modal over it.
fn refusal(
    id: &'static str,
    status: &Status,
    title: &'static str,
    retry: impl Fn(&mut Window, &mut App) + 'static,
    cx: &App,
) -> Option<gpui::AnyElement> {
    match status {
        Status::Failed(t) => Some(widgets::failed_alert(
            id,
            title,
            Some(t.clone()),
            Some(
                widgets::retry_button(SharedString::from(format!("{id}-retry")), cx)
                    .on_click(move |_, window, cx| retry(window, cx)),
            ),
            cx,
        )),
        _ => None,
    }
}

/// A pop-up row in a sheet: two choices, `on` is the second.
fn choice_row(
    label: &'static str,
    id: &'static str,
    options: [&'static str; 2],
    second: bool,
    disabled: bool,
    pick: impl Fn(bool, &mut Window, &mut App) + 'static,
    window: &Window,
    cx: &App,
) -> gpui::Div {
    let pick = std::rc::Rc::new(pick);
    sheet_form_row(
        label,
        disabled_push(
            widgets::popup_fixed(id, options[usize::from(second)], SELECT_WIDTH, window, cx),
            disabled,
            cx,
        )
        .dropdown_menu(move |menu, _, _| {
            [false, true].into_iter().fold(menu, |menu, v| {
                let pick = pick.clone();
                menu.item(
                    PopupMenuItem::new(options[usize::from(v)])
                        .checked(v == second)
                        .on_click(move |_, window, cx| pick(v, window, cx)),
                )
            })
        }),
        cx,
    )
}

/// The two large buttons at the bottom of a form sheet: 取消 and `default`.
fn sheet_actions(
    id: &'static str,
    default: &'static str,
    busy: bool,
    submit: impl Fn(&mut Window, &mut App) + 'static,
    cx: &App,
) -> [Button; 2] {
    [
        push_button(
            SharedString::from(format!("{id}-cancel")),
            common::CANCEL,
            PushStyle::Outline,
            PushSize::Regular,
            cx,
        )
        .on_click(|_, window, cx| window.close_dialog(cx)),
        disabled_push(
            push_button(
                SharedString::from(format!("{id}-submit")),
                default,
                PushStyle::Default,
                PushSize::Regular,
                cx,
            ),
            busy,
            cx,
        )
        .on_click(move |_, window, cx| submit(window, cx)),
    ]
}

/// The 完成 button of a sheet that only shows a result.
fn done_button(id: &'static str, cx: &App) -> Button {
    push_button(id, s::DONE, PushStyle::Default, PushSize::Regular, cx)
        .on_click(|_, window, cx| window.close_dialog(cx))
}
/// Columns of the look grid: 26 looks and one random cell make 3 rows of
/// 9 (2.9 avatar grid).
const PICKER_COLUMNS: f32 = 9.;

/// The 26 characters and a random cell as a 9-column grid of 40 ghost
/// tiles, gap 8, the idle look at 32; the chosen one has the selected fill
/// and a `label` edge. `pick` gets the clicked (or the random) character.
pub fn character_picker(
    id: &'static str,
    selected: Option<CharacterId>,
    disabled: bool,
    pick: impl Fn(CharacterId, &mut Window, &mut App) + Clone + 'static,
    cx: &App,
) -> gpui::Div {
    let k = native_colors(cx);
    let cell = tokens::avatar::LG;
    let gap = tokens::space::S8;
    let random_pick = pick.clone();
    let random =
        crate::ui::native::tile_button(SharedString::from(format!("{id}-random")), cell, false, cx)
            .disabled(disabled)
            .tooltip(s::CHARACTER_RANDOM)
            .child(crate::ui::icons::icon(
                ui::Sym::Refresh,
                tokens::icon::MD,
                k.secondary,
            ))
            .on_click(move |_, window, cx| {
                let all = CharacterId::ALL;
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos() as usize)
                    .unwrap_or(0);
                let mut c = all[nanos % all.len()];
                if Some(c) == selected {
                    c = all[(nanos + 1) % all.len()];
                }
                random_pick(c, window, cx)
            });
    // ← → ↑ ↓ walk the tiles (§2.9 grid, review 3 C1-10): a key on a
    // focused tile bubbles here and moves the focus, ↑ / ↓ by a row.
    let walk = |e: &gpui::KeyDownEvent, window: &mut Window, cx: &mut App| {
        if e.keystroke.modifiers.modified() {
            return;
        }
        let (next, steps) = match e.keystroke.key.as_str() {
            "right" => (true, 1),
            "left" => (false, 1),
            "down" => (true, PICKER_COLUMNS as usize),
            "up" => (false, PICKER_COLUMNS as usize),
            _ => return,
        };
        for _ in 0..steps {
            if next {
                window.focus_next(cx);
            } else {
                window.focus_prev(cx);
            }
        }
        cx.stop_propagation();
    };
    // Centered in its column, so the void is shared by both sides.
    let grid = div()
        .w(cell * PICKER_COLUMNS + gap * (PICKER_COLUMNS - 1.))
        .flex()
        .flex_wrap()
        .gap(gap)
        .children(CharacterId::ALL.iter().enumerate().map(|(ix, c)| {
            let c = *c;
            let pick = pick.clone();
            crate::ui::native::tile_button(
                ElementId::NamedInteger(id.into(), ix as u64),
                cell,
                selected == Some(c),
                cx,
            )
            .disabled(disabled)
            .child(characters::image(
                c,
                CharacterStatus::Idle,
                tokens::avatar::MD,
            ))
            .on_click(move |_, window, cx| pick(c, window, cx))
        }))
        .child(random);
    div()
        .w_full()
        .flex()
        .justify_center()
        .on_key_down(walk)
        .child(grid)
}

/// The look a new bot gets until one is picked by hand: the roster's character for `name`,
/// else the first of the 26 that no member in `taken` uses, else the first one.
pub fn default_character(name: &str, taken: &[CharacterId]) -> CharacterId {
    characters::roster_character(name)
        .or_else(|| {
            CharacterId::ALL
                .iter()
                .copied()
                .find(|c| !taken.contains(c))
        })
        .unwrap_or(CharacterId::ALL[0])
}

// --- invite ---

/// The 邀请成员 sheet: a form, then the code shown once.
pub struct InviteForm {
    services: Services,
    name: Entity<InputState>,
    role: Entity<InputState>,
    kind: MemberKind,
    level: MemberLevel,
    status: Status,
    created: Option<InviteCreated>,
    _subscriptions: Vec<Subscription>,
}

impl InviteForm {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder(s::INVITE_NAME_PLACEHOLDER));
        let role = cx.new(|cx| InputState::new(window, cx).placeholder(s::ROLE_PLACEHOLDER));
        // The default button and the field errors follow the typing.
        let notify =
            |_: &mut Self, _: Entity<InputState>, e: &InputEvent, cx: &mut Context<Self>| {
                if matches!(e, InputEvent::Change) {
                    cx.notify();
                }
            };
        let subscriptions = vec![cx.subscribe(&name, notify), cx.subscribe(&role, notify)];
        Self {
            services,
            name,
            role,
            kind: MemberKind::Human,
            level: MemberLevel::Member,
            status: Status::Idle,
            created: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn created(&self) -> Option<&InviteCreated> {
        self.created.as_ref()
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn fill(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.name
            .update(cx, |st, cx| st.set_value(name.to_string(), window, cx));
    }

    /// Types `role` into 岗位 (self-test scenes).
    pub fn fill_role(&mut self, role: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.role
            .update(cx, |st, cx| st.set_value(role.to_string(), window, cx));
    }

    pub fn submit(&mut self, cx: &mut Context<Self>) {
        if self.status.busy() || self.created.is_some() {
            return;
        }
        let name = self.name.read(cx).value().trim().to_string();
        let role = self.role.read(cx).value().trim().to_string();
        if name.is_empty() {
            self.status = Status::Failed(s::NAME_REQUIRED.into());
            cx.notify();
            return;
        }
        if let Err(e) = widgets::check_len(&name, 64).and(widgets::check_len(&role, 64)) {
            self.status = Status::Failed(e.into());
            cx.notify();
            return;
        }
        let reply = self.services.provider.create_invite(InviteRequest {
            name,
            kind: self.kind,
            level: self.level,
            role: (!role.is_empty()).then_some(role),
        });
        self.status = Status::Busy(s::SAVING.into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(created) => {
                        this.created = Some(created);
                        this.status = Status::Idle;
                    }
                    Err(e) => this.status = Status::from_error(e),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for InviteForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(created) = &self.created {
            return SheetLayout::new(s::INVITE_CREATED)
                .explanation(s::SHOWN_ONCE)
                .body(secret_row(
                    "invite-code",
                    s::INVITE_CODE,
                    created.code.expose(),
                    cx,
                ))
                .body(widgets::quiet(
                    fill(
                        s::INVITE_EXPIRES,
                        &[("date", &widgets::date_text(created.expires_at))],
                    ),
                    cx,
                ))
                .body(secret_row(
                    "invite-download",
                    s::INVITE_DOWNLOAD,
                    created.download_url.expose(),
                    cx,
                ))
                .body(widgets::quiet(s::INVITE_DOWNLOAD_HINT, cx))
                .buttons([done_button("invite-done", cx)])
                .into_any_element();
        }
        let busy = self.status.busy();
        let (off, name_err, role_err) = name_role_problem(
            self.name.read(cx).value().trim(),
            self.role.read(cx).value().trim(),
        );
        let (kind_page, level_page, submit, retry) =
            (cx.entity(), cx.entity(), cx.entity(), cx.entity());
        let failed = refusal(
            "invite-failed",
            &self.status,
            s::INVITE_FAILED,
            move |_, cx| retry.update(cx, |f, cx| f.submit(cx)),
            cx,
        );
        SheetLayout::new(s::INVITE)
            .explanation(s::INVITE_HINT)
            .body(
                sheet_form()
                    .child(field_row(
                        s::NAME,
                        "member-name",
                        &self.name,
                        name_err.is_some(),
                        cx,
                    ))
                    .child(field_row(
                        s::ROLE,
                        "member-role",
                        &self.role,
                        role_err.is_some(),
                        cx,
                    ))
                    .child(choice_row(
                        s::INVITE_KIND,
                        "invite-kind",
                        [s::KIND_HUMAN, s::KIND_AI],
                        self.kind == MemberKind::Bot,
                        busy,
                        move |bot, _, cx| {
                            kind_page.update(cx, |f, cx| {
                                f.kind = if bot {
                                    MemberKind::Bot
                                } else {
                                    MemberKind::Human
                                };
                                cx.notify();
                            })
                        },
                        window,
                        cx,
                    ))
                    .child(choice_row(
                        s::MEMBER_LEVEL,
                        "invite-level",
                        [s::LEVEL_MEMBER, s::LEVEL_OWNER],
                        self.level == MemberLevel::Owner,
                        busy,
                        move |owner, _, cx| {
                            level_page.update(cx, |f, cx| {
                                f.level = if owner {
                                    MemberLevel::Owner
                                } else {
                                    MemberLevel::Member
                                };
                                cx.notify();
                            })
                        },
                        window,
                        cx,
                    )),
            )
            .when_some(failed, |l, f| l.body(f))
            .when_some(name_err.clone().or(role_err.clone()), |l, e| {
                l.status(crate::ui::native::sheet::sheet_status_error(e, cx))
            })
            .buttons(sheet_actions(
                "invite",
                s::INVITE_CREATE,
                busy || off,
                move |_, cx| submit.update(cx, |f, cx| f.submit(cx)),
                cx,
            ))
            .into_any_element()
    }
}

/// Opens a form sheet holding `form`; Return runs `submit`, the first field
/// takes the focus once.
fn open_form_sheet<V: Render>(
    form: Entity<V>,
    width: SheetWidth,
    floor: gpui::Pixels,
    first_field: impl Fn(&V) -> Option<Entity<InputState>> + 'static,
    submit: impl Fn(&mut V, &mut Context<V>) + Clone + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let first = std::cell::Cell::new(true);
    widgets::open_sheet(window, cx, width, floor, move |dialog, window, cx| {
        if first.replace(false)
            && let Some(field) = first_field(form.read(cx))
        {
            field.read(cx).focus_handle(cx).focus(window, cx);
        }
        let (ok, submit) = (form.clone(), submit.clone());
        dialog
            .on_ok(move |_, _, cx| {
                ok.update(cx, |f, cx| submit(f, cx));
                false
            })
            .child(form.clone())
    });
}

pub fn open_invite(services: Services, window: &mut Window, cx: &mut App) -> Entity<InviteForm> {
    let form = cx.new(|cx| InviteForm::new(services, window, cx));
    open_form_sheet(
        form.clone(),
        SheetWidth::Form,
        px(470.),
        |f| (f.created.is_none()).then(|| f.name.clone()),
        |f, cx| f.submit(cx),
        window,
        cx,
    );
    form
}

// --- AI member ---

/// The 添加 AI 成员 sheet: name and 岗位, the look, then either an outside
/// credential (shown once) or work on the team's computers.
pub struct BotForm {
    services: Services,
    name: Entity<InputState>,
    role: Entity<InputState>,
    runner: bool,
    labels: Entity<InputState>,
    runtime: Entity<InputState>,
    repo: Entity<InputState>,
    branch: Entity<InputState>,
    /// Runtime names that machines report.
    known_runtimes: Vec<String>,
    /// The character picked by hand; until then [`default_character`] for the typed name.
    character: Option<CharacterId>,
    status: Status,
    created: Option<BotCreated>,
    _subscriptions: Vec<Subscription>,
}

impl BotForm {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let machines = services.provider.machines();
        cx.spawn(async move |this, cx| {
            if let Ok(list) = answer(machines).await {
                let mut known: Vec<String> = list.into_iter().flat_map(|m| m.runtimes).collect();
                known.sort();
                known.dedup();
                let _ = this.update(cx, |this, cx| {
                    this.known_runtimes = known;
                    cx.notify();
                });
            }
        })
        .detach();
        let input = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
        };
        let name = input(s::BOT_NAME_PLACEHOLDER, window, cx);
        // The roster's character follows the name as it is typed.
        let role = input(s::ROLE_PLACEHOLDER, window, cx);
        let runtime = input(s::RUNNER_RUNTIME_PLACEHOLDER, window, cx);
        let notify =
            |_: &mut Self, _: Entity<InputState>, e: &InputEvent, cx: &mut Context<Self>| {
                if matches!(e, InputEvent::Change) {
                    cx.notify();
                }
            };
        let subscriptions = vec![
            cx.subscribe(&name, notify),
            cx.subscribe(&role, notify),
            cx.subscribe(&runtime, notify),
        ];
        Self {
            name,
            role,
            labels: input(s::RUNNER_LABELS_PLACEHOLDER, window, cx),
            runtime,
            repo: input(s::RUNNER_REPO_PLACEHOLDER, window, cx),
            branch: input(s::RUNNER_BRANCH_PLACEHOLDER, window, cx),
            services,
            runner: false,
            known_runtimes: Vec::new(),
            character: None,
            status: Status::Idle,
            created: None,
            _subscriptions: subscriptions,
        }
    }

    /// The character the new member gets: picked by hand, or
    /// [`default_character`] for the name.
    fn chosen_character(&self, cx: &App) -> CharacterId {
        self.character.unwrap_or_else(|| {
            let taken: Vec<CharacterId> = self
                .services
                .provider
                .members()
                .into_iter()
                .filter_map(|m| m.character)
                .collect();
            default_character(&self.value(&self.name, cx), &taken)
        })
    }

    pub fn created(&self) -> Option<&BotCreated> {
        self.created.as_ref()
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn fill(
        &mut self,
        name: &str,
        runner: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.name
            .update(cx, |st, cx| st.set_value(name.to_string(), window, cx));
        self.runner = runner.is_some();
        if let Some(rt) = runner {
            self.runtime
                .update(cx, |st, cx| st.set_value(rt.to_string(), window, cx));
        }
    }

    fn value(&self, input: &Entity<InputState>, cx: &App) -> String {
        input.read(cx).value().trim().to_string()
    }

    pub fn submit(&mut self, cx: &mut Context<Self>) {
        if self.status.busy() || self.created.is_some() {
            return;
        }
        let name = self.value(&self.name, cx);
        let role = self.value(&self.role, cx);
        if name.is_empty() {
            self.status = Status::Failed(s::NAME_REQUIRED.into());
            cx.notify();
            return;
        }
        if let Err(e) = widgets::check_len(&name, 64).and(widgets::check_len(&role, 64)) {
            self.status = Status::Failed(e.into());
            cx.notify();
            return;
        }
        let runner = if self.runner {
            let runtime = self.value(&self.runtime, cx);
            if runtime.is_empty() {
                self.status = Status::Failed(s::RUNNER_RUNTIME_REQUIRED.into());
                cx.notify();
                return;
            }
            let repo = self.value(&self.repo, cx);
            let branch = self.value(&self.branch, cx);
            Some(RunnerSetup {
                runtime,
                labels: widgets::split_labels(&self.value(&self.labels, cx)),
                repo: (!repo.is_empty()).then_some(repo),
                base_branch: (!branch.is_empty()).then_some(branch),
            })
        } else {
            None
        };
        let character = Some(self.chosen_character(cx));
        let reply = self.services.provider.create_bot_member(
            name,
            (!role.is_empty()).then_some(role),
            character,
            runner,
        );
        self.status = Status::Busy(s::SAVING.into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(created) => {
                        this.created = Some(created);
                        this.status = Status::Idle;
                    }
                    Err(e) => this.status = Status::from_error(e),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for BotForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(created) = &self.created {
            let name = created.member.name.clone();
            let layout = SheetLayout::new(fill(s::BOT_CREATED, &[("name", &name)]));
            let layout = match &created.credential {
                Some(credential) => layout
                    .explanation(s::SHOWN_ONCE)
                    .body(secret_row(
                        "bot-credential",
                        s::BOT_CREDENTIAL,
                        credential.expose(),
                        cx,
                    ))
                    .body(widgets::quiet(s::BOT_CREDENTIAL_HINT, cx)),
                None => layout.explanation(fill(s::BOT_RUNNER_READY, &[("name", &name)])),
            };
            return layout
                .buttons([done_button("bot-done", cx)])
                .into_any_element();
        }
        let busy = self.status.busy();
        let (off, name_err, role_err) =
            name_role_problem(&self.value(&self.name, cx), &self.value(&self.role, cx));
        // Work on the team's computers needs a runtime.
        let off = off || (self.runner && self.value(&self.runtime, cx).is_empty());
        let (mode_page, pick_page, submit, retry) =
            (cx.entity(), cx.entity(), cx.entity(), cx.entity());
        let failed = refusal(
            "bot-failed",
            &self.status,
            s::BOT_FAILED,
            move |_, cx| retry.update(cx, |f, cx| f.submit(cx)),
            cx,
        );
        let mut form = sheet_form()
            .child(field_row(
                s::NAME,
                "member-name",
                &self.name,
                name_err.is_some(),
                cx,
            ))
            .child(field_row(
                s::ROLE,
                "member-role",
                &self.role,
                role_err.is_some(),
                cx,
            ))
            .child(choice_row(
                s::BOT_MODE,
                "bot-mode",
                [s::BOT_MODE_EXTERNAL, s::BOT_MODE_RUNNER],
                self.runner,
                busy,
                move |runner, _, cx| {
                    mode_page.update(cx, |f, cx| {
                        f.runner = runner;
                        f.status = Status::Idle;
                        cx.notify();
                    })
                },
                window,
                cx,
            ));
        form = form.child(sheet_footnote(
            if self.runner {
                s::BOT_MODE_RUNNER_HINT
            } else {
                s::BOT_MODE_EXTERNAL_HINT
            },
            cx,
        ));
        if self.runner {
            let runtime = sheet_form_row(
                s::RUNNER_RUNTIME,
                div()
                    .flex()
                    .items_center()
                    .gap(crate::ui::native::tokens::space::S6)
                    .when(!self.known_runtimes.is_empty(), |d| {
                        let known = self.known_runtimes.clone();
                        let input = self.runtime.clone();
                        d.child(
                            pull_down(
                                "known-runtime",
                                s::RUNNER_RUNTIME_KNOWN.trim_end_matches('：'),
                                PushSize::Regular,
                                window,
                                cx,
                            )
                            .dropdown_menu(move |menu, _, _| {
                                known.iter().fold(menu, |menu, rt| {
                                    let (rt, input) = (rt.clone(), input.clone());
                                    menu.item(PopupMenuItem::new(rt.clone()).on_click(
                                        move |_, window, cx| {
                                            input.update(cx, |st, cx| {
                                                st.set_value(rt.clone(), window, cx)
                                            })
                                        },
                                    ))
                                })
                            }),
                        )
                    })
                    .child(NativeTextField::new("member-runtime", &self.runtime).full_width()),
                cx,
            );
            form = form
                .child(runtime)
                .child(field_row(
                    s::RUNNER_LABELS,
                    "member-labels",
                    &self.labels,
                    false,
                    cx,
                ))
                .child(field_row(
                    s::RUNNER_REPO,
                    "member-repo",
                    &self.repo,
                    false,
                    cx,
                ))
                .child(field_row(
                    s::RUNNER_BRANCH,
                    "member-branch",
                    &self.branch,
                    false,
                    cx,
                ));
        }
        form = form.child(sheet_form_row_top(
            s::BOT_CHARACTER,
            character_picker(
                "bot-character",
                Some(self.chosen_character(cx)),
                busy,
                move |c, _, cx| {
                    pick_page.update(cx, |f, cx| {
                        f.character = Some(c);
                        cx.notify();
                    })
                },
                cx,
            ),
            cx,
        ));
        SheetLayout::new(s::ADD_AI_TITLE)
            .explanation(s::ADD_AI_HINT)
            .body(
                div()
                    .id("bot-form-body")
                    .max_h(px(420.))
                    .overflow_y_scroll()
                    .child(form),
            )
            .when_some(failed, |l, f| l.body(f))
            .when_some(name_err.clone().or(role_err.clone()), |l, e| {
                l.status(crate::ui::native::sheet::sheet_status_error(e, cx))
            })
            .buttons(sheet_actions(
                "bot",
                s::BOT_CREATE,
                busy || off,
                move |_, cx| submit.update(cx, |f, cx| f.submit(cx)),
                cx,
            ))
            .into_any_element()
    }
}

pub fn open_add_bot(services: Services, window: &mut Window, cx: &mut App) -> Entity<BotForm> {
    let form = cx.new(|cx| BotForm::new(services, window, cx));
    open_form_sheet(
        form.clone(),
        SheetWidth::List,
        px(660.),
        |f| (f.created.is_none()).then(|| f.name.clone()),
        |f, cx| f.submit(cx),
        window,
        cx,
    );
    form
}

// --- the look of an AI member ---

/// The sheet that changes an AI member's look: click one, it saves at once.
struct CharacterForm {
    services: Services,
    member_id: String,
    name: String,
    selected: Option<CharacterId>,
    status: Status,
}

impl CharacterForm {
    fn pick(&mut self, c: CharacterId, window: &mut Window, cx: &mut Context<Self>) {
        if self.status.busy() {
            return;
        }
        let reply = self
            .services
            .provider
            .set_member_character(&self.member_id, Some(c));
        self.selected = Some(c);
        self.status = Status::Busy(s::SAVING.into());
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(_) => window.close_dialog(cx),
                Err(e) => {
                    this.status = Status::from_error(e);
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

impl Render for CharacterForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = cx.entity();
        SheetLayout::new(fill(s::CHANGE_CHARACTER_TITLE, &[("name", &self.name)]))
            .explanation(s::BOT_CHARACTER_HINT)
            .body(character_picker(
                "member-character",
                self.selected,
                self.status.busy(),
                move |c, window, cx| page.update(cx, |this, cx| this.pick(c, window, cx)),
                cx,
            ))
            .when_some(
                match &self.status {
                    Status::Failed(t) => Some(widgets::failed_alert(
                        "character-failed",
                        s::CHARACTER_FAILED,
                        Some(t.clone()),
                        None,
                        cx,
                    )),
                    _ => None,
                },
                |l, f| l.body(f),
            )
            .buttons([push_button(
                "member-character-cancel",
                common::CANCEL,
                PushStyle::Outline,
                PushSize::Regular,
                cx,
            )
            .on_click(|_, window, cx| window.close_dialog(cx))])
    }
}

pub fn open_character(services: Services, member: &Member, window: &mut Window, cx: &mut App) {
    let form = cx.new(|_| CharacterForm {
        services,
        member_id: member.id.clone(),
        name: one_line(&member.name),
        selected: member.character,
        status: Status::Idle,
    });
    widgets::open_sheet(
        window,
        cx,
        SheetWidth::List,
        px(420.),
        move |dialog, _, _| dialog.child(form.clone()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(mut m: Member) -> Member {
        m.level = MemberLevel::Owner;
        m
    }

    fn m(id: &str, name: &str, bot: bool, off: bool) -> Member {
        Member {
            id: id.into(),
            kind: if bot {
                MemberKind::Bot
            } else {
                MemberKind::Human
            },
            name: name.into(),
            role: None,
            level: MemberLevel::Member,
            avatar_url: None,
            deactivated: off,
            online: false,
            character: None,
        }
    }

    #[test]
    fn order_me_owners_people_bots_then_deactivated() {
        let list = sorted(
            vec![
                m("1", "Zed", false, true),
                m("2", "bot b", true, false),
                m("3", "amy", false, false),
                m("4", "Bob", false, false),
                owner(m("5", "Yan", false, false)),
                owner(m("6", "Wu", false, false)),
                owner(m("7", "Ace bot", true, false)),
                m("8", "Cara", false, false),
            ],
            Some("5"),
        );
        let ids: Vec<_> = list.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["5", "6", "3", "4", "8", "7", "2", "1"]);
    }

    #[test]
    fn a_new_bot_always_gets_a_character() {
        // The roster wins, even when another member uses that look.
        assert_eq!(
            default_character("Jojo", &[CharacterId::Jojo]),
            CharacterId::Jojo
        );
        // A name outside the roster gets the first free look.
        let all = CharacterId::ALL;
        assert_eq!(default_character("新机器人", &[]), all[0]);
        assert_eq!(default_character("新机器人", &[all[0], all[2]]), all[1]);
        // All 26 used: the first one.
        assert_eq!(default_character("新机器人", all), all[0]);
        assert_eq!(default_character("", &all[..25]), all[25]);
    }

    #[test]
    fn spend_limit_is_whole_yuan_or_none() {
        assert_eq!(parse_spend(" "), Ok(None));
        assert_eq!(parse_spend("¥50"), Ok(Some(50)));
        assert_eq!(parse_spend("1,000,000"), Ok(Some(1_000_000)));
        assert_eq!(parse_spend("0"), Err(()));
        assert_eq!(parse_spend("1000001"), Err(()));
        assert_eq!(parse_spend("12.5"), Err(()));
    }
}
