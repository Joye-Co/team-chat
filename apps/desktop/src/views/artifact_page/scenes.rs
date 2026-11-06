//! Screenshot scenes of the 产出 page (the artifact track owns this slice).

#[allow(unused_imports)]
use crate::{
    app::RightPane,
    fake::v2 as d,
    selftest::scenes::{Scene, Start, open_main, v2_open_topic},
};

pub const SCENES: &[Scene] = &[
    Scene {
        name: "artifact-v2",
        about: "产出 (Artifact.dc.html): 10 月数据月报 v3 live in the window, with 来龙去脉",
        start: Start::V2,
        setup: |s, cx| {
            s.nav.update(cx, |nav, cx| {
                nav.open_artifact(d::ART_REPORT.into(), None, cx)
            })
        },
        open: open_main,
    },
    Scene {
        name: "artifact-v2-old",
        about: "产出: 10 月数据月报 at v2, the old-version band and v2 picked in 版本",
        start: Start::V2,
        setup: |s, cx| {
            s.nav.update(cx, |nav, cx| {
                nav.open_artifact(d::ART_REPORT.into(), Some(2), cx)
            })
        },
        open: open_main,
    },
    Scene {
        name: "artifact-pane-v2",
        about: "The 产出 pane beside 10 月数据月报: the page live in the pane",
        start: Start::V2,
        setup: |s, cx| {
            v2_open_topic(s, d::GROWTH, d::T_REPORT, cx);
            s.nav.update(cx, |nav, cx| {
                nav.set_right_pane(
                    RightPane::Artifact {
                        id: d::ART_REPORT.into(),
                        version: None,
                    },
                    cx,
                )
            })
        },
        open: open_main,
    },
    Scene {
        name: "artifact-v2-leave",
        about: "产出 page left for the first 群 once its page loaded: the 群 shows, no web view stays on top",
        start: Start::V2,
        setup: setup_leave,
        open: open_main,
    },
    // --- r2-trust ---
    Scene {
        name: "artifact-v2-choose",
        about: "产出: a real click on 抖音 in 10 月数据月报 sends exactly one choose event to the member",
        start: Start::V2,
        setup: super::gate_check::setup_choose,
        open: open_main,
    },
    Scene {
        name: "artifact-quiet-page",
        about: "产出: a page that calls emitEvent on load, from a timer and a script click sends nothing",
        start: Start::V2,
        setup: |_, _| {},
        open: super::gate_check::open_quiet,
    },
    // --- end r2-trust ---
    Scene {
        name: "artifacts-v2",
        about: "产出 destination: the table of every 产出 (名称, 类型, 话题, 做的人, 版本, 更新) with its footer",
        start: Start::V2,
        setup: |s, cx| s.nav.update(cx, |nav, cx| nav.open_artifacts(cx)),
        open: open_main,
    },
    // --- v13/ux-pages ---
    Scene {
        name: "artifacts-grid",
        about: "产出 in the icon view: tiles grouped by day, the page snapshot fitted whole",
        start: Start::V2,
        setup: |s, cx| {
            list_start(true, None);
            s.nav.update(cx, |nav, cx| nav.open_artifacts(cx))
        },
        open: open_main,
    },
    Scene {
        name: "artifacts-no-match",
        about: "产出 with a search that finds nothing: the Empty state with 清空搜索",
        start: Start::V2,
        setup: |s, cx| {
            list_start(false, Some("季度预算"));
            s.nav.update(cx, |nav, cx| nav.open_artifacts(cx))
        },
        open: open_main,
    },
    Scene {
        name: "artifacts-empty",
        about: "产出 before any member made one: the Empty state",
        start: Start::V2Bare,
        setup: |s, cx| s.nav.update(cx, |nav, cx| nav.open_artifacts(cx)),
        open: open_main,
    },
    Scene {
        name: "artifacts-loading",
        about: "产出 before the first sync: table skeleton",
        start: Start::V2Unsynced,
        setup: |s, cx| s.nav.update(cx, |nav, cx| nav.open_artifacts(cx)),
        open: open_main,
    },
    Scene {
        name: "artifacts-failed",
        about: "产出 with nothing cached while the server cannot be reached: the Alert with 重试",
        start: Start::V2Unreachable,
        setup: |s, cx| s.nav.update(cx, |nav, cx| nav.open_artifacts(cx)),
        open: open_main,
    },
    // --- end v13/ux-pages ---
];

/// How the next 产出 list starts in a scene: the icon view, a search.
#[derive(Clone, Default)]
pub struct ListStart {
    pub grid: bool,
    pub query: Option<&'static str>,
}

static LIST_START: std::sync::Mutex<Option<ListStart>> = std::sync::Mutex::new(None);

fn list_start(grid: bool, query: Option<&'static str>) {
    if let Ok(mut s) = LIST_START.lock() {
        *s = Some(ListStart { grid, query });
    }
}

/// Takes the start a scene set for the 产出 list (once).
pub fn take_list_start() -> Option<ListStart> {
    LIST_START.lock().ok().and_then(|mut s| s.take())
}

/// True once the 产出 page's web view loaded and shows.
pub fn ready(cx: &gpui::App) -> bool {
    crate::ui::web_embed::live(cx).iter().any(|e| {
        let s = e.read(cx);
        s.artifact_id() == d::ART_REPORT
            && *s.phase() == crate::ui::web_embed::Phase::Ready
            && s.shown()
    })
}

/// `artifact-v2-leave`: after leaving the 产出 page, no page shows.
pub fn checks(name: &str) -> Option<crate::selftest::scenes::Checks> {
    (name == "artifact-v2-leave").then_some(|_, cx| {
        let left = crate::ui::web_embed::live(cx).len();
        vec![if left == 0 {
            Ok("no page shows after leaving the 产出 page".into())
        } else {
            Err(format!(
                "{left} page(s) still show after leaving the 产出 page"
            ))
        }]
    })
}

static LEFT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `artifact-v2-leave`: opens the 产出 page, and once its page shows, the first 群.
fn setup_leave(s: &crate::app::Services, cx: &mut gpui::App) {
    LEFT.store(false, std::sync::atomic::Ordering::SeqCst);
    s.nav.update(cx, |nav, cx| {
        nav.open_artifact(d::ART_REPORT.into(), None, cx)
    });
    let services = s.clone();
    cx.spawn(async move |cx| {
        for _ in 0..200 {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(100))
                .await;
            if cx.update(|cx| ready(cx)) {
                cx.update(|cx| crate::selftest::scenes::open_first_group(&services, cx));
                LEFT.store(true, std::sync::atomic::Ordering::SeqCst);
                return;
            }
        }
    })
    .detach();
}

/// True once `artifact-v2-leave` left the loaded 产出 page.
pub fn left(_: &gpui::App) -> bool {
    LEFT.load(std::sync::atomic::Ordering::SeqCst)
}
