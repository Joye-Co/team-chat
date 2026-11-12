use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

use super::*;

/// What the made-up new version does after the helper starts it.
#[derive(Clone, Copy, PartialEq)]
enum NewVersion {
    /// Finishes the update as the app does (removes the backup and the marker) and keeps
    /// running.
    Starts,
    /// Finishes the update and exits at once.
    StartsAndExits,
    /// Exits without touching the marker.
    Crashes,
    /// Keeps running and never removes the marker.
    Hangs,
}

/// A valid signature of `subject`.
fn valid(subject: &str) -> Signer {
    signed("Valid", subject, subject)
}

fn signed(status: &str, subject: &str, thumbprint: &str) -> Signer {
    Signer::Signed {
        status: status.into(),
        subject: subject.into(),
        thumbprint: thumbprint.into(),
    }
}

/// The helper's process id in the tests.
const HELPER: u32 = 4242;

struct Fake {
    app_exits: bool,
    /// The helper of the journal is still running, and this is another helper looking.
    helper_alive: bool,
    /// The process that has the journal's helper id now was made at another time: the
    /// computer restarted and the id went to some other program.
    helper_id_reused: bool,
    /// The step at which this helper is killed (a panic the test catches).
    dies_at: Option<&'static str>,
    /// The helper is killed while the installer is half way through.
    dies_in_installer: bool,
    installer_ok: bool,
    writes_app: bool,
    new_version: NewVersion,
    installed: Signer,
    installer: Signer,
    marker: PathBuf,
    /// The sign-in entry: its name and the journal it names.
    recovery: RefCell<Option<(String, PathBuf)>>,
    shown: RefCell<BTreeMap<String, String>>,
    launches: RefCell<Vec<(PathBuf, Vec<OsString>, String)>>,
    killed: Rc<Cell<bool>>,
}

struct FakeChild {
    exited: bool,
    killed: Rc<Cell<bool>>,
}

impl Running for FakeChild {
    fn exited(&mut self) -> bool {
        self.exited
    }
    fn kill(&mut self) {
        self.killed.set(true);
    }
}

impl Fake {
    /// What the app does at its first start after an update.
    fn finish_like_the_app(&self) {
        let marker: Marker = serde_json::from_slice(&std::fs::read(&self.marker).unwrap()).unwrap();
        std::fs::remove_dir_all(&marker.old).unwrap();
        std::fs::remove_file(&self.marker).unwrap();
    }
}

impl Sys for Fake {
    fn wait_exit(&self, pid: u32, _timeout: Duration) -> bool {
        if pid == HELPER {
            !self.helper_alive
        } else {
            self.app_exits
        }
    }

    fn pid(&self) -> u32 {
        if self.helper_alive {
            HELPER + 1
        } else {
            HELPER
        }
    }

    fn started(&self, pid: u32) -> Option<u64> {
        // The helper that writes the journal is made at 1000. Whoever has its id when
        // another helper looks was made then too, unless the id was reused.
        Some(if pid == HELPER && self.helper_id_reused {
            2000
        } else {
            1000
        })
    }

    fn register_recovery(&self, name: &str, journal: &Path) -> Result<(), String> {
        *self.recovery.borrow_mut() = Some((name.into(), journal.to_path_buf()));
        Ok(())
    }

    fn clear_recovery(&self, name: &str) {
        let mut recovery = self.recovery.borrow_mut();
        if recovery.as_ref().is_some_and(|(n, _)| n == name) {
            *recovery = None;
        }
    }

    fn checkpoint(&self, step: &str) {
        if self.dies_at == Some(step) {
            panic!("killed at {step}");
        }
    }

