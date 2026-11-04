//! The inline diagram element (REFS §B4): a framed block with the type word and title, the
//! image at content width (never above 1×), the node hit map with a hover ring and
//! 问这里, keyboard focus over the nodes, 看原文 / 复制原文 / 放大, and the error frames
//! that keep the source visible.

use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, ClipboardItem, ElementId, FocusHandle,
    ImageSource, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, canvas, div, img,
    linear_color_stop, linear_gradient, prelude::FluentBuilder as _, px,
};
use gpui_component::{Selectable as _, Sizable as _, button::ButtonVariants as _};

use super::{
    DiagramSource, ErrorKind, Hint, INLINE_SCALE, Ready, State,
    cache::Node,
    normalize::{self, DiagramType, Normalized},
};
use crate::{
    copy::diagrams as c,
    theme::{self, Type as _, radius, space},
    ui::{self, NamedButton as _, Sym},
};

/// Largest inline height before the image scales down or is cut.
pub const MAX_INLINE_HEIGHT: f32 = 360.;
/// The smallest scale a tall diagram may shrink to before it is cut instead.
pub const MIN_FIT_SCALE: f32 = 0.6;
/// The box shown while a diagram renders.
pub const PENDING_HEIGHT: f32 = 160.;
/// Padding around the image inside the frame.
const PAD: f32 = 16.;
/// Lines of source shown before 展开.
pub const SOURCE_LINES: usize = 8;

/// One inline diagram.
#[derive(Clone, Debug)]
pub struct InlineSpec {
    /// Stable for the block (message id + fence offset, or card block id).
    pub id: SharedString,
    /// The source as written.
    pub source: String,
    pub title: Option<String>,
    pub caption: Option<String>,
    pub alt: Option<String>,
    pub origin: DiagramSource,
}

/// How the image fits the inline width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    /// Display px per CSS px of the diagram.
    pub scale: f32,
    pub w: f32,
    /// Height of the visible part.
    pub h: f32,
    /// The bottom is cut (fade and 展开).
    pub clipped: bool,
}

/// REFS §B4: content width, never above 1×; taller than 360 shrinks while the scale
/// stays ≥ 0.6, else the top 360 shows (unless expanded).
pub fn inline_fit(w: f32, h: f32, avail: f32, expanded: bool) -> Fit {
    let w = w.max(1.);
    let h = h.max(1.);
    let s0 = (avail.max(1.) / w).min(1.);
    if h * s0 <= MAX_INLINE_HEIGHT || expanded {
        return Fit {
            scale: s0,
            w: w * s0,
            h: h * s0,
            clipped: false,
        };
    }
    let s1 = MAX_INLINE_HEIGHT / h;
    if s1 >= MIN_FIT_SCALE {
        Fit {
            scale: s1,
            w: w * s1,
            h: MAX_INLINE_HEIGHT,
            clipped: false,
        }
    } else {
        Fit {
            scale: s0,
            w: w * s0,
            h: MAX_INLINE_HEIGHT,
            clipped: true,
        }
    }
}

/// A node's box at a display scale: `(x, y, w, h)` in display px.
pub fn hit_rect(node: &Node, scale: f32) -> (f32, f32, f32, f32) {
    (
        node.x * scale,
        node.y * scale,
        node.w * scale,
        node.h * scale,
    )
}

struct Ui {
    width: f32,
    show_source: bool,
    expanded: bool,
    source_expanded: bool,
    focus: FocusHandle,
    focused: Option<usize>,
    hovered: Option<usize>,
}

