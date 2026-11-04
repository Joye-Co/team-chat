//! Self-tests that drive the real app. `main.rs` hands over to
//! [`main_if_requested`] before it parses its own arguments.
//!
//! Screenshot self-test:
//!
//! ```text
//! desktop --screenshot <dir> [--theme light|dark|both] [--scenes main,login,...]
//!         [--worst-case] [--size 1440x900] [--settle-ms 400] [--list]
//! ```
//!
//! For each scene and theme it opens the window with the fake provider
//! (no Dock icon, no focus, no activation), forces the theme (the NSApp
//! appearance and gpui-component's theme mode), waits for the frames, captures
//! only that window with `screencapture -l <windowNumber> -o -x`, writes
//! `<dir>/<scene>-<theme>.png` (`-worst` before `.png` with `--worst-case`)
//! and `<dir>/screenshots.json`, closes the window and finally exits
//! (0 = every PNG written and every ready and verify check passed, else 1).
//! Scenes live in [`scenes::all`].

pub mod capture;
pub mod clock;
pub mod scenes;
pub mod worst_case;

use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use gpui::{
    ActivationPolicy, AnyWindowHandle, App, AsyncApp, Pixels, Size, WindowAppearance, px, size,
};
use gpui_component::ThemeMode;

use crate::{app::Services, data::ChatProvider, fake};
use scenes::{Scene, Start};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Light,
    Dark,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::Light => "light",
            Mode::Dark => "dark",
        }
    }
}

struct Options {
    dir: PathBuf,
    modes: Vec<Mode>,
    scenes: Vec<&'static Scene>,
    worst_case: bool,
    size: Size<Pixels>,
    settle: Duration,
}

const USAGE: &str = "usage: desktop --screenshot <dir> [--theme light|dark|both] \
[--scenes a,b,...] [--worst-case] [--size WxH] [--settle-ms n] [--list]";

/// Runs a self-test when the arguments ask for one and returns its exit code.
pub fn main_if_requested() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--list-scenes") {
        for s in scenes::all() {
            println!("{:<12} {}", s.name, s.about);
        }
        return Some(0);
    }
    if !args.iter().any(|a| a == "--screenshot") {
        return None;
    }
    crate::ui::rows::draw_as_key_window();
    // Every scene reads the artboards' morning; the real app keeps the real clock.
    clock::pin_to_artboards();
    match parse(&args) {
        Ok(options) => Some(run(options)),
        Err(error) => {
            eprintln!("{error}\n{USAGE}\nscenes: {}", scene_names());
            Some(2)
        }
    }
}

fn scene_names() -> String {
    scenes::all()
        .iter()
        .map(|s| s.name)
        .collect::<Vec<_>>()
        .join(",")
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        dir: PathBuf::new(),
        modes: vec![Mode::Light, Mode::Dark],
        scenes: scenes::all(),
        worst_case: false,
        size: size(px(1440.), px(900.)),
        settle: Duration::from_millis(400),
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().cloned().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--screenshot" => options.dir = value("--screenshot")?.into(),
            "--theme" => {
                options.modes = match value("--theme")?.as_str() {
                    "light" => vec![Mode::Light],
                    "dark" => vec![Mode::Dark],
                    "both" => vec![Mode::Light, Mode::Dark],
                    other => return Err(format!("unknown theme: {other}")),
                }
            }
            "--scenes" => {
                options.scenes = value("--scenes")?
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(|s| scenes::find(s).ok_or(format!("unknown scene: {s}")))
                    .collect::<Result<_, _>>()?;
            }
            "--worst-case" => options.worst_case = true,
            "--size" => {
                let v = value("--size")?;
                let (w, h) = v
                    .split_once('x')
                    .and_then(|(w, h)| Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?)))
                    .ok_or(format!("--size wants WxH, got {v}"))?;
                options.size = size(px(w), px(h));
            }
            "--settle-ms" => {
                let v = value("--settle-ms")?;
                options.settle =
                    Duration::from_millis(v.parse().map_err(|_| format!("bad --settle-ms {v}"))?);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    if options.dir.as_os_str().is_empty() {
        return Err("--screenshot needs a directory".into());
    }
    if options.scenes.is_empty() {
        return Err("no scenes".into());
    }
    Ok(options)
}

