//! The expand viewer (REFS §B4): a dialog at 90 % of the window. Fit on open, zoom
//! 25 %–400 % by layout size (never a transform), ⌘= ⌘- ⌘0 and ⌘ + wheel, pan by scroll,
//! drag and arrow keys, 看原文 beside the image, 复制原文, 复制图片, Esc closes and the
//! focus goes back to the diagram.

use gpui::{
    AnyElement, App, AppContext as _, ClipboardItem, Context, ElementId, Entity, FocusHandle,
    ImageSource, InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, ParentElement as _, Pixels, Point, Render, ScrollEvent, ScrollHandle,
    StatefulInteractiveElement as _, Styled as _, Window, canvas, div, img,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{Disableable as _, Selectable as _, WindowExt as _};

use super::{
    DiagramSource, INLINE_SCALE, Ready, State, ZOOM_SCALE,
    normalize::{DiagramType, Normalized},
    view::hit_rect,
};
use crate::{
    copy::diagrams as c,
    theme::{self, Type as _, radius, space},
    ui::{self, NamedButton as _, Sym},
};

pub const MIN_ZOOM: f32 = 0.25;
pub const MAX_ZOOM: f32 = 4.0;
/// Zoom steps for the buttons and keys.
pub const STEPS: &[f32] = &[
    0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0,
];
/// Arrow keys move this far.
pub const ARROW_STEP: f32 = 40.;
/// Space around the diagram inside the canvas (page padding + card padding).
const MARGIN: f32 = 56.;
const SOURCE_PANE: f32 = 320.;

/// What the viewer shows.
#[derive(Clone)]
pub struct ViewerSpec {
    pub normalized: Normalized,
    pub raw: String,
    pub title: Option<String>,
    pub origin: DiagramSource,
    /// Focused again when the viewer closes.
    pub return_focus: Option<FocusHandle>,
}

/// The zoom that fits the diagram in the viewport, at most 100 %.
pub fn fit_zoom(w: f32, h: f32, viewport_w: f32, viewport_h: f32) -> f32 {
    let zw = (viewport_w - 2. * MARGIN).max(1.) / w.max(1.);
    let zh = (viewport_h - 2. * MARGIN).max(1.) / h.max(1.);
    zw.min(zh).clamp(MIN_ZOOM, 1.0)
}

/// The next step above `z`.
pub fn zoom_in(z: f32) -> f32 {
    STEPS
        .iter()
        .copied()
        .find(|s| *s > z + 0.001)
        .unwrap_or(MAX_ZOOM)
}

/// The next step below `z`.
pub fn zoom_out(z: f32) -> f32 {
    STEPS
        .iter()
        .rev()
        .copied()
        .find(|s| *s < z - 0.001)
        .unwrap_or(MIN_ZOOM)
}

/// The raster scale to ask for at a zoom: 4 above 200 %.
pub fn scale_for_zoom(z: f32) -> u32 {
    if z > 2.0 { ZOOM_SCALE } else { INLINE_SCALE }
}

pub struct DiagramViewer {
    spec: ViewerSpec,
    zoom: f32,
    fitted: bool,
    show_source: bool,
    scroll: ScrollHandle,
    drag: Option<(Point<Pixels>, Point<Pixels>)>,
    focus: FocusHandle,
    viewport: (f32, f32),
    hovered: Option<usize>,
}

// --- inc-refs: a scene opens the viewer with a node under the pointer (问这里 shows).
thread_local! {
    static PREVIEW_HOVER: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// The next viewer starts with node `index` hovered (screenshot scenes).
pub fn preview_hover_next(index: usize) {
    PREVIEW_HOVER.with(|h| h.set(Some(index)));
}
// --- end inc-refs

/// Opens the viewer over `window`.
pub fn open(spec: ViewerSpec, window: &mut Window, cx: &mut App) -> Entity<DiagramViewer> {
    let size = window.viewport_size();
    let (w, h) = (f32::from(size.width), f32::from(size.height));
    let view = cx.new(|cx| DiagramViewer::new(spec.clone(), cx));
    let handle = view.clone();
    let focus = view.read(cx).focus.clone();
    let back = spec.return_focus.clone();
    let first = std::cell::Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            focus.focus(window, cx);
        }
        let back = back.clone();
        dialog
            .w(px((w * 0.9).max(480.)))
            .margin_top(px(h * 0.05))
            .close_button(false)
            .p_0()
            .child(
                div()
                    .h(px((h * 0.9).max(360.)))
                    .w_full()
                    .child(view.clone()),
            )
            .on_close(move |_, window, cx| {
                if let Some(f) = &back {
                    f.focus(window, cx);
                }
            })
    });
    handle
}

