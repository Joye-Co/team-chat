use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

use super::*;

/// What the made-up new version does after the helper starts it.
#[derive(Clone, Copy, PartialEq)]
enum NewVersion {
    /// Removes the marker and keeps running.
    Starts,
    /// Removes the marker and exits at once.
    StartsAndExits,
    /// Exits without touching the marker.
    Crashes,
    /// Keeps running and never removes the marker.
    Hangs,
}

struct Fake {
    app_exits: bool,
    installer_ok: bool,
    /// The installer writes a file the rollback cannot remove (a program that runs).
    writes_app: bool,
    new_version: NewVersion,
    installed: Signer,
    installer: Signer,
    marker: PathBuf,
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

impl Sys for Fake {
    fn wait_exit(&self, _pid: u32, _timeout: Duration) -> bool {
        self.app_exits
    }

    fn run_installer(&self, installer: &Path, install_dir: &Path) -> Result<(), String> {
        assert!(installer.is_file());
        // The installer starts from an empty folder: the old files are in the backup.
        assert_eq!(std::fs::read_dir(install_dir).unwrap().count(), 0);
        if !self.installer_ok {
            std::fs::write(install_dir.join("half.tmp"), "x").unwrap();
            return Err("the installer failed (exit code: 2)".into());
        }
        if self.writes_app {
            std::fs::write(install_dir.join("App.exe"), "new app").unwrap();
        }
        std::fs::write(install_dir.join("daemon.exe"), "new daemon").unwrap();
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
                NewVersion::Starts => std::fs::remove_file(&self.marker).unwrap(),
                NewVersion::StartsAndExits => {
                    std::fs::remove_file(&self.marker).unwrap();
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
        wait_secs: 0,
        confirm_secs: 0,
    };
    let sys = Fake {
        app_exits: true,
        installer_ok: true,
        writes_app: true,
        new_version: NewVersion::Starts,
        installed: Signer::Unsigned,
        installer: Signer::Unsigned,
        marker,
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

/// The install folder holds exactly the old version again.
fn assert_old_version(w: &World) {
    let dir = &w.job.install_dir;
    assert_eq!(read(&dir.join("App.exe")), "old app");
    assert_eq!(read(&dir.join("daemon.exe")), "old daemon");
    assert_eq!(read(&dir.join("resources/a.txt")), "old resource");
    assert_eq!(std::fs::read_dir(dir).unwrap().count(), 3);
    assert!(!w.job.backup_dir.exists());
    assert!(!w.job.marker.exists());
    assert_eq!(
        w.sys.shown_version("Uninstall\\app").as_deref(),
        Some("1.0.0")
    );
}

#[test]
fn updates_and_leaves_the_backup_for_the_new_version_to_remove() {
    let w = world();
    assert_eq!(run(&w.job, &w.sys), Outcome::Updated);
    let dir = &w.job.install_dir;
    assert_eq!(read(&dir.join("App.exe")), "new app");
    assert_eq!(read(&dir.join("daemon.exe")), "new daemon");
    assert!(!dir.join("resources").exists(), "old files do not stay");
    // The old files wait in the backup: the new version removes them (it knows the folder
    // from the marker, which the fake new version removed like the app does).
    assert_eq!(read(&w.job.backup_dir.join("App.exe")), "old app");
    assert_eq!(
        read(&w.job.backup_dir.join("resources/a.txt")),
        "old resource"
    );
    assert!(!w.job.installer.exists(), "the installer is used up");
    assert_eq!(
        w.sys.shown_version("Uninstall\\app").as_deref(),
        Some("2.0.0")
    );
    let launches = w.sys.launches.borrow();
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0].0, dir.join("App.exe"));
    assert_eq!(launches[0].1, vec![OsString::from("--flag")]);
    assert_eq!(result(&w.job)["outcome"], "updated");
    assert_eq!(result(&w.job)["version"], "2.0.0");
    assert!(read(w.job.log.as_ref().unwrap()).contains("updated"));
}

#[test]
fn the_marker_names_the_install_folder_the_backup_and_the_version() {
    let mut w = world();
    // A new version that never removes the marker lets the test read it before rollback.
    w.sys.new_version = NewVersion::Hangs;
    let marker = Rc::new(RefCell::new(None));
    struct Peek<'a>(&'a Fake, Rc<RefCell<Option<Marker>>>, PathBuf);
    impl Sys for Peek<'_> {
        fn wait_exit(&self, pid: u32, t: Duration) -> bool {
            self.0.wait_exit(pid, t)
        }
        fn run_installer(&self, i: &Path, d: &Path) -> Result<(), String> {
            self.0.run_installer(i, d)
        }
        fn launch(&self, exe: &Path, args: &[OsString]) -> Result<Box<dyn Running>, String> {
            if let Ok(bytes) = std::fs::read(&self.2) {
                *self.1.borrow_mut() = serde_json::from_slice(&bytes).ok();
            }
            self.0.launch(exe, args)
        }
        fn signer(&self, f: &Path) -> Signer {
            self.0.signer(f)
        }
        fn shown_version(&self, k: &str) -> Option<String> {
            self.0.shown_version(k)
        }
        fn set_shown_version(&self, k: &str, v: &str) {
            self.0.set_shown_version(k, v)
        }
        fn sleep(&self, _d: Duration) {}
    }
    w.job.confirm_secs = 1;
    let peek = Peek(&w.sys, marker.clone(), w.job.marker.clone());
    assert!(matches!(run(&w.job, &peek), Outcome::RolledBack(_)));
    assert_eq!(
        marker.borrow().clone(),
        Some(Marker {
            bundle: w.job.install_dir.clone(),
            old: w.job.backup_dir.clone(),
            version: "2.0.0".into(),
        })
    );
}

