//! Screenshot scenes of the topic view (DESIGN.md §13.4 to §13.6, §11): every
//! message kind, the send states, typing, the offline band, and the composer
//! with a quote, file chips, the @mention popup and edit mode.
//!
//! [`TopicPreview`] wraps the fake provider and adds data to two topics, so
//! the shared fake data stays as it is. Openers drive the views through small
//! preview hooks (`preview_*`) that call the same code paths as the keyboard.

use std::sync::Arc;

use futures::channel::mpsc::UnboundedReceiver;
use gpui::{AnyWindowHandle, App, AppContext as _, Bounds, Entity, Pixels, Size, WindowBounds};

use crate::{
    app::{RootView, Services, main_window_options},
    data::*,
};

const GROUP: &str = "g-dev";
/// Code, a list, mentions and the 新消息 divider in the fake data; the
/// preview adds a quote reply, an emoji-only message and a streaming reply.
const KINDS_TOPIC: &str = "t-login";
/// Edited, deleted, pending and failed messages in the fake data.
const STATES_TOPIC: &str = "t-edits";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Kinds,
    States,
}

pub struct TopicPreview {
    inner: Arc<dyn ChatProvider>,
    mode: Mode,
}

impl TopicPreview {
    fn new(inner: Arc<dyn ChatProvider>, mode: Mode) -> Arc<Self> {
        Arc::new(Self { inner, mode })
    }

    fn me(&self) -> String {
        match self.inner.session() {
            SessionState::SignedIn { me, .. } => me.id,
            _ => String::new(),
        }
    }

    /// Two other people (for typing and the quote) and a bot.
    fn cast(&self) -> (Vec<MemberId>, Option<MemberId>) {
        let me = self.me();
        let members = self.inner.members();
        let people = members
            .iter()
            .filter(|m| m.id != me && !m.is_bot())
            .map(|m| m.id.clone())
            .take(2)
            .collect();
        let bot = members.iter().find(|m| m.is_bot()).map(|m| m.id.clone());
        (people, bot)
    }

    fn extra_messages(&self, base: &[Message]) -> Vec<Message> {
        if self.mode != Mode::Kinds {
            return Vec::new();
        }
        let (people, bot) = self.cast();
        let Some(first) = base.iter().find(|m| m.deleted_at.is_none()) else {
            return Vec::new();
        };
        let last = base.last().map_or(0, |m| m.created_at);
        let last_seq = base.last().map_or(0, |m| m.created_seq);
        let person = people.first().cloned().unwrap_or_default();
        let make = |n: u64, author: &str, body: &str| Message {
            id: format!("tp-{n}"),
            client_msg_id: format!("c-tp-{n}"),
            group_id: GROUP.into(),
            topic_id: KINDS_TOPIC.into(),
            author_id: author.into(),
            body: body.into(),
            mentions: Vec::new(),
            created_seq: last_seq + n,
            created_at: last + n as i64 * 20_000,
            edited_at: None,
            deleted_at: None,
            streaming: false,
            send_state: SendState::Sent,
            quote_of: None,
            attachments: Vec::new(),
            artifact_refs: Vec::new(),
        };
        let mut quote = make(1, &person, "两种都要，邀请码优先。错误提示放在输入框下面，不要弹窗。");
        quote.quote_of = Some(first.id.clone());
        let emoji = make(2, &person, "👍🎉");
        let mut stream = make(
            3,
            bot.as_deref().unwrap_or(&person),
            "正在补充暗色模式的截图，**浅色**已经核对完：\n\n- 输入框的边框\n- 错误提示的颜色",
        );
        stream.streaming = true;
        vec![quote, emoji, stream]
    }

    fn draft_for(&self, key: &DraftKey) -> Option<Draft> {
        if self.mode != Mode::States
            || key.group_id != GROUP
            || key.topic_id.as_deref() != Some(STATES_TOPIC)
        {
            return None;
        }
        let quoted = self
            .inner
            .messages(GROUP, STATES_TOPIC, 50)
            .messages
            .into_iter()
            .find(|m| m.deleted_at.is_none() && !m.id.is_empty())?;
        let file = |id: &str, name: &str, image: bool, size: u64| PendingFile {
            id: id.into(),
            name: name.into(),
            is_image: image,
            size_bytes: Some(size),
            source: NewFile::Path(std::path::PathBuf::from(format!("/tmp/{name}"))),
        };
        Some(Draft {
            body: "好的，我把议程整理好了，周四前发出来。".into(),
            mentions: Vec::new(),
            quote_of: Some(quoted.id),
            files: vec![
                file("pf-1", "demo 议程（第二版）.pdf", false, 1_284_000),
                file("pf-2", "截图 2026-11-03 14.32.10.png", true, 482_000),
            ],
        })
    }
}

