//! A 产出 page live inside the main window: [`WebEmbed`].
//!
//! ```ignore
//! WebEmbed::new("weekly-report", WebEmbedSpec::new(artifact_id, version_spec))
//!     .height(px(320.))
//!     .pass_scroll(true) // inline in a 话题: the timeline scrolls under the pointer
//!     .on_event(|event, window, cx| { /* Emit, ExternalLink, Loaded … */ })
//! ```
//!
//! The page is a native, isolated WKWebView from `crates/artifact-view`
//! ([`artifact_view::EmbeddedArtifact`]: per-version origin, scheme handler, strict CSP,
//! no network, navigation policy, bridge capabilities). It is a sibling view above GPUI's
//! own view, so every frame the element:
//!
//! - places it from its layout bounds, clipped to the content mask (a scrolled-out part is
//!   cut off; nothing visible hides it), at whole device pixels;
//! - hides it while a GPUI overlay covers it, and draws the page's last snapshot in its
//!   place: an overlay that blocks the mouse (`occlude()`: popovers, menus, dialogs,
//!   sheets) is found by a hit test right after the frame; an overlay that does not can
//!   wrap itself in [`occluder`], which hides the page in the same frame;
//! - follows the light or dark theme (`prefers-color-scheme` in the page);
//! - gives keyboard focus back to GPUI on a click outside the page or on Esc, and blurs
//!   GPUI's focused element when the page takes focus.
//!
//! The view is removed (and its page and media stopped) when the element is no longer
//! rendered: its state is keyed element state.

use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use artifact_view::{ArtifactEvent, Capabilities, VersionSpec};
use gpui::{
    AnyWindowHandle, App, Bounds, Context, DispatchPhase, Element, ElementId, Entity,
    GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, InteractiveElement as _,
    IntoElement, LayoutId, MouseDownEvent, ParentElement, Pixels, Point, RenderOnce, SharedString,
    Style, Styled, Window, WindowId, div, img, point, px, relative,
};

use crate::{
    copy::artifacts as ca,
    theme::{self, Type as _, radius},
};

/// What to show: one version of one 产出.
#[derive(Clone, Debug)]
pub struct WebEmbedSpec {
    pub artifact_id: String,
    pub version: VersionSpec,
    pub capabilities: Capabilities,
}

impl WebEmbedSpec {
    pub fn new(artifact_id: impl Into<String>, version: VersionSpec) -> Self {
        Self {
            artifact_id: artifact_id.into(),
            version,
            capabilities: Capabilities::ALL,
        }
    }

    pub fn capabilities(mut self, caps: Capabilities) -> Self {
        self.capabilities = caps;
        self
    }
}

type EventHandler = Rc<dyn Fn(&ArtifactEvent, &mut Window, &mut App)>;

/// A live 产出 page in the layout. See the module docs.
#[derive(IntoElement)]
pub struct WebEmbed {
    id: ElementId,
    spec: WebEmbedSpec,
    height: Pixels,
    radius: Pixels,
    pass_scroll: bool,
    autoplay: bool,
    on_event: Option<EventHandler>,
}

impl WebEmbed {
    /// `id` keys the native view: keep it stable while the same page shows.
    pub fn new(id: impl Into<ElementId>, spec: WebEmbedSpec) -> Self {
        Self {
            id: id.into(),
            spec,
            height: px(320.),
            radius: radius::LG,
            pass_scroll: false,
            autoplay: false,
            on_event: None,
        }
    }

    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
        self
    }

    pub fn rounded(mut self, r: Pixels) -> Self {
        self.radius = r;
        self
    }

    /// Wheel scrolling goes to the surrounding GPUI list instead of the page.
    pub fn pass_scroll(mut self, pass: bool) -> Self {
        self.pass_scroll = pass;
        self
    }

    /// `<video autoplay>` / `<audio autoplay>` start without a click (keep them muted).
    pub fn autoplay(mut self, autoplay: bool) -> Self {
        self.autoplay = autoplay;
        self
    }

    /// Every page event (loaded, failed, `emitEvent`, external link, focus, closed), on
    /// the main thread with the window.
    pub fn on_event(mut self, f: impl Fn(&ArtifactEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_event = Some(Rc::new(f));
        self
    }
}

