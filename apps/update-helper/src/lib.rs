//! The update helper of the Windows app.
//!
//! Windows cannot overwrite a program that is running, so the app does not replace itself.
//! When the user chooses 重启安装 it copies this program out of its install folder, starts
//! the copy and quits (`apps/desktop/src/updater/install/windows.rs`). The copy then:
//!
//! 1. waits until the app's process is gone;
//! 2. checks the downloaded installer again: its SHA-256, and (when the installed app is
//!    signed) that the same publisher signed it;
//! 3. moves every file of the install folder into a backup folder next to it. A rename
//!    works on a program that still runs, which is how the machine daemon is replaced
//!    while it works;
//! 4. runs the installer silently in update mode into the install folder;
//! 5. writes the marker the new version removes when it has started, and starts the app;
//! 6. waits for that marker to go. When the installer fails, the new version exits early
//!    or the marker is still there after the time limit, it puts the backup back, restores
//!    the version Windows shows for the app, and starts the old version again.
//!
//! [`run`] is that sequence over [`Sys`], the few things only the system can do, so the
//! tests run it on every platform with a made-up installer.
//!
//! `--shortcut` is a second, small job for the installer: it writes the Start menu shortcut
//! with the app's AppUserModelID on it, which Windows needs to show the app's notifications
//! under its name.

use std::{
    ffi::OsString,
    fmt::Write as _,
    io::Read as _,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

mod sys;

pub use sys::Os;

/// How long the helper waits for the app to exit.
pub const WAIT_FOR_EXIT: Duration = Duration::from_secs(60);
/// How long the new version has to remove the marker.
pub const CONFIRM_WITHIN: Duration = Duration::from_secs(90);

/// One update.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Job {
    /// The app's process: the helper starts when it is gone.
    pub wait_pid: Option<u32>,
    /// The downloaded installer.
    pub installer: PathBuf,
    /// Its expected SHA-256 (hex).
    pub sha256: String,
    /// Where the app is installed.
    pub install_dir: PathBuf,
    /// Where the old files wait until the new version has started. A folder next to the
    /// install folder (same volume: the files are renamed, not copied).
    pub backup_dir: PathBuf,
    /// File name of the app's program in the install folder.
    pub app_exe: String,
    /// The version the installer holds.
    pub version: String,
    /// The marker the new version removes at start.
    pub marker: PathBuf,
    /// Where the outcome is written as JSON.
    pub result: Option<PathBuf>,
    /// The helper's log.
    pub log: Option<PathBuf>,
    /// Arguments for the app when the helper starts it (tests).
    pub launch_args: Vec<OsString>,
    /// Registry key under `HKEY_CURRENT_USER` whose `DisplayVersion` is put back after a
    /// rollback (the app's entry in 应用和功能).
    pub uninstall_key: Option<String>,
    /// Seconds to wait for the app to exit; 0 = [`WAIT_FOR_EXIT`].
    pub wait_secs: u64,
    /// Seconds the new version has to remove the marker; 0 = [`CONFIRM_WITHIN`].
    pub confirm_secs: u64,
}

/// The marker file. The same JSON the app's `updater::install::Marker` reads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    pub bundle: PathBuf,
    pub old: PathBuf,
    pub version: String,
}

/// How an update ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The new version runs.
    Updated,
    /// Nothing was changed.
    Refused(String),
    /// The install folder was changed and put back; the old version was started again.
    RolledBack(String),
}

impl Outcome {
    pub fn exit_code(&self) -> i32 {
        match self {
            Outcome::Updated => 0,
            Outcome::Refused(_) => 2,
            Outcome::RolledBack(_) => 3,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Outcome::Updated => "updated",
            Outcome::Refused(_) => "refused",
            Outcome::RolledBack(_) => "rolled_back",
        }
    }

    fn error(&self) -> Option<&str> {
        match self {
            Outcome::Updated => None,
            Outcome::Refused(e) | Outcome::RolledBack(e) => Some(e),
        }
    }
}

/// Who signed a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Signer {
    /// No signature, or the system cannot tell.
    Unsigned,
    /// A valid signature of this publisher (the certificate's subject).
    Valid(String),
    /// A signature that does not verify.
    Invalid(String),
}

/// A program the helper started.
pub trait Running {
    /// True when it has exited.
    fn exited(&mut self) -> bool;
    fn kill(&mut self);
}

/// What only the system can do.
pub trait Sys {
    /// Waits until the process is gone. False when it still runs after `timeout`.
    fn wait_exit(&self, pid: u32, timeout: Duration) -> bool;
    /// Runs the installer silently in update mode into `install_dir` and waits for it.
    fn run_installer(&self, installer: &Path, install_dir: &Path) -> Result<(), String>;
    /// Starts the app.
    fn launch(&self, exe: &Path, args: &[OsString]) -> Result<Box<dyn Running>, String>;
    fn signer(&self, file: &Path) -> Signer;
    /// The version the system shows for the installed app.
    fn shown_version(&self, uninstall_key: &str) -> Option<String>;
    fn set_shown_version(&self, uninstall_key: &str, version: &str);
    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
}