impl ChatProvider for TopicPreview {
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
        match self.mode {
            Mode::States => ConnectionState::Offline,
            Mode::Kinds => self.inner.connection(),
        }
    }
    fn members(&self) -> Vec<Member> {
        self.inner.members()
    }
    fn member(&self, id: &str) -> Option<Member> {
        self.inner.member(id)
    }
    fn groups(&self) -> Vec<GroupSummary> {
        self.inner.groups()
    }
    fn topics(&self, group_id: &str) -> Vec<Topic> {
        self.inner.topics(group_id)
    }
    fn topic(&self, group_id: &str, topic_id: &str) -> Option<Topic> {
        self.inner.topic(group_id, topic_id)
    }
    fn todo(&self) -> Vec<TodoItem> {
        self.inner.todo()
    }
    fn messages(&self, group_id: &str, topic_id: &str, limit: usize) -> MessageWindow {
        let mut w = self.inner.messages(group_id, topic_id, limit);
        if group_id == GROUP && topic_id == KINDS_TOPIC {
            let extra = self.extra_messages(&w.messages);
            w.messages.extend(extra);
        }
        w
    }
    fn load_older(&self, group_id: &str, topic_id: &str) {
        self.inner.load_older(group_id, topic_id)
    }
    fn typing(&self, group_id: &str, topic_id: &str) -> Vec<MemberId> {
        if group_id == GROUP && topic_id == KINDS_TOPIC && self.mode == Mode::Kinds {
            return self.cast().0;
        }
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
    fn edit(&self, group_id: &str, message_id: &str, body: String) {
        self.inner.edit(group_id, message_id, body)
    }
    fn delete(&self, group_id: &str, message_id: &str) {
        self.inner.delete(group_id, message_id)
    }
    fn set_topic_status(&self, g: &str, t: &str, status: TopicStatus, waiting_on: Vec<MemberId>) {
        self.inner.set_topic_status(g, t, status, waiting_on)
    }
    fn mark_read(&self, group_id: &str, topic_id: &str, seq: u64) {
        self.inner.mark_read(group_id, topic_id, seq)
    }
    fn set_typing(&self, group_id: &str, topic_id: &str) {
        self.inner.set_typing(group_id, topic_id)
    }
    fn draft(&self, key: &DraftKey) -> Draft {
        self.draft_for(key).unwrap_or_else(|| self.inner.draft(key))
    }
    fn save_draft_text(&self, key: &DraftKey, body: String, mentions: Vec<MemberId>) {
        self.inner.save_draft_text(key, body, mentions)
    }
    fn topic_read_seq(&self, group_id: &str, topic_id: &str) -> Option<u64> {
        self.inner.topic_read_seq(group_id, topic_id)
    }
    fn topic_following(&self, group_id: &str, topic_id: &str) -> bool {
        self.inner.topic_following(group_id, topic_id)
    }
    fn subscribe(&self) -> UnboundedReceiver<Vec<Change>> {
        self.inner.subscribe()
    }
}

/// What the opener does once the window is up.
#[derive(Clone, Copy)]
enum Then {
    Nothing,
    /// The @ list for `@` typed after some text.
    Mention,
    /// The composer edits my last message (Up in an empty composer).
    ComposerEdit,
    /// The timeline edits my last message in place (menu 编辑).
    RowEdit,
    /// A message row drawn as hovered, with its toolbar.
    Hover,
}

fn open(
    services: Services,
    content: Size<Pixels>,
    mode: Mode,
    topic: &str,
    then: Then,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    let provider: Arc<dyn ChatProvider> = TopicPreview::new(services.provider.clone(), mode);
    let services = Services::new(provider, cx);
    let topic = topic.to_string();
    services
        .nav
        .update(cx, |nav, cx| nav.open_topic(GROUP.into(), topic, cx));
    let mut options = main_window_options(false, cx);
    options.window_bounds = Some(WindowBounds::Windowed(Bounds::centered(None, content, cx)));
    options.window_min_size = None;
    let (handle, root): (AnyWindowHandle, Entity<RootView>) =
        gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| RootView::new(services, None, window, cx))
        })
        .map_err(|e| e.to_string())?;
    handle
        .update(cx, |_, window, cx| {
            let (timeline, composer) = {
                let root = root.read(cx);
                (root.timeline().clone(), root.composer().clone())
            };
            match then {
                Then::Nothing => {}
                Then::Mention => composer.update(cx, |c, cx| {
                    c.preview_text("合并前再请 @", window, cx)
                }),
                Then::ComposerEdit => composer.update(cx, |c, cx| c.preview_edit(window, cx)),
                Then::RowEdit => timeline.update(cx, |t, cx| t.preview_edit(window, cx)),
                Then::Hover => timeline.update(cx, |t, cx| t.preview_hover(cx)),
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(handle)
}

/// `topic-kinds`: text, Markdown, code, a quote reply, emoji only, the
/// divider, a streaming reply and two members typing.
pub fn open_kinds(s: Services, size: Size<Pixels>, cx: &mut App) -> Result<AnyWindowHandle, String> {
    open(s, size, Mode::Kinds, KINDS_TOPIC, Then::Nothing, cx)
}

/// `topic-states`: edited, deleted, pending and failed messages, offline,
/// and a draft with a quote and two files.
pub fn open_states(s: Services, size: Size<Pixels>, cx: &mut App) -> Result<AnyWindowHandle, String> {
    open(s, size, Mode::States, STATES_TOPIC, Then::Nothing, cx)
}

/// `topic-mention`: the @mention popup over the composer.
pub fn open_mention(s: Services, size: Size<Pixels>, cx: &mut App) -> Result<AnyWindowHandle, String> {
    open(s, size, Mode::Kinds, KINDS_TOPIC, Then::Mention, cx)
}

/// `topic-edit`: a message edited in place, and nothing else changed.
pub fn open_row_edit(s: Services, size: Size<Pixels>, cx: &mut App) -> Result<AnyWindowHandle, String> {
    open(s, size, Mode::Kinds, KINDS_TOPIC, Then::RowEdit, cx)
}

/// `composer-edit`: the composer in edit mode.
pub fn open_composer_edit(
    s: Services,
    size: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    open(s, size, Mode::Kinds, KINDS_TOPIC, Then::ComposerEdit, cx)
}

/// `topic-hover`: a hovered message with its toolbar.
pub fn open_hover(s: Services, size: Size<Pixels>, cx: &mut App) -> Result<AnyWindowHandle, String> {
    open(s, size, Mode::States, STATES_TOPIC, Then::Hover, cx)
}