/// Load phase of one embedded page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Loading,
    Ready,
    Failed(String),
}

/// The keyed state behind one [`WebEmbed`]: the native view and what the element shows
/// around it.
pub struct WebEmbedState {
    spec: WebEmbedSpec,
    pass_scroll: bool,
    autoplay: bool,
    radius: Pixels,
    on_event: Option<EventHandler>,
    window: Option<AnyWindowHandle>,
    phase: Phase,
    /// The last picture of the page, drawn while the view hides under an overlay.
    snapshot: Option<Arc<gpui::Image>>,
    /// An overlay that blocks the mouse covered the page in the last rendered frame.
    hit_covered: bool,
    /// An [`occluder`] covered the page in this frame.
    cover_covered: bool,
    check_pending: bool,
    focused: bool,
    #[cfg(target_os = "macos")]
    native: Option<artifact_view::EmbeddedArtifact>,
    _events: Option<gpui::Task<()>>,
    /// Bounds of the element in window points, for self-tests.
    last_bounds: Option<Bounds<Pixels>>,
}

thread_local! {
    /// Every live embedded page, for self-tests (load state and where it is).
    static LIVE: RefCell<Vec<gpui::WeakEntity<WebEmbedState>>> = const { RefCell::new(Vec::new()) };
}

impl WebEmbedState {
    fn new(spec: WebEmbedSpec, cx: &mut Context<Self>) -> Self {
        let me = cx.weak_entity();
        LIVE.with(|l| {
            let mut l = l.borrow_mut();
            l.retain(|w| w.upgrade().is_some());
            l.push(me);
        });
        Self {
            spec,
            pass_scroll: false,
            autoplay: false,
            radius: px(0.),
            on_event: None,
            window: None,
            phase: Phase::Loading,
            snapshot: None,
            hit_covered: false,
            cover_covered: false,
            check_pending: false,
            focused: false,
            #[cfg(target_os = "macos")]
            native: None,
            _events: None,
            last_bounds: None,
        }
    }

    pub fn artifact_id(&self) -> &str {
        &self.spec.artifact_id
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    /// The page is hidden under an overlay and its snapshot shows instead.
    pub fn covered(&self) -> bool {
        self.hit_covered || self.cover_covered
    }

    pub fn has_snapshot(&self) -> bool {
        self.snapshot.is_some()
    }

    pub fn focused(&self) -> bool {
        self.focused
    }

    /// True while the native view is on screen.
    pub fn shown(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.native.as_ref().is_some_and(|n| n.is_shown())
        }
        #[cfg(not(target_os = "macos"))]
        false
    }

    pub fn bounds(&self) -> Option<Bounds<Pixels>> {
        self.last_bounds
    }

    /// Moves keyboard focus into the page.
    pub fn focus(&self) {
        #[cfg(target_os = "macos")]
        if let Some(n) = &self.native {
            n.focus();
        }
    }

