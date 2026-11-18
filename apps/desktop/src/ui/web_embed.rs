//! A 产出 page live inside the main window: [`WebEmbed`].
//!
//! ```ignore
//! WebEmbed::new("weekly-report", WebEmbedSpec::new(artifact_id, version_spec))
//!     .height(px(320.))
//!     .pass_scroll(true) // inline in a 话题: the timeline scrolls under the pointer
//!     .on_event(|event, window, cx| { /* Emit, ExternalLink, Loaded … */ })
//! ```
//!
//! The page is a native, isolated web view (WKWebView; WebView2 on Windows) from
//! `crates/artifact-view`
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

use artifact_view::{ArtifactEvent, Capabilities, Failure, RefMenu, Snapshot, VersionSpec, Viewer};
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

/// What a page that fills its area sits on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Canvas {
    /// The app background, in the app's theme (a 产出 page's content area).
    #[default]
    Content,
    /// A sheet's fill, in the app's theme, so the sheet's header and body are one tone.
    Sheet,
    /// A white page in the light scheme in both themes, as a browser shows a file
    /// that was written for the default page: an attached HTML file sets its text
    /// colors but no background, and they must stay readable.
    Paper,
}

impl Canvas {
    fn colors(self, cx: &App) -> crate::ui::native::NativeColors {
        match self {
            Canvas::Paper => crate::ui::native::NativeColors::light(),
            _ => crate::ui::native::native_colors(cx),
        }
    }

    fn surface(self, cx: &App) -> gpui::Hsla {
        let n = self.colors(cx);
        match self {
            Canvas::Sheet => n.sheet,
            _ => n.content,
        }
    }
}

/// A live 产出 page in the layout. See the module docs.
#[derive(IntoElement)]
pub struct WebEmbed {
    id: ElementId,
    spec: WebEmbedSpec,
    height: Pixels,
    /// Fill the parent's height instead of [`WebEmbed::height`].
    fill: bool,
    /// The side and bottom inset of the failure Alert ([`WebEmbed::failed_inset`]).
    failed_inset: Option<Pixels>,
    canvas: Canvas,
    /// Draw the card frame (1 px border on the card fill) around the page.
    framed: bool,
    radius: Pixels,
    /// The bottom-left and bottom-right radius of the fill, when the page ends at
    /// the rounded corners of a sheet ([`WebEmbed::rounded_bottom`]).
    bottom: (Pixels, Pixels),
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
            failed_inset: None,
            canvas: Canvas::Content,
            framed: false,
            radius: radius::LG,
            bottom: (px(0.), px(0.)),
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

