//! The three lists of the rail (RailOptions.dc.html A, B, C).

use gpui::{
    AnyElement, App, Context, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, div, prelude::FluentBuilder as _, px,
};

use super::{
    MEMBER_AVATAR, RailView,
    model::{self, Bucket, MemberRow, Meta, RowState},
    rows::{NESTED_PAD, SlotStyle},
};
use crate::{
    copy::rail as c,
    data::{GroupKind, Member, MemberId},
    theme::{self, Type as _, space},
    ui::{self, Lucide, MemberAvatar, Ring, RowSurface},
};

type Open<'a> = Option<&'a (String, String)>;

fn is_open(open: Open, g: &str, t: &str) -> bool {
    open.is_some_and(|(og, ot)| og == g && ot == t)
}

impl RailView {
    /// A: 话题 grouped by 群, then 私聊.
    pub(super) fn work(
        &self,
        open: Open,
        group_page: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let p = self.services.provider.clone();
        let me = self.me();
        let k = theme::colors(cx);
        let mut out = Vec::new();
        let groups = p.groups();
        for g in groups.iter().filter(|g| g.kind == GroupKind::Group) {
            let topics: Vec<_> = p
                .topics(&g.id)
                .into_iter()
                .filter(model::listed)
                .filter(|t| {
                    self.show_all
                        || model::involves_me(p.as_ref(), me.as_deref(), t)
                        || is_open(open, &t.group_id, &t.id)
                })
                .collect();
            if topics.is_empty() && !self.show_all {
                continue;
            }
            let folded = self.folded_groups.contains(&g.id);
            let mut section: Vec<AnyElement> = Vec::new();
            let fold_id = g.id.clone();
            let this = cx.entity().downgrade();
            let nav = self.services.nav.clone();
            let open_id = g.id.clone();
            section.push(self.section_header(
                SharedString::from(format!("rail-group-{}", g.id)),
                g.name.clone(),
                None,
                false,
                folded,
                group_page == Some(g.id.as_str()),
                move |_, _, cx| {
                    let id = fold_id.clone();
                    let _ = this.update(cx, |this, cx| {
                        if !this.folded_groups.remove(&id) {
                            this.folded_groups.insert(id);
                        }
                        cx.notify();
                    });
                },
                Some(Box::new(move |_, _, cx| {
                    let g = open_id.clone();
                    nav.update(cx, |nav, cx| nav.open_group(g, cx))
                })),
                cx,
            ));
            if !folded {
                for t in &topics {
                    let (state, meta) = model::row_state(p.as_ref(), me.as_deref(), t);
                    section.push(self.topic_row(
                        "topic",
                        t,
                        &state,
                        meta,
                        false,
                        is_open(open, &g.id, &t.id),
                        SlotStyle::Tile,
                        space::S10,
                        cx,
                    ));
                }
            }
            out.push(column(section));
        }
        let dms: Vec<_> = groups.iter().filter(|g| g.kind == GroupKind::Dm).collect();
        if !dms.is_empty() {
            let mut section: Vec<AnyElement> = Vec::new();
            let folded = self.folded_groups.contains("rail-dms");
            let this = cx.entity().downgrade();
            section.push(self.section_header(
                "rail-dms",
                c::RAIL_DMS.into(),
                None,
                false,
                folded,
                false,
                move |_, _, cx| {
                    let _ = this.update(cx, |this, cx| {
                        if !this.folded_groups.remove("rail-dms") {
                            this.folded_groups.insert("rail-dms".into());
                        }
                        cx.notify();
                    });
                },
                None,
                cx,
            ));
            if !folded {
                for g in dms {
                    let peer = g.dm_peer.as_deref().and_then(|m| p.member(m));
                    let nav = self.services.nav.clone();
                    let id = g.id.clone();
                    let selected = group_page == Some(g.id.as_str())
                        || open.is_some_and(|(og, _)| og == &g.id);
                    section.push(
                        ui::list_row(
                            SharedString::from(format!("rail-dm-{}", g.id)),
                            RowSurface::Rail,
                            selected,
                            cx,
                        )
                        .h(theme::RAIL_ROW)
                        .px(space::S10)
                        .gap(space::S8)
                        .child(MemberAvatar::new(peer.as_ref(), theme::RAIL_SLOT))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .one_line()
                                .t_small()
                                .text_color(if selected { k.ink } else { k.ink_2 })
                                .when(selected, |el| el.medium())
                                .child(g.name.clone()),
                        )
                        .on_click(move |_, _, cx| {
                            let id = id.clone();
                            nav.update(cx, |nav, cx| nav.open_group(id, cx))
                        })
                        .into_any_element(),
                    );
                }
            }
            out.push(column(section));
        }
        out
    }

    /// B: 需要你 / 在做 / 等别人 / 做完了 · 今天, of the chosen 群.
    pub(super) fn attention(&self, open: Open, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = self.services.provider.clone();
        let me = self.me();
        let names: Vec<(String, String)> = model::groups(p.as_ref());
        let mut out = Vec::new();
        for (bucket, topics) in model::buckets(p.as_ref(), me.as_deref(), self.chip.as_deref()) {
            if topics.is_empty() {
                continue;
            }
            let folded = self.folded_buckets.contains(&bucket);
            let this = cx.entity().downgrade();
            let mut section: Vec<AnyElement> = vec![self.section_header(
                SharedString::from(format!("rail-bucket-{bucket:?}")),
                bucket.label().into(),
                Some(topics.len()),
                bucket == Bucket::NeedsYou,
                folded,
                false,
                move |_, _, cx| {
                    let _ = this.update(cx, |this, cx| {
                        if !this.folded_buckets.remove(&bucket) {
                            this.folded_buckets.insert(bucket);
                        }
                        cx.notify();
                    });
                },
                None,
                cx,
            )];
            if !folded {
                for t in &topics {
                    let (state, mut meta) = model::row_state(p.as_ref(), me.as_deref(), t);
                    // Needing me, the row names its 群 (unless one 群 is chosen).
                    if bucket == Bucket::NeedsYou && self.chip.is_none() {
                        meta = names
                            .iter()
                            .find(|(id, _)| *id == t.group_id)
                            .map(|(_, name)| Meta {
                                text: name.clone(),
                                mono: false,
                            });
                    }
                    section.push(self.topic_row(
                        "bucket",
                        t,
                        &state,
                        meta,
                        false,
                        is_open(open, &t.group_id, &t.id),
                        SlotStyle::Tile,
                        space::S10,
                        cx,
                    ));
                }
            }
            out.push(column(section));
        }
        out
    }

    /// C: members (busy first), each expandable to its 话题; idle members
    /// in one row; 同事 below.
    pub(super) fn by_member(&mut self, open: Open, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = self.services.provider.clone();
        let me = self.me();
        let k = theme::colors(cx);
        let rows = model::member_rows(p.as_ref(), me.as_deref(), self.member_sort);
        let opened = self
            .open_members
            .get_or_insert_with(|| {
                rows.iter()
                    .find(|r| r.busy())
                    .map(|r| r.member.id.clone())
                    .into_iter()
                    .collect()
            })
            .clone();
        let (bots, people): (Vec<&MemberRow>, Vec<&MemberRow>) =
            rows.iter().partition(|r| r.member.is_bot());
        let mut out = Vec::new();
        let mut index = 0;
        let mut idle: Vec<Member> = Vec::new();
        for r in bots {
            if r.busy() || self.member_sort == model::MemberSort::Roster {
                out.push(self.member_block(index, r, opened.contains(&r.member.id), open, cx));
                index += 1;
            } else {
                idle.push(r.member.clone());
            }
        }
        if !idle.is_empty() {
            out.push(self.grouped_row("rail-idle", c::RAIL_IDLE, &idle, false, cx));
        }
        if !people.is_empty() {
            out.push(
                div()
                    .flex_none()
                    .h(theme::RAIL_ROW)
                    .px(space::S10)
                    .flex()
                    .items_center()
                    .t_meta_medium()
                    .text_color(k.ink_3)
                    .child(c::RAIL_PEOPLE)
                    .into_any_element(),
            );
            let mut away: Vec<Member> = Vec::new();
            for r in people {
                if r.busy() {
                    out.push(self.member_block(index, r, opened.contains(&r.member.id), open, cx));
                    index += 1;
                } else if r.member.online {
                    out.push(self.plain_member(&r.member, cx));
                } else {
                    away.push(r.member.clone());
                }
            }
            if !away.is_empty() {
                out.push(self.grouped_row("rail-away", c::RAIL_AWAY, &away, true, cx));
            }
        }
        out
    }

    /// One member row of C and, when open, its 话题.
    fn member_block(
        &self,
        index: usize,
        r: &MemberRow,
        expanded: bool,
        open: Open,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = theme::colors(cx);
        let id = r.member.id.clone();
        let this = cx.entity().downgrade();
        let ring = if r.working {
            Some(Ring::Live)
        } else if r.waiting_on_me > 0 {
            Some(Ring::You)
        } else {
            None
        };
        let trailing: Option<AnyElement> = if r.waiting_on_me > 0 {
            Some(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(space::S6)
                    .t_meta()
                    .text_color(k.you_ink)
                    .child(div().size(px(6.)).rounded_full().bg(k.you))
                    .child(c::RAIL_WAITS_ON_YOU)
                    .when(r.waiting_on_me > 1, |el| {
                        el.child(div().mono().child(r.waiting_on_me.to_string()))
                    })
                    .into_any_element(),
            )
        } else {
            r.plan.clone().map(|plan| {
                div()
                    .flex_none()
                    .t_mono()
                    .text_color(k.ink_3)
                    .child(plan)
                    .into_any_element()
            })
        };
        let can_open = r.busy();
        let head = ui::list_row(
            SharedString::from(format!("rail-member-{}", r.member.id)),
            RowSurface::Rail,
            false,
            cx,
        )
        .h(theme::RAIL_ROW)
        .pl(space::S6)
        .pr(space::S10)
        .gap(space::S8)
        .child(div().flex_none().w(px(12.)).flex().when(can_open, |el| {
            el.child(ui::icon(
                if expanded {
                    Lucide::ChevronDown
                } else {
                    Lucide::ChevronRight
                },
                px(12.),
                k.ink_3,
            ))
        }))
        .child(MemberAvatar::new(Some(&r.member), MEMBER_AVATAR).ring(ring))
        .child(
            div()
                .flex_shrink(1.)
                .min_w_0()
                .max_w(px(140.))
                .one_line()
                .t_small()
                .medium()
                .text_color(k.ink)
                .child(r.member.name.clone()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .one_line()
                .t_meta()
                .text_color(k.ink_3)
                .children(r.member.role.clone()),
        )
        .children(trailing)
        .on_click(move |_, _, cx| {
            if !can_open {
                return;
            }
            let id: MemberId = id.clone();
            let _ = this.update(cx, |this, cx| {
                let set = this.open_members.get_or_insert_with(Default::default);
                if !set.remove(&id) {
                    set.insert(id);
                }
                cx.notify();
            });
        });
        let mut children: Vec<AnyElement> = vec![head.into_any_element()];
        if expanded {
            for (i, mt) in r.topics.iter().enumerate() {
                let meta_you = mt.state == RowState::NeedsYou;
                let row = self.topic_row(
                    &format!("member-{}", r.member.id),
                    &mt.topic,
                    &mt.state,
                    mt.meta.clone(),
                    meta_you,
                    is_open(open, &mt.topic.group_id, &mt.topic.id),
                    SlotStyle::Dot,
                    NESTED_PAD,
                    cx,
                );
                children.push(ui::motion::rise(
                    SharedString::from(format!("rail-member-{}-{}", r.member.id, mt.topic.id)),
                    i,
                    div().child(row),
                    cx,
                ));
            }
        }
        let _ = index;
        column_tight(children)
    }

    /// An online person with nothing on: avatar, name, 岗位.
    fn plain_member(&self, m: &Member, cx: &App) -> AnyElement {
        let k = theme::colors(cx);
        let nav = self.services.nav.clone();
        let id = m.id.clone();
        ui::list_row(
            SharedString::from(format!("rail-person-{}", m.id)),
            RowSurface::Rail,
            false,
            cx,
        )
        .h(theme::RAIL_ROW)
        .pl(px(26.))
        .pr(space::S10)
        .gap(space::S8)
        .child(MemberAvatar::new(Some(m), MEMBER_AVATAR))
        .child(
            div()
                .flex_shrink(1.)
                .min_w_0()
                .max_w(px(140.))
                .one_line()
                .t_small()
                .medium()
                .text_color(k.ink)
                .child(m.name.clone()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .one_line()
                .t_meta()
                .text_color(k.ink_3)
                .children(m.role.clone()),
        )
        .on_click(move |_, _, cx| {
            let id = id.clone();
            nav.update(cx, |nav, cx| nav.open_members(Some(id), cx))
        })
        .into_any_element()
    }

    /// `空闲 · Ledger、Scout、Pocket` and `不在 · Alex Chen、陈晨`: small
    /// avatars, the word, the names.
    fn grouped_row(
        &self,
        id: &'static str,
        word: &'static str,
        members: &[Member],
        dim: bool,
        cx: &App,
    ) -> AnyElement {
        let k = theme::colors(cx);
        let nav = self.services.nav.clone();
        let names = members
            .iter()
            .map(|m| m.name.as_str())
            .collect::<Vec<_>>()
            .join(c::RAIL_NAMES_SEP);
        let size = if dim { MEMBER_AVATAR } else { theme::RAIL_SLOT };
        ui::list_row(id, RowSurface::Rail, false, cx)
            .h(theme::RAIL_ROW)
            .pl(px(26.))
            .pr(space::S10)
            .gap(space::S8)
            .child(
                div()
                    .flex_none()
                    .flex()
                    .gap(px(3.))
                    .when(dim, |el| el.opacity(0.5))
                    .children(
                        members
                            .iter()
                            .take(3)
                            .map(|m| MemberAvatar::new(Some(m), size)),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .one_line()
                    .t_small()
                    .text_color(k.ink_3)
                    .child(
                        gpui::StyledText::new(format!("{word} · {names}")).with_highlights([(
                            0..word.len(),
                            gpui::HighlightStyle {
                                color: Some(k.ink_2),
                                ..Default::default()
                            },
                        )]),
                    ),
            )
            .on_click(move |_, _, cx| nav.update(cx, |nav, cx| nav.open_members(None, cx)))
            .into_any_element()
    }
}

/// A section: its rows 1 pt apart.
fn column(children: Vec<AnyElement>) -> AnyElement {
    div()
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(1.))
        .children(children)
        .into_any_element()
}

/// A member block of C: rows with no gap.
fn column_tight(children: Vec<AnyElement>) -> AnyElement {
    div()
        .flex_none()
        .flex()
        .flex_col()
        .children(children)
        .into_any_element()
}
