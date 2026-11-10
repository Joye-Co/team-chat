//! Find in the open 话题 (⌘F), as in Mail, Notes and Safari: a find bar
//! under the toolbar row, a search field, `n / m`, the previous and next
//! match and 完成.
//!
//! It searches the text of the loaded messages only (the window the
//! timeline holds), case-insensitively and by characters, so Chinese,
//! Japanese and Korean text match as typed. Every match gets a light fill
//! behind its words (`<mark>` in the body source), the current one a
//! stronger fill; Return / ⌘G goes to the next match, ⇧Return / ⇧⌘G to the
//! previous one, wrapping around; the list scrolls the match's row into
//! view. Escape or 完成 closes the bar and clears the marks.
//!
//! The menu bar owns the 编辑 menu: it lists [`FindInTopic`] (查找…),
//! [`FindNext`] (查找下一个) and [`FindPrevious`] (查找上一个); the keys
//! are bound here ([`install`]).

use std::ops::Range;

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, Hsla, InteractiveElement as _, IntoElement,
    KeyBinding, ParentElement as _, Styled as _, Subscription, Window, div,
};
use gpui_component::input::{InputEvent, InputState};

use super::{TimelineView, extras, model, v2};
use crate::{
    copy::{fill, timeline as t},
    theme::{self, Type as _, space},
    ui::Sym,
};

gpui::actions!(
    timeline,
    [
        /// 查找… (⌘F): open the find bar of the open 话题 and focus its field.
        FindInTopic,
        /// 查找下一个 (⌘G): the next match (further down), wrapping around.
        FindNext,
        /// 查找上一个 (⇧⌘G): the previous match (further up), wrapping around.
        FindPrevious,
    ]
);

/// The keys of find and their global handlers. Call once at start-up, after
/// `nav::install` (the handlers run in the main window).
///
/// ⌘F opens the find bar while a 话题, 群聊 or 私聊 is open; on any other
/// page it opens the switcher scoped to the current 群, as ⌘F did before.
/// Inside the switcher its own ⌘F (scope to the 群) wins: it is bound in the
/// switcher's key context, deeper than this one.
pub fn install(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(crate::secondary!("f"), FindInTopic, None),
        KeyBinding::new(crate::secondary!("g"), FindNext, None),
        KeyBinding::new(crate::secondary!("shift-g"), FindPrevious, None),
    ]);
    cx.on_action(|_: &FindInTopic, cx| {
        crate::views::nav::in_main_window(cx, |window, cx| {
            use gpui_component::WindowExt as _;
            if window.has_active_dialog(cx) {
                return;
            }
            let services = crate::views::nav::services_of(cx);
            let on_topic = services.nav.read(cx).page() == crate::app::Page::Topic;
            match timeline_of(window, cx) {
                Some(timeline) if on_topic => {
                    timeline.update(cx, |this, cx| this.open_find(window, cx))
                }
                _ => crate::views::search::open(services, false, "", window, cx),
            }
        })
    });
    cx.on_action(|_: &FindNext, cx| step_in_main_window(true, cx));
    cx.on_action(|_: &FindPrevious, cx| step_in_main_window(false, cx));
}

fn timeline_of(window: &Window, cx: &App) -> Option<Entity<TimelineView>> {
    crate::app::root_view(window, cx).map(|root| root.read(cx).timeline().clone())
}

fn step_in_main_window(forward: bool, cx: &mut App) {
    crate::views::nav::in_main_window(cx, move |window, cx| {
        if let Some(timeline) = timeline_of(window, cx) {
            timeline.update(cx, |this, cx| this.step_find(forward, cx));
        }
    });
}

// ---------------------------------------------------------------------------
// Matching (pure, tested below)

