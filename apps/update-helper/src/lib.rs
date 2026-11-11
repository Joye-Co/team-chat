//! The update helper of the Windows app.
//!
//! Windows cannot overwrite a program that is running, so the app does not replace itself.
//! When the user chooses 重启安装 it copies this program out of its install folder, starts
//! the copy and quits (`apps/desktop/src/updater/install/windows.rs`). The copy then:
//!
//! 1. waits until the app's process is gone;
//! 2. checks the downloaded installer again: its SHA-256, and (when the installed app is
//!    signed) that the same publisher signed it;
//! 3. writes a journal of what it is about to do and registers itself to run at the next
//!    sign-in, so an update that is cut short (the helper is killed, the user signs out,
//!    the power goes) is undone later: see [`recover`];
//! 4. moves every file of the install folder into a backup folder next to it. The machine
//!    daemon is the exception (`--keep`): it may be running a bot, so it is never moved or
//!    stopped; the helper only copies it into the backup, and the installer replaces it by
//!    the daemon's rename protocol (docs/machine-protocol.md);
//! 5. runs the installer silently in update mode into the install folder;
//! 6. writes the marker the new version removes when it has started, and starts the app;
//! 7. waits for that marker to go. When the installer fails, the new version exits early
//!    or the marker is still there after the time limit, it puts the backup back, restores
//!    the version Windows shows for the app, and starts the old version again.
//!
//! [`run`] is that sequence over [`Sys`], the few things only the system can do, so the
//! tests run it on every platform with a made-up installer, and stop it at every step to
//! check the recovery.
//!
//! `--recover <journal>` undoes an interrupted update: Windows runs it at the next sign-in
//! (a `RunOnce` entry), the app runs it at start, and every new update runs it first.
//!
//! `--shortcut` is a small job for the installer: it writes the Start menu shortcut with
//! the app's AppUserModelID on it, which Windows needs to show the app's notifications
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
/// File name of the journal, next to the marker.
pub const JOURNAL: &str = "update-journal.json";

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
    /// File names in the install folder that stay where they are (the machine daemon): the
    /// installer replaces each by renaming the old file to `<name>.old-<suffix>`, and a
    /// rollback puts the old program back the same way.
    pub keep: Vec<String>,
    /// Seconds to wait for the app to exit; 0 = [`WAIT_FOR_EXIT`].
    pub wait_secs: u64,
    /// Seconds the new version has to remove the marker; 0 = [`CONFIRM_WITHIN`].
    pub confirm_secs: u64,
}

impl Job {
    /// The journal of this update: next to the marker.
    pub fn journal(&self) -> PathBuf {
        self.marker.with_file_name(JOURNAL)
    }
}

/// The marker file. The same JSON the app's `updater::install::Marker` reads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    pub bundle: PathBuf,
    pub old: PathBuf,
    pub version: String,
}

/// How far an update got; in the journal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    /// Old files are moving into the backup; the installer has not run.
    Moving,
    /// Every old file is in the backup; the installer runs or has run.
    Installing,
    /// The marker is written and the new version was started.
    Started,
}

/// What an interrupted update needs to be undone. Written before the first file moves and
/// removed when the update has ended either way.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Journal {
    pub stage: Stage,
    /// The helper that works on this update; while it lives nobody else touches it.
    pub helper_pid: u32,
    /// When that helper's process was made. A process id alone is not the helper: after a
    /// restart of the computer another program may have it. Missing in a journal of an older
    /// helper, which is then known by its id alone.
    #[serde(default)]
    pub helper_started: Option<u64>,
    pub install_dir: PathBuf,
    pub backup_dir: PathBuf,
    pub marker: PathBuf,
    pub app_exe: String,
    pub version: String,
    #[serde(default)]
    pub keep: Vec<String>,
    #[serde(default)]
    pub uninstall_key: Option<String>,
    /// The version Windows showed before the update.
    #[serde(default)]
    pub shown_version: Option<String>,
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

/// What [`recover`] found and did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Recovery {
    /// No journal: no update was interrupted.
    Nothing,
    /// The helper of that update still runs; left alone.
    Busy,
    /// The new version had started and finished the update; only the journal was left.
    Finished,
    /// The old version is back in the install folder.
    Restored,
    /// The old version could not be put back; the journal stays for the next try.
    Failed(String),
}

