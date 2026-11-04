//! State scenes of COMPLETENESS.md §4 and E1–E3: hover, focus, loading,
//! error and empty states that the feature scenes do not show.
//!
//! Each track keeps its own block below (`--- <track> ---` … `--- end
//! <track> ---`) so parallel branches merge without conflicts. A scene that
//! needs a pointer or a key press also adds one line to
//! [`super::interact`].

use super::{
    scenes::{Scene, Start, open_first_topic, open_main},
    shaped::Shape,
};

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
        name: "state-hover-code",
        about: "process with the pointer on a code fence's action strip: 问这里 shows only on hover",
        start: Start::SignedIn,
        setup: crate::views::process::preview::open_running,
        open: open_main,
    },
    Scene {
        name: "topic-no-messages",
        about: "A 话题 with no messages yet: the thread-start block and the empty thread (§2.13)",
        start: Start::Shaped(Shape::NoMessages),
        setup: open_first_topic,
        open: open_main,
    },
    Scene {
        name: "topic-older-loading",
        about: "A 话题 while its older messages load: skeleton rows above the first message (§2.14)",
        start: Start::Shaped(Shape::OlderLoading),
        setup: open_first_topic,
        open: open_main,
    },
    Scene {
        name: "sidebar-first-sync",
        about: "Signed in, connecting, nothing synced yet: the sidebar and page before the first sync (§2.14)",
        start: Start::Shaped(Shape::FirstSync),
        setup: |_, _| {},
        open: open_main,
    },
    Scene {
        name: "media-embed-in-pane",
        about: "An HTML 产出 in the thread while the 产出 pane shows it: the embed is one reference row (§2.17)",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| {
            crate::views::media::preview::open(
                s,
                c,
                crate::views::media::preview::Sample::EmbedInPane,
                cx,
            )
        },
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
    Scene {
        name: "diagram-loading",
        about: "A diagram in the thread while it draws: the skeleton in place of the picture",
        start: Start::SignedIn,
        setup: |_, _| {},
        open: |s, c, cx| {
            crate::diagrams::preview::open(s, c, crate::diagrams::preview::Scene::Loading, cx)
        },
    },
    Scene {
        name: "refs-too-many",
        about: "Five reference chips in the composer and a sixth refused: the 最多引用 5 处 Toast bottom-right",
        start: Start::V2Refs,
        setup: crate::views::refs::scenes::too_many_setup,
        open: crate::views::refs::scenes::open_too_many,
    },
    // --- end t5-media ---
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
    Scene {
        name: "state-card-tooltip",
        about: "Card v2 13: the pointer on the cut label of option G: the full text in a tooltip (§2.21)",
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
