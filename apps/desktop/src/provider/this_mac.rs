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
//! The config folder gets an access list with this user alone before the token is written.

use std::io::Write as _;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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
    let definition = std::fs::read(service_file()).ok().map(|b| service_text(&b));
    state_of(bundled_daemon().as_deref(), definition.as_deref())
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
#[cfg_attr(windows, allow(dead_code))]
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

/// Restarts the daemon's scheduled task now: ends it and starts it again from the daemon
/// file on disk.
#[cfg(windows)]
pub(super) fn restart() -> Result<(), String> {
    let name = format!("{}.daemon", env!("PRODUCT_BUNDLE_ID"));
    let mut end = system_command("schtasks.exe");
    end.args(["/End", "/TN", &name]);
    // Not running is fine: starting it is what was asked for.
    let _ = run_quiet(end, "schtasks /End");
    let mut run = system_command("schtasks.exe");
    run.args(["/Run", "/TN", &name]);
    run_quiet(run, "schtasks /Run").map(|_| ())
}

/// The security id (`S-1-5-…`) of the user this app runs as, from `whoami /user`.
#[cfg(windows)]
fn user_sid() -> Result<String, String> {
    let mut cmd = system_command("whoami.exe");
    cmd.args(["/user", "/fo", "csv", "/nh"]);
    let out = run_quiet(cmd, "whoami")?;
    sid_in(&String::from_utf8_lossy(&out)).ok_or_else(|| "whoami printed no user id".to_string())
}

/// The id in `"domain\user","S-1-5-21-…"`. The name may be in any code page; the id is ASCII.
#[cfg(any(windows, test))]
fn sid_in(csv: &str) -> Option<String> {
    let last = csv.trim().rsplit(',').next()?.trim().trim_matches('"');
    let ok = last.starts_with("S-1-")
        && last.len() > 6
        && last.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    ok.then(|| last.to_string())
}

/// Makes `dir` a folder only this user can open: inherited entries (SYSTEM, administrators,
/// whatever the parent folder gives) are dropped and one entry is set, full control for this
/// user, inherited by the files made in it afterwards. `icacls` does it because the app has no
/// access-list code; when the daemon's own helpers exist (`apps/daemon/src/sys.rs`), the daemon
/// should do this in `install-service` and this call goes away.
#[cfg(windows)]
fn only_this_user(dir: &Path) -> Result<(), String> {
    let sid = user_sid()?;
    let mut cmd = system_command("icacls.exe");
    cmd.arg(dir)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg(format!("*{sid}:(OI)(CI)F"));
    run_quiet(cmd, "icacls").map(|_| ())
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
/// only this user can open, set before the token is written into it). An existing config is
/// kept as it is: it already names a machine and its token.
fn write_config(new: &NewMachine) -> Result<(), String> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    #[cfg(unix)]
    let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    #[cfg(windows)]
    only_this_user(&dir)?;
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
        write_config(new)?;
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_says_where_launchd_runs_the_daemon() {
        let bundled = Path::new("/Applications/X.app/Contents/MacOS/x-daemon");
        assert_eq!(state_of(None, None), ThisMac::Unavailable);
        assert_eq!(
            state_of(None, Some("<string>/x</string>")),
            ThisMac::Unavailable
        );
        assert_eq!(state_of(Some(bundled), None), ThisMac::NotSetUp);
        let mine = "<array>\n<string>/Applications/X.app/Contents/MacOS/x-daemon</string>\n<string>run</string>";
        assert_eq!(state_of(Some(bundled), Some(mine)), ThisMac::Working);
        let other = "<array>\n<string>/Users/a/bin/x-daemon</string>";
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
    fn the_user_id_is_the_last_field_of_whoami() {
        let csv = "\"pc\\李\",\"S-1-5-21-1004336348-1177238915-682003330-1001\"\r\n";
        assert_eq!(
            sid_in(csv).as_deref(),
            Some("S-1-5-21-1004336348-1177238915-682003330-1001")
        );
        assert_eq!(sid_in("ERROR: something"), None);
        assert_eq!(sid_in(""), None);
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
}
