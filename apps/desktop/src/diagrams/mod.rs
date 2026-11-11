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

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

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
    /// The `{block}` segment of a node chip (REFS §A2.2): the diagram's title, else 图 when
    /// the source has more than one diagram, else `None` (omitted). The renderer that knows
    /// the block's neighbours builds it with `refs::parts::block_name`.
    pub block_name: Option<String>,
}

/// The block that holds the diagram.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiagramBlock {
    /// A ```mermaid fence of a Markdown body, by the byte offset of its opening line in the
    /// normalized body, and (inc-refs) its top-level block index `md:<index>` (REFS §A2.1).
    MarkdownFence { offset: usize, index: Option<usize> },
    /// A Card v2 `diagram` block by its id.
    Card { block_id: String },
}

/// A node reference (⌥-click, 问这里, the context menu, ⌥↵): a `node` part (REFS §A2.2)
/// handed to [`refs::ask_about`]. The label is `{source} · {block} · {node}`: the author's
/// name (a Markdown fence), the card title (a card block) or `{title} v{n}` (a 产出); the
/// block is [`DiagramSource::block_name`].
pub fn on_node_ref(
    source: DiagramSource,
    node_id: &str,
    label: &str,
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) {
    use crate::views::refs::{self, RefIntent};
    let Some(target) = node_target(&source, node_id, label, cx) else {
        return;
    };
    refs::ask_about(target, RefIntent::Topic, window, cx);
}

/// The reference to a node of a diagram (inc-refs).
pub fn node_target(
    source: &DiagramSource,
    node_id: &str,
    label: &str,
    cx: &gpui::App,
) -> Option<crate::views::refs::PartTarget> {
    use crate::views::refs::{RefSource, parts};
    let topic_id = source.topic_id.clone().unwrap_or_default();
    let services = crate::views::refs::services(cx);
    let provider = services.as_ref().map(|s| s.provider.clone());
    let group_id = provider
        .as_ref()
        .and_then(|p| {
            p.groups()
                .into_iter()
                .find(|g| p.topic(&g.id, &topic_id).is_some())
        })
        .map(|g| g.id)
        .unwrap_or_default();
    let (target_source, source_label, card_rev) = match (&source.artifact, &source.message_id) {
        (Some((artifact_id, version)), _) => {
            let title = provider
                .as_ref()
                .and_then(|p| {
                    p.artifacts(&group_id, &topic_id)
                        .into_iter()
                        .find(|a| &a.id == artifact_id)
                })
                .map(|a| a.title)
                .unwrap_or_default();
            (
                RefSource::Artifact {
                    group_id: group_id.clone(),
                    topic_id: topic_id.clone(),
                    artifact_id: artifact_id.clone(),
                    version: *version,
                },
                parts::artifact_source(&title, *version),
                None,
            )
        }
        (None, message_id) => {
            let message_id = message_id.clone().unwrap_or_default();
            let (label, rev) = match (&source.block, provider.as_ref()) {
                (DiagramBlock::Card { .. }, Some(p)) => p
                    .card_v2(&group_id, &message_id)
                    .map(|c| (c.card.title.clone(), Some(u64::from(c.card.rev))))
                    .unwrap_or_default(),
                (DiagramBlock::MarkdownFence { .. }, Some(p)) => {
                    let author = p
                        .messages(&group_id, &topic_id, 400)
                        .messages
                        .into_iter()
                        .find(|m| m.id == message_id)
                        .map(|m| m.author_id);
                    (
                        author
                            .and_then(|a| p.member(&a))
                            .map(|m| m.name)
                            .unwrap_or_default(),
                        None,
                    )
                }
                _ => (String::new(), None),
            };
            (
                RefSource::Message {
                    group_id: group_id.clone(),
                    topic_id: topic_id.clone(),
                    message_id,
                },
                label,
                rev,
            )
        }
    };
    let block_id = match &source.block {
        DiagramBlock::Card { block_id } => block_id.clone(),
        DiagramBlock::MarkdownFence { index, .. } => format!("md:{}", (*index)?),
    };
    Some(parts::node(node_id, label, None).target(
        target_source,
        &source_label,
        block_id,
        source.block_name.as_deref(),
        card_rev,
    ))
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

/// What an error frame says after its reason (Refs.dc.html 状态一览): the source line the
/// library could not read, or how many nodes a too large diagram has. Numbers only: the
/// library's message may quote the source and stays in the log.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hint {
    /// 1-based line of the normalized source.
    Line(u32),
    Nodes(u32),
}

