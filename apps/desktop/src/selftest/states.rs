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
    // --- r1-kit ---
    Scene {
        name: "state-hover-sidebar",
        about: "首页 with the pointer on a sidebar row: the row hover, muted at 50 % (§2.1, R1-02)",
        start: Start::V2,
        setup: |s, cx| s.nav.update(cx, |nav, cx| nav.open_desk(cx)),
        open: open_main,
    },
    Scene {
        name: "state-key-sidebar",
        about: "首页 after Tab and ↓ ↓: keyboard focus on a sidebar row, a ring distinct from the selected row (E3, R1-01)",
        start: Start::V2,
        setup: |s, cx| s.nav.update(cx, |nav, cx| nav.open_desk(cx)),
        open: open_main,
    },
    // --- end r1-kit ---
    // --- v15/r5-cards: keyboard focus on list rows, card options and the picker (E3); the sidebar scenes are r1-kit's ---
    Scene {
        name: "state-focus-page-list",
        about: "group-v2 after Tab reaches the 话题 list: the focus ring on a list row",
        start: Start::V2,
        setup: |s, cx| (like("group-v2").setup)(s, cx),
        open: |s, c, cx| (like("group-v2").open)(s, c, cx),
    },
    Scene {
        name: "state-focus-card-option",
        about: "cards-v2-choice after Tab reaches an option row: the focus ring on the option, apart from the picked one",
        start: Start::V2,
        setup: |s, cx| (like("cards-v2-choice").setup)(s, cx),
        open: |s, c, cx| (like("cards-v2-choice").open)(s, c, cx),
    },
    Scene {
        name: "state-focus-picker",
        about: "new-group after Tab reaches the member picker: the focus ring on a picker row",
        start: Start::SignedIn,
        setup: |s, cx| (like("new-group").setup)(s, cx),
        open: |s, c, cx| (like("new-group").open)(s, c, cx),
    },
    // --- end v15/r5-cards ---
    // --- v16/c-cards: hover and focus on a page row, a 产出 row and a Settings row (E2, E3) ---
    Scene {
        name: "state-hover-page-row",
        about: "group-v2 with the pointer on a 话题 row: the ListRow hover, muted at 50 % (§2.6)",
        start: Start::V2,
        setup: |s, cx| (like("group-v2").setup)(s, cx),
        open: |s, c, cx| (like("group-v2").open)(s, c, cx),
    },
    Scene {
        name: "state-hover-artifact-row",
        about: "artifacts-v2 with the pointer on a 产出 row: the table row hover (§2.7)",
        start: Start::V2,
        setup: |s, cx| (like("artifacts-v2").setup)(s, cx),
        open: |s, c, cx| (like("artifacts-v2").open)(s, c, cx),
    },
    Scene {
        name: "state-focus-artifact-row",
        about: "artifacts-v2 after Tab reaches the 产出 table: the focus ring on a row",
        start: Start::V2,
        setup: |s, cx| (like("artifacts-v2").setup)(s, cx),
        open: |s, c, cx| (like("artifacts-v2").open)(s, c, cx),
    },
    Scene {
        name: "state-hover-settings-row",
        about: "settings-notifications-end with the pointer on the 按群设置 navigation row (§2.8)",
        start: Start::SignedIn,
        setup: |s, cx| (like("settings-notifications-end").setup)(s, cx),
        open: |s, c, cx| (like("settings-notifications-end").open)(s, c, cx),
    },
    // --- end v16/c-cards ---
];

/// The scene each state scene starts from: its capture must differ from that
/// scene's (review 3 §4, checked after every run that captures both).
const BASES: &[(&str, &str)] = &[
    ("state-hover-message", "main"),
    ("state-hover-code", "process"),
    ("state-card-option-hover", "cards-v2-states"),
    ("state-card-button-hover", "cards-v2-states"),
    ("state-card-tooltip", "cards-v2-states"),
    ("state-hover-sidebar", "desk"),
    ("state-key-sidebar", "desk"),
    ("state-focus-page-list", "group-v2"),
    ("state-focus-card-option", "cards-v2-choice"),
    ("state-focus-picker", "new-group"),
    ("state-hover-page-row", "group-v2"),
    ("state-hover-artifact-row", "artifacts-v2"),
    ("state-focus-artifact-row", "artifacts-v2"),
    ("state-hover-settings-row", "settings-notifications-end"),
];

/// The base scene of state scene `name`.
pub fn base(name: &str) -> Option<&'static str> {
    BASES.iter().find(|(n, _)| *n == name).map(|(_, b)| *b)
}

/// The scene a hover or focus scene starts from: the same setup and window,
/// so the capture differs from that scene only by the pointer or the keys.
fn like(name: &str) -> &'static Scene {
    super::scenes::all()
        .into_iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("no scene {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every state scene names its base, and starts from the same data.
    #[test]
    fn state_scenes_start_like_their_base() {
        for scene in SCENES.iter().filter(|s| s.name.starts_with("state-")) {
            let base = base(scene.name).unwrap_or_else(|| panic!("{} has no base", scene.name));
            assert_eq!(scene.start, like(base).start, "{}", scene.name);
        }
    }
}
