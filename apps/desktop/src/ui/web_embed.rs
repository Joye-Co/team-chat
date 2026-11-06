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
//! The page stays alive while it scrolls out of a virtualized list (inc-host track): the
//! state is cached per window and element id, an embed that was not painted in a frame
//! hides, and at most [`MAX_LIVE`] pages keep a native view; the least recently painted
//! off-screen ones give theirs up and show their last snapshot when they come back, while
//! the page loads again.

use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use artifact_view::{ArtifactEvent, Capabilities, RefMenu, Reject, Snapshot, VersionSpec, Viewer};
use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, DispatchPhase, Element, ElementId,
    Entity, GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, InteractiveElement as _,
    IntoElement, LayoutId, MouseDownEvent, ParentElement, Pixels, Point, RenderOnce, SharedString,
    Style, Styled, Window, WindowId, div, img, point, prelude::FluentBuilder as _, px, relative,
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
    /// `window.teamchat.viewer`.
    pub viewer: Option<Viewer>,
    /// The ⌥-click outline and `就这里提问` menu, when the page may `ref`.
    pub ref_menu: Option<RefMenu>,
}

impl WebEmbedSpec {
    pub fn new(artifact_id: impl Into<String>, version: VersionSpec) -> Self {
        Self {
            artifact_id: artifact_id.into(),
            version,
            capabilities: Capabilities::ALL,
            viewer: None,
            ref_menu: None,
        }
    }

    pub fn capabilities(mut self, caps: Capabilities) -> Self {
        self.capabilities = caps;
        self
    }

    pub fn viewer(mut self, viewer: Option<Viewer>) -> Self {
        self.viewer = viewer;
        self
    }

    pub fn ref_menu(mut self, menu: Option<RefMenu>) -> Self {
        self.ref_menu = menu;
        self
    }
}

type EventHandler = Rc<dyn Fn(&ArtifactEvent, &mut Window, &mut App)>;
/// The 产出 page host's hook: every page event with the embed it came from.
pub type HostHandler =
    Rc<dyn Fn(&ArtifactEvent, gpui::WeakEntity<WebEmbedState>, &mut Window, &mut App)>;

/// Pages that keep a native view at once, beyond the ones on screen (INTERACT §5.5,
/// MEDIA §1: at most 2 live; off-screen ones show their snapshot).
pub const MAX_LIVE: usize = 2;
/// Cached embeds (with or without a native view) kept per app.
const MAX_CACHED: usize = 24;

/// A live 产出 page in the layout. See the module docs.
#[derive(IntoElement)]
pub struct WebEmbed {
    id: ElementId,
    spec: WebEmbedSpec,
    height: Pixels,
    /// Fill the parent's height instead of [`WebEmbed::height`].
    fill: bool,
    /// Draw the card frame (1 px border on the card fill) around the page.
    framed: bool,
    radius: Pixels,
    pass_scroll: bool,
    autoplay: bool,
    /// The page's side and top padding in the host base stylesheet (`--tc-pad`).
    pad: Pixels,
    /// Its snapshots stand for the 产出 version in thumbnails ([`page_preview`]).
    remember: bool,
    on_event: Option<EventHandler>,
    on_host: Option<HostHandler>,
}

impl WebEmbed {
    /// `id` keys the native view: keep it stable while the same page shows.
    pub fn new(id: impl Into<ElementId>, spec: WebEmbedSpec) -> Self {
        Self {
            id: id.into(),
            spec,
            height: px(320.),
            fill: false,
            framed: false,
            radius: radius::LG,
            pass_scroll: false,
            autoplay: false,
            pad: crate::ui::native::tokens::space::S24,
            remember: true,
            on_event: None,
            on_host: None,
        }
    }

    /// A page that is not the 产出's own (a self-test probe under the 产出's id):
    /// its snapshots never stand for the version in thumbnails.
    pub fn forget_preview(mut self) -> Self {
        self.remember = false;
        self
    }

    /// The page's padding in the base stylesheet (`--tc-pad`, 24 by default; 16 in
    /// a 400 pane, §2.5). A page that sets its own padding keeps it.
    pub fn pad(mut self, pad: Pixels) -> Self {
        self.pad = pad;
        self
    }

    /// Routes page events to the 产出 page host with this embed (bridge requests, loads).
    pub fn on_host(
        mut self,
        f: impl Fn(&ArtifactEvent, gpui::WeakEntity<WebEmbedState>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_host = Some(Rc::new(f));
        self
    }

    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
        self
    }