/// The byte ranges of `query` in `text`, in order and not overlapping:
/// case-insensitive (Unicode lowercase, character by character) and on
/// character boundaries, so CJK text and mixed scripts match as typed. An
/// empty query matches nothing.
pub fn match_ranges(text: &str, query: &str) -> Vec<Range<usize>> {
    if query.is_empty() || text.is_empty() {
        return Vec::new();
    }
    let needle = fold(query);
    // The folded text, and for each folded char the source char it came
    // from (its start and end in `text`).
    let mut folded = String::with_capacity(text.len());
    let mut origin: Vec<(usize, Range<usize>)> = Vec::with_capacity(text.len());
    for (at, c) in text.char_indices() {
        let src = at..at + c.len_utf8();
        let start = folded.len();
        folded.extend(c.to_lowercase());
        origin.push((start, src));
    }
    // The source char whose folded text holds folded byte `ix`.
    let source_of = |ix: usize| {
        let i = origin.partition_point(|(start, _)| *start <= ix) - 1;
        origin[i].1.clone()
    };
    folded
        .match_indices(&needle)
        .map(|(at, m)| source_of(at).start..source_of(at + m.len() - 1).end)
        .fold(Vec::new(), |mut out: Vec<Range<usize>>, r| {
            // A char that folds to several chars can map two matches onto
            // one source char: keep the ranges apart.
            if out.last().is_none_or(|last| last.end <= r.start) {
                out.push(r);
            }
            out
        })
}

fn fold(s: &str) -> String {
    s.chars().flat_map(char::to_lowercase).collect()
}

/// One match: the row's key, its row index in the window, and the byte
/// range in the row's shown body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindMatch {
    pub key: String,
    pub row: usize,
    pub range: Range<usize>,
}

impl FindMatch {
    /// What keeps a match when the window changes: its row and its range.
    fn anchor(&self) -> (String, usize) {
        (self.key.clone(), self.range.start)
    }
}

/// The match after (`forward`) or before `current`, wrapping around. With
/// no current match, the first (forward) or the last.
pub fn step_index(current: Option<usize>, len: usize, forward: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match (current, forward) {
        (None, true) => 0,
        (None, false) => len - 1,
        (Some(i), true) => (i + 1) % len,
        (Some(i), false) => (i + len - 1) % len,
    })
}

/// Where a new search starts: the first match at or below the first row on
/// screen (as Safari does), else the last one.
pub fn start_index(matches: &[FindMatch], top_row: usize) -> Option<usize> {
    if matches.is_empty() {
        return None;
    }
    Some(
        matches
            .iter()
            .position(|m| m.row >= top_row)
            .unwrap_or(matches.len() - 1),
    )
}

/// The byte ranges of `source` that are code: fenced blocks (with their
/// fence lines) and inline code spans (with their backticks).
fn code_ranges(source: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut fenced = false;
    let mut at = 0;
    for line in source.split_inclusive('\n') {
        let start = at;
        at += line.len();
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            out.push(start..at);
            continue;
        }
        if fenced {
            out.push(start..at);
            continue;
        }
        let mut open: Option<usize> = None;
        for (i, _) in line.match_indices('`') {
            match open.take() {
                Some(o) => out.push(start + o..start + i + 1),
                None => open = Some(i),
            }
        }
    }
    out
}

/// Text a mark must not wrap: Markdown and HTML syntax, and line breaks.
const UNMARKABLE: &[char] = &[
    '<', '>', '[', ']', '(', ')', '`', '*', '_', '~', '\\', '|', '\n', '\r', '@',
];

/// Whether the match `r` of `source` can carry a `<mark>` without changing
/// how the Markdown renders: not in code, not around syntax, not inside a
/// link, an address or an @mention, and not at the start of a line that
/// starts with a block marker.
fn markable(source: &str, r: &Range<usize>, code: &[Range<usize>]) -> bool {
    let text = &source[r.clone()];
    if text.contains(UNMARKABLE) || code.iter().any(|c| c.start < r.end && r.start < c.end) {
        return false;
    }
    let word_start = source[..r.start].rfind(char::is_whitespace).map_or(0, |i| {
        i + source[i..].chars().next().map_or(1, char::len_utf8)
    });
    let word_end = source[r.end..]
        .find(char::is_whitespace)
        .map_or(source.len(), |i| r.end + i);
    let word = &source[word_start..word_end];
    if word.contains("http") || word.contains("](") || word.contains('@') || word.contains('<') {
        return false;
    }
    let line_start = source[..r.start].rfind('\n').map_or(0, |i| i + 1);
    let at_line_start = source[line_start..r.start].trim().is_empty();
    let first = text.chars().next().unwrap_or(' ');
    !(at_line_start && (first.is_ascii_punctuation() || first.is_ascii_digit()))
}

