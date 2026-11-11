//! The Windows install step: hand the downloaded installer to the update helper.
//!
//! Windows cannot overwrite a program that runs, so the app does not replace itself as it
//! does on macOS. [`prepare`] checks the installer and the install folder and copies the
//! helper (`apps/update-helper`, installed next to the app) out of the install folder, because
//! the helper's own file is replaced too. [`start`] runs that copy and the app quits. The
//! helper waits for the app's process, moves the old files into a backup folder next to
//! the install folder, runs the installer silently, writes the marker and starts the new
//! version, which removes the marker and the backup in `finish_on_launch` as on macOS.
//! When the new version does not get that far, the helper puts the old files back.
//!
//! The module compiles on every system so its tests run on the Mac; only [`start`] and
//! [`install_dir_from_exe`] are used by the Windows app.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use super::{InstallError, Marker, Plan, can_replace, old_path};
use crate::updater::version;

/// File name of the update helper in the install folder.
pub const HELPER: &str = concat!(env!("PRODUCT_SLUG"), "-update.exe");
/// File name of the uninstaller the installer writes.
pub const UNINSTALLER: &str = "uninstall.exe";
/// File name of the machine daemon in the install folder, when the installer shipped it.
/// It may be running a bot, so an update never moves or stops it: the installer replaces
/// it by the daemon's rename protocol (docs/machine-protocol.md) and the helper leaves it
/// in place.
pub const DAEMON_EXE: &str = concat!(env!("PRODUCT_SLUG"), "-daemon.exe");
/// File name of the copy of the helper that runs an update, in the updates folder.
const HELPER_COPY: &str = concat!(env!("PRODUCT_SLUG"), "-update-run.exe");
/// File name of the app's program in the install folder.
pub const APP_EXE: &str = concat!(env!("PRODUCT_NAME"), ".exe");
/// The app's entry in 应用和功能, under `HKEY_CURRENT_USER`.
pub const UNINSTALL_KEY: &str = concat!(
    r"Software\Microsoft\Windows\CurrentVersion\Uninstall\",
    env!("PRODUCT_BUNDLE_ID")
);

/// The helper, ready to run.
#[derive(Clone, Debug)]
pub struct Staged {
    /// The copy of the helper outside the install folder.
    pub helper: PathBuf,
    pub job: update_helper::Job,
}

/// The install folder that holds `exe`, when the installer put it there (the helper and
/// the uninstaller are next to it). `None` for a build that runs from `target/`.
pub fn install_dir_from_exe(exe: &Path) -> Option<PathBuf> {
    let dir = exe.parent()?;
    (dir.join(HELPER).is_file() && dir.join(UNINSTALLER).is_file()).then(|| dir.to_path_buf())
}

/// The file name of a downloaded installer.
pub fn installer_name(version: &str) -> String {
    format!("{}-{version}-setup.exe", env!("PRODUCT_NAME"))
}

/// Checks the installer and the install folder and copies the helper next to the
/// installer. Changes nothing in the install folder. `launch_args` are passed to the app
/// when the helper starts it (empty for the app; the self-test names a report).
pub fn prepare(plan: &Plan, launch_args: Vec<OsString>) -> Result<(Marker, Staged), InstallError> {
    if !version::checksum_matches(&plan.dmg, &plan.sha256)? {
        return Err(InstallError::Checksum);
    }
    if !plan.bundle.is_dir() || !can_replace(&plan.bundle) {
        return Err(InstallError::NotWritable);
    }
    let source = plan.bundle.join(HELPER);
    if !source.is_file() {
        return Err(InstallError::BadImage("no update helper".into()));
    }
    let updates = plan
        .marker
        .parent()
        .ok_or_else(|| InstallError::Io("the marker has no folder".into()))?;
    std::fs::create_dir_all(updates)?;
    // Its own name: the downloads are cleared by their "-setup.exe" ending.
    let helper = updates.join(HELPER_COPY);
    std::fs::copy(&source, &helper)?;
    let marker = Marker {
        bundle: plan.bundle.clone(),
        old: old_path(&plan.bundle, &plan.version),
        version: plan.version.clone(),
    };
    let job = update_helper::Job {
        wait_pid: None,
        installer: plan.dmg.clone(),
        sha256: plan.sha256.clone(),
        install_dir: plan.bundle.clone(),
        backup_dir: marker.old.clone(),
        app_exe: APP_EXE.into(),
        version: plan.version.clone(),
        marker: plan.marker.clone(),
        result: Some(updates.join("update-result.json")),
        log: Some(updates.join("update.log")),
        launch_args,
        uninstall_key: Some(UNINSTALL_KEY.into()),
        keep: vec![DAEMON_EXE.into()],
        wait_secs: 0,
        confirm_secs: 0,
    };
    Ok((marker, Staged { helper, job }))
}

/// Starts the helper; it begins when the process `pid` is gone. The caller quits.
pub fn start(staged: &Staged, pid: u32) -> std::io::Result<()> {
    let mut job = staged.job.clone();
    job.wait_pid = Some(pid);
    let mut cmd = std::process::Command::new(&staged.helper);
    cmd.args(update_helper::job_args(&job))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        // Its own process group and no console: it outlives the app.
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
    cmd.spawn().map(|_| ())
}

