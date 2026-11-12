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

use super::{
    RailView,
    model::{self, Bucket, MemberRow, Meta, RowState, SideChats},
    rows::{NESTED_PAD, ROW_AVATAR, SlotStyle},
};
use crate::{
    app::RailMode,
    copy::rail as c,
    data::{ChatProvider, GroupId, GroupKind, Member, MemberId, Topic},
    ui::{MemberAvatar, native::sidebar::RightColumn},
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
    /// A: `⌄ 开发`, which opens the 群's 群聊; `unread` when the 群聊 has
    /// messages I have not read.
    GroupHeader {
        id: GroupId,
        name: String,
        folded: bool,
        selected: bool,
        unread: bool,
        /// I muted the 群: a `bell.slash` after its name.
        muted: bool,
    },
    /// A: `⌄ 私聊`.
    DmHeader {
        folded: bool,
    },
    /// A: a DM. T-11: the member-row right column: the state of its 话题
    /// that waits on me (else its newest stateful 话题), and the unread dot.
    Dm {
        id: GroupId,
        name: String,
        peer: Option<Member>,
        selected: bool,
        line: Option<crate::views::topic_page::StateLine>,
        unread: bool,
        /// I muted the 私聊.
        muted: bool,
    },
    /// B: `⌄ • 需要你 4`.
    BucketHeader {
        bucket: Bucket,
        count: usize,
        folded: bool,
    },
    /// B: a folded bucket as one dimmed row (`完成 · 今天 · 2`) that
    /// opens it.
    BucketTail {
        bucket: Bucket,
        count: usize,
    },
    Topic(Box<TopicItem>),
    /// A: the muted row of an open 群 with no 话题 (B1-31).
    NoTopics {
        id: GroupId,
    },
    /// A: a 群 whose `n` 话题 all leave me out (我参与的): one row that opens
    /// the 群.
    OtherTopics {
        id: GroupId,
        n: usize,
    },
    /// A: an open 群 with no cached 话题 while the first sync runs: sidebar
    /// item skeletons (§2.14, T-13).
    Syncing {
        id: GroupId,
    },
    /// C: a member and its state.
    MemberHead {
        row: Box<MemberRow>,
        expanded: bool,
        /// The state of its newest 话题, for the badge of a member with no
        /// run.
        first: Option<crate::views::topic_page::StateLine>,
    },
    /// C: a section header (`在做`, `空闲`, `不在`) that folds its rows.
    MemberSection {
        id: &'static str,
        word: &'static str,
        folded: bool,
    },
    /// C: a member with nothing on (`away`: not online): one row each.
    Plain {
        member: Member,
        away: bool,
    },
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
    /// The one state of the 话题 (`topic_page::topic_state`): its short
    /// words go to the right column.
    pub line: crate::views::topic_page::StateLine,
    /// The 话题 is muted, read with the rest so a frame reads no store.
    pub muted: bool,
}

/// Where the first sync stands (T-13), from the connection state: the
/// sidebar says 还没有话题 only for a 群 the server has answered for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Sync {
    Synced,
    Syncing,
    Unreachable,
}

impl Sync {
    pub fn of(state: &crate::data::ConnectionState) -> Self {
        use crate::data::ConnectionState as S;
        match state {
            S::Online | S::Offline => Sync::Synced,
            S::Connecting => Sync::Syncing,
            S::Reconnecting { .. } | S::TokenUnreadable => Sync::Unreachable,
        }
    }
}

