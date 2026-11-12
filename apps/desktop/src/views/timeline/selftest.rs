//! `desktop --selftest-timeline <n> --report <path>`: opens a topic with `n`
//! generated messages (Chinese and English, varied lengths, headings, lists,
//! tables, code blocks), scrolls by code from the newest message up to the
//! oldest and back down, and writes a JSON report.
//!
//! Report fields:
//! - `first_frame_ms`: from opening the topic (the window is already up) to
//!   the end of the timeline's first paint with messages.
//! - `frames`, `p50_ms`, `p95_ms`, `max_ms`: frame work while scrolling from
//!   the top to the bottom: from the start of the frame (the frame callback)
//!   to the end of the timeline's paint. Frame pacing is not measured: the
//!   window is not active, so GPUI caps it at about 30 fps on purpose.
//! - `up`: the same for the scroll from the bottom to the top, which also
//!   grows the window page by page.
//!
//! The window opens without a Dock icon and without taking focus.

use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui::{
    ActivationPolicy, App, Bounds, Element, ElementId, Entity, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, Style, Window, px,
};

use super::{FrameProbe, TimelineView, WINDOW};
use crate::{
    app::{self, Services},
    data::*,
    fake::{self, FakeProvider},
};

pub const GROUP: &str = fake::LONG_TOPIC_GROUP;
pub const TOPIC: &str = "t-selftest";
/// Pixels scrolled per frame. A fast trackpad fling moves about this much.
const UP_STEP: Pixels = px(1200.);
const DOWN_STEP: Pixels = px(600.);
/// Frames to wait at the bottom for the window to shrink.
const SETTLE_FRAMES: usize = 20;
const TIMEOUT: Duration = Duration::from_secs(180);

/// Records when the timeline finished painting. Put it last in the view so
/// it paints after the list.
pub struct PaintProbe(pub Rc<FrameProbe>);

