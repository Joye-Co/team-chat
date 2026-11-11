//! The pages beyond chat (M5): 产出, members, search, 单独问, runs and group info.
//!
//! Cache reads return at once. The calls that wait for the network (`load_artifact`,
//! `open_artifact_version`, `search_messages`, `run_terminal`, `cancel_run`, `ask_aside`,
//! `open_dm`) block their thread, so the app calls them off the main thread.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use chat_core::proto::{Artifact, ArtifactKind, ArtifactVersion, BotRun, Member, MemberKind};
use chat_core::{ClientError, SearchError, SearchRequest, SideChatRequest, TerminalLine};
use chat_protocol::{GroupKind, TopicContext, main_topic_id};

use crate::{ChatClient, CoreError, Directory, MemberItem, TopicItem, role_of, topic_item, wire};

const WEEK_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// The most output of one terminal line the live view keeps.
const MAX_OUTPUT_CHARS: usize = 4000;

/// A 产出 in a list.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ArtifactItem {
    pub id: String,
    pub group_id: String,
    pub group_name: String,
    pub topic_id: String,
    /// Empty when the 话题 is not in the cache.
    pub topic_title: String,
    pub title: String,
    /// A web page (opens in the page view); otherwise a file (opens in the file preview).
    pub is_page: bool,
    pub current_version: u32,
    pub created_by: String,
    pub created_by_name: String,
    pub created_by_is_bot: bool,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ArtifactVersionItem {
    pub version: u32,
    /// The file that opens: the page, or the only file.
    pub entry: String,
    pub entry_mime: String,
    /// The author's note on what changed; empty when none.
    pub note: String,
    pub created_by_name: String,
    pub created_at: u64,
    pub file_count: u32,
    pub size: u64,
    /// The run that made the version, when a member made it while working.
    pub run_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ArtifactDetail {
    pub artifact: ArtifactItem,
    /// Newest first; empty until [`ChatClient::load_artifact`] fetched them.
    pub versions: Vec<ArtifactVersionItem>,
}

/// A version on disk.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct OpenedVersion {
    pub version: u32,
    /// The version folder: a page may read only inside it.
    pub dir: String,
    /// The file to open, inside `dir`.
    pub entry: String,
    pub is_page: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PlanStep {
    pub text: String,
    /// `done`, `current`, `waiting` or `pending`.
    pub state: String,
}

/// A run of an AI member in a 话题.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RunItem {
    pub id: String,
    pub member_id: String,
    pub member_name: String,
    pub group_id: String,
    pub topic_id: String,
    pub topic_title: String,
    /// The protocol's `RunStatus` in its wire form (`running`, `done`, ...).
    pub status: String,
    /// Still working: it can be watched and stopped.
    pub active: bool,
    /// Waits for a free computer.
    pub queued: bool,
    /// What the member says it did or does; empty when none.
    pub summary: String,
    pub plan: Vec<PlanStep>,
    pub steps_done: u32,
    /// The step it works on (or waits on), when it has a plan.
    pub current_step: Option<String>,
    /// The member who asked for it.
    pub requested_by: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub ended_at: Option<u64>,
}

impl RunItem {
    /// 0...1 by plan steps, when the run has a plan.
    fn progress(&self) -> Option<f64> {
        (!self.plan.is_empty()).then(|| self.steps_done as f64 / self.plan.len() as f64)
    }
}

/// One line of the live view: a command the member ran, or what it printed.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TerminalLineItem {
    pub seq: u64,
    pub at: u64,
    pub is_command: bool,
    pub text: String,
    /// For output: the command failed.
    pub failed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TerminalPage {
    pub lines: Vec<TerminalLineItem>,
    /// Give it back to [`ChatClient::run_terminal`] to follow the run.
    pub last_seq: u64,
}