    fn run_installer(&self, installer: &Path, install_dir: &Path) -> Result<(), String> {
        assert!(installer.is_file());
        // The old files are in the backup; only the daemon stayed (it may be running).
        let left: Vec<_> = std::fs::read_dir(install_dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left, vec![OsString::from("daemon.exe")]);
        if !self.installer_ok {
            std::fs::write(install_dir.join("half.tmp"), "x").unwrap();
            return Err("the installer failed (exit code: 2)".into());
        }
        // The daemon by its protocol: the new program beside it, the old one renamed.
        std::fs::write(install_dir.join("daemon.exe.new"), "new daemon").unwrap();
        std::fs::rename(
            install_dir.join("daemon.exe"),
            install_dir.join("daemon.exe.old-1"),
        )
        .unwrap();
        if self.dies_in_installer {
            // Between the two renames: no daemon.exe at all.
            panic!("killed in the installer");
        }
        std::fs::rename(
            install_dir.join("daemon.exe.new"),
            install_dir.join("daemon.exe"),
        )
        .unwrap();
        if self.writes_app {
            std::fs::write(install_dir.join("App.exe"), "new app").unwrap();
        }
        std::fs::write(install_dir.join("only-new.dll"), "new").unwrap();
        self.shown
            .borrow_mut()
            .insert("Uninstall\\app".into(), "2.0.0".into());
        Ok(())
    }

    fn launch(&self, exe: &Path, args: &[OsString]) -> Result<Box<dyn Running>, String> {
        let content = std::fs::read_to_string(exe).map_err(|e| e.to_string())?;
        self.launches
            .borrow_mut()
            .push((exe.to_path_buf(), args.to_vec(), content.clone()));
        let mut exited = false;
        if content == "new app" {
            match self.new_version {
                NewVersion::Starts => self.finish_like_the_app(),
                NewVersion::StartsAndExits => {
                    self.finish_like_the_app();
                    exited = true;
                }
                NewVersion::Crashes => exited = true,
                NewVersion::Hangs => {}
            }
        }
        Ok(Box::new(FakeChild {
            exited,
            killed: self.killed.clone(),
        }))
    }

    fn signer(&self, file: &Path) -> Signer {
        if file.extension().is_some_and(|e| e == "bin") {
            self.installer.clone()
        } else {
            self.installed.clone()
        }
    }

    fn shown_version(&self, key: &str) -> Option<String> {
        self.shown.borrow().get(key).cloned()
    }

    fn set_shown_version(&self, key: &str, version: &str) {
        self.shown.borrow_mut().insert(key.into(), version.into());
    }

    fn sleep(&self, _d: Duration) {}
}

