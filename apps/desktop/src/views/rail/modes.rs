//! The three lists of the rail (RailOptions.dc.html A, B, C).
//!
//! The lists are read from the provider into flat [`Entry`] rows once per
//! change ([`RailView`] keeps them), and drawn from memory by a virtual list:
//! a breathing ring renders the rail again on every frame it asks for, so
//! the frame path must not read the cache.

use gpui::{
    AnyElement, App, Context, InteractiveElement as _, IntoElement, ParentElement, Pixels,
    SharedString, Styled as _, div, prelude::FluentBuilder as _, px,
};
use gpui_component::menu::ContextMenuExt as _;

use super::{
    RailView,
    model::{self, Bucket, MemberRow, Meta, RowState, SideChats},
    rows::{NESTED_PAD, ROW_AVATAR, SlotStyle},
};
use crate::{
    app::RailMode,
    copy::rail as c,
    data::{ChatProvider, GroupId, GroupKind, Member, MemberId, Topic},
    ui::MemberAvatar,
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
    Topic(Box<TopicItem>),
    /// C: a member and its state.
    MemberHead {
        row: Box<MemberRow>,
        expanded: bool,
    },
    /// C: a section header (`在做`, `空闲`, `不在`) that folds its rows.
    MemberSection {
        id: &'static str,
        word: &'static str,
        folded: bool,
    },
    /// C: a member with nothing on (`away`: not online): one row each.
    Plain { member: Member, away: bool },
    /// C: the collapsed tail of a section (`空闲 · 3`, `不在 · 2`): one
    /// dimmed row that opens to its members.
    MemberTail {
        id: &'static str,
        label: String,
        open: bool,
    },
}

impl Item {
    fn topic(t: TopicItem) -> Self {
        Item::Topic(Box::new(t))
    }
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
            // Section headers carry their own space above (§9.1).
            let _ = (si, i);
            let gap = px(0.);
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
                section.push(Item::topic(TopicItem {
                    key: "topic".into(),
                    selected: is_open(open, &g.id, &t.id),
                    tile: tile(p, &state),
                    topic: t,
                    state,
                    meta,
                    meta_you: false,
                    style: SlotStyle::Tile,
                    indent: px(0.),
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
                section.push(Item::topic(TopicItem {
                    key: "bucket".into(),
                    selected: is_open(i.open.as_ref(), &t.group_id, &t.id),
                    tile: tile(p, &state),
                    topic: t,
                    state,
                    meta,
                    meta_you: false,
                    style: SlotStyle::Tile,
                    indent: px(0.),
                    rise: None,
                }));
            }
        }
        out.push(section);
    }
    sections(out)
}