impl DiagramViewer {
    pub fn new(spec: ViewerSpec, cx: &mut Context<Self>) -> Self {
        Self {
            spec,
            zoom: 1.0,
            fitted: false,
            show_source: false,
            scroll: ScrollHandle::new(),
            drag: None,
            focus: cx.focus_handle().tab_stop(true),
            viewport: (0., 0.),
            hovered: PREVIEW_HOVER.with(|h| h.take()),
        }
    }

    pub fn set_zoom(&mut self, zoom: f32, cx: &mut Context<Self>) {
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.fitted = true;
        cx.notify();
    }

    fn ready(&self, cx: &mut App) -> Option<Ready> {
        let want = scale_for_zoom(self.zoom);
        let n = &self.spec.normalized;
        match super::request(n, want, true, cx) {
            State::Ready(r) => Some(r),
            _ if want != INLINE_SCALE => match super::request(n, INLINE_SCALE, true, cx) {
                State::Ready(r) => Some(r),
                _ => None,
            },
            _ => None,
        }
    }

    fn fit(&mut self, cx: &mut Context<Self>) {
        let Some(r) = self.ready(cx) else { return };
        let (vw, vh) = self.viewport;
        if vw > 0. {
            self.zoom = fit_zoom(r.meta.width, r.meta.height, vw, vh);
            self.fitted = true;
            self.scroll.set_offset(Point::default());
            cx.notify();
        }
    }