struct World {
    _dir: tempfile::TempDir,
    job: Job,
    sys: Fake,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let install_dir = root.join("Programs/App");
    std::fs::create_dir_all(install_dir.join("resources")).unwrap();
    std::fs::write(install_dir.join("App.exe"), "old app").unwrap();
    std::fs::write(install_dir.join("daemon.exe"), "old daemon").unwrap();
    std::fs::write(install_dir.join("resources/a.txt"), "old resource").unwrap();
    let installer = root.join("updates/setup.bin");
    std::fs::create_dir_all(installer.parent().unwrap()).unwrap();
    std::fs::write(&installer, "installer bytes").unwrap();
    let marker = root.join("data/updates/pending.json");
    let job = Job {
        wait_pid: Some(1),
        sha256: sha256_file(&installer).unwrap(),
        installer,
        backup_dir: root.join("Programs/.App.old-2.0.0"),
        install_dir,
        app_exe: "App.exe".into(),
        version: "2.0.0".into(),
        marker: marker.clone(),
        result: Some(root.join("data/updates/result.json")),
        log: Some(root.join("data/updates/update.log")),
        launch_args: vec!["--flag".into()],
        uninstall_key: Some("Uninstall\\app".into()),
        keep: vec!["daemon.exe".into()],
        wait_secs: 0,
        confirm_secs: 0,
    };
    let sys = Fake {
        app_exits: true,
        helper_alive: false,
        helper_id_reused: false,
        dies_at: None,
        dies_in_installer: false,
        installer_ok: true,
        writes_app: true,
        new_version: NewVersion::Starts,
        installed: Signer::Unsigned,
        installer: Signer::Unsigned,
        marker,
        recovery: RefCell::default(),
        shown: RefCell::new(BTreeMap::from([("Uninstall\\app".into(), "1.0.0".into())])),
        launches: RefCell::default(),
        killed: Rc::default(),
    };
    World {
        _dir: dir,
        job,
        sys,
    }
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

fn result(job: &Job) -> serde_json::Value {
    serde_json::from_str(&read(job.result.as_ref().unwrap())).unwrap()
}

fn shown(w: &World) -> Option<String> {
    w.sys.shown_version("Uninstall\\app")
}

/// The install folder holds exactly the old version again, and nothing waits for a
/// recovery.
fn assert_old_version(w: &World) {
    let dir = &w.job.install_dir;
    assert_eq!(read(&dir.join("App.exe")), "old app");
    assert_eq!(read(&dir.join("daemon.exe")), "old daemon");
    assert_eq!(read(&dir.join("resources/a.txt")), "old resource");
    assert_eq!(std::fs::read_dir(dir).unwrap().count(), 3);
    assert!(!w.job.backup_dir.exists());
    assert!(!w.job.marker.exists());
    assert_eq!(shown(w).as_deref(), Some("1.0.0"));
    assert!(!w.job.journal().exists());
    assert_eq!(*w.sys.recovery.borrow(), None);
}

#[test]
fn updates_and_replaces_the_daemon_by_its_protocol() {
    let w = world();
    assert_eq!(run(&w.job, &w.sys), Outcome::Updated);
    let dir = &w.job.install_dir;
    assert_eq!(read(&dir.join("App.exe")), "new app");
    assert_eq!(read(&dir.join("daemon.exe")), "new daemon");
    assert!(!dir.join("resources").exists(), "old files do not stay");
    // The new version removed the backup (it knows the folder from the marker). The old
    // daemon is beside the new one under the name its protocol gives it, for the daemon
    // to remove when it restarts.
    assert!(!w.job.backup_dir.exists());
    assert_eq!(read(&dir.join("daemon.exe.old-1")), "old daemon");
    assert!(!w.job.installer.exists(), "the installer is used up");
    assert_eq!(shown(&w).as_deref(), Some("2.0.0"));
    assert!(!w.job.journal().exists());
    assert_eq!(*w.sys.recovery.borrow(), None);
    let launches = w.sys.launches.borrow();
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0].0, dir.join("App.exe"));
    assert_eq!(launches[0].1, vec![OsString::from("--flag")]);
    assert_eq!(result(&w.job)["outcome"], "updated");
    assert_eq!(result(&w.job)["version"], "2.0.0");
    assert!(read(w.job.log.as_ref().unwrap()).contains("updated"));
}

#[test]
fn the_backup_holds_the_old_files_and_a_copy_of_the_daemon_while_the_installer_runs() {
    let mut w = world();
    w.sys.dies_at = Some("moved");
    killed(&w);
    let backup = &w.job.backup_dir;
    assert_eq!(read(&backup.join("App.exe")), "old app");
    assert_eq!(read(&backup.join("resources/a.txt")), "old resource");
    assert_eq!(read(&backup.join("daemon.exe")), "old daemon");
    // The daemon itself never left its place.
    assert_eq!(read(&w.job.install_dir.join("daemon.exe")), "old daemon");
    let journal: Journal =
        serde_json::from_slice(&std::fs::read(w.job.journal()).unwrap()).unwrap();
    assert_eq!(journal.stage, Stage::Installing);
    assert_eq!(journal.helper_pid, HELPER);
    assert_eq!(journal.shown_version.as_deref(), Some("1.0.0"));
    assert_eq!(journal.keep, vec!["daemon.exe".to_string()]);
}

#[test]
fn the_marker_names_the_install_folder_the_backup_and_the_version() {
    let mut w = world();
    // Killed right after the marker is written: it is still there to read.
    w.sys.dies_at = Some("marker");
    killed(&w);
    let marker: Marker = serde_json::from_slice(&std::fs::read(&w.job.marker).unwrap()).unwrap();
    assert_eq!(
        marker,
        Marker {
            bundle: w.job.install_dir.clone(),
            old: w.job.backup_dir.clone(),
            version: "2.0.0".into(),
        }
    );
}

#[test]
fn a_new_version_that_finishes_the_update_and_exits_counts_as_started() {
    let mut w = world();
    w.sys.new_version = NewVersion::StartsAndExits;
    assert_eq!(run(&w.job, &w.sys), Outcome::Updated);
}

