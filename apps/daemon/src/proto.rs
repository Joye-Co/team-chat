//! Machine protocol frames (docs/machine-protocol.md). The types are shared with the server
//! side through `chat_protocol::machine`; this module adds daemon helpers.

pub use chat_protocol::ProtocolV1;
pub use chat_protocol::bots::{MAX_PLAN_ITEMS, MAX_PLAN_TEXT_CHARS, RunPlanItem, RunPlanState};
pub use chat_protocol::machine::MachineErrorBody as ErrorBody;
pub use chat_protocol::machine::*;

/// Run ids become folder names, so only a safe character set is accepted.
pub fn valid_run_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Cut text to at most `max` bytes on a char boundary, marking the cut.
pub fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[... {} bytes cut]", &s[..end], s.len() - end)
}
