//! 成员 in the main window (Members.dc.html): a filtered list and one
//! member's detail (现在, 这周, 需要批准的事, 能用的工具, 电脑, 最近的问题).
//!
//! Foundation stub: the list and the detail read the provider; the members
//! track builds the artboard (filters, state sentences, settings forms).

pub mod scenes;

use gpui::{
    App, Context, InteractiveElement as _, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div, px,
};

use crate::{
    app::{Place, Services},
    copy::{fill, members_page as c},
    theme::{self, Type as _, space},
    ui::{self, Lucide, MemberAvatar, Ring, RowSurface},
    views::shell::{PageView, TitleBarSpec, panes::watch, section_label, stub_body},
};

pub struct MembersPage {
    services: Services,
    _subscriptions: Vec<Subscription>,
}

impl MembersPage {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        Self {
            _subscriptions: watch(&services, cx),
            services,
        }
    }

    fn selected(&self, cx: &App) -> Option<String> {
        match &self.services.nav.read(cx).place {
            Some(Place::Members(m)) => m.clone(),
            _ => None,
        }
    }
}

impl PageView for MembersPage {
    fn title_bar(&self, _cx: &App) -> TitleBarSpec {
        TitleBarSpec::page(c::MEMBERS_PAGE_TITLE)
    }
}

impl Render for MembersPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let p = self.services.provider.clone();
        let members = p.members();
        let selected = self.selected(cx);
        let working: Vec<String> = p.active_runs().into_iter().map(|r| r.bot_id).collect();
        let rows = members.iter().filter(|m| !m.deactivated).map(|m| {
            let nav = self.services.nav.clone();
            let id = m.id.clone();
            let ring = working.contains(&m.id).then_some(Ring::Live);
            ui::list_row(
                SharedString::from(format!("member-{}", m.id)),
                RowSurface::Content,
                selected.as_deref() == Some(m.id.as_str()),
                cx,
            )
            .h(px(52.))
            .px(space::S12)
            .gap(space::S12)
            .child(MemberAvatar::new(Some(m), px(28.)).ring(ring))
            .child(
                div()
                    .t_ui()
                    .medium()
                    .text_color(k.ink)
                    .child(m.name.clone()),
            )
            .children(m.role.clone().map(ui::JobTag::new))
            .on_click(move |_, _, cx| {
                let id = id.clone();
                nav.update(cx, |nav, cx| nav.open_members(Some(id), cx))
            })
        });
        let detail = match selected.as_deref().and_then(|id| p.member(id)) {
            None => stub_body(Lucide::Users, c::MEMBERS_PICK, None, cx).into_any_element(),
            Some(m) => {
                let stats = p.member_stats(&m.id).unwrap_or_default();
                let list = |label: &'static str, items: Vec<String>| {
                    (!items.is_empty()).then(|| {
                        ui::rail_card(cx)
                            .child(section_label(label, None, cx))
                            .children(
                                items
                                    .into_iter()
                                    .map(|i| div().t_small().text_color(k.ink).child(i)),
                            )
                    })
                };
                let now: Vec<String> = stats
                    .active
                    .iter()
                    .filter_map(|(g, t)| p.topic(g, t).map(|t| t.title))
                    .collect();
                div()
                    .id("member-detail")
                    .size_full()
                    .overflow_y_scroll()
                    .p(space::S24)
                    .flex()
                    .flex_col()
                    .gap(space::S12)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space::S12)
                            .child(MemberAvatar::new(Some(&m), px(48.)))
                            .child(div().t_title().text_color(k.ink).child(m.name.clone()))
                            .children(m.role.clone().map(ui::JobTag::new)),
                    )
                    .children(list(c::MEMBERS_NOW, now))
                    .children(list(
                        c::MEMBERS_THIS_WEEK,
                        vec![format!(
                            "{} · {}",
                            stats.topics_this_week, stats.artifacts_this_week
                        )],
                    ))
                    .children(list(
                        c::MEMBERS_NEEDS_APPROVAL,
                        stats.needs_approval.clone(),
                    ))
                    .children(list(c::MEMBERS_TOOLS, stats.tools.clone()))
                    .children(list(
                        c::MEMBERS_COMPUTER,
                        stats.machine.iter().map(|mc| mc.name.clone()).collect(),
                    ))
                    .children(list(
                        c::MEMBERS_RECENT_PROBLEMS,
                        stats.problems.iter().map(|x| x.text.clone()).collect(),
                    ))
                    .child(div().child(ui::button(
                        "member-hand-over",
                        fill(c::MEMBERS_HAND_OVER, &[("name", &m.name)]),
                        ui::ButtonKind::Primary,
                        ui::ButtonSize::Md,
                        cx,
                    )))
                    .into_any_element()
            }
        };
        div()
            .size_full()
            .flex()
            .child(
                div()
                    .id("members-list")
                    .flex_none()
                    .w(theme::MEMBERS_LIST_WIDTH)
                    .h_full()
                    .overflow_y_scroll()
                    .px(space::S16)
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(section_label(
                        &fill(c::MEMBERS_FILTER_ALL, &[("n", &members.len().to_string())]),
                        None,
                        cx,
                    ))
                    .children(rows),
            )
            .child(ui::vhairline(cx))
            .child(div().flex_1().min_w(px(0.)).h_full().child(detail))
    }
}