    pub fn rounded(mut self, r: Pixels) -> Self {
        self.radius = r;
        self
    }

    /// The page fills its parent (a page's content area) instead of a fixed height.
    pub fn fill(mut self) -> Self {
        self.fill = true;
        self
    }

    /// A card frame around the page (COMPLETENESS.md §1.7, §2.4): a 1 px border on
    /// the card fill in both themes. Leave it off inside a card that has its own frame.
    pub fn framed(mut self) -> Self {
        self.framed = true;
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
    pad: Pixels,
    /// The page fills a content area (on the background), not a card.
    fill: bool,
    remember: bool,
    on_event: Option<EventHandler>,
    on_host: Option<HostHandler>,
    window: Option<AnyWindowHandle>,
    /// Not painted in the last frame: hidden, kept for when it scrolls back.
    parked: bool,
    phase: Phase,
    /// The last picture of the page, drawn while the view hides under an overlay.
    snapshot: Option<Arc<gpui::Image>>,
    /// An overlay that blocks the mouse covered the page in the last rendered frame.
    hit_covered: bool,
    /// An [`occluder`] covered the page in this frame.
    cover_covered: bool,
    check_pending: bool,
    focused: bool,
    /// The frame color the inline page's cut edge fades into, as last given to the page.
    fade: Option<String>,
    /// Where an inline page ends on a whole block (its own height when it fits): the
    /// frame shrinks to it, so no blank band sits under the cover.
    end: Option<Pixels>,
    /// The host base stylesheet given to the page, by its token values.
    base: Option<String>,
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
            pad: crate::ui::native::tokens::space::S24,
            fill: false,
            remember: true,
            on_event: None,
            on_host: None,
            window: None,
            parked: false,
            phase: Phase::Loading,
            snapshot: None,
            hit_covered: false,
            cover_covered: false,
            check_pending: false,
            focused: false,
            fade: None,
            end: None,
            base: None,
            #[cfg(target_os = "macos")]
            native: None,
            _events: None,
            last_bounds: None,
        }
    }

    pub fn artifact_id(&self) -> &str {
        &self.spec.artifact_id
    }

    /// The version the page shows.
    pub fn version(&self) -> u32 {
        self.spec.version.version
    }

    /// True while a native page exists (on screen or parked).
    pub fn has_native(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.native.is_some()
        }
        #[cfg(not(target_os = "macos"))]
        false
    }

    /// Gives the page of `version` a data snapshot (the 产出 page host). False when no
    /// such page shows.
    pub fn push_data(&self, version: u32, snap: Snapshot) -> bool {
        #[cfg(target_os = "macos")]
        if let Some(n) = &self.native {
            return n.push_data(version, snap);
        }
        let _ = (version, snap);
        false
    }

    /// Finishes an open `emit` / `ask` of the page of `version`. False when it is not
    /// open any more (the host drops a late reply).
    pub fn finish(
        &self,
        version: u32,
        reply_id: &serde_json::Value,
        result: Result<serde_json::Value, Reject>,
    ) -> bool {
        #[cfg(target_os = "macos")]
        if let Some(n) = &self.native {
            return n.finish(version, reply_id, result);
        }
        let _ = (version, reply_id, result);
        false
    }

    /// Hides a page that was not painted in the last frame.
    fn park(&mut self) {
        if self.parked {
            return;
        }
        self.parked = true;
        self.focused = false;
        #[cfg(target_os = "macos")]
        if let Some(n) = &self.native {
            n.set_hidden(true);
        }
    }

    /// Gives up the native page (it loads again when it shows); the snapshot stays.
    fn release_native(&mut self) {
        #[cfg(target_os = "macos")]
        if self.native.take().is_some() {
            self._events = None;
            self.phase = Phase::Loading;
        }
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

    /// Runs JavaScript in the page and gives its string value (self-tests). `done` runs
    /// once, with `None` when there is no page or the value is not a string.
    pub fn evaluate_text(&self, script: &str, done: impl FnOnce(Option<String>) + 'static) {
        #[cfg(target_os = "macos")]
        if let Some(n) = &self.native {
            n.evaluate_text(script, done);
            return;
        }
        let _ = script;
        done(None);
    }

    /// Posts a real left click (mouse-down, mouse-up) through the app's event queue at a
    /// point in the page (points from its top-left corner), or with `in_page` false at a
    /// point in the window's content. Self-tests only. False when it could not be posted.
    pub fn post_click(&self, x: f64, y: f64, in_page: bool) -> bool {
        #[cfg(target_os = "macos")]
        if let Some(n) = &self.native {
            return if in_page {
                n.post_click(x, y)
            } else {
                n.post_window_click(x, y)
            };
        }
        let _ = (x, y, in_page);
        false
    }

    fn sync(&mut self, embed: &WebEmbed) {
        self.pass_scroll = embed.pass_scroll;
        self.autoplay = embed.autoplay;
        self.radius = embed.radius;
        self.pad = embed.pad;
        self.fill = embed.fill;
        self.remember = embed.remember;
        self.on_event = embed.on_event.clone();
        self.on_host = embed.on_host.clone();
        self.spec.viewer = embed.spec.viewer.clone();
        self.spec.ref_menu = embed.spec.ref_menu.clone();
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
                && let Err(e) = n.show_version(self.spec.version.clone(), self.spec.capabilities)
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
                // A new document: it has no fade and no base stylesheet yet.
                self.fade = None;
                self.end = None;
                self.base = None;
                self.apply_base(cx);
                // Let the page draw, then keep a picture for overlays.
                self.snapshot_later(cx, 250);
                self.measure_end_later(cx, 300);
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
            ArtifactEvent::Emit { .. } | ArtifactEvent::Ask { .. } => {
                self.snapshot_later(cx, 120);
                self.measure_end_later(cx, 150);
            }
            ArtifactEvent::ExternalLink { .. }
            | ArtifactEvent::VersionSelected { .. }
            | ArtifactEvent::Ref { .. }
            | ArtifactEvent::Closed => {}
        }
        cx.notify();
        if let (Some(host), Some(handle)) = (self.on_host.clone(), self.window) {
            let me = cx.weak_entity();
            let event = event.clone();
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, cx| host(&event, me, window, cx));
            });
        }
        if let (Some(handler), Some(handle)) = (self.on_event.clone(), self.window) {
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, window, cx| handler(&event, window, cx));
            });
        }
    }

    /// An inline page (wheel scrolling passes to the list) starts at its top and is cut at
    /// the frame's height: while more of the page lies below, it ends on the last whole
    /// block above the frame edge (a solid cover in the frame's surface hides the rest),
    /// never under a fade or mid-glyph (§2.21, A4). The cover is a click-through layer the
    /// page cannot see in its layout; it follows the page's scroll position, size and
    /// content, and the theme.
    fn apply_fade(&mut self, cx: &App) {
        if !self.pass_scroll || self.phase != Phase::Ready {
            return;
        }
        let c = gpui::Rgba::from(theme::colors(cx).raised);
        let rgb = format!(
            "{},{},{}",
            (c.r * 255.).round() as u8,
            (c.g * 255.).round() as u8,
            (c.b * 255.).round() as u8
        );
        if self.fade.as_deref() == Some(rgb.as_str()) {
            return;
        }
        #[cfg(target_os = "macos")]
        if let Some(n) = &self.native {
            n.evaluate_script(&fade_script(&rgb));
            self.fade = Some(rgb);
        }
    }

    /// Gives the page the host's base stylesheet (COMPLETENESS.md §2.22, F14): the app
    /// font at 14/22, text and lines in the app's tokens, table lines, 24 padding, and
    /// the tokens as CSS variables (`--tc-fg`, `--tc-muted`, `--tc-border`, `--tc-fill`,
    /// `--tc-primary`, `--tc-bg`). It sits in a cascade layer, so any rule of the page
    /// wins over it; a page with no styles reads like the app in light and dark. It
    /// follows the theme.
    fn apply_base(&mut self, cx: &App) {
        if self.phase != Phase::Ready {
            return;
        }
        let css = base_css(self.pad, self.fill, cx);
        if self.base.as_deref() == Some(css.as_str()) {
            return;
        }
        #[cfg(target_os = "macos")]
        if let Some(n) = &self.native {
            n.evaluate_script(&base_script(&css));
            self.base = Some(css);
        }
    }

    /// Reads, after `delay_ms`, where an inline page ends on a whole block
    /// (`window.__tcEnd`, kept by the cover script; a heading is never the
    /// last block: it goes with the block it heads, §2.21) and shrinks the frame to it.
    fn measure_end_later(&self, cx: &mut Context<Self>, delay_ms: u64) {
        if !self.pass_scroll {
            return;
        }
        #[cfg(target_os = "macos")]
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(delay_ms))
                .await;
            let (tx, rx) = futures::channel::oneshot::channel();
            let started = this
                .update(cx, |s, _| match &s.native {
                    Some(n) => {
                        n.evaluate_text("String(window.__tcEnd||'')", move |v| {
                            let _ = tx.send(v);
                        });
                        true
                    }
                    None => false,
                })
                .unwrap_or(false);
            if !started {
                return;
            }
            let end = rx
                .await
                .ok()
                .flatten()
                .and_then(|v| v.parse::<f32>().ok())
                .filter(|v| *v >= MIN_INLINE_END);
            if let Some(end) = end {
                let _ = this.update(cx, |s, cx| {
                    s.end = Some(px(end.ceil()));
                    cx.notify();
                });
            }
        })
        .detach();
        #[cfg(not(target_os = "macos"))]
        let _ = (cx, delay_ms);
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
                    if s.remember {
                        remember_preview(s.artifact_id(), s.version(), image.clone());
                    }
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
        options.viewer = self.spec.viewer.clone();
        options.ref_menu = self.spec.ref_menu.clone();
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
        self.parked = false;
        // A float band over the top of the region (the sticky date pill) stays
        // above the page: the page shows only below it (§2.17, A4).
        let visible = below_bands(window, visible, cx);
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
        self.apply_fade(cx);
        self.apply_base(cx);
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

