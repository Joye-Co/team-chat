//! Character art fitted into a bot tile (DESIGN.md §7.9).
//!
//! The tile draws the character cropped to the painted bounds of its idle
//! pose (measured from pixels by `docs/design/mockups-v2/src/charbox.mjs`
//! into `assets/characters/boxes.json`), fitted by the larger side to 62 % of
//! the tile with its center 2 % above the middle. At 40 pt and below the
//! status lamp, the question bubble, the working ring and the sleep marks are
//! removed: at list sizes they read as notification badges.
//!
//! The fitted SVG is made on demand from the bundled file and served through
//! the asset source under `characters-tile/…`, so GPUI caches it like any
//! other image.

use std::{collections::HashMap, sync::OnceLock};

use gpui::{Img, Pixels, SharedString, Styled as _, img};

use super::{CharacterId, CharacterStatus, assets, bundled_size};

/// Share of the tile the art's larger side takes.
pub const FILL: f32 = 0.62;
/// The art's center sits 2 % of the side above the middle: `y = cy - side * 0.48`.
const CENTER_Y: f32 = 0.48;
/// Tiles at or under this size use the small art.
pub const SMALL_MAX: f32 = 40.;

const PREFIX: &str = "characters-tile/";

static BOXES_JSON: &str = include_str!("../../assets/characters/boxes.json");

fn boxes() -> &'static HashMap<String, [f32; 4]> {
    static B: OnceLock<HashMap<String, [f32; 4]>> = OnceLock::new();
    B.get_or_init(|| serde_json::from_str(BOXES_JSON).unwrap_or_default())
}

#[cfg(test)]
/// The painted bounds `[x, y, w, h]` of a character's idle pose, in the
/// units of its SVG viewBox.
pub fn idle_box(character: CharacterId) -> Option<[f32; 4]> {
    boxes()
        .get(&format!("{}__idle", character.as_str()))
        .copied()
}

/// The asset path of the fitted art of `character` at `status` in a tile of
/// `size`.
pub fn tile_path(character: CharacterId, status: CharacterStatus, size: Pixels) -> SharedString {
    format!(
        "{PREFIX}{}/{}-{}.svg",
        character.as_str(),
        status.as_str(),
        bundled_size(size)
    )
    .into()
}

/// The fitted character art filling a tile of `size` (the tile itself, its
/// tint and radius, are drawn by `ui::MemberAvatar`).
pub fn tile_image(character: CharacterId, status: CharacterStatus, size: Pixels) -> Img {
    img(tile_path(character, status, size))
        .size(size)
        .flex_none()
}

/// Makes the fitted SVG for a `characters-tile/…` path.
pub fn load(path: &str) -> Option<Vec<u8>> {
    let rest = path.strip_prefix(PREFIX)?;
    let (id, file) = rest.split_once('/')?;
    let source = assets::load(&format!("characters/{id}/{file}"))?;
    let size: f32 = file
        .trim_end_matches(".svg")
        .rsplit('-')
        .next()?
        .parse()
        .ok()?;
    let mut svg = std::str::from_utf8(source).ok()?.to_string();
    if size <= SMALL_MAX {
        svg = small_art(&svg);
    }
    if let Some(b) = boxes().get(&format!("{id}__idle")) {
        let side = b[2].max(b[3]) / FILL;
        let x = b[0] + b[2] / 2. - side / 2.;
        let y = b[1] + b[3] / 2. - side * CENTER_Y;
        svg = replace_view_box(&svg, &format!("{x:.1} {y:.1} {side:.1} {side:.1}"));
    }
    Some(svg.into_bytes())
}

fn replace_view_box(svg: &str, value: &str) -> String {
    let Some(start) = svg.find("viewBox=\"") else {
        return svg.to_string();
    };
    let from = start + "viewBox=\"".len();
    let Some(len) = svg[from..].find('"') else {
        return svg.to_string();
    };
    format!("{}{}{}", &svg[..from], value, &svg[from + len..])
}

