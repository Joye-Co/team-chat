//! The 产出 page host (inc-host track, INTERACT §5.2–§5.5, REFS §A7): one place for every
//! open 产出 page, in the 产出 window, the 产出 page and pane, and inline in a 话题.
//!
//! - **Capabilities by author** (R10): every page reads its data and may point at its parts;
//!   only a version that a bot member of the 群 made (not deactivated) may `emit` or `ask`,
//!   and only the names it declares, each with its kind ([`version_spec`]).
//! - **Requests**: `emit` and `ask` go to the member at once (never queued; offline rejects
//!   `offline`). An `ask` waits in the pending map until the member's `artifact.reply`, or
//!   rejects `timeout` after `min(timeoutMs, 120 s)`; a late reply is dropped. Events count
//!   only right after the viewer's own input in that page (`user_action`).
//! - **Live data**: a page that opens gets the version's snapshot (`GET …/data`); each
//!   transient `artifact.data` goes to every open page of that version.
//! - **Pending band**: while an emit or ask waits, the page's footer shows 等 {name} 回应
//!   with the INTERACT §4.4 timings ([`band`]).
//! - **New versions**: a page that shows the latest version reloads in place and says
//!   已更新到 v{n} for 3 s; a page on an older version shows 有 v{n} ([`notice`]).
//! - **引用提问**: the bridge `ref` method and ⌥-click on `data-ref-label` call
//!   [`refs::ask_about`] with the page part.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use artifact_view::{
    ArtifactEvent, Capabilities, DataCause, EventDecl, EventKind, RefMenu, Reject, Snapshot,
    VersionSpec, Viewer,
};
use futures::StreamExt as _;
use gpui::{App, AsyncApp, Global, Hsla, SharedString, WeakEntity, Window};
use serde_json::{Value, json};

use crate::{
    app::{Changes, Services},
    copy::{fill, page_host as cp},
    data::{
        Change, ChatProvider, SessionState,
        page_host::{ExpectMode, PageBridge, PageEventKind, PageSendError, PageSignal},
    },
    theme,
    ui::web_embed::{self, WebEmbedState},
    views::refs,
};

/// How long 已更新到 v{n} shows (INTERACT §5.5).
pub const UPDATED_FOR: Duration = Duration::from_secs(3);
/// How long 不在线，这一步没有发出 shows (INTERACT §4.4).
pub const OFFLINE_FOR: Duration = Duration::from_secs(4);
/// Before this, a waiting event shows only its optimistic state (INTERACT §4.4).
pub const QUIET_FOR: Duration = Duration::from_millis(400);

/// Which page a request came from.
#[derive(Clone)]
pub enum PageRef {
    /// The 产出 window of this 产出.
    Window(String),
    /// A page in the main window.
    Embed(WeakEntity<WebEmbedState>),
}

impl PageRef {
    fn same(&self, other: &PageRef) -> bool {
        match (self, other) {
            (PageRef::Window(a), PageRef::Window(b)) => a == b,
            (PageRef::Embed(a), PageRef::Embed(b)) => a == b,
            _ => false,
        }
    }
}

/// What the footer of a page says about its last request.
#[derive(Clone, Debug, PartialEq)]
pub enum Band {
    /// An emit or ask waits for the member; `mode` once the server acked it.
    Waiting {
        since: Instant,
        mode: Option<ExpectMode>,
    },
    /// The member is not connected: nothing went out.
    Offline { until: Instant },
    /// The member cannot take more now (rate limit).
    Busy { until: Instant },
    /// The member pushed new data.
    Pushed,
    /// The member answered an ask in the page.
    Answered,
}

/// The words and state of a footer band at one moment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BandView {
    /// 等 {name} 回应 (spinner).
    Waiting(String),
    /// {name} 还在处理 (spinner).
    Working(String),
    /// {name} 没有回应, with 重试 and 不等了.
    NoAnswer(String),
    /// A short notice that nothing went out (amber).
    Failed(String),
    /// Done (a green check).
    Done(String),
}

struct Pending {
    page: PageRef,
    artifact_id: String,
    version: u32,
    kind: PageEventKind,
    name: String,
    payload: Value,
    reply_id: Option<Value>,
    event_id: Option<String>,
    /// When an ask rejects `timeout` (it keeps it when 重试 moves it to a new event).
    deadline: Option<Instant>,
    /// Earlier events of this request that 重试 replaced: a reply to any of them
    /// answers the page.
    older_events: Vec<String>,
}

/// The last event a version's pages sent (重试): the page that owns the band and what it
/// sent, with its client event id while it waits.
#[derive(Clone)]
struct Last {
    page: PageRef,
    kind: PageEventKind,
    name: String,
    payload: Value,
    cid: Option<String>,
}

/// The app's page host. A global; [`start`] sets it.
pub struct PageHost {
    provider: Option<Arc<dyn ChatProvider>>,
    snapshots: HashMap<(String, u32), Snapshot>,
    fetching: HashSet<(String, u32)>,
    /// By client event id.
    pending: HashMap<String, Pending>,
    /// Server event id → client event id.
    by_event: HashMap<String, String>,
    bands: HashMap<(String, u32), Band>,
    /// The last event of a version (重试).
    last: HashMap<(String, u32), Last>,
    /// The latest version this host saw of each 产出.
    latest: HashMap<String, u32>,
    /// 已更新到 v{n} until.
    updated: HashMap<String, (u32, Instant)>,
    /// Inline pages (by row id) that follow the latest version.
    follow: HashMap<SharedString, bool>,
    /// Self-tests: events sent, as `(artifact, version, name, kind)`.
    sent: Vec<(String, u32, String, PageEventKind)>,
}

impl Global for PageHost {}

impl PageHost {
    fn new(provider: Option<Arc<dyn ChatProvider>>) -> Self {
        Self {
            provider,
            snapshots: HashMap::new(),
            fetching: HashSet::new(),
            pending: HashMap::new(),
            by_event: HashMap::new(),
            bands: HashMap::new(),
            last: HashMap::new(),
            latest: HashMap::new(),
            updated: HashMap::new(),
            follow: HashMap::new(),
            sent: Vec::new(),
        }
    }
}

/// The provider the page host works with.
pub fn provider(cx: &App) -> Option<Arc<dyn ChatProvider>> {
    cx.try_global::<PageHost>().and_then(|h| h.provider.clone())
}

