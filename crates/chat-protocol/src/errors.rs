//! Error codes and bodies shared by WebSocket frames and HTTP responses.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Unauthorized,
    Forbidden,
    NotFound,
    Invalid,
    Conflict,
    RateLimited,
    TooLarge,
    UnsupportedProtocol,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
    /// 1.1: the specific reason, e.g. `card.already_answered` (no new `ErrorCode` values).
    #[serde(
        default,
        deserialize_with = "crate::serde_util::non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub reason: Option<String>,
    /// 1.1, `rate_limited`: wait at least this long (ms) before the next try.
    #[serde(
        default,
        deserialize_with = "crate::serde_util::non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub retry_after_ms: Option<u64>,
}

/// A frame that could not be parsed or encoded. Codes match `parseFrame` in packages/protocol:
/// `too_large` for a frame over 1 MiB, `invalid` otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameError {
    pub code: ErrorCode,
    pub message: String,
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for FrameError {}
