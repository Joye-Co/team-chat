//! The isolation and bridge checks of a 产出 page, run inside a real web view: WKWebView on
//! macOS, WebView2 on Windows, with the same pages and the same assertions.
//!
//! `cargo test -p artifact-view --test isolation` runs it (no test harness: the web view
//! needs the main thread). It opens a hidden window, never takes the focus, and exits with
//! status 0 when every check passed. `-- --report <path>` also writes the JSON report.
//!
//! What it proves: files of the version load and nothing else does (no http, https, ws,
//! WebRTC, path traversal, other version); the CSP is enforced; navigation away is denied;
//! each version has its own origin; the bridge answers, refuses unknown methods and refuses
//! `emit` and `ask` of a page that has no capability for them. A last page tries the ways
//! out the review asked about (a fresh frame's WebRTC against a TURN server on this
//! computer, frames, a frame's bridge message, permission prompts, a download, another
//! app's URL scheme) while a local TCP listener counts connections: there must be none.
//! Then a second window of the same origin must find the first one's storage gone.

#[cfg(any(target_os = "macos", windows))]
mod isolation {
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use artifact_view::{
        ArtifactEvent, ArtifactWindow, Capabilities, Content, OpenOptions, Presentation,
        VersionSpec, origin_host,
    };
    use serde_json::{Value, json};

    const ARTIFACT_ID: &str = "selftest-a1";
    const TIMEOUT: Duration = Duration::from_secs(120);

