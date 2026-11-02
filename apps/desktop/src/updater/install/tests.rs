//! Replace-and-relaunch with fake bundles in a temp folder. Nothing is installed: every
//! path is under a `tempfile` folder, and the DMG is made and mounted hidden from Finder.

use std::{
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use super::*;

const ID: &str = "co.example.fake-updater-test";

/// The DMG tests run one at a time: each checks that no mount of this process is left.
static DMG_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());
const NAME: &str = "Fake.app";

/// A fake bundle whose executable writes "<version> <args>" to `$UPDATER_TEST_OUT`.
fn fake_bundle(dir: &Path, version: &str, id: &str) -> PathBuf {
    let app = dir.join(NAME);
    let macos = app.join("Contents/MacOS");
    std::fs::create_dir_all(&macos).unwrap();
    std::fs::write(
        app.join("Contents/Info.plist"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>{id}</string>
  <key>CFBundleExecutable</key><string>Fake</string>
  <key>CFBundleShortVersionString</key><string>{version}</string>
  <key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
"#
        ),
    )
    .unwrap();
    let exe = macos.join("Fake");
    std::fs::write(
        &exe,
        format!("#!/bin/sh\necho \"{version} $*\" > \"$UPDATER_TEST_OUT.tmp\"\nmv \"$UPDATER_TEST_OUT.tmp\" \"$UPDATER_TEST_OUT\"\n"),
    )
    .unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    app
}

fn make_dmg(src: &Path, out: &Path) {
    let status = Command::new("/usr/bin/hdiutil")
        .args(["create", "-quiet", "-ov", "-format", "UDZO", "-fs", "HFS+"])
        .args(["-volname", "FakeUpdate", "-srcfolder"])
        .arg(src)
        .arg(out)
        .status()
        .unwrap();
    assert!(status.success(), "hdiutil create");
}

fn version_of(app: &Path) -> String {
    plist_value(&app.join("Contents/Info.plist"), "CFBundleShortVersionString").unwrap()
}

struct Fixture {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    installed: PathBuf,
    dmg: PathBuf,
}

fn fixture(new_id: &str) -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(tmp.path()).unwrap();
    let apps = root.join("Applications");
    std::fs::create_dir_all(&apps).unwrap();
    let installed = fake_bundle(&apps, "1.0.0", ID);
    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    fake_bundle(&src, "1.1.0", new_id);
    std::os::unix::fs::symlink("/Applications", src.join("Applications")).unwrap();
    let dmg = root.join("Fake-1.1.0.dmg");
    make_dmg(&src, &dmg);
    Fixture {
        _tmp: tmp,
        root,
        installed,
        dmg,
    }
}

fn plan(f: &Fixture) -> Plan {
    Plan {
        bundle: f.installed.clone(),
        dmg: f.dmg.clone(),
        version: "1.1.0".into(),
        sha256: version::sha256_file(&f.dmg).unwrap(),
        bundle_id: ID.into(),
        app_name: Some(NAME.into()),
        marker: f.root.join("data/updates/pending.json"),
    }
}

fn wait_for(path: &Path, timeout: Duration) -> Option<String> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Ok(text) = std::fs::read_to_string(path) {
            return Some(text);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

#[test]
fn bundle_paths() {
    assert_eq!(
        bundle_from_exe(Path::new("/Applications/X.app/Contents/MacOS/X")),
        Some(PathBuf::from("/Applications/X.app"))
    );
    assert_eq!(bundle_from_exe(Path::new("/tmp/target/debug/desktop")), None);
    assert_eq!(bundle_from_exe(Path::new("/a/X/Contents/MacOS/X")), None);
    assert_eq!(bundle_from_exe(Path::new("X")), None);
    let b = Path::new("/Applications/X.app");
    assert_eq!(staging_path(b, "1.2.3"), Path::new("/Applications/.X.app.update-1.2.3"));
    assert_eq!(old_path(b, "1.2.3"), Path::new("/Applications/.X.app.old-1.2.3"));
}

#[test]
fn swap_then_finish_on_launch() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = std::fs::canonicalize(tmp.path()).unwrap();
    let bundle = fake_bundle(&dir, "1.0.0", ID);
    let staged_src = tempfile::tempdir().unwrap();
    let new = fake_bundle(staged_src.path(), "2.0.0", ID);
    let staged = staging_path(&bundle, "2.0.0");
    std::fs::rename(&new, &staged).unwrap();
    let old = old_path(&bundle, "2.0.0");
    swap(&bundle, &staged, &old).unwrap();
    assert_eq!(version_of(&bundle), "2.0.0");
    assert_eq!(version_of(&old), "1.0.0");
    assert!(!staged.exists());

    let marker = dir.join("pending.json");
    let m = Marker {
        bundle: bundle.clone(),
        old: old.clone(),
        version: "2.0.0".into(),
    };
    write_marker(&marker, &m).unwrap();
    // The old process (or another copy) does not clean up.
    assert_eq!(finish_on_launch(&marker, Some(&bundle), "1.0.0"), Finish::NotForMe);
    assert_eq!(
        finish_on_launch(&marker, Some(&dir.join("Other.app")), "2.0.0"),
        Finish::NotForMe
    );
    assert!(old.exists() && marker.exists());
    // A leftover staging folder is removed at start.
    let stale = staging_path(&bundle, "9.9.9");
    std::fs::create_dir_all(stale.join("Contents")).unwrap();
    assert_eq!(finish_on_launch(&marker, Some(&bundle), "2.0.0"), Finish::RemovedOld);
    assert!(!old.exists() && !marker.exists() && !stale.exists());
    assert_eq!(version_of(&bundle), "2.0.0");
    assert_eq!(finish_on_launch(&marker, Some(&bundle), "2.0.0"), Finish::NoMarker);
}

