//! Self-tests of the page-event gate: a page event (`emitEvent`) reaches the member only
//! right after the viewer's own input in that page.
//!
//! - `artifact-v2-choose`: the 产出 page of 10 月数据月报 v3 in the main window. The test
//!   posts a real left click (mouse-down, mouse-up through `NSApplication`'s event queue)
//!   on the 抖音 option, then checks that the provider got exactly one `choose` event with
//!   that option, and nothing before the click.
//! - `artifact-quiet-page`: a page that calls `emitEvent` on load, from a timer and from a
//!   script `click()`, with a real click in the window outside the page. The test checks
//!   that the page called `emitEvent` and that the provider got nothing.
//!
//! Each scene checks itself after the capture ([`verify`]).

use std::cell::RefCell;
use std::sync::Arc;
use std::time::{Duration, Instant};

use artifact_view::{ArtifactEvent, Capabilities, Content, VersionSpec};
use gpui::{
    AnyWindowHandle, App, AppContext as _, AsyncApp, Context, Entity, IntoElement, ParentElement,
    Render, SharedString, Styled, Window, WindowBounds, div, px,
};

use crate::{
    app::{Services, main_window_options},
    data::ChatProvider,
    fake::v2::{self as d, sent_page_events},
    selftest::scenes::capture_bounds,
    theme::{self, Type as _, space},
    ui::{
        parts::{hairline, toolbar_band},
        web_embed::{Phase, WebEmbed, WebEmbedSpec, WebEmbedState, live},
    },
};

/// The version the quiet page claims (the sample provider declares `choose` on it).
const QUIET_VERSION: u32 = 3;
const QUIET_HEIGHT: f32 = 320.;

#[derive(Default)]
struct Probe {
    /// The check finished (passed or failed).
    done: bool,
    /// Why the check failed.
    error: Option<String>,
    /// `user_action` of each page event the bridge passed up (quiet page).
    seen: Vec<bool>,
}

thread_local! {
    static PROBE: RefCell<Probe> = RefCell::new(Probe::default());
}

fn reset() {
    sent_page_events::take();
    PROBE.with(|p| *p.borrow_mut() = Probe::default());
}

fn finish(result: Result<(), String>) {
    PROBE.with(|p| {
        let mut p = p.borrow_mut();
        p.done = true;
        p.error = result.err();
    });
}

/// True once the scene's check finished.
pub fn ready(_cx: &App) -> bool {
    PROBE.with(|p| p.borrow().done)
}

/// The result of the scene's check.
pub fn verify(_png: &std::path::Path, _cx: &mut App) -> Result<(), String> {
    PROBE.with(|p| {
        let p = p.borrow();
        match (&p.error, p.done) {
            (Some(e), _) => Err(e.clone()),
            (None, true) => Ok(()),
            (None, false) => Err("the page-event check did not finish".into()),
        }
    })
}

