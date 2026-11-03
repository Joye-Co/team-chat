//! Diagrams in the conversation (REFS §B): ```mermaid fences in message bodies drawn as
//! images, natively, with a hit map of their nodes.
//!
//! - [`normalize`]: the static checks and the cleaned source.
//! - [`cache`]: keys, the disk cache and the memory LRU.
//! - this file: the service. One long-lived hidden renderer window
//!   (`artifact_view::diagram::DiagramRenderer`), one request at a time, FIFO with visible
//!   diagrams first, a 4 s timeout that reloads the page, and a new window when the web
//!   content process ends (the request that ended it is not retried).
//! - [`view`]: the inline element; [`viewer`]: the expand viewer.

pub mod cache;
pub mod normalize;
pub mod preview;
pub mod view;
pub mod viewer;

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use artifact_view::diagram::{self as renderer, FailReason, RenderOutcome, ThemeVars};
use futures::{StreamExt as _, channel::mpsc};
use gpui::{App, AsyncApp, Global, Hsla, Image, ImageFormat};

use cache::{DiskCache, ErrMeta, Lru, Meta, Node, ThemeId};
use normalize::{DiagramType, Normalized, Refusal};

/// Pixels per CSS px for inline images.
pub const INLINE_SCALE: u32 = 2;
/// Pixels per CSS px when the viewer zooms past 200 %.
pub const ZOOM_SCALE: u32 = 4;
/// Render timeout per request.
pub const TIMEOUT: Duration = Duration::from_secs(4);
/// Diagrams drawn per message; later fences stay code.
pub const MAX_PER_MESSAGE: usize = 4;

/// Where a diagram lives, for a node reference (REFS §A2: `messageId` or `artifact`,
/// plus the block).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagramSource {
    pub message_id: Option<String>,
    pub topic_id: Option<String>,
    pub artifact: Option<(String, u32)>,
    pub block: DiagramBlock,
}

/// The block that holds the diagram.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiagramBlock {
    /// A ```mermaid fence of a Markdown body, by the byte offset of its opening line in the
    /// normalized body. The 引用提问 track maps it to `md:<index>` (REFS §A2.1).
    MarkdownFence { offset: usize },
    /// A Card v2 `diagram` block by its id (built by the Card v2.1 renderer track).
    #[allow(dead_code)]
    Card { block_id: String },
}

/// A node reference (⌥-click, 问这里, the context menu, ⌥↵). For now it only logs; the
/// 引用提问 track wires it to the composer chip (REFS §A3, §A4).
pub fn on_node_ref(source: DiagramSource, node_id: &str, label: &str) {
    tracing::info!(
        target: "diagrams",
        message = ?source.message_id,
        block = ?source.block,
        node = node_id,
        label_chars = label.chars().count(),
        "diagram node ref"
    );
}

/// The four error kinds the person sees (REFS §B4 table).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// parse, internal, timeout: 图没有画出来.
    Failed,
    /// 这种图还画不了.
    Unsupported,
    /// 图太大了，只显示原文.
    TooBig,
    /// 图里有不支持的写法.
    Forbidden,
}

impl ErrorKind {
    pub fn from_refusal(r: &Refusal) -> Self {
        match r {
            Refusal::Empty => ErrorKind::Failed,
            Refusal::TooLong => ErrorKind::TooBig,
            Refusal::Unsupported(_) => ErrorKind::Unsupported,
            Refusal::Forbidden(_) => ErrorKind::Forbidden,
        }
    }

    pub fn from_reason(r: FailReason) -> Self {
        match r {
            FailReason::Parse | FailReason::Internal | FailReason::Timeout => ErrorKind::Failed,
            FailReason::TooBig => ErrorKind::TooBig,
            FailReason::Unsupported => ErrorKind::Unsupported,
        }
    }