    /// Runs JavaScript in the page (self-tests and developer tools).
    pub fn evaluate_script(&self, script: &str) {
        #[cfg(target_os = "macos")]
        if let Some(n) = &self.native {
            n.evaluate_script(script);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = script;
    }

    fn sync(&mut self, embed: &WebEmbed) {
        self.pass_scroll = embed.pass_scroll;
        self.autoplay = embed.autoplay;
        self.radius = embed.radius;
        self.on_event = embed.on_event.clone();
        let same = self.spec.artifact_id == embed.spec.artifact_id
            && self.spec.version.version == embed.spec.version.version;
        #[cfg(target_os = "macos")]
        if let Some(n) = &self.native {
            n.set_pass_scroll(embed.pass_scroll);
        }
        if same {
            // The same page, but what its bridge may do can change (the author's member
            // record arrived, or the version's declared events did).
            if self.spec.capabilities != embed.spec.capabilities {
                self.spec.capabilities = embed.spec.capabilities;
                #[cfg(target_os = "macos")]
                if let Some(n) = &self.native {
                    n.set_capabilities(self.spec.capabilities);
                }
            }
            return;
        }
        let new_id = self.spec.artifact_id != embed.spec.artifact_id;
        self.spec = embed.spec.clone();
        self.snapshot = None;
        self.phase = Phase::Loading;
        #[cfg(target_os = "macos")]
        {
            if new_id {
                // A different 产出: a new view (the origin family changes).
                self.native = None;
            } else if let Some(n) = &self.native
                && let Err(e) =
                    n.show_version(self.spec.version.clone(), self.spec.capabilities)
            {
                self.phase = Phase::Failed(e.to_string());
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = new_id;
    }

    fn on_native(&mut self, event: ArtifactEvent, cx: &mut Context<Self>) {
        match &event {
            ArtifactEvent::Loaded { .. } => {
                self.phase = Phase::Ready;
                // Let the page draw, then keep a picture for overlays.
                self.snapshot_later(cx, 250);
            }
            ArtifactEvent::LoadFailed { reason, .. } => {
                tracing::warn!(artifact_id = %self.spec.artifact_id, %reason, "embedded 产出 failed to load");
                self.phase = Phase::Failed(reason.clone());
            }
            ArtifactEvent::FocusChanged { focused } => {
                self.focused = *focused;
                if *focused {
                    // The page has the keyboard: GPUI's field gives up its caret.
                    if let Some(handle) = self.window {
                        cx.defer(move |cx| {
                            let _ = handle.update(cx, |_, window, cx| window.blur(cx));
                        });
                    }
                } else {
                    self.snapshot_later(cx, 0);
                }
            }
            ArtifactEvent::Emit { .. } => self.snapshot_later(cx, 120),
            ArtifactEvent::ExternalLink { .. }
            | ArtifactEvent::VersionSelected { .. }
            | ArtifactEvent::Closed => {}
        }
        cx.notify();
        if let (Some(handler), Some(handle)) = (self.on_event.clone(), self.window) {
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, cx| handler(&event, window, cx));
            });
        }
    }

