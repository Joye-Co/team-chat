//! The Windows embedding prototype (docs/plans/windows.zh.md, C2): one WebView2 page from
//! memory, embedded as a child of a plain host window through [`EmbeddedArtifact`].
//!
//! `cargo run -p artifact-view --example win_embed_proto -- <out-dir>` writes
//! `<out-dir>/report.json` and screenshots of the host window (`embed.png`,
//! `embed-clipped.png`, `embed-hidden.png`) plus the page's own capture
//! (`embed-snapshot.png`). Exit status 0 when every check passed.
//!
//! It checks what only a real Windows desktop shows: the child window paints above the
//! host's content, follows `set_frame` (also clipped and hidden), has round corners, a
//! click reaches the page and counts as the viewer's input, and the bridge works at the
//! page's own origin.

#[cfg(windows)]
mod proto {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use artifact_view::win_support::{
        capture_screen_png, client_rect_on_screen, create_host_window, png_pixel, pump, scale_of,
        set_dpi_aware,
    };
    use artifact_view::{
        ArtifactEvent, Capabilities, Content, EmbedOptions, EmbeddedArtifact, MainThread, Rect,
        VersionSpec, origin_host,
    };
    use serde_json::{Value, json};

    const ARTIFACT_ID: &str = "proto-a1";
    /// The host window's background, `0x00BBGGRR`: a blue no page draws.
    const HOST_COLOR: u32 = 0x00C8_5A1E;
    const HOST_RGB: (u8, u8, u8) = (0x1e, 0x5a, 0xc8);
    /// The page's background.
    const PAGE_RGB: (u8, u8, u8) = (255, 214, 10);

    const PAGE: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>proto</title>
<style>
  html, body { margin: 0; height: 100%; background: rgb(255, 214, 10); font: 16px system-ui; }
  h1 { margin: 0; padding: 16px 24px; font-size: 20px; }
  #b { position: fixed; left: 40px; top: 120px; width: 160px; height: 48px; font-size: 16px; }
  #log { position: fixed; left: 24px; top: 190px; }
</style></head><body>
<h1>WebView2 embedded · 产出</h1>
<button id="b">Click me</button>
<div id="log">waiting</div>
<script>
(async function () {
  var r = { origin: location.origin, ua: navigator.userAgent, dpr: devicePixelRatio,
            width: innerWidth, height: innerHeight };
  try { r.data = await window.artifact.getData(); } catch (e) { r.dataError = String(e); }
  try { await fetch('https://example.com/'); r.fetchFails = false; } catch (e) { r.fetchFails = true; }
  r.hostObjects = !!(window.chrome && window.chrome.webview && window.chrome.webview.hostObjects
    && window.chrome.webview.hostObjects.sync);
  document.getElementById('b').addEventListener('click', function (e) {
    document.getElementById('log').textContent = 'clicked';
    window.artifact.emitEvent('clicked', { trusted: e.isTrusted });
  });
  document.getElementById('log').textContent = 'ready';
  window.artifact.emitEvent('proto-results', r);
})();
</script></body></html>"#;

    #[derive(Default)]
    struct Seen {
        loaded: bool,
        load_failed: Option<String>,
        results: Option<Value>,
        clicked: Option<(Value, bool)>,
        focus: Vec<bool>,
        closed: bool,
    }

    fn near(a: Option<(u8, u8, u8)>, b: (u8, u8, u8)) -> bool {
        a.is_some_and(|a| {
            let d = |x: u8, y: u8| (x as i32 - y as i32).abs();
            d(a.0, b.0) + d(a.1, b.1) + d(a.2, b.2) <= 24
        })
    }

    pub fn main() {
        let out = std::env::args()
            .nth(1)
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("artifact-view-proto"));
        std::fs::create_dir_all(&out).expect("create out dir");
        let started = Instant::now();
        set_dpi_aware();
        let main = MainThread::current();
        artifact_view::set_user_data_dir(
            std::env::temp_dir().join(format!("artifact-view-proto-{}", std::process::id())),
        );

        let host = create_host_window("embed prototype", 640, 480, HOST_COLOR);
        assert!(host != 0, "host window");
        let scale = scale_of(host);

