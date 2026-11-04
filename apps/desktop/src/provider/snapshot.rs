//! Aggregates of the 首页, 成员, 群 and 产出 pages for [`super::CoreProvider`], read from the
//! chat-core cache once and kept until a change: this week's runs (one `runs_since` query),
//! the 产出 of each 群 (one query per 群), the open asks for me (one indexed query) and the
//! results of today. The pages call these getters on every frame; a getter only clones a
//! part of the snapshot. Any change other than typing, run log steps, drafts and the
//! connection marks the snapshot stale; it is also rebuilt after [`TTL`] (the week and the
//! day move on). The machine list and the bots' runner settings are fetched in the
//! background (60 s apart at most) and their arrival rebuilds the snapshot.

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use chat_core::proto::{self, MemberLevel};

use super::{CoreProvider, desk::kind_of_v2, files::artifact_summary, lock};
use crate::data::{
    ArtifactSummary, BotRunSummary, Change, ChatProvider as _, GroupId, Millis, TodoReason, Topic,
    TopicStatus,
    pages::{
        Delivery, DeliveryKind, DeliveryState, MemberExtras, MemberTool, QueuedWork, RecentDone,
    },
    v2::{
        AskItem, AskKind, ChangeState, Currency, DoneItem, MemberMachine, MemberProblem,
        MemberStats,
    },
};

/// A snapshot older than this is rebuilt even without a change.
const TTL: Duration = Duration::from_secs(30);
/// The machine list and runner settings are fetched again at most this often.
const REMOTE_REFRESH: Duration = Duration::from_secs(60);
const WEEK_MS: i64 = 7 * 24 * 3_600_000;
/// Newest card asks read for 需要你拍板.
const ASKS_SCANNED: usize = 500;
/// Messages of a waiting 话题 searched for a v1 card with buttons.
const V1_ASK_SCAN: usize = 30;
/// 最近的问题 kept per member.
const PROBLEMS_KEPT: usize = 3;
/// 最近交付 kept per member.
const DELIVERIES_KEPT: usize = 5;

/// The cached aggregates.
#[derive(Default)]
pub(super) struct SnapshotCache {
    /// Bumped by every change that can move an aggregate.
    stamp: AtomicU64,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    generation: u64,
    built: Option<Built>,
    machines: Option<Vec<proto::Machine>>,
    machines_at: Option<Instant>,
    machines_loading: bool,
    /// Runner settings per bot (`None`: not runner-backed); owners only.
    runners: HashMap<String, (Instant, Option<proto::BotRunner>)>,
    runners_loading: HashSet<String>,
}

struct Built {
    stamp: u64,
    at: Instant,
    snapshot: Arc<Snapshot>,
}

impl Inner {
    /// Forgets everything of an older account.
    fn for_generation(&mut self, generation: u64) -> &mut Self {
        if self.generation != generation {
            *self = Inner {
                generation,
                ..Inner::default()
            };
        }
        self
    }
}

/// One build of the aggregates.
#[derive(Default)]
pub(super) struct Snapshot {
    pub stats: HashMap<String, MemberStats>,
    pub extras: HashMap<String, MemberExtras>,
    pub artifacts: Vec<(GroupId, ArtifactSummary)>,
    pub done_today: Vec<DoneItem>,
    pub recent_done: HashMap<GroupId, Vec<RecentDone>>,
    pub asks: Vec<AskItem>,
    pub active_runs: Vec<BotRunSummary>,
}

impl SnapshotCache {
    /// Marks the snapshot stale when a change can move an aggregate.
    pub(super) fn note(&self, changes: &[Change]) {
        let moves = changes.iter().any(|c| {
            !matches!(
                c,
                Change::Typing { .. }
                    | Change::RunLog { .. }
                    | Change::Draft { .. }
                    | Change::Connection
            )
        });
        if moves {
            self.stamp.fetch_add(1, Ordering::SeqCst);
        }
    }
}

