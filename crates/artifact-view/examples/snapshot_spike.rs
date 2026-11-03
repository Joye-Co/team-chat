//! Spike: can a web view in a window that is never on screen give snapshot pixels?
//!
//! `cargo run -p artifact-view --example snapshot_spike -- <out-dir> [hidden|offscreen]`
//! writes `<out-dir>/spike-<mode>.png` (a red box on a transparent background, 2x) and
//! prints the PNG size, then exits (status 0 when a PNG came back within 10 s).

#[cfg(target_os = "macos")]
mod spike {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::time::Duration;

    use artifact_view::{
        ArtifactEvent, ArtifactWindow, Capabilities, Content, OpenOptions, Presentation, Rect,
        VersionSpec,
    };
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    use objc2_foundation::MainThreadMarker;

    const PAGE: &str = r#"<!doctype html><meta charset="utf-8">
<style>html,body{margin:0;background:transparent}#b{width:200px;height:100px;background:#e00;color:#fff;font:20px sans-serif}</style>
<div id="b">snapshot 图</div>"#;

    pub fn main() {
        let mut args = std::env::args().skip(1);
        let dir = PathBuf::from(args.next().unwrap_or_else(|| ".".into()));
        let mode = args.next().unwrap_or_else(|| "hidden".into());
        let mtm = MainThreadMarker::new().expect("main thread");
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(10));
            eprintln!("spike: no snapshot within 10 s");
            std::process::exit(2);
        });
        let mut options = OpenOptions::new(
            "spike",
            "spike",
            VersionSpec::new(1, Content::single("index.html", PAGE.as_bytes().to_vec())),
        );
        options.presentation = if mode == "offscreen" {
            Presentation::Offscreen
        } else {
            Presentation::Hidden
        };
        options.capabilities = Capabilities::NONE;
        options.transparent = true;
        options.width = 400.0;
        options.height = 300.0;
        let slot: Rc<RefCell<Option<ArtifactWindow>>> = Rc::new(RefCell::new(None));
        let slot2 = slot.clone();
        let window = ArtifactWindow::open(mtm, options, move |event| {
            if let ArtifactEvent::Loaded { .. } = event {
                let slot = slot2.borrow();
                let Some(w) = slot.as_ref() else { return };
                let dir = dir.clone();
                let mode = mode.clone();
                w.snapshot(Rect::new(0.0, 0.0, 300.0, 150.0), 2.0, move |r| match r {
                    Ok(png) => {
                        let path = dir.join(format!("spike-{mode}.png"));
                        std::fs::write(&path, &png).expect("write png");
                        let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
                        let h = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
                        println!("spike {mode}: {} bytes, {w}x{h} px -> {}", png.len(), path.display());
                        std::process::exit(0);
                    }
                    Err(e) => {
                        eprintln!("spike {mode}: snapshot failed: {e}");
                        std::process::exit(1);
                    }
                });
            } else if let ArtifactEvent::LoadFailed { reason, .. } = event {
                eprintln!("spike: load failed: {reason}");
            }
        })
        .expect("open");
        *slot.borrow_mut() = Some(window);
        app.run();
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    spike::main();
}
