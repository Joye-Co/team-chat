//! The page-to-host bridge (INTERACT §5.1, REFS §A7): request parsing, capability and
//! declaration checks, deferred replies, data pushes and the scripts the host injects.
//!
//! A page calls `window.webkit.messageHandlers.bridge.postMessage({id, method, params})`
//! through the injected [`page_script`]. One [`Bridge`] per page checks each request and
//! answers at once, or keeps the request open until the host finishes it ([`Bridge::finish`]:
//! `emit` after the server's ack, `ask` after the member's reply or a timeout) or until the
//! first data snapshot arrives ([`Bridge::push_data`]: `getData` and `subscribe`). Every reply
//! and push goes to the page as a script built from a serde-serialized literal, so page data
//! cannot break out of the expression.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};

/// What a page may ask the host for (INTERACT §5.2, REFS §A7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// `getData`: read the version's latest data snapshot.
    pub get_data: bool,
    /// `onData` / `subscribe`: get each new snapshot the member pushes.
    pub live_data: bool,
    /// `emit` (and the legacy `emitEvent`): send a declared event to the member.
    pub emit: bool,
    /// `ask`: send a declared question and wait for the member's reply.
    pub ask: bool,
    /// `ref`: put a part of the page into the viewer's own composer (引用提问).
    pub r#ref: bool,
}

impl Capabilities {
    pub const NONE: Self = Self {
        get_data: false,
        live_data: false,
        emit: false,
        ask: false,
        r#ref: false,
    };
    pub const ALL: Self = Self {
        get_data: true,
        live_data: true,
        emit: true,
        ask: true,
        r#ref: true,
    };
    /// A version a person made, or a bot that left or was deactivated: read data, follow
    /// pushes and point at parts; never send anything to a member.
    pub const READ: Self = Self {
        get_data: true,
        live_data: true,
        emit: false,
        ask: false,
        r#ref: true,
    };
}

/// `emit` (fire and forget) or `ask` (the page waits for the member's reply).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Emit,
    Ask,
}

/// One event a version declares (`ArtifactVersion.events`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EventDecl {
    pub name: String,
    pub kind: EventKind,
}

impl EventDecl {
    pub fn new(name: impl Into<String>, kind: EventKind) -> Self {
        Self {
            name: name.into(),
            kind,
        }
    }
}

/// Who looks at the page (`window.teamchat.viewer`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewer {
    pub member_id: String,
    pub name: String,
}

/// Colours and words of the injected ⌥-click outline and the `就这里提问` menu. The host
/// passes them from its theme and copy catalog; the page never chooses them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RefMenu {
    pub label: String,
    /// CSS colours (`#rrggbb`).
    pub bg: String,
    pub ink: String,
    pub line: String,
    pub accent: String,
}

/// What the cause of a data push was (`meta.cause` in `onData`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataCause {
    pub event_id: String,
    pub actor_id: String,
}

/// One data snapshot of a version.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub data: Value,
    pub rev: u64,
    pub cause: Option<DataCause>,
}

impl Snapshot {
    pub fn new(data: Value, rev: u64) -> Self {
        Self {
            data,
            rev,
            cause: None,
        }
    }

    fn meta(&self) -> Value {
        match &self.cause {
            Some(c) => json!({ "rev": self.rev, "cause": c }),
            None => json!({ "rev": self.rev }),
        }
    }
}

/// Why a request failed (`Error.code` in the page).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Reject {
    NotAllowed,
    BadRequest,
    TooLarge,
    RateLimited,
    Offline,
    Timeout,
    Declined,
    Failed,
    Reloaded,
    UnknownMethod,
}

impl Reject {
    pub fn code(self) -> &'static str {
        match self {
            Reject::NotAllowed => "not_allowed",
            Reject::BadRequest => "bad_request",
            Reject::TooLarge => "too_large",
            Reject::RateLimited => "rate_limited",
            Reject::Offline => "offline",
            Reject::Timeout => "timeout",
            Reject::Declined => "declined",
            Reject::Failed => "failed",
            Reject::Reloaded => "reloaded",
            Reject::UnknownMethod => "unknown_method",
        }
    }
}

/// Largest accepted bridge message (serialized JSON).
pub const MAX_MESSAGE_BYTES: usize = 256 * 1024;
/// Largest `emit` / `ask` payload (INTERACT §5.6).
pub const MAX_PAYLOAD_BYTES: usize = 16 * 1024;
/// Largest `ref` value (REFS §A7).
pub const MAX_REF_VALUE_BYTES: usize = 2048;
pub const MAX_REF_LABEL_CHARS: usize = 80;
pub const MAX_REF_SELECTOR_CHARS: usize = 512;
/// `ref` calls per second per page.
pub const REF_PER_SECOND: usize = 2;
/// Open `ask`s per page (INTERACT §5.5).
pub const MAX_OPEN_ASKS: usize = 4;
pub const ASK_TIMEOUT_DEFAULT_MS: u64 = 30_000;
pub const ASK_TIMEOUT_MAX_MS: u64 = 120_000;
const MAX_EVENT_NAME: usize = 128;

