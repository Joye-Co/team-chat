//! Machine daemon: connects out to the server, runs coding CLIs for bot members and streams
//! normalized progress back. See docs/machine-protocol.md.

pub mod acceptance;
pub mod chat_run;
pub mod config;
pub mod daemon;
pub mod doctor;
pub mod health;
pub mod input;
pub mod proctree;
pub mod product;
pub mod proto;
pub mod redact;
pub mod run;
pub mod runtime;
pub mod service;
pub mod upload;
pub mod workspace;