/// A CSS color: `#rrggbb`, or `rgba(…)` for a token with alpha (dark lines are
/// white at a low alpha).
fn hex(c: gpui::Hsla) -> String {
    let c = gpui::Rgba::from(c);
    let b = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
    if c.a < 0.999 {
        format!("rgba({},{},{},{:.3})", b(c.r), b(c.g), b(c.b), c.a)
    } else {
        format!("#{:02x}{:02x}{:02x}", b(c.r), b(c.g), b(c.b))
    }
}

/// `top` composited over the opaque `under`.
fn over(top: gpui::Hsla, under: gpui::Hsla) -> gpui::Hsla {
    let (t, u) = (gpui::Rgba::from(top), gpui::Rgba::from(under));
    let mix = |a: f32, b: f32| a * t.a + b * (1. - t.a);
    gpui::Rgba {
        r: mix(t.r, u.r),
        g: mix(t.g, u.g),
        b: mix(t.b, u.b),
        a: 1.,
    }
    .into()
}

/// The host base stylesheet in this theme's token values (see
/// [`WebEmbedState::apply_base`]). It only supplies defaults (owner decision
/// O-4): its rules sit in the `tc-base` cascade layer, so any color or type
/// a page that a member made sets for itself wins.
fn base_css(pad: Pixels, fill: bool, cx: &App) -> String {
    let k = crate::ui::native::native_colors(cx);
    // The surface the page sits on: the background when it fills a content
    // area, else its card's fill (see `WebEmbed::render`).
    let surface = if fill {
        k.content
    } else {
        theme::colors(cx).raised
    };
    let scheme = if theme::is_dark(cx) { "dark" } else { "light" };
    format!(
        ":root{{--tc-pad:{pad}px;--tc-bg:{bg};--tc-fg:{fg};--tc-muted:{muted};--tc-border:{border};\
--tc-fill:{fill};--tc-primary:{fg};color-scheme:{scheme}}}\
@layer tc-base{{\
html{{font:14px/22px -apple-system,BlinkMacSystemFont,'PingFang SC','Helvetica Neue',sans-serif;\
color:var(--tc-fg);background:transparent;-webkit-font-smoothing:antialiased}}\
body{{margin:0;padding:var(--tc-pad)}}\
h1{{font-size:24px;line-height:32px;font-weight:600;margin:0 0 8px}}\
h2{{font-size:18px;line-height:28px;font-weight:600;margin:24px 0 8px}}\
h3{{font-size:16px;line-height:24px;font-weight:600;margin:16px 0 8px}}\
p{{margin:0 0 12px}}small{{color:var(--tc-muted)}}\
a{{color:inherit;text-underline-offset:2px}}\
table{{border-collapse:collapse;width:100%;margin:8px 0 16px;font-variant-numeric:tabular-nums}}\
th,td{{border-bottom:1px solid var(--tc-border);padding:8px 12px;text-align:start}}\
[data-tc-num]{{text-align:right!important}}\
th{{font-size:12px;font-weight:500;color:var(--tc-muted)}}\
hr{{border:0;border-top:1px solid var(--tc-border);margin:16px 0}}\
code,pre{{font-family:ui-monospace,'SF Mono',Menlo,monospace;font-size:13px}}\
pre{{background:var(--tc-fill);border:1px solid var(--tc-border);border-radius:8px;padding:12px;overflow:auto}}}}",
        pad = f32::from(pad).round(),
        bg = hex(k.content),
        fg = hex(k.label),
        muted = hex(k.secondary),
        // Opaque on the page's own surface (the card fill inline): a line
        // the page draws twice (a row and its cells) keeps the token's value
        // and its contrast on that surface (§2.22, D2, D4).
        border = hex(over(k.separator, surface)),
        fill = hex(k.fill),
    )
}

