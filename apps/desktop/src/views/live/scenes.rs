//! Screenshot scenes of 实时查看 (the live track owns this slice).

use gpui::App;

use crate::{
    app::{RightPane, Services},
    fake::v2 as d,
    selftest::scenes::{Scene, Start, open_main, v2_open_topic},
};

fn live(services: &Services, cx: &mut App) {
    v2_open_topic(services, d::DEV, d::T_VIRTUAL, cx);
    services.nav.update(cx, |nav, cx| {
        nav.set_right_pane(RightPane::Live(d::RUN_VIRTUAL.into()), cx)
    });
}

// --- v12/fx-overlays: the stop scenes ---

/// The main window on the running sample run's steps, with its own
/// provider: `hold` keeps a stop unanswered (`正在停止…`).
fn open_stop_with(
    hold: bool,
    content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<(gpui::AnyWindowHandle, Services), String> {
    use crate::views::process::preview as p;
    let fake = crate::fake::FakeProvider::new();
    if hold {
        fake.hold_stop(p::RUN);
    }
    // With `--worst-case` the stop alert and 正在停止… read worst-case names.
    let provider: std::sync::Arc<dyn crate::data::ChatProvider> = if crate::selftest::worst_case() {
        crate::selftest::worst_case::WorstCase::new(fake)
    } else {
        fake
    };
    let services = Services::new(provider, cx);
    p::open_running(&services, cx);
    let handle = open_main(services.clone(), content, cx)?;
    Ok((handle, services))
}

/// 停止 pressed: the alert that asks first.
pub fn open_stop(
    _services: Services,
    content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<gpui::AnyWindowHandle, String> {
    use crate::views::process::preview as p;
    let (handle, services) = open_stop_with(false, content, cx)?;
    let run = services
        .provider
        .runs(p::GROUP, p::TOPIC)
        .into_iter()
        .find(|r| r.id == p::RUN)
        .ok_or("no sample run")?;
    handle
        .update(cx, |_, window, cx| {
            crate::views::process::confirm_stop(&services, &run, window, cx)
        })
        .map_err(|e| e.to_string())?;
    Ok(handle)
}

/// After 停止: `正在停止…` until the run ends.
pub fn open_stopping(
    _services: Services,
    content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<gpui::AnyWindowHandle, String> {
    open_stop_with(true, content, cx).map(|(h, _)| h)
}

// --- end v12/fx-overlays ---

// --- usable/takeover (design-takeover T6, T7): one scene per phase and the command box ---

use crate::data::run_phase::{HandCommand, HandResult, RunHold};

/// The main window on 实时查看 of the sample run, its phase set by `hold` on its own
/// provider (and its held commands by `hand`).
fn open_phase(
    content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
    hold: impl FnOnce(&mut RunHold),
    hand: Vec<HandCommand>,
) -> Result<gpui::AnyWindowHandle, String> {
    let fake = d::V2Samples::new();
    fake.update_hold(hold);
    fake.set_hand(hand);
    let provider: std::sync::Arc<dyn crate::data::ChatProvider> = if crate::selftest::worst_case() {
        crate::selftest::worst_case::WorstCase::new(fake)
    } else {
        fake
    };
    let services = Services::new(provider, cx);
    live(&services, cx);
    open_main(services, content, cx)
}

fn phase(h: &mut RunHold, phase: &str, waiting: Option<&str>) {
    h.phase = Some(phase.into());
    h.waiting_for = waiting.map(Into::into);
}

fn held(h: &mut RunHold, by: &str, kind: &str) {
    h.paused_by = Some(by.into());
    h.hold_kind = Some(kind.into());
    h.held_at = Some(d::at(9, 50));
    h.phase = Some(
        if kind == "takeover" {
            "taken_over"
        } else {
            "paused"
        }
        .into(),
    );
}

fn hand(id: &str, command: &str, output: &str, result: Option<HandResult>) -> HandCommand {
    HandCommand {
        id: id.into(),
        command: command.into(),
        by: d::ME.into(),
        output: output.into(),
        result,
        at: d::at(9, 52),
    }
}

fn ended(ok: bool, exit_code: Option<i64>, timed_out: bool) -> Option<HandResult> {
    Some(HandResult {
        ok,
        exit_code,
        ms: 1200,
        timed_out,
        error: None,
    })
}

type Open =
    fn(Services, gpui::Size<gpui::Pixels>, &mut App) -> Result<gpui::AnyWindowHandle, String>;

macro_rules! phase_scene {
    ($name:literal, $about:literal, $hold:expr) => {
        phase_scene!($name, $about, $hold, Vec::new())
    };
    ($name:literal, $about:literal, $hold:expr, $hand:expr) => {
        Scene {
            name: $name,
            about: $about,
            start: Start::V2,
            setup: |_, _| {},
            open: {
                fn open(
                    _: Services,
                    content: gpui::Size<gpui::Pixels>,
                    cx: &mut App,
                ) -> Result<gpui::AnyWindowHandle, String> {
                    open_phase(content, cx, $hold, $hand)
                }
                open as Open
            },
        }
    };
}

/// Registered with the scenes of the live track (`selftest::scenes`).
pub const PHASE_SCENES: &[Scene] = &[
    phase_scene!(
        "live-phase-waiting-person",
        "实时查看: Forge waits for my reply in a question card (等你回复, 暂停 / 接管 / 停止…)",
        |h| phase(h, "waiting_person", Some("question"))
    ),
    phase_scene!(
        "live-phase-stuck",
        "实时查看: the watcher found Forge stuck (卡住了，需要你看一下)",
        |h| phase(h, "waiting_person", Some("stuck"))
    ),
    phase_scene!(
        "live-phase-needs-reply",
        "实时查看: the run ended and waits for my reply (去回复 only)",
        |h| phase(h, "needs_reply", Some("report"))
    ),
    phase_scene!(
        "live-phase-waiting-approval",
        "实时查看: Forge waits for my approval (等你批准)",
        |h| phase(h, "waiting_approval", None)
    ),
    phase_scene!(
        "live-phase-waiting-computer",
        "实时查看: queued for a computer, second in line (在等电脑：排队第 2 个, 停止… only)",
        |h| {
            phase(h, "waiting_computer", Some("queue"));
            h.queue_position = Some(2);
        }
    ),
    phase_scene!(
        "live-phase-computer-offline",
        "实时查看: the run's computer is offline (在等电脑：电脑不在线)",
        |h| phase(h, "waiting_computer", Some("offline"))
    ),
    phase_scene!(
        "live-phase-paused",
        "实时查看: I paused Forge (你暂停了 Forge, the note box, 继续 / 接管 / 停止…)",
        |h| held(h, d::ME, "pause")
    ),
    phase_scene!(
        "live-phase-paused-other",
        "实时查看: 小王 paused Forge on my computer (继续 for me as its owner)",
        |h| held(h, d::XIAOWANG, "pause")
    ),
    phase_scene!(
        "live-phase-taken-over-other",
        "实时查看: 小王 took over Forge on a team VM; I am an admin (替 小王 交还)",
        |h| {
            held(h, d::XIAOWANG, "takeover");
            h.machine_owner = None;
            h.takeover = Some("isolated".into());
        }
    ),
    phase_scene!(
        "live-phase-pause-only",
        "实时查看: a computer without take-over (an older daemon): 暂停 and 停止… only",
        |h| h.takeover = None
    ),
    phase_scene!(
        "live-phase-done",
        "实时查看: the run is done (完成, no buttons)",
        |h| phase(h, "done", None)
    ),
    phase_scene!(
        "live-phase-failed",
        "实时查看: the run failed (没做完)",
        |h| phase(h, "failed", None)
    ),
    phase_scene!(
        "live-phase-stopped",
        "实时查看: the run was stopped (已停止)",
        |h| phase(h, "stopped", None)
    ),
    phase_scene!(
        "live-command-empty",
        "实时查看 after 接管: the command box, nothing run yet, the work folder and 在访达中打开",
        |h| held(h, d::ME, "takeover")
    ),
    phase_scene!(
        "live-command-running",
        "实时查看 after 接管: a command runs (运行中, 中止)",
        |h| held(h, d::ME, "takeover"),
        vec![hand(
            "hc-1",
            "cargo test -p desktop live",
            "running 12 tests\ntest live::phase::tests::the_topic_lines ... ok\n",
            None
        )]
    ),
    phase_scene!(
        "live-command-output",
        "实时查看 after 接管: two commands ended (成功) with their output; the note counts them",
        |h| held(h, d::ME, "takeover"),
        vec![
            hand(
                "hc-1",
                "git status --short",
                " M .env.example\n?? notes.md\n",
                ended(true, Some(0), false)
            ),
            hand(
                "hc-2",
                "git commit -am \"fix env\"",
                "[bot/forge-virtual d4e5f6a] fix env\n 1 file changed, 2 insertions(+)\n",
                ended(true, Some(0), false)
            ),
        ]
    ),
    phase_scene!(
        "live-command-failed",
        "实时查看 after 接管: a failed command and one past 10 minutes (失败, 超过 10 分钟，已中止)",
        |h| held(h, d::ME, "takeover"),
        vec![
            hand(
                "hc-1",
                "npm test",
                "FAIL src/env.test.ts\n  expected API_URL to be set\nTests: 1 failed, 41 passed\n",
                ended(false, Some(1), false)
            ),
            hand(
                "hc-2",
                "npm run e2e",
                "waiting for the dev server…\n",
                ended(false, None, true)
            ),
        ]
    ),
    phase_scene!(
        "live-command-offline",
        "实时查看 after 接管: the computer went away while I hold it (the command box is off, 电脑不在线)",
        |h| {
            held(h, d::ME, "takeover");
            h.waiting_for = Some("offline".into());
        },
        vec![hand(
            "hc-1",
            "git status --short",
            " M .env.example\n",
            ended(true, Some(0), false)
        )]
    ),
];
// --- end usable/takeover ---

pub const SCENES: &[Scene] = &[
    Scene {
        name: "live",
        about: "实时查看 (Live.dc.html): Forge's computer on 话题列表虚拟滚动, 3/5",
        start: Start::V2,
        setup: live,
        open: open_main,
    },
    Scene {
        name: "live-takeover",
        about: "实时查看 after 接管: I took over Forge on my Mac (你在操作, 交还 first, the command box)",
        start: Start::V2,
        setup: |s, cx| {
            live(s, cx);
            drop(
                s.provider
                    .take_over_run(d::DEV, d::T_VIRTUAL, d::RUN_VIRTUAL),
            );
        },
        open: open_main,
    },
];
