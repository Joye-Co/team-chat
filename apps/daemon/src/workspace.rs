//! Run folders: `<work_root>/runs/<runId>/` with `run.json`, `events.jsonl`, `stream.jsonl`,
//! `stderr.log` and the copied raw session. The CLI works in a work line's folder
//! (`<work_root>/lines/<key>/work`, shared by one member's runs in one topic), or, for a side
//! chat run, in its own `<run>/work`. Either is a git worktree or a scratch folder.

use crate::config::Config;
use crate::proto::{RepoSpec, now_ms};
use chat_protocol::relay::RunCode;
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
    /// The work line folder whose `work/` the run used; the run folder then has no worktree
    /// of its own, and removing the run leaves the line alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<PathBuf>,
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

tokio::task_local! {
    /// Environment for the daemon's own git commands of one run (feature "github": the
    /// person's GitHub for the mirror's clone and fetch). `None` removes the variable.
    static GIT_ENV: Vec<(String, Option<String>)>;
}

/// Runs `f` (a run's [`prepare`] or [`prepare_line`]) with `env` on every git it starts.
pub async fn with_git_env<F: std::future::Future>(
    env: Vec<(String, Option<String>)>,
    f: F,
) -> F::Output {
    GIT_ENV.scope(env, f).await
}

async fn git(dir: Option<&Path>, args: &[&str]) -> Result<String, String> {
    let mut c = tokio::process::Command::new("git");
    let _ = GIT_ENV.try_with(|env| {
        for (k, v) in env {
            match v {
                Some(v) => c.env(k, v),
                None => c.env_remove(k),
            };
        }
    });
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
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    // Windows: in a job, so a git that is given up after the time limit takes the helpers it
    // started (`git-remote-https`) with it.
    let out = tokio::time::timeout(Duration::from_secs(600), crate::health::output(&mut c))
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

/// Makes the run folder. Windows: `<work_root>/runs` and the run folder in it are made with
/// their access list (the daemon's user, SYSTEM and Administrators, nothing inherited), so
/// the working folder, the `chat-run` requests and the settings files, which hold secrets,
/// are closed to every other user from the first moment, wherever `work_root` is. A folder of
/// either name that is already there and that others can get into is refused, and so is a
/// volume without access lists (FAT, exFAT, a network share). Unix: `create_dir_all`; each of
/// those files has its own mode there.
fn private_run_dir(cfg: &Config, dir: &Path) -> Result<(), String> {
    let refuse = |what: &Path, e: std::io::Error| {
        if cfg!(windows) {
            format!(
                "cannot create run folder: {} cannot be made private to this user ({e}); put \
work_root on an NTFS volume of this machine, in a folder of your own",
                what.display()
            )
        } else {
            format!("cannot create run folder: {e}")
        }
    };
    if cfg!(windows) {
        let runs = runs_dir(cfg);
        crate::sys::create_private_dir(&runs).map_err(|e| refuse(&runs, e))?;
    }
    crate::sys::create_private_dir(dir).map_err(|e| refuse(dir, e))
}

/// The git dir a run's worktree is made from: the bare mirror of `repo.url` (cloned once,
/// fetched now) or the local repository `repo.path`. Call with [`GIT_LOCK`] held.
async fn repo_git_dir(cfg: &Config, repo: &RepoSpec) -> Result<PathBuf, String> {
    Ok(if let Some(url) = &repo.url {
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
    })
}

/// Where a new run branch starts: `origin/<base>` when it exists, else `<base>`, else `HEAD`.
async fn start_point(git_dir: &Path, repo: &RepoSpec) -> String {
    let base = match &repo.base_branch {
        Some(b) => Some(b.clone()),
        // The default branch: what HEAD names (in a mirror, the remote's default at clone time).
        None => git(
            Some(git_dir),
            &["symbolic-ref", "--quiet", "--short", "HEAD"],
        )
        .await
        .ok()
        .filter(|b| safe_ref(b)),
    };
    match &base {
        Some(b) => {
            let remote = format!("refs/remotes/origin/{b}");
            if git(
                Some(git_dir),
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
    }
}

/// Makes `<run>/work`: a git worktree on a new branch `bot/<runId>`, or an empty folder.
pub async fn prepare(
    cfg: &Config,
    run_id: &str,
    repo: Option<&RepoSpec>,
) -> Result<(PathBuf, RunMeta), String> {
    let dir = run_dir(cfg, run_id);
    private_run_dir(cfg, &dir)?;
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
    let git_dir = repo_git_dir(cfg, repo).await?;
    let start = start_point(&git_dir, repo).await;
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

/// A work line: the working folder and branch that one member's runs in one topic (and one
/// repository) share, so a follow-up run finds what the earlier ones left.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineMeta {
    pub key: String,
    pub bot_member_id: String,
    pub topic_id: String,
    #[serde(default)]
    pub git_dir: Option<PathBuf>,
    #[serde(default)]
    pub branch: Option<String>,
    pub created_at: u64,
    /// When a run last ended in it (or started, while one runs): the idle clock of cleanup.
    pub last_used_at: u64,
    #[serde(default)]
    pub last_run_id: Option<String>,
}

pub fn lines_dir(cfg: &Config) -> PathBuf {
    cfg.work_root.join("lines")
}

fn read_line(dir: &Path) -> Option<LineMeta> {
    serde_json::from_slice(&std::fs::read(dir.join("line.json")).ok()?).ok()
}

fn write_line(dir: &Path, meta: &LineMeta) {
    if let Ok(s) = serde_json::to_vec_pretty(meta) {
        let _ = std::fs::write(dir.join("line.json"), s);
    }
}

/// The line of a member's runs in a topic: the same for every run of that member in that
/// topic on the same repository (`repo.url`, or `repo.path` as given), another one when the
/// repository changes.
pub fn line_key(bot_member_id: &str, topic_id: &str, repo: Option<&RepoSpec>) -> String {
    let repo = match repo {
        Some(RepoSpec { url: Some(u), .. }) => format!("url:{u}"),
        Some(RepoSpec { path: Some(p), .. }) => format!("path:{p}"),
        _ => String::new(),
    };
    hex(&Sha256::digest(
        format!("{bot_member_id}\n{topic_id}\n{repo}").as_bytes(),
    ))[..16]
        .to_string()
}

/// The line folders a run or the cleanup is using in this daemon, and whether the holder is the
/// cleanup. A line is used by one at a time.
static BUSY_LINES: std::sync::Mutex<std::collections::BTreeMap<PathBuf, bool>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// Takes the line for a run (`cleanup` false) or for the cleanup. `Err(holder_is_cleanup)`
/// when it is held already.
fn claim_line(dir: &Path, cleanup: bool) -> Result<(), bool> {
    let mut busy = BUSY_LINES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(&c) = busy.get(dir) {
        return Err(c);
    }
    busy.insert(dir.to_path_buf(), cleanup);
    Ok(())
}

/// The folder that work which could not be committed is moved to instead of being deleted.
pub fn kept_dir(cfg: &Config) -> PathBuf {
    cfg.work_root.join("kept")
}

/// Moves `src` to `<kept>/<label>-<ms>` so its files survive; false when it could not be moved.
fn move_aside(kept: &Path, src: &Path, label: &str) -> bool {
    let to = kept.join(format!("{label}-{}", now_ms()));
    match crate::sys::create_private_dir(kept).and_then(|_| std::fs::rename(src, &to)) {
        Ok(()) => {
            eprintln!(
                "{} cleanup: moved {} to {} to keep its files",
                now_ms(),
                src.display(),
                to.display()
            );
            true
        }
        Err(e) => {
            eprintln!(
                "{} cleanup: {} not moved aside: {e}",
                now_ms(),
                src.display()
            );
            false
        }
    }
}

fn release_line(dir: &Path) {
    BUSY_LINES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(dir);
}

/// A run's hold on its line. Dropping it (when the run's CLI is done) marks the line used now
/// and lets the next run in.
#[derive(Debug)]
pub struct LineGuard {
    dir: PathBuf,
    run_id: String,
    /// The line's folder was made for this run (first run, or the old one was cleaned up).
    pub fresh: bool,
    /// A fresh folder that checked out the line's existing branch (what earlier runs committed,
    /// and what cleanup committed for them).
    pub restored: bool,
}

impl Drop for LineGuard {
    fn drop(&mut self) {
        if let Some(mut m) = read_line(&self.dir) {
            m.last_used_at = now_ms();
            m.last_run_id = Some(self.run_id.clone());
            write_line(&self.dir, &m);
        }
        release_line(&self.dir);
    }
}

/// Makes the run folder (logs and metadata) and readies the run's line: the line folder's
/// `work/` is used as it is when it is there, made again otherwise (a git worktree of the
/// line's branch `bot/<key>` when that branch is still there, else a new one from the base
/// branch, or an empty scratch folder). `Ok(None)` when another run holds the line: the caller
/// gives the run its own folder ([`prepare`]) instead, so two runs never share a checkout.
pub async fn prepare_line(
    cfg: &Config,
    run_id: &str,
    bot_member_id: &str,
    topic_id: &str,
    repo: Option<&RepoSpec>,
) -> Result<Option<(PathBuf, RunMeta, LineGuard)>, String> {
    let repo = repo.filter(|r| r.url.is_some() || r.path.is_some());
    let key = line_key(bot_member_id, topic_id, repo);
    let line = lines_dir(cfg).join(&key);
    // The cleanup's hold is short (a commit and a removal): wait for it instead of giving the
    // run a folder of its own. Another run's hold lasts the whole run.
    let mut waited = Duration::ZERO;
    loop {
        match claim_line(&line, false) {
            Ok(()) => break,
            Err(true) if waited < Duration::from_secs(600) => {
                tokio::time::sleep(Duration::from_millis(200)).await;
                waited += Duration::from_millis(200);
            }
            Err(_) => return Ok(None),
        }
    }
    let mut guard = LineGuard {
        dir: line.clone(),
        run_id: run_id.into(),
        fresh: false,
        restored: false,
    };
    let dir = run_dir(cfg, run_id);
    private_run_dir(cfg, &dir)?;
    if cfg!(windows) {
        crate::sys::create_private_dir(&lines_dir(cfg))
            .map_err(|e| format!("cannot create line folder: {e}"))?;
    }
    crate::sys::create_private_dir(&line).map_err(|e| format!("cannot create line folder: {e}"))?;
    let work = line.join("work");
    let now = now_ms();
    let mut lm = read_line(&line).unwrap_or_else(|| LineMeta {
        key: key.clone(),
        bot_member_id: bot_member_id.into(),
        topic_id: topic_id.into(),
        created_at: now,
        ..Default::default()
    });
    lm.last_used_at = now;
    lm.last_run_id = Some(run_id.into());
    let mut meta = RunMeta {
        run_id: run_id.into(),
        started_at: now,
        line: Some(line.clone()),
        ..Default::default()
    };
    match repo {
        None => {
            guard.fresh = !work.is_dir();
            std::fs::create_dir_all(&work)
                .map_err(|e| format!("cannot create work folder: {e}"))?;
        }
        Some(repo) => {
            let _lock = GIT_LOCK.lock().await;
            let git_dir = repo_git_dir(cfg, repo).await?;
            let branch = format!("bot/{key}");
            let usable = work.join(".git").exists()
                && git(Some(&work), &["rev-parse", "--git-dir"]).await.is_ok();
            if !usable {
                guard.fresh = true;
                if work.exists() {
                    set_read_only(&work, false);
                    // Not a worktree git can use: its files may still be work, so they are
                    // moved aside, not deleted.
                    if !move_aside(&kept_dir(cfg), &work, &format!("line-{key}-lost")) {
                        let _ = tokio::fs::remove_dir_all(&work).await;
                    }
                }
                let _ = git(Some(&git_dir), &["worktree", "prune"]).await;
                let w = work.to_string_lossy().to_string();
                let local = format!("refs/heads/{branch}");
                let kept = git(
                    Some(&git_dir),
                    &["rev-parse", "--verify", "--quiet", &local],
                )
                .await
                .is_ok();
                if kept {
                    git(Some(&git_dir), &["worktree", "add", "--quiet", &w, &branch]).await?;
                    guard.restored = true;
                } else {
                    // The branch was cleaned up after it was pushed under its own name: go on
                    // from the pushed one.
                    let pushed = format!("refs/remotes/origin/{branch}");
                    let start = if git(
                        Some(&git_dir),
                        &["rev-parse", "--verify", "--quiet", &pushed],
                    )
                    .await
                    .is_ok()
                    {
                        guard.restored = true;
                        format!("origin/{branch}")
                    } else {
                        start_point(&git_dir, repo).await
                    };
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
                }
            }
            lm.git_dir = Some(git_dir.clone());
            lm.branch = Some(branch.clone());
            meta.git_dir = Some(git_dir);
            meta.branch = Some(branch);
        }
    }
    write_line(&line, &lm);
    write_meta(&dir, &meta);
    Ok(Some((work, meta, guard)))
}

/// Commits what is uncommitted in the worktree `work` onto the branch it has checked out, so
/// that removing the folder loses nothing. A detached HEAD gets a branch
/// `<branch>-kept-<ms>` for that commit. Ok(false): there was nothing to commit.
async fn keep_uncommitted(work: &Path, branch: Option<&str>) -> Result<bool, String> {
    // A worktree git cannot read has nothing that can be kept: it goes.
    let Ok(st) = git(Some(work), &["status", "--porcelain"]).await else {
        return Ok(false);
    };
    if st.is_empty() {
        return Ok(false);
    }
    git(Some(work), &["add", "-A"]).await?;
    git(
        Some(work),
        &[
            "-c",
            "user.name=daemon",
            "-c",
            "user.email=daemon@localhost",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "--no-verify",
            "-m",
            "wip: uncommitted changes kept by cleanup",
        ],
    )
    .await?;
    if git(Some(work), &["symbolic-ref", "--quiet", "HEAD"])
        .await
        .is_err()
    {
        let name = format!("{}-kept-{}", branch.unwrap_or("bot/detached"), now_ms());
        git(Some(work), &["branch", &name, "HEAD"]).await?;
    }
    Ok(true)
}

/// Removes `git_dir`'s index lock of the worktree `work`. Only for a worktree no run holds: a
/// lock there was left by a git that was killed (a cancelled run, a VM torn down).
async fn remove_stale_index_lock(work: &Path) {
    if let Ok(p) = git(Some(work), &["rev-parse", "--git-path", "index.lock"]).await {
        let p = PathBuf::from(p);
        let p = if p.is_absolute() { p } else { work.join(p) };
        if std::fs::remove_file(&p).is_ok() {
            eprintln!("{} cleanup: removed a stale {}", now_ms(), p.display());
        }
    }
}

/// Removes the worktree `work` of `git_dir` after its uncommitted changes are committed, and
/// `branch` when all its commits are also elsewhere. When the changes cannot be committed (also
/// after a stale index lock is removed), the folder is moved to `kept` with its files instead,
/// so that it never blocks the cleanup. False (nothing removed) only when that fails too.
async fn remove_worktree(
    git_dir: &Path,
    work: &Path,
    branch: Option<&str>,
    what: &str,
    kept: &Path,
    label: &str,
) -> bool {
    if work.join(".git").exists() {
        let mut res = keep_uncommitted(work, branch).await;
        if res.is_err() {
            remove_stale_index_lock(work).await;
            res = keep_uncommitted(work, branch).await;
        }
        match res {
            Ok(true) => eprintln!(
                "{} cleanup: {what} had uncommitted changes; committed them to {}",
                now_ms(),
                branch.unwrap_or("its branch")
            ),
            Ok(false) => {}
            Err(e) => {
                eprintln!(
                    "{} cleanup: {what}: its uncommitted changes could not be committed ({e})",
                    now_ms()
                );
                if !move_aside(kept, work, label) {
                    return false;
                }
            }
        }
    }
    let w = work.to_string_lossy().to_string();
    let _ = git(Some(git_dir), &["worktree", "remove", "--force", &w]).await;
    if let Some(b) = branch {
        match unpushed_commits(git_dir, b).await {
            Ok(0) => {
                let _ = git(Some(git_dir), &["branch", "-D", b]).await;
            }
            Ok(n) => eprintln!(
                "{} cleanup: kept branch {b} in {} ({n} commits not pushed)",
                now_ms(),
                git_dir.display()
            ),
            Err(e) => eprintln!(
                "{} cleanup: kept branch {b} in {} ({e})",
                now_ms(),
                git_dir.display()
            ),
        }
    }
    let _ = git(Some(git_dir), &["worktree", "prune"]).await;
    true
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

/// Where a run left its code (`run.done.code`, design-relay R5): the branch the worktree `work`
/// has checked out (`fallback` for a detached HEAD), its head commit, and whether every commit of
/// it is on the remote. "Pushed" is checked, not taken from the CLI: the branch is fetched from
/// `origin` into its remote-tracking ref and the head must be in it; uncommitted changes to
/// tracked files, a failed fetch or a branch the remote lacks all read as not pushed. `None`
/// when `work` is not a git worktree with a commit. Run it with the run's [`with_git_env`].
pub async fn code_location(work: &Path, fallback: Option<&str>) -> Option<RunCode> {
    git(Some(work), &["rev-parse", "--is-inside-work-tree"])
        .await
        .ok()
        .filter(|s| s == "true")?;
    let head = git(Some(work), &["rev-parse", "--verify", "--quiet", "HEAD"])
        .await
        .ok()
        .filter(|h| (4..=64).contains(&h.len()) && h.bytes().all(|b| b.is_ascii_hexdigit()))?
        .to_ascii_lowercase();
    let branch = match git(Some(work), &["symbolic-ref", "--quiet", "--short", "HEAD"]).await {
        Ok(b) => b,
        Err(_) => fallback?.to_string(),
    };
    if !safe_ref(&branch) || branch.len() > 256 {
        return None;
    }
    let pushed = branch_pushed(work, &branch, &head).await;
    Some(RunCode {
        branch,
        head,
        pushed,
    })
}

/// True when `head` is on `origin`'s `branch` as fetched now, and `work` has no uncommitted
/// changes to tracked files.
async fn branch_pushed(work: &Path, branch: &str, head: &str) -> bool {
    match git(
        Some(work),
        &["status", "--porcelain", "--untracked-files=no"],
    )
    .await
    {
        Ok(st) if st.is_empty() => {}
        _ => return false,
    }
    let tracking = format!("refs/remotes/origin/{branch}");
    let refspec = format!("+refs/heads/{branch}:{tracking}");
    // Other runs fetch into the same git dir; the run is over, so a short wait is fine.
    let fetched = tokio::time::timeout(Duration::from_secs(90), async {
        let _lock = GIT_LOCK.lock().await;
        git(
            Some(work),
            &["fetch", "--quiet", "--no-tags", "origin", &refspec],
        )
        .await
    })
    .await;
    if !matches!(fetched, Ok(Ok(_))) {
        return false;
    }
    git(
        Some(work),
        &["merge-base", "--is-ancestor", head, &tracking],
    )
    .await
    .is_ok()
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

/// Removes a finished run folder and its worktree registration. Uncommitted changes in its
/// worktree are committed to the run's branch first; the branch is deleted only when all its
/// commits are also on a remote or another branch, otherwise it is kept (it costs no disk
/// beyond its objects) and the daemon log says so. A run that worked in a line leaves the line
/// alone ([`cleanup_lines`] removes it). False when the folder was kept.
pub async fn remove_run(cfg: &Config, dir: &Path) -> bool {
    // A side chat run's checkout is read-only, also after a crash.
    set_read_only(&dir.join("work"), false);
    if let Some(meta) = read_meta(dir)
        && meta.line.is_none()
        && let Some(gd) = &meta.git_dir
    {
        let _guard = GIT_LOCK.lock().await;
        let what = format!("run {}", meta.run_id);
        let label = format!(
            "run-{}",
            dir.file_name().unwrap_or_default().to_string_lossy()
        );
        let work = dir.join("work");
        let branch = meta.branch.as_deref();
        if !remove_worktree(gd, &work, branch, &what, &kept_dir(cfg), &label).await {
            return false;
        }
    }
    remove_dir(dir).await;
    true
}

/// Removes the lines no run has used for `keep_runs_hours`: their uncommitted changes are
/// committed to the line's branch, the worktree goes, and the branch goes only when all its
/// commits are elsewhere. Lines a run holds are skipped, and so are lines of `active` runs.
/// Returns the lines removed.
pub async fn cleanup_lines(cfg: &Config, active: &[String]) -> usize {
    let Ok(rd) = std::fs::read_dir(lines_dir(cfg)) else {
        return 0;
    };
    let in_use: Vec<PathBuf> = active
        .iter()
        .filter_map(|id| read_meta(&run_dir(cfg, id))?.line)
        .collect();
    let cutoff = now_ms().saturating_sub(cfg.keep_runs_hours * 3_600_000);
    let mut removed = 0;
    for e in rd.flatten() {
        let path = e.path();
        let key = e.file_name().to_string_lossy().to_string();
        if !path.is_dir() || in_use.contains(&path) {
            continue;
        }
        let meta = read_line(&path);
        let at = meta
            .as_ref()
            .map(|m| m.last_used_at)
            .or_else(|| modified_ms(&path))
            .unwrap_or(0);
        if at > cutoff || claim_line(&path, true).is_err() {
            continue;
        }
        let work = path.join("work");
        set_read_only(&work, false);
        let gone = match meta.as_ref().and_then(|m| m.git_dir.clone()) {
            Some(gd) => {
                let _guard = GIT_LOCK.lock().await;
                let what = format!("line {key}");
                let branch = meta.as_ref().and_then(|m| m.branch.as_deref());
                let label = format!("line-{key}");
                remove_worktree(&gd, &work, branch, &what, &kept_dir(cfg), &label).await
            }
            None => true,
        };
        if gone {
            remove_dir(&path).await;
            removed += 1;
        }
        release_line(&path);
    }
    removed
}

fn modified_ms(p: &Path) -> Option<u64> {
    std::fs::metadata(p)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as u64)
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
/// in the working folders of runs and lines whose CLI is not running (not in `active`). Only the
/// transport's own files go, then the folder when it is empty. Returns the folders removed.
pub fn sweep_spools(cfg: &Config, active: &[String]) -> usize {
    let mut removed = 0;
    let runs = std::fs::read_dir(runs_dir(cfg)).into_iter().flatten();
    for e in runs.flatten() {
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
    // Lines: the request folders of every run that is not running.
    let running: Vec<PathBuf> = active
        .iter()
        .map(|id| crate::chat_run::spool_dir(Path::new(""), id))
        .collect();
    let lines = std::fs::read_dir(lines_dir(cfg)).into_iter().flatten();
    for line in lines.flatten() {
        let entries = std::fs::read_dir(line.path().join("work"))
            .into_iter()
            .flatten();
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.starts_with(crate::chat_run::DIR_PREFIX)
                || running.iter().any(|r| r.as_os_str() == name.as_str())
                || !e.file_type().is_ok_and(|t| t.is_dir())
            {
                continue;
            }
            crate::chat_run::remove_spool(&e.path());
            if !e.path().exists() {
                removed += 1;
            }
        }
    }
    removed
}

/// Removes finished runs older than `keep_runs_hours`, then the oldest finished runs while
/// free disk is below `min_free_gb`, then the lines idle for `keep_runs_hours` (a line goes
/// only when idle, also when disk is low: its folder holds work in progress). Active runs and
/// the lines they use are never touched.
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
        if (at <= cutoff || low_disk) && remove_run(cfg, &path).await {
            removed += 1;
        }
    }
    removed + cleanup_lines(cfg, active).await + cleanup_kept(cfg).await
}

/// Removes the oldest folders that were moved to `kept` while free disk is below `min_free_gb`.
async fn cleanup_kept(cfg: &Config) -> usize {
    let Ok(rd) = std::fs::read_dir(kept_dir(cfg)) else {
        return 0;
    };
    let mut kept: Vec<(u64, PathBuf)> = rd
        .flatten()
        .map(|e| (modified_ms(&e.path()).unwrap_or(0), e.path()))
        .collect();
    kept.sort();
    let min_free = cfg.min_free_gb * (1 << 30);
    let mut removed = 0;
    for (_, path) in kept {
        if crate::health::disk(&cfg.work_root).0 >= min_free {
            break;
        }
        set_read_only(&path, false);
        remove_dir(&path).await;
        eprintln!("{} cleanup: disk low, removed {}", now_ms(), path.display());
        removed += 1;
    }
    removed
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Windows: the run folder is private when its work root is a folder that everyone may
    /// write to, and a run folder that somebody else made first is refused.
    #[cfg(windows)]
    #[tokio::test]
    async fn a_run_folder_is_private_under_an_open_work_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        std::fs::create_dir(&root).unwrap();
        let opened = std::process::Command::new("icacls")
            .arg(&root)
            .args(["/grant", "*S-1-1-0:(OI)(CI)F"])
            .output()
            .unwrap();
        assert!(opened.status.success(), "{opened:?}");
        assert!(crate::sys::acl_problem(&root).is_some());
        let cfg = Config::parse(&format!(
            "server_url = \"http://127.0.0.1:1\"\ntoken_file = '{}'\nwork_root = '{}'\n",
            dir.path().join("token").display(),
            root.display()
        ))
        .unwrap();

        let (work, _) = prepare(&cfg, "r1", None).await.unwrap();
        for p in [runs_dir(&cfg), run_dir(&cfg, "r1"), work.clone()] {
            assert_eq!(crate::sys::acl_problem(&p), None, "{}", p.display());
        }
        std::fs::write(work.join("secret"), "s").unwrap();
        assert_eq!(crate::sys::acl_problem(&work.join("secret")), None);

        // A run folder that was there before, with the access list of an open folder.
        let other = dir.path().join("other");
        std::fs::create_dir_all(other.join("runs")).unwrap();
        let _ = std::process::Command::new("icacls")
            .arg(other.join("runs"))
            .args(["/grant", "*S-1-1-0:(OI)(CI)F"])
            .output()
            .unwrap();
        let cfg2 = Config::parse(&format!(
            "server_url = \"http://127.0.0.1:1\"\ntoken_file = '{}'\nwork_root = '{}'\n",
            dir.path().join("token").display(),
            other.display()
        ))
        .unwrap();
        let e = prepare(&cfg2, "r1", None).await.unwrap_err();
        assert!(e.contains("cannot be made private"), "{e}");
        assert!(!run_dir(&cfg2, "r1").exists(), "nothing is put into it");
    }

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
            // Windows: not asserted. Deleting (and making a folder) is done with backup intent,
            // which skips the access list for a process that holds the restore privilege, and
            // the CI runner is elevated. So this test does not show that a delete is refused
            // there; it is for a CLI of an ordinary user, which no test here runs as.
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

    fn sh_git(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A work root and a local repository with one commit on `main`.
    fn setup(keep_hours: u64) -> (tempfile::TempDir, Config, RepoSpec) {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir(&repo).unwrap();
        sh_git(&repo, &["init", "--quiet", "-b", "main"]);
        std::fs::write(repo.join("README"), "hi\n").unwrap();
        sh_git(&repo, &["add", "-A"]);
        sh_git(&repo, &["commit", "--quiet", "-m", "init"]);
        let cfg = Config::parse(&format!(
            "server_url = \"http://127.0.0.1:1\"\ntoken_file = '{}'\nwork_root = '{}'\n\
keep_runs_hours = {keep_hours}\nmin_free_gb = 0\n",
            dir.path().join("token").display(),
            dir.path().join("root").display()
        ))
        .unwrap();
        let spec = RepoSpec {
            url: None,
            path: Some(repo.to_string_lossy().to_string()),
            base_branch: Some("main".into()),
        };
        (dir, cfg, spec)
    }

    /// The code location of a line (design-relay R5): "pushed" is what the remote has after a
    /// fetch, not what the CLI did or what a stale remote-tracking ref says.
    #[tokio::test]
    async fn the_code_location_is_pushed_only_when_the_remote_has_it() {
        let (d, cfg, repo) = setup(72);
        let remote = d.path().join("remote.git");
        let r = remote.to_string_lossy().to_string();
        sh_git(d.path(), &["init", "--quiet", "--bare", &r]);
        let local = PathBuf::from(repo.path.clone().unwrap());
        sh_git(&local, &["remote", "add", "origin", &r]);
        sh_git(&local, &["push", "--quiet", "origin", "main"]);

        let (w, m, g) = prepare_line(&cfg, "r1", "BOT1", "T1", Some(&repo))
            .await
            .unwrap()
            .unwrap();
        let branch = m.branch.clone().unwrap();
        // A new branch the remote does not have: not pushed.
        let c = code_location(&w, None).await.unwrap();
        assert_eq!(c.branch, branch);
        assert_eq!(c.head, sh_git(&w, &["rev-parse", "HEAD"]));
        assert!(!c.pushed);

        std::fs::write(w.join("done.txt"), "x").unwrap();
        sh_git(&w, &["add", "done.txt"]);
        sh_git(&w, &["commit", "--quiet", "-m", "work"]);
        // A remote-tracking ref that says so is not enough: the fetch finds no such branch.
        sh_git(
            &w,
            &[
                "update-ref",
                &format!("refs/remotes/origin/{branch}"),
                "HEAD",
            ],
        );
        assert!(!code_location(&w, None).await.unwrap().pushed);

        sh_git(&w, &["push", "--quiet", "origin", &branch]);
        let c = code_location(&w, None).await.unwrap();
        assert!(c.pushed);
        assert_eq!(c.head, sh_git(&w, &["rev-parse", "HEAD"]));

        // Changed tracked files are not on the remote.
        std::fs::write(w.join("done.txt"), "y").unwrap();
        assert!(!code_location(&w, None).await.unwrap().pushed);
        sh_git(&w, &["checkout", "--quiet", "--", "done.txt"]);
        // An untracked file does not count.
        std::fs::write(w.join("scratch.log"), "x").unwrap();
        assert!(code_location(&w, None).await.unwrap().pushed);

        // A commit after the push: not pushed.
        sh_git(&w, &["commit", "--quiet", "--allow-empty", "-m", "more"]);
        assert!(!code_location(&w, None).await.unwrap().pushed);
        // A remote that cannot be reached: not pushed either.
        sh_git(&w, &["push", "--quiet", "origin", &branch]);
        std::fs::rename(&remote, d.path().join("gone.git")).unwrap();
        let c = code_location(&w, None).await.unwrap();
        assert!(!c.pushed);
        std::fs::rename(d.path().join("gone.git"), &remote).unwrap();

        // A detached HEAD reports the branch it is given.
        sh_git(&w, &["checkout", "--quiet", "--detach"]);
        let c = code_location(&w, Some(&branch)).await.unwrap();
        assert_eq!(c.branch, branch);
        assert!(c.pushed);
        assert!(code_location(&w, None).await.is_none());
        drop(g);

        // Not a git repository: no code location.
        let plain = tempfile::tempdir().unwrap();
        assert!(code_location(plain.path(), Some("main")).await.is_none());
    }

    /// The next run of a member in a topic works in the folder and on the branch of the last
    /// one: uncommitted and committed changes are both there. Another topic, another member
    /// or another repository is another line.
    #[tokio::test]
    async fn the_next_run_in_a_topic_finds_the_last_runs_work() {
        let (_d, cfg, repo) = setup(72);
        let (w1, m1, g1) = prepare_line(&cfg, "r1", "BOT1", "T1", Some(&repo))
            .await
            .unwrap()
            .unwrap();
        assert!(g1.fresh && !g1.restored);
        assert_eq!(read_meta(&run_dir(&cfg, "r1")).unwrap().line, m1.line);
        std::fs::write(w1.join("done.txt"), "committed").unwrap();
        sh_git(&w1, &["add", "done.txt"]);
        sh_git(&w1, &["commit", "--quiet", "-m", "first"]);
        std::fs::write(w1.join("draft.txt"), "uncommitted").unwrap();
        drop(g1);

        let (w2, m2, g2) = prepare_line(&cfg, "r2", "BOT1", "T1", Some(&repo))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(w2, w1);
        assert_eq!(m2.branch, m1.branch);
        assert!(!g2.fresh);
        assert!(w2.join("done.txt").is_file());
        assert!(w2.join("draft.txt").is_file());
        assert!(run_dir(&cfg, "r2").join("run.json").is_file());
        assert!(!run_dir(&cfg, "r2").join("work").exists());

        // While r2 holds the line, a second run of the same member in the topic gets none.
        assert!(
            prepare_line(&cfg, "r3", "BOT1", "T1", Some(&repo))
                .await
                .unwrap()
                .is_none()
        );
        for (bot, topic, r) in [
            ("BOT1", "T2", Some(&repo)),
            ("BOT2", "T1", Some(&repo)),
            ("BOT1", "T1", None),
        ] {
            let (w, _, g) = prepare_line(&cfg, "rx", bot, topic, r)
                .await
                .unwrap()
                .unwrap();
            assert_ne!(w, w1);
            assert!(g.fresh);
            drop(g);
        }
        drop(g2);
        let line = read_line(&m1.line.clone().unwrap()).unwrap();
        assert_eq!(line.last_run_id.as_deref(), Some("r2"));

        // Removing a run that used the line leaves the line.
        assert!(remove_run(&cfg, &run_dir(&cfg, "r1")).await);
        assert!(!run_dir(&cfg, "r1").exists());
        assert!(w1.join("draft.txt").is_file());
    }

    /// An idle line is cleaned up only after `keep_runs_hours`, and its uncommitted changes
    /// are committed to its branch first; the next run checks that branch out again.
    #[tokio::test]
    async fn cleanup_keeps_a_lines_uncommitted_work_on_its_branch() {
        let (_d, cfg, repo) = setup(1);
        let (w, m, g) = prepare_line(&cfg, "r1", "BOT1", "T1", Some(&repo))
            .await
            .unwrap()
            .unwrap();
        std::fs::write(w.join("draft.txt"), "uncommitted").unwrap();
        drop(g);
        // Used just now: kept.
        assert_eq!(cleanup_lines(&cfg, &[]).await, 0);
        assert!(w.join("draft.txt").is_file());
        // A run holds it: kept, also when idle.
        let line = m.line.clone().unwrap();
        let mut lm = read_line(&line).unwrap();
        lm.last_used_at = now_ms() - 2 * 3_600_000;
        write_line(&line, &lm);
        assert!(claim_line(&line, false).is_ok());
        assert_eq!(cleanup_lines(&cfg, &[]).await, 0);
        release_line(&line);
        // Idle for longer than keep_runs_hours: committed, then removed; the branch stays.
        assert_eq!(cleanup_lines(&cfg, &[]).await, 1);
        assert!(!line.exists());
        let branch = m.branch.clone().unwrap();
        let repo_dir = PathBuf::from(repo.path.clone().unwrap());
        let files = sh_git(&repo_dir, &["ls-tree", "--name-only", &branch]);
        assert!(files.lines().any(|f| f == "draft.txt"), "{files}");

        let (w2, _, g2) = prepare_line(&cfg, "r2", "BOT1", "T1", Some(&repo))
            .await
            .unwrap()
            .unwrap();
        assert!(g2.fresh && g2.restored);
        assert_eq!(
            std::fs::read_to_string(w2.join("draft.txt")).unwrap(),
            "uncommitted"
        );
    }

    /// A line with a draft in its worktree, idle for two hours.
    async fn idle_line_with_draft(cfg: &Config, repo: &RepoSpec) -> (PathBuf, PathBuf, String) {
        let (w, m, g) = prepare_line(cfg, "r1", "BOT1", "T1", Some(repo))
            .await
            .unwrap()
            .unwrap();
        std::fs::write(w.join("draft.txt"), "uncommitted").unwrap();
        drop(g);
        let line = m.line.clone().unwrap();
        let mut lm = read_line(&line).unwrap();
        lm.last_used_at = now_ms() - 2 * 3_600_000;
        write_line(&line, &lm);
        (w, line, m.branch.unwrap())
    }

    /// A git killed in the middle of a command (a cancelled run) leaves `index.lock`: the
    /// cleanup removes it and still commits the line's uncommitted work.
    #[tokio::test]
    async fn cleanup_gets_past_a_stale_index_lock() {
        let (_d, cfg, repo) = setup(1);
        let (w, line, branch) = idle_line_with_draft(&cfg, &repo).await;
        let lock = PathBuf::from(sh_git(
            &w,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                "index.lock",
            ],
        ));
        std::fs::write(&lock, "").unwrap();
        assert_eq!(cleanup_lines(&cfg, &[]).await, 1);
        assert!(!line.exists());
        let repo_dir = PathBuf::from(repo.path.clone().unwrap());
        let files = sh_git(&repo_dir, &["ls-tree", "--name-only", &branch]);
        assert!(files.lines().any(|f| f == "draft.txt"), "{files}");
    }

    /// When the work cannot be committed at all, the folder is moved to `kept` with its files
    /// and the line is still cleaned up; `kept` is emptied only when disk is low.
    #[tokio::test]
    async fn work_that_cannot_be_committed_is_moved_aside() {
        let (_d, cfg, repo) = setup(1);
        let (w, line, _) = idle_line_with_draft(&cfg, &repo).await;
        // A lock that is a folder: nothing removes it, every commit fails.
        let lock = PathBuf::from(sh_git(
            &w,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                "index.lock",
            ],
        ));
        std::fs::create_dir(&lock).unwrap();
        assert_eq!(cleanup_lines(&cfg, &[]).await, 1);
        assert!(!line.exists());
        let kept: Vec<_> = std::fs::read_dir(kept_dir(&cfg))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
        assert_eq!(kept.len(), 1);
        assert!(
            kept[0]
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("line-")
        );
        assert_eq!(
            std::fs::read_to_string(kept[0].join("draft.txt")).unwrap(),
            "uncommitted"
        );
        // min_free_gb = 0: disk is never low, kept stays.
        assert_eq!(cleanup_kept(&cfg).await, 0);
        assert!(kept[0].exists());
        let _ = std::fs::remove_dir(&lock);
    }

    /// A run that meets the cleanup's hold on its line waits for it and then takes the line,
    /// instead of getting a folder of its own.
    #[tokio::test]
    async fn a_run_waits_for_the_cleanups_hold() {
        let (_d, cfg, repo) = setup(72);
        let line = lines_dir(&cfg).join(line_key("BOT1", "T1", Some(&repo)));
        assert!(claim_line(&line, true).is_ok());
        let held = line.clone();
        let release = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(500)).await;
            release_line(&held);
        });
        let (w, _, g) = prepare_line(&cfg, "r1", "BOT1", "T1", Some(&repo))
            .await
            .unwrap()
            .expect("the line, after the cleanup let go");
        assert!(w.starts_with(&line));
        release.await.unwrap();
        drop(g);
    }

    /// A line folder git cannot use is moved aside with its files when a run starts, not
    /// deleted.
    #[tokio::test]
    async fn an_unusable_line_folder_is_moved_aside() {
        let (_d, cfg, repo) = setup(72);
        let key = line_key("BOT1", "T1", Some(&repo));
        let work = lines_dir(&cfg).join(&key).join("work");
        std::fs::create_dir_all(&work).unwrap();
        std::fs::write(work.join(".git"), "gitdir: /nowhere\n").unwrap();
        std::fs::write(work.join("draft.txt"), "x").unwrap();
        let (w, _, g) = prepare_line(&cfg, "r1", "BOT1", "T1", Some(&repo))
            .await
            .unwrap()
            .unwrap();
        assert!(g.fresh);
        assert!(!w.join("draft.txt").exists());
        let kept: Vec<_> = std::fs::read_dir(kept_dir(&cfg))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
        assert_eq!(kept.len(), 1);
        let name = kept[0].file_name().unwrap().to_string_lossy().to_string();
        assert!(name.starts_with(&format!("line-{key}-lost-")), "{name}");
        assert!(kept[0].join("draft.txt").is_file());
    }

    /// A line without a repository is a scratch folder that is kept between runs.
    #[tokio::test]
    async fn a_scratch_line_is_kept_between_runs() {
        let (_d, cfg, _) = setup(72);
        let (w, _, g) = prepare_line(&cfg, "r1", "BOT9", "T9", None)
            .await
            .unwrap()
            .unwrap();
        assert!(g.fresh);
        std::fs::write(w.join("notes.md"), "x").unwrap();
        drop(g);
        let (w2, _, g2) = prepare_line(&cfg, "r2", "BOT9", "T9", None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(w2, w);
        assert!(!g2.fresh);
        assert!(w2.join("notes.md").is_file());
    }

    /// A crash leaves `chat-run` request folders in a line's folder too: they go when their run
    /// is not running.
    #[test]
    fn spools_left_in_a_line_are_swept() {
        let (_d, cfg, _) = setup(72);
        let work = lines_dir(&cfg).join("k").join("work");
        let left = crate::chat_run::spool_dir(&work, "dead1");
        let live = crate::chat_run::spool_dir(&work, "live1");
        for d in [&left, &live] {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(d.join(".gitignore"), "*\n").unwrap();
        }
        std::fs::create_dir_all(work.join("src")).unwrap();
        assert_eq!(sweep_spools(&cfg, &["live1".to_string()]), 1);
        assert!(!left.exists());
        assert!(live.exists());
        assert!(work.join("src").exists());
    }

    /// Removing a run with its own worktree commits its uncommitted changes to the run's
    /// branch, which is then kept (its commit is nowhere else).
    #[tokio::test]
    async fn removing_a_run_keeps_its_uncommitted_changes() {
        let (_d, cfg, repo) = setup(72);
        let (w, meta) = prepare(&cfg, "r1", Some(&repo)).await.unwrap();
        std::fs::write(w.join("draft.txt"), "uncommitted").unwrap();
        assert!(remove_run(&cfg, &run_dir(&cfg, "r1")).await);
        assert!(!run_dir(&cfg, "r1").exists());
        let repo_dir = PathBuf::from(repo.path.clone().unwrap());
        let branch = meta.branch.unwrap();
        let files = sh_git(&repo_dir, &["ls-tree", "--name-only", &branch]);
        assert!(files.lines().any(|f| f == "draft.txt"), "{files}");

        // A run without changes: its branch goes with it.
        let (_, meta) = prepare(&cfg, "r2", Some(&repo)).await.unwrap();
        assert!(remove_run(&cfg, &run_dir(&cfg, "r2")).await);
        let b = meta.branch.unwrap();
        assert_eq!(sh_git(&repo_dir, &["branch", "--list", &b]), "");
    }
}
