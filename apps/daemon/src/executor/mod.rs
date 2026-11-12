//! Where a run's CLI executes.
//!
//! The runtimes speak each CLI's protocol over stdin and stdout and never start a process
//! themselves: they ask the run's [`Executor`]. `local` starts the CLI as a child process on
//! this machine. A backend that runs each CLI in its own VM plugs in here, without changes to
//! the runtimes, the run supervisor or the machine protocol.

#[cfg(unix)]
pub mod firecracker;
#[cfg(unix)]
pub mod local;
// The same `Local`, with a Job Object in place of process groups.
#[cfg(windows)]
#[path = "local_windows.rs"]
pub mod local;
pub mod vmwire;

use crate::runtime::RunSpec;
use crate::workspace::RunMeta;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type Stdin = Box<dyn AsyncWrite + Send + Unpin>;
pub type Stdout = Box<dyn AsyncRead + Send + Unpin>;

pub trait Executor: Send + Sync + std::fmt::Debug {
    /// Starts `program` with `args` for the run in `spec` (its working folder, environment and
    /// run folder). The CLI's stderr goes to `stderr.log` in the run folder.
    fn spawn(&self, spec: &RunSpec, program: &str, args: &[String]) -> std::io::Result<Spawned>;

    /// Stops the run's CLI and the tools it started (接管). None while the CLI has not
    /// started yet: the caller asks again.
    fn stop(&self, run_dir: &Path) -> Option<Box<dyn Stopped>>;

    /// Ends what a daemon that crashed left running for an unfinished run.
    fn kill_leftover(&self, meta: &RunMeta);

    /// 盯 1.4: the CPU time each process the run's CLI started has used, by pid (see
    /// [`crate::proctree::tools_cpu`]). None when it is not known (the CLI is not running, or
    /// it runs where the daemon cannot see its processes).
    fn tools_cpu(&self, _run_dir: &Path) -> Option<std::collections::HashMap<i32, u64>> {
        None
    }

    /// The CLI runs apart from this machine's processes (in a VM): it reaches `chat-run`
    /// only through the relayed socket, never through files the daemon watches.
    fn isolated(&self) -> bool {
        false
    }

    /// How a person's commands run while they hold the run (接管, feature "takeover"):
    /// [`TAKEOVER_COMMANDS`] as the daemon's user, [`TAKEOVER_ISOLATED`] as another user than
    /// the run, None when they cannot run at all.
    fn takeover(&self) -> Option<&'static str> {
        None
    }

    /// Starts one command of the person holding the run, in its work folder, without the
    /// run's secrets, `chat-run` or the gate shims (design-takeover 3.4).
    fn hand(&self, _req: &HandReq) -> std::io::Result<HandChild> {
        Err(std::io::ErrorKind::Unsupported.into())
    }

    /// The person handed the run back: what their commands made is the run's again (in a VM,
    /// files they made go back to the run's user and their temporary home is removed).
    fn hand_back(&self, _run_dir: &Path) {}

    /// The run is over: ends what the person's commands left running.
    fn hand_end(&self, _run_dir: &Path) {}
}

/// [`Executor::takeover`] of a machine whose person's commands run as the daemon's user.
pub const TAKEOVER_COMMANDS: &str = "commands";
/// [`Executor::takeover`] of a VM: the person's commands run as another user than the run.
pub const TAKEOVER_ISOLATED: &str = "isolated";

/// One command of the person holding a run.
#[derive(Debug, Clone)]
pub struct HandReq {
    pub run_dir: std::path::PathBuf,
    /// The run's work folder: every command starts there.
    pub cwd: std::path::PathBuf,
    pub command: String,
    /// The command's environment on this machine (a VM makes its own).
    pub env: Vec<(String, String)>,
}

/// A started command of a person: its output (stdout and stderr together), its end and the
/// handle that ends its process group.
pub struct HandChild {
    pub output: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    pub done: tokio::sync::oneshot::Receiver<HandEnd>,
    pub cancel: Box<dyn Fn() + Send + Sync>,
}

/// How a person's command ended.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HandEnd {
    /// The exit code; None when a signal ended it or it did not start.
    pub code: Option<i64>,
    /// The machine cannot run commands for this run (in a VM: the work folder could not be
    /// opened to the person's user).
    pub unsupported: bool,
    /// Why it did not start.
    pub error: Option<String>,
    /// Output bytes the executor counted but never put on `output` (in a VM: what the agent
    /// dropped past its cap, less the last bytes it sent at the end).
    pub skipped: u64,
}

/// Reads `r` on a thread of its own and sends what it reads to `tx`, until the pipe ends or
/// nobody takes it. A process the command left in the background can keep the pipe open; the
/// reader then outlives the command and what it reads goes nowhere.
pub fn pipe_reader(
    mut r: impl std::io::Read + Send + 'static,
    tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
) {
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 16 << 10];
        loop {
            match r.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
}

/// A started CLI: its pipes and the handle that ends it.
pub struct Spawned {
    pub stdin: Option<Stdin>,
    pub stdout: Stdout,
    pub process: Box<dyn Process>,
}

/// The running CLI and everything it started.
pub trait Process: Send {
    /// Sends `sig` to the CLI's process group. Windows has no signals: 9 ends the CLI and
    /// its tools, any other number asks the CLI to stop.
    fn signal(&self, sig: i32);
    /// SIGTERM the CLI and its tools, SIGKILL after `grace`. Returns the exit code.
    fn kill(&mut self, grace: Duration) -> BoxFuture<'_, Option<i64>>;
    /// Waits up to `limit` for the CLI to exit by itself, then kills it. Returns the exit code.
    fn finish(&mut self, limit: Duration) -> BoxFuture<'_, Option<i64>>;
    /// Kills the CLI and its tools now, without waiting (the run is being dropped).
    fn kill_now(&mut self);
}

/// A run stopped for 接管.
pub trait Stopped: Send {
    /// Lets the CLI and its tools continue.
    fn resume(&self);
}

/// The executor for this machine.
pub fn local() -> Arc<dyn Executor> {
    Arc::new(local::Local)
}

/// A microVM per run when `[vm]` is configured, else the local executor.
pub fn for_config(cfg: &crate::config::Config) -> Arc<dyn Executor> {
    #[cfg(unix)]
    if let Some(vm) = &cfg.vm {
        return Arc::new(firecracker::Firecracker::new(
            vm.clone(),
            cfg.max_parallel_runs,
        ));
    }
    // Not on Windows: `Config::problems` refuses a `[vm]` section there.
    #[cfg(not(unix))]
    let _ = cfg;
    local()
}