    /// Takes a new snapshot after `delay_ms` (only while the page is on screen).
    fn snapshot_later(&self, cx: &mut Context<Self>, delay_ms: u64) {
        #[cfg(target_os = "macos")]
        cx.spawn(async move |this, cx| {
            if delay_ms > 0 {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(delay_ms))
                    .await;
            }
            let (tx, rx) = futures::channel::oneshot::channel();
            let started = this
                .update(cx, |s, _| match &s.native {
                    Some(n) if n.is_shown() => {
                        n.snapshot_png(move |png| {
                            let _ = tx.send(png);
                        });
                        true
                    }
                    _ => false,
                })
                .unwrap_or(false);
            if !started {
                return;
            }
            if let Ok(Some(png)) = rx.await {
                let image = Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, png));
                let _ = this.update(cx, |s, cx| {
                    s.snapshot = Some(image);
                    cx.notify();
                });
            }
        })
        .detach();
        #[cfg(not(target_os = "macos"))]
        let _ = (cx, delay_ms);
    }

    /// Creates the native view on the first paint.
    #[cfg(target_os = "macos")]
    fn attach(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.native.is_some() || matches!(self.phase, Phase::Failed(_)) {
            return;
        }
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let Some(mtm) = objc2_foundation::MainThreadMarker::new() else {
            return;
        };
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
            return;
        };
        // SAFETY: GPUI's AppKit handle points at the window's live content NSView, and paint
        // runs on the main thread.
        let Some(host) = (unsafe { artifact_view::view_from_ptr(appkit.ns_view.as_ptr()) }) else {
            return;
        };
        self.window = Some(Window::window_handle(window));
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<ArtifactEvent>();
        let mut options = artifact_view::EmbedOptions::new(
            self.spec.artifact_id.clone(),
            self.spec.version.clone(),
        );
        options.capabilities = self.spec.capabilities;
        options.dark = theme::is_dark(cx);
        options.corner_radius = f32::from(self.radius) as f64;
        options.pass_scroll = self.pass_scroll;
        options.autoplay = self.autoplay;
        options.inspectable = cfg!(debug_assertions);
        match artifact_view::EmbeddedArtifact::attach(mtm, host, options, move |e| {
            let _ = tx.unbounded_send(e);
        }) {
            Ok(native) => {
                self.native = Some(native);
                self._events = Some(cx.spawn(async move |this, cx| {
                    use futures::StreamExt as _;
                    while let Some(event) = rx.next().await {
                        if this.update(cx, |s, cx| s.on_native(event, cx)).is_err() {
                            break;
                        }
                    }
                }));
            }
            Err(e) => self.phase = Phase::Failed(e.to_string()),
        }
    }

    /// Applies this frame's bounds, theme and covers to the native view.
    fn apply(
        &mut self,
        bounds: Bounds<Pixels>,
        visible: Bounds<Pixels>,
        hitbox: Hitbox,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.last_bounds = Some(bounds);
        self.cover_covered = covers_overlap(window, visible, cx);
        #[cfg(target_os = "macos")]
        {
            self.attach(window, cx);
            let covered = self.covered();
            if let Some(n) = &self.native {
                n.set_dark(theme::is_dark(cx));
                n.set_frame(rect(bounds), rect(visible), window.scale_factor() as f64);
                n.set_hidden(covered);
            }
        }
        // Right after this frame shows, test whether an overlay that blocks the mouse lies
        // over the page (its hitbox is not the top one at a sample point).
        if !self.check_pending {
            self.check_pending = true;
            let me = cx.weak_entity();
            let samples = sample_points(visible);
            window.on_next_frame(move |window, cx| {
                let covered = !samples.is_empty()
                    && samples.iter().any(|p| !hitbox.is_hovered_at(*p, window));
                let _ = me.update(cx, |s, cx| {
                    s.check_pending = false;
                    if s.hit_covered != covered {
                        s.hit_covered = covered;
                        #[cfg(target_os = "macos")]
                        if let Some(n) = &s.native {
                            n.set_hidden(s.covered());
                        }
                        cx.notify();
                    }
                });
            });
        }
    }
}

/// Points spread over the visible part (corners inset by 2 pt, edges and a 3 x 3 grid).
fn sample_points(visible: Bounds<Pixels>) -> Vec<Point<Pixels>> {
    if visible.size.width <= px(4.) || visible.size.height <= px(4.) {
        return Vec::new();
    }
    let inset = px(2.);
    let x0 = visible.origin.x + inset;
    let y0 = visible.origin.y + inset;
    let w = visible.size.width - inset * 2.;
    let h = visible.size.height - inset * 2.;
    let mut out = Vec::with_capacity(25);
    for i in 0..5 {
        for j in 0..5 {
            out.push(point(x0 + w * (i as f32 / 4.), y0 + h * (j as f32 / 4.)));
        }
    }
    out
}

#[cfg(target_os = "macos")]
fn rect(b: Bounds<Pixels>) -> artifact_view::Rect {
    artifact_view::Rect::new(
        f32::from(b.origin.x) as f64,
        f32::from(b.origin.y) as f64,
        f32::from(b.size.width) as f64,
        f32::from(b.size.height) as f64,
    )
}

impl RenderOnce for WebEmbed {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let spec = self.spec.clone();
        let state =
            window.use_keyed_state(self.id.clone(), cx, |_, cx| WebEmbedState::new(spec, cx));
        state.update(cx, |s, _| s.sync(&self));
        let k = theme::colors(cx);
        let s = state.read(cx);
        let backdrop = match (&s.phase, s.covered(), &s.snapshot) {
            (_, true, Some(image)) => img(image.clone()).size_full().into_any_element(),
            (Phase::Failed(_), _, _) => message(ca::LOAD_FAILED, k.text_3).into_any_element(),
            (Phase::Loading, _, _) => message(ca::OPENING, k.text_3).into_any_element(),
            _ => div().into_any_element(),
        };
        div()
            .id(self.id.clone())
            .relative()
            .w_full()
            .h(self.height)
            .flex_none()
            .rounded(self.radius)
            .overflow_hidden()
            .bg(k.raised)
            .border_1()
            .border_color(k.border)
            .child(backdrop)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .child(NativeSlot { state }),
            )
    }
}

