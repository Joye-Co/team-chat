//! 团队管理 › 机器: every computer of the team as a card (state, what runs, CPU and memory, the AI
//! members at work there), a dashed card that adds one, and how runs are given to machines.
//! The numbers come from each machine's monitor (`GET /v1/machines/:id/monitor`).

use std::collections::HashMap;

use gpui::{
    AnyElement, ClipboardItem, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, relative,
};
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};

use chat_core::proto::MachineMonitor;

use super::{AdminPage, AdminView, model};
use crate::{
    copy::{admin as m, fill},
    data::MachineInfo,
    ui::{
        MemberAvatar, Sym,
        icons::sym as sym_icon,
        native::{
            Alert, AlertTone, BadgeState, NativeType as _, PushSize, PushStyle, StatusBadge,
            TruncText, card_surface, native_colors, push_button,
            tokens::{avatar, height, monitor as t, radius, space},
        },
        sf_symbols::SymSize,
    },
    views::settings::widgets::answer,
};

/// The monitors of every machine, for the cards.
#[derive(Default)]
pub struct Fleet {
    pub monitors: HashMap<String, MachineMonitor>,
    /// 用这台电脑 is setting this Mac up; then its answer.
    pub using: bool,
    /// Why the last 用这台电脑 failed.
    pub using_note: Option<String>,
    /// What this computer still lacks (用这台电脑).
    pub this: super::this_computer::ThisComputer,
    /// Why the last change from a card's "⋯" failed.
    pub action_failed: Option<String>,
}

impl AdminView {
    /// Reads every machine's monitor (on 打开 机器 and with every refresh while it shows).
    pub(super) fn fetch_fleet(&mut self, cx: &mut Context<Self>) {
        // Other people's own computers have no monitor for me (design-roles 3.5).
        let me = crate::views::settings::my_id(&self.services);
        let ids: Vec<String> = self
            .machines
            .iter()
            .flatten()
            .filter(|x| x.ownership(me.as_deref()) != crate::data::Ownership::Theirs)
            .map(|x| x.id.clone())
            .collect();
        for id in ids {
            let reply = self.services.provider.machine_monitor(
                &id,
                model::Range::M15.wire(),
                super::day_start(),
            );
            cx.spawn(async move |this, cx| {
                if let Ok(d) = answer(reply).await {
                    let _ = this.update(cx, |this, cx| {
                        this.fleet.monitors.insert(id, d);
                        cx.notify();
                    });
                }
            })
            .detach();
        }
        self.look_on_this_computer(cx);
    }

    /// 用这台电脑: sets this Mac up with the app's own daemon (as Settings › 机器 did).
    pub(super) fn use_this_mac(&mut self, cx: &mut Context<Self>) {
        if self.fleet.using {
            return;
        }
        self.fleet.using = true;
        self.fleet.using_note = None;
        let reply = self.services.provider.use_this_mac();
        cx.spawn(async move |this, cx| {
            let result = answer(reply).await;
            let _ = this.update(cx, |this, cx| {
                this.fleet.using = false;
                // Whether it can work is what the three lines say, not that the service is in.
                let t = &mut this.fleet.this;
                t.set_up_here = true;
                t.failed = result.is_err();
                t.local = None;
                this.fleet.using_note = result
                    .err()
                    .map(|e| crate::views::settings::widgets::error_text(e).to_string());
                this.retry(cx);
                this.look_on_this_computer(cx);
            });
        })
        .detach();
        cx.notify();
    }

    /// What a change from a card's "⋯" does next: reload, or say why it failed.
    fn fleet_after(
        &self,
        cx: &mut Context<Self>,
    ) -> crate::views::settings::machine_actions::After {
        let view = cx.entity().downgrade();
        std::rc::Rc::new(move |result, cx| {
            let _ = view.update(cx, |v, cx| {
                v.fleet.action_failed = result
                    .err()
                    .map(|e| crate::views::settings::widgets::error_text(e).to_string());
                v.retry(cx);
                cx.notify();
            });
        })
    }

    fn open_machine(&mut self, id: String, cx: &mut Context<Self>) {
        self.select_machine(id, cx);
        self.show_page(AdminPage::Overview, cx);
    }

