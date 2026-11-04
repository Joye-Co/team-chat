//! What the 首页 shows, read once per change from the provider.

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use chat_core::proto::CardV2;

use crate::{
    app::Services,
    data::{
        BotRunSummary, Member, MemberId, PlanState,
        card::Card,
        v2::{AskItem, AskKind, DeskNote, DoneItem, MachineScreen, QueuedItem},
    },
};

/// An answer to send for an ask.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    /// A v1 button press (`card.action`).
    V1 { action_id: String },
    /// A v2 answer (`card.answer`).
    V2 {
        rev: u32,
        action_id: String,
        inputs: BTreeMap<String, serde_json::Value>,
    },
}

/// One ask of 需要你拍板 with the card to draw.
#[derive(Clone, Debug)]
pub struct AskCard {
    pub item: AskItem,
    pub card: Option<Arc<Card>>,
    pub card_v2: Option<Arc<CardV2>>,
    /// My answer that the provider already has (after a restart, or sent).
    pub answered: Option<Recorded>,
    /// Changed files of the 话题's PR, for a review footer.
    pub files: Option<u32>,
}

/// An answer the provider knows about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recorded {
    pub record: String,
    pub at: crate::data::Millis,
    /// Queued, not stored by the server yet.
    pub pending: bool,
}

impl AskCard {
    /// The ask's key: its card message, else its 话题.
    pub fn key(&self) -> String {
        self.item
            .message_id
            .clone()
            .unwrap_or_else(|| format!("topic-{}", self.item.topic_id))
    }
}

/// One run of 正在做.
#[derive(Clone, Debug)]
pub struct RunCard {
    pub run: BotRunSummary,
    pub topic_title: String,
    /// The live terminal, when the run reported tool events.
    pub screen: Option<MachineScreen>,
    /// When the current step started, for `已 3 分钟`.
    pub step_started: Option<crate::data::Millis>,
}

impl RunCard {
    /// The step number shown as `3/5`: done steps plus the one in progress.
    pub fn step(&self) -> (usize, usize) {
        let plan = &self.run.plan;
        let done = plan.iter().filter(|p| p.state == PlanState::Done).count();
        let current = plan.iter().any(|p| p.state == PlanState::Current);
        ((done + usize::from(current)).min(plan.len()), plan.len())
    }

    /// The step in progress (or waited on), else the run's summary.
    pub fn current_text(&self) -> Option<String> {
        self.run
            .plan
            .iter()
            .find(|p| matches!(p.state, PlanState::Current | PlanState::Waiting))
            .map(|p| p.text.clone())
            .or_else(|| self.run.summary.clone())
    }
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub note: Option<DeskNote>,
    pub asks: Vec<AskCard>,
    pub done: Vec<DoneItem>,
    pub runs: Vec<RunCard>,
    pub queue: Vec<QueuedItem>,
    /// The kind of each 产出 a result proves with.
    pub artifact_kinds: std::collections::HashMap<String, crate::data::ArtifactKind>,
    /// The members the page names.
    pub members: HashMap<MemberId, Member>,
    /// The 群 name and 话题 title of each ask, by ask key.
    pub places: HashMap<String, (String, String)>,
}

impl Snapshot {
    pub fn member(&self, id: &str) -> Option<Member> {
        self.members.get(id).cloned()
    }

    pub fn name(&self, id: &str) -> String {
        self.members
            .get(id)
            .map(|m| m.name.clone())
            .unwrap_or_default()
    }
}

