//! 产出: cards for 产出 references in messages ([`extra::ArtifactRefsExtra`]),
//! the native 产出 window ([`window`]), and the side panel list with uploads
//! from the app ([`panel::TopicOutputs`]).

pub mod extra;
pub mod panel;
pub mod window;

use gpui::{Pixels, px};

/// Size of a 产出 card in the timeline (fixed, so the row height is stable).
pub const CARD_W: Pixels = px(360.);
pub const CARD_H: Pixels = px(52.);
