//! Part affordances (REFS §A3): after the pointer rests 300 ms on a part, a small 问这里
//! button shows at its top-right edge; ⌥-click adds the chip at once (also on controls); while
//! ⌥ is held the part shows a 1 pt outline; right-click opens 就这里提问 / 单独问这里 / 复制.
//!
//! A renderer wraps each referenceable part in [`part`] with a [`PartSpec`]. Nothing shows
//! when the server does not take references.

use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, ClipboardItem, Div, ElementId, InteractiveElement as _,
    IntoElement, ParentElement as _, Stateful, StatefulInteractiveElement as _, Styled as _,
    Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _,
    button::{Button, ButtonVariants as _},
    menu::{ContextMenuExt as _, PopupMenuItem},
};

use super::{PartTarget, RefIntent, RefsHub, ask_about, label};
use crate::{
    copy::refs as c,
    theme::{self, radius},
    ui::{self, Lucide},
};

/// Where the 问这里 button sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// Inside the part, at its top-right corner (metric, image, a step, a node).
    TopRight,
    /// At the end of a row, centered on its height (a table row, a diff line).
    RowEnd,
    /// Inside the part, in its top-right corner (a metric: next to its label).
    Corner,
}

/// One referenceable part.
#[derive(Clone)]
pub struct PartSpec {
    /// Unique among the visible parts (the message id, the block and the part).
    pub id: String,
    /// Builds the reference when it is used (a label and a value snapshot are not free).
    pub target: Rc<dyn Fn(&App) -> PartTarget>,
    /// A control (a choice row of a pending ask, an input, a button): no hover button, so
    /// answering stays the plain click; ⌥-click and the menu still work.
    pub control: bool,
    pub anchor: Anchor,
    /// The hover look covers the part (cells and rows); off for parts that draw their own
    /// hover look (a chart point's crosshair): no fill and no ring.
    pub fill: bool,
    pub radius: gpui::Pixels,
    /// The renderer draws 问这里 itself (after a step's text, in a chart's tooltip) with
    /// [`hover_state`] and [`inline_button`]; the part keeps the hover look and the rest.
    pub own_button: bool,
}

impl PartSpec {
    pub fn new(id: impl Into<String>, target: impl Fn(&App) -> PartTarget + 'static) -> Self {
        PartSpec {
            id: id.into(),
            target: Rc::new(target),
            control: false,
            anchor: Anchor::TopRight,
            fill: true,
            radius: radius::SM,
            own_button: false,
        }
    }

    pub fn own_button(mut self) -> Self {
        self.own_button = true;
        self
    }

    pub fn anchor(mut self, anchor: Anchor) -> Self {
        self.anchor = anchor;
        self
    }

    pub fn control(mut self, control: bool) -> Self {
        self.control = control;
        self
    }

    pub fn radius(mut self, r: gpui::Pixels) -> Self {
        self.radius = r;
        self
    }

    pub fn no_fill(mut self) -> Self {
        self.fill = false;
        self
    }
}

fn hub_of(cx: &App) -> Option<gpui::Entity<RefsHub>> {
    cx.try_global::<super::HubGlobal>().map(|h| h.0.clone())
}

/// The ink 问这里 button (Refs.dc.html: ink fill, the quote glyph, 12 pt).
pub fn ask_button(id: impl Into<ElementId>, cx: &App) -> Button {
    let _ = cx;
    Button::new(id)
        .primary()
        .compact()
        .with_size(gpui_component::Size::XSmall)
        .icon(ui::icon(
            Lucide::TextQuote,
            theme::icon::TAG,
            theme::colors(cx).bg,
        ))
        .label(c::ASK_HERE)
        .rounded(radius::SM)
}

/// Starts the 300 ms hover timer of `id`, or clears it.
fn on_hover(id: &str, on: bool, window: &mut Window, cx: &mut App) {
    let h = super::hub(cx);
    let id = id.to_string();
    let changed = h.update(cx, |h, _| {
        if on {
            if h.hover.as_ref().is_none_or(|(x, _)| *x != id) {
                h.hover = Some((id.clone(), std::time::Instant::now()));
                return true;
            }
            false
        } else if h.hover.as_ref().is_some_and(|(x, _)| *x == id) {
            h.hover = None;
            true
        } else {
            false
        }
    });
    if !changed {
        return;
    }
    window.refresh();
    if on {
        let handle = window.window_handle();
        cx.spawn(async move |cx| {
            cx.background_executor().timer(super::HOVER_DELAY).await;
            let _ = cx.update_window(handle, |_, window, _| window.refresh());
        })
        .detach();
    }
}