#[test]
fn rolls_back_when_the_new_version_exits_early() {
    let mut w = world();
    w.sys.new_version = NewVersion::Crashes;
    let outcome = run(&w.job, &w.sys);
    assert!(
        matches!(&outcome, Outcome::RolledBack(why) if why.contains("exited before")),
        "{outcome:?}"
    );
    assert_old_version(&w);
    // The old version is started again, with the same arguments.
    let launches = w.sys.launches.borrow();
    assert_eq!(launches.len(), 2);
    assert_eq!(launches[0].2, "new app");
    assert_eq!(launches[1].2, "old app");
    assert_eq!(launches[1].1, vec![OsString::from("--flag")]);
    assert!(w.job.installer.exists(), "a failed installer is kept");
    assert_eq!(result(&w.job)["outcome"], "rolled_back");
    assert_eq!(outcome.exit_code(), 3);
}

#[test]
fn rolls_back_and_stops_a_new_version_that_never_confirms() {
    let mut w = world();
    w.sys.new_version = NewVersion::Hangs;
    w.job.confirm_secs = 1;
    let outcome = run(&w.job, &w.sys);
    assert!(
        matches!(&outcome, Outcome::RolledBack(why) if why.contains("in time")),
        "{outcome:?}"
    );
    assert!(w.sys.killed.get());
    assert_old_version(&w);
}

#[test]
fn rolls_back_when_the_installer_fails_or_writes_no_app() {
    let mut w = world();
    w.sys.installer_ok = false;
    let outcome = run(&w.job, &w.sys);
    assert!(
        matches!(&outcome, Outcome::RolledBack(why) if why.contains("installer failed")),
        "{outcome:?}"
    );
    assert_old_version(&w);
    assert_eq!(w.sys.launches.borrow().len(), 1, "only the old version");

    let mut w = world();
    w.sys.writes_app = false;
    let outcome = run(&w.job, &w.sys);
    assert!(
        matches!(&outcome, Outcome::RolledBack(why) if why.contains("did not write the app")),
        "{outcome:?}"
    );
    assert_old_version(&w);
}

#[test]
fn refuses_without_touching_anything() {
    // The app still runs.
    let mut w = world();
    w.sys.app_exits = false;
    assert_eq!(
        run(&w.job, &w.sys),
        Outcome::Refused("the app did not exit".into())
    );
    assert_old_version(&w);
    assert!(w.sys.launches.borrow().is_empty(), "it still runs");

    // The installer changed on disk.
    let w = world();
    std::fs::write(&w.job.installer, "other bytes").unwrap();
    let outcome = run(&w.job, &w.sys);
    assert!(
        matches!(&outcome, Outcome::Refused(why) if why.contains("checksum")),
        "{outcome:?}"
    );
    assert_old_version(&w);
    assert_eq!(w.sys.launches.borrow().len(), 1, "the old app opens again");
    assert_eq!(w.sys.launches.borrow()[0].2, "old app");
    assert_eq!(outcome.exit_code(), 2);

    // A signed app and an installer from someone else, from nobody, changed, or unreadable.
    for installer in [
        Signer::Unsigned,
        valid("CN=Somebody Else"),
        signed("HashMismatch", "CN=Publisher", "CN=Publisher"),
        Signer::Unknown("the query failed".into()),
    ] {
        let mut w = world();
        w.sys.installed = valid("CN=Publisher");
        w.sys.installer = installer;
        assert!(matches!(run(&w.job, &w.sys), Outcome::Refused(_)));
        assert_old_version(&w);
    }

    // The system cannot say whether the installed app is signed: that is not "unsigned".
    let mut w = world();
    w.sys.installed = Signer::Unknown("the query failed".into());
    let outcome = run(&w.job, &w.sys);
    assert!(
        matches!(&outcome, Outcome::Refused(why) if why.contains("cannot read the signature")),
        "{outcome:?}"
    );
    assert_old_version(&w);
}