/// Who signed a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Signer {
    /// The file carries no signature.
    Unsigned,
    /// The file carries a signature.
    Signed {
        /// What Windows says about it: `Valid`, `HashMismatch`, `NotTrusted`, `UnknownError`...
        status: String,
        /// The certificate's subject.
        subject: String,
        /// The certificate's thumbprint.
        thumbprint: String,
    },
    /// The system could not say (the query failed). Never the same as unsigned.
    Unknown(String),
}

impl Signer {
    fn valid(&self) -> bool {
        matches!(self, Signer::Signed { status, .. } if status == "Valid")
    }
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
    /// Makes the system run `--recover <journal>` at the user's next sign-in.
    fn register_recovery(&self, name: &str, journal: &Path) -> Result<(), String>;
    fn clear_recovery(&self, name: &str);
    /// This helper's process id.
    fn pid(&self) -> u32 {
        std::process::id()
    }
    /// When the running process `pid` was made (any unit, the same for one process as long
    /// as it lives); `None` when the system cannot say.
    fn started(&self, _pid: u32) -> Option<u64> {
        None
    }
    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
    /// Called between the steps of an update with the step's name. The tests stop there.
    fn checkpoint(&self, _step: &str) {}
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

/// The rule for who may update the app.
///
/// - An app without a signature (a build made without the operator's certificate) takes an
///   installer without one, or one with a valid signature. A signature that does not
///   verify is refused: the file was changed.
/// - An app with a valid signature takes only an installer with a valid signature of the
///   same publisher (the certificate's subject, so a renewed certificate still updates).
/// - An app whose signature does not verify on this computer (a certificate this computer
///   does not trust, or one that expired) takes only an installer signed with the very same
///   certificate, in the same state or valid. A subject alone proves nothing there.
/// - When the system cannot say who signed either file, nothing is installed: a failed
///   query is never read as "unsigned".
pub fn signer_allows(installed: &Signer, installer: &Signer) -> Result<(), String> {
    match (installed, installer) {
        (Signer::Unknown(why), _) => Err(format!(
            "cannot read the signature of the installed app: {why}"
        )),
        (_, Signer::Unknown(why)) => {
            Err(format!("cannot read the signature of the installer: {why}"))
        }
        (Signer::Unsigned, Signer::Unsigned) => Ok(()),
        (Signer::Unsigned, Signer::Signed { status, .. }) => {
            if installer.valid() {
                Ok(())
            } else {
                Err(format!("the installer's signature is not valid: {status}"))
            }
        }
        (Signer::Signed { .. }, Signer::Unsigned) => {
            Err("the installed app is signed and the installer is not".into())
        }
        (
            Signer::Signed {
                status: was,
                subject: publisher,
                thumbprint: certificate,
            },
            Signer::Signed {
                status,
                subject,
                thumbprint,
            },
        ) => {
            if installed.valid() {
                if !installer.valid() {
                    return Err(format!("the installer's signature is not valid: {status}"));
                }
                if publisher != subject {
                    return Err(
                        "the installer is signed by another publisher than the installed app"
                            .into(),
                    );
                }
                return Ok(());
            }
            if !installer.valid() && status != was {
                return Err(format!("the installer's signature is not valid: {status}"));
            }
            if certificate != thumbprint {
                return Err(
                    "the installer is signed with another certificate than the installed app"
                        .into(),
                );
            }
            Ok(())
        }
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

/// The kept program this entry belongs to: the program itself, a `<name>.old-*` file the
/// rename protocol left, or a `<name>.new` file it was writing.
fn kept<'a>(entry: &Path, keep: &'a [String]) -> Option<&'a str> {
    let name = entry.file_name()?.to_string_lossy();
    keep.iter().map(String::as_str).find(|k| {
        name == *k
            || name
                .strip_prefix(*k)
                .is_some_and(|rest| rest.starts_with(".old-") || rest == ".new")
    })
}

/// A suffix no other file of this run has.
fn unique() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}-{nanos}", std::process::id())
}

