//! Screenshot scenes of Card v2.1 (the inc-cards21 track owns this slice): CardsCompose.dc.html
//! section by section and the interaction loop of Interact.dc.html.

use gpui::{AnyWindowHandle, App, AppContext as _, Pixels, Size, WindowBounds};

use super::gallery::{Gallery21, Page};
use crate::{
    app::{Services, main_window_options},
    selftest::scenes::{Scene, Start, capture_bounds},
};

fn open(
    services: Services,
    content: Size<Pixels>,
    page: Page,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let mut options = main_window_options(false, cx);
    options.window_bounds = Some(WindowBounds::Windowed(capture_bounds(content, cx)));
    options.window_min_size = None;
    gpui_kit::open_window(options, cx, move |_, cx| {
        cx.new(|_| Gallery21::new(services, page))
    })
    .map(|(handle, _)| handle)
    .map_err(|e| e.to_string())
}

macro_rules! scene {
    ($name:literal, $about:literal, $page:expr) => {
        Scene {
            name: $name,
            about: $about,
            start: Start::V2,
            setup: |_, _| {},
            open: |s, c, cx| open(s, c, $page, cx),
        }
    };
}

pub const SCENES: &[Scene] = &[
    scene!(
        "compose",
        "Card v2.1 01 组合 (CardsCompose.dc.html): questionnaire, compare, groups, metrics with a chart and an approval, the read-only view and a superseded card",
        Page::Compose
    ),
    scene!(
        "compose-choice",
        "Card v2.1 02 选择题: single with 其他, multiple with min and max, rank with the cut line, rating and scale in a questionnaire",
        Page::Choice
    ),
    scene!(
        "compose-choice-2",
        "Card v2.1 02 选择题: image choice, confirm, an answer that did not get there, matrix",
        Page::Choice2
    ),
    scene!(
        "compose-choice-3",
        "Card v2.1 02 选择题: time slots and a public poll after my vote",
        Page::Choice3
    ),
    scene!(
        "compose-extend",
        "Card v2.1 03 扩展方式: tabs and inline buttons, a diagram block, an embedded 产出 and a block this version does not know",
        Page::Extend
    ),
    scene!(
        "interact_report_rev1",
        "INTERACT 7.1 rev 1: metrics, a row with the chart and the question (720), and the same card under 560 where the row stacks",
        Page::Report
    ),
    scene!(
        "interact_pending",
        "INTERACT §4.4: my bar click waiting for Ledger: 等回应, 还在处理, 没有回应 with 重试 and 不等了",
        Page::Pending
    ),
    scene!(
        "interact_drilldown_rev2",
        "INTERACT 7.2 rev 2: the inserted breakdown with its tint and the attribution line, for me and for 小王",
        Page::Drill
    ),
    scene!(
        "interact_conflict_toast",
        "INTERACT §4.4: my submit was refused because the card changed: the latest rev with my choice kept, and the toast",
        Page::Conflict
    ),
    scene!(
        "interact_readonly_other_viewer",
        "INTERACT 7.1 as 小王 sees it: the question read-only with 等 Joye",
        Page::Readonly
    ),
    // v11/pb-cards
    scene!(
        "interact_notes",
        "INTERACT §4.4 and §6.2: my click offline (the bar goes back), a block the member said is not interactive (read-only, no 重试), a rev that changed the block I typed in (内容变了 · 用新的) and a reply off screen ({name} 回了一条新消息)",
        Page::Notes
    ),
];

/// The checks of the Card v2.1 scenes.
pub fn checks(name: &str) -> Option<crate::selftest::scenes::Checks> {
    use super::samples::NOTE_BUSY;
    use crate::selftest::drawn::{check, ids};
    match name {
        "interact_notes" => Some(|_, _| {
            let (retry, refused) = (ids("card-retry"), ids("card-refused"));
            vec![
                check(
                    refused.contains(NOTE_BUSY),
                    format!("expect.mode none: the block is read-only ({refused:?})"),
                ),
                check(
                    !retry.contains(NOTE_BUSY),
                    format!("expect.mode none: no 重试 ({retry:?})"),
                ),
            ]
        }),
        "compose-choice-2" => Some(|_, _| {
            let retry = ids("card-band-retry");
            vec![check(
                retry.contains(super::samples::FAILED),
                format!(
                    "the red 失败 band of an answer that did not get there has 重试 ({retry:?})"
                ),
            )]
        }),
        "interact_pending" => Some(|_, _| {
            let retry = ids("card-retry");
            vec![check(
                !retry.is_empty(),
                format!("没有回应 shows 重试 ({retry:?})"),
            )]
        }),
        _ => None,
    }
}