    fn pan(&mut self, dx: f32, dy: f32) {
        let o = self.scroll.offset();
        let max = self.scroll.max_offset();
        let x = (f32::from(o.x) + dx).clamp(-f32::from(max.x), 0.);
        let y = (f32::from(o.y) + dy).clamp(-f32::from(max.y), 0.);
        self.scroll.set_offset(Point::new(px(x), px(y)));
    }

    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        let cmd = k.modifiers.platform;
        match k.key.as_str() {
            "=" | "+" if cmd => self.set_zoom(zoom_in(self.zoom), cx),
            "-" if cmd => self.set_zoom(zoom_out(self.zoom), cx),
            "0" if cmd => self.fit(cx),
            "left" => self.pan(ARROW_STEP, 0.),
            "right" => self.pan(-ARROW_STEP, 0.),
            "up" => self.pan(0., ARROW_STEP),
            "down" => self.pan(0., -ARROW_STEP),
            "escape" => {
                window.close_dialog(cx);
                return;
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn toolbar(&self, ready: Option<&Ready>, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let kind = self.spec.normalized.kind;
        let raw = self.spec.raw.clone();
        let image = ready.map(|r| r.image.clone());
        div()
            .h(px(52.))
            .px(space::S16)
            .flex()
            .items_center()
            .gap(space::S8)
            .child(ui::icon(Sym::Diagram, px(16.), k.ink_3))
            .child(
                div().t_bar_title().text_color(k.ink).child(
                    self.spec
                        .title
                        .clone()
                        .unwrap_or_else(|| kind.word().to_string()),
                ),
            )
            .when(self.spec.title.is_some(), |el| {
                el.child(
                    div()
                        .t_meta()
                        .text_color(k.ink_3)
                        .child(DiagramType::word(kind)),
                )
            })
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(28.))
                    .child(
                        ui::icon_button_sm("dv-out", Sym::Minus)
                            .named(c::ZOOM_OUT)
                            .on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.set_zoom(zoom_out(this.zoom), cx)
                                }),
                            ),
                    )
                    .child(
                        div()
                            .w(px(48.))
                            .t_mono()
                            .text_color(k.ink_2)
                            .flex()
                            .justify_center()
                            .child(crate::copy::fill(
                                c::ZOOM_PERCENT,
                                &[("n", &format!("{}", (self.zoom * 100.).round() as i32))],
                            )),
                    )
                    .child(
                        ui::icon_button_sm("dv-in", Sym::Plus)
                            .named(c::ZOOM_IN)
                            .on_click(
                                cx.listener(|this, _, _, cx| this.set_zoom(zoom_in(this.zoom), cx)),
                            ),
                    ),
            )
            .child(
                ui::icon_button_sm("dv-fit", Sym::Scan)
                    .named(c::FIT)
                    .on_click(cx.listener(|this, _, _, cx| this.fit(cx))),
            )
            .child(
                ui::icon_button_sm("dv-src", Sym::Code)
                    .named(c::VIEW_SOURCE)
                    .selected(self.show_source)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_source = !this.show_source;
                        cx.notify();
                    })),
            )
            .child(
                ui::icon_button_sm("dv-copy", Sym::Copy)
                    .named(c::COPY_SOURCE)
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(raw.clone()))
                    }),
            )
            .child(
                ui::icon_button_sm("dv-img", Sym::Image)
                    .named(c::COPY_IMAGE)
                    .disabled(image.is_none())
                    .on_click(move |_, _, cx| {
                        if let Some(i) = &image {
                            cx.write_to_clipboard(ClipboardItem::new_image(i));
                        }
                    }),
            )
            .child(
                ui::icon_button_sm("dv-close", Sym::Close)
                    .named(c::CLOSE)
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
    }

    fn canvas_layer(&self, ready: &Ready, cx: &mut Context<Self>) -> AnyElement {
        let k = theme::colors(cx);
        let z = self.zoom;
        let (w, h) = (ready.meta.width * z, ready.meta.height * z);
        let mut layer = div().relative().w(px(w)).h(px(h)).child(
            img(ImageSource::Image(ready.image.clone()))
                .w(px(w))
                .h(px(h)),
        );
        let refs_on = crate::views::refs::enabled(cx);
        for (i, node) in ready.meta.nodes.iter().enumerate() {
            let (x, y, nw, nh) = hit_rect(node, z);
            let origin = self.spec.origin.clone();
            let (nid, label) = (node.id.clone(), node.label.clone());
            let hovered = self.hovered == Some(i);
            layer = layer.child(
                div()
                    .id(ElementId::Name(format!("dv-node-{i}").into()))
                    .absolute()
                    .left(px(x - 3.))
                    .top(px(y - 3.))
                    .w(px(nw + 6.))
                    .h(px(nh + 6.))
                    .rounded(radius::MD)
                    .when(hovered, |el| el.border_1().border_color(k.line_2))
                    // --- inc-refs: 问这里 beside the hovered node.
                    .when(hovered && refs_on, |el| {
                        let (o, nid, label) = (
                            self.spec.origin.clone(),
                            node.id.clone(),
                            node.label.clone(),
                        );
                        el.child(div().absolute().top(px(-12.)).right(px(-8.)).child(
                            crate::views::refs::affordance::inline_button(
                                &format!("dv-{i}"),
                                std::rc::Rc::new(move |cx: &App| {
                                    super::node_target(&o, &nid, &label, cx)
                                        .unwrap_or_else(crate::views::refs::empty_target)
                                }),
                                cx,
                            ),
                        ))
                    })
                    // --- end inc-refs
                    .on_hover(cx.listener(move |this, on: &bool, _, cx| {
                        let next = if *on {
                            Some(i)
                        } else if this.hovered == Some(i) {
                            None
                        } else {
                            this.hovered
                        };
                        if next != this.hovered {
                            this.hovered = next;
                            cx.notify();
                        }
                    }))
                    .on_click(move |e, window, cx| {
                        if e.modifiers().alt {
                            super::on_node_ref(origin.clone(), &nid, &label, window, cx);
                        }
                    }),
            );
        }
        // One panel (Quick Look): the drawing sits on the panel itself, no card inside.
        div().p(px(32.)).flex_none().child(layer).into_any_element()
    }
}

