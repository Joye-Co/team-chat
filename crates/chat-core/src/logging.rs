//! Logging to a daily rolling file in `<data_dir>/logs`. Log ids, seqs, counts and error
//! codes only: never tokens, invite codes or message bodies.

use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;

/// Installs the global subscriber once per process. Keep the guard alive until exit so
/// buffered lines are written. `RUST_LOG` overrides the default filter `info`.
/// Returns `None` when a global subscriber is already set.
pub fn init(data_dir: &Path, file_prefix: &str) -> Option<WorkerGuard> {
    use tracing_subscriber::{EnvFilter, fmt, prelude::*};
    let dir = data_dir.join("logs");
    let _ = std::fs::create_dir_all(&dir);
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix(file_prefix)
        .filename_suffix("log")
        .max_log_files(14)
        .build(&dir)
        .ok()?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(writer).with_ansi(false))
        .try_init()
        .ok()?;
    Some(guard)
}