#[test]
fn marker_never_removes_other_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = std::fs::canonicalize(tmp.path()).unwrap();
    let bundle = fake_bundle(&dir, "2.0.0", ID);
    let precious = dir.join("precious");
    std::fs::create_dir_all(&precious).unwrap();
    let marker = dir.join("pending.json");
    write_marker(
        &marker,
        &Marker {
            bundle: bundle.clone(),
            old: precious.clone(),
            version: "2.0.0".into(),
        },
    )
    .unwrap();
    assert_eq!(finish_on_launch(&marker, Some(&bundle), "2.0.0"), Finish::RemovedOld);
    assert!(precious.exists());
    // A damaged marker is dropped.
    std::fs::write(&marker, b"{not json").unwrap();
    assert_eq!(finish_on_launch(&marker, Some(&bundle), "2.0.0"), Finish::NoMarker);
    assert!(!marker.exists());
}

#[test]
fn install_from_dmg_and_relaunch() {
    let _serial = DMG_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture(ID);
    let plan = plan(&f);
    let marker = install(&plan).unwrap();
    assert_eq!(marker.bundle, f.installed);
    assert_eq!(version_of(&f.installed), "1.1.0");
    // The old bundle waits under a hidden name; nothing is left in staging.
    assert_eq!(version_of(&marker.old), "1.0.0");
    assert!(!staging_path(&f.installed, "1.1.0").exists());
    // The volume was detached and its mount folder removed.
    let mounts = String::from_utf8(Command::new("/sbin/mount").output().unwrap().stdout).unwrap();
    assert!(!mounts.contains(&format!("update-mount-{}-", std::process::id())));

    // Relaunch waits for the "app" process to exit, then starts the new bundle.
    let out = f.root.join("started.txt");
    let mut app = Command::new("/bin/sleep").arg("30").spawn().unwrap();
    let exe = f.installed.join("Contents/MacOS/Fake");
    let mut env_arg = OsString::from("UPDATER_TEST_OUT=");
    env_arg.push(&out);
    let argv = vec!["/usr/bin/env".into(), env_arg, exe.into(), "--after-update".into()];
    relaunch(app.id(), &Launch::Exec(argv)).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert!(!out.exists(), "launched before the old process exited");
    app.kill().unwrap();
    app.wait().unwrap();
    let started = wait_for(&out, Duration::from_secs(15)).expect("new version started");
    assert_eq!(started.trim(), "1.1.0 --after-update");

    // The new process finishes the update: the old bundle goes away.
    assert_eq!(
        finish_on_launch(&plan.marker, Some(&f.installed), "1.1.0"),
        Finish::RemovedOld
    );
    assert!(!marker.old.exists());
    assert!(!plan.marker.exists());
}

#[test]
fn install_refuses_bad_input_and_keeps_the_running_bundle() {
    let _serial = DMG_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    let f = fixture("co.example.someone-else");
    // Wrong checksum.
    let mut p = plan(&f);
    p.sha256 = "0".repeat(64);
    assert!(matches!(install(&p), Err(InstallError::Checksum)));
    // Wrong bundle id inside the image.
    let p = plan(&f);
    assert!(matches!(install(&p), Err(InstallError::BadImage(_))));
    // Wrong version.
    let mut p = plan(&f);
    p.bundle_id = "co.example.someone-else".into();
    p.version = "1.2.0".into();
    assert!(matches!(install(&p), Err(InstallError::BadImage(_))));
    for leftover in [staging_path(&f.installed, "1.1.0"), old_path(&f.installed, "1.1.0")] {
        assert!(!leftover.exists(), "{leftover:?}");
    }
    assert_eq!(version_of(&f.installed), "1.0.0");
    assert!(!p.marker.exists());

    // A folder this user cannot write: the caller falls back to opening the DMG.
    let apps = f.installed.parent().unwrap().to_path_buf();
    std::fs::set_permissions(&apps, std::fs::Permissions::from_mode(0o555)).unwrap();
    let mut p = plan(&f);
    p.bundle_id = "co.example.someone-else".into();
    let result = install(&p);
    std::fs::set_permissions(&apps, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(result, Err(InstallError::NotWritable)), "{result:?}");
    assert_eq!(version_of(&f.installed), "1.0.0");
}