    pub fn text(self) -> &'static str {
        use crate::copy::diagrams as c;
        match self {
            ErrorKind::Failed => c::ERROR_FAILED,
            ErrorKind::Unsupported => c::ERROR_UNSUPPORTED,
            ErrorKind::TooBig => c::ERROR_TOO_BIG,
            ErrorKind::Forbidden => c::ERROR_FORBIDDEN,
        }
    }
}

/// A rendered diagram ready to draw.
#[derive(Clone)]
pub struct Ready {
    pub image: Arc<Image>,
    pub meta: Arc<Meta>,
}

/// What a request has now.
#[derive(Clone)]
pub enum State {
    Pending,
    Ready(Ready),
    Failed(ErrorKind),
}

impl State {
    pub fn is_pending(&self) -> bool {
        matches!(self, State::Pending)
    }
}

struct Job {
    key: String,
    source: String,
    kind: DiagramType,
    theme: ThemeId,
    scale: u32,
    visible: bool,
}

struct Running {
    job: Job,
    req_id: u64,
}

enum Msg {
    #[cfg(target_os = "macos")]
    Renderer(renderer::RendererEvent),
    Png {
        req_id: u64,
        result: Result<Vec<u8>, String>,
        rendered: Box<renderer::Rendered>,
    },
    Timeout(u64),
}

struct Service {
    mem: Lru<String, State>,
    queue: VecDeque<Job>,
    running: Option<Running>,
    #[cfg(target_os = "macos")]
    renderer: Option<renderer::DiagramRenderer>,
    renderer_ready: bool,
    disk: DiskCache,
    tx: mpsc::UnboundedSender<Msg>,
    seq: u64,
    /// Sources seen recently, to render the other theme when idle.
    recent: Lru<String, DiagramType>,
}

impl Global for Service {}

fn init(cx: &mut App) {
    if cx.has_global::<Service>() {
        return;
    }
    let (tx, mut rx) = mpsc::unbounded::<Msg>();
    cx.set_global(Service {
        mem: Lru::new(cache::MEMORY_ENTRIES),
        queue: VecDeque::new(),
        running: None,
        #[cfg(target_os = "macos")]
        renderer: None,
        renderer_ready: false,
        disk: DiskCache::open(
            preview::CACHE_DIR
                .lock()
                .ok()
                .and_then(|d| d.clone())
                .unwrap_or_else(DiskCache::default_dir),
            cache::DISK_LIMIT,
        ),
        tx,
        seq: 0,
        recent: Lru::new(32),
    });
    cx.spawn(async move |cx: &mut AsyncApp| {
        while let Some(msg) = rx.next().await {
            cx.update(|cx| handle(msg, cx));
        }
    })
    .detach();
}

/// The theme id for the current appearance.
pub fn current_theme(cx: &App) -> ThemeId {
    ThemeId::new(crate::theme::is_dark(cx), crate::theme::increase_contrast())
}

fn key_for(n: &Normalized, theme: ThemeId, scale: u32) -> String {
    cache::key(
        renderer::RENDERER_VERSION,
        renderer::MERMAID_SHA256,
        theme,
        scale,
        &n.source,
    )
}

fn decode(png: Vec<u8>, meta: Meta) -> Ready {
    Ready {
        image: Arc::new(Image::from_bytes(ImageFormat::Png, png)),
        meta: Arc::new(meta),
    }
}

/// What is known about a normalized source in the current theme at `scale`; starts a
/// render when nothing is. `visible` puts it before diagrams that are off screen.
pub fn request(n: &Normalized, scale: u32, visible: bool, cx: &mut App) -> State {
    request_theme(n, current_theme(cx), scale, visible, cx)
}

