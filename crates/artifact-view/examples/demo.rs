//! Opens a sample 产出 in an artifact window.
//!
//! `cargo run -p artifact-view --example demo` shows it.
//! `cargo run -p artifact-view --example demo -- --selftest --report <path>` checks the
//! isolation and the bridge inside the page, writes a JSON report and exits (status 0 when
//! every check passed). The self-test never activates the app or shows a window.

#[cfg(target_os = "macos")]
mod demo {
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use artifact_view::{
        ArtifactEvent, ArtifactWindow, Capabilities, Content, OpenOptions, Presentation,
        VersionItem, VersionSpec, origin_host,
    };
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    use objc2_foundation::MainThreadMarker;
    use serde_json::{Value, json};

    const ARTIFACT_ID: &str = "selftest-a1";
    const TIMEOUT: Duration = Duration::from_secs(40);

    /// 1x1 PNG.
    const PIXEL_PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8,
        0xcf, 0xc0, 0xf0, 0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d, 0x1d, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    const V1_HTML: &str = r#"<!doctype html>
    <html><head><meta charset="utf-8"><title>sample</title>
    <link rel="stylesheet" href="style.css">
    <script src="app.js"></script>
    <script src="/..%2fsecret.js"></script>
    <script src="/%2e%2e/secret.js"></script>
    </head><body>
    <h1 id="h">Sample 产出 v1</h1>
    <p><a id="ext" href="https://example.com/">example.com</a></p>
    <img id="px" src="pixel.png" width="16" height="16">
    <script>
    (async function () {
      var r = {};
      var violations = [];
      document.addEventListener('securitypolicyviolation', function (e) { violations.push(e.violatedDirective); });
      function imgLoads(src) {
        return new Promise(function (res) {
          var i = new Image();
          i.onload = function () { res(i.naturalWidth === 1); };
          i.onerror = function () { res(false); };
          i.src = src;
          setTimeout(function () { res(false); }, 5000);
        });
      }
      try {
        r.origin = location.origin; r.host = location.host;
        r.appJsLoaded = window.__appJs === 42;
        r.cssApplied = getComputedStyle(document.getElementById('h')).color === 'rgb(1, 2, 3)';
        r.imageLoaded = await imgLoads('pixel.png');
        r.traversalBlocked = window.__secretLoaded !== true;
        try { await fetch('https://example.com/'); r.fetchHttpsFails = false; } catch (e) { r.fetchHttpsFails = true; }
        try { await fetch('http://example.com/'); r.fetchHttpFails = false; } catch (e) { r.fetchHttpFails = true; }
        r.remoteImageFails = !(await imgLoads('https://example.com/favicon.ico'));
        try {
          var ws = new WebSocket('wss://example.com/');
          r.webSocketFails = await new Promise(function (res) {
            ws.onopen = function () { res(false); }; ws.onerror = function () { res(true); };
            setTimeout(function () { res(true); }, 3000);
          });
        } catch (e) { r.webSocketFails = true; }
        try { await fetch('pixel.png'); r.selfFetchBlocked = false; } catch (e) { r.selfFetchBlocked = true; }
        r.webRtcGone = typeof RTCPeerConnection === 'undefined' && typeof webkitRTCPeerConnection === 'undefined'
          && typeof RTCDataChannel === 'undefined';
        try {
          var pc = new RTCPeerConnection({ iceServers: [{ urls: 'stun:stun.l.google.com:19302' }] });
          pc.createDataChannel('x');
          await pc.setLocalDescription(await pc.createOffer());
          r.stunGathered = await new Promise(function (res) {
            pc.onicecandidate = function (e) { if (e.candidate && / srflx /.test(e.candidate.candidate)) res(true); };
            setTimeout(function () { res(false); }, 3000);
          });
        } catch (e) { r.stunGathered = false; }
        r.data = await window.artifact.getData();
        var raw = await new Promise(function (res) {
          function on(e) { if (e.detail && e.detail.id === 'raw-1') { window.removeEventListener('artifactbridgereply', on); res(e.detail); } }
          window.addEventListener('artifactbridgereply', on);
          window.webkit.messageHandlers.bridge.postMessage({ id: 'raw-1', method: 'getData', params: null });
        });
        r.rawRoundTrip = JSON.stringify(raw.result) === JSON.stringify(r.data);
        try { await window.artifact.call('readFile', { path: '/etc/passwd' }); r.unknownMethodRejected = false; }
        catch (e) { r.unknownMethodRejected = e.code === 'unknown_method'; }
        await new Promise(function (res) { setTimeout(res, 300); });
        r.cspViolations = violations;
      } catch (e) { r.error = String(e); }
      await window.artifact.emitEvent('v1-results', r);
      document.getElementById('ext').click();
      setTimeout(function () { location.href = 'https://example.com/'; }, 100);
      setTimeout(function () { window.artifact.emitEvent('v1-after-nav', { href: location.href }); }, 1200);
    })();
    </script>
    </body></html>"#;

