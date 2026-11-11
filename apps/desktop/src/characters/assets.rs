//! The bundled character SVGs, embedded by `build.rs` from the vendored
//! avatar pack (`apps/desktop/assets/characters/`).
//! `ui::AppAssets` serves them; gpui's `img()` renders them in full color
//! (resvg).

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/character_assets.rs"));
}

/// The bytes of a bundled asset path such as `characters/jojo/calm-24.svg`.
pub fn load(path: &str) -> Option<&'static [u8]> {
    if !path.starts_with("characters/") {
        return None;
    }
    embedded::FILES
        .binary_search_by(|(p, _)| (*p).cmp(path))
        .ok()
        .map(|i| embedded::FILES[i].1)
}

/// The bundled paths under `prefix`.
pub fn list(prefix: &str) -> impl Iterator<Item = &'static str> + '_ {
    embedded::FILES
        .iter()
        .map(|(p, _)| *p)
        .filter(move |p| p.starts_with(prefix))
}

/// Whether any file of the character `id` is bundled.
pub fn has_character(id: &str) -> bool {
    let prefix = format!("characters/{id}/");
    let at = embedded::FILES.partition_point(|(p, _)| *p < prefix.as_str());
    embedded::FILES
        .get(at)
        .is_some_and(|(p, _)| p.starts_with(&prefix))
}

/// How many files are bundled.
pub fn count() -> usize {
    embedded::FILES.len()
}
