//! Restart on a new version: when the daemon's own program file is replaced (the app updated
//! itself, or someone copied a new binary over it), the running process still runs the old
//! code. The daemon notices the new file and, once no run is active, exits so that the service
//! manager (launchd `KeepAlive`, systemd `Restart=always`) starts the new version. Nobody has to restart anything by hand.
//!
//! Windows: a running program file cannot be written or deleted, but it can be renamed. The
//! replace protocol every updater follows there (the app's updater, the PowerShell install
//! command, a person doing it by hand):
//!
//! 1. write the new program completely under another name in the same folder
//!    (`<name>.exe.new`), and check it (SHA-256);
//! 2. rename the running `<name>.exe` to `<name>.exe.old-<anything unique>` in the same folder;
//! 3. rename `<name>.exe.new` to `<name>.exe`. If that fails, rename the old file back.
//!
//! The updater never stops the daemon or its scheduled task (that would cancel the runs). The
//! daemon does the rest: it sees another file (file index, size, write time)
//! at its own path, waits until the stamp is the same at two checks and no run is active, and
//! exits; the scheduled task starts the new file within a minute ([`crate::service`]). When a
//! daemon starts it deletes the `<name>.exe.old-*` files next to it ([`clean_stale`]); one
//! that is still running (the old daemon has not exited yet) stays until the next start.
//!
//! Updating itself ([`Updater`]): a daemon installed on its own, not by the app (a Linux host,
//! whose release file is the daemon alone), asks the server for the newest release of its
//! platform on a timer. When it is newer than the running version, the daemon downloads it next
//! to its program as `<name>.new` and checks its size and SHA-256. Once no run is active it
//! puts the file in place (one rename on Unix; the steps above on Windows) and exits, and the
//! service manager starts the new version. It never replaces itself while a run is active, and
//! `self_update = false` in the config turns it off.

use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest as _, Sha256};

/// What identifies one version of the program file on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    dev: u64,
    ino: u64,
    size: u64,
    mtime: i64,
    mtime_nsec: i64,
}

impl Stamp {
    #[cfg(unix)]
    pub fn of(path: &Path) -> Option<Stamp> {
        use std::os::unix::fs::MetadataExt as _;
        let m = std::fs::metadata(path).ok()?;
        (m.is_file() && m.mode() & 0o111 != 0).then(|| Stamp {
            dev: m.dev(),
            ino: m.ino(),
            size: m.size(),
            mtime: m.mtime(),
            mtime_nsec: m.mtime_nsec(),
        })
    }

    /// Windows: the volume serial number and the file index are what the device and inode
    /// are on Unix. The times alone would not do: a file renamed into a name that was just
    /// freed gets the creation time of the file that had the name (NTFS tunneling), and a
    /// copied file keeps its write time.
    #[cfg(windows)]
    pub fn of(path: &Path) -> Option<Stamp> {
        use std::os::windows::fs::MetadataExt as _;
        use std::os::windows::io::AsRawHandle as _;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };
        let file = std::fs::File::open(path).ok()?;
        let m = file.metadata().ok()?;
        if !m.is_file() {
            return None;
        }
        // SAFETY: an all-zero BY_HANDLE_FILE_INFORMATION is a valid value to be filled in.
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: the handle is an open file and `info` is a valid place to write.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) } == 0 {
            return None;
        }
        Some(Stamp {
            dev: u64::from(info.dwVolumeSerialNumber),
            ino: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
            size: m.file_size(),
            mtime: m.last_write_time() as i64,
            mtime_nsec: 0,
        })
    }
}

/// Watches the program file. [`Watch::check`] says when a new version is ready: the file
/// changed and stayed the same between two checks (an update may still be writing it).
#[derive(Debug)]
pub struct Watch {
    path: PathBuf,
    started: Option<Stamp>,
    pending: Option<Stamp>,
}

impl Watch {
    pub fn new(path: PathBuf) -> Watch {
        let started = Stamp::of(&path);
        Watch {
            path,
            started,
            pending: None,
        }
    }

    /// The running program's file. On Windows this is also when the program files an update
    /// moved aside are deleted.
    pub fn current() -> Option<Watch> {
        let exe = std::env::current_exe().ok()?;
        #[cfg(windows)]
        clean_stale(&exe);
        Some(Watch::new(exe))
    }

