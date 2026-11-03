//! Replaces the running `.app` bundle with the one inside a downloaded DMG, and
//! relaunches. Blocking: run it off the UI thread.
//!
//! Steps ([`install`]): check the DMG's SHA-256 again, check that the folder of the
//! running bundle is writable, mount the DMG read-only and hidden from Finder
//! (`hdiutil attach -nobrowse`), check the new bundle's id and version, copy it next to
//! the running bundle under a hidden temporary name (`ditto`), check that the copy
//! satisfies the running bundle's designated requirement (when that names a signing
//! certificate, not only a cdhash), detach, then swap the
//! two names with `rename` (same folder, so each rename is atomic). The old bundle stays
//! under a hidden name until the new one starts: a marker file names it, and
//! [`finish_on_launch`] in the new process removes it. [`relaunch`] starts a small shell
//! that waits for this process to exit and then opens the new bundle.

use std::{
    ffi::OsString,
    fmt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use serde::{Deserialize, Serialize};

use super::version;

/// What an install needs.
#[derive(Clone, Debug)]
pub struct Plan {
    /// The running `.app` bundle.
    pub bundle: PathBuf,
    /// The verified DMG.
    pub dmg: PathBuf,
    /// Expected version of the new bundle.
    pub version: String,
    /// Expected SHA-256 of the DMG (hex).
    pub sha256: String,
    /// Expected `CFBundleIdentifier`.
    pub bundle_id: String,
    /// Expected name of the bundle in the DMG, such as `Name.app`. When missing, the only
    /// `.app` of the volume is used.
    pub app_name: Option<String>,
    /// Where the marker for [`finish_on_launch`] goes.
    pub marker: PathBuf,
}

#[derive(Debug)]
pub enum InstallError {
    /// The DMG changed or is damaged.
    Checksum,
    /// The bundle's folder is not writable by this user (for example `/Applications`
    /// owned by root, or the app runs from the read-only DMG).
    NotWritable,
    /// The DMG has no usable bundle, or its id or version is wrong.
    BadImage(String),
    /// A tool or file operation failed.
    Io(String),
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstallError::Checksum => f.write_str("checksum mismatch"),
            InstallError::NotWritable => f.write_str("bundle folder not writable"),
            InstallError::BadImage(m) => write!(f, "bad image: {m}"),
            InstallError::Io(m) => write!(f, "io: {m}"),
        }
    }
}

impl From<std::io::Error> for InstallError {
    fn from(e: std::io::Error) -> Self {
        InstallError::Io(e.to_string())
    }
}

/// The marker the new process reads on its first launch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    pub bundle: PathBuf,
    pub old: PathBuf,
    pub version: String,
}

/// The `.app` bundle that holds `exe` (`X.app/Contents/MacOS/<exe>`), if any.
pub fn bundle_from_exe(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let is_app = bundle.extension().is_some_and(|e| e == "app");
    (macos.file_name()? == "MacOS" && contents.file_name()? == "Contents" && is_app)
        .then(|| bundle.to_path_buf())
}