/// Something the page wants the host to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// Send a declared `emit` event. `reply_id` is `None` for the legacy `emitEvent`, which
    /// the bridge answered already; otherwise the host calls [`Bridge::finish`] with
    /// `{eventId}` or an error.
    Emit {
        reply_id: Option<Value>,
        name: String,
        payload: Value,
    },
    /// Send a declared `ask` and finish `reply_id` with the reply, an error, or
    /// [`Reject::Timeout`] after `timeout_ms`.
    Ask {
        reply_id: Value,
        name: String,
        payload: Value,
        timeout_ms: u64,
    },
    /// Put a part of the page into the viewer's composer. Already answered.
    Ref {
        label: String,
        value: Option<Value>,
        selector: Option<String>,
    },
}

/// The result of one page request.
#[derive(Debug, PartialEq)]
pub struct Outcome {
    /// The script that answers the page now, or `None` while the request stays open.
    pub reply: Option<String>,
    pub request: Option<Request>,
}

impl Outcome {
    fn reply(reply: Value) -> Self {
        Self {
            reply: Some(reply_script(&reply)),
            request: None,
        }
    }

    fn deferred(request: Option<Request>) -> Self {
        Self {
            reply: None,
            request,
        }
    }
}

fn err_value(id: &Value, code: &str, message: &str) -> Value {
    json!({ "id": id, "error": { "code": code, "message": message } })
}

fn err(id: &Value, r: Reject, message: &str) -> Outcome {
    Outcome::reply(err_value(id, r.code(), message))
}