    /// True when the file at the program's path is no longer the one the daemon started from.
    pub fn replaced(&self) -> bool {
        self.started.is_some() && Stamp::of(&self.path) != self.started
    }

    /// True when a new program file has been in place since the previous check.
    pub fn check(&mut self) -> bool {
        let Some(started) = &self.started else {
            return false;
        };
        let now = Stamp::of(&self.path);
        match now {
            Some(s) if &s != started => {
                let ready = self.pending.as_ref() == Some(&s);
                self.pending = Some(s);
                ready
            }
            _ => {
                self.pending = None;
                false
            }
        }
    }
}

/// What an updater puts after the program's file name when it moves the running file aside
/// (`daemon.exe` becomes `daemon.exe.old-<anything>`).
pub const STALE_MARK: &str = ".old-";

/// The files next to `exe` that an update moved aside.
pub fn stale_files(exe: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(name)) = (exe.parent(), exe.file_name()) else {
        return vec![];
    };
    let prefix = format!("{}{STALE_MARK}", name.to_string_lossy());
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .collect()
}

/// Deletes the program files an update moved aside; returns how many are gone. One that
/// cannot be deleted (it is still running) is left for the next start.
pub fn clean_stale(exe: &Path) -> usize {
    stale_files(exe)
        .iter()
        .filter(|p| std::fs::remove_file(p).is_ok())
        .count()
}

/// Puts the program file back after an update that was interrupted between its two renames
/// (a power loss): when `exe` is missing and files moved aside are there, the newest of them
/// is renamed to `exe`. Returns the file it renamed. The scheduled task runs this before it
/// starts the daemon ([`crate::service`]), from a copy of the program that updates never touch.
pub fn repair(exe: &Path) -> Option<PathBuf> {
    if exe.exists() {
        return None;
    }
    let newest = stale_files(exe).into_iter().max_by_key(|p| {
        std::fs::metadata(p)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH)
    })?;
    std::fs::rename(&newest, exe).ok().map(|()| newest)
}

// ---------------------------------------------------------------------------
// Updating itself from the server's releases
// ---------------------------------------------------------------------------

/// The version of the running daemon.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The first look for a release after the daemon starts (not at once: a daemon that keeps
/// failing at start must not download on every start), then how often.
pub const FIRST_CHECK: Duration = Duration::from_secs(10 * 60);
pub const CHECK_EVERY: Duration = Duration::from_secs(60 * 60);
/// A download of the program may take this long.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// The release platform whose file is this daemon alone. None where the daemon comes inside
/// the app (macOS, Windows): the app updates it.
pub fn release_platform() -> Option<&'static str> {
    cfg!(all(target_os = "linux", target_arch = "x86_64"))
        .then_some(chat_protocol::updates::PLATFORM_LINUX_X86_64)
}

/// True when `latest` is a release newer than `own`. A prerelease is older than its release.
pub fn is_newer(latest: &str, own: &str) -> bool {
    let Some(l) = chat_protocol::updates::parse_release_version(latest) else {
        return false;
    };
    let (core, pre) = match own.split_once('-') {
        Some((core, _)) => (core, true),
        None => (own, false),
    };
    let Some(o) = chat_protocol::updates::parse_release_version(core) else {
        return false;
    };
    l > o || (l == o && pre)
}

/// Why the daemon does not update itself, or None when it does. `service` says whether a
/// service manager started it: a daemon started by hand would just stop after an update.
pub fn off_reason(enabled: bool, platform: Option<&str>, service: bool) -> Option<&'static str> {
    if !enabled {
        Some("self_update = false in the config")
    } else if platform.is_none() {
        Some("the app updates the daemon on this system")
    } else if !service {
        Some("not started by systemd (a daemon started by hand would not come back)")
    } else {
        None
    }
}

/// systemd sets `INVOCATION_ID` for every unit it starts (`daemon install-service`).
fn under_service_manager() -> bool {
    std::env::var_os("INVOCATION_ID").is_some_and(|v| !v.is_empty())
}

/// Where a downloaded program waits until it is put in place: `<name>.new` next to `exe`.
pub fn staged_path(exe: &Path) -> Option<PathBuf> {
    let name = exe.file_name()?;
    Some(exe.with_file_name(format!("{}.new", name.to_string_lossy())))
}

/// A downloaded and checked program, ready to be put in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staged {
    pub version: String,
    pub file: PathBuf,
}

