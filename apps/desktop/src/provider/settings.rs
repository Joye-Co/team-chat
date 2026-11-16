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
    AdminError, AdminReply, BotCreated, BotMemberEdit, Change, CharacterId, CliChoice, GroupKind,
    GroupPrefs, InviteCreated, InviteRequest, MachineCreated, MachineInfo, Member, MemberKind,
    MemberLevel, ModelChoice, NotifyLevel, RunnerChange, RunnerSetup, SecretText, TeamRole,
    reply_now,
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

/// What the server said to forgetting a computer: one that is gone already is forgotten.
fn forget_machine(r: Result<(), ApiError>) -> Result<(), AdminError> {
    match r {
        Ok(()) => Ok(()),
        Err(e) if machine_error(&e) == AdminError::NotFound => Ok(()),
        Err(e) => Err(machine_error(&e)),
    }
}

/// [`api_error`] for the computer routes: a conflict there is a change made meanwhile (the
/// computer's owner changed), not the last 所有者.
pub(super) fn machine_error(e: &ApiError) -> AdminError {
    match api_error(e) {
        AdminError::LastOwner => AdminError::Conflict,
        other => other,
    }
}

fn level_to_wire(level: MemberLevel) -> proto::MemberLevel {
    match level {
        MemberLevel::Owner => proto::MemberLevel::Owner,
        MemberLevel::Member => proto::MemberLevel::Member,
    }
}

/// The member update that gives a 身份: 所有者 and 管理员 are both `level: owner`, told apart by
/// `teamOwner`; `level: member` clears `teamOwner` on the server.
fn role_request(role: TeamRole) -> UpdateMemberRequest {
    let (level, team_owner) = match role {
        TeamRole::Owner => (MemberLevel::Owner, Some(true)),
        TeamRole::Admin => (MemberLevel::Owner, Some(false)),
        TeamRole::Member => (MemberLevel::Member, None),
    };
    UpdateMemberRequest {
        level: Some(level_to_wire(level)),
        team_owner,
        ..Default::default()
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
        model: non_empty(runner.model),
        // Always sent: `null` goes back to the CLI's default.
        effort: Some(non_empty(runner.effort)),
        ..Default::default()
    }
}

