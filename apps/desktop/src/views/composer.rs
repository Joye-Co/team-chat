//! The composer: a multi-line field that grows to 8 lines. Chinese IME works
//! through the platform input handler of gpui-component's input (marked text
//! is underlined, the candidate window follows the cursor).
//!
//! Keys: Enter and Cmd+Enter send, Shift+Enter adds a line. Enter during IME
//! composition confirms the candidate and does not send. `@` opens the member
//! list (Up, Down, Enter or Tab pick, Esc closes). Up in an empty composer
//! edits my last message; Esc cancels the edit or removes the quote.
//!
//! Without an open topic it starts a new topic in the current 群; the first
//! line becomes the title. Each topic (and each 群's new topic) keeps its own
//! draft in the provider: text, mentions, quote and pending files. Pasted or
//! dropped files and images become chips until the message is sent.

mod model;
#[cfg(test)]
mod tests;

use std::time::Instant;

use gpui::{
    AnimationExt as _, AppContext, ClipboardEntry, Context, ElementId, Entity, EntityInputHandler,
    ExternalPaths, Focusable as _, InteractiveElement, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px, relative,
};
use gpui_component::{
    Disableable as _, Selectable as _, Sizable as _,
    input::{
        Enter, Escape, IndentInline, InputEvent, MoveDown, MoveUp, Paste, Textarea, TextareaState,
    },
};

use crate::{
    app::{Changes, Services},
    copy::{composer as c, fill},
    data::{
        Change, DraftKey, Member, MemberId, MessageId, NewFile, PendingFile, SendRequest,
        StartTopicRequest,
    },
    theme::{self, Type as _, space},
    ui::{self, Lucide},
};

pub use model::MentionQuery;

/// Longest body the protocol accepts.
pub const MAX_BODY_CHARS: usize = 32_768;
/// Messages searched for the one Up edits and for the quoted message.
const RECENT_MESSAGES: usize = 200;

/// The message being edited.
#[derive(Clone, Debug, PartialEq, Eq)]
struct EditTarget {
    group_id: String,
    message_id: MessageId,
}

/// The quote chip.
#[derive(Clone, Debug)]
struct QuoteChip {
    message_id: MessageId,
    text: String,
}

/// The open @ list.
#[derive(Clone, Debug)]
struct MentionPopup {
    query: MentionQuery,
    results: Vec<Member>,
    selected: usize,
}

pub struct ComposerView {
    services: Services,
    input: Entity<TextareaState>,
    /// Whose draft the composer shows. `None` when no 群 is open.
    key: Option<DraftKey>,
    /// The text the composer last saw or set, to tell user edits from ours.
    last_text: String,
    /// Members picked from the @ list for the current draft.
    picked: Vec<MemberId>,
    quote: Option<QuoteChip>,
    files: Vec<PendingFile>,
    mention: Option<MentionPopup>,
    editing: Option<EditTarget>,
    typing: model::TypingThrottle,
    too_long: bool,
    /// Files the last paste or drop could not add (folders, over 100 MB).
    rejected: Vec<String>,
    /// The emoji picker is open.
    emoji_open: bool,
    /// The format bar is shown (the `Aa` toggle).
    format_open: bool,
    /// Bounds of the composer box in the window, for the @ list's anchor.
    anchor: std::rc::Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
    _subscriptions: Vec<Subscription>,
}

