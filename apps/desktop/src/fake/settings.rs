//! The settings requests of [`FakeProvider`], in memory. Owners-only rules and
//! the last-owner rule follow the server, so the views can show their errors.

use std::collections::{HashMap, HashSet};

use super::{FakeProvider, ME, RoleView};
use crate::data::*;

/// Settings state that the main fake state does not hold.
pub struct FakeSettings {
    notify: HashMap<GroupId, NotifyLevel>,
    /// (group, topic) pairs I follow.
    followed: HashSet<(GroupId, TopicId)>,
    /// (group, topic) pairs I muted.
    muted_topics: HashSet<(GroupId, TopicId)>,
    machines: Vec<MachineInfo>,
    /// How runner-backed bot members work, by member id.
    runners: HashMap<String, RunnerSetup>,
    next: u64,
}

impl FakeSettings {
    pub fn new() -> Self {
        Self {
            notify: HashMap::new(),
            followed: HashSet::new(),
            muted_topics: HashSet::new(),
            machines: sample_machines(),
            runners: sample_runners(),
            next: 1,
        }
    }
}

/// Jojo works on the office Mac mini.
fn sample_runners() -> HashMap<String, RunnerSetup> {
    HashMap::from([(
        "m-deploy".to_string(),
        RunnerSetup {
            runtime: "claude".into(),
            labels: vec!["mac".into(), "office".into()],
            repo: Some("https://github.com/example/app".into()),
            base_branch: Some("main".into()),
            machine_id: None,
            model: Some("opus".into()),
            effort: Some("high".into()),
        },
    )])
}

/// What the office Mac's CLIs report (feature "effort").
fn sample_clis() -> Vec<crate::data::CliChoice> {
    use crate::data::{CliChoice, ModelChoice};
    let levels = |l: &[&str]| l.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let model = |id: &str, name: Option<&str>, efforts: &[&str], default: bool| ModelChoice {
        id: id.into(),
        name: name.map(str::to_string),
        efforts: levels(efforts),
        is_default: default,
    };
    vec![
        CliChoice {
            runtime: "claude".into(),
            kind: "claude".into(),
            models: vec![
                model("fable", None, &[], false),
                model("opus", None, &[], true),
                model("sonnet", None, &[], false),
                model("haiku", None, &[], false),
            ],
            efforts: levels(&["low", "medium", "high", "xhigh", "max"]),
        },
        CliChoice {
            runtime: "codex".into(),
            kind: "codex".into(),
            models: vec![
                model(
                    "gpt-6-astra",
                    Some("GPT-6-Astra"),
                    &["low", "medium", "high", "xhigh", "max"],
                    true,
                ),
                model(
                    "gpt-6-sol",
                    Some("GPT-6-Sol"),
                    &["low", "medium", "high"],
                    false,
                ),
            ],
            efforts: levels(&["low", "medium", "high", "xhigh", "max"]),
        },
    ]
}