/// The PUT for a runner edit. PUT replaces the whole runner, so what the sheets do not show
/// (allowed env, on/off) is kept from the stored one; the model and 思考强度 are the sheet's.
fn runner_update(setup: RunnerSetup, old: Option<&proto::BotRunner>) -> SetBotRunnerRequest {
    let mut req = runner_request(setup);
    if let Some(old) = old {
        req.env_allowlist = Some(old.env_allowlist.clone());
        req.enabled = Some(old.enabled);
    }
    req
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
        model: r.model.clone(),
        effort: r.effort.clone(),
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
        unhealthy: m.unhealthy.clone(),
        last_heartbeat_at: m.last_heartbeat_at.map(|v| v as i64),
        version: m.daemon_version.clone(),
        disk_free_bytes: health.map(|h| h.disk_free_bytes),
        disk_total_bytes: health.map(|h| h.disk_total_bytes),
        active_runs: health
            .map(|h| h.active_runs)
            .unwrap_or(m.running_runs.len() as u32)
            .max(m.running_runs.len() as u32),
        max_parallel_runs: m.max_parallel_runs,
        parallel_runs: m.parallel_runs,
        limits: m
            .features
            .iter()
            .flatten()
            .any(|f| f == proto::machine::feature::LIMITS),
        runtimes,
        owner_id: m.owner_id.clone(),
        personal_bots: m
            .personal_bots
            .iter()
            .flatten()
            .map(|b| crate::data::PersonalBot {
                bot_id: b.bot_id.clone(),
                repo_path: b.repo_path.clone(),
            })
            .collect(),
        clis: m
            .clis
            .iter()
            .flatten()
            .filter(|c| c.available)
            .map(|c| CliChoice {
                runtime: c.runtime.clone(),
                kind: c.kind.clone(),
                models: c
                    .models
                    .iter()
                    .flatten()
                    .map(|x| ModelChoice {
                        id: x.id.clone(),
                        name: x.name.clone(),
                        efforts: x.efforts.clone().unwrap_or_default(),
                        is_default: x.is_default == Some(true),
                    })
                    .collect(),
                efforts: c.efforts.clone().unwrap_or_default(),
            })
            .collect(),
        // Usable: the computer's own GitHub sign-in (admins and its owner only).
        github: m.github.as_ref().map(|g| crate::data::MachineGithub {
            state: g.state.clone(),
            login: g.login.clone(),
        }),
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

/// Shell lines for a Mac that write the daemon config and token (mode 600) and install
/// the launchd service. The daemon ships inside the app (`Contents/MacOS/<slug>-daemon`,
/// see scripts/build-dmg.sh), so the Mac needs the app installed; a `<slug>-daemon` on
/// `PATH` is the fallback. A Linux computer has [`install_command_linux`].
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

/// The release platform of the Linux daemon (server `ReleasePlatform`).
const LINUX_PLATFORM: &str = "linux-x86_64";

/// Shell lines for a Linux computer (机 1.8): the same config and token as [`install_command`],
/// then the released Linux daemon, downloaded from this server with the machine token (sent in
/// a curl config on standard input, never on a command line) and checked against the SHA-256
/// the server gives, into `~/.local/bin`, where it later updates itself
/// (docs/machine-protocol.md, "Updating itself"); then `doctor` and `install-service` (a
/// systemd user unit). Any step that fails stops the rest and the last line says so.
pub(super) fn install_command_linux(
    server_url: &str,
    machine: &proto::Machine,
    token: &str,
) -> String {
    let slug = env!("PRODUCT_SLUG");
    let dir = format!("$HOME/.config/{slug}");
    let labels = machine
        .labels
        .iter()
        .map(|l| toml_str(l))
        .collect::<Vec<_>>()
        .join(", ");
    let latest = format!(
        "{}/v1/machine/updates/latest?platform={LINUX_PLATFORM}",
        server_url.trim_end_matches('/')
    );
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
         B=\"$HOME/.local/bin\"; D=\"$B/{slug}-daemon\"\n\
         A=\"header = \\\"Authorization: Bearer $(cat \"{dir}/machine-token\")\\\"\"\n\
         I=\"$(printf '%s\\n' \"$A\" | curl -fsSL -K - {latest})\"\n\
         U=\"$(printf '%s' \"$I\" | sed -n 's/.*\"downloadUrl\":\"\\([^\"]*\\)\".*/\\1/p')\"\n\
         H=\"$(printf '%s' \"$I\" | sed -n 's/.*\"sha256\":\"\\([0-9a-f]\\{{64\\}}\\)\".*/\\1/p')\"\n\
         [ \"$(uname -m)\" = x86_64 ] && [ -n \"$U\" ] && [ -n \"$H\" ] && mkdir -p \"$B\" \\\n\
         && printf '%s\\n' \"$A\" | curl -fsSL -K - -o \"$D.install\" \"$U\" \\\n\
         && printf '%s  %s\\n' \"$H\" \"$D.install\" | sha256sum -c - \\\n\
         && chmod 755 \"$D.install\" && mv -f \"$D.install\" \"$D\" \\\n\
         && \"$D\" doctor && \"$D\" install-service \\\n\
         || printf '%s\\n' {failed}",
        token = shell_quote(token),
        server = toml_str(server_url),
        id = toml_str(&machine.id),
        name = toml_str(&machine.name),
        latest = shell_quote(&latest),
        failed = shell_quote(crate::copy::settings::MACHINE_INSTALL_LINUX_FAILED),
    )
}

/// Quotes a value as a PowerShell literal string.
fn powershell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// PowerShell lines that do on a Windows computer what [`install_command`] does on a Mac
/// or Linux one: write the daemon config and the token into a folder only that user,
/// SYSTEM and Administrators can open (the daemon refuses anything wider), then run the
/// daemon's `doctor` and `install-service` (a scheduled task of that user). The daemon is
/// the one the installed app ships (`<install folder>\<slug>-daemon.exe`, see
/// [`install_command_windows_for`]); a `<slug>-daemon` on `PATH` is the fallback; with neither the command says so in a
/// sentence and stops. Nothing is downloaded, so no hash is needed; a computer without the app uses `scripts/install-daemon.ps1` (docs/bots.md).
///
/// Works in Windows PowerShell 5.1. The config is a literal here-string: the values are
/// TOML strings on one line each ([`toml_str`] escapes quotes, `$`, backticks and control
/// characters), so none can end it.
pub(super) fn install_command_windows(
    server_url: &str,
    machine: &proto::Machine,
    token: &str,
) -> String {
    install_command_windows_for(server_url, machine, token, env!("PRODUCT_BUNDLE_ID"))
}