impl ComposerView {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                // Two lines empty; the box grows to about 40 % of the topic
                // view in the smallest window (§7.2), then scrolls inside.
                .auto_grow(2, 10)
                .submit_on_enter(true)
                .placeholder(c::PLACEHOLDER)
        });
        let subscriptions = vec![
            cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::Change = event {
                    this.on_text_changed(window, cx);
                }
            }),
            cx.observe_in(&services.nav, window, |this, _, window, cx| {
                this.sync_key(window, cx);
            }),
            cx.subscribe(&services.hub, |this, _, changes: &Changes, cx| {
                this.on_changes(changes, cx);
            }),
        ];
        let mut this = Self {
            services,
            input,
            key: None,
            last_text: String::new(),
            picked: Vec::new(),
            quote: None,
            files: Vec::new(),
            mention: None,
            editing: None,
            typing: model::TypingThrottle::default(),
            too_long: false,
            rejected: Vec::new(),
            emoji_open: false,
            format_open: false,
            anchor: Default::default(),
            _subscriptions: subscriptions,
        };
        this.sync_key(window, cx);
        this
    }

    /// Focuses the field with the caret at the end and opens the @ list for
    /// a draft that ends in `@query` (screenshot scenes).
    pub fn preview_mention(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.text(cx);
        let len = text.len();
        self.input.update(cx, |state, cx| {
            state.focus(window, cx);
            state.set_selected_range(len..len, cx);
        });
        self.update_mention(&text, cx);
        cx.notify();
    }

    /// Opens the format bar and the emoji picker (screenshot scenes).
    pub fn preview_tools(&mut self, cx: &mut Context<Self>) {
        self.format_open = true;
        self.emoji_open = true;
        cx.notify();
    }

    /// The draft key the navigation points at.
    fn nav_key(&self, cx: &Context<Self>) -> Option<DraftKey> {
        let nav = self.services.nav.read(cx);
        match (&nav.topic, nav.current_group()) {
            (Some((g, t)), _) => Some(DraftKey::topic(g.clone(), t.clone())),
            (None, Some(g)) => Some(DraftKey::new_topic(g)),
            (None, None) => None,
        }
    }

    fn text(&self, cx: &Context<Self>) -> String {
        self.input.read(cx).value().to_string()
    }

    fn composing(&self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.input.update(cx, |state, cx| {
            state.marked_text_range(window, cx).is_some()
        })
    }

    /// Replaces the field's text (no change event) and puts the cursor at the end.
    fn set_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.last_text = text.to_string();
        let len = text.len();
        self.input.update(cx, |state, cx| {
            state.set_value(text.to_string(), window, cx);
            state.set_selected_range(len..len, cx);
        });
    }

    /// Saves the old draft and loads the new one when the open topic or 群 changes.
    fn sync_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.nav_key(cx);
        if key == self.key {
            return;
        }
        if self.editing.take().is_none() {
            self.save_text(cx);
        }
        self.key = key;
        self.mention = None;
        self.too_long = false;
        self.rejected.clear();
        let draft = self
            .key
            .as_ref()
            .map(|k| self.services.provider.draft(k))
            .unwrap_or_default();
        self.picked = draft.mentions;
        self.set_text(&draft.body, window, cx);
        self.refresh_extras(cx);
        self.update_placeholder(window, cx);
        cx.notify();
    }

    fn update_placeholder(&self, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder = self.placeholder();
        self.input.update(cx, |state, cx| {
            state.set_placeholder(placeholder, window, cx)
        });
    }

    /// The placeholder names the destination (§7.2): `回复「title」`, or
    /// `在「群」发起新话题，第一行是标题`.
    fn placeholder(&self) -> String {
        let provider = &self.services.provider;
        let Some(key) = &self.key else {
            return c::PLACEHOLDER.to_string();
        };
        if self.editing.is_some() {
            return c::PLACEHOLDER_EDIT.to_string();
        }
        match &key.topic_id {
            Some(t) => match provider.topic(&key.group_id, t) {
                Some(topic) => fill(
                    c::PLACEHOLDER_REPLY,
                    &[(
                        "title",
                        &model::excerpt(&crate::views::sidebar::topic_line(&topic.title), 30),
                    )],
                ),
                None => c::PLACEHOLDER.to_string(),
            },
            None => match provider.groups().into_iter().find(|g| g.id == key.group_id) {
                Some(group) => fill(c::PLACEHOLDER_NEW_IN, &[("group", &group.name)]),
                None => c::PLACEHOLDER_NEW_TOPIC.to_string(),
            },
        }
    }

    fn save_text(&self, cx: &Context<Self>) {
        if let Some(key) = &self.key {
            let body = self.text(cx);
            self.services
                .provider
                .save_draft_text(key, body, self.picked.clone());
        }
    }

    /// Re-reads the quote and pending files of the current draft.
    fn refresh_extras(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.key.clone() else {
            self.quote = None;
            self.files.clear();
            return;
        };
        let draft = self.services.provider.draft(&key);
        let rejected = self.services.provider.take_rejected_files(&key);
        if !rejected.is_empty() {
            self.rejected = rejected
                .iter()
                .map(crate::views::files::rejected_text)
                .collect();
        }
        self.files = draft.files;
        self.quote = draft.quote_of.map(|id| self.quote_chip(&key, id));
        cx.notify();
    }

    fn quote_chip(&self, key: &DraftKey, message_id: MessageId) -> QuoteChip {
        let provider = &self.services.provider;
        let message = key.topic_id.as_ref().and_then(|t| {
            provider
                .messages(&key.group_id, t, RECENT_MESSAGES)
                .messages
                .into_iter()
                .find(|m| m.id == message_id)
        });
        let text = match message {
            Some(m) if m.deleted_at.is_some() => c::QUOTE_DELETED.to_string(),
            Some(m) => {
                let name = provider
                    .member(&m.author_id)
                    .map(|a| a.name)
                    .unwrap_or_else(|| c::UNKNOWN_MEMBER.to_string());
                format!(
                    "{}：{}",
                    fill(c::QUOTING, &[("name", &name)]),
                    model::excerpt(&strip_marks(&m.body), 40)
                )
            }
            None => fill(c::QUOTING, &[("name", c::UNKNOWN_MEMBER)]),
        };
        QuoteChip { message_id, text }
    }

    fn on_changes(&mut self, changes: &Changes, cx: &mut Context<Self>) {
        let Some(key) = self.key.clone() else {
            return;
        };
        let draft_changed = changes.any(|ch| matches!(ch, Change::Draft { key: k } if *k == key));
        let quoted_changed = self.quote.is_some()
            && key
                .topic_id
                .as_ref()
                .is_some_and(|t| changes.touches_topic(&key.group_id, t))
            || changes.any(|ch| matches!(ch, Change::Members));
        if draft_changed || (self.quote.is_some() && quoted_changed) {
            self.refresh_extras(cx);
        }
    }

    fn on_text_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.text(cx);
        if text == self.last_text {
            return;
        }
        self.last_text = text.clone();
        self.too_long = false;
        // Keep the @ list as it is while the IME composes (the marked text is
        // pinyin, not a name yet).
        if !self.composing(window, cx) {
            self.update_mention(&text, cx);
        }
        if self.editing.is_some() {
            cx.notify();
            return;
        }
        self.save_text(cx);
        if !text.trim().is_empty()
            && let Some(DraftKey {
                group_id,
                topic_id: Some(topic_id),
            }) = &self.key
            && self.typing.hit(group_id, topic_id, Instant::now())
        {
            self.services.provider.set_typing(group_id, topic_id);
        }
        cx.notify();
    }

    fn update_mention(&mut self, text: &str, cx: &mut Context<Self>) {
        let cursor = self.input.read(cx).cursor();
        let Some(query) = model::mention_query(text, cursor) else {
            self.mention = None;
            return;
        };
        let group_members = self
            .key
            .as_ref()
            .and_then(|k| {
                self.services
                    .provider
                    .groups()
                    .into_iter()
                    .find(|g| g.id == k.group_id)
            })
            .map(|g| g.member_ids)
            .unwrap_or_default();
        let me = self.services.me();
        let results = model::match_members(
            &self.services.provider.members(),
            &group_members,
            me.as_deref(),
            &query.text,
        );
        if results.is_empty() {
            self.mention = None;
            return;
        }
        let selected = match &self.mention {
            Some(old) if old.query.start == query.start => {
                old.selected.min(results.len().saturating_sub(1))
            }
            _ => 0,
        };
        self.mention = Some(MentionPopup {
            query,
            results,
            selected,
        });
    }

    /// Replaces the `@query` with `@name ` and remembers the member.
    fn pick_mention(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(popup) = self.mention.take() else {
            return;
        };
        let Some(member) = popup.results.get(index).cloned() else {
            return;
        };
        // The text may have moved since the list opened; find the query again.
        let text = self.text(cx);
        let cursor = self.input.read(cx).cursor();
        let Some(query) = model::mention_query(&text, cursor) else {
            return;
        };
        let insert = format!("@{} ", member.name);
        self.input.update(cx, |state, cx| {
            state.set_selected_range(query.start..query.end, cx);
            state.replace(insert.clone(), window, cx);
        });
        if !self.picked.contains(&member.id) {
            self.picked.push(member.id);
        }
        self.last_text = self.text(cx);
        if self.editing.is_none() {
            self.save_text(cx);
        }
        cx.notify();
    }

    // --- key handling (capture phase, before the input sees the key) ---

    fn on_enter(&mut self, action: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        if self.composing(window, cx) {
            // On macOS the IME gets Enter first and confirms the candidate.
            // If the key still reaches us, confirm it here; never send.
            self.input
                .update(cx, |state, cx| state.unmark_text(window, cx));
            cx.stop_propagation();
            return;
        }
        if action.shift {
            return; // the input adds a line
        }
        cx.stop_propagation();
        if let Some(popup) = &self.mention {
            let index = popup.selected;
            self.pick_mention(index, window, cx);
            return;
        }
        self.submit(window, cx);
    }

    fn on_tab(&mut self, _: &IndentInline, window: &mut Window, cx: &mut Context<Self>) {
        if self.composing(window, cx) {
            return;
        }
        if let Some(popup) = &self.mention {
            let index = popup.selected;
            cx.stop_propagation();
            self.pick_mention(index, window, cx);
        }
    }

    fn on_up(&mut self, _: &MoveUp, window: &mut Window, cx: &mut Context<Self>) {
        if self.composing(window, cx) {
            return;
        }
        if let Some(popup) = &mut self.mention {
            let n = popup.results.len();
            popup.selected = (popup.selected + n - 1) % n;
            cx.stop_propagation();
            cx.notify();
            return;
        }
        if self.editing.is_none() && self.text(cx).is_empty() && self.start_edit(window, cx) {
            cx.stop_propagation();
        }
    }

    fn on_down(&mut self, _: &MoveDown, window: &mut Window, cx: &mut Context<Self>) {
        if self.composing(window, cx) {
            return;
        }
        if let Some(popup) = &mut self.mention {
            popup.selected = (popup.selected + 1) % popup.results.len();
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn on_escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        if self.composing(window, cx) {
            return;
        }
        if self.mention.take().is_some() {
            cx.stop_propagation();
            cx.notify();
        } else if self.editing.is_some() {
            cx.stop_propagation();
            self.cancel_edit(window, cx);
        } else if self.quote.is_some() {
            cx.stop_propagation();
            self.remove_quote(cx);
        }
    }

    fn on_paste(&mut self, _: &Paste, _window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_some() || self.key.is_none() {
            return;
        }
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        let files = clipboard_files(item.entries());
        if !files.is_empty() {
            cx.stop_propagation();
            self.attach(files, cx);
        }
    }

    fn on_drop_paths(&mut self, paths: &ExternalPaths, cx: &mut Context<Self>) {
        if self.editing.is_some() {
            return;
        }
        let files = paths
            .paths()
            .iter()
            .cloned()
            .map(NewFile::Path)
            .collect::<Vec<_>>();
        self.attach(files, cx);
    }

    fn attach(&mut self, files: Vec<NewFile>, cx: &mut Context<Self>) {
        if let Some(key) = &self.key
            && !files.is_empty()
        {
            self.rejected.clear();
            cx.notify();
            self.services.provider.attach_files(key, files);
        }
    }

    /// The 添加文件 button: picks files with the system dialog.
    fn pick_files(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await {
                let files = paths.into_iter().map(NewFile::Path).collect();
                let _ = this.update(cx, |this, cx| this.attach(files, cx));
            }
        })
        .detach();
    }

    fn remove_file(&mut self, file_id: &str, cx: &mut Context<Self>) {
        if let Some(key) = &self.key {
            self.services.provider.remove_file(key, file_id);
        }
        cx.notify();
    }

    fn remove_quote(&mut self, cx: &mut Context<Self>) {
        self.quote = None;
        if let Some(key) = &self.key {
            self.services.provider.set_quote(key, None);
        }
        cx.notify();
    }

    // --- edit mode ---

    /// Starts editing my last message of the open topic. False when there is none.
    fn start_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(DraftKey {
            group_id,
            topic_id: Some(topic_id),
        }) = self.key.clone()
        else {
            return false;
        };
        let Some(me) = self.services.me() else {
            return false;
        };
        let window_msgs = self
            .services
            .provider
            .messages(&group_id, &topic_id, RECENT_MESSAGES);
        let Some(message) = model::last_editable(&window_msgs.messages, &me).cloned() else {
            return false;
        };
        self.editing = Some(EditTarget {
            group_id,
            message_id: message.id,
        });
        self.mention = None;
        self.set_text(&message.body, window, cx);
        self.update_placeholder(window, cx);
        cx.notify();
        true
    }

    fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = None;
        self.mention = None;
        self.too_long = false;
        // The composer was empty when the edit started; the draft keeps the rest.
        let body = self
            .key
            .as_ref()
            .map(|k| self.services.provider.draft(k).body)
            .unwrap_or_default();
        self.set_text(&body, window, cx);
        self.update_placeholder(window, cx);
        cx.notify();
    }

    // --- sending ---

    /// Sends the text (or saves the edit), or starts a topic when none is open.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.key.clone() else {
            return;
        };
        let body = self.text(cx).trim().to_string();
        if body.chars().count() > MAX_BODY_CHARS {
            self.too_long = true;
            cx.notify();
            return;
        }
        let provider = self.services.provider.clone();
        if let Some(edit) = self.editing.clone() {
            if body.is_empty() {
                return;
            }
            provider.edit(&edit.group_id, &edit.message_id, body);
            self.cancel_edit(window, cx);
            return;
        }
        if body.is_empty() && self.files.is_empty() {
            return;
        }
        let members = provider.members();
        let mentions = model::resolve_mentions(&body, &self.picked, &members);
        let files: Vec<_> = self.files.iter().map(|f| f.id.clone()).collect();
        let title =
            model::topic_title(&body).or_else(|| self.files.first().map(|f| f.name.clone()));
        let quote_of = self.quote.as_ref().map(|q| q.message_id.clone());

        // Clear first: opening the new topic below saves the old draft.
        self.picked.clear();
        self.quote = None;
        self.files.clear();
        self.rejected.clear();
        self.mention = None;
        self.set_text("", window, cx);

        match key.topic_id.clone() {
            Some(topic_id) => {
                self.typing.reset(&key.group_id, &topic_id);
                provider.send_with_files(
                    SendRequest {
                        group_id: key.group_id.clone(),
                        topic_id,
                        body,
                        mentions,
                        quote_of,
                    },
                    &key,
                    files,
                );
                provider.clear_draft(&key);
            }
            None => {
                let group_id = key.group_id.clone();
                provider.start_topic_with_files(
                    StartTopicRequest {
                        group_id: group_id.clone(),
                        title,
                        body,
                        mentions,
                    },
                    &key,
                    files,
                );
                provider.clear_draft(&key);
                // Open the new topic: it is the newest one in the group.
                if let Some(topic) = provider.topics(&group_id).into_iter().next() {
                    self.services
                        .nav
                        .update(cx, |nav, cx| nav.open_topic(group_id, topic.id, cx));
                }
            }
        }
        cx.notify();
    }
}

