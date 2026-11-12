//! 用这台电脑 (设置 › 机器): makes this Mac one of the team's computers without a terminal.
//! The daemon ships inside the app (`Contents/MacOS/<slug>-daemon`, see scripts/build-dmg.sh).
//! The app writes the daemon config and machine token (mode 600) the way the install command
//! does, then runs `<daemon> install-service`, so launchd runs the daemon from the app bundle.
//! An app update replaces that file, and the daemon restarts itself on the new version
//! (`daemon::selfupdate`).
//!
//! Windows: the same module is 「用这台电脑」 there. The daemon is `<slug>-daemon.exe` next to
//! the app's exe in the install folder; `install-service` registers a scheduled task of this
//! user (`daemon::service`), whose definition the daemon keeps in
//! `%LOCALAPPDATA%\<slug>-daemon\task.xml`, read here the way the launchd plist is on a Mac.
//! The daemon makes the config folder, private from its creation, before the token is
//! written there; whether the task is registered is asked of the Task Scheduler.

use std::io::Write as _;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
#[cfg(not(windows))]
use std::time::{Duration, Instant};

use crate::data::ThisMac;

/// The user's folder, as the daemon finds it (`daemon::config::home`).
fn home() -> PathBuf {
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty());
    #[cfg(windows)]
    let home = home.or_else(|| std::env::var_os("USERPROFILE").filter(|h| !h.is_empty()));
    home.map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// `~/.config/<slug>`: the daemon's config folder.
fn config_dir() -> PathBuf {
    home().join(".config").join(env!("PRODUCT_SLUG"))
}

pub(super) fn config_path() -> PathBuf {
    config_dir().join("daemon.toml")
}

/// Where the service manager's definition of the daemon is: the launchd agent.
#[cfg(not(windows))]
fn service_file() -> PathBuf {
    home()
        .join("Library/LaunchAgents")
        .join(format!("{}.daemon.plist", env!("PRODUCT_BUNDLE_ID")))
}

/// Where the service manager's definition of the daemon is: the scheduled task's XML, which
/// `install-service` keeps (`daemon::service::task_file`).
#[cfg(windows)]
fn service_file() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join("AppData").join("Local"))
        .join(format!("{}-daemon", env!("PRODUCT_SLUG")))
        .join("task.xml")
}

/// The definition's text: the plist is UTF-8, the task XML is UTF-16 with a byte order mark.
fn service_text(bytes: &[u8]) -> String {
    match bytes {
        [0xFF, 0xFE, rest @ ..] => {
            let units: Vec<u16> = rest
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&units)
        }
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

#[cfg(unix)]
fn executable(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(p: &Path) -> bool {
    p.is_file()
}

/// The daemon inside this app bundle; `None` for a build that is not an app bundle (a debug
/// build from a worktree must never take over the installed app's daemon).
#[cfg(not(windows))]
pub(super) fn bundled_daemon() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    let dir = exe.parent()?;
    if !dir.ends_with("Contents/MacOS") {
        return None;
    }
    let d = dir.join(format!("{}-daemon", env!("PRODUCT_SLUG")));
    executable(&d).then_some(d)
}

/// The daemon the installer put next to this app's exe; `None` for a build that was not
/// installed (a build from a worktree must never take over the installed app's daemon).
#[cfg(windows)]
pub(super) fn bundled_daemon() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    daemon_next_to(&exe, cfg!(debug_assertions))
}

/// The installed daemon is `<slug>-daemon.exe` next to the app's exe. A build from a worktree
/// is a debug build, or sits in cargo's `target` folder, where the daemon is not named so.
///
/// Whether other users of the computer could replace that file is checked by the daemon's
/// `install-service` (`daemon::wintrust`), which refuses such a folder before it registers
/// anything. Not here: this app's own exe is in the same folder, so a folder others can write
/// has already decided what runs when the user starts the app.
#[cfg(any(windows, test))]
fn daemon_next_to(app_exe: &Path, debug_build: bool) -> Option<PathBuf> {
    if debug_build {
        return None;
    }
    let dir = app_exe.parent()?;
    if dir.components().any(|c| c.as_os_str() == "target") {
        return None;
    }
    let d = dir.join(format!("{}-daemon.exe", env!("PRODUCT_SLUG")));
    executable(&d).then_some(d)
}

/// Whether the service manager runs the daemon, and from where. `definition` is the launchd
/// agent's text, or on Windows the scheduled task's.
pub(super) fn state_of(bundled: Option<&Path>, definition: Option<&str>) -> ThisMac {
    let Some(bundled) = bundled else {
        return ThisMac::Unavailable;
    };
    match definition {
        None => ThisMac::NotSetUp,
        Some(text) => {
            if runs(text, bundled, cfg!(windows)) {
                ThisMac::Working
            } else {
                ThisMac::Elsewhere
            }
        }
    }
}

