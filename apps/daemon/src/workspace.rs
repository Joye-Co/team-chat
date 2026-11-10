//! Run folders: `<work_root>/runs/<runId>/` with `work/` (git worktree or scratch folder),
//! `run.json`, `events.jsonl`, `stream.jsonl`, `stderr.log` and the copied raw session.

use crate::config::Config;
use crate::proto::{RepoSpec, now_ms};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunMeta {
    pub run_id: String,
    pub started_at: u64,
    #[serde(default)]
    pub finished_at: Option<u64>,
    /// Git dir that owns the worktree, when there is one.
    #[serde(default)]
    pub git_dir: Option<PathBuf>,
    #[serde(default)]
    pub branch: Option<String>,
    /// Process group of the CLI, so a daemon that restarts after a crash can stop it.
    #[serde(default)]
    pub pgid: Option<i32>,
    /// The CLI program, checked against the process before that group is killed.
    #[serde(default)]
    pub program: Option<String>,
}

pub fn runs_dir(cfg: &Config) -> PathBuf {
    cfg.work_root.join("runs")
}

pub fn run_dir(cfg: &Config, run_id: &str) -> PathBuf {
    runs_dir(cfg).join(run_id)
}

pub fn write_meta(dir: &Path, meta: &RunMeta) {
    if let Ok(s) = serde_json::to_vec_pretty(meta) {
        let _ = std::fs::write(dir.join("run.json"), s);
    }
}

/// Records the CLI's process group in `run.json` (the run folder of `run_dir`).
pub fn record_process(run_dir: &Path, pgid: i32, program: &str) {
    if let Some(mut meta) = read_meta(run_dir) {
        meta.pgid = Some(pgid);
        meta.program = Some(program.to_string());
        write_meta(run_dir, &meta);
    }
}

pub fn read_meta(dir: &Path) -> Option<RunMeta> {
    serde_json::from_slice(&std::fs::read(dir.join("run.json")).ok()?).ok()
}

fn safe_ref(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && !s.contains("..")
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'))
}

