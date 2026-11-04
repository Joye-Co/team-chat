//! 产出: cards for 产出 references in messages ([`extra::ArtifactRefsExtra`]),
//! the native 产出 window ([`window`]), and the side panel list with uploads
//! from the app ([`panel::TopicOutputs`]).

pub mod embed_preview;
pub mod extra;
// --- inc-host track ---
pub mod host;
pub mod host_preview;
// --- end inc-host track ---
pub mod panel;
// The 产出 page states no other scene reaches (embed failures, the page ref toast).
pub mod state_scenes;
pub mod window;

use gpui::{Pixels, px};

/// Size of a 产出 card in the timeline: a file row (§7.11) at the one
/// measure, fixed height so the row height is stable.
pub const CARD_W: Pixels = px(620.);
pub const CARD_H: Pixels = px(64.);
