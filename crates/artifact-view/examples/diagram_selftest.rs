//! Renders sample diagrams in the hidden diagram renderer and writes, per sample,
//! `<out>/<name>.png` and `<out>/<name>.json` (size, type, nodes or the failure), then a
//! `<out>/report.json`. Exit status 0 when every sample gave the expected result.
//!
//! `cargo run -p artifact-view --example diagram_selftest -- <out-dir>`

#[cfg(target_os = "macos")]
mod selftest {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::time::Duration;

    use artifact_view::diagram::{
        DiagramRenderer, RenderOutcome, RenderRequest, RendererEvent, ThemeVars,
    };
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    use objc2_foundation::MainThreadMarker;
    use serde_json::{Value, json};

    const SAMPLES: &[(&str, bool, &str)] = &[
        (
            "flowchart",
            true,
            "flowchart LR\n  build[构建<br/>2 分钟] --> staging[staging 已部署]\n  staging --> check{Scout 验收}\n  check -->|通过| approve[批准]\n  check -->|没通过| build\n  approve --> prod[生产]",
        ),
        (
            "sequence",
            true,
            "sequenceDiagram\n  participant C as 客户端\n  participant D as seq 去重\n  participant S as 服务器\n  loop 连不上时\n    C->>C: 等 1、2、4 … 秒再试\n  end\n  C->>S: 重连 · 带上最后确认的 seq 1041\n  S->>D: 从 1042 推到 1050\n  Note over D: 已经有的 seq 直接跳过\n  D->>C: 9 条新消息放进列表",
        ),
        (
            "gantt",
            true,
            "gantt\n  dateFormat YYYY-MM-DD\n  section 发布\n  构建 :done, a1, 2026-11-01, 1d\n  验收 :active, a2, after a1, 2d\n  回滚演练 :crit, a3, after a2, 1d\n  上线 :a4, after a3, 1d",
        ),
        (
            "state",
            true,
            "stateDiagram-v2\n  [*] --> 排队\n  排队 --> 正在做\n  正在做 --> 做完了\n  做完了 --> [*]",
        ),
        (
            "class",
            true,
            "classDiagram\n  class Topic {\n    +String id\n    +title()\n  }\n  Topic <|-- Group",
        ),
        (
            "er",
            true,
            "erDiagram\n  GROUP ||--o{ TOPIC : has\n  TOPIC ||--|{ MESSAGE : holds",
        ),
        (
            "timeline",
            true,
            "timeline\n  title 1.1.0\n  11 月 : 设计 : 开发\n  12 月 : 发布",
        ),
        (
            "parse-error",
            false,
            "sequenceDiagram\n  客户端 -> > 服务器: 重连",
        ),
        ("unsupported", false, "pie\n  \"a\" : 1"),
    ];

    fn theme() -> ThemeVars {
        ThemeVars {
            raised: "#ffffff".into(),
            line: "#ebebea".into(),
            line_2: "#dcdcda".into(),
            ink: "#1a1a19".into(),
            ink_2: "#5c5c5a".into(),
            ink_3: "#8a8a87".into(),
            sunken: "#f4f4f2".into(),
            rail: "#f8f8f7".into(),
            bg: "#ffffff".into(),
            sel: "#ececea".into(),
            live_mix: "#dbe6fb".into(),
            ok_mix: "#d9efdf".into(),
            bad_mix: "#f9dcdc".into(),
            font_family: "-apple-system, BlinkMacSystemFont, \"PingFang SC\", sans-serif".into(),
            font_size_px: 13,
        }
    }

    struct Run {
        renderer: Option<DiagramRenderer>,
        queue: VecDeque<(usize, &'static str, bool, &'static str)>,
        current: Option<(usize, &'static str, bool)>,
        results: Vec<Value>,
        ok: bool,
        out: PathBuf,
    }

    fn next(run: &Rc<RefCell<Run>>) {
        let mut r = run.borrow_mut();
        let Some((i, name, expect, source)) = r.queue.pop_front() else {
            let report = json!({ "ok": r.ok, "samples": r.results });
            let _ = std::fs::write(
                r.out.join("report.json"),
                serde_json::to_vec_pretty(&report).unwrap(),
            );
            println!(
                "diagram selftest: {}",
                if r.ok { "passed" } else { "FAILED" }
            );
            std::process::exit(if r.ok { 0 } else { 1 });
        };
        r.current = Some((i, name, expect));
        if let Some(renderer) = &r.renderer {
            renderer.request(&RenderRequest {
                req_id: i as u64 + 1,
                source: source.to_string(),
                theme: theme(),
            });
        }
    }

    pub fn main() {
        let out = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
        std::fs::create_dir_all(&out).expect("out dir");
        let mtm = MainThreadMarker::new().expect("main thread");
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(60));
            eprintln!("diagram selftest: timed out");
            std::process::exit(2);
        });
        let run = Rc::new(RefCell::new(Run {
            renderer: None,
            queue: SAMPLES
                .iter()
                .enumerate()
                .map(|(i, (n, e, s))| (i, *n, *e, *s))
                .collect(),
            current: None,
            results: Vec::new(),
            ok: true,
            out: out.clone(),
        }));
        let run2 = run.clone();
        let renderer = DiagramRenderer::open(mtm, move |event| match event {
            RendererEvent::Ready => next(&run2),
            RendererEvent::Crashed(reason) => {
                eprintln!("renderer crashed: {reason}");
                std::process::exit(1);
            }
            RendererEvent::Outcome(outcome) => {
                let Some((_, name, expect)) = run2.borrow().current else { return };
                match outcome {
                    RenderOutcome::Rendered(r) => {
                        let meta = json!({
                            "name": name, "type": r.kind, "width": r.width, "height": r.height,
                            "nodes": r.nodes.iter().map(|n| json!({"id": n.id, "label": n.label,
                                "kind": n.kind, "x": n.x, "y": n.y, "w": n.w, "h": n.h})).collect::<Vec<_>>(),
                            "svgBytes": r.svg.len(),
                        });
                        let run3 = run2.clone();
                        let out = out.clone();
                        let (w, h) = (r.width, r.height);
                        let svg = r.svg.clone();
                        let renderer = run2.borrow();
                        let Some(renderer) = renderer.renderer.as_ref() else { return };
                        renderer.capture(w, h, 2.0, move |png| {
                            let mut meta = meta;
                            match png {
                                Ok(png) => {
                                    let _ = std::fs::write(out.join(format!("{name}.png")), &png);
                                    let _ = std::fs::write(out.join(format!("{name}.svg")), &svg);
                                    meta["png"] = json!(png.len());
                                }
                                Err(e) => meta["pngError"] = json!(e),
                            }
                            let good = expect && meta.get("png").is_some();
                            {
                                let mut r = run3.borrow_mut();
                                r.ok &= good;
                                let _ = std::fs::write(
                                    r.out.join(format!("{name}.json")),
                                    serde_json::to_vec_pretty(&meta).unwrap(),
                                );
                                r.results.push(json!({"name": name, "ok": good, "nodes": meta["nodes"].as_array().map_or(0, |a| a.len())}));
                            }
                            next(&run3);
                        });
                    }
                    RenderOutcome::Failed { reason, detail, .. } => {
                        {
                            let mut r = run2.borrow_mut();
                            r.ok &= !expect;
                            r.results.push(json!({"name": name, "ok": !expect,
                                "reason": reason.as_str(), "detail": detail}));
                        }
                        next(&run2);
                    }
                }
            }
        })
        .expect("open renderer");
        run.borrow_mut().renderer = Some(renderer);
        app.run();
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    selftest::main();
}