fn safe_url(u: &str) -> bool {
    (u.starts_with("https://") || u.starts_with("ssh://") || u.starts_with("git@"))
        && !u.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Checks an assignment's repo against the config before the run is accepted.
pub fn validate_repo(cfg: &Config, repo: &RepoSpec) -> Result<(), String> {
    if let Some(b) = &repo.base_branch
        && !safe_ref(b)
    {
        return Err("invalid baseBranch".into());
    }
    match (&repo.url, &repo.path) {
        (Some(_), Some(_)) => Err("repo has both url and path".into()),
        (Some(u), None) if !safe_url(u) => Err("repo url must be https://, ssh:// or git@".into()),
        (None, Some(p)) => {
            let p = std::fs::canonicalize(p).map_err(|_| "repo path does not exist".to_string())?;
            if !cfg.repo_roots.is_empty()
                && !cfg.repo_roots.iter().any(|r| {
                    std::fs::canonicalize(r)
                        .map(|r| p.starts_with(r))
                        .unwrap_or(false)
                })
            {
                return Err("repo path is outside repo_roots".into());
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

async fn git(dir: Option<&Path>, args: &[&str]) -> Result<String, String> {
    let mut c = tokio::process::Command::new("git");
    // Windows: paths over 260 characters (deep trees under the run folder).
    if cfg!(windows) {
        c.args(["-c", "core.longpaths=true"]);
    }
    if let Some(d) = dir {
        c.arg("-C").arg(d);
    }
    c.args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let out = tokio::time::timeout(Duration::from_secs(600), c.output())
        .await
        .map_err(|_| format!("git {} timed out", args.first().unwrap_or(&"")))?
        .map_err(|e| format!("git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(format!(
            "git {} failed: {}",
            args.first().unwrap_or(&""),
            err.trim()
        ))
    }
}

/// Fetch refspec of the bare mirrors kept for `repo.url`.
const MIRROR_REFSPEC: &str = "+refs/heads/*:refs/remotes/origin/*";

static GIT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Windows: closes the run folder (and with it the working folder, the `chat-run` requests and
/// the settings files, which hold secrets) to every other user of the machine, wherever
/// `work_root` is. A volume that cannot do that (FAT, exFAT, a network share) gets no run.
/// Unix: nothing; each of those files has its own mode there.
fn private_run_dir(dir: &Path) -> Result<(), String> {
    let refuse = |why: String| {
        format!(
            "run folder {} cannot be made private to this user ({why}); put work_root on an \
NTFS volume of this machine",
            dir.display()
        )
    };
    crate::sys::protect(dir).map_err(|e| refuse(e.to_string()))?;
    match crate::sys::acl_problem(dir) {
        Some(why) => Err(refuse(why)),
        None => Ok(()),
    }
}

/// Makes `<run>/work`: a git worktree on a new branch `bot/<runId>`, or an empty folder.
pub async fn prepare(
    cfg: &Config,
    run_id: &str,
    repo: Option<&RepoSpec>,
) -> Result<(PathBuf, RunMeta), String> {
    let dir = run_dir(cfg, run_id);
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create run folder: {e}"))?;
    private_run_dir(&dir)?;
    let work = dir.join("work");
    let mut meta = RunMeta {
        run_id: run_id.into(),
        started_at: now_ms(),
        ..Default::default()
    };
    let Some(repo) = repo.filter(|r| r.url.is_some() || r.path.is_some()) else {
        std::fs::create_dir_all(&work).map_err(|e| format!("cannot create work folder: {e}"))?;
        write_meta(&dir, &meta);
        return Ok((work, meta));
    };
    let _guard = GIT_LOCK.lock().await;
    let git_dir = if let Some(url) = &repo.url {
        let h = hex(&Sha256::digest(url.as_bytes()))[..16].to_string();
        let mirror = cfg.work_root.join("repos").join(format!("{h}.git"));
        if !mirror.join("HEAD").exists() {
            std::fs::create_dir_all(cfg.work_root.join("repos")).map_err(|e| e.to_string())?;
            let m = mirror.to_string_lossy().to_string();
            git(None, &["clone", "--bare", "--quiet", "--", url, &m]).await?;
        }
        // Remote heads go to refs/remotes/origin/*, never to refs/heads/*: the local heads hold
        // the bot/<runId> branches of runs, and a fetch must not delete or overwrite them.
        // Mirrors made by older daemons mapped remote heads onto refs/heads/*; this resets them.
        git(
            Some(&mirror),
            &[
                "config",
                "--replace-all",
                "remote.origin.fetch",
                MIRROR_REFSPEC,
            ],
        )
        .await?;
        git(
            Some(&mirror),
            &["fetch", "--quiet", "--prune", "origin", MIRROR_REFSPEC],
        )
        .await?;
        mirror
    } else {
        let p = PathBuf::from(repo.path.as_deref().unwrap_or_default());
        // Best effort: a local repo without a reachable remote still works.
        let _ = git(Some(&p), &["fetch", "--quiet", "origin"]).await;
        p
    };
    let base = match &repo.base_branch {
        Some(b) => Some(b.clone()),
        // The default branch: what HEAD names (in a mirror, the remote's default at clone time).
        None => git(
            Some(&git_dir),
            &["symbolic-ref", "--quiet", "--short", "HEAD"],
        )
        .await
        .ok()
        .filter(|b| safe_ref(b)),
    };
    let start = match &base {
        Some(b) => {
            let remote = format!("refs/remotes/origin/{b}");
            if git(
                Some(&git_dir),
                &["rev-parse", "--verify", "--quiet", &remote],
            )
            .await
            .is_ok()
            {
                format!("origin/{b}")
            } else {
                b.clone()
            }
        }
        None => "HEAD".into(),
    };
    let branch = format!("bot/{run_id}");
    let w = work.to_string_lossy().to_string();
    git(
        Some(&git_dir),
        &[
            "worktree",
            "add",
            "--quiet",
            "--no-track",
            "-b",
            &branch,
            &w,
            &start,
        ],
    )
    .await?;
    meta.git_dir = Some(git_dir);
    meta.branch = Some(branch);
    write_meta(&dir, &meta);
    Ok((work, meta))
}

/// Commits of `branch` that no remote-tracking ref and no other local branch has. These exist
/// only in this git dir: deleting the branch would lose them.
async fn unpushed_commits(git_dir: &Path, branch: &str) -> Result<u64, String> {
    let n = git(
        Some(git_dir),
        &[
            "rev-list",
            "--count",
            branch,
            "--not",
            "--remotes",
            "--exclude=bot/*",
            "--branches",
        ],
    )
    .await?;
    n.parse().map_err(|_| format!("rev-list gave {n}"))
}

/// Makes `dir` and everything below it read-only (`a-w`; a side chat run's checkout, so its CLI
/// cannot write there whatever its own modes say), or gives the owner write access back
/// (`u+w`). Symlinks are not followed. Errors are skipped: the walk does what it can.
///
/// Windows: one entry in the folder's access list that denies the user the write rights, and
/// that everything in the folder inherits ([`crate::winacl::set_read_only`]); files cannot be
/// changed, made or removed there.
pub fn set_read_only(dir: &Path, read_only: bool) {
    #[cfg(windows)]
    {
        if dir.is_dir()
            && let Err(e) = crate::winacl::set_read_only(dir, read_only)
        {
            eprintln!(
                "{} {} not made {}: {e}",
                now_ms(),
                dir.display(),
                if read_only { "read-only" } else { "writable" }
            );
        }
    }
    #[cfg(not(windows))]
    set_mode_bits(dir, read_only);
}

#[cfg(not(windows))]
fn set_mode_bits(dir: &Path, read_only: bool) {
    #[cfg(unix)]
    let set = |p: &Path, m: &std::fs::Metadata| {
        use std::os::unix::fs::PermissionsExt;
        let mode = m.permissions().mode();
        let next = if read_only {
            mode & !0o222
        } else {
            mode | 0o200
        };
        if next != mode {
            let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(next));
        }
    };
    #[cfg(not(unix))]
    let set = |_: &Path, _: &std::fs::Metadata| {};
    let Ok(meta) = std::fs::symlink_metadata(dir) else {
        return;
    };
    if !meta.is_dir() {
        if meta.is_file() {
            set(dir, &meta);
        }
        return;
    }
    // A folder gets write access before its entries are walked and loses it after.
    if !read_only {
        set(dir, &meta);
    }
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            set_mode_bits(&e.path(), read_only);
        }
    }
    if read_only {
        set(dir, &meta);
    }
}

/// Removes a finished run folder and its worktree registration. The run's branch is deleted
/// only when all its commits are also on a remote or another branch; otherwise it is kept (it
/// costs no disk beyond its objects) and the daemon log says so.
pub async fn remove_run(dir: &Path) {
    // A side chat run's checkout is read-only, also after a crash.
    set_read_only(&dir.join("work"), false);
    if let Some(meta) = read_meta(dir)
        && let Some(gd) = &meta.git_dir
    {
        let _guard = GIT_LOCK.lock().await;
        let work = dir.join("work");
        let w = work.to_string_lossy().to_string();
        if let Ok(st) = git(Some(&work), &["status", "--porcelain"]).await
            && !st.is_empty()
        {
            eprintln!(
                "{} cleanup: run {} had uncommitted changes in its worktree; removed",
                now_ms(),
                meta.run_id
            );
        }
        let _ = git(Some(gd), &["worktree", "remove", "--force", &w]).await;
        if let Some(b) = &meta.branch {
            match unpushed_commits(gd, b).await {
                Ok(0) => {
                    let _ = git(Some(gd), &["branch", "-D", b]).await;
                }
                Ok(n) => eprintln!(
                    "{} cleanup: kept branch {b} in {} ({n} commits not pushed)",
                    now_ms(),
                    gd.display()
                ),
                Err(e) => eprintln!(
                    "{} cleanup: kept branch {b} in {} ({e})",
                    now_ms(),
                    gd.display()
                ),
            }
        }
        let _ = git(Some(gd), &["worktree", "prune"]).await;
    }
    remove_dir(dir).await;
}

/// Removes the folder `dir`. Windows cannot remove a file that a program still has open (a
/// tool that is just ending, an indexer or a virus scanner looking at it): tries again for a
/// few seconds, and leaves what is still held to the next cleanup.
async fn remove_dir(dir: &Path) {
    let tries = if cfg!(windows) { 10 } else { 1 };
    for n in 1..=tries {
        match tokio::fs::remove_dir_all(dir).await {
            Ok(()) => return,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) if n == tries => {
                if cfg!(windows) {
                    eprintln!(
                        "{} cleanup: {} not removed yet: {e}",
                        now_ms(),
                        dir.display()
                    );
                }
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(300)).await,
        }
    }
}

/// Removes the `chat-run` request folders (`work/.chat-run-<run id>`) that a daemon crash left
/// in the working folders of runs whose CLI is not running (not in `active`). Only the
/// transport's own files go, then the folder when it is empty. Returns the folders removed.
pub fn sweep_spools(cfg: &Config, active: &[String]) -> usize {
    let Ok(rd) = std::fs::read_dir(runs_dir(cfg)) else {
        return 0;
    };
    let mut removed = 0;
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if active.contains(&name) {
            continue;
        }
        let spool = crate::chat_run::spool_dir(&e.path().join("work"), &name);
        let is_dir = std::fs::symlink_metadata(&spool).is_ok_and(|m| m.is_dir());
        if is_dir {
            crate::chat_run::remove_spool(&spool);
            if !spool.exists() {
                removed += 1;
            }
        }
    }
    removed
}