fn sample_machines() -> Vec<MachineInfo> {
    vec![
        MachineInfo {
            id: "mc-mini".into(),
            name: "办公室 Mac mini".into(),
            labels: vec!["mac".into(), "office".into()],
            connected: true,
            healthy: true,
            last_heartbeat_at: Some(super::now_ms() - 8_000),
            version: Some("0.4.0".into()),
            disk_free_bytes: Some(212 * 1024 * 1024 * 1024),
            disk_total_bytes: Some(494 * 1024 * 1024 * 1024),
            active_runs: 1,
            max_parallel_runs: Some(2),
            runtimes: vec!["claude".into(), "codex".into()],
            clis: sample_clis(),
            ..Default::default()
        },
        MachineInfo {
            id: "mc-vps".into(),
            name: "团队 VPS".into(),
            labels: vec!["linux".into(), "vps".into()],
            connected: true,
            healthy: true,
            last_heartbeat_at: Some(super::now_ms() - 5_000),
            version: Some("0.4.0".into()),
            disk_free_bytes: Some(118 * 1024 * 1024 * 1024),
            disk_total_bytes: Some(160 * 1024 * 1024 * 1024),
            active_runs: 2,
            max_parallel_runs: Some(4),
            runtimes: vec!["claude".into(), "codex".into()],
            clis: sample_clis(),
            github: Some(MachineGithub {
                state: "signed_in".into(),
                login: Some("example-bot".into()),
            }),
            ..Default::default()
        },
        MachineInfo {
            id: "mc-old".into(),
            name: "旧笔记本".into(),
            labels: vec!["mac".into()],
            connected: false,
            healthy: false,
            last_heartbeat_at: Some(super::now_ms() - 3 * 86_400_000),
            version: Some("0.3.2".into()),
            disk_free_bytes: Some(3 * 1024 * 1024 * 1024),
            disk_total_bytes: Some(250 * 1024 * 1024 * 1024),
            active_runs: 0,
            max_parallel_runs: Some(1),
            runtimes: Vec::new(),
            github: Some(MachineGithub {
                state: "signed_out".into(),
                login: None,
            }),
            ..Default::default()
        },
    ]
}

/// My own computer in the role views: this one, with Jojo's work for me here in my folder.
fn my_computer() -> MachineInfo {
    MachineInfo {
        id: MY_COMPUTER.into(),
        name: "MacBook Air".into(),
        labels: vec!["mac".into()],
        connected: true,
        healthy: true,
        last_heartbeat_at: Some(super::now_ms() - 3_000),
        version: Some("0.4.0".into()),
        active_runs: 0,
        max_parallel_runs: Some(2),
        runtimes: vec!["claude".into()],
        clis: sample_clis().into_iter().take(1).collect(),
        owner_id: Some(ME.into()),
        personal_bots: vec![PersonalBot {
            bot_id: "m-deploy".into(),
            repo_path: Some("~/code/app".into()),
        }],
        ..Default::default()
    }
}

/// The id of [`my_computer`]: this computer in the role views.
pub const MY_COMPUTER: &str = "mc-air";

/// A computer as a member sees it: the name, the owner and whether it is online.
fn cut_down(m: MachineInfo) -> MachineInfo {
    MachineInfo {
        id: m.id,
        name: m.name,
        connected: m.connected,
        healthy: m.healthy,
        owner_id: m.owner_id,
        ..Default::default()
    }
}

/// A made-up `XXXX-XXXX` code (never a real one).
fn fake_code(n: u64) -> String {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut x = n.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x5DEE_CE66;
    let mut out = String::new();
    for i in 0..8 {
        if i > 0 && i % 4 == 0 {
            out.push('-');
        }
        out.push(ALPHABET[(x % 32) as usize] as char);
        x /= 32;
        x ^= n.rotate_left(i * 5);
    }
    out
}

impl FakeProvider {
    fn is_owner(&self) -> bool {
        self.with(|s| {
            s.members
                .iter()
                .any(|m| m.id == ME && m.level == MemberLevel::Owner)
        })
    }

    fn next_id(&self) -> u64 {
        let mut st = self.settings.lock().unwrap();
        st.next += 1;
        st.next
    }

    fn server_url(&self) -> String {
        self.with(|s| match &s.session {
            SessionState::SignedIn { server_url, .. } => server_url.clone(),
            _ => String::new(),
        })
    }