#[test]
fn signer_rule() {
    let unknown = || Signer::Unknown("x".into());
    // An unsigned app: unsigned or validly signed installers, never a broken signature.
    assert!(signer_allows(&Signer::Unsigned, &Signer::Unsigned).is_ok());
    assert!(signer_allows(&Signer::Unsigned, &valid("CN=A")).is_ok());
    assert!(signer_allows(&Signer::Unsigned, &signed("HashMismatch", "CN=A", "1")).is_err());
    // A validly signed app: only the same publisher, validly signed.
    assert!(signer_allows(&valid("CN=A"), &valid("CN=A")).is_ok());
    assert!(signer_allows(&valid("CN=A"), &signed("Valid", "CN=A", "renewed")).is_ok());
    assert!(signer_allows(&valid("CN=A"), &valid("CN=B")).is_err());
    assert!(signer_allows(&valid("CN=A"), &Signer::Unsigned).is_err());
    assert!(signer_allows(&valid("CN=A"), &signed("HashMismatch", "CN=A", "CN=A")).is_err());
    assert!(signer_allows(&valid("CN=A"), &signed("NotTrusted", "CN=A", "CN=A")).is_err());
    // An app signed with a certificate this computer does not trust: only the very same
    // certificate, never unsigned, never a changed file, never the subject alone.
    let untrusted = signed("UnknownError", "CN=A", "1");
    assert!(signer_allows(&untrusted, &signed("UnknownError", "CN=A", "1")).is_ok());
    assert!(signer_allows(&untrusted, &signed("Valid", "CN=A", "1")).is_ok());
    assert!(signer_allows(&untrusted, &signed("UnknownError", "CN=A", "2")).is_err());
    assert!(signer_allows(&untrusted, &signed("Valid", "CN=A", "2")).is_err());
    assert!(signer_allows(&untrusted, &signed("HashMismatch", "CN=A", "1")).is_err());
    assert!(signer_allows(&untrusted, &Signer::Unsigned).is_err());
    // A failed query is an error on either side, whatever the other says.
    for other in [
        Signer::Unsigned,
        valid("CN=A"),
        untrusted.clone(),
        unknown(),
    ] {
        assert!(signer_allows(&unknown(), &other).is_err());
        assert!(signer_allows(&other, &unknown()).is_err());
    }
}

#[test]
fn a_signed_update_from_the_same_publisher_installs() {
    let mut w = world();
    w.sys.installed = valid("CN=Publisher");
    w.sys.installer = valid("CN=Publisher");
    assert_eq!(run(&w.job, &w.sys), Outcome::Updated);
}

/// Runs the update until the helper is killed (at `dies_at`, or inside the installer),
/// then disarms the kill: what follows is another process.
fn killed(w: &World) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&w.job, &w.sys)));
    assert!(result.is_err(), "the helper was not killed");
    // What a kill leaves: the journal, and the entry that runs the recovery at sign-in.
    assert!(w.job.journal().exists());
    assert_eq!(
        *w.sys.recovery.borrow(),
        Some(("App.update-recovery".into(), w.job.journal()))
    );
}

/// The recovery, as the sign-in entry, the app or the next update runs it.
fn recover_later(w: &World) -> Recovery {
    recover(&w.job.journal(), &w.sys, &Log::new(w.job.log.clone()))
}

#[test]
fn an_update_killed_at_any_step_is_undone_by_the_recovery() {
    for step in ["journaled", "moved-one", "moved", "installed", "marker"] {
        let mut w = world();
        w.sys.dies_at = Some(step);
        killed(&w);
        w.sys.dies_at = None;
        assert_eq!(recover_later(&w), Recovery::Restored, "{step}");
        assert_old_version(&w);
        // Running it again finds nothing to do.
        assert_eq!(recover_later(&w), Recovery::Nothing, "{step}");
        assert_old_version(&w);
        // The recovery starts nothing: at a sign-in the user did not ask for the app.
        assert!(w.sys.launches.borrow().is_empty(), "{step}");
    }
}