/// Text without Markdown emphasis marks, for one-line previews.
fn strip_marks(body: &str) -> String {
    body.replace("**", "")
        .replace("~~", "")
        .replace('`', "")
        .lines()
        .map(|l| l.trim_start_matches(['#', '>', ' ']))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Files and images on the clipboard (a copied Finder file, a screenshot).
fn clipboard_files(entries: &[ClipboardEntry]) -> Vec<NewFile> {
    let mut files = Vec::new();
    for entry in entries {
        match entry {
            ClipboardEntry::ExternalPaths(paths) => {
                files.extend(paths.paths().iter().cloned().map(NewFile::Path));
            }
            ClipboardEntry::Image(image) => files.push(NewFile::Image {
                mime: image.format.mime_type().to_string(),
                extension: image.format.extension().to_string(),
                bytes: image.bytes.clone().into(),
            }),
            ClipboardEntry::String(_) => {}
        }
    }
    files
}

/// Emoji in the picker: the reactions a team uses most.
const EMOJI: &[&str] = &[
    "👍", "👌", "🙏", "👏", "🎉", "✅", "❌", "👀", "🤔", "😂", "😅", "🙂", "😮", "😢", "🔥", "💯",
    "🚀", "⚠️", "📌", "💡", "☕", "🐛", "🛠️", "📎",
];

/// A Markdown format the format bar and ⌘B / ⌘I / ⌘⇧X apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    Bold,
    Italic,
    Strike,
    Link,
    Numbered,
    Bulleted,
    Quote,
    Code,
}