    /// Screenshot scenes: who I am in the sample team (design-roles 3.9). 陈晨 is the other
    /// 管理员 or the 所有者; alone, the other people are gone.
    pub fn set_role_view(&self, view: RoleView) {
        const OTHER: &str = "m-chenchen";
        self.with(|s| {
            if view == RoleView::Alone {
                s.members.retain(|m| m.id == ME || m.is_bot());
            }
            for m in &mut s.members {
                let (level, team_owner) = match (m.id.as_str(), view) {
                    (ME, RoleView::Owner | RoleView::Alone) => (MemberLevel::Owner, true),
                    (ME, RoleView::Admin) => (MemberLevel::Owner, false),
                    (ME, RoleView::Member) => (MemberLevel::Member, false),
                    (OTHER, RoleView::Owner) => (MemberLevel::Owner, false),
                    (OTHER, _) => (MemberLevel::Owner, true),
                    _ => continue,
                };
                m.level = level;
                m.team_owner = team_owner;
            }
            let me = s.members.iter().find(|m| m.id == ME).cloned();
            if let (SessionState::SignedIn { me: mine, .. }, Some(me)) = (&mut s.session, me) {
                *mine = me;
            }
        });
        let mut st = self.settings.lock().unwrap();
        st.machines.insert(0, my_computer());
        if view != RoleView::Alone {
            st.machines.insert(
                1,
                MachineInfo {
                    id: "mc-home".into(),
                    name: "家里的 Mac mini".into(),
                    labels: vec!["mac".into()],
                    connected: false,
                    last_heartbeat_at: Some(super::now_ms() - 2 * 86_400_000),
                    version: Some("0.4.0".into()),
                    owner_id: Some(ME.into()),
                    ..Default::default()
                },
            );
            // Forge works on 陈晨's own computer: only 陈晨 places it there.
            st.runners.insert(
                "m-coder".into(),
                RunnerSetup {
                    runtime: "claude".into(),
                    machine_id: Some("mc-chen".into()),
                    ..Default::default()
                },
            );
            st.machines.push(MachineInfo {
                id: "mc-chen".into(),
                name: "陈晨的 iMac".into(),
                labels: vec!["mac".into()],
                connected: true,
                healthy: true,
                last_heartbeat_at: Some(super::now_ms() - 6_000),
                owner_id: Some(OTHER.into()),
                ..Default::default()
            });
        }
        drop(st);
        self.this_mac_used
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.emit(vec![Change::Members, Change::Session]);
    }

    /// True when I am the 所有者 (the 身份 changes and stopping 管理员 are mine).
    fn is_team_owner(&self) -> bool {
        self.with(|s| {
            s.members
                .iter()
                .any(|m| m.id == ME && TeamRole::of(m, &s.members) == TeamRole::Owner)
        })
    }

    /// Applies `f` to a member and keeps my session copy in step. `f` gets the member's 身份
    /// and the number of active 所有者.
    fn change_member(
        &self,
        id: &str,
        f: impl FnOnce(&mut Member, TeamRole, usize) -> Result<(), AdminError>,
    ) -> Result<Member, AdminError> {
        let result = self.with(|s| {
            let all = s.members.clone();
            let active_owners = all
                .iter()
                .filter(|m| !m.deactivated && TeamRole::of(m, &all) == TeamRole::Owner)
                .count();
            let m = s
                .members
                .iter_mut()
                .find(|m| m.id == id)
                .ok_or(AdminError::NotFound)?;
            let role = TeamRole::of(m, &all);
            f(m, role, active_owners)?;
            let m = m.clone();
            if let SessionState::SignedIn { me, .. } = &mut s.session
                && me.id == m.id
            {
                *me = m.clone();
            }
            Ok(m)
        });
        if result.is_ok() {
            self.emit(vec![Change::Members, Change::Session]);
        }
        result
    }

    pub(super) fn update_profile_fake(
        &self,
        name: String,
        role: Option<String>,
    ) -> AdminReply<Member> {
        let name = name.trim().to_string();
        if name.is_empty() || name.chars().count() > 64 {
            return reply_now(Err(AdminError::Invalid));
        }
        let role = role.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
        if role.as_ref().is_some_and(|r| r.chars().count() > 64) {
            return reply_now(Err(AdminError::Invalid));
        }
        reply_now(self.change_member(ME, |m, _, _| {
            m.name = name;
            m.role = role;
            Ok(())
        }))
    }