#[test]
fn an_update_killed_inside_the_installer_gets_its_daemon_back() {
    let mut w = world();
    w.sys.dies_in_installer = true;
    killed(&w);
    // The worst moment of the daemon's protocol: its program is renamed away and the new
    // one is not in place.
    let dir = &w.job.install_dir;
    assert!(!dir.join("daemon.exe").exists());
    assert!(dir.join("daemon.exe.old-1").exists());
    assert!(!dir.join("App.exe").exists());
    w.sys.dies_in_installer = false;
    assert_eq!(recover_later(&w), Recovery::Restored);
    assert_old_version(&w);
}

#[test]
fn a_helper_killed_after_the_new_version_started_leaves_the_new_version() {
    // The new version finished the update (marker and backup gone) before the kill.
    let mut w = world();
    w.sys.dies_at = Some("started");
    killed(&w);
    w.sys.dies_at = None;
    assert_eq!(recover_later(&w), Recovery::Finished);
    let dir = &w.job.install_dir;
    assert_eq!(read(&dir.join("App.exe")), "new app");
    assert_eq!(read(&dir.join("daemon.exe")), "new daemon");
    assert_eq!(shown(&w).as_deref(), Some("2.0.0"));
    assert!(!w.job.journal().exists());
    assert_eq!(*w.sys.recovery.borrow(), None);

    // The new version never finished it: the old version comes back.
    let mut w = world();
    w.sys.new_version = NewVersion::Hangs;
    w.sys.dies_at = Some("started");
    killed(&w);
    assert!(w.job.marker.exists());
    w.sys.dies_at = None;
    assert_eq!(recover_later(&w), Recovery::Restored);
    assert_old_version(&w);
}

#[test]
fn the_recovery_leaves_a_running_update_alone() {
    let mut w = world();
    w.sys.dies_at = Some("moved");
    killed(&w);
    // Another helper looks while the first one still runs.
    w.sys.dies_at = None;
    w.sys.helper_alive = true;
    assert_eq!(recover_later(&w), Recovery::Busy);
    assert!(!w.job.install_dir.join("App.exe").exists());
    assert!(w.job.journal().exists());
    // And a new update is refused until it is over.
    assert_eq!(
        run(&w.job, &w.sys),
        Outcome::Refused("another update is running".into())
    );
    w.sys.helper_alive = false;
    assert_eq!(recover_later(&w), Recovery::Restored);
    assert_old_version(&w);
}

#[test]
fn a_recovery_that_must_wait_is_registered_for_the_next_sign_in_again() {
    let mut w = world();
    w.sys.dies_at = Some("moved");
    killed(&w);
    w.sys.dies_at = None;
    // Windows has run the sign-in entry and removed it; the helper still lives.
    *w.sys.recovery.borrow_mut() = None;
    w.sys.helper_alive = true;
    assert_eq!(recover_later(&w), Recovery::Busy);
    assert_eq!(
        w.sys.recovery.borrow().as_ref().map(|(_, j)| j.clone()),
        Some(w.job.journal())
    );
}

#[test]
fn a_process_that_only_has_the_dead_helpers_id_does_not_block_the_recovery() {
    let mut w = world();
    w.sys.dies_at = Some("moved");
    killed(&w);
    w.sys.dies_at = None;
    // After a restart of the computer some other program has the helper's process id: a
    // live process, made at another time than the journal says.
    w.sys.helper_alive = true;
    w.sys.helper_id_reused = true;
    assert_eq!(recover_later(&w), Recovery::Restored);
    assert_old_version(&w);
    assert!(w.sys.recovery.borrow().is_none());
    // A journal of a helper from before the start time was written is known by the id alone.
    let mut w = world();
    w.sys.dies_at = Some("moved");
    killed(&w);
    w.sys.dies_at = None;
    let mut journal: Journal =
        serde_json::from_slice(&std::fs::read(w.job.journal()).unwrap()).unwrap();
    assert_eq!(journal.helper_started, Some(1000));
    journal.helper_started = None;
    std::fs::write(w.job.journal(), serde_json::to_vec(&journal).unwrap()).unwrap();
    w.sys.helper_alive = true;
    w.sys.helper_id_reused = true;
    assert_eq!(recover_later(&w), Recovery::Busy);
}

