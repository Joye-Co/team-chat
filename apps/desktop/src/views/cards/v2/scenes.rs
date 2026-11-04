//! Screenshot scenes of Card v2 (the cards v2 track owns this slice): the
//! gallery of Cards.dc.html from the top and at each section, and the
//! approval card in its own topic.

use gpui::{AnyWindowHandle, App, AppContext as _, Pixels, Size, WindowBounds};

use super::gallery::{CardsGallery, Section};
use crate::{
    app::{Services, main_window_options},
    fake::v2 as d,
    selftest::scenes::{Scene, Start, capture_bounds, open_main, v2_open_topic},
};

pub(crate) fn open_gallery(
    services: Services,
    content: Size<Pixels>,
    start: Section,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let mut options = main_window_options(false, cx);
    options.window_bounds = Some(WindowBounds::Windowed(capture_bounds(content, cx)));
    options.window_min_size = None;
    gpui_kit::open_window(options, cx, move |_, cx| {
        cx.new(|_| CardsGallery::new(services, start))
    })
    .map(|(handle, _)| handle)
    .map_err(|e| e.to_string())
}

macro_rules! gallery_scene {
    ($name:literal, $about:literal, $section:expr) => {
        Scene {
            name: $name,
            about: $about,
            start: Start::V2,
            setup: |_, _| {},
            open: |s, c, cx| open_gallery(s, c, $section, cx),
        }
    };
}

pub const SCENES: &[Scene] = &[
    gallery_scene!(
        "cards-v2",
        "Card v2 gallery (Cards.dc.html) from the top: the anatomy, the state overview and 01 选择",
        Section::States
    ),
    gallery_scene!(
        "cards-v2-choice",
        "Card v2 01 选择: a quick choice, three options with 推荐, the answer record and the view of someone who is not asked",
        Section::Choice
    ),
    gallery_scene!(
        "cards-v2-approve",
        "Card v2 02 批准: the approval, the confirm sheet with type-to-confirm, executing with a live bar and done with the reverse action",
        Section::Approve
    ),
    gallery_scene!(
        "cards-v2-draft",
        "Card v2 03 草稿: the draft well, the edit mode with counters, confirmed with edits and superseded",
        Section::Draft
    ),
    gallery_scene!(
        "cards-v2-form",
        "Card v2 04 表单: required fields missing, then filled and ready",
        Section::Form
    ),
    gallery_scene!(
        "cards-v2-plan",
        "Card v2 05 计划: a running plan, a plan waiting on Joye (seen by 小王) and the collapsed rows of a topic",
        Section::Plan
    ),
    gallery_scene!(
        "cards-v2-data",
        "Card v2 06 数据: metrics with a line chart, a table with a total row and metrics with notes",
        Section::Data
    ),
    gallery_scene!(
        "cards-v2-diff",
        "Card v2 07 对比: a diff to review, PR refs with their states and a PR with no evidence",
        Section::Diff
    ),
    gallery_scene!(
        "cards-v2-evidence",
        "Card v2 08 证据: thumbnails with +n and a test line, no evidence, and proof chips",
        Section::Evidence
    ),
    gallery_scene!(
        "cards-v2-refs",
        "Card v2 09 链接: PR, 产出 with a newer version, 话题 and 成员 refs; a merged PR and a deleted 产出; an answer on its way",
        Section::Refs
    ),
    gallery_scene!(
        "cards-v2-ack",
        "Card v2 10 轻回应 and 11 交互产出: the emoji acknowledgement and a 产出 snapshot with its answer",
        Section::Ack
    ),
    gallery_scene!(
        "cards-v2-notify",
        "Card v2 12 通知: the notification banner at rest and on hover, then 13: loading and failed cards and a long choice",
        Section::Notify
    ),
    gallery_scene!(
        "cards-v2-states",
        "Card v2 13: a card not fetched yet (skeleton), a card that cannot be read (Alert with 重试) and a choice with 8 options",
        Section::Load
    ),
    Scene {
        name: "cards-v2-topic",
        about: "The approval card of qqbot v2.14 上线 as a v2 card in its 话题",
        start: Start::V2,
        setup: |s, cx| v2_open_topic(s, d::OPS, d::T_RELEASE, cx),
        open: open_main,
    },
];