fn request_theme(n: &Normalized, theme: ThemeId, scale: u32, visible: bool, cx: &mut App) -> State {
    init(cx);
    let key = key_for(n, theme, scale);
    let s = cx.global_mut::<Service>();
    if scale == INLINE_SCALE {
        s.recent.insert(n.source.clone(), n.kind);
    }
    if let Some(state) = s.mem.get(&key).cloned() {
        if state.is_pending() && visible {
            if let Some(i) = s.queue.iter().position(|j| j.key == key) {
                if let Some(mut job) = s.queue.remove(i) {
                    job.visible = true;
                    s.queue.push_front(job);
                }
            }
        }
        return state;
    }
    if let Some((png, meta)) = s.disk.read_ok(&key) {
        let state = State::Ready(decode(png, meta));
        s.mem.insert(key, state.clone());
        return state;
    }
    if let Some(err) = s.disk.read_err(&key) {
        let state = State::Failed(ErrorKind::from_reason(FailReason::parse(&err.reason)));
        s.mem.insert(key, state.clone());
        return state;
    }
    s.mem.insert(key.clone(), State::Pending);
    let job = Job {
        key,
        source: n.source.clone(),
        kind: n.kind,
        theme,
        scale,
        visible,
    };
    if visible {
        s.queue.push_front(job);
    } else {
        s.queue.push_back(job);
    }
    pump(cx);
    State::Pending
}

/// The state of a key without starting anything (self-tests).
pub fn peek(n: &Normalized, scale: u32, cx: &App) -> Option<State> {
    let key = key_for(n, current_theme(cx), scale);
    cx.try_global::<Service>()
        .and_then(|s| s.mem.peek(&key).cloned())
}

/// True when nothing waits or renders.
pub fn idle(cx: &App) -> bool {
    cx.try_global::<Service>()
        .is_none_or(|s| s.running.is_none() && s.queue.is_empty())
}

/// Forgets what is in memory (self-tests start clean; the disk cache stays).
pub fn clear_memory(cx: &mut App) {
    if cx.has_global::<Service>() {
        cx.global_mut::<Service>().mem.clear();
    }
}

#[cfg(target_os = "macos")]
fn ensure_renderer(cx: &mut App) -> bool {
    let s = cx.global_mut::<Service>();
    if s.renderer.as_ref().is_some_and(|r| !r.is_closed()) {
        return s.renderer_ready;
    }
    s.renderer_ready = false;
    let Some(mtm) = objc2_foundation::MainThreadMarker::new() else {
        return false;
    };
    let tx = s.tx.clone();
    match renderer::DiagramRenderer::open(mtm, move |event| {
        let _ = tx.unbounded_send(Msg::Renderer(event));
    }) {
        Ok(r) => s.renderer = Some(r),
        Err(e) => tracing::warn!(target: "diagrams", error = %e, "diagram renderer did not open"),
    }
    false
}

#[cfg(not(target_os = "macos"))]
fn ensure_renderer(_cx: &mut App) -> bool {
    false
}