/// `owner/name` of a runner's repository, from its URL or its path.
fn repo_name(repo: &proto::RunnerRepo) -> Option<String> {
    if let Some(url) = repo.url.as_deref() {
        let trimmed = url.trim_end_matches('/').trim_end_matches(".git");
        let parts: Vec<&str> = trimmed
            .rsplit(['/', ':'])
            .filter(|s| !s.is_empty())
            .take(2)
            .collect();
        if parts.len() == 2 {
            return Some(format!("{}/{}", parts[1], parts[0]));
        }
    }
    repo.path
        .as_deref()
        .map(|p| p.trim_end_matches('/'))
        .and_then(|p| p.rsplit('/').next())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Cents of a run's reported cost (`costUsd`), rounded.
fn run_cost_cents(r: &proto::BotRun) -> Option<u64> {
    let usd = r.usage.as_ref()?.cost_usd()?;
    (usd.is_finite() && usd >= 0.).then(|| (usd * 100.).round() as u64)
}

fn ms(v: u64) -> Millis {
    v.min(i64::MAX as u64) as Millis
}

/// Local midnight of today, in ms.
fn midnight_ms() -> Millis {
    crate::selftest::clock::local_now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|t| t.and_local_timezone(chrono::Local).single())
        .map(|t| t.timestamp_millis())
        .unwrap_or(0)
}

impl CoreProvider {
    /// The current aggregates; rebuilt when stale.
    pub(super) fn snapshot(&self) -> Arc<Snapshot> {
        let generation = self.state().generation;
        let stamp = self.shared.snapshots.stamp.load(Ordering::SeqCst);
        {
            let mut cache = lock(&self.shared.snapshots.inner);
            let inner = cache.for_generation(generation);
            if let Some(b) = &inner.built
                && b.stamp == stamp
                && b.at.elapsed() < TTL
            {
                return b.snapshot.clone();
            }
        }
        // Built without the lock: the build reads other caches and may start fetches.
        let snapshot = Arc::new(self.build_snapshot());
        let mut cache = lock(&self.shared.snapshots.inner);
        let inner = cache.for_generation(generation);
        inner.built = Some(Built {
            stamp,
            at: Instant::now(),
            snapshot: snapshot.clone(),
        });
        snapshot
    }

