//! Desktop app entry: arguments and self-tests.
//!
//! Usage:
//!   desktop                          open the app (fake data until the chat-core provider lands)
//!   desktop --signed-out             start at the login view (fake provider)
//!   desktop --selftest-smoke --report <path>
//!       open the window with fake data without taking focus, render the long
//!       topic, write a JSON report (first_frame_ms, ...) and exit. Exit code 0
//!       on success, 1 on failure or timeout. `--hold-ms <n>` keeps the window
//!       open n ms after the first frame (for a screenshot with `screencapture`).
//!   desktop --screenshot <dir> [--theme light|dark|both] [--scenes ...] [--worst-case]
//!       capture every scene's window in each theme; see `selftest/mod.rs`.
//!   desktop --selftest-nav --report <path>
//!       drive the navigation actions and dialogs (views/nav/selftest.rs).
//!   desktop --selftest-timeline <n> --report <path>
//!       open a topic with n generated messages, scroll it top to bottom by
//!       code and write frame times (see views/timeline/selftest.rs).

// The skeleton defines API that the timeline, composer, navigation and
// chat-core tracks use; unused parts are not warnings yet.
#[allow(dead_code)]
mod app;
#[allow(dead_code)]
mod copy;
#[allow(dead_code)]
mod data;
mod drafts;
#[allow(dead_code)]
mod fake;
mod markdown;
mod selftest;
#[allow(dead_code)]
mod theme;
#[allow(dead_code)]
mod views;

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicI32, Ordering},
    },
    time::{Duration, Instant},
};

use gpui::{ActivationPolicy, App};

use crate::{app::Services, data::ChatProvider};

/// Exit code set by self-tests before they quit the app.
static EXIT_CODE: AtomicI32 = AtomicI32::new(0);

/// Sets the exit code a self-test reports when the app quits.
pub fn set_exit_code(code: i32) {
    EXIT_CODE.store(code, Ordering::SeqCst);
}

#[derive(Debug, Default)]
struct Args {
    selftest_smoke: bool,
    selftest_nav: bool,
    selftest_timeline: Option<usize>,
    report: Option<PathBuf>,
    signed_out: bool,
    hold_ms: u64,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--selftest-smoke" => args.selftest_smoke = true,
            "--selftest-nav" => args.selftest_nav = true,
            "--selftest-timeline" => {
                args.selftest_timeline = Some(
                    it.next()
                        .and_then(|v| v.parse().ok())
                        .ok_or("--selftest-timeline needs a message count")?,
                )
            }
            "--report" => args.report = Some(it.next().ok_or("--report needs a path")?.into()),
            "--signed-out" => args.signed_out = true,
            "--hold-ms" => {
                args.hold_ms = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or("--hold-ms needs a number")?
            }
            "--help" | "-h" => return Err(String::new()),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    if (args.selftest_smoke || args.selftest_nav || args.selftest_timeline.is_some())
        && args.report.is_none() {
        return Err("a self-test needs --report <path>".into());
    }
    Ok(args)
}

fn main() {
    let started = Instant::now();
    // `--screenshot` and other self-tests in `selftest/` parse their own arguments.
    if let Some(code) = selftest::main_if_requested() {
        std::process::exit(code);
    }
    let args = match parse_args() {
        Ok(args) => args,
        Err(error) => {
            if !error.is_empty() {
                eprintln!("{error}");
            }
            eprintln!(
                "usage: desktop [--signed-out] [--selftest-smoke|--selftest-nav --report <path> [--hold-ms <n>]] [--selftest-timeline <n> --report <path>]"
            );
            std::process::exit(2);
        }
    };

    let application = gpui_kit::application().with_assets(gpui_kit::assets::Assets);
    // A click on the Dock icon with no window open shows the main window again.
    application.on_reopen(views::nav::show_main_window);
    application.run(move |cx: &mut App| {
        gpui_kit::init(cx);
        let provider: Arc<dyn ChatProvider> = if let Some(n) = args.selftest_timeline {
            views::timeline::selftest::provider(n)
        } else if args.signed_out {
            fake::FakeProvider::signed_out()
        } else {
            fake::FakeProvider::new()
        };
        let services = Services::new(provider, cx);
        app::install_menus_and_actions(services.clone(), cx);
        // The smoke test leaves the menu bar alone; the nav test checks the item.
        views::nav::install(
            services.clone(),
            !args.selftest_smoke && args.selftest_timeline.is_none(),
            cx,
        );

        if let Some(n) = args.selftest_timeline {
            views::timeline::selftest::run(services, n, args.report.clone().unwrap(), cx);
        } else if args.selftest_nav {
            views::nav::selftest::run(services, args.report.clone().unwrap(), finish, cx);
        } else if args.selftest_smoke {
            run_smoke(
                services,
                args.report.clone().unwrap(),
                started,
                args.hold_ms,
                cx,
            );
        } else {
            views::nav::restore(&services, cx);
            if let Err(error) = app::open_main_window(services, true, None, cx) {
                eprintln!("failed to open the window: {error}");
                cx.quit();
            }
            cx.activate(true);
        }
    });
    std::process::exit(EXIT_CODE.load(Ordering::SeqCst));
}

