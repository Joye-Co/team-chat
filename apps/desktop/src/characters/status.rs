//! The live status a member's character shows.
//!
//! [`status_of`] is the rule; [`StatusTracker`] collects its inputs from the
//! provider's snapshots and change batches and publishes the result in the
//! [`StatusBoard`] global, which avatars read while they render.
//!
//! The rule for a bot, first match wins:
//! 1. offline: the bot is not present (not online, or deactivated);
//! 2. working: it is writing a streaming message;
//! 3. needs-input: a topic it owns waits on a person (`waiting_answer`,
//!    `waiting_review`);
//! 4. error: a topic it owns is `blocked`;
//! 5. success: for [`SUCCESS_FOR`] after its streaming reply ends or a topic
//!    it owns becomes `done`;
//! 6. idle.
//!
//! The desktop provider does not expose bot runs yet, so "working" comes from
//! streaming replies and "error" only from blocked topics. A person with a
//! character shows idle or offline.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use gpui::{App, AppContext as _, Context, Entity, Global, Subscription};

use super::CharacterStatus;
use crate::{
    app::{ChangeHub, Changes},
    data::{Change, ChatProvider, GroupId, Member, MemberId, Millis, TopicId, TopicStatus},
};

/// How long a bot shows success after a run or a topic ends.
pub const SUCCESS_FOR: Duration = Duration::from_secs(8);
/// At start, topics active this recently are checked for streaming replies.
const RECENT_MS: Millis = 60 * 60 * 1000;
/// Messages read at the end of a topic to find streaming replies.
const TAIL: usize = 4;

/// What is known about one member, the inputs of [`status_of`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Signals {
    pub bot: bool,
    pub present: bool,
    pub streaming: bool,
    pub waits_on_person: bool,
    pub blocked: bool,
    pub recent_success: bool,
}

/// The status rule (see the module docs).
pub fn status_of(s: Signals) -> CharacterStatus {
    if !s.present {
        CharacterStatus::Offline
    } else if !s.bot {
        CharacterStatus::Idle
    } else if s.streaming {
        CharacterStatus::Working
    } else if s.waits_on_person {
        CharacterStatus::NeedsInput
    } else if s.blocked {
        CharacterStatus::Error
    } else if s.recent_success {
        CharacterStatus::Success
    } else {
        CharacterStatus::Idle
    }
}

/// The status of a member for whom the tracker has no entry yet.
pub fn fallback(member: &Member) -> CharacterStatus {
    status_of(Signals {
        present: member.online && !member.deactivated,
        ..Signals::default()
    })
}

/// The statuses avatars draw. Set by [`StatusTracker`].
#[derive(Clone, Debug, Default)]
pub struct StatusBoard {
    statuses: HashMap<MemberId, CharacterStatus>,
}

impl Global for StatusBoard {}

impl StatusBoard {
    pub fn get(&self, member_id: &str) -> Option<CharacterStatus> {
        self.statuses.get(member_id).copied()
    }

    /// The status of a member: the board's entry, else [`fallback`].
    pub fn status(cx: &App, member: &Member) -> CharacterStatus {
        cx.try_global::<StatusBoard>()
            .and_then(|b| b.get(&member.id))
            .unwrap_or_else(|| fallback(member))
    }
}

/// A topic owned by a member, as far as the status needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Owned {
    topic_id: TopicId,
    owner: MemberId,
    status: TopicStatus,
}

/// Collects the inputs of [`status_of`] from the provider and keeps the
/// [`StatusBoard`] current.
pub struct StatusTracker {
    provider: Arc<dyn ChatProvider>,
    owned: HashMap<GroupId, Vec<Owned>>,
    streaming: HashMap<(GroupId, TopicId), HashSet<MemberId>>,
    success_until: HashMap<MemberId, Instant>,
    _changes: Subscription,
}

impl StatusTracker {
    /// Starts tracking and sets the [`StatusBoard`] global.
    pub fn start(
        provider: &Arc<dyn ChatProvider>,
        hub: &Entity<ChangeHub>,
        cx: &mut App,
    ) -> Entity<StatusTracker> {
        let provider = provider.clone();
        let hub = hub.clone();
        cx.new(|cx| {
            let changes = cx.subscribe(&hub, |this: &mut Self, _, changes: &Changes, cx| {
                this.apply(changes, cx)
            });
            let mut this = StatusTracker {
                provider,
                owned: HashMap::new(),
                streaming: HashMap::new(),
                success_until: HashMap::new(),
                _changes: changes,
            };
            this.scan_all();
            this.publish(cx);
            this
        })
    }