fn pump(cx: &mut App) {
    if cx.global::<Service>().running.is_some() {
        return;
    }
    if cx.global::<Service>().queue.is_empty() {
        queue_other_theme(cx);
        if cx.global::<Service>().queue.is_empty() {
            return;
        }
    }
    if !ensure_renderer(cx) {
        #[cfg(not(target_os = "macos"))]
        {
            let s = cx.global_mut::<Service>();
            while let Some(job) = s.queue.pop_front() {
                s.mem.insert(job.key, State::Failed(ErrorKind::Failed));
            }
        }
        return;
    }
    let s = cx.global_mut::<Service>();
    let Some(job) = s.queue.pop_front() else {
        return;
    };
    s.seq += 1;
    let req_id = s.seq;
    let request = renderer::RenderRequest {
        req_id,
        source: job.source.clone(),
        theme: theme_vars(job.theme),
    };
    #[cfg(target_os = "macos")]
    if let Some(r) = &s.renderer {
        r.request(&request);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = request;
    s.running = Some(Running { job, req_id });
    let tx = s.tx.clone();
    cx.spawn(async move |cx: &mut AsyncApp| {
        cx.background_executor().timer(TIMEOUT).await;
        let _ = tx.unbounded_send(Msg::Timeout(req_id));
    })
    .detach();
}

/// When idle: render the sources seen recently in the other theme, so a theme change
/// draws at once.
fn queue_other_theme(cx: &mut App) {
    let theme = current_theme(cx).other();
    let s = cx.global_mut::<Service>();
    let sources: Vec<(String, DiagramType)> = s
        .recent
        .newest_first()
        .into_iter()
        .take(8)
        .collect();
    for (source, kind) in sources {
        let n = Normalized { source, kind };
        let key = key_for(&n, theme, INLINE_SCALE);
        if s.mem.peek(&key).is_some() || s.disk.contains(&key) {
            continue;
        }
        s.mem.insert(key.clone(), State::Pending);
        s.queue.push_back(Job {
            key,
            source: n.source,
            kind,
            theme,
            scale: INLINE_SCALE,
            visible: false,
        });
    }
}

fn finish(key: String, state: State, cx: &mut App) {
    let s = cx.global_mut::<Service>();
    s.mem.insert(key, state);
    s.running = None;
    s.disk.save_index(false);
    cx.refresh_windows();
    pump(cx);
}

fn fail(job: Job, reason: FailReason, detail: &str, store: bool, cx: &mut App) {
    // The parse detail may quote the source: redacted, and only its length and the
    // first line go to the log, never to UI text.
    let redacted = chat_core::diagnostics::redact(detail);
    let first = redacted.lines().next().unwrap_or("").chars().take(120).collect::<String>();
    tracing::info!(target: "diagrams", reason = reason.as_str(), detail = %first, "diagram not drawn");
    if store {
        let _ = cx.global_mut::<Service>().disk.write_err(
            &job.key,
            &ErrMeta {
                reason: reason.as_str().to_string(),
                detail: first,
            },
        );
    }
    finish(job.key, State::Failed(ErrorKind::from_reason(reason)), cx);
}

fn take_running(req_id: u64, cx: &mut App) -> Option<Job> {
    let s = cx.global_mut::<Service>();
    if s.running.as_ref().is_some_and(|r| r.req_id == req_id) {
        s.running.take().map(|r| r.job)
    } else {
        None
    }
}

fn handle(msg: Msg, cx: &mut App) {
    match msg {
        #[cfg(target_os = "macos")]
        Msg::Renderer(event) => handle_renderer(event, cx),
        Msg::Png {
            req_id,
            result,
            rendered,
        } => {
            let Some(job) = take_running(req_id, cx) else {
                return;
            };
            match result {
                Ok(png) => {
                    let nodes = source_order(&job.source, &rendered.nodes);
                    let kind = job.kind;
                    let labels: Vec<String> = nodes.iter().map(|n| n.label.clone()).collect();
                    let meta = Meta {
                        width: rendered.width as f32,
                        height: rendered.height as f32,
                        scale: renderer::raster_scale(rendered.width, rendered.height, job.scale as f64)
                            as f32,
                        kind: kind.as_str().to_string(),
                        alt: alt_text(None, None, None, kind, &labels),
                        nodes,
                    };
                    let s = cx.global_mut::<Service>();
                    if let Err(e) = s.disk.write_ok(&job.key, &png, &rendered.svg, &meta) {
                        tracing::warn!(target: "diagrams", error = %e, "diagram cache write failed");
                    }
                    let key = job.key.clone();
                    finish(key, State::Ready(decode(png, meta)), cx);
                }
                Err(e) => fail(job, FailReason::Internal, &e, false, cx),
            }
        }
        Msg::Timeout(req_id) => {
            let Some(job) = take_running(req_id, cx) else {
                return;
            };
            #[cfg(target_os = "macos")]
            {
                let s = cx.global_mut::<Service>();
                s.renderer_ready = false;
                if let Some(r) = &s.renderer {
                    r.reload();
                }
            }
            fail(job, FailReason::Timeout, "timeout", true, cx);
        }
    }
}

#[cfg(target_os = "macos")]
fn handle_renderer(event: renderer::RendererEvent, cx: &mut App) {
    use renderer::RendererEvent;
    match event {
        RendererEvent::Ready => {
            cx.global_mut::<Service>().renderer_ready = true;
            pump(cx);
        }
        RendererEvent::Crashed(reason) => {
            tracing::warn!(target: "diagrams", %reason, "diagram renderer ended; opening a new one");
            let s = cx.global_mut::<Service>();
            s.renderer = None;
            s.renderer_ready = false;
            match s.running.take() {
                // Not retried and not cached on disk: a later launch tries again.
                Some(r) => fail(r.job, FailReason::Internal, &reason, false, cx),
                None => pump(cx),
            }
        }
        RendererEvent::Outcome(outcome) => {
            let req_id = outcome.req_id();
            let matches = cx
                .global::<Service>()
                .running
                .as_ref()
                .is_some_and(|r| r.req_id == req_id);
            if !matches {
                return;
            }
            match outcome {
                RenderOutcome::Rendered(r) => {
                    let s = cx.global::<Service>();
                    let Some(running) = &s.running else { return };
                    let scale = running.job.scale as f64;
                    let tx = s.tx.clone();
                    let rendered = Box::new(r);
                    if let Some(renderer) = &s.renderer {
                        let (w, h) = (rendered.width, rendered.height);
                        renderer.capture(w, h, scale, move |result| {
                            let _ = tx.unbounded_send(Msg::Png {
                                req_id,
                                result,
                                rendered,
                            });
                        });
                    }
                }
                RenderOutcome::Failed { reason, detail, .. } => {
                    if let Some(job) = take_running(req_id, cx) {
                        fail(job, reason, &detail, true, cx);
                    }
                }
            }
        }
    }
}

/// Nodes sorted by the first place their id appears in the source (REFS §B9).
pub fn source_order(source: &str, nodes: &[renderer::DiagramNode]) -> Vec<Node> {
    let mut out: Vec<(usize, usize, Node)> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let at = first_word_at(source, &n.id)
                .or_else(|| (!n.label.is_empty()).then(|| source.find(&n.label)).flatten())
                .unwrap_or(usize::MAX);
            (
                at,
                i,
                Node {
                    id: n.id.clone(),
                    label: n.label.clone(),
                    kind: n.kind.clone(),
                    x: n.x as f32,
                    y: n.y as f32,
                    w: n.w as f32,
                    h: n.h as f32,
                },
            )
        })
        .collect();
    out.sort_by_key(|(at, i, _)| (*at, *i));
    out.into_iter().map(|(_, _, n)| n).collect()
}