/// A member's page.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MemberDetail {
    pub member: MemberItem,
    /// This week: for an AI member the runs it did, for a person the work they handed out.
    pub week_runs: u32,
    pub week_done: u32,
    /// 产出 the member made this week.
    pub week_artifacts: u32,
    pub active_runs: u32,
    /// Newest first, at most 20.
    pub recent: Vec<RunItem>,
    /// Our 私聊, when there is one.
    pub dm_group_id: Option<String>,
    /// The 群 we are both in.
    pub shared_groups: Vec<String>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct GroupInfo {
    pub id: String,
    pub name: String,
    pub is_dm: bool,
    /// What the group is for; empty when not set.
    pub purpose: String,
    pub main_topic_id: String,
    /// People first, then AI members.
    pub members: Vec<MemberItem>,
    pub topic_count: u32,
    pub artifact_count: u32,
    pub active_runs: Vec<RunItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct HighlightRange {
    /// In characters (Unicode code points) of the snippet.
    pub start: u32,
    pub length: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SearchHitItem {
    pub group_id: String,
    pub group_name: String,
    pub topic_id: String,
    pub topic_title: String,
    /// The hit is in the group's own chat.
    pub in_main: bool,
    pub message_id: String,
    pub author_name: String,
    pub snippet: String,
    pub highlights: Vec<HighlightRange>,
    pub created_at: u64,
    pub seq: u64,
}

/// Where a message is, for 单独问 about it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MessageContext {
    pub group_id: String,
    pub topic_id: String,
    pub seq: u64,
    pub author_id: String,
    pub author_is_bot: bool,
    pub excerpt: String,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

/// Waits on a core task from a thread outside the core runtime.
fn wait<T>(task: tokio::task::JoinHandle<Result<T, ClientError>>) -> Result<T, CoreError> {
    chat_core::client::background()
        .block_on(task)
        .map_err(|e| CoreError::Failed {
            message: e.to_string(),
        })?
        .map_err(Into::into)
}

/// A store read as the bridge's error.
fn store<T, E: Into<ClientError>>(r: Result<T, E>) -> Result<T, CoreError> {
    r.map_err(|e| e.into().into())
}

fn member_item(client: &chat_core::Client, dir: &Directory, m: &Member) -> MemberItem {
    MemberItem {
        id: m.id.clone(),
        name: m.name.clone(),
        is_bot: m.kind == MemberKind::Bot,
        role: role_of(m),
        online: client.is_online(&m.id),
        is_me: dir.is_me(&m.id),
    }
}

fn shorten(text: &str, max: usize) -> String {
    let n = text.chars().count();
    if n <= max {
        return text.to_string();
    }
    let tail: String = text.chars().skip(n - max).collect();
    format!("…{tail}")
}

/// Names of groups and 话题, read once per call.
struct Names {
    groups: HashMap<String, (String, bool)>,
    topics: HashMap<String, String>,
}

impl ChatClient {
    fn names(&self, dir: &Directory) -> Result<Names, CoreError> {
        let mut groups = HashMap::new();
        for row in self.inner.groups()? {
            let is_dm = row.group.kind == GroupKind::Dm;
            let name = if is_dm || row.group.name.is_empty() {
                row.group
                    .member_ids
                    .iter()
                    .filter(|id| !dir.is_me(id))
                    .map(|id| dir.name(id))
                    .collect::<Vec<_>>()
                    .join("、")
            } else {
                row.group.name.clone()
            };
            groups.insert(row.group.id, (name, is_dm));
        }
        Ok(Names {
            groups,
            topics: HashMap::new(),
        })
    }

    fn topic_title(&self, names: &mut Names, topic_id: &str) -> String {
        if let Some(t) = names.topics.get(topic_id) {
            return t.clone();
        }
        let title = self
            .inner
            .store()
            .topic(topic_id)
            .ok()
            .flatten()
            .map(|r| r.topic.title)
            .unwrap_or_default();
        names.topics.insert(topic_id.to_string(), title.clone());
        title
    }

    fn artifact_item(&self, dir: &Directory, names: &mut Names, a: &Artifact) -> ArtifactItem {
        let author = dir.member(&a.created_by);
        ArtifactItem {
            id: a.id.clone(),
            group_id: a.group_id.clone(),
            group_name: names
                .groups
                .get(&a.group_id)
                .map(|g| g.0.clone())
                .unwrap_or_default(),
            topic_id: a.topic_id.clone(),
            topic_title: self.topic_title(names, &a.topic_id),
            title: a.title.clone(),
            is_page: a.kind == ArtifactKind::Html,
            current_version: a.current_version,
            created_by: a.created_by.clone(),
            created_by_name: author.map(|m| m.name.clone()).unwrap_or_default(),
            created_by_is_bot: author.is_some_and(|m| m.kind == MemberKind::Bot),
            updated_at: a.updated_at,
        }
    }

    fn version_item(&self, dir: &Directory, v: &ArtifactVersion) -> ArtifactVersionItem {
        ArtifactVersionItem {
            version: v.version,
            entry_mime: v
                .files
                .iter()
                .find(|f| f.path == v.entry)
                .map(|f| f.mime.clone())
                .unwrap_or_default(),
            entry: v.entry.clone(),
            note: v.note.clone().unwrap_or_default(),
            created_by_name: dir.name(&v.created_by),
            created_at: v.created_at,
            file_count: v.files.len() as u32,
            size: v.files.iter().map(|f| f.size).sum(),
            run_id: v.run_id.clone(),
        }
    }

    fn run_item(&self, dir: &Directory, names: &mut Names, r: &BotRun) -> RunItem {
        let plan: Vec<PlanStep> = r
            .plan
            .iter()
            .flatten()
            .map(|p| PlanStep {
                text: p.text.clone(),
                state: wire(&p.state),
            })
            .collect();
        RunItem {
            id: r.id.clone(),
            member_id: r.bot_id.clone(),
            member_name: dir.name(&r.bot_id),
            group_id: r.group_id.clone(),
            topic_id: r.topic_id.clone(),
            topic_title: self.topic_title(names, &r.topic_id),
            status: wire(&r.status),
            active: r.is_active(),
            queued: r.is_queued(),
            summary: r.summary.clone().unwrap_or_default(),
            steps_done: plan.iter().filter(|p| p.state == "done").count() as u32,
            current_step: plan
                .iter()
                .find(|p| p.state == "current" || p.state == "waiting")
                .map(|p| p.text.clone()),
            plan,
            requested_by: r.requested_by.clone(),
            created_at: r.created_at,
            updated_at: r.updated_at,
            ended_at: r.ended_at,
        }
    }

    /// Runs of groups I am in, without 单独问 runs.
    fn visible_runs(&self, since: u64, names: &Names) -> Result<Vec<BotRun>, CoreError> {
        Ok(self
            .inner
            .runs_since(since)?
            .into_iter()
            .filter(|r| !r.is_aside() && names.groups.contains_key(&r.group_id))
            .collect())
    }
}

#[uniffi::export]
impl ChatClient {
    // ---------------------------------------------------------------- 产出

    /// 产出 from the cache, newest change first: of one 话题, of one 群, or of every 群.
    pub fn artifacts(
        &self,
        group_id: Option<String>,
        topic_id: Option<String>,
    ) -> Result<Vec<ArtifactItem>, CoreError> {
        let dir = Directory::read(&self.inner)?;
        let mut names = self.names(&dir)?;
        let mut list = match (&group_id, &topic_id) {
            (_, Some(t)) => self.inner.artifacts(t)?,
            (Some(g), None) => store(self.inner.store().group_artifacts(g))?,
            (None, None) => {
                let mut all = Vec::new();
                for g in names.groups.keys() {
                    all.extend(store(self.inner.store().group_artifacts(g))?);
                }
                all
            }
        };
        list.sort_by_key(|a| std::cmp::Reverse(a.updated_at));
        Ok(list
            .iter()
            .map(|a| self.artifact_item(&dir, &mut names, a))
            .collect())
    }

    /// Fetches the 产出 lists in the background (every 群 when `group_id` is `None`);
    /// [`crate::Update::Artifacts`] follows for each 话题 that has some.
    pub fn refresh_artifacts(&self, group_id: Option<String>) {
        let groups = match group_id {
            Some(g) => vec![g],
            None => self
                .inner
                .groups()
                .map(|gs| gs.into_iter().map(|g| g.group.id).collect())
                .unwrap_or_default(),
        };
        for g in groups {
            drop(self.inner.refresh_artifacts(&g, None));
        }
    }

    /// A 产出 with the versions in the cache, without network.
    pub fn artifact(&self, artifact_id: String) -> Result<Option<ArtifactDetail>, CoreError> {
        let dir = Directory::read(&self.inner)?;
        let mut names = self.names(&dir)?;
        Ok(self
            .inner
            .artifact(&artifact_id)?
            .map(|row| ArtifactDetail {
                artifact: self.artifact_item(&dir, &mut names, &row.artifact),
                versions: row
                    .versions
                    .iter()
                    .map(|v| self.version_item(&dir, v))
                    .collect(),
            }))
    }

    /// Fetches a 产出 with all its versions. Blocks: call it off the main thread.
    pub fn load_artifact(&self, artifact_id: String) -> Result<ArtifactDetail, CoreError> {
        wait(self.inner.load_artifact(&artifact_id))?;
        self.artifact(artifact_id)?.ok_or(CoreError::Failed {
            message: "not found".into(),
        })
    }

    /// Puts every file of a version (the current one when `version` is `None`) on disk.
    /// Blocks: call it off the main thread.
    pub fn open_artifact_version(
        &self,
        artifact_id: String,
        version: Option<u32>,
    ) -> Result<OpenedVersion, CoreError> {
        let folder = wait(self.inner.open_artifact_version(
            &artifact_id,
            version,
            chat_core::no_progress(),
        ))?;
        let is_page = self
            .inner
            .artifact(&artifact_id)?
            .is_some_and(|r| r.artifact.kind == ArtifactKind::Html);
        Ok(OpenedVersion {
            version: folder.version,
            dir: folder.dir.to_string_lossy().into_owned(),
            entry: folder.entry.to_string_lossy().into_owned(),
            is_page,
        })
    }

    // ---------------------------------------------------------------- 成员

    /// A member's page: who, this week's numbers and recent work.
    pub fn member_detail(&self, member_id: String) -> Result<Option<MemberDetail>, CoreError> {
        let dir = Directory::read(&self.inner)?;
        let Some(m) = dir.member(&member_id) else {
            return Ok(None);
        };
        let member = member_item(&self.inner, &dir, m);
        let mut names = self.names(&dir)?;
        let is_bot = member.is_bot;
        let mine = |r: &BotRun| {
            if is_bot {
                r.bot_id == member_id
            } else {
                r.requested_by.as_deref() == Some(member_id.as_str())
            }
        };
        let week_ago = now_ms().saturating_sub(WEEK_MS);
        let mut runs: Vec<BotRun> = self
            .visible_runs(0, &names)?
            .into_iter()
            .filter(|r| mine(r))
            .collect();
        runs.sort_by_key(|r| std::cmp::Reverse(r.updated_at));
        let week: Vec<&BotRun> = runs.iter().filter(|r| r.created_at >= week_ago).collect();
        let mut week_artifacts = 0;
        for g in names.groups.keys() {
            week_artifacts += store(self.inner.store().group_artifacts(g))?
                .iter()
                .filter(|a| a.created_by == member_id && a.updated_at >= week_ago)
                .count() as u32;
        }
        let shared_groups = self
            .inner
            .groups()?
            .into_iter()
            .filter(|g| g.group.kind != GroupKind::Dm && g.group.member_ids.contains(&member_id))
            .map(|g| g.group.name)
            .collect();
        Ok(Some(MemberDetail {
            week_runs: week.len() as u32,
            week_done: week
                .iter()
                .filter(|r| r.status == chat_core::proto::RunStatus::Done)
                .count() as u32,
            week_artifacts,
            active_runs: runs.iter().filter(|r| r.is_active()).count() as u32,
            recent: runs
                .iter()
                .take(20)
                .map(|r| self.run_item(&dir, &mut names, r))
                .collect(),
            dm_group_id: if member.is_me {
                None
            } else {
                self.inner.dm_with(&member_id)?
            },
            shared_groups,
            member,
        }))
    }

    /// Our 私聊 with a member: found in the cache, or made on the server. Answers the 私聊's
    /// group id; its chat is `main_topic_id` of it. Blocks: call it off the main thread.
    pub fn open_dm(&self, member_id: String) -> Result<String, CoreError> {
        if let Some(id) = self.inner.dm_with(&member_id)? {
            return Ok(id);
        }
        let http = self.inner.http();
        let task = self.inner.spawn(async move { http.dm(&member_id).await });
        let summary = chat_core::client::background()
            .block_on(task)
            .map_err(|e| CoreError::Failed {
                message: e.to_string(),
            })??;
        store(self.inner.store().apply_directory(
            None,
            Some(std::slice::from_ref(&summary)),
            None,
        ))?;
        Ok(summary.group.id)
    }

    /// The chat of a group: its own topic id.
    pub fn main_topic_of(&self, group_id: String) -> String {
        main_topic_id(&group_id)
    }

    // ---------------------------------------------------------------- 群

    pub fn group_info(&self, group_id: String) -> Result<Option<GroupInfo>, CoreError> {
        let dir = Directory::read(&self.inner)?;
        let Some(row) = store(self.inner.store().group(&group_id))? else {
            return Ok(None);
        };
        let mut names = self.names(&dir)?;
        let mut members: Vec<MemberItem> = row
            .group
            .member_ids
            .iter()
            .filter_map(|id| dir.member(id))
            .map(|m| member_item(&self.inner, &dir, m))
            .collect();
        members.sort_by(|a, b| (a.is_bot, &a.name).cmp(&(b.is_bot, &b.name)));
        let topic_count = self
            .inner
            .topics(&group_id, 1000)?
            .iter()
            .filter(|t| !t.topic.is_main())
            .count() as u32;
        let artifact_count = store(self.inner.store().group_artifacts(&group_id))?.len() as u32;
        let mut active: Vec<BotRun> = self
            .inner
            .active_runs()?
            .into_iter()
            .filter(|r| r.group_id == group_id && !r.is_aside())
            .collect();
        active.sort_by_key(|r| std::cmp::Reverse(r.updated_at));
        Ok(Some(GroupInfo {
            name: names
                .groups
                .get(&group_id)
                .map(|g| g.0.clone())
                .unwrap_or_default(),
            is_dm: row.group.kind == GroupKind::Dm,
            purpose: row.group.purpose.clone().unwrap_or_default(),
            main_topic_id: main_topic_id(&group_id),
            members,
            topic_count,
            artifact_count,
            active_runs: active
                .iter()
                .map(|r| self.run_item(&dir, &mut names, r))
                .collect(),
            id: group_id,
        }))
    }

    // ---------------------------------------------------------------- runs

    /// Runs of a 话题, newest first, without 单独问 runs.
    pub fn runs(&self, topic_id: String) -> Result<Vec<RunItem>, CoreError> {
        let dir = Directory::read(&self.inner)?;
        let mut names = self.names(&dir)?;
        Ok(self
            .inner
            .runs(&topic_id)?
            .iter()
            .filter(|r| !r.is_aside())
            .map(|r| self.run_item(&dir, &mut names, r))
            .collect())
    }

    /// Runs still working in every group, newest change first.
    pub fn active_runs(&self) -> Result<Vec<RunItem>, CoreError> {
        let dir = Directory::read(&self.inner)?;
        let mut names = self.names(&dir)?;
        let mut runs: Vec<BotRun> = self
            .inner
            .active_runs()?
            .into_iter()
            .filter(|r| !r.is_aside() && names.groups.contains_key(&r.group_id))
            .collect();
        runs.sort_by_key(|r| std::cmp::Reverse(r.updated_at));
        Ok(runs
            .iter()
            .map(|r| self.run_item(&dir, &mut names, r))
            .collect())
    }

    /// One run from the cache.
    pub fn run(&self, run_id: String) -> Result<Option<RunItem>, CoreError> {
        let dir = Directory::read(&self.inner)?;
        let mut names = self.names(&dir)?;
        Ok(store(self.inner.store().run(&run_id))?.map(|r| self.run_item(&dir, &mut names, &r)))
    }

    /// 0...1 by plan steps, for the Live Activity; `None` without a plan.
    pub fn run_progress(&self, run_id: String) -> Option<f64> {
        self.run(run_id).ok().flatten().and_then(|r| r.progress())
    }

    /// The commands a run ran and their output after `after_seq`. Call again with
    /// `last_seq` while the run works. Blocks: call it off the main thread.
    pub fn run_terminal(&self, run_id: String, after_seq: u64) -> Result<TerminalPage, CoreError> {
        let (lines, last_seq) = wait(self.inner.run_terminal(&run_id, after_seq))?;
        Ok(TerminalPage {
            lines: lines
                .into_iter()
                .filter_map(|l| match l {
                    TerminalLine::Command {
                        seq,
                        at,
                        tool,
                        title,
                        ..
                    } => Some(TerminalLineItem {
                        seq,
                        at,
                        is_command: true,
                        text: if title.is_empty() { tool } else { title },
                        failed: false,
                    }),
                    TerminalLine::Output {
                        seq,
                        at,
                        ok,
                        output,
                        ..
                    } => output
                        .filter(|o| !o.trim().is_empty())
                        .map(|o| TerminalLineItem {
                            seq,
                            at,
                            is_command: false,
                            text: shorten(o.trim_end(), MAX_OUTPUT_CHARS),
                            failed: !ok,
                        }),
                })
                .collect(),
            last_seq: last_seq.max(after_seq),
        })
    }

    /// Stops a run. Answers false when it had already ended. Blocks: call it off the main
    /// thread.
    pub fn cancel_run(&self, run_id: String) -> Result<bool, CoreError> {
        Ok(wait(self.inner.cancel_run(&run_id))? == chat_core::CancelRun::Cancelled)
    }

    // ---------------------------------------------------------------- search

    /// Messages on the server, best match first; every 群 when `group_id` is `None`.
    /// Blocks: call it off the main thread. Offline answers [`CoreError::Network`].
    pub fn search_messages(
        &self,
        query: String,
        group_id: Option<String>,
    ) -> Result<Vec<SearchHitItem>, CoreError> {
        let query = query.trim().to_string();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let hits = chat_core::client::background()
            .block_on(
                self.inner.search_hits(SearchRequest {
                    query: query
                        .chars()
                        .take(chat_core::proto::MAX_QUERY_CHARS)
                        .collect(),
                    group_id,
                    topic_id: None,
                    limit: Some(40),
                }),
            )
            .map_err(|e| match e {
                SearchError::Invalid(_) => CoreError::Invalid,
                SearchError::NotConnected | SearchError::Timeout => CoreError::Network,
                SearchError::Refused { message, .. } => CoreError::Failed { message },
            })?;
        let dir = Directory::read(&self.inner)?;
        let mut names = self.names(&dir)?;
        Ok(hits
            .into_iter()
            .map(|h| {
                let in_main = h.topic_id == main_topic_id(&h.group_id);
                SearchHitItem {
                    group_name: names
                        .groups
                        .get(&h.group_id)
                        .map(|g| g.0.clone())
                        .unwrap_or_default(),
                    topic_title: if in_main {
                        String::new()
                    } else {
                        self.topic_title(&mut names, &h.topic_id)
                    },
                    in_main,
                    author_name: dir.name(&h.author_id),
                    highlights: h
                        .highlights
                        .iter()
                        .flatten()
                        .map(|x| HighlightRange {
                            start: x.start,
                            length: x.length,
                        })
                        .collect(),
                    group_id: h.group_id,
                    topic_id: h.topic_id,
                    message_id: h.message_id,
                    snippet: h.snippet,
                    created_at: h.created_at,
                    seq: h.seq,
                }
            })
            .collect())
    }

    /// 话题 whose title holds `query` (any case), newest activity first, from the cache.
    pub fn search_topics(&self, query: String) -> Result<Vec<TopicItem>, CoreError> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let dir = Directory::read(&self.inner)?;
        let mut out = Vec::new();
        for g in self.inner.groups()? {
            for t in self.inner.topics(&g.group.id, 500)? {
                if !t.topic.is_main() && t.topic.title.to_lowercase().contains(&q) {
                    out.push(topic_item(t, &dir));
                }
            }
        }
        out.sort_by_key(|t| std::cmp::Reverse(t.last_activity_at));
        out.truncate(30);
        Ok(out)
    }

    /// 产出 whose title holds `query` (any case), newest change first, from the cache.
    pub fn search_artifacts(&self, query: String) -> Result<Vec<ArtifactItem>, CoreError> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let mut all = self.artifacts(None, None)?;
        all.retain(|a| a.title.to_lowercase().contains(&q));
        all.truncate(30);
        Ok(all)
    }

    // ---------------------------------------------------------------- 单独问

    /// Where a message is, from the cache.
    pub fn message_context(&self, message_id: String) -> Result<Option<MessageContext>, CoreError> {
        let dir = Directory::read(&self.inner)?;
        Ok(
            store(self.inner.store().message(&message_id))?.map(|m| MessageContext {
                author_is_bot: dir
                    .member(&m.author_id)
                    .is_some_and(|a| a.kind == MemberKind::Bot),
                excerpt: crate::one_line(&m).chars().take(120).collect(),
                group_id: m.group_id,
                topic_id: m.topic_id,
                seq: m.created_seq,
                author_id: m.author_id,
            }),
        )
    }

    /// Asks an AI member privately about a 话题, read up to `upto_seq`: the question and the
    /// answer stay in my 私聊 with it. Answers the 私聊's group id. Blocks: call it off the
    /// main thread.
    pub fn ask_aside(
        &self,
        member_id: String,
        group_id: String,
        topic_id: String,
        upto_seq: u64,
        question: String,
    ) -> Result<String, CoreError> {
        let started = wait(self.inner.start_side_chat(SideChatRequest {
            bot_id: member_id,
            context: TopicContext {
                group_id,
                topic_id,
                upto_seq,
            },
            body: question,
            title: None,
            block_refs: None,
        }))?;
        Ok(started.dm_group_id)
    }

    /// My earlier 单独问 about a 话题, newest first. Each lives in my 私聊 with the member.
    pub fn asides(&self, group_id: String, topic_id: String) -> Result<Vec<TopicItem>, CoreError> {
        let dir = Directory::read(&self.inner)?;
        Ok(self
            .inner
            .side_chats(&group_id, &topic_id)?
            .into_iter()
            .map(|t| topic_item(t, &dir))
            .collect())
    }
}

#[cfg(test)]
mod tests;