impl Snapshot {
    pub fn read(services: &Services) -> Self {
        let p = &services.provider;
        let me = services.me();
        let asks: Vec<AskCard> = p
            .asks_for_me()
            .into_iter()
            .map(|item| {
                let (card, card_v2) = match &item.message_id {
                    Some(m) => (
                        p.card(&item.group_id, m),
                        p.card_v2(&item.group_id, m).map(|c| c.card),
                    ),
                    None => (None, None),
                };
                let answered = item.message_id.as_deref().and_then(|m| {
                    recorded(
                        services,
                        &item,
                        m,
                        card.as_deref(),
                        card_v2.as_deref(),
                        me.as_deref(),
                    )
                });
                let files = (item.kind == AskKind::Review)
                    .then(|| p.topic_changes(&item.group_id, &item.topic_id))
                    .flatten()
                    .map(|c| c.files);
                AskCard {
                    item,
                    card,
                    card_v2,
                    answered,
                    files,
                }
            })
            .collect();
        // 单独问 asides are not work on the desk.
        let side = p.side_chat_topics();
        let aside = |g: &str, t: &str| side.contains(&(g.to_string(), t.to_string()));
        let runs: Vec<RunCard> = p
            .active_runs()
            .into_iter()
            .filter(|run| !aside(&run.group_id, &run.topic_id))
            .map(|run| {
                let topic_title = p
                    .topic(&run.group_id, &run.topic_id)
                    .map(|t| t.title)
                    .unwrap_or_default();
                let screen = p.machine_screen(&run.id).filter(|s| !s.lines.is_empty());
                let times = p.run_step_times(&run.id);
                let current = run
                    .plan
                    .iter()
                    .position(|i| matches!(i.state, PlanState::Current | PlanState::Waiting));
                let step_started = current.and_then(|i| times.get(i).copied());
                RunCard {
                    run,
                    topic_title,
                    screen,
                    step_started,
                }
            })
            .collect();
        let mut done = p.done_today();
        done.retain(|d| !aside(&d.group_id, &d.topic_id));
        let artifact_kinds = done
            .iter()
            .flat_map(|d| d.proof.iter())
            .filter_map(|proof| match proof {
                crate::data::v2::Proof::Artifact { artifact_id, .. } => p
                    .artifact(artifact_id)
                    .map(|a| (artifact_id.clone(), a.summary.kind)),
                _ => None,
            })
            .collect();
        let note = p.desk_note();
        let queue = p.queue();
        let groups: HashMap<String, String> =
            p.groups().into_iter().map(|g| (g.id, g.name)).collect();
        let places = asks
            .iter()
            .map(|a: &AskCard| {
                let topic = p
                    .topic(&a.item.group_id, &a.item.topic_id)
                    .map(|t| t.title)
                    .unwrap_or_default();
                let group = groups.get(&a.item.group_id).cloned().unwrap_or_default();
                (a.key(), (group, topic))
            })
            .collect();
        let ids = note
            .iter()
            .map(|n| n.member_id.as_str())
            .chain(asks.iter().map(|a: &AskCard| a.item.from.as_str()))
            .chain(done.iter().map(|d| d.member_id.as_str()))
            .chain(runs.iter().map(|r: &RunCard| r.run.bot_id.as_str()))
            .chain(queue.iter().map(|q| q.member_id.as_str()));
        let mut members = HashMap::new();
        for id in ids {
            if !members.contains_key(id)
                && let Some(m) = p.member(id)
            {
                members.insert(id.to_string(), m);
            }
        }
        Self {
            note,
            asks,
            done,
            runs,
            queue,
            artifact_kinds,
            members,
            places,
        }
    }
}

/// Asks I answered on 首页 this session (by ask key), also while the answer
/// waits in the undo window. The sidebar 首页 count and the 首页 subtitle
/// both read [`waiting_on_me`], so they never disagree.
#[derive(Default)]
pub struct DeskAnswers {
    pub keys: std::collections::HashSet<String>,
}

impl gpui::Global for DeskAnswers {}

/// How many asks wait on me: the provider's asks minus every ask I
/// answered (sent, queued, or answered here and not sent yet).
pub fn waiting_on_me(services: &Services, cx: &gpui::App) -> usize {
    let p = &services.provider;
    let me = services.me();
    let local = cx.try_global::<DeskAnswers>();
    p.asks_for_me()
        .into_iter()
        .filter(|item| {
            let key = item
                .message_id
                .clone()
                .unwrap_or_else(|| format!("topic-{}", item.topic_id));
            if local.is_some_and(|l| l.keys.contains(&key)) {
                return false;
            }
            let Some(m) = item.message_id.as_deref() else {
                return true;
            };
            let card = p.card(&item.group_id, m);
            let card_v2 = p.card_v2(&item.group_id, m).map(|c| c.card);
            recorded(
                services,
                item,
                m,
                card.as_deref(),
                card_v2.as_deref(),
                me.as_deref(),
            )
            .is_none()
        })
        .count()
}

/// My answer to an ask that the provider already has.
fn recorded(
    services: &Services,
    item: &AskItem,
    message_id: &str,
    card: Option<&Card>,
    card_v2: Option<&CardV2>,
    me: Option<&str>,
) -> Option<Recorded> {
    let me = me?;
    let p = &services.provider;
    if let Some(v2) = card_v2 {
        // My answer on its way first, then the one the server stored.
        if let Some(pending) = p
            .card_v2_pending(&item.group_id, message_id)
            .filter(|x| matches!(x.phase, crate::data::cards_v2::CardV2SendPhase::Sending))
        {
            let inputs = pending.inputs.unwrap_or_default();
            return Some(Recorded {
                record: super::ask::v2_record(v2, &pending.action_id, &inputs),
                at: pending.at,
                pending: true,
            });
        }
        let answer = p
            .card_v2_answers(&item.group_id, message_id)
            .into_iter()
            .rev()
            .find(|a| a.actor_id == me)?;
        let inputs = answer.inputs.unwrap_or_default();
        let record = super::ask::v2_record(v2, &answer.action_id, &inputs);
        return Some(Recorded {
            record,
            at: answer.at,
            pending: false,
        });
    }
    let press = p
        .card_presses(&item.group_id, message_id)
        .into_iter()
        .rev()
        .find(|x| x.actor_id == me)?;
    let label = press.label.clone().or_else(|| {
        card.and_then(|c| {
            c.actions
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .find(|a| a.id == press.action_id)
                .map(|a| a.label.clone())
        })
    })?;
    let record = match (item.kind, card) {
        (AskKind::Choice, Some(c)) => super::ask::v1_option_record(c, &press.action_id)
            .unwrap_or_else(|| super::ask::button_record(&label)),
        _ => super::ask::button_record(&label),
    };
    Some(Recorded {
        record,
        at: press.at,
        pending: false,
    })
}
