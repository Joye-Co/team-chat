//! The image viewer: a picture of a message, full size, over the whole window.
//!
//! It shows every picture of the 话题 (oldest first) on a dark screen, starting at the one
//! that was clicked: who sent it and when, `3 / 12`, the picture fitted to the window,
//! round previous / next buttons at the sides and a filmstrip under it. The thumbnail
//! shows at once and the original replaces it when it is downloaded (with its progress).
//!
//! - ← → (and the buttons, the filmstrip) go through the pictures; Home / End jump to the
//!   ends.
//! - Double-click zooms to actual size at the pointer and back to fit; ⌘ + wheel, a pinch,
//!   ⌘+ ⌘- zoom, ⌘0 fits, ⌘1 is actual size. A zoomed picture pans by dragging, scrolling
//!   or the arrow keys with ⌥.
//! - ⌘C copies the picture, ⌘S saves it to「下载」; the bar also opens it in Preview, shows
//!   it in Finder and shares it.
//! - Esc, Space or a click on the screen around the picture closes it; the focus goes back
//!   where it was.
//!
//! [`open`] draws it in the main window ([`crate::app::RootView`]); a window without one
//! opens the picture with the default app, as before.

use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Instant};

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, Context, ElementId, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ObjectFit, ParentElement, PinchEvent, Pixels, Point, Render,
    ScrollEvent, ScrollHandle, StatefulInteractiveElement as _, Styled, StyledImage as _, Window,
    div, img, point, prelude::FluentBuilder as _, px,
};
use gpui_component::Disableable as _;

use super::{
    OpenAction, error_text,
    export::{self, FileRef},
    open_file, size_text,
};
use crate::{
    app::{Changes, Services},
    copy::{files as f, fill},
    data::{ChatProvider, FileAttachment, FileError, Member, Millis, TransferState, download_key},
    theme::{Type as _, space},
    ui::{
        self, Sym,
        native::{
            NativeType as _, screen_button,
            tokens::{layout, lightbox as lb},
        },
    },
    views::media::parts,
};

/// The picture that was clicked.
#[derive(Clone)]
pub struct Opening {
    pub group_id: String,
    pub topic_id: String,
    pub message_id: String,
    pub file: FileAttachment,
    pub provider: Arc<dyn ChatProvider>,
}

/// Opens the viewer at `opening` in this window, or, in a window without the main view,
/// the picture with the default app.
pub fn open(opening: Opening, window: &mut Window, cx: &mut App) {
    match crate::app::root_view(window, cx) {
        Some(root) => root.update(cx, |root, cx| root.open_lightbox(opening, window, cx)),
        None => open_file(
            &opening.provider,
            &opening.group_id,
            &opening.topic_id,
            &opening.file,
            OpenAction::Open,
            cx,
        ),
    }
}

/// The viewer asks its window to take it away.
pub struct Dismiss;

/// One picture of the 话题.
#[derive(Clone, Debug, PartialEq)]
struct Item {
    file: FileAttachment,
    message_id: String,
    author_id: String,
    created_at: Millis,
}

impl Item {
    fn key(&self) -> (&str, &str) {
        (&self.message_id, &self.file.file_id)
    }
}

/// The original of a picture.
#[derive(Clone, Debug)]
enum Original {
    Loading,
    Ready(PathBuf),
    Failed(FileError),
}

/// A press that may become a pan.
#[derive(Clone, Copy)]
struct Drag {
    from: Point<Pixels>,
    pan: Point<f32>,
}