impl Format {
    const ALL: [Format; 8] = [
        Format::Bold,
        Format::Italic,
        Format::Strike,
        Format::Link,
        Format::Numbered,
        Format::Bulleted,
        Format::Quote,
        Format::Code,
    ];

    fn glyph(self) -> Lucide {
        match self {
            Format::Bold => Lucide::Bold,
            Format::Italic => Lucide::Italic,
            Format::Strike => Lucide::Strikethrough,
            Format::Link => Lucide::Link,
            Format::Numbered => Lucide::ListOrdered,
            Format::Bulleted => Lucide::List,
            Format::Quote => Lucide::Quote,
            Format::Code => Lucide::Code,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Format::Bold => c::BOLD,
            Format::Italic => c::ITALIC,
            Format::Strike => c::STRIKE,
            Format::Link => c::LINK,
            Format::Numbered => c::NUMBERED,
            Format::Bulleted => c::BULLETED,
            Format::Quote => c::QUOTE_BLOCK,
            Format::Code => c::CODE,
        }
    }
}

impl ComposerView {
    /// Applies a format to the selection: inline marks wrap it, block
    /// formats prefix its first line.
    fn apply_format(&mut self, format: Format, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.text(cx);
        let range = self.input.read(cx).selected_range();
        let range = range.start.min(text.len())..range.end.min(text.len());
        let selected = text.get(range.clone()).unwrap_or_default().to_string();
        let edit = model::format_edit(&text, range, &selected, model_format(format));
        self.input.update(cx, |state, cx| {
            state.focus(window, cx);
            state.set_selected_range(edit.range.clone(), cx);
            state.replace(edit.insert.clone(), window, cx);
            if let Some(caret) = edit.select.clone() {
                state.set_selected_range(caret, cx);
            }
        });
        self.on_text_changed(window, cx);
    }