impl IntoElement for PaintProbe {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for PaintProbe {
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
        (window.request_layout(Style::default(), [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
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
        self.0.painted.set(Some(Instant::now()));
    }
}

// --- the provider -----------------------------------------------------------

/// The fake provider plus one generated topic in [`GROUP`].
pub struct SelftestProvider {
    inner: Arc<FakeProvider>,
    topic: Topic,
    messages: Vec<Message>,
    extra: Mutex<usize>,
    subscribers: Mutex<Vec<UnboundedSender<Vec<Change>>>>,
}

/// The provider for `--selftest-timeline <count>`.
pub fn provider(count: usize) -> Arc<dyn ChatProvider> {
    let inner = FakeProvider::new();
    let now = crate::selftest::clock::utc_now().timestamp_millis();
    let messages = generate(count, now);
    let last_seq = messages.last().map(|m| m.created_seq).unwrap_or(0);
    let topic = Topic {
        id: TOPIC.into(),
        group_id: GROUP.into(),
        title: "滚动自测".into(),
        status: TopicStatus::InProgress,
        waiting_on: Vec::new(),
        owner_id: None,
        created_by: fake::ME.into(),
        created_at: messages.first().map(|m| m.created_at).unwrap_or(now),
        root_message_id: messages.first().map(|m| m.id.clone()).unwrap_or_default(),
        last_seq,
        last_activity_at: now,
        message_count: count as u32,
        unread: 0,
        mentions: 0,
        participants: Vec::new(),
        relay: None,
    };
    Arc::new(SelftestProvider {
        inner,
        topic,
        messages,
        extra: Mutex::new(0),
        subscribers: Mutex::default(),
    })
}

const AUTHORS: [&str; 6] = [
    "m-joye",
    "m-xiaowang",
    "m-lixue",
    "m-alex",
    "m-coder",
    "m-sentry",
];

const BODIES: &[&str] = &[
    "好的，我看一下。",
    "Sounds good, merging after CI.",
    "这个问题我复现了：在网络切换后重连，第一条消息会重复一次。原因是重发时没有带上原来的 clientMsgId，服务端无法去重。我在 `sync.rs` 里改了重发逻辑，下午提 PR。",
    "## 今日进展\n\n- 完成登录页\n- 修复侧边栏未读数\n- **待办**：话题状态的同步\n\n明天继续做搜索。",
    "```rust\nfn apply(&mut self, event: Event) -> Result<()> {\n    if event.seq <= self.last_applied {\n        return Ok(());\n    }\n    self.buffer.insert(event.seq, event);\n    self.drain()\n}\n```",
    "| 指标 | 目标 | 当前 |\n| --- | --- | --- |\n| p95 投递 | 300 ms | 212 ms |\n| 冷启动 | 1 s | 0.8 s |",
    "The deploy to staging finished. See [the run](https://example.com/runs/42) for logs; nothing unusual except one retry on D1.",
    "> 引用：先把一万条消息的话题打开时间压到 100 ms 以内。\n\n同意，先做缓存。",
    "1. 拉取最新代码\n2. 运行 `cargo test -p chat-core`\n3. 在 staging 上验证\n4. 发布",
    "Long English paragraph to test wrapping: the timeline must keep rows of very different heights stable while the user scrolls fast through ten thousand messages, and it must never jump when an older page arrives above the viewport.",
    "收到 👍",
    "```ts\nexport async function handle(req: Request): Promise<Response> {\n  const body = await req.json();\n  return Response.json({ ok: true, id: body.id });\n}\n```\n\n上面是新的 handler，`id` 由客户端生成。",
];

fn generate(count: usize, now: Millis) -> Vec<Message> {
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let start = now - 30 * 24 * 3_600_000;
    let step = (30 * 24 * 3_600_000) / count.max(1) as i64;
    let mut author = 0usize;
    (0..count)
        .map(|i| {
            // Runs of one author, so grouping shows up.
            if next() % 3 == 0 {
                author = (next() % AUTHORS.len() as u64) as usize;
            }
            let body = BODIES[(next() % BODIES.len() as u64) as usize];
            Message {
                id: format!("st-{i}"),
                client_msg_id: format!("st-c-{i}"),
                group_id: GROUP.into(),
                topic_id: TOPIC.into(),
                author_id: AUTHORS[author].into(),
                body: format!("#{} {body}", i + 1),
                mentions: Vec::new(),
                created_seq: 1_000_000 + i as u64,
                created_at: start + i as i64 * step,
                edited_at: (i % 37 == 0).then_some(start + i as i64 * step + 1000),
                deleted_at: None,
                streaming: false,
                send_state: SendState::Sent,
                quote_of: None,
                attachments: Vec::new(),
                artifact_refs: Vec::new(),
                reactions: Vec::new(),
            }
        })
        .collect()
}

impl SelftestProvider {
    fn mine(&self, group_id: &str, topic_id: &str) -> bool {
        group_id == GROUP && topic_id == TOPIC
    }

    fn emit(&self, changes: Vec<Change>) {
        let mut subs = self.subscribers.lock().unwrap();
        subs.retain(|tx| tx.unbounded_send(changes.clone()).is_ok());
    }
}

impl ChatProvider for SelftestProvider {
    fn session(&self) -> SessionState {
        self.inner.session()
    }
    fn login(&self, request: LoginRequest) {
        self.inner.login(request)
    }
    fn logout(&self) {
        self.inner.logout()
    }
    fn connection(&self) -> ConnectionState {
        self.inner.connection()
    }
    fn members(&self) -> Vec<Member> {
        self.inner.members()
    }
    fn groups(&self) -> Vec<GroupSummary> {
        self.inner.groups()
    }
    fn topics(&self, group_id: &str) -> Vec<Topic> {
        let mut topics = self.inner.topics(group_id);
        if group_id == GROUP {
            topics.insert(0, self.topic.clone());
        }
        topics
    }
    fn todo(&self) -> Vec<TodoItem> {
        self.inner.todo()
    }
    fn messages(&self, group_id: &str, topic_id: &str, limit: usize) -> MessageWindow {
        if !self.mine(group_id, topic_id) {
            return self.inner.messages(group_id, topic_id, limit);
        }
        let want = limit.saturating_add(*self.extra.lock().unwrap());
        let start = self.messages.len().saturating_sub(want);
        MessageWindow {
            messages: self.messages[start..].to_vec(),
            has_older: start > 0,
            loading_older: false,
        }
    }
    fn load_older(&self, group_id: &str, topic_id: &str) {
        if !self.mine(group_id, topic_id) {
            return self.inner.load_older(group_id, topic_id);
        }
        *self.extra.lock().unwrap() += super::PAGE;
        self.emit(vec![Change::Messages {
            group_id: group_id.into(),
            topic_id: topic_id.into(),
        }]);
    }
    fn typing(&self, group_id: &str, topic_id: &str) -> Vec<MemberId> {
        self.inner.typing(group_id, topic_id)
    }
    fn artifacts(&self, group_id: &str, topic_id: &str) -> Vec<ArtifactSummary> {
        self.inner.artifacts(group_id, topic_id)
    }
    fn send(&self, request: SendRequest) -> ClientMsgId {
        self.inner.send(request)
    }
    fn start_topic(&self, request: StartTopicRequest) -> ClientMsgId {
        self.inner.start_topic(request)
    }
    fn retry_send(&self, group_id: &str, client_msg_id: &str) {
        self.inner.retry_send(group_id, client_msg_id)
    }
    fn edit(&self, group_id: &str, message_id: &str, body: String) {
        self.inner.edit(group_id, message_id, body)
    }
    fn delete(&self, group_id: &str, message_id: &str) {
        self.inner.delete(group_id, message_id)
    }
    fn set_topic_status(
        &self,
        group_id: &str,
        topic_id: &str,
        status: TopicStatus,
        waiting_on: Vec<MemberId>,
    ) {
        self.inner
            .set_topic_status(group_id, topic_id, status, waiting_on)
    }
    fn mark_read(&self, group_id: &str, topic_id: &str, seq: u64) {
        self.inner.mark_read(group_id, topic_id, seq)
    }
    fn set_typing(&self, group_id: &str, topic_id: &str) {
        self.inner.set_typing(group_id, topic_id)
    }
    fn subscribe(&self) -> UnboundedReceiver<Vec<Change>> {
        let (tx, rx) = unbounded();
        self.subscribers.lock().unwrap().push(tx.clone());
        // Forward the fake provider's changes into the same stream.
        let mut inner = self.inner.subscribe();
        std::thread::spawn(move || {
            futures::executor::block_on(async move {
                use futures::StreamExt as _;
                while let Some(batch) = inner.next().await {
                    if tx.unbounded_send(batch).is_err() {
                        break;
                    }
                }
            })
        });
        rx
    }
}

// --- the driver -------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    WaitFirst,
    Up,
    Down,
    Settle(usize),
}

struct Driver {
    timeline: Entity<TimelineView>,
    probe: Rc<FrameProbe>,
    phase: Phase,
    count: usize,
    opened_at: Instant,
    first_frame_ms: Option<f64>,
    frame_start: Option<Instant>,
    up: Vec<f64>,
    down: Vec<f64>,
    window_at_top: usize,
    first_at_top: String,
    report: PathBuf,
    done: bool,
}

/// Runs the self-test: opens the main window, then drives the timeline.
pub fn run(services: Services, count: usize, report: PathBuf, cx: &mut App) {
    cx.set_activation_policy(ActivationPolicy::Accessory);
    services
        .nav
        .update(cx, |nav, cx| nav.open_group(GROUP.into(), cx));
    let slot: Rc<RefCell<Option<Entity<app::RootView>>>> = Rc::default();
    let hook_slot = slot.clone();
    let hook_services = services.clone();
    let hook_report = report.clone();
    let hook: app::FirstFrameHook = Box::new(move |window, cx| {
        let Some(root) = hook_slot.borrow().clone() else {
            return write_and_quit(
                &hook_report,
                serde_json::json!({"ok": false, "error": "no root view"}),
                false,
                cx,
            );
        };
        let timeline = root.read(cx).timeline().clone();
        let probe = Rc::new(FrameProbe::default());
        timeline.update(cx, |tl, _| tl.probe = Some(probe.clone()));
        let opened_at = Instant::now();
        hook_services
            .nav
            .update(cx, |nav, cx| nav.open_topic(GROUP.into(), TOPIC.into(), cx));
        let driver = Rc::new(RefCell::new(Driver {
            timeline,
            probe,
            phase: Phase::WaitFirst,
            count,
            opened_at,
            first_frame_ms: None,
            frame_start: Some(opened_at),
            up: Vec::new(),
            down: Vec::new(),
            window_at_top: 0,
            first_at_top: String::new(),
            report: hook_report.clone(),
            done: false,
        }));
        // A fully covered window gets no frames: keep it above other apps'
        // windows (without focus or activation) while the test runs.
        let _ = crate::selftest::capture::order_front(window);
        let handle = window.window_handle();
        let watch = Rc::downgrade(&driver);
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if watch.upgrade().is_none_or(|d| d.borrow().done) {
                    break;
                }
                let _ = handle.update(cx, |_, window, _| {
                    crate::selftest::capture::order_front(window)
                });
            }
        })
        .detach();
        schedule(driver, window);
    });
    match app::open_main_window(services, false, Some(hook), cx) {
        Ok(root) => *slot.borrow_mut() = Some(root),
        Err(error) => {
            write_and_quit(
                &report,
                serde_json::json!({ "ok": false, "error": error }),
                false,
                cx,
            );
            return;
        }
    }
    cx.spawn(async move |cx| {
        cx.background_executor().timer(TIMEOUT).await;
        cx.update(|cx| {
            let json = serde_json::json!({ "ok": false, "error": "self-test timed out" });
            write_and_quit(&report, json, false, cx);
        });
    })
    .detach();
}