/// Opens the main window on the long topic without taking focus, waits for
/// the first frame, writes the report and quits.
fn run_smoke(services: Services, report: PathBuf, started: Instant, hold_ms: u64, cx: &mut App) {
    // No Dock icon and no activation: the user keeps working while this runs.
    cx.set_activation_policy(ActivationPolicy::Accessory);
    services.nav.update(cx, |nav, cx| {
        nav.open_topic(fake::LONG_TOPIC_GROUP.into(), fake::LONG_TOPIC.into(), cx)
    });

    let provider = services.provider.clone();
    let report_path = report.clone();
    let root_slot: std::rc::Rc<std::cell::RefCell<Option<gpui::Entity<app::RootView>>>> =
        Default::default();
    let slot = root_slot.clone();
    let hook: app::FirstFrameHook = Box::new(move |window, cx| {
        let first_frame_ms = started.elapsed().as_secs_f64() * 1000.0;
        let root = slot.borrow().clone();
        let messages_in_window = root
            .map(|r| r.read(cx).timeline().read(cx).message_count())
            .unwrap_or(0);
        let groups = provider.groups().len();
        let topics = provider.topics(fake::LONG_TOPIC_GROUP).len();
        let ok = messages_in_window > 0 && groups > 0 && topics > 0;
        let json = serde_json::json!({
            "ok": ok,
            "first_frame_ms": first_frame_ms,
            "window_title": env!("PRODUCT_NAME"),
            "appearance": if theme::is_dark(cx) { "dark" } else { "light" },
            "viewport": { "width": f32::from(window.viewport_size().width), "height": f32::from(window.viewport_size().height) },
            "groups": groups,
            "topics_in_group": topics,
            "topic": fake::LONG_TOPIC,
            "topic_messages": fake::LONG_TOPIC_MESSAGES,
            "messages_in_window": messages_in_window,
        });
        if hold_ms == 0 {
            finish(&report_path, json, ok, cx);
        } else {
            let path = report_path.clone();
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(hold_ms))
                    .await;
                let _ = cx.update(|cx| finish(&path, json, ok, cx));
            })
            .detach();
        }
    });

    match app::open_main_window(services, false, Some(hook), cx) {
        Ok(root) => *root_slot.borrow_mut() = Some(root),
        Err(error) => {
            let json = serde_json::json!({ "ok": false, "error": error });
            finish(&report, json, false, cx);
            return;
        }
    }

    // Fail instead of hanging when no frame arrives.
    let timeout_report = report;
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(Duration::from_secs(30) + Duration::from_millis(hold_ms))
            .await;
        let _ = cx.update(|cx| {
            let json = serde_json::json!({ "ok": false, "error": "no frame within 30 s" });
            finish(&timeout_report, json, false, cx);
        });
    })
    .detach();
}

fn finish(path: &PathBuf, json: serde_json::Value, ok: bool, cx: &mut App) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let text = serde_json::to_string_pretty(&json).unwrap_or_default();
    if let Err(error) = std::fs::write(path, text + "\n") {
        eprintln!("cannot write {}: {error}", path.display());
        EXIT_CODE.store(1, Ordering::SeqCst);
    } else if !ok {
        EXIT_CODE.store(1, Ordering::SeqCst);
    }
    cx.quit();
    // On macOS `run` does not return after `quit`, so exit with the code here.
    std::process::exit(EXIT_CODE.load(Ordering::SeqCst));
}