fn message(text: &'static str, color: gpui::Hsla) -> gpui::Div {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .t_ui()
        .text_color(color)
        .child(SharedString::new_static(text))
}

/// The element that places the native view. It draws nothing itself.
struct NativeSlot {
    state: Entity<WebEmbedState>,
}

impl IntoElement for NativeSlot {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for NativeSlot {
    type RequestLayoutState = ();
    type PrepaintState = (Hitbox, Bounds<Pixels>);

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> (Hitbox, Bounds<Pixels>) {
        begin_frame(window, cx);
        let visible = bounds.intersect(&window.content_mask().bounds);
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        (hitbox, visible)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        (hitbox, visible): &mut (Hitbox, Bounds<Pixels>),
        window: &mut Window,
        cx: &mut App,
    ) {
        mark_painting(window, cx);
        let (hitbox, visible) = (hitbox.clone(), *visible);
        self.state
            .update(cx, |s, cx| s.apply(bounds, visible, hitbox, window, cx));
        // A GPUI click never lands on the page (AppKit sends those to the web view), so
        // any GPUI mouse-down takes the keyboard back from the page.
        let weak = self.state.downgrade();
        window.on_mouse_event(move |_: &MouseDownEvent, phase, _, cx| {
            if phase != DispatchPhase::Capture {
                return;
            }
            let _ = weak.update(cx, |_s, _| {
                #[cfg(target_os = "macos")]
                if let Some(n) = &_s.native {
                    n.resign_focus();
                }
            });
        });
    }
}

// --- covers -----------------------------------------------------------------

/// Overlay bounds registered in this frame, per window.
#[derive(Default)]
struct Covers {
    windows: HashMap<WindowId, CoverFrame>,
}

#[derive(Default)]
struct CoverFrame {
    painting: bool,
    rects: Vec<Bounds<Pixels>>,
}

impl gpui::Global for Covers {}

/// The first prepaint after a paint starts a new frame: drop the last frame's covers.
fn begin_frame(window: &Window, cx: &mut App) {
    let id = window.window_handle().window_id();
    let frame = cx.default_global::<Covers>().windows.entry(id).or_default();
    if frame.painting {
        frame.painting = false;
        frame.rects.clear();
    }
}

fn mark_painting(window: &Window, cx: &mut App) {
    let id = window.window_handle().window_id();
    cx.default_global::<Covers>()
        .windows
        .entry(id)
        .or_default()
        .painting = true;
}

fn covers_overlap(window: &Window, visible: Bounds<Pixels>, cx: &App) -> bool {
    let id = window.window_handle().window_id();
    cx.try_global::<Covers>()
        .and_then(|c| c.windows.get(&id))
        .is_some_and(|f| f.rects.iter().any(|r| r.intersects(&visible)))
}

/// Wrap an overlay that does not block the mouse (or that must hide the page in the same
/// frame) with this: put `occluder()` as an absolutely positioned child that fills the
/// overlay panel. Every embedded page under its bounds hides while it is drawn.
pub fn occluder() -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .child(Occluder)
}

struct Occluder;

impl IntoElement for Occluder {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Occluder {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if LIVE.with(|l| l.borrow().is_empty()) {
            return;
        }
        begin_frame(window, cx);
        let visible = bounds.intersect(&window.content_mask().bounds);
        let id = window.window_handle().window_id();
        let frame = cx.default_global::<Covers>().windows.entry(id).or_default();
        if frame.rects.len() >= 64 {
            frame.rects.remove(0);
        }
        frame.rects.push(visible);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }
}

// --- self-test hooks --------------------------------------------------------

/// The live embedded pages (self-tests).
pub fn live(cx: &App) -> Vec<Entity<WebEmbedState>> {
    let _ = cx;
    LIVE.with(|l| l.borrow().iter().filter_map(|w| w.upgrade()).collect())
}
