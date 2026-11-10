//! 沉淀 (docs/design/agent-harness.zh.md §7.7): 记得的事 on an AI member's page and
//! 设置 › 团队约定. One board per scope: at the top the newest day's 记忆更新 (新增, 改了,
//! 停用, 可能过时), each line with 撤回; then the memories grouped by kind, each with
//! when it applies, its evidence, where it came from and "⋯" (编辑, 停用); then what was
//! 停用, folded, with 恢复. Nothing asks before it is kept: people correct afterwards.
//! Writers (owners; an AI member for its own) change it, everyone else reads.

pub mod model;
pub mod scenes;
pub mod sheet;

use std::collections::HashSet;

use gpui::{
    AnyElement, App, Context, ElementId, InteractiveElement as _, IntoElement, ParentElement,
    Render, SharedString, Styled, Subscription, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::{
    Disableable as _,
    menu::{DropdownMenu as _, PopupMenuItem},
};

use crate::{
    app::{Changes, Services},
    copy::{fill, memory as c},
    data::{
        AdminError, Change, SessionState,
        memory::{Memory, MemoryDay, MemoryScope, MemoryStatus},
    },
    ui::{
        native::{
            GroupedForm, NativeType as _, PushSize, PushStyle, SectionTitle, metrics,
            native_colors,
            page::{self, AlertKind, SkeletonKind},
            push_button, tokens,
        },
        sf_symbols::Sym,
        truncate::TruncText,
    },
    views::settings::widgets,
};

/// Where the board is drawn: a card on the 成员 page, or sections of the settings tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Host {
    Page,
    Settings,
}

pub struct MemoryBoard {
    services: Services,
    scope: MemoryScope,
    host: Host,
    /// The AI member's name (`MemoryScope::Member`).
    name: String,
    memories: Option<Result<Vec<Memory>, AdminError>>,
    day: Option<Result<MemoryDay, AdminError>>,
    /// Bumped per load, so an older answer never lands over a newer one.
    generation: u64,
    loading: bool,
    /// Memories and lines with a request in flight.
    busy: HashSet<String>,
    /// The last 停用, 恢复 or 撤回 the server refused: its title and why.
    failure: Option<(&'static str, AdminError)>,
    show_retired: bool,
    _subscriptions: Vec<Subscription>,
}

impl MemoryBoard {
    pub fn new(
        services: Services,
        scope: MemoryScope,
        host: Host,
        name: String,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = vec![cx.subscribe(&services.hub, |_, _, changes: &Changes, cx| {
            // Who may write follows my level.
            if changes.any(|c| matches!(c, Change::Members | Change::Session)) {
                cx.notify();
            }
        })];
        let mut this = Self {
            services,
            scope,
            host,
            name,
            memories: None,
            day: None,
            generation: 0,
            loading: false,
            busy: HashSet::new(),
            failure: None,
            show_retired: false,
            _subscriptions: subscriptions,
        };
        this.load(cx);
        this
    }

    pub fn scope(&self) -> &MemoryScope {
        &self.scope
    }

    /// The first answer arrived (or failed).
    pub fn loaded(&self) -> bool {
        self.memories.is_some()
    }