#[test]
fn a_recovery_that_fails_is_tried_at_the_next_sign_in() {
    let mut w = world();
    w.sys.dies_at = Some("moved");
    killed(&w);
    w.sys.dies_at = None;
    *w.sys.recovery.borrow_mut() = None;
    // The install folder is gone and a file has its name: nothing can be put back into it.
    std::fs::remove_dir_all(&w.job.install_dir).unwrap();
    std::fs::write(&w.job.install_dir, b"in the way").unwrap();
    assert!(matches!(recover_later(&w), Recovery::Failed(_)));
    assert!(w.job.journal().exists());
    assert_eq!(
        w.sys.recovery.borrow().as_ref().map(|(_, j)| j.clone()),
        Some(w.job.journal())
    );
    // The way is clear again: the next sign-in puts the old version back.
    std::fs::remove_file(&w.job.install_dir).unwrap();
    std::fs::create_dir_all(&w.job.install_dir).unwrap();
    assert_eq!(recover_later(&w), Recovery::Restored);
    assert_old_version(&w);
}

#[test]
fn a_new_update_first_undoes_an_interrupted_one() {
    let mut w = world();
    w.sys.dies_in_installer = true;
    killed(&w);
    w.sys.dies_in_installer = false;
    // The next update starts from the old version again and goes through.
    assert_eq!(run(&w.job, &w.sys), Outcome::Updated);
    assert_eq!(read(&w.job.install_dir.join("App.exe")), "new app");
    assert_eq!(read(&w.job.install_dir.join("daemon.exe")), "new daemon");
    assert!(!w.job.journal().exists());
    assert_eq!(*w.sys.recovery.borrow(), None);
}

#[test]
fn recovery_without_a_journal_does_nothing() {
    let w = world();
    assert_eq!(recover_later(&w), Recovery::Nothing);
    assert_old_version(&w);
    std::fs::create_dir_all(w.job.journal().parent().unwrap()).unwrap();
    std::fs::write(w.job.journal(), "not json").unwrap();
    assert_eq!(recover_later(&w), Recovery::Nothing);
    assert_eq!(read(&w.job.install_dir.join("App.exe")), "old app");
}

#[test]
fn command_line_round_trip() {
    let w = world();
    let args = job_args(&w.job);
    assert_eq!(parse_job(&args).unwrap(), w.job);
    let mut plain = w.job.clone();
    plain.wait_pid = None;
    plain.result = None;
    plain.log = None;
    plain.launch_args.clear();
    plain.uninstall_key = None;
    plain.keep.clear();
    plain.confirm_secs = 5;
    assert_eq!(parse_job(&job_args(&plain)).unwrap(), plain);
}

#[test]
fn command_line_errors() {
    let w = world();
    let without = |name: &str| {
        let args = job_args(&w.job);
        let at = args.iter().position(|a| a == name).unwrap();
        let mut out = args.clone();
        out.drain(at..at + 2);
        parse_job(&out)
    };
    assert!(without("--installer").unwrap_err().contains("--installer"));
    assert!(without("--marker").unwrap_err().contains("--marker"));
    assert!(parse_job(&["--nope".into()]).is_err());
    assert!(parse_job(&["--installer".into()]).is_err());
    let mut bad = w.job.clone();
    bad.app_exe = "sub\\App.exe".into();
    assert!(parse_job(&job_args(&bad)).is_err());
    // The backup must be on the install folder's volume: a sibling folder.
    let mut bad = w.job.clone();
    bad.backup_dir = std::env::temp_dir().join("elsewhere");
    assert!(parse_job(&job_args(&bad)).is_err());
    let mut bad = w.job.clone();
    bad.backup_dir = bad.install_dir.clone();
    assert!(parse_job(&job_args(&bad)).is_err());
    // A kept program is a file name, and never the app.
    let mut bad = w.job.clone();
    bad.keep = vec!["sub/daemon.exe".into()];
    assert!(parse_job(&job_args(&bad)).is_err());
    let mut bad = w.job.clone();
    bad.keep = vec!["App.exe".into()];
    assert!(parse_job(&job_args(&bad)).is_err());
}

