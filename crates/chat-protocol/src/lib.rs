//! Wire protocol types, version 1. The contract is docs/protocol.md; the Zod schemas in
//! packages/protocol are the executable source of truth.
//!
//! Conventions that match the Zod schemas:
//! - Field names are camelCase. Unknown fields are ignored on read and dropped on write.
//! - Optional fields are `Option<T>`: omitted when `None`, and `null` is rejected on read.
//! - Optional nullable fields (`x?: T | null`) are `Option<Option<T>>`: `None` = omitted,
//!   `Some(None)` = `null`, `Some(Some(v))` = a value. This keeps `null` on a round trip.
//! - Frames are tagged on `t`, events on `type`. An unknown tag fails to parse.
//!
//! Parse a server frame with [`parse_server_frame`]; encode a client frame with
//! [`encode_client_frame`], which runs [`ClientFrame::validate`] first.

pub mod entities;
pub mod errors;
pub mod events;
pub mod frames;
pub mod http;
pub mod limits;
mod serde_util;

pub use entities::*;
pub use errors::*;
pub use events::*;
pub use frames::*;
pub use http::*;
pub use limits::*;
pub use serde_util::{ProtocolV1, True};

pub const PROTOCOL_VERSION: u32 = 1;