/// The first place `word` appears in `text` as a whole word.
fn first_word_at(text: &str, word: &str) -> Option<usize> {
    if word.is_empty() {
        return None;
    }
    let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    let mut from = 0;
    while let Some(i) = text[from..].find(word) {
        let at = from + i;
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        if !before.is_some_and(is_word) && !after.is_some_and(is_word) {
            return Some(at);
        }
        from = at + word.len().max(1);
        if from >= text.len() {
            break;
        }
    }
    None
}

/// Alt text (REFS §B9): `alt` → `caption` → `title` → generated
/// `{type word}：{n} 个节点。` + labels joined by ` → ` (flowcharts) or `、`, cut at 300.
pub fn alt_text(
    alt: Option<&str>,
    caption: Option<&str>,
    title: Option<&str>,
    kind: DiagramType,
    labels: &[String],
) -> String {
    for given in [alt, caption, title].into_iter().flatten() {
        let t = given.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    let head = crate::copy::fill(
        crate::copy::diagrams::ALT,
        &[("type", kind.word()), ("n", &labels.len().to_string())],
    );
    let sep = if kind == DiagramType::Flowchart { " → " } else { "、" };
    let text = format!("{head}{}", labels.join(sep));
    crate::markdown::clip(&text, 300)
}

// --- theming (REFS §B8) ----------------------------------------------------------------

fn hex(c: Hsla) -> String {
    let r = c.to_rgb();
    let b = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
    if r.a >= 0.999 {
        format!("#{:02x}{:02x}{:02x}", b(r.r), b(r.g), b(r.b))
    } else {
        format!("#{:02x}{:02x}{:02x}{:02x}", b(r.r), b(r.g), b(r.b), b(r.a))
    }
}

/// `top` mixed `amount` into `base` (opaque result).
fn mix(top: Hsla, base: Hsla, amount: f32) -> String {
    let (t, b) = (top.to_rgb(), base.to_rgb());
    let m = |x: f32, y: f32| y + (x - y) * amount;
    hex(Hsla::from(gpui::Rgba {
        r: m(t.r, b.r),
        g: m(t.g, b.g),
        b: m(t.b, b.b),
        a: 1.,
    }))
}

/// The renderer's colors for a theme id, from the current theme tokens.
pub fn theme_vars(id: ThemeId) -> ThemeVars {
    let k = crate::theme::colors_for(id.is_dark(), id.is_contrast());
    ThemeVars {
        raised: hex(k.raised),
        line: hex(k.line),
        line_2: hex(k.line_2),
        ink: hex(k.ink),
        ink_2: hex(k.ink_2),
        ink_3: hex(k.ink_3),
        sunken: hex(k.sunken),
        rail: hex(k.rail),
        bg: hex(k.bg),
        sel: hex(k.sel),
        live_mix: mix(k.live, k.raised, 0.2),
        ok_mix: mix(k.ok, k.raised, 0.2),
        bad_mix: mix(k.bad, k.raised, 0.2),
        font_family: "-apple-system, BlinkMacSystemFont, \"PingFang SC\", \"Helvetica Neue\", sans-serif"
            .into(),
        font_size_px: 13,
    }
}

// --- Markdown fences -------------------------------------------------------------------

/// A top-level fenced code block of a Markdown body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fence {
    /// Byte offset of the opening line.
    pub offset: usize,
    /// Info string, trimmed.
    pub info: String,
    pub closed: bool,
}