/// Asks the server for the newest release of `platform`. When it is newer than `own`, downloads
/// it to [`staged_path`], checks its size and SHA-256 against the answer and makes it
/// executable. Ok(None): nothing newer (or no release at all). A failed download leaves no file.
pub async fn fetch(
    client: &reqwest::Client,
    server_url: &str,
    token: &str,
    platform: &str,
    own: &str,
    exe: &Path,
) -> Result<Option<Staged>, String> {
    let url = format!(
        "{}/v1/machine/updates/latest?platform={platform}",
        server_url.trim_end_matches('/')
    );
    let res = client
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("cannot reach the server: {}", e.without_url()))?;
    match res.status().as_u16() {
        404 => return Ok(None),
        200..=299 => {}
        s => return Err(format!("the update check answered HTTP {s}")),
    }
    let body = res
        .bytes()
        .await
        .map_err(|e| format!("cannot reach the server: {}", e.without_url()))?;
    let info: chat_protocol::updates::UpdateInfo = serde_json::from_slice(&body)
        .map_err(|e| format!("the update check answered no release: {e}"))?;
    info.validate().map_err(|e| format!("bad release: {e}"))?;
    if info.platform() != platform {
        return Err(format!(
            "the server answered with a release for {}",
            info.platform()
        ));
    }
    if !is_newer(&info.version, own) {
        return Ok(None);
    }
    // The machine token goes only to the configured server.
    if !crate::upload::same_origin(&info.download_url, server_url) {
        return Err("the release is not on the configured server".into());
    }
    let file = staged_path(exe).ok_or("the daemon's program has no file name")?;
    match download(client, &info, token, &file).await {
        Ok(()) => Ok(Some(Staged {
            version: info.version,
            file,
        })),
        Err(e) => {
            let _ = std::fs::remove_file(&file);
            Err(e)
        }
    }
}

async fn download(
    client: &reqwest::Client,
    info: &chat_protocol::updates::UpdateInfo,
    token: &str,
    file: &Path,
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt as _;
    // First the file: a folder the daemon cannot write fails before the download.
    let mut out = tokio::fs::File::create(file)
        .await
        .map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    let mut res = client
        .get(&info.download_url)
        .bearer_auth(token)
        .timeout(DOWNLOAD_TIMEOUT)
        .send()
        .await
        .map_err(|e| format!("download failed: {}", e.without_url()))?;
    if !res.status().is_success() {
        return Err(format!("download failed: HTTP {}", res.status().as_u16()));
    }
    let mut hash = Sha256::new();
    let mut got = 0u64;
    while let Some(chunk) = res
        .chunk()
        .await
        .map_err(|e| format!("download failed: {}", e.without_url()))?
    {
        got += chunk.len() as u64;
        if got > info.size {
            return Err("the download is larger than the release".into());
        }
        hash.update(&chunk);
        out.write_all(&chunk)
            .await
            .map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    }
    out.sync_all()
        .await
        .map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    drop(out);
    if got != info.size {
        return Err(format!(
            "the download has {got} bytes, the release {}",
            info.size
        ));
    }
    if crate::workspace::hex(&hash.finalize()) != info.sha256 {
        return Err("the download does not match the release's SHA-256".into());
    }
    crate::sys::set_mode(file, 0o755).map_err(|e| format!("chmod {}: {e}", file.display()))
}