    pub(super) fn upload_avatar_fake(&self, path: std::path::PathBuf) -> AdminReply<Member> {
        self.upload_member_avatar_fake(ME, path)
    }

    pub(super) fn remove_member_avatar_fake(&self, id: &str) -> AdminReply<Member> {
        if id != ME && !self.is_owner() {
            return reply_now(Err(AdminError::Forbidden));
        }
        reply_now(self.change_member(id, |m, _, _| {
            m.avatar_url = None;
            Ok(())
        }))
    }

    pub(super) fn upload_member_avatar_fake(
        &self,
        id: &str,
        path: std::path::PathBuf,
    ) -> AdminReply<Member> {
        if id != ME && !self.is_owner() {
            return reply_now(Err(AdminError::Forbidden));
        }
        let Ok(meta) = std::fs::metadata(&path) else {
            return reply_now(Err(AdminError::NotFound));
        };
        if meta.len() > 1024 * 1024 {
            return reply_now(Err(AdminError::TooLarge));
        }
        let url = format!("file://{}", path.display());
        reply_now(self.change_member(id, |m, _, _| {
            m.avatar_url = Some(url);
            Ok(())
        }))
    }

    pub(super) fn create_invite_fake(&self, request: InviteRequest) -> AdminReply<InviteCreated> {
        if !self.is_owner() {
            return reply_now(Err(AdminError::Forbidden));
        }
        if request.name.trim().is_empty() {
            return reply_now(Err(AdminError::Invalid));
        }
        let code = fake_code(self.next_id());
        let download = format!("{}/download#code={code}", self.server_url());
        reply_now(Ok(InviteCreated {
            code: SecretText::new(code),
            download_url: SecretText::new(download),
            expires_at: super::now_ms() + 7 * 86_400_000,
        }))
    }

    /// The server's rules (design-roles 3.2): only a 所有者 changes 身份, an AI member is never
    /// a 所有者, and the team keeps one active 所有者.
    pub(super) fn set_member_role_fake(&self, id: &str, role: TeamRole) -> AdminReply<Member> {
        if !self.is_team_owner() {
            return reply_now(Err(AdminError::Forbidden));
        }
        reply_now(self.change_member(id, |m, was, owners| {
            if role == TeamRole::Owner && m.is_bot() {
                return Err(AdminError::Invalid);
            }
            if was == TeamRole::Owner && role != TeamRole::Owner && !m.deactivated && owners <= 1 {
                return Err(AdminError::LastOwner);
            }
            m.level = if role == TeamRole::Member {
                MemberLevel::Member
            } else {
                MemberLevel::Owner
            };
            m.team_owner = role == TeamRole::Owner;
            Ok(())
        }))
    }

    pub(super) fn set_member_deactivated_fake(&self, id: &str, off: bool) -> AdminReply<Member> {
        if !self.is_owner() {
            return reply_now(Err(AdminError::Forbidden));
        }
        let team_owner = self.is_team_owner();
        reply_now(self.change_member(id, |m, role, owners| {
            // Only a 所有者 stops a 管理员 or a 所有者.
            if role != TeamRole::Member && !team_owner {
                return Err(AdminError::Forbidden);
            }
            if off && role == TeamRole::Owner && !m.deactivated && owners <= 1 {
                return Err(AdminError::LastOwner);
            }
            m.deactivated = off;
            if off {
                m.online = false;
            }
            Ok(())
        }))
    }

    pub(super) fn set_member_character_fake(
        &self,
        id: &str,
        character: Option<CharacterId>,
    ) -> AdminReply<Member> {
        if !self.is_owner() {
            return reply_now(Err(AdminError::Forbidden));
        }
        reply_now(self.change_member(id, |m, _, _| {
            m.character = character;
            Ok(())
        }))
    }