    pub(super) fn machines_page(
        &self,
        now: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = native_colors(cx);
        // The team's computers and mine; other people's own computers are theirs to manage
        // (they show in 设置 › 电脑).
        let me = crate::views::settings::my_id(&self.services);
        let cards: Vec<AnyElement> = self
            .machines
            .iter()
            .flatten()
            .filter(|x| x.ownership(me.as_deref()) != crate::data::Ownership::Theirs)
            .map(|x| self.machine_card(x, now, window, cx))
            .collect();
        let services = self.services.clone();
        let view = cx.entity();
        let add = div()
            .id("fleet-add")
            .flex_1()
            .min_w(t::FLEET_CARD_MIN)
            .p(space::S24)
            .rounded(radius::CARD)
            .border_1()
            .border_dashed()
            .border_color(k.separator)
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(space::S8)
            .child(
                div()
                    .size(height::LG)
                    .rounded(radius::CONTROL)
                    .bg(k.fill)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(sym_icon(Sym::Plus, SymSize::INLINE, k.secondary)),
            )
            .child(div().n_headline().child(m::FLEET_ADD))
            .child(
                div()
                    .max_w(t::FLEET_ADD_TEXT)
                    .text_center()
                    .n_footnote()
                    .text_color(k.secondary)
                    .child(m::FLEET_ADD_DESC),
            )
            .on_click(move |_, window, cx| {
                let form = crate::views::settings::machines::open_add_machine(
                    services.clone(),
                    window,
                    cx,
                );
                let view = view.clone();
                cx.observe(&form, move |form, cx| {
                    if form.read(cx).created().is_some() {
                        view.update(cx, |v, cx| v.retry(cx));
                    }
                })
                .detach();
            });
        let mut grid: Vec<AnyElement> = cards;
        grid.push(add.into_any_element());
        // Two cards a row.
        let mut rows: Vec<AnyElement> = vec![];
        let mut it = grid.into_iter();
        while let Some(a) = it.next() {
            let b = it.next();
            rows.push(
                div()
                    .w_full()
                    .flex()
                    .gap(space::S16)
                    .child(div().flex_1().min_w(gpui::Pixels::ZERO).flex().child(a))
                    .child(div().flex_1().min_w(gpui::Pixels::ZERO).flex().children(b))
                    .into_any_element(),
            );
        }
        use crate::data::ThisMac;
        let this_mac = match self.services.provider.this_mac() {
            ThisMac::NotSetUp => Some(crate::copy::settings::THIS_MAC_USE),
            ThisMac::Elsewhere => Some(crate::copy::settings::THIS_MAC_SWITCH),
            ThisMac::Unavailable | ThisMac::Working => None,
        };
        let view = cx.entity();
        // After a failed try the three lines below say what is wrong and offer 再试一次.
        let this_mac =
            this_mac.filter(|_| !self.fleet.this.failed && self.fleet.this.pinned.is_none());
        let use_row = this_mac.is_some().then(|| {
            div()
                .flex()
                .items_center()
                .gap(space::S12)
                .child(div().flex_1())
                .children(this_mac.map(|label| {
                    crate::ui::native::loading_push(
                        push_button(
                            "fleet-use-this-mac",
                            label,
                            PushStyle::Outline,
                            PushSize::Small,
                            cx,
                        ),
                        self.fleet.using,
                        PushSize::Small,
                    )
                    .on_click(move |_, window, cx| {
                        // 用这台电脑 says what it means first (机 3.1); moving an existing
                        // set-up onto the app's daemon changes nothing it does.
                        let view = view.clone();
                        if label == crate::copy::settings::THIS_MAC_USE {
                            let alone = crate::views::settings::solo(&view.read(cx).services);
                            crate::views::settings::machine_actions::open_use_this(
                                alone,
                                move |cx| view.update(cx, |v, cx| v.use_this_mac(cx)),
                                window,
                                cx,
                            );
                        } else {
                            view.update(cx, |v, cx| v.use_this_mac(cx))
                        }
                    })
                }))
        });
        let action_failed = self.fleet.action_failed.clone().map(|why| {
            Alert::new(Sym::Warning, crate::copy::settings::MACHINE_ACTION_FAILED)
                .tone(AlertTone::Warning)
                .description(why)
        });
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(space::S16)
            .children(use_row)
            .children(action_failed)
            .children(self.this_computer_card(cx))
            .children(rows)
            .child(
                Alert::new(Sym::Info, m::FLEET_NOTE_TITLE)
                    .tone(AlertTone::Neutral)
                    .description(m::FLEET_NOTE_DESC),
            )
            .into_any_element()
    }