/// `<path>` with `suffix` after its whole file name (`daemon.exe` -> `daemon.exe.new`).
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// Sets the old files aside: moves every entry of `from` into the new folder `to`, and
/// copies the kept programs there (they stay in `from`). When an entry cannot move, the
/// moved ones go back and `from` is as it was.
fn set_aside(sys: &dyn Sys, from: &Path, to: &Path, keep: &[String]) -> Result<(), String> {
    remove_all(to).map_err(|e| format!("cannot clear {}: {e}", to.display()))?;
    std::fs::create_dir_all(to).map_err(|e| format!("cannot create {}: {e}", to.display()))?;
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    let undo = |moved: &[(PathBuf, PathBuf)]| {
        for (back, at) in moved.iter().rev() {
            let _ = std::fs::rename(at, back);
        }
        let _ = remove_all(to);
    };
    let all = entries(from).map_err(|e| format!("cannot list {}: {e}", from.display()))?;
    for entry in all {
        let Some(name) = entry.file_name() else {
            continue;
        };
        let dest = to.join(name);
        if let Some(program) = kept(&entry, keep) {
            // Only the program itself: its leftovers are the daemon's to clear.
            if name.to_string_lossy() == program
                && let Err(e) = std::fs::copy(&entry, &dest)
            {
                undo(&moved);
                return Err(format!("cannot copy {}: {e}", entry.display()));
            }
            continue;
        }
        if let Err(e) = rename_patiently(sys, &entry, &dest) {
            undo(&moved);
            return Err(format!("cannot move {}: {e}", entry.display()));
        }
        moved.push((entry, dest));
        sys.checkpoint("moved-one");
    }
    Ok(())
}

/// Puts a kept program back by its own protocol (docs/machine-protocol.md): the old program
/// goes to `<name>.new`, the present file is renamed to `<name>.old-<suffix>` (a rename
/// works while it runs), and `<name>.new` takes its place. The process that runs sees the
/// change and restarts itself. Without a copy in the backup the old version had no such
/// program, and what the installer wrote is removed.
fn restore_kept(sys: &dyn Sys, program: &Path, copy: &Path, log: &Log) -> Result<(), String> {
    if !copy.is_file() {
        if program.exists() && std::fs::remove_file(program).is_err() {
            let aside = with_suffix(program, &format!(".old-{}", unique()));
            let _ = rename_patiently(sys, program, &aside);
        }
        return Ok(());
    }
    let same = match (sha256_file(program), sha256_file(copy)) {
        (Ok(now), Ok(old)) => now == old,
        _ => false,
    };
    if !same {
        let new = with_suffix(program, ".new");
        std::fs::copy(copy, &new).map_err(|e| format!("cannot write {}: {e}", new.display()))?;
        if sha256_file(&new).ok() != sha256_file(copy).ok() {
            return Err(format!("{} is not the copy in the backup", new.display()));
        }
        let aside = with_suffix(program, &format!(".old-{}", unique()));
        let had = program.exists();
        if had {
            rename_patiently(sys, program, &aside)
                .map_err(|e| format!("cannot rename {}: {e}", program.display()))?;
        }
        if let Err(e) = rename_patiently(sys, &new, program) {
            if had {
                let _ = std::fs::rename(&aside, program);
            }
            return Err(format!("cannot restore {}: {e}", program.display()));
        }
        log.line(&format!("restored {}", program.display()));
    }
    // What nobody runs any more goes now; the rest goes when its process has ended.
    if let Some(dir) = program.parent() {
        let keep = [program
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()];
        for entry in entries(dir).unwrap_or_default() {
            if entry != program && kept(&entry, &keep).is_some() {
                let _ = std::fs::remove_file(&entry);
            }
        }
    }
    Ok(())
}

