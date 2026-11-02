//! The Cmd+K quick switcher: one search field over 群, 私聊, 话题 and 成员.
//! The palette matches the typed text as a substring of each name or title,
//! ignoring case. Picking a member opens (or starts) the DM with them.

use std::{cell::Cell, rc::Rc};

use gpui::{
    App, AppContext as _, ParentElement, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    WindowExt as _,
    command::{Command, CommandGroup, CommandItem, CommandState},
};

use crate::{
    app::Services,
    copy::nav,
    data::{ChatProvider, GroupId, GroupKind, Member, MemberId, Topic, TopicId},
    theme::{self, Type as _, space},
    ui::{self, KbdHint, Lucide, StatusChip},
    views::{sidebar::topic_line, topic_list::list_time},
};

/// What the palette shows (DESIGN.md §13.12): 话题 first, then 群, DMs and
/// members together under 群和成员. Confirm indexes refer to this list.
pub fn display(sections: Vec<(Section, Vec<Entry>)>) -> Vec<(&'static str, Vec<Entry>)> {
    let mut topics = Vec::new();
    let mut others = Vec::new();
    for (section, items) in sections {
        match section {
            Section::Topics => topics.extend(items),
            _ => others.extend(items),
        }
    }
    [
        (nav::SECTION_TOPICS, topics),
        (nav::SECTION_GROUPS_MEMBERS, others),
    ]
    .into_iter()
    .filter(|(_, e)| !e.is_empty())
    .collect()
}

/// What an entry opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Group(GroupId),
    Topic(GroupId, TopicId),
    /// Opens the DM with this member, creating it when needed.
    Member(MemberId),
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub target: Target,
    /// The searched text: a group name, a DM peer's name, a topic title or a member name.
    pub label: String,
    /// Shown after the label: the topic's group, or the member's 岗位.
    pub detail: Option<String>,
    /// For DMs and members: the avatar.
    pub member: Option<Member>,
    /// For topics: the topic, for its status chip and time.
    pub topic: Option<Topic>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Groups,
    Dms,
    Topics,
    Members,
}

/// The switcher's entries in display order; empty sections are left out.
pub fn entries(provider: &dyn ChatProvider, me: Option<&str>) -> Vec<(Section, Vec<Entry>)> {
    let members = provider.members();
    let groups = provider.groups();
    let find = |id: &str| members.iter().find(|m| m.id == id).cloned();
    let mut group_entries = Vec::new();
    let mut dm_entries = Vec::new();
    let mut topic_entries = Vec::new();
    for g in &groups {
        let entry = Entry {
            target: Target::Group(g.id.clone()),
            label: g.name.clone(),
            detail: None,
            member: g.dm_peer.as_deref().and_then(find),
            topic: None,
        };
        match g.kind {
            GroupKind::Group => group_entries.push(entry),
            GroupKind::Dm => dm_entries.push(entry),
        }
        for t in provider.topics(&g.id) {
            topic_entries.push(Entry {
                target: Target::Topic(g.id.clone(), t.id.clone()),
                label: t.title.clone(),
                detail: Some(g.name.clone()),
                member: None,
                topic: Some(t),
            });
        }
    }
    // Recent topics first (DESIGN.md §13.12).
    topic_entries.sort_by_key(|e: &Entry| {
        std::cmp::Reverse(e.topic.as_ref().map(|t| t.last_activity_at).unwrap_or(0))
    });
    let has_dm = |id: &str| {
        groups
            .iter()
            .any(|g| g.kind == GroupKind::Dm && g.dm_peer.as_deref() == Some(id))
    };
    let member_entries: Vec<Entry> = members
        .iter()
        .filter(|m| Some(m.id.as_str()) != me && !m.deactivated && !has_dm(&m.id))
        .map(|m| Entry {
            target: Target::Member(m.id.clone()),
            label: m.name.clone(),
            detail: m.role.clone(),
            member: Some(m.clone()),
            topic: None,
        })
        .collect();
    [
        (Section::Groups, group_entries),
        (Section::Dms, dm_entries),
        (Section::Topics, topic_entries),
        (Section::Members, member_entries),
    ]
    .into_iter()
    .filter(|(_, e)| !e.is_empty())
    .collect()
}