/// Puts the checked program `staged` in place of `exe`. Unix: one rename over the running file
/// (the running process keeps its old file). Windows: the replace protocol at the top.
pub fn install(staged: &Path, exe: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        let aside = PathBuf::from(format!(
            "{}{STALE_MARK}{}",
            exe.display(),
            crate::proto::now_ms()
        ));
        std::fs::rename(exe, &aside).map_err(|e| format!("cannot move the program aside: {e}"))?;
        if let Err(e) = std::fs::rename(staged, exe) {
            let _ = std::fs::rename(&aside, exe);
            return Err(format!("cannot put the new program in place: {e}"));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    std::fs::rename(staged, exe).map_err(|e| format!("cannot put the new program in place: {e}"))
}

/// What a check found: a program ready to put in place, nothing newer, or a problem.
pub type Checked = Result<Option<Staged>, String>;

/// The daemon's self-update: when to look for a release, the program that waits for the
/// daemon to be idle, and putting it in place.
#[derive(Debug)]
pub struct Updater {
    platform: &'static str,
    exe: PathBuf,
    checking: bool,
    staged: Option<Staged>,
}

impl Updater {
    /// The updater of this daemon, or None (with the reason) when it does not update itself.
    pub fn new(enabled: bool) -> Result<Updater, &'static str> {
        let platform = release_platform();
        if let Some(why) = off_reason(enabled, platform, under_service_manager()) {
            return Err(why);
        }
        let exe = std::env::current_exe().map_err(|_| "the daemon's program file is unknown")?;
        Ok(Updater::at(platform.unwrap_or_default(), exe))
    }

    pub fn at(platform: &'static str, exe: PathBuf) -> Updater {
        // A program left from an update that never went in.
        if let Some(old) = staged_path(&exe) {
            let _ = std::fs::remove_file(old);
        }
        Updater {
            platform,
            exe,
            checking: false,
            staged: None,
        }
    }

    /// True when it is time to ask: no check running and nothing waiting to go in.
    pub fn wants_check(&self) -> bool {
        !self.checking && self.staged.is_none()
    }

    /// Starts a check; its result comes back through `done`.
    pub fn check(
        &mut self,
        server_url: String,
        token: std::sync::Arc<String>,
        done: tokio::sync::mpsc::UnboundedSender<Checked>,
    ) {
        if !self.wants_check() {
            return;
        }
        self.checking = true;
        let (platform, exe) = (self.platform, self.exe.clone());
        tokio::spawn(async move {
            let r = match reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
            {
                Ok(client) => fetch(&client, &server_url, &token, platform, VERSION, &exe).await,
                Err(e) => Err(e.to_string()),
            };
            let _ = done.send(r);
        });
    }

    /// The result of a check. Returns what to log.
    pub fn checked(&mut self, r: Checked) -> Option<String> {
        self.checking = false;
        match r {
            Ok(Some(s)) => {
                let line = format!(
                    "version {} of the daemon is downloaded; it goes in once no run is active",
                    s.version
                );
                self.staged = Some(s);
                Some(line)
            }
            Ok(None) => None,
            Err(e) => Some(format!("update check: {e}")),
        }
    }

    /// Call only when no run is active. Puts a waiting program in place; true when the daemon
    /// should now exit for the service manager to start it. `watch` is the daemon's own file:
    /// when somebody replaced it meanwhile, theirs wins and the download is dropped.
    pub fn install_when_idle(&mut self, watch: Option<&Watch>) -> Result<bool, String> {
        let Some(s) = self.staged.take() else {
            return Ok(false);
        };
        if watch.is_some_and(Watch::replaced) {
            let _ = std::fs::remove_file(&s.file);
            return Err(
                "the program file was replaced by someone else; dropping the download".into(),
            );
        }
        match install(&s.file, &self.exe) {
            Ok(()) => Ok(true),
            Err(e) => {
                let _ = std::fs::remove_file(&s.file);
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod update_tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    const PLATFORM: &str = "linux-x86_64";

    #[test]
    fn only_a_newer_release_is_an_update() {
        assert!(is_newer("1.2.8", "1.2.7"));
        assert!(is_newer("1.10.0", "1.9.9"));
        assert!(is_newer("2.0.0", "1.99.99"));
        assert!(!is_newer("1.2.7", "1.2.7"));
        assert!(
            !is_newer("1.2.6", "1.2.7"),
            "never back to an older version"
        );
        // A prerelease is older than its release, newer than the one before.
        assert!(is_newer("1.3.0", "1.3.0-rc.1"));
        assert!(!is_newer("1.2.9", "1.3.0-rc.1"));
        // What is not a release version never is an update.
        assert!(!is_newer("1.3", "1.2.7"));
        assert!(!is_newer("1.03.0", "1.2.7"));
        assert!(!is_newer("1.3.0-rc.2", "1.2.7"));
        assert!(!is_newer("1.3.0", "dev"));
    }

    #[test]
    fn it_updates_itself_only_when_on_installed_alone_under_systemd() {
        assert_eq!(off_reason(true, Some(PLATFORM), true), None);
        assert!(
            off_reason(false, Some(PLATFORM), true)
                .unwrap()
                .contains("self_update")
        );
        assert!(off_reason(true, None, true).unwrap().contains("app"));
        assert!(
            off_reason(true, Some(PLATFORM), false)
                .unwrap()
                .contains("systemd")
        );
        assert_eq!(
            release_platform().is_some(),
            cfg!(all(target_os = "linux", target_arch = "x86_64"))
        );
    }

    #[test]
    fn the_download_waits_next_to_the_program() {
        assert_eq!(
            staged_path(Path::new("/opt/x/team-chat-daemon")),
            Some(PathBuf::from("/opt/x/team-chat-daemon.new"))
        );
    }

    struct Release {
        version: &'static str,
        platform: &'static str,
        body: Vec<u8>,
        /// The SHA-256 the server announces; None = the body's.
        sha256: Option<String>,
        /// The size the server announces; None = the body's.
        size: Option<u64>,
        /// Another origin for `downloadUrl`.
        elsewhere: bool,
    }

    fn release(version: &'static str, body: &[u8]) -> Release {
        Release {
            version,
            platform: PLATFORM,
            body: body.to_vec(),
            sha256: None,
            size: None,
            elsewhere: false,
        }
    }

    /// A stand-in server: `latest` answers `release` (404 when None), `download` its body.
    /// Records each request's first line and whether it carried the machine token.
    async fn server(r: Option<Release>) -> (String, Arc<Mutex<Vec<String>>>) {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(vec![]));
        let (seen2, base) = (seen.clone(), url.clone());
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else {
                    return;
                };
                let mut head = vec![];
                let mut b = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    if s.read(&mut b).await.unwrap_or(0) == 0 {
                        break;
                    }
                    head.push(b[0]);
                }
                let head = String::from_utf8_lossy(&head).to_string();
                let line = head.lines().next().unwrap_or_default().to_string();
                let authed = head
                    .to_ascii_lowercase()
                    .contains("authorization: bearer tm_test");
                seen2.lock().unwrap().push(format!("{line} auth={authed}"));
                let (status, body): (&str, Vec<u8>) = match &r {
                    None => (
                        "404 Not Found",
                        br#"{"error":{"code":"not_found"}}"#.to_vec(),
                    ),
                    Some(r) if line.contains("/latest?platform=") => {
                        let origin = if r.elsewhere {
                            "http://198.51.100.1:9".to_string()
                        } else {
                            base.clone()
                        };
                        let info = serde_json::json!({
                            "version": r.version,
                            "notes": "",
                            "size": r.size.unwrap_or(r.body.len() as u64),
                            "sha256": r.sha256.clone().unwrap_or_else(|| crate::workspace::hex(&Sha256::digest(&r.body))),
                            "publishedAt": 1,
                            "downloadUrl": format!("{origin}/v1/machine/updates/{}/download?platform={}", r.version, r.platform),
                            "platform": r.platform,
                        });
                        ("200 OK", info.to_string().into_bytes())
                    }
                    Some(r) => ("200 OK", r.body.clone()),
                };
                let out = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(out.as_bytes()).await;
                let _ = s.write_all(&body).await;
                let _ = s.shutdown().await;
            }
        });
        (url, seen)
    }

    async fn try_fetch(
        r: Option<Release>,
        own: &str,
    ) -> (Checked, PathBuf, Vec<String>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("team-chat-daemon");
        std::fs::write(&exe, "running").unwrap();
        let (url, seen) = server(r).await;
        let client = reqwest::Client::new();
        let got = fetch(&client, &url, "tm_test", PLATFORM, own, &exe).await;
        let seen = seen.lock().unwrap().clone();
        (got, exe, seen, dir)
    }

    #[tokio::test]
    async fn a_newer_release_is_downloaded_and_checked() {
        let (got, exe, seen, _dir) =
            try_fetch(Some(release("1.2.8", b"\x7fELF new")), "1.2.7").await;
        let staged = got.unwrap().unwrap();
        assert_eq!(staged.version, "1.2.8");
        assert_eq!(staged.file, staged_path(&exe).unwrap());
        assert_eq!(std::fs::read(&staged.file).unwrap(), b"\x7fELF new");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&staged.file)
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o755);
        }
        assert_eq!(std::fs::read(&exe).unwrap(), b"running", "not in place yet");
        assert_eq!(
            seen,
            [
                "GET /v1/machine/updates/latest?platform=linux-x86_64 HTTP/1.1 auth=true",
                "GET /v1/machine/updates/1.2.8/download?platform=linux-x86_64 HTTP/1.1 auth=true",
            ]
        );
    }

    #[tokio::test]
    async fn nothing_newer_downloads_nothing() {
        let (got, exe, seen, _dir) = try_fetch(Some(release("1.2.7", b"same")), "1.2.7").await;
        assert_eq!(got, Ok(None));
        assert_eq!(seen.len(), 1, "only the check: {seen:?}");
        assert!(!staged_path(&exe).unwrap().exists());
        let (got, _, _, _dir) = try_fetch(None, "1.2.7").await;
        assert_eq!(got, Ok(None), "no release at all is not an error");
    }

    #[tokio::test]
    async fn a_download_that_does_not_match_leaves_no_file() {
        let mut bad_sha = release("1.2.8", b"\x7fELF tampered");
        bad_sha.sha256 = Some("0".repeat(64));
        let mut short = release("1.2.8", b"\x7fELF cut");
        short.size = Some(1000);
        let mut long = release("1.2.8", b"\x7fELF longer than said");
        long.size = Some(4);
        for (r, why) in [(bad_sha, "SHA-256"), (short, "bytes"), (long, "larger")] {
            let (got, exe, _, _dir) = try_fetch(Some(r), "1.2.7").await;
            let e = got.unwrap_err();
            assert!(e.contains(why), "{e}");
            assert!(!staged_path(&exe).unwrap().exists(), "{why}");
        }
    }

    #[tokio::test]
    async fn the_token_goes_only_to_the_server_and_only_its_platform_is_taken() {
        let mut away = release("1.2.8", b"x");
        away.elsewhere = true;
        let (got, exe, seen, _dir) = try_fetch(Some(away), "1.2.7").await;
        assert!(got.unwrap_err().contains("configured server"));
        assert_eq!(seen.len(), 1);
        assert!(!staged_path(&exe).unwrap().exists());
        let mut mac = release("1.2.8", b"x");
        mac.platform = "macos";
        let (got, _, _, _dir) = try_fetch(Some(mac), "1.2.7").await;
        assert!(got.unwrap_err().contains("macos"));
    }

    #[tokio::test]
    async fn a_folder_the_daemon_cannot_write_fails_before_the_download() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("missing-folder").join("team-chat-daemon");
        let (url, seen) = server(Some(release("1.2.8", b"x"))).await;
        let got = fetch(
            &reqwest::Client::new(),
            &url,
            "tm_test",
            PLATFORM,
            "1.2.7",
            &exe,
        )
        .await;
        assert!(got.unwrap_err().contains("cannot write"));
        assert_eq!(seen.lock().unwrap().len(), 1, "no download");
    }

    fn staged_in(dir: &Path, version: &str, body: &str) -> (Updater, PathBuf) {
        let exe = dir.join("team-chat-daemon");
        std::fs::write(&exe, "old").unwrap();
        crate::sys::set_mode(&exe, 0o755).unwrap();
        let mut u = Updater::at(PLATFORM, exe.clone());
        let file = staged_path(&exe).unwrap();
        std::fs::write(&file, body).unwrap();
        crate::sys::set_mode(&file, 0o755).unwrap();
        let line = u.checked(Ok(Some(Staged {
            version: version.into(),
            file,
        })));
        assert!(line.unwrap().contains(version));
        (u, exe)
    }

    #[test]
    fn the_downloaded_program_goes_in_and_the_daemon_restarts() {
        let dir = tempfile::tempdir().unwrap();
        let (mut u, exe) = staged_in(dir.path(), "1.2.8", "new");
        let mut watch = Watch::new(exe.clone());
        assert!(!u.wants_check(), "nothing more to fetch while one waits");
        assert_eq!(u.install_when_idle(Some(&watch)), Ok(true));
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new");
        assert!(!staged_path(&exe).unwrap().exists());
        assert_eq!(u.install_when_idle(Some(&watch)), Ok(false), "once");
        assert!(u.wants_check());
        // The restart that follows is the same one a replaced file causes.
        assert!(watch.replaced());
        assert!(!watch.check());
        assert!(watch.check());
    }

    #[test]
    fn a_program_someone_else_put_in_place_wins() {
        let dir = tempfile::tempdir().unwrap();
        let (mut u, exe) = staged_in(dir.path(), "1.2.8", "downloaded");
        let watch = Watch::new(exe.clone());
        let theirs = dir.path().join("theirs");
        std::fs::write(&theirs, "theirs, a bit longer").unwrap();
        crate::sys::set_mode(&theirs, 0o755).unwrap();
        std::fs::rename(&theirs, &exe).unwrap();
        let e = u.install_when_idle(Some(&watch)).unwrap_err();
        assert!(e.contains("replaced"), "{e}");
        assert_eq!(
            std::fs::read_to_string(&exe).unwrap(),
            "theirs, a bit longer"
        );
        assert!(!staged_path(&exe).unwrap().exists());
    }

    #[test]
    fn a_check_that_failed_is_logged_and_tried_again_later() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("team-chat-daemon");
        // A program left by an update that never went in is removed at start.
        std::fs::write(staged_path(&exe).unwrap(), "left").unwrap();
        let mut u = Updater::at(PLATFORM, exe.clone());
        assert!(!staged_path(&exe).unwrap().exists());
        assert!(u.wants_check());
        assert_eq!(u.checked(Ok(None)), None);
        assert!(
            u.checked(Err("HTTP 500".into()))
                .unwrap()
                .contains("HTTP 500")
        );
        assert!(u.wants_check());
        assert_eq!(u.install_when_idle(None), Ok(false));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_program_comes_back_from_the_file_moved_aside() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("x-daemon.exe");
        let aside = dir.path().join("x-daemon.exe.old-7");
        // Nothing to repair: the program is there, or nothing was moved aside.
        assert_eq!(repair(&exe), None);
        std::fs::write(&exe, "v2").unwrap();
        std::fs::write(&aside, "v1").unwrap();
        assert_eq!(repair(&exe), None);
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "v2");
        // The update stopped after its first rename.
        std::fs::remove_file(&exe).unwrap();
        assert_eq!(repair(&exe), Some(aside.clone()));
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "v1");
        assert!(!aside.exists());
    }

    #[test]
    fn only_the_files_an_update_moved_aside_are_stale() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("x-daemon.exe");
        for name in [
            "x-daemon.exe",
            "x-daemon.exe.old-1",
            "x-daemon.exe.old-20261110",
            "x-daemon.exe.new",
            "x-daemon.exe.older",
            "y-daemon.exe.old-1",
            "x-daemon.toml",
        ] {
            std::fs::write(dir.path().join(name), "x").unwrap();
        }
        std::fs::create_dir(dir.path().join("x-daemon.exe.old-dir")).unwrap();
        assert_eq!(stale_files(&exe).len(), 2);
        assert_eq!(clean_stale(&exe), 2);
        let mut left: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "x-daemon.exe",
                "x-daemon.exe.new",
                "x-daemon.exe.old-dir",
                "x-daemon.exe.older",
                "x-daemon.toml",
                "y-daemon.exe.old-1",
            ]
        );
        assert_eq!(clean_stale(&exe), 0);
    }

    fn write_exe(path: &Path, body: &str) {
        let tmp = path.with_extension("new");
        std::fs::write(&tmp, body).unwrap();
        crate::sys::set_mode(&tmp, 0o755).unwrap();
        // Replaced the way an update replaces it: a new file moved over the old one.
        std::fs::rename(&tmp, path).unwrap();
    }

    #[test]
    fn a_replaced_program_is_ready_after_it_stays_the_same() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("daemon");
        write_exe(&exe, "v1");
        let mut w = Watch::new(exe.clone());
        assert!(!w.check(), "unchanged");
        write_exe(&exe, "v2 longer");
        assert!(!w.check(), "just changed: it may still be written");
        assert!(w.check(), "the same new file one check later");
        assert!(w.check(), "stays ready until the daemon restarts");
        write_exe(&exe, "v3 even longer");
        assert!(!w.check(), "changed again: wait once more");
        assert!(w.check());
    }

    #[test]
    fn a_missing_or_not_executable_file_is_not_a_new_version() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("daemon");
        write_exe(&exe, "v1");
        let mut w = Watch::new(exe.clone());
        std::fs::remove_file(&exe).unwrap();
        assert!(!w.check());
        assert!(!w.check());
        // Not executable (a mode Windows does not have: any file there is a version).
        #[cfg(unix)]
        {
            std::fs::write(&exe, "half written").unwrap();
            assert!(!w.check());
            assert!(!w.check());
        }
        // No file at start: nothing to watch.
        let mut none = Watch::new(dir.path().join("missing"));
        assert!(!none.check());
    }
}
