//! The segmented control (NATIVE.md §5.5): a `fill` capsule track with the
//! selected segment raised. For 2–5 views of the same content; never
//! underline tabs.

use std::rc::Rc;

use gpui::{
    AnyElement, App, ElementId, IntoElement, ParentElement, RenderOnce, SharedString, Styled,
    Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _,
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
};

use super::tokens::{NativeType as _, metrics, native_colors};
use crate::ui::sf_symbols::{Sf, Weight, symbol};

/// One segment: a label or a symbol (symbols carry a name for VoiceOver).
#[derive(Clone)]
pub enum Segment {
    Label(SharedString),
    Symbol(Sf, SharedString),
}

/// A segmented control. 24 pt; [`Segmented::toolbar`] makes it the 32 pt
/// toolbar size.
#[derive(IntoElement)]
pub struct Segmented {
    id: ElementId,
    segments: Vec<Segment>,
    selected: usize,
    height: gpui::Pixels,
    on_change: Option<Rc<dyn Fn(usize, &mut Window, &mut App)>>,
}

impl Segmented {
    pub fn new(id: impl Into<ElementId>, segments: impl IntoIterator<Item = Segment>) -> Self {
        Self {
            id: id.into(),
            segments: segments.into_iter().collect(),
            selected: 0,
            height: metrics::SEGMENTED,
            on_change: None,
        }
    }

    pub fn selected(mut self, ix: usize) -> Self {
        self.selected = ix;
        self
    }

    pub fn toolbar(mut self) -> Self {
        self.height = metrics::SEGMENTED_TOOLBAR;
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
        let inner = self.height - metrics::SEGMENT_INSET * 2.;
        let clear = gpui::transparent_black();
        let id = self.id.clone();
        let items: Vec<AnyElement> = self
            .segments
            .into_iter()
            .enumerate()
            .map(|(ix, seg)| {
                let selected = ix == self.selected;
                let content: AnyElement = match &seg {
                    Segment::Label(l) => div()
                        .n_body()
                        .text_color(k.label)
                        .child(l.clone())
                        .into_any_element(),
                    Segment::Symbol(sf, _) => {
                        symbol(*sf, px(15.), Weight::Regular, k.label, window)
                    }
                };
                let on_change = self.on_change.clone();
                Button::new(ElementId::NamedInteger(
                    format!("{id:?}-seg").into(),
                    ix as u64,
                ))
                .custom(
                    ButtonCustomVariant::new(cx)
                        .color(clear)
                        .foreground(k.label)
                        .hover(if selected { k.segment } else { clear })
                        .active(if selected { k.segment } else { k.fill_hover }),
                )
                .with_size(gpui_component::Size::Small)
                .h(inner)
                .px(metrics::SEGMENT_PAD)
                .rounded(inner / 2.)
                .when(selected, |b| b.bg(k.segment).shadow(k.segment()))
                .when_some(
                    match &seg {
                        Segment::Symbol(_, name) => Some(name.clone()),
                        Segment::Label(_) => None,
                    },
                    |b, name| b.accessibility_label(name.clone()).tooltip(name),
                )
                .child(content)
                .when_some(on_change, |b, f| b.on_click(move |_, w, cx| f(ix, w, cx)))
                .into_any_element()
            })
            .collect();
        div()
            .flex_none()
            .h(self.height)
            .p(metrics::SEGMENT_INSET)
            .rounded(self.height / 2.)
            .bg(k.fill)
            .flex()
            .items_center()
            .children(items)
    }
}