/// Where the picture area sits in the window, measured each frame.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
struct Stage {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Stage {
    fn center(&self) -> Point<f32> {
        point(self.x + self.w / 2., self.y + self.h / 2.)
    }
}

pub struct Lightbox {
    provider: Arc<dyn ChatProvider>,
    group_id: String,
    topic_id: String,
    items: Vec<Item>,
    index: usize,
    /// `None` fits the picture in the window; otherwise image pixels per point.
    zoom: Option<f32>,
    /// The picture's centre, from the stage's centre, in points.
    pan: Point<f32>,
    originals: HashMap<String, Original>,
    drag: Option<Drag>,
    /// The last press began on the screen around the picture (a click there closes).
    pressed_outside: bool,
    stage: Stage,
    strip: ScrollHandle,
    focus: FocusHandle,
    return_focus: Option<FocusHandle>,
    opened: Instant,
    _subscriptions: Vec<gpui::Subscription>,
}

impl EventEmitter<Dismiss> for Lightbox {}

impl Focusable for Lightbox {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The pictures of a 话题 the viewer goes through: the images among its files, oldest first.
fn items_of(provider: &Arc<dyn ChatProvider>, group_id: &str, topic_id: &str) -> Vec<Item> {
    let mut items: Vec<Item> = provider
        .topic_files(group_id, topic_id)
        .into_iter()
        .filter(|t| super::gallery::shows_inline(&t.file))
        .map(|t| Item {
            file: t.file,
            message_id: t.message_id,
            author_id: t.author_id,
            created_at: t.created_at,
        })
        .collect();
    items.reverse();
    // The files of one message come newest-first too; keep each message's own order.
    items.sort_by_key(|i| i.created_at);
    items
}

impl Lightbox {
    pub fn new(
        services: &Services,
        opening: Opening,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let provider = services.provider.clone();
        let mut items = items_of(&provider, &opening.group_id, &opening.topic_id);
        let at = |items: &[Item]| {
            items.iter().position(|i| {
                i.message_id == opening.message_id && i.file.file_id == opening.file.file_id
            })
        };
        let index = match at(&items) {
            Some(i) => i,
            None => {
                // Not in the cached files (yet): show it alone.
                let author = provider
                    .messages(&opening.group_id, &opening.topic_id, 500)
                    .messages
                    .into_iter()
                    .find(|m| m.id == opening.message_id)
                    .map(|m| (m.author_id, m.created_at))
                    .unwrap_or_default();
                items = vec![Item {
                    file: opening.file.clone(),
                    message_id: opening.message_id.clone(),
                    author_id: author.0,
                    created_at: author.1,
                }];
                0
            }
        };
        let (g, t) = (opening.group_id.clone(), opening.topic_id.clone());
        let subscriptions = vec![cx.subscribe(
            &services.hub,
            move |this: &mut Self, _, changes: &Changes, cx| {
                if changes.touches_topic(&g, &t) {
                    this.refresh(cx);
                    cx.notify();
                }
            },
        )];
        let focus = cx.focus_handle();
        let return_focus = window.focused(cx);
        window.focus(&focus, cx);
        let mut this = Self {
            provider,
            group_id: opening.group_id,
            topic_id: opening.topic_id,
            items,
            index,
            zoom: None,
            pan: point(0., 0.),
            originals: HashMap::new(),
            drag: None,
            pressed_outside: false,
            stage: Stage::default(),
            strip: ScrollHandle::new(),
            focus,
            return_focus,
            opened: Instant::now(),
            _subscriptions: subscriptions,
        };
        this.load_around(cx);
        this.strip.scroll_to_item(index);
        this
    }

    /// The pictures changed (a new one, one deleted): keep the one shown.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let items = items_of(&self.provider, &self.group_id, &self.topic_id);
        if items.is_empty() || items == self.items {
            return;
        }
        let current = self.items.get(self.index).cloned();
        match current.and_then(|c| items.iter().position(|i| i.key() == c.key())) {
            Some(i) => {
                self.items = items;
                self.index = i;
            }
            // The one shown is gone (deleted): stay near where it was.
            None => {
                self.index = self.index.min(items.len() - 1);
                self.items = items;
                self.reset_view();
            }
        }
        self.load_around(cx);
    }

    fn current(&self) -> &Item {
        &self.items[self.index.min(self.items.len() - 1)]
    }

    /// Starts the download of the shown picture and its neighbours.
    fn load_around(&mut self, cx: &mut Context<Self>) {
        let lo = self.index.saturating_sub(1);
        let hi = (self.index + 1).min(self.items.len() - 1);
        for i in lo..=hi {
            self.load(i, cx);
        }
    }

    fn load(&mut self, i: usize, cx: &mut Context<Self>) {
        let file = self.items[i].file.clone();
        if matches!(
            self.originals.get(&file.file_id),
            Some(Original::Loading | Original::Ready(_))
        ) {
            return;
        }
        if let Some(path) = self.provider.cached_file(&file) {
            self.originals
                .insert(file.file_id.clone(), Original::Ready(path));
            return;
        }
        self.originals
            .insert(file.file_id.clone(), Original::Loading);
        let rx = self
            .provider
            .fetch_file(&self.group_id, &self.topic_id, &file);
        cx.spawn(async move |this, cx| {
            let result = rx.await.unwrap_or(Err(FileError::Other));
            let _ = this.update(cx, |this, cx| {
                let state = match result {
                    Ok(path) => Original::Ready(path),
                    Err(e) => Original::Failed(e),
                };
                this.originals.insert(file.file_id.clone(), state);
                cx.notify();
            });
        })
        .detach();
    }