/// The inline element for a diagram.
pub fn inline(spec: InlineSpec, window: &mut Window, cx: &mut App) -> AnyElement {
    let ui = window.use_keyed_state(
        ElementId::Name(format!("diagram-ui-{}", spec.id).into()),
        cx,
        |_, cx| Ui {
            width: 560.,
            show_source: false,
            expanded: false,
            source_expanded: false,
            focus: cx.focus_handle().tab_stop(true),
            focused: None,
            hovered: None,
        },
    );
    let normalized = normalize::normalize(&spec.source);
    let (state, kind) = match &normalized {
        Ok(n) => (super::request(n, INLINE_SCALE, true, cx), Some(n.kind)),
        Err(r) => (State::Failed(ErrorKind::from_refusal(r), None), None),
    };
    let k = theme::colors(cx);
    let id = spec.id.clone();
    let measure = {
        let ui = ui.clone();
        canvas(
            move |bounds, window, cx| {
                let w = f32::from(bounds.size.width);
                if (ui.read(cx).width - w).abs() > 0.5 {
                    ui.update(cx, |s, _| s.width = w);
                    window.refresh();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .left_0()
        .top_0()
        .size_full()
    };
    let (show_source, width, focused_frame) = {
        let u = ui.read(cx);
        (u.show_source, u.width, u.focus.is_focused(window))
    };

    if let State::Failed(kind_err, hint) = &state {
        return error_frame(&spec, *kind_err, *hint, kind, &ui, window, cx);
    }

    let header = header(&spec, kind, &ui, normalized.as_ref().ok(), true, cx);
    let avail = (width - 2. * PAD).max(80.);
    let body: AnyElement = if show_source {
        source_listing(&id, &spec.source, &ui, cx)
    } else {
        match &state {
            State::Ready(ready) => image_with_hits(&spec, ready, avail, &ui, window, cx),
            _ => pending(&id, avail, cx),
        }
    };
    let alt = match (&state, kind) {
        (State::Ready(r), Some(kind)) => {
            let labels: Vec<String> = r.meta.nodes.iter().map(|n| n.label.clone()).collect();
            super::alt_text(
                spec.alt.as_deref(),
                spec.caption.as_deref(),
                spec.title.as_deref(),
                kind,
                &labels,
            )
        }
        _ => c::DRAWING.to_string(),
    };
    let focus = ui.read(cx).focus.clone();
    let key_ui = ui.clone();
    let key_spec = spec.clone();
    let key_norm = normalized.clone().ok();
    div()
        .id(ElementId::Name(format!("diagram-{id}").into()))
        .relative()
        .w_full()
        .max_w(px(theme::PAGE_MEASURE.into()))
        .flex()
        .flex_col()
        .rounded(radius::LG)
        .border_1()
        .border_color(if focused_frame { k.ink_3 } else { k.line })
        .bg(k.raised)
        .overflow_hidden()
        .aria_label(alt)
        .track_focus(&focus)
        .on_key_down(move |e: &KeyDownEvent, window, cx| {
            on_key(e, &key_spec, key_norm.as_ref(), &key_ui, window, cx)
        })
        .child(header)
        .child(
            div()
                .relative()
                .w_full()
                .child(measure)
                .p(px(PAD))
                .child(body),
        )
        .when_some(spec.caption.clone(), |el, caption| {
            el.child(
                div()
                    .px(px(PAD))
                    .pb(space::S12)
                    .t_meta()
                    .text_color(k.ink_2)
                    .child(caption),
            )
        })
        .into_any_element()
}

fn on_key(
    e: &KeyDownEvent,
    spec: &InlineSpec,
    normalized: Option<&Normalized>,
    ui: &gpui::Entity<Ui>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(n) = normalized else { return };
    let ready = match super::peek(n, INLINE_SCALE, cx) {
        Some(State::Ready(r)) => r,
        _ => return,
    };
    let count = ready.meta.nodes.len();
    let key = e.keystroke.key.as_str();
    let alt = e.keystroke.modifiers.alt;
    match key {
        "right" | "down" | "left" | "up" if count > 0 => {
            ui.update(cx, |s, cx| {
                let cur = s.focused;
                s.focused = Some(match (key, cur) {
                    ("right" | "down", None) => 0,
                    (_, None) => count - 1,
                    ("right" | "down", Some(i)) => (i + 1).min(count - 1),
                    (_, Some(i)) => i.saturating_sub(1),
                });
                cx.notify();
            });
            cx.stop_propagation();
        }
        "enter" if alt => {
            if let Some(i) = ui.read(cx).focused
                && let Some(node) = ready.meta.nodes.get(i)
            {
                let (origin, id, label) =
                    (spec.origin.clone(), node.id.clone(), node.label.clone());
                super::on_node_ref(origin, &id, &label, window, cx);
            }
            cx.stop_propagation();
        }
        "enter" => {
            open_viewer(spec, n, ui.read(cx).focus.clone(), window, cx);
            cx.stop_propagation();
        }
        "escape" => {
            ui.update(cx, |s, cx| {
                s.focused = None;
                cx.notify();
            });
        }
        _ => {}
    }
}

fn open_viewer(
    spec: &InlineSpec,
    n: &Normalized,
    return_focus: FocusHandle,
    window: &mut Window,
    cx: &mut App,
) {
    super::viewer::open(
        super::viewer::ViewerSpec {
            normalized: n.clone(),
            raw: spec.source.clone(),
            title: spec.title.clone(),
            origin: spec.origin.clone(),
            return_focus: Some(return_focus),
        },
        window,
        cx,
    );
}

fn header(
    spec: &InlineSpec,
    kind: Option<DiagramType>,
    ui: &gpui::Entity<Ui>,
    normalized: Option<&Normalized>,
    actions: bool,
    cx: &App,
) -> impl IntoElement {
    let k = theme::colors(cx);
    let id = spec.id.clone();
    let show_source = ui.read(cx).show_source;
    let toggle_ui = ui.clone();
    let source = spec.source.clone();
    let viewer = normalized
        .cloned()
        .map(|n| (spec.clone(), n, ui.read(cx).focus.clone()));
    div()
        .h(px(36.))
        .px(space::S12)
        .flex()
        .items_center()
        .gap(space::S6)
        .border_b_1()
        .border_color(k.line)
        .child(ui::icon(Sym::Diagram, px(14.), k.ink_3))
        .child(
            div()
                .t_meta()
                .text_color(k.ink_3)
                .child(kind.map_or(c::BLOCK, DiagramType::word)),
        )
        .when_some(spec.title.clone(), |el, t| {
            el.child(
                div()
                    .t_small()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(k.ink)
                    .child(crate::markdown::clip(&t, 80)),
            )
        })
        .child(div().flex_1())
        .when(actions, |el| {
            el.child(
                gpui_component::button::Button::new(ElementId::Name(format!("dsrc-{id}").into()))
                    .ghost()
                    .compact()
                    .label(c::VIEW_SOURCE)
                    .with_size(gpui_component::Size::XSmall)
                    .selected(show_source)
                    .on_click(move |_, _, cx| {
                        toggle_ui.update(cx, |s, cx| {
                            s.show_source = !s.show_source;
                            cx.notify();
                        })
                    }),
            )
            .child(
                ui::icon_button_sm(ElementId::Name(format!("dcopy-{id}").into()), Sym::Copy)
                    .named(c::COPY_SOURCE)
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(source.clone()))
                    }),
            )
            .when_some(viewer, |el, (spec, n, focus)| {
                el.child(
                    ui::icon_button_sm(ElementId::Name(format!("dzoom-{id}").into()), Sym::Expand)
                        .named(c::ZOOM_IN)
                        .on_click(move |_, window, cx| {
                            open_viewer(&spec, &n, focus.clone(), window, cx)
                        }),
                )
            })
        })
}

