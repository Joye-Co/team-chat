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
    // --- t5-media ---
    Scene {
        name: "media-inline-docs",
        about: "The inline previews of a page, a Markdown document and a PDF in the real thread (640 wide)",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| {
            crate::views::media::preview::open(
                s,
                c,
                crate::views::media::preview::Sample::from_name("media-inline-docs")
                    .expect("sample"),
                cx,
            )
        },
    },
    Scene {
        name: "media-inline-data",
        about: "The inline previews of a code file and a table in the real thread (640 wide)",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| {
            crate::views::media::preview::open(
                s,
                c,
                crate::views::media::preview::Sample::from_name("media-inline-data")
                    .expect("sample"),
                cx,
            )
        },
    },
    Scene {
        name: "media-inline-av",
        about: "The inline previews of an image set, a video and audio in the real thread (640 wide)",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| {
            crate::views::media::preview::open(
                s,
                c,
                crate::views::media::preview::Sample::from_name("media-inline-av").expect("sample"),
                cx,
            )
        },
    },
    Scene {
        name: "media-viewer-loading",
        about: "The 产出 viewer sheet while the version is read: header, then skeletons",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| {
            crate::views::media::preview::open(
                s,
                c,
                crate::views::media::preview::Sample::from_name("media-viewer-loading")
                    .expect("sample"),
                cx,
            )
        },
    },
    Scene {
        name: "media-viewer-failed",
        about: "The 产出 viewer sheet after the version could not be read: header, then an Alert with 重试",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| {
            crate::views::media::preview::open(
                s,
                c,
                crate::views::media::preview::Sample::from_name("media-viewer-failed")
                    .expect("sample"),
                cx,
            )
        },
    },
    // --- end t5-media ---
];
