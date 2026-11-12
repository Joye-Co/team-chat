//! A repository's caches on a computer that runs each task in a microVM (design-vm-state §3.3).
//!
//! `<work_root>/caches/<key>/` holds the dependency caches (cargo, sccache, npm, pnpm, pip, uv,
//! go) and the user-level tools (rustup toolchains, `cargo install`, bun, `npm -g`, pipx, uv
//! tools) of one repository, shared by all members and topics that work on it. It is shared
//! with the run's VM at the same path, and the CLI finds it through [`guest_vars`].
//!
//! A run holds a shared flock on `<C>/.lock` from start to end; cleanup and `daemon caches
//! clear` take it exclusively without waiting and leave a cache that is in use alone.

use crate::proto::{RepoSpec, now_ms};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{File, TryLockError};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The cache of runs without a repository.
pub const SCRATCH: &str = "scratch";
/// What the cache's lock file is called.
const LOCK: &str = ".lock";
/// The cache's record.
const META: &str = "cache.json";
/// How long a run waits for cleanup to let go of its cache.
const WAIT: Duration = Duration::from_secs(600);
/// How long the toolchain may take, the wait for another run's install included.
const TOOLCHAIN_LIMIT: Duration = Duration::from_secs(900);
/// The rustup that `host-setup.sh` installs on the host.
pub const HOST_RUSTUP: &str = "/usr/local/lib/runner-rustup/cargo/bin/rustup";
/// The environment variable `firecracker::guest_env` puts into the guest `PATH`.
pub const ENV_PATH_EXTRA: &str = "CHAT_VM_PATH_EXTRA";

/// Subfolders of a cache, made before each run.
const SUBDIRS: &[&str] = &[
    "cargo",
    "rustup",
    "sccache",
    "npm",
    "npm-global",
    "pnpm-store",
    "pnpm",
    "bun",
    "xdg-cache",
    "pipx",
    "uv-tools",
    "uv-python",
    "go",
    "go-build",
    "bin",
];

/// Known credential files a run may have left in a cache: removed before every run.
const CREDENTIALS: &[&str] = &[
    "cargo/credentials",
    "cargo/credentials.toml",
    "npm-global/etc/npmrc",
    "bun/bunfig.toml",
];

/// `cache.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub key: String,
    /// The repository's URL without user info, or its local path.
    pub repo: String,
    pub created_at: u64,
    pub last_used_at: u64,
}

/// The folder that holds every cache.
pub fn root(work_root: &Path) -> PathBuf {
    work_root.join("caches")
}

/// The cache key of a run's repository: `sha256(url)[..16]` (the mirror's key in
/// `workspace`), `sha256("path:" + canonical path)[..16]`, or [`SCRATCH`].
pub fn key(repo: Option<&RepoSpec>) -> String {
    let h = |s: &str| crate::workspace::hex(&Sha256::digest(s.as_bytes()))[..16].to_string();
    match repo {
        Some(RepoSpec { url: Some(url), .. }) => h(url),
        Some(RepoSpec {
            path: Some(path), ..
        }) => h(&format!("path:{}", canonical(path))),
        _ => SCRATCH.into(),
    }
}

fn canonical(path: &str) -> String {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string())
}

/// What `cache.json` says the repository is: never the user info of a URL.
fn repo_name(repo: Option<&RepoSpec>) -> String {
    match repo {
        Some(RepoSpec { url: Some(url), .. }) => strip_userinfo(url),
        Some(RepoSpec {
            path: Some(path), ..
        }) => canonical(path),
        _ => String::new(),
    }
}

/// `https://user:token@host/x` → `https://host/x`.
pub fn strip_userinfo(url: &str) -> String {
    let Some(i) = url.find("://") else {
        return url.to_string();
    };
    let rest = &url[i + 3..];
    let host_end = rest.find('/').unwrap_or(rest.len());
    match rest[..host_end].rfind('@') {
        Some(at) => format!("{}{}", &url[..i + 3], &rest[at + 1..]),
        None => url.to_string(),
    }
}

