//! Screenshot self-test scenes of the 产出 page host (Interact.dc.html 产出里的页面): the
//! 10 月数据月报 page live inline in its 话题, on the sample provider, through the real
//! inline frame (`media::inline`) and the real page host (`host`).
//!
//! - `artifact_live_data`: the page loads, gets its snapshot (`GET …/data`), then Ledger
//!   pushes the B 站 numbers (`artifact.data`): the page redraws and the footer says
//!   Ledger 推来了新数据.
//! - `artifact_reloaded_v2`: the page shows v3; v4 arrives and the page reloads in place
//!   with 已更新到 v4 in the frame's header.
//! - `artifact_ask_pending`: a real click on 数据有问题 sends one `ask` (the page waits);
//!   the footer says 等 Ledger 回应. The page's own `ask` on load, without a click, is
//!   refused (`not_allowed`) and never reaches the member.
//!
//! Each scene checks itself after the capture ([`verify`]).

use std::cell::RefCell;
use std::time::{Duration, Instant};

use gpui::{
    AnyWindowHandle, App, AppContext as _, AsyncApp, Context, Entity, IntoElement, ParentElement,
    Render, SharedString, Styled, Window, WindowBounds, div, px,
};

use super::host;
use crate::{
    app::{Services, main_window_options},
    data::{Member, page_host::PageEventKind, page_host::PageSignal},
    fake::v2 as d,
    selftest::scenes::capture_bounds,
    theme::{self, Type as _, space},
    ui::{
        MemberAvatar,
        parts::{hairline, toolbar_band},
        web_embed::{Phase, WebEmbedState, live},
    },
    views::media::{
        MediaKind,
        inline::{self, InlineSpec},
        state::Target,
        store::{self, MediaKey},
    },
};

const PAGE: &str = include_str!("host_dashboard.html");
const ROW: &str = "host-scene";
/// The message of the sample (sample data, like a member's message).
const SAID: &str = "做好了，页面里可以按渠道看每周的数。觉得哪个数不对，直接在页面里问我。";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    LiveData,
    Reloaded,
    AskPending,
}

#[derive(Default)]
struct Probe {
    done: bool,
    error: Option<String>,
}

thread_local! {
    static PROBE: RefCell<Probe> = RefCell::new(Probe::default());
}

fn finish(r: Result<(), String>) {
    PROBE.with(|p| {
        let mut p = p.borrow_mut();
        p.done = true;
        p.error = r.err();
    });
}

/// True once the scene reached its end state.
pub fn ready(_cx: &App) -> bool {
    PROBE.with(|p| p.borrow().done)
}

/// The result of the scene's check.
pub fn verify(_png: &std::path::Path, _cx: &mut App) -> Result<(), String> {
    PROBE.with(|p| match &p.borrow().error {
        Some(e) => Err(e.clone()),
        None => Ok(()),
    })
}

/// The page of a version on disk (v4 says what it added).
fn write_version(version: u32) -> std::path::PathBuf {
    let html = if version >= 4 {
        PAGE.replace("<!--v-->", " · v4 加了「B 站动态」")
            .replace("<!--ch-->", "<button data-ch=\"B 站动态\">B 站动态</button>")
    } else {
        PAGE.to_string()
    };
    let dir = std::env::temp_dir()
        .join("team-chat-host-scene")
        .join(version.to_string());
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("index.html"), html);
    dir
}

pub struct HostPreview {
    services: Services,
    ledger: Option<Member>,
    title: SharedString,
}

