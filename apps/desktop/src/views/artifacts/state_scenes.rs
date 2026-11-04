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

use gpui::{AnyWindowHandle, App, AsyncApp};

use super::host::{self, PageRef};
use crate::{
    app::Services,
    data::DraftKey,
    fake::v2 as d,
    selftest::scenes::open_main,
    views::cards::v21::samples::{EMBED_BROKEN, EMBED_FILE},
};

// --- the embed states of a card ----------------------------------------------------------

/// Opens `artifact-embed-states`: the real main window on the report 话题, with the two
/// cards that embed a 产出 at the end of its thread (E2, B3: the same thread, widths and
/// x as every other card).
pub fn open_embed_states(
    services: Services,
    content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let thread = crate::views::media::preview::MediaThread::with_cards(
        services.provider.clone(),
        d::GROWTH,
        d::T_REPORT,
        [EMBED_BROKEN, EMBED_FILE]
            .iter()
            .map(|id| (id.to_string(), d::LEDGER.to_string()))
            .collect(),
    );
    let provider: std::sync::Arc<dyn crate::data::ChatProvider> = thread;
    let services = Services::new(provider, cx);
    crate::selftest::scenes::v2_open_topic(&services, d::GROWTH, d::T_REPORT, cx);
    open_main(services, content, cx)
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

/// The checks of `artifact-embed-states`: the page that did not open keeps the page frame;
/// the PDF is framed as a file (its kind, no sandbox, no page actions).
pub fn embed_states_checks(_: &Services, _: &mut App) -> Vec<Result<String, String>> {
    use crate::selftest::drawn::{check, ids};
    let frames = ids("embed-frame");
    let kinds: Vec<&str> = frames
        .iter()
        .filter_map(|f| f.rsplit_once(':').map(|(_, k)| k))
        .collect();
    vec![
        check(
            kinds.contains(&crate::copy::media::KIND_PDF),
            format!("the PDF embed is framed as a PDF file ({frames:?})"),
        ),
        check(
            kinds
                .iter()
                .filter(|k| **k == crate::copy::cards21::EMBED_KIND)
                .count()
                == 1,
            format!("only the web page keeps the 网页 frame ({frames:?})"),
        ),
    ]
}
