//! The result list of the search dialog: a gpui-component list delegate that
//! asks the provider as the user types (debounced) and renders each hit.

use std::{collections::HashMap, ops::Range, time::Duration};

use gpui::{
    App, Context, FontWeight, HighlightStyle, IntoElement, ParentElement, Styled, StyledText, Task,
    Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    IndexPath, Sizable as _, WindowExt as _,
    button::Button,
    list::{ListDelegate, ListItem, ListState},
};

use crate::{
    app::Services,
    copy::{dev, fill, search as s},
    data::{GroupId, Member, SearchError, SearchHit, SearchQuery},
    theme::{self, Type as _, space},
    ui::{self, Lucide, MemberAvatar},
    views::time_text,
};

/// Wait this long after the last key before asking the server.
const DEBOUNCE: Duration = Duration::from_millis(180);
/// Hits per search (the protocol allows 1..50).
const LIMIT: u32 = 50;
/// Longest query the protocol accepts, in characters.
const MAX_QUERY: usize = 200;
/// Row content height; the list item adds its padding (40 pt rows, §13.12).
const ROW_HEIGHT: f32 = 28.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// No query yet.
    Idle,
    Searching,
    Done,
    Failed(SearchError),
}

pub struct SearchDelegate {
    services: Services,
    /// The current 群 (for Cmd+F), if there is one.
    group: Option<GroupId>,
    everywhere: bool,
    query: String,
    hits: Vec<SearchHit>,
    phase: Phase,
    selected: Option<usize>,
    /// Bumped on every new search; older answers are dropped.
    generation: u64,
    group_names: HashMap<GroupId, String>,
    members: Vec<Member>,
    task: Task<()>,
}

/// Trims the query and cuts it to what the protocol accepts.
pub fn clip_query(text: &str) -> String {
    text.trim().chars().take(MAX_QUERY).collect()
}

/// Turns code-point ranges of `text` into sorted, merged byte ranges on
/// char boundaries. Ranges outside the text are cut or dropped.
pub fn byte_ranges(text: &str, ranges: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut offsets: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
    let chars = offsets.len();
    offsets.push(text.len());
    let mut sorted: Vec<Range<usize>> = ranges
        .iter()
        .map(|r| r.start.min(chars)..r.end.min(chars))
        .filter(|r| r.start < r.end)
        .collect();
    sorted.sort_by_key(|r| r.start);
    let mut merged: Vec<Range<usize>> = Vec::new();
    for r in sorted {
        match merged.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => merged.push(r),
        }
    }
    merged
        .into_iter()
        .map(|r| offsets[r.start]..offsets[r.end])
        .collect()
}

impl SearchDelegate {
    pub fn new(services: Services, group: Option<GroupId>, everywhere: bool, _cx: &App) -> Self {
        let provider = services.provider.clone();
        let group_names = provider
            .groups()
            .into_iter()
            .map(|g| (g.id, g.name))
            .collect();
        Self {
            everywhere: everywhere || group.is_none(),
            group,
            query: String::new(),
            hits: Vec::new(),
            phase: Phase::Idle,
            selected: None,
            generation: 0,
            group_names,
            members: provider.members(),
            services,
            task: Task::ready(()),
        }
    }

    pub fn group(&self) -> Option<&GroupId> {
        self.group.as_ref()
    }

    pub fn everywhere(&self) -> bool {
        self.everywhere
    }

