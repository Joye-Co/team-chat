//! What the 记得的事 and 团队约定 lists show, without the UI: who may change them, how the
//! memories group by kind, and the words of a 记忆更新 line. Unit-tested.

use chrono::{Datelike as _, NaiveDate};

use crate::{
    copy::{fill, memory as c},
    data::{
        Member, MemberLevel,
        memory::{
            Memory, MemoryChange, MemoryDay, MemoryKind, MemoryScope, MemorySourceKind,
            MemoryStatus, MemoryUpdate,
        },
    },
};

/// True when `me` may write `scope` (docs/protocol.md "Memory": the team is written by
/// owners; an AI member's memories by owners and that member; a group's by its members, and
/// a group board only shows in a group I am in, the server checks the rest; a person's
/// only by that person).
pub fn can_write(scope: &MemoryScope, me: Option<&Member>) -> bool {
    let Some(me) = me.filter(|m| !m.deactivated) else {
        return false;
    };
    match scope {
        MemoryScope::Team => me.level == MemberLevel::Owner,
        MemoryScope::Member(id) => me.level == MemberLevel::Owner || me.id == *id,
        MemoryScope::Group(_) => true,
        MemoryScope::Person(id) => me.id == *id,
    }
}

/// True when exactly one person uses the server (盯 4.12): the memory screens fold into
/// 我的约定 (the team's and mine together) and each AI member's 记得的事. AI members and
/// deactivated people do not count.
pub fn single_user(members: &[Member]) -> bool {
    members
        .iter()
        .filter(|m| !m.is_bot() && !m.deactivated)
        .count()
        == 1
}

/// The scope a board reads once the signed-in person is `me`: a person's board (我的偏好,
/// 我的约定) is always the signed-in person's, so it follows a change of account; every
/// other scope stays. `None` while nobody is signed in: the board reads nothing.
pub fn scope_for(scope: &MemoryScope, me: Option<&str>) -> Option<MemoryScope> {
    match scope {
        MemoryScope::Person(_) => me
            .filter(|id| !id.is_empty())
            .map(|id| MemoryScope::Person(id.to_string())),
        other => Some(other.clone()),
    }
}

/// Where 记一条 in 我的约定 keeps a memory: 偏好 is mine; 约定 and 事实 are the team's while
/// I may write the team, else mine as well.
pub fn folded_scope(kind: MemoryKind, me: &str, team_writer: bool) -> MemoryScope {
    if kind == MemoryKind::Preference || !team_writer {
        MemoryScope::Person(me.to_string())
    } else {
        MemoryScope::Team
    }
}

/// The 记忆更新 of several scopes as one: the newest day any of them has, with that day's
/// lines of each, oldest first.
pub fn merge_days(days: Vec<MemoryDay>) -> MemoryDay {
    let newest = days.iter().filter_map(|d| d.day.clone()).max();
    let mut updates: Vec<MemoryUpdate> = days
        .into_iter()
        .filter(|d| d.day.is_some() && d.day == newest)
        .flat_map(|d| d.updates)
        .collect();
    updates.sort_by_key(|u| u.at);
    MemoryDay {
        day: newest,
        updates,
    }
}

/// The words of a kind.
pub fn kind_word(kind: MemoryKind) -> &'static str {
    match kind {
        MemoryKind::Convention => c::KIND_CONVENTION,
        MemoryKind::Fact => c::KIND_FACT,
        MemoryKind::Decision => c::KIND_DECISION,
        MemoryKind::Preference => c::KIND_PREFERENCE,
        MemoryKind::Lesson => c::KIND_LESSON,
        MemoryKind::Playbook => c::KIND_PLAYBOOK,
        MemoryKind::Other => c::KIND_OTHER,
    }
}

/// The kind groups of a board, in the order they show; every other kind is 其他. An AI
/// member: 经验, 做法. The team and a group: 约定, 事实. A person: 偏好, 事实. 我的约定
/// (`folded`, the team's and mine together): 约定, 偏好, 事实.
pub fn kind_order(scope: &MemoryScope, folded: bool) -> &'static [MemoryKind] {
    use MemoryKind::*;
    if folded {
        return &[Convention, Preference, Fact];
    }
    match scope {
        MemoryScope::Member(_) => &[Lesson, Playbook],
        MemoryScope::Team | MemoryScope::Group(_) => &[Convention, Fact],
        MemoryScope::Person(_) => &[Preference, Fact],
    }
}