/// What the lists depend on besides the provider.
#[derive(Clone, PartialEq)]
pub(super) struct Inputs {
    pub sync: Sync,
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
        let listed: Vec<_> = p.topics(&g.id).into_iter().filter(model::listed).collect();
        let all = listed.len();
        let topics: Vec<_> = listed
            .into_iter()
            .filter(|t| {
                i.show_all || model::involves_me(p, me, t) || is_open(open, &t.group_id, &t.id)
            })
            .collect();
        // 1.1.1: every 群 I am in keeps its entry, which opens its 群聊, also
        // with no 话题 or none of mine (onboarding: one row under it then).
        let open_group = i.group_page.as_deref() == Some(g.id.as_str());
        let empty = topics.is_empty();
        let folded = i.folded_groups.contains(&g.id);
        let mut section = vec![Item::GroupHeader {
            id: g.id.clone(),
            name: g.name.clone(),
            folded,
            selected: open_group,
            unread: chat_unread(p, &g.id),
            muted: p.group_muted(&g.id),
        }];
        // T-13: 还没有话题 only once the server answered; while it syncs the
        // open 群 shows sidebar skeleton rows, and nothing while unreachable.
        if !folded && empty {
            match i.sync {
                Sync::Synced if all > 0 => section.push(Item::OtherTopics {
                    id: g.id.clone(),
                    n: all,
                }),
                Sync::Synced => section.push(Item::NoTopics { id: g.id.clone() }),
                Sync::Syncing => section.push(Item::Syncing { id: g.id.clone() }),
                Sync::Unreachable => {}
            }
        }
        if !folded {
            for t in topics {
                let (state, meta) = model::row_state(p, me, &t);
                section.push(Item::topic(TopicItem {
                    line: crate::views::topic_page::topic_state(p, me, &t),
                    muted: p.topic_muted(&t.group_id, &t.id),
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
                let selected = i.group_page.as_deref() == Some(g.id.as_str())
                    || open.is_some_and(|(og, _)| og == &g.id);
                let (line, unread) = dm_state(p, me, &g.id);
                section.push(Item::Dm {
                    id: g.id.clone(),
                    name: g.name.clone(),
                    peer: g.dm_peer.as_deref().and_then(|m| p.member(m)),
                    selected,
                    line,
                    unread: unread && !selected,
                    muted: p.group_muted(&g.id),
                });
            }
        }
        out.push(section);
    }
    sections(out)
}

/// T-11: a DM row's state: the 话题 of the DM that waits on me, else its
/// newest 话题 with a sidebar state; and whether any of its 话题 is unread.
/// The 群's own chat (群聊) has messages I have not read.
fn chat_unread(p: &dyn ChatProvider, group_id: &str) -> bool {
    p.topic(group_id, &crate::data::main_topic_id(group_id))
        .is_some_and(|t| t.unread > 0)
}

fn dm_state(
    p: &dyn ChatProvider,
    me: Option<&str>,
    group_id: &str,
) -> (Option<crate::views::topic_page::StateLine>, bool) {
    let mut topics: Vec<Topic> = p
        .topics(group_id)
        .into_iter()
        .filter(model::listed)
        .collect();
    topics.sort_by_key(|t| std::cmp::Reverse(t.last_activity_at));
    let unread = topics.iter().any(|t| t.unread > 0) || chat_unread(p, group_id);
    let lines: Vec<_> = topics
        .iter()
        .map(|t| crate::views::topic_page::topic_state(p, me, t))
        .filter(|l| !l.short.is_empty())
        .collect();
    let line = lines
        .iter()
        .find(|l| l.badge == crate::ui::native::BadgeState::NeedsYou)
        .or(lines.first())
        .cloned();
    (line, unread)
}

/// B: 需要你 / 在做 / 等别人 / 完成 · 今天, of the chosen 群.
pub(super) fn attention(p: &dyn ChatProvider, i: &Inputs, side: &SideChats) -> Vec<Entry> {
    let me = i.me.as_deref();
    let names = model::groups(p);
    let mut out: Vec<Vec<Item>> = Vec::new();
    for (bucket, topics) in model::buckets(p, me, i.chip.as_deref(), side) {
        if topics.is_empty() {
            continue;
        }
        let folded = i.folded_buckets.contains(&bucket);
        // §2.1: a folded section is one dimmed "More" row with its count
        // (`完成 · 今天 · 2`), never a label with no rows under it.
        // B1-25: under its own label, so it never reads as part of the
        // section above.
        if folded {
            out.push(vec![
                Item::BucketHeader {
                    bucket,
                    count: topics.len(),
                    folded,
                },
                Item::BucketTail {
                    bucket,
                    count: topics.len(),
                },
            ]);
            continue;
        }
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
                    line: crate::views::topic_page::topic_state(p, me, &t),
                    muted: p.topic_muted(&t.group_id, &t.id),
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
    let (mut bots, mut idle, mut people, mut away): (Vec<Item>, Vec<Item>, Vec<Item>, Vec<Item>) =
        Default::default();
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
                            line: crate::views::topic_page::topic_state(
                                p,
                                i.me.as_deref(),
                                &mt.topic,
                            ),
                            muted: p.topic_muted(&mt.topic.group_id, &mt.topic.id),
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
            let first = r
                .topics
                .first()
                .map(|mt| crate::views::topic_page::topic_state(p, i.me.as_deref(), &mt.topic));
            list.push(Item::MemberHead {
                row: Box::new(r),
                expanded,
                first,
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
            Item::GroupHeader {
                id,
                name,
                folded,
                selected,
                unread,
                muted,
            } => {
                let fold_id = id.clone();
                let this = cx.entity().downgrade();
                let nav = self.services.nav.clone();
                let open_id = id.clone();
                let (menu_services, menu_id) = (self.services.clone(), id.clone());
                // A right click on the header opens the 群's menu (§7.3).
                let menu = self.row_menu(move |menu, window, cx| {
                    super::menus::group(menu, &menu_services, &menu_id, window, cx)
                });
                self.section_header_selected(
                    SharedString::from(format!("rail-group-{id}")),
                    name.clone(),
                    (*folded, *selected, *unread, *muted),
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
                        nav.update(cx, |nav, cx| nav.open_group_chat(g, cx))
                    })),
                    Some(menu),
                )
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
                line,
                unread,
                muted,
            } => {
                let nav = self.services.nav.clone();
                let open_id = id.clone();
                let (services, menu_id) = (self.services.clone(), id.clone());
                let menu = self.row_menu(move |menu, window, cx| {
                    super::menus::group(menu, &services, &menu_id, window, cx)
                });
                let row = self
                    .row(
                        SharedString::from(format!("rail-dm-{id}")),
                        // D1-56: beside the avatar, no leading emoji.
                        crate::ui::avatar::name_beside_avatar(name).to_string(),
                    )
                    .leading(MemberAvatar::new(peer.as_ref(), ROW_AVATAR))
                    .selected(*selected)
                    .unread(*unread)
                    .muted(*muted)
                    // One signal per row: the state's mark only.
                    .right(match line {
                        Some(l) => RightColumn::Mark(l.badge),
                        None => RightColumn::None,
                    })
                    .on_click(move |_, _, cx| {
                        let id = open_id.clone();
                        nav.update(cx, |nav, cx| nav.open_group_chat(id, cx))
                    })
                    .menu(menu);
                div()
                    .id(SharedString::from(format!("rail-dm-row-{id}")))
                    .child(row)
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
            Item::BucketTail { bucket, count } => {
                let bucket = *bucket;
                let this = cx.entity().downgrade();
                self.row(
                    SharedString::from(format!("rail-bucket-tail-{bucket:?}")),
                    crate::copy::fill(c::RAIL_FOLDED_N, &[("n", &count.to_string())]),
                )
                .symbol(crate::ui::sf_symbols::Sf::More)
                .disclosure(false)
                .dim(true)
                .on_click(move |_, _, cx| {
                    let _ = this.update(cx, |this, cx| {
                        this.folded_buckets.remove(&bucket);
                        cx.notify();
                    });
                })
                .into_any_element()
            }
            Item::Topic(t) => self.topic_row(t, cx),
            Item::MemberHead {
                row,
                expanded,
                first,
            } => self.member_head(row, *expanded, first.clone(), cx),
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
            // B1-51: the compact Empty line in the muted token, at a row's
            // place and inset.
            Item::NoTopics { id } => div()
                .id(SharedString::from(format!("rail-no-topics-{id}")))
                .h(crate::ui::native::tokens::metrics::SIDEBAR_ROW)
                .mx(crate::ui::native::tokens::metrics::SIDEBAR_ROW_INSET)
                .pl(crate::ui::native::tokens::space::S8)
                .flex()
                .items_center()
                .child(crate::ui::native::EmptyLine::new(c::RAIL_NO_TOPICS))
                .into_any_element(),
            Item::OtherTopics { id, n } => {
                let nav = self.services.nav.clone();
                let open_id = id.clone();
                self.row(
                    SharedString::from(format!("rail-other-topics-{id}")),
                    crate::copy::fill(c::RAIL_OTHER_TOPICS, &[("n", &n.to_string())]),
                )
                .symbol(crate::ui::sf_symbols::Sf::More)
                .dim(true)
                .on_click(move |_, _, cx| {
                    let g = open_id.clone();
                    nav.update(cx, |nav, cx| nav.open_group(g, cx))
                })
                .into_any_element()
            }
            Item::Syncing { id } => div()
                .id(SharedString::from(format!("rail-syncing-{id}")))
                .child(
                    crate::ui::native::skeleton_rows(
                        crate::ui::native::SkeletonKind::SidebarItem,
                        3,
                    )
                    .mx(crate::ui::native::tokens::metrics::SIDEBAR_ROW_INSET),
                )
                .into_any_element(),
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
    fn member_head(
        &self,
        r: &MemberRow,
        expanded: bool,
        first: Option<crate::views::topic_page::StateLine>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = crate::views::shell::chrome::colors(cx);
        let id = r.member.id.clone();
        let this = cx.entity().downgrade();
        let mut row = self
            .row(
                SharedString::from(format!("rail-member-{}", r.member.id)),
                crate::ui::avatar::name_beside_avatar(&r.member.name).to_string(),
            )
            .leading(MemberAvatar::new(Some(&r.member), ROW_AVATAR).on(k.sidebar));
        // R2-11, §2.1: one right-column kind in the group: a StatusBadge on
        // every busy member (`等你 2`, else `在做 3`). An open member keeps its
        // badge (its 话题 under it show that it is open); a closed one shows
        // the chevron on hover.
        if !expanded {
            row = row.disclosure(false);
        }
        // B1-24, §2.12: 在做 only while the member's run works; a busy
        // member with no run says the state of its newest 话题 (等X, 排队).
        let badge = if r.waiting_on_me > 0 {
            (
                crate::ui::native::BadgeState::NeedsYou,
                crate::copy::fill(
                    c::RAIL_WAITS_ON_YOU_N,
                    &[("n", &r.waiting_on_me.to_string())],
                ),
            )
        } else if r.working {
            let text = r.plan.as_ref().map_or_else(
                || c::RAIL_WORKING.to_string(),
                |n| crate::copy::fill(c::RAIL_WORKING_N, &[("n", n)]),
            );
            (crate::ui::native::BadgeState::Working, text)
        } else {
            first.map_or(
                (
                    crate::ui::native::BadgeState::Queued,
                    crate::copy::common::STATUS_QUEUED.to_string(),
                ),
                |l| {
                    let text = if l.short.is_empty() { l.text } else { l.short };
                    (l.badge, text)
                },
            )
        };
        // T-18, F3: the kit's capped right column, as on a 话题 row: a long
        // badge cuts with "…" (tooltip), and the name keeps its room.
        row = row.right(RightColumn::Status(badge.0, badge.1.into()));
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
        self.row(
            SharedString::from(format!("rail-person-{}", m.id)),
            crate::ui::avatar::name_beside_avatar(&m.name).to_string(),
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::v2::{self as d, V2Samples};

    fn inputs(me: &str, show_all: bool) -> Inputs {
        Inputs {
            sync: Sync::Synced,
            mode: RailMode::Work,
            me: Some(me.into()),
            open: None,
            group_page: None,
            show_all,
            chip: None,
            member_sort: model::MemberSort::Busy,
            folded_groups: Default::default(),
            folded_buckets: Default::default(),
            open_members: None,
        }
    }

    /// Onboarding: a member no 话题 involves yet still sees every 群 they are in, each with
    /// one row that opens it, never an empty sidebar.
    #[test]
    fn a_new_member_sees_every_group_they_are_in() {
        let p = V2Samples::new();
        // Someone new follows nothing yet.
        for g in p.groups() {
            for t in p.topics(&g.id) {
                p.set_topic_following(&g.id, &t.id, false);
            }
        }
        let entries = work(p.as_ref(), &inputs("someone-new", false));
        let headers: Vec<_> = entries
            .iter()
            .filter_map(|e| match &e.item {
                Item::GroupHeader { id, .. } => Some(id.clone()),
                _ => None,
            })
            .collect();
        let groups: Vec<_> = model::groups(p.as_ref())
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert_eq!(headers, groups);
        // Under each header: its 话题 of mine, else one row (还没有话题 or N 个话题).
        for (ix, e) in entries.iter().enumerate() {
            if matches!(e.item, Item::GroupHeader { .. }) {
                assert!(matches!(
                    entries.get(ix + 1).map(|e| &e.item),
                    Some(Item::Topic(_) | Item::OtherTopics { .. } | Item::NoTopics { .. })
                ));
            }
        }
        // A 群 with 话题 that all leave me out folds them into one row.
        assert!(
            entries
                .iter()
                .any(|e| matches!(e.item, Item::OtherTopics { n, .. } if n > 0))
        );
    }

    /// 我参与的 still lists only my 话题; 全部话题 lists them all.
    #[test]
    fn my_topics_and_all_topics_still_differ() {
        let p = V2Samples::new();
        let count = |all| {
            work(p.as_ref(), &inputs(d::ME, all))
                .iter()
                .filter(|e| matches!(e.item, Item::Topic(_)))
                .count()
        };
        assert!(count(false) > 0);
        assert!(count(true) >= count(false));
    }
}
