//! 设置 → 外观 (1.1.0): 主题 (跟随系统, 浅色, 深色) and 侧栏样式 (rail modes
//! A, B, C). Both are kept on this Mac in `layout.json`.
//!
//! Each choice is a card with a small preview: a window in light, dark or
//! both for 主题, a rail drawn in its mode for 侧栏样式. The rail's list
//! header menu switches the mode too (`views::rail`).

use gpui::{
    AnyElement, AnyWindowHandle, App, Context, InteractiveElement as _, IntoElement, ParentElement,
    Pixels, Render, SharedString, Size, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};

use super::Page;
use crate::{
    app::{RailMode, Services},
    copy::appearance as c,
    theme::{self, ThemeChoice, Type as _, space},
    ui::{self, Lucide},
    views::shell::{layout, section_label},
};

pub struct AppearancePage {
    services: Services,
    /// Save choices to `layout.json` (the app, never a self-test scene).
    persist: bool,
}

impl AppearancePage {
    pub fn new(services: Services, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self {
            services,
            persist: true,
        }
    }

    /// A choice card: a preview on top, the title (and help) below, a 1 pt
    /// `line` border that turns `ink` when chosen, with a check.
    fn card(
        &self,
        id: String,
        preview: AnyElement,
        title: &'static str,
        help: Option<&'static str>,
        on: bool,
        cx: &App,
    ) -> gpui::Stateful<gpui::Div> {
        let k = theme::colors(cx);
        div()
            .id(SharedString::from(id))
            .flex_1()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .gap(space::S8)
            .cursor_pointer()
            .child(
                div()
                    .relative()
                    .h(PREVIEW_HEIGHT)
                    .rounded(px(10.))
                    .border_1()
                    .border_color(if on { k.ink } else { k.line })
                    .when(on, |el| el.shadow(k.lift()))
                    .when(!on, |el| el.hover(|s| s.border_color(k.ink_3)))
                    .overflow_hidden()
                    .child(preview)
                    .when(on, |el| {
                        el.child(
                            div()
                                .absolute()
                                .top(space::S6)
                                .right(space::S6)
                                .size(px(18.))
                                .rounded_full()
                                .bg(k.btn)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(ui::icon(Lucide::Check, px(12.), k.btn_ink)),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(space::S2)
                    .child(
                        div()
                            .t_small()
                            .text_color(k.ink)
                            .when(on, |el| el.medium())
                            .child(title),
                    )
                    .children(help.map(|h| div().t_meta().text_color(k.ink_3).child(h))),
            )
    }

    /// A tiny window in `dark` or light colors (or both halves for 跟随系统).
    fn theme_preview(choice: ThemeChoice) -> AnyElement {
        // Fixed swatches: the preview shows the other theme too, so it does
        // not follow the current one (HELMET2 `--bg`, `--rail`, `--line-2`).
        let light = (
            gpui::rgb(0xFFFFFF),
            gpui::rgb(0xF7F7F8),
            gpui::rgb(0xDCDCE0),
            gpui::rgb(0x111113),
        );
        let dark = (
            gpui::rgb(0x111113),
            gpui::rgb(0x161618),
            gpui::rgb(0x34343A),
            gpui::rgb(0xF1F1F3),
        );
        // Children paint over the card's rounded border, so each half rounds
        // its own outer corners (`left`, `right`).
        let r = PREVIEW_RADIUS;
        let half = |(bg, rail, line, ink): (gpui::Rgba, gpui::Rgba, gpui::Rgba, gpui::Rgba),
                    left: bool,
                    right: bool| {
            div()
                .flex_1()
                .h_full()
                .bg(bg)
                .overflow_hidden()
                .when(left, |el| el.rounded_l(r))
                .when(right, |el| el.rounded_r(r))
                .flex()
                .child(
                    div()
                        .w(px(34.))
                        .h_full()
                        .bg(rail)
                        .when(left, |el| el.rounded_l(r))
                        .p(px(6.))
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(div().h(px(4.)).w(px(18.)).rounded(px(2.)).bg(ink))
                        .child(div().h(px(4.)).w(px(14.)).rounded(px(2.)).bg(line))
                        .child(div().h(px(4.)).w(px(16.)).rounded(px(2.)).bg(line)),
                )
                .child(
                    div()
                        .flex_1()
                        .p(px(8.))
                        .flex()
                        .flex_col()
                        .gap(px(5.))
                        .child(div().h(px(5.)).w(px(40.)).rounded(px(2.)).bg(ink))
                        .child(div().h(px(4.)).w(px(56.)).rounded(px(2.)).bg(line))
                        .child(div().h(px(4.)).w(px(48.)).rounded(px(2.)).bg(line)),
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

    /// A tiny rail of `mode`, drawn in the current theme.
    fn rail_preview(mode: RailMode, cx: &App) -> AnyElement {
        let k = theme::colors(cx);
        let line = |w: f32, color: gpui::Hsla| div().h(px(4.)).w(px(w)).rounded(px(2.)).bg(color);
        let row = |lead: AnyElement, w: f32| {
            div()
                .h(px(10.))
                .flex()
                .items_center()
                .gap(px(5.))
                .child(div().w(px(8.)).flex().justify_center().child(lead))
                .child(line(w, k.line_2))
        };
        let dot = |color: gpui::Hsla| {
            div()
                .size(px(5.))
                .rounded_full()
                .bg(color)
                .into_any_element()
        };
        let tile = |ring: gpui::Hsla| {
            div()
                .size(px(8.))
                .rounded(px(2.))
                .bg(k.sunken)
                .border_1()
                .border_color(ring)
                .into_any_element()
        };
        let header = |w: f32| {
            div()
                .h(px(10.))
                .flex()
                .items_center()
                .child(line(w, k.ink_3))
        };
        let rows: Vec<AnyElement> = match mode {
            RailMode::Work => vec![
                header(20.).into_any_element(),
                row(dot(k.you), 52.).into_any_element(),
                row(tile(k.live), 44.).into_any_element(),
                row(dot(k.ink_3), 48.).into_any_element(),
                header(16.).into_any_element(),
                row(dot(k.you), 40.).into_any_element(),
                row(
                    ui::icon(Lucide::Check, px(8.), k.ok).into_any_element(),
                    50.,
                )
                .into_any_element(),
            ],
            RailMode::Attention => vec![
                div()
                    .h(px(10.))
                    .flex()
                    .gap(px(3.))
                    .child(div().w(px(14.)).h(px(8.)).rounded(px(2.)).bg(k.sel))
                    .child(line(12., k.line_2))
                    .child(line(12., k.line_2))
                    .into_any_element(),
                header(18.).into_any_element(),
                row(dot(k.you), 52.).into_any_element(),
                row(dot(k.you), 44.).into_any_element(),
                header(14.).into_any_element(),
                row(tile(k.live), 48.).into_any_element(),
                row(tile(k.live), 40.).into_any_element(),
            ],
            RailMode::Member => vec![
                header(20.).into_any_element(),
                row(tile(k.live), 30.).into_any_element(),
                div()
                    .pl(px(10.))
                    .child(row(dot(k.live), 40.))
                    .into_any_element(),
                div()
                    .pl(px(10.))
                    .child(row(dot(k.you), 44.))
                    .into_any_element(),
                row(tile(k.you), 30.).into_any_element(),
                row(tile(k.live), 34.).into_any_element(),
                row(tile(k.line_2), 26.).into_any_element(),
            ],
        };
        div()
            .size_full()
            .rounded(PREVIEW_RADIUS)
            .bg(k.rail)
            .p(px(10.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .children(rows)
            .into_any_element()
    }
}

/// The preview of every choice card.
const PREVIEW_HEIGHT: Pixels = px(96.);
/// Inside the card's 10 pt corner and 1 pt border.
const PREVIEW_RADIUS: Pixels = px(9.);

impl Render for AppearancePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current_theme = theme::theme_choice();
        let current_rail = self.services.nav.read(cx).rail_mode;
        let persist = self.persist;
        let themes: Vec<AnyElement> = ThemeChoice::ALL
            .into_iter()
            .map(|choice| {
                let title = match choice {
                    ThemeChoice::System => c::APPEARANCE_THEME_SYSTEM,
                    ThemeChoice::Light => c::APPEARANCE_THEME_LIGHT,
                    ThemeChoice::Dark => c::APPEARANCE_THEME_DARK,
                };
                self.card(
                    format!("appearance-theme-{}", choice.slug()),
                    Self::theme_preview(choice),
                    title,
                    None,
                    choice == current_theme,
                    cx,
                )
                .on_click(move |_, window, cx| {
                    theme::set_theme_choice(choice);
                    if persist {
                        layout::save_theme(choice);
                    }
                    theme::apply_theme_choice(window, cx);
                })
                .into_any_element()
            })
            .collect();
        let rails: Vec<AnyElement> = RailMode::ALL
            .into_iter()
            .map(|mode| {
                let (title, help) = match mode {
                    RailMode::Work => (c::APPEARANCE_RAIL_WORK, c::APPEARANCE_RAIL_WORK_HELP),
                    RailMode::Attention => (
                        c::APPEARANCE_RAIL_ATTENTION,
                        c::APPEARANCE_RAIL_ATTENTION_HELP,
                    ),
                    RailMode::Member => (c::APPEARANCE_RAIL_MEMBER, c::APPEARANCE_RAIL_MEMBER_HELP),
                };
                let nav = self.services.nav.clone();
                self.card(
                    format!("appearance-rail-{}", mode.slug()),
                    Self::rail_preview(mode, cx),
                    title,
                    Some(help),
                    mode == current_rail,
                    cx,
                )
                .on_click(move |_, _, cx| {
                    nav.update(cx, |nav, cx| nav.set_rail_mode(mode, cx));
                    if persist {
                        layout::save_rail_mode(mode);
                    }
                })
                .into_any_element()
            })
            .collect();
        let grid =
            |cards: Vec<AnyElement>| div().flex().items_start().gap(space::S16).children(cards);
        div()
            .flex()
            .flex_col()
            .gap(space::S8)
            .child(section_label(c::APPEARANCE_THEME, None, cx))
            .child(grid(themes))
            .child(div().h(space::S24))
            .child(section_label(c::APPEARANCE_RAIL, None, cx))
            .child(grid(rails))
    }
}

/// The `settings-appearance` scene: the settings window on 外观.
pub fn open_scene(
    services: Services,
    _content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    super::open_window(services, Page::Appearance, false, cx).map(|(h, _)| h)
}