/// True when the definition starts the daemon at `daemon`. A plist names it as one
/// `<string>`; a task has it in quotes in the arguments of the console host
/// (`--headless "<daemon>" run …`), and Windows paths compare without case.
fn runs(definition: &str, daemon: &Path, task: bool) -> bool {
    let path = xml_escape(&daemon.to_string_lossy());
    if task {
        definition
            .to_lowercase()
            .contains(&format!("&quot;{path}&quot; run ").to_lowercase())
    } else {
        definition.contains(&format!("<string>{path}</string>"))
    }
}

pub(super) fn state() -> ThisMac {
    state_of(bundled_daemon().as_deref(), definition().as_deref())
}

/// The definition of the daemon's service when the service manager has it. A Mac: the
/// launchd agent's file is the registration. Windows: the Task Scheduler is asked whether
/// the task is registered (a definition file alone does not say so); its text is the copy
/// `install-service` keeps, empty when that copy is gone (a task somebody registered by hand).
fn definition() -> Option<String> {
    let text = std::fs::read(service_file()).ok().map(|b| service_text(&b));
    #[cfg(windows)]
    let text = task_registered().then(|| text.unwrap_or_default());
    text
}

/// Windows: whether the Task Scheduler has the daemon's task, asked through `schtasks`
/// (its exit code; its text depends on the system's language). The answer is kept for two
/// seconds: the settings page asks whenever it draws.
#[cfg(windows)]
fn task_registered() -> bool {
    let mut seen = TASK_STATE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, registered)) = *seen
        && at.elapsed() < std::time::Duration::from_secs(2)
    {
        return registered;
    }
    let mut cmd = system_command("schtasks.exe");
    cmd.args(["/Query", "/TN", &task_name()])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let registered = cmd.status().is_ok_and(|s| s.success());
    *seen = Some((std::time::Instant::now(), registered));
    registered
}

#[cfg(windows)]
static TASK_STATE: std::sync::Mutex<Option<(std::time::Instant, bool)>> =
    std::sync::Mutex::new(None);

/// The daemon's scheduled task, as the daemon names it (`daemon::service::task_name`).
#[cfg(windows)]
fn task_name() -> String {
    format!("{}.daemon", env!("PRODUCT_BUNDLE_ID"))
}

/// What was last heard from the service manager may be out of date now (an install, a
/// restart, an uninstall just ran).
fn forget_task_state() {
    #[cfg(windows)]
    {
        *TASK_STATE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// Why 用这台电脑 did not work.
pub(super) enum SetUpError {
    /// The server did not register the machine.
    Api(chat_core::ApiError),
    /// The config or the service could not be put in place (for the log; never a secret).
    Install(String),
}

/// 用这台电脑, the whole of it: a computer with a daemon config keeps it (its machine and
/// token) and only moves the service to the app's own daemon; a new one is registered with
/// the server first (as an owner), then gets its config, its token and the service.
pub(super) async fn set_up(
    daemon: &Path,
    http: &chat_core::HttpClient,
    server_url: String,
) -> Result<(), SetUpError> {
    let new = if config_path().is_file() {
        None
    } else {
        let labels = default_labels();
        let req = chat_core::proto::CreateMachineRequest {
            name: computer_name(),
            labels: Some(labels.clone()),
            // 用这台电脑 makes it my own computer (design-roles 3.3): only what I ask is done here.
            personal: Some(true),
        };
        let r = http.create_machine(&req).await.map_err(SetUpError::Api)?;
        Some(NewMachine {
            server_url,
            id: r.machine.id.clone(),
            name: r.machine.name.clone(),
            labels,
            token: r.token,
        })
    };
    install(daemon, new.as_ref()).map_err(SetUpError::Install)
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn toml_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The config of a new machine, the same keys the install command writes.
pub(super) fn config_text(
    server_url: &str,
    token_file: &Path,
    machine_id: &str,
    name: &str,
    labels: &[String],
) -> String {
    let labels = labels
        .iter()
        .map(|l| toml_str(l))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "server_url = {}\ntoken_file = {}\nmachine_id = {}\nname = {}\nlabels = [{labels}]\n",
        toml_str(server_url),
        toml_str(&token_file.to_string_lossy()),
        toml_str(machine_id),
        toml_str(name),
    )
}

/// The label a machine registered from this computer gets: its system.
pub(super) fn default_labels() -> Vec<String> {
    vec![if cfg!(windows) { "windows" } else { "mac" }.to_string()]
}

/// This computer's name (系统 › 关于 › 设备名称).
#[cfg(windows)]
pub(super) fn computer_name() -> String {
    std::env::var("COMPUTERNAME")
        .ok()
        .map(|n| n.trim().chars().take(64).collect::<String>())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "Windows".into())
}

/// This Mac's name as people see it (系统设置 › 通用 › 关于本机), else the host name.
#[cfg(not(windows))]
pub(super) fn computer_name() -> String {
    let out = Command::new("/usr/sbin/scutil")
        .args(["--get", "ComputerName"])
        .stderr(Stdio::null())
        .output();
    let name = out
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if !name.is_empty() {
        return name.chars().take(64).collect();
    }
    std::env::var("HOST").unwrap_or_else(|_| "Mac".into())
}

/// Runs `cmd` and waits at most `limit`; the output when it exited with 0.
#[cfg(not(windows))]
fn output_within(mut cmd: Command, limit: Duration) -> Option<Vec<u8>> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out = Vec::new();
                use std::io::Read as _;
                child.stdout.take()?.read_to_end(&mut out).ok()?;
                return status.success().then_some(out);
            }
            Ok(None) if start.elapsed() < limit => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// Folders where coding CLIs are often installed, added after the shell's PATH.