/// Marks every table column whose body cells all hold a number (`1,200`,
/// `¥3.2`, `+12%`, `−0.3 秒`) with `data-tc-num`, header included, so the
/// base stylesheet right-aligns it (§2.21: numbers right-aligned in columns).
/// It runs again when the page changes its tables (live data).
const NUMERIC_COLUMNS: &str = "var tcNum=function(){var re=/^[\\s+\\-\\u2212\\u00a5$\\u20ac]*\\d[\\d.,]*\\s*(%|\\S{0,2})$/;\
document.querySelectorAll('table').forEach(function(t){var rows=[].slice.call(t.rows);\
var head=function(r){return r.cells.length>0&&[].every.call(r.cells,function(c){return c.tagName==='TH';});};\
var body=rows.filter(function(r){return !head(r);}),heads=rows.filter(head);\
var n=0;body.forEach(function(r){n=Math.max(n,r.cells.length)});\
for(var i=0;i<n;i++){var any=false,all=true;body.forEach(function(r){var c=r.cells[i];if(!c)return;var v=c.textContent.trim();if(!v||v==='\\u2014'||v==='\\u2013'||v==='-')return;any=true;if(!re.test(v))all=false;});\
if(any&&all){rows.forEach(function(r){if(r.cells[i])r.cells[i].setAttribute('data-tc-num','');});}}});};\
tcNum();if(!window.__tcNumObs&&window.MutationObserver&&document.body){var p=0;window.__tcNumObs=new MutationObserver(function(){if(p)return;p=1;setTimeout(function(){p=0;tcNum();},50);});\
window.__tcNumObs.observe(document.body,{childList:true,subtree:true,characterData:true});}";

