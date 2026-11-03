//! The three lists of the rail (RailOptions.dc.html A, B, C).
//!
//! The lists are read from the provider into flat [`Entry`] rows once per
//! change ([`RailView`] keeps them), and drawn from memory by a virtual list:
//! a breathing ring renders the rail again on every frame it asks for, so
//! the frame path must not read the cache.

use gpui::{
    AnyElement, App, Context, IntoElement, ParentElement, Pixels, SharedString,
    StatefulInteractiveElement as _, Styled, div, prelude::FluentBuilder as _, px,
};

use super::{
    MEMBER_AVATAR, RailView,
    model::{self, Bucket, MemberRow, Meta, RowState, SideChats},
    rows::{NESTED_PAD, SlotStyle},
};
use crate::{
    app::RailMode,
    copy::rail as c,
    data::{ChatProvider, GroupId, GroupKind, Member, MemberId, Topic},
    theme::{self, Type as _, space},
    ui::{self, Lucide, MemberAvatar, Ring, RowSurface},
};

type Open<'a> = Option<&'a (String, String)>;

fn is_open(open: Open, g: &str, t: &str) -> bool {
    open.is_some_and(|(og, ot)| og == g && ot == t)
}

/// One row of a rail list, with the space above it.
pub(super) struct Entry {
    pub gap: Pixels,
    pub item: Item,
}

/// What a row of a rail list shows.
pub(super) enum Item {
    /// A: `⌄ 开发`, which opens the 群 page.
    GroupHeader {
        id: GroupId,
        name: String,
        folded: bool,
        selected: bool,
    },
    /// A: `⌄ 私聊`.
    DmHeader {
        folded: bool,
    },
    /// A: a DM.
    Dm {
        id: GroupId,
        name: String,
        peer: Option<Member>,
        selected: bool,
    },
    /// B: `⌄ • 需要你 4`.
    BucketHeader {
        bucket: Bucket,
        count: usize,
        folded: bool,
    },
    Topic(TopicItem),
    /// C: a member and its state.
    MemberHead {
        row: MemberRow,
        expanded: bool,
    },
    /// C: `空闲 · …` and `不在 · …`.
    Grouped {
        id: &'static str,
        word: &'static str,
        members: Vec<Member>,
        dim: bool,
    },
    /// C: `同事`.
    PeopleLabel,
    /// C: an online person with nothing on.
    Plain(Member),
}

/// A 话题 row.
pub(super) struct TopicItem {
    pub key: String,
    pub topic: Topic,
    pub state: RowState,
    pub meta: Option<Meta>,
    pub meta_you: bool,
    pub selected: bool,
    pub style: SlotStyle,
    pub indent: Pixels,
    /// The doing member, for its tile in the slot.
    pub tile: Option<Member>,
    /// C: the first-paint rise of a member's 话题 (its place in the stagger).
    pub rise: Option<usize>,
}

/// What the lists depend on besides the provider.
#[derive(Clone, PartialEq)]
pub(super) struct Inputs {
    pub mode: RailMode,
    pub me: Option<MemberId>,
    pub open: Option<(String, String)>,
    pub group_page: Option<GroupId>,
    pub show_all: bool,
    pub chip: Option<GroupId>,
    pub member_sort: model::MemberSort,
    pub folded_groups: std::collections::HashSet<GroupId>,
    pub folded_buckets: std::collections::HashSet<Bucket>,
    pub open_members: Option<std::collections::HashSet<MemberId>>,
}

/// A section's first row sits [`space::S6`] below the last one; rows of one
/// section 1 pt apart.
fn sections(list: Vec<Vec<Item>>) -> Vec<Entry> {
    let mut out = Vec::new();
    for (si, section) in list.into_iter().enumerate() {
        for (i, item) in section.into_iter().enumerate() {
            let gap = match (si, i) {
                (0, 0) => px(0.),
                (_, 0) => space::S6,
                _ => px(1.),
            };
            out.push(Entry { gap, item });
        }
    }
    out
}

fn tile(p: &dyn ChatProvider, state: &RowState) -> Option<Member> {
    match state {
        RowState::Working(m) => p.member(m),
        _ => None,
    }
}