/// `source` with every match in `ranges` wrapped in a `<mark>` of `other`,
/// and the match `current` (an index into `ranges`) in one of `strong`.
/// Matches that cannot carry a mark ([`markable`]) stay as they are. The
/// flag says whether the current match got its mark.
pub fn mark_matches(
    source: &str,
    ranges: &[Range<usize>],
    current: Option<usize>,
    other: Hsla,
    strong: Hsla,
) -> (String, bool) {
    let code = code_ranges(source);
    let mut out = String::with_capacity(source.len() + ranges.len() * 32);
    let mut at = 0;
    let mut current_marked = false;
    for (i, r) in ranges.iter().enumerate() {
        if r.start < at || r.end > source.len() || !markable(source, r, &code) {
            continue;
        }
        let is_current = current == Some(i);
        current_marked |= is_current;
        out.push_str(&source[at..r.start]);
        out.push_str("<mark color=\"");
        out.push_str(&hex(if is_current { strong } else { other }));
        out.push_str("\">");
        out.push_str(&source[r.clone()]);
        out.push_str("</mark>");
        at = r.end;
    }
    out.push_str(&source[at..]);
    (out, current_marked)
}

/// `#RRGGBBAA` of a color, for a `<mark color>`.
fn hex(color: Hsla) -> String {
    let c = gpui::Rgba::from(color);
    let b = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
    format!("#{:02X}{:02X}{:02X}{:02X}", b(c.r), b(c.g), b(c.b), b(c.a))
}

// ---------------------------------------------------------------------------
// The find bar

/// The open find bar.
pub(super) struct FindState {
    input: Entity<InputState>,
    /// The query the matches are for.
    query: String,
    /// Every match in the window, oldest row first.
    matches: Vec<FindMatch>,
    current: Option<usize>,
    _subscription: Subscription,
}

/// Find on one row, for `render_row`.
pub(super) struct RowFind {
    /// The matches in the row's shown body, in order.
    pub ranges: Vec<Range<usize>>,
    /// The current match, as an index into `ranges`.
    pub current: Option<usize>,
}