fn schedule(driver: Rc<RefCell<Driver>>, window: &mut Window) {
    window.on_next_frame(move |window, cx| tick(driver, window, cx));
}

fn tick(driver: Rc<RefCell<Driver>>, window: &mut Window, cx: &mut App) {
    {
        let mut d = driver.borrow_mut();
        if d.done {
            return;
        }
        let painted = d.probe.painted.get();
        let frame_ms = match (d.frame_start, painted) {
            (Some(start), Some(end)) if end > start => {
                Some(end.duration_since(start).as_secs_f64() * 1000.0)
            }
            _ => None,
        };
        let timeline = d.timeline.clone();
        match d.phase {
            Phase::WaitFirst => {
                if let Some(end) = painted
                    && end > d.opened_at
                    && timeline.read(cx).message_count() > 0
                {
                    d.first_frame_ms = Some(end.duration_since(d.opened_at).as_secs_f64() * 1000.0);
                    d.phase = Phase::Up;
                }
            }
            Phase::Up => {
                d.up.extend(frame_ms);
                let at_top = timeline.read(cx).at_top();
                if at_top {
                    let tl = timeline.read(cx);
                    d.window_at_top = tl.message_count();
                    d.first_at_top = tl
                        .messages
                        .first()
                        .map(|m| m.id.clone())
                        .unwrap_or_default();
                    d.phase = Phase::Down;
                }
            }
            Phase::Down => {
                d.down.extend(frame_ms);
                if timeline.read(cx).list.is_following_tail() {
                    d.phase = Phase::Settle(0);
                }
            }
            Phase::Settle(n) => {
                if n >= SETTLE_FRAMES {
                    d.done = true;
                } else {
                    d.phase = Phase::Settle(n + 1);
                }
            }
        }
        let step = match d.phase {
            Phase::Up => Some(-UP_STEP),
            Phase::Down => Some(DOWN_STEP),
            _ => None,
        };
        if let Some(step) = step {
            timeline.update(cx, |tl, cx| {
                tl.list.scroll_by(step);
                cx.notify();
            });
        } else {
            // Keep frames coming while waiting.
            timeline.update(cx, |_, cx| cx.notify());
        }
        d.frame_start = Some(Instant::now());
    }
    if driver.borrow().done {
        finish(&driver.borrow(), cx);
    } else {
        schedule(driver, window);
    }
}