/// True when this user can create and rename entries next to `bundle`.
pub fn can_replace(bundle: &Path) -> bool {
    let Some(parent) = bundle.parent() else {
        return false;
    };
    let probe = parent.join(format!(".update-probe-{}", std::process::id()));
    match std::fs::create_dir(&probe) {
        Ok(()) => {
            let _ = std::fs::remove_dir(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Hidden sibling path of `bundle`: `.<Name>.app.<tag>`.
fn sibling(bundle: &Path, tag: &str) -> PathBuf {
    let name = bundle
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    bundle.with_file_name(format!(".{name}.{tag}"))
}

/// The staging path of a new version next to `bundle`.
pub fn staging_path(bundle: &Path, version: &str) -> PathBuf {
    sibling(bundle, &format!("update-{version}"))
}

/// Where the old bundle waits until the new one starts.
pub fn old_path(bundle: &Path, version: &str) -> PathBuf {
    sibling(bundle, &format!("old-{version}"))
}

/// Runs the whole install. On success the new bundle is at `plan.bundle`, the old one at
/// the returned marker's `old`, and the marker is written.
pub fn install(plan: &Plan) -> Result<Marker, InstallError> {
    if !version::checksum_matches(&plan.dmg, &plan.sha256)? {
        return Err(InstallError::Checksum);
    }
    if !plan.bundle.is_dir() || !can_replace(&plan.bundle) {
        return Err(InstallError::NotWritable);
    }
    // Read before anything changes: the update must come from the same signer.
    let required = designated_requirement(&plan.bundle);
    let staged = staging_path(&plan.bundle, &plan.version);
    remove_all(&staged)?;
    {
        let mount = Mount::attach(&plan.dmg)?;
        let app = find_app(&mount.point, plan.app_name.as_deref())?;
        check_bundle(&app, &plan.bundle_id, &plan.version)?;
        if let Err(e) = copy_bundle(&app, &staged) {
            let _ = remove_all(&staged);
            return Err(e);
        }
        mount.detach();
    }
    let checked =
        check_bundle(&staged, &plan.bundle_id, &plan.version).and_then(|()| match &required {
            Some(req) => check_signature(&staged, req),
            None => Ok(()),
        });
    if let Err(e) = checked {
        let _ = remove_all(&staged);
        return Err(e);
    }
    let old = old_path(&plan.bundle, &plan.version);
    let marker = Marker {
        bundle: plan.bundle.clone(),
        old: old.clone(),
        version: plan.version.clone(),
    };
    if let Err(e) = swap(&plan.bundle, &staged, &old) {
        let _ = remove_all(&staged);
        return Err(e);
    }
    if let Err(e) = write_marker(&plan.marker, &marker) {
        // Without the marker the old bundle would stay forever; undo the swap.
        let _ = undo_swap(&plan.bundle, &old);
        return Err(e);
    }
    Ok(marker)
}

/// Moves `bundle` to `old` and `staged` to `bundle`. Puts `bundle` back when the second
/// rename fails.
pub fn swap(bundle: &Path, staged: &Path, old: &Path) -> Result<(), InstallError> {
    remove_all(old)?;
    std::fs::rename(bundle, old)?;
    if let Err(e) = std::fs::rename(staged, bundle) {
        let _ = std::fs::rename(old, bundle);
        return Err(e.into());
    }
    Ok(())
}

fn undo_swap(bundle: &Path, old: &Path) -> Result<(), InstallError> {
    remove_all(bundle)?;
    std::fs::rename(old, bundle)?;
    Ok(())
}

/// Copies a bundle with `ditto` (keeps symlinks, modes and extended attributes) and drops
/// a quarantine flag if one came along.
fn copy_bundle(from: &Path, to: &Path) -> Result<(), InstallError> {
    run(Command::new("/usr/bin/ditto").arg(from).arg(to))?;
    let _ = Command::new("/usr/bin/xattr")
        .args(["-dr", "com.apple.quarantine"])
        .arg(to)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    Ok(())
}

/// The bundle to install from a mounted volume.
pub fn find_app(volume: &Path, preferred: Option<&str>) -> Result<PathBuf, InstallError> {
    if let Some(name) = preferred {
        let p = volume.join(name);
        if p.is_dir() {
            return Ok(p);
        }
    }
    let apps: Vec<PathBuf> = std::fs::read_dir(volume)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "app") && p.is_dir())
        .filter(|p| !std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink()))
        .collect();
    match apps.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err(InstallError::BadImage("no .app in the image".into())),
        _ => Err(InstallError::BadImage("several .app in the image".into())),
    }
}