#[test]
fn a_new_version_that_removes_the_marker_and_exits_counts_as_started() {
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

    // A signed app and an installer from someone else, or from nobody.
    for installer in [
        Signer::Unsigned,
        Signer::Valid("CN=Somebody Else".into()),
        Signer::Invalid("HashMismatch".into()),
    ] {
        let mut w = world();
        w.sys.installed = Signer::Valid("CN=Publisher".into());
        w.sys.installer = installer;
        assert!(matches!(run(&w.job, &w.sys), Outcome::Refused(_)));
        assert_old_version(&w);
    }
}

#[test]
fn signer_rule() {
    let valid = |s: &str| Signer::Valid(s.into());
    assert!(signer_allows(&Signer::Unsigned, &Signer::Unsigned).is_ok());
    assert!(signer_allows(&Signer::Unsigned, &valid("CN=A")).is_ok());
    assert!(signer_allows(&valid("CN=A"), &valid("CN=A")).is_ok());
    assert!(signer_allows(&valid("CN=A"), &valid("CN=B")).is_err());
    assert!(signer_allows(&valid("CN=A"), &Signer::Unsigned).is_err());
    assert!(signer_allows(&valid("CN=A"), &Signer::Invalid("x".into())).is_err());
    // A tampered installer is refused whoever asks.
    assert!(signer_allows(&Signer::Unsigned, &Signer::Invalid("HashMismatch".into())).is_err());
    // A broken signature on the installed app does not block updates for ever.
    assert!(signer_allows(&Signer::Invalid("x".into()), &valid("CN=A")).is_ok());
}

#[test]
fn a_signed_update_from_the_same_publisher_installs() {
    let mut w = world();
    w.sys.installed = Signer::Valid("CN=Publisher".into());
    w.sys.installer = Signer::Valid("CN=Publisher".into());
    assert_eq!(run(&w.job, &w.sys), Outcome::Updated);
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
        sys::parse_signer("Valid\r\nCN=Example Co, O=Example Co, C=US\r\n"),
        Signer::Valid("CN=Example Co, O=Example Co, C=US".into())
    );
    assert_eq!(sys::parse_signer("NotSigned\r\n"), Signer::Unsigned);
    assert_eq!(sys::parse_signer(""), Signer::Unsigned);
    assert_eq!(
        sys::parse_signer("HashMismatch\r\nCN=Example Co\r\n"),
        Signer::Invalid("HashMismatch".into())
    );
    assert_eq!(
        sys::parse_signer("UnknownError\r\nCN=Self Signed\r\n"),
        Signer::Invalid("UnknownError".into())
    );
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

    // An installer that fails: the old files come back.
    let w = world();
    let mut job = w.job.clone();
    script(&job.installer, "#!/bin/sh\nexit 7\n");
    job.sha256 = sha256_file(&job.installer).unwrap();
    script(&job.install_dir.join("App.exe"), "#!/bin/sh\nexit 0\n");
    job.wait_pid = None;
    job.launch_args.clear();
    assert!(matches!(run(&job, &Os), Outcome::RolledBack(_)));
    assert_eq!(read(&job.install_dir.join("daemon.exe")), "old daemon");
    assert!(!job.backup_dir.exists());
}
