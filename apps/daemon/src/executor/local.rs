//! `local`: the CLI runs as a child process of the daemon, in its own process group, as the
//! daemon's user. Tools that leave the CLI's tree are found by [`crate::proctree`].

use super::{BoxFuture, Executor, Process, Spawned, Stopped};
use crate::proctree;
use crate::runtime::RunSpec;
use crate::workspace::{self, RunMeta};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Child;

#[derive(Debug)]
pub struct Local;

impl Executor for Local {
    fn spawn(&self, spec: &RunSpec, program: &str, args: &[String]) -> std::io::Result<Spawned> {
        let stderr = std::fs::File::create(spec.run_dir.join("stderr.log"))?;
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args)
            .current_dir(&spec.cwd)
            .env_clear()
            .envs(spec.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(stderr))
            .process_group(0)
            .kill_on_drop(true);
        let mut child = cmd.spawn()?;
        let pgid = child.id().map(|p| p as i32).unwrap_or(0);
        workspace::record_process(&spec.run_dir, pgid, program);
        let stdin = child.stdin.take().map(|s| Box::new(s) as super::Stdin);
        let stdout = Box::new(child.stdout.take().expect("piped stdout"));
        Ok(Spawned {
            stdin,
            stdout,
            process: Box::new(LocalProcess {
                child,
                pgid,
                run_dir: spec.run_dir.clone(),
            }),
        })
    }

    fn stop(&self, run_dir: &Path) -> Option<Box<dyn Stopped>> {
        let pgid = workspace::read_meta(run_dir)?.pgid.filter(|p| *p > 1)?;
        let mut tools = vec![];
        proctree::stop(pgid, run_dir, &mut tools);
        Some(Box::new(LocalStopped { pgid, tools }))
    }

    fn tools_cpu(&self, run_dir: &Path) -> Option<std::collections::HashMap<i32, u64>> {
        let pgid = workspace::read_meta(run_dir)?.pgid.filter(|p| *p > 1)?;
        proctree::tools_cpu(pgid)
    }

    /// A pid can be reused, so a live group leader must still name the CLI program; while any
    /// member of the group lives, its id is not reused.
    fn kill_leftover(&self, meta: &RunMeta) {
        let Some(pgid) = meta.pgid.filter(|p| *p > 1) else {
            return;
        };
        // SAFETY: signal 0 only probes whether the group exists.
        if unsafe { libc::kill(-pgid, 0) } != 0 {
            return;
        }
        // SAFETY: probe of the leader pid.
        if unsafe { libc::kill(pgid, 0) } == 0 {
            let name = meta
                .program
                .as_deref()
                .and_then(|p| Path::new(p).file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let cmd = std::process::Command::new("ps")
                .args(["-p", &pgid.to_string(), "-o", "command="])
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                .unwrap_or_default();
            if name.is_empty() || !cmd.contains(&name) {
                return;
            }
        }
        eprintln!(
            "{} run {}: stopping CLI left by the previous daemon",
            crate::proto::now_ms(),
            meta.run_id
        );
        // SAFETY: plain syscall; a negative pid addresses the process group.
        unsafe {
            libc::kill(-pgid, libc::SIGKILL);
        }
    }
}

struct LocalStopped {
    pgid: i32,
    tools: Vec<proctree::Target>,
}

impl Stopped for LocalStopped {
    fn resume(&self) {
        proctree::resume(self.pgid, &self.tools);
    }
}

/// A CLI child in its own process group.
struct LocalProcess {
    child: Child,
    pgid: i32,
    run_dir: PathBuf,
}

impl LocalProcess {
    /// The tool groups below the CLI (see [`crate::proctree`]) and the run's orphans (tools
    /// that daemonized). Only orphans once the CLI is reaped: its pid may then belong to another
    /// process.
    fn tool_targets(&mut self) -> Vec<proctree::Target> {
        match self.child.try_wait() {
            Ok(None) => proctree::run_targets(self.pgid, &self.run_dir),
            _ => proctree::orphans(&self.run_dir),
        }
    }

    async fn kill_group(&mut self, grace: Duration) -> Option<i64> {
        let mut tools = self.tool_targets();
        self.signal(libc::SIGTERM);
        proctree::signal(&tools, libc::SIGTERM);
        // A stopped tool cannot handle SIGTERM.
        proctree::signal(&tools, libc::SIGCONT);
        match tokio::time::timeout(grace, self.child.wait()).await {
            Ok(Ok(s)) => {
                self.signal(libc::SIGKILL);
                proctree::signal(&tools, libc::SIGKILL);
                s.code().map(i64::from)
            }
            _ => {
                for t in self.tool_targets() {
                    if !tools.contains(&t) {
                        tools.push(t);
                    }
                }
                self.signal(libc::SIGKILL);
                proctree::signal(&tools, libc::SIGKILL);
                self.child
                    .wait()
                    .await
                    .ok()
                    .and_then(|s| s.code().map(i64::from))
            }
        }
    }
}

impl Process for LocalProcess {
    fn signal(&self, sig: i32) {
        if self.pgid > 0 {
            // SAFETY: plain syscall; a negative pid addresses the child's process group.
            unsafe {
                libc::kill(-self.pgid, sig);
            }
        }
    }

    fn kill(&mut self, grace: Duration) -> BoxFuture<'_, Option<i64>> {
        Box::pin(self.kill_group(grace))
    }

    fn finish(&mut self, limit: Duration) -> BoxFuture<'_, Option<i64>> {
        Box::pin(async move {
            match tokio::time::timeout(limit, self.child.wait()).await {
                Ok(Ok(s)) => {
                    // Remove helpers the CLI left in its group.
                    self.signal(libc::SIGKILL);
                    s.code().map(i64::from)
                }
                _ => self.kill_group(Duration::from_secs(3)).await,
            }
        })
    }

    fn kill_now(&mut self) {
        let tools = self.tool_targets();
        proctree::signal(&tools, libc::SIGKILL);
        self.signal(libc::SIGKILL);
    }
}