    fn build_snapshot(&self) -> Snapshot {
        let Some(a) = self.account() else {
            return Snapshot::default();
        };
        let now = self.clock_now();
        let week_ago = now - WEEK_MS;
        let midnight = midnight_ms();
        let me = a.me.id.clone();
        let owner = a.me.level == MemberLevel::Owner;

        let groups = self.groups();
        let mut topics: HashMap<String, Topic> = HashMap::new();
        for g in &groups {
            for t in self.topics(&g.id) {
                topics.insert(t.id.clone(), t);
            }
        }
        let group_ids: HashSet<&str> = groups.iter().map(|g| g.id.as_str()).collect();
        let visible = |r: &proto::BotRun| !r.is_aside() && group_ids.contains(r.group_id.as_str());
        let week_runs: Vec<proto::BotRun> = a
            .client
            .runs_since(week_ago.max(0) as u64)
            .unwrap_or_default()
            .into_iter()
            .filter(visible)
            .collect();
        // 排队: runs that wait for a free machine are not 正在做; they go to the queue.
        let (queued, active): (Vec<proto::BotRun>, Vec<proto::BotRun>) = a
            .client
            .active_runs()
            .unwrap_or_default()
            .into_iter()
            .filter(visible)
            .partition(|r| r.is_queued());

        let ask_messages: Vec<proto::Message> = a
            .client
            .ask_messages(ASKS_SCANNED)
            .unwrap_or_default()
            .into_iter()
            .filter(|m| group_ids.contains(m.group_id.as_str()))
            .collect();
        // 话题 beyond the newest ones of a 群 that a run or an ask is in.
        let wanted: Vec<(String, String)> = week_runs
            .iter()
            .chain(active.iter())
            .chain(queued.iter())
            .map(|r| (r.group_id.clone(), r.topic_id.clone()))
            .chain(
                ask_messages
                    .iter()
                    .map(|m| (m.group_id.clone(), m.topic_id.clone())),
            )
            .collect();
        for (g, t) in wanted {
            if !topics.contains_key(&t)
                && let Some(topic) = self.topic(&g, &t)
            {
                topics.insert(t, topic);
            }
        }

        let mut snap = Snapshot::default();

        // 正在做: the run summaries (with machine names) of the 话题 with an active run.
        let mut seen = HashSet::new();
        let queued_ids: HashSet<&str> = queued.iter().map(|r| r.id.as_str()).collect();
        for r in &active {
            if seen.insert((r.group_id.clone(), r.topic_id.clone())) {
                snap.active_runs.extend(
                    self.runs_inner(&r.group_id, &r.topic_id)
                        .into_iter()
                        .filter(|s| s.status.is_active() && !queued_ids.contains(s.id.as_str())),
                );
            }
        }
        snap.active_runs
            .sort_by_key(|r| std::cmp::Reverse(r.updated_at));

        // 产出 of every 群.
        for g in &groups {
            for art in a.client.store().group_artifacts(&g.id).unwrap_or_default() {
                snap.artifacts
                    .push((g.id.clone(), artifact_summary(&art, &art.topic_id)));
            }
        }
        snap.artifacts
            .sort_by_key(|(_, x)| std::cmp::Reverse(x.updated_at));

        // Results: done runs with a result message, the newest per 话题.
        let mut results: HashMap<String, &proto::BotRun> = HashMap::new();
        for r in &week_runs {
            if r.status != proto::RunStatus::Done || r.result_message_id.is_none() {
                continue;
            }
            let Some(end) = r.ended_at else { continue };
            let newer = results
                .get(&r.topic_id)
                .is_none_or(|x| x.ended_at.unwrap_or(0) < end);
            if newer {
                results.insert(r.topic_id.clone(), r);
            }
        }
        for r in results.values() {
            let Some(t) = topics.get(&r.topic_id) else {
                continue;
            };
            let result = r.result_message_id.as_deref().unwrap_or_default();
            let card_title = a
                .client
                .store()
                .message(result)
                .ok()
                .flatten()
                .filter(|m| !m.is_deleted())
                .and_then(|m| m.card_v2.map(|c| c.title))
                .filter(|s| !s.trim().is_empty());
            let at = ms(r.ended_at.unwrap_or(r.updated_at));
            if at >= week_ago {
                snap.recent_done
                    .entry(t.group_id.clone())
                    .or_default()
                    .push(RecentDone {
                        group_id: t.group_id.clone(),
                        topic_id: t.id.clone(),
                        member_id: r.bot_id.clone(),
                        title: t.title.clone(),
                        note: card_title.clone().filter(|c| *c != t.title),
                        at,
                    });
            }
            if at >= midnight {
                snap.done_today.push(DoneItem {
                    group_id: t.group_id.clone(),
                    topic_id: t.id.clone(),
                    member_id: r.bot_id.clone(),
                    title: card_title.unwrap_or_else(|| t.title.clone()),
                    at,
                    proof: self.message_proof_inner(result),
                });
            }
        }
        snap.done_today.sort_by_key(|d| std::cmp::Reverse(d.at));
        for list in snap.recent_done.values_mut() {
            list.sort_by_key(|d| std::cmp::Reverse(d.at));
        }

        snap.asks = self.open_asks(&a.client, ask_messages, &me, owner, now, &topics);

        // 群里的决定 once per 群, for 用到的决定 of each member's 话题.
        let mut decisions_by_topic: HashMap<String, Vec<crate::data::v2::Decision>> =
            HashMap::new();
        for g in &groups {
            for d in self.group_decisions_inner(&g.id) {
                if let Some(t) = d.topic_id.clone() {
                    decisions_by_topic.entry(t).or_default().push(d);
                }
            }
        }

        // 成员: this week per member.
        let machines = self.remote_machines(&a.client);
        let members = self.members();
        // 汇报给 and 限额 are on the synced member rows.
        let plans: HashMap<String, (Option<String>, Option<u64>)> = a
            .client
            .members()
            .unwrap_or_default()
            .into_iter()
            .map(|m| (m.id, (m.reports_to, m.weekly_budget_cents)))
            .collect();
        for m in &members {
            let mine: Vec<&proto::BotRun> = week_runs.iter().filter(|r| r.bot_id == m.id).collect();
            let mut stats = MemberStats::default();
            let mut extras = MemberExtras::default();
            let mut week_topics: Vec<(String, String)> = Vec::new();
            let mut cost: Option<u64> = None;
            let mut spans = Vec::new();
            for r in &mine {
                let key = (r.group_id.clone(), r.topic_id.clone());
                if !week_topics.contains(&key) {
                    week_topics.push(key);
                }
                if r.status == proto::RunStatus::Done {
                    stats.done_this_week += 1;
                    let span = r.duration_ms.or_else(|| match (r.started_at, r.ended_at) {
                        (Some(a), Some(b)) => Some(b.saturating_sub(a)),
                        _ => None,
                    });
                    spans.extend(span);
                }
                if let Some(c) = run_cost_cents(r) {
                    cost = Some(cost.unwrap_or(0) + c);
                }
            }
            stats.topics_this_week = week_topics.len() as u32;
            stats.cost_cents_this_week = cost;
            stats.cost_currency = Currency::Usd;
            if let Some((reports_to, budget)) = plans.get(&m.id) {
                stats.reports_to = reports_to.clone().filter(|r| !r.is_empty() && *r != m.id);
                extras.budget_cents = *budget;
            }
            // 排队 of this member, in its queue order.
            let mut mine_queued: Vec<&proto::BotRun> =
                queued.iter().filter(|r| r.bot_id == m.id).collect();
            mine_queued.sort_by_key(|r| {
                (
                    r.queue_position.unwrap_or(u32::MAX),
                    r.queued_at.unwrap_or(r.created_at),
                )
            });
            stats.queued = mine_queued.len() as u32;
            extras.queued = mine_queued
                .iter()
                .map(|r| QueuedWork {
                    topic: Some((r.group_id.clone(), r.topic_id.clone())),
                    title: topics
                        .get(&r.topic_id)
                        .map(|t| t.title.clone())
                        .unwrap_or_default(),
                    note: None,
                    queued_at: r.queued_at.map(ms),
                })
                .collect();
            for r in active.iter().filter(|r| r.bot_id == m.id) {
                let key = (r.group_id.clone(), r.topic_id.clone());
                if !stats.active.contains(&key) {
                    stats.active.push(key);
                }
            }

            // 电脑: the machine of the member's newest run.
            let newest = active
                .iter()
                .chain(week_runs.iter())
                .filter(|r| r.bot_id == m.id && r.machine_id.is_some())
                .max_by_key(|r| r.updated_at);
            if let (Some(run), Some(list)) = (newest, machines.as_ref())
                && let Some(machine) = list.iter().find(|x| Some(&x.id) == run.machine_id.as_ref())
            {
                let busy = active
                    .iter()
                    .any(|r| r.bot_id == m.id && r.machine_id.as_ref() == Some(&machine.id));
                stats.machine = Some(MemberMachine {
                    name: machine.name.clone(),
                    online: machine.connected,
                    busy,
                    disk_used_pct: machine.health.as_ref().and_then(|h| {
                        (h.disk_total_bytes > 0).then(|| {
                            let used = h.disk_total_bytes.saturating_sub(h.disk_free_bytes);
                            (used.saturating_mul(100) / h.disk_total_bytes).min(100) as u8
                        })
                    }),
                });
            }

            // 最近的问题: 话题 whose newest run of the member failed or got stuck.
            let mut newest_per_topic: HashMap<&str, &proto::BotRun> = HashMap::new();
            for r in &mine {
                let slot = newest_per_topic.entry(r.topic_id.as_str()).or_insert(r);
                if r.updated_at > slot.updated_at {
                    *slot = r;
                }
            }
            for r in newest_per_topic.values() {
                if matches!(
                    r.status,
                    proto::RunStatus::Failed | proto::RunStatus::Blocked
                ) {
                    let text = r
                        .summary
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                        .or_else(|| topics.get(&r.topic_id).map(|t| t.title.clone()))
                        .unwrap_or_default();
                    stats.problems.push(MemberProblem {
                        text,
                        at: ms(r.updated_at),
                        run_id: Some(r.id.clone()),
                    });
                }
            }
            stats.problems.sort_by_key(|p| std::cmp::Reverse(p.at));
            stats.problems.truncate(PROBLEMS_KEPT);

            // 能用的工具: the runner's CLI and repository (owners only).
            if m.is_bot()
                && owner
                && let Some(runner) = self.remote_runner(&a.client, &m.id)
            {
                stats.tools.push(runner.runtime.clone());
                extras.tools.push(MemberTool {
                    name: runner.runtime.clone(),
                    access: runner.model.clone(),
                    detail: None,
                });
                if let Some(repo) = runner.repo.as_ref().and_then(repo_name) {
                    extras.tools.push(MemberTool {
                        name: repo,
                        access: None,
                        detail: runner.base_branch.clone(),
                    });
                }
            }

            // 产出 and 最近交付.
            if !spans.is_empty() {
                extras.avg_topic_ms = Some(spans.iter().sum::<u64>() / spans.len() as u64);
            }
            let mut decision_ids = HashSet::new();
            for (g, t) in &week_topics {
                let changes = self.pr_link(t).and_then(|_| self.topic_changes_inner(g, t));
                if let Some(ch) = changes {
                    extras.prs_this_week += 1;
                    let waits = topics.get(t).is_some_and(|x| x.waiting_on.contains(&me));
                    extras.deliveries.push(Delivery {
                        kind: DeliveryKind::Pr {
                            number: ch.number,
                            additions: ch.additions,
                            deletions: ch.deletions,
                        },
                        title: ch.title.clone(),
                        at: mine
                            .iter()
                            .filter(|r| &r.topic_id == t)
                            .map(|r| ms(r.updated_at))
                            .max()
                            .unwrap_or(0),
                        topic: Some((g.clone(), t.clone())),
                        state: Some(match ch.state {
                            ChangeState::Merged => DeliveryState::Merged,
                            _ if waits => DeliveryState::WaitsOnMe,
                            _ => DeliveryState::Open,
                        }),
                    });
                }
                for d in decisions_by_topic.get(t).into_iter().flatten() {
                    if decision_ids.insert(d.id.clone()) {
                        extras.decisions.push(d.clone());
                    }
                }
            }
            for (g, art) in &snap.artifacts {
                if art.created_by == m.id && art.updated_at >= week_ago {
                    extras.pages_this_week += 1;
                    extras.deliveries.push(Delivery {
                        kind: DeliveryKind::Artifact {
                            artifact_id: art.id.clone(),
                            version: art.version,
                        },
                        title: art.title.clone(),
                        at: art.updated_at,
                        topic: Some((g.clone(), art.topic_id.clone())),
                        state: None,
                    });
                }
            }
            stats.artifacts_this_week = extras.prs_this_week + extras.pages_this_week;
            extras.deliveries.sort_by_key(|d| std::cmp::Reverse(d.at));
            extras.deliveries.truncate(DELIVERIES_KEPT);
            snap.stats.insert(m.id.clone(), stats);
            snap.extras.insert(m.id.clone(), extras);
        }
        snap
    }