// --- v11/inc-diagrams: scenes that build their own data read the worst-case flag ---
static WORST_CASE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// True while the self-test runs with `--worst-case`.
pub fn worst_case() -> bool {
    WORST_CASE.load(std::sync::atomic::Ordering::Relaxed)
}
// --- end v11/inc-diagrams ---

fn provider(start: Start, worst_case: bool) -> Arc<dyn ChatProvider> {
    let base: Arc<dyn ChatProvider> = match start {
        Start::SignedOut => fake::FakeProvider::signed_out(),
        Start::SignedIn | Start::Empty => fake::FakeProvider::new(),
        Start::V2 => fake::v2::V2Samples::new(),
        Start::V2Quiet => fake::v2::V2Samples::quiet(),
        // --- inc-refs
        Start::V2Refs => fake::v2::V2Samples::with_refs(),
        Start::V2RefParts => fake::v2::V2Samples::with_card_parts(),
        // --- provider track ---
        Start::Core => {
            static SHOT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = SHOT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "{}-selftest-core-{}-{n}",
                env!("PRODUCT_SLUG"),
                std::process::id()
            ));
            crate::provider::seeded::seeded(&root, clock::now_ms() as u64)
        }
        // --- end provider track ---
        Start::Offline => {
            let fake = fake::FakeProvider::new();
            fake.set_connection(crate::data::ConnectionState::Reconnecting {
                attempt: 3,
                retry_in_ms: 8_000,
            });
            fake
        }
    };
    let base: Arc<dyn ChatProvider> = if start == Start::Empty {
        worst_case::Empty::new(base)
    } else {
        base
    };
    if worst_case {
        worst_case::WorstCase::new(base)
    } else {
        base
    }
}

fn run(options: Options) -> i32 {
    WORST_CASE.store(options.worst_case, std::sync::atomic::Ordering::Relaxed);
    if let Err(error) = std::fs::create_dir_all(&options.dir) {
        eprintln!("cannot create {}: {error}", options.dir.display());
        return 1;
    }
    let code = std::rc::Rc::new(std::cell::Cell::new(1));
    let result = code.clone();
    // At least the old 20-shot budget; more shots get 15 s each.
    let shots = (options.scenes.len() * options.modes.len()).max(20) as u64;
    gpui_kit::application()
        .with_assets(crate::ui::AppAssets)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            crate::render_text_like_the_mockups(cx);
            // No Dock icon and never an activation: the person who uses
            // this Mac keeps the focus. The windows draw as inactive
            // windows (grey traffic lights).
            cx.set_activation_policy(ActivationPolicy::Accessory);
            capture::keep_frames();
            let started = Instant::now();
            cx.spawn(async move |cx| {
                let ok = run_all(&options, cx).await;
                eprintln!(
                    "screenshots done in {:.1} s",
                    started.elapsed().as_secs_f32()
                );
                code.set(if ok { 0 } else { 1 });
                // AppKit ends the process with 0 when the app quits, so `run` never
                // returns: exit here with the result.
                exit_now(code.get());
            })
            .detach();
            // Never hang: quit after a generous per-shot budget.
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(Duration::from_secs(60 + 15 * shots))
                    .await;
                eprintln!("screenshot self-test timed out");
                exit_now(1);
            })
            .detach();
        });
    result.get()
}

/// Ends the self-test with `code`. `cx.quit()` would end it with 0 on macOS
/// whatever the result, because AppKit's terminate exits the process itself.
fn exit_now(code: i32) -> ! {
    use std::io::Write as _;
    let _ = std::io::stderr().flush();
    let _ = std::io::stdout().flush();
    std::process::exit(code)
}

