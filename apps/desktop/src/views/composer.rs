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
    AppContext, ClipboardEntry, Context, ElementId, Entity, EntityInputHandler, ExternalPaths,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::FluentBuilder as _, px,
    relative,
};
use gpui_component::{
    Disableable as _,
    button::{Button, ButtonVariants as _},
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
    theme,
    views::avatar,
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
    _subscriptions: Vec<Subscription>,
}

impl ComposerView {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 8)
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
            _subscriptions: subscriptions,
        };
        this.sync_key(window, cx);
        this
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
        let placeholder = if self.editing.is_some() {
            c::PLACEHOLDER_EDIT
        } else if self.key.as_ref().is_some_and(|k| k.topic_id.is_none()) {
            c::PLACEHOLDER_NEW_TOPIC
        } else {
            c::PLACEHOLDER
        };
        self.input.update(cx, |state, cx| {
            state.set_placeholder(placeholder, window, cx)
        });
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
            self.rejected = rejected.iter().map(crate::views::files::rejected_text).collect();
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
                    model::excerpt(&m.body, 40)
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

impl ComposerView {
    fn render_chip(
        &self,
        id: ElementId,
        text: String,
        on_remove: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = theme::colors(cx);
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .max_w(px(360.))
            .px(px(8.))
            .py(px(2.))
            .rounded(px(4.))
            .bg(colors.code_background)
            .text_xs()
            .child(div().truncate().child(text))
            .child(
                div()
                    .id(id)
                    .flex_none()
                    .text_color(colors.muted_foreground)
                    .cursor_pointer()
                    .hover(|s| s.text_color(colors.foreground))
                    .child(c::REMOVE)
                    .on_click(cx.listener(move |this, _, _, cx| on_remove(this, cx))),
            )
    }

    fn render_mentions(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let popup = self.mention.as_ref()?;
        let colors = theme::colors(cx);
        let rows = popup.results.iter().enumerate().map(|(i, m)| {
            div()
                .id(ElementId::Name(SharedString::from(format!(
                    "mention-{}",
                    m.id
                ))))
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(8.))
                .py(px(4.))
                .rounded(px(4.))
                .cursor_pointer()
                .when(i == popup.selected, |el| el.bg(colors.row_selected))
                .hover(|s| s.bg(colors.row_hover))
                .child(avatar(Some(m), px(20.), cx))
                .child(div().text_sm().child(m.name.clone()))
                .when_some(m.role.clone(), |el, role| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .truncate()
                            .child(role),
                    )
                })
                .on_click(cx.listener(move |this, _, window, cx| this.pick_mention(i, window, cx)))
        });
        Some(
            div()
                .absolute()
                .left(px(0.))
                .bottom(relative(1.))
                .mb(px(4.))
                .w(px(300.))
                .p(px(4.))
                .rounded(px(6.))
                .border_1()
                .border_color(colors.border)
                .bg(colors.background)
                .shadow_md()
                .flex()
                .flex_col()
                .children(rows),
        )
    }
}

impl Render for ComposerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let enabled = self.key.is_some();
        let editing = self.editing.is_some();
        let new_topic = !editing && self.key.as_ref().is_some_and(|k| k.topic_id.is_none());
        let title_line = new_topic.then(|| match model::topic_title(&self.last_text) {
            Some(title) => fill(c::NEW_TOPIC_TITLE, &[("title", &title)]),
            None => c::NEW_TOPIC_NO_TITLE.to_string(),
        });
        let quote = (!editing).then(|| self.quote.clone()).flatten();
        let files = if editing {
            Vec::new()
        } else {
            self.files.clone()
        };
        let has_chips = quote.is_some() || !files.is_empty();

        let mut chips = div().flex().flex_wrap().gap(px(6.));
        if let Some(q) = quote {
            chips = chips.child(self.render_chip(
                "quote-remove".into(),
                q.text,
                |this, cx| this.remove_quote(cx),
                cx,
            ));
        }
        for file in files {
            let id = file.id.clone();
            chips = chips.child(self.render_chip(
                ElementId::Name(SharedString::from(format!("file-remove-{}", file.id))),
                file.name,
                move |this, cx| this.remove_file(&id, cx),
                cx,
            ));
        }

        div()
            .id("composer")
            .p(px(12.))
            .border_t_1()
            .border_color(colors.border)
            .flex()
            .flex_col()
            .gap(px(6.))
            .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(colors.row_hover))
            .on_drop(
                cx.listener(|this, paths: &ExternalPaths, _, cx| this.on_drop_paths(paths, cx)),
            )
            .when(editing, |el| {
                el.child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .text_xs()
                        .child(div().text_color(colors.accent).child(c::EDITING))
                        .child(
                            div()
                                .text_color(colors.muted_foreground)
                                .child(c::EDIT_HINT),
                        ),
                )
            })
            .when_some(title_line, |el, line| {
                el.child(
                    div()
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .truncate()
                        .child(line),
                )
            })
            .when(has_chips, |el| el.child(chips))
            .when(self.too_long, |el| {
                el.child(div().text_xs().text_color(colors.danger).child(c::TOO_LONG))
            })
            .children(
                self.rejected
                    .iter()
                    .map(|line| div().text_xs().text_color(colors.danger).child(line.clone())),
            )
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap(px(8.))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .capture_action(cx.listener(Self::on_enter))
                            .capture_action(cx.listener(Self::on_tab))
                            .capture_action(cx.listener(Self::on_up))
                            .capture_action(cx.listener(Self::on_down))
                            .capture_action(cx.listener(Self::on_escape))
                            .capture_action(cx.listener(Self::on_paste))
                            .child(Textarea::new(&self.input).disabled(!enabled))
                            .children(self.render_mentions(cx)),
                    )
                    .child(
                        Button::new("attach")
                            .ghost()
                            .icon(gpui_component::IconName::Plus)
                            .tooltip(c::ATTACH)
                            .disabled(!enabled || editing)
                            .on_click(cx.listener(|this, _, _, cx| this.pick_files(cx))),
                    )
                    .child(
                        Button::new("send")
                            .primary()
                            .label(if editing {
                                crate::copy::common::SAVE
                            } else {
                                c::SEND
                            })
                            .disabled(!enabled)
                            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                    ),
            )
    }
}