fn key(id: &Value) -> String {
    id.to_string()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Open {
    Emit,
    Ask,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Waiter {
    Get,
    Subscribe,
}

/// The bridge state of one page. Platform independent; the window and the embedded view
/// keep one each and replace it on every load.
#[derive(Debug)]
pub struct Bridge {
    caps: Capabilities,
    events: Option<Vec<EventDecl>>,
    snapshot: Option<Snapshot>,
    waiting: Vec<(Value, Waiter)>,
    subscribed: bool,
    open: HashMap<String, (Value, Open)>,
    refs: VecDeque<Instant>,
    max_bytes: usize,
    max_payload_bytes: usize,
}

impl Bridge {
    /// `events: None` puts no declaration rule on `emit` (self-tests and the diagram
    /// renderer). `initial: None` makes `getData` and `subscribe` wait for the host's first
    /// [`push_data`](Self::push_data) (the `GET …/data` when the page opens).
    pub fn new(
        caps: Capabilities,
        events: Option<Vec<EventDecl>>,
        initial: Option<Snapshot>,
    ) -> Self {
        Self {
            caps,
            events,
            snapshot: initial,
            waiting: Vec::new(),
            subscribed: false,
            open: HashMap::new(),
            refs: VecDeque::new(),
            max_bytes: MAX_MESSAGE_BYTES,
            max_payload_bytes: MAX_PAYLOAD_BYTES,
        }
    }

    pub fn with_max_bytes(mut self, max_bytes: usize) -> Self {
        self.max_bytes = max_bytes;
        self
    }

    pub fn set_max_bytes(&mut self, max_bytes: usize) {
        self.max_bytes = max_bytes;
    }

    /// Largest `emit` / `ask` payload. Default [`MAX_PAYLOAD_BYTES`] (an 产出 page); the
    /// diagram renderer sends whole SVGs and uses its message limit.
    pub fn with_max_payload_bytes(mut self, max_payload_bytes: usize) -> Self {
        self.max_payload_bytes = max_payload_bytes;
        self
    }

    pub fn set_max_payload_bytes(&mut self, max_payload_bytes: usize) {
        self.max_payload_bytes = max_payload_bytes;
    }

    pub fn caps(&self) -> Capabilities {
        self.caps
    }

    pub fn events(&self) -> Option<&[EventDecl]> {
        self.events.as_deref()
    }

    /// Changes what the page may do from now on (the author's member record changed).
    pub fn set_caps(&mut self, caps: Capabilities) {
        self.caps = caps;
    }

    /// The latest snapshot the page has, if any.
    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.snapshot.as_ref()
    }

    /// Requests that wait for the host (`emit` acks and `ask` replies).
    pub fn open_requests(&self) -> usize {
        self.open.len()
    }

    /// True while the request `reply_id` waits for [`finish`](Self::finish).
    pub fn is_open(&self, reply_id: &Value) -> bool {
        self.open.contains_key(&key(reply_id))
    }

    /// The declared kind of `name`, or `Some(kind)` for any name when nothing is declared.
    fn declared(&self, name: &str, kind: EventKind) -> bool {
        match &self.events {
            None => true,
            Some(list) => list.iter().any(|e| e.name == name && e.kind == kind),
        }
    }

    /// Handles one raw message (`{ id, method, params }`) from the page. `now` paces `ref`.
    pub fn handle(&mut self, raw: &[u8], now: Instant) -> Outcome {
        if raw.len() > self.max_bytes {
            return err(&Value::Null, Reject::TooLarge, "message too large");
        }
        let Ok(Value::Object(msg)) = serde_json::from_slice::<Value>(raw) else {
            return err(
                &Value::Null,
                Reject::BadRequest,
                "message must be an object",
            );
        };
        let id = match msg.get("id") {
            Some(v @ (Value::String(_) | Value::Number(_))) => v.clone(),
            _ => {
                return err(
                    &Value::Null,
                    Reject::BadRequest,
                    "id must be a string or number",
                );
            }
        };
        if let Value::String(s) = &id
            && s.len() > 128
        {
            return err(&Value::Null, Reject::BadRequest, "id too long");
        }
        if self.open.contains_key(&key(&id)) || self.waiting.iter().any(|(w, _)| *w == id) {
            return err(&id, Reject::BadRequest, "id in use");
        }
        let Some(method) = msg.get("method").and_then(Value::as_str) else {
            return err(&id, Reject::BadRequest, "method must be a string");
        };
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        match method {
            "getData" => self.get_data(id, Waiter::Get),
            "subscribe" => self.get_data(id, Waiter::Subscribe),
            "emit" | "emitEvent" => self.emit(id, &params, method == "emitEvent"),
            "ask" => self.ask(id, &params),
            "ref" => self.make_ref(id, &params, now),
            _ => err(&id, Reject::UnknownMethod, "unknown method"),
        }
    }

    fn get_data(&mut self, id: Value, waiter: Waiter) -> Outcome {
        let allowed = match waiter {
            Waiter::Get => self.caps.get_data,
            Waiter::Subscribe => self.caps.live_data,
        };
        if !allowed {
            return err(&id, Reject::NotAllowed, "capability not granted");
        }
        if waiter == Waiter::Subscribe {
            self.subscribed = true;
        }
        match &self.snapshot {
            Some(s) => Outcome::reply(json!({ "id": id, "result": answer(s, waiter) })),
            None => {
                self.waiting.push((id, waiter));
                Outcome::deferred(None)
            }
        }
    }

    fn event_params(&self, id: &Value, params: &Value) -> Result<(String, Value), Box<Outcome>> {
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return Err(Box::new(err(
                id,
                Reject::BadRequest,
                "params.name must be a string",
            )));
        };
        if name.is_empty() || name.len() > MAX_EVENT_NAME {
            return Err(Box::new(err(id, Reject::BadRequest, "params.name length")));
        }
        let payload = params.get("payload").cloned().unwrap_or(Value::Null);
        if serde_json::to_vec(&payload).map_or(usize::MAX, |b| b.len()) > self.max_payload_bytes {
            return Err(Box::new(err(id, Reject::TooLarge, "payload too large")));
        }
        Ok((name.to_string(), payload))
    }

    fn emit(&mut self, id: Value, params: &Value, legacy: bool) -> Outcome {
        if !self.caps.emit {
            return err(&id, Reject::NotAllowed, "capability not granted");
        }
        let (name, payload) = match self.event_params(&id, params) {
            Ok(p) => p,
            Err(o) => return *o,
        };
        if !self.declared(&name, EventKind::Emit) {
            return err(&id, Reject::NotAllowed, "event not declared");
        }
        if legacy {
            return Outcome {
                reply: Some(reply_script(&json!({ "id": id, "result": true }))),
                request: Some(Request::Emit {
                    reply_id: None,
                    name,
                    payload,
                }),
            };
        }
        self.open.insert(key(&id), (id.clone(), Open::Emit));
        Outcome::deferred(Some(Request::Emit {
            reply_id: Some(id),
            name,
            payload,
        }))
    }

    fn ask(&mut self, id: Value, params: &Value) -> Outcome {
        if !self.caps.ask {
            return err(&id, Reject::NotAllowed, "capability not granted");
        }
        let (name, payload) = match self.event_params(&id, params) {
            Ok(p) => p,
            Err(o) => return *o,
        };
        if !self.declared(&name, EventKind::Ask) {
            return err(&id, Reject::NotAllowed, "event not declared");
        }
        let asks = self.open.values().filter(|(_, o)| *o == Open::Ask).count();
        if asks >= MAX_OPEN_ASKS {
            return err(&id, Reject::RateLimited, "too many open asks");
        }
        let timeout_ms = match params.get("timeoutMs") {
            None | Some(Value::Null) => ASK_TIMEOUT_DEFAULT_MS,
            Some(v) => match v.as_f64() {
                Some(ms) if ms.is_finite() && ms > 0. => (ms as u64).clamp(1, ASK_TIMEOUT_MAX_MS),
                _ => {
                    return err(
                        &id,
                        Reject::BadRequest,
                        "timeoutMs must be a positive number",
                    );
                }
            },
        };
        self.open.insert(key(&id), (id.clone(), Open::Ask));
        Outcome::deferred(Some(Request::Ask {
            reply_id: id,
            name,
            payload,
            timeout_ms,
        }))
    }

    fn make_ref(&mut self, id: Value, params: &Value, now: Instant) -> Outcome {
        if !self.caps.r#ref {
            return err(&id, Reject::NotAllowed, "capability not granted");
        }
        let label: String = params
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .chars()
            .filter(|c| !c.is_control())
            .collect();
        let label = label.trim().to_string();
        let n = label.chars().count();
        if n == 0 || n > MAX_REF_LABEL_CHARS {
            return err(&id, Reject::BadRequest, "label must be 1 to 80 characters");
        }
        let value = match params.get("value") {
            None | Some(Value::Null) => None,
            Some(v) => {
                if serde_json::to_vec(v).map_or(usize::MAX, |b| b.len()) > MAX_REF_VALUE_BYTES {
                    return err(&id, Reject::TooLarge, "value exceeds 2048 bytes");
                }
                Some(v.clone())
            }
        };
        let selector = match params.get("selector") {
            None | Some(Value::Null) => None,
            Some(Value::String(s)) if s.chars().count() <= MAX_REF_SELECTOR_CHARS => {
                Some(s.clone())
            }
            Some(_) => return err(&id, Reject::BadRequest, "selector must be a short string"),
        };
        while self
            .refs
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) >= Duration::from_secs(1))
        {
            self.refs.pop_front();
        }
        if self.refs.len() >= REF_PER_SECOND {
            return err(&id, Reject::RateLimited, "too many refs");
        }
        self.refs.push_back(now);
        Outcome {
            reply: Some(reply_script(&json!({ "id": id, "result": true }))),
            request: Some(Request::Ref {
                label,
                value,
                selector,
            }),
        }
    }

    /// A new data snapshot: answers the waiting `getData` and `subscribe` calls and, for a
    /// later snapshot, pushes it to the page's `onData` callbacks. Returns the scripts to
    /// run in the page, in order. An older or equal rev than the one the page has is
    /// ignored (a `GET` that comes back after a push).
    pub fn push_data(&mut self, snap: Snapshot) -> Vec<String> {
        if self.snapshot.as_ref().is_some_and(|s| snap.rev <= s.rev) {
            return Vec::new();
        }
        let first = self.snapshot.is_none();
        let mut out: Vec<String> = self
            .waiting
            .drain(..)
            .map(|(id, w)| reply_script(&json!({ "id": id, "result": answer(&snap, w) })))
            .collect();
        if !first && self.subscribed && self.caps.live_data {
            out.push(data_script(&snap.data, &snap.meta()));
        }
        self.snapshot = Some(snap);
        out
    }

    /// Finishes an open `emit` or `ask`: `Ok(result)` resolves the page's promise, `Err`
    /// rejects it with that code. `None` when the request is no longer open (a late reply
    /// after a timeout or a reload), so the host drops it.
    pub fn finish(&mut self, reply_id: &Value, result: Result<Value, Reject>) -> Option<String> {
        let (id, _) = self.open.remove(&key(reply_id))?;
        let reply = match result {
            Ok(v) => json!({ "id": id, "result": v }),
            Err(r) => err_value(&id, r.code(), r.code()),
        };
        Some(reply_script(&reply))
    }

    /// The page goes away (a reload, another version, or close): every open request ends.
    /// Returns their ids so the host can drop its timers; the page itself is gone, so no
    /// script runs.
    pub fn reset(&mut self) -> Vec<Value> {
        self.waiting.clear();
        self.subscribed = false;
        self.refs.clear();
        self.open.drain().map(|(_, (id, _))| id).collect()
    }

    /// The `window.teamchat` facts for the next load: viewer, capabilities, declarations.
    pub fn init(&self, viewer: Option<&Viewer>, menu: Option<&RefMenu>) -> Value {
        let events: Vec<&EventDecl> = self.events.iter().flatten().collect();
        json!({
            "viewer": viewer,
            "can": {
                "emit": self.caps.emit,
                "ask": self.caps.ask,
                "events": if self.caps.emit || self.caps.ask { events } else { Vec::new() },
            },
            "ref": self.caps.r#ref,
            "menu": menu,
        })
    }
}