    fn original(&self, item: &Item) -> Option<PathBuf> {
        match self.originals.get(&item.file.file_id) {
            Some(Original::Ready(p)) => Some(p.clone()),
            _ => None,
        }
    }

    fn reset_view(&mut self) {
        self.zoom = None;
        self.pan = point(0., 0.);
        self.drag = None;
    }

    fn go(&mut self, index: usize, cx: &mut Context<Self>) {
        let index = index.min(self.items.len() - 1);
        if index == self.index {
            return;
        }
        self.index = index;
        self.reset_view();
        self.load_around(cx);
        self.strip.scroll_to_item(index);
        cx.notify();
    }

    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let i = self.index as isize + delta;
        if i >= 0 && (i as usize) < self.items.len() {
            self.go(i as usize, cx);
        }
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(f) = self.return_focus.take() {
            window.focus(&f, cx);
        }
        cx.emit(Dismiss);
    }

    // --- zoom ------------------------------------------------------------------------

    /// The picture's size in pixels, when the message says it.
    fn natural(&self) -> Option<(f32, f32)> {
        match (self.current().file.width, self.current().file.height) {
            (Some(w), Some(h)) if w > 0 && h > 0 => Some((w as f32, h as f32)),
            _ => None,
        }
    }

    fn fit(&self) -> f32 {
        match self.natural() {
            Some((w, h)) if self.stage.w > 0. => fit_zoom(w, h, self.stage.w, self.stage.h),
            _ => 1.0,
        }
    }

    fn zoom(&self) -> f32 {
        self.zoom.unwrap_or_else(|| self.fit())
    }

    /// Zooms to `to`, keeping the picture point under `at` (window coordinates; the stage
    /// centre when `None`) where it is.
    fn zoom_to(&mut self, to: f32, at: Option<Point<Pixels>>, cx: &mut Context<Self>) {
        if self.natural().is_none() {
            return;
        }
        let from = self.zoom();
        let fit = self.fit();
        let to = to.clamp(lb::ZOOM_MIN.min(fit), lb::ZOOM_MAX);
        let c = self.stage.center();
        let a = at.map_or(point(0., 0.), |p| {
            point(f32::from(p.x) - c.x, f32::from(p.y) - c.y)
        });
        let r = to / from;
        self.pan = point(a.x - (a.x - self.pan.x) * r, a.y - (a.y - self.pan.y) * r);
        // Back at (or under) the fit size: fit again, centred.
        if to <= fit + 1e-3 {
            self.zoom = None;
            self.pan = point(0., 0.);
        } else {
            self.zoom = Some(to);
        }
        self.clamp_pan();
        cx.notify();
    }

    fn zoom_in(&mut self, cx: &mut Context<Self>) {
        self.zoom_to(self.zoom() * lb::ZOOM_STEP, None, cx);
    }

    fn zoom_out(&mut self, cx: &mut Context<Self>) {
        self.zoom_to(self.zoom() / lb::ZOOM_STEP, None, cx);
    }

    /// Double-click: actual size at the pointer (twice that for a picture smaller than the
    /// window), and back to fit.
    fn toggle_zoom(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        if self.zoom.is_some() {
            self.zoom_to(0., None, cx);
        } else {
            let fit = self.fit();
            let to = if fit < 0.999 { 1.0 } else { fit * 2. };
            self.zoom_to(to, Some(at), cx);
        }
    }

    fn zoomed(&self) -> bool {
        self.zoom.is_some()
    }

    /// Keeps a zoomed picture over the stage: it never leaves a gap at an edge it could
    /// cover, and a side smaller than the stage stays centred.
    fn clamp_pan(&mut self) {
        let Some((w, h)) = self.natural() else { return };
        let z = self.zoom();
        let room = |size: f32, stage: f32| ((size * z - stage) / 2.).max(0.);
        let (rx, ry) = (room(w, self.stage.w), room(h, self.stage.h));
        self.pan = point(self.pan.x.clamp(-rx, rx), self.pan.y.clamp(-ry, ry));
    }