    fn insert_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |state, cx| {
            state.focus(window, cx);
            state.replace(text.to_string(), window, cx);
        });
        self.on_text_changed(window, cx);
    }

    fn on_format_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let k = &event.keystroke;
        if !k.modifiers.platform || k.modifiers.control || k.modifiers.alt {
            return;
        }
        let format = match (k.key.as_str(), k.modifiers.shift) {
            ("b", false) => Format::Bold,
            ("i", false) => Format::Italic,
            ("x", true) => Format::Strike,
            _ => return,
        };
        cx.stop_propagation();
        self.apply_format(format, window, cx);
    }

    fn render_chip(
        &self,
        id: ElementId,
        glyph: Lucide,
        text: String,
        on_remove: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = theme::colors(cx);
        div()
            .flex()
            .items_center()
            .gap(space::S6)
            .max_w(px(360.))
            .h(px(28.))
            .pl(space::S8)
            .pr(space::S2)
            .rounded(theme::radius::SM)
            .bg(colors.field)
            .t_small()
            .text_color(colors.text_2)
            .child(ui::icon(glyph, theme::icon::CHIP, colors.text_3))
            .child(div().min_w(px(0.)).one_line().child(text))
            .child(
                ui::icon_button(id, Lucide::X)
                    .with_size(px(24.))
                    .rounded(theme::radius::SM)
                    .icon(ui::button_glyph(Lucide::X, theme::icon::CHIP, px(24.)))
                    .tooltip(c::REMOVE)
                    .on_click(cx.listener(move |this, _, _, cx| on_remove(this, cx))),
            )
    }

    /// Left edge of the @ list inside the composer: 12 pt left of the `@`
    /// being typed (§7.2), kept inside the composer.
    fn mention_left(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui::Pixels {
        let anchor = self.anchor.get();
        let Some(popup) = &self.mention else {
            return px(0.);
        };
        let Some((caret, _)) = self.input.read(cx).cursor_layout() else {
            return px(0.);
        };
        let typed: SharedString = format!("@{}", popup.query.text).into();
        let style = window.text_style();
        let run = gpui::TextRun {
            len: typed.len(),
            font: style.font(),
            color: style.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let width = window
            .text_system()
            .shape_line(typed, theme::type_scale::BODY.0, &[run], None)
            .width;
        let at = caret.origin.x - width - anchor.origin.x - space::S12;
        let max = (anchor.size.width - theme::size::MENTION_POPUP_WIDTH).max(px(0.));
        at.max(px(0.)).min(max)
    }

    fn render_mentions(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let popup = self.mention.as_ref()?;
        let colors = theme::colors(cx);
        let left = self.mention_left(window, cx);
        let rows = popup.results.iter().enumerate().map(|(i, m)| {
            let active = i == popup.selected;
            ui::popup_row(
                ElementId::Name(SharedString::from(format!("mention-{}", m.id))),
                active,
                cx,
            )
            .child(ui::MemberAvatar::new(Some(m), px(24.)).on(colors.overlay))
            .child(div().flex_none().child(m.name.clone()))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .t_small()
                    .text_color(colors.text_3)
                    .one_line()
                    .children(m.role.clone()),
            )
            .when(active, |el| el.child(ui::return_hint(cx)))
            .on_click(cx.listener(move |this, _, window, cx| this.pick_mention(i, window, cx)))
        });
        Some(
            ui::mention_surface(cx)
                .id("mention-popup")
                .absolute()
                .left(left)
                .bottom(relative(1.))
                .mb(space::S8)
                .overflow_y_scroll()
                .child(
                    div()
                        .flex_none()
                        .h(px(28.))
                        .px(space::S10)
                        .flex()
                        .items_center()
                        .t_meta_medium()
                        .text_color(colors.text_3)
                        .child(crate::copy::nav::SECTION_MEMBERS),
                )
                .children(rows),
        )
    }

    fn render_emoji(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if !self.emoji_open {
            return None;
        }
        let colors = theme::colors(cx);
        let reduce = cx.reduce_motion();
        Some(
            ui::menu_surface(cx)
                .id("emoji-picker")
                .absolute()
                .left(px(72.))
                .bottom(relative(1.))
                .mb(space::S8)
                .w(px(8. * 36. + 8.))
                .flex_row()
                .flex_wrap()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.emoji_open = false;
                    cx.notify();
                }))
                .children(EMOJI.iter().enumerate().map(|(i, e)| {
                    div()
                        .id(("emoji", i))
                        .size(px(36.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(theme::radius::SM)
                        .cursor_pointer()
                        .text_size(px(20.))
                        .hover(|el| el.bg(colors.hover))
                        .active(|el| el.bg(colors.pressed))
                        .child(*e)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.emoji_open = false;
                            this.insert_text(e, window, cx);
                        }))
                }))
                .with_animation(
                    "emoji-picker-in",
                    // Popovers: 150 ms opacity + 4 pt travel from the
                    // trigger; opacity only with reduce motion (§12).
                    gpui::Animation::new(theme::motion::MENU).with_easing(theme::motion::out_fn()),
                    move |el, t| {
                        let el = el.opacity(t);
                        if reduce {
                            el
                        } else {
                            el.mb(space::S8 - theme::motion::TRAVEL * (1. - t))
                        }
                    },
                ),
        )
    }
}