/// A: 话题 grouped by 群, then 私聊.
pub(super) fn work(p: &dyn ChatProvider, i: &Inputs) -> Vec<Entry> {
    let me = i.me.as_deref();
    let open = i.open.as_ref();
    let mut out: Vec<Vec<Item>> = Vec::new();
    let groups = p.groups();
    for g in groups.iter().filter(|g| g.kind == GroupKind::Group) {
        let topics: Vec<_> = p
            .topics(&g.id)
            .into_iter()
            .filter(model::listed)
            .filter(|t| {
                i.show_all || model::involves_me(p, me, t) || is_open(open, &t.group_id, &t.id)
            })
            .collect();
        if topics.is_empty() && !i.show_all {
            continue;
        }
        let folded = i.folded_groups.contains(&g.id);
        let mut section = vec![Item::GroupHeader {
            id: g.id.clone(),
            name: g.name.clone(),
            folded,
            selected: i.group_page.as_deref() == Some(g.id.as_str()),
        }];
        if !folded {
            for t in topics {
                let (state, meta) = model::row_state(p, me, &t);
                section.push(Item::Topic(TopicItem {
                    key: "topic".into(),
                    selected: is_open(open, &g.id, &t.id),
                    tile: tile(p, &state),
                    topic: t,
                    state,
                    meta,
                    meta_you: false,
                    style: SlotStyle::Tile,
                    indent: space::S10,
                    rise: None,
                }));
            }
        }
        out.push(section);
    }
    let dms: Vec<_> = groups.iter().filter(|g| g.kind == GroupKind::Dm).collect();
    if !dms.is_empty() {
        let folded = i.folded_groups.contains("rail-dms");
        let mut section = vec![Item::DmHeader { folded }];
        if !folded {
            for g in dms {
                section.push(Item::Dm {
                    id: g.id.clone(),
                    name: g.name.clone(),
                    peer: g.dm_peer.as_deref().and_then(|m| p.member(m)),
                    selected: i.group_page.as_deref() == Some(g.id.as_str())
                        || open.is_some_and(|(og, _)| og == &g.id),
                });
            }
        }
        out.push(section);
    }
    sections(out)
}

/// B: 需要你 / 在做 / 等别人 / 做完了 · 今天, of the chosen 群.
pub(super) fn attention(p: &dyn ChatProvider, i: &Inputs, side: &SideChats) -> Vec<Entry> {
    let me = i.me.as_deref();
    let names = model::groups(p);
    let mut out: Vec<Vec<Item>> = Vec::new();
    for (bucket, topics) in model::buckets(p, me, i.chip.as_deref(), side) {
        if topics.is_empty() {
            continue;
        }
        let folded = i.folded_buckets.contains(&bucket);
        let mut section = vec![Item::BucketHeader {
            bucket,
            count: topics.len(),
            folded,
        }];
        if !folded {
            for t in topics {
                let (state, mut meta) = model::row_state(p, me, &t);
                // Needing me, the row names its 群 (unless one 群 is chosen).
                if bucket == Bucket::NeedsYou && i.chip.is_none() {
                    meta = names
                        .iter()
                        .find(|(id, _)| *id == t.group_id)
                        .map(|(_, name)| Meta {
                            text: name.clone(),
                            mono: false,
                        });
                }
                section.push(Item::Topic(TopicItem {
                    key: "bucket".into(),
                    selected: is_open(i.open.as_ref(), &t.group_id, &t.id),
                    tile: tile(p, &state),
                    topic: t,
                    state,
                    meta,
                    meta_you: false,
                    style: SlotStyle::Tile,
                    indent: space::S10,
                    rise: None,
                }));
            }
        }
        out.push(section);
    }
    sections(out)
}