        let seen = Rc::new(RefCell::new(Seen::default()));
        let seen2 = seen.clone();
        let mut options = EmbedOptions::new(
            ARTIFACT_ID,
            VersionSpec::new(1, Content::single("index.html", PAGE.as_bytes().to_vec()))
                .with_data(json!({ "hello": "世界" })),
        );
        options.capabilities = Capabilities::ALL;
        options.corner_radius = 24.0;
        let embed = EmbeddedArtifact::attach(main, host, options, move |event| {
            let mut s = seen2.borrow_mut();
            match event {
                ArtifactEvent::Loaded { .. } => s.loaded = true,
                ArtifactEvent::LoadFailed { reason, .. } => s.load_failed = Some(reason),
                ArtifactEvent::Emit {
                    name,
                    payload,
                    user_action,
                    ..
                } => match name.as_str() {
                    "proto-results" => s.results = Some(payload),
                    "clicked" => s.clicked = Some((payload, user_action)),
                    _ => {}
                },
                ArtifactEvent::FocusChanged { focused } => s.focus.push(focused),
                ArtifactEvent::Closed => s.closed = true,
                _ => {}
            }
        })
        .expect("attach");

        // In points from the host's top-left corner.
        let full = Rect::new(60.0, 60.0, 400.0, 300.0);
        let whole = Rect::new(0.0, 0.0, 640.0, 480.0);
        embed.set_frame(full, whole, scale);

        let mut checks = serde_json::Map::new();
        let mut details = serde_json::Map::new();
        details.insert("scale".into(), json!(scale));
        let mut check = |name: &str, ok: bool| {
            checks.insert(name.into(), json!(ok));
        };

        let ready = pump(Duration::from_secs(90), || {
            let s = seen.borrow();
            (s.loaded && s.results.is_some()) || s.load_failed.is_some()
        });
        check("pageLoaded", ready && seen.borrow().load_failed.is_none());
        if let Some(reason) = &seen.borrow().load_failed {
            details.insert("loadFailed".into(), json!(reason));
        }
        details.insert("loadMs".into(), json!(started.elapsed().as_millis() as u64));
        let results = seen.borrow().results.clone().unwrap_or(Value::Null);
        check(
            "ownOrigin",
            results["origin"]
                == json!(format!(
                    "artifact://{}",
                    origin_host(ARTIFACT_ID, 1).unwrap()
                )),
        );
        check(
            "bridgeGetData",
            results["data"] == json!({ "hello": "世界" }),
        );
        check("fetchFails", results["fetchFails"] == json!(true));
        check("noHostObjects", results["hostObjects"] == json!(false));
        check(
            "pageSizeFollowsFrame",
            results["width"] == json!(400) && results["height"] == json!(300),
        );
        details.insert("page".into(), results);

        // Let the page paint, then look at the screen.
        pump(Duration::from_millis(1500), || false);
        let (ox, oy, cw, ch) = client_rect_on_screen(host);
        details.insert("client".into(), json!([ox, oy, cw, ch]));
        let px = |points: f64| (points * scale).round() as u32;
        let shot = |name: &str| -> Option<Vec<u8>> {
            let png = capture_screen_png(ox, oy, cw, ch)?;
            std::fs::write(out.join(name), &png).ok()?;
            Some(png)
        };
        let at = |png: &Option<Vec<u8>>, x: f64, y: f64| {
            png.as_ref().and_then(|p| png_pixel(p, px(x), px(y)))
        };

        let first = shot("embed.png");
        check("screenshotTaken", first.is_some());
        check("hostPaints", near(at(&first, 20.0, 20.0), HOST_RGB));
        check("embedAboveHost", near(at(&first, 360.0, 300.0), PAGE_RGB));
        check("embedEdgesInPlace", {
            near(at(&first, 60.0 + 200.0, 60.0 + 3.0), PAGE_RGB)
                && near(at(&first, 60.0 + 200.0, 60.0 - 3.0), HOST_RGB)
                && near(at(&first, 60.0 + 397.0, 60.0 + 150.0), PAGE_RGB)
                && near(at(&first, 60.0 + 403.0, 60.0 + 150.0), HOST_RGB)
                && near(at(&first, 60.0 + 200.0, 60.0 + 297.0), PAGE_RGB)
                && near(at(&first, 60.0 + 200.0, 60.0 + 303.0), HOST_RGB)
        });
        check(
            "cornersRound",
            near(at(&first, 60.0 + 3.0, 60.0 + 3.0), HOST_RGB),
        );
        details.insert(
            "pixels".into(),
            json!({
                "host": format!("{:?}", at(&first, 20.0, 20.0)),
                "page": format!("{:?}", at(&first, 360.0, 300.0)),
                "corner": format!("{:?}", at(&first, 63.0, 63.0)),
            }),
        );

