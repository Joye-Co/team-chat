//! Member characters (docs/PRODUCT.md, "Bot members"): a member with a
//! jojo-friends character draws it instead of the monogram, with its live
//! status as the character's signal (idle, working, needs-input, success,
//! error, offline).
//!
//! The pieces are small so the design layer can wrap them:
//! - [`image`]: character, status and size in, an `Img` out;
//! - [`status`]: the status of a bot from what the provider knows, and the
//!   [`StatusBoard`] global that avatars read;
//! - [`assets`]: the bundled SVGs (rendered by `scripts/characters/`), served
//!   through the app's asset source.

pub mod assets;
pub mod status;

use gpui::{Img, Pixels, SharedString, Styled as _, img};

pub use chat_core::proto::CharacterId;
pub use status::StatusTracker;

/// The status a character shows. The wire and file names are the
/// jojo-friends status ids.
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
pub fn image_path(character: CharacterId, status: CharacterStatus, size: Pixels) -> SharedString {
    format!(
        "characters/{}/{}-{}.svg",
        character.as_str(),
        status.as_str(),
        bundled_size(size)
    )
    .into()
}

/// The character at `size` showing `status`. The character is its own shape:
/// do not clip it to a circle.
pub fn image(character: CharacterId, status: CharacterStatus, size: Pixels) -> Img {
    img(image_path(character, status, size))
        .size(size)
        .flex_none()
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

    #[test]
    fn every_character_status_and_size_is_bundled() {
        assert_eq!(CharacterId::ALL.len(), 26);
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
