//! Screenshot self-test scenes of the 产出 page states that no other scene reaches:
//!
//! - `artifact-embed-states`: two cards that embed a 产出. The first one's folder does not
//!   open (页面没有打开 · 重试); the second one is a file, not a web page (这个产出不是网页 ·
//!   在窗口打开).

use gpui::{
    AnyWindowHandle, App, AppContext as _, Context, IntoElement, ParentElement, Render, Styled,
    Window, WindowBounds, div, px,
};

use crate::{
    app::{Services, main_window_options},
    fake::v2 as d,
    selftest::scenes::capture_bounds,
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