#[test]
fn registry_and_signature_output() {
    let out = "\r\nHKEY_CURRENT_USER\\Software\\X\r\n    DisplayVersion    REG_SZ    1.2.6\r\n\r\n";
    assert_eq!(
        sys::parse_reg_value(out, "DisplayVersion").as_deref(),
        Some("1.2.6")
    );
    assert_eq!(sys::parse_reg_value(out, "Other"), None);
    assert_eq!(sys::parse_reg_value("", "DisplayVersion"), None);
    assert_eq!(
        sys::parse_signer("Valid\r\nCN=Example Co, O=Example Co, C=US\r\nABCDEF0123\r\n"),
        signed("Valid", "CN=Example Co, O=Example Co, C=US", "ABCDEF0123")
    );
    assert_eq!(sys::parse_signer("NotSigned\r\n"), Signer::Unsigned);
    assert_eq!(
        sys::parse_signer("HashMismatch\r\nCN=Example Co\r\nAB12\r\n"),
        signed("HashMismatch", "CN=Example Co", "AB12")
    );
    assert_eq!(
        sys::parse_signer("UnknownError\r\nCN=Self Signed\r\nCD34\r\n"),
        signed("UnknownError", "CN=Self Signed", "CD34")
    );
    // A query that printed nothing, or a status without its certificate, is not "unsigned".
    assert!(matches!(sys::parse_signer(""), Signer::Unknown(_)));
    assert!(matches!(sys::parse_signer("\r\n"), Signer::Unknown(_)));
    assert!(matches!(sys::parse_signer("Valid\r\n"), Signer::Unknown(_)));
    assert!(matches!(
        sys::parse_signer("Valid\r\nCN=Example Co\r\n"),
        Signer::Unknown(_)
    ));
}

/// The real system on Unix, with shell scripts as the installer and the app: the whole
/// sequence with real processes.
#[cfg(unix)]
#[test]
fn real_processes() {
    use std::os::unix::fs::PermissionsExt as _;
    let script = |path: &Path, text: &str| {
        std::fs::write(path, text).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    let w = world();
    let mut job = w.job.clone();
    job.keep.clear();
    // The installer writes an app that removes the marker ($1) and leaves a note.
    script(
        &job.installer,
        "#!/bin/sh\nset -e\nfor a in \"$@\"; do case \"$a\" in /D=*) d=\"${a#/D=}\";; esac; done\n\
         printf '#!/bin/sh\\nrm -f \"$1\"\\necho new > \"$2\"\\nsleep 5\\n' > \"$d/App.exe\"\n\
         chmod 755 \"$d/App.exe\"\n",
    );
    job.sha256 = sha256_file(&job.installer).unwrap();
    script(&job.install_dir.join("App.exe"), "#!/bin/sh\nexit 0\n");
    let note = job.install_dir.parent().unwrap().join("note.txt");
    job.launch_args = vec![job.marker.clone().into(), note.clone().into()];
    let mut app = std::process::Command::new("/bin/sleep")
        .arg("30")
        .spawn()
        .unwrap();
    job.wait_pid = Some(app.id());
    job.wait_secs = 1;
    // While the app runs, nothing happens.
    assert_eq!(
        run(&job, &Os),
        Outcome::Refused("the app did not exit".into())
    );
    app.kill().unwrap();
    app.wait().unwrap();
    job.wait_secs = 10;
    job.confirm_secs = 20;
    assert_eq!(run(&job, &Os), Outcome::Updated);
    assert_eq!(read(&note).trim(), "new");
    assert!(job.backup_dir.join("daemon.exe").exists());
    assert!(!job.journal().exists());

    // An installer that fails: the old files come back.
    let w = world();
    let mut job = w.job.clone();
    job.keep.clear();
    script(&job.installer, "#!/bin/sh\nexit 7\n");
    job.sha256 = sha256_file(&job.installer).unwrap();
    script(&job.install_dir.join("App.exe"), "#!/bin/sh\nexit 0\n");
    job.wait_pid = None;
    job.launch_args.clear();
    assert!(matches!(run(&job, &Os), Outcome::RolledBack(_)));
    assert_eq!(read(&job.install_dir.join("daemon.exe")), "old daemon");
    assert!(!job.backup_dir.exists());
    assert!(!job.journal().exists());
}
