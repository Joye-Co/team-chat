//! Member characters (docs/PRODUCT.md, "Bot members"): a member with a
//! character draws it instead of the monogram, with its live status as the
//! character's signal (idle, working, needs-input, success, error, offline).
//!
//! The art is the optional character pack (from TC_OPS_DIR, see `build.rs`).
//! A build without it still knows the 26 ids (they are a protocol field) but
//! has no art: [`has_art`] is false and every member draws its initial.
//!
//! The pieces are small so the design layer can wrap them:
//! - [`image`]: character, status and size in, an `Img` out;
//! - [`status`]: the status of a bot from what the provider knows, and the
//!   [`StatusBoard`] global that avatars read;
//! - [`assets`]: the bundled SVGs of the character pack, served through the
//!   app's asset source.

pub mod assets;
pub mod status;
pub mod tile;

use gpui::{Img, Pixels, Styled as _, img};

pub use chat_core::proto::CharacterId;
pub use status::StatusTracker;

/// The status a character shows. The wire and file names are the character
/// pack's status ids.
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

    pub fn as_str(self) -> &'static str {
        match self {
            CharacterStatus::Idle => "idle",
            CharacterStatus::Working => "working",
            CharacterStatus::NeedsInput => "needs-input",
            CharacterStatus::Success => "success",
            CharacterStatus::Error => "error",
            CharacterStatus::Offline => "offline",
        }
    }
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

/// The asset path of one bundled image, for example
/// `characters/o03/needs-input-24.svg`.
#[cfg(test)]
pub fn image_path(
    character: CharacterId,
    status: CharacterStatus,
    size: Pixels,
) -> gpui::SharedString {
    format!(
        "characters/{}/{}-{}.svg",
        character.as_str(),
        status.as_str(),
        bundled_size(size)
    )
    .into()
}

/// The character at `size` showing `status`, without the lamp, bubble, ring
/// or sleep marks up to 64 pt. The character is its own shape: do not clip
/// it to a circle.
pub fn image(character: CharacterId, status: CharacterStatus, size: Pixels) -> Img {
    img(tile::clean_path(character, status, size))
        .size(size)
        .flex_none()
}

/// Whether this build has the art of `character` (the optional character pack).
/// Without it the member draws its initial, like a member with no character.
pub fn has_art(character: CharacterId) -> bool {
    assets::has_character(character.as_str())
}

/// Whether this build has any character art: without it the look pickers hide.
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
    fn the_roster_gives_new_bots_their_character() {
        assert_eq!(roster_character("新机器人"), None);
        if roster::ROSTER.is_empty() {
            assert_eq!(roster_character("Jojo"), None);
            return;
        }
        assert_eq!(roster_character("Jojo"), Some(CharacterId::Jojo));
        assert_eq!(roster_character(" ledger "), Some(CharacterId::H02));
        assert_eq!(roster_character("Tata"), Some(CharacterId::O03));
        assert_eq!(roster_character("新机器人"), None);
    }

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

    #[test]
    fn every_character_status_and_size_is_bundled() {
        assert_eq!(CharacterId::ALL.len(), 26);
        if !available() {
            // A build without the character pack has no art at all.
            assert_eq!(assets::count(), 0);
            assert!(CharacterId::ALL.iter().all(|&c| !has_art(c)));
            return;
        }
        for &c in CharacterId::ALL {
            for s in CharacterStatus::ALL {
                for size in SIZES {
                    let path = image_path(c, s, px(size as f32));
                    let bytes = assets::load(&path).unwrap_or_else(|| panic!("missing {path}"));
                    let text = std::str::from_utf8(bytes).unwrap();
                    assert!(text.starts_with("<svg"), "{path}");
                    assert!(text.contains(&format!("width=\"{size}\"")), "{path}");
                }
            }
        }
        assert_eq!(assets::count(), 26 * 6 * SIZES.len());
    }
}