impl Render for HostPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let spec = InlineSpec {
            key: MediaKey::new(d::ART_REPORT, 3),
            kind: MediaKind::Html,
            title: self.title.to_string(),
            bytes: PAGE.len() as u64,
            target: Target {
                provider: Some(self.services.provider.clone()),
                group_id: d::GROWTH.into(),
                topic_id: d::T_REPORT.into(),
            },
            id: SharedString::from(ROW),
        };
        let page = inline::render(&spec, window, cx).unwrap_or_else(|| div().into_any_element());
        let message = div()
            .flex()
            .gap(space::S12)
            .child(MemberAvatar::new(self.ledger.as_ref(), theme::AVATAR_SIZE))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(space::S2)
                    .child(
                        div()
                            .t_ui()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(k.text_1)
                            .child(SharedString::from(
                                self.ledger.as_ref().map(|m| m.name.clone()).unwrap_or_default(),
                            )),
                    )
                    .child(div().t_body().text_color(k.text_1).child(SAID))
                    .child(div().pt(space::S8).child(page)),
            );
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(k.content)
            .text_color(k.text_1)
            .child(
                toolbar_band("host-scene-bar")
                    .pl(theme::TRAFFIC_LIGHTS_INSET + space::S12)
                    .pr(space::S16)
                    .child(div().t_title().text_color(k.text_1).child(self.title.clone())),
            )
            .child(hairline(cx))
            .child(
                div()
                    .w_full()
                    .max_w(theme::MEASURE)
                    .mx_auto()
                    .px(theme::CONTENT_GUTTER)
                    .py(space::S24)
                    .child(message),
            )
    }
}

/// Opens the scene window and starts its check.
pub fn open(
    services: Services,
    content: gpui::Size<gpui::Pixels>,
    mode: Mode,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    PROBE.with(|p| *p.borrow_mut() = Probe::default());
    let title = services
        .provider
        .artifact(d::ART_REPORT)
        .map(|a| a.summary.title)
        .unwrap_or_default();
    for v in [3, 4] {
        store::request_folder(
            &MediaKey::new(d::ART_REPORT, v),
            &title,
            write_version(v),
            "index.html".into(),
            cx,
        );
    }
    let mut options = main_window_options(false, cx);
    options.window_bounds = Some(WindowBounds::Windowed(capture_bounds(content, cx)));
    options.window_min_size = None;
    let ledger = services.provider.member(d::LEDGER);
    let handle = gpui_kit::open_window(options, cx, move |_, cx| {
        cx.new(|_| HostPreview {
            services,
            ledger,
            title: title.into(),
        })
    })
    .map(|(handle, _)| handle)
    .map_err(|e| e.to_string())?;
    cx.spawn(async move |cx: &mut AsyncApp| finish(run(mode, cx).await))
        .detach();
    Ok(handle)
}

fn page(version: u32, cx: &App) -> Option<Entity<WebEmbedState>> {
    live(cx).into_iter().find(|e| {
        let s = e.read(cx);
        s.artifact_id() == d::ART_REPORT
            && s.version() == version
            && *s.phase() == Phase::Ready
            && s.shown()
            && s.bounds().is_some()
    })
}

async fn wait_for<T>(
    cx: &mut AsyncApp,
    timeout: Duration,
    mut f: impl FnMut(&mut App) -> Option<T>,
) -> Option<T> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(v) = cx.update(|cx| f(cx)) {
            return Some(v);
        }
        if Instant::now() > deadline {
            return None;
        }
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
    }
}

async fn eval(cx: &mut AsyncApp, page: &Entity<WebEmbedState>, script: &str) -> String {
    let (tx, rx) = futures::channel::oneshot::channel();
    let script = script.to_string();
    cx.update(|cx| {
        page.read(cx).evaluate_text(&script, move |v| {
            let _ = tx.send(v);
        })
    });
    rx.await.ok().flatten().unwrap_or_default()
}

/// Waits until `script` gives `want` in the page.
async fn until(
    cx: &mut AsyncApp,
    page: &Entity<WebEmbedState>,
    script: &str,
    want: &str,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let got = eval(cx, page, script).await;
        if got == want {
            return Ok(());
        }
        if Instant::now() > deadline {
            return Err(format!("{script} is {got:?}, not {want:?}"));
        }
        cx.background_executor()
            .timer(Duration::from_millis(60))
            .await;
    }
}

const REV: &str = "document.body.dataset.rev||''";
const SCRIPTED: &str = "document.body.dataset.scripted||''";
const ASK_AT: &str = "(function(){var r=document.getElementById('ask').getBoundingClientRect();\
return JSON.stringify({x:r.left+r.width/2,y:r.top+r.height/2});})()";