fn host(cx: &mut App) -> &mut PageHost {
    if !cx.has_global::<PageHost>() {
        cx.set_global(PageHost::new(None));
    }
    cx.global_mut::<PageHost>()
}

/// Starts the page host: follows 产出 changes (new versions) and the provider's page
/// signals (acks, replies, data pushes).
pub fn start(services: &Services, cx: &mut App) {
    let provider = services.provider.clone();
    let mut h = PageHost::new(Some(provider.clone()));
    for (_, a) in provider.all_artifacts() {
        h.latest.insert(a.id.clone(), a.version);
    }
    cx.set_global(h);
    cx.subscribe(&services.hub, |_, changes: &Changes, cx| {
        if changes.any(|c| matches!(c, Change::Artifacts { .. })) {
            on_artifacts_changed(changes, cx);
        }
    })
    .detach();
    if let Some(mut rx) = provider.page_signals() {
        cx.spawn(async move |cx: &mut AsyncApp| {
            while let Some(s) = rx.next().await {
                cx.update(|cx| on_signal(s, cx));
            }
        })
        .detach();
    }
}

// --- what a version's page may do ---------------------------------------------------

/// Capabilities by author (R10): every page reads and points; a version an active bot
/// member of the 群 made may also send the events it declares.
pub fn capabilities_for(bridge: Option<&PageBridge>) -> Capabilities {
    let mut caps = Capabilities::READ;
    if let Some(b) = bridge
        && b.author_active_bot
    {
        caps.emit = b.events.iter().any(|e| e.kind == PageEventKind::Emit);
        caps.ask = b.events.iter().any(|e| e.kind == PageEventKind::Ask);
    }
    caps
}

