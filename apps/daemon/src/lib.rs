//! Machine daemon: connects out to the server, runs coding CLIs for bot members and streams
//! normalized progress back. See docs/machine-protocol.md.

pub mod acceptance;
pub mod accounts;
pub mod chat_run;
pub mod chat_run_context;
pub mod chat_run_files;
pub mod config;
pub mod daemon;
pub mod doctor;
pub mod executor;
pub mod gates;
pub mod health;
pub mod input;
pub mod metrics;
pub mod models;
#[cfg(unix)]
pub mod proctree;
#[cfg(not(unix))]
#[path = "proctree_windows.rs"]
pub mod proctree;
pub mod product;
pub mod proto;
pub mod redact;
pub mod run;
pub mod runtime;
pub mod secrets;
pub mod selfupdate;
pub mod service;
pub mod sys;
pub mod upload;
pub mod usage;
pub mod vm_agent;
#[cfg(windows)]
pub mod winacl;
#[cfg(windows)]
pub mod winjob;
#[cfg(windows)]
pub mod winpty;
pub mod wintrust;
pub mod workspace;
