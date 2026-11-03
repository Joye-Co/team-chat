//! What the rail lists, computed from the synced cache (no view code): the
//! state of a 话题 row, the 我参与的 filter, the buckets of mode B, the
//! member rows of mode C and the team strip.

use std::collections::HashSet;

use crate::{
    copy::{fill, rail as c},
    data::{ChatProvider, GroupId, GroupKind, Member, MemberId, PlanState, Topic, TopicStatus},
};

/// What the slot at the start of a 话题 row shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowState {
    /// Amber dot: needs me.
    NeedsYou,
    /// The doing member's tile with a breathing ring.
    Working(MemberId),
    /// A check: done.
    Done,
    /// A hollow ring: waits on someone else.
    Waiting,
    Idle,
}

impl RowState {
    /// Rows that need nothing from anyone now read quieter (`ink-2`).
    pub fn quiet(&self) -> bool {
        matches!(self, RowState::Done | RowState::Waiting | RowState::Idle)
    }
}

/// The trailing text of a 话题 row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meta {
    pub text: String,
    /// Numerals (`3/5`, `09:12`) use the mono face.
    pub mono: bool,
}

impl Meta {
    fn mono(text: String) -> Self {
        Self { text, mono: true }
    }
    fn plain(text: String) -> Self {
        Self { text, mono: false }
    }
}

/// `3/5`: the step a run works on of its plan.
pub fn plan_meta(plan: &[crate::data::PlanItem]) -> Option<String> {
    if plan.is_empty() {
        return None;
    }
    let done = plan.iter().filter(|i| i.state == PlanState::Done).count();
    Some(format!("{}/{}", (done + 1).min(plan.len()), plan.len()))
}

/// The state of a 话题 row and its trailing meta (`3/5`, `09:12`, `等小王`).
pub fn row_state(
    p: &dyn ChatProvider,
    me: Option<&str>,
    topic: &Topic,
) -> (RowState, Option<Meta>) {
    if me.is_some_and(|me| topic.waiting_on.iter().any(|m| m == me)) {
        return (RowState::NeedsYou, None);
    }
    let runs = p.runs(&topic.group_id, &topic.id);
    if let Some(run) = runs.iter().find(|r| r.status.is_active()) {
        return (
            RowState::Working(run.bot_id.clone()),
            plan_meta(&run.plan).map(Meta::mono),
        );
    }
    match topic.status {
        TopicStatus::Done => (
            RowState::Done,
            Some(Meta::mono(crate::views::shell::clock(
                topic.last_activity_at,
            ))),
        ),
        TopicStatus::WaitingAnswer | TopicStatus::WaitingReview => {
            let who = topic
                .waiting_on
                .first()
                .and_then(|m| p.member(m))
                .map(|m| Meta::plain(fill(c::RAIL_WAITING_ON, &[("name", &m.name)])));
            (RowState::Waiting, who)
        }
        _ => (RowState::Idle, None),
    }
}

/// 我参与的: I started it, own it, follow it, it waits on me, or I asked a
/// member to work on it.
pub fn involves_me(p: &dyn ChatProvider, me: Option<&str>, topic: &Topic) -> bool {
    let Some(me) = me else {
        return true;
    };
    topic.created_by == me
        || topic.owner_id.as_deref() == Some(me)
        || topic.waiting_on.iter().any(|m| m == me)
        || p.topic_following(&topic.group_id, &topic.id)
        || p.runs(&topic.group_id, &topic.id)
            .iter()
            .any(|r| r.requested_by.as_deref() == Some(me))
}

/// A 话题 the rail lists at all: not a new empty one.
pub fn listed(topic: &Topic) -> bool {
    topic.status != TopicStatus::Open || topic.message_count > 0
}

/// Local midnight in milliseconds.
pub fn midnight() -> crate::data::Millis {
    chrono::Local::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|t| t.and_local_timezone(chrono::Local).single())
        .map(|t| t.timestamp_millis())
        .unwrap_or(0)
}

/// The buckets of mode B, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Bucket {
    NeedsYou,
    Working,
    Waiting,
    DoneToday,
}

impl Bucket {
    pub const ALL: [Bucket; 4] = [
        Bucket::NeedsYou,
        Bucket::Working,
        Bucket::Waiting,
        Bucket::DoneToday,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Bucket::NeedsYou => c::RAIL_NEEDS_YOU,
            Bucket::Working => c::RAIL_WORKING,
            Bucket::Waiting => c::RAIL_WAITING,
            Bucket::DoneToday => c::RAIL_DONE_TODAY,
        }
    }
}