async fn run_all(options: &Options, cx: &mut AsyncApp) -> bool {
    let mut all_ok = true;
    let mut shots = Vec::new();
    for &mode in &options.modes {
        cx.update(|cx| {
            cx.set_window_appearance(Some(match mode {
                Mode::Light => WindowAppearance::Light,
                Mode::Dark => WindowAppearance::Dark,
            }))
        });
        for scene in &options.scenes {
            let suffix = if options.worst_case { "-worst" } else { "" };
            let file = format!("{}-{}{suffix}.png", scene.name, mode.name());
            let png = options.dir.join(&file);
            let result = shoot(scene, mode, options, &png, cx).await;
            let entry = match &result {
                Ok((number, (w, h))) => {
                    eprintln!("{file}: {w}x{h} px (window {number})");
                    serde_json::json!({ "scene": scene.name, "theme": mode.name(), "file": file,
                        "ok": true, "window_number": number, "width_px": w, "height_px": h })
                }
                Err(error) => {
                    all_ok = false;
                    eprintln!("{file}: FAILED: {error}");
                    serde_json::json!({ "scene": scene.name, "theme": mode.name(), "file": file,
                        "ok": false, "error": error })
                }
            };
            shots.push(entry);
        }
    }
    cx.update(|cx| cx.set_window_appearance(None));
    let manifest = serde_json::json!({
        "ok": all_ok,
        "worst_case": options.worst_case,
        "size": { "width": f32::from(options.size.width), "height": f32::from(options.size.height) },
        "shots": shots,
    });
    let name = if options.worst_case {
        "screenshots-worst.json"
    } else {
        "screenshots.json"
    };
    let _ = std::fs::write(
        options.dir.join(name),
        serde_json::to_string_pretty(&manifest).unwrap_or_default() + "\n",
    );
    all_ok
}

async fn shoot(
    scene: &Scene,
    mode: Mode,
    options: &Options,
    png: &std::path::Path,
    cx: &mut AsyncApp,
) -> Result<(isize, (u32, u32)), String> {
    let provider = provider(scene.start, options.worst_case);
    let content = options.size;
    let handle: AnyWindowHandle = cx.update(|cx| {
        // Every scene starts from an empty media store: a scene that put its own page
        // under a sample 产出's key (the host scenes) must not leak it into a later scene
        // that shows that 产出, whatever the scene order.
        crate::views::media::store::clear(cx);
        let services = Services::new(provider, cx);
        (scene.setup)(&services, cx);
        (scene.open)(services, content, cx)
    })?;
    let theme_mode = match mode {
        Mode::Light => ThemeMode::Light,
        Mode::Dark => ThemeMode::Dark,
    };
    let result = async {
        cx.update(|cx| {
            handle.update(cx, |_, window, cx| {
                crate::theme::set_mode(theme_mode, cx);
                // The settings window keeps its own 960 x 720; the main and
                // login windows take the requested size.
                let own = crate::views::settings::WINDOW_SIZE;
                let (own_w, own_h): (f64, f64) =
                    (f32::from(own.width).into(), f32::from(own.height).into());
                let (w, h) = capture::frame_size(window)?;
                if w == own_w && (h - own_h).abs() <= 1. {
                    capture::undo_clamp(window, own_w, own_h, true)?;
                } else {
                    capture::undo_clamp(
                        window,
                        f32::from(content.width).into(),
                        f32::from(content.height).into(),
                        false,
                    )?;
                }
                // The new frame's resize callback cannot reach this window
                // while this update holds it, so GPUI would keep drawing at
                // the old size: a black band at the bottom, over the rounded
                // corners. Read the size here.
                window.bounds_changed(cx);
                window.refresh();
                capture::order_front(window)?;
                capture::draw_as_key(window)?;
                Ok::<_, String>(())
            })
        })
        .map_err(|e| e.to_string())??;
        // Three frames (layout, lists that measure themselves, images), a
        // settle pause, then one more. A frame can fail to come while AppKit
        // still reports the new window as covered; then order it front again
        // and retry, up to 8 waits.
        let mut frames = 0;
        let mut waits = 0;
        let mut settled = false;
        while frames < 4 {
            if waits == 8 {
                return Err(format!("only {frames} of 4 frames within 8 waits"));
            }
            waits += 1;
            if frames == 3 && !settled {
                cx.background_executor().timer(options.settle).await;
                settled = true;
            }
            park_pointer(handle, cx);
            match next_frame(handle, cx).await {
                Ok(()) => frames += 1,
                Err(_) => {
                    let _ = cx.update(|cx| {
                        handle.update(cx, |_, window, _| capture::order_front(window))
                    });
                }
            }
        }
        // web-embed: native content (a web page) draws on its own schedule.
        if let Some(ready) = scenes::ready_check(scene.name) {
            let deadline = Instant::now() + Duration::from_secs(20);
            while !cx.update(|cx| ready(cx)) {
                if Instant::now() > deadline {
                    return Err("the native content was not ready within 20 s".into());
                }
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let _ = next_frame(handle, cx).await;
            }
            for _ in 0..2 {
                let _ = next_frame(handle, cx).await;
            }
            cx.background_executor().timer(options.settle).await;
        }
        let number = cx
            .update(|cx| handle.update(cx, |_, window, _| capture::window_number(window)))
            .map_err(|e| e.to_string())??;
        // Hold the caret of the focused field visible while the window is
        // captured, so a capture never lands on the off half of a blink.
        hold_caret(handle, cx);
        let _ = next_frame(handle, cx).await;
        let path = png.to_path_buf();
        let shot = cx
            .background_executor()
            .spawn(async move { capture::capture(number, &path) });
        futures::pin_mut!(shot);
        loop {
            let tick = cx.background_executor().timer(Duration::from_millis(100));
            futures::pin_mut!(tick);
            match futures::future::select(shot, tick).await {
                futures::future::Either::Left((result, _)) => {
                    result?;
                    break;
                }
                futures::future::Either::Right(((), pending)) => {
                    hold_caret(handle, cx);
                    shot = pending;
                }
            }
        }
        let size = capture::png_size(png).ok_or("the PNG has no size")?;
        if let Some(verify) = scenes::verify_check(scene.name) {
            cx.update(|cx| verify(png, cx))?;
        }
        Ok((number, size))
    }
    .await;
    let _ = cx.update(|cx| handle.update(cx, |_, window, _| window.remove_window()));
    cx.background_executor()
        .timer(Duration::from_millis(150))
        .await;
    result
}