/// Checks `CFBundleIdentifier`, `CFBundleShortVersionString` and the main executable.
pub fn check_bundle(app: &Path, bundle_id: &str, version: &str) -> Result<(), InstallError> {
    let plist = app.join("Contents/Info.plist");
    let id = plist_value(&plist, "CFBundleIdentifier")?;
    if id != bundle_id {
        return Err(InstallError::BadImage(format!("bundle id {id}")));
    }
    let v = plist_value(&plist, "CFBundleShortVersionString")?;
    if v != version {
        return Err(InstallError::BadImage(format!("bundle version {v}")));
    }
    let exe = plist_value(&plist, "CFBundleExecutable")?;
    if exe.contains('/') || !app.join("Contents/MacOS").join(&exe).is_file() {
        return Err(InstallError::BadImage("missing executable".into()));
    }
    Ok(())
}

/// The designated requirement of a signed bundle, when it is stable across builds: `None`
/// for an unsigned bundle and for one whose requirement is only its cdhash (an ad-hoc
/// signature, which every build changes).
pub fn designated_requirement(bundle: &Path) -> Option<String> {
    let out = Command::new("/usr/bin/codesign")
        .args(["-d", "-r-"])
        .arg(bundle)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    // codesign prints the requirement on stdout, the "Executable=" line on stderr.
    let text = String::from_utf8_lossy(&out.stdout);
    stable_requirement(&text)
}

/// Picks the `designated => <req>` line of `codesign -d -r-` output; `None` when there is
/// none or it is a bare cdhash.
pub fn stable_requirement(codesign_output: &str) -> Option<String> {
    let line = codesign_output
        .lines()
        .map(str::trim)
        .find_map(|l| l.trim_start_matches("# ").strip_prefix("designated => "))?
        .trim();
    (!line.is_empty() && !line.starts_with("cdhash ")).then(|| line.to_string())
}

/// Checks that `app` has a valid signature that satisfies `requirement`.
pub fn check_signature(app: &Path, requirement: &str) -> Result<(), InstallError> {
    let out = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(format!("-R={requirement}"))
        .arg(app)
        .stdin(Stdio::null())
        .output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(InstallError::BadImage(
            "signature does not match the running app".into(),
        ))
    }
}