/// Mode B: the 话题 of each bucket, optionally of one 群 only.
pub fn buckets(
    p: &dyn ChatProvider,
    me: Option<&str>,
    group: Option<&str>,
) -> Vec<(Bucket, Vec<Topic>)> {
    let midnight = midnight();
    let mut out: Vec<(Bucket, Vec<Topic>)> = Bucket::ALL.iter().map(|b| (*b, Vec::new())).collect();
    for g in p.groups() {
        if group.is_some_and(|f| f != g.id) {
            continue;
        }
        for t in p.topics(&g.id).into_iter().filter(listed) {
            let bucket = match row_state(p, me, &t).0 {
                RowState::NeedsYou => Bucket::NeedsYou,
                RowState::Working(_) => Bucket::Working,
                RowState::Waiting => Bucket::Waiting,
                RowState::Done if t.last_activity_at >= midnight => Bucket::DoneToday,
                _ => continue,
            };
            if let Some((_, list)) = out.iter_mut().find(|(b, _)| *b == bucket) {
                list.push(t);
            }
        }
    }
    for (_, list) in &mut out {
        list.sort_by_key(|t| std::cmp::Reverse(t.last_activity_at));
    }
    out
}

/// One 话题 under a member in mode C.
#[derive(Clone, Debug)]
pub struct MemberTopic {
    pub topic: Topic,
    pub state: RowState,
    pub meta: Option<Meta>,
}

/// A member row of mode C.
#[derive(Clone, Debug)]
pub struct MemberRow {
    pub member: Member,
    pub topics: Vec<MemberTopic>,
    /// 话题 of this member that wait on me.
    pub waiting_on_me: usize,
    /// Works now (an active run).
    pub working: bool,
    /// The plan step of the newest active run (`2/4`).
    pub plan: Option<String>,
    /// When the member last did something, for 先看在忙的.
    pub last: crate::data::Millis,
}

impl MemberRow {
    pub fn busy(&self) -> bool {
        !self.topics.is_empty()
    }
}

/// How mode C orders its members.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MemberSort {
    /// Waiting on me first, then working, then the rest (default).
    #[default]
    Busy,
    /// The team's roster order.
    Roster,
}

/// Mode C: every member but me, each with the 话题 it works on, waits on me
/// with, or that wait on it.
pub fn member_rows(p: &dyn ChatProvider, me: Option<&str>, sort: MemberSort) -> Vec<MemberRow> {
    let topics: Vec<Topic> = p
        .groups()
        .iter()
        .flat_map(|g| p.topics(&g.id))
        .filter(listed)
        .collect();
    let runs = p.active_runs();
    let mut rows: Vec<MemberRow> = p
        .members()
        .into_iter()
        .filter(|m| !m.deactivated && Some(m.id.as_str()) != me)
        .map(|m| {
            let mut out: Vec<MemberTopic> = Vec::new();
            let mut seen = HashSet::new();
            let mut last = 0;
            // Working on it now.
            let mine: Vec<_> = runs.iter().filter(|r| r.bot_id == m.id).collect();
            for r in &mine {
                if let Some(t) = topics.iter().find(|t| t.id == r.topic_id)
                    && seen.insert(t.id.clone())
                {
                    last = last.max(r.updated_at);
                    out.push(MemberTopic {
                        topic: t.clone(),
                        state: RowState::Working(m.id.clone()),
                        meta: plan_meta(&r.plan).map(Meta::mono),
                    });
                }
            }
            // Its 话题 that wait on me.
            let mut waiting_on_me = 0;
            for t in topics.iter().filter(|t| {
                t.owner_id.as_deref() == Some(m.id.as_str())
                    && me.is_some_and(|me| t.waiting_on.iter().any(|w| w == me))
            }) {
                if seen.insert(t.id.clone()) {
                    waiting_on_me += 1;
                    last = last.max(t.last_activity_at);
                    out.push(MemberTopic {
                        topic: t.clone(),
                        state: RowState::NeedsYou,
                        meta: Some(Meta::plain(c::RAIL_WAITS_ON_YOU.into())),
                    });
                }
            }
            // 话题 that wait on this member.
            for t in topics
                .iter()
                .filter(|t| t.waiting_on.iter().any(|w| w == &m.id))
            {
                if seen.insert(t.id.clone()) {
                    last = last.max(t.last_activity_at);
                    out.push(MemberTopic {
                        topic: t.clone(),
                        state: RowState::Waiting,
                        meta: Some(Meta::plain(fill(c::RAIL_WAITING_ON, &[("name", &m.name)]))),
                    });
                }
            }
            MemberRow {
                plan: mine.first().and_then(|r| plan_meta(&r.plan)),
                working: !mine.is_empty(),
                waiting_on_me,
                topics: out,
                last,
                member: m,
            }
        })
        .collect();
    if sort == MemberSort::Busy {
        // Stable: equal rows keep the roster order.
        rows.sort_by_key(|r| {
            (
                std::cmp::Reverse(r.waiting_on_me),
                !r.working,
                std::cmp::Reverse(if r.working { r.last } else { 0 }),
            )
        });
    }
    rows
}

