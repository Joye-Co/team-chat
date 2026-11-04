//! The `sf-symbols` scene: Lucide (today) next to SF Symbols (Regular and
//! Medium) in the title bar's right group, rail rows, settings tiles, and all
//! 40 symbols in outlined 16 pt slots. While it waits to be captured it
//! switches the theme twice and records the first frame after each switch.

use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{
    AnyElement, AnyWindowHandle, App, AppContext as _, Context, FontWeight, Hsla, IntoElement,
    ParentElement, Pixels, Render, Size, Styled, Window, WindowBounds, div, linear_color_stop,
    linear_gradient, prelude::FluentBuilder as _, px, rgb,
};
use gpui_component::ThemeMode;

use super::{Sf, Weight, gates, symbol, take_stats};
use crate::{
    app::{Services, main_window_options},
    copy::{dev, rail, settings, topic_rail, update},
    selftest::scenes::{Scene, Start, capture_bounds},
    theme::{self, Type as _},
    ui::icons,
};

/// The scene's slice of the self-test (`selftest::scenes::all`).
pub const SCENES: &[Scene] = &[Scene {
    name: "sf-symbols",
    about: "SF Symbols spike: Lucide next to SF Symbols in title bar buttons, rail rows and settings tiles; writes the gate numbers",
    start: Start::SignedIn,
    setup: |_, _| {},
    open,
}];

static DONE: AtomicBool = AtomicBool::new(false);

/// True once the scene switched the theme there and back (`ready_check`).
pub fn ready(_cx: &App) -> bool {
    DONE.load(Ordering::SeqCst)
}

/// Writes the gate report next to the capture (`verify_check`).
pub fn verify(png: &std::path::Path, cx: &mut App) -> Result<(), String> {
    gates::write(png, cx)?;
    let switches = gates::SWITCHES.lock().map(|s| s.clone()).unwrap_or_default();
    match switches.iter().find(|s| s.missing > 0 || s.drawn == 0 || !s.new_ink) {
        Some(bad) => Err(format!("theme switch drew a blank or old symbol: {bad:?}")),
        None => Ok(()),
    }
}

fn open(_services: Services, content: Size<Pixels>, cx: &mut App) -> Result<AnyWindowHandle, String> {
    // Cold numbers first: nothing in this process has drawn a symbol yet.
    gates::cold();
    DONE.store(false, Ordering::SeqCst);
    if let Ok(mut s) = gates::SWITCHES.lock() {
        s.clear();
    }
    let mut options = main_window_options(false, cx);
    options.window_bounds = Some(WindowBounds::Windowed(capture_bounds(content, cx)));
    options.window_min_size = None;
    gpui_kit::open_window(options, cx, |_, cx| cx.new(|_| Compare::default()))
        .map(|(handle, _)| handle)
        .map_err(|e| e.to_string())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Settling,
    /// The theme was switched; the next render is the first frame after it.
    Switched { back: bool },
    Done,
}

#[derive(Default)]
struct Compare {
    frames: u32,
    phase: Phase,
    /// The theme the runner asked for; the scene returns to it.
    start_dark: Option<bool>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Set {
    Lucide,
    Sf(Weight),
    /// What the app draws: the concept through `icons::icon` (GPUI `svg()`).
    App,
}

impl Set {
    fn glyph(self, sf: Sf, slot: f32, color: Hsla, window: &Window) -> AnyElement {
        match self {
            Set::Lucide => icons::lucide_raw(sf.lucide(), px(slot), color).into_any_element(),
            Set::Sf(w) => symbol(sf, px(slot), w, color, window),
            Set::App => icons::icon(sf.sym(), px(slot), color).into_any_element(),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Set::Lucide => dev::SF_SET_BEFORE,
            Set::App => dev::SF_SET_APP,
            Set::Sf(Weight::Medium) => dev::SF_SET_MEDIUM,
            Set::Sf(_) => dev::SF_SET_REGULAR,
        }
    }
}

const SETS: [Set; 3] = [Set::Lucide, Set::Sf(Weight::Regular), Set::App];
const COLUMN: Pixels = px(440.);

impl Compare {
    fn switch(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) {
        let start = self.start_dark.unwrap_or(false);
        let dark = if back { start } else { !start };
        window.on_next_frame({
            let entity = cx.entity();
            move |window, cx| {
                theme::set_mode(if dark { ThemeMode::Dark } else { ThemeMode::Light }, cx);
                // Only the switch frame counts.
                take_stats();
                entity.update(cx, |this, _| this.phase = Phase::Switched { back });
                window.refresh();
            }
        });
    }

