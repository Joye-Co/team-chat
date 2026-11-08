//! Where a run's CLI executes.
//!
//! The runtimes speak each CLI's protocol over stdin and stdout and never start a process
//! themselves: they ask the run's [`Executor`]. `local` starts the CLI as a child process on
//! this machine. A backend that runs each CLI in its own VM plugs in here, without changes to
//! the runtimes, the run supervisor or the machine protocol.

pub mod local;

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
}

/// A started CLI: its pipes and the handle that ends it.
pub struct Spawned {
    pub stdin: Option<Stdin>,
    pub stdout: Stdout,
    pub process: Box<dyn Process>,
}

/// The running CLI and everything it started.
pub trait Process: Send {
    /// Sends `sig` to the CLI's process group.
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
