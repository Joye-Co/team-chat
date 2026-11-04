//! The inspector (NATIVE.md §2, §9.3) and plain lists: inspector sections
//! with a disclosure header and label/value rows; list rows with the
//! native selection (accent when focused, gray when not or when the window
//! is inactive, no hover); the determinate bar and the spinner (§5.7).

use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, ElementId, Hsla, InteractiveElement as _, IntoElement,
    ParentElement, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};

use super::tokens::{NativeType as _, emphasized, metrics, native_colors};
use crate::ui::sf_symbols::{Sf, Weight, symbol};

type Click = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// The inspector column (§2): 280 wide, edge to edge, `sidebar` fill, a
/// 1 px `separator` on its leading edge.
pub fn inspector_column(cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .flex_none()
        .w(metrics::INSPECTOR_WIDTH)
        .h_full()
        .bg(k.sidebar)
        .border_l_1()
        .border_color(k.separator)
        .flex()
        .flex_col()
}

/// An inspector section (§9.3): a Section-style header with a disclosure
/// chevron and an optional trailing action, then its rows.
#[derive(IntoElement)]
pub struct InspectorSection {
    id: ElementId,
    title: SharedString,
    collapsed: bool,
    action: Option<AnyElement>,
    rows: Vec<AnyElement>,
    on_toggle: Option<Click>,
}

impl InspectorSection {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            collapsed: false,
            action: None,
            rows: Vec::new(),
            on_toggle: None,
        }
    }

    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }

    /// A small push button or a borderless symbol button at the trailing
    /// edge of the header.
    pub fn action(mut self, el: impl IntoElement) -> Self {
        self.action = Some(el.into_any_element());
        self
    }

    pub fn row(mut self, el: impl IntoElement) -> Self {
        self.rows.push(el.into_any_element());
        self
    }

    pub fn on_toggle(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for InspectorSection {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        let chevron = if self.collapsed {
            Sf::ChevronRight
        } else {
            Sf::ChevronDown
        };
        // COMPLETENESS.md §2.5: px 16, sections 24 apart; the GroupLabel h32
        // (12 medium muted) with an optional 28 ghost action.
        div()
            .w_full()
            .px(metrics::INSPECTOR_PAD)
            .pt(super::tokens::space::S16)
            .pb(super::tokens::space::S8)
            .flex()
            .flex_col()
            .child(
                div()
                    .id(self.id)
                    .h(super::tokens::height::ROW)
                    .flex()
                    .items_center()
                    .gap(super::tokens::space::S4)
                    .child(symbol(
                        chevron,
                        super::tokens::icon::SM,
                        Weight::Semibold,
                        k.secondary,
                        window,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .n_section()
                            .text_color(k.secondary)
                            .child(self.title),
                    )
                    .children(self.action)
                    .when_some(self.on_toggle, |d, f| {
                        d.on_click(move |e, w, cx| f(e, w, cx))
                    }),
            )
            .when(!self.collapsed, |d| d.children(self.rows))
    }
}

/// A label/value row (§9.3): label `secondary` in a fixed leading column
/// ([`metrics::LABEL_COLUMN`], cut at the tail), at least
/// [`metrics::LABEL_GAP`] before the value; the value `label` at the
/// trailing edge, one line, cut at the tail. Body 13, 24 pt. A long value
/// never runs over its label.
pub fn label_value(label: impl Into<SharedString>, value: impl IntoElement, cx: &App) -> gpui::Div {
    label_value_with(label, value, metrics::LABEL_COLUMN, cx)
}

/// [`label_value`] with a label column `column` wide (a form whose labels
/// are longer than the inspector's).
pub fn label_value_with(
    label: impl Into<SharedString>,
    value: impl IntoElement,
    column: gpui::Pixels,
    cx: &App,
) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .w_full()
        .min_w(px(0.))
        .min_h(metrics::INSPECTOR_ROW)
        .flex()
        .items_center()
        .gap(metrics::LABEL_GAP)
        .child(
            div()
                .flex_none()
                .w(column)
                .n_body()
                .text_color(k.secondary)
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(label.into()),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden()
                .flex()
                // §2.5: values left-aligned at one x.
                .justify_start()
                .n_body()
                .n_figures()
                .text_color(k.label)
                .whitespace_nowrap()
                .text_ellipsis()
                .child(
                    div()
                        .min_w(px(0.))
                        .max_w_full()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(value),
                ),
        )
}

/// A list row (Mail, Finder): 28 pt by default, radius 6 inset 8 pt.
/// Selection: `accent` fill with `onAccent` content when the list has focus
/// in the key window, `selection` gray when it does not or the window is
/// inactive. No hover.
#[derive(IntoElement)]
pub struct CompactRow {
    id: ElementId,
    selected: bool,
    focused: bool,
    inactive: Option<bool>,
    height: gpui::Pixels,
    children: Vec<AnyElement>,
    on_click: Option<Click>,
}

impl CompactRow {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            selected: false,
            focused: true,
            inactive: None,
            height: metrics::LIST_ROW,
            children: Vec::new(),
            on_click: None,
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Whether the list holds keyboard focus (default `true`).
    pub fn list_focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn inactive(mut self, inactive: bool) -> Self {
        self.inactive = Some(inactive);
        self
    }

    pub fn height(mut self, h: gpui::Pixels) -> Self {
        self.height = h;
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }

    /// The (fill, text) of this row's selection, for callers that color
    /// their own children: `None` when not selected.
    pub fn selection_ink(
        selected: bool,
        focused: bool,
        active: bool,
        cx: &App,
    ) -> Option<(Hsla, Hsla)> {
        // D10: the selected row is the `muted` fill with foreground text,
        // never an inverted black row, focused or not.
        let _ = (focused, active);
        let k = native_colors(cx);
        selected.then_some((k.selection, k.label))
    }
}

impl ParentElement for CompactRow {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for CompactRow {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        let active = self
            .inactive
            .map(|i| !i)
            .unwrap_or_else(|| emphasized(window));
        let sel = CompactRow::selection_ink(self.selected, self.focused, active, cx);
        div()
            .id(self.id)
            .flex_none()
            .h(self.height)
            .mx(metrics::LIST_ROW_INSET)
            .px(px(8.))
            .rounded(metrics::LIST_ROW_RADIUS)
            .flex()
            .items_center()
            .gap(px(8.))
            .n_body()
            .text_color(sel.map(|s| s.1).unwrap_or(k.label))
            .when_some(sel, |d, (fill, _)| {
                d.bg(fill).font_weight(crate::theme::MEDIUM_WEIGHT)
            })
            .children(self.children)
            .when_some(self.on_click, |d, f| {
                d.on_click(move |e, w, cx| f(e, w, cx))
            })
    }
}

/// The one Progress (COMPLETENESS.md D12): h4, radius full, track primary at
/// 20 %, value primary. `value` is 0–1. A state color never fills a bar
/// (§1.3): `_color` is kept for its callers and ignored.
pub fn determinate_bar(value: f32, _color: Option<Hsla>, cx: &App) -> gpui::Div {
    super::feedback::progress(value, cx)
}

/// A plan of `done` out of `total` steps: the Progress bar (D12).
pub fn steps_bar(done: usize, total: usize, _color: Option<Hsla>, cx: &App) -> gpui::Div {
    let value = if total == 0 {
        0.
    } else {
        done as f32 / total as f32
    };
    super::feedback::progress(value, cx)
}