/// The script that puts the base stylesheet first in a page's `<head>` (or replaces it).
fn base_script(css: &str) -> String {
    let css = serde_json::to_string(css).unwrap_or_else(|_| "\"\"".into());
    format!(
        "(function(){{var s=document.getElementById('__tc_base');\
if(!s){{s=document.createElement('style');s.id='__tc_base';\
var h=document.head||document.documentElement;h.insertBefore(s,h.firstChild);}}\
s.textContent={css};{num}}})()",
        num = NUMERIC_COLUMNS
    )
}

/// The script that ends an inline page on a block boundary (or recolors the
/// cover): `rgb` is `r,g,b`, the frame's surface. While more of the page lies
/// below the frame, a solid cover in the surface color hides everything from
/// the top of the first block the frame edge would cut (a line of text, a
/// table row, a small card, a chart, an image) down to the edge, so the page
/// ends on whole blocks and never under a fade or mid-glyph (§2.21, A4); the
/// frame's `展开` row follows. The frame ends 16 (the card gap) under the
/// bottom of the last whole block, so the margin between blocks never shows
/// as a blank band above the footer (A1). It keeps the id `__tc_fade` the
/// self-tests read.
fn fade_script(rgb: &str) -> String {
    format!(
        "(function(){{var d=document.getElementById('__tc_fade');\
if(!d){{d=document.createElement('div');d.id='__tc_fade';d.setAttribute('aria-hidden','true');\
var s=d.style;s.position='fixed';s.left='0';s.right='0';s.bottom='0';s.height='0px';\
s.pointerEvents='none';s.zIndex='2147483647';s.margin='0';s.padding='0';\
document.documentElement.appendChild(d);\
var atomic=function(el,r){{if(!el.firstElementChild)return true;var t=el.tagName.toUpperCase();\
if(/^(SVG|IMG|CANVAS|VIDEO|AUDIO|IFRAME|TR|LI|BUTTON|PRE|P|H1|H2|H3|H4|H5|H6|DT|DD|FIGURE|INPUT|SELECT|TEXTAREA|CODE)$/.test(t))return true;\
if(r.height<=160){{var c=getComputedStyle(el);if(parseFloat(c.borderTopWidth)>0||parseFloat(c.borderBottomWidth)>0||\
(c.backgroundColor&&c.backgroundColor!=='rgba(0, 0, 0, 0)'&&c.backgroundColor!=='transparent'))return true;}}return false;}};\
var u=function(){{var e=document.scrollingElement||document.documentElement,h=window.innerHeight;\
var more=e.scrollHeight-(window.scrollY+h)>1;if(!more){{d.style.opacity='0';d.style.height='0px';var b=document.body,bs=b?getComputedStyle(b):null;var end=b?Math.ceil(b.getBoundingClientRect().bottom+parseFloat(bs.marginBottom||0)):h;\
if(b){{var lw=-1,ds=b.getElementsByTagName('*');for(var i=0;i<ds.length;i++){{if(ds[i]===d)continue;var q=ds[i].getBoundingClientRect();\
if(q.height>0&&q.width>0&&q.bottom>lw)lw=q.bottom;}}if(lw>0&&Math.ceil(lw+16)<end)end=Math.ceil(lw+16);}}\
window.__tcEnd=end;return;}}\
var cut=h,all=document.body?document.body.getElementsByTagName('*'):[];\
for(var pass=0;pass<6;pass++){{var top=cut;for(var i=0;i<all.length;i++){{var el=all[i];if(el===d)continue;\
var r=el.getBoundingClientRect();if(r.height<=0||r.width<=0||r.top>=cut||r.bottom<=cut)continue;\
if(atomic(el,r)&&r.top<top)top=r.top;}}if(top>=cut)break;cut=top;}}\
var hs=document.body?document.body.querySelectorAll('h1,h2,h3,h4,h5,h6,figcaption'):[],lh=null,lb=-1;\
for(var j=0;j<hs.length;j++){{var q=hs[j].getBoundingClientRect();if(q.height>0&&q.bottom<=cut&&q.bottom>lb){{lb=q.bottom;lh=q;}}}}\
if(lh){{var after=false;for(var i=0;i<all.length;i++){{var el=all[i];if(el===d||/^(H[1-6]|FIGCAPTION)$/.test(el.tagName.toUpperCase()))continue;\
var r=el.getBoundingClientRect();if(r.height>0&&r.width>0&&r.top>=lb-1&&r.bottom<=cut&&atomic(el,r)){{after=true;break;}}}}if(!after)cut=lh.top;}}\
if(cut<h*0.35)cut=h;\
if(cut<h){{var lz=-1;for(var i=0;i<all.length;i++){{var el=all[i];if(el===d)continue;var r=el.getBoundingClientRect();\
if(r.height>0&&r.width>0&&r.bottom<=cut&&r.bottom>lz)lz=r.bottom;}}if(lz>0&&lz+16<cut)cut=lz+16;}}\
window.__tcEnd=Math.floor(cut);d.style.height=Math.max(0,Math.ceil(h-cut))+'px';d.style.opacity='1';}};\
window.addEventListener('scroll',u,{{passive:true}});window.addEventListener('resize',u);\
try{{var o=new ResizeObserver(u);o.observe(document.documentElement);\
if(document.body)o.observe(document.body);}}catch(_){{}}\
try{{var m=new MutationObserver(function(){{u();}});m.observe(document.body,{{childList:true,subtree:true,characterData:true}});}}catch(_){{}}\
d.__tcUpdate=u;}}\
d.style.background='rgb({rgb})';\
d.__tcUpdate();}})()"
    )
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

/// The shortest an inline page's frame gets when it ends early.
const MIN_INLINE_END: f32 = 120.;

impl RenderOnce for WebEmbed {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = cached_state(&self, window, cx);
        state.update(cx, |s, _| s.sync(&self));
        let k = theme::colors(cx);
        let s = state.read(cx);
        // An inline page ends on its last whole block (or its own end).
        let height = match s.end {
            Some(end) if self.pass_scroll => end.min(self.height),
            _ => self.height,
        };
        let backdrop = match (&s.phase, s.covered(), &s.snapshot) {
            (_, true, Some(image)) | (Phase::Loading, _, Some(image)) => {
                img(image.clone()).size_full().into_any_element()
            }
            (Phase::Failed(_), _, _) => message(ca::LOAD_FAILED, k.text_3).into_any_element(),
            (Phase::Loading, _, _) => message(ca::OPENING, k.text_3).into_any_element(),
            _ => div().into_any_element(),
        };
        let n = crate::ui::native::native_colors(cx);
        div()
            .id(self.id.clone())
            .relative()
            .w_full()
            .when(self.fill, |d| d.flex_1().h_full().min_h(px(0.)))
            .when(!self.fill, |d| d.h(height).flex_none())
            .rounded(self.radius)
            .overflow_hidden()
            // A page that fills a content area sits on the app background; inline it
            // sits on its card's fill. `framed` adds the card's 1 px line (§1.7).
            .bg(if self.fill { n.content } else { k.raised })
            .when(self.framed, |d| {
                d.border_1().border_color(n.separator).bg(n.sheet)
            })
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
        mark_painted(&self.state, window, cx);
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
    /// Floating bands (the sticky date pill) the page is cut below.
    bands: Vec<Bounds<Pixels>>,
    /// Frames of this window so far (the embed cache's clock).
    count: u64,
}

impl gpui::Global for Covers {}

/// The first prepaint after a paint starts a new frame: drop the last frame's covers.
fn begin_frame(window: &Window, cx: &mut App) {
    let id = window.window_handle().window_id();
    let frame = cx.default_global::<Covers>().windows.entry(id).or_default();
    if frame.painting {
        frame.painting = false;
        frame.rects.clear();
        frame.bands.clear();
        frame.count += 1;
    }
}

// --- the live embed cache (inc-host track) ----------------------------------------

struct CacheEntry {
    state: Entity<WebEmbedState>,
    last_paint: u64,
}

#[derive(Default)]
struct EmbedCache {
    entries: HashMap<(WindowId, ElementId), CacheEntry>,
    /// Windows with a sweep queued for the end of the frame.
    sweeping: std::collections::HashSet<WindowId>,
}

impl gpui::Global for EmbedCache {}

/// The state of one embed, kept across frames in which a virtualized list does not
/// render it.
fn cached_state(embed: &WebEmbed, window: &mut Window, cx: &mut App) -> Entity<WebEmbedState> {
    let key = (window.window_handle().window_id(), embed.id.clone());
    if let Some(e) = cx
        .try_global::<EmbedCache>()
        .and_then(|c| c.entries.get(&key))
    {
        return e.state.clone();
    }
    let spec = embed.spec.clone();
    let state = cx.new(|cx| WebEmbedState::new(spec, cx));
    cx.default_global::<EmbedCache>().entries.insert(
        key,
        CacheEntry {
            state: state.clone(),
            last_paint: 0,
        },
    );
    state
}

fn frame_count(window: &Window, cx: &App) -> u64 {
    let id = window.window_handle().window_id();
    cx.try_global::<Covers>()
        .and_then(|c| c.windows.get(&id))
        .map_or(0, |f| f.count)
}

/// Notes that `state` was painted in this frame and queues the end-of-frame sweep.
fn mark_painted(state: &Entity<WebEmbedState>, window: &mut Window, cx: &mut App) {
    let wid = window.window_handle().window_id();
    let now = frame_count(window, cx);
    let cache = cx.default_global::<EmbedCache>();
    for ((w, _), e) in cache.entries.iter_mut() {
        if *w == wid && e.state == *state {
            e.last_paint = now;
        }
    }
    if cache.sweeping.insert(wid) {
        window.on_next_frame(move |window, cx| sweep(wid, now, window, cx));
    }
}

/// After a frame: hides the embeds of `wid` that were not painted in it, then keeps at
/// most [`MAX_LIVE`] native pages beyond the painted ones and at most [`MAX_CACHED`]
/// cached embeds. Embeds of closed windows go.
fn sweep(wid: WindowId, frame: u64, _window: &mut Window, cx: &mut App) {
    let open: std::collections::HashSet<WindowId> =
        cx.windows().iter().map(|w| w.window_id()).collect();
    let mut cache = cx.remove_global::<EmbedCache>();
    cache.sweeping.remove(&wid);
    cache.entries.retain(|(w, _), _| open.contains(w));
    let mut parked: Vec<(u64, (WindowId, ElementId))> = Vec::new();
    for (key, e) in cache.entries.iter() {
        if e.last_paint < frame {
            if key.0 == wid {
                e.state.update(cx, |s, _| s.park());
            }
            parked.push((e.last_paint, key.clone()));
        }
    }
    parked.sort_by_key(|(t, _)| *t);
    let mut live = cache
        .entries
        .values()
        .filter(|e| e.state.read(cx).has_native())
        .count();
    let painted = cache.entries.len() - parked.len();
    for (_, key) in &parked {
        if live <= MAX_LIVE.max(painted) {
            break;
        }
        if let Some(e) = cache.entries.get(key)
            && e.state.read(cx).has_native()
        {
            e.state.update(cx, |s, _| s.release_native());
            live -= 1;
        }
    }
    let extra = cache.entries.len().saturating_sub(MAX_CACHED);
    for (_, key) in parked.iter().take(extra) {
        cache.entries.remove(key);
    }
    cx.set_global(cache);
}

/// Call from a window's root view at the top of every render, before anything paints.
///
/// The sweep that hides unpainted pages is otherwise queued only by a page that paints,
/// so leaving the last screen with a page on it (the 产出 page for a 话题 with none)
/// would leave its native view on top of the window forever. While any page of this
/// window shows, this starts a new frame and queues the sweep for it, so a page that
/// this frame does not paint hides right after it.
pub fn begin_window_frame(window: &mut Window, cx: &mut App) {
    let wid = window.window_handle().window_id();
    let shown = cx.try_global::<EmbedCache>().is_some_and(|c| {
        c.entries
            .iter()
            .any(|((w, _), e)| *w == wid && !e.state.read(cx).parked)
    });
    if !shown {
        return;
    }
    let frame = {
        let f = cx
            .default_global::<Covers>()
            .windows
            .entry(wid)
            .or_default();
        f.count += 1;
        f.count
    };
    if cx.default_global::<EmbedCache>().sweeping.insert(wid) {
        window.on_next_frame(move |window, cx| sweep(wid, frame, window, cx));
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
        .child(Occluder { band: false })
}

/// Wrap a small float that sits at the top of a scrolling region (the sticky date
/// pill, §2.17) with this, as `occluder()`: an embedded page that reaches under it is
/// cut below its bottom edge for as long as it is drawn, so the float stays on top of
/// the native page (A4). The page keeps showing under the rest of the region.
pub fn clip_band() -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .child(Occluder { band: true })
}

/// `visible` cut below every band of this frame that reaches into it.
fn below_bands(window: &Window, visible: Bounds<Pixels>, cx: &App) -> Bounds<Pixels> {
    let id = window.window_handle().window_id();
    let Some(frame) = cx.try_global::<Covers>().and_then(|c| c.windows.get(&id)) else {
        return visible;
    };
    let mut out = visible;
    for b in &frame.bands {
        if !b.intersects(&out) {
            continue;
        }
        let top = b.bottom().max(out.top());
        let bottom = out.bottom();
        out.origin.y = top;
        out.size.height = (bottom - top).max(px(0.));
    }
    out
}

struct Occluder {
    /// A band to cut the page below, not a cover that hides it.
    band: bool,
}

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
        let list = if self.band {
            &mut frame.bands
        } else {
            &mut frame.rects
        };
        if list.len() >= 64 {
            list.remove(0);
        }
        list.push(visible);
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
    let open: std::collections::HashSet<WindowId> =
        cx.windows().iter().map(|w| w.window_id()).collect();
    LIVE.with(|l| {
        l.borrow()
            .iter()
            .filter_map(|w| w.upgrade())
            .filter(|e| {
                let s = e.read(cx);
                !s.parked && s.window.is_none_or(|w| open.contains(&w.window_id()))
            })
            .collect()
    })
}