/// Removes the `<g>` element that encloses byte offset `at`, children
/// included.
fn drop_group_around(svg: &str, at: usize) -> String {
    // `at` may point at the `<g` itself, as `lastIndexOf` allows in the mockups.
    let Some(start) = svg[..(at + 2).min(svg.len())].rfind("<g") else {
        return svg.to_string();
    };
    let mut depth = 0i32;
    let mut i = start;
    while i < svg.len() {
        let rest = &svg[i..];
        if rest.starts_with("</g>") {
            depth -= 1;
            if depth == 0 {
                return format!("{}{}", &svg[..start], &svg[i + 4..]);
            }
            i += 4;
        } else if rest.starts_with("<g")
            && rest
                .as_bytes()
                .get(2)
                .is_some_and(|b| b.is_ascii_whitespace() || *b == b'>')
        {
            depth += 1;
            i += 2;
        } else {
            i += rest.chars().next().map_or(1, char::len_utf8);
        }
    }
    svg.to_string()
}

/// The small-size art (§7.9): the same layers the mockups remove
/// (`smallArt` in `kit.mjs`).
pub fn small_art(svg: &str) -> String {
    let mut s = svg.to_string();
    if let Some(glow) = s.find("-g-glow)\"") {
        s = drop_group_around(&s, glow);
    }
    if let Some(bubble) = find_bubble(&s) {
        s = drop_group_around(&s, bubble);
    }
    const RING: &str = "<g opacity=\"1\" fill=\"none\" stroke-linecap=\"round\"";
    while let Some(ring) = s.find(RING) {
        if ring == 0 {
            break;
        }
        let next = drop_group_around(&s, ring);
        if next == s {
            break;
        }
        s = next;
    }
    remove_sleep_marks(&s)
}

/// `fill="#ffffff" stroke="#xxxxxx" stroke-linejoin="round"`: the bubble.
fn find_bubble(s: &str) -> Option<usize> {
    const HEAD: &str = "fill=\"#ffffff\" stroke=\"#";
    const TAIL: &str = "\" stroke-linejoin=\"round\"";
    let mut from = 0;
    while let Some(off) = s[from..].find(HEAD) {
        let at = from + off;
        let hex = &s[at + HEAD.len()..];
        if hex.len() > 6 + TAIL.len()
            && hex[..6].bytes().all(|b| b.is_ascii_hexdigit())
            && hex[6..].starts_with(TAIL)
        {
            return Some(at);
        }
        from = at + HEAD.len();
    }
    None
}

/// Removes `<path transform="…" … d="M-13,-13 H13 L-13,13 H13" …>` (the Z marks).
fn remove_sleep_marks(s: &str) -> String {
    const MARK: &str = "d=\"M-13,-13 H13 L-13,13 H13\"";
    let mut out = s.to_string();
    while let Some(at) = out.find(MARK) {
        let Some(start) = out[..at].rfind("<path") else {
            break;
        };
        let Some(gt) = out[at..].find('>') else {
            break;
        };
        let mut end = at + gt + 1;
        if out[end..].starts_with("</path>") {
            end += "</path>".len();
        }
        out.replace_range(start..end, "");
    }
    out
}

#[cfg(test)]
mod tests {
    use gpui::px;

    use super::*;

    #[test]
    fn every_character_has_an_idle_box_and_a_tile() {
        for &c in CharacterId::ALL {
            assert!(idle_box(c).is_some(), "no box for {}", c.as_str());
            let path = tile_path(c, CharacterStatus::NeedsInput, px(24.));
            let svg = String::from_utf8(load(&path).expect("tile loads")).unwrap();
            assert!(svg.starts_with("<svg"));
            assert!(!svg.contains("-g-glow)\""), "lamp kept in {path}");
            assert_eq!(svg.matches("<g").count(), svg.matches("</g>").count());
        }
    }
}