fn group_of(order: &[MemoryKind], kind: MemoryKind) -> MemoryKind {
    if order.contains(&kind) {
        kind
    } else {
        MemoryKind::Other
    }
}

fn group_rank(order: &[MemoryKind], group: MemoryKind) -> usize {
    order
        .iter()
        .position(|k| *k == group)
        .unwrap_or(order.len())
}

/// One kind group: its title and its memories (newest change first).
#[derive(Debug, PartialEq)]
pub struct KindGroup<'a> {
    pub kind: MemoryKind,
    pub title: &'static str,
    pub items: Vec<&'a Memory>,
}

/// The lists of a scope: the kind groups of what is in use (active and today's pending),
/// then what was 停用.
#[derive(Debug, PartialEq)]
pub struct Lists<'a> {
    pub groups: Vec<KindGroup<'a>>,
    pub retired: Vec<&'a Memory>,
}

impl Lists<'_> {
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty() && self.retired.is_empty()
    }
}

pub fn lists<'a>(scope: &MemoryScope, memories: &'a [Memory]) -> Lists<'a> {
    lists_in(kind_order(scope, false), memories)
}

/// [`lists`] with the kind groups of `order`.
pub fn lists_in<'a>(order: &[MemoryKind], memories: &'a [Memory]) -> Lists<'a> {
    let mut groups: Vec<KindGroup<'a>> = Vec::new();
    let mut retired = Vec::new();
    let mut sorted: Vec<&Memory> = memories.iter().collect();
    sorted.sort_by_key(|m| std::cmp::Reverse(m.updated_at));
    for m in sorted {
        match m.status {
            MemoryStatus::Retired => retired.push(m),
            MemoryStatus::Active | MemoryStatus::Pending => {
                let g = group_of(order, m.kind);
                match groups.iter_mut().find(|x| x.kind == g) {
                    Some(x) => x.items.push(m),
                    None => groups.push(KindGroup {
                        kind: g,
                        title: kind_word(g),
                        items: vec![m],
                    }),
                }
            }
            MemoryStatus::Other => {}
        }
    }
    groups.sort_by_key(|g| group_rank(order, g.kind));
    Lists { groups, retired }
}

/// Where a memory came from, in words (来龙去脉). `name` reads a member id as a name.
pub fn source_text(m: &Memory, name: &dyn Fn(&str) -> String) -> String {
    let author = name(&m.author_id);
    match m.source {
        MemorySourceKind::Manual => fill(c::SOURCE_MANUAL, &[("name", &author)]),
        // A person changed what an AI wrote: say who, the change is the newest fact.
        _ if m.human && m.source != MemorySourceKind::Manual => {
            fill(c::SOURCE_EDITED, &[("name", &author)])
        }
        MemorySourceKind::Run => fill(c::SOURCE_RUN, &[("name", &author)]),
        MemorySourceKind::Message => c::SOURCE_MESSAGE.to_string(),
        MemorySourceKind::Card => c::SOURCE_CARD.to_string(),
        MemorySourceKind::Other => c::SOURCE_OTHER.to_string(),
    }
}

/// One line of a day's 记忆更新, as it shows.
#[derive(Debug, PartialEq, Eq)]
pub struct UpdateLine {
    pub id: String,
    /// 新增, 改了, 停用 or 可能过时.
    pub word: &'static str,
    pub change: MemoryChange,
    /// The memory's text after the change (before it, for a retired one).
    pub text: String,
    /// What it said before (改了), or why (the consolidation's note).
    pub detail: Option<String>,
    pub undone: bool,
    /// 撤回 shows: a writer, a change that can be undone, not undone yet.
    pub can_undo: bool,
}