    pub(super) fn create_bot_member_fake(
        &self,
        name: String,
        role: Option<String>,
        character: Option<CharacterId>,
        runner: Option<RunnerSetup>,
    ) -> AdminReply<BotCreated> {
        if !self.is_owner() {
            return reply_now(Err(AdminError::Forbidden));
        }
        let name = name.trim().to_string();
        if name.is_empty() || runner.as_ref().is_some_and(|r| r.runtime.trim().is_empty()) {
            return reply_now(Err(AdminError::Invalid));
        }
        let n = self.next_id();
        let member = Member {
            id: format!("m-bot-{n}"),
            kind: MemberKind::Bot,
            name,
            role: role.map(|r| r.trim().to_string()).filter(|r| !r.is_empty()),
            level: MemberLevel::Member,
            avatar_url: None,
            deactivated: false,
            online: false,
            character,
            team_owner: false,
            deleted: false,
        };
        self.with(|s| s.members.push(member.clone()));
        if let Some(r) = &runner {
            let mut settings = self.settings.lock().unwrap();
            settings.runners.insert(member.id.clone(), r.clone());
        }
        self.emit(vec![Change::Members]);
        let credential = runner
            .is_none()
            .then(|| SecretText::new(format!("tc_fake{n:040}")));
        reply_now(Ok(BotCreated { member, credential }))
    }

    pub(super) fn bot_runner_fake(&self, id: &str) -> AdminReply<Option<RunnerSetup>> {
        if !self.is_owner() {
            return reply_now(Err(AdminError::Forbidden));
        }
        reply_now(Ok(self.settings.lock().unwrap().runners.get(id).cloned()))
    }

    pub(super) fn update_bot_member_fake(&self, id: &str, edit: BotMemberEdit) -> AdminReply<()> {
        if !self.is_owner() {
            return reply_now(Err(AdminError::Forbidden));
        }
        let name = edit.name.map(|n| n.trim().to_string());
        if name
            .as_ref()
            .is_some_and(|n| n.is_empty() || n.chars().count() > 64)
        {
            return reply_now(Err(AdminError::Invalid));
        }
        if let Some(RunnerChange::Set(r)) = &edit.runner
            && r.runtime.trim().is_empty()
        {
            return reply_now(Err(AdminError::Invalid));
        }
        if !self.members().iter().any(|m| m.id == id && m.is_bot()) {
            return reply_now(Err(AdminError::NotFound));
        }
        if name.is_some() || edit.role.is_some() {
            let role = edit
                .role
                .map(|r| r.map(|r| r.trim().to_string()).filter(|r| !r.is_empty()));
            if let Err(e) = self.change_member(id, |m, _, _| {
                if let Some(name) = name {
                    m.name = name;
                }
                if let Some(role) = role {
                    m.role = role;
                }
                Ok(())
            }) {
                return reply_now(Err(e));
            }
        }
        if let Some(change) = edit.runner {
            let mut settings = self.settings.lock().unwrap();
            match change {
                RunnerChange::Set(r) => {
                    settings.runners.insert(id.to_string(), r);
                }
                RunnerChange::Remove => {
                    settings.runners.remove(id);
                }
            }
        }
        self.emit(vec![Change::Members]);
        reply_now(Ok(()))
    }

    /// The computers as the server shows them to me (design-roles 3.5): team computers in full
    /// for an admin and cut down for a member, mine in full with my AI members, other people's
    /// cut down.
    pub(super) fn machines_fake(&self) -> AdminReply<Vec<MachineInfo>> {
        let admin = self.is_owner();
        let list = self.settings.lock().unwrap().machines.clone();
        reply_now(Ok(list
            .into_iter()
            .map(|m| match m.ownership(Some(ME)) {
                Ownership::Mine => m,
                Ownership::Team if admin => MachineInfo {
                    personal_bots: Vec::new(),
                    ..m
                },
                _ => cut_down(m),
            })
            .collect()))
    }

