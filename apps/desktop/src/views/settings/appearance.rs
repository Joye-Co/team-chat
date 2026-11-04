//! 外观 (NATIVE.md §8): three image tiles 浅色 / 深色 / 自动 as in System
//! Settings, and 消息文字大小 as a slider between a small and a large A. The
//! theme is kept in `layout.json` (`theme`), the text size with the other
//! settings of this Mac ([`super::prefs`]).

use gpui::{
    AnyElement, AnyWindowHandle, App, AppContext as _, Context, Entity, InteractiveElement as _,
    IntoElement, ParentElement, Pixels, Render, SharedString, Size, StatefulInteractiveElement as _,
    Styled, Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::slider::{Slider, SliderEvent, SliderState};

use super::{Page, prefs, widgets};
use crate::{
    app::Services,
    copy::settings as s,
    theme::{self, ThemeChoice},
    ui::native::{FormRow, GroupedForm, NativeType as _, native_colors},
    views::shell::layout,
};

/// A tile's picture (System Settings: about 68 × 44).
const TILE: (Pixels, Pixels) = (px(68.), px(44.));
const TILE_RADIUS: Pixels = px(7.);
/// The selection ring around the chosen tile.
const RING: Pixels = px(3.);

pub struct AppearancePage {
    size: Entity<SliderState>,
    _subscriptions: Vec<Subscription>,
}

impl AppearancePage {
    pub fn new(_services: Services, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let now = prefs::get().message_size;
        let size = cx.new(|_| {
            SliderState::new()
                .min(f32::from(prefs::MESSAGE_SIZE_MIN))
                .max(f32::from(prefs::MESSAGE_SIZE_MAX))
                .step(1.)
                .default_value(f32::from(now))
        });
        let subscriptions = vec![cx.subscribe(&size, |_, _, e: &SliderEvent, cx| {
            let (SliderEvent::Change(v) | SliderEvent::Release(v)) = e;
            let pt = v.start().round().clamp(
                f32::from(prefs::MESSAGE_SIZE_MIN),
                f32::from(prefs::MESSAGE_SIZE_MAX),
            ) as u8;
            if prefs::get().message_size != pt {
                prefs::update(|p| p.message_size = pt);
                cx.notify();
            }
        })];
        Self {
            size,
            _subscriptions: subscriptions,
        }
    }

    /// A tiny window in light or dark colors, or both halves for 自动. The
    /// tile shows the other theme too, so its swatches do not follow the
    /// current one (§4 `content` and `sidebar` of each mode).
    fn picture(choice: ThemeChoice) -> AnyElement {
        let light = (gpui::rgb(0xFFFFFF), gpui::rgb(0xE9E9EB), gpui::rgb(0xC9C9CE));
        let dark = (gpui::rgb(0x1E1E1E), gpui::rgb(0x2E2E31), gpui::rgb(0x4A4A4E));
        let half = |(content, sidebar, line): (gpui::Rgba, gpui::Rgba, gpui::Rgba)| {
            div()
                .flex_1()
                .h_full()
                .bg(content)
                .flex()
                .child(
                    div()
                        .w(px(20.))
                        .h_full()
                        .bg(sidebar)
                        .p(px(4.))
                        .flex()
                        .flex_col()
                        .gap(px(3.))
                        .child(div().h(px(3.)).w(px(10.)).rounded(px(1.5)).bg(line))
                        .child(div().h(px(3.)).w(px(8.)).rounded(px(1.5)).bg(line)),
                )
                .child(
                    div()
                        .flex_1()
                        .p(px(5.))
                        .flex()
                        .flex_col()
                        .gap(px(3.))
                        .child(div().h(px(3.)).w(px(22.)).rounded(px(1.5)).bg(line))
                        .child(div().h(px(3.)).w(px(16.)).rounded(px(1.5)).bg(line)),
                )
        };
        let row = div().size_full().flex();
        match choice {
            ThemeChoice::Light => row.child(half(light)),
            ThemeChoice::Dark => row.child(half(dark)),
            ThemeChoice::System => row.child(half(light)).child(half(dark)),
        }
        .into_any_element()
    }
}

impl Render for AppearancePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        let current = theme::theme_choice();
        let saving = prefs::saving();
        let tiles: Vec<AnyElement> = [ThemeChoice::Light, ThemeChoice::Dark, ThemeChoice::System]
            .into_iter()
            .map(|choice| {
                let on = choice == current;
                let title = match choice {
                    ThemeChoice::Light => s::THEME_LIGHT,
                    ThemeChoice::Dark => s::THEME_DARK,
                    ThemeChoice::System => s::THEME_AUTO,
                };
                div()
                    .id(SharedString::from(format!("appearance-theme-{}", choice.slug())))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(5.))
                    .child(
                        div()
                            .p(RING)
                            .rounded(TILE_RADIUS + RING)
                            .when(on, |d| d.bg(k.accent))
                            .child(
                                div()
                                    .w(TILE.0)
                                    .h(TILE.1)
                                    .rounded(TILE_RADIUS)
                                    .overflow_hidden()
                                    .shadow(vec![k.inner_edge(k.separator)])
                                    .child(Self::picture(choice)),
                            ),
                    )
                    .child(
                        div()
                            .n_subheadline()
                            .text_color(if on { k.label } else { k.secondary })
                            .child(title),
                    )
                    .on_click(move |_, window, cx| {
                        theme::set_theme_choice(choice);
                        if saving {
                            layout::save_theme(choice);
                        }
                        theme::apply_theme_choice(window, cx);
                    })
                    .into_any_element()
            })
            .collect();
        let mark = |size: Pixels| {
            div()
                .flex_none()
                .text_size(size)
                .line_height(px(18.))
                .text_color(k.secondary)
                .child(s::TEXT_SIZE_MARK)
        };
        let theme_row = div()
            .w_full()
            .px(crate::ui::native::metrics::FORM_ROW_INSET)
            .py(px(12.))
            .flex()
            .items_start()
            .child(
                div()
                    .flex_1()
                    .pt(px(16.))
                    .n_body()
                    .text_color(k.label)
                    .child(s::APPEARANCE),
            )
            .child(div().flex().gap(px(16.)).children(tiles));
        widgets::pane().child(
            GroupedForm::new().row(theme_row).row(
                FormRow::new(s::MESSAGE_TEXT_SIZE).control(
                    div()
                        .w(px(220.))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(mark(px(11.)))
                        .child(
                            div().flex_1().child(
                                Slider::new(&self.size)
                                    .horizontal()
                                    .accessibility_label(s::MESSAGE_TEXT_SIZE),
                            ),
                        )
                        .child(mark(px(17.))),
                ),
            ),
        )
    }
}

/// The `settings-appearance` scene: the settings window on 外观.
pub fn open_scene(
    services: Services,
    _content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    super::prefs::stop_saving();
    super::open_window(services, Page::Appearance, false, cx).map(|(h, _)| h)
}
