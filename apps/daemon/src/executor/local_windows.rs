//! `local` on Windows: the CLI runs as a child process of the daemon, as the daemon's user,
//! inside a Job Object of its own ([`crate::winjob`]). Everything the CLI starts is in that
//! job, so ending the job ends the whole tree.
//!
//! - Cancel: `CTRL_BREAK_EVENT` to the CLI's process group, then the job is ended after the
//!   grace period (at once when the event cannot be sent: the daemon has no console).
//! - 接管 (pause): every thread of the job's processes is suspended, and resumed later.
//! - A daemon that crashes: the job is killed when its last handle closes and the daemon holds
//!   the only one, so no CLI outlives it and there is nothing left to clean up at the next
//!   start.

use super::{BoxFuture, Executor, HandChild, HandEnd, HandReq, Process, Spawned, Stopped};
use crate::runtime::RunSpec;
use crate::winjob::{self, Job};
use crate::workspace::{self, RunMeta};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::process::Child;

#[derive(Debug)]
pub struct Local;

/// The jobs of the running CLIs by run folder, for [`Executor::stop`].
static JOBS: Mutex<Option<HashMap<PathBuf, Arc<Job>>>> = Mutex::new(None);

fn jobs<T>(f: impl FnOnce(&mut HashMap<PathBuf, Arc<Job>>) -> T) -> T {
    let mut g = JOBS.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(HashMap::new))
}

/// A run's job that another module started (the pty runtime starts its program itself): while
/// the value lives, [`Executor::stop`] finds the job by the run folder.
pub struct Tracked(PathBuf);

impl Drop for Tracked {
    fn drop(&mut self) {
        jobs(|m| m.remove(&self.0));
    }
}

pub fn track(run_dir: &Path, job: Arc<Job>) -> Tracked {
    jobs(|m| m.insert(run_dir.to_path_buf(), job));
    Tracked(run_dir.to_path_buf())
}

impl Executor for Local {
    fn spawn(&self, spec: &RunSpec, program: &str, args: &[String]) -> std::io::Result<Spawned> {
        let stderr = std::fs::File::create(spec.run_dir.join("stderr.log"))?;
        // The CLI is looked up in the run's PATH, with the extensions of PATHEXT; an npm
        // `.cmd` shim is replaced by what it starts (see `health::launch`).
        let path = spec
            .env
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("PATH"))
            .map(|(_, v)| std::ffi::OsString::from(v));
        let found = crate::health::which_in(program, path.as_deref())
            .or_else(|| crate::health::which(program))
            .unwrap_or_else(|| PathBuf::from(program));
        let launch = crate::health::launch(&found);
        if let Some(why) = crate::health::batch_problem(&launch.program, args) {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, why));
        }
        let mut cmd = tokio::process::Command::new(&launch.program);
        cmd.args(&launch.args)
            .args(args)
            .current_dir(&spec.cwd)
            .env_clear()
            .envs(spec.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(stderr))
            .kill_on_drop(true);
        // In a job, and in a process group of its own for CTRL_BREAK_EVENT.
        let (mut child, job) = winjob::spawn(&mut cmd, true)?;
        let job = Arc::new(job);
        let pid = child.id().unwrap_or(0);
        workspace::record_process(&spec.run_dir, pid as i32, program);
        jobs(|m| m.insert(spec.run_dir.clone(), job.clone()));
        let stdin = child.stdin.take().map(|s| Box::new(s) as super::Stdin);
        let stdout = Box::new(child.stdout.take().expect("piped stdout"));
        Ok(Spawned {
            stdin,
            stdout,
            process: Box::new(LocalProcess {
                child,
                pid,
                job,
                run_dir: spec.run_dir.clone(),
            }),
        })
    }

    fn stop(&self, run_dir: &Path) -> Option<Box<dyn Stopped>> {
        let job = jobs(|m| m.get(run_dir).cloned())?;
        job.suspend();
        Some(Box::new(LocalStopped { job }))
    }

    /// Nothing to do: the job of a daemon that is gone was killed when its handle closed.
    fn kill_leftover(&self, _meta: &RunMeta) {}

    fn takeover(&self) -> Option<&'static str> {
        Some(super::TAKEOVER_COMMANDS)
    }

    /// `powershell.exe -NoLogo -NoProfile -NonInteractive` with the command encoded (no quoting
    /// rules to get wrong), in a job of its own: not in the CLI's suspended job, and ending the
    /// job ends what the command started.
    fn hand(&self, req: &HandReq) -> std::io::Result<HandChild> {
        let (r, w) = std::io::pipe()?;
        let w2 = w.try_clone()?;
        let mut cmd = tokio::process::Command::new("powershell.exe");
        cmd.args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-EncodedCommand",
            &encoded_command(&req.command),
        ])
        .current_dir(&req.cwd)
        .env_clear()
        .envs(req.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(w)
        .stderr(w2);
        let spawned = winjob::spawn(&mut cmd, true);
        // The pipe's write ends live in `cmd` until it is dropped: the output ends only then.
        drop(cmd);
        let (mut child, job) = spawned?;
        let job = Arc::new(job);
        hands(|m| m.entry(req.run_dir.clone()).or_default().push(job.clone()));
        let (tx, output) = tokio::sync::mpsc::unbounded_channel();
        super::pipe_reader(r, tx);
        let (done_tx, done) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let status = child.wait().await;
            let _ = done_tx.send(HandEnd {
                code: status.ok().and_then(|s| s.code().map(i64::from)),
                ..HandEnd::default()
            });
        });
        Ok(HandChild {
            output,
            done,
            cancel: Box::new(move || job.terminate()),
        })
    }

    /// Ending each command's job ends what it left running.
    fn hand_end(&self, run_dir: &Path) {
        for job in hands(|m| m.remove(run_dir)).unwrap_or_default() {
            job.terminate();
        }
    }
}