    /// A card whose content edge is `inset` and that shows nothing under a page that
    /// failed: the Alert keeps to that edge and the card ends `inset` under it.
    pub fn failed_inset(mut self, inset: Pixels) -> Self {
        self.failed_inset = Some(inset);
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

    /// Rounds the fill's bottom corners (left, right) on top of [`WebEmbed::rounded`]:
    /// for a page whose bottom edge is a sheet's, so its square fill does not show
    /// past the sheet's corners.
    pub fn rounded_bottom(mut self, left: Pixels, right: Pixels) -> Self {
        self.bottom = (left, right);
        self
    }

    /// The page fills its parent (a page's content area) instead of a fixed height.
    pub fn fill(mut self) -> Self {
        self.fill = true;
        self
    }

    /// What a page that fills its area sits on (the app background by default).
    pub fn canvas(mut self, canvas: Canvas) -> Self {
        self.canvas = canvas;
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
    canvas: Canvas,
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
    /// A float band (the sticky date pill) cut the top of the page in this frame: the
    /// snapshot shows in the strip beside the band, so the page reads as continuous.
    band_cut: bool,
    check_pending: bool,
    focused: bool,
    /// The frame color the inline page's cut edge fades into, as last given to the page.
    fade: Option<String>,
    /// Where an inline page ends on a whole block (its own height when it fits): the
    /// frame shrinks to it, so no blank band sits under the cover.
    end: Option<Pixels>,
    /// The read of `end` for the loaded document came back (with an end or without).
    end_read: bool,
    /// The host base stylesheet given to the page, by its token values.
    base: Option<String>,
    #[cfg(any(target_os = "macos", windows))]
    native: Option<artifact_view::EmbeddedArtifact>,
    _events: Option<gpui::Task<()>>,
    /// Bounds of the element in window points, for self-tests.
    last_bounds: Option<Bounds<Pixels>>,
}

thread_local! {
    /// Every live embedded page, for self-tests (load state and where it is).
    static LIVE: RefCell<Vec<gpui::WeakEntity<WebEmbedState>>> = const { RefCell::new(Vec::new()) };
}

/// What runs when an embedded page is gone for good ([`on_page_gone`]).
type PageGone = Rc<dyn Fn(gpui::WeakEntity<WebEmbedState>, &mut App)>;

struct PageGoneHook(PageGone);

impl gpui::Global for PageGoneHook {}

/// `f` runs when an embedded page is gone for good: its state left the cache (the
/// window closed, or it was the oldest of too many), or it now shows another 产出. A page
/// that only gave up its native view while off screen is not gone. The 产出 page host
/// drops what it kept for the page.
pub fn on_page_gone(f: impl Fn(gpui::WeakEntity<WebEmbedState>, &mut App) + 'static, cx: &mut App) {
    cx.set_global(PageGoneHook(Rc::new(f)));
}

/// Tells the host, after the current update, that `page` is gone for good.
fn page_gone(page: gpui::WeakEntity<WebEmbedState>, cx: &mut App) {
    let Some(hook) = cx.try_global::<PageGoneHook>().map(|h| h.0.clone()) else {
        return;
    };
    cx.defer(move |cx| hook(page, cx));
}

impl WebEmbedState {
    fn new(spec: WebEmbedSpec, cx: &mut Context<Self>) -> Self {
        let me = cx.weak_entity();
        LIVE.with(|l| {
            let mut l = l.borrow_mut();
            l.retain(|w| w.upgrade().is_some());
            l.push(me.clone());
        });
        cx.on_release(move |_, cx| page_gone(me, cx)).detach();
        Self {
            spec,
            pass_scroll: false,
            autoplay: false,
            radius: px(0.),
            pad: crate::ui::native::tokens::space::S24,
            fill: false,
            canvas: Canvas::Content,
            remember: true,
            on_event: None,
            on_host: None,
            window: None,
            parked: false,
            phase: Phase::Loading,
            snapshot: None,
            hit_covered: false,
            cover_covered: false,
            band_cut: false,
            check_pending: false,
            focused: false,
            fade: None,
            end: None,
            end_read: false,
            base: None,
            #[cfg(any(target_os = "macos", windows))]
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
        #[cfg(any(target_os = "macos", windows))]
        {
            self.native.is_some()
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        false
    }

    /// Gives the page of `version` a data snapshot (the 产出 page host). False when no
    /// such page shows. The page redraws with the data, so its picture for overlays is
    /// taken again and an inline page says again where it ends: the frame follows a page
    /// that got longer or shorter.
    pub fn push_data(&mut self, version: u32, snap: Snapshot, cx: &mut Context<Self>) -> bool {
        #[cfg(any(target_os = "macos", windows))]
        if let Some(n) = &self.native {
            let pushed = n.push_data(version, snap);
            if pushed && self.phase == Phase::Ready {
                self.snapshot_later(cx, 120);
                self.measure_end_later(cx, 150);
            }
            return pushed;
        }
        let _ = (version, snap, cx);
        false
    }

    /// Finishes an open `emit` / `ask` of the page of `version`. False when it is not
    /// open any more (the host drops a late reply).
    pub fn finish(
        &self,
        version: u32,
        reply_id: &serde_json::Value,
        result: Result<serde_json::Value, Failure>,
    ) -> bool {
        #[cfg(any(target_os = "macos", windows))]
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
        #[cfg(any(target_os = "macos", windows))]
        if let Some(n) = &self.native {
            n.set_hidden(true);
        }
    }

    /// Gives up the native page (it loads again when it shows); the snapshot stays.
    fn release_native(&mut self) {
        #[cfg(any(target_os = "macos", windows))]
        if self.native.take().is_some() {
            self._events = None;
            self.phase = Phase::Loading;
        }
    }

    /// Self-tests: the page's load fails now, as [`ArtifactEvent::LoadFailed`] reports it.
    pub fn fail_for_self_test(&mut self, cx: &mut Context<Self>) {
        let version = self.version();
        self.on_native(
            ArtifactEvent::LoadFailed {
                version,
                reason: "self-test".into(),
            },
            cx,
        );
    }

    /// 重试 of a page that did not open: drops the view that failed (or that could not be
    /// made) and loads the page again in a new one on the next frame.
    pub fn retry(&mut self, cx: &mut Context<Self>) {
        #[cfg(any(target_os = "macos", windows))]
        {
            self.native = None;
        }
        self._events = None;
        self.phase = Phase::Loading;
        self.fade = None;
        self.base = None;
        self.end = None;
        self.end_read = false;
        cx.notify();
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

    /// An inline page is loaded but has not said yet where it ends (self-tests wait
    /// for it, so a capture never shows the declared height in one theme only).
    pub fn end_pending(&self) -> bool {
        self.pass_scroll && self.phase == Phase::Ready && !self.end_read
    }

    pub fn focused(&self) -> bool {
        self.focused
    }

    /// True while the native view is on screen.
    pub fn shown(&self) -> bool {
        #[cfg(any(target_os = "macos", windows))]
        {
            self.native.as_ref().is_some_and(|n| n.is_shown())
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        false
    }

    pub fn bounds(&self) -> Option<Bounds<Pixels>> {
        self.last_bounds
    }

    /// Moves keyboard focus into the page.
    pub fn focus(&self) {
        #[cfg(any(target_os = "macos", windows))]
        if let Some(n) = &self.native {
            n.focus();
        }
    }

    /// Runs JavaScript in the page (self-tests and developer tools).
    pub fn evaluate_script(&self, script: &str) {
        #[cfg(any(target_os = "macos", windows))]
        if let Some(n) = &self.native {
            n.evaluate_script(script);
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        let _ = script;
    }

    /// Runs JavaScript in the page and gives its string value (self-tests). `done` runs
    /// once, with `None` when there is no page or the value is not a string.
    pub fn evaluate_text(&self, script: &str, done: impl FnOnce(Option<String>) + 'static) {
        #[cfg(any(target_os = "macos", windows))]
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
        #[cfg(any(target_os = "macos", windows))]
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

    fn sync(&mut self, embed: &WebEmbed, cx: &mut Context<Self>) {
        self.pass_scroll = embed.pass_scroll;
        self.autoplay = embed.autoplay;
        self.radius = embed.radius;
        self.pad = embed.pad;
        self.fill = embed.fill;
        self.canvas = embed.canvas;
        self.remember = embed.remember;
        self.on_event = embed.on_event.clone();
        self.on_host = embed.on_host.clone();
        self.spec.viewer = embed.spec.viewer.clone();
        if self.spec.ref_menu != embed.spec.ref_menu {
            self.spec.ref_menu = embed.spec.ref_menu.clone();
            // The menu follows the theme in the page that is loaded, not only in the next.
            #[cfg(any(target_os = "macos", windows))]
            if let (Some(n), Some(menu)) = (&self.native, &self.spec.ref_menu) {
                n.set_ref_menu(menu.clone());
            }
        }
        let same = self.spec.artifact_id == embed.spec.artifact_id
            && self.spec.version.version == embed.spec.version.version;
        #[cfg(any(target_os = "macos", windows))]
        if let Some(n) = &self.native {
            n.set_pass_scroll(embed.pass_scroll);
        }
        if same {
            // The same page, but what its bridge may do can change (the author's member
            // record arrived, or the version's declared events did).
            if self.spec.capabilities != embed.spec.capabilities {
                self.spec.capabilities = embed.spec.capabilities;
                #[cfg(any(target_os = "macos", windows))]
                if let Some(n) = &self.native {
                    n.set_capabilities(self.spec.capabilities);
                }
            }
            return;
        }
        let new_id = self.spec.artifact_id != embed.spec.artifact_id;
        if new_id {
            // The page of the 产出 shown until now is gone for good.
            page_gone(cx.weak_entity(), cx);
        }
        self.spec = embed.spec.clone();
        self.snapshot = None;
        self.phase = Phase::Loading;
        #[cfg(any(target_os = "macos", windows))]
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
        #[cfg(not(any(target_os = "macos", windows)))]
        let _ = new_id;
    }

    fn on_native(&mut self, event: ArtifactEvent, cx: &mut Context<Self>) {
        match &event {
            ArtifactEvent::Loaded { .. } => {
                self.phase = Phase::Ready;
                // A new document: it has no fade and no base stylesheet yet.
                self.fade = None;
                self.end = None;
                self.end_read = false;
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
            | ArtifactEvent::RequestsEnded { .. }
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
        #[cfg(any(target_os = "macos", windows))]
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
        let css = base_css(self.pad, self.fill, self.canvas, false, cx);
        if self.base.as_deref() == Some(css.as_str()) {
            return;
        }
        #[cfg(any(target_os = "macos", windows))]
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
        #[cfg(any(target_os = "macos", windows))]
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
            let _ = this.update(cx, |s, cx| {
                s.end_read = true;
                if let Some(end) = end {
                    s.end = Some(px(end.ceil()));
                }
                cx.notify();
            });
        })
        .detach();
        #[cfg(not(any(target_os = "macos", windows)))]
        let _ = (cx, delay_ms);
    }

    /// Takes a new snapshot after `delay_ms` (only while the page is on screen).
    fn snapshot_later(&self, cx: &mut Context<Self>, delay_ms: u64) {
        #[cfg(any(target_os = "macos", windows))]
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
                // Diagnosis of the 产出 thumbnail: which view's picture is the last one.
                if std::env::var_os("TC_PREVIEW_LOG").is_some() && png.len() > 24 {
                    let dim =
                        |i: usize| u32::from_be_bytes([png[i], png[i + 1], png[i + 2], png[i + 3]]);
                    let _ = this.update(cx, |s, _| {
                        eprintln!(
                            "preview snapshot: {} v{} {}x{} remember {}",
                            s.artifact_id(),
                            s.version(),
                            dim(16),
                            dim(20),
                            s.remember
                        )
                    });
                }
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
        #[cfg(not(any(target_os = "macos", windows)))]
        let _ = (cx, delay_ms);
    }

    /// Creates the native view on the first paint.
    #[cfg(any(target_os = "macos", windows))]
    fn attach(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.native.is_some() || matches!(self.phase, Phase::Failed(_)) {
            return;
        }
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
        // The bindings can change in 设置 › 快捷键, so they are read again for each page.
        #[cfg(windows)]
        artifact_view::set_host_keys(host_keys(cx));
        let Some(attached) = attach_native(window, options, move |e| {
            let _ = tx.unbounded_send(e);
        }) else {
            return;
        };
        match attached {
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
        let uncut = visible;
        // A float band over the top of the region (the sticky date pill) stays
        // above the page: the page shows only below it (§2.17, A4).
        let whole = visible;
        let visible = below_bands(window, visible, cx);
        let band_cut = visible != whole;
        if band_cut != self.band_cut {
            self.band_cut = band_cut;
            cx.notify();
        }
        self.cover_covered = covers_overlap(window, visible, cx);
        #[cfg(any(target_os = "macos", windows))]
        {
            self.attach(window, cx);
            // A page that failed shows the Alert in its place: its blank view hides.
            let covered = self.covered() || matches!(self.phase, Phase::Failed(_));
            if let Some(n) = &self.native {
                n.set_dark(theme::is_dark(cx));
                n.set_frame(rect(bounds), rect(visible), window.scale_factor() as f64);
                n.set_hidden(covered);
                // The band's float gives way to a page that shows under its
                // place (`clip_band`).
                if !covered && uncut.size.height > px(0.) {
                    let id = window.window_handle().window_id();
                    let frame = cx.default_global::<Covers>().windows.entry(id).or_default();
                    if frame.pages.len() < 64 {
                        frame.pages.push(uncut);
                    }
                }
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
                        #[cfg(any(target_os = "macos", windows))]
                        if let Some(n) = &s.native {
                            n.set_hidden(s.covered() || matches!(s.phase, Phase::Failed(_)));
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
///
/// Besides text, tables and code it draws the plain controls a page uses (button, text
/// input, select, textarea, checkbox, radio, progress) like the app's own: one height,
/// one radius, the 3 px focus ring, 50 % when disabled, and `button[data-tc=primary]`
/// filled. `opaque` is the 产出 window, which has no app surface behind the page: the
/// page background is `--tc-bg` there, and transparent where the app draws it.
fn base_css(pad: Pixels, fill: bool, canvas: Canvas, opaque: bool, cx: &App) -> String {
    use crate::ui::native::tokens::{height, metrics, radius, space, text};
    let px_of = |p: Pixels| f32::from(p).round();
    let k = canvas.colors(cx);
    // The surface the page sits on: its canvas when it fills an area, else its
    // card's fill (see `WebEmbed::render`).
    let surface = if fill {
        canvas.surface(cx)
    } else {
        theme::colors(cx).raised
    };
    let scheme = if theme::is_dark(cx) && canvas != Canvas::Paper {
        "dark"
    } else {
        "light"
    };
    format!(
        ":root{{--tc-pad:{pad}px;--tc-bg:{bg};--tc-fg:{fg};--tc-muted:{muted};--tc-border:{border};\
--tc-fill:{fill};--tc-primary:{fg};--tc-danger:{danger};--tc-radius:{radius}px;--tc-gap:{gap}px;\
--tc-font-mono:{mono};color-scheme:{scheme}}}\
@layer tc-base{{\
html{{font:14px/22px -apple-system,BlinkMacSystemFont,'PingFang SC','Helvetica Neue','Segoe UI Variable Text','Segoe UI','Microsoft YaHei UI',sans-serif;\
color:var(--tc-fg);background:{page};-webkit-font-smoothing:antialiased}}\
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
code,pre{{font-family:var(--tc-font-mono);font-size:13px}}\
pre{{background:var(--tc-fill);border:1px solid var(--tc-border);border-radius:8px;padding:12px;overflow:auto}}\
button,input,select,textarea{{font:inherit;color:inherit;margin:0;box-sizing:border-box}}\
button,input[type=button],input[type=submit],input[type=reset]{{height:{ctl}px;padding:0 {ctl_pad}px;\
border:0;border-radius:var(--tc-radius);background:{push};font-size:{ui}px;\
line-height:{ui_line}px;font-weight:500;white-space:nowrap;cursor:default}}\
:is(button,input[type=button],input[type=submit],input[type=reset]):hover:not(:disabled){{background:{push_hover}}}\
:is(button,input[type=button],input[type=submit],input[type=reset]):active:not(:disabled){{background:{push_pressed}}}\
button[data-tc=primary]{{background:var(--tc-primary);border-color:var(--tc-primary);color:var(--tc-bg)}}\
button[data-tc=primary]:hover:not(:disabled),button[data-tc=primary]:active:not(:disabled){{background:var(--tc-primary);opacity:.9}}\
input:not([type]),input:is([type=text],[type=search],[type=email],[type=url],[type=tel],[type=number],[type=password],[type=date],[type=time],[type=datetime-local]),select,textarea{{\
height:{ctl}px;padding:0 {ctl_pad}px;border:1px solid {edge};border-radius:var(--tc-radius);background:{control};\
font-size:{ui}px;line-height:{ui_line}px;outline:0}}\
label>br+:is(input:not([type]),input:is([type=text],[type=search],[type=email],[type=url],[type=tel],[type=number],[type=password],[type=date],[type=time],[type=datetime-local]),select),\
p>input:only-child:is(:not([type]),[type=text],[type=search],[type=email],[type=url],[type=tel],[type=number],[type=password]){{display:block;width:100%}}\
p:has(>button):not(:has(>:not(button))){{display:flex;flex-wrap:wrap;align-items:center;gap:{s8}px}}\
textarea{{height:auto;min-height:{area}px;padding:{s6}px {ctl_pad}px;line-height:22px;resize:vertical;display:block;width:100%}}\
select{{appearance:none;-webkit-appearance:none;padding-right:{s32}px;background-repeat:no-repeat;background-position:right {s8}px center;\
background-image:url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='16' height='16' viewBox='0 0 16 16' fill='none' stroke='{chevron}' stroke-width='1.5' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='M4 6l4 4 4-4'/%3E%3C/svg%3E\")}}\
::placeholder{{color:var(--tc-muted);opacity:1}}\
input:is([type=checkbox],[type=radio]){{accent-color:var(--tc-primary);width:{mark}px;height:{mark}px;margin:0 {s8}px 0 0;vertical-align:-1px}}\
input[type=range]{{accent-color:var(--tc-primary)}}\
progress{{appearance:none;-webkit-appearance:none;display:block;width:100%;height:{bar}px;border:0;border-radius:{bar_r}px;background:var(--tc-fill);overflow:hidden}}\
progress::-webkit-progress-bar{{background:var(--tc-fill)}}\
progress::-webkit-progress-value{{background:var(--tc-primary)}}\
progress::-moz-progress-bar{{background:var(--tc-primary)}}\
:is(button,input,select,textarea):focus-visible{{outline:0;border-color:{focus_edge};box-shadow:0 0 0 {ring}px {focus}}}\
input:is([type=checkbox],[type=radio]):focus-visible{{outline:{ring}px solid {focus};outline-offset:1px;box-shadow:none}}\
:is(button,input,select,textarea):disabled{{opacity:{disabled}}}\
fieldset{{border:0;padding:0;margin:0 0 12px}}\
legend{{padding:0;margin-bottom:4px;font-size:14px;line-height:20px;color:var(--tc-fg)}}\
label{{line-height:{ui_line}px}}\
label:has(>input:is([type=checkbox],[type=radio]))+label{{margin-left:var(--tc-gap)}}}}",
        pad = px_of(pad),
        page = if opaque {
            "var(--tc-bg)"
        } else {
            "transparent"
        },
        danger = hex(k.red),
        radius = px_of(radius::CONTROL),
        gap = px_of(space::S12),
        mono = "ui-monospace,'SF Mono',Menlo,'Cascadia Mono',Consolas,monospace",
        ctl = px_of(height::SM),
        ctl_pad = px_of(space::S12),
        ui = px_of(text::UI.0),
        ui_line = px_of(text::UI.1),
        area = px_of(crate::ui::native::tokens::size::COMPOSER_MIN),
        s6 = px_of(space::S6),
        s8 = px_of(space::S8),
        s32 = px_of(space::S32),
        mark = px_of(crate::ui::native::tokens::icon::MD),
        bar = px_of(height::PROGRESS),
        bar_r = px_of(metrics::BAR_RADIUS),
        ring = px_of(metrics::FOCUS_RING),
        disabled = metrics::DISABLED_OPACITY,
        // Opaque on the page's surface, like the lines: a control on a card or in the
        // pane reads as a raised control in both themes.
        edge = hex(over(k.quaternary, surface)),
        control = hex(over(k.control, surface)),
        // A page's button is the app's outline button: the borderless tint (D11).
        push = hex(over(k.push, surface)),
        push_hover = hex(over(k.push_hover, surface)),
        push_pressed = hex(over(k.push_pressed, surface)),
        focus = hex(k.focus),
        focus_edge = hex(over(k.focus, surface)),
        chevron = hex(k.secondary).replace('#', "%23"),
        bg = hex(if fill { surface } else { k.content }),
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

/// The base stylesheet for a page in its own 产出 window, as the script that applies it:
/// the same defaults and `--tc-*` variables an embedded page gets, on the app background
/// (the window has no app surface behind the page). Run it after every load and when the
/// theme changes; it replaces the one before.
pub(crate) fn window_base_script(cx: &App) -> String {
    base_script(&base_css(
        crate::ui::native::tokens::space::S24,
        true,
        Canvas::Content,
        true,
        cx,
    ))
}

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

/// Adds a page above GPUI's own view of `window` (an AppKit sibling view). `None` when
/// the window has no native view yet.
#[cfg(target_os = "macos")]
fn attach_native(
    window: &Window,
    options: artifact_view::EmbedOptions,
    on_event: impl Fn(ArtifactEvent) + 'static,
) -> Option<Result<artifact_view::EmbeddedArtifact, artifact_view::Error>> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let mtm = objc2_foundation::MainThreadMarker::new()?;
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    // SAFETY: GPUI's AppKit handle points at the window's live content NSView, and paint
    // runs on the main thread.
    let host = unsafe { artifact_view::view_from_ptr(appkit.ns_view.as_ptr()) }?;
    Some(artifact_view::EmbeddedArtifact::attach(
        mtm, host, options, on_event,
    ))
}

/// Adds a page above GPUI's content of `window` (a child window of its `HWND`). `None`
/// when the window has no native handle yet. [`prepare_process`] must have run: a child
/// window shows only when GPUI does not draw through DirectComposition.
#[cfg(windows)]
fn attach_native(
    window: &Window,
    options: artifact_view::EmbedOptions,
    on_event: impl Fn(ArtifactEvent) + 'static,
) -> Option<Result<artifact_view::EmbeddedArtifact, artifact_view::Error>> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return None;
    };
    // Paint runs on the thread of GPUI's message loop.
    Some(artifact_view::EmbeddedArtifact::attach(
        artifact_view::MainThread::current(),
        win32.hwnd.get(),
        options,
        on_event,
    ))
}

/// The app's own shortcuts, for a page that has the keyboard focus: a key with Ctrl or
/// Alt, or a function key, that is bound for the whole app. On the Mac such a key reaches
/// the menu bar whatever has the focus; on Windows the page's process gets it first, so
/// the page host hands these keys back to the window ([`artifact_view::set_host_keys`]).
/// A binding of a key context (a text field, a list, a sheet) is left out: its context
/// does not have the focus while a page has it. The editing keys stay with the page.
#[cfg(windows)]
fn host_keys(cx: &App) -> Vec<artifact_view::HostKey> {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let mut keys = Vec::new();
    for binding in keymap.bindings() {
        if binding.predicate().is_some() {
            continue;
        }
        let [stroke] = binding.keystrokes() else {
            continue;
        };
        let (m, name) = (stroke.modifiers(), stroke.key());
        let Some(key) = virtual_key(name) else {
            continue;
        };
        let function = (0x70..=0x87).contains(&key);
        let editing = m.control && !m.alt && matches!(name, "a" | "c" | "v" | "x" | "z" | "y");
        if m.platform || editing || !(m.control || m.alt || function) {
            continue;
        }
        let key = artifact_view::HostKey {
            ctrl: m.control,
            alt: m.alt,
            shift: m.shift,
            key,
        };
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    keys
}

/// The Windows virtual-key code of a GPUI key name, for the keys shortcuts use.
#[cfg(any(windows, test))]
fn virtual_key(name: &str) -> Option<u32> {
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(c @ ('a'..='z' | '0'..='9')), None) => Some(c.to_ascii_uppercase() as u32),
        (Some(c), None) => match c {
            ';' => Some(0xBA),
            '=' => Some(0xBB),
            ',' => Some(0xBC),
            '-' => Some(0xBD),
            '.' => Some(0xBE),
            '/' => Some(0xBF),
            '`' => Some(0xC0),
            '[' => Some(0xDB),
            '\\' => Some(0xDC),
            ']' => Some(0xDD),
            '\'' => Some(0xDE),
            _ => None,
        },
        _ => match name {
            "enter" => Some(0x0D),
            "space" => Some(0x20),
            _ => name
                .strip_prefix('f')
                .and_then(|n| n.parse::<u32>().ok())
                .filter(|n| (1..=24).contains(n))
                .map(|n| 0x6F + n),
        },
    }
}

/// Sets up the process for embedded pages. Call it first in `main`, before GPUI starts.
///
/// On Windows: GPUI draws through a HWND swap chain instead of a topmost DirectComposition
/// tree, which would cover every child window (the embedded WebView2 pages); and the
/// WebView2 user data goes to a folder of its own under the app's cache folder.
pub fn prepare_process() {
    #[cfg(windows)]
    {
        // SAFETY: `main` calls this before it starts any thread.
        unsafe { std::env::set_var("GPUI_DISABLE_DIRECT_COMPOSITION", "1") };
        artifact_view::set_user_data_dir(
            crate::paths::cache_base()
                .join("team-chat")
                .join("webview2"),
        );
    }
}

#[cfg(any(target_os = "macos", windows))]
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
        state.update(cx, |s, cx| s.sync(&self, cx));
        let k = theme::colors(cx);
        let s = state.read(cx);
        let failed = matches!(s.phase, Phase::Failed(_));
        // An inline page ends on its last whole block (or its own end).
        let height = match s.end {
            Some(end) if self.pass_scroll => end.min(self.height),
            _ => self.height,
        };
        // The snapshot also shows while a band cuts the page's top: the native view is
        // clipped across its whole width below the band, and the picture fills the strip
        // beside the band (§2.17: the pill is on top, with no full-width fill).
        let backdrop = match (&s.phase, s.covered() || s.band_cut, &s.snapshot) {
            (_, true, Some(image)) | (Phase::Loading, _, Some(image)) => {
                img(image.clone()).size_full().into_any_element()
            }
            // §2.15: the Alert in place of the page, with 重试; never a lone line in a well.
            (Phase::Failed(_), _, _) => {
                let again = state.downgrade();
                let edge = crate::ui::native::tokens::space::S12;
                div()
                    .pt(edge)
                    .px(self.failed_inset.unwrap_or(edge))
                    .pb(self.failed_inset.unwrap_or(edge))
                    .child(
                        crate::ui::native::Alert::new(crate::ui::Sym::Failed, ca::LOAD_FAILED)
                            .tone(crate::ui::native::AlertTone::Destructive)
                            .when(!self.fill, |a| a.on_card())
                            .description(ca::LOAD_FAILED_LINE)
                            .action(
                                crate::ui::native::push_button(
                                    SharedString::from(format!("{}-retry", self.id)),
                                    ca::LOAD_RETRY,
                                    crate::ui::native::PushStyle::Outline,
                                    crate::ui::native::PushSize::Small,
                                    cx,
                                )
                                .on_click(move |_, _, cx| {
                                    let _ = again.update(cx, |s, cx| s.retry(cx));
                                }),
                            ),
                    )
                    .into_any_element()
            }
            (Phase::Loading, _, _) => message(ca::OPENING, k.text_3).into_any_element(),
            _ => div().into_any_element(),
        };
        let n = crate::ui::native::native_colors(cx);
        div()
            .id(self.id.clone())
            .relative()
            .w_full()
            .when(self.fill, |d| d.flex_1().h_full().min_h(px(0.)))
            // A page that did not open: the frame hugs the Alert.
            .when(!self.fill && failed, |d| d.flex_none())
            .when(!self.fill && !failed, |d| d.h(height).flex_none())
            .rounded(self.radius)
            .when(self.bottom.0 > px(0.), |d| d.rounded_bl(self.bottom.0))
            .when(self.bottom.1 > px(0.), |d| d.rounded_br(self.bottom.1))
            .overflow_hidden()
            // A page that fills a content area sits on the app background; inline it
            // sits on its card's fill. `framed` adds the card's 1 px line (§1.7).
            .bg(if self.fill {
                self.canvas.surface(cx)
            } else {
                k.raised
            })
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
                #[cfg(any(target_os = "macos", windows))]
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
    /// Where the native pages of this frame show, before any band cut.
    pages: Vec<Bounds<Pixels>>,
    /// A native page showed under the band's place in the last frame
    /// (`page_under_band`).
    under_band: bool,
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
        frame.pages.clear();
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

/// Whether the embed `id` shows a page that did not open (in any window).
pub fn page_failed(id: &ElementId, cx: &App) -> bool {
    cx.try_global::<EmbedCache>().is_some_and(|c| {
        c.entries.iter().any(|((_, embed), entry)| {
            embed == id && matches!(entry.state.read(cx).phase, Phase::Failed(_))
        })
    })
}

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
        .child(Occluder)
}

/// Wrap a small float that sits at the top of a scrolling region (the sticky date
/// pill, §2.17) with this, as `occluder()`: an embedded page that reaches under it is
/// cut below its bottom edge for as long as it is drawn, so the float stays on top of
/// the native page (A4). The page keeps showing under the rest of the region.
///
/// The cut runs across the page's whole width, so a page that fills the row under the
/// float would show a blank strip beside it. The caller keeps this element at the
/// float's place in every frame and draws the float only while [`page_under_band`] is
/// false (`drawn`): the page is then cut at the region's own edge, like every other row.
pub fn clip_band(drawn: bool) -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .child(Band { drawn })
}

/// A native page showed under the place of this window's [`clip_band`] in the last frame.
pub fn page_under_band(window: &Window, cx: &App) -> bool {
    let id = window.window_handle().window_id();
    cx.try_global::<Covers>()
        .and_then(|c| c.windows.get(&id))
        .is_some_and(|f| f.under_band)
}

/// The place of a float band: registers the band while its float is drawn, and says after
/// every frame whether a native page showed under it.
struct Band {
    drawn: bool,
}

impl IntoElement for Band {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Band {
    type RequestLayoutState = ();
    type PrepaintState = Bounds<Pixels>;

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
    ) -> Bounds<Pixels> {
        let visible = bounds.intersect(&window.content_mask().bounds);
        if LIVE.with(|l| l.borrow().is_empty()) {
            return visible;
        }
        begin_frame(window, cx);
        if self.drawn {
            let id = window.window_handle().window_id();
            let frame = cx.default_global::<Covers>().windows.entry(id).or_default();
            if frame.bands.len() >= 64 {
                frame.bands.remove(0);
            }
            frame.bands.push(visible);
        }
        visible
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        place: &mut Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let live = !LIVE.with(|l| l.borrow().is_empty());
        if live {
            // The pages of this frame are dropped before the next one, also
            // when no page paints in it.
            mark_painting(window, cx);
        }
        let id = window.window_handle().window_id();
        let frame = cx.default_global::<Covers>().windows.entry(id).or_default();
        let under = live && frame.pages.iter().any(|p| p.intersects(place));
        if under != frame.under_band {
            frame.under_band = under;
            // The float shows or hides in the next frame.
            window.on_next_frame(|window, _| window.refresh());
        }
    }
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
        let list = &mut frame.rects;
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
    preview_disk::save(artifact_id, version, &image);
    PREVIEWS.with(|p| {
        p.borrow_mut()
            .insert((artifact_id.to_string(), version), image)
    });
}

/// The rendered page of one version of `artifact_id` as last seen: on screen this
/// session, else the picture kept on disk from an earlier one, so the 产出 list shows
/// a page's thumbnail after a restart too.
pub fn page_preview(artifact_id: &str, version: u32) -> Option<Arc<gpui::Image>> {
    let key = (artifact_id.to_string(), version);
    if let Some(image) = PREVIEWS.with(|p| p.borrow().get(&key).cloned()) {
        return Some(image);
    }
    let image = preview_disk::load(artifact_id, version)?;
    PREVIEWS.with(|p| p.borrow_mut().insert(key, image.clone()));
    Some(image)
}

/// The page previews kept between launches: one small PNG per 产出 version under the
/// cache folder (`<cache>/team-chat/page-previews`), written when a page was
/// snapshotted and read back by [`page_preview`]. A cache: the newest
/// [`preview_disk::KEEP`] stay. Self-tests and scenes neither read nor write it.
mod preview_disk {
    use std::{cell::RefCell, collections::HashSet, path::PathBuf, sync::Arc};

    /// The width a preview is kept at (a tile is at most about 320 pt wide, at 2x).
    pub const MAX_WIDTH: u32 = 640;
    /// How many previews stay on disk.
    pub const KEEP: usize = 400;

    thread_local! {
        /// Versions with no file, so a tile without a preview asks the disk once.
        static MISSING: RefCell<HashSet<(String, u32)>> = RefCell::new(HashSet::new());
    }

    fn enabled() -> bool {
        crate::views::settings::prefs::saving()
    }

    fn dir() -> PathBuf {
        crate::paths::cache_base()
            .join("team-chat")
            .join("page-previews")
    }

    /// `<id>-v<version>.png`, the id kept to letters, digits, `-` and `_`.
    pub fn file_name(artifact_id: &str, version: u32) -> String {
        let id: String = artifact_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        format!("{id}-v{version}.png")
    }

    /// `png` scaled down to at most [`MAX_WIDTH`] wide, as PNG bytes. `None` when it
    /// does not decode.
    pub fn small_png(png: &[u8]) -> Option<Vec<u8>> {
        let image = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?;
        let image = if image.width() > MAX_WIDTH {
            let height = (u64::from(image.height()) * u64::from(MAX_WIDTH)
                / u64::from(image.width()))
            .max(1) as u32;
            image.resize_exact(MAX_WIDTH, height, image::imageops::FilterType::Triangle)
        } else {
            image
        };
        let mut out = std::io::Cursor::new(Vec::new());
        image.write_to(&mut out, image::ImageFormat::Png).ok()?;
        Some(out.into_inner())
    }

    /// The files of `names_by_age` (newest first) past the first `keep`.
    pub fn to_drop(names_by_age: Vec<String>, keep: usize) -> Vec<String> {
        names_by_age.into_iter().skip(keep).collect()
    }

    /// Writes the preview in the background (scaled down), then trims the folder.
    pub fn save(artifact_id: &str, version: u32, image: &Arc<gpui::Image>) {
        if !enabled() || image.format() != gpui::ImageFormat::Png {
            return;
        }
        MISSING.with(|m| m.borrow_mut().remove(&(artifact_id.to_string(), version)));
        let (image, name) = (image.clone(), file_name(artifact_id, version));
        std::thread::spawn(move || {
            let Some(png) = small_png(image.bytes()) else {
                return;
            };
            let dir = dir();
            if std::fs::create_dir_all(&dir).is_err()
                || std::fs::write(dir.join(&name), png).is_err()
            {
                return;
            }
            let mut files: Vec<(std::time::SystemTime, String)> = std::fs::read_dir(&dir)
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|e| {
                    let at = e.metadata().ok()?.modified().ok()?;
                    Some((at, e.file_name().to_string_lossy().into_owned()))
                })
                .collect();
            files.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
            for old in to_drop(files.into_iter().map(|(_, n)| n).collect(), KEEP) {
                let _ = std::fs::remove_file(dir.join(old));
            }
        });
    }

    /// The preview kept for this version, if any.
    pub fn load(artifact_id: &str, version: u32) -> Option<Arc<gpui::Image>> {
        if !enabled() {
            return None;
        }
        let key = (artifact_id.to_string(), version);
        if MISSING.with(|m| m.borrow().contains(&key)) {
            return None;
        }
        match std::fs::read(dir().join(file_name(artifact_id, version))) {
            Ok(png) => Some(Arc::new(gpui::Image::from_bytes(
                gpui::ImageFormat::Png,
                png,
            ))),
            Err(_) => {
                MISSING.with(|m| m.borrow_mut().insert(key));
                None
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_preview_file_is_named_by_its_version_with_a_safe_id() {
            assert_eq!(file_name("art-01HX_z", 3), "art-01HX_z-v3.png");
            assert_eq!(file_name("../etc/passwd", 1), "___etc_passwd-v1.png");
            assert_ne!(file_name("a", 1), file_name("a", 2));
        }

        #[test]
        fn a_wide_snapshot_is_kept_small_and_a_small_one_as_it_is() {
            let png = |w: u32, h: u32| {
                let mut out = std::io::Cursor::new(Vec::new());
                image::DynamicImage::ImageRgba8(image::RgbaImage::new(w, h))
                    .write_to(&mut out, image::ImageFormat::Png)
                    .unwrap();
                out.into_inner()
            };
            let size = |bytes: &[u8]| {
                let i = image::load_from_memory(bytes).unwrap();
                (i.width(), i.height())
            };
            assert_eq!(size(&small_png(&png(2560, 1600)).unwrap()), (640, 400));
            assert_eq!(size(&small_png(&png(320, 200)).unwrap()), (320, 200));
            assert_eq!(small_png(b"not a png"), None);
        }

        #[test]
        fn the_oldest_previews_past_the_limit_are_dropped() {
            let names: Vec<String> = (0..5).map(|i| format!("{i}.png")).collect();
            assert_eq!(to_drop(names.clone(), 3), ["3.png", "4.png"]);
            assert!(to_drop(names, 10).is_empty());
        }
    }
}
// --- end v12/fx-cards ---

#[cfg(test)]
mod tests {
    use super::virtual_key;
    use super::{Canvas, base_css, window_base_script};
    use crate::ui::native::tokens::space;

    #[gpui::test]
    fn the_base_stylesheet_draws_controls_and_fills_only_the_window(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
            crate::theme::set_mode(gpui_component::ThemeMode::Light, cx);
            let embedded = base_css(space::S16, true, Canvas::Content, false, cx);
            // Everything a page may rely on is a variable with a value.
            for var in [
                "--tc-pad:16px",
                "--tc-bg:",
                "--tc-fg:",
                "--tc-muted:",
                "--tc-border:",
                "--tc-fill:",
                "--tc-primary:",
                "--tc-danger:",
                "--tc-radius:8px",
                "--tc-gap:12px",
                "--tc-font-mono:ui-monospace",
            ] {
                assert!(embedded.contains(var), "{var}");
            }
            // The defaults sit in the layer, so any rule of the page wins.
            let (vars, layer) = embedded.split_once("@layer tc-base{").unwrap();
            assert!(vars.starts_with(":root{") && !vars.contains("button"));
            for rule in [
                "height:28px",
                "button[data-tc=primary]{background:var(--tc-primary)",
                "accent-color:var(--tc-primary)",
                ":focus-visible{outline:0",
                "0 0 0 3px",
                ":disabled{opacity:0.5}",
                "progress::-webkit-progress-value{background:var(--tc-primary)}",
            ] {
                assert!(layer.contains(rule), "{rule}");
            }
            // Embedded, the app draws the surface behind the page; the 产出 window has
            // none, so its page fills itself with the app background.
            assert!(embedded.contains("background:transparent;-webkit-font-smoothing"));
            let window = window_base_script(cx);
            assert!(window.contains("background:var(--tc-bg);-webkit-font-smoothing"));
            assert!(window.contains("--tc-pad:24px") && window.contains("__tc_base"));
            // The same variables and rules in both.
            assert!(window.contains("--tc-radius:8px") && window.contains("height:28px"));
        });
    }

    #[test]
    fn shortcut_keys_have_their_windows_key_codes() {
        assert_eq!(virtual_key("k"), Some(0x4B));
        assert_eq!(virtual_key("1"), Some(0x31));
        assert_eq!(virtual_key(","), Some(0xBC));
        assert_eq!(virtual_key(";"), Some(0xBA));
        assert_eq!(virtual_key("f6"), Some(0x75));
        assert_eq!(virtual_key("f11"), Some(0x7A));
        assert_eq!(virtual_key("enter"), Some(0x0D));
        assert_eq!(virtual_key("up"), None);
        assert_eq!(virtual_key("f"), Some(0x46));
        assert_eq!(virtual_key("f0"), None);
    }
}