/// The palette's own matching rule, for tests and other callers: a
/// case-insensitive substring of the label.
pub fn matches(entry: &Entry, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty() || entry.label.to_lowercase().contains(&query)
}

/// Opens what an entry points at.
pub fn go(services: &Services, target: &Target, cx: &mut App) {
    match target {
        Target::Group(g) => {
            let g = g.clone();
            services.nav.update(cx, |nav, cx| nav.open_group(g, cx));
        }
        Target::Topic(g, t) => {
            let (g, t) = (g.clone(), t.clone());
            services.nav.update(cx, |nav, cx| {
                nav.open_group(g.clone(), cx);
                nav.open_topic(g, t, cx);
            });
        }
        Target::Member(m) => super::open_dm_with(m.clone(), cx),
    }
}

/// The widest a row's title gets before it truncates.
const LABEL_MAX: gpui::Pixels = px(320.);

fn row(
    entry: &Entry,
    me: Option<String>,
    members: Rc<Vec<Member>>,
) -> impl Fn(&mut Window, &mut App) -> gpui::Div + 'static {
    let entry = entry.clone();
    move |_, cx| {
        let colors = theme::colors(cx);
        let tile = |glyph: Lucide| {
            div()
                .flex_none()
                .size(px(24.))
                .rounded(theme::radius::SM)
                .bg(colors.fill_field)
                .flex()
                .items_center()
                .justify_center()
                .child(ui::icon(glyph, theme::icon::BUTTON, colors.text_2))
        };
        let lead = match (&entry.target, &entry.member) {
            (Target::Topic(..), _) => tile(Lucide::MessageSquarePlus),
            (_, Some(m)) => div()
                .flex_none()
                .child(ui::MemberAvatar::new(Some(m), px(24.)).on(colors.elevated)),
            (Target::Group(g), _) => {
                div()
                    .flex_none()
                    .child(ui::GroupTile::new(g.clone(), entry.label.clone(), px(24.)))
            }
            _ => tile(Lucide::Hash),
        };
        let (label, untitled) = match &entry.topic {
            Some(_) => {
                let line = topic_line(&entry.label);
                if line.is_empty() {
                    (nav::UNTITLED.to_string(), true)
                } else {
                    (line, false)
                }
            }
            None => (entry.label.clone(), false),
        };
        let chip = entry
            .topic
            .as_ref()
            .and_then(|t| StatusChip::for_topic(t, me.as_deref(), &members));
        // `开发 · 14:32` for topics, the 岗位 for members.
        let detail = match (&entry.topic, &entry.detail) {
            (Some(t), Some(group)) => {
                let time = list_time(t.last_activity_at, chrono::Local::now());
                Some(if time.is_empty() {
                    group.clone()
                } else {
                    format!("{group} · {time}")
                })
            }
            (_, detail) => detail.clone(),
        };
        // CommandItem adds 6 pt above and below: 28 + 12 = the 40 pt row (§13.12).
        div()
            .h(px(28.))
            .flex()
            .items_center()
            .gap(space::S10)
            .w_full()
            .overflow_hidden()
            .t_ui()
            .child(lead)
            .child(
                // A definite cap so the ellipsis shows and the chip and the
                // detail keep room (DESIGN.md §12).
                div()
                    .flex_none()
                    .max_w(LABEL_MAX)
                    .one_line()
                    .when(untitled, |el| el.text_color(colors.text_2))
                    .child(label),
            )
            .children(chip.map(|c| div().flex_none().child(c)))
            .children(detail.map(|d| {
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .t_callout()
                    .tnum()
                    .text_color(colors.text_2)
                    .one_line()
                    .child(d)
            }))
    }
}

