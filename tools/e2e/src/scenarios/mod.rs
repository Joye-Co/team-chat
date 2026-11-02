//! The scenario registry. One file per scenario; add a new one with a `pub const SCENARIO`
//! and one line in `registry()`.

use crate::harness::Harness;
use anyhow::Result;
use serde_json::{Map, Value};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

mod catchup;
mod chat;
mod gap;
mod idempotent;
mod load;
mod permissions;
mod pipeline;
mod soak;
mod streaming;

pub type Run<'a> = Pin<Box<dyn Future<Output = Result<()>> + 'a>>;

pub struct Scenario {
    pub name: &'static str,
    pub about: &'static str,
    /// Part of `--scenario all`. Soak runs there only when `--minutes` is given.
    pub in_all: bool,
    /// Upper bound on the run time, in seconds, before `--minutes`.
    pub timeout_secs: u64,
    pub run: for<'a> fn(&'a mut Ctx) -> Run<'a>,
}

pub fn registry() -> Vec<Scenario> {
    vec![
        chat::SCENARIO,
        catchup::SCENARIO,
        idempotent::SCENARIO,
        gap::SCENARIO,
        streaming::SCENARIO,
        permissions::SCENARIO,
        pipeline::SCENARIO,
        load::SCENARIO,
        soak::SCENARIO,
    ]
}

pub struct Ctx {
    pub h: Arc<Harness>,
    /// Soak length.
    pub minutes: f64,
    pub details: Vec<String>,
    pub metrics: Map<String, Value>,
}

impl Ctx {
    pub fn note(&mut self, s: impl Into<String>) {
        self.details.push(s.into());
    }
    pub fn metric(&mut self, k: &str, v: impl Into<Value>) {
        self.metrics.insert(k.into(), v.into());
    }
}

/// `anyhow::ensure!` with the scenario's wording.
#[macro_export]
macro_rules! check {
    ($cond:expr, $($arg:tt)+) => {
        if !$cond {
            anyhow::bail!($($arg)+);
        }
    };
}
