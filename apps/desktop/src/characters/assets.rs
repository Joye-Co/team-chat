//! The bundled character SVGs, embedded by `build.rs` from
//! `apps/desktop/assets/characters/`. `ui::AppAssets` serves them; gpui's
//! `img()` renders them in full color (resvg).

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/character_assets.rs"));
}

/// The bytes of a bundled asset path such as `characters/jojo/idle-24.svg`.
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

/// How many files are bundled.
#[cfg(test)]
pub fn count() -> usize {
    embedded::FILES.len()
}