/// Opens the switcher in `window`.
pub fn open(services: Services, window: &mut Window, cx: &mut App) {
    let me = services.me();
    let members = Rc::new(services.provider.members());
    let sections = Rc::new(display(entries(services.provider.as_ref(), me.as_deref())));
    let state = cx.new(|cx| CommandState::new(window, cx));
    let first = Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            state.update(cx, |s, cx| s.focus(window, cx));
        }
        let mut command = Command::new(&state)
            .placeholder(nav::QUICK_SWITCH_PLACEHOLDER)
            .max_h(px(420.))
            .bordered(false)
            .empty(|_, _, _| {
                div().h(px(160.)).child(ui::EmptyState::new(
                    Lucide::SearchX,
                    nav::QUICK_SWITCH_EMPTY,
                ))
            })
            .footer(|_, _, cx| {
                let colors = theme::colors(cx);
                div()
                    .h(px(36.))
                    .px(space::S16)
                    .flex()
                    .items_center()
                    .gap(space::S16)
                    .border_t_1()
                    .border_color(colors.sep)
                    .child(KbdHint::new(&["up", "down"]).label(nav::SWITCHER_SELECT))
                    .child(KbdHint::new(&["enter"]).label(nav::SWITCHER_OPEN))
            });
        for (heading, items) in sections.iter() {
            command = command.group(CommandGroup::new().label(*heading).items(items.iter().map(
                |e| {
                    CommandItem::new().label(e.label.clone()).child(row(
                        e,
                        me.clone(),
                        members.clone(),
                    ))
                },
            )));
        }
        let sections = sections.clone();
        let services = services.clone();
        let command = command.on_confirm(move |ix, window, cx| {
            let target = sections
                .get(ix.section)
                .and_then(|(_, items)| items.get(ix.row))
                .map(|e| e.target.clone());
            window.close_dialog(cx);
            if let Some(target) = target {
                go(&services, &target, cx);
            }
        });
        crate::ui::sheet::switcher(dialog)
            .close_button(false)
            .p_0()
            .child(command)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::{FakeProvider, ME};

    fn search(p: &FakeProvider, q: &str) -> Vec<Target> {
        entries(p, Some(ME))
            .into_iter()
            .flat_map(|(_, e)| e)
            .filter(|e| matches(e, q))
            .map(|e| e.target)
            .collect()
    }

    #[test]
    fn covers_groups_dms_topics_and_members() {
        let p = FakeProvider::new();
        let sections: Vec<Section> = entries(p.as_ref(), Some(ME))
            .iter()
            .map(|(s, _)| *s)
            .collect();
        assert_eq!(sections[0], Section::Groups);
        assert!(sections.contains(&Section::Dms));
        assert!(sections.contains(&Section::Topics));
        let all = search(&p, "");
        // I am never in the member list.
        assert!(!all.contains(&Target::Member(ME.into())));
        // Every group and every topic is there.
        for g in p.groups() {
            assert!(all.contains(&Target::Group(g.id.clone())));
            for t in p.topics(&g.id) {
                assert!(all.contains(&Target::Topic(g.id.clone(), t.id)));
            }
        }
    }

    #[test]
    fn substring_ignores_case_and_finds_each_kind() {
        let p = FakeProvider::new();
        let group = p
            .groups()
            .into_iter()
            .find(|g| g.kind == GroupKind::Group)
            .unwrap();
        let middle: String = group.name.chars().skip(1).take(2).collect();
        let hits = search(&p, &middle.to_uppercase());
        assert!(hits.contains(&Target::Group(group.id.clone())), "{middle}");
        let topic = p.topics(&group.id).into_iter().next().unwrap();
        let part: String = topic.title.chars().skip(1).take(3).collect();
        assert!(search(&p, &part).contains(&Target::Topic(group.id.clone(), topic.id)));
        let dm = p
            .groups()
            .into_iter()
            .find(|g| g.kind == GroupKind::Dm)
            .unwrap();
        assert!(search(&p, &dm.name).contains(&Target::Group(dm.id)));
        assert!(search(&p, "zzzz-no-such-thing").is_empty());
    }

    #[test]
    fn members_without_a_dm_are_offered() {
        let p = FakeProvider::new();
        let members: Vec<Target> = entries(p.as_ref(), Some(ME))
            .into_iter()
            .filter(|(s, _)| *s == Section::Members)
            .flat_map(|(_, e)| e)
            .map(|e| e.target)
            .collect();
        let groups = p.groups();
        for t in &members {
            let Target::Member(id) = t else { panic!() };
            assert!(
                !groups
                    .iter()
                    .any(|g| g.dm_peer.as_deref() == Some(id.as_str()))
            );
            assert!(!p.member(id).unwrap().deactivated);
        }
    }
}
