//! The expand viewer (REFS §B4): a 560 sheet under the toolbar row (F52, §2.9). Fit on
//! open with a 12 px text floor (a larger diagram scrolls), zoom
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
/// Space around the diagram inside the canvas (review 3 C-02: fit the width
/// with 24 on each side).
const MARGIN: f32 = 24.;
/// The room 问这里 needs above a node: the 22 pill, the 8 gap and 8 clear of the header.
const ASK_PILL_ROOM: f32 = 38.;
/// The height of the 看原文 pane under the diagram.
const SOURCE_PANE: f32 = 200.;

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

/// The smallest zoom a fit picks: diagram text is 15–16 px at 100 % (the library's
/// sizes), and a fit never draws it under 12 px (a larger diagram scrolls instead; D2-37).
pub const FIT_FLOOR: f32 = 12. / 15.;

/// The zoom that fits the diagram's width in the viewport (24 on each side),
/// at most 100 % and at least [`FIT_FLOOR`]: a taller diagram scrolls down
/// from its top, a wider one at the floor scrolls sideways from its left.
pub fn fit_zoom(w: f32, viewport_w: f32) -> f32 {
    ((viewport_w - 2. * MARGIN).max(1.) / w.max(1.)).clamp(FIT_FLOOR, 1.0)
}

/// Writes the drawn picture to `<temp>/<slug>-diagrams/<name>.png`, for 存到本机 (copied
/// into 下载 from there) and 用系统应用打开.
fn export_png(image: &gpui::Image, name: &str) -> Option<std::path::PathBuf> {
    let dir = std::env::temp_dir().join(format!("{}-diagrams", env!("PRODUCT_SLUG")));
    std::fs::create_dir_all(&dir).ok()?;
    let safe: String = name
        .chars()
        .map(|ch| {
            if matches!(ch, '/' | ':' | '\\') {
                '-'
            } else {
                ch
            }
        })
        .collect();
    let path = dir.join(format!("{safe}.png"));
    std::fs::write(&path, image.bytes()).ok()?;
    Some(path)
}