async fn run(mode: Mode, cx: &mut AsyncApp) -> Result<(), String> {
    let p3 = wait_for(cx, Duration::from_secs(15), |cx| page(3, cx))
        .await
        .ok_or("the 产出 page did not load")?;
    // The page got its snapshot (GET …/data) through onData.
    until(cx, &p3, REV, "1").await?;
    // The page's own ask on load never went out.
    until(cx, &p3, SCRIPTED, "not_allowed").await?;
    let asks_before = |cx: &App| {
        host::sent(cx)
            .iter()
            .filter(|s| s.3 == PageEventKind::Ask)
            .count()
    };
    if cx.update(|cx| asks_before(cx)) != 0 {
        return Err("an ask without a click reached the member".into());
    }
    match mode {
        Mode::LiveData => {
            cx.update(|cx| {
                host::on_signal(
                    PageSignal::Data {
                        artifact_id: d::ART_REPORT.into(),
                        version: 3,
                        data_rev: 2,
                        data: Some(serde_json::json!({
                            "channel": "B 站",
                            "weeks": ["10-06", "10-13", "10-20", "10-27"],
                            "values": [3, 2, 2, 2],
                            "total": 9,
                            "delta": "−1"
                        })),
                        cause: None,
                    },
                    cx,
                )
            });
            until(cx, &p3, REV, "2").await?;
            let band = cx.update(|cx| host::band(d::ART_REPORT, 3, cx));
            if !matches!(band, Some(host::BandView::Done(_))) {
                return Err(format!("the footer does not say the data came: {band:?}"));
            }
        }
        Mode::Reloaded => {
            cx.update(|cx| host::version_added(d::ART_REPORT, 4, cx));
            let p4 = wait_for(cx, Duration::from_secs(15), |cx| page(4, cx))
                .await
                .ok_or("the page did not reload in place to v4")?;
            until(cx, &p4, REV, "1").await?;
            let notice = cx.update(|cx| host::notice(d::ART_REPORT, 4, 4, cx));
            if notice.as_deref() != Some("已更新到 v4") {
                return Err(format!("the header note is {notice:?}"));
            }
        }
        Mode::AskPending => {
            let at = eval(cx, &p3, ASK_AT).await;
            let at: serde_json::Value = serde_json::from_str(&at).map_err(|e| e.to_string())?;
            let (x, y) = (
                at["x"].as_f64().unwrap_or(-1.),
                at["y"].as_f64().unwrap_or(-1.),
            );
            if !cx.update(|cx| p3.read(cx).post_click(x, y, true)) {
                return Err("the click could not be posted".into());
            }
            wait_for(cx, Duration::from_secs(5), |cx| {
                (asks_before(&*cx) >= 1).then_some(())
            })
            .await
            .ok_or("the click did not send the ask")?;
            // Past the quiet 400 ms: the band shows; doubled asks would show here.
            cx.background_executor()
                .timer(Duration::from_millis(700))
                .await;
            let sent: Vec<_> = cx.update(|cx| host::sent(cx));
            let asks: Vec<_> = sent
                .iter()
                .filter(|s| s.3 == PageEventKind::Ask)
                .collect();
            match asks.as_slice() {
                [(id, 3, name, _)] if id == d::ART_REPORT && name == "question" => {}
                other => return Err(format!("expected one question ask, got {other:?}")),
            }
            let band = cx.update(|cx| host::band(d::ART_REPORT, 3, cx));
            if !matches!(band, Some(host::BandView::Waiting(_))) {
                return Err(format!("the footer does not wait: {band:?}"));
            }
            // Show the question in the page (it is below the chart).
            cx.update(|cx| {
                p3.read(cx)
                    .evaluate_script("window.scrollTo(0,document.body.scrollHeight)")
            });
            // Still waiting in the page.
            let state = eval(cx, &p3, "document.body.dataset.ask||'waiting'").await;
            if state != "waiting" {
                return Err(format!("the page's ask ended: {state}"));
            }
        }
    }
    // Let the last frame draw.
    cx.background_executor()
        .timer(Duration::from_millis(250))
        .await;
    Ok(())
}
