//! Member characters (docs/PRODUCT.md, "Bot members"): a member with a
//! character draws it instead of the monogram, with its live [`Look`]: the
//! status (idle, working, needs-input, success, error, offline) and the
//! emotion its face shows.
//!
//! The art is the Jojo & Friends avatar pack (CC BY 4.0), vendored in
//! `apps/desktop/assets/characters/` by `scripts/characters/sync.sh` and
//! embedded by `build.rs`: every character at each of the 16 emotions and
//! 7 sizes, plus offline.
//!
//! The pieces are small so the design layer can wrap them:
//! - [`image`]: character, look and size in, an `Img` out;
//! - [`status`]: the look of a bot from what the provider knows, and the
//!   [`StatusBoard`] global that avatars read;
//! - [`emotion`]: the 16 emotions and the default face of each status;
//! - [`assets`]: the bundled SVGs of the pack, served through the app's
//!   asset source.

pub mod assets;
pub mod emotion;
pub mod status;
pub mod tile;

use gpui::{Img, Pixels, Styled as _, img};

pub use chat_core::proto::CharacterId;
pub use emotion::{Emotion, Look};
pub use status::StatusTracker;

/// The status a character shows: the six statuses of the avatar pack.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CharacterStatus {
    #[default]
    Idle,
    Working,
    NeedsInput,
    Success,
    Error,
    Offline,
}

impl CharacterStatus {
    #[cfg(test)]
    pub const ALL: [CharacterStatus; 6] = [
        CharacterStatus::Idle,
        CharacterStatus::Working,
        CharacterStatus::NeedsInput,
        CharacterStatus::Success,
        CharacterStatus::Error,
        CharacterStatus::Offline,
    ];
}

/// The bundled sizes in points. Each file is drawn for its size (16 to 32 pt
/// use the library's simplified small-size art); GPUI rasterizes an SVG at
/// twice its own size, so every file is sharp on a Retina display.
pub const SIZES: [u32; 7] = [16, 20, 24, 32, 48, 64, 128];

/// The bundled size to draw at `size`: the smallest one that is not smaller,
/// or the largest.
pub fn bundled_size(size: Pixels) -> u32 {
    let want = f32::from(size).ceil().max(1.) as u32;
    SIZES
        .iter()
        .copied()
        .find(|&s| s >= want)
        .unwrap_or(SIZES[SIZES.len() - 1])
}

/// The character at `size` with `look` (a [`Look`], or a [`CharacterStatus`]
/// with its default face), without the lamp, bubble, ring or sleep marks up
/// to 64 pt. The character is its own shape: do not clip it to a circle.
pub fn image(character: CharacterId, look: impl Into<Look>, size: Pixels) -> Img {
    img(tile::clean_path(character, look.into(), size))
        .size(size)
        .flex_none()
}

/// Whether this build has the art of `character`. Every character of the pack
/// is bundled; a member whose art is missing draws its initial.
pub fn has_art(character: CharacterId) -> bool {
    assets::has_character(character.as_str())
}

/// Whether this build has any character art (the look pickers need it).
pub fn available() -> bool {
    assets::count() > 0
}

mod roster {
    include!(concat!(env!("OUT_DIR"), "/bot_roster.rs"));
}

/// The character the bot roster of the ops checkout (`roster/roster.json`, see `build.rs`) gives
/// a bot of this name, compared without case: the default look of a new bot member. `None` for
/// every name in a build without the roster.
pub fn roster_character(name: &str) -> Option<CharacterId> {
    let name = name.trim().to_lowercase();
    roster::ROSTER
        .iter()
        .find(|(n, _)| *n == name)
        .and_then(|(_, c)| CharacterId::parse(c))
}

#[cfg(test)]
mod tests {
    use gpui::px;

    use super::*;

    #[test]
    fn picks_the_bundled_size_at_or_above() {
        assert_eq!(bundled_size(px(16.)), 16);
        assert_eq!(bundled_size(px(18.)), 20);
        assert_eq!(bundled_size(px(20.)), 20);
        assert_eq!(bundled_size(px(32.)), 32);
        assert_eq!(bundled_size(px(40.)), 48);
        assert_eq!(bundled_size(px(64.)), 64);
        assert_eq!(bundled_size(px(96.)), 128);
        assert_eq!(bundled_size(px(300.)), 128);
        assert_eq!(bundled_size(px(0.)), 16);
    }
}