impl TimelineView {
    /// ⌘F: shows the find bar (or brings the focus back to it) with its
    /// text selected.
    pub fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.topic.is_none() {
            return;
        }
        if self.find.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder(t::FIND_PLACEHOLDER));
            let subscription = cx.subscribe_in(
                &input,
                window,
                |this, _, event: &InputEvent, _, cx| match event {
                    InputEvent::Change => this.update_find(cx),
                    InputEvent::PressEnter { shift, .. } => this.step_find(!*shift, cx),
                    _ => {}
                },
            );
            self.find = Some(FindState {
                input,
                query: String::new(),
                matches: Vec::new(),
                current: None,
                _subscription: subscription,
            });
        }
        if let Some(find) = &self.find {
            find.input.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(window, cx);
            });
        }
        cx.notify();
    }

    /// Opens the find bar with `query` typed (self-test scenes).
    pub fn find_query(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open_find(window, cx);
        if let Some(find) = &self.find {
            find.input.update(cx, |input, cx| {
                input.set_value(query.to_string(), window, cx)
            });
        }
        self.update_find(cx);
    }

    /// The find bar is open.
    pub fn find_open(&self) -> bool {
        self.find.is_some()
    }

    /// The matches and the current one (tests).
    pub fn find_matches(&self) -> (Vec<FindMatch>, Option<usize>) {
        self.find
            .as_ref()
            .map(|f| (f.matches.clone(), f.current))
            .unwrap_or_default()
    }

    /// Escape or 完成: closes the bar and clears the marks. The focus goes
    /// back to the thread.
    pub fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(find) = self.find.take() else {
            return;
        };
        self.remeasure_rows(find.matches.iter().map(|m| m.row));
        window.focus(&self.key_focus, cx);
        cx.notify();
    }

    /// The query changed: search again and go to the first match from the
    /// top of the screen.
    fn update_find(&mut self, cx: &mut Context<Self>) {
        let Some(query) = self
            .find
            .as_ref()
            .map(|f| f.input.read(cx).value().to_string())
        else {
            return;
        };
        let old: Vec<usize> = self.find_rows();
        let matches = self.collect_matches(&query);
        let top = self.list.logical_scroll_top().item_ix;
        let current = start_index(&matches, top);
        if let Some(find) = &mut self.find {
            find.query = query;
            find.matches = matches;
            find.current = current;
        }
        self.remeasure_rows(old.into_iter().chain(self.find_rows()));
        self.reveal_find();
        cx.notify();
    }

    /// ⌘G / Return (`forward`) or ⇧⌘G / ⇧Return: the next or previous match.
    pub fn step_find(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(find) = &mut self.find else {
            return;
        };
        let old = find
            .current
            .and_then(|i| find.matches.get(i))
            .map(|m| m.row);
        find.current = step_index(find.current, find.matches.len(), forward);
        let new = find
            .current
            .and_then(|i| find.matches.get(i))
            .map(|m| m.row);
        self.remeasure_rows(old.into_iter().chain(new));
        self.reveal_find();
        cx.notify();
    }

    /// The window changed (`refresh`): search the new window, keeping the
    /// current match where it still is.
    pub(super) fn refresh_find(&mut self) {
        let Some(find) = &self.find else {
            return;
        };
        let anchor = find
            .current
            .and_then(|i| find.matches.get(i))
            .map(FindMatch::anchor);
        let query = find.query.clone();
        let matches = self.collect_matches(&query);
        let current = anchor
            .and_then(|a| matches.iter().position(|m| m.anchor() == a))
            .or_else(|| start_index(&matches, self.list.logical_scroll_top().item_ix));
        if let Some(find) = &mut self.find {
            find.matches = matches;
            find.current = current;
        }
    }

    /// Every match of `query` in the loaded messages, oldest first. Recalled
    /// messages and rows drawn in place of their body (cards, special rows)
    /// are left out: their body text is not on screen.
    fn collect_matches(&self, query: &str) -> Vec<FindMatch> {
        let ctx = extras::ExtrasContext {
            provider: &self.services.provider,
            members: &self.members,
            window_messages: &self.messages,
            me: self.me.as_deref(),
        };
        let mut out = Vec::new();
        for (row, message) in self.messages.iter().enumerate() {
            // v2 special rows (a routing line, a decision record, a done
            // card) draw their own text, not the body.
            if message.deleted_at.is_some()
                || extras::hides_body(message, &ctx)
                || (self.v2 && self.v2_special(message).is_some())
            {
                continue;
            }
            let body = crate::views::refs::chips::shown_body(message, &self.services.provider);
            for range in match_ranges(body, query) {
                out.push(FindMatch {
                    key: model::key(message).to_string(),
                    row,
                    range,
                });
            }
        }
        out
    }

    fn find_rows(&self) -> Vec<usize> {
        let mut rows: Vec<usize> = self
            .find
            .iter()
            .flat_map(|f| f.matches.iter().map(|m| m.row))
            .collect();
        rows.dedup();
        rows
    }

    /// The marks change how a row draws: measure it again.
    fn remeasure_rows(&self, rows: impl IntoIterator<Item = usize>) {
        let len = self.messages.len();
        let mut rows: Vec<usize> = rows.into_iter().filter(|r| *r < len).collect();
        rows.sort_unstable();
        rows.dedup();
        for row in rows {
            self.list.remeasure_items(row..row + 1);
        }
    }

    /// Scrolls the current match's row into view.
    fn reveal_find(&mut self) {
        let Some(row) = self
            .find
            .as_ref()
            .and_then(|f| f.current.and_then(|i| f.matches.get(i)))
            .map(|m| m.row)
        else {
            return;
        };
        // Freeze the tail: the list follows the newest message again once
        // the person scrolls back down.
        self.list.pause_following_tail();
        self.list.scroll_to_reveal_item(row);
    }

    /// The matches on row `key`, for its body marks.
    pub(super) fn row_find(&self, key: &str) -> Option<RowFind> {
        let find = self.find.as_ref()?;
        let mut ranges = Vec::new();
        let mut current = None;
        for (i, m) in find.matches.iter().enumerate() {
            if m.key == key {
                if find.current == Some(i) {
                    current = Some(ranges.len());
                }
                ranges.push(m.range.clone());
            }
        }
        (!ranges.is_empty()).then_some(RowFind { ranges, current })
    }

    /// `body` with the find marks of row `key`, and whether the row holds
    /// the current match that could not be marked in the text (in code, a
    /// link): the row then shows the current match with a band.
    pub(super) fn find_marked_body(
        &self,
        key: &str,
        body: &str,
        cx: &App,
    ) -> Option<(String, bool)> {
        let row = self.row_find(key)?;
        let palette = theme::colors(cx).palette;
        let (marked, current_marked) = mark_matches(
            body,
            &row.ranges,
            row.current,
            crate::ui::native::tokens::find::match_fill(&palette),
            crate::ui::native::tokens::find::current_fill(&palette),
        );
        Some((marked, row.current.is_some() && !current_marked))
    }

    /// The row holds the current match.
    pub(super) fn is_find_current_row(&self, key: &str) -> bool {
        self.row_find(key).is_some_and(|r| r.current.is_some())
    }

    /// The find bar (NATIVE.md §5.6, as in Safari and Notes): under the
    /// toolbar row, 40 tall with a hairline under it; the search field, the
    /// count, the previous and next match and 完成 on one center line.
    pub(super) fn render_find_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use crate::ui::native::{
            IconSize, NativeType as _, PushSize, PushStyle, SearchField, disabled_push,
            icon_button_sym, native_colors, push_button, tokens,
        };
        let find = self.find.as_ref()?;
        let n = native_colors(cx);
        let total = find.matches.len();
        let count = if find.query.is_empty() {
            None
        } else if total == 0 {
            Some(t::FIND_NOT_FOUND.to_string())
        } else {
            Some(fill(
                t::FIND_POSITION,
                &[
                    ("n", &find.current.map_or(0, |i| i + 1).to_string()),
                    ("total", &total.to_string()),
                ],
            ))
        };
        let leading = if self.v2 {
            v2::LEADING
        } else {
            theme::CONTENT_GUTTER
        };
        let step = |id: &'static str, sym: Sym, name: &'static str, cx: &mut Context<Self>| {
            disabled_push(
                icon_button_sym(id, sym, name, IconSize::Regular, cx),
                total == 0,
                cx,
            )
        };
        Some(
            div()
                .id("find-bar")
                .flex_none()
                .h(tokens::height::FOOTER_ROW)
                .pl(leading)
                .pr(v2::SIDE)
                .border_b_1()
                .border_color(n.separator)
                .bg(n.content)
                .flex()
                .items_center()
                .gap(space::S8)
                // Escape in the bar closes it (before the field takes it).
                .capture_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, window, cx| {
                    if e.keystroke.key == "escape" && !e.keystroke.modifiers.modified() {
                        this.close_find(window, cx);
                        cx.stop_propagation();
                    }
                }))
                .child(
                    SearchField::new("find-field", &find.input)
                        .width(tokens::size::FIND_FIELD_WIDTH),
                )
                .children(count.map(|count| {
                    div()
                        .flex_none()
                        .n_body()
                        .tnum()
                        .text_color(n.secondary)
                        .child(count)
                }))
                .child(div().flex_1())
                .child(
                    step("find-previous", Sym::ChevronUp, t::FIND_PREVIOUS, cx)
                        .on_click(cx.listener(|this, _, _, cx| this.step_find(false, cx))),
                )
                .child(
                    step("find-next", Sym::ChevronDown, t::FIND_NEXT, cx)
                        .on_click(cx.listener(|this, _, _, cx| this.step_find(true, cx))),
                )
                .child(
                    push_button(
                        "find-done",
                        t::FIND_DONE,
                        PushStyle::Outline,
                        PushSize::Regular,
                        cx,
                    )
                    .on_click(cx.listener(|this, _, window, cx| this.close_find(window, cx))),
                )
                .into_any_element(),
        )
    }
}