/// The context menu items of a part.
fn menu(
    menu: gpui_component::menu::PopupMenu,
    target: Rc<dyn Fn(&App) -> PartTarget>,
) -> gpui_component::menu::PopupMenu {
    let (t1, t2, t3) = (target.clone(), target.clone(), target);
    menu.item(
        PopupMenuItem::new(c::ASK_IN_TOPIC)
            .on_click(move |_, window, cx| ask_about(t1(cx), RefIntent::Topic, window, cx)),
    )
    .item(
        PopupMenuItem::new(c::ASK_ALONE)
            .on_click(move |_, window, cx| ask_about(t2(cx), RefIntent::SideChat, window, cx)),
    )
    .separator()
    .item(PopupMenuItem::new(c::COPY).on_click(move |_, _, cx| {
        let t = t3(cx);
        let display = t
            .value
            .as_ref()
            .and_then(|v| v.get("display"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| {
                let lead = label::lead_of(&t.label);
                label::split(&t.label, lead).1.to_string()
            });
        cx.write_to_clipboard(ClipboardItem::new_string(display));
    }))
}

/// Wraps `el` (the part) with its affordances. Returns `el` unchanged when the server does not
/// take references.
pub fn part(el: Stateful<Div>, spec: PartSpec, window: &Window, cx: &App) -> AnyElement {
    if !super::enabled(cx) {
        return el.into_any_element();
    }
    let k = theme::colors(cx);
    let hub = hub_of(cx);
    let (hovered, shown, flashing) = match &hub {
        Some(h) => {
            let h = h.read(cx);
            let flashing = h.flash.is_some()
                && !cx.reduce_motion()
                && h.flashing(&(spec.target)(cx).address());
            (h.hovered(&spec.id), h.hover_shown(&spec.id), flashing)
        }
        None => (false, false, false),
    };
    let alt = window.modifiers().alt;
    let show_button = shown && !spec.control;
    let ring = (spec.fill && hovered && (alt || show_button)).then_some(k.line_2);
    let id = spec.id.clone();
    let click_target = spec.target.clone();
    let menu_target = spec.target.clone();
    let button_target = spec.target.clone();
    let el = el
        .relative()
        .rounded(spec.radius)
        .when(flashing, |el| el.bg(k.sel))
        .when(!flashing && show_button && spec.fill, |el| el.bg(k.hover))
        .when_some(ring, |el, color| el.border_1().border_color(color))
        .on_hover({
            let id = id.clone();
            move |on, window, cx| on_hover(&id, *on, window, cx)
        })
        .on_click(move |e, window, cx| {
            if e.modifiers().alt {
                cx.stop_propagation();
                ask_about(click_target(cx), RefIntent::Topic, window, cx);
            }
        })
        .when(show_button && !spec.own_button, |el| {
            let button = ask_button(ElementId::Name(format!("ask-{id}").into()), cx).on_click(
                move |_, window, cx| {
                    cx.stop_propagation();
                    ask_about(button_target(cx), RefIntent::Topic, window, cx);
                },
            );
            let holder = div().absolute().flex().items_center();
            let holder = match spec.anchor {
                Anchor::TopRight => holder.top(px(-11.)).right(px(-6.)),
                Anchor::RowEnd => holder.top_0().bottom_0().right(px(8.)),
                Anchor::Corner => holder.top(px(4.)).right(px(4.)),
            };
            el.child(holder.child(button))
        });
    el.context_menu(move |m, _, _| menu(m, menu_target.clone()))
        .into_any_element()
}

/// The 问这里 button by itself, for renderers that place it (a chart tooltip, a table row's
/// name cell, a diagram node).
pub fn inline_button(id: &str, target: Rc<dyn Fn(&App) -> PartTarget>, cx: &App) -> AnyElement {
    ask_button(ElementId::Name(format!("ask-{id}").into()), cx)
        .on_click(move |_, window, cx| {
            cx.stop_propagation();
            ask_about(target(cx), RefIntent::Topic, window, cx);
        })
        .into_any_element()
}

/// The hover state of `id` for renderers that draw the button themselves.
pub fn hover_state(id: &str, cx: &App) -> (bool, bool) {
    hub_of(cx).map_or((false, false), |h| {
        let h = h.read(cx);
        (h.hovered(id), h.hover_shown(id))
    })
}

/// Hover tracking for renderers that draw the button themselves.
pub fn track_hover(id: String) -> impl Fn(&bool, &mut Window, &mut App) + 'static {
    move |on, window, cx| on_hover(&id, *on, window, cx)
}

/// A card header with 就这张卡提问 in its context menu (the whole card, REFS §A3).
pub fn whole_card(header: Div, target: Option<PartTarget>, cx: &App) -> AnyElement {
    let Some(target) = target.filter(|_| super::enabled(cx)) else {
        return header.into_any_element();
    };
    let id = ElementId::Name(format!("card-head-{}", target.address()).into());
    let (t1, t2) = (target.clone(), target);
    header
        .id(id)
        .context_menu(move |m, _, _| {
            let (a, b) = (t1.clone(), t2.clone());
            m.item(
                PopupMenuItem::new(c::ASK_CARD).on_click(move |_, window, cx| {
                    ask_about(a.clone(), RefIntent::Topic, window, cx)
                }),
            )
            .item(
                PopupMenuItem::new(c::ASK_ALONE).on_click(move |_, window, cx| {
                    ask_about(b.clone(), RefIntent::SideChat, window, cx)
                }),
            )
        })
        .into_any_element()
}