fn answer(s: &Snapshot, w: Waiter) -> Value {
    match w {
        Waiter::Get => s.data.clone(),
        Waiter::Subscribe => {
            let mut v = json!({ "data": s.data, "rev": s.rev });
            if let Some(c) = &s.cause {
                v["cause"] = json!(c);
            }
            v
        }
    }
}

/// The script that delivers a reply to the page. `reply` is serialized by serde_json, which
/// yields a valid JavaScript literal, so page data cannot break out of the expression.
pub fn reply_script(reply: &Value) -> String {
    format!(
        "window.dispatchEvent(new CustomEvent('artifactbridgereply',{{detail:{reply}}}));void 0"
    )
}

/// The script that pushes a data snapshot to the page's `onData` callbacks
/// (`teamchatdata`), with the same escaping as [`reply_script`].
pub fn data_script(data: &Value, meta: &Value) -> String {
    let detail = json!({ "data": data, "meta": meta });
    format!("window.dispatchEvent(new CustomEvent('teamchatdata',{{detail:{detail}}}));void 0")
}

const PAGE_TEMPLATE: &str = r#"(function(){
  var h = window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.bridge;
  if (!h) return;
  var init = __INIT__;
  var n = 0, pending = new Map(), subs = [], last = null, asking = false;
  window.addEventListener('artifactbridgereply', function (e) {
    var d = e.detail || {}, p = pending.get(d.id);
    if (!p) return;
    pending.delete(d.id);
    if (d.error) { var x = new Error(d.error.message); x.code = d.error.code; p.reject(x); }
    else p.resolve(d.result);
  });
  function deliver() {
    var l = last;
    subs.slice().forEach(function (cb) { try { cb(l.data, l.meta); } catch (e) {} });
  }
  window.addEventListener('teamchatdata', function (e) {
    var d = e.detail || {};
    last = { data: d.data === undefined ? null : d.data, meta: d.meta || { rev: 0 } };
    deliver();
  });
  function call(method, params) {
    return new Promise(function (resolve, reject) {
      var id = '__ab' + (++n);
      pending.set(id, { resolve: resolve, reject: reject });
      try { h.postMessage({ id: id, method: method, params: params === undefined ? null : params }); }
      catch (e) { pending.delete(id); reject(e); }
    });
  }
  function onData(cb) {
    if (typeof cb !== 'function') throw new TypeError('onData needs a function');
    subs.push(cb);
    if (last) {
      var l = last;
      Promise.resolve().then(function () { if (subs.indexOf(cb) >= 0) { try { cb(l.data, l.meta); } catch (e) {} } });
    } else if (!asking) {
      asking = true;
      call('subscribe').then(function (s) {
        if (!last) { last = { data: s.data === undefined ? null : s.data, meta: s.cause ? { rev: s.rev, cause: s.cause } : { rev: s.rev } }; deliver(); }
      }, function () { asking = false; });
    }
    return function () { var i = subs.indexOf(cb); if (i >= 0) subs.splice(i, 1); };
  }
  function ref(o) { o = o || {}; return call('ref', { label: o.label, value: o.value, selector: o.selector }); }
  function freeze(o) { Object.getOwnPropertyNames(o).forEach(function (k) { var v = o[k]; if (v && typeof v === 'object') freeze(v); }); return Object.freeze(o); }
  var can = freeze({ emit: !!init.can.emit, ask: !!init.can.ask, events: (init.can.events || []).map(function (e) { return { name: e.name, kind: e.kind }; }) });
  Object.defineProperty(window, 'artifact', { value: Object.freeze({
    call: call,
    getData: function () { return call('getData'); },
    emitEvent: function (name, payload) { return call('emitEvent', { name: name, payload: payload }); },
    ref: ref
  }) });
  Object.defineProperty(window, 'teamchat', { value: Object.freeze({
    version: 1,
    viewer: init.viewer ? freeze({ memberId: init.viewer.memberId, name: init.viewer.name }) : null,
    can: can,
    getData: function () { return call('getData'); },
    onData: onData,
    emit: function (name, payload) { return call('emit', { name: name, payload: payload }); },
    ask: function (name, payload, opts) { return call('ask', { name: name, payload: payload, timeoutMs: opts && opts.timeoutMs }); },
    ref: ref
  }) });
})();"#;

