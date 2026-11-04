//! The topic view header (DESIGN.md §8.1): a 92 pt band. Row 1 (52) holds
//! the title (`heading` 18/26) and the `bell`, `panel-right` and `ellipsis`
//! icon buttons; row 2 (40) the breadcrumb `# 群`, the status label and
//! `n 位成员`, split by `·`. Its bottom rule sits level with the tab rule of
//! the topic list and the side panel header.

use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div, px,
};
use gpui_component::menu::DropdownMenu as _;

use super::TimelineView;
use crate::ui::NamedButton as _;
use crate::{
    copy::{fill, realtime as r, timeline as t},
    data::{GroupKind, Topic, TopicStatus},
    theme::{self, Type as _, space},
    ui::{self, Sym},
    views::topic_menu,
};

/// The 私聊 peer's avatar in the breadcrumb: the size of the `hash` glyph's line.
const BREADCRUMB_AVATAR: gpui::Pixels = px(16.);

impl TimelineView {
    pub(super) fn render_header(
        &self,
        topic: &Topic,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = theme::colors(cx);
        let provider = &self.services.provider;
        let following = provider.topic_following(&topic.group_id, &topic.id);
        let panel_open = self.services.nav.read(cx).side_panel_open;
        let group = provider
            .groups()
            .into_iter()
            .find(|g| g.id == topic.group_id);
        let group_name = group.as_ref().map(|g| g.name.clone()).unwrap_or_default();
        // The topic's members, the same list as the side panel's 成员.
        let member_count = crate::views::side_panel::topic_member_ids(
            provider.as_ref(),
            self.me.as_deref(),
            topic,
        )
        .iter()
        .filter(|id| self.members.iter().any(|m| &m.id == *id))
        .count();
        let (title, untitled) = crate::views::sidebar::topic_display_title(&topic.title);

        let row1 = ui::toolbar_band("topic-toolbar")
            .gap(space::S8)
            .child(
                ui::truncating("topic-title", title)
                    .flex_1()
                    .t_heading()
                    .text_color(if untitled {
                        colors.text_2
                    } else {
                        colors.text_1
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(space::S4)
                    .child({
                        let services = self.services.clone();
                        let (g, tid) = (topic.group_id.clone(), topic.id.clone());
                        ui::icon_button(
                            "topic-follow",
                            if following {
                                Sym::BellBadge
                            } else {
                                Sym::Bell
                            },
                        )
                        .named(if following { t::UNFOLLOW } else { t::FOLLOW })
                        .on_click(move |_, _, _| {
                            services.provider.set_topic_following(&g, &tid, !following)
                        })
                    })
                    .child(
                        ui::icon_button(
                            "toggle-panel",
                            if panel_open {
                                Sym::Inspector
                            } else {
                                Sym::Inspector
                            },
                        )
                        .named(t::TOGGLE_PANEL)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.services
                                .nav
                                .update(cx, |nav, cx| nav.toggle_side_panel(cx))
                        })),
                    )
                    .child({
                        let services = self.services.clone();
                        let (g, tid) = (topic.group_id.clone(), topic.id.clone());
                        ui::icon_button("topic-status", Sym::More)
                            .named(r::STATUS_MENU)
                            .dropdown_menu(move |menu, window, cx| {
                                // Read the topic again: the menu shows the newest state.
                                match services.provider.topic(&g, &tid) {
                                    Some(topic) => {
                                        topic_menu::build(menu, &services, &g, &topic, window, cx)
                                    }
                                    None => menu,
                                }
                            })
                    }),
            );