    /// 1x1 PNG.
    const PIXEL_PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8,
        0xcf, 0xc0, 0xf0, 0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d, 0x1d, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    /// The page of v1. `post` is the raw message channel of the platform.
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
      function post(m) {
        if (window.webkit && window.webkit.messageHandlers) window.webkit.messageHandlers.bridge.postMessage(m);
        else window.chrome.webview.postMessage(m);
      }
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
        r.otherVersionFails = !(await imgLoads('artifact://selftest-a1-v2/pixel.png'));
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
        r.popupBlocked = !window.open('https://example.com/');
        r.data = await window.artifact.getData();
        var raw = await new Promise(function (res) {
          function on(e) { if (e.detail && e.detail.id === 'raw-1') { window.removeEventListener('artifactbridgereply', on); res(e.detail); } }
          window.addEventListener('artifactbridgereply', on);
          post({ id: 'raw-1', method: 'getData', params: null });
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
      setTimeout(function () { location.href = 'artifact://selftest-a1-v2/index.html'; }, 400);
      setTimeout(function () { window.artifact.emitEvent('v1-after-nav', { href: location.href }); }, 1500);
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

    /// v3 may only read and point (a person's version): `emit` and `ask` must be refused.
    /// It reports through `ref`, which goes to the viewer's own composer, not to a member.
    const V3_HTML: &str = r#"<!doctype html><html><head><meta charset="utf-8"></head><body><h1>Sample 产出 v3</h1>
    <script>
    (async function () {
      var r = { host: location.host, can: window.teamchat.can.emit || window.teamchat.can.ask };
      try { await window.teamchat.emit('v3-denied', { secret: 1 }); r.emit = 'sent'; }
      catch (e) { r.emit = e.code; }
      try { await window.artifact.emitEvent('v3-denied', { secret: 1 }); r.legacy = 'sent'; }
      catch (e) { r.legacy = e.code; }
      try { await window.teamchat.ask('v3-denied', {}); r.ask = 'sent'; }
      catch (e) { r.ask = e.code; }
      r.data = await window.teamchat.getData();
      await window.teamchat.ref({ label: 'v3-results', value: r });
    })();
    </script></body></html>"#;

    /// v4 tries to get out. `__PORT__` is a TCP listener of this test on 127.0.0.1: any
    /// connection to it is a hole. The download and the late report come after the
    /// results, because a download that was not stopped could take the page away.
    const V4_HTML: &str = r#"<!doctype html><html><head><meta charset="utf-8"></head><body><h1>Sample 产出 v4</h1>
    <a id="dl" download="__DOWNLOAD__">download</a>
    <script>
    (async function () {
      var r = {};
      var port = __PORT__, local = 'http://127.0.0.1:' + port + '/';
      function sleep(ms) { return new Promise(function (res) { setTimeout(res, ms); }); }
      function within(ms, p, late) { return Promise.race([p, sleep(ms).then(function () { return late; })]); }
      try {
        try { localStorage.setItem('isolation-k', 'v4'); r.storageWrote = localStorage.getItem('isolation-k') === 'v4'; }
        catch (e) { r.storageWrote = false; }

        // A fresh about:blank frame has a window of its own: reach it both ways.
        var f = document.createElement('iframe');
        document.body.appendChild(f);
        var byIndex = window[window.length - 1], byGetter = f.contentWindow;
        r.frameRtcByIndex = typeof (byIndex && byIndex.RTCPeerConnection);
        r.frameRtcByGetter = typeof (byGetter && byGetter.RTCPeerConnection);
        r.mainRtc = typeof window.RTCPeerConnection;
        var RTC = (byIndex && byIndex.RTCPeerConnection) || (byGetter && byGetter.RTCPeerConnection) || window.RTCPeerConnection;
        r.frameRtcCandidates = [];
        if (RTC) {
          try {
            var pc = new RTC({ iceServers: [
              { urls: 'turn:127.0.0.1:' + port + '?transport=tcp', username: 'u', credential: 'p' },
              { urls: 'stun:127.0.0.1:' + port }
            ] });
            pc.createDataChannel('x');
            pc.onicecandidate = function (e) { if (e.candidate) r.frameRtcCandidates.push(e.candidate.candidate); };
            await pc.setLocalDescription(await pc.createOffer());
            await sleep(3000);
          } catch (e) { r.frameRtcError = String(e); }
        }
        var nativePrint = function (w) { try { return /native code/.test(Function.prototype.toString.call(w.print)); } catch (e) { return String(e); } };
        r.printNative = nativePrint(window);
        r.framePrintNative = nativePrint(byIndex || byGetter);

        // The frame's own message channel must not reach the bridge.
        try {
          var m = { id: 'frame-1', method: 'emitEvent', params: { name: 'from-frame', payload: {} } };
          var fw = f.contentWindow;
          if (fw.webkit && fw.webkit.messageHandlers && fw.webkit.messageHandlers.bridge) { fw.webkit.messageHandlers.bridge.postMessage(m); r.framePosted = 'webkit'; }
          else if (fw.chrome && fw.chrome.webview) { fw.chrome.webview.postMessage(m); r.framePosted = 'chrome'; }
          else r.framePosted = 'no channel';
        } catch (e) { r.framePosted = String(e); }

        // Frames: its own origin, the listener, inline content.
        window.__innerRan = false; window.__srcdocRan = false;
        [['src', 'inner.html'], ['src', local + 'frame'], ['srcdoc', '<script>parent.__srcdocRan = true;<\/script>']].forEach(function (a) {
          var x = document.createElement('iframe'); x[a[0]] = a[1]; document.body.appendChild(x);
        });

        // The listener, every way a page has.
        try { await within(3000, fetch(local + 'fetch'), null); } catch (e) {}
        new Image().src = local + 'img';
        try { new WebSocket('ws://127.0.0.1:' + port + '/ws'); } catch (e) {}
        try { navigator.sendBeacon(local + 'beacon', 'x'); } catch (e) {}
        try { window.open(local + 'popup'); } catch (e) {}

        // Permission prompts: nothing is granted and nothing waits for the viewer.
        r.geolocation = await new Promise(function (res) {
          if (!navigator.geolocation) return res('absent');
          var t = setTimeout(function () { res('timeout'); }, 3000);
          navigator.geolocation.getCurrentPosition(
            function () { clearTimeout(t); res('granted'); },
            function (e) { clearTimeout(t); res('denied:' + e.code); });
        });
        r.microphone = await (async function () {
          if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) return 'absent';
          try { var s = await within(3000, navigator.mediaDevices.getUserMedia({ audio: true }), 'timeout'); return s === 'timeout' ? s : 'granted'; }
          catch (e) { return 'denied:' + e.name; }
        })();
        r.notification = await (async function () {
          if (typeof Notification === 'undefined') return 'absent';
          try { return await within(3000, Notification.requestPermission(), 'timeout'); } catch (e) { return 'error'; }
        })();
        r.clipboard = await (async function () {
          if (!navigator.clipboard || !navigator.clipboard.readText) return 'absent';
          try { var t = await within(3000, navigator.clipboard.readText(), { late: 1 }); return t && t.late ? 'timeout' : 'granted'; }
          catch (e) { return 'denied:' + e.name; }
        })();

        // Another app's URL scheme and the listener as navigations: the page stays.
        location.href = 'artifact-view-isolation-test://location';
        await sleep(200);
        location.href = local + 'navigation';
        await sleep(200);
        var link = document.createElement('a'); link.href = 'artifact-view-isolation-test://link';
        document.body.appendChild(link); link.click();
        await sleep(400);
        r.innerRan = window.__innerRan === true;
        r.srcdocRan = window.__srcdocRan === true;
        r.href = location.href;
      } catch (e) { r.error = String(e); }
      await window.artifact.emitEvent('v4-results', r);
      // The late report is scheduled first: whatever the download does, it still comes.
      var late = { step: 'scheduled' };
      setTimeout(function () { late.href = location.href; window.artifact.emitEvent('v4-late', late); }, 1500);
      try {
        var a = document.getElementById('dl');
        a.href = URL.createObjectURL(new Blob(['isolation'], { type: 'application/octet-stream' }));
        late.step = 'url made';
        a.click();
        late.step = 'clicked';
      } catch (e) { late.error = String(e); }
    })();
    </script></body></html>"#;

    /// The page of the second window, at v4's origin: what is left of v4's storage.
    const STORE_HTML: &str = r#"<!doctype html><html><head><meta charset="utf-8"></head><body>
    <script>
    (function () {
      var v;
      try { v = localStorage.getItem('isolation-k'); } catch (e) { v = 'error: ' + e; }
      window.artifact.emitEvent('store-results', { value: v });
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

    fn v3_data() -> Value {
        json!({ "v": 3 })
    }

    /// The capabilities of a page that may only read.
    const READ_ONLY: Capabilities = Capabilities::READ;

    struct Report {
        started: Instant,
        checks: serde_json::Map<String, Value>,
        details: serde_json::Map<String, Value>,
        path: Option<PathBuf>,
        external_links: u32,
        denied_events: u32,
        frame_events: u32,
        closed: u32,
        late_seen: bool,
    }

    impl Report {
        fn check(&mut self, name: &str, ok: bool) {
            // A check that failed once stays failed.
            let ok = ok && self.checks.get(name) != Some(&Value::Bool(false));
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
                "platform": std::env::consts::OS,
                "durationMs": self.started.elapsed().as_millis() as u64,
                "checks": self.checks,
                "details": self.details,
            });
            let text = serde_json::to_string_pretty(&out).unwrap();
            if let Some(p) = &self.path {
                if let Some(parent) = p.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                std::fs::write(p, text.clone() + "\n").expect("write report");
            }
            println!("{text}");
            ok
        }
    }

    thread_local! {
        /// Work for the main thread that waits for its time ([`platform::later`]).
        static LATER: RefCell<Vec<Box<dyn FnOnce()>>> = const { RefCell::new(Vec::new()) };
    }

    /// Runs what [`platform::later`] queued. Main thread only.
    fn run_later() {
        let jobs: Vec<_> = LATER.with(|l| l.borrow_mut().drain(..).collect());
        for job in jobs {
            job();
        }
    }

    #[cfg(target_os = "macos")]
    mod platform {
        use artifact_view::MainThread;
        use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

        /// Runs `job` on the main thread after `wait`, whatever the page does meanwhile.
        pub fn later(wait: std::time::Duration, job: impl FnOnce() + 'static) {
            super::LATER.with(|l| l.borrow_mut().push(Box::new(job)));
            std::thread::spawn(move || {
                std::thread::sleep(wait);
                dispatch2::DispatchQueue::main().exec_async(super::run_later);
            });
        }

        pub fn start(_data_dir: &std::path::Path) -> MainThread {
            let mtm = MainThread::new().expect("main thread");
            // Prohibited: no Dock icon, no menu bar, the app can never become active.
            NSApplication::sharedApplication(mtm)
                .setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
            mtm
        }

        pub fn run(mtm: MainThread) {
            NSApplication::sharedApplication(mtm).run();
        }
    }

    #[cfg(windows)]
    mod platform {
        use std::sync::atomic::{AtomicBool, Ordering};

        use artifact_view::MainThread;

        static DUE: AtomicBool = AtomicBool::new(false);

        /// Runs `job` on the main thread after `wait`, whatever the page does meanwhile.
        pub fn later(wait: std::time::Duration, job: impl FnOnce() + 'static) {
            super::LATER.with(|l| l.borrow_mut().push(Box::new(job)));
            std::thread::spawn(move || {
                std::thread::sleep(wait);
                DUE.store(true, Ordering::SeqCst);
            });
        }

        pub fn start(data_dir: &std::path::Path) -> MainThread {
            artifact_view::win_support::set_dpi_aware();
            artifact_view::set_user_data_dir(data_dir.join("webview2"));
            MainThread::current()
        }

        pub fn run(_main: MainThread) {
            loop {
                artifact_view::win_support::pump(std::time::Duration::from_millis(50), || false);
                if DUE.swap(false, Ordering::SeqCst) {
                    super::run_later();
                }
            }
        }
    }

    pub fn main() {
        let args: Vec<String> = std::env::args().collect();
        let report_path = args
            .iter()
            .position(|a| a == "--report")
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("ARTIFACT_VIEW_ISOLATION_REPORT").map(PathBuf::from));

        let dir =
            std::env::temp_dir().join(format!("artifact-view-isolation-{}", std::process::id()));
        let v1 = write_sample(&dir).expect("write sample");
        let main = platform::start(&dir);

        let report = Arc::new(Mutex::new(Report {
            started: Instant::now(),
            checks: Default::default(),
            details: Default::default(),
            path: report_path,
            external_links: 0,
            denied_events: 0,
            frame_events: 0,
            closed: 0,
            late_seen: false,
        }));

        // Counts every TCP connection a page manages to make to this computer.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind the listener");
        let port = listener.local_addr().expect("listener address").port();
        let connections = Arc::new(AtomicU32::new(0));
        {
            let connections = connections.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if stream.is_ok() {
                        connections.fetch_add(1, Ordering::SeqCst);
                    }
                }
            });
        }
        let download_name = format!("artifact-view-isolation-{}.bin", std::process::id());
        let download_path = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(|home| PathBuf::from(home).join("Downloads").join(&download_name));
        let v4_page = V4_HTML
            .replace("__PORT__", &port.to_string())
            .replace("__DOWNLOAD__", &download_name);
        let v4_spec = move |html: &str| {
            VersionSpec::new(
                4,
                Content::memory([
                    ("index.html".to_string(), html.as_bytes().to_vec()),
                    (
                        "inner.html".to_string(),
                        b"<script>parent.__innerRan = true;</script>".to_vec(),
                    ),
                ]),
            )
        };
        let v4_host = origin_host(ARTIFACT_ID, 4).unwrap();
        {
            let report = report.clone();
            std::thread::spawn(move || {
                std::thread::sleep(TIMEOUT);
                let r = report.lock().unwrap_or_else(|e| e.into_inner());
                r.write(false);
                eprintln!("isolation: timed out");
                std::process::exit(2);
            });
        }

        let v1_host = origin_host(ARTIFACT_ID, 1).unwrap();
        let v2_host = origin_host(ARTIFACT_ID, 2).unwrap();
        let v3_host = origin_host(ARTIFACT_ID, 3).unwrap();
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
        let dir2 = dir.clone();
        // Both windows report to the same handler; the second one is opened from it.
        type Handler = Rc<dyn Fn(ArtifactEvent)>;
        let handler_slot: Rc<RefCell<Option<Handler>>> = Rc::new(RefCell::new(None));
        let handler_slot2 = handler_slot.clone();
        let handler: Handler = Rc::new(move |event| {
            let mut r = report2.lock().unwrap();
            // The order of events, for a report that stops half way.
            let seen = match &event {
                ArtifactEvent::Emit { name, .. } => format!("emit {name}"),
                ArtifactEvent::Ref { label, .. } => format!("ref {label}"),
                other => format!("{other:?}").chars().take(60).collect(),
            };
            if let Some(Value::Array(log)) = r
                .details
                .entry("events")
                .or_insert_with(|| Value::Array(Vec::new()))
                .into()
            {
                log.push(json!(seen));
            }
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
                    r.external_links += 1;
                    r.details.insert("externalLink".into(), json!(url));
                }
                ArtifactEvent::Emit {
                    version,
                    name,
                    payload,
                    ..
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
                            r.check("otherVersionNotReadable", b("otherVersionFails"));
                            r.check("connectSrcNoneBlocksSelfFetch", b("selfFetchBlocked"));
                            r.check(
                                "webRtcRemoved",
                                b("webRtcGone") && payload["stunGathered"] == json!(false),
                            );
                            r.check("popupBlocked", b("popupBlocked"));
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
                            // The page's script clicked its external link: reported once
                            // (the `location.href` changes after it are not links).
                            let links = r.external_links;
                            r.check("externalLinkReportedOnce", links == 1);
                            drop(r);
                            let spec = VersionSpec::new(
                                2,
                                Content::single("index.html", V2_HTML.as_bytes().to_vec()),
                            )
                            .with_data(v2_data());
                            let ok = slot2
                                .borrow()
                                .as_ref()
                                .map(|w| w.show_version(spec, Capabilities::ALL).is_ok())
                                .unwrap_or(false);
                            report2.lock().unwrap().check("showVersionAccepted", ok);
                        }
                        "v2-results" => {
                            r.check("v2Version", version == 2);
                            r.check("v2OwnOrigin", payload["host"] == json!(v2_host));
                            r.check("v2GetData", payload["data"] == v2_data());
                            r.check("v2FetchFails", b("fetchHttpsFails"));
                            drop(r);
                            let spec = VersionSpec::new(
                                3,
                                Content::single("index.html", V3_HTML.as_bytes().to_vec()),
                            )
                            .with_data(v3_data());
                            let ok = slot2
                                .borrow()
                                .as_ref()
                                .map(|w| w.show_version(spec, READ_ONLY).is_ok())
                                .unwrap_or(false);
                            report2.lock().unwrap().check("showReadOnlyVersion", ok);
                        }
                        "v3-denied" => r.denied_events += 1,
                        "from-frame" => r.frame_events += 1,
                        "v4-results" => {
                            let is = |k: &str, v: &str| payload[k] == json!(v);
                            r.check("v4NoPageError", payload.get("error").is_none());
                            r.check(
                                "freshFrameHasNoWebRtc",
                                is("frameRtcByIndex", "undefined")
                                    && is("frameRtcByGetter", "undefined")
                                    && is("mainRtc", "undefined"),
                            );
                            r.check("ownOriginFrameBlocked", payload["innerRan"] == json!(false));
                            // WebKit never answers a geolocation request without a delegate (no
                            // prompt shows): there "timeout" is the refusal. WebView2 must
                            // answer at once, or a prompt would be waiting for the viewer.
                            let refused = |v: &str| {
                                v != "granted" && (cfg!(target_os = "macos") || v != "timeout")
                            };
                            r.check(
                                "permissionsNotGranted",
                                ["geolocation", "microphone", "notification", "clipboard"]
                                    .iter()
                                    .all(|k| payload[*k].as_str().is_some_and(refused)),
                            );
                            r.check(
                                "otherSchemesAndNavigationsDenied",
                                payload["href"].as_str().is_some_and(|h| {
                                    h.starts_with(&format!("artifact://{v4_host}/"))
                                }),
                            );
                            // WebKit has no print panel without a delegate; WebView2 would
                            // show one, so there `print` must be this crate's empty stub.
                            if cfg!(windows) {
                                r.check(
                                    "printSuppressed",
                                    payload["printNative"] == json!(false)
                                        && payload["framePrintNative"] == json!(false),
                                );
                            }
                            drop(r);
                            // The page now starts a download. The host looks at the outcome
                            // itself, on its own clock: the page may not live to tell.
                            let report = report2.clone();
                            let slot = slot2.clone();
                            let download_path = download_path.clone();
                            let connections = connections.clone();
                            platform::later(Duration::from_millis(3000), move || {
                                let mut r = report.lock().unwrap();
                                let saved = download_path.as_ref().is_some_and(|p| p.exists());
                                if saved && let Some(p) = &download_path {
                                    let _ = std::fs::remove_file(p);
                                }
                                r.check("downloadNotSaved", !saved);
                                let reached = connections.load(Ordering::SeqCst);
                                r.details
                                    .insert("listenerConnections".into(), json!(reached));
                                r.check("noConnectionReachedTheListener", reached == 0);
                                let from_frame = r.frame_events;
                                r.check("frameMessageIgnored", from_frame == 0);
                                // The page reports once more 1.5 s after the click. WebView2
                                // must still run it; for WKWebView it is recorded (see the
                                // Windows plan: on macOS 15 the page stops after the click).
                                let alive = r.late_seen;
                                r.details
                                    .insert("pageAliveAfterDownload".into(), json!(alive));
                                if cfg!(windows) {
                                    r.check("pageStaysAfterDownload", alive);
                                }
                                drop(r);
                                // Close; the Closed event opens the second window.
                                if let Some(w) = slot.borrow().as_ref() {
                                    w.close();
                                }
                            });
                        }
                        "v4-late" => r.late_seen = true,
                        "store-results" => {
                            r.check("storageGoneWithTheView", payload["value"].is_null());
                            drop(r);
                            if let Some(w) = slot2.borrow().as_ref() {
                                w.close();
                            }
                        }
                        _ => {}
                    }
                }
                ArtifactEvent::Ask { name, .. } => {
                    if name == "v3-denied" {
                        r.denied_events += 1;
                    }
                }
                ArtifactEvent::Ref { label, value, .. } => {
                    if label != "v3-results" {
                        return;
                    }
                    let payload = value.unwrap_or(Value::Null);
                    r.details.insert(label, payload.clone());
                    r.check("v3OwnOrigin", payload["host"] == json!(v3_host));
                    r.check("pageSeesItMayNotSend", payload["can"] == json!(false));
                    r.check(
                        "emitWithoutCapabilityRefused",
                        payload["emit"] == json!("not_allowed")
                            && payload["legacy"] == json!("not_allowed"),
                    );
                    r.check(
                        "askWithoutCapabilityRefused",
                        payload["ask"] == json!("not_allowed"),
                    );
                    let denied = r.denied_events;
                    r.check("refusedCallsNeverReachTheHost", denied == 0);
                    r.check("readStillAllowed", payload["data"] == v3_data());
                    drop(r);
                    let spec = v4_spec(&v4_page);
                    let ok = slot2
                        .borrow()
                        .as_ref()
                        .map(|w| w.show_version(spec, Capabilities::ALL).is_ok())
                        .unwrap_or(false);
                    report2.lock().unwrap().check("showEscapeVersion", ok);
                }
                ArtifactEvent::Closed => {
                    r.closed += 1;
                    if r.closed == 1 {
                        // A new view at v4's origin: the first view's storage must be gone.
                        drop(r);
                        let mut options =
                            OpenOptions::new("selftest", ARTIFACT_ID, v4_spec(STORE_HTML));
                        options.presentation = Presentation::Hidden;
                        let handler = handler_slot2.borrow().clone().expect("the handler");
                        let window = ArtifactWindow::open(main, options, move |e| handler(e));
                        report2
                            .lock()
                            .unwrap()
                            .check("secondWindowOpens", window.is_ok());
                        *slot2.borrow_mut() = window.ok();
                        return;
                    }
                    r.check("closedEvent", true);
                    let ok = r.write(true);
                    eprintln!("isolation: {}", if ok { "passed" } else { "FAILED" });
                    drop(r);
                    let _ = std::fs::remove_dir_all(&dir2);
                    std::process::exit(if ok { 0 } else { 1 });
                }
                ArtifactEvent::VersionSelected { .. } | ArtifactEvent::FocusChanged { .. } => {}
            }
        });
        *handler_slot.borrow_mut() = Some(handler.clone());
        let window = ArtifactWindow::open(main, options, move |e| handler(e)).expect("open window");
        *slot.borrow_mut() = Some(window);
        platform::run(main);
    }
}

fn main() {
    #[cfg(any(target_os = "macos", windows))]
    isolation::main();
    #[cfg(not(any(target_os = "macos", windows)))]
    eprintln!("isolation: no web view backend on this platform, nothing to run");
}