    pub fn set_everywhere(&mut self, everywhere: bool) {
        self.everywhere = everywhere || self.group.is_none();
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn hits(&self) -> &[SearchHit] {
        &self.hits
    }

    /// True while a search waits for its answer.
    pub fn searching(&self) -> bool {
        self.phase == Phase::Searching
    }

    /// The line under the list: the result count.
    pub fn footer_text(&self) -> String {
        match self.phase {
            Phase::Done => fill(s::RESULT_COUNT, &[("n", &self.hits.len().to_string())]),
            Phase::Searching => s::SEARCHING.to_string(),
            _ => String::new(),
        }
    }

    fn request(&self) -> SearchQuery {
        SearchQuery {
            text: self.query.clone(),
            group_id: if self.everywhere {
                None
            } else {
                self.group.clone()
            },
            limit: LIMIT,
        }
    }

    /// Starts the search for the current query and scope.
    fn run(&mut self, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        self.generation += 1;
        let generation = self.generation;
        if self.query.is_empty() {
            self.hits.clear();
            self.phase = Phase::Idle;
            self.task = Task::ready(());
            cx.notify();
            return;
        }
        self.phase = Phase::Searching;
        let request = self.request();
        let provider = self.services.provider.clone();
        self.task = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(DEBOUNCE).await;
            let reply = provider.search(request);
            let result = reply.await.unwrap_or(Err(SearchError::Unavailable));
            let _ = this.update_in(cx, |state, window, cx| {
                let d = state.delegate_mut();
                if d.generation != generation {
                    return;
                }
                match result {
                    Ok(hits) => {
                        d.hits = hits;
                        d.phase = Phase::Done;
                    }
                    Err(error) => {
                        d.hits.clear();
                        d.phase = Phase::Failed(error);
                    }
                }
                let first = (!state.delegate().hits.is_empty()).then(IndexPath::default);
                state.set_selected_index(first, window, cx);
                cx.notify();
            });
        });
        cx.notify();
    }

    fn member(&self, id: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.id == id)
    }

    fn topic_title(&self, hit: &SearchHit) -> String {
        self.services
            .provider
            .topic(&hit.group_id, &hit.topic_id)
            .map(|t| t.title)
            .unwrap_or_else(|| s::UNKNOWN_TOPIC.to_string())
    }

    fn open_hit(&self, hit: &SearchHit, window: &mut Window, cx: &mut App) {
        window.close_dialog(cx);
        let hit = hit.clone();
        self.services.nav.update(cx, |nav, cx| {
            nav.open_message(hit.group_id, hit.topic_id, hit.message_id, hit.seq, cx)
        });
    }
}

impl ListDelegate for SearchDelegate {
    type Item = ListItem;

    fn perform_search(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.query = clip_query(query);
        self.run(window, cx);
        Task::ready(())
    }