/// The 正在画图 box: `--sunken`, 160 high, a moving line (static with Reduce motion).
fn pending(id: &SharedString, avail: f32, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let line_w = 64.;
    let travel = (avail - line_w).max(0.);
    let line = div()
        .absolute()
        .bottom(px(48.))
        .h(px(2.))
        .w(px(line_w))
        .rounded(px(1.))
        .bg(k.line_2);
    let line: AnyElement = if cx.reduce_motion() {
        line.left(px(travel / 2.)).into_any_element()
    } else {
        line.with_animation(
            ElementId::Name(format!("dpending-{id}").into()),
            Animation::new(Duration::from_millis(1400)).repeat(),
            move |el, t| {
                // Ease in and out across the width and back.
                let p = if t < 0.5 { t * 2. } else { 2. - t * 2. };
                let eased = p * p * (3. - 2. * p);
                el.left(px(travel * eased))
            },
        )
        .into_any_element()
    };
    div()
        .relative()
        .w_full()
        .h(px(PENDING_HEIGHT))
        .rounded(radius::MD)
        .bg(k.sunken)
        .flex()
        .items_center()
        .justify_center()
        .child(div().t_small().text_color(k.ink_3).child(c::DRAWING))
        .child(line)
        .into_any_element()
}

fn image_with_hits(
    spec: &InlineSpec,
    ready: &Ready,
    avail: f32,
    ui: &gpui::Entity<Ui>,
    _window: &mut Window,
    cx: &App,
) -> AnyElement {
    let k = theme::colors(cx);
    let (expanded, hovered, focused) = {
        let u = ui.read(cx);
        (u.expanded, u.hovered, u.focused)
    };
    let fit = inline_fit(ready.meta.width, ready.meta.height, avail, expanded);
    let full_h = ready.meta.height * fit.scale;
    let id = spec.id.clone();
    let mut layer = div().relative().w(px(fit.w)).h(px(full_h)).child(
        img(ImageSource::Image(ready.image.clone()))
            .w(px(fit.w))
            .h(px(full_h)),
    );
    let refs_on = crate::views::refs::enabled(cx);
    for (i, node) in ready.meta.nodes.iter().enumerate() {
        let (x, y, w, h) = hit_rect(node, fit.scale);
        let ring = if focused == Some(i) {
            Some((px(2.), k.ink_3))
        } else if hovered == Some(i) {
            Some((px(1.), k.line_2))
        } else {
            None
        };
        let hover_ui = ui.clone();
        let origin = spec.origin.clone();
        let (nid, label) = (node.id.clone(), node.label.clone());
        let (ask_origin, ask_id, ask_label) =
            (spec.origin.clone(), node.id.clone(), node.label.clone());
        layer = layer.child(
            div()
                .id(ElementId::Name(format!("dnode-{id}-{i}").into()))
                .absolute()
                .left(px(x - 3.))
                .top(px(y - 3.))
                .w(px(w + 6.))
                .h(px(h + 6.))
                .rounded(radius::MD)
                .aria_label(node.label.clone())
                .when_some(ring, |el, (wd, color)| el.border(wd).border_color(color))
                .on_hover(move |on, _, cx| {
                    hover_ui.update(cx, |s, cx| {
                        let next = if *on {
                            Some(i)
                        } else if s.hovered == Some(i) {
                            None
                        } else {
                            s.hovered
                        };
                        if next != s.hovered {
                            s.hovered = next;
                            cx.notify();
                        }
                    })
                })
                .on_click(move |e, window, cx| {
                    if e.modifiers().alt {
                        super::on_node_ref(origin.clone(), &nid, &label, window, cx);
                    }
                })
                // inc-refs: the ink 问这里 of every part, only when the server takes refs.
                .when(hovered == Some(i) && refs_on, |el| {
                    el.child(
                        div().absolute().top(px(-12.)).right(px(-8.)).child(
                            crate::views::refs::affordance::ask_button(
                                ElementId::Name(format!("dask-{id}-{i}").into()),
                                cx,
                            )
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                super::on_node_ref(
                                    ask_origin.clone(),
                                    &ask_id,
                                    &ask_label,
                                    window,
                                    cx,
                                )
                            }),
                        ),
                    )
                }),
        );
    }
    let toggle = ui.clone();
    div()
        .relative()
        .w_full()
        .flex()
        .justify_center()
        .child(
            div()
                .relative()
                .w(px(fit.w))
                .h(px(fit.h))
                .when(fit.clipped, |el| el.overflow_hidden())
                .child(layer)
                .when(fit.clipped, |el| {
                    el.child(
                        div()
                            .absolute()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .h(px(72.))
                            .bg(linear_gradient(
                                180.,
                                linear_color_stop(k.raised.opacity(0.), 0.),
                                linear_color_stop(k.raised, 1.),
                            ))
                            .flex()
                            .items_end()
                            .justify_center()
                            .pb(space::S8)
                            .child(
                                ui::button(
                                    ElementId::Name(format!("dexpand-{id}").into()),
                                    c::EXPAND,
                                    ui::ButtonKind::Secondary,
                                    ui::ButtonSize::Sm,
                                    cx,
                                )
                                .on_click(move |_, _, cx| {
                                    toggle.update(cx, |s, cx| {
                                        s.expanded = true;
                                        cx.notify();
                                    })
                                }),
                            ),
                    )
                }),
        )
        .into_any_element()
}

