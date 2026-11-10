//! Opens a sample 产出 in an artifact window: `cargo run -p artifact-view --example demo`.
//!
//! The isolation and bridge checks that ran here as `--selftest` are the `isolation` test
//! (`cargo test -p artifact-view --test isolation`), on macOS and Windows.

#[cfg(target_os = "macos")]
mod demo {
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;

    use artifact_view::{
        ArtifactEvent, ArtifactWindow, Capabilities, Content, OpenOptions, VersionItem, VersionSpec,
    };
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    use objc2_foundation::MainThreadMarker;
    use serde_json::{Value, json};

    const ARTIFACT_ID: &str = "demo-a1";

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
    </head><body>
    <h1 id="h">Sample 产出 v1</h1>
    <p><a id="ext" href="https://example.com/">example.com</a></p>
    <img id="px" src="pixel.png" width="16" height="16">
    <pre id="out"></pre>
    <script>
    window.artifact.getData().then(function (d) {
      document.getElementById('out').textContent = JSON.stringify(d, null, 2);
    });
    </script>
    </body></html>"#;

    const V2_HTML: &str = r#"<!doctype html><html><head><meta charset="utf-8"></head><body><h1>Sample 产出 v2</h1>
    <pre id="out"></pre>
    <script>
    window.artifact.getData().then(function (d) {
      document.getElementById('out').textContent = location.host + ' ' + JSON.stringify(d);
    });
    </script></body></html>"#;

    fn write_sample(dir: &Path) -> std::io::Result<PathBuf> {
        let v1 = dir.join("v1");
        std::fs::create_dir_all(&v1)?;
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

    pub fn main() {
        let mtm = MainThreadMarker::new().expect("main thread");
        let app = NSApplication::sharedApplication(mtm);
        let dir = std::env::temp_dir().join(format!("artifact-view-demo-{}", std::process::id()));
        let v1 = write_sample(&dir).expect("write sample");
        run_demo(mtm, &app, v1);
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
                        let _ = w.show_version(spec, Capabilities::ALL);
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
}

fn main() {
    #[cfg(target_os = "macos")]
    demo::main();
    #[cfg(not(target_os = "macos"))]
    eprintln!("artifact-view demo runs only on macOS");
}