    const V2_HTML: &str = r#"<!doctype html><html><head><meta charset="utf-8"></head><body><h1>Sample 产出 v2</h1>
    <script>
    (async function () {
      var r = { host: location.host };
      r.data = await window.artifact.getData();
      try { await fetch('https://example.com/'); r.fetchHttpsFails = false; } catch (e) { r.fetchHttpsFails = true; }
      await window.artifact.emitEvent('v2-results', r);
    })();
    </script></body></html>"#;

    fn write_sample(dir: &Path) -> std::io::Result<PathBuf> {
        let v1 = dir.join("v1");
        std::fs::create_dir_all(&v1)?;
        std::fs::write(dir.join("secret.js"), "window.__secretLoaded = true;")?;
        std::fs::write(v1.join("index.html"), V1_HTML)?;
        std::fs::write(v1.join("app.js"), "window.__appJs = 42;")?;
        std::fs::write(v1.join("style.css"), "h1 { color: rgb(1, 2, 3); }")?;
        std::fs::write(v1.join("pixel.png"), PIXEL_PNG)?;
        Ok(v1)
    }

    fn v1_data() -> Value {
        json!({ "title": "示例", "rows": [1, 2, 3], "nested": { "ok": true, "quote": "\"</script>" } })
    }

    fn v2_data() -> Value {
        json!({ "v": 2 })
    }

    struct Report {
        started: Instant,
        checks: serde_json::Map<String, Value>,
        details: serde_json::Map<String, Value>,
        path: Option<PathBuf>,
    }

    impl Report {
        fn check(&mut self, name: &str, ok: bool) {
            self.checks.insert(name.into(), Value::Bool(ok));
        }

        /// Writes the report and returns whether every check passed.
        fn write(&self, finished: bool) -> bool {
            let ok = finished
                && !self.checks.is_empty()
                && self.checks.values().all(|v| v == &Value::Bool(true));
            let out = json!({
                "ok": ok,
                "finished": finished,
                "durationMs": self.started.elapsed().as_millis() as u64,
                "checks": self.checks,
                "details": self.details,
            });
            let text = serde_json::to_string_pretty(&out).unwrap();
            match &self.path {
                Some(p) => {
                    if let Some(parent) = p.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    std::fs::write(p, text + "\n").expect("write report");
                }
                None => println!("{text}"),
            }
            ok
        }
    }

    pub fn main() {
        let args: Vec<String> = std::env::args().collect();
        let selftest = args.iter().any(|a| a == "--selftest");
        let report_path = args
            .iter()
            .position(|a| a == "--report")
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from);

        let mtm = MainThreadMarker::new().expect("main thread");
        let app = NSApplication::sharedApplication(mtm);
        let dir = std::env::temp_dir().join(format!("artifact-view-demo-{}", std::process::id()));
        let v1 = write_sample(&dir).expect("write sample");