/// Appends lines to the log file; never fails.
pub struct Log(Option<PathBuf>);

impl Log {
    pub fn new(path: Option<PathBuf>) -> Log {
        if let Some(parent) = path.as_deref().and_then(Path::parent) {
            let _ = std::fs::create_dir_all(parent);
        }
        Log(path)
    }

    pub fn line(&self, text: &str) {
        use std::io::Write as _;
        let Some(path) = &self.0 else { return };
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(f, "{secs} {text}");
        }
    }
}

/// Lowercase hex SHA-256 of a file.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let mut hex = String::with_capacity(64);
    for b in hasher.finalize() {
        let _ = write!(hex, "{b:02x}");
    }
    Ok(hex)
}

/// The rule for who may update a signed app: when the installed app carries a valid
/// signature, the installer must carry a valid signature of the same publisher. An
/// unsigned app (a build without the operator's certificate) takes any installer whose
/// SHA-256 matched.
pub fn signer_allows(installed: &Signer, installer: &Signer) -> Result<(), String> {
    match (installed, installer) {
        (Signer::Valid(a), Signer::Valid(b)) if a == b => Ok(()),
        (Signer::Valid(_), Signer::Valid(_)) => {
            Err("the installer is signed by another publisher than the installed app".into())
        }
        (Signer::Valid(_), Signer::Unsigned) => {
            Err("the installed app is signed and the installer is not".into())
        }
        (_, Signer::Invalid(why)) => Err(format!("the installer's signature is not valid: {why}")),
        (Signer::Unsigned | Signer::Invalid(_), _) => Ok(()),
    }
}

/// Renames with a few retries: right after a process exits, a scanner may still hold its
/// file for a moment.
fn rename_patiently(sys: &dyn Sys, from: &Path, to: &Path) -> std::io::Result<()> {
    let mut last = None;
    for attempt in 0..10 {
        if attempt > 0 {
            sys.sleep(Duration::from_millis(300));
        }
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::ErrorKind::Other.into()))
}

/// Removes a file or a folder tree; a missing path is fine. Never follows a link.
fn remove_all(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

fn entries(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut all: Vec<PathBuf> = std::fs::read_dir(dir)?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|e| e.path())
        .collect();
    all.sort();
    Ok(all)
}

/// Moves every entry of `from` into the new folder `to`. When one cannot move, the moved
/// ones go back and `from` is as it was.
fn move_entries(sys: &dyn Sys, from: &Path, to: &Path) -> Result<(), String> {
    remove_all(to).map_err(|e| format!("cannot clear {}: {e}", to.display()))?;
    std::fs::create_dir_all(to).map_err(|e| format!("cannot create {}: {e}", to.display()))?;
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    let all = entries(from).map_err(|e| format!("cannot list {}: {e}", from.display()))?;
    for entry in all {
        let Some(name) = entry.file_name() else {
            continue;
        };
        let dest = to.join(name);
        if let Err(e) = rename_patiently(sys, &entry, &dest) {
            for (back, at) in moved.iter().rev() {
                let _ = std::fs::rename(at, back);
            }
            let _ = std::fs::remove_dir(to);
            return Err(format!("cannot move {}: {e}", entry.display()));
        }
        moved.push((entry, dest));
    }
    Ok(())
}

/// Puts the backup back: clears what the installer wrote, then moves the old entries home.
/// An entry that cannot be removed (a program that runs) is renamed out of the way into
/// the backup folder, which then stays for the next start of the app to clear.
fn restore(sys: &dyn Sys, install_dir: &Path, backup_dir: &Path, log: &Log) -> Result<(), String> {
    let _ = std::fs::create_dir_all(install_dir);
    let aside = backup_dir.join(".failed-update");
    for entry in entries(install_dir).unwrap_or_default() {
        if remove_all(&entry).is_ok() {
            continue;
        }
        let Some(name) = entry.file_name() else {
            continue;
        };
        let _ = std::fs::create_dir_all(&aside);
        if let Err(e) = rename_patiently(sys, &entry, &aside.join(name)) {
            return Err(format!("cannot clear {}: {e}", entry.display()));
        }
        log.line(&format!("moved aside {}", entry.display()));
    }
    let old = entries(backup_dir).map_err(|e| format!("cannot list the backup: {e}"))?;
    for entry in old {
        let Some(name) = entry.file_name() else {
            continue;
        };
        if name == ".failed-update" {
            continue;
        }
        rename_patiently(sys, &entry, &install_dir.join(name))
            .map_err(|e| format!("cannot restore {}: {e}", entry.display()))?;
    }
    if remove_all(backup_dir).is_err() {
        log.line("the backup folder still holds files of the failed update");
    }
    Ok(())
}