    /// Runs after the elements of a frame are built.
    fn after_render(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.frames += 1;
        match self.phase {
            Phase::Settling => {
                take_stats();
                // The runner forces the theme after the window opens; three
                // frames later the scene starts from that theme.
                if self.frames >= 3 {
                    self.start_dark = Some(theme::is_dark(cx));
                    self.switch(false, window, cx);
                }
            }
            Phase::Switched { back } => {
                let stats = take_stats();
                let dark = theme::is_dark(cx);
                let start = self.start_dark.unwrap_or(false);
                let wanted = if back { start } else { !start };
                if let Ok(mut s) = gates::SWITCHES.lock() {
                    s.push(gates::Switch {
                        to_dark: dark,
                        drawn: stats.drawn,
                        hits: stats.hits,
                        recolors: stats.recolors,
                        masks: stats.masks,
                        missing: stats.missing,
                        busy_ms: stats.busy.as_secs_f64() * 1000.,
                        new_ink: dark == wanted,
                    });
                }
                if back {
                    self.phase = Phase::Done;
                    DONE.store(true, Ordering::SeqCst);
                } else {
                    self.switch(true, window, cx);
                }
            }
            Phase::Done => {
                take_stats();
            }
        }
    }
}

impl Render for Compare {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let columns: Vec<AnyElement> = SETS
            .iter()
            .map(|&set| column(set, &k, window).into_any_element())
            .collect();
        let cold = gates::cold();
        let summary = crate::copy::fill(
            dev::SF_COLD,
            &[
                ("lookup", &format!("{:.1}", cold.first_lookup_ms)),
                ("x1", &format!("{:.1}", cold.x1_ms)),
                ("x2", &format!("{:.1}", cold.x2_ms)),
                ("warm", &format!("{:.1}", cold.warm_ms)),
            ],
        );
        let root = div()
            .size_full()
            .bg(k.bg)
            .text_color(k.ink)
            .font_family(".SystemUIFont")
            .flex()
            .flex_col()
            .child(
                div()
                    .h(theme::TITLE_BAR)
                    .flex_none()
                    .pl(theme::TRAFFIC_LIGHTS_INSET)
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(div().t_title().child(dev::SF_TITLE))
                    .child(div().t_meta().text_color(k.ink_3).child(summary)),
            )
            .child(
                div()
                    .flex()
                    .gap(px(24.))
                    .px(px(24.))
                    .children(columns),
            );
        self.after_render(window, cx);
        root
    }
}

fn column(set: Set, k: &theme::Colors, window: &Window) -> impl IntoElement {
    div()
        .w(COLUMN)
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(12.))
        .child(div().t_meta_medium().text_color(k.ink_3).child(set.name()))
        .child(title_bar(set, k, window))
        .child(
            div()
                .flex()
                .gap(px(12.))
                .child(rail_rows(set, k, window))
                .child(tiles(set, k, window)),
        )
        .child(grid(set, k, window))
}

/// The right group of a title bar (§5): 28 x 28 buttons, glyph 16, gap 4,
/// a divider, then the toggled pane button.
fn title_bar(set: Set, k: &theme::Colors, window: &Window) -> impl IntoElement {
    let button = |sf: Sf, on: bool| {
        div()
            .size(theme::TITLE_BUTTON)
            .flex_none()
            .rounded(px(7.))
            .when(on, |d| d.bg(k.sel))
            .flex()
            .items_center()
            .justify_center()
            .child(set.glyph(sf, 16., if on { k.ink } else { k.ink_2 }, window))
    };
    div()
        .h(theme::TITLE_BAR)
        .w_full()
        .bg(k.bg)
        .border_b_1()
        .border_color(k.line)
        .flex()
        .items_center()
        .justify_between()
        .pl(px(20.))
        .pr(px(12.))
        .child(div().t_card_title().child(topic_rail::TITLE_OUTPUTS))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(button(Sf::Search, false))
                .child(button(Sf::SideChat, false))
                .child(button(Sf::Outputs, false))
                .child(button(Sf::Computer, false))
                .child(button(Sf::Changes, false))
                .child(button(Sf::More, false))
                .child(div().mx(px(4.)).w(px(1.)).h(px(16.)).bg(k.line))
                .child(button(Sf::PaneRight, true)),
        )
}