    /// 需要你拍板: every open Card v2 ask for me in a 话题 I can see (not only the newest
    /// messages of the 话题 that wait on me), plus, for a waiting 话题 without one, its newest
    /// v1 card with buttons from someone else, else the 话题 itself.
    fn open_asks(
        &self,
        client: &Arc<chat_core::Client>,
        ask_messages: Vec<proto::Message>,
        me: &str,
        owner: bool,
        now: Millis,
        topics: &HashMap<String, Topic>,
    ) -> Vec<AskItem> {
        let mut out = Vec::new();
        let mut asked = HashSet::new();
        for m in ask_messages {
            if m.author_id == me {
                continue;
            }
            let Some(t) = topics.get(&m.topic_id) else {
                continue;
            };
            let Some(card) = &m.card_v2 else { continue };
            let Some(ask) = &card.ask else { continue };
            if !ask.allows(me, owner, Some(t.created_by.as_str())) {
                continue;
            }
            if ask.expires_at.is_some_and(|e| ms(e) < now) {
                continue;
            }
            let answered = client
                .card_answers(&m.id)
                .unwrap_or_default()
                .iter()
                .any(|r| r.actor_id == me && r.rev == card.rev && r.retracted_at.is_none());
            if answered {
                continue;
            }
            asked.insert(t.id.clone());
            out.push(AskItem {
                group_id: t.group_id.clone(),
                topic_id: t.id.clone(),
                message_id: Some(m.id.clone()),
                from: m.author_id.clone(),
                kind: kind_of_v2(card),
                title: card.title.clone(),
                at: ms(m.created_at),
            });
        }
        for t in self.todo_items() {
            if t.reason != TodoReason::WaitingOnMe {
                continue;
            }
            let real = self.resolve_topic(&t.topic.id);
            if asked.contains(&real) || asked.contains(&t.topic.id) {
                continue;
            }
            let messages = client
                .messages(&real, None, V1_ASK_SCAN)
                .unwrap_or_default();
            let v1 = messages.iter().rev().find_map(|m| {
                if m.is_deleted() || m.author_id == me || m.card_v2.is_some() {
                    return None;
                }
                let c = m.card.as_ref()?;
                let has_buttons = c.actions.as_ref().is_some_and(|x| !x.is_empty());
                let pressed = self
                    .shared
                    .cards
                    .presses(client, &m.id)
                    .iter()
                    .any(|p| p.actor_id == me);
                (has_buttons && !pressed).then_some((m, c))
            });
            out.push(match v1 {
                Some((m, c)) => AskItem {
                    group_id: t.group_id.clone(),
                    topic_id: t.topic.id.clone(),
                    message_id: Some(m.id.clone()),
                    from: m.author_id.clone(),
                    kind: super::desk::kind_of_v1(c),
                    title: c.title.clone(),
                    at: ms(m.created_at),
                },
                None => AskItem {
                    group_id: t.group_id.clone(),
                    topic_id: t.topic.id.clone(),
                    message_id: None,
                    from: t
                        .topic
                        .owner_id
                        .clone()
                        .unwrap_or_else(|| t.topic.created_by.clone()),
                    kind: match t.topic.status {
                        TopicStatus::WaitingReview => AskKind::Review,
                        _ => AskKind::Topic,
                    },
                    title: t.topic.title.clone(),
                    at: t.topic.last_activity_at,
                },
            });
        }
        out.sort_by_key(|x| std::cmp::Reverse(x.at));
        out
    }