    fn scan_all(&mut self) {
        let now = chrono::Utc::now().timestamp_millis();
        for group in self.provider.groups() {
            let topics = self.provider.topics(&group.id);
            for t in &topics {
                if now - t.last_activity_at <= RECENT_MS {
                    self.read_streaming(&group.id, &t.id);
                }
            }
            self.owned.insert(group.id.clone(), owned_of(&topics));
        }
    }

    fn read_streaming(&mut self, group_id: &str, topic_id: &str) -> HashSet<MemberId> {
        let window = self.provider.messages(group_id, topic_id, TAIL);
        let now: HashSet<MemberId> = window
            .messages
            .iter()
            .filter(|m| m.streaming && m.deleted_at.is_none())
            .map(|m| m.author_id.clone())
            .collect();
        let key = (group_id.to_string(), topic_id.to_string());
        let before = if now.is_empty() {
            self.streaming.remove(&key)
        } else {
            self.streaming.insert(key, now.clone())
        };
        before.unwrap_or_default()
    }

    fn apply(&mut self, changes: &Changes, cx: &mut Context<Self>) {
        let mut ended: Vec<MemberId> = Vec::new();
        for change in &changes.0 {
            match change {
                Change::Messages { group_id, topic_id } => {
                    let before = self.read_streaming(group_id, topic_id);
                    let key = (group_id.clone(), topic_id.clone());
                    let after = self.streaming.get(&key).cloned().unwrap_or_default();
                    ended.extend(before.difference(&after).cloned());
                }
                Change::Topics { group_id } => {
                    let next = owned_of(&self.provider.topics(group_id));
                    let before = self.owned.insert(group_id.clone(), next.clone());
                    ended.extend(newly_done(before.as_deref().unwrap_or(&[]), &next));
                }
                Change::Groups | Change::Session => {
                    for group in self.provider.groups() {
                        if !self.owned.contains_key(&group.id) {
                            let topics = self.provider.topics(&group.id);
                            self.owned.insert(group.id.clone(), owned_of(&topics));
                        }
                    }
                }
                _ => {}
            }
        }
        if !ended.is_empty() {
            let until = cx.background_executor().now() + SUCCESS_FOR;
            for id in ended {
                self.success_until.insert(id, until);
            }
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(SUCCESS_FOR).await;
                this.update(cx, |this, cx| this.publish(cx)).ok();
            })
            .detach();
        }
        self.publish(cx);
    }

    /// The signals of every member with a character or a bot.
    fn signals(&self, members: &[Member], now: Instant) -> HashMap<MemberId, Signals> {
        let mut out: HashMap<MemberId, Signals> = members
            .iter()
            .map(|m| {
                let s = Signals {
                    bot: m.is_bot(),
                    present: m.online && !m.deactivated,
                    recent_success: self.success_until.get(&m.id).is_some_and(|&t| t > now),
                    ..Signals::default()
                };
                (m.id.clone(), s)
            })
            .collect();
        for bots in self.streaming.values() {
            for id in bots {
                if let Some(s) = out.get_mut(id) {
                    s.streaming = true;
                }
            }
        }
        for topic in self.owned.values().flatten() {
            if let Some(s) = out.get_mut(&topic.owner) {
                match topic.status {
                    TopicStatus::WaitingAnswer | TopicStatus::WaitingReview => {
                        s.waits_on_person = true
                    }
                    TopicStatus::Blocked => s.blocked = true,
                    _ => {}
                }
            }
        }
        out
    }

    fn publish(&mut self, cx: &mut Context<Self>) {
        // The executor's clock, so tests can move it.
        let now = cx.background_executor().now();
        self.success_until.retain(|_, t| *t > now);
        let members = self.provider.members();
        let statuses: HashMap<MemberId, CharacterStatus> = self
            .signals(&members, now)
            .into_iter()
            .map(|(id, s)| (id, status_of(s)))
            .collect();
        let changed = cx
            .try_global::<StatusBoard>()
            .is_none_or(|b| b.statuses != statuses);
        if changed {
            cx.set_global(StatusBoard { statuses });
            cx.refresh_windows();
        }
    }
}

