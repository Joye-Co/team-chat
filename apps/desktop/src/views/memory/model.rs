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
/// owners; an AI member's memories by owners and that member).
pub fn can_write(scope: &MemoryScope, me: Option<&Member>) -> bool {
    let Some(me) = me.filter(|m| !m.deactivated) else {
        return false;
    };
    match scope {
        MemoryScope::Team => me.level == MemberLevel::Owner,
        MemoryScope::Member(id) => me.level == MemberLevel::Owner || me.id == *id,
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

/// The kind groups of a scope, in the order they show. An AI member: 经验, 做法, then 其他
/// (偏好, 事实, 约定 …). The team: 约定, 事实, then 其他.
fn group_of(scope: &MemoryScope, kind: MemoryKind) -> MemoryKind {
    let own: &[MemoryKind] = match scope {
        MemoryScope::Member(_) => &[MemoryKind::Lesson, MemoryKind::Playbook],
        MemoryScope::Team => &[MemoryKind::Convention, MemoryKind::Fact],
    };
    if own.contains(&kind) {
        kind
    } else {
        MemoryKind::Other
    }
}

fn group_rank(scope: &MemoryScope, group: MemoryKind) -> usize {
    let order: &[MemoryKind] = match scope {
        MemoryScope::Member(_) => &[MemoryKind::Lesson, MemoryKind::Playbook],
        MemoryScope::Team => &[MemoryKind::Convention, MemoryKind::Fact],
    };
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
    let mut groups: Vec<KindGroup<'a>> = Vec::new();
    let mut retired = Vec::new();
    let mut sorted: Vec<&Memory> = memories.iter().collect();
    sorted.sort_by_key(|m| std::cmp::Reverse(m.updated_at));
    for m in sorted {
        match m.status {
            MemoryStatus::Retired => retired.push(m),
            MemoryStatus::Active | MemoryStatus::Pending => {
                let g = group_of(scope, m.kind);
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
    groups.sort_by_key(|g| group_rank(scope, g.kind));
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