    /// The machine list, when fetched; starts a fetch when it is missing or old.
    fn remote_machines(&self, client: &Arc<chat_core::Client>) -> Option<Vec<proto::Machine>> {
        let generation = self.state().generation;
        let (list, fetch) = {
            let mut cache = lock(&self.shared.snapshots.inner);
            let inner = cache.for_generation(generation);
            let stale = inner
                .machines_at
                .is_none_or(|at| at.elapsed() > REMOTE_REFRESH);
            let fetch = stale && !inner.machines_loading;
            if fetch {
                inner.machines_loading = true;
            }
            (inner.machines.clone(), fetch)
        };
        if fetch {
            let this = self.clone();
            let http = client.http();
            client.spawn(async move {
                let result = http.machines().await;
                let changed = {
                    let mut cache = lock(&this.shared.snapshots.inner);
                    let inner = cache.for_generation(generation);
                    inner.machines_loading = false;
                    inner.machines_at = Some(Instant::now());
                    match result {
                        Ok(list) => {
                            let list: Vec<proto::Machine> = list
                                .into_iter()
                                .filter(|m| m.revoked != Some(true))
                                .collect();
                            let changed = inner.machines.as_ref() != Some(&list);
                            inner.machines = Some(list);
                            changed
                        }
                        // Not every member may list machines: the 电脑 card stays hidden.
                        Err(e) => {
                            tracing::info!(error = %e, "成员: cannot list machines");
                            false
                        }
                    }
                };
                if changed && this.state().generation == generation {
                    this.emit(vec![Change::Members]);
                }
            });
        }
        list
    }