/// Top-level fences of a body in order (CommonMark rules for the fence lines; fences
/// inside lists and quotes are not top level and are not listed).
pub fn fences(body: &str) -> Vec<Fence> {
    let mut out = Vec::new();
    let mut open: Option<(char, usize, Fence)> = None;
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        let at = offset;
        offset += line.len();
        let text = line.trim_end_matches(['\n', '\r']);
        let indent = text.len() - text.trim_start_matches(' ').len();
        if indent > 3 {
            continue;
        }
        let t = &text[indent..];
        let ch = match t.chars().next() {
            Some(c @ ('`' | '~')) => c,
            _ => continue,
        };
        let run = t.chars().take_while(|c| *c == ch).count();
        if run < 3 {
            continue;
        }
        match &mut open {
            Some((c, n, _)) => {
                if *c == ch && run >= *n && t[run..].trim().is_empty() {
                    let (_, _, mut f) = open.take().unwrap_or((ch, run, Fence {
                        offset: at,
                        info: String::new(),
                        closed: false,
                    }));
                    f.closed = true;
                    out.push(f);
                }
            }
            None => {
                let info = t[run..].trim();
                if ch == '`' && info.contains('`') {
                    continue;
                }
                open = Some((
                    ch,
                    run,
                    Fence {
                        offset: at,
                        info: info.to_string(),
                        closed: false,
                    },
                ));
            }
        }
    }
    if let Some((_, _, f)) = open {
        out.push(f);
    }
    out
}

/// Whether an info string asks for a diagram (`mermaid`, case-insensitive, trimmed).
pub fn is_diagram_info(info: &str) -> bool {
    info.trim().eq_ignore_ascii_case("mermaid")
}

/// Offsets of the fences of `body` to draw as diagrams: closed ```mermaid fences, at most
/// [`MAX_PER_MESSAGE`], none while the message is streaming.
pub fn diagram_fences(body: &str, streaming: bool) -> Vec<usize> {
    if streaming {
        return Vec::new();
    }
    fences(body)
        .into_iter()
        .filter(|f| f.closed && is_diagram_info(&f.info))
        .take(MAX_PER_MESSAGE)
        .map(|f| f.offset)
        .collect()
}

#[cfg(test)]
mod tests;