    fn pan_by(&mut self, dx: f32, dy: f32, cx: &mut Context<Self>) {
        self.pan = point(self.pan.x + dx, self.pan.y + dy);
        self.clamp_pan();
        cx.notify();
    }

    // --- actions ---------------------------------------------------------------------

    fn target(&self) -> FileRef {
        FileRef::attachment(
            &self.provider,
            &self.group_id,
            &self.topic_id,
            &self.current().file,
        )
    }

    fn copy(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.original(self.current()) {
            parts::copy_image_file(&path, cx);
        }
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.original(self.current()) {
            parts::save_to_downloads(&[path.as_path()], cx);
        }
    }

    fn open_outside(&mut self, what: OpenAction, cx: &mut Context<Self>) {
        let file = self.current().file.clone();
        open_file(
            &self.provider,
            &self.group_id,
            &self.topic_id,
            &file,
            what,
            cx,
        );
    }

    fn on_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let m = &e.keystroke.modifiers;
        let cmd = crate::command_key(m);
        let key = e.keystroke.key.as_str();
        match key {
            "escape" | "space" if !cmd => self.close(window, cx),
            "left" if m.alt && self.zoomed() => self.pan_by(lb::PAN_STEP, 0., cx),
            "right" if m.alt && self.zoomed() => self.pan_by(-lb::PAN_STEP, 0., cx),
            "up" if m.alt && self.zoomed() => self.pan_by(0., lb::PAN_STEP, cx),
            "down" if m.alt && self.zoomed() => self.pan_by(0., -lb::PAN_STEP, cx),
            "left" | "up" | "pageup" => self.step(-1, cx),
            "right" | "down" | "pagedown" => self.step(1, cx),
            "home" => self.go(0, cx),
            "end" => self.go(self.items.len() - 1, cx),
            "=" | "+" if cmd => self.zoom_in(cx),
            "-" if cmd => self.zoom_out(cx),
            "0" if cmd => self.zoom_to(0., None, cx),
            "1" if cmd => self.zoom_to(1., None, cx),
            "c" if cmd => self.copy(cx),
            "s" if cmd => self.save(cx),
            "w" if cmd => self.close(window, cx),
            _ => return,
        }
        cx.stop_propagation();
    }
}

/// The zoom that fits `w` x `h` pixels in a stage of `sw` x `sh` points, never above 1.
pub fn fit_zoom(w: f32, h: f32, sw: f32, sh: f32) -> f32 {
    if w <= 0. || h <= 0. || sw <= 0. || sh <= 0. {
        return 1.0;
    }
    (sw / w).min(sh / h).min(1.0)
}

/// `11-12 01:25` for the bar: the day when it is not today, and the time.
fn when(at: Millis) -> String {
    let day = crate::views::shell::clock(at);
    if day.contains(':') {
        day
    } else {
        format!("{day} {}", crate::views::timeline::model::clock(at))
    }
}

impl Render for Lightbox {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let screen = crate::views::media::screen(cx);
        let ink = screen.ink;
        let dim = screen.dim;
        let n = self.items.len();
        let many = n > 1;
        let vp = window.viewport_size();
        let (vw, vh) = (f32::from(vp.width), f32::from(vp.height));
        // The stage: the window less the bar, the strip and the side buttons.
        let side = if many {
            f32::from(lb::NAV_INSET + lb::NAV + space::S8)
        } else {
            f32::from(lb::PAD)
        };
        let top = f32::from(lb::BAR_H);
        let bottom = if many {
            f32::from(lb::STRIP_H)
        } else {
            f32::from(lb::PAD)
        };
        let stage = Stage {
            x: side,
            y: top,
            w: (vw - side * 2.).max(1.),
            h: (vh - top - bottom - f32::from(space::S8)).max(1.),
        };
        if stage != self.stage {
            self.stage = stage;
            self.clamp_pan();
        }
        let item = self.current().clone();
        let original = self.original(&item);
        let thumb = self
            .provider
            .thumbnail(&self.group_id, &self.topic_id, &item.file);
        let shown = original.clone().or(thumb);
        let zoom = self.zoom();
        let zoomed = self.zoomed();
        let natural = self.natural();