/// A key `daemon caches clear` accepts: 16 hex digits or [`SCRATCH`].
pub fn valid_key(k: &str) -> bool {
    k == SCRATCH || (k.len() == 16 && k.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The CLI's variables for the cache `c` (design-vm-state §3.3).
pub fn guest_vars(c: &Path) -> Vec<(String, String)> {
    let p = |sub: &str| c.join(sub).display().to_string();
    let path_extra = [
        "bin",
        "cargo/bin",
        "bun/bin",
        "npm-global/bin",
        "pnpm",
        "go/bin",
    ]
    .iter()
    .map(|s| p(s))
    .collect::<Vec<_>>()
    .join(":");
    [
        ("CARGO_HOME", p("cargo")),
        ("RUSTUP_HOME", p("rustup")),
        ("SCCACHE_DIR", p("sccache")),
        ("RUSTC_WRAPPER", "sccache".into()),
        ("npm_config_cache", p("npm")),
        ("npm_config_prefix", p("npm-global")),
        ("npm_config_store_dir", p("pnpm-store")),
        ("PNPM_HOME", p("pnpm")),
        ("BUN_INSTALL", p("bun")),
        ("XDG_CACHE_HOME", p("xdg-cache")),
        ("PIPX_HOME", p("pipx")),
        ("PIPX_BIN_DIR", p("bin")),
        ("UV_TOOL_DIR", p("uv-tools")),
        ("UV_TOOL_BIN_DIR", p("bin")),
        ("UV_PYTHON_INSTALL_DIR", p("uv-python")),
        ("GOPATH", p("go")),
        ("GOCACHE", p("go-build")),
        (ENV_PATH_EXTRA, path_extra),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// Adds the cache's variables to `env`, except the ones the runtime's configured `env` sets.
pub fn merge_vars(
    env: &mut Vec<(String, String)>,
    c: &Path,
    user: &std::collections::BTreeMap<String, String>,
) {
    for (k, v) in guest_vars(c) {
        if !user.contains_key(&k) {
            env.retain(|(n, _)| n != &k);
            env.push((k, v));
        }
    }
}

/// A run's hold on its repository's cache: the shared lock, until dropped. Dropping it notes
/// the time in `cache.json`.
#[derive(Debug)]
pub struct Held {
    pub dir: PathBuf,
    _lock: File,
}

impl Drop for Held {
    fn drop(&mut self) {
        touch(&self.dir, None);
    }
}

fn open_lock(dir: &Path) -> std::io::Result<File> {
    std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(LOCK))
}

/// The lock file is still the one at its path (cleanup did not move the cache away).
fn same_file(f: &File, path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (f.metadata(), std::fs::metadata(path)) {
            (Ok(a), Ok(b)) => a.ino() == b.ino() && a.dev() == b.dev(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = f;
        path.exists()
    }
}

/// Takes the run's shared hold on the cache of `repo`: makes the folders, waits up to 10
/// minutes while cleanup holds it, removes known credential files and notes the time.
pub async fn hold(work_root: &Path, repo: Option<&RepoSpec>) -> Result<Held, String> {
    hold_for(work_root, repo, WAIT).await
}

async fn hold_for(work_root: &Path, repo: Option<&RepoSpec>, wait: Duration) -> Result<Held, String> {
    let key = key(repo);
    let dir = root(work_root).join(&key);
    let until = tokio::time::Instant::now() + wait;
    let lock = loop {
        std::fs::create_dir_all(&dir).map_err(|e| format!("cache folder: {e}"))?;
        let f = open_lock(&dir).map_err(|e| format!("cache lock: {e}"))?;
        match f.try_lock_shared() {
            Ok(()) if same_file(&f, &dir.join(LOCK)) => break f,
            // Cleanup moved the cache away while this run waited: a new one.
            Ok(()) => continue,
            Err(TryLockError::WouldBlock) if tokio::time::Instant::now() < until => {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            Err(TryLockError::WouldBlock) => {
                return Err("the repository's cache is being removed".into());
            }
            Err(TryLockError::Error(e)) => return Err(format!("cache lock: {e}")),
        }
    };
    for s in SUBDIRS {
        let _ = std::fs::create_dir_all(dir.join(s));
    }
    remove_credentials(&dir);
    touch(&dir, Some(&repo_name(repo)));
    Ok(Held { dir, _lock: lock })
}

/// Removes the known credential files of the cache `c`.
pub fn remove_credentials(c: &Path) {
    for f in CREDENTIALS {
        let _ = std::fs::remove_file(c.join(f));
    }
}

pub fn read_meta(c: &Path) -> Option<Meta> {
    serde_json::from_slice(&std::fs::read(c.join(META)).ok()?).ok()
}

/// Sets `lastUsedAt` (and makes `cache.json` when there is none).
fn touch(c: &Path, repo: Option<&str>) {
    let now = now_ms();
    let key = c
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut m = read_meta(c).unwrap_or(Meta {
        key,
        repo: String::new(),
        created_at: now,
        last_used_at: now,
    });
    if let Some(r) = repo
        && m.repo.is_empty()
    {
        m.repo = r.to_string();
    }
    m.last_used_at = now;
    let tmp = c.join(format!(".{META}.{}.{now}", std::process::id()));
    if std::fs::write(&tmp, serde_json::to_vec_pretty(&m).unwrap_or_default()).is_ok()
        && std::fs::rename(&tmp, c.join(META)).is_err()
    {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// One cache, for `daemon caches` and cleanup.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub key: String,
    pub repo: String,
    pub bytes: u64,
    /// `lastUsedAt`, else the folder's modification time.
    pub last_used_at: u64,
}

/// The caches under `work_root`, least recently used first.
pub fn list(work_root: &Path) -> Vec<Entry> {
    let Ok(rd) = std::fs::read_dir(root(work_root)) else {
        return vec![];
    };
    let mut out: Vec<Entry> = rd
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let key = e.file_name().to_string_lossy().to_string();
            if !valid_key(&key) {
                return None;
            }
            let meta = read_meta(&e.path());
            let mtime = e
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            Some(Entry {
                key,
                repo: meta.as_ref().map(|m| m.repo.clone()).unwrap_or_default(),
                bytes: size(&e.path()),
                last_used_at: meta.map(|m| m.last_used_at).unwrap_or(mtime),
            })
        })
        .collect();
    out.sort_by(|a, b| (a.last_used_at, &a.key).cmp(&(b.last_used_at, &b.key)));
    out
}

/// Bytes of the files under `p` (symbolic links not followed).
pub fn size(p: &Path) -> u64 {
    let Ok(m) = std::fs::symlink_metadata(p) else {
        return 0;
    };
    if !m.is_dir() {
        return m.len();
    }
    std::fs::read_dir(p)
        .map(|rd| rd.flatten().map(|e| size(&e.path())).sum())
        .unwrap_or(0)
}

/// Removes the cache `key` unless a run holds it. Moves it aside under the exclusive lock, so
/// a run that waits for the lock makes a new one, then deletes it.
pub fn remove(work_root: &Path, key: &str) -> Result<(), String> {
    let dir = root(work_root).join(key);
    if !dir.is_dir() {
        return Err(format!("no cache {key}"));
    }
    let f = open_lock(&dir).map_err(|e| format!("cache {key}: {e}"))?;
    match f.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            return Err(format!("cache {key} is in use by a run"));
        }
        Err(TryLockError::Error(e)) => return Err(format!("cache {key}: {e}")),
    }
    let gone = root(work_root).join(format!(".removing-{key}-{}", now_ms()));
    std::fs::rename(&dir, &gone).map_err(|e| format!("cache {key}: {e}"))?;
    drop(f);
    let _ = std::fs::remove_dir_all(&gone);
    Ok(())
}

/// `daemon caches clear <key|all>`: the keys removed, and why each other one was not.
pub fn clear(work_root: &Path, which: &str) -> (Vec<String>, Vec<String>) {
    let keys: Vec<String> = if which == "all" {
        list(work_root).into_iter().map(|e| e.key).collect()
    } else if valid_key(which) {
        vec![which.to_string()]
    } else {
        return (vec![], vec![format!("{which}: not a cache key")]);
    };
    let (mut done, mut refused) = (vec![], vec![]);
    for k in keys {
        match remove(work_root, &k) {
            Ok(()) => done.push(k),
            Err(e) => refused.push(e),
        }
    }
    (done, refused)
}

/// Cleanup: while `free()` is below `min_free`, removes the least recently used caches, one at
/// a time and measuring again after each; caches in use stay. Returns how many it removed.
pub fn remove_lru(work_root: &Path, min_free: u64, free: impl Fn() -> u64) -> usize {
    let mut removed = 0;
    for e in list(work_root) {
        if free() >= min_free {
            break;
        }
        if remove(work_root, &e.key).is_ok() {
            eprintln!(
                "{} cache {} removed (last used {}, {})",
                now_ms(),
                e.key,
                e.last_used_at,
                e.bytes
            );
            removed += 1;
        }
    }
    removed
}

/// The Rust toolchain a working folder asks for: the `channel` of `rust-toolchain.toml` or
/// `rust-toolchain`, else `stable` when it has a `Cargo.toml`. None when it is no Rust project
/// or names no channel the daemon may pass on.
pub fn toolchain_channel(cwd: &Path) -> Option<String> {
    let ok = |c: &str| {
        !c.is_empty()
            && c.len() <= 64
            && c.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            && !c.starts_with('-')
    };
    for name in ["rust-toolchain.toml", "rust-toolchain"] {
        let Ok(text) = std::fs::read_to_string(cwd.join(name)) else {
            continue;
        };
        let channel = match text.parse::<toml::Table>() {
            Ok(t) => t
                .get("toolchain")
                .and_then(|t| t.get("channel"))
                .and_then(|c| c.as_str())
                .map(str::to_string),
            // The old form: just the channel.
            Err(_) => text.lines().next().map(|l| l.trim().to_string()),
        };
        return channel.filter(|c| ok(c));
    }
    cwd.join("Cargo.toml").is_file().then(|| "stable".into())
}

/// Installs `channel` into the cache `c` with the host's rustup, one install at a time per
/// cache, and makes it the default when the cache has none. Returns how long it took (ms).
pub async fn install_toolchain(c: &Path, channel: &str, rustup: &Path) -> Result<u64, String> {
    let started = std::time::Instant::now();
    if !rustup.is_file() {
        return Err(format!("{} is not installed", rustup.display()));
    }
    let work = async {
        let lock_path = c.join("rustup").join(".install.lock");
        std::fs::create_dir_all(c.join("rustup")).map_err(|e| e.to_string())?;
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .map_err(|e| e.to_string())?;
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(TryLockError::WouldBlock) => {
                    tokio::time::sleep(Duration::from_millis(500)).await
                }
                Err(TryLockError::Error(e)) => return Err(e.to_string()),
            }
        }
        let rustup_cmd = |args: &[&str]| {
            let mut cmd = tokio::process::Command::new(rustup);
            cmd.args(args)
                .env("RUSTUP_HOME", c.join("rustup"))
                .env("CARGO_HOME", c.join("cargo"))
                .stdin(std::process::Stdio::null())
                .kill_on_drop(true);
            cmd
        };
        let out = rustup_cmd(&[
            "toolchain",
            "install",
            channel,
            "--profile",
            "minimal",
            "-c",
            "clippy,rustfmt",
        ])
        .output()
        .await
        .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(tail(&out.stderr));
        }
        if !has_default(c) {
            let out = rustup_cmd(&["default", channel])
                .output()
                .await
                .map_err(|e| e.to_string())?;
            if !out.status.success() {
                return Err(tail(&out.stderr));
            }
        }
        drop(lock);
        Ok(())
    };
    match tokio::time::timeout(TOOLCHAIN_LIMIT, work).await {
        Ok(Ok(())) => Ok(started.elapsed().as_millis() as u64),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("took longer than 15 minutes".into()),
    }
}