    fn items_count(&self, _section: usize, _cx: &App) -> usize {
        self.hits.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let hit = self.hits.get(ix.row)?.clone();
        let colors = theme::colors(cx);
        let developer = self.services.nav.read(cx).developer_view;
        let group = self
            .group_names
            .get(&hit.group_id)
            .cloned()
            .unwrap_or_else(|| s::UNKNOWN_GROUP.to_string());
        let topic = self.topic_title(&hit);
        let author = self.member(&hit.author_id).cloned();
        let snippet = hit.snippet.replace(['\n', '\r'], " ");
        let style = HighlightStyle {
            font_weight: Some(FontWeight::SEMIBOLD),
            ..Default::default()
        };
        let highlights: Vec<_> = byte_ranges(&snippet, &hit.highlights)
            .into_iter()
            .map(|r| (r, style))
            .collect();
        // One 40 pt row as in the switcher's 消息 group: 24 pt avatar,
        // "作者：片段" with the match in 600, and "群 · 话题 · 时间" trailing.
        let selected = self.selected == Some(ix.row);
        let name = author.as_ref().map(|m| m.name.clone()).unwrap_or_default();
        let mut place = format!("{group} · {topic} · {}", time_text(hit.created_at));
        if developer {
            place.push(' ');
            place.push_str(&fill(
                dev::SEARCH_HIT,
                &[("id", &hit.message_id), ("seq", &hit.seq.to_string())],
            ));
        }
        let trailing = div()
            .flex_none()
            .max_w(px(260.))
            .t_callout()
            .tnum()
            .text_color(colors.text_2)
            .one_line()
            .child(place);
        let line = div()
            .h(px(ROW_HEIGHT))
            .w_full()
            .flex()
            .items_center()
            .gap(space::S10)
            .t_ui()
            .child(
                div()
                    .flex_none()
                    .child(MemberAvatar::new(author.as_ref(), px(24.)).on(colors.elevated)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(120.))
                            .one_line()
                            .text_color(colors.text_1)
                            .child(fill(s::AUTHOR_SAID, &[("name", &name)])),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .one_line()
                            .text_color(colors.text_1)
                            .child(StyledText::new(snippet).with_highlights(highlights)),
                    ),
            )
            .child(trailing)
            .when(selected, |el| el.child(ui::KbdHint::new(&["enter"])));
        Some(
            ListItem::new(ix.row)
                .rounded(theme::radius::SM)
                .when(selected, |el| el.bg(colors.fill_active))
                .child(line),
        )
    }

    fn render_empty(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        // §13.15: the search empty state says where it looked and offers all
        // 群; the idle and failed states use the same layout.
        let state = match self.phase {
            Phase::Idle => ui::EmptyState::new(Lucide::Search, s::HINT),
            Phase::Searching => ui::EmptyState::new(Lucide::Search, s::SEARCHING),
            Phase::Done => {
                let title = fill(s::EMPTY_TITLE, &[("q", &self.query)]);
                let state = ui::EmptyState::new(Lucide::SearchX, title);
                match (self.everywhere, self.group.as_ref()) {
                    (false, Some(g)) => {
                        let name = self
                            .group_names
                            .get(g)
                            .cloned()
                            .unwrap_or_else(|| s::UNKNOWN_GROUP.to_string());
                        state
                            .description(fill(s::EMPTY_IN_GROUP, &[("group", &name)]))
                            .action(
                                Button::new("search-all")
                                    .small()
                                    .label(s::SEARCH_ALL)
                                    .shadow(theme::colors(cx).shadow(theme::Elevation::Control))
                                    .on_click(cx.listener(|state, _, window, cx| {
                                        state.delegate_mut().set_everywhere(true);
                                        let query = state.delegate().query().to_string();
                                        state.set_query(&query, window, cx);
                                    })),
                            )
                    }
                    _ => state,
                }
            }
            Phase::Failed(e) => ui::EmptyState::new(
                Lucide::CircleAlert,
                match e {
                    SearchError::Offline => s::OFFLINE,
                    SearchError::Refused => s::REFUSED,
                    SearchError::TimedOut => s::TIMED_OUT,
                    SearchError::Unavailable => s::UNAVAILABLE,
                },
            ),
        };
        div().size_full().child(state)
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        self.selected = ix.map(|ix| ix.row);
        cx.notify();
    }

    fn confirm(
        &mut self,
        _secondary: bool,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        if let Some(hit) = self.selected.and_then(|i| self.hits.get(i)).cloned() {
            self.open_hit(&hit, window, cx);
        }
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        window.close_dialog(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_point_ranges_become_byte_ranges() {
        // "中文搜索 ok": each Chinese char is 3 bytes.
        let text = "中文搜索 ok";
        assert_eq!(byte_ranges(text, &[2..4]), vec![6..12]);
        assert_eq!(byte_ranges(text, &[5..7]), vec![13..15]);
        // Overlaps merge, order does not matter, out of range is cut.
        assert_eq!(byte_ranges(text, &[3..4, 0..2, 1..3]), vec![0..12]);
        assert_eq!(byte_ranges(text, &[6..100]), vec![14..15]);
        assert!(byte_ranges(text, &[20..30, 3..3]).is_empty());
        assert!(byte_ranges("", &[0..1]).is_empty());
        // Emoji is one code point of four bytes.
        assert_eq!(byte_ranges("a😀b", &[1..2]), vec![1..5]);
    }

    #[test]
    fn query_is_trimmed_and_clipped() {
        assert_eq!(clip_query("  登录  "), "登录");
        let long: String = "字".repeat(300);
        assert_eq!(clip_query(&long).chars().count(), 200);
    }
}