fn decls(bridge: Option<&PageBridge>) -> Vec<EventDecl> {
    bridge
        .map(|b| {
            b.events
                .iter()
                .map(|e| {
                    EventDecl::new(
                        e.name.clone(),
                        match e.kind {
                            PageEventKind::Emit => EventKind::Emit,
                            PageEventKind::Ask => EventKind::Ask,
                        },
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The page of a version on disk, with its declarations and live data, and its
/// capabilities.
pub fn version_spec(
    provider: &dyn ChatProvider,
    artifact_id: &str,
    version: u32,
    dir: std::path::PathBuf,
    entry: String,
) -> (VersionSpec, Capabilities) {
    let bridge = provider.page_bridge(artifact_id, version);
    let spec = VersionSpec::new(version, artifact_view::Content::Folder(dir))
        .with_entry(entry)
        .live_data()
        .with_events(decls(bridge.as_ref()));
    (
        spec,
        page_capabilities(bridge.as_ref(), provider.refs_supported()),
    )
}

/// [`capabilities_for`], and `ref` (the ⌥-click outline, 就这里提问 and `teamchat.ref`)
/// only when the server takes references to a part (`refs/1`): without it nothing could
/// take the chip.
fn page_capabilities(bridge: Option<&PageBridge>, refs_on: bool) -> Capabilities {
    let mut caps = capabilities_for(bridge);
    caps.r#ref &= refs_on;
    caps
}

/// `window.teamchat.viewer`: me.
pub fn viewer(provider: &dyn ChatProvider) -> Option<Viewer> {
    match provider.session() {
        SessionState::SignedIn { me, .. } => Some(Viewer {
            member_id: me.id,
            name: me.name,
        }),
        _ => None,
    }
}

fn hex(c: Hsla) -> String {
    let c: gpui::Rgba = c.into();
    let b = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c.r), b(c.g), b(c.b))
}

/// The ⌥-click outline and `就这里提问` menu, in the app's theme.
pub fn ref_menu(cx: &App) -> RefMenu {
    let k = theme::colors(cx);
    RefMenu {
        label: cp::PAGE_REF_MENU.to_string(),
        bg: hex(k.raised),
        ink: hex(k.ink),
        line: hex(k.line),
        accent: hex(k.accent),
    }
}

/// The name of the member that made a version.
pub fn author_name(provider: &dyn ChatProvider, artifact_id: &str, version: u32) -> String {
    provider
        .page_bridge(artifact_id, version)
        .and_then(|b| provider.member(&b.author_id))
        .map(|m| m.name)
        .unwrap_or_default()
}

// --- page events ----------------------------------------------------------------------

/// Handles a page's event: loads get the data, emits and asks go to the member, refs go
/// to the composer. `window` is the main window when the page is in it.
pub fn on_page_event(
    page: PageRef,
    artifact_id: &str,
    event: &ArtifactEvent,
    window: Option<&mut Window>,
    cx: &mut App,
) {
    match event {
        ArtifactEvent::Loaded { version } => give_data(&page, artifact_id, *version, cx),
        ArtifactEvent::Emit {
            version,
            name,
            payload,
            user_action,
            reply_id,
        } => {
            // Paced (latest wins per name) and gated on the viewer's input.
            crate::views::artifact_page::forward_page_event(
                page,
                artifact_id,
                *version,
                name,
                payload,
                *user_action,
                reply_id.clone(),
                cx,
            );
        }
        ArtifactEvent::Ask {
            version,
            reply_id,
            name,
            payload,
            timeout_ms,
            user_action,
        } => {
            if !user_action {
                tracing::debug!(artifact_id, version, "page ask without user input dropped");
                finish(&page, *version, reply_id, Err(Reject::NotAllowed), cx);
                return;
            }
            if let Some(cid) = send(
                page,
                artifact_id,
                *version,
                PageEventKind::Ask,
                name,
                payload.clone(),
                Some(reply_id.clone()),
                cx,
            ) {
                let ms = (*timeout_ms).min(artifact_view::bridge::ASK_TIMEOUT_MAX_MS);
                time_out_at(cid, Instant::now() + Duration::from_millis(ms), cx);
            }
        }
        ArtifactEvent::Ref {
            version,
            label,
            value,
            selector,
        } => ask_about(artifact_id, *version, label, value, selector, window, cx),
        _ => {}
    }
}

/// The handler a [`web_embed::WebEmbed`] of a 产出 page uses.
pub fn embed_handler(
    artifact_id: String,
) -> impl Fn(&ArtifactEvent, WeakEntity<WebEmbedState>, &mut Window, &mut App) + 'static {
    move |event, me, window, cx| {
        on_page_event(PageRef::Embed(me), &artifact_id, event, Some(window), cx)
    }
}

/// Sends a paced emit now (`artifact_page::forward_page_event` calls it when the
/// limiter lets it go).
pub fn send_emit(
    page: PageRef,
    artifact_id: &str,
    version: u32,
    name: &str,
    payload: Value,
    reply_id: Option<Value>,
    cx: &mut App,
) {
    send(
        page,
        artifact_id,
        version,
        PageEventKind::Emit,
        name,
        payload,
        reply_id,
        cx,
    );
}

/// Answers an open request of one page (see [`finish`]).
pub fn finish_request(
    page: &PageRef,
    version: u32,
    reply_id: &Value,
    result: Result<Value, Reject>,
    cx: &mut App,
) {
    finish(page, version, reply_id, result, cx);
}

/// Sends one event now. Returns its client event id when it went out.
#[allow(clippy::too_many_arguments)]
fn send(
    page: PageRef,
    artifact_id: &str,
    version: u32,
    kind: PageEventKind,
    name: &str,
    payload: Value,
    reply_id: Option<Value>,
    cx: &mut App,
) -> Option<String> {
    let provider = host(cx).provider.clone()?;
    let key = (artifact_id.to_string(), version);
    host(cx).last.insert(
        key.clone(),
        Last {
            page: page.clone(),
            kind,
            name: name.to_string(),
            payload: payload.clone(),
            cid: None,
        },
    );
    let body = (!payload.is_null()).then(|| payload.clone());
    match provider.send_page_event(artifact_id, version, name, kind, body) {
        Ok(cid) => {
            let h = host(cx);
            h.sent
                .push((artifact_id.to_string(), version, name.to_string(), kind));
            // Latest wins: an older emit of the same page stops waiting.
            let older: Vec<String> = h
                .pending
                .iter()
                .filter(|(_, p)| {
                    p.kind == PageEventKind::Emit
                        && kind == PageEventKind::Emit
                        && p.page.same(&page)
                        && p.artifact_id == artifact_id
                })
                .map(|(c, _)| c.clone())
                .collect();
            for c in older {
                if let Some(p) = h.pending.remove(&c) {
                    forget(h, &p);
                }
            }
            if let Some(last) = h.last.get_mut(&key) {
                last.cid = Some(cid.clone());
            }
            h.pending.insert(
                cid.clone(),
                Pending {
                    page,
                    artifact_id: artifact_id.to_string(),
                    version,
                    kind,
                    name: name.to_string(),
                    payload,
                    reply_id,
                    event_id: None,
                    deadline: None,
                    older_events: Vec::new(),
                },
            );
            h.bands.insert(
                key,
                Band::Waiting {
                    since: Instant::now(),
                    mode: None,
                },
            );
            refresh_at(QUIET_FOR, cx);
            cx.refresh_windows();
            Some(cid)
        }
        Err(e) => {
            let reject = match e {
                PageSendError::Offline => Reject::Offline,
                PageSendError::Refused => Reject::Failed,
            };
            if let Some(id) = &reply_id {
                finish(&page, version, id, Err(reject), cx);
            }
            if e == PageSendError::Offline {
                host(cx).bands.insert(
                    key,
                    Band::Offline {
                        until: Instant::now() + OFFLINE_FOR,
                    },
                );
                refresh_at(OFFLINE_FOR, cx);
            }
            cx.refresh_windows();
            None
        }
    }
}

/// Re-renders the windows after `d` (time-based band phases).
fn refresh_at(d: Duration, cx: &mut App) {
    cx.spawn(async move |cx: &mut AsyncApp| {
        cx.background_executor()
            .timer(d + Duration::from_millis(30))
            .await;
        cx.update(|cx| cx.refresh_windows());
    })
    .detach();
}

/// Drops the server event ids of a request that ends.
fn forget(h: &mut PageHost, p: &Pending) {
    for e in p.event_id.iter().chain(&p.older_events) {
        h.by_event.remove(e);
    }
}

/// The ask `cid` rejects `timeout` at `deadline` unless it ended before.
fn time_out_at(cid: String, deadline: Instant, cx: &mut App) {
    if let Some(p) = host(cx).pending.get_mut(&cid) {
        p.deadline = Some(deadline);
    }
    cx.spawn(async move |cx: &mut AsyncApp| {
        cx.background_executor()
            .timer(deadline.saturating_duration_since(Instant::now()))
            .await;
        cx.update(|cx| time_out(&cid, cx));
    })
    .detach();
}

/// An ask that got no reply in time.
fn time_out(cid: &str, cx: &mut App) {
    let Some(p) = host(cx).pending.remove(cid) else {
        return;
    };
    forget(host(cx), &p);
    if let Some(id) = &p.reply_id {
        finish(&p.page, p.version, id, Err(Reject::Timeout), cx);
    }
    cx.refresh_windows();
}

/// Answers an open request of one page.
fn finish(
    page: &PageRef,
    version: u32,
    reply_id: &Value,
    result: Result<Value, Reject>,
    cx: &mut App,
) {
    #[cfg(test)]
    tests::FINISHED.with(|f| f.borrow_mut().push((reply_id.clone(), result.clone())));
    let done = match page {
        PageRef::Window(id) => super::window::finish_in_window(id, version, reply_id, result, cx),
        PageRef::Embed(e) => e
            .read_with(cx, |s, _| s.finish(version, reply_id, result))
            .unwrap_or(false),
    };
    if !done {
        tracing::debug!(version, "page request answered after it ended; dropped");
    }
}

fn reject_for(reason: &str) -> Reject {
    if reason == "client.offline" {
        Reject::Offline
    } else if reason.contains("rate") {
        Reject::RateLimited
    } else if reason.contains("undeclared") || reason.contains("no_bridge") {
        Reject::NotAllowed
    } else {
        Reject::Failed
    }
}

/// A signal from the server (through the provider).
pub fn on_signal(signal: PageSignal, cx: &mut App) {
    match signal {
        PageSignal::Acked {
            client_event_id,
            result,
        } => {
            let Some(p) = host(cx).pending.get_mut(&client_event_id) else {
                return;
            };
            let key = (p.artifact_id.clone(), p.version);
            match result {
                Ok(ack) => {
                    p.event_id = Some(ack.event_id.clone());
                    let (page, version, kind, reply) =
                        (p.page.clone(), p.version, p.kind, p.reply_id.clone());
                    let h = host(cx);
                    h.by_event
                        .insert(ack.event_id.clone(), client_event_id.clone());
                    if kind == PageEventKind::Emit
                        && let Some(id) = reply
                    {
                        finish(
                            &page,
                            version,
                            &id,
                            Ok(json!({ "eventId": ack.event_id })),
                            cx,
                        );
                    }
                    let h = host(cx);
                    match ack.mode {
                        ExpectMode::Offline | ExpectMode::None => {
                            let p = h.pending.remove(&client_event_id);
                            if let Some(p) = &p {
                                forget(h, p);
                            }
                            h.bands.insert(
                                key,
                                Band::Offline {
                                    until: Instant::now() + OFFLINE_FOR,
                                },
                            );
                            if let Some(p) = p
                                && let Some(id) = &p.reply_id
                                && p.kind == PageEventKind::Ask
                            {
                                finish(&p.page, p.version, id, Err(Reject::Offline), cx);
                            }
                            refresh_at(OFFLINE_FOR, cx);
                        }
                        mode => {
                            if let Some(Band::Waiting { mode: m, .. }) = h.bands.get_mut(&key) {
                                *m = Some(mode);
                            }
                            if let Some((soft, hard)) = mode.timings() {
                                refresh_at(Duration::from_millis(soft), cx);
                                refresh_at(Duration::from_millis(hard), cx);
                            }
                        }
                    }
                }
                Err(reason) => {
                    let Some(p) = host(cx).pending.remove(&client_event_id) else {
                        return;
                    };
                    forget(host(cx), &p);
                    let reject = reject_for(&reason);
                    if let Some(id) = &p.reply_id {
                        finish(&p.page, p.version, id, Err(reject), cx);
                    }
                    let until = Instant::now() + OFFLINE_FOR;
                    let band = match reject {
                        Reject::Offline => Some(Band::Offline { until }),
                        Reject::RateLimited => Some(Band::Busy { until }),
                        _ => None,
                    };
                    let h = host(cx);
                    match band {
                        Some(b) => {
                            h.bands.insert(key, b);
                            refresh_at(OFFLINE_FOR, cx);
                        }
                        None => {
                            h.bands.remove(&key);
                        }
                    }
                }
            }
            cx.refresh_windows();
        }
        PageSignal::Reply {
            event_id,
            ok,
            payload,
            declined,
        } => {
            let h = host(cx);
            let Some(cid) = h.by_event.remove(&event_id) else {
                return;
            };
            let Some(p) = h.pending.remove(&cid) else {
                return;
            };
            forget(h, &p);
            h.bands
                .insert((p.artifact_id.clone(), p.version), Band::Answered);
            if let Some(id) = &p.reply_id {
                let result = if ok {
                    Ok(payload.unwrap_or(Value::Null))
                } else if declined {
                    Err(Reject::Declined)
                } else {
                    Err(Reject::Failed)
                };
                finish(&p.page, p.version, id, result, cx);
            }
            cx.refresh_windows();
        }
        PageSignal::Data {
            artifact_id,
            version,
            data_rev,
            data,
            cause,
        } => {
            // The emit that caused it stops waiting; the footer says the data came.
            let h = host(cx);
            let done: Vec<String> = h
                .pending
                .iter()
                .filter(|(_, p)| {
                    p.artifact_id == artifact_id
                        && p.version == version
                        && p.kind == PageEventKind::Emit
                        && (cause.is_none() || p.event_id == cause)
                })
                .map(|(c, _)| c.clone())
                .collect();
            let mut actor = None;
            for c in done {
                if let Some(p) = h.pending.remove(&c) {
                    forget(h, &p);
                    actor = p.event_id.or(actor);
                }
            }
            h.bands.insert((artifact_id.clone(), version), Band::Pushed);
            let me = h
                .provider
                .as_deref()
                .and_then(viewer)
                .map(|v| v.member_id)
                .unwrap_or_default();
            let cause = cause.map(|event_id| DataCause {
                actor_id: if actor.as_deref() == Some(event_id.as_str()) {
                    me
                } else {
                    String::new()
                },
                event_id,
            });
            match data {
                Some(data) => store_and_push(
                    &artifact_id,
                    version,
                    Snapshot {
                        data,
                        rev: data_rev,
                        cause,
                    },
                    cx,
                ),
                None => fetch(&artifact_id, version, cx),
            }
            cx.refresh_windows();
        }
    }
}

// --- data -----------------------------------------------------------------------------

/// A page of `(artifact, version)` loaded: give it the snapshot this host has, and fetch
/// the server's.
fn give_data(page: &PageRef, artifact_id: &str, version: u32, cx: &mut App) {
    let key = (artifact_id.to_string(), version);
    if let Some(s) = host(cx).snapshots.get(&key).cloned() {
        push_to(page, version, s, cx);
    }
    fetch(artifact_id, version, cx);
}

fn fetch(artifact_id: &str, version: u32, cx: &mut App) {
    let key = (artifact_id.to_string(), version);
    let h = host(cx);
    let Some(provider) = h.provider.clone() else {
        return;
    };
    if !h.fetching.insert(key.clone()) {
        return;
    }
    let rx = provider.fetch_page_data(artifact_id, version);
    cx.spawn(async move |cx: &mut AsyncApp| {
        let got = rx.await.ok().flatten();
        cx.update(|cx| {
            host(cx).fetching.remove(&key);
            let snap = match got {
                Some((data, rev)) => Snapshot::new(data, rev),
                // No data (or it could not be read): the page's getData resolves null.
                None => Snapshot::new(Value::Null, 0),
            };
            store_and_push(&key.0, key.1, snap, cx);
        });
    })
    .detach();
}

/// Keeps the newest snapshot of a version and gives it to every open page of it.
pub fn store_and_push(artifact_id: &str, version: u32, snap: Snapshot, cx: &mut App) {
    let key = (artifact_id.to_string(), version);
    let h = host(cx);
    if h.snapshots.get(&key).is_some_and(|s| s.rev > snap.rev) {
        return;
    }
    h.snapshots.insert(key, snap.clone());
    super::window::push_to_window(artifact_id, version, &snap, cx);
    for e in web_embed::all(cx) {
        let s = e.read(cx);
        if s.artifact_id() == artifact_id && s.version() == version {
            s.push_data(version, snap.clone());
        }
    }
}

fn push_to(page: &PageRef, version: u32, snap: Snapshot, cx: &mut App) {
    match page {
        PageRef::Window(id) => {
            super::window::push_to_window(id, version, &snap, cx);
        }
        PageRef::Embed(e) => {
            let _ = e.read_with(cx, |s, _| s.push_data(version, snap));
        }
    }
}

// --- 引用提问 ----------------------------------------------------------------------------

fn ask_about(
    artifact_id: &str,
    version: u32,
    label: &str,
    value: &Option<Value>,
    selector: &Option<String>,
    window: Option<&mut Window>,
    cx: &mut App,
) {
    let Some(provider) = host(cx).provider.clone() else {
        return;
    };
    let Some(detail) = provider.artifact(artifact_id) else {
        return;
    };
    // inc-integrate: the 引用提问 label builder (REFS §A2.2: `{title} v{n} · {label}`, each
    // segment cut at its limit), so the chip draws its lead and part like every other.
    let chip = refs::label::build(
        &refs::parts::artifact_source(&detail.summary.title, version),
        None,
        Some(label),
    )
    .text;
    let mut keys = std::collections::BTreeMap::new();
    if let Some(s) = selector {
        keys.insert(
            "selector".to_string(),
            chat_core::proto::card2::PartKey::Key(selector_key(s)),
        );
    }
    let target = refs::PartTarget {
        source: refs::RefSource::Artifact {
            group_id: detail.group_id.clone(),
            topic_id: detail.summary.topic_id.clone(),
            artifact_id: artifact_id.to_string(),
            version,
        },
        block_id: "page".into(),
        part: Some(chat_core::proto::card2::PartRef {
            kind: "element".into(),
            keys: (!keys.is_empty()).then_some(keys),
        }),
        label: chip,
        value: value.clone().map(refs::value::cap),
        card_rev: None,
    };
    let asked = match window {
        Some(w) => Some(refs::ask_about_outcome(
            target,
            refs::RefIntent::Topic,
            w,
            cx,
        )),
        None => cx
            .active_window()
            .or_else(|| cx.windows().into_iter().next())
            .and_then(|handle| {
                handle
                    .update(cx, |_, w, cx| {
                        refs::ask_about_outcome(target, refs::RefIntent::Topic, w, cx)
                    })
                    .ok()
            }),
    };
    if let Some(text) = asked.and_then(ref_notice) {
        crate::views::files::notify(
            gpui_component::notification::Notification::new().message(text.to_string()),
            cx,
        );
    }
}

/// Most characters of a part key's string value (REFS §A2, `PartRef::validate`).
const PART_KEY_MAX_CHARS: usize = 256;

/// The `selector` key of a page part. The page's own selector when it fits a part key;
/// a longer one (the bridge takes up to 512 characters) becomes `sha256:` and 32 hex
/// digits of its digest. The same element always gets the same key, so a chip dedupes
/// and the send with it is never refused; the label and value keep the meaning.
fn selector_key(selector: &str) -> String {
    if selector.chars().count() <= PART_KEY_MAX_CHARS {
        return selector.to_string();
    }
    use sha2::{Digest as _, Sha256};
    let digest = Sha256::digest(selector.as_bytes());
    let hex: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

/// What the page says after 就这里提问: 已放进话题的输入框 only when the chip was added to
/// the 话题 composer, 已引用 when the part is a chip already. A sixth part already shows
/// 最多引用 5 处 (from [`refs::ask_about_outcome`]); a chip in 单独问 shows in its composer.
fn ref_notice(asked: refs::Asked) -> Option<&'static str> {
    match asked.added {
        crate::data::AddRef::Added if !asked.side_chat => Some(cp::PAGE_REF_ADDED),
        crate::data::AddRef::Duplicate(_) => Some(crate::copy::refs::CITED_HERE),
        _ => None,
    }
}

// --- the footer band ---------------------------------------------------------------------

/// The band of a version's pages now, in words.
pub fn band(artifact_id: &str, version: u32, cx: &App) -> Option<BandView> {
    let h = cx.try_global::<PageHost>()?;
    let b = h.bands.get(&(artifact_id.to_string(), version))?;
    let name = h
        .provider
        .as_deref()
        .map(|p| author_name(p, artifact_id, version))
        .unwrap_or_default();
    let n = [("name", name.as_str())];
    let now = Instant::now();
    match b {
        Band::Waiting { since, mode } => {
            let waited = now.saturating_duration_since(*since);
            if waited < QUIET_FOR {
                return None;
            }
            let (soft, hard) = mode.and_then(|m| m.timings()).unwrap_or((8_000, 45_000));
            let ms = waited.as_millis() as u64;
            Some(if ms >= hard {
                BandView::NoAnswer(fill(cp::PAGE_NO_ANSWER, &n))
            } else if ms >= soft {
                BandView::Working(fill(cp::PAGE_STILL_WORKING, &n))
            } else {
                BandView::Waiting(fill(cp::PAGE_WAITING, &n))
            })
        }
        Band::Offline { until } => {
            (now < *until).then(|| BandView::Failed(fill(cp::PAGE_OFFLINE, &n)))
        }
        Band::Busy { until } => (now < *until).then(|| BandView::Failed(fill(cp::PAGE_BUSY, &n))),
        Band::Pushed => Some(BandView::Done(fill(cp::PAGE_PUSHED, &n))),
        Band::Answered => Some(BandView::Done(fill(cp::PAGE_ANSWERED, &n))),
    }
}

/// 重试: sends the version's last event again (a new event, same value) from the page
/// that sent it. A request that still waits moves to the new event with its deadline,
/// so the reply to either event answers the page's promise once.
pub fn retry(artifact_id: &str, version: u32, cx: &mut App) {
    let key = (artifact_id.to_string(), version);
    let Some(last) = host(cx).last.get(&key).cloned() else {
        return;
    };
    let old = last.cid.as_ref().and_then(|c| host(cx).pending.remove(c));
    let reply_id = old.as_ref().and_then(|p| p.reply_id.clone());
    let sent = send(
        last.page,
        artifact_id,
        version,
        last.kind,
        &last.name,
        last.payload,
        reply_id,
        cx,
    );
    let Some(old) = old else {
        return;
    };
    let h = host(cx);
    let Some(cid) = sent else {
        // Nothing went out: send() ended the page's request.
        forget(h, &old);
        return;
    };
    let events: Vec<String> = old.older_events.into_iter().chain(old.event_id).collect();
    for e in &events {
        h.by_event.insert(e.clone(), cid.clone());
    }
    if let Some(p) = h.pending.get_mut(&cid) {
        p.older_events = events;
    }
    if let Some(deadline) = old.deadline {
        time_out_at(cid, deadline, cx);
    }
}

/// 不等了: drops the waiting state; open asks of that version end with `timeout`.
pub fn stop_waiting(artifact_id: &str, version: u32, cx: &mut App) {
    let h = host(cx);
    h.bands.remove(&(artifact_id.to_string(), version));
    let ids: Vec<String> = h
        .pending
        .iter()
        .filter(|(_, p)| p.artifact_id == artifact_id && p.version == version)
        .map(|(c, _)| c.clone())
        .collect();
    for c in ids {
        time_out(&c, cx);
    }
    cx.refresh_windows();
}

/// The footer line of a version: what its page may do, from its declarations.
pub fn capability_line(provider: &dyn ChatProvider, artifact_id: &str, version: u32) -> String {
    let bridge = provider.page_bridge(artifact_id, version);
    let caps = capabilities_for(bridge.as_ref());
    let Some(b) = bridge.filter(|_| caps.emit || caps.ask) else {
        return cp::PAGE_READ_ONLY.to_string();
    };
    let name = provider
        .member(&b.author_id)
        .map(|m| m.name)
        .unwrap_or_default();
    let mut parts = vec![fill(cp::PAGE_SENDS_TO, &[("name", &name)])];
    parts.extend(
        b.events
            .iter()
            .filter_map(|e| e.label.as_ref())
            .map(|l| l.chars().take(40).collect::<String>()),
    );
    if b.runner && caps.ask {
        parts.push(fill(cp::PAGE_RUN_LOG, &[("name", &name)]));
    }
    parts.join(" · ")
}

/// The footer of a 产出 page frame: what the page may do on the left, and the band of
/// its last request on the right (INTERACT §4.4, §5.5; Interact.dc.html 产出里的页面).
pub fn page_footer(
    id: &SharedString,
    provider: &dyn ChatProvider,
    artifact_id: &str,
    version: u32,
    lead: Option<gpui::AnyElement>,
    cx: &App,
) -> gpui::Div {
    use crate::{
        theme::{Type as _, radius, space},
        ui::{self, Lucide},
    };
    use gpui::{InteractiveElement as _, ParentElement as _, Styled as _, div, px};
    use gpui_component::Sizable as _;
    let k = theme::colors(cx);
    let line = capability_line(provider, artifact_id, version);
    let base = crate::views::media::parts::footer(cx)
        .children(lead)
        .child(div().flex_1().min_w(px(0.)).one_line().child(line));
    let pill = || {
        div()
            .flex_none()
            .h(px(24.))
            .px(space::S8)
            .rounded(radius::MD)
            .border_1()
            .border_color(k.border)
            .bg(k.content)
            .flex()
            .items_center()
            .gap(space::S6)
            .t_meta()
            .text_color(k.text_2)
            .whitespace_nowrap()
    };
    let Some(view) = band(artifact_id, version, cx) else {
        return base;
    };
    let busy = |text: String| {
        pill()
            .child(
                ui::spinner()
                    .with_size(gpui_component::Size::XSmall)
                    .color(k.live),
            )
            .child(text)
    };
    match view {
        BandView::Waiting(t) | BandView::Working(t) => base.child(busy(t)),
        BandView::NoAnswer(t) => {
            let (a, b) = (artifact_id.to_string(), artifact_id.to_string());
            base.child(div().flex_none().text_color(k.text_2).child(t))
                .child(
                    ui::button(
                        SharedString::from(format!("{id}-retry")),
                        cp::PAGE_RETRY,
                        ui::ButtonKind::Ghost,
                        ui::ButtonSize::Sm,
                        cx,
                    )
                    .on_click(move |_, _, cx| retry(&a, version, cx)),
                )
                .child(
                    ui::button(
                        SharedString::from(format!("{id}-stop")),
                        cp::PAGE_STOP_WAITING,
                        ui::ButtonKind::Ghost,
                        ui::ButtonSize::Sm,
                        cx,
                    )
                    .on_click(move |_, _, cx| stop_waiting(&b, version, cx)),
                )
        }
        BandView::Failed(t) => base.child(
            pill()
                .child(ui::icon(Lucide::CircleAlert, px(13.), k.ink_3))
                .child(t),
        ),
        BandView::Done(t) => base.child(
            pill()
                .id(SharedString::from(format!("{id}-band")))
                .child(ui::icon(Lucide::Check, px(13.), k.ok))
                .child(t),
        ),
    }
}

/// A chip for the header of a page frame: 已更新到 v{n} or 有 v{n}.
pub fn notice_chip(text: String, cx: &App) -> gpui::Div {
    use crate::theme::{Type as _, radius, space};
    use gpui::{ParentElement as _, Styled as _, div, px};
    let k = theme::colors(cx);
    div()
        .flex_none()
        .h(px(20.))
        .px(space::S6)
        .rounded(radius::SM)
        .bg(k.info_bg)
        .flex()
        .items_center()
        .t_meta()
        .text_color(k.info_text)
        .whitespace_nowrap()
        .child(text)
}

// --- new versions -------------------------------------------------------------------------

/// What the header of a page that shows `shown` says about newer versions:
/// `已更新到 v{n}` for 3 s after it reloaded in place, or `有 v{n}`.
pub fn notice(artifact_id: &str, shown: u32, latest: u32, cx: &App) -> Option<String> {
    let h = cx.try_global::<PageHost>();
    if let Some((n, at)) = h.and_then(|h| h.updated.get(artifact_id))
        && *n == shown
        && at.elapsed() < UPDATED_FOR
    {
        return Some(fill(cp::PAGE_UPDATED_TO, &[("n", &n.to_string())]));
    }
    (latest > shown).then(|| fill(cp::PAGE_NEWER, &[("n", &latest.to_string())]))
}

/// The version an inline page of row `id` shows: the latest when the row pointed at the
/// latest version when it first drew (it reloads in place), else the version it points at.
pub fn inline_version(id: &SharedString, artifact_id: &str, points_at: u32, cx: &mut App) -> u32 {
    let h = host(cx);
    let latest = h.latest.get(artifact_id).copied().unwrap_or(points_at);
    let following = *h.follow.entry(id.clone()).or_insert(points_at >= latest);
    if following {
        latest.max(points_at)
    } else {
        points_at
    }
}

/// A new version of `artifact_id` exists (also from self-tests).
pub fn version_added(artifact_id: &str, version: u32, cx: &mut App) {
    let h = host(cx);
    let before = h.latest.get(artifact_id).copied().unwrap_or(0);
    if version <= before {
        return;
    }
    h.latest.insert(artifact_id.to_string(), version);
    h.updated
        .insert(artifact_id.to_string(), (version, Instant::now()));
    // Requests of the pages that reload end (their page is gone).
    let ended: Vec<String> = h
        .pending
        .iter()
        .filter(|(_, p)| p.artifact_id == artifact_id && p.version == before)
        .map(|(c, _)| c.clone())
        .collect();
    for c in ended {
        if let Some(p) = h.pending.remove(&c)
            && let Some(e) = p.event_id
        {
            h.by_event.remove(&e);
        }
    }
    h.bands.remove(&(artifact_id.to_string(), before));
    super::window::on_version_added(artifact_id, before, version, cx);
    refresh_at(UPDATED_FOR, cx);
    cx.refresh_windows();
}

/// The latest version this host knows of a 产出.
pub fn latest(artifact_id: &str, cx: &App) -> Option<u32> {
    cx.try_global::<PageHost>()
        .and_then(|h| h.latest.get(artifact_id).copied())
}

fn on_artifacts_changed(changes: &Changes, cx: &mut App) {
    let Some(provider) = host(cx).provider.clone() else {
        return;
    };
    let mut seen = HashSet::new();
    for c in &changes.0 {
        if let Change::Artifacts { group_id, topic_id } = c
            && seen.insert((group_id.clone(), topic_id.clone()))
        {
            for a in provider.artifacts(group_id, topic_id) {
                let known = host(cx).latest.get(&a.id).copied();
                match known {
                    None => {
                        host(cx).latest.insert(a.id.clone(), a.version);
                    }
                    Some(v) if a.version > v => version_added(&a.id, a.version, cx),
                    Some(_) => {}
                }
            }
        }
    }
}

// --- self-tests ---------------------------------------------------------------------------

/// Events this host sent, as `(artifact, version, name, kind)` (self-tests).
pub fn sent(cx: &App) -> Vec<(String, u32, String, PageEventKind)> {
    cx.try_global::<PageHost>()
        .map(|h| h.sent.clone())
        .unwrap_or_default()
}

/// Sets the latest version this host knows of a 产出 (self-tests start from a known
/// version, also on the worst-case data).
pub fn set_latest(artifact_id: &str, version: u32, cx: &mut App) {
    host(cx).latest.insert(artifact_id.to_string(), version);
}

/// Starts clean, with `provider` (self-tests that run without the app's services).
pub fn reset_for_test(provider: Arc<dyn ChatProvider>, cx: &mut App) {
    cx.set_global(PageHost::new(Some(provider)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::page_host::{PageAck, PageEventDecl};
    use std::cell::RefCell;

    type Finished = Vec<(Value, Result<Value, Reject>)>;

    thread_local! {
        /// Page requests [`finish`] answered on this test's thread.
        pub(super) static FINISHED: RefCell<Finished> = const { RefCell::new(Vec::new()) };
    }

    fn finished() -> Finished {
        FINISHED.with(|f| std::mem::take(&mut *f.borrow_mut()))
    }

    const ART: &str = crate::fake::v2::ART_REPORT;

    /// A page host on the v2 samples (Ledger's report v3 declares `filter` and
    /// `question`).
    fn start_host(cx: &mut App) {
        let provider: Arc<dyn ChatProvider> = crate::fake::v2::V2Samples::new();
        reset_for_test(provider, cx);
        finished();
    }

    fn ask(page: PageRef, reply: i64, cx: &mut App) -> String {
        let before: HashSet<String> = host(cx).pending.keys().cloned().collect();
        on_page_event(
            page,
            ART,
            &ArtifactEvent::Ask {
                version: 3,
                reply_id: json!(reply),
                name: "question".into(),
                payload: json!({ "q": "B 站" }),
                timeout_ms: 120_000,
                user_action: true,
            },
            None,
            cx,
        );
        new_cid(&before, cx)
    }

    fn new_cid(before: &HashSet<String>, cx: &mut App) -> String {
        let now: Vec<String> = host(cx)
            .pending
            .keys()
            .filter(|c| !before.contains(*c))
            .cloned()
            .collect();
        assert_eq!(now.len(), 1, "expected one new request");
        now[0].clone()
    }

    fn ack(cid: &str, event: &str, mode: ExpectMode, cx: &mut App) {
        on_signal(
            PageSignal::Acked {
                client_event_id: cid.into(),
                result: Ok(PageAck {
                    event_id: event.into(),
                    mode,
                }),
            },
            cx,
        );
    }

    fn reply(event: &str, cx: &mut App) {
        on_signal(
            PageSignal::Reply {
                event_id: event.into(),
                ok: true,
                payload: Some(json!({ "answer": event })),
                declined: false,
            },
            cx,
        );
    }

    fn retry_now(cx: &mut App) -> String {
        let before: HashSet<String> = host(cx).pending.keys().cloned().collect();
        retry(ART, 3, cx);
        new_cid(&before, cx)
    }

    #[gpui::test]
    fn retry_of_an_ask_answers_the_page_from_the_new_event(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            start_host(cx);
            // The ask came from an inline page, not the 产出 window.
            let first = ask(PageRef::Embed(WeakEntity::new_invalid()), 7, cx);
            ack(&first, "ev-1", ExpectMode::Live, cx);
            let second = retry_now(cx);
            assert_ne!(first, second);
            let p = &host(cx).pending[&second];
            assert!(
                matches!(p.page, PageRef::Embed(_)),
                "retry bound to another page"
            );
            assert_eq!(p.reply_id, Some(json!(7)));
            assert!(p.deadline.is_some(), "the retried ask lost its deadline");
            assert!(!host(cx).pending.contains_key(&first));
            ack(&second, "ev-2", ExpectMode::Live, cx);
            // The member answers the retried event: the page's promise resolves once.
            reply("ev-2", cx);
            assert_eq!(
                finished(),
                vec![(json!(7), Ok(json!({ "answer": "ev-2" })))]
            );
            // A late answer to the first event finds nothing open.
            reply("ev-1", cx);
            assert!(finished().is_empty());
            assert!(host(cx).pending.is_empty() && host(cx).by_event.is_empty());
        });
    }

    #[gpui::test]
    fn a_reply_to_the_replaced_event_still_answers_the_page(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            start_host(cx);
            let first = ask(PageRef::Window(ART.into()), 8, cx);
            ack(&first, "ev-1", ExpectMode::Run, cx);
            let second = retry_now(cx);
            ack(&second, "ev-2", ExpectMode::Run, cx);
            reply("ev-1", cx);
            assert_eq!(
                finished(),
                vec![(json!(8), Ok(json!({ "answer": "ev-1" })))]
            );
            assert!(host(cx).pending.is_empty() && host(cx).by_event.is_empty());
        });
    }

    fn bridge(active: bool, kinds: &[PageEventKind]) -> PageBridge {
        PageBridge {
            author_id: "m-ledger".into(),
            author_active_bot: active,
            runner: false,
            events: kinds
                .iter()
                .enumerate()
                .map(|(i, k)| PageEventDecl {
                    name: format!("e{i}"),
                    kind: *k,
                    label: None,
                })
                .collect(),
            data_rev: 0,
        }
    }

    #[test]
    fn capabilities_follow_the_author() {
        // A person's page, or no facts yet: read and point only.
        assert_eq!(capabilities_for(None), Capabilities::READ);
        let person = bridge(false, &[PageEventKind::Emit, PageEventKind::Ask]);
        assert_eq!(capabilities_for(Some(&person)), Capabilities::READ);
        // An active bot member: only the kinds it declares.
        let c = capabilities_for(Some(&bridge(true, &[PageEventKind::Emit])));
        assert!(c.emit && !c.ask && c.get_data && c.live_data && c.r#ref);
        let c = capabilities_for(Some(&bridge(true, &[PageEventKind::Ask])));
        assert!(!c.emit && c.ask);
        let c = capabilities_for(Some(&bridge(true, &[])));
        assert!(!c.emit && !c.ask);
    }

    #[test]
    fn pages_point_at_parts_only_when_the_server_takes_refs() {
        let b = bridge(true, &[PageEventKind::Emit]);
        let on = page_capabilities(Some(&b), true);
        assert!(on.r#ref && on.emit);
        let off = page_capabilities(Some(&b), false);
        assert!(!off.r#ref && off.emit && off.get_data);
        assert!(!page_capabilities(None, false).r#ref);
    }

    #[test]
    fn page_selectors_fit_a_part_key_and_stay_stable() {
        use chat_core::proto::card2::{PartKey, PartRef};
        let part = |s: String| PartRef {
            kind: "element".into(),
            keys: Some([("selector".to_string(), PartKey::Key(s))].into()),
        };
        // A short selector stays as the page gave it.
        assert_eq!(selector_key("#bili"), "#bili");
        let exact = "a".repeat(PART_KEY_MAX_CHARS);
        assert_eq!(selector_key(&exact), exact);
        // A 6-level path of long ids (the bridge allows 512): a digest that validates.
        let long = (0..6)
            .map(|i| format!("#{}{i}", "频道".repeat(40)))
            .collect::<Vec<_>>()
            .join(" > ");
        assert!(long.chars().count() > PART_KEY_MAX_CHARS);
        let key = selector_key(&long);
        assert!(key.starts_with("sha256:") && key.chars().count() == 39);
        assert!(part(key.clone()).validate().is_ok());
        assert!(part(long.clone()).validate().is_err());
        // Stable: the same element gets the same key; another element another one.
        assert_eq!(selector_key(&long), key);
        assert_ne!(selector_key(&format!("{long}x")), key);
    }

    #[test]
    fn the_page_confirms_a_ref_only_when_it_was_added() {
        use crate::data::AddRef;
        let at = |added, side_chat| ref_notice(refs::Asked { added, side_chat });
        assert_eq!(at(AddRef::Added, false), Some(cp::PAGE_REF_ADDED));
        // In 单独问 the chip shows in that composer; 已放进话题的输入框 would be wrong.
        assert_eq!(at(AddRef::Added, true), None);
        assert_eq!(
            at(AddRef::Duplicate(2), false),
            Some(crate::copy::refs::CITED_HERE)
        );
        // 最多引用 5 处 is shown by refs; no second, contradicting toast.
        assert_eq!(at(AddRef::Full, false), None);
        assert_eq!(at(AddRef::Unsupported, false), None);
    }

    #[test]
    fn declarations_keep_their_kind() {
        let d = decls(Some(&bridge(
            true,
            &[PageEventKind::Emit, PageEventKind::Ask],
        )));
        assert_eq!(
            d,
            vec![
                EventDecl::new("e0", EventKind::Emit),
                EventDecl::new("e1", EventKind::Ask)
            ]
        );
    }

    #[test]
    fn server_reasons_become_page_errors() {
        assert_eq!(reject_for("client.offline"), Reject::Offline);
        assert_eq!(reject_for("interact.rate_limited"), Reject::RateLimited);
        assert_eq!(reject_for("artifact.undeclared_event"), Reject::NotAllowed);
        assert_eq!(reject_for("artifact.no_bridge"), Reject::NotAllowed);
        assert_eq!(reject_for(""), Reject::Failed);
    }

    #[test]
    fn colours_become_css_hex() {
        assert_eq!(hex(gpui::hsla(0., 0., 1., 1.)), "#ffffff");
        assert_eq!(hex(gpui::hsla(0., 0., 0., 1.)), "#000000");
    }
}