/// The cache's rustup has a default toolchain.
fn has_default(c: &Path) -> bool {
    std::fs::read_to_string(c.join("rustup").join("settings.toml"))
        .ok()
        .and_then(|s| s.parse::<toml::Table>().ok())
        .is_some_and(|t| t.get("default_toolchain").is_some_and(|v| v.is_str()))
}

fn tail(b: &[u8]) -> String {
    let s = String::from_utf8_lossy(b);
    let s = s.trim();
    s[s.len().saturating_sub(500)..].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_url(u: &str) -> RepoSpec {
        RepoSpec {
            url: Some(u.into()),
            path: None,
            base_branch: None
        }
    }

    #[test]
    fn the_key_of_a_url_is_its_mirrors_key() {
        let url = "https://github.com/o/r.git";
        let mirror = crate::workspace::hex(&Sha256::digest(url.as_bytes()))[..16].to_string();
        assert_eq!(key(Some(&repo_url(url))), mirror);
        assert_eq!(key(None), SCRATCH);
        let tmp = tempfile::tempdir().unwrap();
        let path = RepoSpec {
            url: None,
            path: Some(tmp.path().display().to_string()),
            base_branch: None
        };
        let canon = std::fs::canonicalize(tmp.path()).unwrap();
        let want = crate::workspace::hex(&Sha256::digest(
            format!("path:{}", canon.display()).as_bytes(),
        ))[..16]
            .to_string();
        assert_eq!(key(Some(&path)), want);
        assert!(valid_key(&want) && valid_key(SCRATCH) && !valid_key("../x"));
    }

    #[test]
    fn cache_json_has_no_user_info() {
        assert_eq!(
            strip_userinfo("https://x:tok@github.com/o/r.git"),
            "https://github.com/o/r.git"
        );
        assert_eq!(strip_userinfo("git@github.com:o/r"), "git@github.com:o/r");
        assert_eq!(strip_userinfo("https://h/a@b"), "https://h/a@b");
    }

    #[test]
    fn the_variables_point_into_the_cache_and_the_users_env_wins() {
        let c = Path::new("/w/caches/k");
        let vars = guest_vars(c);
        let get = |k: &str| vars.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        assert_eq!(get("CARGO_HOME").as_deref(), Some("/w/caches/k/cargo"));
        assert_eq!(get("RUSTC_WRAPPER").as_deref(), Some("sccache"));
        assert_eq!(get("UV_TOOL_BIN_DIR").as_deref(), Some("/w/caches/k/bin"));
        assert_eq!(get("GOCACHE").as_deref(), Some("/w/caches/k/go-build"));
        assert_eq!(
            get(ENV_PATH_EXTRA).as_deref(),
            Some(
                "/w/caches/k/bin:/w/caches/k/cargo/bin:/w/caches/k/bun/bin:\
/w/caches/k/npm-global/bin:/w/caches/k/pnpm:/w/caches/k/go/bin"
            )
        );
        assert_eq!(vars.len(), 18);
        let mut env = vec![("CARGO_HOME".to_string(), "/host".to_string())];
        let user = [("CARGO_HOME".to_string(), "/mine".to_string())].into();
        merge_vars(&mut env, c, &user);
        let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("CARGO_HOME"), Some("/host"));
        assert_eq!(get("GOPATH"), Some("/w/caches/k/go"));
    }

    #[tokio::test]
    async fn a_run_gets_its_cache_without_old_credentials() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo_url("https://u:secret@example.com/r.git");
        let c = root(tmp.path()).join(key(Some(&repo)));
        for f in CREDENTIALS {
            std::fs::create_dir_all(c.join(f).parent().unwrap()).unwrap();
            std::fs::write(c.join(f), "token").unwrap();
        }
        let held = hold(tmp.path(), Some(&repo)).await.unwrap();
        assert_eq!(held.dir, c);
        for f in CREDENTIALS {
            assert!(!c.join(f).exists(), "{f}");
        }
        for s in SUBDIRS {
            assert!(c.join(s).is_dir(), "{s}");
        }
        let m = read_meta(&c).unwrap();
        assert_eq!(m.repo, "https://example.com/r.git");
        assert_eq!(m.key, key(Some(&repo)));
        // In use: neither cleanup nor `caches clear` takes it.
        assert!(remove(tmp.path(), &m.key).unwrap_err().contains("in use"));
        let started = m.last_used_at;
        std::thread::sleep(Duration::from_millis(5));
        drop(held);
        assert!(read_meta(&c).unwrap().last_used_at > started);
        remove(tmp.path(), &m.key).unwrap();
        assert!(!c.exists());
        assert!(list(tmp.path()).is_empty());
    }

    #[tokio::test]
    async fn a_run_waits_while_cleanup_holds_the_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let c = root(tmp.path()).join(SCRATCH);
        std::fs::create_dir_all(&c).unwrap();
        let f = open_lock(&c).unwrap();
        f.try_lock().unwrap();
        let err = hold_for(tmp.path(), None, Duration::from_millis(600))
            .await
            .unwrap_err();
        assert!(err.contains("being removed"));
        drop(f);
        hold_for(tmp.path(), None, Duration::from_millis(600))
            .await
            .unwrap();
    }

    /// Another process (`daemon caches clear`) sees a run's lock: a second process of this
    /// test binary holds the shared lock while `clear` runs.
    #[cfg(unix)]
    #[test]
    fn caches_clear_refuses_a_cache_a_second_process_holds() {
        if let Ok(dir) = std::env::var("VM_CACHE_HOLD") {
            let f = open_lock(Path::new(&dir)).unwrap();
            f.lock_shared().unwrap();
            println!("held");
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let c = root(tmp.path()).join(SCRATCH);
        std::fs::create_dir_all(&c).unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "vm_cache::tests::caches_clear_refuses_a_cache_a_second_process_holds",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("VM_CACHE_HOLD", &c)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut out = std::io::BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        loop {
            line.clear();
            use std::io::BufRead;
            if out.read_line(&mut line).unwrap() == 0 || line.trim() == "held" {
                break;
            }
        }
        let (done, refused) = clear(tmp.path(), SCRATCH);
        drop(child.stdin.take());
        let _ = child.wait();
        assert!(done.is_empty());
        assert!(refused[0].contains("in use"), "{refused:?}");
        assert!(c.is_dir());
        let (done, _) = clear(tmp.path(), SCRATCH);
        assert_eq!(done, vec![SCRATCH.to_string()]);
    }

    #[test]
    fn low_disk_removes_the_oldest_cache_first_and_skips_ones_in_use() {
        let tmp = tempfile::tempdir().unwrap();
        let make = |k: &str, at: u64| {
            let c = root(tmp.path()).join(k);
            std::fs::create_dir_all(&c).unwrap();
            std::fs::write(c.join("blob"), vec![0u8; 100]).unwrap();
            let m = Meta {
                key: k.into(),
                repo: k.into(),
                created_at: at,
                last_used_at: at,
            };
            std::fs::write(c.join(META), serde_json::to_vec(&m).unwrap()).unwrap();
            c
        };
        let (a, b, c) = (
            "aaaaaaaaaaaaaaaa",
            "bbbbbbbbbbbbbbbb",
            "cccccccccccccccc",
        );
        make(b, 200);
        let busy = make(a, 100);
        make(c, 300);
        let lock = open_lock(&busy).unwrap();
        lock.try_lock_shared().unwrap();
        let keys: Vec<String> = list(tmp.path()).into_iter().map(|e| e.key).collect();
        assert_eq!(keys, vec![a, b, c]);
        // Free space grows by one with each removal; one is enough.
        let n = remove_lru(tmp.path(), 1, || 3 - list(tmp.path()).len() as u64);
        assert_eq!(n, 1);
        let left: Vec<String> = list(tmp.path()).into_iter().map(|e| e.key).collect();
        assert_eq!(left, vec![a, c]);
        // Never enough: everything not in use goes.
        assert_eq!(remove_lru(tmp.path(), u64::MAX, || 0), 1);
        let left: Vec<String> = list(tmp.path()).into_iter().map(|e| e.key).collect();
        assert_eq!(left, vec![a]);
        // Enough disk: nothing goes.
        drop(lock);
        assert_eq!(remove_lru(tmp.path(), 0, || 0), 0);
    }

    #[test]
    fn the_toolchain_comes_from_the_working_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        assert_eq!(toolchain_channel(d), None);
        std::fs::write(d.join("Cargo.toml"), "[workspace]\n").unwrap();
        assert_eq!(toolchain_channel(d).as_deref(), Some("stable"));
        std::fs::write(
            d.join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"1.99.1\"\ncomponents = [\"rustfmt\"]\n",
        )
        .unwrap();
        assert_eq!(toolchain_channel(d).as_deref(), Some("1.99.1"));
        std::fs::write(d.join("rust-toolchain.toml"), "[toolchain]\nchannel = \"--x\"\n").unwrap();
        assert_eq!(toolchain_channel(d), None);
        std::fs::remove_file(d.join("rust-toolchain.toml")).unwrap();
        std::fs::write(d.join("rust-toolchain"), "nightly-2026-01-01\n").unwrap();
        assert_eq!(toolchain_channel(d).as_deref(), Some("nightly-2026-01-01"));
    }

    #[tokio::test]
    async fn no_host_rustup_is_an_error_not_a_hang() {
        let tmp = tempfile::tempdir().unwrap();
        let err = install_toolchain(tmp.path(), "stable", &tmp.path().join("rustup"))
            .await
            .unwrap_err();
        assert!(err.contains("not installed"));
    }

    /// A fake rustup: installs, then sets the default only when there is none.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_toolchain_is_installed_with_the_caches_rustup_home() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let c = tmp.path().join("c");
        std::fs::create_dir_all(&c).unwrap();
        let fake = tmp.path().join("rustup");
        let log = tmp.path().join("log");
        std::fs::write(
            &fake,
            format!(
                "#!/bin/sh\necho \"$RUSTUP_HOME $CARGO_HOME $*\" >> {}\n\
if [ \"$1\" = default ]; then echo 'default_toolchain = \"'$2'\"' > \"$RUSTUP_HOME/settings.toml\"; fi\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        install_toolchain(&c, "stable", &fake).await.unwrap();
        install_toolchain(&c, "1.99.1", &fake).await.unwrap();
        let lines = std::fs::read_to_string(&log).unwrap();
        let r = c.join("rustup").display().to_string();
        let k = c.join("cargo").display().to_string();
        assert_eq!(
            lines.lines().collect::<Vec<_>>(),
            vec![
                format!("{r} {k} toolchain install stable --profile minimal -c clippy,rustfmt"),
                format!("{r} {k} default stable"),
                format!("{r} {k} toolchain install 1.99.1 --profile minimal -c clippy,rustfmt"),
            ]
        );
    }
}
