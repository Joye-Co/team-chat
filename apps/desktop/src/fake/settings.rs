//! The settings requests of [`FakeProvider`], in memory. Owners-only rules and
//! the last-owner rule follow the server, so the views can show their errors.

use std::collections::{HashMap, HashSet};

use super::{FakeProvider, ME};
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
        },
    )])
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
        },
    ]
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

    /// Applies `f` to a member and keeps my session copy in step.
    fn change_member(
        &self,
        id: &str,
        f: impl FnOnce(&mut Member, usize) -> Result<(), AdminError>,
    ) -> Result<Member, AdminError> {
        let result = self.with(|s| {
            let active_owners = s
                .members
                .iter()
                .filter(|m| m.level == MemberLevel::Owner && !m.deactivated)
                .count();
            let m = s
                .members
                .iter_mut()
                .find(|m| m.id == id)
                .ok_or(AdminError::NotFound)?;
            f(m, active_owners)?;
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
        reply_now(self.change_member(ME, |m, _| {
            m.name = name;
            m.role = role;
            Ok(())
        }))
    }

    pub(super) fn upload_avatar_fake(&self, path: std::path::PathBuf) -> AdminReply<Member> {
        let Ok(meta) = std::fs::metadata(&path) else {
            return reply_now(Err(AdminError::NotFound));
        };
        if meta.len() > 1024 * 1024 {
            return reply_now(Err(AdminError::TooLarge));
        }
        let url = format!("file://{}", path.display());
        reply_now(self.change_member(ME, |m, _| {
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

    pub(super) fn set_member_level_fake(&self, id: &str, level: MemberLevel) -> AdminReply<Member> {
        if !self.is_owner() {
            return reply_now(Err(AdminError::Forbidden));
        }
        reply_now(self.change_member(id, |m, owners| {
            if m.level == MemberLevel::Owner
                && level != MemberLevel::Owner
                && !m.deactivated
                && owners <= 1
            {
                return Err(AdminError::LastOwner);
            }
            m.level = level;
            Ok(())
        }))
    }

    pub(super) fn set_member_deactivated_fake(&self, id: &str, off: bool) -> AdminReply<Member> {
        if !self.is_owner() {
            return reply_now(Err(AdminError::Forbidden));
        }
        reply_now(self.change_member(id, |m, owners| {
            if off && m.level == MemberLevel::Owner && !m.deactivated && owners <= 1 {
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
        reply_now(self.change_member(id, |m, _| {
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
            if let Err(e) = self.change_member(id, |m, _| {
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

    pub(super) fn machines_fake(&self) -> AdminReply<Vec<MachineInfo>> {
        reply_now(Ok(self.settings.lock().unwrap().machines.clone()))
    }

    pub(super) fn create_machine_fake(
        &self,
        name: String,
        labels: Vec<String>,
    ) -> AdminReply<MachineCreated> {
        if !self.is_owner() {
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
            ..Default::default()
        };
        self.settings.lock().unwrap().machines.push(machine.clone());
        let token = format!("tm_fake{n:040}");
        let install = format!("# fake\nprintf '%s' '{token}' > machine-token");
        reply_now(Ok(MachineCreated {
            machine,
            credential: SecretText::new(token),
            install_command: SecretText::new(install),
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

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;

    fn answer<T>(reply: AdminReply<T>) -> Result<T, AdminError> {
        block_on(reply).unwrap_or(Err(AdminError::Other))
    }

    #[test]
    fn fake_codes_look_like_invites() {
        let c = fake_code(7);
        assert_eq!(c.len(), 9);
        assert_eq!(c.matches('-').count(), 1);
        assert_ne!(fake_code(7), fake_code(8));
    }

    #[test]
    fn last_owner_is_kept() {
        let p = FakeProvider::new();
        let owners: Vec<String> = p
            .members()
            .into_iter()
            .filter(|m| m.level == MemberLevel::Owner && !m.deactivated)
            .map(|m| m.id)
            .collect();
        // Demote every owner but the last; the last one is refused.
        let (last, rest) = owners.split_last().expect("an owner");
        for id in rest.iter().filter(|id| id.as_str() != ME) {
            assert!(answer(p.set_member_level(id, MemberLevel::Member)).is_ok());
        }
        if last != ME {
            assert!(answer(p.set_member_level(last, MemberLevel::Member)).is_ok());
        }
        assert_eq!(
            answer(p.set_member_deactivated(ME, true)),
            Err(AdminError::LastOwner)
        );
    }

    #[test]
    fn profile_and_prefs_round_trip() {
        let p = FakeProvider::new();
        let m = answer(p.update_profile(" 新名字 ".into(), Some("".into()))).unwrap();
        assert_eq!(m.name, "新名字");
        assert_eq!(m.role, None);
        let SessionState::SignedIn { me, .. } = p.session() else {
            panic!("signed in")
        };
        assert_eq!(me.name, "新名字");
        let g = p.groups()[0].id.clone();
        answer(p.set_group_prefs(&g, Some(true), Some(NotifyLevel::None))).unwrap();
        assert_eq!(
            p.group_prefs(&g),
            GroupPrefs {
                muted: true,
                notify: NotifyLevel::None
            }
        );
        let followed = p.followed_topics();
        assert!(!followed.is_empty());
        let t = &followed[0];
        p.set_topic_following(&t.group_id, &t.topic.id, false);
        assert_eq!(p.followed_topics().len(), followed.len() - 1);
        // Muting is separate from following.
        assert!(!p.topic_muted(&t.group_id, &t.topic.id));
        p.set_topic_muted(&t.group_id, &t.topic.id, true);
        assert!(p.topic_muted(&t.group_id, &t.topic.id));
        assert!(!p.topic_following(&t.group_id, &t.topic.id));
        p.set_topic_muted(&t.group_id, &t.topic.id, false);
        assert!(!p.topic_muted(&t.group_id, &t.topic.id));
        p.set_topic_muted(&t.group_id, "no-such-topic", true);
        assert!(!p.topic_muted(&t.group_id, "no-such-topic"));
    }

    #[test]
    fn bot_member_credentials() {
        let p = FakeProvider::new();
        let plain = answer(p.create_bot_member("助手".into(), None, None, None)).unwrap();
        assert!(plain.credential.is_some());
        assert!(!format!("{plain:?}").contains("tc_fake"));
        let runner = RunnerSetup {
            runtime: "main".into(),
            ..Default::default()
        };
        let backed = answer(p.create_bot_member("跑腿".into(), None, None, Some(runner))).unwrap();
        assert!(backed.credential.is_none());
        assert!(p.members().iter().any(|m| m.id == backed.member.id));
    }

    #[test]
    fn bot_member_edit_changes_name_role_and_runner() {
        let p = FakeProvider::new();
        let runner = RunnerSetup {
            runtime: "main".into(),
            labels: vec!["mac".into()],
            ..Default::default()
        };
        let id = answer(p.create_bot_member("跑腿".into(), None, None, Some(runner.clone())))
            .unwrap()
            .member
            .id;
        assert_eq!(answer(p.bot_runner(&id)).unwrap(), Some(runner));
        let moved = RunnerSetup {
            runtime: "other".into(),
            repo: Some("/srv/app".into()),
            ..Default::default()
        };
        answer(p.update_bot_member(
            &id,
            BotMemberEdit {
                name: Some(" 跑腿二号 ".into()),
                role: Some(Some("测试".into())),
                runner: Some(RunnerChange::Set(moved.clone())),
            },
        ))
        .unwrap();
        let m = p.members().into_iter().find(|m| m.id == id).unwrap();
        assert_eq!(
            (m.name.as_str(), m.role.as_deref()),
            ("跑腿二号", Some("测试"))
        );
        assert_eq!(answer(p.bot_runner(&id)).unwrap(), Some(moved));
        answer(p.update_bot_member(
            &id,
            BotMemberEdit {
                role: Some(None),
                runner: Some(RunnerChange::Remove),
                ..Default::default()
            },
        ))
        .unwrap();
        let m = p.members().into_iter().find(|m| m.id == id).unwrap();
        assert_eq!(m.role, None);
        assert_eq!(answer(p.bot_runner(&id)).unwrap(), None);
        let blank = BotMemberEdit {
            name: Some("  ".into()),
            ..Default::default()
        };
        assert_eq!(
            answer(p.update_bot_member(&id, blank)),
            Err(AdminError::Invalid)
        );
    }
}