    /// The runner settings of a bot (`GET /v1/bots/:id/runner`, owners), when fetched;
    /// starts a fetch when missing or old.
    fn remote_runner(
        &self,
        client: &Arc<chat_core::Client>,
        bot_id: &str,
    ) -> Option<proto::BotRunner> {
        let generation = self.state().generation;
        let (runner, fetch) = {
            let mut cache = lock(&self.shared.snapshots.inner);
            let inner = cache.for_generation(generation);
            let entry = inner.runners.get(bot_id);
            let stale = entry.is_none_or(|(at, _)| at.elapsed() > REMOTE_REFRESH);
            let fetch = stale && !inner.runners_loading.contains(bot_id);
            if fetch {
                inner.runners_loading.insert(bot_id.to_string());
            }
            (entry.and_then(|(_, r)| r.clone()), fetch)
        };
        if fetch {
            let this = self.clone();
            let http = client.http();
            let bot = bot_id.to_string();
            client.spawn(async move {
                let result = http.bot_runner(&bot).await;
                let changed = {
                    let mut cache = lock(&this.shared.snapshots.inner);
                    let inner = cache.for_generation(generation);
                    inner.runners_loading.remove(&bot);
                    match result {
                        Ok(r) => {
                            let changed = inner.runners.get(&bot).is_none_or(|(_, old)| *old != r);
                            inner.runners.insert(bot, (Instant::now(), r));
                            changed
                        }
                        Err(e) => {
                            tracing::info!(error = %e, "成员: cannot read a runner");
                            let old = inner.runners.get(&bot).and_then(|(_, r)| r.clone());
                            inner.runners.insert(bot, (Instant::now(), old));
                            false
                        }
                    }
                };
                if changed && this.state().generation == generation {
                    this.emit(vec![Change::Members]);
                }
            });
        }
        runner
    }

