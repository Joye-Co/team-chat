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

pub mod account;
pub mod activity;
pub mod audit;
pub mod bots;
pub mod card;
pub mod card2;
pub mod characters;
pub mod desk;
pub mod diagnostics;
pub mod diagram;
pub mod entities;
pub mod errors;
pub mod events;
pub mod files;
pub mod frames;
pub mod hooks;
pub mod http;
pub mod limits;
pub mod links;
pub mod machine;
pub mod media;
pub mod members;
pub mod memory;
pub mod message;
pub mod monitor;
pub mod push;
pub mod reactions;
pub mod reads;
pub mod roster;
pub mod search;
mod serde_util;
pub mod settings;
pub mod topic;
pub mod updates;
pub mod usage;

pub use account::*;
pub use activity::ActivityResponse;
pub use audit::*;
pub use bots::*;
pub use card::*;
pub use card2::*;
pub use characters::*;
pub use desk::*;
pub use diagnostics::*;
pub use diagram::*;
pub use entities::*;
pub use errors::*;
pub use events::*;
pub use files::*;
pub use frames::*;
pub use hooks::*;
pub use http::*;
pub use limits::*;
pub use links::*;
pub use media::*;
pub use members::*;
pub use memory::*;
pub use message::*;
pub use monitor::MachineMonitor;
pub use push::*;
pub use reactions::*;
pub use reads::*;
pub use roster::TeamRoster;
pub use search::*;
pub use serde_util::{ProtocolV1, True};
pub use settings::*;
pub use topic::*;
pub use updates::*;
pub use usage::TeamUsage;

// Usable: GitHub sign-in, relays and chains, run phase and take-over.
pub mod github;
pub mod relay;
pub mod takeover;
pub use github::*;
pub use relay::*;
pub use takeover::*;

pub const PROTOCOL_VERSION: u32 = 1;
/// `hello.features` of a 1.1 client: the interaction loop and refs to parts.
pub const CLIENT_FEATURES: [&str; 2] = ["interact/1", "refs/1"];
/// `welcome.features` of a 1.1 server; clients hide affordances the server does not list.
pub const SERVER_FEATURES: [&str; 3] = ["interact/1", "refs/1", "diagram/1"];
/// The feature that turns on the 1.1 transients for a connection.
pub const FEATURE_INTERACT: &str = "interact/1";
pub const FEATURE_REFS: &str = "refs/1";
pub const FEATURE_DIAGRAM: &str = "diagram/1";