/// [`install_command_windows`] for an install recorded under the uninstall key
/// `uninstall_id` (the app id; tests use one of their own).
///
/// Where the daemon is looked for, in this order: the folder the installer recorded
/// (`InstallLocation` of the app's uninstall key; an install made under an earlier name
/// of the product stays in that name's folder when it updates), the folder of the current
/// name, the folders of the earlier names, then `PATH`.
pub(super) fn install_command_windows_for(
    server_url: &str,
    machine: &proto::Machine,
    token: &str,
    uninstall_id: &str,
) -> String {
    let slug = env!("PRODUCT_SLUG");
    let app = env!("PRODUCT_NAME");
    let names = std::iter::once(app)
        .chain(env!("PRODUCT_FORMER_NAMES").split('|'))
        .filter(|name| !name.is_empty())
        .map(powershell_quote)
        .collect::<Vec<_>>()
        .join(", ");
    let uninstall = powershell_quote(&format!(
        r"HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{uninstall_id}"
    ));
    let labels = machine
        .labels
        .iter()
        .map(|l| toml_str(l))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "$d = \"$env:USERPROFILE\\.config\\{slug}\"\n\
         New-Item -ItemType Directory -Force $d | Out-Null\n\
         $me = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value\n\
         icacls $d /inheritance:r /grant:r \"*${{me}}:(OI)(CI)F\" \"*S-1-5-18:(OI)(CI)F\" \"*S-1-5-32-544:(OI)(CI)F\" | Out-Null\n\
         [IO.File]::WriteAllText(\"$d\\machine-token\", {token})\n\
         $toml = @'\n\
         server_url = {server}\n\
         machine_id = {id}\n\
         name = {name}\n\
         labels = [{labels}]\n\
         '@\n\
         $toml += \"`ntoken_file = `\"$($d.Replace('\\', '/'))/machine-token`\"`n\"\n\
         [IO.File]::WriteAllText(\"$d\\daemon.toml\", $toml)\n\
         $dirs = @((Get-ItemProperty {uninstall} -ErrorAction SilentlyContinue).InstallLocation)\n\
         $dirs += @({names}) | ForEach-Object {{ Join-Path $env:LOCALAPPDATA \"Programs\\$_\" }}\n\
         $D = $dirs | Where-Object {{ $_ }} | ForEach-Object {{ Join-Path $_ '{slug}-daemon.exe' }} | Where-Object {{ Test-Path $_ }} | Select-Object -First 1\n\
         if (-not $D) {{ $D = (Get-Command {slug}-daemon -ErrorAction SilentlyContinue).Source }}\n\
         if ($D) {{ & $D doctor; if ($LASTEXITCODE -eq 0) {{ & $D install-service }} }} else {{ Write-Host {missing} }}",
        token = powershell_quote(token),
        missing = powershell_quote(&crate::copy::fill(
            crate::copy::settings::MACHINE_INSTALL_NO_DAEMON,
            &[("app", app)]
        )),
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
        self.upload_member_avatar_inner(me, path)
    }

    pub(super) fn remove_member_avatar_inner(&self, id: String) -> AdminReply<Member> {
        self.request(move |client, _| async move {
            let m = client
                .remove_avatar(&id)
                .await
                .map_err(|_| AdminError::Other)?
                .map_err(|e| admin_error(&e))?;
            let online = client.is_online(&m.id);
            Ok(convert::member(&m, online))
        })
    }

    pub(super) fn upload_member_avatar_inner(
        &self,
        me: String,
        path: PathBuf,
    ) -> AdminReply<Member> {
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
            let online = client.is_online(&m.id);
            Ok(convert::member(&m, online))
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

    pub(super) fn set_member_role_inner(&self, id: &str, role: TeamRole) -> AdminReply<Member> {
        self.patch_member(id.to_string(), role_request(role))
    }

    pub(super) fn set_member_deactivated_inner(&self, id: &str, off: bool) -> AdminReply<Member> {
        let req = UpdateMemberRequest {
            deactivated: Some(off),
            ..Default::default()
        };
        self.patch_member(id.to_string(), req)
    }

    /// hide-delete/1: deletes a deactivated AI member for good; the cache keeps it as
    /// `deleted` for its old messages.
    pub(super) fn delete_member_inner(&self, id: &str) -> AdminReply<Member> {
        let id = id.to_string();
        self.request(move |client, _| async move {
            let m = client
                .delete_member(&id)
                .await
                .map_err(|_| AdminError::Other)?
                .map_err(|e| admin_error(&e))?;
            Ok(convert::member(&m, false))
        })
    }

    /// 限额 alone (owners, 用量 › 改预算): `None` clears; 汇报给 stays.
    pub(super) fn set_member_budget_inner(
        &self,
        id: &str,
        cents: Option<u64>,
    ) -> AdminReply<Member> {
        let req = UpdateMemberRequest {
            weekly_budget_cents: Some(cents),
            ..Default::default()
        };
        if req.validate().is_err() {
            return reply_now(Err(AdminError::Invalid));
        }
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
                    let old = http.bot_runner(&id).await.map_err(|e| api_error(&e))?;
                    let req = runner_update(setup, old.as_ref());
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
        personal: Option<bool>,
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
                personal,
            };
            let r = client
                .http()
                .create_machine(&req)
                .await
                .map_err(|e| api_error(&e))?;
            let install = install_command(&server_url, &r.machine, &r.token);
            let install_windows = install_command_windows(&server_url, &r.machine, &r.token);
            let install_linux = install_command_linux(&server_url, &r.machine, &r.token);
            Ok(MachineCreated {
                machine: machine(&r.machine),
                credential: SecretText::new(r.token),
                install_command: SecretText::new(install),
                install_command_windows: SecretText::new(install_windows),
                install_command_linux: SecretText::new(install_linux),
            })
        })
    }

    /// `PATCH /v1/machines/:id`: a new name, or `owner` (`Some(Some("me"))` 改成我的电脑,
    /// `Some(None)` 交给团队).
    pub(super) fn update_machine_inner(
        &self,
        machine_id: &str,
        name: Option<String>,
        owner: Option<Option<String>>,
        parallel_runs: Option<Option<u32>>,
    ) -> AdminReply<MachineInfo> {
        let name = name.map(|n| n.trim().to_string());
        if name.as_deref() == Some("") {
            return reply_now(Err(AdminError::Invalid));
        }
        let id = machine_id.to_string();
        self.request(move |client, _| async move {
            let req = proto::UpdateMachineRequest {
                name,
                owner,
                parallel_runs,
            };
            let m = client
                .http()
                .update_machine(&id, &req)
                .await
                .map_err(|e| machine_error(&e))?;
            Ok(machine(&m))
        })
    }

    /// `DELETE /v1/machines/:id`; one that is already gone counts as removed.
    pub(super) fn remove_machine_inner(&self, machine_id: &str) -> AdminReply<()> {
        let id = machine_id.to_string();
        self.request(move |client, _| async move {
            match client.http().delete_machine(&id).await {
                Ok(()) => Ok(()),
                Err(e) if machine_error(&e) == AdminError::NotFound => Ok(()),
                Err(e) => Err(machine_error(&e)),
            }
        })
    }

    /// My work for `bot_id` on my computer: `Some(repo)` sets it (with my code folder, or
    /// none), `None` takes it away.
    pub(super) fn machine_bot_inner(
        &self,
        machine_id: &str,
        bot_id: &str,
        set: Option<Option<String>>,
    ) -> AdminReply<MachineInfo> {
        let (id, bot) = (machine_id.to_string(), bot_id.to_string());
        let set = set.map(|repo| repo.map(|r| r.trim().to_string()).filter(|r| !r.is_empty()));
        self.request(move |client, _| async move {
            let http = client.http();
            let m = match set {
                Some(repo_path) => {
                    let req = proto::SetMachineBotRequest {
                        repo_path: Some(repo_path),
                    };
                    http.set_machine_bot(&id, &bot, &req).await
                }
                None => http.delete_machine_bot(&id, &bot).await,
            }
            .map_err(|e| machine_error(&e))?;
            Ok(machine(&m))
        })
    }

    /// 不再用这台电脑 (机 1.6): the service and the files that registered this computer are
    /// removed here first, then the server forgets it (its token stops working; one that is
    /// gone already, removed from another device, is fine). A failure on this computer leaves
    /// the registration, so the row and its "⋯" stay to try again; once the files are gone the
    /// row is no longer this computer and 移除… on it finishes the rest.
    pub(super) fn stop_using_this_computer_inner(&self) -> AdminReply<()> {
        let Some(daemon) = super::this_mac::bundled_daemon() else {
            return reply_now(Err(AdminError::Unsupported));
        };
        let id = super::this_mac::machine_id();
        self.request(move |client, _| async move {
            super::this_mac::stop_using(&daemon).map_err(|e| {
                tracing::warn!(error = %e, "不再用这台电脑: uninstall failed");
                AdminError::Other
            })?;
            match id {
                Some(id) => forget_machine(client.http().delete_machine(&id).await),
                None => Ok(()),
            }
        })
    }

    /// 用这台电脑: a Mac with a daemon config keeps it (its machine and token) and only moves
    /// the service to the app's own daemon; a new Mac is registered first.
    pub(super) fn use_this_mac_inner(&self) -> AdminReply<()> {
        let Some(daemon) = super::this_mac::bundled_daemon() else {
            return reply_now(Err(AdminError::Unsupported));
        };
        self.request(move |client, server_url| async move {
            super::this_mac::set_up(&daemon, &client.http(), server_url)
                .await
                .map_err(|e| match e {
                    super::this_mac::SetUpError::Api(e) => api_error(&e),
                    super::this_mac::SetUpError::Install(e) => {
                        tracing::warn!(error = %e, "用这台电脑: install failed");
                        AdminError::Other
                    }
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
                hidden: None,
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

    /// 不再用这台电脑 after this computer was removed elsewhere (another device, an admin):
    /// the server no longer knows it, and that is the end of it, not a failure.
    #[test]
    fn forgetting_a_computer_that_is_gone_is_done() {
        assert_eq!(forget_machine(Ok(())), Ok(()));
        assert_eq!(
            forget_machine(Err(ApiError::Unexpected { status: 404 })),
            Ok(())
        );
        assert_eq!(
            forget_machine(Err(ApiError::Unexpected { status: 403 })),
            Err(AdminError::Forbidden)
        );
        assert_eq!(
            forget_machine(Err(ApiError::NoToken)),
            Err(AdminError::Offline)
        );
    }

    /// A model changed in the full edit sheet is saved; allowed env and on/off stay as stored.
    #[test]
    fn a_runner_edit_saves_the_sheets_model_and_keeps_the_rest() {
        let old: proto::BotRunner = serde_json::from_value(serde_json::json!({
            "botId": "b1", "runtime": "claude", "labels": [], "model": "old-model",
            "effort": "low", "envAllowlist": ["FOO"], "enabled": false, "updatedAt": 1
        }))
        .expect("runner json");
        let setup = RunnerSetup {
            runtime: "claude".into(),
            labels: vec![],
            repo: None,
            base_branch: None,
            machine_id: None,
            model: Some("new-model".into()),
            effort: Some("high".into()),
        };
        let req = runner_update(setup.clone(), Some(&old));
        assert_eq!(req.model.as_deref(), Some("new-model"));
        assert_eq!(req.effort, Some(Some("high".into())));
        assert_eq!(req.env_allowlist, Some(vec!["FOO".to_string()]));
        assert_eq!(req.enabled, Some(false));
        // Back to the default: the stored model is not put back.
        let cleared = runner_update(
            RunnerSetup {
                model: None,
                ..setup
            },
            Some(&old),
        );
        assert_eq!(cleared.model, None);
    }

    #[test]
    fn machine_maps_health_and_runtimes() {
        let m = machine(&sample_machine());
        assert_eq!(m.disk_free_bytes, Some(10));
        assert_eq!(m.active_runs, 1);
        assert_eq!(m.runtimes, vec!["rt-a".to_string()]);
        assert_eq!(m.labels, vec!["mac".to_string(), "gpu".to_string()]);
    }

    /// The Windows command writes the same config as the Mac one, quotes the token for
    /// PowerShell, makes the folder private before the token is written, and runs the
    /// installed app's daemon.
    #[test]
    fn the_windows_install_command_is_powershell() {
        let cmd = install_command_windows("https://x.example", &sample_machine(), "tm_a'b");
        let line = |start: &str| {
            cmd.lines()
                .position(|l| l.starts_with(start))
                .unwrap_or_else(|| panic!("no line starting with {start:?} in:\n{cmd}"))
        };
        assert!(cmd.contains("\\machine-token\", 'tm_a''b')"), "{cmd}");
        assert!(cmd.contains("name = \"mini \\\"x\\\"\""), "{cmd}");
        assert!(cmd.contains("labels = [\"mac\", \"gpu\"]"));
        assert!(cmd.contains("server_url = \"https://x.example\""));
        assert!(line("icacls") < line("[IO.File]::WriteAllText(\"$d\\machine-token"));
        assert!(cmd.contains("/inheritance:r") && cmd.contains("*S-1-5-18:(OI)(CI)F"));
        // The here-string is literal and closes on its own line.
        assert!(line("$toml = @'") < line("'@"));
        assert!(cmd.contains("token_file = `\"$($d.Replace('\\', '/'))/machine-token`\""));
        // The daemon is looked for where the installer says the app is, then under the
        // current and the former names, then on PATH.
        let (slug, app, id) = (
            env!("PRODUCT_SLUG"),
            env!("PRODUCT_NAME"),
            env!("PRODUCT_BUNDLE_ID"),
        );
        assert!(cmd.contains(&format!(
            "Get-ItemProperty 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{id}' -ErrorAction SilentlyContinue).InstallLocation"
        )));
        let names = line("$dirs += @(");
        let names = cmd.lines().nth(names).unwrap();
        assert!(names.contains(&format!("'{app}'")), "{names}");
        for former in env!("PRODUCT_FORMER_NAMES")
            .split('|')
            .filter(|n| !n.is_empty())
        {
            assert!(names.contains(&format!("'{former}'")), "{names}");
        }
        assert!(
            line("$dirs = @(") < line("$dirs += @(") && line("$dirs += @(") < line("$D = $dirs")
        );
        assert!(cmd.contains(&format!("Join-Path $_ '{slug}-daemon.exe'")));
        assert!(cmd.contains(&format!("Get-Command {slug}-daemon")));
        assert!(cmd.contains(
            "if ($D) { & $D doctor; if ($LASTEXITCODE -eq 0) { & $D install-service } }"
        ));
        // Without a daemon the last line says what to do (one statement, so it also
        // works pasted line by line).
        assert!(cmd.ends_with("。' }"), "{cmd}");
        assert!(cmd.lines().last().unwrap().contains("else { Write-Host '"));
        // No POSIX shell in it.
        for posix in ["chmod", "umask", "<<EOF", "$HOME", "/Applications"] {
            assert!(!cmd.contains(posix), "{posix}");
        }
        // A value cannot close the here-string or run as PowerShell.
        let mut machine = sample_machine();
        machine.name = "a\n'@\n$(calc) `x`".into();
        let cmd = install_command_windows("https://x.example", &machine, "t");
        assert_eq!(
            cmd.lines().filter(|l| l.starts_with("'@")).count(),
            1,
            "{cmd}"
        );
        assert!(!cmd.contains("$(calc)"), "{cmd}");
    }

    /// Runs an install command in Windows PowerShell 5.1 with `home` as the user's
    /// profile and `home\local` as its local app data.
    #[cfg(target_os = "windows")]
    fn run_install(cmd: &str, home: &std::path::Path) -> std::process::Output {
        let script = home.join("install.ps1");
        // With a BOM, as Windows PowerShell needs to read a UTF-8 script file.
        std::fs::write(&script, format!("\u{feff}{cmd}\n")).unwrap();
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        std::process::Command::new(
            std::path::Path::new(&root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe"),
        )
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script)
        .env("USERPROFILE", home)
        .env("LOCALAPPDATA", home.join("local"))
        .output()
        .unwrap()
    }

    /// The command finds the daemon of an installed app wherever the install is: where
    /// the installer's uninstall key says (an install made under the product's former
    /// name stays in that name's folder after it updates), else in the folder of a
    /// former name, else in the folder of the current name. The "daemon" here is this
    /// test program under the daemon's name: run with `doctor` it runs no test and exits
    /// with 0, and says so.
    #[cfg(target_os = "windows")]
    #[test]
    fn the_windows_install_command_finds_an_install_under_a_former_name() {
        use windows_registry::CURRENT_USER;
        let id = format!("tc-test-{}", chat_core::new_id());
        let key = format!(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{id}");
        let cmd = install_command_windows_for("https://x.example", &sample_machine(), "tm_x", &id);
        let daemon = concat!(env!("PRODUCT_SLUG"), "-daemon.exe");
        let me = std::env::current_exe().unwrap();
        let sentence = crate::copy::fill(
            crate::copy::settings::MACHINE_INSTALL_NO_DAEMON,
            &[("app", env!("PRODUCT_NAME"))],
        );
        let ran = |out: &std::process::Output| {
            let said = String::from_utf8_lossy(&out.stdout);
            println!("{said}\n{}", String::from_utf8_lossy(&out.stderr));
            (
                said.contains("running"),
                said.contains(env!("PRODUCT_NAME")) && !said.contains("running"),
            )
        };
        let _ = sentence;

        // 1. Recorded by the installer, in a folder no name would guess.
        let home = tempfile::tempdir().unwrap();
        let install = home
            .path()
            .join("local")
            .join("Programs")
            .join("An Older Name");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::copy(&me, install.join(daemon)).unwrap();
        CURRENT_USER
            .create(&key)
            .unwrap()
            .set_string("InstallLocation", install.display().to_string())
            .unwrap();
        let out = run_install(&cmd, home.path());
        let _ = CURRENT_USER.remove_tree(&key);
        assert_eq!(ran(&out), (true, false), "found through InstallLocation");

        // 2. No record: the folder of a former name, then of the current name.
        let mut names: Vec<&str> = env!("PRODUCT_FORMER_NAMES")
            .split('|')
            .filter(|n| !n.is_empty())
            .collect();
        names.push(env!("PRODUCT_NAME"));
        for name in names {
            let home = tempfile::tempdir().unwrap();
            let install = home.path().join("local").join("Programs").join(name);
            std::fs::create_dir_all(&install).unwrap();
            std::fs::copy(&me, install.join(daemon)).unwrap();
            let out = run_install(&cmd, home.path());
            assert_eq!(ran(&out), (true, false), "found in Programs\\{name}");
        }

        // 3. Nowhere: the sentence, and nothing is run.
        let home = tempfile::tempdir().unwrap();
        let out = run_install(&cmd, home.path());
        assert_eq!(ran(&out), (false, true), "says to install the app");
    }

    /// The command really runs in Windows PowerShell 5.1: it leaves a config the daemon
    /// reads and the token, in a folder with exactly the three entries the daemon accepts.
    /// (No daemon is installed in the test's profile, so the last line finds none.)
    #[cfg(target_os = "windows")]
    #[test]
    fn the_windows_install_command_runs_in_powershell() {
        let mut machine = sample_machine();
        machine.name = "会议室 \"mini\" $x `y`".into();
        let cmd = install_command_windows("https://x.example", &machine, "tm_a'b$c");
        let home = tempfile::tempdir().unwrap();
        let script = home.path().join("install.ps1");
        // With a BOM, as Windows PowerShell needs to read a UTF-8 script file.
        std::fs::write(&script, format!("\u{feff}{cmd}\n")).unwrap();
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let run = |program: &str, args: &[&std::ffi::OsStr]| {
            std::process::Command::new(std::path::Path::new(&root).join(program))
                .args(args)
                .env("USERPROFILE", home.path())
                .env("LOCALAPPDATA", home.path().join("local"))
                .output()
                .unwrap()
        };
        let out = run(
            r"System32\WindowsPowerShell\v1.0\powershell.exe",
            &[
                "-NoProfile".as_ref(),
                "-NonInteractive".as_ref(),
                "-ExecutionPolicy".as_ref(),
                "Bypass".as_ref(),
                "-File".as_ref(),
                script.as_os_str(),
            ],
        );
        // No daemon in this profile: the command says what to do, not a PowerShell error.
        let said = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "the script ends cleanly");
        assert!(
            out.stderr.is_empty(),
            "no PowerShell error: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(said.contains(env!("PRODUCT_NAME")), "{said}");
        println!(
            "powershell: {}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let dir = home.path().join(".config").join(env!("PRODUCT_SLUG"));
        assert_eq!(
            std::fs::read_to_string(dir.join("machine-token")).unwrap(),
            "tm_a'b$c"
        );
        let toml = std::fs::read_to_string(dir.join("daemon.toml")).unwrap();
        println!("daemon.toml:\n{toml}");
        assert!(!toml.starts_with('\u{feff}'), "no BOM");
        assert!(toml.contains("server_url = \"https://x.example\""));
        assert!(toml.contains("machine_id = "));
        assert!(toml.contains("labels = [\"mac\", \"gpu\"]"));
        // The name comes back as it was, through TOML's own escapes.
        assert!(
            toml.contains(r#"name = "会议室 \"mini\" \u0024x \u0060y\u0060""#),
            "{toml}"
        );
        let token_file = toml
            .lines()
            .find_map(|l| l.strip_prefix("token_file = \""))
            .and_then(|l| l.strip_suffix('"'))
            .expect("a token_file line");
        assert!(!token_file.contains('\\'), "forward slashes: {token_file}");
        assert_eq!(
            std::fs::read_to_string(token_file).unwrap(),
            "tm_a'b$c",
            "the config points at the token"
        );
        // The folder is private: nothing inherited, three full-control entries.
        let acl = run(r"System32\icacls.exe", &[dir.as_os_str()]);
        let acl = String::from_utf8_lossy(&acl.stdout).into_owned();
        assert!(!acl.contains("(I)"), "inherited entries: {acl}");
        assert_eq!(acl.matches("(OI)(CI)(F)").count(), 3, "{acl}");
    }

    /// The Linux command (机 1.8) writes the config, downloads the daemon this server releases
    /// with the machine token kept off every command line, checks its SHA-256, installs it in
    /// `~/.local/bin` and runs `doctor` and `install-service`; a wrong hash or another
    /// processor stops it before anything is installed, with one line that says so. `curl`
    /// and `uname` are stand-ins; the hash check is the real one.
    #[cfg(unix)]
    #[test]
    fn the_linux_install_command_downloads_and_checks_the_daemon() {
        use sha2::Digest as _;
        use std::os::unix::fs::PermissionsExt as _;
        let tmp = tempfile::tempdir().unwrap();
        let fake = tmp.path().join("bin");
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&fake).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let script = |name: &str, body: String| {
            let p = fake.join(name);
            std::fs::write(&p, format!("#!/bin/sh\n{body}")).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        let f = fake.display().to_string();
        let daemon = format!("#!/bin/sh\necho \"$1\" >> '{f}/daemon-log'\n");
        std::fs::write(fake.join("daemon-bin"), &daemon).unwrap();
        let good = format!("{:x}", sha2::Sha256::digest(daemon.as_bytes()));
        script(
            "curl",
            format!(
                "cat >> '{f}/curl-config'\nout=''; url=''\n\
                 while [ $# -gt 0 ]; do case \"$1\" in -o) out=\"$2\"; shift 2;; -*) shift;; *) url=\"$1\"; shift;; esac; done\n\
                 echo \"$url\" >> '{f}/urls'\n\
                 if [ -n \"$out\" ]; then cp '{f}/daemon-bin' \"$out\"; else printf '{{\"version\":\"9.9.9\",\"sha256\":\"%s\",\"downloadUrl\":\"https://x.example/v1/machine/updates/9.9.9/download?platform=linux-x86_64\"}}' \"$(cat '{f}/hash')\"; fi\n"
            ),
        );
        script("uname", format!("cat '{f}/arch'\n"));
        script(
            "sha256sum",
            "if [ -x /usr/bin/sha256sum ]; then exec /usr/bin/sha256sum \"$@\"; else exec shasum -a 256 \"$@\"; fi\n"
                .into(),
        );
        let cmd = install_command_linux("https://x.example/", &sample_machine(), "tm_a'b");
        let run = |hash: &str, arch: &str| {
            std::fs::write(fake.join("hash"), hash).unwrap();
            std::fs::write(fake.join("arch"), arch).unwrap();
            let out = std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(&cmd)
                .env("HOME", &home)
                .env("PATH", format!("{f}:/usr/bin:/bin"))
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).into_owned()
        };
        let slug = env!("PRODUCT_SLUG");
        let installed = home.join(".local/bin").join(format!("{slug}-daemon"));
        let failed = crate::copy::settings::MACHINE_INSTALL_LINUX_FAILED;

        // Another processor: nothing is downloaded or installed.
        assert!(run(&good, "aarch64").contains(failed));
        assert!(!installed.exists());
        // A hash that does not match: the download is not put in place.
        assert!(run(&"0".repeat(64), "x86_64").contains(failed));
        assert!(!installed.exists());

        let said = run(&good, "x86_64");
        assert!(!said.contains(failed), "{said}");
        assert_eq!(std::fs::read_to_string(&installed).unwrap(), daemon);
        let mode = std::fs::metadata(&installed).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
        let log = std::fs::read_to_string(fake.join("daemon-log")).unwrap();
        assert_eq!(log, "doctor\ninstall-service\n");
        let cfg = home.join(".config").join(slug);
        assert_eq!(
            std::fs::read_to_string(cfg.join("machine-token")).unwrap(),
            "tm_a'b"
        );
        let toml = std::fs::read_to_string(cfg.join("daemon.toml")).unwrap();
        assert!(
            toml.contains("server_url = \"https://x.example/\""),
            "{toml}"
        );
        assert!(toml.contains(&format!("token_file = \"{}/machine-token\"", cfg.display())));
        // The token goes in curl's config on standard input, never in a URL or an argument.
        let config = std::fs::read_to_string(fake.join("curl-config")).unwrap();
        assert!(
            config.contains("header = \"Authorization: Bearer tm_a'b\""),
            "{config}"
        );
        let urls = std::fs::read_to_string(fake.join("urls")).unwrap();
        assert!(!urls.contains("tm_"));
        assert!(
            urls.starts_with("https://x.example/v1/machine/updates/latest?platform=linux-x86_64\n")
        );
        assert!(urls.contains("/v1/machine/updates/9.9.9/download?platform=linux-x86_64"));
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
    fn roles_go_on_the_wire_as_level_and_team_owner() {
        let json = |r| serde_json::to_value(role_request(r)).unwrap();
        assert_eq!(
            json(TeamRole::Owner),
            serde_json::json!({ "level": "owner", "teamOwner": true })
        );
        assert_eq!(
            json(TeamRole::Admin),
            serde_json::json!({ "level": "owner", "teamOwner": false })
        );
        assert_eq!(
            json(TeamRole::Member),
            serde_json::json!({ "level": "member" })
        );
    }

    #[test]
    fn header_keeps_description() {
        let h = diagnostics_header("  发送失败  ");
        assert!(h.starts_with("description: 发送失败\n"));
    }
}
