//! The Windows embedding check inside a real GPUI window (docs/plans/windows.zh.md, C2):
//! one WebView2 page as a child of GPUI's own `HWND`, then a look at the screen.
//!
//! `cargo run -p desktop --example win_embed_gpui -- <out-dir>` writes
//! `<out-dir>/report.json` and screenshots of the window. Exit status 0 when the page shows
//! above GPUI's content and a click reaches it.
//!
//! GPUI draws through a topmost DirectComposition tree by default, which covers every
//! child window. With `GPUI_DISABLE_DIRECT_COMPOSITION=1` (what the app sets on Windows,
//! `ui::web_embed::prepare_process`) it presents to the `HWND` and child windows show. Run
//! it both ways to see the difference.

#[cfg(windows)]
mod check {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use artifact_view::win_support::{capture_screen_png, client_rect_on_screen, png_pixel};
    use artifact_view::{
        ArtifactEvent, Capabilities, Content, EmbedOptions, EmbeddedArtifact, MainThread, Rect,
        VersionSpec,
    };
    use gpui::{
        App, AppContext as _, AsyncApp, Bounds, Context, IntoElement, Render, Styled, Window,
        WindowBounds, WindowOptions, div, point, px, rgb, size,
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use serde_json::{Value, json};

    /// GPUI paints the whole window in this blue.
    const HOST_RGB: (u8, u8, u8) = (0x1e, 0x5a, 0xc8);
    const PAGE_RGB: (u8, u8, u8) = (255, 214, 10);

    const PAGE: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><style>
  html, body { margin: 0; height: 100%; background: rgb(255, 214, 10); font: 16px system-ui; }
  h1 { margin: 0; padding: 16px 24px; font-size: 20px; }
  #b { position: fixed; left: 40px; top: 120px; width: 160px; height: 48px; font-size: 16px; }
</style></head><body>
<h1>WebView2 in a GPUI window · 产出</h1>
<button id="b">Click me</button>
<script>
  document.getElementById('b').addEventListener('click', function (e) {
    e.target.textContent = 'clicked';
    window.artifact.emitEvent('clicked', { trusted: e.isTrusted });
  });
  window.artifact.emitEvent('ready', { width: innerWidth, height: innerHeight, dpr: devicePixelRatio });
</script></body></html>"#;

    struct Host;

    impl Render for Host {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().bg(rgb(0x1e5ac8))
        }
    }

    #[derive(Default)]
    struct Seen {
        ready: Option<Value>,
        load_failed: Option<String>,
        clicked: Option<(Value, bool)>,
    }

    fn near(a: Option<(u8, u8, u8)>, b: (u8, u8, u8)) -> bool {
        a.is_some_and(|a| {
            let d = |x: u8, y: u8| (x as i32 - y as i32).abs();
            d(a.0, b.0) + d(a.1, b.1) + d(a.2, b.2) <= 24
        })
    }

