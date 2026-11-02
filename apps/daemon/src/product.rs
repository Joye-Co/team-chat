//! Product identifiers from the repository's product.json (never typed in code).

use std::sync::OnceLock;

const PRODUCT_JSON: &str = include_str!("../../../product.json");

fn product() -> &'static serde_json::Value {
    static P: OnceLock<serde_json::Value> = OnceLock::new();
    P.get_or_init(|| serde_json::from_str(PRODUCT_JSON).unwrap_or(serde_json::Value::Null))
}

pub fn slug() -> &'static str {
    product()["slug"].as_str().unwrap_or("chat")
}

pub fn bundle_id() -> &'static str {
    product()["bundleId"].as_str().unwrap_or("chat")
}

/// launchd label of the daemon service.
pub fn service_label() -> String {
    format!("{}.daemon", bundle_id())
}
