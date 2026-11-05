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

/// What the runner does in a scene before it captures. `TC_KEYS=tab,down`
/// replaces the keys of a key scene while you look for the right count.
#[derive(Clone, Copy, Debug)]
pub struct Steps {
    /// Where the pointer rests while the window draws (`None`: outside).
    pub pointer: Option<(f32, f32)>,
    /// Keys pressed once after the first frame, in order (`"tab"`,
    /// `"shift-tab"`, `"down"`). Write the struct out for a key scene.
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
    ("state-hover-message", hover(600., 315., 600)),
    // The 问这里 slot left of the copy button in the code block's top strip.
    ("state-hover-code", hover(905., 410., 400)),
    // --- end t5 ---
    // --- t5-cards ---
    ("state-card-option-hover", hover(1011., 290., 400)),
    ("state-card-button-hover", hover(667., 320., 400)),
    ("state-card-tooltip", hover(900., 530., 1200)),
    // --- end t5-cards ---
    // --- r1-kit ---
    ("state-hover-sidebar", hover(120., 225., 300)),
    (
        "state-key-sidebar",
        Steps {
            pointer: None,
            keys: &["tab", "tab", "tab", "tab", "down", "down"],
            settle_ms: 200,
        },
    ),
    // --- end r1-kit ---
    // --- v15/r5-cards ---
    // Tab past the sidebar rows and the toolbar to the first 话题 row, then ↓.
    (
        "state-focus-page-list",
        keys(&[
            "tab", "tab", "tab", "tab", "tab", "tab", "tab", "tab", "tab", "tab", "tab", "tab",
            "tab", "tab", "tab", "tab", "tab", "tab", "tab", "tab", "tab", "tab", "tab", "tab",
            "tab", "tab", "down",
        ]),
    ),
    ("state-focus-card-option", keys(&["tab", "down"])),
    ("state-focus-picker", keys(&["tab", "tab", "down"])),
    // --- end v15/r5-cards ---
    // --- v16/c-cards ---
    ("state-hover-page-row", hover(535., 201., 300)),
    ("state-hover-artifact-row", hover(893., 189., 300)),
    // The last tab stops are the last row's "⋯", then the row itself.
    (
        "state-focus-artifact-row",
        keys(&["shift-tab", "shift-tab"]),
    ),
    ("state-hover-settings-row", hover(320., 673., 300)),
    ("state-focus-diagram-node", keys(&["tab", "tab"])),
    // --- end v16/c-cards ---
];

/// The steps of scene `name`, if it has any.
pub fn steps(name: &str) -> Option<Steps> {
    STEPS.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
}

/// Whether scene `name` presses keys with no pointer: the runner then stops
/// moving the pointer after the keys, since a pointer move ends the keyboard
/// modality and GPUI would hide the focus ring (`focus_visible`).
pub fn keys_only(name: &str) -> bool {
    steps(name).is_some_and(|s| s.pointer.is_none() && !s.keys.is_empty())
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