    /// Changes computer `id` when I may (`manage`), and answers it as I see it.
    fn change_machine(
        &self,
        id: &str,
        f: impl FnOnce(&mut MachineInfo, bool) -> Result<(), AdminError>,
    ) -> AdminReply<MachineInfo> {
        let admin = self.is_owner();
        let mut st = self.settings.lock().unwrap();
        let Some(m) = st.machines.iter_mut().find(|m| m.id == id) else {
            return reply_now(Err(AdminError::NotFound));
        };
        if !m.manageable(Some(ME), admin) {
            return reply_now(Err(AdminError::Forbidden));
        }
        reply_now(f(m, admin).map(|()| m.clone()))
    }

    pub(super) fn has_machine(&self, id: &str) -> bool {
        self.settings
            .lock()
            .unwrap()
            .machines
            .iter()
            .any(|m| m.id == id)
    }

    pub(super) fn rename_machine_fake(&self, id: &str, name: String) -> AdminReply<MachineInfo> {
        let name = name.trim().to_string();
        self.change_machine(id, |m, _| {
            if name.is_empty() {
                return Err(AdminError::Invalid);
            }
            m.name = name;
            Ok(())
        })
    }

    pub(super) fn set_machine_owner_fake(&self, id: &str, mine: bool) -> AdminReply<MachineInfo> {
        self.change_machine(id, |m, admin| {
            if !admin {
                return Err(AdminError::Forbidden);
            }
            if mine {
                m.owner_id = Some(ME.into());
            } else {
                m.owner_id = None;
                m.personal_bots.clear();
            }
            Ok(())
        })
    }

    pub(super) fn remove_machine_fake(&self, id: &str) -> AdminReply<()> {
        let admin = self.is_owner();
        let mut st = self.settings.lock().unwrap();
        match st.machines.iter().position(|m| m.id == id) {
            Some(i) if st.machines[i].manageable(Some(ME), admin) => {
                st.machines.remove(i);
                reply_now(Ok(()))
            }
            Some(_) => reply_now(Err(AdminError::Forbidden)),
            None => reply_now(Ok(())),
        }
    }

    pub(super) fn machine_bot_fake(
        &self,
        id: &str,
        bot: &str,
        set: Option<Option<String>>,
    ) -> AdminReply<MachineInfo> {
        let bot = bot.to_string();
        self.change_machine(id, |m, _| {
            if m.owner_id.as_deref() != Some(ME) {
                return Err(AdminError::Forbidden);
            }
            m.personal_bots.retain(|b| b.bot_id != bot);
            if let Some(repo_path) = set {
                m.personal_bots.push(PersonalBot {
                    bot_id: bot,
                    repo_path: repo_path.filter(|r| !r.trim().is_empty()),
                });
            }
            Ok(())
        })
    }

    pub(super) fn create_machine_fake(
        &self,
        name: String,
        labels: Vec<String>,
        personal: Option<bool>,
    ) -> AdminReply<MachineCreated> {
        let admin = self.is_owner();
        let personal = personal.unwrap_or(!admin);
        if !personal && !admin {
            return reply_now(Err(AdminError::Forbidden));
        }
        let name = name.trim().to_string();
        if name.is_empty() {
            return reply_now(Err(AdminError::Invalid));
        }
        let n = self.next_id();
        let machine = MachineInfo {
            id: format!("mc-{n}"),
            name,
            labels: labels
                .into_iter()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect(),
            owner_id: personal.then(|| ME.to_string()),
            ..Default::default()
        };
        self.settings.lock().unwrap().machines.push(machine.clone());
        let token = format!("tm_fake{n:040}");
        let install = format!("# fake\nprintf '%s' '{token}' > machine-token");
        let install_windows =
            format!("# fake\n[IO.File]::WriteAllText(\"machine-token\", '{token}')");
        let install_linux = format!("# fake\nprintf '%s' '{token}' > ~/.config/machine-token");
        reply_now(Ok(MachineCreated {
            machine,
            credential: SecretText::new(token),
            install_command: SecretText::new(install),
            install_command_windows: SecretText::new(install_windows),
            install_command_linux: SecretText::new(install_linux),
        }))
    }