    async fn wait(cx: &mut AsyncApp, limit: Duration, mut done: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + limit;
        loop {
            if done() {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            cx.background_executor()
                .timer(Duration::from_millis(50))
                .await;
        }
    }

    pub fn main() {
        let out = std::env::args()
            .nth(1)
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("win-embed-gpui"));
        std::fs::create_dir_all(&out).expect("create out dir");
        let composition = !std::env::var("GPUI_DISABLE_DIRECT_COMPOSITION")
            .is_ok_and(|v| v == "1" || v == "true");
        artifact_view::set_user_data_dir(
            std::env::temp_dir().join(format!("win-embed-gpui-{}", std::process::id())),
        );
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(150));
            eprintln!("win_embed_gpui: timed out");
            std::process::exit(2);
        });

        gpui_platform::application().run(move |cx: &mut App| {
            let bounds = Bounds::new(point(px(80.), px(80.)), size(px(640.), px(480.)));
            let window = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(bounds)),
                        ..Default::default()
                    },
                    |_, cx| cx.new(|_| Host),
                )
                .expect("open the window");
            cx.spawn(async move |cx: &mut AsyncApp| {
                let started = Instant::now();
                let mut checks = serde_json::Map::new();
                let mut details = serde_json::Map::new();
                details.insert("directComposition".into(), json!(composition));
                // Let GPUI draw its first frames.
                wait(cx, Duration::from_millis(1500), || false).await;

                let seen = Rc::new(RefCell::new(Seen::default()));
                let seen2 = seen.clone();
                let attached = window.update(cx, |_, window, _| {
                    let handle = HasWindowHandle::window_handle(window).ok()?;
                    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
                        return None;
                    };
                    let hwnd = win32.hwnd.get();
                    let scale = window.scale_factor() as f64;
                    let mut options = EmbedOptions::new(
                        "gpui-a1",
                        VersionSpec::new(
                            1,
                            Content::single("index.html", PAGE.as_bytes().to_vec()),
                        ),
                    );
                    options.capabilities = Capabilities::ALL;
                    options.corner_radius = 12.0;
                    let embed = EmbeddedArtifact::attach(
                        MainThread::current(),
                        hwnd,
                        options,
                        move |event| {
                            let mut s = seen2.borrow_mut();
                            match event {
                                ArtifactEvent::LoadFailed { reason, .. } => {
                                    s.load_failed = Some(reason)
                                }
                                ArtifactEvent::Emit {
                                    name,
                                    payload,
                                    user_action,
                                    ..
                                } => match name.as_str() {
                                    "ready" => s.ready = Some(payload),
                                    "clicked" => s.clicked = Some((payload, user_action)),
                                    _ => {}
                                },
                                _ => {}
                            }
                        },
                    )
                    .ok()?;
                    Some((embed, hwnd, scale))
                });
                let Ok(Some((embed, hwnd, scale))) = attached else {
                    eprintln!("win_embed_gpui: could not attach");
                    std::process::exit(1);
                };
                details.insert("scale".into(), json!(scale));
                let full = Rect::new(60.0, 60.0, 400.0, 300.0);
                embed.set_frame(full, Rect::new(0.0, 0.0, 4000.0, 4000.0), scale);

                let loaded = wait(cx, Duration::from_secs(90), || {
                    let s = seen.borrow();
                    s.ready.is_some() || s.load_failed.is_some()
                })
                .await;
                checks.insert(
                    "pageLoaded".into(),
                    json!(loaded && seen.borrow().ready.is_some()),
                );
                details.insert("page".into(), json!(seen.borrow().ready.clone()));
                details.insert(
                    "loadFailed".into(),
                    json!(seen.borrow().load_failed.clone()),
                );
                wait(cx, Duration::from_millis(1500), || false).await;

                let (ox, oy, cw, ch) = client_rect_on_screen(hwnd);
                details.insert("client".into(), json!([ox, oy, cw, ch]));
                let shot = |name: &str| -> Option<Vec<u8>> {
                    let png = capture_screen_png(ox, oy, cw, ch)?;
                    std::fs::write(out.join(name), &png).ok()?;
                    Some(png)
                };
                let at = |png: &Option<Vec<u8>>, x: f64, y: f64| {
                    png.as_ref().and_then(|p| {
                        png_pixel(p, (x * scale).round() as u32, (y * scale).round() as u32)
                    })
                };
                let first = shot("gpui-embed.png");
                checks.insert(
                    "gpuiPaints".into(),
                    json!(near(at(&first, 20.0, 20.0), HOST_RGB)),
                );
                checks.insert(
                    "embedAboveGpui".into(),
                    json!(near(at(&first, 360.0, 300.0), PAGE_RGB)),
                );
                details.insert(
                    "pixels".into(),
                    json!({
                        "gpui": format!("{:?}", at(&first, 20.0, 20.0)),
                        "page": format!("{:?}", at(&first, 360.0, 300.0)),
                        "corner": format!("{:?}", at(&first, 62.0, 62.0)),
                    }),
                );

                let posted = embed.post_click(120.0, 144.0);
                let clicked = wait(cx, Duration::from_secs(8), || {
                    seen.borrow().clicked.is_some()
                })
                .await;
                checks.insert("clickReachesPage".into(), json!(posted && clicked));
                if let Some((payload, user_action)) = seen.borrow().clicked.clone() {
                    checks.insert(
                        "clickCountsAsViewerInput".into(),
                        json!(user_action && payload["trusted"] == json!(true)),
                    );
                }
                checks.insert("pageHasFocusAfterClick".into(), json!(embed.is_focused()));

                // Under a GPUI overlay: hidden, GPUI's pixels show again.
                embed.set_hidden(true);
                wait(cx, Duration::from_millis(800), || false).await;
                let hidden = shot("gpui-embed-hidden.png");
                checks.insert(
                    "hiddenShowsGpui".into(),
                    json!(near(at(&hidden, 360.0, 300.0), HOST_RGB)),
                );
                embed.set_hidden(false);
                // Scrolled: the top 100 pt clipped away.
                embed.set_frame(full, Rect::new(0.0, 160.0, 4000.0, 4000.0), scale);
                wait(cx, Duration::from_millis(800), || false).await;
                let clipped = shot("gpui-embed-clipped.png");
                checks.insert(
                    "clippedPartShowsGpui".into(),
                    json!(near(at(&clipped, 260.0, 110.0), HOST_RGB)),
                );
                checks.insert(
                    "unclippedPartShowsPage".into(),
                    json!(near(at(&clipped, 260.0, 320.0), PAGE_RGB)),
                );
                embed.close();

                let ok = checks.values().all(|v| v == &json!(true));
                let report = json!({
                    "ok": ok,
                    "durationMs": started.elapsed().as_millis() as u64,
                    "checks": checks,
                    "details": details,
                });
                let text = serde_json::to_string_pretty(&report).unwrap();
                let _ = std::fs::write(out.join("report.json"), text.clone() + "\n");
                println!("{text}");
                std::process::exit(if ok { 0 } else { 1 });
            })
            .detach();
        });
    }
}

fn main() {
    #[cfg(windows)]
    check::main();
    #[cfg(not(windows))]
    eprintln!("the GPUI embedding check runs only on Windows");
}