/// The jobs of the person's commands, by run folder, for [`Executor::hand_end`].
static HANDS: Mutex<Option<HashMap<PathBuf, Vec<Arc<Job>>>>> = Mutex::new(None);

fn hands<T>(f: impl FnOnce(&mut HashMap<PathBuf, Vec<Arc<Job>>>) -> T) -> T {
    let mut g = HANDS.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(HashMap::new))
}

/// `-EncodedCommand`: the script as base64 of UTF-16LE. Output in UTF-8, and no progress
/// records (they would come as CLIXML on a redirected stream).
pub fn encoded_command(command: &str) -> String {
    use base64::Engine;
    let script = format!(
        "$ProgressPreference = 'SilentlyContinue'; [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; {command}"
    );
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

struct LocalStopped {
    job: Arc<Job>,
}

impl Stopped for LocalStopped {
    fn resume(&self) {
        self.job.resume();
    }
}

/// A CLI child and its job.
struct LocalProcess {
    child: Child,
    pid: u32,
    job: Arc<Job>,
    run_dir: PathBuf,
}

impl LocalProcess {
    async fn kill_job(&mut self, grace: Duration) -> Option<i64> {
        // A suspended process cannot handle the event.
        self.job.resume();
        let asked = matches!(self.child.try_wait(), Ok(None)) && winjob::ctrl_break(self.pid);
        let code = if asked {
            match tokio::time::timeout(grace, self.child.wait()).await {
                Ok(Ok(s)) => Some(s),
                _ => None,
            }
        } else {
            None
        };
        // Ends the CLI when it did not stop by itself, and every tool it left.
        self.job.terminate();
        let status = match code {
            Some(s) => Some(s),
            None => self.child.wait().await.ok(),
        };
        status.and_then(|s| s.code().map(i64::from))
    }
}

impl Drop for LocalProcess {
    fn drop(&mut self) {
        jobs(|m| m.remove(&self.run_dir));
    }
}

impl Process for LocalProcess {
    /// Unix signal numbers: 9 (SIGKILL) ends the job; any other asks the CLI's process group
    /// to stop with `CTRL_BREAK_EVENT`.
    fn signal(&self, sig: i32) {
        if sig == 9 {
            self.job.terminate();
        } else {
            winjob::ctrl_break(self.pid);
        }
    }

    fn kill(&mut self, grace: Duration) -> BoxFuture<'_, Option<i64>> {
        Box::pin(self.kill_job(grace))
    }

    fn finish(&mut self, limit: Duration) -> BoxFuture<'_, Option<i64>> {
        Box::pin(async move {
            match tokio::time::timeout(limit, self.child.wait()).await {
                Ok(Ok(s)) => {
                    // Remove helpers the CLI left.
                    self.job.terminate();
                    s.code().map(i64::from)
                }
                _ => self.kill_job(Duration::from_secs(3)).await,
            }
        })
    }

    fn kill_now(&mut self) {
        self.job.terminate();
    }
}