impl Hint {
    /// The hint in a failure detail: `Parse error on line N` (parse) or `nodes N`
    /// (too_big).
    pub fn of(reason: FailReason, detail: &str) -> Option<Self> {
        let number_after = |key: &str| -> Option<u32> {
            let at = detail.find(key)? + key.len();
            let digits: String = detail[at..]
                .chars()
                .skip_while(|c| *c == ' ')
                .take_while(char::is_ascii_digit)
                .collect();
            digits.parse().ok().filter(|n| *n > 0)
        };
        match reason {
            FailReason::Parse => number_after("on line").map(Hint::Line),
            FailReason::TooBig => number_after("nodes").map(Hint::Nodes),
            _ => None,
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
    Failed(ErrorKind, Option<Hint>),
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
    #[cfg(any(target_os = "macos", windows))]
    Renderer(renderer::RendererEvent),
    Png {
        req_id: u64,
        result: Result<Vec<u8>, String>,
        rendered: Box<renderer::Rendered>,
    },
    Timeout(u64),
}

/// Renderer starts that ended before the page was ready, and when the next start may run.
///
/// Without it a renderer that cannot start (no web view runtime on the computer, a broken
/// profile folder) is opened again at once, forever, and its diagrams never leave 正在画.
#[derive(Default)]
struct StartBackoff {
    failures: u32,
    retry_at: Option<Instant>,
}

impl StartBackoff {
    /// The wait after the first failed start; it doubles up to [`Self::MAX`].
    const FIRST: Duration = Duration::from_secs(2);
    const MAX: Duration = Duration::from_secs(300);

    /// True while no new renderer may be opened.
    fn blocked(&self, now: Instant) -> bool {
        self.retry_at.is_some_and(|at| now < at)
    }

    /// Records a failed start and gives the wait before the next one.
    fn failed(&mut self, now: Instant) -> Duration {
        self.failures = self.failures.saturating_add(1);
        let doubled = Self::FIRST.saturating_mul(1u32 << (self.failures - 1).min(16));
        let wait = doubled.min(Self::MAX);
        self.retry_at = Some(now + wait);
        wait
    }

    /// The renderer's page is ready: the next failure starts from the first wait again.
    fn ready(&mut self) {
        *self = Self::default();
    }
}

struct Service {
    mem: Lru<String, State>,
    queue: VecDeque<Job>,
    running: Option<Running>,
    #[cfg(any(target_os = "macos", windows))]
    renderer: Option<renderer::DiagramRenderer>,
    renderer_ready: bool,
    start_backoff: StartBackoff,
    /// Keys that failed because the renderer did not start: asked for again after the
    /// wait, they are tried again.
    start_failed: HashSet<String>,
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
        #[cfg(any(target_os = "macos", windows))]
        renderer: None,
        renderer_ready: false,
        start_backoff: StartBackoff::default(),
        start_failed: HashSet::new(),
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
    // A diagram that failed only because the renderer did not start gets another try once
    // the wait is over.
    if s.start_failed.contains(&key) && !s.start_backoff.blocked(Instant::now()) {
        s.start_failed.remove(&key);
        s.mem.remove(&key);
    }
    if let Some(state) = s.mem.get(&key).cloned() {
        if state.is_pending()
            && visible
            && let Some(i) = s.queue.iter().position(|j| j.key == key)
            && let Some(mut job) = s.queue.remove(i)
        {
            job.visible = true;
            s.queue.push_front(job);
        }
        return state;
    }
    if let Some((png, meta)) = s.disk.read_ok(&key) {
        let state = State::Ready(decode(png, meta));
        s.mem.insert(key, state.clone());
        return state;
    }
    if let Some(err) = s.disk.read_err(&key) {
        let reason = FailReason::parse(&err.reason);
        let state = State::Failed(
            ErrorKind::from_reason(reason),
            Hint::of(reason, &err.detail),
        );
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
        let s = cx.global_mut::<Service>();
        s.mem.clear();
        s.start_failed.clear();
    }
}

/// The CSS font stack of diagram text: the system's interface and Chinese fonts.
#[cfg(not(windows))]
const DIAGRAM_FONTS: &str =
    "-apple-system, BlinkMacSystemFont, \"PingFang SC\", \"Helvetica Neue\", sans-serif";
#[cfg(windows)]
const DIAGRAM_FONTS: &str = "\"Segoe UI Variable Text\", \"Segoe UI\", \"Microsoft YaHei UI\", \"Microsoft YaHei\", sans-serif";

#[cfg(any(target_os = "macos", windows))]
fn ensure_renderer(cx: &mut App) -> bool {
    let s = cx.global_mut::<Service>();
    if s.renderer.as_ref().is_some_and(|r| !r.is_closed()) {
        return s.renderer_ready;
    }
    s.renderer_ready = false;
    if s.start_backoff.blocked(Instant::now()) {
        return false;
    }
    // Unit tests never open a web view (on Windows any thread could).
    if cfg!(test) {
        return false;
    }
    #[cfg(target_os = "macos")]
    let Some(mtm) = artifact_view::MainThread::new() else {
        return false;
    };
    // The app's globals live on the thread of GPUI's message loop.
    #[cfg(windows)]
    let mtm = artifact_view::MainThread::current();
    let tx = s.tx.clone();
    match renderer::DiagramRenderer::open(mtm, move |event| {
        let _ = tx.unbounded_send(Msg::Renderer(event));
    }) {
        Ok(r) => s.renderer = Some(r),
        Err(e) => renderer_start_failed(&e.to_string(), cx),
    }
    false
}

/// The renderer ended before its page was ready (or did not open): every waiting diagram
/// fails now, and no new renderer opens until the wait is over.
fn renderer_start_failed(reason: &str, cx: &mut App) {
    let s = cx.global_mut::<Service>();
    #[cfg(any(target_os = "macos", windows))]
    {
        s.renderer = None;
    }
    s.renderer_ready = false;
    let wait = s.start_backoff.failed(Instant::now());
    tracing::warn!(
        target: "diagrams",
        %reason,
        failures = s.start_backoff.failures,
        wait_secs = wait.as_secs(),
        "diagram renderer did not start; waiting before the next try"
    );
    fail_waiting(cx);
}

/// Fails the running and every queued diagram as "the renderer is not there": in memory
/// only, so they are tried again after the wait.
fn fail_waiting(cx: &mut App) {
    let s = cx.global_mut::<Service>();
    let jobs: Vec<Job> = s
        .running
        .take()
        .map(|r| r.job)
        .into_iter()
        .chain(s.queue.drain(..))
        .collect();
    if jobs.is_empty() {
        return;
    }
    let reason = FailReason::Internal;
    for job in jobs {
        s.mem.insert(
            job.key.clone(),
            State::Failed(ErrorKind::from_reason(reason), Hint::of(reason, "")),
        );
        s.start_failed.insert(job.key);
    }
    cx.refresh_windows();
}

#[cfg(not(any(target_os = "macos", windows)))]
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
        // No renderer for a while: say so instead of leaving the diagrams waiting.
        if cx.global::<Service>().start_backoff.blocked(Instant::now()) {
            fail_waiting(cx);
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            let s = cx.global_mut::<Service>();
            while let Some(job) = s.queue.pop_front() {
                s.mem
                    .insert(job.key, State::Failed(ErrorKind::Failed, None));
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
    #[cfg(any(target_os = "macos", windows))]
    if let Some(r) = &s.renderer {
        r.request(&request);
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = request;
    s.running = Some(Running { job, req_id });
    let tx = s.tx.clone();
    cx.spawn(async move |cx: &mut AsyncApp| {
        cx.background_executor().timer(TIMEOUT).await;
        let _ = tx.unbounded_send(Msg::Timeout(req_id));
    })
    .detach();
}

/// Starts the first queued job without a renderer window (unit tests run off the main
/// thread, where no web view opens) and gives its request id.
#[cfg(test)]
fn start_next_for_test(cx: &mut App) -> Option<u64> {
    let s = cx.global_mut::<Service>();
    let job = s.queue.pop_front()?;
    s.seq += 1;
    let req_id = s.seq;
    s.running = Some(Running { job, req_id });
    Some(req_id)
}

/// When idle: render the sources seen recently in the other theme, so a theme change
/// draws at once.
fn queue_other_theme(cx: &mut App) {
    let theme = current_theme(cx).other();
    let s = cx.global_mut::<Service>();
    let sources: Vec<(String, DiagramType)> = s.recent.newest_first().into_iter().take(8).collect();
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
    let first = redacted
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(120)
        .collect::<String>();
    tracing::info!(target: "diagrams", reason = reason.as_str(), detail = %first, "diagram not drawn");
    let hint = Hint::of(reason, &first);
    if store {
        let _ = cx.global_mut::<Service>().disk.write_err(
            &job.key,
            &ErrMeta {
                reason: reason.as_str().to_string(),
                detail: first,
            },
        );
    }
    finish(
        job.key,
        State::Failed(ErrorKind::from_reason(reason), hint),
        cx,
    );
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
        #[cfg(any(target_os = "macos", windows))]
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
                        scale: renderer::raster_scale(
                            rendered.width,
                            rendered.height,
                            job.scale as f64,
                        ) as f32,
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
            #[cfg(any(target_os = "macos", windows))]
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

#[cfg(any(target_os = "macos", windows))]
fn handle_renderer(event: renderer::RendererEvent, cx: &mut App) {
    use renderer::RendererEvent;
    match event {
        RendererEvent::Ready => {
            let s = cx.global_mut::<Service>();
            s.renderer_ready = true;
            s.start_backoff.ready();
            pump(cx);
        }
        RendererEvent::Crashed(reason) => {
            // Before its page was ever ready, the renderer cannot start here: opening
            // another one at once would do the same, forever.
            if !cx.global::<Service>().renderer_ready {
                renderer_start_failed(&reason, cx);
                return;
            }
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
                .or_else(|| {
                    (!n.label.is_empty())
                        .then(|| source.find(&n.label))
                        .flatten()
                })
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
    let sep = if kind == DiagramType::Flowchart {
        " → "
    } else {
        "、"
    };
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
        // Gantt task states in greys (§1.3: no state color as decoration):
        // done lightest, active darker, critical darkest; the text stays ink.
        live_mix: mix(k.ink, k.raised, 0.26),
        ok_mix: mix(k.ink, k.raised, 0.12),
        bad_mix: mix(k.ink, k.raised, 0.4),
        font_family: DIAGRAM_FONTS.into(),
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
                    let (_, _, mut f) = open.take().unwrap_or((
                        ch,
                        run,
                        Fence {
                            offset: at,
                            info: String::new(),
                            closed: false,
                        },
                    ));
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