/// Every cached embed, also the parked ones (the 产出 page host pushes data and finishes
/// asks through them).
pub fn all(cx: &App) -> Vec<Entity<WebEmbedState>> {
    let _ = cx;
    LIVE.with(|l| l.borrow().iter().filter_map(|w| w.upgrade()).collect())
}

/// Forgets every cached embed (self-tests start clean).
pub fn clear_cache(cx: &mut App) {
    if let Some(c) = cx.try_global::<EmbedCache>()
        && !c.entries.is_empty()
    {
        cx.set_global(EmbedCache::default());
    }
}

// --- v12/fx-cards: page previews for the 产出 list ---
thread_local! {
    /// The last snapshot of each 产出 version's page, by (产出 id, version): the 产出
    /// list's thumbnail shows the rendered latest version once it was on screen
    /// (NATIVE.md §9.6), else the kind symbol. Keyed by version, so an older version
    /// (or a check page under the same id) never stands in for the latest one.
    static PREVIEWS: RefCell<std::collections::HashMap<(String, u32), Arc<gpui::Image>>> =
        RefCell::new(std::collections::HashMap::new());
}

fn remember_preview(artifact_id: &str, version: u32, image: Arc<gpui::Image>) {
    PREVIEWS.with(|p| {
        p.borrow_mut()
            .insert((artifact_id.to_string(), version), image)
    });
}

/// The rendered page of one version of `artifact_id` as last seen, if it was on screen
/// this session.
pub fn page_preview(artifact_id: &str, version: u32) -> Option<Arc<gpui::Image>> {
    PREVIEWS.with(|p| p.borrow().get(&(artifact_id.to_string(), version)).cloned())
}
// --- end v12/fx-cards ---