/// The bridge script injected at document start in the main frame: `window.artifact`
/// (legacy: `getData`, `emitEvent`, `call`, plus `ref`) and the frozen `window.teamchat`
/// (INTERACT §5.1). `init` is [`Bridge::init`].
pub fn page_script(init: &Value) -> String {
    PAGE_TEMPLATE.replace("__INIT__", &init.to_string())
}

const REF_TEMPLATE: &str = r#"(function(){
  var m = __MENU__;
  if (!m || !window.artifact || !window.artifact.ref) return;
  function target(el) {
    while (el && el.nodeType === 1) { if (el.hasAttribute('data-ref-label')) return el; el = el.parentElement; }
    return null;
  }
  function esc(s) { return window.CSS && CSS.escape ? CSS.escape(s) : s; }
  function path(el) {
    var parts = [];
    while (el && el.nodeType === 1 && el !== document.documentElement && parts.length < 6) {
      if (el.id) { parts.unshift('#' + esc(el.id)); break; }
      var i = 1, s = el;
      while ((s = s.previousElementSibling)) if (s.tagName === el.tagName) i++;
      parts.unshift(el.tagName.toLowerCase() + ':nth-of-type(' + i + ')');
      el = el.parentElement;
    }
    return parts.join(' > ');
  }
  function build(el) {
    var raw = el.getAttribute('data-ref-value'), value;
    if (raw !== null) { try { value = JSON.parse(raw); } catch (e) { value = raw; } }
    else { value = Array.from((el.textContent || '').trim()).slice(0, 200).join(''); }
    return { label: el.getAttribute('data-ref-label'), value: value, selector: path(el) };
  }
  function send(el) { window.artifact.ref(build(el)).catch(function () {}); }
  var lit = null, saved = '';
  function light(el) {
    if (lit === el) return;
    if (lit) lit.style.outline = saved;
    lit = el;
    if (el) { saved = el.style.outline; el.style.outline = '2px solid ' + m.accent; }
  }
  document.addEventListener('mousemove', function (e) { light(e.altKey ? target(e.target) : null); }, true);
  document.addEventListener('keyup', function (e) { if (e.key === 'Alt') light(null); }, true);
  document.addEventListener('click', function (e) {
    if (!e.altKey) return;
    var el = target(e.target);
    if (!el) return;
    e.preventDefault(); e.stopPropagation();
    light(null);
    send(el);
  }, true);
  var host = null;
  function close() { if (host) { host.remove(); host = null; } }
  document.addEventListener('contextmenu', function (e) {
    var el = target(e.target);
    close();
    if (!el) return;
    e.preventDefault();
    host = document.createElement('div');
    host.style.cssText = 'position:fixed;left:0;top:0;z-index:2147483647';
    var root = host.attachShadow({ mode: 'closed' });
    var b = document.createElement('button');
    b.textContent = m.label;
    b.style.cssText = 'position:fixed;left:' + e.clientX + 'px;top:' + e.clientY + 'px;font:13px -apple-system,system-ui;padding:6px 12px;border-radius:8px;cursor:default;' +
      'background:' + m.bg + ';color:' + m.ink + ';border:1px solid ' + m.line + ';box-shadow:0 4px 16px rgba(0,0,0,.16)';
    b.addEventListener('click', function (ev) { ev.stopPropagation(); close(); send(el); });
    root.appendChild(b);
    document.documentElement.appendChild(host);
  }, true);
  document.addEventListener('mousedown', function (e) { if (host && e.target !== host) close(); }, true);
  document.addEventListener('keydown', function (e) { if (e.key === 'Escape') close(); }, true);
  window.addEventListener('scroll', close, true);
})();"#;