/// Puts the backup back. With `clear` (the installer may have run) it first removes what is
/// in the install folder; an entry that cannot be removed (a program that runs) is renamed
/// out of the way into the backup folder, which then stays for a later start of the app to
/// clear. Without `clear` (the update stopped while the old files were moving) the files
/// still in the install folder are old ones and stay.
fn restore(
    sys: &dyn Sys,
    install_dir: &Path,
    backup_dir: &Path,
    keep: &[String],
    clear: bool,
    log: &Log,
) -> Result<(), String> {
    let _ = std::fs::create_dir_all(install_dir);
    if !backup_dir.is_dir() {
        // Nothing was set aside.
        return Ok(());
    }
    if clear {
        let aside = backup_dir.join(".failed-update");
        for entry in entries(install_dir).unwrap_or_default() {
            if kept(&entry, keep).is_some() || remove_all(&entry).is_ok() {
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
    }
    let old = entries(backup_dir).map_err(|e| format!("cannot list the backup: {e}"))?;
    for entry in old {
        let Some(name) = entry.file_name() else {
            continue;
        };
        if name == ".failed-update" || kept(&entry, keep).is_some() {
            continue;
        }
        rename_patiently(sys, &entry, &install_dir.join(name))
            .map_err(|e| format!("cannot restore {}: {e}", entry.display()))?;
    }
    if clear {
        for program in keep {
            restore_kept(
                sys,
                &install_dir.join(program),
                &backup_dir.join(program),
                log,
            )?;
        }
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

fn write_journal(path: &Path, journal: &Journal) -> Result<(), String> {
    let json = serde_json::to_value(journal).map_err(|e| e.to_string())?;
    write_json(path, &json).map_err(|e| format!("cannot write the journal: {e}"))
}

/// The name of the sign-in entry that recovers an update of this app.
fn recovery_name(app_exe: &str) -> String {
    let stem = app_exe.strip_suffix(".exe").unwrap_or(app_exe);
    format!("{stem}.update-recovery")
}

/// Undoes what the journal says, and removes the journal and the sign-in entry.
fn roll_back(sys: &dyn Sys, path: &Path, journal: &Journal, log: &Log) -> Result<(), String> {
    let _ = std::fs::remove_file(&journal.marker);
    restore(
        sys,
        &journal.install_dir,
        &journal.backup_dir,
        &journal.keep,
        journal.stage != Stage::Moving,
        log,
    )?;
    if let (Some(key), Some(version)) = (
        journal.uninstall_key.as_deref(),
        journal.shown_version.as_deref(),
    ) {
        sys.set_shown_version(key, version);
    }
    let _ = std::fs::remove_file(path);
    sys.clear_recovery(&recovery_name(&journal.app_exe));
    Ok(())
}

/// Ends an update that was cut short (the helper was killed, the user signed out, the power
/// went): reads the journal and either finds that the new version had started and finished
/// the update, or puts the old version back. Safe to run at any time and more than once:
/// without a journal it does nothing, and while the helper of that update lives it waits
/// for nothing and leaves it alone.
pub fn recover(journal_path: &Path, sys: &dyn Sys, log: &Log) -> Recovery {
    let Ok(bytes) = std::fs::read(journal_path) else {
        return Recovery::Nothing;
    };
    let Ok(journal) = serde_json::from_slice::<Journal>(&bytes) else {
        // The journal is written whole (a rename), so this is not ours.
        log.line("the journal cannot be read; leaving it");
        return Recovery::Nothing;
    };
    let name = recovery_name(&journal.app_exe);
    if journal.helper_pid != sys.pid() && helper_lives(sys, &journal) {
        // Not ours to touch now. The sign-in entry is used up once Windows has run it, so
        // it is written again: the recovery is tried at every sign-in until it has ended.
        let _ = sys.register_recovery(&name, journal_path);
        return Recovery::Busy;
    }
    log.line(&format!(
        "recovering the update to {} (stopped while {:?})",
        journal.version, journal.stage
    ));
    if journal.stage == Stage::Started && !journal.marker.exists() {
        // The new version removed the marker: it runs, and the backup is its to remove.
        let _ = remove_all(&journal.backup_dir);
        let _ = std::fs::remove_file(journal_path);
        sys.clear_recovery(&recovery_name(&journal.app_exe));
        log.line("the update had finished");
        return Recovery::Finished;
    }
    match roll_back(sys, journal_path, &journal, log) {
        Ok(()) => {
            log.line("the old version is back");
            Recovery::Restored
        }
        Err(e) => {
            log.line(&format!("recovery failed: {e}"));
            // Tried again at the next sign-in (and at the app's next start).
            if let Err(why) = sys.register_recovery(&name, journal_path) {
                log.line(&why);
            }
            Recovery::Failed(e)
        }
    }
}

/// Whether the helper that wrote the journal still runs: a live process with its id that
/// was also made at the time the journal names. After a restart of the computer the id may
/// belong to any program; that is not the helper, and the update is ours to recover.
fn helper_lives(sys: &dyn Sys, journal: &Journal) -> bool {
    if sys.wait_exit(journal.helper_pid, Duration::ZERO) {
        return false;
    }
    match (journal.helper_started, sys.started(journal.helper_pid)) {
        (Some(then), Some(now)) => then == now,
        // An older helper's journal, or a system that cannot say: the id is all there is.
        _ => true,
    }
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
    let journal_path = job.journal();

    // 1. The app must be gone: its files cannot be replaced while it runs.
    if let Some(pid) = job.wait_pid
        && !sys.wait_exit(pid, wait)
    {
        return Outcome::Refused("the app did not exit".into());
    }

    // An earlier update that was cut short is undone first.
    match recover(&journal_path, sys, log) {
        Recovery::Busy => return Outcome::Refused("another update is running".into()),
        Recovery::Failed(e) => {
            return Outcome::Refused(format!("an interrupted update cannot be undone: {e}"));
        }
        Recovery::Nothing | Recovery::Finished | Recovery::Restored => {}
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

    // 3. From here an interruption can be undone: the journal says how, and the system
    //    runs the recovery at the next sign-in.
    let mut journal = Journal {
        stage: Stage::Moving,
        helper_pid: sys.pid(),
        helper_started: sys.started(sys.pid()),
        install_dir: job.install_dir.clone(),
        backup_dir: job.backup_dir.clone(),
        marker: job.marker.clone(),
        app_exe: job.app_exe.clone(),
        version: job.version.clone(),
        keep: job.keep.clone(),
        uninstall_key: job.uninstall_key.clone(),
        shown_version: job
            .uninstall_key
            .as_deref()
            .and_then(|k| sys.shown_version(k)),
    };
    let name = recovery_name(&job.app_exe);
    let journaled = write_journal(&journal_path, &journal)
        .and_then(|()| sys.register_recovery(&name, &journal_path));
    if let Err(e) = journaled {
        let _ = std::fs::remove_file(&journal_path);
        sys.clear_recovery(&name);
        return refuse_and_reopen(job, sys, log, &e);
    }
    sys.checkpoint("journaled");

    // 4. Set the old files aside.
    if let Err(e) = set_aside(sys, &job.install_dir, &job.backup_dir, &job.keep) {
        let _ = std::fs::remove_file(&journal_path);
        sys.clear_recovery(&name);
        return refuse_and_reopen(job, sys, log, &e);
    }
    journal.stage = Stage::Installing;
    let installed = write_journal(&journal_path, &journal).and_then(|()| {
        log.line("old files moved to the backup");
        sys.checkpoint("moved");
        // 5-7. Install, start, wait for the new version to say it runs.
        install_and_confirm(job, sys, log, &app, &mut journal, confirm)
    });
    match installed {
        Ok(()) => {
            let _ = std::fs::remove_file(&job.installer);
            let _ = std::fs::remove_file(&journal_path);
            sys.clear_recovery(&name);
            Outcome::Updated
        }
        Err(why) => {
            log.line(&format!("rolling back: {why}"));
            if let Err(e) = roll_back(sys, &journal_path, &journal, log) {
                // The journal and the sign-in entry stay: the recovery tries again.
                return Outcome::RolledBack(format!("{why}; and the rollback failed: {e}"));
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
    journal: &mut Journal,
    confirm: Duration,
) -> Result<(), String> {
    sys.run_installer(&job.installer, &job.install_dir)?;
    if !app.is_file() {
        return Err("the installer did not write the app".into());
    }
    log.line("installer finished");
    sys.checkpoint("installed");
    let marker = Marker {
        bundle: job.install_dir.clone(),
        old: job.backup_dir.clone(),
        version: job.version.clone(),
    };
    let json = serde_json::to_value(&marker).map_err(|e| e.to_string())?;
    write_json(&job.marker, &json).map_err(|e| format!("cannot write the marker: {e}"))?;
    journal.stage = Stage::Started;
    write_journal(&job.journal(), journal)?;
    sys.checkpoint("marker");
    let mut child = sys.launch(app, &job.launch_args)?;
    log.line("new version started");
    sys.checkpoint("started");
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

/// Parses the helper's command line. Every option takes one value; `--launch-arg` and
/// `--keep` may repeat.
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
            "--keep" => job.keep.push(text(value()?)),
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
    let file_name = |name: &str| !name.is_empty() && !name.contains(['/', '\\']);
    if !file_name(&job.app_exe) {
        return Err("--app-exe is a file name, not a path".into());
    }
    if !job.keep.iter().all(|k| file_name(k)) || job.keep.contains(&job.app_exe) {
        return Err("--keep is the file name of a program other than the app".into());
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
    for k in &job.keep {
        push("--keep", k.clone().into());
    }
    if job.wait_secs != 0 {
        push("--wait-secs", job.wait_secs.to_string().into());
    }
    if job.confirm_secs != 0 {
        push("--confirm-secs", job.confirm_secs.to_string().into());
    }
    args
}

/// The program's entry: an update job, `--recover <journal>` after an interrupted one, or
/// `--shortcut` for the installer.
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
    if args.first().is_some_and(|a| a == "--recover") {
        let Some(journal) = args.get(1).map(PathBuf::from) else {
            eprintln!("usage: --recover <journal>");
            return 64;
        };
        let log = Log::new(Some(journal.with_file_name("update.log")));
        return match recover(&journal, &Os, &log) {
            Recovery::Failed(_) => 1,
            _ => 0,
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