    pub(super) fn machine_card(
        &self,
        x: &MachineInfo,
        now: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = native_colors(cx);
        let d = self.fleet.monitors.get(&x.id);
        let os = d.and_then(|d| d.machine.os.clone());
        let vm = d.is_some_and(|d| {
            d.capacity.slots > 0 && d.host.is_some() && d.runs.iter().any(|r| r.boot_ms.is_some())
        }) || x.labels.iter().any(|l| l == "vm");
        let kind = if vm {
            m::FLEET_KIND_VM
        } else {
            m::FLEET_KIND_LOCAL
        };
        let meta = match os.as_deref() {
            Some("linux") => format!("Linux · {kind}"),
            Some("macos") => format!("macOS · {kind}"),
            Some(o) => format!("{o} · {kind}"),
            None => kind.to_string(),
        };
        // Whose it is, once more than one person uses the server (design-roles 3.9).
        let me = crate::views::settings::my_id(&self.services);
        let meta = if crate::views::settings::solo(&self.services) {
            meta
        } else {
            let whose = match x.ownership(me.as_deref()) {
                crate::data::Ownership::Mine => crate::copy::settings::MACHINE_MINE,
                _ => crate::copy::settings::MACHINE_TEAM,
            };
            format!("{whose} · {meta}")
        };
        let (badge, word) = if x.connected {
            (BadgeState::Online, m::MACHINE_ONLINE.to_string())
        } else {
            let since = x
                .last_heartbeat_at
                .map(|t| model::duration(now.saturating_sub(t.max(0) as u64)))
                .map(|d| fill(m::FLEET_OFFLINE_FOR, &[("d", &d)]))
                .unwrap_or_else(|| m::MACHINE_OFFLINE.to_string());
            (BadgeState::Offline, since)
        };
        let running = d.map(|d| d.capacity.running).unwrap_or(x.active_runs);
        let slots = d
            .map(|d| d.capacity.running + d.capacity.can_take)
            .filter(|n| *n > 0)
            .or(x.max_parallel_runs)
            .unwrap_or(0);
        let cpu = d.and_then(|d| d.host.as_ref()?.cpu_pct);
        let mem = d.and_then(|d| {
            let h = d.host.as_ref()?;
            Some(
                model::share(
                    h.mem_total_bytes?.saturating_sub(h.mem_available_bytes?),
                    h.mem_total_bytes?,
                ) as f64
                    * 100.,
            )
        });
        let members: Vec<String> = d
            .map(|d| {
                d.runs
                    .iter()
                    .filter_map(|r| r.member_id.clone())
                    .chain(d.members.iter().map(|m| m.member_id.clone()))
                    .fold(vec![], |mut acc: Vec<String>, id| {
                        if !acc.contains(&id) {
                            acc.push(id);
                        }
                        acc
                    })
            })
            .unwrap_or_default();
        let avatars: Vec<AnyElement> = members
            .iter()
            .take(5)
            .filter_map(|id| self.services.provider.member(id))
            .map(|mm| MemberAvatar::new(Some(&mm), avatar::XS).into_any_element())
            .collect();
        let footer = if !x.connected {
            x.last_heartbeat_at
                .map(|t| {
                    fill(
                        m::FLEET_LAST_SEEN,
                        &[("when", &super::clock_day(t.max(0) as u64))],
                    )
                })
                .unwrap_or_else(|| m::HOST_NEVER.to_string())
        } else if members.is_empty() {
            m::FLEET_NOBODY.to_string()
        } else {
            fill(m::FLEET_MEMBERS, &[("n", &members.len().to_string())])
        };
        let meter = |label: &'static str, value: String, share: Option<f32>| {
            div()
                .flex_1()
                .min_w(gpui::Pixels::ZERO)
                .flex()
                .flex_col()
                .gap(space::S6)
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .n_footnote()
                        .child(div().text_color(k.secondary).child(label))
                        .child(
                            div()
                                .n_figures()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .child(value),
                        ),
                )
                .child(
                    div()
                        .h(t::MEMBER_BAR)
                        .rounded_full()
                        .bg(k.fill)
                        .overflow_hidden()
                        .children(share.map(|s| {
                            div()
                                .h_full()
                                .rounded_full()
                                .bg(k.label)
                                .w(relative(s.clamp(0., 1.)))
                        })),
                )
        };
        let pct = |v: Option<f64>| {
            v.map(|v| format!("{}%", v.round() as i64))
                .unwrap_or_else(|| "—".into())
        };
        let id = x.id.clone();
        let view = cx.entity();
        let name = x.name.clone();
        let this = self.services.provider.this_machine_id().as_deref() == Some(x.id.as_str());
        let may =
            crate::views::settings::machine_actions::allowed(x, me.as_deref(), self.owner, this);
        let (services, machine, after) = (self.services.clone(), x.clone(), self.fleet_after(cx));
        let more = crate::views::settings::widgets::more_button(
            SharedString::from(format!("fleet-more-{}", x.id)),
            window,
            cx,
        )
        .dropdown_menu(move |menu, _, _| {
            let name = name.clone();
            let menu = menu.item(PopupMenuItem::new(m::FLEET_COPY_NAME).on_click(
                move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(name.clone())),
            ));
            crate::views::settings::machine_actions::menu_items(
                menu, &services, &machine, may, &after, &after,
            )
        });
        let open = push_button(
            SharedString::from(format!("fleet-open-{}", x.id)),
            m::FLEET_OPEN,
            PushStyle::Outline,
            PushSize::Small,
            cx,
        )
        .on_click(move |_, _, cx| {
            let id = id.clone();
            view.update(cx, |v, cx| v.open_machine(id, cx))
        });
        card_surface(cx)
            .flex_1()
            .min_w(t::FLEET_CARD_MIN)
            .p(space::S24)
            .flex()
            .flex_col()
            .gap(space::S16)
            .when(!x.connected, |d| d.opacity(0.75))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S12)
                    .child(
                        div()
                            .flex_none()
                            .size(height::LG)
                            .rounded(radius::CONTROL)
                            .bg(if x.connected { k.primary } else { k.fill })
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(sym_icon(
                                if vm { Sym::Server } else { Sym::Laptop },
                                SymSize::INLINE,
                                if x.connected {
                                    k.on_primary
                                } else {
                                    k.secondary
                                },
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(gpui::Pixels::ZERO)
                            .flex()
                            .flex_col()
                            .child(
                                TruncText::new(x.name.clone())
                                    .n_title3()
                                    .font_weight(gpui::FontWeight::SEMIBOLD),
                            )
                            .child(TruncText::new(meta).n_footnote().text_color(k.secondary)),
                    )
                    .child(StatusBadge::new(badge, word))
                    .child(more),
            )
            .child(
                div()
                    .flex()
                    .gap(space::S16)
                    .child(meter(
                        m::FLEET_RUNNING,
                        format!("{running} / {slots}"),
                        (slots > 0).then(|| model::share(running as u64, slots as u64)),
                    ))
                    .child(meter(m::KPI_CPU, pct(cpu), cpu.map(|v| (v / 100.) as f32)))
                    .child(meter(m::KPI_MEM, pct(mem), mem.map(|v| (v / 100.) as f32))),
            )
            .child(
                div()
                    .pt(space::S16)
                    .border_t_1()
                    .border_color(k.separator)
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .child(div().flex().gap(space::S2).children(avatars))
                    .child(
                        TruncText::new(footer)
                            .flex_1()
                            .n_footnote()
                            .text_color(k.secondary),
                    )
                    .child(open),
            )
            .into_any_element()
    }
}