        if selftest {
            run_selftest(mtm, &app, v1, dir, report_path);
        } else {
            run_demo(mtm, &app, v1);
        }
    }

    fn run_demo(mtm: MainThreadMarker, app: &NSApplication, v1: PathBuf) {
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        let mut options = OpenOptions::new(
            "Sample",
            ARTIFACT_ID,
            VersionSpec::new(1, Content::Folder(v1.clone())).with_data(v1_data()),
        );
        options.versions = vec![
            VersionItem {
                version: 1,
                label: "v1".into(),
            },
            VersionItem {
                version: 2,
                label: "v2".into(),
            },
        ];
        options.inspectable = true;
        let slot: Rc<RefCell<Option<ArtifactWindow>>> = Rc::new(RefCell::new(None));
        let slot2 = slot.clone();
        let window = ArtifactWindow::open(mtm, options, move |event| {
            println!("event: {event:?}");
            match event {
                ArtifactEvent::VersionSelected { version } => {
                    let spec = if version == 1 {
                        VersionSpec::new(1, Content::Folder(v1.clone())).with_data(v1_data())
                    } else {
                        VersionSpec::new(
                            2,
                            Content::single("index.html", V2_HTML.as_bytes().to_vec()),
                        )
                        .with_data(v2_data())
                    };
                    if let Some(w) = slot2.borrow().as_ref() {
                        let _ = w.show_version(spec);
                    }
                }
                ArtifactEvent::Closed => NSApplication::sharedApplication(mtm).terminate(None),
                _ => {}
            }
        })
        .expect("open window");
        *slot.borrow_mut() = Some(window);
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
        app.run();
    }

    fn run_selftest(
        mtm: MainThreadMarker,
        app: &NSApplication,
        v1: PathBuf,
        dir: PathBuf,
        report_path: Option<PathBuf>,
    ) {
        // Prohibited: no Dock icon, no menu bar, the app can never become active.
        app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);

        let report = Arc::new(Mutex::new(Report {
            started: Instant::now(),
            checks: Default::default(),
            details: Default::default(),
            path: report_path,
        }));
        {
            let report = report.clone();
            let dir = dir.clone();
            std::thread::spawn(move || {
                std::thread::sleep(TIMEOUT);
                let r = report.lock().unwrap_or_else(|e| e.into_inner());
                r.write(false);
                eprintln!("selftest: timed out");
                let _ = std::fs::remove_dir_all(&dir);
                std::process::exit(2);
            });
        }

        let v1_host = origin_host(ARTIFACT_ID, 1).unwrap();
        let v2_host = origin_host(ARTIFACT_ID, 2).unwrap();
        let mut options = OpenOptions::new(
            "selftest",
            ARTIFACT_ID,
            VersionSpec::new(1, Content::Folder(v1)).with_data(v1_data()),
        );
        options.presentation = Presentation::Hidden;
        options.capabilities = Capabilities::ALL;
        options.width = 480.0;
        options.height = 360.0;

        let slot: Rc<RefCell<Option<ArtifactWindow>>> = Rc::new(RefCell::new(None));
        let slot2 = slot.clone();
        let report2 = report.clone();
        let window = ArtifactWindow::open(mtm, options, move |event| {
            let mut r = report2.lock().unwrap();
            match event {
                ArtifactEvent::Loaded { version } => {
                    r.check(&format!("v{version}Loaded"), true);
                }
                ArtifactEvent::LoadFailed { version, reason } => {
                    r.details
                        .insert(format!("v{version}LoadFailed"), json!(reason));
                    r.check(&format!("v{version}NoLoadFailure"), false);
                }
                ArtifactEvent::ExternalLink { url } => {
                    r.check("externalLinkReported", url == "https://example.com/");
                }
                ArtifactEvent::Emit {
                    version,
                    name,
                    payload,
                } => {
                    r.details.insert(name.clone(), payload.clone());
                    let b = |k: &str| payload.get(k) == Some(&Value::Bool(true));
                    match name.as_str() {
                        "v1-results" => {
                            r.check("v1Version", version == 1);
                            r.check(
                                "v1OwnOrigin",
                                payload["origin"] == json!(format!("artifact://{v1_host}")),
                            );
                            r.check(
                                "fileFromArtifactLoads",
                                b("appJsLoaded") && b("cssApplied") && b("imageLoaded"),
                            );
                            r.check("pathTraversalBlocked", b("traversalBlocked"));
                            r.check(
                                "fetchExampleComFails",
                                b("fetchHttpsFails") && b("fetchHttpFails"),
                            );
                            r.check(
                                "remoteImageAndWebSocketFail",
                                b("remoteImageFails") && b("webSocketFails"),
                            );
                            r.check("connectSrcNoneBlocksSelfFetch", b("selfFetchBlocked"));
                            r.check(
                                "webRtcRemoved",
                                b("webRtcGone") && payload["stunGathered"] == json!(false),
                            );
                            r.check(
                                "cspEnforced",
                                payload["cspViolations"].as_array().is_some_and(|v| {
                                    v.iter().any(|d| {
                                        d.as_str().is_some_and(|d| d.starts_with("connect-src"))
                                    })
                                }),
                            );
                            r.check("getDataReturnsData", payload["data"] == v1_data());
                            r.check("bridgeRawRoundTrip", b("rawRoundTrip"));
                            r.check("unknownMethodRejected", b("unknownMethodRejected"));
                            r.check("bridgeEmitEvent", true);
                            r.check("noPageError", payload.get("error").is_none());
                        }
                        "v1-after-nav" => {
                            r.check(
                                "navigationAwayDenied",
                                payload["href"].as_str().is_some_and(|h| {
                                    h.starts_with(&format!("artifact://{v1_host}/"))
                                }),
                            );
                            drop(r);
                            let spec = VersionSpec::new(
                                2,
                                Content::single("index.html", V2_HTML.as_bytes().to_vec()),
                            )
                            .with_data(v2_data());
                            let ok = slot2
                                .borrow()
                                .as_ref()
                                .map(|w| w.show_version(spec).is_ok())
                                .unwrap_or(false);
                            report2.lock().unwrap().check("showVersionAccepted", ok);
                        }
                        "v2-results" => {
                            r.check("v2Version", version == 2);
                            r.check("v2OwnOrigin", payload["host"] == json!(v2_host));
                            r.check("v2GetData", payload["data"] == v2_data());
                            r.check("v2FetchFails", b("fetchHttpsFails"));
                            drop(r);
                            // Close; the Closed event finishes the test.
                            if let Some(w) = slot2.borrow().as_ref() {
                                w.close();
                            }
                        }
                        _ => {}
                    }
                }
                ArtifactEvent::Closed => {
                    r.check("closedEvent", true);
                    let ok = r.write(true);
                    eprintln!("selftest: {}", if ok { "passed" } else { "FAILED" });
                    drop(r);
                    let _ = std::fs::remove_dir_all(&dir);
                    std::process::exit(if ok { 0 } else { 1 });
                }
                ArtifactEvent::VersionSelected { .. } => {}
            }
        })
        .expect("open window");
        *slot.borrow_mut() = Some(window);
        app.run();
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    demo::main();
    #[cfg(not(target_os = "macos"))]
    eprintln!("artifact-view demo runs only on macOS");
}