#[cfg(not(windows))]
const USUAL_BIN: &[&str] = &[
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "~/.local/bin",
    "~/.claude/local",
    "~/.npm-global/bin",
    "~/.bun/bin",
    "~/.cargo/bin",
    "/usr/bin",
    "/bin",
    "/usr/sbin",
    "/sbin",
];

/// The PATH the daemon service gets: an app started from the Dock has launchd's minimal PATH,
/// so the user's login shell is asked for its PATH (where `claude` and `codex` are), then the
/// usual install folders that exist are added.
#[cfg(not(windows))]
pub(super) fn service_path() -> String {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let mut cmd = Command::new(&shell);
    cmd.args(["-l", "-i", "-c", "printf '\\n__PATH__%s__PATH__' \"$PATH\""]);
    let from_shell = output_within(cmd, Duration::from_secs(5))
        .map(|o| String::from_utf8_lossy(&o).into_owned())
        .and_then(|o| {
            let start = o.find("__PATH__")? + "__PATH__".len();
            let end = o[start..].find("__PATH__")? + start;
            Some(o[start..end].to_string())
        })
        .unwrap_or_default();
    merge_path(&from_shell, &home(), |p| p.is_dir())
}

#[cfg(not(windows))]
pub(super) fn merge_path(from_shell: &str, home: &Path, exists: impl Fn(&Path) -> bool) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut add = |p: String| {
        if !p.is_empty() && !parts.contains(&p) {
            parts.push(p);
        }
    };
    for p in from_shell.split(':') {
        add(p.to_string());
    }
    for p in USUAL_BIN {
        let p = match p.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None => PathBuf::from(p),
        };
        if exists(&p) {
            add(p.to_string_lossy().into_owned());
        }
    }
    parts.join(":")
}

/// The coding programs the daemon would find on this computer ("claude", "codex"): it adds a
/// runtime for each one it finds in its PATH when it starts, and stops when there is none. A
/// Mac looks in the PATH the service gets ([`service_path`]); Windows in the app's PATH and
/// the folders the official installers use, which the scheduled task's PATH has.
pub(super) fn local_clis() -> Vec<String> {
    #[cfg(not(windows))]
    let path = service_path();
    #[cfg(windows)]
    let path = {
        let mut path = std::env::var("PATH").unwrap_or_default();
        for extra in [
            Some(home().join(".local").join("bin")),
            std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("npm")),
        ]
        .into_iter()
        .flatten()
        {
            path.push(';');
            path.push_str(&extra.to_string_lossy());
        }
        path
    };
    found_in(&path, cfg!(windows), executable)
}

/// [`local_clis`] in `path` (`;` between folders on Windows, where a program has an extension).
fn found_in(path: &str, windows: bool, exists: impl Fn(&Path) -> bool) -> Vec<String> {
    let sep = if windows { ';' } else { ':' };
    ["claude", "codex"]
        .into_iter()
        .filter(|cmd| {
            let names: Vec<String> = if windows {
                [".exe", ".cmd", ".bat", ".com"]
                    .iter()
                    .map(|ext| format!("{cmd}{ext}"))
                    .collect()
            } else {
                vec![cmd.to_string()]
            };
            path.split(sep)
                .filter(|d| !d.trim().is_empty())
                .any(|d| names.iter().any(|n| exists(&Path::new(d.trim()).join(n))))
        })
        .map(str::to_string)
        .collect()
}

/// The machine id in this Mac's daemon config (`machine_id = "…"`), if it has one.
pub(super) fn machine_id() -> Option<String> {
    machine_id_in(&std::fs::read_to_string(config_path()).ok()?)
}

