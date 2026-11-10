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
use crate::views::nav::dialogs::{sheet_form, sheet_form_row};
use crate::{
    app::{Changes, Services},
    characters::{self, CharacterId, CharacterStatus},
    copy::{common, fill, settings as s},
    data::{
        ApprovalDefaults, ApproverRole, BotCreated, BotMemberEdit, Change, InviteCreated,
        InviteRequest, MachineInfo, Member, MemberKind, MemberLevel, ModelChoice, RunnerChange,
        RunnerSetup,
    },
    theme,
    ui::{
        self,
        native::{
            AlertLayout, BadgeState, FormRow, GroupedForm, NativeSwitch, NativeTextField,
            NativeType as _, PushSize, PushStyle, SearchField, SectionTitle, SheetLayout,
            SheetWidth, StatusBadge, alert_button, disabled_push, inline_commit, metrics,
            native_colors, page::outline_badge, push_button, tokens,
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
/// 在哪台电脑干活 shows a computer name and 这台电脑.
const MACHINE_SELECT_WIDTH: gpui::Pixels = tokens::size::SELECT_WIDE;
/// The spend limit field (2.8 number field).
const SPEND_WIDTH: gpui::Pixels = metrics::LABEL_COLUMN;

/// The spend limit as the field shows it: "1,000,000" (the "¥" is the
/// field's prefix), or empty (不限).
fn spend_text(n: Option<u32>) -> String {
    n.map(|n| widgets::grouped(n.into())).unwrap_or_default()
}

/// What the 成员 pane draws, read from the provider when it changes rather
/// than on every frame: the members come from the store, and each AI
/// member's word reads the asks and the runs (scrolling redraws the pane
/// every frame).
#[derive(Default)]
struct Roster {
    owner: bool,
    me: Option<String>,
    /// Sorted for the pane (`sorted`).
    members: Vec<Member>,
    /// Lowercased one-line names that more than one member has.
    same_names: std::collections::HashSet<String>,
    /// An AI member's word (等你, 在做 n/m, 空闲) by member id.
    words: std::collections::HashMap<String, String>,
}

impl Roster {
    fn read(services: &Services) -> Self {
        let me = services.me();
        let members = sorted(services.provider.members(), me.as_deref());
        let mut seen = std::collections::HashMap::<String, usize>::new();
        for m in &members {
            *seen.entry(one_line(&m.name).to_lowercase()).or_default() += 1;
        }
        let same_names = seen
            .into_iter()
            .filter(|(_, n)| *n > 1)
            .map(|(name, _)| name)
            .collect();
        let words = Self::words(services, me.as_deref(), &members);
        Roster {
            owner: i_am_owner(services),
            me,
            members,
            same_names,
            words,
        }
    }

    fn words(
        services: &Services,
        me: Option<&str>,
        members: &[Member],
    ) -> std::collections::HashMap<String, String> {
        members
            .iter()
            .filter(|m| m.is_bot() && !m.deactivated)
            .map(|m| {
                let word = crate::views::members_page::ai_member_word(
                    services.provider.as_ref(),
                    me,
                    &m.id,
                );
                (m.id.clone(), word)
            })
            .collect()
    }

    fn same_name(&self, m: &Member) -> bool {
        self.same_names.contains(&one_line(&m.name).to_lowercase())
    }
}

pub struct MembersPage {
    services: Services,
    roster: Roster,
    /// The last row request's result (level or deactivate).
    status: Status,
    rules: Option<ApprovalDefaults>,
    rules_status: Status,
    spend: Entity<InputState>,
    filter: Entity<InputState>,
    /// What the pane draws: both parts in Settings (self-tests), one in 团队管理.
    part: MembersPart,
    _subscriptions: Vec<Subscription>,
}

/// The parts of the 成员 pane: the roster, and 默认规则.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MembersPart {
    #[default]
    Both,
    Roster,
    Rules,
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
            cx.subscribe(&services.hub, |this, _, changes: &Changes, cx| {
                if changes.any(|c| matches!(c, Change::Members | Change::Session)) {
                    this.roster = Roster::read(&this.services);
                    cx.notify();
                } else if changes.any(|c| {
                    matches!(
                        c,
                        Change::Todo | Change::Runs { .. } | Change::Messages { .. }
                    )
                }) {
                    // An AI member's word follows its asks and runs.
                    let words = Roster::words(
                        &this.services,
                        this.roster.me.as_deref(),
                        &this.roster.members,
                    );
                    if words != this.roster.words {
                        this.roster.words = words;
                        cx.notify();
                    }
                }
            }),
            inline_commit(&spend, window, cx, |this, text, window, cx| {
                this.commit_spend(&text, window, cx)
            }),
        ];
        Self {
            roster: Roster::read(&services),
            services,
            status: Status::Idle,
            rules: None,
            rules_status: Status::Idle,
            spend,
            filter,
            part: MembersPart::Both,
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
    /// 团队管理 shows the roster and 默认规则 as two pages.
    pub fn set_part(&mut self, part: MembersPart, cx: &mut Context<Self>) {
        self.part = part;
        cx.notify();
    }

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
        // An AI member says the word of its 成员 row (§2.12, review 5 K-19:
        // 等你, 在做 n/m, 空闲); a person says 在线 or 离线.
        let state: Option<String> = if m.deactivated {
            None
        } else if m.is_bot() {
            Some(self.roster.words.get(&m.id).cloned().unwrap_or_default())
        } else if m.online {
            Some(s::MEMBER_ONLINE.to_string())
        } else {
            Some(s::MEMBER_OFFLINE.to_string())
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
            .chain(state.as_deref())
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
        // "⋯": 编辑… and 更换形象… for an AI member, 停用…; or 恢复.
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
                let (edit_page, edit_member) = (page.clone(), member.clone());
                let (page, member) = (page.clone(), member.clone());
                let menu = menu.item(PopupMenuItem::new(s::EDIT).on_click(move |_, window, cx| {
                    let services = edit_page.read(cx).services.clone();
                    open_edit_bot(services, &edit_member, window, cx);
                }));
                // No look to change without the character pack.
                if characters::available() {
                    menu.item(PopupMenuItem::new(s::CHANGE_CHARACTER).on_click(
                        move |_, window, cx| {
                            let services = page.read(cx).services.clone();
                            open_character(services, &member, window, cx);
                        },
                    ))
                } else {
                    menu
                }
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
        let owner = self.roster.owner;
        let me = self.roster.me.clone();
        let members = &self.roster.members;
        let same_name = |m: &Member| self.roster.same_name(m);
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
        if self.part == MembersPart::Rules {
            return widgets::pane().child(self.rules_section(owner, window, cx));
        }
        widgets::pane()
            .child(members_section)
            .children(off_section)
            .when(self.part == MembersPart::Both, |p| {
                p.child(self.rules_section(owner, window, cx))
            })
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
///
/// The grid is one Tab stop (§2.9 grid, E3): the focused tile, else the
/// chosen one, else the first; ← → ↑ ↓ walk the tiles inside it.
pub fn character_picker(
    id: &'static str,
    selected: Option<CharacterId>,
    disabled: bool,
    pick: impl Fn(CharacterId, &mut Window, &mut App) + Clone + 'static,
    window: &mut Window,
    cx: &mut App,
) -> gpui::Div {
    let count = CharacterId::ALL.len() + 1;
    let handles = window
        .use_keyed_state(
            SharedString::from(format!("{id}-focus")),
            cx,
            move |_, cx| (0..count).map(|_| cx.focus_handle()).collect::<Vec<_>>(),
        )
        .read(cx)
        .clone();
    let focused = handles.iter().position(|h| h.is_focused(window));
    let rover = focused
        .or_else(|| selected.and_then(|c| CharacterId::ALL.iter().position(|x| *x == c)))
        .unwrap_or(0);
    let k = native_colors(cx);
    let cell = tokens::avatar::LG;
    let gap = tokens::space::S8;
    let random_pick = pick.clone();
    let last = count - 1;
    let random =
        crate::ui::native::tile_button(SharedString::from(format!("{id}-random")), cell, false, cx)
            .track_focus(&handles[last])
            .tab_stop(rover == last)
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
    // focused tile bubbles here and moves the focus, ↑ / ↓ by a row; the
    // walk stops at the grid's edges.
    let walk_handles = handles.clone();
    let walk = move |e: &gpui::KeyDownEvent, window: &mut Window, cx: &mut App| {
        if e.keystroke.modifiers.modified() {
            return;
        }
        let Some(at) = walk_handles.iter().position(|h| h.is_focused(window)) else {
            return;
        };
        let columns = PICKER_COLUMNS as usize;
        let to = match e.keystroke.key.as_str() {
            "right" => (at + 1 < count).then_some(at + 1),
            "left" => at.checked_sub(1),
            "down" => (at + columns < count).then_some(at + columns),
            "up" => at.checked_sub(columns),
            _ => return,
        };
        if let Some(to) = to {
            window.focus(&walk_handles[to], cx);
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
            .track_focus(&handles[ix])
            .tab_stop(rover == ix)
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

/// A runner as the sheet compares it: trimmed, no empty labels, repo or
/// branch.
fn clean_runner(r: &RunnerSetup) -> RunnerSetup {
    let text = |t: &Option<String>| {
        t.as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
    };
    RunnerSetup {
        runtime: r.runtime.trim().to_string(),
        labels: r
            .labels
            .iter()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
        repo: text(&r.repo),
        base_branch: text(&r.base_branch),
        machine_id: text(&r.machine_id),
        model: text(&r.model),
        effort: text(&r.effort),
    }
}

/// The models and 思考强度 a runtime offers on the computers that can run the member: the
/// picked computer only, else every computer that has the runtime. Models keep their first
/// report; efforts are the union in order.
fn cli_choices(
    machines: &[MachineInfo],
    runtime: &str,
    machine: Option<&str>,
) -> (Vec<ModelChoice>, Vec<String>) {
    let mut models: Vec<ModelChoice> = Vec::new();
    let mut efforts: Vec<String> = Vec::new();
    let clis = machines
        .iter()
        .filter(|m| machine.is_none_or(|id| m.id == id))
        .flat_map(|m| m.clis.iter())
        .filter(|c| c.runtime == runtime);
    for c in clis {
        for m in &c.models {
            if !models.iter().any(|x| x.id == m.id) {
                models.push(m.clone());
            }
        }
        for e in &c.efforts {
            if !efforts.contains(e) {
                efforts.push(e.clone());
            }
        }
    }
    (models, efforts)
}

/// The 思考强度 levels for a model: its own list, else the runtime's. With no model picked,
/// the default model's.
fn effort_options(models: &[ModelChoice], efforts: &[String], model: Option<&str>) -> Vec<String> {
    let chosen = match model {
        Some(id) => models.iter().find(|m| m.id == id),
        None => models.iter().find(|m| m.is_default),
    };
    match chosen {
        Some(m) if !m.efforts.is_empty() => m.efforts.clone(),
        _ => efforts.to_vec(),
    }
}

fn model_text(m: &ModelChoice) -> String {
    m.name.clone().unwrap_or_else(|| m.id.clone())
}

/// 默认, with the default model's name when a computer said which it is.
fn default_model_text(models: &[ModelChoice]) -> String {
    match models.iter().find(|m| m.is_default) {
        Some(m) => s::RUNNER_MODEL_DEFAULT_NAMED.replace("{name}", &model_text(m)),
        None => s::RUNNER_MODEL_DEFAULT.to_string(),
    }
}

/// A 思考强度 level in words; one this build does not know shows as the CLI spells it.
fn effort_text(e: &str) -> String {
    match e {
        "minimal" => s::EFFORT_MINIMAL,
        "low" => s::EFFORT_LOW,
        "medium" => s::EFFORT_MEDIUM,
        "high" => s::EFFORT_HIGH,
        "xhigh" => s::EFFORT_XHIGH,
        "max" => s::EFFORT_MAX,
        "ultra" => s::EFFORT_ULTRA,
        other => return other.to_string(),
    }
    .to_string()
}

/// How 在哪台电脑干活 names a choice: 任意一台空闲的电脑, a computer by name (this Mac
/// marked 这台电脑), or a computer that is no longer in the list.
pub fn machine_choice_text(
    id: Option<&str>,
    machines: &[MachineInfo],
    this: Option<&str>,
) -> String {
    let Some(id) = id else {
        return s::RUNNER_MACHINE_ANY.to_string();
    };
    match machines.iter().find(|m| m.id == id) {
        Some(m) if this == Some(id) => fill(s::RUNNER_MACHINE_THIS, &[("name", &m.name)]),
        Some(m) => m.name.clone(),
        None => s::RUNNER_MACHINE_GONE.to_string(),
    }
}

/// What a save of the edit sheet sends: the name and 岗位 only when they
/// changed, the runner set when it changed and removed when it was switched
/// off.
pub fn bot_edit(
    before_name: &str,
    before_role: Option<&str>,
    before_runner: Option<&RunnerSetup>,
    name: &str,
    role: &str,
    runner: Option<&RunnerSetup>,
) -> BotMemberEdit {
    let name = name.trim();
    let role = Some(role.trim()).filter(|r| !r.is_empty());
    let before_role = before_role.map(str::trim).filter(|r| !r.is_empty());
    let runner = match (before_runner.map(clean_runner), runner.map(clean_runner)) {
        (Some(_), None) => Some(RunnerChange::Remove),
        (before, Some(after)) if before.as_ref() != Some(&after) => Some(RunnerChange::Set(after)),
        _ => None,
    };
    BotMemberEdit {
        name: (name != before_name.trim()).then(|| name.to_string()),
        role: (role != before_role).then(|| role.map(str::to_string)),
        runner,
    }
}

/// The runner of the member being edited, as it loads.
#[derive(Clone, Debug, PartialEq)]
enum RunnerLoad {
    Loading,
    Failed(SharedString),
    Loaded(Option<RunnerSetup>),
}

/// The AI member the sheet edits, as it was when the sheet opened.
struct BotEditing {
    member_id: String,
    name: String,
    role: Option<String>,
    runner: RunnerLoad,
}

/// The 添加 AI 成员 sheet: name and 岗位, the look, then either an outside
/// credential (shown once) or work on the team's computers. The same sheet
/// edits an AI member: name, 岗位 and how it works.
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
    /// The team's computers, for 在哪台电脑干活.
    machines: Vec<MachineInfo>,
    /// This Mac's machine id, marked 这台电脑 in the list.
    this_machine: Option<String>,
    /// The one computer picked to run this member; `None` = any free computer.
    machine: Option<String>,
    /// The model picked from the computers' list; `None` = the CLI's default.
    model: Option<String>,
    /// The model typed when no computer reported a list.
    model_input: Entity<InputState>,
    /// 思考强度 picked; `None` = the CLI's default.
    effort: Option<String>,
    /// The character picked by hand; until then [`default_character`] for the typed name.
    character: Option<CharacterId>,
    status: Status,
    created: Option<BotCreated>,
    /// Set when the sheet edits an existing AI member.
    editing: Option<BotEditing>,
    /// The window the sheet is in: a saved edit closes it.
    window: gpui::AnyWindowHandle,
    _subscriptions: Vec<Subscription>,
}

impl BotForm {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let machines = services.provider.machines();
        let this_machine = services.provider.this_machine_id();
        cx.spawn(async move |this, cx| {
            if let Ok(list) = answer(machines).await {
                let mut known: Vec<String> = list.iter().flat_map(|m| m.runtimes.clone()).collect();
                known.sort();
                known.dedup();
                let _ = this.update(cx, |this, cx| {
                    this.known_runtimes = known;
                    this.machines = list;
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
        let model_input = input(s::RUNNER_MODEL_PLACEHOLDER, window, cx);
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
            cx.subscribe(&model_input, notify),
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
            machines: Vec::new(),
            this_machine,
            machine: None,
            model: None,
            model_input,
            effort: None,
            character: None,
            status: Status::Idle,
            created: None,
            editing: None,
            window: window.window_handle(),
            _subscriptions: subscriptions,
        }
    }

    /// Turns the sheet into the edit sheet of `member` and loads its runner.
    fn edit(&mut self, member: &Member, window: &mut Window, cx: &mut Context<Self>) {
        let role = member.role.clone().unwrap_or_default();
        self.name
            .update(cx, |st, cx| st.set_value(member.name.clone(), window, cx));
        self.role
            .update(cx, |st, cx| st.set_value(role, window, cx));
        self.editing = Some(BotEditing {
            member_id: member.id.clone(),
            name: member.name.clone(),
            role: member.role.clone(),
            runner: RunnerLoad::Loading,
        });
        self.load_runner(window, cx);
    }

    /// Whether the sheet edits an existing member.
    pub fn editing(&self) -> bool {
        self.editing.is_some()
    }

    /// Whether the edited member's runner is in.
    pub fn runner_loaded(&self) -> bool {
        self.editing
            .as_ref()
            .is_some_and(|e| matches!(e.runner, RunnerLoad::Loaded(_)))
    }

    fn load_runner(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editing) = self.editing.as_mut() else {
            return;
        };
        editing.runner = RunnerLoad::Loading;
        let reply = self.services.provider.bot_runner(&editing.member_id);
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(runner) => {
                        this.fill_runner(runner.as_ref(), window, cx);
                        if let Some(e) = this.editing.as_mut() {
                            e.runner = RunnerLoad::Loaded(runner);
                        }
                    }
                    Err(e) => {
                        if let Some(editing) = this.editing.as_mut() {
                            editing.runner = RunnerLoad::Failed(widgets::error_text(e).into());
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn fill_runner(
        &mut self,
        r: Option<&RunnerSetup>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.runner = r.is_some();
        let Some(r) = r else { return };
        let set = |input: &Entity<InputState>, text: String, window: &mut Window, cx: &mut App| {
            input.update(cx, |st, cx| st.set_value(text, window, cx));
        };
        set(&self.runtime, r.runtime.clone(), window, cx);
        set(&self.labels, r.labels.join(", "), window, cx);
        self.machine = r.machine_id.clone();
        self.model = r.model.clone();
        self.effort = r.effort.clone();
        set(
            &self.model_input,
            r.model.clone().unwrap_or_default(),
            window,
            cx,
        );
        set(&self.repo, r.repo.clone().unwrap_or_default(), window, cx);
        set(
            &self.branch,
            r.base_branch.clone().unwrap_or_default(),
            window,
            cx,
        );
    }

    /// 在哪台电脑干活 picked: the hand-typed labels no longer count.
    fn pick_machine(&mut self, id: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.machine = id;
        self.labels
            .update(cx, |st, cx| st.set_value(String::new(), window, cx));
        cx.notify();
    }

    /// The runner the fields describe; `None` when the member does not work
    /// on a machine.
    fn runner_fields(&self, cx: &App) -> Option<RunnerSetup> {
        if !self.runner {
            return None;
        }
        let repo = self.value(&self.repo, cx);
        let branch = self.value(&self.branch, cx);
        Some(RunnerSetup {
            runtime: self.value(&self.runtime, cx),
            labels: widgets::split_labels(&self.value(&self.labels, cx)),
            repo: (!repo.is_empty()).then_some(repo),
            base_branch: (!branch.is_empty()).then_some(branch),
            machine_id: self.machine.clone(),
            model: self.model_value(cx),
            effort: self.effort.clone(),
        })
    }

    /// The model the sheet shows: picked from the list, or typed when no computer reported one.
    fn model_value(&self, cx: &App) -> Option<String> {
        let (models, _) = cli_choices(
            &self.machines,
            &self.value(&self.runtime, cx),
            self.machine.as_deref(),
        );
        if models.is_empty() {
            let typed = self.value(&self.model_input, cx);
            (!typed.is_empty()).then_some(typed)
        } else {
            self.model.clone()
        }
    }

    /// What saving the edit sheet now would change; `None` until the runner
    /// is in.
    fn pending_edit(&self, cx: &App) -> Option<BotMemberEdit> {
        let e = self.editing.as_ref()?;
        let RunnerLoad::Loaded(before) = &e.runner else {
            return None;
        };
        Some(bot_edit(
            &e.name,
            e.role.as_deref(),
            before.as_ref(),
            &self.value(&self.name, cx),
            &self.value(&self.role, cx),
            self.runner_fields(cx).as_ref(),
        ))
    }

    /// Saves the edit sheet; a saved edit closes it.
    fn save(&mut self, cx: &mut Context<Self>) {
        if self.status.busy() {
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
        if self.runner && self.value(&self.runtime, cx).is_empty() {
            self.status = Status::Failed(s::RUNNER_RUNTIME_REQUIRED.into());
            cx.notify();
            return;
        }
        let Some(edit) = self.pending_edit(cx) else {
            return;
        };
        let Some(id) = self.editing.as_ref().map(|e| e.member_id.clone()) else {
            return;
        };
        let handle = self.window;
        if edit.is_empty() {
            let _ = handle.update(cx, |_, window, cx| window.close_dialog(cx));
            return;
        }
        let reply = self.services.provider.update_bot_member(&id, edit);
        self.status = Status::Busy(s::SAVING.into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let saved = result.is_ok();
            let _ = this.update(cx, |this, cx| {
                this.status = match result {
                    Ok(()) => Status::Idle,
                    Err(e) => Status::from_error(e),
                };
                cx.notify();
            });
            if saved {
                let _ = handle.update(cx, |_, window, cx| window.close_dialog(cx));
            }
        })
        .detach();
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
        if self.editing.is_some() {
            return self.save(cx);
        }
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
        let runner = self.runner_fields(cx);
        if runner.as_ref().is_some_and(|r| r.runtime.is_empty()) {
            self.status = Status::Failed(s::RUNNER_RUNTIME_REQUIRED.into());
            cx.notify();
            return;
        }
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
        // Editing: the runner loads first; saving needs it in and a change.
        let load = self.editing.as_ref().map(|e| e.runner.clone());
        let runner_ready = !matches!(load, Some(RunnerLoad::Loading | RunnerLoad::Failed(_)));
        let was_runner = matches!(&load, Some(RunnerLoad::Loaded(Some(_))));
        let off =
            off || (self.editing.is_some() && self.pending_edit(cx).is_none_or(|e| e.is_empty()));
        let (mode_page, pick_page, submit, retry) =
            (cx.entity(), cx.entity(), cx.entity(), cx.entity());
        let failed = refusal(
            "bot-failed",
            &self.status,
            if self.editing.is_some() {
                s::BOT_EDIT_FAILED
            } else {
                s::BOT_FAILED
            },
            move |_, cx| retry.update(cx, |f, cx| f.submit(cx)),
            cx,
        );
        use crate::ui::native::{Segment, Segmented, tokens::NativeType as _};
        let k = crate::ui::native::native_colors(cx);
        // A small label over each block of the sheet (2.3, sheet size).
        let section = |text: &'static str| {
            div()
                .pb(tokens::space::S8)
                .n_subheadline()
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(k.secondary)
                .child(text)
        };
        let mut body = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(tokens::space::S16)
            .child(
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
                    )),
            );
        match &load {
            Some(RunnerLoad::Loading) => {
                body = body.child(widgets::list_loading("bot-runner-loading", 2, false, cx));
            }
            Some(RunnerLoad::Failed(text)) => {
                let again = cx.entity();
                body = body.child(widgets::failed_alert(
                    "bot-runner-failed",
                    s::BOT_RUNNER_LOAD_FAILED,
                    Some(text.clone()),
                    Some(widgets::retry_button("bot-runner-retry", cx).on_click(
                        move |_, window, cx| again.update(cx, |f, cx| f.load_runner(window, cx)),
                    )),
                    cx,
                ));
            }
            _ => {}
        }
        if runner_ready {
            // 工作方式: two choices side by side, the one in effect raised,
            // and one line on what it means under them.
            let hint = if self.runner {
                s::BOT_MODE_RUNNER_HINT
            } else if was_runner {
                s::BOT_EDIT_EXTERNAL_HINT
            } else {
                s::BOT_MODE_EXTERNAL_HINT
            };
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .child(section(s::BOT_MODE))
                    .child(
                        div().flex().child(
                            Segmented::new(
                                "bot-mode",
                                [
                                    Segment::Label(s::BOT_MODE_RUNNER.into()),
                                    Segment::Label(s::BOT_MODE_EXTERNAL.into()),
                                ],
                            )
                            .selected(if self.runner { 0 } else { 1 })
                            .disabled(busy)
                            .on_change(move |ix, _, cx| {
                                mode_page.update(cx, |f, cx| {
                                    f.runner = ix == 0;
                                    f.status = Status::Idle;
                                    cx.notify();
                                })
                            }),
                        ),
                    )
                    .child(
                        div()
                            .pt(tokens::space::S8)
                            .n_subheadline()
                            .text_color(k.secondary)
                            .child(hint),
                    ),
            );
        }
        if runner_ready && self.runner {
            // 运行方式: picked from what the team's computers have; typed only
            // when no computer has reported yet.
            let current = self.value(&self.runtime, cx);
            let (models, efforts) = cli_choices(&self.machines, &current, self.machine.as_deref());
            let mut known = self.known_runtimes.clone();
            if !current.is_empty() && !known.contains(&current) {
                known.insert(0, current.clone());
            }
            let runtime: gpui::AnyElement = if known.is_empty() {
                NativeTextField::new("member-runtime", &self.runtime)
                    .full_width()
                    .into_any_element()
            } else {
                let input = self.runtime.clone();
                let shown = if current.is_empty() {
                    s::RUNNER_RUNTIME_PICK.to_string()
                } else {
                    current.clone()
                };
                widgets::popup_fixed("member-runtime", shown, SELECT_WIDTH, window, cx)
                    .dropdown_menu(move |menu, _, _| {
                        known.iter().fold(menu, |menu, rt| {
                            let (rt, input) = (rt.clone(), input.clone());
                            menu.item(
                                PopupMenuItem::new(rt.clone())
                                    .checked(rt == current)
                                    .on_click(move |_, window, cx| {
                                        input.update(cx, |st, cx| {
                                            st.set_value(rt.clone(), window, cx)
                                        })
                                    }),
                            )
                        })
                    })
                    .into_any_element()
            };
            // 在哪台电脑干活: one of the team's computers, or any free one. Picking a
            // computer (or 任意) replaces the old hand-typed labels.
            let picked = self.machine.clone();
            let shown = machine_choice_text(
                picked.as_deref(),
                &self.machines,
                self.this_machine.as_deref(),
            );
            let mut options: Vec<(Option<String>, String)> =
                vec![(None, s::RUNNER_MACHINE_ANY.to_string())];
            options.extend(self.machines.iter().map(|m| {
                (
                    Some(m.id.clone()),
                    machine_choice_text(Some(&m.id), &self.machines, self.this_machine.as_deref()),
                )
            }));
            let form = cx.entity();
            // 模型 and 思考强度: what the computers that can run it report.
            let model: gpui::AnyElement = if models.is_empty() {
                NativeTextField::new("member-model", &self.model_input)
                    .full_width()
                    .into_any_element()
            } else {
                let picked = self.model.clone();
                let mut options: Vec<(Option<String>, String)> =
                    vec![(None, default_model_text(&models))];
                if let Some(p) = picked
                    .as_ref()
                    .filter(|p| !models.iter().any(|m| &m.id == *p))
                {
                    options.push((Some(p.clone()), p.clone()));
                }
                options.extend(models.iter().map(|m| (Some(m.id.clone()), model_text(m))));
                let shown = options
                    .iter()
                    .find(|(id, _)| *id == picked)
                    .map(|(_, t)| t.clone())
                    .unwrap_or_default();
                let form = form.clone();
                widgets::popup_fixed("member-model", shown, SELECT_WIDTH, window, cx)
                    .dropdown_menu(move |menu, _, _| {
                        options.iter().fold(menu, |menu, (id, text)| {
                            let (id, form) = (id.clone(), form.clone());
                            menu.item(
                                PopupMenuItem::new(text.clone())
                                    .checked(id == picked)
                                    .on_click(move |_, _, cx| {
                                        form.update(cx, |f, cx| {
                                            f.model = id.clone();
                                            cx.notify();
                                        })
                                    }),
                            )
                        })
                    })
                    .into_any_element()
            };
            let model_id = if models.is_empty() {
                let typed = self.value(&self.model_input, cx);
                (!typed.is_empty()).then_some(typed)
            } else {
                self.model.clone()
            };
            let levels = effort_options(&models, &efforts, model_id.as_deref());
            let effort: Option<gpui::AnyElement> = (!levels.is_empty() || self.effort.is_some())
                .then(|| {
                    let picked = self.effort.clone();
                    let mut options: Vec<(Option<String>, String)> =
                        vec![(None, s::RUNNER_EFFORT_DEFAULT.to_string())];
                    if let Some(p) = picked.as_ref().filter(|p| !levels.contains(p)) {
                        options.push((Some(p.clone()), effort_text(p)));
                    }
                    options.extend(levels.iter().map(|e| (Some(e.clone()), effort_text(e))));
                    let shown = options
                        .iter()
                        .find(|(id, _)| *id == picked)
                        .map(|(_, t)| t.clone())
                        .unwrap_or_default();
                    let form = form.clone();
                    widgets::popup_fixed("member-effort", shown, SELECT_WIDTH, window, cx)
                        .dropdown_menu(move |menu, _, _| {
                            options.iter().fold(menu, |menu, (id, text)| {
                                let (id, form) = (id.clone(), form.clone());
                                menu.item(
                                    PopupMenuItem::new(text.clone())
                                        .checked(id == picked)
                                        .on_click(move |_, _, cx| {
                                            form.update(cx, |f, cx| {
                                                f.effort = id.clone();
                                                cx.notify();
                                            })
                                        }),
                                )
                            })
                        })
                        .into_any_element()
                });
            let machine =
                widgets::popup_fixed("member-machine", shown, MACHINE_SELECT_WIDTH, window, cx)
                    .dropdown_menu(move |menu, _, _| {
                        options.iter().fold(menu, |menu, (id, text)| {
                            let (id, form) = (id.clone(), form.clone());
                            menu.item(
                                PopupMenuItem::new(text.clone())
                                    .checked(id == picked)
                                    .on_click(move |_, window, cx| {
                                        form.update(cx, |f, cx| {
                                            f.pick_machine(id.clone(), window, cx)
                                        })
                                    }),
                            )
                        })
                    })
                    .into_any_element();
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .child(section(s::RUNNER_SECTION))
                    .child(
                        sheet_form()
                            .child(sheet_form_row(s::RUNNER_RUNTIME, runtime, cx))
                            .child(sheet_form_row(s::RUNNER_MACHINE, machine, cx))
                            .child(sheet_form_row(s::RUNNER_MODEL, model, cx))
                            .children(effort.map(|e| sheet_form_row(s::RUNNER_EFFORT, e, cx)))
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
                            )),
                    ),
            );
        }
        // The look of an existing member changes in its own sheet (更换形象…).
        // A build without the character pack has no looks to pick from.
        if self.editing.is_none() && characters::available() {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .child(section(s::BOT_CHARACTER))
                    .child(sheet_form().child(div().p(tokens::space::S12).child(
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
                            window,
                            cx,
                        ),
                    ))),
            );
        }
        let (title, explanation, default) = match &self.editing {
            Some(e) => (
                fill(s::EDIT_AI_TITLE, &[("name", &one_line(&e.name))]),
                s::EDIT_AI_HINT,
                common::SAVE,
            ),
            None => (s::ADD_AI_TITLE.to_string(), s::ADD_AI_HINT, s::BOT_CREATE),
        };
        SheetLayout::new(title)
            .explanation(explanation)
            .body(
                div()
                    .id("bot-form-body")
                    .max_h(px(460.))
                    .overflow_y_scroll()
                    .child(body),
            )
            .when_some(failed, |l, f| l.body(f))
            .when_some(name_err.clone().or(role_err.clone()), |l, e| {
                l.status(crate::ui::native::sheet::sheet_status_error(e, cx))
            })
            .buttons(sheet_actions(
                "bot",
                default,
                busy || off,
                move |_, cx| submit.update(cx, |f, cx| f.submit(cx)),
                cx,
            ))
            .into_any_element()
    }
}

pub fn open_add_bot(services: Services, window: &mut Window, cx: &mut App) -> Entity<BotForm> {
    let form = cx.new(|cx| BotForm::new(services, window, cx));
    open_bot_sheet(form, window, cx)
}

/// The edit sheet of an AI member: the 添加 sheet with its name, 岗位 and
/// how it works filled in.
pub fn open_edit_bot(
    services: Services,
    member: &Member,
    window: &mut Window,
    cx: &mut App,
) -> Entity<BotForm> {
    let form = cx.new(|cx| {
        let mut form = BotForm::new(services, window, cx);
        form.edit(member, window, cx);
        form
    });
    open_bot_sheet(form, window, cx)
}

fn open_bot_sheet(form: Entity<BotForm>, window: &mut Window, cx: &mut App) -> Entity<BotForm> {
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page = cx.entity();
        SheetLayout::new(fill(s::CHANGE_CHARACTER_TITLE, &[("name", &self.name)]))
            .explanation(s::BOT_CHARACTER_HINT)
            .body(character_picker(
                "member-character",
                self.selected,
                self.status.busy(),
                move |c, window, cx| page.update(cx, |this, cx| this.pick(c, window, cx)),
                window,
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

    fn cli(
        runtime: &str,
        models: &[(&str, &[&str], bool)],
        efforts: &[&str],
    ) -> crate::data::CliChoice {
        crate::data::CliChoice {
            runtime: runtime.into(),
            models: models
                .iter()
                .map(|(id, e, d)| ModelChoice {
                    id: (*id).into(),
                    name: None,
                    efforts: e.iter().map(|x| x.to_string()).collect(),
                    is_default: *d,
                })
                .collect(),
            efforts: efforts.iter().map(|x| x.to_string()).collect(),
        }
    }

    #[test]
    fn models_and_efforts_come_from_the_computers_that_can_run_it() {
        let a = MachineInfo {
            id: "a".into(),
            clis: vec![
                cli(
                    "codex",
                    &[("gpt-6-astra", &["low", "max"], true)],
                    &["low", "max"],
                ),
                cli("claude", &[("opus", &[], true)], &["low", "high"]),
            ],
            ..Default::default()
        };
        let b = MachineInfo {
            id: "b".into(),
            clis: vec![cli(
                "codex",
                &[
                    ("gpt-6-astra", &["low"], false),
                    ("gpt-6-sol", &["medium"], false),
                ],
                &["medium"],
            )],
            ..Default::default()
        };
        let all = [a, b];
        let (models, efforts) = cli_choices(&all, "codex", None);
        assert_eq!(
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["gpt-6-astra", "gpt-6-sol"]
        );
        assert_eq!(efforts, ["low", "max", "medium"]);
        // A picked computer narrows the list to its own.
        let (only_b, _) = cli_choices(&all, "codex", Some("b"));
        assert_eq!(only_b.len(), 2);
        assert!(!only_b[0].is_default);
        // No model picked: the default model's levels; a model without its own: the runtime's.
        assert_eq!(effort_options(&models, &efforts, None), ["low", "max"]);
        assert_eq!(
            effort_options(&models, &efforts, Some("gpt-6-sol")),
            ["medium"]
        );
        let (claude, ce) = cli_choices(&all, "claude", None);
        assert_eq!(effort_options(&claude, &ce, Some("opus")), ["low", "high"]);
        assert!(cli_choices(&all, "acp", None).0.is_empty());
    }

    #[test]
    fn efforts_and_the_default_model_read_as_words() {
        assert_eq!(effort_text("high"), s::EFFORT_HIGH);
        assert_eq!(effort_text("turbo"), "turbo");
        let m = ModelChoice {
            id: "opus".into(),
            name: None,
            efforts: vec![],
            is_default: true,
        };
        assert_eq!(default_model_text(&[m]), "默认（opus）");
        assert_eq!(default_model_text(&[]), s::RUNNER_MODEL_DEFAULT);
    }

    /// The look grid in a bare window with the app's Tab keys; its root
    /// holds the focus, as the app's windows do.
    struct Grid(gpui::FocusHandle);

    impl Render for Grid {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .track_focus(&self.0)
                .on_action(|_: &crate::app::FocusNext, window, cx| window.focus_next(cx))
                .on_action(|_: &crate::app::FocusPrev, window, cx| window.focus_prev(cx))
                .child(character_picker(
                    "grid",
                    Some(CharacterId::ALL[3]),
                    false,
                    |_, _, _| {},
                    window,
                    cx,
                ))
        }
    }

    /// The grid is one Tab stop on the chosen look; the arrow keys walk the
    /// tiles, and Tab stays on the tile the arrows reached (E3, §2.9).
    #[gpui::test]
    fn the_look_grid_is_one_tab_stop(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::app::bind_view_keys(cx);
        });
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |window, cx| {
                let view = cx.new(|cx| Grid(cx.focus_handle()));
                let root = view.read(cx).0.clone();
                window.focus(&root, cx);
                cx.new(|cx| gpui_base::Root::new(view, window, cx))
            })
            .expect("the window opens")
        });
        let cx = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        cx.run_until_parked();
        cx.simulate_keystrokes("tab");
        let chosen = cx.update(|window, cx| window.focused(cx));
        assert!(chosen.is_some(), "Tab reaches the grid");
        cx.simulate_keystrokes("tab");
        assert_eq!(
            cx.update(|window, cx| window.focused(cx)),
            chosen,
            "a second Tab finds no other tile"
        );
        cx.simulate_keystrokes("right");
        let next = cx.update(|window, cx| window.focused(cx));
        assert!(next.is_some() && next != chosen, "→ moves to the next tile");
        cx.simulate_keystrokes("left");
        assert_eq!(
            cx.update(|window, cx| window.focused(cx)),
            chosen,
            "← moves back"
        );
        cx.simulate_keystrokes("down");
        let below = cx.update(|window, cx| window.focused(cx));
        assert!(below.is_some() && below != chosen, "↓ moves a row down");
        cx.simulate_keystrokes("tab");
        assert_eq!(
            cx.update(|window, cx| window.focused(cx)),
            below,
            "the reached tile is the grid's Tab stop"
        );
    }

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
        if let Some(c) = characters::roster_character("Jojo") {
            assert_eq!(default_character("Jojo", &[c]), c);
        }
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

    fn rs(runtime: &str, labels: &[&str], repo: Option<&str>) -> RunnerSetup {
        RunnerSetup {
            runtime: runtime.into(),
            labels: labels.iter().map(|l| l.to_string()).collect(),
            repo: repo.map(str::to_string),
            base_branch: None,
            machine_id: None,
            ..Default::default()
        }
    }

    #[test]
    fn the_computer_choice_reads_as_words() {
        let m = |id: &str, name: &str| MachineInfo {
            id: id.into(),
            name: name.into(),
            labels: Vec::new(),
            connected: true,
            healthy: true,
            last_heartbeat_at: None,
            version: None,
            disk_free_bytes: None,
            disk_total_bytes: None,
            active_runs: 0,
            max_parallel_runs: None,
            runtimes: Vec::new(),
            ..Default::default()
        };
        let list = [m("a", "macbook-pro"), m("b", "mac-mini")];
        assert_eq!(
            machine_choice_text(None, &list, Some("a")),
            "任意一台空闲电脑"
        );
        assert_eq!(
            machine_choice_text(Some("a"), &list, Some("a")),
            "macbook-pro（这台电脑）"
        );
        assert_eq!(machine_choice_text(Some("b"), &list, Some("a")), "mac-mini");
        assert_eq!(
            machine_choice_text(Some("gone"), &list, None),
            "已移除的电脑"
        );
    }

    #[test]
    fn picking_a_computer_is_a_runner_change() {
        let before = rs("claude", &["mac-mini"], None);
        let after = RunnerSetup {
            labels: Vec::new(),
            machine_id: Some("m1".into()),
            ..before.clone()
        };
        let e = bot_edit("Jojo", None, Some(&before), "Jojo", "", Some(&after));
        assert_eq!(e.runner, Some(RunnerChange::Set(after)));
    }

    #[test]
    fn an_unchanged_edit_sends_nothing() {
        let before = rs("main", &["mac"], Some("/srv/app"));
        // Spaces and an empty 岗位 field are no change.
        let after = rs(" main ", &["mac", " "], Some("/srv/app "));
        let edit = bot_edit("跑腿", None, Some(&before), " 跑腿 ", " ", Some(&after));
        assert!(edit.is_empty(), "{edit:?}");
        assert!(bot_edit("助手", Some("测试"), None, "助手", "测试", None).is_empty());
    }

    #[test]
    fn an_edit_sends_only_what_changed() {
        let before = rs("main", &["mac"], None);
        let edit = bot_edit(
            "跑腿",
            Some("测试"),
            Some(&before),
            "跑腿二号",
            "测试",
            Some(&before),
        );
        assert_eq!(
            edit,
            BotMemberEdit {
                name: Some("跑腿二号".into()),
                ..Default::default()
            }
        );
        // Emptying the 岗位 clears it.
        let edit = bot_edit(
            "跑腿",
            Some("测试"),
            Some(&before),
            "跑腿",
            "",
            Some(&before),
        );
        assert_eq!(edit.role, Some(None));
        assert_eq!((edit.name, edit.runner), (None, None));
        // A new label replaces the runner, cleaned.
        let after = rs("main", &["mac", " office "], None);
        let edit = bot_edit("跑腿", None, Some(&before), "跑腿", "", Some(&after));
        assert_eq!(
            edit.runner,
            Some(RunnerChange::Set(rs("main", &["mac", "office"], None)))
        );
    }

    #[test]
    fn switching_the_runner_sets_or_removes_it() {
        let runner = rs("main", &[], None);
        let on = bot_edit("助手", None, None, "助手", "", Some(&runner));
        assert_eq!(on.runner, Some(RunnerChange::Set(runner.clone())));
        let off = bot_edit("助手", None, Some(&runner), "助手", "", None);
        assert_eq!(off.runner, Some(RunnerChange::Remove));
    }
}