fn model_format(f: Format) -> model::Format {
    match f {
        Format::Bold => model::Format::Wrap("**"),
        Format::Italic => model::Format::Wrap("_"),
        Format::Strike => model::Format::Wrap("~~"),
        Format::Code => model::Format::Wrap("`"),
        Format::Link => model::Format::Link,
        Format::Numbered => model::Format::Line("1. "),
        Format::Bulleted => model::Format::Line("- "),
        Format::Quote => model::Format::Line("> "),
    }
}

impl Render for ComposerView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let enabled = self.key.is_some();
        let editing = self.editing.is_some();
        let new_topic = !editing && self.key.as_ref().is_some_and(|k| k.topic_id.is_none());
        // The placeholder already says the first line is the title; once
        // there is text, the line shows the title it makes.
        let title_line = new_topic
            .then(|| model::topic_title(&self.last_text))
            .flatten()
            .map(|title| fill(c::NEW_TOPIC_TITLE, &[("title", &title)]));
        let quote = (!editing).then(|| self.quote.clone()).flatten();
        let files = if editing {
            Vec::new()
        } else {
            self.files.clone()
        };
        let has_chips = quote.is_some() || !files.is_empty();

        let mut chips = div().flex().flex_wrap().gap(space::S6);
        if let Some(q) = quote {
            chips = chips.child(self.render_chip(
                "quote-remove".into(),
                Lucide::TextQuote,
                q.text,
                |this, cx| this.remove_quote(cx),
                cx,
            ));
        }
        for file in files {
            let id = file.id.clone();
            chips = chips.child(self.render_chip(
                ElementId::Name(SharedString::from(format!("file-remove-{}", file.id))),
                Lucide::Paperclip,
                file.name,
                move |this, cx| this.remove_file(&id, cx),
                cx,
            ));
        }

        let focused = self.input.read(cx).focus_handle(cx).is_focused(window);
        let has_text = !self.last_text.trim().is_empty() || has_chips;
        let send = ui::send_button(
            "send",
            has_text && enabled,
            if editing {
                crate::copy::common::SAVE
            } else {
                c::SEND_HINT
            },
            cx,
        )
        .when(editing, |b| {
            b.icon(ui::button_glyph(
                Lucide::Check,
                theme::icon::HEADER,
                theme::size::ICON_BUTTON,
            ))
        })
        .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx)));

        let mut shell = ui::ComposerShell::new(
            "composer-box",
            div()
                .relative()
                .w_full()
                .capture_action(cx.listener(Self::on_enter))
                .capture_action(cx.listener(Self::on_tab))
                .capture_action(cx.listener(Self::on_up))
                .capture_action(cx.listener(Self::on_down))
                .capture_action(cx.listener(Self::on_escape))
                .capture_action(cx.listener(Self::on_paste))
                .capture_key_down(cx.listener(Self::on_format_key))
                .child(
                    // XSmall drops the editor's own inset; the shell pads the
                    // text 12 top and 14 at the sides (DESIGN.md §7.2).
                    Textarea::new(&self.input)
                        .appearance(false)
                        .xsmall()
                        .p_0()
                        .t_body()
                        .disabled(!enabled),
                ),
        )
        .focused(focused && enabled)
        .disabled(!enabled)
        .tool(
            ui::composer_tool("attach", Lucide::Plus, c::ATTACH)
                .disabled(!enabled || editing)
                .on_click(cx.listener(|this, _, _, cx| this.pick_files(cx))),
        )
        .tool(
            ui::composer_tool("mention", Lucide::AtSign, c::MENTION)
                .disabled(!enabled)
                .on_click(cx.listener(|this, _, window, cx| this.insert_text("@", window, cx))),
        )
        .tool(
            ui::composer_tool("emoji", Lucide::FaceSlightlySmiling, c::EMOJI)
                .disabled(!enabled)
                .when(self.emoji_open, |b| b.selected(true))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.emoji_open = !this.emoji_open;
                    cx.notify();
                })),
        )
        .tool(
            ui::composer_tool("format", Lucide::Type, c::FORMAT)
                .disabled(!enabled)
                .when(self.format_open, |b| b.selected(true))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.format_open = !this.format_open;
                    cx.notify();
                })),
        )
        .send(send);
        if self.format_open && enabled {
            shell =
                shell.format_bar(
                    Format::ALL
                        .iter()
                        .map(|&f| {
                            ui::composer_tool(
                                ElementId::Name(format!("fmt-{f:?}").into()),
                                f.glyph(),
                                f.label(),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.apply_format(f, window, cx)
                            }))
                            .into_any_element()
                        })
                        .collect(),
                );
        }
        if editing {
            shell = shell.above(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .t_small()
                    .child(div().semibold().text_color(colors.text_1).child(c::EDITING))
                    .child(
                        ui::text_button("cancel-edit", crate::copy::common::CANCEL, cx).on_click(
                            cx.listener(|this, _, window, cx| this.cancel_edit(window, cx)),
                        ),
                    ),
            );
        }
        if let Some(line) = title_line {
            shell = shell.above(
                div()
                    .t_small()
                    .text_color(colors.text_2)
                    .one_line()
                    .child(line),
            );
        }
        if has_chips {
            shell = shell.above(chips);
        }
        if self.too_long {
            shell = shell.above(
                div()
                    .t_meta()
                    .text_color(colors.danger_text)
                    .child(c::TOO_LONG),
            );
        }
        for line in &self.rejected {
            shell = shell.above(
                div()
                    .t_meta()
                    .text_color(colors.danger_text)
                    .child(line.clone()),
            );
        }

        let anchor = self.anchor.clone();
        div()
            .id("composer")
            .flex_none()
            .px(theme::CONTENT_GUTTER)
            .pb(space::S16)
            .pt(space::S8)
            .bg(colors.content)
            .on_drop(
                cx.listener(|this, paths: &ExternalPaths, _, cx| this.on_drop_paths(paths, cx)),
            )
            .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(colors.hover))
            .child(
                div()
                    .relative()
                    .w_full()
                    .child(shell)
                    .child(
                        gpui::canvas(move |bounds, _, _| anchor.set(bounds), |_, _, _, _| {})
                            .absolute()
                            .size_full(),
                    )
                    .children(self.render_mentions(window, cx))
                    .children(self.render_emoji(cx)),
            )
    }
}