pub(super) fn machine_id_in(config: &str) -> Option<String> {
    config.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        if k.trim() != "machine_id" {
            return None;
        }
        let v = v.split('#').next()?.trim().trim_matches('"').trim();
        (!v.is_empty()).then(|| v.to_string())
    })
}

/// A program of Windows itself (`System32`), started without a console window: the app has
/// no console, and a console program would open one.
#[cfg(windows)]
fn system_command(program: &str) -> Command {
    use std::os::windows::process::CommandExt as _;
    let root = std::env::var_os("SystemRoot")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let mut cmd = Command::new(root.join("System32").join(program));
    cmd.stdin(Stdio::null()).creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// `CREATE_NO_WINDOW`: a console program gets a console without a window.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Runs a command to its end; its error output when it failed.
#[cfg(windows)]
fn run_quiet(mut cmd: Command, what: &str) -> Result<Vec<u8>, String> {
    let out = cmd.output().map_err(|e| format!("{what}: {e}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(format!(
            "{what} failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Restarts the daemon's scheduled task now (`<daemon> restart-service`): the running daemon
/// is asked to stop and the task starts the daemon file now on disk. Ending the task alone
/// would not do: that ends only the console host in front of the daemon.
#[cfg(windows)]
pub(super) fn restart() -> Result<(), String> {
    restart_with(&bundled_daemon().ok_or("this build has no daemon next to it")?)
}

/// [`restart`] with the daemon to ask.
#[cfg(windows)]
fn restart_with(daemon: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt as _;
    forget_task_state();
    let mut cmd = Command::new(daemon);
    cmd.arg("restart-service")
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    run_quiet(cmd, "restart-service").map(|_| ())
}

/// Makes `dir` a folder of this user alone, private from the moment it exists: the daemon
/// makes it (`<daemon> private-dir`), with its access list as part of the creation (this
/// user, SYSTEM and Administrators, nothing inherited), the way it makes its own run folders.
/// A folder that is already there is taken when it is private, or made private while it is
/// still empty; anything else is refused, so the token is never written where somebody else
/// can read it.
#[cfg(windows)]
fn private_dir(daemon: &Path, dir: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt as _;
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot make {}: {e}", parent.display()))?;
    }
    let mut cmd = Command::new(daemon);
    cmd.arg("private-dir")
        .arg(dir)
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    run_quiet(cmd, "private-dir").map(|_| ())
}

/// Restarts the daemon service now (`launchctl kickstart -k`): it starts again from the
/// daemon file on disk, so a daemon from before 1.2.1 (which does not restart itself after
/// an app update) runs the app's current one.
#[cfg(not(any(target_os = "macos", windows)))]
pub(super) fn restart() -> Result<(), String> {
    Err("the daemon service is only managed on macOS and Windows".into())
}

/// See the macOS-less twin above: only launchd runs the daemon today.
#[cfg(target_os = "macos")]
pub(super) fn restart() -> Result<(), String> {
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc::getuid() };
    let target = format!("gui/{uid}/{}.daemon", env!("PRODUCT_BUNDLE_ID"));
    let out = Command::new("/bin/launchctl")
        .args(["kickstart", "-k", &target])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("launchctl: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// A new machine for this Mac: its id, name, labels and token.
pub(super) struct NewMachine {
    pub server_url: String,
    pub id: String,
    pub name: String,
    pub labels: Vec<String>,
    pub token: String,
}

/// Writes the config and token of a new machine (folder 700, files 600; on Windows a folder
/// that is private from its creation, see [`private_dir`]). An existing config is
/// kept as it is: it already names a machine and its token.
fn write_config(daemon: &Path, new: &NewMachine) -> Result<(), String> {
    let dir = config_dir();
    #[cfg(not(windows))]
    {
        let _ = daemon;
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    }
    #[cfg(unix)]
    let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    #[cfg(windows)]
    private_dir(daemon, &dir)?;
    let token_file = dir.join("machine-token");
    let write = |path: &Path, text: &str| -> Result<(), String> {
        let tmp = path.with_extension("new");
        let _ = std::fs::remove_file(&tmp);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut f = options
            .open(&tmp)
            .map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        f.write_all(text.as_bytes())
            .map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("cannot write {}: {e}", path.display()))
    };
    write(&token_file, &new.token)?;
    write(
        &config_path(),
        &config_text(
            &new.server_url,
            &token_file,
            &new.id,
            &new.name,
            &new.labels,
        ),
    )
}

/// Sets this Mac up: writes the config of `new` (when the Mac has none yet), then installs
/// the launchd service (on Windows the scheduled task) from the app's own daemon. Errors are
/// for the log, never shown with secrets in them (the token is written to a file, never passed
/// on a command line).
pub(super) fn install(daemon: &Path, new: Option<&NewMachine>) -> Result<(), String> {
    if let Some(new) = new {
        write_config(daemon, new)?;
    }
    forget_task_state();
    let config = config_path();
    if !config.is_file() {
        return Err(format!("no daemon config at {}", config.display()));
    }
    let mut cmd = Command::new(daemon);
    cmd.arg("install-service")
        .arg("--config")
        .arg(&config)
        .stdin(Stdio::null());
    // launchd starts agents with a minimal PATH, so the service keeps the one found here. A
    // scheduled task gets the user's own environment at every start: nothing to keep.
    #[cfg(not(windows))]
    cmd.env("PATH", service_path());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd
        .output()
        .map_err(|e| format!("cannot start the daemon: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "install-service failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// The files in the config folder that make this computer one of the team's: its config, its
/// token, its machine key and the API keys it was given (`daemon::accounts`). Other files there
/// (a developer's own token copies, say) stay.
const REGISTRATION_FILES: [&str; 4] = [
    "daemon.toml",
    "machine-token",
    "machine.key",
    "secrets.json",
];

/// Removes this computer's registration files from `dir`; one that is not there is fine.
fn remove_registration(dir: &Path) -> Result<(), String> {
    for name in REGISTRATION_FILES {
        for path in [dir.join(name), dir.join(name).with_extension("new")] {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("cannot remove {}: {e}", path.display())),
            }
        }
    }
    Ok(())
}

/// 不再用这台电脑 (机 1.6), the part on this computer: `<daemon> uninstall-service` stops the
/// daemon and removes the launchd agent (on Windows the scheduled task), then the files that
/// registered this computer go. Errors are for the log.
pub(super) fn stop_using(daemon: &Path) -> Result<(), String> {
    stop_using_in(daemon, &config_dir())
}

fn stop_using_in(daemon: &Path, dir: &Path) -> Result<(), String> {
    forget_task_state();
    let mut cmd = Command::new(daemon);
    cmd.arg("uninstall-service").stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd
        .output()
        .map_err(|e| format!("cannot start the daemon: {e}"))?;
    forget_task_state();
    if !out.status.success() {
        return Err(format!(
            "uninstall-service failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    remove_registration(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopping_removes_only_the_registration_files() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "daemon.toml",
            "machine-token",
            "machine.key",
            "secrets.json",
            "machine-token.new",
        ] {
            std::fs::write(dir.path().join(name), "x").unwrap();
        }
        // A developer's own files in the same folder stay.
        std::fs::create_dir(dir.path().join("local")).unwrap();
        std::fs::write(dir.path().join("local").join("owner-token"), "x").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "x").unwrap();
        remove_registration(dir.path()).unwrap();
        let mut left: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, ["local", "notes.txt"]);
        // Nothing left to remove: still fine (a second try after a failure).
        remove_registration(dir.path()).unwrap();
    }

    /// The daemon that set this computer up removes its own service first; when it cannot,
    /// the registration stays, so a second try still has the machine to forget.
    #[cfg(unix)]
    #[test]
    fn stopping_asks_the_daemon_to_remove_its_service() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("args");
        let fake = |name: &str, exit: u8| {
            let daemon = dir.path().join(name);
            std::fs::write(
                &daemon,
                format!(
                    "#!/bin/sh\nprintf '%s' \"$*\" > '{}'\nexit {exit}\n",
                    log.display()
                ),
            )
            .unwrap();
            std::fs::set_permissions(&daemon, std::fs::Permissions::from_mode(0o755)).unwrap();
            daemon
        };
        let cfg = dir.path().join("cfg");
        std::fs::create_dir(&cfg).unwrap();
        std::fs::write(cfg.join("daemon.toml"), "machine_id = \"m\"\n").unwrap();
        assert!(stop_using_in(&fake("broken", 1), &cfg).is_err());
        assert!(cfg.join("daemon.toml").exists());
        stop_using_in(&fake("daemon", 0), &cfg).unwrap();
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "uninstall-service");
        assert!(!cfg.join("daemon.toml").exists());
    }

    #[test]
    fn the_state_says_where_launchd_runs_the_daemon() {
        let bundled = Path::new("/Applications/X.app/Contents/MacOS/x-daemon");
        let mine = "<array>\n<string>/Applications/X.app/Contents/MacOS/x-daemon</string>\n<string>run</string>";
        let other = "<array>\n<string>/Users/a/bin/x-daemon</string>";
        assert!(runs(mine, bundled, false));
        assert!(!runs(other, bundled, false));
    }

    #[test]
    fn the_state_says_whether_the_service_runs_this_apps_daemon() {
        // The definition of this system's service manager: a plist, on Windows a task.
        let (bundled, mine, other) = if cfg!(windows) {
            (
                Path::new(r"C:\Programs\X\x-daemon.exe"),
                "<Arguments>--headless &quot;C:\\Programs\\X\\x-daemon.exe&quot; run --config</Arguments>",
                "<Arguments>--headless &quot;C:\\tools\\x-daemon.exe&quot; run --config</Arguments>",
            )
        } else {
            (
                Path::new("/Applications/X.app/Contents/MacOS/x-daemon"),
                "<string>/Applications/X.app/Contents/MacOS/x-daemon</string>",
                "<string>/Users/a/bin/x-daemon</string>",
            )
        };
        assert_eq!(state_of(None, None), ThisMac::Unavailable);
        assert_eq!(state_of(None, Some(mine)), ThisMac::Unavailable);
        assert_eq!(state_of(Some(bundled), None), ThisMac::NotSetUp);
        assert_eq!(state_of(Some(bundled), Some(mine)), ThisMac::Working);
        assert_eq!(state_of(Some(bundled), Some(other)), ThisMac::Elsewhere);
    }

    #[test]
    fn the_state_says_where_the_scheduled_task_runs_the_daemon() {
        let mine = Path::new(r"C:\Users\li & co\AppData\Local\Programs\X\x-daemon.exe");
        let task = "<Command>C:\\Windows\\System32\\conhost.exe</Command>\n<Arguments>--headless &quot;c:\\users\\li &amp; co\\AppData\\Local\\Programs\\X\\x-daemon.exe&quot; run --config &quot;C:\\c.toml&quot;</Arguments>";
        assert!(runs(task, mine, true), "paths compare without case");
        let other = Path::new(r"C:\tools\x-daemon.exe");
        assert!(!runs(task, other, true));
        // The file the daemon keeps is UTF-16 with a byte order mark.
        let mut bytes = vec![0xFF, 0xFE];
        for u in task.encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(service_text(&bytes), task);
        assert_eq!(service_text(b"<plist/>"), "<plist/>");
    }

    #[test]
    fn only_an_installed_build_has_a_daemon_next_to_it() {
        let dir = tempfile::tempdir().unwrap();
        let install = dir.path().join("Programs").join("X");
        let worktree = dir.path().join("repo").join("target").join("release");
        let name = format!("{}-daemon.exe", env!("PRODUCT_SLUG"));
        for d in [&install, &worktree] {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(d.join(&name), "x").unwrap();
            #[cfg(unix)]
            std::fs::set_permissions(d.join(&name), std::fs::Permissions::from_mode(0o755))
                .unwrap();
        }
        let app = install.join("app.exe");
        assert_eq!(daemon_next_to(&app, false), Some(install.join(&name)));
        assert_eq!(daemon_next_to(&app, true), None, "a debug build");
        assert_eq!(
            daemon_next_to(&worktree.join("app.exe"), false),
            None,
            "a build in cargo's target folder"
        );
        std::fs::remove_file(install.join(&name)).unwrap();
        assert_eq!(daemon_next_to(&app, false), None, "no daemon installed");
    }

    #[test]
    fn the_coding_programs_are_looked_up_in_the_services_path() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("claude"), "x").unwrap();
        std::fs::write(bin.join("codex.cmd"), "x").unwrap();
        let is_file = |p: &Path| p.is_file();
        // A folder of Windows has a drive letter and a colon: only a Unix path splits on ':'.
        if !cfg!(windows) {
            let unix = format!("/nowhere:{}", bin.display());
            assert_eq!(found_in(&unix, false, is_file), ["claude"]);
        }
        let win = format!("C:\\nowhere; {}", bin.display());
        assert_eq!(found_in(&win, true, is_file), ["codex"]);
        assert!(found_in("", false, is_file).is_empty());
    }

    #[test]
    fn the_machine_id_comes_from_the_config() {
        let c = "server_url = \"https://x\"\nmachine_id = \"01M6N4A0P5\"  # mine\nname = \"m\"\n";
        assert_eq!(machine_id_in(c).as_deref(), Some("01M6N4A0P5"));
        assert_eq!(machine_id_in("name = \"m\"\n"), None);
        assert_eq!(machine_id_in("machine_id = \"\"\n"), None);
    }

    #[test]
    fn the_config_has_the_machine_and_quotes_values() {
        let t = config_text(
            "https://x.example",
            Path::new("/h/.config/x/machine-token"),
            "01M",
            "Joye 的 \"Mac\"",
            &["mac".into()],
        );
        assert!(t.contains("server_url = \"https://x.example\"\n"));
        assert!(t.contains("token_file = \"/h/.config/x/machine-token\"\n"));
        assert!(t.contains("machine_id = \"01M\"\n"));
        assert!(t.contains("name = \"Joye 的 \\\"Mac\\\"\"\n"));
        assert!(t.contains("labels = [\"mac\"]\n"));
    }

    // launchd's PATH: a scheduled task gets the user's environment, nothing is merged there.
    #[cfg(not(windows))]
    #[test]
    fn the_service_path_keeps_the_shell_path_first_and_adds_usual_folders() {
        let home = Path::new("/Users/a");
        let p = merge_path("/Users/a/.local/bin:/usr/bin:/bin", home, |p| {
            p == Path::new("/opt/homebrew/bin")
                || p == Path::new("/Users/a/.local/bin")
                || p == Path::new("/usr/bin")
        });
        assert_eq!(p, "/Users/a/.local/bin:/usr/bin:/bin:/opt/homebrew/bin");
        // No answer from the shell: the usual folders that exist.
        let p = merge_path("", home, |p| {
            p == Path::new("/usr/bin") || p == Path::new("/Users/a/.claude/local")
        });
        assert_eq!(p, "/Users/a/.claude/local:/usr/bin");
    }

    /// The pids of the running programs named `image`.
    #[cfg(windows)]
    fn pids_of(image: &str) -> Vec<u32> {
        let mut cmd = system_command("tasklist.exe");
        cmd.args(["/FI", &format!("IMAGENAME eq {image}"), "/FO", "CSV", "/NH"]);
        let out = run_quiet(cmd, "tasklist").unwrap_or_default();
        String::from_utf8_lossy(&out)
            .lines()
            .filter_map(|l| l.split(',').nth(1)?.trim_matches('"').parse().ok())
            .collect()
    }

    #[cfg(windows)]
    fn wait_for<T>(what: &str, seconds: u64, mut ready: impl FnMut() -> Option<T>) -> T {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
        loop {
            if let Some(v) = ready() {
                return v;
            }
            assert!(std::time::Instant::now() < end, "timed out: {what}");
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    /// 「用这台电脑」 from end to end on a real Windows, with a real daemon, the real Task
    /// Scheduler and a real server (a local one): the code the settings page runs
    /// ([`set_up`]) finds the daemon the installer put next to the app, registers this
    /// computer with the server as an owner, writes the config and the token into a folder of
    /// this user alone, and has the daemon register its task. The machine then comes online
    /// at the server under the id in the config; the state reads 正常; a restart starts
    /// another daemon, which comes online again; the token is in no log; and after the
    /// daemon's own uninstall the machine is offline and nothing is left.
    ///
    /// It changes this user's profile (the daemon config, the scheduled task), so it runs only
    /// where `TC_THIS_COMPUTER_DAEMON` names a built daemon, `TC_THIS_COMPUTER_SERVER` a
    /// local server and `TC_THIS_COMPUTER_OWNER_TOKEN` a file with an owner's token (CI), on
    /// a computer that is not a machine yet, and removes all it made. A coding CLI must be
    /// installed: a machine without one is not registered (the daemon would stop at once).
    #[cfg(windows)]
    #[test]
    fn this_computer_becomes_a_machine_and_stops_being_one() {
        let Some(built) = std::env::var_os("TC_THIS_COMPUTER_DAEMON").map(PathBuf::from) else {
            eprintln!("skipped: TC_THIS_COMPUTER_DAEMON is not set");
            return;
        };
        let server = std::env::var("TC_THIS_COMPUTER_SERVER").expect("TC_THIS_COMPUTER_SERVER");
        let owner = std::fs::read_to_string(
            std::env::var_os("TC_THIS_COMPUTER_OWNER_TOKEN").expect("TC_THIS_COMPUTER_OWNER_TOKEN"),
        )
        .expect("the owner token file")
        .trim()
        .to_string();
        assert!(built.is_file(), "no daemon at {}", built.display());
        let image = format!("{}-daemon.exe", env!("PRODUCT_SLUG"));
        assert!(
            !config_dir().exists() && pids_of(&image).is_empty() && !task_registered(),
            "this computer already is a machine: not touching it"
        );
        // Where the installer puts the app and its daemon: a folder of this user under
        // Programs. The daemon is found the way the installed app finds it.
        let programs = PathBuf::from(std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA"))
            .join("Programs")
            .join(format!("{}-this-computer-test", env!("PRODUCT_SLUG")));
        std::fs::create_dir_all(&programs).unwrap();
        std::fs::copy(&built, programs.join(&image)).unwrap();
        let daemon = daemon_next_to(&programs.join("app.exe"), false)
            .expect("the daemon next to the installed app");

        struct Remove(PathBuf, PathBuf);
        impl Drop for Remove {
            fn drop(&mut self) {
                let mut cmd = Command::new(&self.0);
                cmd.arg("uninstall-service").stdin(Stdio::null());
                let _ = cmd.output();
                let state = service_file().parent().map(Path::to_path_buf);
                for dir in [Some(config_dir()), Some(self.1.clone()), state]
                    .into_iter()
                    .flatten()
                {
                    for _ in 0..20 {
                        if std::fs::remove_dir_all(&dir).is_ok() || !dir.exists() {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(300));
                    }
                }
            }
        }
        let _remove = Remove(daemon.clone(), programs.clone());
        let read_state = || {
            forget_task_state();
            state_of(Some(&daemon), definition().as_deref())
        };
        assert_eq!(read_state(), ThisMac::NotSetUp);
        // A definition file without a registered task (an install that failed half way, a
        // task removed by hand) is not "set up": the Task Scheduler is what is asked.
        let stale = service_file();
        std::fs::create_dir_all(stale.parent().unwrap()).unwrap();
        std::fs::write(&stale, format!("\"{}\" run ", daemon.display())).unwrap();
        assert_eq!(read_state(), ThisMac::NotSetUp);
        std::fs::remove_file(&stale).unwrap();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let http = chat_core::HttpClient::new(&server, Some(owner)).expect("client");
        let online = |id: &str| -> Option<bool> {
            rt.block_on(http.machines())
                .ok()?
                .into_iter()
                .find(|m| m.id == id)
                .map(|m| m.connected)
        };
        if let Err(e) = rt.block_on(set_up(&daemon, &http, server.clone())) {
            panic!(
                "用这台电脑 failed: {}",
                match e {
                    SetUpError::Api(e) => format!("the server: {e:?}"),
                    SetUpError::Install(e) => e,
                }
            );
        }

        // What the app wrote, where the daemon looks for it; the server knows the machine.
        let id = machine_id().expect("a machine id in the config");
        assert!(online(&id).is_some(), "the server has the machine {id}");
        let token_file = config_dir().join("machine-token");
        let token = std::fs::read_to_string(&token_file).unwrap();
        assert!(
            token.starts_with("tm_") && token.trim() == token,
            "a machine token"
        );
        // The folder is this user's alone: nothing inherited, three entries.
        let mut acl = system_command("icacls.exe");
        acl.arg(config_dir());
        let acl = String::from_utf8_lossy(&run_quiet(acl, "icacls").unwrap()).into_owned();
        assert!(!acl.contains("(I)"), "inherited entries: {acl}");
        assert_eq!(acl.matches("(OI)(CI)(F)").count(), 3, "{acl}");

        // The task is registered and runs this daemon, without a window; the machine is
        // online at the server with the token the app wrote.
        assert_eq!(read_state(), ThisMac::Working);
        assert_eq!(
            state_of(
                Some(Path::new(r"C:\Elsewhere\daemon.exe")),
                definition().as_deref()
            ),
            ThisMac::Elsewhere
        );
        let first = wait_for("one daemon runs", 90, || {
            let p = pids_of(&image);
            (p.len() == 1).then(|| p[0])
        });
        wait_for("the machine is online", 90, || {
            online(&id).filter(|c| *c).map(|_| ())
        });
        let log = service_file().with_file_name("daemon.log");

        // Setting it up again keeps the machine it already is (nothing is registered twice).
        let machines = rt.block_on(http.machines()).unwrap().len();
        rt.block_on(set_up(&daemon, &http, server.clone()))
            .unwrap_or_else(|_| panic!("set up again"));
        assert_eq!(machine_id().as_deref(), Some(id.as_str()));
        assert_eq!(rt.block_on(http.machines()).unwrap().len(), machines);
        assert_eq!(std::fs::read_to_string(&token_file).unwrap(), token);

        // 重启: another daemon process, one only, online again.
        let before = wait_for("one daemon runs", 90, || {
            let p = pids_of(&image);
            (p.len() == 1).then(|| p[0])
        });
        restart_with(&daemon).expect("restart");
        let after = wait_for("the restart started another daemon", 90, || {
            let p = pids_of(&image);
            (p.len() == 1 && p[0] != before).then(|| p[0])
        });
        assert!(after != before && after != first);
        wait_for("the machine is online again", 90, || {
            online(&id).filter(|c| *c).map(|_| ())
        });
        // The daemon has signed in to the server three times by now: its log has no token.
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(text.contains("daemon started"), "{text}");
        assert!(!text.contains(&token), "the token is in the log");

        // The daemon's own uninstall: no task, no process, the machine offline.
        let mut cmd = Command::new(&daemon);
        cmd.arg("uninstall-service").stdin(Stdio::null());
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        wait_for("the daemon ended", 60, || {
            pids_of(&image).is_empty().then_some(())
        });
        assert_eq!(read_state(), ThisMac::NotSetUp);
        wait_for("the machine is offline", 90, || {
            online(&id).filter(|c| !*c).map(|_| ())
        });
    }
}