/// Tells the window the pointer is far outside it, so no row, button or
/// tooltip under the real pointer shows its hover state in the capture.
/// The real pointer does not move.
fn park_pointer(handle: AnyWindowHandle, cx: &mut AsyncApp) {
    let _ = cx.update(|cx| {
        handle.update(cx, |_, window, cx| {
            window.dispatch_event(
                gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                    position: gpui::point(px(-10_000.), px(-10_000.)),
                    pressed_button: None,
                    modifiers: gpui::Modifiers::default(),
                }),
                cx,
            );
        })
    });
}

/// Shows the caret of the focused text field without blinking for the next
/// 300 ms: gpui-component pauses the blink on every keystroke, and F20 has
/// no binding and types nothing.
fn hold_caret(handle: AnyWindowHandle, cx: &mut AsyncApp) {
    let _ = cx.update(|cx| {
        handle.update(cx, |_, window, cx| {
            if let Ok(key) = gpui::Keystroke::parse("f20") {
                window.dispatch_keystroke(key, cx);
            }
        })
    });
}

/// Resolves after the window draws its next frame (or fails after 2 s).
async fn next_frame(handle: AnyWindowHandle, cx: &mut AsyncApp) -> Result<(), String> {
    let (tx, rx) = futures::channel::oneshot::channel();
    cx.update(|cx| {
        handle.update(cx, |_, window, _| {
            window.on_next_frame(move |_, _| {
                let _ = tx.send(());
            });
            window.refresh();
        })
    })
    .map_err(|e| e.to_string())?;
    let timeout = cx.background_executor().timer(Duration::from_secs(2));
    futures::pin_mut!(timeout);
    match futures::future::select(rx, timeout).await {
        futures::future::Either::Left((Ok(()), _)) => Ok(()),
        _ => Err("no frame within 2 s".into()),
    }
}
