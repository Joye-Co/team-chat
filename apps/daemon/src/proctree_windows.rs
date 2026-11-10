//! The process tree of a run on Windows is a Job Object ([`crate::winjob`]): the CLI and
//! everything it starts are in it, so nothing has to be found by walking the process table
//! the way the Unix module of this name does.

/// The variable that marks every process of a run: the daemon gives the CLI
/// `CHAT_RUN_FOLDER=<run folder>` (unique on the machine, unlike the run id across work roots),
/// and its tools inherit it.
pub const ENV_FOLDER: &str = "CHAT_RUN_FOLDER";
