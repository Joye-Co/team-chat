//! The segmented control (NATIVE.md §5.5): a `fill` capsule track with the
//! selected segment raised. For 2–5 views of the same content; never
//! underline tabs.

use std::rc::Rc;

use gpui::{
    AnyElement, App, ElementId, IntoElement, ParentElement, Pixels, RenderOnce, ScrollHandle,
    SharedString, Styled, Window, div, point, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _,
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
};

use super::tokens::{NativeType as _, height, icon, metrics, native_colors, radius, shadow, space};
use crate::ui::sf_symbols::{Sf, Weight, symbol};

/// One segment: a label or a symbol (symbols carry a name for VoiceOver).
#[derive(Clone)]
pub enum Segment {
    Label(SharedString),
    Symbol(Sf, SharedString),
    /// A label with a count pill after it.
    Counted(SharedString, u32),
}

/// The one tabs control (COMPLETENESS.md §1.8, K09): a list h32, p 3,
/// radius 10, `segment_track` fill; triggers h26, px 8, 14 medium, inactive at 60 %
/// foreground, the active one on `background` with shadow sm.
#[derive(IntoElement)]
pub struct Segmented {
    id: ElementId,
    segments: Vec<Segment>,
    selected: usize,
    disabled: bool,
    reveal: Option<(ScrollHandle, Pixels)>,
    on_change: Option<Rc<dyn Fn(usize, &mut Window, &mut App)>>,
}

impl Segmented {
    pub fn new(id: impl Into<ElementId>, segments: impl IntoIterator<Item = Segment>) -> Self {
        Self {
            id: id.into(),
            segments: segments.into_iter().collect(),
            selected: 0,
            disabled: false,
            reveal: None,
            on_change: None,
        }
    }

    /// In a strip that scrolls sideways: each newly selected segment comes into `scroll`'s
    /// view, `edge` clear of its right end (where the strip fades). Scrolling by hand stays
    /// where it is put until the selection changes.
    pub fn reveal_in(mut self, scroll: &ScrollHandle, edge: Pixels) -> Self {
        self.reveal = Some((scroll.clone(), edge));
        self
    }

    pub fn selected(mut self, ix: usize) -> Self {
        self.selected = ix;
        self
    }

    /// Kept for its callers: every segmented control is h32 (§1.8).
    pub fn toolbar(self) -> Self {
        self
    }

    /// No hover or press; the value stays readable.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_change(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Segmented {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        let clear = gpui::transparent_black();
        let id = self.id.clone();
        let disabled = self.disabled;
        let items: Vec<AnyElement> = self
            .segments
            .into_iter()
            .enumerate()
            .map(|(ix, seg)| {
                let selected = ix == self.selected;
                let ink = if selected {
                    k.label
                } else {
                    k.label.opacity(0.6)
                };
                let content: AnyElement = match &seg {
                    Segment::Label(l) => div()
                        .n_headline()
                        .text_color(ink)
                        .whitespace_nowrap()
                        .child(l.clone())
                        .into_any_element(),
                    Segment::Counted(l, n) => div()
                        .flex()
                        .items_center()
                        .gap(space::S6)
                        .n_headline()
                        .text_color(ink)
                        .whitespace_nowrap()
                        .child(l.clone())
                        .child(super::badge::CountPill::new(*n).on_fill(!selected))
                        .into_any_element(),
                    Segment::Symbol(sf, _) => symbol(*sf, icon::MD, Weight::Regular, ink, window),
                };
                let on_change = self.on_change.clone();
                let rest = if selected { k.content } else { clear };
                Button::new(ElementId::NamedInteger(
                    format!("{id:?}-seg").into(),
                    ix as u64,
                ))
                .custom(
                    ButtonCustomVariant::new(cx)
                        .color(rest)
                        .foreground(ink)
                        .hover(if selected || disabled {
                            rest
                        } else {
                            k.content.opacity(0.5)
                        })
                        .active(rest),
                )
                .with_size(gpui_component::Size::Medium)
                .h(height::SEGMENT_TRIGGER)
                .px(metrics::SEGMENT_PAD)
                .rounded(radius::ITEM)
                .bg(rest)
                .when(selected, |b| b.shadow(shadow::sm()))
                .when_some(
                    match &seg {
                        Segment::Symbol(_, name) => Some(name.clone()),
                        _ => None,
                    },
                    |b, name| b.accessibility_label(name.clone()).tooltip(name),
                )
                .child(content)
                .when(!disabled, |b| {
                    b.when_some(on_change, |b, f| b.on_click(move |_, w, cx| f(ix, w, cx)))
                })
                .into_any_element()
            })
            .collect();
        let selected = self.selected;
        let reveal = self.reveal.map(|(scroll, edge)| {
            let shown = window.use_keyed_state(
                ElementId::Name(format!("{id:?}-revealed").into()),
                cx,
                |_, _| None::<usize>,
            );
            move |items: Vec<gpui::Bounds<Pixels>>, window: &mut Window, cx: &mut App| {
                let max = scroll.max_offset().x;
                // Everything fits, or the strip is not measured yet.
                if max <= px(0.) || *shown.read(cx) == Some(selected) {
                    return;
                }
                let Some(item) = items.get(selected) else {
                    return;
                };
                shown.update(cx, |s, _| *s = Some(selected));
                let view = scroll.bounds();
                let over = item.right() - (view.right() - edge);
                let under = view.left() - item.left();
                let shift = if under > px(0.) {
                    under
                } else if over > px(0.) {
                    -over
                } else {
                    return;
                };
                let offset = scroll.offset();
                let x = (offset.x + shift).min(px(0.)).max(-max);
                scroll.set_offset(point(x, offset.y));
                window.refresh();
            }
        });
        div()
            .when_some(reveal, |d, reveal| d.on_children_prepainted(reveal))
            .flex_none()
            .h(metrics::SEGMENTED)
            .p(metrics::SEGMENT_INSET)
            .rounded(radius::CONTAINER)
            .bg(k.segment_track)
            // Disabled: 50 % opacity (§1.8), so a value that cannot be
            // changed does not look like a live control.
            .when(disabled, |d| d.opacity(metrics::DISABLED_OPACITY))
            .flex()
            .items_center()
            .children(items)
    }
}
