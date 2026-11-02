//! JSON reports: one file per scenario and a summary per run.

use serde::Serialize;
use serde_json::{Map, Value};
use std::path::Path;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub run_id: String,
    pub scenario: String,
    pub base_url: String,
    pub passed: bool,
    pub started_at: u64,
    pub duration_ms: u64,
    pub error: Option<String>,
    pub details: Vec<String>,
    pub metrics: Map<String, Value>,
}

pub fn write(dir: &Path, name: &str, v: &impl Serialize) -> std::io::Result<std::path::PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(name);
    std::fs::write(&path, serde_json::to_vec_pretty(v)?)?;
    Ok(path)
}