/// C: one row per member under the group labels `AI 成员` and `同事`
/// (COMPLETENESS.md §2.1, F33). A busy member opens to its 话题; its right
/// column holds a StatusBadge (`等你 2`) or the count of its 话题, never
/// text. The members with nothing on collapse into one tail item per group
/// (`空闲 · 3` for AI members, `不在 · 2` for people who are away). Opens
/// the first busy member when the user has not opened or closed one yet.
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
    let (mut bots, mut idle, mut people, mut away): (
        Vec<Item>,
        Vec<Item>,
        Vec<Item>,
        Vec<Item>,
    ) = Default::default();
    for r in rows {
        let bot = r.member.is_bot();
        if r.busy() {
            let expanded = opened.contains(&r.member.id);
            let topics: Vec<Item> = if expanded {
                r.topics
                    .iter()
                    .enumerate()
                    .map(|(ix, mt)| {
                        Item::topic(TopicItem {
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
                    .collect()
            } else {
                Vec::new()
            };
            let list = if bot { &mut bots } else { &mut people };
            list.push(Item::MemberHead {
                row: Box::new(r),
                expanded,
            });
            list.extend(topics);
        } else if bot {
            idle.push(Item::Plain {
                member: r.member,
                away: false,
            });
        } else if r.member.online {
            people.push(Item::Plain {
                member: r.member,
                away: false,
            });
        } else {
            away.push(Item::Plain {
                member: r.member,
                away: true,
            });
        }
    }
    let mut out: Vec<Item> = Vec::new();
    for (id, word, head, tail_id, tail_word, tail) in [
        (
            "rail-sec-ai",
            c::RAIL_AI_MEMBERS,
            bots,
            "rail-tail-idle",
            c::RAIL_IDLE,
            idle,
        ),
        (
            "rail-sec-people",
            c::RAIL_PEOPLE,
            people,
            "rail-tail-away",
            c::RAIL_AWAY,
            away,
        ),
    ] {
        if head.is_empty() && tail.is_empty() {
            continue;
        }
        let folded = i.folded_groups.contains(id);
        out.push(Item::MemberSection { id, word, folded });
        if folded {
            continue;
        }
        out.extend(head);
        if !tail.is_empty() {
            // Tails start closed: the set holds the open ones.
            let tail_open = i.folded_groups.contains(tail_id);
            out.push(Item::MemberTail {
                id: tail_id,
                label: crate::copy::fill(
                    c::RAIL_TAIL,
                    &[("word", tail_word), ("n", &tail.len().to_string())],
                ),
                open: tail_open,
            });
            if tail_open {
                out.extend(tail);
            }
        }
    }
    out.into_iter()
        .map(|item| Entry { gap: px(0.), item })
        .collect()
}

impl RailView {
    /// Draws one list row from memory.
    pub(super) fn item(&self, item: &Item, cx: &mut Context<Self>) -> AnyElement {
        match item {
            Item::GroupHeader { id, name, folded, .. } => {
                let fold_id = id.clone();
                let this = cx.entity().downgrade();
                let nav = self.services.nav.clone();
                let open_id = id.clone();
                let (menu_services, menu_id) = (self.services.clone(), id.clone());
                let header = self.section_header(
                    SharedString::from(format!("rail-group-{id}")),
                    name.clone(),
                    *folded,
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
                );
                // A right click on the header opens the 群's menu (§7.3).
                div()
                    .id(SharedString::from(format!("rail-group-menu-{id}")))
                    .child(header)
                    .context_menu(move |menu, _, _| {
                        super::menus::group(menu, &menu_services, &menu_id)
                    })
                    .into_any_element()
            }
            Item::DmHeader { folded } => {
                let this = cx.entity().downgrade();
                self.section_header(
                    "rail-dms",
                    c::RAIL_DMS.into(),
                    *folded,
                    move |_, _, cx| {
                        let _ = this.update(cx, |this, cx| {
                            if !this.folded_groups.remove("rail-dms") {
                                this.folded_groups.insert("rail-dms".into());
                            }
                            cx.notify();
                        });
                    },
                    None,
                )
            }
            Item::Dm {
                id,
                name,
                peer,
                selected,
            } => {
                let nav = self.services.nav.clone();
                let open_id = id.clone();
                let row = self
                    .row(SharedString::from(format!("rail-dm-{id}")), name.clone())
                    .leading(MemberAvatar::new(peer.as_ref(), ROW_AVATAR))
                    .selected(*selected)
                    .on_click(move |_, _, cx| {
                        let id = open_id.clone();
                        nav.update(cx, |nav, cx| nav.open_group(id, cx))
                    });
                div()
                    .id(SharedString::from(format!("rail-dm-row-{id}")))
                    .child(row)
                    .context_menu({
                        let (services, id) = (self.services.clone(), id.clone());
                        move |menu, _, _| super::menus::group(menu, &services, &id)
                    })
                    .into_any_element()
            }
            Item::BucketHeader { bucket, folded, .. } => {
                let bucket = *bucket;
                let this = cx.entity().downgrade();
                self.section_header(
                    SharedString::from(format!("rail-bucket-{bucket:?}")),
                    bucket.label().into(),
                    *folded,
                    move |_, _, cx| {
                        let _ = this.update(cx, |this, cx| {
                            if !this.folded_buckets.remove(&bucket) {
                                this.folded_buckets.insert(bucket);
                            }
                            cx.notify();
                        });
                    },
                    None,
                )
            }
            Item::Topic(t) => self.topic_row(t, cx),
            Item::MemberHead { row, expanded } => self.member_head(row, *expanded, cx),
            Item::MemberSection { id, word, folded } => {
                let id = *id;
                let this = cx.entity().downgrade();
                self.section_header(
                    id,
                    word.to_string(),
                    *folded,
                    move |_, _, cx| {
                        let _ = this.update(cx, |this, cx| {
                            if !this.folded_groups.remove(id) {
                                this.folded_groups.insert(id.into());
                            }
                            cx.notify();
                        });
                    },
                    None,
                )
            }
            Item::Plain { member, away } => self.plain_member(member, *away, cx),
            Item::MemberTail { id, label, open } => {
                let id = *id;
                let this = cx.entity().downgrade();
                self.row(id, label.clone())
                    .symbol(crate::ui::sf_symbols::Sf::More)
                    .disclosure(*open)
                    .dim(true)
                    .on_click(move |_, _, cx| {
                        let _ = this.update(cx, |this, cx| {
                            if !this.folded_groups.remove(id) {
                                this.folded_groups.insert(id.into());
                            }
                            cx.notify();
                        });
                    })
                    .into_any_element()
            }
        }
    }

    /// The row of a busy member in C: avatar 16, name, and one right
    /// column item: `等你 2` as a StatusBadge when 话题 wait on me, else the
    /// count of its 话题. The disclosure chevron shows when it is open, and
    /// on hover. A click opens or closes its 话题.
    fn member_head(&self, r: &MemberRow, expanded: bool, cx: &mut Context<Self>) -> AnyElement {
        let k = crate::views::shell::chrome::colors(cx);
        let id = r.member.id.clone();
        let this = cx.entity().downgrade();
        let mut row = self
            .row(
                SharedString::from(format!("rail-member-{}", r.member.id)),
                r.member.name.clone(),
            )
            .leading(MemberAvatar::new(Some(&r.member), ROW_AVATAR).on(k.sidebar))
            .unread(r.waiting_on_me > 0)
            .disclosure(expanded);
        row = if r.waiting_on_me > 0 {
            row.trailing_element(crate::ui::native::page::status_badge(
                crate::ui::native::page::Status::NeedsYou,
                crate::copy::fill(c::RAIL_WAITS_ON_YOU_N, &[("n", &r.waiting_on_me.to_string())]),
                cx,
            ))
        } else {
            row.count(r.topics.len() as u32)
        };
        row.on_click(move |_, _, cx| {
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

    /// A member with nothing on: avatar 16 and name; a click opens the
    /// member. A member who is away has a dimmed avatar.
    fn plain_member(&self, m: &Member, away: bool, _cx: &App) -> AnyElement {
        let nav = self.services.nav.clone();
        let id = m.id.clone();
        self.row(SharedString::from(format!("rail-person-{}", m.id)), m.name.clone())
            .leading(
                div()
                    .when(away, |d| d.opacity(0.5))
                    .child(MemberAvatar::new(Some(m), ROW_AVATAR)),
            )
            .on_click(move |_, _, cx| {
                let id = id.clone();
                nav.update(cx, |nav, cx| nav.open_members(Some(id), cx))
            })
            .into_any_element()
    }
}