    pub(super) fn group_prefs_fake(&self, group_id: &str) -> GroupPrefs {
        let (muted, dm) = self.with(|s| {
            (
                s.muted.contains(group_id),
                s.groups
                    .iter()
                    .any(|g| g.id == group_id && g.kind == GroupKind::Dm),
            )
        });
        let notify = self
            .settings
            .lock()
            .unwrap()
            .notify
            .get(group_id)
            .copied()
            .unwrap_or(if dm {
                NotifyLevel::All
            } else {
                NotifyLevel::Mentions
            });
        GroupPrefs { muted, notify }
    }

    pub(super) fn set_group_prefs_fake(
        &self,
        group_id: &str,
        muted: Option<bool>,
        notify: Option<NotifyLevel>,
    ) -> AdminReply<()> {
        if !self.with(|s| s.groups.iter().any(|g| g.id == group_id)) {
            return reply_now(Err(AdminError::NotFound));
        }
        if let Some(level) = notify {
            self.settings
                .lock()
                .unwrap()
                .notify
                .insert(group_id.to_string(), level);
        }
        if let Some(muted) = muted {
            self.with(|s| {
                if muted {
                    s.muted.insert(group_id.to_string());
                } else {
                    s.muted.remove(group_id);
                }
            });
        }
        self.emit(vec![Change::Groups]);
        reply_now(Ok(()))
    }

    pub(super) fn topic_following_fake(&self, group_id: &str, topic_id: &str) -> bool {
        self.settings
            .lock()
            .unwrap()
            .followed
            .contains(&(group_id.to_string(), topic_id.to_string()))
    }

    /// Seeds the followed set like the server's auto-follow (called once).
    pub(super) fn seed_followed(&self) {
        let seed: Vec<(GroupId, TopicId)> = self.with(|s| {
            s.topics
                .iter()
                .filter(|t| t.created_by == ME || t.waiting_on.iter().any(|w| w == ME))
                .map(|t| (t.group_id.clone(), t.id.clone()))
                .collect()
        });
        self.settings.lock().unwrap().followed.extend(seed);
    }

    pub(super) fn set_topic_following_fake(&self, group_id: &str, topic_id: &str, follow: bool) {
        if !self.with(|s| {
            s.topics
                .iter()
                .any(|t| t.group_id == group_id && t.id == topic_id)
        }) {
            return;
        }
        {
            let mut st = self.settings.lock().unwrap();
            let key = (group_id.to_string(), topic_id.to_string());
            if follow {
                st.followed.insert(key);
            } else {
                st.followed.remove(&key);
            }
        }
        self.emit(vec![Change::Topics {
            group_id: group_id.to_string(),
        }]);
    }

    pub(super) fn topic_muted_fake(&self, group_id: &str, topic_id: &str) -> bool {
        self.settings
            .lock()
            .unwrap()
            .muted_topics
            .contains(&(group_id.to_string(), topic_id.to_string()))
    }

    pub(super) fn set_topic_muted_fake(&self, group_id: &str, topic_id: &str, muted: bool) {
        if !self.with(|s| {
            s.topics
                .iter()
                .any(|t| t.group_id == group_id && t.id == topic_id)
        }) {
            return;
        }
        {
            let mut st = self.settings.lock().unwrap();
            let key = (group_id.to_string(), topic_id.to_string());
            if muted {
                st.muted_topics.insert(key);
            } else {
                st.muted_topics.remove(&key);
            }
        }
        self.emit(vec![Change::Topics {
            group_id: group_id.to_string(),
        }]);
    }

    pub(super) fn send_diagnostics_fake(&self, description: String) -> AdminReply<String> {
        if description.chars().count() > 2000 {
            return reply_now(Err(AdminError::TooLarge));
        }
        reply_now(Ok(format!("dg-{:06}", self.next_id())))
    }
}