/// Ends an update that was cut short (the helper was killed, the user signed out, the power
/// went) when its journal is still in the updates folder: starts the helper's recovery,
/// which puts the old version back or finds that the new one had finished. Windows also
/// runs the same recovery at the next sign-in; this covers an app that is started first.
/// While the helper of a running update lives, the recovery leaves it alone.
pub fn recover_interrupted(updates: &Path) {
    let journal = updates.join(update_helper::JOURNAL);
    let helper = updates.join(HELPER_COPY);
    if !journal.is_file() || !helper.is_file() {
        return;
    }
    let mut cmd = std::process::Command::new(&helper);
    cmd.arg("--recover")
        .arg(&journal)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    match cmd.spawn() {
        Ok(_) => tracing::info!("an update was interrupted; recovery started"),
        Err(e) => tracing::warn!(error = %e, "cannot start the update recovery"),
    }
}

/// Removes backup folders of earlier updates next to the install folder. The new version
/// removes its own backup at start; one stays when a program in it still ran then (the
/// machine daemon keeps running from its renamed file until it restarts).
pub fn remove_stale_backups(install_dir: &Path) {
    let (Some(parent), Some(name)) = (install_dir.parent(), install_dir.file_name()) else {
        return;
    };
    let prefix = format!(".{}.old-", name.to_string_lossy());
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = super::remove_all(&entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed(root: &Path) -> PathBuf {
        let dir = root.join("Programs").join(env!("PRODUCT_NAME"));
        std::fs::create_dir_all(&dir).unwrap();
        for name in [APP_EXE, HELPER, UNINSTALLER] {
            std::fs::write(dir.join(name), name).unwrap();
        }
        dir
    }

    fn plan(root: &Path, dir: &Path) -> Plan {
        let installer = root.join("data/updates").join(installer_name("9.0.0"));
        std::fs::create_dir_all(installer.parent().unwrap()).unwrap();
        std::fs::write(&installer, b"installer").unwrap();
        Plan {
            bundle: dir.to_path_buf(),
            sha256: version::sha256_file(&installer).unwrap(),
            dmg: installer,
            version: "9.0.0".into(),
            bundle_id: env!("PRODUCT_BUNDLE_ID").into(),
            app_name: None,
            marker: root.join("data/updates/pending.json"),
        }
    }

    #[test]
    fn finds_the_install_folder_only_for_an_installed_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = installed(tmp.path());
        assert_eq!(install_dir_from_exe(&dir.join(APP_EXE)), Some(dir.clone()));
        std::fs::remove_file(dir.join(UNINSTALLER)).unwrap();
        assert_eq!(install_dir_from_exe(&dir.join(APP_EXE)), None);
        assert_eq!(install_dir_from_exe(Path::new(APP_EXE)), None);
    }

    #[test]
    fn prepare_copies_the_helper_and_describes_the_update() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = installed(tmp.path());
        let plan = plan(tmp.path(), &dir);
        let (marker, staged) = prepare(&plan, vec!["--flag".into()]).unwrap();
        // The helper runs from outside the folder it replaces.
        assert!(!staged.helper.starts_with(&dir));
        assert_eq!(std::fs::read_to_string(&staged.helper).unwrap(), HELPER);
        assert!(!staged.helper.to_string_lossy().ends_with("-setup.exe"));
        // The marker is the helper's to write, after the installer ran.
        assert!(!plan.marker.exists());
        assert_eq!(marker.bundle, dir);
        assert_eq!(marker.old, old_path(&dir, "9.0.0"));
        assert_eq!(marker.old.parent(), dir.parent());
        let job = &staged.job;
        assert_eq!(job.backup_dir, marker.old);
        assert_eq!(job.install_dir, dir);
        assert_eq!(job.installer, plan.dmg);
        assert_eq!(job.marker, plan.marker);
        assert_eq!(job.app_exe, APP_EXE);
        assert_eq!(job.keep, vec![DAEMON_EXE.to_string()]);
        assert_eq!(
            job.journal(),
            plan.marker.with_file_name(update_helper::JOURNAL)
        );
        assert_eq!(job.version, "9.0.0");
        assert!(
            job.uninstall_key
                .as_deref()
                .unwrap()
                .ends_with(env!("PRODUCT_BUNDLE_ID"))
        );
        // The helper accepts exactly this command line.
        let mut with_pid = job.clone();
        with_pid.wait_pid = Some(42);
        assert_eq!(
            update_helper::parse_job(&update_helper::job_args(&with_pid)).unwrap(),
            with_pid
        );
        // Nothing in the install folder changed.
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 3);
    }

    #[test]
    fn prepare_refuses_a_changed_installer_and_a_folder_without_the_helper() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = installed(tmp.path());
        let mut bad = plan(tmp.path(), &dir);
        bad.sha256 = "0".repeat(64);
        assert!(matches!(prepare(&bad, vec![]), Err(InstallError::Checksum)));
        let good = plan(tmp.path(), &dir);
        std::fs::remove_file(dir.join(HELPER)).unwrap();
        assert!(matches!(
            prepare(&good, vec![]),
            Err(InstallError::BadImage(_))
        ));
        let gone = plan(tmp.path(), &tmp.path().join("nowhere"));
        assert!(matches!(
            prepare(&gone, vec![]),
            Err(InstallError::NotWritable)
        ));
    }

    #[test]
    fn stale_backups_go_and_nothing_else() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = installed(tmp.path());
        let stale = old_path(&dir, "1.0.0");
        std::fs::create_dir_all(stale.join("sub")).unwrap();
        let other = dir.parent().unwrap().join("Other");
        std::fs::create_dir_all(&other).unwrap();
        remove_stale_backups(&dir);
        assert!(!stale.exists());
        assert!(other.exists() && dir.join(APP_EXE).exists());
    }
}
