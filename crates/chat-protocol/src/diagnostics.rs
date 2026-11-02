//! Diagnostics upload (docs/protocol.md, "Diagnostics").

use serde::{Deserialize, Serialize};

/// Max gzip body of `POST /v1/diagnostics`.
pub const MAX_DIAGNOSTICS_BYTES: u64 = 5_242_880;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsResponse {
    pub id: String,
}