/// Rail rows (§2.3): leading slot 20–36, text at 44, rows 28.
fn rail_rows(set: Set, k: &theme::Colors, window: &Window) -> impl IntoElement {
    let row = |sf: Sf, label: &'static str, on: bool, trailing: Option<AnyElement>| {
        div()
            .h(theme::RAIL_ROW)
            .mx(px(10.))
            .px(px(10.))
            .rounded(px(6.))
            .when(on, |d| d.bg(k.rail_sel).font_weight(FontWeight::MEDIUM))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(set.glyph(sf, 16., if on { k.ink } else { k.ink_2 }, window))
            .child(div().flex_1().t_ui().text_color(k.ink).child(label))
            .children(trailing)
    };
    let field = div()
        .h(px(28.))
        .mx(px(10.))
        .px(px(8.))
        .rounded(px(7.))
        .bg(k.sunken)
        .flex()
        .items_center()
        .gap(px(6.))
        .child(set.glyph(Sf::Search, 14., k.ink_3, window))
        .child(div().t_ui().text_color(k.ink_3).child(rail::RAIL_SEARCH));
    div()
        .w(px(260.))
        .flex_none()
        .bg(k.rail)
        .rounded(px(8.))
        .py(px(10.))
        .flex()
        .flex_col()
        .child(field)
        .child(div().h(px(12.)))
        .child(row(Sf::Desk, rail::RAIL_DESK, true, None))
        .child(row(Sf::Outputs, rail::RAIL_ARTIFACTS, false, None))
        .child(row(Sf::Members, settings::PAGE_MEMBERS, false, None))
        .child(div().h(px(20.)))
        .child(
            div()
                .h(px(24.))
                .pl(px(20.))
                .flex()
                .items_center()
                .t_meta_medium()
                .text_color(k.ink_3)
                .child(rail::RAIL_TOPICS),
        )
        .child(row(Sf::Group, dev::SF_SAMPLE_GROUP_1, false, None))
        .child(row(
            Sf::Group,
            dev::SF_SAMPLE_GROUP_2,
            false,
            Some(set.glyph(Sf::ChevronRight, 12., k.ink_3, window)),
        ))
        .child(row(
            Sf::Done,
            dev::SF_SAMPLE_TOPIC_DONE,
            false,
            Some(set.glyph(Sf::Check, 12., k.ok, window)),
        ))
        .child(row(Sf::Alert, dev::SF_SAMPLE_TOPIC_YOU, false, None))
        .child(row(Sf::Settings, settings::TITLE, false, None))
}

/// Settings sidebar tiles (§7.2 option A): 20 graphite tiles, glyph 12 white.
fn tiles(set: Set, k: &theme::Colors, window: &Window) -> impl IntoElement {
    let (top, bottom) = if k.is_dark() {
        (rgb(0x6A6A72), rgb(0x4A4A52))
    } else {
        (rgb(0x5B5B63), rgb(0x3C3C43))
    };
    // Tiles take the stroke 2 Lucide glyph; SF draws them a step heavier.
    let glyph_set = match set {
        Set::Sf(Weight::Regular) => Set::Sf(Weight::Semibold),
        Set::Sf(Weight::Medium) => Set::Sf(Weight::Semibold),
        other => other,
    };
    let row = |sf: Sf, label: &'static str| {
        div()
            .h(px(28.))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .size(px(20.))
                    .flex_none()
                    .rounded(px(5.))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(top, 0.),
                        linear_color_stop(bottom, 1.),
                    ))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(glyph_set.glyph(sf, 12., gpui::white(), window)),
            )
            .child(div().t_ui().text_color(k.ink).child(label))
    };
    div()
        .flex_1()
        .bg(k.rail)
        .rounded(px(8.))
        .py(px(10.))
        .flex()
        .flex_col()
        .child(row(Sf::Bell, settings::PAGE_NOTIFICATIONS))
        .child(row(Sf::Members, settings::PAGE_MEMBERS))
        .child(row(Sf::Machines, settings::PAGE_MACHINES))
        .child(div().h(px(12.)))
        .child(row(Sf::Appearance, dev::SF_PAGE_APPEARANCE))
        .child(row(Sf::Shortcuts, dev::SF_PAGE_SHORTCUTS))
        .child(div().h(px(12.)))
        .child(row(Sf::Privacy, dev::SF_PAGE_PRIVACY))
        .child(row(Sf::About, update::ABOUT))
}

/// All 40 at 16 in a sunken square of the slot, so centering can be read;
/// then the 12 and 10 pt sizes (§7.1).
fn grid(set: Set, k: &theme::Colors, window: &Window) -> impl IntoElement {
    let cell = |sf: Sf, slot: f32| {
        div()
            .size(px(28.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .size(px(slot))
                    .bg(k.sunken)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(set.glyph(sf, slot, k.ink_2, window)),
            )
    };
    let small = [Sf::Check, Sf::Close, Sf::ChevronDown, Sf::ChevronRight, Sf::Plus];
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .flex_wrap()
                .w(px(28. * 14.))
                .children(Sf::ALL[..40].iter().map(|&s| cell(s, 16.))),
        )
        .child(
            div()
                .flex()
                .children(small.iter().map(|&s| cell(s, 12.)))
                .child(cell(Sf::Popup, 10.))
                .child(cell(Sf::ChevronDown, 10.)),
        )
}