/// The declarative 引用提问 path (REFS §A7), injected after [`page_script`] when the page
/// may `ref`: ⌥-hover outlines an element with `data-ref-label`, ⌥-click sends it, and its
/// context menu offers `menu.label`.
pub fn ref_listener_script(menu: &RefMenu) -> String {
    let m = serde_json::to_string(menu).unwrap_or_else(|_| "null".into());
    REF_TEMPLATE.replace("__MENU__", &m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> Instant {
        Instant::now()
    }

    fn reply_of(script: &str) -> Value {
        let start = script.find("detail:").unwrap() + "detail:".len();
        let end = script.rfind("}));void 0").unwrap();
        serde_json::from_str(&script[start..end]).unwrap()
    }

    fn bot_page() -> Bridge {
        Bridge::new(
            Capabilities::ALL,
            Some(vec![
                EventDecl::new("filter", EventKind::Emit),
                EventDecl::new("question", EventKind::Ask),
            ]),
            None,
        )
    }

    #[test]
    fn capabilities_gate_every_method() {
        let mut b = Bridge::new(Capabilities::NONE, None, Some(Snapshot::new(json!(1), 1)));
        for (m, p) in [
            ("getData", Value::Null),
            ("subscribe", Value::Null),
            ("emit", json!({"name":"x"})),
            ("emitEvent", json!({"name":"x"})),
            ("ask", json!({"name":"x"})),
            ("ref", json!({"label":"x"})),
        ] {
            let raw = serde_json::to_vec(&json!({"id": m, "method": m, "params": p})).unwrap();
            let o = b.handle(&raw, now());
            assert_eq!(
                reply_of(&o.reply.unwrap())["error"]["code"],
                "not_allowed",
                "{m}"
            );
            assert_eq!(o.request, None);
        }
        let o = b.handle(br#"{"id":"x","method":"readFile"}"#, now());
        assert_eq!(
            reply_of(&o.reply.unwrap())["error"]["code"],
            "unknown_method"
        );
        let o = b.handle(br#"{"id":{},"method":"getData"}"#, now());
        assert_eq!(reply_of(&o.reply.unwrap())["error"]["code"], "bad_request");
        let o = b.handle(b"[1]", now());
        assert_eq!(reply_of(&o.reply.unwrap())["error"]["code"], "bad_request");
    }

    #[test]
    fn a_person_s_page_reads_and_points_but_never_sends() {
        let mut b = Bridge::new(
            Capabilities::READ,
            None,
            Some(Snapshot::new(json!({"a":1}), 3)),
        );
        let o = b.handle(br#"{"id":1,"method":"getData"}"#, now());
        assert_eq!(
            reply_of(&o.reply.unwrap()),
            json!({"id":1,"result":{"a":1}})
        );
        let o = b.handle(br#"{"id":2,"method":"emit","params":{"name":"x"}}"#, now());
        assert_eq!(reply_of(&o.reply.unwrap())["error"]["code"], "not_allowed");
        let init = b.init(None, None);
        assert_eq!(init["can"], json!({"emit":false,"ask":false,"events":[]}));
        assert_eq!(init["ref"], true);
        assert_eq!(init["viewer"], Value::Null);
    }

    #[test]
    fn only_declared_names_of_the_declared_kind_go_out() {
        let mut b = bot_page();
        let o = b.handle(
            br#"{"id":1,"method":"emit","params":{"name":"question"}}"#,
            now(),
        );
        assert_eq!(reply_of(&o.reply.unwrap())["error"]["code"], "not_allowed");
        let o = b.handle(
            br#"{"id":2,"method":"ask","params":{"name":"filter"}}"#,
            now(),
        );
        assert_eq!(reply_of(&o.reply.unwrap())["error"]["code"], "not_allowed");
        let o = b.handle(
            br#"{"id":3,"method":"emitEvent","params":{"name":"other"}}"#,
            now(),
        );
        assert_eq!(reply_of(&o.reply.unwrap())["error"]["code"], "not_allowed");
        // emit waits for the ack and resolves with the event id.
        let o = b.handle(
            br#"{"id":4,"method":"emit","params":{"name":"filter","payload":{"channel":"bili"}}}"#,
            now(),
        );
        assert_eq!(o.reply, None);
        assert_eq!(
            o.request,
            Some(Request::Emit {
                reply_id: Some(json!(4)),
                name: "filter".into(),
                payload: json!({"channel":"bili"})
            })
        );
        let s = b.finish(&json!(4), Ok(json!({"eventId":"e1"}))).unwrap();
        assert_eq!(reply_of(&s), json!({"id":4,"result":{"eventId":"e1"}}));
        assert_eq!(b.finish(&json!(4), Ok(Value::Null)), None);
        // The legacy emitEvent answers at once.
        let o = b.handle(
            br#"{"id":5,"method":"emitEvent","params":{"name":"filter"}}"#,
            now(),
        );
        assert_eq!(reply_of(&o.reply.unwrap())["result"], true);
        assert!(matches!(
            o.request,
            Some(Request::Emit { reply_id: None, .. })
        ));
        assert_eq!(
            b.init(None, None)["can"]["events"],
            json!([{"name":"filter","kind":"emit"},{"name":"question","kind":"ask"}])
        );
    }

    #[test]
    fn asks_wait_time_out_and_are_capped() {
        let mut b = bot_page();
        let ask = |id: u32, extra: &str| {
            format!(r#"{{"id":{id},"method":"ask","params":{{"name":"question"{extra}}}}}"#)
        };
        let o = b.handle(ask(1, "").as_bytes(), now());
        assert_eq!(o.reply, None);
        let Some(Request::Ask { timeout_ms, .. }) = o.request else {
            panic!("no ask")
        };
        assert_eq!(timeout_ms, ASK_TIMEOUT_DEFAULT_MS);
        let o = b.handle(ask(2, r#","timeoutMs":999999"#).as_bytes(), now());
        assert!(matches!(
            o.request,
            Some(Request::Ask {
                timeout_ms: ASK_TIMEOUT_MAX_MS,
                ..
            })
        ));
        let o = b.handle(ask(3, r#","timeoutMs":"soon""#).as_bytes(), now());
        assert_eq!(reply_of(&o.reply.unwrap())["error"]["code"], "bad_request");
        b.handle(ask(3, "").as_bytes(), now());
        b.handle(ask(4, "").as_bytes(), now());
        let o = b.handle(ask(5, "").as_bytes(), now());
        assert_eq!(reply_of(&o.reply.unwrap())["error"]["code"], "rate_limited");
        // An id that is still open cannot be used again.
        let o = b.handle(ask(4, "").as_bytes(), now());
        assert_eq!(reply_of(&o.reply.unwrap())["error"]["code"], "bad_request");
        // Timeout, then the late reply is dropped.
        let s = b.finish(&json!(1), Err(Reject::Timeout)).unwrap();
        assert_eq!(reply_of(&s)["error"]["code"], "timeout");
        assert_eq!(b.finish(&json!(1), Ok(json!("late"))), None);
        let s = b.finish(&json!(2), Err(Reject::Declined)).unwrap();
        assert_eq!(reply_of(&s)["error"]["code"], "declined");
        // A reload ends the rest.
        let mut ended = b.reset();
        ended.sort_by_key(|v| v.as_u64());
        assert_eq!(ended, vec![json!(3), json!(4)]);
        assert_eq!(b.open_requests(), 0);
    }

    #[test]
    fn payloads_over_16_kib_are_refused() {
        let mut b = bot_page();
        let big = format!(
            r#"{{"id":1,"method":"emit","params":{{"name":"filter","payload":"{}"}}}}"#,
            "x".repeat(MAX_PAYLOAD_BYTES)
        );
        let o = b.handle(big.as_bytes(), now());
        assert_eq!(reply_of(&o.reply.unwrap())["error"]["code"], "too_large");
    }

    #[test]
    fn get_data_waits_for_the_first_snapshot_then_pushes_follow() {
        let mut b = Bridge::new(Capabilities::READ, None, None);
        assert_eq!(
            b.handle(br#"{"id":"g","method":"getData"}"#, now()).reply,
            None
        );
        assert_eq!(
            b.handle(br#"{"id":"s","method":"subscribe"}"#, now()).reply,
            None
        );
        let out = b.push_data(Snapshot::new(json!({"total":38}), 2));
        assert_eq!(out.len(), 2);
        assert_eq!(reply_of(&out[0]), json!({"id":"g","result":{"total":38}}));
        assert_eq!(
            reply_of(&out[1]),
            json!({"id":"s","result":{"data":{"total":38},"rev":2}})
        );
        // An older GET that comes back late is ignored.
        assert!(b.push_data(Snapshot::new(json!({"total":1}), 1)).is_empty());
        let mut snap = Snapshot::new(json!({"total":9}), 3);
        snap.cause = Some(DataCause {
            event_id: "e1".into(),
            actor_id: "m-joye".into(),
        });
        let out = b.push_data(snap);
        assert_eq!(out.len(), 1);
        assert!(out[0].starts_with("window.dispatchEvent(new CustomEvent('teamchatdata',"));
        assert_eq!(
            reply_of(&out[0]),
            json!({"data":{"total":9},"meta":{"rev":3,"cause":{"eventId":"e1","actorId":"m-joye"}}})
        );
        let o = b.handle(br#"{"id":"g2","method":"getData"}"#, now());
        assert_eq!(reply_of(&o.reply.unwrap())["result"], json!({"total":9}));
    }

    #[test]
    fn refs_are_checked_and_paced() {
        let mut b = Bridge::new(Capabilities::READ, None, None);
        let t0 = now();
        let o = b.handle(
            "{\"id\":1,\"method\":\"ref\",\"params\":{\"label\":\" B 站\\u0007 \",\"value\":9,\"selector\":\"#bili\"}}".as_bytes(),
            t0,
        );
        assert_eq!(reply_of(&o.reply.unwrap())["result"], true);
        assert_eq!(
            o.request,
            Some(Request::Ref {
                label: "B 站".into(),
                value: Some(json!(9)),
                selector: Some("#bili".into())
            })
        );
        let long = format!(
            r#"{{"id":2,"method":"ref","params":{{"label":"{}"}}}}"#,
            "长".repeat(81)
        );
        let o = b.handle(long.as_bytes(), t0);
        assert_eq!(reply_of(&o.reply.unwrap())["error"]["code"], "bad_request");
        let big = format!(
            r#"{{"id":3,"method":"ref","params":{{"label":"x","value":"{}"}}}}"#,
            "x".repeat(MAX_REF_VALUE_BYTES)
        );
        let o = b.handle(big.as_bytes(), t0);
        assert_eq!(reply_of(&o.reply.unwrap())["error"]["code"], "too_large");
        let ok = |id: u32| format!(r#"{{"id":{id},"method":"ref","params":{{"label":"x"}}}}"#);
        assert!(b.handle(ok(4).as_bytes(), t0).request.is_some());
        let o = b.handle(ok(5).as_bytes(), t0);
        assert_eq!(reply_of(&o.reply.unwrap())["error"]["code"], "rate_limited");
        let later = t0 + Duration::from_millis(1001);
        assert!(b.handle(ok(6).as_bytes(), later).request.is_some());
    }

    #[test]
    fn message_size_limit_per_page() {
        let big = format!(
            r#"{{"id":1,"method":"emitEvent","params":{{"name":"n","payload":"{}"}}}}"#,
            "x".repeat(MAX_MESSAGE_BYTES)
        );
        let mut b = Bridge::new(Capabilities::ALL, None, None);
        let o = b.handle(big.as_bytes(), now());
        assert_eq!(reply_of(&o.reply.unwrap())["error"]["code"], "too_large");
        // A larger window limit lets the message in; the payload rule still applies.
        let mut b = Bridge::new(Capabilities::ALL, None, None).with_max_bytes(4 << 20);
        let o = b.handle(big.as_bytes(), now());
        assert_eq!(reply_of(&o.reply.unwrap())["error"]["code"], "too_large");
        let ok = format!(
            r#"{{"id":1,"method":"emitEvent","params":{{"name":"n","payload":"{}"}}}}"#,
            "x".repeat(MAX_PAYLOAD_BYTES - 100)
        );
        assert_eq!(
            reply_of(&b.handle(ok.as_bytes(), now()).reply.unwrap())["result"],
            true
        );
        // The diagram renderer raises both limits: a whole SVG gets through.
        let mut b = Bridge::new(Capabilities::ALL, None, None)
            .with_max_bytes(4 << 20)
            .with_max_payload_bytes(4 << 20);
        assert_eq!(
            reply_of(&b.handle(big.as_bytes(), now()).reply.unwrap())["result"],
            true
        );
    }

    #[test]
    fn scripts_escape_page_data() {
        let s = reply_script(&json!({"id":"</script>'\u{2028}","result":"\"); alert(1); (\""}));
        assert!(
            s.starts_with("window.dispatchEvent(new CustomEvent('artifactbridgereply',{detail:{")
        );
        assert!(s.contains(r#"\"); alert(1); (\""#));
        let d = data_script(&json!("\"});alert(1);({\""), &json!({"rev":1}));
        assert!(d.contains(r#"\"});alert(1);({\""#));
        let init = Bridge::new(Capabilities::ALL, None, None).init(
            Some(&Viewer {
                member_id: "m1".into(),
                name: "\"};alert(1);//".into(),
            }),
            None,
        );
        let p = page_script(&init);
        assert!(p.contains(r#"\"};alert(1);//"#));
        assert!(!p.contains("__INIT__"));
        let menu = RefMenu {
            label: "就这里提问".into(),
            bg: "#ffffff".into(),
            ink: "#111111".into(),
            line: "#e5e5e5".into(),
            accent: "#2f6fed".into(),
        };
        let r = ref_listener_script(&menu);
        assert!(r.contains("\"label\":\"就这里提问\"") && !r.contains("__MENU__"));
    }
}
