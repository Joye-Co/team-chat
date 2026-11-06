//! The settings requests of [`CoreProvider`]: profile, 成员 admin, 机器, group
//! preferences, followed topics and diagnostics. Each request runs on the
//! chat-core runtime and answers through a [`AdminReply`]. Credentials and invite
//! codes are never logged.

use std::{future::Future, path::PathBuf, sync::Arc};

use chat_core::{
    ApiError, Client, ClientError,
    proto::{
        self, CreateMachineRequest, ErrorCode, RunnerRepo, SetBotRunnerRequest,
        UpdateGroupPrefsRequest, UpdateMemberRequest,
    },
};

use super::{CoreProvider, convert};
use crate::data::{
    AdminError, AdminReply, BotCreated, BotMemberEdit, Change, CharacterId, GroupKind, GroupPrefs,
    InviteCreated, InviteRequest, MachineCreated, MachineInfo, Member, MemberKind, MemberLevel,
    NotifyLevel, RunnerChange, RunnerSetup, SecretText, reply_now,
};

/// Largest avatar the server takes.
const AVATAR_MAX: u64 = 1024 * 1024;

pub(super) fn admin_error(e: &ClientError) -> AdminError {
    match e {
        ClientError::Api(api) => api_error(api),
        ClientError::NotFound => AdminError::NotFound,
        ClientError::Invalid(_) => AdminError::Invalid,
        ClientError::Store(_) | ClientError::Runtime(_) => AdminError::Other,
    }
}

pub(super) fn api_error(e: &ApiError) -> AdminError {
    match e {
        ApiError::Network(_) | ApiError::NoToken | ApiError::BadUrl => AdminError::Offline,
        ApiError::Invalid(_) => AdminError::Invalid,
        ApiError::Server { code, status, .. } => match code {
            ErrorCode::Forbidden | ErrorCode::Unauthorized => AdminError::Forbidden,
            ErrorCode::NotFound => AdminError::NotFound,
            ErrorCode::Invalid => AdminError::Invalid,
            ErrorCode::Conflict => AdminError::LastOwner,
            ErrorCode::RateLimited => AdminError::RateLimited,
            ErrorCode::TooLarge => AdminError::TooLarge,
            ErrorCode::UnsupportedProtocol => AdminError::Unsupported,
            ErrorCode::Internal if *status >= 500 => AdminError::Other,
            ErrorCode::Internal => AdminError::Other,
        },
        ApiError::Unexpected { status: 401 | 403 } => AdminError::Forbidden,
        ApiError::Unexpected { status: 404 } => AdminError::NotFound,
        ApiError::Unexpected { status: 413 } => AdminError::TooLarge,
        ApiError::Unexpected { status: 429 } => AdminError::RateLimited,
        _ => AdminError::Other,
    }
}

fn level_to_wire(level: MemberLevel) -> proto::MemberLevel {
    match level {
        MemberLevel::Owner => proto::MemberLevel::Owner,
        MemberLevel::Member => proto::MemberLevel::Member,
    }
}

fn notify_to_wire(level: NotifyLevel) -> proto::NotifyLevel {
    match level {
        NotifyLevel::All => proto::NotifyLevel::All,
        NotifyLevel::Mentions => proto::NotifyLevel::Mentions,
        NotifyLevel::None => proto::NotifyLevel::None,
    }
}

fn notify_from_wire(level: proto::NotifyLevel) -> NotifyLevel {
    match level {
        proto::NotifyLevel::All => NotifyLevel::All,
        proto::NotifyLevel::Mentions => NotifyLevel::Mentions,
        proto::NotifyLevel::None => NotifyLevel::None,
    }
}