        let dot = || {
            div()
                .flex_none()
                .text_color(colors.text_3)
                .child("·")
                .into_any_element()
        };
        // A 群 shows `# name`; a 私聊 the peer's 16 pt avatar (a person's
        // circle or a bot's tile, as in its sidebar row) and the name.
        let peer = group.as_ref().filter(|g| g.kind == GroupKind::Dm).map(|g| {
            g.dm_peer
                .as_ref()
                .and_then(|p| self.members.iter().find(|m| &m.id == p))
        });
        let lead = match peer {
            Some(peer) => div()
                .flex_none()
                .mr(px(2.))
                .child(ui::MemberAvatar::new(peer, BREADCRUMB_AVATAR).on(colors.content))
                .into_any_element(),
            None => ui::icon(Sym::Topic, theme::icon::CHIP, colors.text_3).into_any_element(),
        };
        let mut items: Vec<AnyElement> = vec![
            div()
                .flex_shrink(1.)
                .min_w(px(0.))
                .flex()
                .items_center()
                .gap(space::S4)
                .child(lead)
                .child(ui::truncating("topic-group-name", group_name))
                .into_any_element(),
        ];
        if let Some(status) = self.header_status(topic, cx) {
            items.push(dot());
            items.push(status);
        }
        items.push(dot());
        items.push(
            div()
                .id("topic-members")
                .flex_none()
                .flex()
                .items_center()
                .gap(space::S4)
                .px(space::S4)
                .mx(-space::S4)
                .h(px(24.))
                .rounded(theme::radius::SM)
                .hover(|el| el.bg(colors.hover))
                .active(|el| el.bg(colors.pressed))
                .child(ui::icon(Sym::Members, theme::icon::CHIP, colors.text_3))
                .child(
                    div()
                        .tnum()
                        .child(fill(t::MEMBER_COUNT, &[("n", &member_count.to_string())])),
                )
                .tooltip(|window, cx| {
                    gpui_component::tooltip::Tooltip::new(t::MEMBERS_TOOLTIP).build(window, cx)
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.services.nav.update(cx, |nav, cx| {
                        nav.set_side_panel_tab(0, cx);
                        if !nav.side_panel_open {
                            nav.toggle_side_panel(cx);
                        }
                    })
                }))
                .into_any_element(),
        );
        let row2 = div()
            .h(theme::HEADER_SECOND_ROW - theme::HAIRLINE)
            .min_w(px(0.))
            .flex()
            .items_center()
            .gap(space::S8)
            .t_small()
            .text_color(colors.text_2)
            .children(items);

        let nav = self.services.nav.clone();
        ui::header_band(cx)
            .id("topic-header")
            .pl(theme::CONTENT_GUTTER)
            .pr(space::S12)
            // While the pointer is here the side panel waits to open by
            // itself, so these buttons never move under it.
            .on_hover(move |hovered, _, cx| {
                nav.update(cx, |nav, _| nav.set_header_hovered(*hovered))
            })
            .child(row1)
            .child(row2)
    }

    /// The status in the header (§7.8): the quiet 等你… label, a 20 pt
    /// avatar + `等回答` / `等审` when the topic waits on someone else (the
    /// name is the tooltip), the info chip for 进行中, labels for the rest.
    /// Hidden while a card is on screen in a topic that waits for my answer:
    /// the ask card already asks (§7.8).
    fn header_status(&self, topic: &Topic, cx: &mut Context<Self>) -> Option<AnyElement> {
        let colors = theme::colors(cx);
        let me = self.me.as_deref();
        let waits_on_me = me.is_some_and(|me| topic.waiting_on.iter().any(|m| m == me));
        let waiting = matches!(
            topic.status,
            TopicStatus::WaitingAnswer | TopicStatus::WaitingReview
        );
        // Only a question waits in a card (the ask card); a draft to review
        // keeps the label (short-topic-*).
        if topic.status == TopicStatus::WaitingAnswer && waits_on_me && self.card_visible {
            return None;
        }
        if waiting && !waits_on_me {
            let who = topic
                .waiting_on
                .first()
                .and_then(|id| self.members.iter().find(|m| &m.id == id))?;
            let (label, full) = if topic.status == TopicStatus::WaitingAnswer {
                (
                    t::WAITING_ANSWER,
                    crate::copy::common::STATUS_WAITING_ANSWER_NAMED,
                )
            } else {
                (
                    t::WAITING_REVIEW,
                    crate::copy::common::STATUS_WAITING_REVIEW_NAMED,
                )
            };
            let tip = SharedString::from(fill(full, &[("name", &who.name)]));
            return Some(
                div()
                    .id("topic-waiting")
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(space::S6)
                    .child(ui::MemberAvatar::new(Some(who), px(20.)))
                    .child(div().text_color(colors.text_2).child(label))
                    .tooltip(move |window, cx| {
                        gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                    })
                    .into_any_element(),
            );
        }
        ui::StatusChip::header_for_topic(topic, me, &self.members)
            .map(|chip| div().flex_none().child(chip).into_any_element())
    }
}