    // --- the getters ---

    pub(super) fn member_stats_inner(&self, member_id: &str) -> Option<MemberStats> {
        self.account()?;
        Some(
            self.snapshot()
                .stats
                .get(member_id)
                .cloned()
                .unwrap_or_default(),
        )
    }

    pub(super) fn member_extras_inner(&self, member_id: &str) -> MemberExtras {
        self.snapshot()
            .extras
            .get(member_id)
            .cloned()
            .unwrap_or_default()
    }

    pub(super) fn group_recent_done_inner(&self, group_id: &str) -> Vec<RecentDone> {
        self.snapshot()
            .recent_done
            .get(group_id)
            .cloned()
            .unwrap_or_default()
    }
}

#[cfg(test)]
impl CoreProvider {
    /// Tests: the machine list and runner settings as if they were fetched now.
    pub(super) fn seed_remote_for_test(
        &self,
        machines: Vec<proto::Machine>,
        runners: Vec<proto::BotRunner>,
    ) {
        let generation = self.state().generation;
        let mut cache = lock(&self.shared.snapshots.inner);
        let inner = cache.for_generation(generation);
        inner.machines = Some(machines);
        inner.machines_at = Some(Instant::now());
        for r in runners {
            inner
                .runners
                .insert(r.bot_id.clone(), (Instant::now(), Some(r)));
        }
        drop(cache);
        self.shared.snapshots.note(&[Change::Members]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_runner_repo_is_named_owner_slash_name() {
        let url = |u: &str| proto::RunnerRepo {
            url: Some(u.into()),
            path: None,
        };
        assert_eq!(
            repo_name(&url("https://github.com/acme/chat.git")).as_deref(),
            Some("acme/chat")
        );
        assert_eq!(
            repo_name(&url("git@github.com:acme/chat")).as_deref(),
            Some("acme/chat")
        );
        let path = proto::RunnerRepo {
            url: None,
            path: Some("/Users/a/code/chat/".into()),
        };
        assert_eq!(repo_name(&path).as_deref(), Some("chat"));
    }
}
