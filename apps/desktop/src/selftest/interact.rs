//! Pointer and keyboard steps of the screenshot self-test: a scene that shows
//! a hover or a focus state names it here, so its capture differs from its
//! base (COMPLETENESS.md E2, E3).
//!
//! Every other scene captures with the pointer parked outside the window and
//! no key pressed. To add a hover or focus scene, add one line to
//! [`STEPS`]: the scene name, the pointer position (1x window points, from
//! the top left of the content) and the keys to press once after the first
//! frame. Keep one line per scene so parallel tracks merge without conflicts.

use gpui::{Pixels, Point, point, px};

/// What the runner does in a scene before it captures.
#[derive(Clone, Copy, Debug)]
pub struct Steps {
    /// Where the pointer rests while the window draws (`None`: outside).
    pub pointer: Option<(f32, f32)>,
    /// Keys pressed once after the first frame, in order (`"tab"`,
    /// `"shift-tab"`, `"down"`).
    pub keys: &'static [&'static str],
    /// Extra wait after the keys and the pointer, for a hover delay or a
    /// tooltip (ms).
    pub settle_ms: u64,
}

const fn hover(x: f32, y: f32, settle_ms: u64) -> Steps {
    Steps {
        pointer: Some((x, y)),
        keys: &[],
        settle_ms,
    }
}

const fn keys(keys: &'static [&'static str]) -> Steps {
    Steps {
        pointer: None,
        keys,
        settle_ms: 200,
    }
}

/// One line per scene.
const STEPS: &[(&str, Steps)] = &[
    // --- t5 ---
    ("state-hover-message", hover(400., 250., 600)),
    ("state-focus-tab", keys(&["tab"])),
    // --- end t5 ---
    // --- t5-cards ---
    ("state-card-option-hover", hover(1011., 290., 400)),
    ("state-card-button-hover", hover(667., 320., 400)),
    ("state-card-tooltip", hover(900., 530., 1200)),
    // --- end t5-cards ---
];

/// The steps of scene `name`, if it has any.
pub fn steps(name: &str) -> Option<Steps> {
    STEPS.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
}

/// Where the pointer rests in scene `name`: its hover point, else far
/// outside the window.
pub fn pointer(name: &str) -> Point<Pixels> {
    // `TC_POINTER=x,y` moves the hover point of a hover scene while you
    // look for the right spot, without a rebuild.
    let over = std::env::var("TC_POINTER").ok().and_then(|v| {
        let (x, y) = v.split_once(',')?;
        Some((x.parse::<f32>().ok()?, y.parse::<f32>().ok()?))
    });
    match steps(name)
        .and_then(|s| s.pointer)
        .map(|p| over.unwrap_or(p))
    {
        Some((x, y)) => point(px(x), px(y)),
        None => point(px(-10_000.), px(-10_000.)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_step_names_a_scene_once() {
        let names: Vec<&str> = crate::selftest::scenes::all()
            .iter()
            .map(|s| s.name)
            .collect();
        let mut seen = std::collections::HashSet::new();
        for (name, _) in STEPS {
            assert!(names.contains(name), "{name} is not a scene");
            assert!(seen.insert(*name), "{name} has two step lines");
        }
    }
}