/// The find-bar scene: the 话题 page of 开发 / 断线重连后消息重复 with 重连
/// typed in its find bar.
pub fn open_scene(
    services: crate::app::Services,
    content: gpui::Size<gpui::Pixels>,
    cx: &mut App,
) -> Result<gpui::AnyWindowHandle, String> {
    let handle = crate::selftest::scenes::open_main(services, content, cx)?;
    handle
        .update(cx, |_, window, cx| {
            if let Some(timeline) = timeline_of(window, cx) {
                timeline.update(cx, |this, cx| this.find_query(SCENE_QUERY, window, cx));
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(handle)
}

/// What the find-bar scene types.
const SCENE_QUERY: &str = "重连";

#[cfg(test)]
mod tests {
    use super::*;

    fn texts<'a>(text: &'a str, query: &str) -> Vec<&'a str> {
        match_ranges(text, query)
            .into_iter()
            .map(|r| &text[r])
            .collect()
    }

    #[test]
    fn matches_ignore_case() {
        assert_eq!(texts("Seq and SEQ and seq", "seq"), ["Seq", "SEQ", "seq"]);
        assert_eq!(texts("Straße STRASSE", "STRAßE"), ["Straße"]);
        assert_eq!(texts("ΣΊΣΥΦΟΣ", "σίσυφοσ").len(), 1);
    }

    #[test]
    fn matches_cjk_on_char_boundaries() {
        let text = "断线重连后消息重复，重连之后同一条消息会出现两次";
        let found = match_ranges(text, "重连");
        assert_eq!(found.len(), 2);
        for r in &found {
            assert_eq!(&text[r.clone()], "重连");
            assert!(text.is_char_boundary(r.start) && text.is_char_boundary(r.end));
        }
        assert_eq!(texts("全角ＡＢＣ和abc", "ａｂｃ"), ["ＡＢＣ"]);
        assert_eq!(texts("한국어 검색 테스트", "검색"), ["검색"]);
        assert_eq!(texts("emoji 🙂 then 🙂", "🙂"), ["🙂", "🙂"]);
    }

    #[test]
    fn a_char_that_lowercases_to_two_maps_back_whole() {
        // İ lowercases to "i̇" (two chars): a match on its first part still
        // covers the whole source char.
        let text = "İstanbul";
        let found = match_ranges(text, "i");
        assert_eq!(found, vec![0..2]);
        assert_eq!(texts(text, "stan"), ["stan"]);
    }

    #[test]
    fn empty_or_absent_queries_match_nothing() {
        assert!(match_ranges("anything", "").is_empty());
        assert!(match_ranges("", "a").is_empty());
        assert!(match_ranges("abc", "abcd").is_empty());
        // Matches do not overlap.
        assert_eq!(match_ranges("aaaa", "aa"), vec![0..2, 2..4]);
    }

    #[test]
    fn steps_wrap_around() {
        assert_eq!(step_index(None, 0, true), None);
        assert_eq!(step_index(None, 3, true), Some(0));
        assert_eq!(step_index(None, 3, false), Some(2));
        assert_eq!(step_index(Some(2), 3, true), Some(0));
        assert_eq!(step_index(Some(0), 3, false), Some(2));
        assert_eq!(step_index(Some(1), 3, true), Some(2));
    }

    #[test]
    fn a_search_starts_at_the_first_match_on_screen() {
        let m = |row| FindMatch {
            key: format!("k{row}"),
            row,
            range: 0..1,
        };
        let matches = [m(2), m(5), m(5), m(9)];
        assert_eq!(start_index(&matches, 0), Some(0));
        assert_eq!(start_index(&matches, 5), Some(1));
        assert_eq!(start_index(&matches, 6), Some(3));
        // Everything is above the screen: the newest match.
        assert_eq!(start_index(&matches, 20), Some(3));
        assert_eq!(start_index(&[], 0), None);
    }

    fn marks(source: &str, query: &str, current: Option<usize>) -> (String, bool) {
        let other: Hsla = gpui::rgba(0x0000001A).into();
        let strong: Hsla = gpui::rgba(0x00000047).into();
        mark_matches(source, &match_ranges(source, query), current, other, strong)
    }

    #[test]
    fn marks_wrap_matches_and_the_current_one_is_stronger() {
        let (out, current) = marks("重连之后，再重连一次", "重连", Some(1));
        assert_eq!(
            out,
            "<mark color=\"#0000001A\">重连</mark>之后，再<mark color=\"#00000047\">重连</mark>一次"
        );
        assert!(current);
        let (out, current) = marks("Seq seq", "SEQ", None);
        assert_eq!(
            out,
            "<mark color=\"#0000001A\">Seq</mark> <mark color=\"#0000001A\">seq</mark>"
        );
        assert!(!current);
    }

    #[test]
    fn code_links_and_mentions_stay_unmarked() {
        let src = "seq in `seq` and\n```\nseq\n```\n[seq](https://x/seq) @seq";
        let (out, current) = marks(src, "seq", Some(1));
        // Only the first, plain match is marked; the current one is in code.
        assert_eq!(out.matches("<mark").count(), 1);
        assert!(out.starts_with("<mark color=\"#0000001A\">seq</mark> in `seq`"));
        assert!(!current);
        // A match at the start of a list or heading line keeps its marker.
        let (out, _) = marks("1. one\n# two", "1.", None);
        assert!(!out.contains("<mark"));
        // Text across Markdown syntax is not wrapped.
        let (out, _) = marks("a **bold** b", "a **b", None);
        assert!(!out.contains("<mark"));
    }

    /// ⌘F in the sample 话题: 重连 is found in the loaded messages, Return
    /// steps and wraps, 完成 closes the bar and clears the matches.
    #[gpui::test]
    fn find_in_the_open_topic(cx: &mut gpui::TestAppContext) {
        use crate::{
            app::Services,
            data::ChatProvider,
            fake::v2::{DEV, T_RECONNECT, V2Samples},
        };
        cx.update(gpui_component::init);
        let provider: std::sync::Arc<dyn ChatProvider> = V2Samples::new();
        let (view, cx) = cx.add_window_view(move |_, cx| {
            let services = Services::new(provider, cx);
            services.nav.update(cx, |nav, cx| {
                nav.open_topic(DEV.into(), T_RECONNECT.into(), cx)
            });
            TimelineView::new(services, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |this, window, cx| this.find_query("重连", window, cx));
        let (matches, current) = view.update(cx, |this, _| this.find_matches());
        assert!(matches.len() >= 2, "重连 is in several messages");
        assert!(current.is_some());
        let len = matches.len();
        view.update(cx, |this, cx| {
            for _ in 0..len {
                this.step_find(true, cx);
            }
        });
        assert_eq!(view.update(cx, |this, _| this.find_matches().1), current);
        view.update_in(cx, |this, window, cx| {
            this.find_query("RECONNECT-不存在", window, cx)
        });
        assert!(view.update(cx, |this, _| this.find_matches().0.is_empty()));
        view.update_in(cx, |this, window, cx| this.close_find(window, cx));
        assert!(!view.update(cx, |this, _| this.find_open()));
    }

    #[test]
    fn marks_are_hex_with_alpha() {
        let c: Hsla = gpui::rgba(0x0A0A0A47).into();
        assert_eq!(hex(c), "#0A0A0A47");
    }
}