    /// Reads the memories and the newest day's 记忆更新 again; what is shown stays until
    /// the answers arrive.
    pub fn load(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        self.loading = true;
        let p = &self.services.provider;
        let memories = p.memories(&self.scope);
        let day = p.memory_updates(&self.scope);
        cx.spawn(async move |this, cx| {
            let memories = memories.await.unwrap_or(Err(AdminError::Other));
            let day = day.await.unwrap_or(Err(AdminError::Other));
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.loading = false;
                this.memories = Some(memories);
                this.day = Some(day);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn writer(&self) -> bool {
        let me = match self.services.provider.session() {
            SessionState::SignedIn { me, .. } => Some(me),
            _ => None,
        };
        model::can_write(&self.scope, me.as_ref())
    }

    fn name_of(&self, id: &str) -> String {
        self.services
            .provider
            .member(id)
            .map(|m| crate::ui::avatar::name_beside_avatar(&m.name).to_string())
            .unwrap_or_else(|| {
                if self.name.is_empty() {
                    c::SOMEONE.to_string()
                } else {
                    self.name.clone()
                }
            })
    }

    /// 停用 or 恢复.
    fn set_status(&mut self, id: String, status: MemoryStatus, cx: &mut Context<Self>) {
        if !self.busy.insert(id.clone()) {
            return;
        }
        self.failure = None;
        let reply = self.services.provider.update_memory(
            &id,
            crate::data::memory::MemoryEdit {
                status: Some(status),
                ..Default::default()
            },
        );
        cx.spawn(async move |this, cx| {
            let result = reply.await.unwrap_or(Err(AdminError::Other));
            let _ = this.update(cx, |this, cx| {
                this.busy.remove(&id);
                match result {
                    Ok(_) => this.load(cx),
                    Err(e) => this.failure = Some((c::ACTION_FAILED, e)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// 撤回 one line of the day's 记忆更新.
    fn undo(&mut self, id: String, cx: &mut Context<Self>) {
        if !self.busy.insert(id.clone()) {
            return;
        }
        self.failure = None;
        let reply = self.services.provider.undo_memory_update(&id);
        cx.spawn(async move |this, cx| {
            let result = reply.await.unwrap_or(Err(AdminError::Other));
            let _ = this.update(cx, |this, cx| {
                this.busy.remove(&id);
                match result {
                    Ok(_) => this.load(cx),
                    // Someone undid it first: show the day as it is now.
                    Err(AdminError::Conflict) => {
                        this.failure = Some((c::UNDO_FAILED, AdminError::Conflict));
                        this.load(cx);
                    }
                    Err(e) => this.failure = Some((c::UNDO_FAILED, e)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn open_sheet(&self, original: Option<Memory>, window: &mut Window, cx: &mut Context<Self>) {
        let target = sheet::Target {
            services: self.services.clone(),
            board: cx.entity().downgrade(),
            scope: self.scope.clone(),
            host: self.host,
            member_name: self.name.clone(),
        };
        sheet::open(target, original, window, cx);
    }

    /// Opens 记一条, or 编辑 of the newest memory in use (screenshot scenes).
    pub fn open_for_scene(&self, edit: bool, window: &mut Window, cx: &mut Context<Self>) {
        let original = edit
            .then(|| match &self.memories {
                Some(Ok(ms)) => ms
                    .iter()
                    .find(|m| m.status == MemoryStatus::Active)
                    .cloned(),
                _ => None,
            })
            .flatten();
        self.open_sheet(original, window, cx);
    }

    // --- pieces ---

    fn about(&self, writer: bool) -> SharedString {
        match (&self.scope, writer) {
            (MemoryScope::Team, true) => c::TEAM_ABOUT.into(),
            (MemoryScope::Team, false) => c::TEAM_ABOUT_READONLY.into(),
            (MemoryScope::Member(_), true) => fill(c::MEMORY_ABOUT, &[("name", &self.name)]).into(),
            (MemoryScope::Member(_), false) => {
                fill(c::MEMORY_ABOUT_READONLY, &[("name", &self.name)]).into()
            }
        }
    }

    fn add_button(&self, cx: &mut Context<Self>) -> AnyElement {
        push_button(
            self.id("add"),
            c::ADD,
            if self.host == Host::Settings {
                PushStyle::Default
            } else {
                PushStyle::Outline
            },
            PushSize::Small,
            cx,
        )
        .on_click(cx.listener(|this, _, window, cx| this.open_sheet(None, window, cx)))
        .into_any_element()
    }

    fn id(&self, part: impl std::fmt::Display) -> ElementId {
        let host = match self.host {
            Host::Page => "member-memory",
            Host::Settings => "team-memory",
        };
        ElementId::Name(format!("{host}-{part}").into())
    }

    /// The rows of one list: inside the card (lines between rows), or a settings group.
    fn frame(&self, rows: Vec<AnyElement>, cx: &App) -> AnyElement {
        match self.host {
            Host::Settings => GroupedForm::new().rows(rows).into_any_element(),
            Host::Page => {
                let k = native_colors(cx);
                let n = rows.len();
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .children(rows.into_iter().enumerate().map(|(i, r)| {
                        div()
                            .w_full()
                            .when(i + 1 < n, |d| d.border_b_1().border_color(k.separator))
                            .child(r)
                    }))
                    .into_any_element()
            }
        }
    }

    /// A list's label (§2.3 group label): 12 medium muted, a count, and text at the right.
    fn label(
        &self,
        title: impl Into<SharedString>,
        count: Option<usize>,
        trailing: Option<AnyElement>,
        cx: &App,
    ) -> AnyElement {
        let k = native_colors(cx);
        div()
            .w_full()
            .h(tokens::height::ROW)
            .when(self.host == Host::Settings, |d| {
                d.px(metrics::FORM_ROW_INSET)
            })
            .flex()
            .items_center()
            .gap(tokens::space::S8)
            .child(
                div()
                    .flex_none()
                    .n_section()
                    .text_color(k.secondary)
                    .child(title.into()),
            )
            .children(count.map(|n| page::count_pill(n, cx)))
            .child(div().flex_1())
            .children(trailing)
            .into_any_element()
    }

    fn row_pad(&self) -> gpui::Pixels {
        match self.host {
            Host::Settings => metrics::FORM_ROW_INSET,
            Host::Page => gpui::Pixels::ZERO,
        }
    }

    /// One 记忆更新 line: the change as a badge, the memory's text and what it said before
    /// (or why), then 撤回 or 已撤回.
    fn update_row(&self, line: model::UpdateLine, cx: &mut Context<Self>) -> AnyElement {
        let k = native_colors(cx);
        let busy = self.busy.contains(&line.id);
        let id = line.id.clone();
        let trailing: Option<AnyElement> = if line.undone {
            Some(
                div()
                    .n_subheadline()
                    .text_color(k.secondary)
                    .child(c::UNDONE)
                    .into_any_element(),
            )
        } else if line.can_undo {
            Some(
                push_button(
                    self.id(format!("undo-{}", line.id)),
                    c::UNDO,
                    PushStyle::Outline,
                    PushSize::Small,
                    cx,
                )
                .loading(busy)
                .disabled(busy)
                .on_click(cx.listener(move |this, _, _, cx| this.undo(id.clone(), cx)))
                .into_any_element(),
            )
        } else {
            None
        };
        div()
            .id(self.id(format!("line-{}", line.id)))
            .w_full()
            .px(self.row_pad())
            .py(tokens::space::S12)
            .flex()
            .items_start()
            .gap(tokens::space::S12)
            .child(
                div()
                    .flex_none()
                    .w(BADGE_COLUMN)
                    .flex()
                    .child(page::outline_badge(line.word, cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(gpui::Pixels::ZERO)
                    .flex()
                    .flex_col()
                    .gap(tokens::space::S4)
                    .child(
                        div()
                            .n_body()
                            .text_color(if line.undone { k.secondary } else { k.label })
                            .when(line.undone, |d| d.line_through())
                            .overflow_hidden()
                            .text_ellipsis()
                            .line_clamp(3)
                            .child(line.text.clone()),
                    )
                    .children(line.detail.clone().map(|d| {
                        TruncText::new(d)
                            .id(self.id(format!("line-detail-{}", line.id)))
                            .n_subheadline()
                            .text_color(k.secondary)
                    })),
            )
            .children(trailing.map(|t| div().flex_none().child(t)))
            .into_any_element()
    }

    /// One memory: its text (with 明天生效 while it waits for the night's 整理), when it
    /// applies, the evidence, where it came from and when, and "⋯".
    fn memory_row(
        &self,
        m: &Memory,
        writer: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = native_colors(cx);
        let retired = m.status == MemoryStatus::Retired;
        let now = self.services.provider.clock_now();
        let name = |id: &str| self.name_of(id);
        let meta = fill(
            c::META,
            &[
                ("source", &model::source_text(m, &name)),
                (
                    "time",
                    &crate::views::group_page::kit::day_and_time(now, m.updated_at, false),
                ),
            ],
        );
        let line = |key: &str, text: String| {
            TruncText::new(text)
                .id(self.id(format!("{key}-{}", m.id)))
                .n_subheadline()
                .text_color(k.secondary)
        };
        let menu = writer.then(|| {
            let busy = self.busy.contains(&m.id);
            let this = cx.entity().downgrade();
            let item = m.clone();
            widgets::more_button(self.id(format!("more-{}", m.id)), window, cx)
                .loading(busy)
                .disabled(busy)
                .dropdown_menu(move |menu, _, _| {
                    let (edit_this, edit_item) = (this.clone(), item.clone());
                    let menu = if item.status == MemoryStatus::Retired {
                        menu
                    } else {
                        menu.item(PopupMenuItem::new(c::EDIT).on_click(move |_, window, cx| {
                            let item = edit_item.clone();
                            let _ =
                                edit_this.update(cx, |b, cx| b.open_sheet(Some(item), window, cx));
                        }))
                    };
                    let (this, id) = (this.clone(), item.id.clone());
                    let (label, status) = if item.status == MemoryStatus::Retired {
                        (c::RESTORE, MemoryStatus::Active)
                    } else {
                        (c::RETIRE, MemoryStatus::Retired)
                    };
                    menu.item(PopupMenuItem::new(label).on_click(move |_, _, cx| {
                        let id = id.clone();
                        let _ = this.update(cx, |b, cx| b.set_status(id, status, cx));
                    }))
                })
                .into_any_element()
        });
        div()
            .id(self.id(format!("item-{}", m.id)))
            .w_full()
            .px(self.row_pad())
            .py(tokens::space::S12)
            .flex()
            .items_start()
            .gap(tokens::space::S12)
            .child(
                div()
                    .flex_1()
                    .min_w(gpui::Pixels::ZERO)
                    .flex()
                    .flex_col()
                    .gap(tokens::space::S4)
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .gap(tokens::space::S8)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(gpui::Pixels::ZERO)
                                    .n_body()
                                    .text_color(if retired { k.secondary } else { k.label })
                                    .child(m.text.clone()),
                            )
                            .when(m.status == MemoryStatus::Pending, |d| {
                                d.child(div().flex_none().child(page::status_badge(
                                    page::Status::Queued,
                                    c::PENDING,
                                    cx,
                                )))
                            }),
                    )
                    .children(
                        m.when
                            .as_ref()
                            .map(|w| line("when", fill(c::WHEN, &[("when", w)]))),
                    )
                    .children(
                        m.note
                            .as_ref()
                            .map(|n| line("note", fill(c::NOTE, &[("note", n)]))),
                    )
                    .child(line("meta", meta)),
            )
            .children(menu.map(|m| div().flex_none().child(m)))
            .into_any_element()
    }

    /// 记忆更新 of the newest day: its label with the summary, then the lines.
    fn updates_block(
        &self,
        day: &MemoryDay,
        writer: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let k = native_colors(cx);
        let label = match &day.day {
            Some(d) => fill(
                c::UPDATES_TITLE_DAY,
                &[(
                    "day",
                    &model::day_label(d, crate::selftest::clock::local_now().date_naive()),
                )],
            ),
            None => c::UPDATES_TITLE.to_string(),
        };
        let body = if day.updates.is_empty() {
            // Settings shows its own empty line; the member card stays quiet.
            if self.host == Host::Page {
                return None;
            }
            widgets::list_empty(Sym::Book, c::UPDATES_EMPTY, None, cx).into_any_element()
        } else {
            let rows = day
                .updates
                .iter()
                .map(|u| self.update_row(model::update_line(u, writer), cx))
                .collect();
            self.frame(rows, cx)
        };
        let summary = model::summary(day).map(|s| {
            div()
                .n_subheadline()
                .text_color(k.secondary)
                .child(s)
                .into_any_element()
        });
        Some(
            div()
                .w_full()
                .flex()
                .flex_col()
                .child(self.label(label, None, summary, cx))
                .child(body)
                .into_any_element(),
        )
    }

    /// The kind groups, then 已停用 (folded).
    fn lists_block(
        &self,
        memories: &[Memory],
        writer: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let lists = model::lists(&self.scope, memories);
        let mut out: Vec<AnyElement> = Vec::new();
        for g in &lists.groups {
            let rows = g
                .items
                .iter()
                .map(|m| self.memory_row(m, writer, window, cx))
                .collect();
            out.push(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .child(self.label(g.title, Some(g.items.len()), None, cx))
                    .child(self.frame(rows, cx))
                    .into_any_element(),
            );
        }
        if !lists.retired.is_empty() {
            let open = self.show_retired;
            let toggle = push_button(
                self.id("retired-toggle"),
                if open {
                    c::HIDE_RETIRED
                } else {
                    c::SHOW_RETIRED
                },
                PushStyle::Ghost,
                PushSize::Small,
                cx,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.show_retired = !this.show_retired;
                cx.notify();
            }))
            .into_any_element();
            let mut block = div().w_full().flex().flex_col().child(self.label(
                fill(c::RETIRED_N, &[("n", &lists.retired.len().to_string())]),
                None,
                Some(toggle),
                cx,
            ));
            if open {
                let rows = lists
                    .retired
                    .iter()
                    .map(|m| self.memory_row(m, writer, window, cx))
                    .collect();
                block = block.child(self.frame(rows, cx));
            }
            out.push(block.into_any_element());
        }
        out
    }

    fn failure_alert(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (title, e) = self.failure?;
        Some(
            page::page_alert(
                self.id("action-failed"),
                AlertKind::Error,
                title,
                Some(widgets::error_text(e).into()),
                None,
                cx,
            )
            .into_any_element(),
        )
    }

    fn load_failed(&self, e: AdminError, cx: &mut Context<Self>) -> AnyElement {
        let busy = self.loading;
        page::page_alert(
            self.id("failed"),
            AlertKind::Error,
            match self.scope {
                MemoryScope::Team => c::TEAM_FAILED,
                MemoryScope::Member(_) => c::MEMBER_FAILED,
            },
            Some(widgets::error_text(e).into()),
            Some(
                push_button(
                    self.id("retry"),
                    c::RETRY,
                    PushStyle::Outline,
                    PushSize::Small,
                    cx,
                )
                .loading(busy)
                .disabled(busy)
                .on_click(cx.listener(|this, _, _, cx| this.load(cx)))
                .into_any_element(),
            ),
            cx,
        )
        .into_any_element()
    }

    /// Nothing kept yet. 记一条 sits in the header already, so the line has no action.
    fn empty(&self, cx: &mut Context<Self>) -> AnyElement {
        let text: SharedString = match &self.scope {
            MemoryScope::Team => c::TEAM_EMPTY.into(),
            MemoryScope::Member(_) => fill(c::MEMBER_EMPTY, &[("name", &self.name)]).into(),
        };
        let action = None;
        match self.host {
            Host::Settings => GroupedForm::new()
                .row(widgets::list_empty(Sym::Book, text, action, cx))
                .into_any_element(),
            Host::Page => page::compact_empty(Sym::Book, text, action, cx).into_any_element(),
        }
    }

    fn skeleton(&self, cx: &App) -> AnyElement {
        match self.host {
            Host::Settings => GroupedForm::new()
                .row(widgets::list_loading("team-memory-sk", 3, true, cx))
                .into_any_element(),
            Host::Page => page::skeleton_rows("member-memory-sk", SkeletonKind::Compact, 4, cx),
        }
    }

    /// Everything under the header: the failure of an action, 记忆更新 and the lists; or
    /// the loading, failed or empty state.
    fn body(&self, writer: bool, window: &Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out: Vec<AnyElement> = Vec::new();
        out.extend(self.failure_alert(cx));
        let memories = match &self.memories {
            None => {
                out.push(self.skeleton(cx));
                return out;
            }
            Some(Err(e)) => {
                out.push(self.load_failed(*e, cx));
                return out;
            }
            Some(Ok(m)) => m.clone(),
        };
        let day = match &self.day {
            Some(Ok(d)) => d.clone(),
            // The lists still show when only the day could not be read.
            _ => MemoryDay::default(),
        };
        if memories.is_empty() && day.updates.is_empty() {
            out.push(self.empty(cx));
            return out;
        }
        out.extend(self.updates_block(&day, writer, cx));
        if model::lists(&self.scope, &memories).groups.is_empty() {
            out.push(self.empty(cx));
        }
        out.extend(self.lists_block(&memories, writer, window, cx));
        out
    }
}

/// The badge column of a 记忆更新 line: 72, as the time column, so the widest word
/// (可能过时) fits and every line's text starts at one x.
const BADGE_COLUMN: gpui::Pixels = page::m::TIME_COLUMN;

impl Render for MemoryBoard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let writer = self.writer();
        let body = self.body(writer, window, cx);
        let action = writer.then(|| self.add_button(cx));
        crate::selftest::drawn::mark(
            "memory-board",
            format!(
                "scope={} writer={writer} loaded={}",
                self.scope.wire(),
                self.memories.is_some()
            ),
        );
        match self.host {
            Host::Page => page::titled_card(
                "member-memory",
                c::MEMORY_TITLE,
                Some(self.about(writer)),
                action,
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(tokens::space::S16)
                    .children(body),
                None,
                cx,
            )
            .into_any_element(),
            Host::Settings => {
                let count = match &self.memories {
                    Some(Ok(ms)) => Some(
                        ms.iter()
                            .filter(|m| {
                                matches!(m.status, MemoryStatus::Active | MemoryStatus::Pending)
                            })
                            .count() as u32,
                    ),
                    _ => None,
                };
                let mut header = SectionTitle::new(c::TEAM_TITLE).description(self.about(writer));
                if let Some(n) = count {
                    header = header.count(n);
                }
                if let Some(a) = action {
                    header = header.action(a);
                }
                widgets::pane()
                    .child(widgets::section(
                        header,
                        div()
                            .w_full()
                            .flex()
                            .flex_col()
                            .gap(tokens::space::S16)
                            .children(body),
                    ))
                    .into_any_element()
            }
        }
    }
}