fn write_json(path: &Path, value: &serde_json::Value) -> std::io::Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(value).unwrap_or_default())?;
    std::fs::rename(&tmp, path)
}

/// Runs one update and writes the log and the result file.
pub fn run(job: &Job, sys: &dyn Sys) -> Outcome {
    let log = Log::new(job.log.clone());
    log.line(&format!(
        "update to {} in {}",
        job.version,
        job.install_dir.display()
    ));
    let outcome = update(job, sys, &log);
    log.line(&match outcome.error() {
        Some(e) => format!("{}: {e}", outcome.name()),
        None => outcome.name().to_string(),
    });
    if let Some(path) = &job.result {
        let json = serde_json::json!({
            "outcome": outcome.name(),
            "version": job.version,
            "error": outcome.error(),
        });
        if let Err(e) = write_json(path, &json) {
            log.line(&format!("cannot write the result: {e}"));
        }
    }
    outcome
}

fn update(job: &Job, sys: &dyn Sys, log: &Log) -> Outcome {
    let wait = match job.wait_secs {
        0 => WAIT_FOR_EXIT,
        s => Duration::from_secs(s),
    };
    let confirm = match job.confirm_secs {
        0 => CONFIRM_WITHIN,
        s => Duration::from_secs(s),
    };
    let app = job.install_dir.join(&job.app_exe);

    // 1. The app must be gone: its files cannot be replaced while it runs.
    if let Some(pid) = job.wait_pid
        && !sys.wait_exit(pid, wait)
    {
        return Outcome::Refused("the app did not exit".into());
    }

    // 2. The installer is the file the server described, from the same publisher.
    match sha256_file(&job.installer) {
        Ok(sum) if sum.eq_ignore_ascii_case(job.sha256.trim()) => {}
        Ok(_) => return refuse_and_reopen(job, sys, log, "the installer's checksum changed"),
        Err(e) => {
            return refuse_and_reopen(job, sys, log, &format!("cannot read the installer: {e}"));
        }
    }
    if !app.is_file() {
        return Outcome::Refused(format!("no app at {}", app.display()));
    }
    if let Err(why) = signer_allows(&sys.signer(&app), &sys.signer(&job.installer)) {
        return refuse_and_reopen(job, sys, log, &why);
    }
    let shown = job
        .uninstall_key
        .as_deref()
        .and_then(|k| sys.shown_version(k));

    // 3. Set the old files aside.
    if let Err(e) = move_entries(sys, &job.install_dir, &job.backup_dir) {
        return refuse_and_reopen(job, sys, log, &e);
    }
    log.line("old files moved to the backup");

    // 4-6. Install, start, wait for the new version to say it runs.
    match install_and_confirm(job, sys, log, &app, confirm) {
        Ok(()) => {
            let _ = std::fs::remove_file(&job.installer);
            Outcome::Updated
        }
        Err(why) => {
            log.line(&format!("rolling back: {why}"));
            let _ = std::fs::remove_file(&job.marker);
            if let Err(e) = restore(sys, &job.install_dir, &job.backup_dir, log) {
                return Outcome::RolledBack(format!("{why}; and the rollback failed: {e}"));
            }
            if let (Some(key), Some(version)) = (job.uninstall_key.as_deref(), shown.as_deref()) {
                sys.set_shown_version(key, version);
            }
            reopen(job, sys, log, &app);
            Outcome::RolledBack(why)
        }
    }
}

fn install_and_confirm(
    job: &Job,
    sys: &dyn Sys,
    log: &Log,
    app: &Path,
    confirm: Duration,
) -> Result<(), String> {
    sys.run_installer(&job.installer, &job.install_dir)?;
    if !app.is_file() {
        return Err("the installer did not write the app".into());
    }
    log.line("installer finished");
    let marker = Marker {
        bundle: job.install_dir.clone(),
        old: job.backup_dir.clone(),
        version: job.version.clone(),
    };
    let json = serde_json::to_value(&marker).map_err(|e| e.to_string())?;
    write_json(&job.marker, &json).map_err(|e| format!("cannot write the marker: {e}"))?;
    let mut child = sys.launch(app, &job.launch_args)?;
    log.line("new version started");
    let deadline = Instant::now() + confirm;
    loop {
        if !job.marker.exists() {
            return Ok(());
        }
        if child.exited() {
            // It may have removed the marker in its last moment.
            if !job.marker.exists() {
                return Ok(());
            }
            return Err("the new version exited before it finished the update".into());
        }
        if Instant::now() >= deadline {
            child.kill();
            // Give the system a moment to release the program's file.
            sys.sleep(Duration::from_millis(500));
            return Err("the new version did not finish the update in time".into());
        }
        sys.sleep(Duration::from_millis(200));
    }
}

