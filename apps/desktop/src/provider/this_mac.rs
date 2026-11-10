//! 用这台电脑 (设置 › 机器): makes this Mac one of the team's computers without a terminal.
//! The daemon ships inside the app (`Contents/MacOS/<slug>-daemon`, see scripts/build-dmg.sh).
//! The app writes the daemon config and machine token (mode 600) the way the install command
//! does, then runs `<daemon> install-service`, so launchd runs the daemon from the app bundle.
//! An app update replaces that file, and the daemon restarts itself on the new version
//! (`daemon::selfupdate`).

use std::io::Write as _;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::data::ThisMac;

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// `~/.config/<slug>`: the daemon's config folder.
fn config_dir() -> PathBuf {
    home().join(".config").join(env!("PRODUCT_SLUG"))
}

pub(super) fn config_path() -> PathBuf {
    config_dir().join("daemon.toml")
}

fn plist_path() -> PathBuf {
    home()
        .join("Library/LaunchAgents")
        .join(format!("{}.daemon.plist", env!("PRODUCT_BUNDLE_ID")))
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

/// Whether launchd runs the daemon, and from where. `plist` is the launchd agent's text.
pub(super) fn state_of(bundled: Option<&Path>, plist: Option<&str>) -> ThisMac {
    let Some(bundled) = bundled else {
        return ThisMac::Unavailable;
    };
    match plist {
        None => ThisMac::NotSetUp,
        Some(text) => {
            let ours = format!(
                "<string>{}</string>",
                xml_escape(&bundled.to_string_lossy())
            );
            if text.contains(&ours) {
                ThisMac::Working
            } else {
                ThisMac::Elsewhere
            }
        }
    }
}

pub(super) fn state() -> ThisMac {
    let plist = std::fs::read_to_string(plist_path()).ok();
    state_of(bundled_daemon().as_deref(), plist.as_deref())
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

/// This Mac's name as people see it (系统设置 › 通用 › 关于本机), else the host name.
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

/// Restarts the daemon service now (`launchctl kickstart -k`): it starts again from the
/// daemon file on disk, so a daemon from before 1.2.1 (which does not restart itself after
/// an app update) runs the app's current one.
#[cfg(not(target_os = "macos"))]
pub(super) fn restart() -> Result<(), String> {
    Err("the daemon service is only managed on macOS".into())
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

/// Writes the config and token of a new machine (folder 700, files 600). An existing config
/// is kept as it is: it already names a machine and its token.
fn write_config(new: &NewMachine) -> Result<(), String> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    #[cfg(unix)]
    let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
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
/// the launchd service from the app's own daemon. Errors are for the log, never shown with
/// secrets in them (the token is written to a file, never passed on a command line).
pub(super) fn install(daemon: &Path, new: Option<&NewMachine>) -> Result<(), String> {
    if let Some(new) = new {
        write_config(new)?;
    }
    let config = config_path();
    if !config.is_file() {
        return Err(format!("no daemon config at {}", config.display()));
    }
    let out = Command::new(daemon)
        .arg("install-service")
        .arg("--config")
        .arg(&config)
        .env("PATH", service_path())
        .stdin(Stdio::null())
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