fn stats(frames: &[f64]) -> (f64, f64, f64) {
    if frames.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let mut sorted = frames.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pick = |q: f64| sorted[((sorted.len() - 1) as f64 * q).round() as usize];
    (pick(0.5), pick(0.95), *sorted.last().unwrap())
}

fn round(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn finish(d: &Driver, cx: &mut App) {
    let (p50, p95, max) = stats(&d.down);
    let (up50, up95, upmax) = stats(&d.up);
    let first = d.first_frame_ms.unwrap_or(f64::INFINITY);
    let window_after = d.timeline.read(cx).message_count();
    let complete = d.window_at_top == d.count && d.first_at_top == "st-0";
    let shrunk = window_after <= WINDOW;
    let ok = complete && shrunk && !d.down.is_empty();
    let targets_met = first < 100.0 && p95 < 16.0;
    let json = serde_json::json!({
        "ok": ok,
        "targets_met": targets_met,
        "build": if cfg!(debug_assertions) { "debug" } else { "release" },
        "messages": d.count,
        "first_frame_ms": round(first),
        "frames": d.down.len(),
        "p50_ms": round(p50),
        "p95_ms": round(p95),
        "max_ms": round(max),
        "scroll_step_px": f32::from(DOWN_STEP),
        "up": {
            "frames": d.up.len(),
            "p50_ms": round(up50),
            "p95_ms": round(up95),
            "max_ms": round(upmax),
            "scroll_step_px": f32::from(-UP_STEP),
        },
        "window_at_top": d.window_at_top,
        "first_message_at_top": d.first_at_top,
        "window_after_return": window_after,
        "targets": { "first_frame_ms": 100, "p95_ms": 16 },
    });
    write_and_quit(&d.report, json, ok, cx);
}

fn write_and_quit(path: &PathBuf, json: serde_json::Value, ok: bool, cx: &mut App) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let text = serde_json::to_string_pretty(&json).unwrap_or_default();
    let written = std::fs::write(path, text + "\n").is_ok();
    if !written {
        eprintln!("cannot write {}", path.display());
    }
    let code = if ok && written { 0 } else { 1 };
    crate::set_exit_code(code);
    cx.quit();
    // On macOS `run` does not return after `quit`, so exit with the code here.
    std::process::exit(code);
}

impl TimelineView {
    /// The oldest message is loaded and the list shows its top.
    fn at_top(&self) -> bool {
        let top = self.list.logical_scroll_top();
        !self.has_older && top.item_ix == 0 && top.offset_in_item <= px(0.5)
    }
}