fn owned_of(topics: &[crate::data::Topic]) -> Vec<Owned> {
    topics
        .iter()
        .filter_map(|t| {
            t.owner_id.as_ref().map(|owner| Owned {
                topic_id: t.id.clone(),
                owner: owner.clone(),
                status: t.status,
            })
        })
        .collect()
}

/// Owners of topics that became `done` between two snapshots of a group.
fn newly_done(before: &[Owned], after: &[Owned]) -> Vec<MemberId> {
    after
        .iter()
        .filter(|t| t.status == TopicStatus::Done)
        .filter(|t| {
            before
                .iter()
                .any(|b| b.topic_id == t.topic_id && b.status != TopicStatus::Done)
        })
        .map(|t| t.owner.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;

    use super::*;
    use crate::{app::Services, fake::FakeProvider};

    #[gpui::test]
    fn the_fake_roster_shows_live_statuses(cx: &mut TestAppContext) {
        use CharacterStatus::*;
        let provider = FakeProvider::new();
        let dyn_provider: Arc<dyn ChatProvider> = provider.clone();
        let _services = cx.update(|cx| Services::new(dyn_provider, cx));
        cx.run_until_parked();
        let status =
            |cx: &mut TestAppContext, id: &str| cx.read(|cx| cx.global::<StatusBoard>().get(id));
        // Pocket streams the release notes; Forge owns a topic waiting for a
        // review; Jojo and Watch own blocked topics; Ledger is offline.
        assert_eq!(status(cx, "m-docs"), Some(Working));
        assert_eq!(status(cx, "m-coder"), Some(NeedsInput));
        assert_eq!(status(cx, "m-deploy"), Some(Error));
        assert_eq!(status(cx, "m-ledger"), Some(Offline));
        assert_eq!(status(cx, "m-sentry"), Some(Error));
        assert_eq!(status(cx, "m-tata"), Some(Idle));
        assert_eq!(status(cx, "m-reviewer"), Some(Idle));
        // People show presence only.
        assert_eq!(status(cx, crate::fake::ME), Some(Idle));
        assert_eq!(status(cx, "m-lixue"), Some(Offline));

        // Watch's blocked topic becomes done: success for a while, then idle.
        provider.set_topic_status("g-ops", "t-alert", TopicStatus::Done, vec![]);
        cx.run_until_parked();
        assert_eq!(status(cx, "m-sentry"), Some(Success));
        cx.executor()
            .advance_clock(SUCCESS_FOR + Duration::from_millis(100));
        cx.run_until_parked();
        assert_eq!(status(cx, "m-sentry"), Some(Idle));

        // Jojo's blocked topic waits on a person now: needs-input wins over error.
        provider.set_topic_status(
            "g-dev",
            "t-blocked",
            TopicStatus::WaitingAnswer,
            vec![crate::fake::ME.into()],
        );
        cx.run_until_parked();
        assert_eq!(status(cx, "m-deploy"), Some(NeedsInput));
    }

    fn bot() -> Signals {
        Signals {
            bot: true,
            present: true,
            ..Signals::default()
        }
    }

    #[test]
    fn the_rule_picks_the_first_match() {
        use CharacterStatus::*;
        assert_eq!(status_of(bot()), Idle);
        let all = Signals {
            streaming: true,
            waits_on_person: true,
            blocked: true,
            recent_success: true,
            ..bot()
        };
        assert_eq!(
            status_of(Signals {
                present: false,
                ..all
            }),
            Offline
        );
        assert_eq!(status_of(all), Working);
        assert_eq!(
            status_of(Signals {
                streaming: false,
                ..all
            }),
            NeedsInput
        );
        let s = Signals {
            streaming: false,
            waits_on_person: false,
            ..all
        };
        assert_eq!(status_of(s), Error);
        assert_eq!(
            status_of(Signals {
                blocked: false,
                ..s
            }),
            Success
        );
        // People show presence only.
        assert_eq!(status_of(Signals { bot: false, ..all }), Idle);
    }

    #[test]
    fn done_transitions_name_the_owner() {
        let t = |id: &str, owner: &str, status| Owned {
            topic_id: id.into(),
            owner: owner.into(),
            status,
        };
        let before = [
            t("a", "b1", TopicStatus::InProgress),
            t("b", "b2", TopicStatus::Done),
        ];
        let after = [
            t("a", "b1", TopicStatus::Done),
            t("b", "b2", TopicStatus::Done),
            t("c", "b3", TopicStatus::Done),
        ];
        assert_eq!(newly_done(&before, &after), vec!["b1".to_string()]);
    }
}