/// Empty or blank text is "not set".
fn non_empty(text: Option<String>) -> Option<String> {
    text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

/// The `PUT /v1/bots/:id/runner` body for what the sheet shows: a repo that
/// looks like a URL is a URL, anything else a folder on the machine.
fn runner_request(runner: RunnerSetup) -> SetBotRunnerRequest {
    let repo = non_empty(runner.repo).map(|r| {
        if r.contains("://") || r.starts_with("git@") {
            RunnerRepo {
                url: Some(r),
                path: None,
            }
        } else {
            RunnerRepo {
                url: None,
                path: Some(r),
            }
        }
    });
    SetBotRunnerRequest {
        runtime: runner.runtime.trim().to_string(),
        labels: Some(
            runner
                .labels
                .iter()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect(),
        ),
        repo,
        base_branch: non_empty(runner.base_branch),
        // Always sent: `null` lets any computer run it again.
        machine_id: Some(non_empty(runner.machine_id)),
        ..Default::default()
    }
}

/// What the edit sheet shows of a stored runner.
pub(super) fn runner_setup(r: &proto::BotRunner) -> RunnerSetup {
    RunnerSetup {
        runtime: r.runtime.clone(),
        labels: r.labels.clone(),
        repo: r
            .repo
            .as_ref()
            .and_then(|repo| repo.url.clone().or_else(|| repo.path.clone())),
        base_branch: r.base_branch.clone(),
        machine_id: r.machine_id.clone(),
    }
}

pub(super) fn machine(m: &proto::Machine) -> MachineInfo {
    let health = m.health.as_ref();
    let mut runtimes: Vec<String> = m
        .clis
        .iter()
        .flatten()
        .filter(|c| c.available)
        .map(|c| c.runtime.clone())
        .collect();
    runtimes.dedup();
    MachineInfo {
        id: m.id.clone(),
        name: m.name.clone(),
        labels: m.labels.clone(),
        connected: m.connected,
        healthy: m.healthy,
        last_heartbeat_at: m.last_heartbeat_at.map(|v| v as i64),
        version: m.daemon_version.clone(),
        disk_free_bytes: health.map(|h| h.disk_free_bytes),
        disk_total_bytes: health.map(|h| h.disk_total_bytes),
        active_runs: health
            .map(|h| h.active_runs)
            .unwrap_or(m.running_runs.len() as u32)
            .max(m.running_runs.len() as u32),
        max_parallel_runs: m.max_parallel_runs,
        runtimes,
    }
}

/// Quotes a value for a POSIX shell.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Quotes a TOML basic string, safe inside an unquoted shell here-document.
fn toml_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' | '$' | '`' => out.push_str(&format!("\\u{:04X}", c as u32)),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Shell lines that write the daemon config and token (mode 600) and install
/// the launchd service. The daemon ships inside the app (`Contents/MacOS/<slug>-daemon`,
/// see scripts/build-dmg.sh), so the machine needs the app installed; a `<slug>-daemon` on
/// `PATH` (a release asset) is the fallback.
pub(super) fn install_command(server_url: &str, machine: &proto::Machine, token: &str) -> String {
    let slug = env!("PRODUCT_SLUG");
    let app = env!("PRODUCT_NAME");
    let dir = format!("$HOME/.config/{slug}");
    let labels = machine
        .labels
        .iter()
        .map(|l| toml_str(l))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "mkdir -p \"{dir}\" && chmod 700 \"{dir}\"\n\
         (umask 077; printf '%s' {token} > \"{dir}/machine-token\")\n\
         cat > \"{dir}/daemon.toml\" <<EOF\n\
         server_url = {server}\n\
         token_file = \"{dir}/machine-token\"\n\
         machine_id = {id}\n\
         name = {name}\n\
         labels = [{labels}]\n\
         EOF\n\
         D=\"/Applications/{app}.app/Contents/MacOS/{slug}-daemon\"\n\
         [ -x \"$D\" ] || D=\"$(command -v {slug}-daemon)\"\n\
         \"$D\" doctor && \"$D\" install-service",
        token = shell_quote(token),
        server = toml_str(server_url),
        id = toml_str(&machine.id),
        name = toml_str(&machine.name),
    )
}

impl CoreProvider {
    /// Runs `work` on the client's runtime and answers on the returned [`AdminReply`].
    pub(super) fn request<T, F>(&self, work: impl FnOnce(Arc<Client>, String) -> F) -> AdminReply<T>
    where
        T: Send + 'static,
        F: Future<Output = Result<T, AdminError>> + Send + 'static,
    {
        let Some(a) = self.account() else {
            return reply_now(Err(AdminError::Offline));
        };
        let (tx, rx) = futures::channel::oneshot::channel();
        let fut = work(a.client.clone(), a.server_url.clone());
        a.client.spawn(async move {
            let _ = tx.send(fut.await);
        });
        rx
    }

    fn my_id(&self) -> Option<String> {
        self.account().map(|a| a.me.id.clone())
    }

    fn patch_member(&self, id: String, req: UpdateMemberRequest) -> AdminReply<Member> {
        self.request(move |client, _| async move {
            let m = client
                .update_member(&id, req)
                .await
                .map_err(|_| AdminError::Other)?
                .map_err(|e| admin_error(&e))?;
            let online = client.is_online(&m.id);
            Ok(convert::member(&m, online))
        })
    }

    pub(super) fn update_profile_inner(
        &self,
        name: String,
        role: Option<String>,
    ) -> AdminReply<Member> {
        let Some(me) = self.my_id() else {
            return reply_now(Err(AdminError::Offline));
        };
        let name = name.trim().to_string();
        if name.is_empty() {
            return reply_now(Err(AdminError::Invalid));
        }
        let req = UpdateMemberRequest {
            name: Some(name),
            role: Some(non_empty(role)),
            ..Default::default()
        };
        self.patch_member(me, req)
    }

    pub(super) fn upload_avatar_inner(&self, path: PathBuf) -> AdminReply<Member> {
        let Some(me) = self.my_id() else {
            return reply_now(Err(AdminError::Offline));
        };
        self.request(move |client, _| async move {
            let read = super::tokio_blocking(move || -> Result<Vec<u8>, AdminError> {
                let len = std::fs::metadata(&path)
                    .map_err(|_| AdminError::NotFound)?
                    .len();
                if len > AVATAR_MAX {
                    return Err(AdminError::TooLarge);
                }
                std::fs::read(&path).map_err(|_| AdminError::NotFound)
            })
            .await;
            let bytes = read?;
            let mime = if bytes.starts_with(b"\x89PNG") {
                "image/png"
            } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
                "image/jpeg"
            } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
                "image/webp"
            } else {
                return Err(AdminError::Invalid);
            };
            let m = client
                .upload_avatar(&me, bytes, mime)
                .await
                .map_err(|_| AdminError::Other)?
                .map_err(|e| admin_error(&e))?;
            Ok(convert::member(&m, true))
        })
    }

    pub(super) fn create_invite_inner(&self, req: InviteRequest) -> AdminReply<InviteCreated> {
        let name = req.name.trim().to_string();
        if name.is_empty() {
            return reply_now(Err(AdminError::Invalid));
        }
        self.request(move |client, server_url| async move {
            let kind = match req.kind {
                MemberKind::Human => proto::MemberKind::Human,
                MemberKind::Bot => proto::MemberKind::Bot,
            };
            let role = non_empty(req.role);
            let r = client
                .http()
                .create_invite(&name, kind, level_to_wire(req.level), role.as_deref())
                .await
                .map_err(|e| api_error(&e))?;
            let download = format!(
                "{}/download#code={}",
                server_url.trim_end_matches('/'),
                urlencode(&r.code)
            );
            Ok(InviteCreated {
                code: SecretText::new(r.code),
                download_url: SecretText::new(download),
                expires_at: r.expires_at as i64,
            })
        })
    }

    pub(super) fn set_member_level_inner(
        &self,
        id: &str,
        level: MemberLevel,
    ) -> AdminReply<Member> {
        let req = UpdateMemberRequest {
            level: Some(level_to_wire(level)),
            ..Default::default()
        };
        self.patch_member(id.to_string(), req)
    }

    pub(super) fn set_member_deactivated_inner(&self, id: &str, off: bool) -> AdminReply<Member> {
        let req = UpdateMemberRequest {
            deactivated: Some(off),
            ..Default::default()
        };
        self.patch_member(id.to_string(), req)
    }

    /// 汇报给 and 限额 (owners): both fields at once, `None` clears.
    pub(super) fn set_member_plan_inner(
        &self,
        id: &str,
        plan: crate::data::pages::MemberPlan,
    ) -> AdminReply<Member> {
        let req = UpdateMemberRequest {
            reports_to: Some(plan.reports_to.filter(|r| !r.is_empty())),
            weekly_budget_cents: Some(plan.weekly_budget_cents),
            ..Default::default()
        };
        if req.validate().is_err() {
            return reply_now(Err(AdminError::Invalid));
        }
        self.patch_member(id.to_string(), req)
    }

    pub(super) fn set_member_character_inner(
        &self,
        id: &str,
        character: Option<CharacterId>,
    ) -> AdminReply<Member> {
        let req = UpdateMemberRequest {
            character: Some(character),
            ..Default::default()
        };
        self.patch_member(id.to_string(), req)
    }

    pub(super) fn create_bot_member_inner(
        &self,
        name: String,
        role: Option<String>,
        character: Option<CharacterId>,
        runner: Option<RunnerSetup>,
    ) -> AdminReply<BotCreated> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return reply_now(Err(AdminError::Invalid));
        }
        if runner.as_ref().is_some_and(|r| r.runtime.trim().is_empty()) {
            return reply_now(Err(AdminError::Invalid));
        }
        self.request(move |client, _| async move {
            let http = client.http();
            let role = non_empty(role);
            let created = http
                .create_bot(&name, role.as_deref(), character)
                .await
                .map_err(|e| api_error(&e))?;
            let member = convert::member(&created.member, false);
            let Some(runner) = runner else {
                return Ok(BotCreated {
                    member,
                    credential: Some(SecretText::new(created.token)),
                });
            };
            let req = runner_request(runner);
            http.set_bot_runner(&member.id, &req)
                .await
                .map_err(|e| api_error(&e))?;
            // The server runs this member; nobody needs its credential.
            if let Err(e) = http.revoke(Some(&member.id), true).await {
                tracing::warn!(error = %e, "could not revoke the new runner member's token");
            }
            Ok(BotCreated {
                member,
                credential: None,
            })
        })
    }

    pub(super) fn bot_runner_inner(&self, id: &str) -> AdminReply<Option<RunnerSetup>> {
        let id = id.to_string();
        self.request(move |client, _| async move {
            let r = client
                .http()
                .bot_runner(&id)
                .await
                .map_err(|e| api_error(&e))?;
            Ok(r.as_ref().map(runner_setup))
        })
    }

    pub(super) fn update_bot_member_inner(&self, id: &str, edit: BotMemberEdit) -> AdminReply<()> {
        let name = edit.name.map(|n| n.trim().to_string());
        if name.as_ref().is_some_and(|n| n.is_empty()) {
            return reply_now(Err(AdminError::Invalid));
        }
        if let Some(RunnerChange::Set(r)) = &edit.runner
            && r.runtime.trim().is_empty()
        {
            return reply_now(Err(AdminError::Invalid));
        }
        let id = id.to_string();
        let this = self.clone();
        self.request(move |client, _| async move {
            if name.is_some() || edit.role.is_some() {
                let req = UpdateMemberRequest {
                    name,
                    role: edit.role.map(non_empty),
                    ..Default::default()
                };
                client
                    .update_member(&id, req)
                    .await
                    .map_err(|_| AdminError::Other)?
                    .map_err(|e| admin_error(&e))?;
            }
            let http = client.http();
            match edit.runner {
                Some(RunnerChange::Set(setup)) => {
                    // PUT replaces the whole runner: keep what the sheet does not show.
                    let mut req = runner_request(setup);
                    if let Some(old) = http.bot_runner(&id).await.map_err(|e| api_error(&e))? {
                        req.model = old.model;
                        req.env_allowlist = Some(old.env_allowlist);
                        req.enabled = Some(old.enabled);
                    }
                    let saved = http
                        .set_bot_runner(&id, &req)
                        .await
                        .map_err(|e| api_error(&e))?;
                    this.remember_runner(&id, Some(saved));
                }
                Some(RunnerChange::Remove) => {
                    match http.delete_bot_runner(&id).await {
                        Ok(()) => {}
                        // Already gone: what the user asked for.
                        Err(e) if api_error(&e) == AdminError::NotFound => {}
                        Err(e) => return Err(api_error(&e)),
                    }
                    this.remember_runner(&id, None);
                }
                None => {}
            }
            this.emit(vec![Change::Members]);
            Ok(())
        })
    }

    pub(super) fn machines_inner(&self) -> AdminReply<Vec<MachineInfo>> {
        self.request(move |client, _| async move {
            let list = client.http().machines().await.map_err(|e| api_error(&e))?;
            Ok(list
                .iter()
                .filter(|m| m.revoked != Some(true))
                .map(machine)
                .collect())
        })
    }

    pub(super) fn create_machine_inner(
        &self,
        name: String,
        labels: Vec<String>,
    ) -> AdminReply<MachineCreated> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return reply_now(Err(AdminError::Invalid));
        }
        self.request(move |client, server_url| async move {
            let labels: Vec<String> = labels
                .into_iter()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            let req = CreateMachineRequest {
                name,
                labels: (!labels.is_empty()).then_some(labels),
            };
            let r = client
                .http()
                .create_machine(&req)
                .await
                .map_err(|e| api_error(&e))?;
            let install = install_command(&server_url, &r.machine, &r.token);
            Ok(MachineCreated {
                machine: machine(&r.machine),
                credential: SecretText::new(r.token),
                install_command: SecretText::new(install),
            })
        })
    }

    /// 用这台电脑: a Mac with a daemon config keeps it (its machine and token) and only moves
    /// the service to the app's own daemon; a new Mac is registered first.
    pub(super) fn use_this_mac_inner(&self) -> AdminReply<()> {
        let Some(daemon) = super::this_mac::bundled_daemon() else {
            return reply_now(Err(AdminError::Unsupported));
        };
        self.request(move |client, server_url| async move {
            let new = if super::this_mac::config_path().is_file() {
                None
            } else {
                let name = super::this_mac::computer_name();
                let labels = vec!["mac".to_string()];
                let req = CreateMachineRequest {
                    name: name.clone(),
                    labels: Some(labels.clone()),
                };
                let r = client
                    .http()
                    .create_machine(&req)
                    .await
                    .map_err(|e| api_error(&e))?;
                Some(super::this_mac::NewMachine {
                    server_url,
                    id: r.machine.id.clone(),
                    name: r.machine.name.clone(),
                    labels,
                    token: r.token,
                })
            };
            super::this_mac::install(&daemon, new.as_ref()).map_err(|e| {
                tracing::warn!(error = %e, "用这台电脑: install failed");
                AdminError::Other
            })
        })
    }

    pub(super) fn group_prefs_inner(&self, group_id: &str) -> GroupPrefs {
        let row = self
            .account()
            .and_then(|a| a.client.store().group(group_id).ok().flatten());
        match row {
            Some(row) => GroupPrefs {
                muted: row.muted,
                notify: notify_from_wire(row.notify_level()),
            },
            None => GroupPrefs {
                muted: false,
                notify: NotifyLevel::Mentions,
            },
        }
    }

    pub(super) fn set_group_prefs_inner(
        &self,
        group_id: &str,
        muted: Option<bool>,
        notify: Option<NotifyLevel>,
    ) -> AdminReply<()> {
        let gid = group_id.to_string();
        self.request(move |client, _| async move {
            let req = UpdateGroupPrefsRequest {
                muted,
                notify: notify.map(notify_to_wire),
            };
            client
                .set_group_prefs(&gid, req)
                .await
                .map_err(|_| AdminError::Other)?
                .map_err(|e| admin_error(&e))?;
            Ok(())
        })
    }

    pub(super) fn rename_group_inner(&self, group_id: &str, name: String) -> AdminReply<()> {
        let gid = group_id.to_string();
        self.request(move |client, _| async move {
            client
                .rename_group(&gid, &name)
                .await
                .map_err(|_| AdminError::Other)?
                .map_err(|e| {
                    tracing::warn!(error = %e, "rename group refused");
                    admin_error(&e)
                })?;
            Ok(())
        })
    }

    /// The purpose line of a 群 (`""` clears it); the server normalizes it.
    pub(super) fn set_group_purpose_inner(
        &self,
        group_id: &str,
        purpose: String,
    ) -> AdminReply<()> {
        let gid = group_id.to_string();
        let this = self.clone();
        self.request(move |client, _| async move {
            client
                .set_group_purpose(&gid, &purpose)
                .await
                .map_err(|_| AdminError::Other)?
                .map_err(|e| {
                    tracing::warn!(error = %e, "group purpose refused");
                    admin_error(&e)
                })?;
            this.emit(vec![Change::Groups]);
            Ok(())
        })
    }

    pub(super) fn send_diagnostics_inner(&self, description: String) -> AdminReply<String> {
        self.request(move |client, _| async move {
            let header = diagnostics_header(&description);
            client
                .send_diagnostics(&header)
                .await
                .map_err(|_| AdminError::Other)?
                .map_err(|e| admin_error(&e))
        })
    }
}

/// The top of a diagnostics report: what the user wrote, the app version and
/// the macOS version. Secrets in the description are removed by chat-core's
/// redaction with the rest of the report.
fn diagnostics_header(description: &str) -> String {
    let description: String = description.trim().chars().take(2000).collect();
    format!(
        "description: {description}\napp: {} {}\nos: {} {}\n",
        env!("PRODUCT_SLUG"),
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// The default notify level of a group kind (server rule).
#[allow(dead_code)]
pub(super) fn default_notify(kind: GroupKind) -> NotifyLevel {
    match kind {
        GroupKind::Dm => NotifyLevel::All,
        GroupKind::Group => NotifyLevel::Mentions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_machine() -> proto::Machine {
        serde_json::from_value(serde_json::json!({
            "id": "mc_1", "name": "mini \"x\"", "labels": ["mac", "gpu"], "createdBy": "m1",
            "createdAt": 1, "connected": true, "healthy": true, "runningRuns": ["r1"],
            "health": { "diskFreeBytes": 10, "diskTotalBytes": 20, "load1": 0.0, "load5": 0.0,
                        "load15": 0.0, "cpuCount": 8, "activeRuns": 1 },
            "clis": [{ "runtime": "rt-a", "kind": "pty", "command": "x", "available": true },
                     { "runtime": "rt-b", "kind": "pty", "command": "y", "available": false }]
        }))
        .expect("machine json")
    }

    #[test]
    fn machine_maps_health_and_runtimes() {
        let m = machine(&sample_machine());
        assert_eq!(m.disk_free_bytes, Some(10));
        assert_eq!(m.active_runs, 1);
        assert_eq!(m.runtimes, vec!["rt-a".to_string()]);
        assert_eq!(m.labels, vec!["mac".to_string(), "gpu".to_string()]);
    }

    #[test]
    fn install_command_quotes_values() {
        let cmd = install_command("https://x.example", &sample_machine(), "tm_a'b");
        assert!(cmd.contains("printf '%s' 'tm_a'\\''b'"));
        assert!(cmd.contains("name = \"mini \\\"x\\\"\""));
        assert!(cmd.contains("labels = [\"mac\", \"gpu\"]"));
        assert!(cmd.contains("server_url = \"https://x.example\""));
        assert!(cmd.ends_with("install-service"));
        // The daemon comes from the installed app, with a PATH fallback.
        let slug = env!("PRODUCT_SLUG");
        assert!(cmd.contains(&format!(
            ".app/Contents/MacOS/{slug}-daemon\"\n[ -x \"$D\" ] || D=\"$(command -v {slug}-daemon)\""
        )));
        assert!(cmd.contains("\"$D\" doctor && \"$D\" install-service"));
    }

    #[test]
    fn errors_map() {
        let conflict = ApiError::Server {
            status: 409,
            code: ErrorCode::Conflict,
            message: String::new(),
        };
        assert_eq!(api_error(&conflict), AdminError::LastOwner);
        assert_eq!(
            api_error(&ApiError::Network("x".into())),
            AdminError::Offline
        );
    }

    #[test]
    fn header_keeps_description() {
        let h = diagnostics_header("  发送失败  ");
        assert!(h.starts_with("description: 发送失败\n"));
    }
}