/// C: members (busy first), each expandable to its 话题; idle members in
/// one row; 同事 below. Opens the first busy member when the user has not
/// opened or closed one yet.
pub(super) fn by_member(p: &dyn ChatProvider, i: &mut Inputs, side: &SideChats) -> Vec<Entry> {
    let rows = model::member_rows(p, i.me.as_deref(), i.member_sort, side);
    let opened = i
        .open_members
        .get_or_insert_with(|| {
            rows.iter()
                .find(|r| r.busy())
                .map(|r| r.member.id.clone())
                .into_iter()
                .collect()
        })
        .clone();
    let open = i.open.clone();
    let (bots, people): (Vec<MemberRow>, Vec<MemberRow>) =
        rows.into_iter().partition(|r| r.member.is_bot());
    let mut out: Vec<Item> = Vec::new();
    let block = |r: MemberRow, out: &mut Vec<Item>| {
        let expanded = opened.contains(&r.member.id);
        if expanded {
            let topics: Vec<Item> = r
                .topics
                .iter()
                .enumerate()
                .map(|(ix, mt)| {
                    Item::Topic(TopicItem {
                        key: format!("member-{}", r.member.id),
                        topic: mt.topic.clone(),
                        state: mt.state.clone(),
                        meta: mt.meta.clone(),
                        meta_you: mt.state == RowState::NeedsYou,
                        selected: is_open(open.as_ref(), &mt.topic.group_id, &mt.topic.id),
                        style: SlotStyle::Dot,
                        indent: NESTED_PAD,
                        tile: None,
                        rise: Some(ix),
                    })
                })
                .collect();
            out.push(Item::MemberHead { row: r, expanded });
            out.extend(topics);
        } else {
            out.push(Item::MemberHead { row: r, expanded });
        }
    };
    let mut idle: Vec<Member> = Vec::new();
    for r in bots {
        if r.busy() || i.member_sort == model::MemberSort::Roster {
            block(r, &mut out);
        } else {
            idle.push(r.member);
        }
    }
    if !idle.is_empty() {
        out.push(Item::Grouped {
            id: "rail-idle",
            word: c::RAIL_IDLE,
            members: idle,
            dim: false,
        });
    }
    if !people.is_empty() {
        out.push(Item::PeopleLabel);
        let mut away: Vec<Member> = Vec::new();
        for r in people {
            if r.busy() {
                block(r, &mut out);
            } else if r.member.online {
                out.push(Item::Plain(r.member));
            } else {
                away.push(r.member);
            }
        }
        if !away.is_empty() {
            out.push(Item::Grouped {
                id: "rail-away",
                word: c::RAIL_AWAY,
                members: away,
                dim: true,
            });
        }
    }
    out.into_iter()
        .map(|item| Entry { gap: px(0.), item })
        .collect()
}

impl RailView {
    /// Draws one list row from memory.
    pub(super) fn item(&self, item: &Item, cx: &mut Context<Self>) -> AnyElement {
        let k = theme::colors(cx);
        match item {
            Item::GroupHeader {
                id,
                name,
                folded,
                selected,
            } => {
                let fold_id = id.clone();
                let this = cx.entity().downgrade();
                let nav = self.services.nav.clone();
                let open_id = id.clone();
                self.section_header(
                    SharedString::from(format!("rail-group-{id}")),
                    name.clone(),
                    None,
                    false,
                    *folded,
                    *selected,
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
                )
            }
            Item::DmHeader { folded } => {
                let this = cx.entity().downgrade();
                self.section_header(
                    "rail-dms",
                    c::RAIL_DMS.into(),
                    None,
                    false,
                    *folded,
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
                )
            }
            Item::Dm {
                id,
                name,
                peer,
                selected,
            } => {
                let nav = self.services.nav.clone();
                let selected = *selected;
                let open_id = id.clone();
                ui::list_row(
                    SharedString::from(format!("rail-dm-{id}")),
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
                        .child(name.clone()),
                )
                .on_click(move |_, _, cx| {
                    let id = open_id.clone();
                    nav.update(cx, |nav, cx| nav.open_group(id, cx))
                })
                .into_any_element()
            }
            Item::BucketHeader {
                bucket,
                count,
                folded,
            } => {
                let bucket = *bucket;
                let this = cx.entity().downgrade();
                self.section_header(
                    SharedString::from(format!("rail-bucket-{bucket:?}")),
                    bucket.label().into(),
                    Some(*count),
                    bucket == Bucket::NeedsYou,
                    *folded,
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
                )
            }
            Item::Topic(t) => {
                let row = self.topic_row(t, cx);
                match t.rise {
                    Some(ix) => ui::motion::rise(
                        SharedString::from(format!("rail-{}-{}", t.key, t.topic.id)),
                        ix,
                        div().child(row),
                        cx,
                    ),
                    None => row,
                }
            }
            Item::MemberHead { row, expanded } => self.member_head(row, *expanded, cx),
            Item::Grouped {
                id,
                word,
                members,
                dim,
            } => self.grouped_row(id, word, members, *dim, cx),
            Item::PeopleLabel => div()
                .flex_none()
                .h(theme::RAIL_ROW)
                .px(space::S10)
                .flex()
                .items_center()
                .t_meta_medium()
                .text_color(k.ink_3)
                .child(c::RAIL_PEOPLE)
                .into_any_element(),
            Item::Plain(m) => self.plain_member(m, cx),
        }
    }

    /// The head row of a member in C.
    fn member_head(&self, r: &MemberRow, expanded: bool, cx: &mut Context<Self>) -> AnyElement {
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
        ui::list_row(
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
        })
        .into_any_element()
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