impl Render for DiagramViewer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let ready = self.ready(cx);
        if !self.fitted
            && self.viewport.0 > 0.
            && let Some(r) = &ready
        {
            self.zoom = fit_zoom(
                r.meta.width,
                r.meta.height,
                self.viewport.0,
                self.viewport.1,
            );
            self.fitted = true;
        }
        let entity = cx.entity();
        let measure = canvas(
            move |bounds, window, cx| {
                let size = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                entity.update(cx, |this, cx| {
                    if (this.viewport.0 - size.0).abs() > 0.5
                        || (this.viewport.1 - size.1).abs() > 0.5
                    {
                        this.viewport = size;
                        cx.notify();
                        window.refresh();
                    }
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .left_0()
        .top_0()
        .size_full();
        let body: AnyElement = match &ready {
            Some(r) => self.canvas_layer(r, cx),
            None => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .t_small()
                .text_color(k.ink_3)
                .child(c::DRAWING)
                .into_any_element(),
        };
        let toolbar = self.toolbar(ready.as_ref(), cx).into_any_element();
        let on_key = cx.listener(|this, e: &KeyDownEvent, window, cx| this.on_key(e, window, cx));
        div()
            .id("diagram-viewer")
            .size_full()
            .flex()
            .flex_col()
            .rounded(radius::LG)
            .overflow_hidden()
            .bg(k.raised)
            .track_focus(&self.focus)
            .on_key_down(on_key)
            .child(toolbar)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(measure)
                            .child(
                                div()
                                    .id("diagram-viewer-canvas")
                                    .size_full()
                                    .overflow_scroll()
                                    .track_scroll(&self.scroll)
                                    .cursor_grab()
                                    .on_scroll(cx.listener(|this, e: &ScrollEvent, _, cx| {
                                        if e.modifiers.platform {
                                            let dy = f32::from(e.delta.pixel_delta(px(16.)).y);
                                            if dy > 0. {
                                                this.set_zoom(zoom_in(this.zoom), cx);
                                            } else if dy < 0. {
                                                this.set_zoom(zoom_out(this.zoom), cx);
                                            }
                                            cx.stop_propagation();
                                        }
                                    }))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, e: &MouseDownEvent, _, _| {
                                            this.drag = Some((e.position, this.scroll.offset()));
                                        }),
                                    )
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, _| this.drag = None),
                                    )
                                    .on_mouse_move(cx.listener(
                                        |this, e: &MouseMoveEvent, _, cx| {
                                            let Some((start, offset)) = this.drag else {
                                                return;
                                            };
                                            if e.pressed_button != Some(MouseButton::Left) {
                                                this.drag = None;
                                                return;
                                            }
                                            let max = this.scroll.max_offset();
                                            let x = f32::from(offset.x + (e.position.x - start.x))
                                                .clamp(-f32::from(max.x), 0.);
                                            let y = f32::from(offset.y + (e.position.y - start.y))
                                                .clamp(-f32::from(max.y), 0.);
                                            this.scroll.set_offset(Point::new(px(x), px(y)));
                                            cx.notify();
                                        },
                                    ))
                                    .child(body),
                            ),
                    )
                    .when(self.show_source, |el| {
                        el.child(
                            div()
                                .id("diagram-viewer-source")
                                .w(px(SOURCE_PANE))
                                .flex_none()
                                .h_full()
                                .border_l_1()
                                .border_color(k.line)
                                .overflow_y_scroll()
                                .p(space::S12)
                                .flex()
                                .flex_col()
                                .children(self.spec.raw.lines().enumerate().map(|(i, line)| {
                                    div()
                                        .flex()
                                        .gap(space::S12)
                                        .t_code()
                                        .text_size(px(12.))
                                        .line_height(px(20.))
                                        .child(
                                            div()
                                                .w(px(20.))
                                                .flex_none()
                                                .text_color(k.ink_3)
                                                .child(format!("{}", i + 1)),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_color(k.ink)
                                                .child(line.to_string()),
                                        )
                                })),
                        )
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_never_upscales_and_stays_in_range() {
        assert_eq!(fit_zoom(400., 300., 1200., 800.), 1.0);
        let z = fit_zoom(2224., 400., 1224., 800.);
        assert!((z - 0.5).abs() < 1e-3, "{z}");
        assert_eq!(fit_zoom(100_000., 100., 1200., 800.), MIN_ZOOM);
    }

    #[test]
    fn zoom_steps_and_raster_scale() {
        assert_eq!(zoom_in(1.0), 1.1);
        assert_eq!(zoom_out(1.0), 0.9);
        assert_eq!(zoom_in(4.0), MAX_ZOOM);
        assert_eq!(zoom_out(0.25), MIN_ZOOM);
        assert_eq!(zoom_in(0.3), 0.33);
        assert_eq!(scale_for_zoom(2.0), INLINE_SCALE);
        assert_eq!(scale_for_zoom(2.5), ZOOM_SCALE);
    }
}
