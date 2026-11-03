//! The composer: a multi-line field that grows to 7 lines. Chinese IME works
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
    Focusable as _, InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::FluentBuilder as _, px,
    relative,
};
use gpui_component::{
    Disableable as _, Sizable as _,
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
    _subscriptions: Vec<Subscription>,
}

impl ComposerView {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 7)
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
        // `在「登录页改版」里回复` (§13.6); a long title is cut so the
        // placeholder stays one line.
        let topic_title = self.key.as_ref().and_then(|k| {
            let t = k.topic_id.as_ref()?;
            let title = self.services.provider.topic(&k.group_id, t)?.title;
            let line = crate::views::sidebar::topic_line(&title);
            (!line.trim().is_empty()).then(|| model::excerpt(&line, 24))
        });
        let placeholder = if self.editing.is_some() {
            c::PLACEHOLDER_EDIT.to_string()
        } else if self.key.as_ref().is_some_and(|k| k.topic_id.is_none()) {
            c::PLACEHOLDER_NEW_TOPIC.to_string()
        } else if let Some(title) = topic_title {
            fill(c::PLACEHOLDER_TOPIC, &[("title", &title)])
        } else {
            c::PLACEHOLDER.to_string()
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
        let mut results = results;
        results.truncate(model::MENTION_ROWS);
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

    // --- screenshot scenes ---

    /// Types `text` as the user would (the @ list opens after an `@`).
    pub fn preview_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.set_text(text, window, cx);
        self.update_mention(text, cx);
        cx.notify();
    }

    /// Starts editing my last message, as Up in an empty composer does.
    pub fn preview_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.start_edit(window, cx);
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
    /// The quoted message above the text: a quote block like the timeline's
    /// (2 pt `quote-bar`, 10 pt indent, 13 / 19 `text-2`) with an `x` (§13.6).
    fn render_quote(&self, quote: &QuoteChip, cx: &mut Context<Self>) -> gpui::AnyElement {
        let colors = theme::colors(cx);
        div()
            .w_full()
            .flex()
            .items_start()
            .gap(space::S8)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .pl(space::S10)
                    .border_l_2()
                    .border_color(colors.quote_bar)
                    .type_role(theme::type_scale::QUOTE)
                    .text_color(colors.text_2)
                    .one_line()
                    .child(quote.text.clone()),
            )
            .child(
                ui::icon_button("quote-remove", Lucide::X)
                    .xsmall()
                    .tooltip(c::REMOVE)
                    .on_click(cx.listener(|this, _, _, cx| this.remove_quote(cx))),
            )
            .into_any_element()
    }

    /// A pending file: gpui-component's `Attachment` (small) with its icon,
    /// name (middle ellipsis keeps the extension) and size, removable.
    fn render_file(&self, file: &PendingFile, cx: &mut Context<Self>) -> gpui::AnyElement {
        use gpui_component::attachment::{
            Attachment, AttachmentContent, AttachmentDescription, AttachmentMedia,
            AttachmentTitle,
        };
        let colors = theme::colors(cx);
        let icon = if file.is_image {
            Lucide::Image
        } else {
            Lucide::File
        };
        let id = file.id.clone();
        let mut content =
            AttachmentContent::new().title(AttachmentTitle::new(file.name.clone()));
        if let Some(size) = file.size_bytes {
            content = content.description(AttachmentDescription::new(
                crate::views::files::size_text(size),
            ));
        }
        Attachment::new()
            .id(ElementId::Name(SharedString::from(format!(
                "file-chip-{}",
                file.id
            ))))
            .small()
            .max_w(px(240.))
            .tooltip(file.name.clone())
            .media(AttachmentMedia::new().child(ui::icon(
                icon,
                theme::icon::BUTTON,
                colors.text_2,
            )))
            .content(content)
            .on_remove(cx.listener(move |this, _, _, cx| this.remove_file(&id, cx)))
            .into_any_element()
    }

    fn render_mentions(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let popup = self.mention.as_ref()?;
        let colors = theme::colors(cx);
        let query = popup.query.text.clone();
        let rows = popup.results.iter().enumerate().map(|(i, m)| {
            // The typed part of the name is underlined in 600 (§13.6).
            let name = gpui::StyledText::new(m.name.clone()).with_highlights(
                model::match_range(&m.name, &query).map(|range| {
                    (
                        range,
                        gpui::HighlightStyle {
                            font_weight: Some(gpui::FontWeight::SEMIBOLD),
                            underline: Some(gpui::UnderlineStyle {
                                thickness: px(1.),
                                color: Some(colors.text_1),
                                wavy: false,
                            }),
                            ..Default::default()
                        },
                    )
                }),
            );
            div()
                .id(ElementId::Name(SharedString::from(format!(
                    "mention-{}",
                    m.id
                ))))
                .flex()
                .items_center()
                .gap(space::S8)
                .h(px(32.))
                .px(space::S6)
                .rounded(theme::radius::SM)
                .when(i == popup.selected, |el| el.bg(colors.fill_active))
                .child(ui::MemberAvatar::new(Some(m), px(24.)).on(colors.elevated))
                .child(
                    div()
                        .flex_shrink(1.)
                        .min_w(px(0.))
                        .t_ui()
                        .medium()
                        .text_color(colors.text_1)
                        .one_line()
                        .child(name),
                )
                .when_some(m.role.clone().filter(|r| !r.trim().is_empty()), |el, role| {
                    el.child(
                        div()
                            .flex_shrink(2.)
                            .min_w(px(0.))
                            .t_callout()
                            .text_color(colors.text_2)
                            .one_line()
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
                .mb(space::S8)
                .w(px(300.))
                .rounded(theme::radius::LG)
                .bg(colors.elevated)
                .shadow(colors.shadow(theme::Elevation::Popover))
                .flex()
                .flex_col()
                .child(
                    div()
                        .px(space::S10)
                        .pt(space::S8)
                        .pb(space::S4)
                        .t_section()
                        .text_color(colors.text_3)
                        .child(crate::copy::nav::SECTION_MEMBERS),
                )
                .child(div().px(px(5.)).pb(px(5.)).flex().flex_col().children(rows))
                .child(ui::hairline(cx))
                .child(
                    div()
                        .h(px(28.))
                        .px(space::S10)
                        .flex()
                        .items_center()
                        .gap(space::S12)
                        .child(ui::KbdHint::new(&["up", "down"]).label(c::HINT_SELECT))
                        .child(ui::KbdHint::new(&["enter"]).label(c::HINT_MENTION))
                        .child(ui::KbdHint::new(&["escape"]).label(crate::copy::common::CLOSE)),
                ),
        )
    }

    /// The `@` tool: types `@` at the cursor, which opens the member list.
    fn insert_at(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |state, cx| state.focus(window, cx));
        self.input.update(cx, |state, cx| state.replace("@", window, cx));
    }
}

impl Render for ComposerView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        let quote_block = match &quote {
            Some(q) => Some(self.render_quote(q, cx)),
            None => None,
        };
        let mut file_chips = Vec::new();
        for f in &files {
            file_chips.push(self.render_file(f, cx));
        }
        let file_row = (!file_chips.is_empty())
            .then(|| div().flex().flex_wrap().gap(space::S6).children(file_chips));

        let focused = self.input.read(cx).focus_handle(cx).is_focused(window);
        let has_text = !self.last_text.trim().is_empty() || has_chips;
        let send = Button::new("send")
            .icon(gpui_component::Icon::new(if editing {
                Lucide::Check
            } else {
                Lucide::ArrowUp
            }))
            .with_size(px(28.))
            .rounded_full()
            .tooltip(if editing {
                crate::copy::common::SAVE
            } else {
                c::SEND
            })
            .disabled(!enabled)
            .map(|b| {
                if has_text && enabled {
                    b.primary()
                } else {
                    b.ghost().bg(colors.fill_pressed).text_color(colors.text_3)
                }
            })
            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx)));

        div()
            .id("composer")
            .flex_none()
            .px(space::S20)
            .pb(space::S16)
            .bg(colors.content)
            .on_drop(
                cx.listener(|this, paths: &ExternalPaths, _, cx| this.on_drop_paths(paths, cx)),
            )
            .child(
                div()
                    .id("composer-box")
                    .rounded(theme::radius::COMPOSER)
                    .border_1()
                    .border_color(if focused {
                        colors.control_border_focus
                    } else {
                        colors.control_border
                    })
                    .bg(colors.content)
                    // 10 / 12 / 8 with the field's own 2 / 4 pt inset (§5.4).
                    .pt(space::S8)
                    .px(space::S8)
                    .pb(space::S8)
                    .flex()
                    .flex_col()
                    .gap(space::S6)
                    .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(colors.fill_hover))
                    .when(editing, |el| {
                        el.child(
                            div()
                                .px(space::S4)
                                .flex()
                                .items_center()
                                .gap(space::S6)
                                .t_callout()
                                .semibold()
                                .text_color(colors.text_1)
                                .child(ui::icon(Lucide::Pencil, theme::icon::SMALL, colors.text_2))
                                .child(c::EDITING),
                        )
                    })
                    .when_some(title_line, |el, line| {
                        el.child(
                            div()
                                .px(space::S4)
                                .t_callout()
                                .text_color(colors.text_2)
                                .one_line()
                                .child(line),
                        )
                    })
                    .when_some(quote_block, |el, q| el.child(div().px(space::S4).child(q)))
                    .when_some(file_row, |el, row| el.child(div().px(space::S4).child(row)))
                    .when(self.too_long, |el| {
                        el.child(
                            div()
                                .px(space::S4)
                                .t_callout()
                                .text_color(colors.danger_text)
                                .child(c::TOO_LONG),
                        )
                    })
                    .children(self.rejected.iter().map(|line| {
                        div()
                            .px(space::S4)
                            .t_callout()
                            .text_color(colors.danger_text)
                            .child(line.clone())
                    }))
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .capture_action(cx.listener(Self::on_enter))
                            .capture_action(cx.listener(Self::on_tab))
                            .capture_action(cx.listener(Self::on_up))
                            .capture_action(cx.listener(Self::on_down))
                            .capture_action(cx.listener(Self::on_escape))
                            .capture_action(cx.listener(Self::on_paste))
                            .child(
                                // XSmall keeps the editor's own inset at 4 / 2 pt;
                                // the box padding makes up the rest (§5.4).
                                Textarea::new(&self.input)
                                    .appearance(false)
                                    .xsmall()
                                    .t_message()
                                    .disabled(!enabled),
                            )
                            .children(self.render_mentions(cx)),
                    )
                    .child(
                        div()
                            .h(px(28.))
                            .flex()
                            .items_center()
                            .gap(space::S4)
                            .child(
                                ui::icon_button("attach", Lucide::Paperclip)
                                    .tooltip(c::ATTACH)
                                    .disabled(!enabled || editing)
                                    .on_click(cx.listener(|this, _, _, cx| this.pick_files(cx))),
                            )
                            .child(
                                ui::icon_button("mention", Lucide::AtSign)
                                    .tooltip(c::MENTION)
                                    .disabled(!enabled)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.insert_at(window, cx)
                                    })),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(space::S12)
                                    .mr(space::S8)
                                    .map(|el| {
                                        if editing {
                                            el.child(ui::KbdHint::new(&["enter"]).label(c::HINT_SAVE))
                                                .child(
                                                    ui::KbdHint::new(&["escape"])
                                                        .label(c::HINT_CANCEL),
                                                )
                                        } else {
                                            el.child(ui::KbdHint::new(&["enter"]).label(c::HINT_SEND))
                                                .child(
                                                    ui::KbdHint::new(&["shift-enter"])
                                                        .label(c::HINT_NEWLINE),
                                                )
                                        }
                                    }),
                            )
                            .child(send),
                    ),
            )
    }
}
