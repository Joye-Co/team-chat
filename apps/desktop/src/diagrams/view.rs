//! The inline diagram element (REFS §B4): a framed block with the type word and title, the
//! image at content width (never above 1×), the node hit map with a hover ring and
//! 问这里, keyboard focus over the nodes, 看原文 / 复制原文 / 放大, and the error frames
//! that keep the source visible.

use gpui::{
    AnyElement, App, ClipboardItem, ElementId, FocusHandle, ImageSource, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement as _, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, canvas, div, img, prelude::FluentBuilder as _, px,
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
/// The box shown while a diagram renders.
pub const PENDING_HEIGHT: f32 = 160.;
/// The smallest scale inline. Node labels are drawn at 13 px and measure about 9 px of
/// CJK ink at 1×, the least C1 allows, so a diagram never shrinks below 1× inline
/// (COMPLETENESS.md F64): a wider one scrolls sideways, one with too many nodes shows a
/// summary with 打开查看器.
pub const MIN_TEXT_SCALE: f32 = 1.0;
/// Above this many nodes a diagram never shows inline; the viewer has room for it.
pub const MAX_INLINE_NODES: usize = 60;
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

/// The inline fit (F64): the content width, never above 1× and never below
/// [`MIN_TEXT_SCALE`] (12 px text); a wider diagram scrolls sideways inside the card. A
/// diagram taller than [`MAX_INLINE_HEIGHT`] shows its top with 展开 in a footer row.
pub fn inline_fit(w: f32, h: f32, avail: f32, expanded: bool) -> Fit {
    let w = w.max(1.);
    let h = h.max(1.);
    let scale = (avail.max(1.) / w).clamp(MIN_TEXT_SCALE, 1.);
    let full = h * scale;
    let clipped = full > MAX_INLINE_HEIGHT && !expanded;
    Fit {
        scale,
        w: w * scale,
        h: if clipped { MAX_INLINE_HEIGHT } else { full },
        clipped,
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

/// The box a hover or focus outline draws around: a sequence participant's hit area
/// runs down its whole lifeline, so its outline keeps to the head box at the top
/// (E3: one outline around the box only).
pub fn outline_rect(node: &Node, scale: f32) -> (f32, f32, f32, f32) {
    let (x, y, w, h) = hit_rect(node, scale);
    if node.kind == "participant" {
        (x, y, w, h.min(w * 0.6))
    } else {
        (x, y, w, h)
    }
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
        // `diagram-loading` holds the drawing back to show the skeleton.
        Ok(n) if super::preview::LOADING.load(std::sync::atomic::Ordering::Relaxed) => {
            (State::Pending, Some(n.kind))
        }
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
    if let State::Ready(r) = &state
        && r.meta.nodes.len() > MAX_INLINE_NODES
    {
        return dense_alert(&spec, r, &ui, cx);
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
    let n = ui::native::native_colors(cx);
    // §2.4: one card width in the thread, the card frame; focus is the 3 px ring.
    div()
        .id(ElementId::Name(format!("diagram-{id}").into()))
        .relative()
        .w(ui::native::tokens::card_v2::MAX_WIDTH)
        .max_w_full()
        .flex()
        .flex_col()
        .rounded(ui::native::tokens::radius::CARD)
        .border_1()
        .border_color(n.separator)
        .when(focused_frame, |d| d.shadow(vec![ring_shadow(n.focus)]))
        .bg(n.content)
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

/// The 3 px focus ring around a frame, drawn outside it.
pub fn ring_shadow(color: gpui::Hsla) -> gpui::BoxShadow {
    gpui::BoxShadow {
        color,
        offset: gpui::point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(3.),
        inset: false,
    }
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
    let n = ui::native::native_colors(cx);
    let _ = &k;
    div()
        .h(ui::native::tokens::height::LG)
        .px(space::S16)
        .flex()
        .items_center()
        .gap(space::S8)
        .border_b_1()
        .border_color(n.separator)
        .child(ui::icon(Sym::Diagram, px(16.), n.secondary))
        .child(
            div()
                .t_meta()
                .text_color(n.secondary)
                .child(kind.map_or(c::BLOCK, DiagramType::word)),
        )
        .when_some(spec.title.clone(), |el, t| {
            el.child(
                div()
                    .t_small()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(n.label)
                    .min_w(px(0.))
                    .one_line()
                    .child(crate::markdown::clip(&t, 80)),
            )
        })
        .child(div().flex_1())
        .when(actions, |el| {
            el.child(
                ui::native::push_button(
                    ElementId::Name(format!("dsrc-{id}").into()),
                    c::VIEW_SOURCE,
                    ui::native::PushStyle::Ghost,
                    ui::native::PushSize::Small,
                    cx,
                )
                .selected(show_source)
                .on_click(move |_, _, cx| {
                    toggle_ui.update(cx, |s, cx| {
                        s.show_source = !s.show_source;
                        cx.notify();
                    })
                }),
            )
            .child(
                ui::native::icon_button_bare(
                    ElementId::Name(format!("dcopy-{id}").into()),
                    Sym::Copy,
                    ui::native::IconSize::Small,
                )
                .named(c::COPY_SOURCE)
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(source.clone()))
                }),
            )
            .when_some(viewer, |el, (spec, n, focus)| {
                el.child(
                    ui::native::icon_button_bare(
                        ElementId::Name(format!("dzoom-{id}").into()),
                        Sym::Expand,
                        ui::native::IconSize::Small,
                    )
                    .named(c::ZOOM_IN)
                    .on_click(move |_, window, cx| {
                        open_viewer(&spec, &n, focus.clone(), window, cx)
                    }),
                )
            })
        })
}

/// The box while a diagram renders: a pulsing skeleton of the diagram's height
/// (§2.14), static with Reduce motion; no text in the empty region.
fn pending(_id: &SharedString, _avail: f32, _cx: &App) -> AnyElement {
    ui::native::skeleton(gpui::relative(1.), px(PENDING_HEIGHT))
        .rounded(radius::MD)
        .into_any_element()
}

/// Too many nodes to read inline (F64): in place of the picture, the same
/// Alert as a diagram that did not draw (§2.15), with 打开查看器: one style
/// for one failure.
fn dense_alert(spec: &InlineSpec, ready: &Ready, ui: &gpui::Entity<Ui>, cx: &App) -> AnyElement {
    let n = ui::native::native_colors(cx);
    let id = spec.id.clone();
    let viewer = normalize::normalize(&spec.source)
        .ok()
        .map(|nz| (spec.clone(), nz, ui.read(cx).focus.clone()));
    let alert = ui::native::Alert::new(Sym::Diagram, c::TOO_DENSE)
        .icon_color(n.secondary)
        .description(crate::copy::fill(
            c::ERROR_NODES,
            &[("n", &ready.meta.nodes.len().to_string())],
        ));
    let alert = match viewer {
        Some((spec, nz, focus)) => alert.action(
            ui::native::push_button(
                ElementId::Name(format!("dview-{id}").into()),
                c::OPEN_VIEWER,
                ui::native::PushStyle::Outline,
                ui::native::PushSize::Small,
                cx,
            )
            .on_click(move |_, window, cx| open_viewer(&spec, &nz, focus.clone(), window, cx)),
        ),
        None => alert,
    };
    div()
        .id(ElementId::Name(format!("diagram-{id}").into()))
        .w(ui::native::tokens::card_v2::MAX_WIDTH)
        .max_w_full()
        .child(alert)
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
    let n = ui::native::native_colors(cx);
    let id = spec.id.clone();
    let (expanded, hovered, focused) = {
        let u = ui.read(cx);
        (u.expanded, u.hovered, u.focused)
    };
    let fit = inline_fit(ready.meta.width, ready.meta.height, avail, expanded);
    let full_h = ready.meta.height * fit.scale;
    let mut layer = div().relative().w(px(fit.w)).h(px(full_h)).child(
        img(ImageSource::Image(ready.image.clone()))
            .w(px(fit.w))
            .h(px(full_h)),
    );
    let refs_on = crate::views::refs::enabled(cx);
    for (i, node) in ready.meta.nodes.iter().enumerate() {
        let (x, y, w, h) = hit_rect(node, fit.scale);
        let (_, _, _, oh) = outline_rect(node, fit.scale);
        // E3: hover = one 1 px line around the node's box; focus = the 3 px ring.
        let hover_line = hovered == Some(i) && focused != Some(i);
        let hover_ui = ui.clone();
        let origin = spec.origin.clone();
        let (nid, label) = (node.id.clone(), node.label.clone());
        let (ask_origin, ask_id, ask_label) =
            (spec.origin.clone(), node.id.clone(), node.label.clone());
        layer = layer.child(
            div()
                .id(ElementId::Name(format!("dnode-{id}-{i}").into()))
                .absolute()
                .left(px(x - 2.))
                .top(px(y - 2.))
                .w(px(w + 4.))
                .h(px(h + 4.))
                .rounded(radius::MD)
                .aria_label(node.label.clone())
                .when(hover_line || focused == Some(i), |el| {
                    el.child(
                        div()
                            .absolute()
                            .left_0()
                            .top_0()
                            .w(px(w + 4.))
                            .h(px(oh + 4.))
                            .rounded(radius::MD)
                            .when(hover_line, |d| d.border_1().border_color(n.secondary))
                            .when(focused == Some(i), |d| d.shadow(vec![ring_shadow(n.focus)])),
                    )
                })
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
                // inc-refs: 问这里 outside the node's top edge, never over the node's
                // border or a connector beside it.
                .when(hovered == Some(i) && refs_on, |el| {
                    el.child(
                        div()
                            .absolute()
                            .bottom_full()
                            .right(px(0.))
                            .pb(space::S4)
                            .child(
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
    let scrolls = fit.w > avail + 0.5;
    div()
        .w_full()
        .flex()
        .flex_col()
        .child(
            div()
                .relative()
                .w_full()
                .child(
                    div()
                        .id(ElementId::Name(format!("dscroll-{id}").into()))
                        .w_full()
                        .flex()
                        .when(!scrolls, |d| d.justify_center())
                        .when(scrolls, |d| d.overflow_x_scroll())
                        .child(
                            div()
                                .flex_none()
                                .relative()
                                .w(px(fit.w))
                                .h(px(fit.h))
                                .when(fit.clipped, |el| el.overflow_hidden())
                                .child(layer),
                        ),
                )
                // A wider diagram fades at the right edge: there is more to scroll to,
                // never a hard cut mid-glyph (§2.21).
                .when(scrolls, |d| {
                    d.child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .right_0()
                            .w(space::S24)
                            .bg(gpui::linear_gradient(
                                90.,
                                gpui::linear_color_stop(n.content.opacity(0.), 0.),
                                gpui::linear_color_stop(n.content, 1.),
                            )),
                    )
                }),
        )
        // The top of a tall diagram: 展开 in a footer row under a line (§2.4), not on a
        // fade over the picture. A diagram wider than the card at 12 px text also
        // offers the viewer there (F64: an outline sm 打开查看器).
        .when(fit.clipped || scrolls, |el| {
            let viewer = scrolls
                .then(|| normalize::normalize(&spec.source).ok())
                .flatten()
                .map(|nz| (spec.clone(), nz, ui.read(cx).focus.clone()));
            el.child(
                div()
                    .mt(space::S12)
                    .mx(px(-PAD))
                    .mb(px(-PAD))
                    .px(px(PAD))
                    .py(space::S8)
                    .border_t_1()
                    .border_color(n.separator)
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .when(fit.clipped, |row| {
                        row.child(
                            ui::native::push_button(
                                ElementId::Name(format!("dexpand-{id}").into()),
                                c::EXPAND,
                                ui::native::PushStyle::Ghost,
                                ui::native::PushSize::Small,
                                cx,
                            )
                            .on_click(move |_, _, cx| {
                                toggle.update(cx, |s, cx| {
                                    s.expanded = true;
                                    cx.notify();
                                })
                            }),
                        )
                    })
                    .when_some(viewer, |row, (spec, nz, focus)| {
                        row.child(
                            ui::native::push_button(
                                ElementId::Name(format!("dwide-{id}").into()),
                                c::OPEN_VIEWER,
                                ui::native::PushStyle::Outline,
                                ui::native::PushSize::Small,
                                cx,
                            )
                            .on_click(move |_, window, cx| {
                                open_viewer(&spec, &nz, focus.clone(), window, cx)
                            }),
                        )
                    }),
            )
        })
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

/// A diagram that did not draw (F64, §2.15): one Alert in place of the picture: the
/// icon (red only for 图没有画出来, the one the writer can fix), the reason as the
/// title, what it is about (the line the library could not read, or the node count) as
/// the description, then 看原文 and 复制原文. The source shows under it on 看原文.
fn error_frame(
    spec: &InlineSpec,
    error: ErrorKind,
    hint: Option<Hint>,
    _kind: Option<DiagramType>,
    ui: &gpui::Entity<Ui>,
    _window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let n = ui::native::native_colors(cx);
    let id = spec.id.clone();
    let source = spec.source.clone();
    let (icon, icon_color, fallback) = match error {
        ErrorKind::Failed => (Sym::Failed, n.red, c::ERROR_FAILED_DESC),
        ErrorKind::TooBig => (Sym::Diagram, n.secondary, c::ERROR_TOO_BIG_DESC),
        ErrorKind::Unsupported => (Sym::Diagram, n.secondary, c::ERROR_UNSUPPORTED_DESC),
        ErrorKind::Forbidden => (Sym::Diagram, n.secondary, c::ERROR_FORBIDDEN_DESC),
    };
    let hint_text = hint.and_then(|h| match h {
        Hint::Line(n) => super::normalize::original_line(&spec.source, n)
            .map(|line| crate::copy::fill(c::ERROR_LINE, &[("n", &line.to_string())])),
        Hint::Nodes(n) => Some(crate::copy::fill(c::ERROR_NODES, &[("n", &n.to_string())])),
    });
    let description = match hint_text {
        Some(h) => format!("{h} · {fallback}"),
        None => fallback.to_string(),
    };
    let (focus, show_source) = {
        let u = ui.read(cx);
        (u.focus.clone(), u.show_source)
    };
    let toggle = ui.clone();
    div()
        .id(ElementId::Name(format!("diagram-{id}").into()))
        .w(ui::native::tokens::card_v2::MAX_WIDTH)
        .max_w_full()
        .flex()
        .flex_col()
        .gap(space::S8)
        .track_focus(&focus)
        .child(
            ui::native::Alert::new(icon, error.text())
                .icon_color(icon_color)
                .description(description)
                .action(
                    ui::native::push_button(
                        ElementId::Name(format!("dsrc-{id}").into()),
                        c::VIEW_SOURCE,
                        ui::native::PushStyle::Outline,
                        ui::native::PushSize::Small,
                        cx,
                    )
                    .selected(show_source)
                    .on_click(move |_, _, cx| {
                        toggle.update(cx, |s, cx| {
                            s.show_source = !s.show_source;
                            cx.notify();
                        })
                    }),
                )
                .action(
                    ui::native::icon_button_bare(
                        ElementId::Name(format!("dcopy-{id}").into()),
                        Sym::Copy,
                        ui::native::IconSize::Small,
                    )
                    .named(c::COPY_SOURCE)
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(source.clone()))
                    }),
                ),
        )
        .when(show_source, |d| {
            d.child(source_listing(&id, &spec.source, ui, cx))
        })
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
    fn never_shrinks_text_and_scrolls_a_wide_diagram() {
        let f = inline_fit(300., 100., 608., false);
        assert_eq!((f.scale, f.w, f.h, f.clipped), (1., 300., 100., false));
        // Wider than the card: 1×, the rest scrolls sideways.
        let f = inline_fit(1216., 200., 608., false);
        assert_eq!((f.scale, f.w, f.h), (1., 1216., 200.));
    }

    #[test]
    fn tall_diagrams_show_their_top_until_expanded() {
        let f = inline_fit(400., 1000., 608., false);
        assert_eq!((f.scale, f.h, f.clipped), (1., 360., true));
        let f = inline_fit(400., 1000., 608., true);
        assert_eq!((f.h, f.clipped), (1000., false));
    }

    #[test]
    fn hit_map_scales_with_inline_fit_and_viewer_zoom() {
        let n = node(10., 20., 100., 40.);
        assert_eq!(hit_rect(&n, 0.5), (5., 10., 50., 20.));
        assert_eq!(hit_rect(&n, 2.5), (25., 50., 250., 100.));
    }
}
