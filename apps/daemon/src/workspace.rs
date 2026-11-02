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

pub fn read_meta(dir: &Path) -> Option<RunMeta> {
    serde_json::from_slice(&std::fs::read(dir.join("run.json")).ok()?).ok()
}

fn safe_ref(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && !s.contains("..")
        && s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'))
}

fn safe_url(u: &str) -> bool {
    (u.starts_with("https://") || u.starts_with("ssh://") || u.starts_with("git@"))
        && !u.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Checks an assignment's repo against the config before the run is accepted.
pub fn validate_repo(cfg: &Config, repo: &RepoSpec) -> Result<(), String> {
    if let Some(b) = &repo.base_branch {
        if !safe_ref(b) {
            return Err("invalid baseBranch".into());
        }
    }
    match (&repo.url, &repo.path) {
        (Some(_), Some(_)) => Err("repo has both url and path".into()),
        (Some(u), None) if !safe_url(u) => {
            Err("repo url must be https://, ssh:// or git@".into())
        }
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

static GIT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Makes `<run>/work`: a git worktree on a new branch `bot/<runId>`, or an empty folder.
pub async fn prepare(cfg: &Config, run_id: &str, repo: Option<&RepoSpec>) -> Result<(PathBuf, RunMeta), String> {
    let dir = run_dir(cfg, run_id);
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create run folder: {e}"))?;
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
        if mirror.join("HEAD").exists() {
            git(Some(&mirror), &["fetch", "--quiet", "--prune", "origin", "+refs/heads/*:refs/heads/*"]).await?;
        } else {
            std::fs::create_dir_all(cfg.work_root.join("repos")).map_err(|e| e.to_string())?;
            let m = mirror.to_string_lossy().to_string();
            git(None, &["clone", "--bare", "--quiet", "--", url, &m]).await?;
            git(Some(&mirror), &["config", "remote.origin.fetch", "+refs/heads/*:refs/heads/*"]).await?;
        }
        mirror
    } else {
        let p = PathBuf::from(repo.path.as_deref().unwrap_or_default());
        // Best effort: a local repo without a reachable remote still works.
        let _ = git(Some(&p), &["fetch", "--quiet", "origin"]).await;
        p
    };
    let start = match &repo.base_branch {
        Some(b) => {
            let remote = format!("refs/remotes/origin/{b}");
            if git(Some(&git_dir), &["rev-parse", "--verify", "--quiet", &remote]).await.is_ok() {
                format!("origin/{b}")
            } else {
                b.clone()
            }
        }
        None => "HEAD".into(),
    };
    let branch = format!("bot/{run_id}");
    let w = work.to_string_lossy().to_string();
    git(Some(&git_dir), &["worktree", "add", "--quiet", "-b", &branch, &w, &start]).await?;
    meta.git_dir = Some(git_dir);
    meta.branch = Some(branch);
    write_meta(&dir, &meta);
    Ok((work, meta))
}

/// Removes a finished run folder and its worktree registration.
pub async fn remove_run(dir: &Path) {
    if let Some(meta) = read_meta(dir) {
        if let Some(gd) = &meta.git_dir {
            let _guard = GIT_LOCK.lock().await;
            let w = dir.join("work").to_string_lossy().to_string();
            let _ = git(Some(gd), &["worktree", "remove", "--force", &w]).await;
            if let Some(b) = &meta.branch {
                let _ = git(Some(gd), &["branch", "-D", b]).await;
            }
            let _ = git(Some(gd), &["worktree", "prune"]).await;
        }
    }
    let _ = tokio::fs::remove_dir_all(dir).await;
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
        if at < cutoff || low_disk {
            remove_run(&path).await;
            removed += 1;
        }
    }
    removed
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