pub fn update_line(u: &MemoryUpdate, writer: bool) -> UpdateLine {
    let word = match u.change {
        MemoryChange::Added => c::CHANGE_ADDED,
        MemoryChange::Changed => c::CHANGE_CHANGED,
        MemoryChange::Retired => c::CHANGE_RETIRED,
        MemoryChange::Flagged | MemoryChange::Other => c::CHANGE_FLAGGED,
    };
    let shown = match u.change {
        MemoryChange::Retired => u.before.as_ref().or(u.after.as_ref()),
        _ => u.after.as_ref().or(u.before.as_ref()),
    };
    let text = shown.map(|m| m.text.clone()).unwrap_or_default();
    let detail = match (u.change, &u.before, &u.after) {
        (MemoryChange::Changed, Some(b), Some(a)) if b.text != a.text => {
            Some(fill(c::CHANGE_BEFORE, &[("text", &b.text)]))
        }
        _ => u.note.clone(),
    };
    let undone = u.undone_at.is_some();
    let undoable = matches!(
        u.change,
        MemoryChange::Added | MemoryChange::Changed | MemoryChange::Retired
    );
    UpdateLine {
        id: u.id.clone(),
        word,
        change: u.change,
        text,
        detail,
        undone,
        can_undo: writer && undoable && !undone,
    }
}

/// `新增 2 条，改了 1 条，停用 1 条` for the lines not undone; `None` when nothing is left.
pub fn summary(day: &MemoryDay) -> Option<String> {
    let live: Vec<&MemoryUpdate> = day
        .updates
        .iter()
        .filter(|u| u.undone_at.is_none())
        .collect();
    let n = |ch: MemoryChange| live.iter().filter(|u| u.change == ch).count();
    let parts: Vec<String> = [
        (MemoryChange::Added, c::UPDATES_ADDED_N),
        (MemoryChange::Changed, c::UPDATES_CHANGED_N),
        (MemoryChange::Retired, c::UPDATES_RETIRED_N),
        (MemoryChange::Flagged, c::UPDATES_FLAGGED_N),
    ]
    .into_iter()
    .filter_map(|(ch, t)| {
        let k = n(ch);
        (k > 0).then(|| fill(t, &[("n", &k.to_string())]))
    })
    .collect();
    (!parts.is_empty()).then(|| parts.join(c::UPDATES_SUMMARY_SEP))
}

/// `今天`, `昨天` or `11月8日` for a `YYYY-MM-DD` day; the text as it is when it is not one.
pub fn day_label(day: &str, today: NaiveDate) -> String {
    let Ok(d) = NaiveDate::parse_from_str(day, "%Y-%m-%d") else {
        return day.to_string();
    };
    if d == today {
        c::DAY_TODAY.to_string()
    } else if today.pred_opt() == Some(d) {
        c::DAY_YESTERDAY.to_string()
    } else {
        fill(
            c::DAY_DATE,
            &[("m", &d.month().to_string()), ("d", &d.day().to_string())],
        )
    }
}

