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
//! (0 = every PNG written). Scenes live in [`scenes::SCENES`].

pub mod capture;
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
use gpui_component::{Theme, ThemeMode};

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
        for s in scenes::SCENES {
            println!("{:<12} {}", s.name, s.about);
        }
        return Some(0);
    }
    if !args.iter().any(|a| a == "--screenshot") {
        return None;
    }
    match parse(&args) {
        Ok(options) => Some(run(options)),
        Err(error) => {
            eprintln!("{error}\n{USAGE}\nscenes: {}", scene_names());
            Some(2)
        }
    }
}

fn scene_names() -> String {
    scenes::SCENES
        .iter()
        .map(|s| s.name)
        .collect::<Vec<_>>()
        .join(",")
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        dir: PathBuf::new(),
        modes: vec![Mode::Light, Mode::Dark],
        scenes: scenes::SCENES.iter().collect(),
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

fn provider(start: Start, worst_case: bool) -> Arc<dyn ChatProvider> {
    let base: Arc<dyn ChatProvider> = match start {
        Start::SignedOut => fake::FakeProvider::signed_out(),
        Start::SignedIn | Start::Empty => fake::FakeProvider::new(),
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
    if let Err(error) = std::fs::create_dir_all(&options.dir) {
        eprintln!("cannot create {}: {error}", options.dir.display());
        return 1;
    }
    let code = std::rc::Rc::new(std::cell::Cell::new(1));
    let result = code.clone();
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            // No Dock icon and no activation: the user keeps working.
            cx.set_activation_policy(ActivationPolicy::Accessory);
            let started = Instant::now();
            cx.spawn(async move |cx| {
                let ok = run_all(&options, cx).await;
                eprintln!(
                    "screenshots done in {:.1} s",
                    started.elapsed().as_secs_f32()
                );
                code.set(if ok { 0 } else { 1 });
                cx.update(|cx| cx.quit());
            })
            .detach();
            // Never hang: quit after a generous per-shot budget.
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(Duration::from_secs(60 + 15 * 20))
                    .await;
                eprintln!("screenshot self-test timed out");
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
    result.get()
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
                Theme::change(theme_mode, Some(window), cx);
                window.refresh();
            })
        })
        .map_err(|e| e.to_string())?;
        // Three frames: layout, then lists that measure themselves, then images.
        for _ in 0..3 {
            next_frame(handle, cx).await?;
        }
        cx.background_executor().timer(options.settle).await;
        next_frame(handle, cx).await?;
        let number = cx
            .update(|cx| handle.update(cx, |_, window, _| capture::window_number(window)))
            .map_err(|e| e.to_string())??;
        let path = png.to_path_buf();
        cx.background_executor()
            .spawn(async move { capture::capture(number, &path) })
            .await?;
        let size = capture::png_size(png).ok_or("the PNG has no size")?;
        Ok((number, size))
    }
    .await;
    let _ = cx.update(|cx| handle.update(cx, |_, window, _| window.remove_window()));
    cx.background_executor()
        .timer(Duration::from_millis(150))
        .await;
    result
}

/// Resolves after the window draws its next frame (or fails after 5 s).
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
    let timeout = cx.background_executor().timer(Duration::from_secs(5));
    futures::pin_mut!(timeout);
    match futures::future::select(rx, timeout).await {
        futures::future::Either::Left((Ok(()), _)) => Ok(()),
        _ => Err("no frame within 5 s".into()),
    }
}