/// The source with line numbers, collapsed to [`SOURCE_LINES`] with 展开.
fn source_listing(id: &SharedString, source: &str, ui: &gpui::Entity<Ui>, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let expanded = ui.read(cx).source_expanded;
    let lines: Vec<&str> = source.lines().collect();
    let total = lines.len();
    let shown = if expanded {
        total
    } else {
        total.min(SOURCE_LINES)
    };
    let digits = total.max(1).to_string().len();
    let toggle = ui.clone();
    div()
        .w_full()
        .rounded(radius::MD)
        .bg(k.sunken)
        .py(space::S8)
        .flex()
        .flex_col()
        .children(lines.iter().take(shown).enumerate().map(|(i, line)| {
            div()
                .flex()
                .gap(space::S12)
                .px(space::S12)
                .t_code()
                .text_size(px(12.))
                .line_height(px(20.))
                .child(
                    div()
                        .w(px(8. * digits as f32))
                        .flex_none()
                        .text_color(k.ink_3)
                        .child(format!("{}", i + 1)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_color(k.ink)
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .child(line.to_string()),
                )
        }))
        .when(total > SOURCE_LINES, |el| {
            el.child(
                div().px(space::S8).pt(space::S4).child(
                    gpui_component::button::Button::new(ElementId::Name(
                        format!("dsrcmore-{id}").into(),
                    ))
                    .ghost()
                    .compact()
                    .with_size(gpui_component::Size::XSmall)
                    .label(if expanded { c::COLLAPSE } else { c::EXPAND })
                    .on_click(move |_, _, cx| {
                        toggle.update(cx, |s, cx| {
                            s.source_expanded = !s.source_expanded;
                            cx.notify();
                        })
                    }),
                ),
            )
        })
        .into_any_element()
}

/// An error frame (REFS §B4 table): the reason in the header, then what it is about (the
/// line the library could not read, or the node count of a too large diagram), the source
/// below. Red only for 图没有画出来 (Refs.dc.html §07: 红色只用在这里); a too large
/// diagram has its own quiet state (Refs.dc.html 状态一览: 图比较大).
fn error_frame(
    spec: &InlineSpec,
    error: ErrorKind,
    hint: Option<Hint>,
    kind: Option<DiagramType>,
    ui: &gpui::Entity<Ui>,
    _window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let k = theme::colors(cx);
    let id = spec.id.clone();
    let red = error == ErrorKind::Failed;
    let source = spec.source.clone();
    let (border, head_bg, icon, icon_color, text_color) = match error {
        ErrorKind::Failed => (
            k.bad.opacity(0.35),
            k.bad.opacity(0.07),
            Sym::Failed,
            k.bad_ink,
            k.bad_ink,
        ),
        ErrorKind::TooBig => (k.line, k.sunken, Sym::Waiting, k.ink_3, k.ink_2),
        ErrorKind::Unsupported | ErrorKind::Forbidden => {
            (k.line, k.raised, Sym::Diagram, k.ink_3, k.ink)
        }
    };
    let hint_text = hint.and_then(|h| match h {
        Hint::Line(n) => super::normalize::original_line(&spec.source, n)
            .map(|line| crate::copy::fill(c::ERROR_LINE, &[("n", &line.to_string())])),
        Hint::Nodes(n) => Some(crate::copy::fill(c::ERROR_NODES, &[("n", &n.to_string())])),
    });
    let focus = ui.read(cx).focus.clone();
    div()
        .id(ElementId::Name(format!("diagram-{id}").into()))
        .w_full()
        .max_w(px(theme::PAGE_MEASURE.into()))
        .flex()
        .flex_col()
        .rounded(radius::LG)
        .border_1()
        .border_color(border)
        .bg(k.raised)
        .overflow_hidden()
        .track_focus(&focus)
        .child(
            div()
                .h(px(36.))
                .px(space::S12)
                .flex()
                .items_center()
                .gap(space::S6)
                .bg(head_bg)
                .border_b_1()
                .border_color(border)
                .child(ui::icon(icon, px(14.), icon_color))
                .when_some(kind.filter(|_| !red), |el, kind| {
                    el.child(div().t_meta().text_color(k.ink_3).child(kind.word()))
                })
                .child(
                    div()
                        .t_small()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(text_color)
                        .child(error.text()),
                )
                .when_some(hint_text, |el, text| {
                    el.child(
                        div()
                            .min_w_0()
                            .truncate()
                            .t_small()
                            .text_color(k.ink_2)
                            .child(text),
                    )
                })
                .child(div().flex_1())
                .child(
                    ui::icon_button_sm(ElementId::Name(format!("dcopy-{id}").into()), Sym::Copy)
                        .named(c::COPY_SOURCE)
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(source.clone()))
                        }),
                ),
        )
        .child(
            div()
                .p(space::S12)
                .child(source_listing(&id, &spec.source, ui, cx)),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(x: f32, y: f32, w: f32, h: f32) -> Node {
        Node {
            id: "n".into(),
            label: "n".into(),
            kind: "node".into(),
            x,
            y,
            w,
            h,
        }
    }

    #[test]
    fn fits_width_and_never_upscales() {
        let f = inline_fit(300., 100., 560., false);
        assert_eq!((f.scale, f.w, f.h, f.clipped), (1., 300., 100., false));
        let f = inline_fit(1120., 200., 560., false);
        assert_eq!((f.scale, f.w, f.h), (0.5, 560., 100.));
    }

    #[test]
    fn tall_diagrams_shrink_or_cut() {
        // 500 high at 1×: 360 / 500 = 0.72 ≥ 0.6, shrinks.
        let f = inline_fit(400., 500., 560., false);
        assert!((f.scale - 0.72).abs() < 1e-4 && !f.clipped);
        assert!((f.h - 360.).abs() < 1e-3);
        // 1000 high: 0.36 < 0.6, the top 360 at 1× with a fade.
        let f = inline_fit(400., 1000., 560., false);
        assert_eq!((f.scale, f.h, f.clipped), (1., 360., true));
        // Expanded: the whole height.
        let f = inline_fit(400., 1000., 560., true);
        assert_eq!((f.h, f.clipped), (1000., false));
    }

    #[test]
    fn hit_map_scales_with_inline_fit_and_viewer_zoom() {
        let n = node(10., 20., 100., 40.);
        assert_eq!(hit_rect(&n, 0.5), (5., 10., 50., 20.));
        assert_eq!(hit_rect(&n, 2.5), (25., 50., 250., 100.));
    }
}