/// The text a sheet can save: trimmed, 1..=max characters. `Err(true)` when it is too long.
pub fn checked_text(text: &str, max: usize) -> Result<String, bool> {
    let t = text.trim();
    if t.is_empty() {
        Err(false)
    } else if t.chars().count() > max {
        Err(true)
    } else {
        Ok(t.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::MemberKind;

    fn member(id: &str, level: MemberLevel) -> Member {
        Member {
            id: id.into(),
            kind: MemberKind::Human,
            name: id.into(),
            role: None,
            level,
            avatar_url: None,
            deactivated: false,
            online: true,
            character: None,
            team_owner: false,
            owner_id: None,
        }
    }

    #[test]
    fn a_person_board_follows_the_signed_in_person() {
        let mine = MemoryScope::Person("old".into());
        assert_eq!(
            scope_for(&mine, Some("new")),
            Some(MemoryScope::Person("new".into()))
        );
        assert_eq!(scope_for(&mine, Some("old")), Some(mine.clone()));
        // Signed out, or opened before sign-in: nothing to read, never `person:`.
        assert_eq!(scope_for(&mine, None), None);
        assert_eq!(
            scope_for(&MemoryScope::Person(String::new()), Some("")),
            None
        );
        // Other boards keep their scope across an account change.
        for s in [
            MemoryScope::Team,
            MemoryScope::Member("b1".into()),
            MemoryScope::Group("g1".into()),
        ] {
            assert_eq!(scope_for(&s, Some("new")), Some(s.clone()));
            assert_eq!(scope_for(&s, None), Some(s));
        }
    }

    fn mem(id: &str, kind: MemoryKind, status: MemoryStatus, at: i64) -> Memory {
        Memory {
            id: id.into(),
            scope: "member:b1".into(),
            kind,
            text: format!("text {id}"),
            when: None,
            source: MemorySourceKind::Run,
            source_ids: Vec::new(),
            note: None,
            author_id: "b1".into(),
            human: false,
            status,
            uses: 0,
            last_used_at: None,
            version: 1,
            created_at: at,
            updated_at: at,
        }
    }

    fn upd(change: MemoryChange, before: Option<Memory>, after: Option<Memory>) -> MemoryUpdate {
        MemoryUpdate {
            id: "u1".into(),
            day: "2026-11-08".into(),
            memory_id: "m1".into(),
            change,
            before,
            after,
            note: Some("整理时合并".into()),
            undone_at: None,
            at: 1,
        }
    }

    #[test]
    fn owners_write_the_team_and_members_their_own_scope() {
        let owner = member("o", MemberLevel::Owner);
        let person = member("p", MemberLevel::Member);
        let bot = member("b1", MemberLevel::Member);
        let team = MemoryScope::Team;
        let b1 = MemoryScope::Member("b1".into());
        assert!(can_write(&team, Some(&owner)));
        assert!(!can_write(&team, Some(&person)));
        assert!(can_write(&b1, Some(&owner)));
        assert!(!can_write(&b1, Some(&person)), "others read");
        assert!(can_write(&b1, Some(&bot)), "the AI member itself writes");
        assert!(!can_write(&b1, None));
        let off = Member {
            deactivated: true,
            ..owner
        };
        assert!(!can_write(&team, Some(&off)));
    }

    #[test]
    fn group_members_write_the_group_and_only_i_write_mine() {
        let person = member("p", MemberLevel::Member);
        let owner = member("o", MemberLevel::Owner);
        let group = MemoryScope::Group("g1".into());
        assert!(can_write(&group, Some(&person)));
        assert!(!can_write(&group, None));
        let mine = MemoryScope::Person("p".into());
        assert!(can_write(&mine, Some(&person)));
        assert!(
            !can_write(&mine, Some(&owner)),
            "an owner does not write my preferences"
        );
        let off = Member {
            deactivated: true,
            ..person
        };
        assert!(!can_write(&group, Some(&off)));
        assert!(!can_write(&mine, Some(&off)));
    }

    #[test]
    fn one_person_with_ai_members_is_a_single_user() {
        let mut bot = member("b1", MemberLevel::Member);
        bot.kind = MemberKind::Bot;
        let me = member("o", MemberLevel::Owner);
        let mut gone = member("p", MemberLevel::Member);
        gone.deactivated = true;
        assert!(single_user(&[me.clone(), bot.clone(), gone.clone()]));
        let other = member("q", MemberLevel::Member);
        assert!(!single_user(&[me.clone(), bot.clone(), other]));
        assert!(!single_user(&[bot]), "nobody is not one person");
    }

    #[test]
    fn folded_preferences_are_mine_and_conventions_the_teams() {
        assert_eq!(
            folded_scope(MemoryKind::Preference, "o", true),
            MemoryScope::Person("o".into())
        );
        assert_eq!(
            folded_scope(MemoryKind::Convention, "o", true),
            MemoryScope::Team
        );
        assert_eq!(folded_scope(MemoryKind::Fact, "o", true), MemoryScope::Team);
        assert_eq!(
            folded_scope(MemoryKind::Convention, "o", false),
            MemoryScope::Person("o".into()),
            "without team rights it stays mine"
        );
    }

    #[test]
    fn merged_days_keep_the_newest_day_of_any_scope() {
        let a = mem("m1", MemoryKind::Lesson, MemoryStatus::Active, 1);
        let at = |mut u: MemoryUpdate, day: &str, at: i64, id: &str| {
            u.day = day.into();
            u.at = at;
            u.id = id.into();
            u
        };
        let team = MemoryDay {
            day: Some("2026-11-09".into()),
            updates: vec![at(
                upd(MemoryChange::Added, None, Some(a.clone())),
                "2026-11-09",
                5,
                "t1",
            )],
        };
        let mine = MemoryDay {
            day: Some("2026-11-09".into()),
            updates: vec![at(
                upd(MemoryChange::Added, None, Some(a.clone())),
                "2026-11-09",
                2,
                "p1",
            )],
        };
        let old = MemoryDay {
            day: Some("2026-11-01".into()),
            updates: vec![at(
                upd(MemoryChange::Added, None, Some(a.clone())),
                "2026-11-01",
                1,
                "o1",
            )],
        };
        let day = merge_days(vec![team, MemoryDay::default(), mine, old]);
        assert_eq!(day.day.as_deref(), Some("2026-11-09"));
        let ids: Vec<&str> = day.updates.iter().map(|u| u.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["p1", "t1"],
            "oldest first, the older day left out"
        );
        assert_eq!(
            merge_days(vec![MemoryDay::default(), MemoryDay::default()]),
            MemoryDay::default()
        );
    }

    #[test]
    fn groups_people_and_my_conventions_group_their_own_kinds() {
        let ms = vec![
            mem("f", MemoryKind::Fact, MemoryStatus::Active, 5),
            mem("p", MemoryKind::Preference, MemoryStatus::Active, 4),
            mem("c", MemoryKind::Convention, MemoryStatus::Active, 3),
            mem("l", MemoryKind::Lesson, MemoryStatus::Active, 2),
        ];
        let titles = |order: &[MemoryKind]| {
            lists_in(order, &ms)
                .groups
                .iter()
                .map(|g| g.title)
                .collect::<Vec<_>>()
        };
        let group = MemoryScope::Group("g1".into());
        let person = MemoryScope::Person("o".into());
        assert_eq!(
            titles(kind_order(&group, false)),
            vec!["约定", "事实", "其他"]
        );
        assert_eq!(
            titles(kind_order(&person, false)),
            vec!["偏好", "事实", "其他"]
        );
        assert_eq!(
            titles(kind_order(&person, true)),
            vec!["约定", "偏好", "事实", "其他"]
        );
    }

    #[test]
    fn a_member_groups_lessons_then_playbooks_then_the_rest() {
        let ms = vec![
            mem("pref", MemoryKind::Preference, MemoryStatus::Active, 5),
            mem("play", MemoryKind::Playbook, MemoryStatus::Active, 4),
            mem("old", MemoryKind::Lesson, MemoryStatus::Active, 1),
            mem("new", MemoryKind::Lesson, MemoryStatus::Pending, 9),
            mem("gone", MemoryKind::Lesson, MemoryStatus::Retired, 7),
            mem("fact", MemoryKind::Fact, MemoryStatus::Active, 3),
            mem("odd", MemoryKind::Lesson, MemoryStatus::Other, 3),
        ];
        let l = lists(&MemoryScope::Member("b1".into()), &ms);
        let titles: Vec<&str> = l.groups.iter().map(|g| g.title).collect();
        assert_eq!(titles, vec!["经验", "做法", "其他"]);
        let ids = |g: &KindGroup| g.items.iter().map(|m| m.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&l.groups[0]), vec!["new", "old"], "newest change first");
        assert_eq!(ids(&l.groups[2]), vec!["pref", "fact"]);
        assert_eq!(l.retired.len(), 1);
        assert!(!l.is_empty());
    }

    #[test]
    fn the_team_groups_conventions_then_facts() {
        let ms = vec![
            mem("f", MemoryKind::Fact, MemoryStatus::Active, 5),
            mem("c", MemoryKind::Convention, MemoryStatus::Active, 4),
            mem("l", MemoryKind::Lesson, MemoryStatus::Active, 4),
        ];
        let l = lists(&MemoryScope::Team, &ms);
        let titles: Vec<&str> = l.groups.iter().map(|g| g.title).collect();
        assert_eq!(titles, vec!["约定", "事实", "其他"]);
        assert!(lists(&MemoryScope::Team, &[]).is_empty());
    }

    #[test]
    fn update_lines_say_what_changed_and_who_can_undo() {
        let mut before = mem("m1", MemoryKind::Lesson, MemoryStatus::Active, 1);
        before.text = "构建失败就重试一次。".into();
        let after = mem("m1", MemoryKind::Lesson, MemoryStatus::Active, 2);
        let changed = update_line(
            &upd(
                MemoryChange::Changed,
                Some(before.clone()),
                Some(after.clone()),
            ),
            true,
        );
        assert_eq!(changed.word, "改了");
        assert_eq!(changed.text, "text m1");
        assert_eq!(
            changed.detail.as_deref(),
            Some("原来：构建失败就重试一次。")
        );
        assert!(changed.can_undo);

        let mut gone = before.clone();
        gone.status = MemoryStatus::Retired;
        let retired = update_line(
            &upd(MemoryChange::Retired, Some(before.clone()), Some(gone)),
            true,
        );
        assert_eq!(
            (retired.word, retired.text.as_str()),
            ("停用", "构建失败就重试一次。")
        );
        assert_eq!(retired.detail.as_deref(), Some("整理时合并"));

        let added = update_line(&upd(MemoryChange::Added, None, Some(after.clone())), false);
        assert_eq!(added.word, "新增");
        assert!(!added.can_undo, "readers cannot undo");

        let flagged = update_line(
            &upd(
                MemoryChange::Flagged,
                Some(after.clone()),
                Some(after.clone()),
            ),
            true,
        );
        assert_eq!(flagged.word, "可能过时");
        assert!(!flagged.can_undo, "nothing changed, nothing to undo");

        let mut done = upd(MemoryChange::Added, None, Some(after));
        done.undone_at = Some(5);
        let done = update_line(&done, true);
        assert!(done.undone && !done.can_undo);
    }

    #[test]
    fn the_summary_counts_what_was_not_undone() {
        let a = mem("m1", MemoryKind::Lesson, MemoryStatus::Active, 1);
        let mut day = MemoryDay {
            day: Some("2026-11-08".into()),
            updates: vec![
                upd(MemoryChange::Added, None, Some(a.clone())),
                upd(MemoryChange::Added, None, Some(a.clone())),
                upd(MemoryChange::Retired, Some(a.clone()), Some(a.clone())),
                upd(MemoryChange::Flagged, Some(a.clone()), Some(a.clone())),
            ],
        };
        assert_eq!(
            summary(&day).as_deref(),
            Some("新增 2 条，停用 1 条，1 条可能过时")
        );
        day.updates[1].undone_at = Some(3);
        assert_eq!(
            summary(&day).as_deref(),
            Some("新增 1 条，停用 1 条，1 条可能过时")
        );
        assert_eq!(summary(&MemoryDay::default()), None);
    }

    #[test]
    fn days_read_as_today_yesterday_or_a_date() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 9).unwrap();
        assert_eq!(day_label("2026-11-09", today), "今天");
        assert_eq!(day_label("2026-11-08", today), "昨天");
        assert_eq!(day_label("2026-10-31", today), "10月31日");
        assert_eq!(day_label("someday", today), "someday");
    }

    #[test]
    fn the_source_says_who_and_where() {
        let name = |id: &str| {
            if id == "b1" {
                "Forge".to_string()
            } else {
                "Joye".to_string()
            }
        };
        let mut m = mem("m", MemoryKind::Lesson, MemoryStatus::Active, 1);
        assert_eq!(source_text(&m, &name), "Forge 干活时记下");
        m.source = MemorySourceKind::Message;
        assert_eq!(source_text(&m, &name), "从群里的消息记下");
        m.source = MemorySourceKind::Card;
        assert_eq!(source_text(&m, &name), "从卡片的回答记下");
        m.human = true;
        m.author_id = "m-joye".into();
        assert_eq!(source_text(&m, &name), "Joye 改过");
        m.source = MemorySourceKind::Manual;
        assert_eq!(source_text(&m, &name), "Joye 记的");
    }

    #[test]
    fn sheet_text_is_trimmed_and_capped() {
        assert_eq!(checked_text("  先压测  ", 500), Ok("先压测".into()));
        assert_eq!(checked_text("   ", 500), Err(false));
        assert_eq!(checked_text(&"字".repeat(501), 500), Err(true));
        assert!(checked_text(&"字".repeat(500), 500).is_ok());
    }
}