        // --- the picture --------------------------------------------------------------
        let picture: AnyElement = match (&shown, natural) {
            (Some(path), Some((w, h))) => {
                let (dw, dh) = (w * zoom, h * zoom);
                let left = (stage.w - dw) / 2. + self.pan.x;
                let top = (stage.h - dh) / 2. + self.pan.y;
                div()
                    .id("lightbox-picture")
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(dw))
                    .h(px(dh))
                    .when(zoomed, |el| el.cursor_grab())
                    .when(zoomed && self.drag.is_some(), |el| el.cursor_grabbing())
                    .child(img(path.clone()).size_full().object_fit(ObjectFit::Fill))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, e: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.pressed_outside = false;
                            if e.click_count >= 2 {
                                this.toggle_zoom(e.position, cx);
                            } else if this.zoomed() {
                                this.drag = Some(Drag {
                                    from: e.position,
                                    pan: this.pan,
                                });
                            }
                        }),
                    )
                    .into_any_element()
            }
            // No size in the message: fitted by the image itself (no zoom).
            (Some(path), None) => div()
                .id("lightbox-picture")
                .absolute()
                .inset_0()
                .child(
                    img(path.clone())
                        .size_full()
                        .object_fit(ObjectFit::ScaleDown),
                )
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .into_any_element(),
            (None, _) => div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .child(ui::icon(Sym::Image, px(48.), dim))
                .into_any_element(),
        };
        // The original's state: progress while it downloads over the thumbnail, the reason
        // when it failed.
        let status: Option<AnyElement> = match self.originals.get(&item.file.file_id) {
            Some(Original::Failed(e)) => Some(
                pill(cx)
                    .text_color(ink)
                    .child(ui::icon(Sym::Alert, px(14.), ink))
                    .child(fill(f::VIEWER_FAILED, &[("reason", error_text(*e))]))
                    .child(
                        screen_button("lightbox-retry", Sym::Retry, f::RETRY, false, ink, cx)
                            .on_click(cx.listener(|this, _, _, cx| {
                                let id = this.current().file.file_id.clone();
                                this.originals.remove(&id);
                                let i = this.index;
                                this.load(i, cx);
                                cx.notify();
                            })),
                    )
                    .into_any_element(),
            ),
            Some(Original::Loading) => {
                let p = match self.provider.file_transfer(&download_key(&item.file)) {
                    Some(s @ TransferState::Running { .. }) => Some(s.percent()),
                    _ => None,
                };
                Some(
                    pill(cx)
                        .text_color(ink)
                        .child(crate::ui::motion::spinner().color(ink))
                        .child(match p {
                            Some(p) => fill(f::VIEWER_LOADING, &[("p", &format!("{}", p as u32))]),
                            None => f::VIEWER_LOADING_START.to_string(),
                        })
                        .into_any_element(),
                )
            }
            _ => None,
        };
        let stage_el = div()
            .id("lightbox-stage")
            .absolute()
            .left(px(stage.x))
            .top(px(stage.y))
            .w(px(stage.w))
            .h(px(stage.h))
            .overflow_hidden()
            .child(picture)
            .on_scroll(cx.listener(|this, e: &ScrollEvent, _, cx| {
                let m = &e.modifiers;
                let d = e.delta.pixel_delta(px(16.));
                let (dx, dy) = (f32::from(d.x), f32::from(d.y));
                if m.platform || m.control {
                    let factor = (dy * lb::WHEEL_ZOOM).exp();
                    this.zoom_to(this.zoom() * factor, Some(e.position), cx);
                    cx.stop_propagation();
                } else if this.zoomed() {
                    this.pan_by(dx, dy, cx);
                    cx.stop_propagation();
                }
            }))
            .on_pinch(cx.listener(|this, e: &PinchEvent, _, cx| {
                this.zoom_to(this.zoom() * (1. + e.delta), Some(e.position), cx);
            }));

        // --- the bar --------------------------------------------------------------------
        let author: Option<Member> = self.provider.member(&item.author_id);
        let mut meta = vec![when(item.created_at)];
        if let Some((w, h)) = natural {
            meta.push(fill(
                f::VIEWER_DIMS,
                &[
                    ("w", &(w as u32).to_string()),
                    ("h", &(h as u32).to_string()),
                ],
            ));
        }
        meta.push(size_text(item.file.size));
        let lights = cfg!(target_os = "macos") && !window.is_fullscreen();
        let who = div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap(space::S8)
            .child(ui::MemberAvatar::new(
                author.as_ref(),
                crate::ui::native::tokens::avatar::SM,
            ))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .n_headline()
                            .text_color(ink)
                            .one_line()
                            .child(author.as_ref().map(|m| m.name.clone()).unwrap_or_default()),
                    )
                    .child(
                        div()
                            .n_footnote()
                            .n_figures()
                            .text_color(dim)
                            .one_line()
                            .child(meta.join(" · ")),
                    ),
            );
        let has_original = original.is_some();
        let can_zoom = natural.is_some() && shown.is_some();
        let tool = |id: &'static str, sym: Sym, name: &'static str, cx: &App| {
            screen_button(id, sym, name, false, ink, cx)
        };
        let tools = div()
            .flex_1()
            .flex()
            .justify_end()
            .items_center()
            .gap(space::S2)
            .child(
                tool("lightbox-zoom-out", Sym::ZoomOut, f::VIEWER_ZOOM_OUT, cx)
                    .disabled(!can_zoom || !zoomed)
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_out(cx))),
            )
            .child(
                div()
                    .min_w(px(44.))
                    .text_center()
                    .n_footnote()
                    .n_figures()
                    .text_color(if can_zoom { ink } else { dim })
                    .child(fill(
                        f::VIEWER_ZOOM,
                        &[("n", &format!("{}", (zoom * 100.).round() as i32))],
                    )),
            )
            .child(
                tool("lightbox-zoom-in", Sym::ZoomIn, f::VIEWER_ZOOM_IN, cx)
                    .disabled(!can_zoom || zoom >= lb::ZOOM_MAX - 1e-3)
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_in(cx))),
            )
            .child(separator(screen.line))
            .child(
                tool("lightbox-copy", Sym::Copy, f::VIEWER_COPY, cx)
                    .disabled(!has_original)
                    .on_click(cx.listener(|this, _, _, cx| this.copy(cx))),
            )
            .child(
                tool("lightbox-save", Sym::Download, f::VIEWER_SAVE, cx)
                    .disabled(!has_original)
                    .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
            )
            .child(
                tool("lightbox-share", Sym::Share, f::VIEWER_SHARE, cx).on_click(cx.listener(
                    |this, e: &gpui::ClickEvent, window, cx| {
                        let target = this.target();
                        export::share(&target, e.position(), window, cx);
                    },
                )),
            )
            .child(
                tool("lightbox-open", Sym::OpenExternal, f::VIEWER_OPEN, cx).on_click(
                    cx.listener(|this, _, _, cx| this.open_outside(OpenAction::Open, cx)),
                ),
            )
            .child(
                tool("lightbox-reveal", Sym::Folder, f::REVEAL, cx).on_click(
                    cx.listener(|this, _, _, cx| this.open_outside(OpenAction::RevealInFinder, cx)),
                ),
            )
            .child(separator(screen.line))
            .child(
                tool("lightbox-close", Sym::Close, f::VIEWER_CLOSE, cx)
                    .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
            );
        let bar = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(lb::BAR_H)
            .pl(if lights {
                layout::TRAFFIC_LIGHTS_INSET
            } else {
                space::S16
            })
            .pr(space::S12)
            .flex()
            .items_center()
            .gap(space::S16)
            .child(who)
            .when(many, |el| {
                el.child(
                    div()
                        .flex_none()
                        .n_callout()
                        .n_figures()
                        .text_color(ink)
                        .child(fill(
                            f::VIEWER_POSITION,
                            &[("i", &(self.index + 1).to_string()), ("n", &n.to_string())],
                        )),
                )
            })
            .child(tools)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());

        // --- previous / next ------------------------------------------------------------
        let nav = |id: &'static str,
                   sym: Sym,
                   name: &'static str,
                   enabled: bool,
                   delta: isize,
                   cx: &mut Context<Self>| {
            div()
                .absolute()
                .top(px(stage.y + stage.h / 2. - f32::from(lb::NAV) / 2.))
                .child(
                    screen_button(id, sym, name, true, ink, cx)
                        .disabled(!enabled)
                        .on_click(cx.listener(move |this, _, _, cx| this.step(delta, cx))),
                )
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        };
        let prev = many.then(|| {
            nav(
                "lightbox-prev",
                Sym::ChevronLeft,
                f::VIEWER_PREV,
                self.index > 0,
                -1,
                cx,
            )
            .left(lb::NAV_INSET)
        });
        let next = many.then(|| {
            nav(
                "lightbox-next",
                Sym::ChevronRight,
                f::VIEWER_NEXT,
                self.index + 1 < n,
                1,
                cx,
            )
            .right(lb::NAV_INSET)
        });

        // --- the filmstrip --------------------------------------------------------------
        let strip = many.then(|| {
            let thumbs = self.items.iter().enumerate().map(|(i, it)| {
                let picked = i == self.index;
                let src = self
                    .provider
                    .thumbnail(&self.group_id, &self.topic_id, &it.file)
                    .or_else(|| self.original(it));
                div()
                    .id(ElementId::Name(format!("lightbox-thumb-{i}").into()))
                    .flex_none()
                    .size(lb::THUMB)
                    .rounded(lb::THUMB_RADIUS)
                    .border_2()
                    .border_color(if picked {
                        ink
                    } else {
                        gpui::transparent_black()
                    })
                    .bg(screen.line)
                    .opacity(if picked { 1. } else { lb::THUMB_DIM })
                    .hover(|s| s.opacity(1.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(match src {
                        Some(p) => img(p)
                            .size_full()
                            .rounded(lb::THUMB_RADIUS - px(2.))
                            .object_fit(ObjectFit::Cover)
                            .into_any_element(),
                        None => ui::icon(Sym::Image, px(16.), dim).into_any_element(),
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.go(i, cx)))
            });
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .h(lb::STRIP_H)
                .flex()
                .justify_center()
                .items_center()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .id("lightbox-strip")
                        .max_w(px((vw - f32::from(space::S32) * 2.).max(0.)))
                        .overflow_x_scroll()
                        .track_scroll(&self.strip)
                        .flex()
                        .gap(lb::THUMB_GAP)
                        .px(space::S4)
                        .py(space::S4)
                        .children(thumbs),
                )
        });

        let status = status.map(|s| {
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(px(bottom + f32::from(space::S12)))
                .flex()
                .justify_center()
                .child(s)
        });

        let fade = !cx.reduce_motion() && self.opened.elapsed() < lb::OPEN * 2;
        let screen_el = div()
            .id("lightbox")
            .track_focus(&self.focus)
            .key_context("Lightbox")
            .absolute()
            .inset_0()
            .occlude()
            .bg(gpui::hsla(0., 0., 0., lb::BACKDROP))
            .text_color(ink)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, _| this.pressed_outside = true),
            )
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                if let Some(d) = this.drag {
                    if e.pressed_button != Some(MouseButton::Left) {
                        this.drag = None;
                        cx.notify();
                        return;
                    }
                    this.pan = point(
                        d.pan.x + f32::from(e.position.x - d.from.x),
                        d.pan.y + f32::from(e.position.y - d.from.y),
                    );
                    this.clamp_pan();
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, window, cx| {
                    let was_dragging = this.drag.take().is_some();
                    if this.pressed_outside && !was_dragging {
                        this.close(window, cx);
                    }
                    this.pressed_outside = false;
                    cx.notify();
                }),
            )
            .child(stage_el)
            .child(bar)
            .children(prev)
            .children(next)
            .children(status)
            .children(strip)
            // A quiet line of the keys at the bottom, for a single picture (no filmstrip).
            .when(!many, |el| {
                el.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom(space::S8)
                        .flex()
                        .justify_center()
                        .text_size(crate::ui::native::tokens::type_scale::CAPTION.0)
                        .text_color(dim)
                        .child(f::VIEWER_HINT),
                )
            });
        if fade {
            screen_el
                .with_animation(
                    "lightbox-open",
                    Animation::new(lb::OPEN).with_easing(gpui::ease_out_quint()),
                    |el, d| el.opacity(d),
                )
                .into_any_element()
        } else {
            screen_el.into_any_element()
        }
    }
}

/// A floating note on the screen: radius full, black at the bar's opacity.
fn pill(_cx: &App) -> gpui::Div {
    div()
        .h(px(32.))
        .px(space::S12)
        .rounded_full()
        .bg(gpui::hsla(0., 0., 0., lb::BAR_SCRIM + 0.3))
        .flex()
        .items_center()
        .gap(space::S8)
        .n_footnote()
}

fn separator(color: gpui::Hsla) -> gpui::Div {
    div().w(px(1.)).h(px(18.)).mx(space::S6).bg(color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitting_never_enlarges() {
        assert_eq!(fit_zoom(400., 300., 1200., 800.), 1.0);
        assert_eq!(fit_zoom(2400., 1600., 1200., 800.), 0.5);
        assert_eq!(fit_zoom(1000., 4000., 1200., 800.), 0.2);
        assert_eq!(fit_zoom(0., 10., 100., 100.), 1.0);
    }
}