/// Removes finished runs older than `keep_runs_hours`, then the oldest finished runs while
/// free disk is below `min_free_gb`. Active runs are never touched.
pub async fn cleanup(cfg: &Config, active: &[String]) -> usize {
    let Ok(rd) = std::fs::read_dir(runs_dir(cfg)) else {
        return 0;
    };
    let mut finished: Vec<(u64, PathBuf)> = vec![];
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if active.contains(&name) || !e.path().is_dir() {
            continue;
        }
        let at = read_meta(&e.path())
            .map(|m| m.finished_at.unwrap_or(m.started_at))
            .or_else(|| {
                e.metadata()
                    .ok()?
                    .modified()
                    .ok()?
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_millis() as u64)
            })
            .unwrap_or(0);
        finished.push((at, e.path()));
    }
    finished.sort();
    let cutoff = now_ms().saturating_sub(cfg.keep_runs_hours * 3_600_000);
    let min_free = cfg.min_free_gb * (1 << 30);
    let mut removed = 0;
    for (at, path) in finished {
        let low_disk = crate::health::disk(&cfg.work_root).0 < min_free;
        if at <= cutoff || low_disk {
            remove_run(&path).await;
            removed += 1;
        }
    }
    removed
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A side chat run's checkout: nothing in it can be changed, made or removed until it is
    /// writable again (the cleanup makes it so before it removes the run).
    #[test]
    fn a_read_only_checkout_refuses_writes() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("work");
        std::fs::create_dir_all(work.join("src")).unwrap();
        std::fs::write(work.join("src").join("a.txt"), "old").unwrap();
        set_read_only(&work, true);
        // Root ignores mode bits (a container that runs the tests as root).
        let root = cfg!(unix) && crate::sys::euid() == 0;
        if !root {
            assert!(std::fs::write(work.join("src").join("a.txt"), "new").is_err());
            assert!(std::fs::write(work.join("new.txt"), "x").is_err());
            assert!(std::fs::write(work.join("src").join("new.txt"), "x").is_err());
            // Windows: a process with the restore privilege (the CI runner) may still delete.
            #[cfg(unix)]
            assert!(std::fs::remove_file(work.join("src").join("a.txt")).is_err());
        }
        assert_eq!(
            std::fs::read_to_string(work.join("src").join("a.txt")).unwrap(),
            "old"
        );
        set_read_only(&work, false);
        std::fs::write(work.join("src").join("a.txt"), "new").unwrap();
        std::fs::write(work.join("new.txt"), "x").unwrap();
        std::fs::remove_dir_all(&work).unwrap();
    }
}