/// The shown, loaded page of `artifact_id`.
fn page(artifact_id: &str, cx: &App) -> Option<Entity<WebEmbedState>> {
    live(cx).into_iter().find(|e| {
        let s = e.read(cx);
        s.artifact_id() == artifact_id
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

async fn eval(cx: &mut AsyncApp, page: &Entity<WebEmbedState>, script: &str) -> Option<String> {
    let (tx, rx) = futures::channel::oneshot::channel();
    let script = script.to_string();
    cx.update(|cx| {
        page.read(cx).evaluate_text(&script, move |v| {
            let _ = tx.send(v);
        })
    });
    rx.await.ok().flatten()
}

async fn pause(cx: &mut AsyncApp, ms: u64) {
    cx.background_executor()
        .timer(Duration::from_millis(ms))
        .await;
}

// --- artifact-v2-choose ---------------------------------------------------------------

/// Opens 10 月数据月报 like `artifact-v2` and starts the click check.
pub fn setup_choose(services: &Services, cx: &mut App) {
    reset();
    services.nav.update(cx, |nav, cx| {
        nav.open_artifact(d::ART_REPORT.into(), None, cx)
    });
    cx.spawn(async move |cx: &mut AsyncApp| finish(check_choose(cx).await))
        .detach();
}

/// Scrolls the 抖音 option into view and gives its centre in page points.
const FIND_OPTION: &str = "(function(){var b=document.querySelector('#opts .opt[data-id=\"c\"]');\
if(!b)return null;b.scrollIntoView({block:'center',behavior:'instant'});\
var r=b.getBoundingClientRect();return JSON.stringify({x:r.left+r.width/2,y:r.top+r.height/2,\
vw:window.innerWidth,vh:window.innerHeight});})()";
const PICKED: &str = "(function(){var b=document.querySelector('#opts .opt.on');\
return b?b.getAttribute('data-id'):'';})()";
/// Where the ✓ sits, how many there are, and the footer: `c 1 你选了「抖音」· 10:21 ...`.
const MARKED: &str = "(function(){var m=document.querySelectorAll('#opts .ok');\
var o=m.length?m[0].closest('.opt'):null;return (o?o.getAttribute('data-id'):'-')+' '+m.length+' '+\
document.getElementById('sent').textContent;})()";

async fn check_choose(cx: &mut AsyncApp) -> Result<(), String> {
    let page = wait_for(cx, Duration::from_secs(15), |cx| page(d::ART_REPORT, cx))
        .await
        .ok_or("the 产出 page did not load")?;
    // Frames settle; a page that sent on its own would show here.
    pause(cx, 600).await;
    if !sent_page_events::snapshot().is_empty() {
        return Err("a page event went to the member before any click".into());
    }
    let at = eval(cx, &page, FIND_OPTION)
        .await
        .ok_or("the option button is not in the page")?;
    let at: serde_json::Value = serde_json::from_str(&at).map_err(|e| e.to_string())?;
    let (x, y) = (
        at["x"].as_f64().unwrap_or(-1.),
        at["y"].as_f64().unwrap_or(-1.),
    );
    let (vw, vh) = (
        at["vw"].as_f64().unwrap_or(0.),
        at["vh"].as_f64().unwrap_or(0.),
    );
    if !(0. ..vw).contains(&x) || !(0. ..vh).contains(&y) {
        return Err(format!("the option button is outside the page view: {at}"));
    }
    // The scroll draws before the click lands.
    pause(cx, 300).await;
    let posted = cx.update(|cx| page.read(cx).post_click(x, y, true));
    if !posted {
        return Err("the click could not be posted".into());
    }
    let sent = wait_for(cx, Duration::from_secs(5), |_| {
        let s = sent_page_events::snapshot();
        (!s.is_empty()).then_some(s)
    })
    .await;
    // Late or doubled events would show here.
    pause(cx, 800).await;
    let picked = eval(cx, &page, PICKED).await.unwrap_or_default();
    let sent_all = sent_page_events::snapshot();
    if sent.is_none() {
        return Err(if picked == "c" {
            "the click reached the page, but its event never reached the member".into()
        } else {
            format!("the click did not reach the page (picked {picked:?})")
        });
    }
    match sent_all.as_slice() {
        [(id, version, name, Some(payload))]
            if id == d::ART_REPORT
                && *version == 3
                && name == "choose"
                && payload["option"] == "c" =>
        {
            // One choice: the ✓ moved to 抖音, at the sample's 10:21.
            let marked = eval(cx, &page, MARKED).await.unwrap_or_default();
            if marked.starts_with("c 1 ") && marked.contains("「抖音」· 10:21") {
                Ok(())
            } else {
                Err(format!("the page did not show the new choice: {marked:?}"))
            }
        }
        other => Err(format!(
            "expected one choose event for option c, got {other:?}"
        )),
    }
}

// --- artifact-quiet-page --------------------------------------------------------------

/// A page that tries to send on its own: on load, from a timer, and from a script click.
const QUIET_HTML: &str = r#"<!doctype html><html lang="zh-CN"><head><meta charset="utf-8">
<style>
:root{color-scheme:light dark}
body{margin:0;padding:28px 32px;font:14px/1.6 -apple-system,"PingFang SC",sans-serif;color:#111113;background:#fff}
h2{margin:0 0 6px;font-size:18px}p{margin:0 0 16px;color:#5B5B66}
button{height:34px;padding:0 14px;border:1px solid #D9D9DE;border-radius:9px;background:#fff;font:inherit;color:inherit}
@media (prefers-color-scheme:dark){body{color:#F1F1F3;background:#111113}p{color:#A1A1AA}button{background:#1B1B1F;border-color:#34343A}}
</style></head><body>
<h2>渠道预算</h2><p>这个页面在打开时、定时和用脚本点按钮时都试着发回应。</p>
<button type="button" id="go">选小红书</button>
<script>
(function(){
  function send(why){if(window.artifact&&window.artifact.emitEvent){window.artifact.emitEvent('choose',{option:'a',why:why}).catch(function(){});}}
  var b=document.getElementById('go');
  b.addEventListener('click',function(){send('click')});
  send('load');
  b.click();
  var n=0;var t=setInterval(function(){n++;send('timer');if(n>=15)clearInterval(t);},200);
})();
</script></body></html>"#;

/// Opens a window with the quiet page and starts its check.
pub fn open_quiet(
    services: Services,
    content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    reset();
    let mut options = main_window_options(false, cx);
    options.window_bounds = Some(WindowBounds::Windowed(capture_bounds(content, cx)));
    options.window_min_size = None;
    let handle = gpui_kit::open_window(options, cx, move |_, cx| {
        cx.new(|_| QuietPage {
            provider: services.provider.clone(),
            title: services
                .provider
                .artifact(d::ART_REPORT)
                .map(|a| SharedString::from(a.summary.title))
                .unwrap_or_default(),
        })
    })
    .map(|(handle, _)| handle)
    .map_err(|e| e.to_string())?;
    cx.spawn(async move |cx: &mut AsyncApp| finish(check_quiet(cx).await))
        .detach();
    Ok(handle)
}

struct QuietPage {
    provider: Arc<dyn ChatProvider>,
    title: SharedString,
}

impl Render for QuietPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let spec = WebEmbedSpec::new(
            d::ART_REPORT,
            VersionSpec::new(
                QUIET_VERSION,
                Content::single("index.html", QUIET_HTML.as_bytes()),
            ),
        )
        .capabilities(Capabilities {
            emit: true,
            ..Capabilities::NONE
        });
        let page = WebEmbed::new("quiet-page", spec)
            .forget_preview()
            .height(px(QUIET_HEIGHT))
            .rounded(theme::radius::LG)
            .on_event(move |event, _, cx| {
                if let ArtifactEvent::Emit {
                    version,
                    name,
                    payload,
                    user_action,
                    reply_id,
                } = event
                {
                    PROBE.with(|p| p.borrow_mut().seen.push(*user_action));
                    super::forward_page_event(
                        crate::views::artifacts::host::PageRef::Window(d::ART_REPORT.into()),
                        d::ART_REPORT,
                        *version,
                        name,
                        payload,
                        *user_action,
                        reply_id.clone(),
                        cx,
                    );
                }
            });
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(k.content)
            .text_color(k.text_1)
            .child(
                toolbar_band("quiet-bar")
                    .pl(theme::TRAFFIC_LIGHTS_INSET + space::S12)
                    .pr(space::S16)
                    .child(
                        div()
                            .t_title()
                            .text_color(k.text_1)
                            .child(self.title.clone()),
                    ),
            )
            .child(hairline(cx))
            .child(
                div()
                    .w_full()
                    .max_w(theme::MEASURE)
                    .mx_auto()
                    .px(theme::CONTENT_GUTTER)
                    .py(space::S24)
                    .child(page),
            )
    }
}

async fn check_quiet(cx: &mut AsyncApp) -> Result<(), String> {
    let page = wait_for(cx, Duration::from_secs(15), |cx| page(d::ART_REPORT, cx))
        .await
        .ok_or("the quiet page did not load")?;
    // A real click in the window, below the page: input outside the page must not count.
    let posted = cx
        .update(|cx| {
            let s = page.read(cx);
            let b = s.bounds()?;
            let x = f32::from(b.origin.x + b.size.width / 2.) as f64;
            let y = f32::from(b.origin.y + b.size.height) as f64 + 48.;
            Some(s.post_click(x, y, false))
        })
        .unwrap_or(false);
    if !posted {
        return Err("the click outside the page could not be posted".into());
    }
    // The page's timer sends for 3 s after load.
    pause(cx, 3200).await;
    let seen = PROBE.with(|p| p.borrow().seen.clone());
    let sent = sent_page_events::snapshot();
    if seen.len() < 5 {
        return Err(format!(
            "the page called emitEvent only {} times",
            seen.len()
        ));
    }
    if seen.iter().any(|u| *u) {
        return Err("a page event without input in the page counted as the viewer's".into());
    }
    if !sent.is_empty() {
        return Err(format!(
            "page events without a click reached the member: {sent:?}"
        ));
    }
    Ok(())
}