fn plist_value(plist: &Path, key: &str) -> Result<String, InstallError> {
    let out = Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(plist)
        .stdin(Stdio::null())
        .output()?;
    if !out.status.success() {
        return Err(InstallError::BadImage(format!("Info.plist has no {key}")));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// A mounted DMG. Dropping it detaches the volume.
pub struct Mount {
    pub point: PathBuf,
    attached: bool,
}

impl Mount {
    /// Mounts `dmg` read-only at a new private folder, hidden from Finder, without
    /// opening a window.
    pub fn attach(dmg: &Path) -> Result<Mount, InstallError> {
        let point =
            std::env::temp_dir().join(format!("update-mount-{}-{}", std::process::id(), unique()));
        std::fs::create_dir_all(&point)?;
        let mut last = String::new();
        // hdiutil sometimes fails with "resource busy" while another image mounts.
        for attempt in 0..3 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_secs(1));
            }
            let out = Command::new("/usr/bin/hdiutil")
                .args([
                    "attach",
                    "-nobrowse",
                    "-readonly",
                    "-noautoopen",
                    "-noverify",
                ])
                .arg("-mountpoint")
                .arg(&point)
                .arg(dmg)
                .stdin(Stdio::null())
                .output()?;
            if out.status.success() {
                return Ok(Mount {
                    point,
                    attached: true,
                });
            }
            last = String::from_utf8_lossy(&out.stderr).trim().to_string();
        }
        let _ = std::fs::remove_dir(&point);
        Err(InstallError::BadImage(format!("hdiutil attach: {last}")))
    }

    pub fn detach(mut self) {
        self.detach_inner();
    }

    fn detach_inner(&mut self) {
        if !self.attached {
            return;
        }
        self.attached = false;
        for attempt in 0..5 {
            let force = attempt >= 2;
            let mut cmd = Command::new("/usr/bin/hdiutil");
            cmd.arg("detach").arg(&self.point);
            if force {
                cmd.arg("-force");
            }
            let ok = cmd
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if ok {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        let _ = std::fs::remove_dir(&self.point);
    }
}

impl Drop for Mount {
    fn drop(&mut self) {
        self.detach_inner();
    }
}

fn unique() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

fn run(cmd: &mut Command) -> Result<(), InstallError> {
    let out = cmd.stdin(Stdio::null()).output()?;
    if out.status.success() {
        Ok(())
    } else {
        Err(InstallError::Io(format!(
            "{:?}: {}",
            cmd.get_program(),
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

/// Removes a file or a folder tree; a missing path is fine. Never follows a symlink.
pub fn remove_all(path: &Path) -> Result<(), InstallError> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(path)?,
        Ok(_) => std::fs::remove_file(path)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

fn write_marker(path: &Path, marker: &Marker) -> Result<(), InstallError> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let tmp = path.with_extension("tmp");
    let json = serde_json::to_vec_pretty(marker).map_err(|e| InstallError::Io(e.to_string()))?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// What [`finish_on_launch`] did.
#[derive(Debug, PartialEq, Eq)]
pub enum Finish {
    NoMarker,
    /// This process is the new version: the old bundle is gone.
    RemovedOld,
    /// The marker is for another bundle or version; left alone.
    NotForMe,
}

/// Runs in the new process at start: when the marker names this bundle and version,
/// removes the old bundle and the marker. Also removes staging folders left by an
/// install that stopped half way.
pub fn finish_on_launch(marker_path: &Path, bundle: Option<&Path>, current: &str) -> Finish {
    if let Some(bundle) = bundle {
        remove_stale_staging(bundle);
    }
    let Ok(bytes) = std::fs::read(marker_path) else {
        return Finish::NoMarker;
    };
    let Ok(marker) = serde_json::from_slice::<Marker>(&bytes) else {
        let _ = std::fs::remove_file(marker_path);
        return Finish::NoMarker;
    };
    let same_bundle = bundle.is_some_and(|b| same_path(b, &marker.bundle));
    if !same_bundle || marker.version != current {
        return Finish::NotForMe;
    }
    // Only ever remove the hidden sibling the updater made.
    if marker.old == old_path(&marker.bundle, &marker.version)
        && let Err(e) = remove_all(&marker.old)
    {
        tracing::warn!(error = %e, "cannot remove the old bundle");
    }
    let _ = std::fs::remove_file(marker_path);
    Finish::RemovedOld
}

fn same_path(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    canon(a) == canon(b)
}

fn remove_stale_staging(bundle: &Path) {
    let (Some(parent), Some(name)) = (bundle.parent(), bundle.file_name()) else {
        return;
    };
    let prefix = format!(".{}.update-", name.to_string_lossy());
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = remove_all(&entry.path());
        }
    }
}

/// How to start the new version.
#[derive(Clone, Debug)]
pub enum Launch {
    /// `open <bundle>` through Launch Services (the app).
    Open(PathBuf),
    /// Run a program directly (tests).
    Exec(Vec<OsString>),
}

/// Starts a detached shell that waits (at most 60 s) for `pid` to exit, then launches.
/// The caller quits right after.
pub fn relaunch(pid: u32, launch: &Launch) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt as _;
    let argv: Vec<OsString> = match launch {
        Launch::Open(bundle) => vec!["/usr/bin/open".into(), bundle.into()],
        Launch::Exec(argv) => argv.clone(),
    };
    Command::new("/bin/sh")
        .arg("-c")
        .arg(
            "i=0; while kill -0 \"$0\" 2>/dev/null && [ $i -lt 300 ]; do sleep 0.2; i=$((i+1)); done; exec \"$@\"",
        )
        .arg(pid.to_string())
        .args(argv)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // Its own process group: it survives the app quitting.
        .process_group(0)
        .spawn()
        .map(|_| ())
}

#[cfg(test)]
mod tests;
