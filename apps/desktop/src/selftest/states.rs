//! State scenes of COMPLETENESS.md §4 and E1–E3: hover, focus, loading,
//! error and empty states that the feature scenes do not show.
//!
//! Each track keeps its own block below (`--- <track> ---` … `--- end
//! <track> ---`) so parallel branches merge without conflicts. A scene that
//! needs a pointer or a key press also adds one line to
//! [`super::interact`].

use super::scenes::{Scene, Start, open_first_topic, open_main};

/// Every state scene, in track order.
pub const SCENES: &[Scene] = &[
    // --- t5 ---
    Scene {
        name: "state-hover-message",
        about: "main with the pointer on a message: the row hover and its hover toolbar",
        start: Start::SignedIn,
        setup: open_first_topic,
        open: open_main,
    },
    Scene {
        name: "state-focus-tab",
        about: "main after one Tab: the first focusable control shows its 3 px focus ring",
        start: Start::SignedIn,
        setup: open_first_topic,
        open: open_main,
    },
    // --- end t5 ---
    // --- t5-cards ---
    Scene {
        name: "state-card-option-hover",
        about: "Card v2 13: the pointer on choice row B of the long choice (hover: muted at 50%)",
        start: Start::V2,
        setup: |_, _| {},
        open: |s, c, cx| {
            crate::views::cards::v2::scenes::open_gallery(
                s,
                c,
                crate::views::cards::v2::gallery::Section::Load,
                cx,
            )
        },
    },
    Scene {
        name: "state-card-button-hover",
        about: "Card v2 13: the pointer on 重试 of the card that cannot be read (outline sm hover)",
        start: Start::V2,
        setup: |_, _| {},
        open: |s, c, cx| {
            crate::views::cards::v2::scenes::open_gallery(
                s,
                c,
                crate::views::cards::v2::gallery::Section::Load,
                cx,
            )
        },
    },
    // --- end t5-cards ---
];
