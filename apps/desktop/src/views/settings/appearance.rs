//! 外观 (NATIVE.md §8): three image tiles 浅色 / 深色 / 自动 as in System
//! Settings, and 消息文字大小 as a slider between a small and a large A. The
//! theme is kept in `layout.json` (`theme`), the text size with the other
//! settings of this Mac ([`super::prefs`]).

use gpui::{
    AnyElement, AnyWindowHandle, App, AppContext as _, Context, Entity, IntoElement, ParentElement,
    Pixels, Render, SharedString, Size, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
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
const TILE_RADIUS: Pixels = crate::ui::native::tokens::radius::ITEM;
/// The space between a tile and the ring of the chosen one.
const RING: Pixels = crate::ui::native::tokens::space::S2;
/// The button around a tile and its name: the picture, its ring space and
/// 2 px ring, the gap, the 20 name line, and p 4 on every side.
fn tile_box() -> (Pixels, Pixels) {
    use crate::ui::native::tokens::{space, text};
    // Ring space and the 2 px ring on both sides, then p 4 on both sides.
    let around = RING * 2. + space::S4 + space::S8;
    (TILE.0 + around, TILE.1 + around + space::S8 + text::UI.1)
}

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
        // The neutral background, sidebar and border of each theme (1.2).
        let light = (
            gpui::rgb(0xFFFFFF),
            gpui::rgb(0xF5F5F5),
            gpui::rgb(0xD4D4D4),
        );
        let dark = (
            gpui::rgb(0x0A0A0A),
            gpui::rgb(0x262626),
            gpui::rgb(0x525252),
        );
        // Every fill that meets the tile's edge takes the tile's inner
        // radius: gpui does not clip children to a rounded parent (C1-01).
        let inner = TILE_RADIUS - crate::ui::native::tokens::layout::BORDER;
        let half = |(content, sidebar, line): (gpui::Rgba, gpui::Rgba, gpui::Rgba),
                    left: bool,
                    right: bool| {
            div()
                .flex_1()
                .h_full()
                .bg(content)
                .when(left, |d| d.rounded_l(inner))
                .when(right, |d| d.rounded_r(inner))
                .flex()
                .child(
                    div()
                        .w(px(20.))
                        .h_full()
                        .bg(sidebar)
                        .when(left, |d| d.rounded_l(inner))
                        .p(px(4.))
                        .flex()
                        .flex_col()
                        .gap(crate::ui::native::tokens::space::S4)
                        .child(div().h(px(3.)).w(px(10.)).rounded(px(1.5)).bg(line))
                        .child(div().h(px(3.)).w(px(8.)).rounded(px(1.5)).bg(line)),
                )
                .child(
                    div()
                        .flex_1()
                        .p(px(5.))
                        .flex()
                        .flex_col()
                        .gap(crate::ui::native::tokens::space::S4)
                        .child(div().h(px(3.)).w(px(22.)).rounded(px(1.5)).bg(line))
                        .child(div().h(px(3.)).w(px(16.)).rounded(px(1.5)).bg(line)),
                )
        };
        let row = div().size_full().flex();
        match choice {
            ThemeChoice::Light => row.child(half(light, true, true)),
            ThemeChoice::Dark => row.child(half(dark, true, true)),
            ThemeChoice::System => row
                .child(half(light, true, false))
                .child(half(dark, false, true)),
        }
        .into_any_element()
    }
}

impl Render for AppearancePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::ui::native::tokens::{space, text};
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
                // A ghost button: a Tab stop with the focus ring, Space and
                // Return choose, muted on hover (E3, C1-05).
                crate::ui::native::row_button(
                    SharedString::from(format!("appearance-theme-{}", choice.slug())),
                    cx,
                )
                .w(tile_box().0)
                .h(tile_box().1)
                .p(space::S4)
                .accessibility_label(title)
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(space::S8)
                        .child(
                            // The chosen tile: a 2 px foreground ring (2.17
                            // selected = foreground border).
                            div()
                                .p(RING)
                                .rounded(TILE_RADIUS + RING * 2.)
                                .border_2()
                                .border_color(if on {
                                    k.label
                                } else {
                                    gpui::transparent_black()
                                })
                                .child(
                                    div()
                                        .w(TILE.0)
                                        .h(TILE.1)
                                        .rounded(TILE_RADIUS)
                                        .overflow_hidden()
                                        .border_1()
                                        .border_color(k.separator)
                                        .child(Self::picture(choice)),
                                ),
                        )
                        .child(
                            div()
                                .n_body()
                                .when(on, |d| d.font_weight(gpui::FontWeight::MEDIUM))
                                .text_color(if on { k.label } else { k.secondary })
                                .child(title),
                        ),
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
        let size = prefs::get().message_size;
        let mark = |t: (Pixels, Pixels)| {
            div()
                .flex_none()
                .text_size(t.0)
                .line_height(t.1)
                .text_color(k.secondary)
                .child(s::TEXT_SIZE_MARK)
        };
        let theme_section = widgets::section(
            crate::ui::native::SectionTitle::new(s::SECTION_THEME)
                .description(s::SECTION_THEME_HINT),
            GroupedForm::new().row(
                div()
                    .w_full()
                    .p(crate::ui::native::metrics::FORM_ROW_INSET)
                    .flex()
                    .gap(space::S24)
                    .children(tiles),
            ),
        );
        let text_section = widgets::section(
            crate::ui::native::SectionTitle::new(s::SECTION_TEXT).description(s::SECTION_TEXT_HINT),
            GroupedForm::new()
                .row(
                    FormRow::new(s::MESSAGE_TEXT_SIZE).control(
                        div()
                            .w(crate::ui::native::metrics::INLINE_FIELD_WIDTH + space::S48)
                            .flex()
                            .items_center()
                            .gap(space::S12)
                            .child(mark(text::META))
                            .child(
                                div().flex_1().child(
                                    Slider::new(&self.size)
                                        .horizontal()
                                        .accessibility_label(s::MESSAGE_TEXT_SIZE),
                                ),
                            )
                            .child(mark(text::HEADING))
                            .child(
                                div()
                                    .flex_none()
                                    .w(space::S48)
                                    .flex()
                                    .justify_end()
                                    .n_body()
                                    .n_figures()
                                    .text_color(k.secondary)
                                    .child(crate::copy::fill(
                                        s::TEXT_SIZE_VALUE,
                                        &[("n", &size.to_string())],
                                    )),
                            ),
                    ),
                )
                // The preview: a message line at the chosen size.
                .row(
                    div()
                        .w_full()
                        .px(crate::ui::native::metrics::FORM_ROW_INSET)
                        .py(space::S12)
                        .text_size(px(f32::from(size)))
                        .line_height(px(f32::from(size) * 1.5))
                        .text_color(k.label)
                        .child(s::TEXT_PREVIEW),
                ),
        );
        widgets::pane().child(theme_section).child(text_section)
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