        // A real click on the button (40..200 × 120..168 in the page).
        let posted = embed.post_click(120.0, 144.0);
        check("clickPosted", posted);
        let clicked = pump(Duration::from_secs(10), || seen.borrow().clicked.is_some());
        check("clickReachesPage", clicked);
        if let Some((payload, user_action)) = seen.borrow().clicked.clone() {
            check("clickIsTrusted", payload["trusted"] == json!(true));
            check("clickCountsAsViewerInput", user_action);
        }
        pump(Duration::from_millis(300), || false);
        details.insert("focusEvents".into(), json!(seen.borrow().focus.clone()));
        check("focusFollowsClick", embed.is_focused());

        let text: Rc<RefCell<Option<Option<String>>>> = Rc::new(RefCell::new(None));
        let text2 = text.clone();
        embed.evaluate_text("document.getElementById('log').textContent", move |t| {
            *text2.borrow_mut() = Some(t)
        });
        pump(Duration::from_secs(10), || text.borrow().is_some());
        check(
            "evaluateText",
            text.borrow().clone().flatten().as_deref() == Some("clicked"),
        );

        // The page's own capture (what the host draws under an overlay).
        let snap: Rc<RefCell<Option<Option<Vec<u8>>>>> = Rc::new(RefCell::new(None));
        let snap2 = snap.clone();
        embed.snapshot_png(move |png| *snap2.borrow_mut() = Some(png));
        pump(Duration::from_secs(15), || snap.borrow().is_some());
        let snapshot = snap.borrow().clone().flatten();
        check(
            "snapshotPng",
            snapshot.as_ref().is_some_and(|p| p.starts_with(b"\x89PNG")),
        );
        if let Some(png) = &snapshot {
            let _ = std::fs::write(out.join("embed-snapshot.png"), png);
            details.insert("snapshotBytes".into(), json!(png.len()));
        }

        // Scrolled: the host clips the top 100 pt of the page away.
        embed.set_frame(full, Rect::new(0.0, 160.0, 640.0, 320.0), scale);
        pump(Duration::from_millis(800), || false);
        let clipped = shot("embed-clipped.png");
        check(
            "clippedPartShowsHost",
            near(at(&clipped, 260.0, 110.0), HOST_RGB),
        );
        check(
            "unclippedPartShowsPage",
            near(at(&clipped, 260.0, 320.0), PAGE_RGB),
        );

        // Under a host overlay.
        embed.set_frame(full, whole, scale);
        embed.set_hidden(true);
        pump(Duration::from_millis(800), || false);
        let hidden = shot("embed-hidden.png");
        check("hiddenShowsHost", near(at(&hidden, 360.0, 300.0), HOST_RGB));
        check(
            "hiddenResignsFocus",
            !embed.is_focused() && !embed.is_shown(),
        );
        embed.set_hidden(false);
        pump(Duration::from_millis(500), || false);
        let again = shot("embed-shown-again.png");
        check("shownAgain", near(at(&again, 360.0, 300.0), PAGE_RGB));

        embed.close();
        pump(Duration::from_secs(5), || seen.borrow().closed);
        check("closedEvent", seen.borrow().closed);
        pump(Duration::from_millis(500), || false);
        let gone = shot("embed-closed.png");
        check("closedRemovesView", near(at(&gone, 360.0, 300.0), HOST_RGB));

        let ok = checks.values().all(|v| v == &json!(true));
        let report = json!({
            "ok": ok,
            "durationMs": started.elapsed().as_millis() as u64,
            "checks": checks,
            "details": details,
        });
        let text = serde_json::to_string_pretty(&report).unwrap();
        std::fs::write(out.join("report.json"), text.clone() + "\n").expect("write report");
        println!("{text}");
        std::process::exit(if ok { 0 } else { 1 });
    }
}

fn main() {
    #[cfg(windows)]
    proto::main();
    #[cfg(not(windows))]
    eprintln!("the embedding prototype runs only on Windows");
}
