//! Screenshot self-test scenes of the 产出 page states that no other scene reaches:
//!
//! - `artifact-embed-states`: two cards that embed a 产出. The first one's folder does not
//!   open (页面没有打开 · 重试); the second one is a file, not a web page (这个产出不是网页 ·
//!   在窗口打开).
//! - `artifact-ref-toast`: the 话题 of the report; a page element asks 就这里提问 through the
//!   page host (the same `ArtifactEvent::Ref` a page sends). The chip goes into the 话题
//!   composer, and the chip is the confirmation: no toast (NATIVE.md §7.4).

use std::cell::RefCell;
use std::time::{Duration, Instant};

use gpui::{
    AnyWindowHandle, App, AppContext as _, AsyncApp, Context, IntoElement, ParentElement, Render,
    Styled, Window, WindowBounds, div, px,
};

use super::host::{self, PageRef};
use crate::{
    app::{Services, main_window_options},
    data::DraftKey,
    fake::v2 as d,
    selftest::scenes::{capture_bounds, open_main},
    theme::{self, Type as _, space},
    ui::parts::{hairline, toolbar_band},
    views::cards::{
        v2::{CardEnv, CardMode, render},
        v21::samples::{EMBED_BROKEN, EMBED_FILE},
    },
};

// --- the embed states of a card ----------------------------------------------------------

pub struct EmbedStates {
    services: Services,
}

impl Render for EmbedStates {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let env = CardEnv {
            viewer: Some(d::ME.into()),
            ..CardEnv::from_services(&self.services)
        };
        let title = self
            .services
            .provider
            .topic(d::GROWTH, d::T_REPORT)
            .map(|t| t.title)
            .unwrap_or_default();
        // Side by side, so both cards fit the window whole.
        let mut cards = div().flex().items_start().gap(space::S24);
        for id in [EMBED_BROKEN, EMBED_FILE] {
            cards = cards.child(div().flex_1().min_w(px(0.)).children(render(
                &env,
                d::GROWTH,
                id,
                CardMode::Topic,
                window,
                cx,
            )));
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(k.content)
            .text_color(k.text_1)
            .child(
                toolbar_band("embed-states-bar")
                    .pl(theme::TRAFFIC_LIGHTS_INSET + space::S12)
                    .pr(space::S16)
                    .child(div().t_title().text_color(k.text_1).child(title)),
            )
            .child(hairline(cx))
            .child(
                div()
                    .w_full()
                    .max_w(px(1280.))
                    .mx_auto()
                    .px(theme::CONTENT_GUTTER)
                    .py(space::S24)
                    .child(cards),
            )
    }
}

/// Opens the window of `artifact-embed-states`.
pub fn open_embed_states(
    services: Services,
    content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let mut options = main_window_options(false, cx);
    options.window_bounds = Some(WindowBounds::Windowed(capture_bounds(content, cx)));
    options.window_min_size = None;
    gpui_kit::open_window(options, cx, move |_, cx| {
        cx.new(|_| EmbedStates { services })
    })
    .map(|(handle, _)| handle)
    .map_err(|e| e.to_string())
}

// --- the page ref toast -----------------------------------------------------------------

#[derive(Default)]
struct Probe {
    done: bool,
    error: Option<String>,
}

thread_local! {
    static PROBE: RefCell<Probe> = RefCell::new(Probe::default());
}

/// True once `artifact-ref-toast` asked about the page element.
pub fn ref_toast_ready(_cx: &App) -> bool {
    PROBE.with(|p| p.borrow().done)
}

/// The result of `artifact-ref-toast`'s check.
pub fn ref_toast_verify(_png: &std::path::Path, _cx: &mut App) -> Result<(), String> {
    PROBE.with(|p| match &p.borrow().error {
        Some(e) => Err(e.clone()),
        None => Ok(()),
    })
}

/// Setup of `artifact-ref-toast`: the report 话题 with an empty composer.
pub fn ref_toast_setup(services: &Services, cx: &mut App) {
    PROBE.with(|p| *p.borrow_mut() = Probe::default());
    crate::selftest::scenes::v2_open_topic(services, d::GROWTH, d::T_REPORT, cx);
}

/// Opens the main window of `artifact-ref-toast` and asks about the page element.
pub fn open_ref_toast(
    services: Services,
    content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let provider = services.provider.clone();
    let handle = open_main(services, content, cx)?;
    cx.spawn(async move |cx: &mut AsyncApp| {
        let r = ask_once(handle, &provider, cx).await;
        PROBE.with(|p| {
            let mut p = p.borrow_mut();
            p.done = true;
            p.error = r.err();
        });
    })
    .detach();
    Ok(handle)
}

async fn ask_once(
    handle: AnyWindowHandle,
    provider: &std::sync::Arc<dyn crate::data::ChatProvider>,
    cx: &mut AsyncApp,
) -> Result<(), String> {
    // Let the window draw its 话题 first.
    cx.background_executor()
        .timer(Duration::from_millis(300))
        .await;
    let key = DraftKey::topic(d::GROWTH, d::T_REPORT);
    let refs = |cx: &mut AsyncApp| cx.update(|_| provider.draft(&key).refs.len());
    if refs(cx) != 0 {
        return Err("the composer had chips before the ask".into());
    }
    let ask = artifact_view::ArtifactEvent::Ref {
        version: 3,
        label: "B 站 · 10-06".into(),
        value: Some(serde_json::json!(3)),
        selector: Some("#bars .col:nth-child(1)".into()),
    };
    {
        cx.update(|cx| {
            handle.update(cx, |_, window, cx| {
                host::on_page_event(
                    PageRef::Window(d::ART_REPORT.into()),
                    d::ART_REPORT,
                    &ask,
                    Some(window),
                    cx,
                )
            })
        })
        .map_err(|e| e.to_string())?;
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while refs(cx) != 1 {
        if Instant::now() > deadline {
            return Err(format!("the composer has {} chips, not 1", refs(cx)));
        }
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
    }
    // No toast (NATIVE.md §7.4): the composer's chip is the confirmation.
    cx.background_executor()
        .timer(Duration::from_millis(250))
        .await;
    Ok(())
}