/// The team strip: who waits on me (amber ring), then who works (blue
/// ring), at most `max`, and how many members are not shown.
pub fn team_strip(
    p: &dyn ChatProvider,
    me: Option<&str>,
    max: usize,
) -> (Vec<(Member, bool)>, usize) {
    let members = p.members();
    let mut ids: Vec<(MemberId, bool)> = Vec::new();
    let push = |id: &str, you: bool, ids: &mut Vec<(MemberId, bool)>| {
        if Some(id) != me && !ids.iter().any(|(m, _)| m == id) {
            ids.push((id.to_string(), you));
        }
    };
    // A member that works shows the live ring, even when it also waits on me.
    let runs = p.active_runs();
    for a in p.asks_for_me() {
        if !runs.iter().any(|r| r.bot_id == a.from) {
            push(&a.from, true, &mut ids);
        }
    }
    for r in &runs {
        push(&r.bot_id, false, &mut ids);
    }
    let shown: Vec<(Member, bool)> = ids
        .into_iter()
        .filter_map(|(id, you)| {
            members
                .iter()
                .find(|m| m.id == id)
                .map(|m| (m.clone(), you))
        })
        .filter(|(m, _)| !m.deactivated)
        .take(max)
        .collect();
    let total = members.iter().filter(|m| !m.deactivated).count();
    let rest = total.saturating_sub(shown.len());
    (shown, rest)
}

/// The 群 of the rail in order (mode A sections, mode B chips).
pub fn groups(p: &dyn ChatProvider) -> Vec<(GroupId, String)> {
    p.groups()
        .into_iter()
        .filter(|g| g.kind == GroupKind::Group)
        .map(|g| (g.id, g.name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::v2::{self as d, V2Samples};

    #[test]
    fn row_states_follow_the_artboard() {
        let p = V2Samples::new();
        let state = |t: &str| {
            let topic = p
                .groups()
                .iter()
                .flat_map(|g| p.topics(&g.id))
                .find(|x| x.id == t)
                .unwrap();
            row_state(p.as_ref(), Some(d::ME), &topic)
        };
        assert_eq!(state(d::T_RECONNECT).0, RowState::NeedsYou);
        let (s, meta) = state(d::T_VIRTUAL);
        assert_eq!(s, RowState::Working(d::FORGE.into()));
        assert!(meta.unwrap().mono);
        let (s, meta) = state(d::T_SEARCH);
        assert_eq!(s, RowState::Waiting);
        assert_eq!(meta.unwrap().text, "等小王");
        assert_eq!(state(d::T_STAGING).0, RowState::Done);
    }

    #[test]
    fn attention_buckets_hold_each_topic_once() {
        let p = V2Samples::new();
        let b = buckets(p.as_ref(), Some(d::ME), None);
        let count = |k: Bucket| b.iter().find(|(x, _)| *x == k).unwrap().1.len();
        assert_eq!(count(Bucket::NeedsYou), 4);
        assert_eq!(count(Bucket::Working), 3);
        assert_eq!(count(Bucket::Waiting), 1);
        let dev = buckets(p.as_ref(), Some(d::ME), Some(d::DEV));
        assert!(
            dev.iter()
                .flat_map(|(_, l)| l)
                .all(|t| t.group_id == d::DEV)
        );
    }

    #[test]
    fn busy_members_come_first_and_i_am_not_listed() {
        let p = V2Samples::new();
        let rows = member_rows(p.as_ref(), Some(d::ME), MemberSort::Busy);
        assert!(rows.iter().all(|r| r.member.id != d::ME));
        assert_eq!(rows[0].member.id, d::FORGE);
        assert_eq!(rows[0].waiting_on_me, 2);
        let xiaowang = rows.iter().find(|r| r.member.id == d::XIAOWANG).unwrap();
        assert_eq!(xiaowang.topics.len(), 1);
        assert_eq!(xiaowang.topics[0].state, RowState::Waiting);
        // Busy rows stay together at the top.
        let first_idle = rows.iter().position(|r| !r.busy()).unwrap();
        assert!(
            rows[first_idle..]
                .iter()
                .all(|r| !r.busy() || !r.member.is_bot())
        );
    }

    #[test]
    fn team_strip_shows_waiting_then_working_and_counts_the_rest() {
        let p = V2Samples::new();
        let (shown, rest) = team_strip(p.as_ref(), Some(d::ME), 5);
        assert_eq!(shown.len(), 5);
        assert!(shown[0].1, "an amber ring first");
        assert_eq!(rest, p.members().len() - 5);
    }
}