/// The sheet height: the window under the toolbar row, 48 short, at most 760.
fn sheet_height(window: &Window) -> Pixels {
    (window.viewport_size().height - ui::native::metrics::TOOLBAR_ROW - space::S48)
        .min(px(760.))
        .max(px(320.))
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
    /// The height the body needs at the current zoom (the drawing, its margins and
    /// 看原文), once drawn: the sheet hugs it.
    content_h: Option<f32>,
    /// The node Tab reached (C-03): its ring and 问这里 show, ↩ asks about it.
    key_node: Option<usize>,
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
        // The sheet hugs a short diagram (no blank band under it, A1) and stops
        // at the window's height; a taller one scrolls.
        let h = match view.read(cx).content_h {
            Some(c) => sheet_height(window)
                .min(ui::native::metrics::TOOLBAR_ROW + px(c))
                .max(px(240.)),
            None => sheet_height(window),
        };
        ui::native::native_sheet(dialog, ui::native::SheetWidth::List, cx)
            .px(px(0.))
            .h(h)
            .child(div().h(h).w_full().child(view.clone()))
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
            content_h: None,
            key_node: None,
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
        let (vw, _) = self.viewport;
        if vw > 0. {
            self.zoom = fit_zoom(r.meta.width, vw);
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

    /// Tab and ⇧Tab step through the diagram's nodes before the header's buttons
    /// (C-03, E3): `false` when the step leaves the nodes, so focus moves on.
    fn step_node(&mut self, forward: bool, cx: &mut Context<Self>) -> bool {
        if !crate::views::refs::enabled(cx) {
            return false;
        }
        let Some(r) = self.ready(cx) else {
            return false;
        };
        let count = r.meta.nodes.len();
        let next = match (self.key_node, forward) {
            (None, true) if count > 0 => Some(0),
            (None, false) if count > 0 => Some(count - 1),
            (Some(i), true) if i + 1 < count => Some(i + 1),
            (Some(i), false) if i > 0 => Some(i - 1),
            _ => None,
        };
        self.key_node = next;
        if let Some(i) = next {
            self.reveal(&r.meta.nodes[i]);
        }
        cx.notify();
        next.is_some()
    }

    /// Scrolls so node `node` is on screen, with the canvas margin around it.
    fn reveal(&mut self, node: &super::cache::Node) {
        let (x, y, w, h) = hit_rect(node, self.zoom);
        let (x, y) = (x + MARGIN, y + MARGIN);
        let (vw, vh) = self.viewport;
        let o = self.scroll.offset();
        let (mut ox, mut oy) = (-f32::from(o.x), -f32::from(o.y));
        if x - MARGIN < ox {
            ox = x - MARGIN;
        } else if x + w + MARGIN > ox + vw {
            ox = x + w + MARGIN - vw;
        }
        if y - MARGIN < oy {
            oy = y - MARGIN;
        } else if y + h + MARGIN > oy + vh {
            oy = y + h + MARGIN - vh;
        }
        let max = self.scroll.max_offset();
        let ox = ox.clamp(0., f32::from(max.x));
        let oy = oy.clamp(0., f32::from(max.y));
        self.scroll.set_offset(Point::new(px(-ox), px(-oy)));
    }

    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        let cmd = crate::command_key(&k.modifiers);
        if k.key == "enter"
            && let Some(i) = self.key_node
            && let Some(r) = self.ready(cx)
            && let Some(node) = r.meta.nodes.get(i)
        {
            super::on_node_ref(self.spec.origin.clone(), &node.id, &node.label, window, cx);
            cx.stop_propagation();
            return;
        }
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
        let image = ready.map(|r| r.image.clone());
        let file_name = self
            .spec
            .title
            .clone()
            .unwrap_or_else(|| kind.word().to_string());
        let n = ui::native::native_colors(cx);
        div()
            .flex_none()
            .h(ui::native::metrics::TOOLBAR_ROW)
            .px(space::S16)
            .flex()
            .items_center()
            .gap(space::S8)
            .border_b_1()
            .border_color(n.separator)
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
                        ui::native::icon_button_bare(
                            "dv-out",
                            Sym::Minus,
                            ui::native::IconSize::Small,
                        )
                        .named(c::ZOOM_OUT)
                        .on_click(
                            cx.listener(|this, _, _, cx| this.set_zoom(zoom_out(this.zoom), cx)),
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
                        ui::native::icon_button_bare(
                            "dv-in",
                            Sym::Plus,
                            ui::native::IconSize::Small,
                        )
                        .named(c::ZOOM_IN)
                        .on_click(
                            cx.listener(|this, _, _, cx| this.set_zoom(zoom_in(this.zoom), cx)),
                        ),
                    ),
            )
            .child(
                ui::native::icon_button_bare("dv-fit", Sym::Scan, ui::native::IconSize::Small)
                    .named(c::FIT)
                    .on_click(cx.listener(|this, _, _, cx| this.fit(cx))),
            )
            .child(
                ui::native::icon_button_bare("dv-src", Sym::Code, ui::native::IconSize::Small)
                    .named(c::VIEW_SOURCE)
                    .selected(self.show_source)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_source = !this.show_source;
                        cx.notify();
                    })),
            )
            // The zoom group, a separator, then the fixed set of every viewer in
            // the media order: copy, download, open, close (F52, §2.2).
            .child(
                div()
                    .flex_none()
                    .mx(space::S4)
                    .w(ui::native::metrics::HAIRLINE)
                    .h(space::S16)
                    .bg(n.separator),
            )
            .child({
                let image = image.clone();
                ui::native::icon_button_bare("dv-img", Sym::Copy, ui::native::IconSize::Small)
                    .named(c::COPY_IMAGE)
                    .disabled(image.is_none())
                    .on_click(move |_, _, cx| {
                        if let Some(i) = &image {
                            cx.write_to_clipboard(ClipboardItem::new_image(i));
                        }
                    })
            })
            .child({
                let (image, name) = (image.clone(), file_name.clone());
                ui::native::icon_button_bare("dv-save", Sym::Download, ui::native::IconSize::Small)
                    .named(c::SAVE)
                    .disabled(image.is_none())
                    .on_click(move |_, _, cx| {
                        if let Some(path) = image.as_deref().and_then(|i| export_png(i, &name)) {
                            crate::views::media::parts::save_to_downloads(&[path.as_path()], cx);
                        }
                    })
            })
            .child({
                let name = file_name;
                ui::native::icon_button_bare(
                    "dv-open",
                    Sym::OpenExternal,
                    ui::native::IconSize::Small,
                )
                .named(c::OPEN_WITH_SYSTEM)
                .disabled(image.is_none())
                .on_click(move |_, _, cx| {
                    if let Some(path) = image.as_deref().and_then(|i| export_png(i, &name)) {
                        cx.open_with_system(&path);
                    }
                })
            })
            .child(
                ui::native::icon_button_bare("dv-close", Sym::Close, ui::native::IconSize::Small)
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
            let (_, _, _, oh) = super::view::outline_rect(node, z);
            let origin = self.spec.origin.clone();
            let (nid, label) = (node.id.clone(), node.label.clone());
            let keyed = self.key_node == Some(i);
            let hovered = self.hovered == Some(i) || keyed;
            let n = ui::native::native_colors(cx);
            let _ = &k;
            layer = layer.child(
                div()
                    .id(ElementId::Name(format!("dv-node-{i}").into()))
                    .absolute()
                    .left(px(x - 2.))
                    .top(px(y - 2.))
                    .w(px(nw + 4.))
                    .h(px(nh + 4.))
                    // E3: one 1 px outline around the node's box only.
                    .when(hovered, |el| {
                        el.child(
                            div()
                                .absolute()
                                .left_0()
                                .top_0()
                                .w(px(nw + 4.))
                                .h(px(oh + 4.))
                                .rounded(radius::MD)
                                .when(!keyed, |d| d.border_1().border_color(n.secondary))
                                // A ring line, not a shadow: a shadow under a clear box
                                // would fill the node (E3).
                                .when(keyed, |d| {
                                    d.child(crate::views::refs::keyboard::ring(radius::MD, cx))
                                }),
                        )
                    })
                    // --- inc-refs: 问这里 beside the hovered node.
                    .when(hovered && refs_on, |el| {
                        let (o, nid, label) = (
                            self.spec.origin.clone(),
                            node.id.clone(),
                            node.label.clone(),
                        );
                        // Above the node's box, clear of its edge and of the arrows
                        // beside it; under it when the canvas has no room above (a
                        // top-row node), so the header never cuts it (D2-28).
                        let below = MARGIN + y - 2. < ASK_PILL_ROOM;
                        el.child(
                            div()
                                .absolute()
                                .right_0()
                                // Under the head box, not under a participant's lifeline.
                                .when(below, |d| d.top(px(oh + 4.)).pt(space::S8))
                                .when(!below, |d| d.bottom_full().pb(space::S8))
                                .child(crate::views::refs::affordance::inline_button(
                                    &format!("dv-{i}"),
                                    std::rc::Rc::new(move |cx: &App| {
                                        super::node_target(&o, &nid, &label, cx)
                                            .unwrap_or_else(crate::views::refs::empty_target)
                                    }),
                                    cx,
                                )),
                        )
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
        // It starts at the top, 24 in; centered sideways only while it fits, else from
        // the left edge so nothing is cut at either side (C-02).
        // A definite size, so the canvas scrolls exactly to the drawing's edges.
        let fits = w + 2. * MARGIN <= self.viewport.0 + 0.5;
        div()
            .flex_none()
            .w(px((w + 2. * MARGIN).max(self.viewport.0)))
            .h(px(h + 2. * MARGIN))
            .flex()
            .when(fits, |d| d.justify_center())
            .child(div().p(px(MARGIN)).flex_none().child(layer))
            .into_any_element()
    }
}

impl Render for DiagramViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let ready = self.ready(cx);
        if !self.fitted
            && self.viewport.0 > 0.
            && let Some(r) = &ready
        {
            self.zoom = fit_zoom(r.meta.width, self.viewport.0);
            self.fitted = true;
        }
        let want = ready.as_ref().map(|r| {
            r.meta.height * self.zoom
                + 2. * MARGIN
                + if self.show_source { SOURCE_PANE } else { 0. }
        });
        if want != self.content_h {
            self.content_h = want;
            window.refresh();
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
            .on_action(cx.listener(|this, _: &crate::app::FocusNext, _, cx| {
                if !this.step_node(true, cx) {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &crate::app::FocusPrev, _, cx| {
                if !this.step_node(false, cx) {
                    cx.propagate();
                }
            }))
            .on_key_down(on_key)
            .child(toolbar)
            .child(
                // In the 560 sheet 看原文 opens under the diagram.
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .w_full()
                            .child(measure)
                            .child(
                                div()
                                    .id("diagram-viewer-canvas")
                                    .size_full()
                                    .overflow_scroll()
                                    .track_scroll(&self.scroll)
                                    .cursor_grab()
                                    .on_scroll(cx.listener(|this, e: &ScrollEvent, _, cx| {
                                        if crate::command_key(&e.modifiers) {
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
                            )
                            // Both scrollbars show while the diagram is larger than the
                            // sheet, so a cut edge always reads as "scrolls" (C-02).
                            .child(
                                div().absolute().inset_0().child(
                                    gpui_component::scroll::Scrollbar::new(&self.scroll)
                                        .mode(gpui_component::scroll::ScrollbarMode::Always),
                                ),
                            ),
                    )
                    .when(self.show_source, |el| {
                        let raw = self.spec.raw.clone();
                        el.child(
                            div()
                                .flex_none()
                                .h(px(SOURCE_PANE))
                                .w_full()
                                .border_t_1()
                                .border_color(k.line)
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .flex_none()
                                        .px(space::S16)
                                        .pt(space::S8)
                                        .flex()
                                        .items_center()
                                        .child(
                                            div()
                                                .flex_1()
                                                .t_meta()
                                                .text_color(k.ink_3)
                                                .child(c::VIEW_SOURCE),
                                        )
                                        .child(
                                            ui::native::icon_button_bare(
                                                "dv-copy",
                                                Sym::Copy,
                                                ui::native::IconSize::Small,
                                            )
                                            .named(c::COPY_SOURCE)
                                            .on_click(
                                                move |_, _, cx| {
                                                    cx.write_to_clipboard(
                                                        ClipboardItem::new_string(raw.clone()),
                                                    )
                                                },
                                            ),
                                        ),
                                )
                                .child(
                                    div()
                                        .id("diagram-viewer-source")
                                        .flex_1()
                                        .min_h_0()
                                        .w_full()
                                        .overflow_y_scroll()
                                        .px(space::S16)
                                        .pb(space::S12)
                                        .flex()
                                        .flex_col()
                                        .children(self.spec.raw.lines().enumerate().map(
                                            |(i, line)| {
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
                                            },
                                        )),
                                ),
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
        assert_eq!(fit_zoom(400., 1200.), 1.0);
        let z = fit_zoom(1200., 1224.);
        assert!((z - 1176. / 1200.).abs() < 1e-3, "{z}");
        assert_eq!(fit_zoom(2224., 1224.), FIT_FLOOR);
        // A huge diagram keeps 12 px text and scrolls.
        assert_eq!(fit_zoom(100_000., 1200.), FIT_FLOOR);
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