/// Nothing changed: start the app again so the user is not left without it.
fn refuse_and_reopen(job: &Job, sys: &dyn Sys, log: &Log, why: &str) -> Outcome {
    reopen(job, sys, log, &job.install_dir.join(&job.app_exe));
    Outcome::Refused(why.to_string())
}

fn reopen(job: &Job, sys: &dyn Sys, log: &Log, app: &Path) {
    if let Err(e) = sys.launch(app, &job.launch_args) {
        log.line(&format!("cannot start the app again: {e}"));
    }
}

/// Parses the helper's command line. Every option takes one value; `--launch-arg` may
/// repeat.
pub fn parse_job(args: &[OsString]) -> Result<Job, String> {
    let mut job = Job::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let name = arg.to_string_lossy().into_owned();
        let mut value = || {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        let text = |v: OsString| v.to_string_lossy().into_owned();
        let number = |v: OsString, name: &str| {
            v.to_string_lossy()
                .parse::<u64>()
                .map_err(|_| format!("{name} needs a number"))
        };
        match name.as_str() {
            "--wait-pid" => job.wait_pid = Some(number(value()?, &name)? as u32),
            "--installer" => job.installer = value()?.into(),
            "--sha256" => job.sha256 = text(value()?),
            "--install-dir" => job.install_dir = value()?.into(),
            "--backup-dir" => job.backup_dir = value()?.into(),
            "--app-exe" => job.app_exe = text(value()?),
            "--version" => job.version = text(value()?),
            "--marker" => job.marker = value()?.into(),
            "--result" => job.result = Some(value()?.into()),
            "--log" => job.log = Some(value()?.into()),
            "--launch-arg" => job.launch_args.push(value()?),
            "--uninstall-key" => job.uninstall_key = Some(text(value()?)),
            "--wait-secs" => job.wait_secs = number(value()?, &name)?,
            "--confirm-secs" => job.confirm_secs = number(value()?, &name)?,
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    for (what, missing) in [
        ("--installer", job.installer.as_os_str().is_empty()),
        ("--sha256", job.sha256.is_empty()),
        ("--install-dir", job.install_dir.as_os_str().is_empty()),
        ("--backup-dir", job.backup_dir.as_os_str().is_empty()),
        ("--app-exe", job.app_exe.is_empty()),
        ("--version", job.version.is_empty()),
        ("--marker", job.marker.as_os_str().is_empty()),
    ] {
        if missing {
            return Err(format!("{what} is missing"));
        }
    }
    if job.app_exe.contains(['/', '\\']) {
        return Err("--app-exe is a file name, not a path".into());
    }
    if job.backup_dir.parent() != job.install_dir.parent() || job.backup_dir == job.install_dir {
        return Err("--backup-dir must be a folder next to --install-dir".into());
    }
    Ok(job)
}

/// The command line of a [`Job`], as [`parse_job`] reads it.
pub fn job_args(job: &Job) -> Vec<OsString> {
    let mut args: Vec<OsString> = Vec::new();
    let mut push = |name: &str, value: OsString| {
        args.push(name.into());
        args.push(value);
    };
    if let Some(pid) = job.wait_pid {
        push("--wait-pid", pid.to_string().into());
    }
    push("--installer", job.installer.clone().into());
    push("--sha256", job.sha256.clone().into());
    push("--install-dir", job.install_dir.clone().into());
    push("--backup-dir", job.backup_dir.clone().into());
    push("--app-exe", job.app_exe.clone().into());
    push("--version", job.version.clone().into());
    push("--marker", job.marker.clone().into());
    if let Some(p) = &job.result {
        push("--result", p.clone().into());
    }
    if let Some(p) = &job.log {
        push("--log", p.clone().into());
    }
    for a in &job.launch_args {
        push("--launch-arg", a.clone());
    }
    if let Some(k) = &job.uninstall_key {
        push("--uninstall-key", k.clone().into());
    }
    if job.wait_secs != 0 {
        push("--wait-secs", job.wait_secs.to_string().into());
    }
    if job.confirm_secs != 0 {
        push("--confirm-secs", job.confirm_secs.to_string().into());
    }
    args
}

/// The program's entry: an update job, or `--shortcut` for the installer.
pub fn main(args: Vec<OsString>) -> i32 {
    if args.first().is_some_and(|a| a == "--shortcut") {
        return match sys::shortcut_from_args(&args[1..]) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("{e}");
                1
            }
        };
    }
    match parse_job(&args) {
        Ok(job) => run(&job, &Os).exit_code(),
        Err(e) => {
            eprintln!("{e}");
            64
        }
    }
}

#[cfg(test)]
mod tests;
