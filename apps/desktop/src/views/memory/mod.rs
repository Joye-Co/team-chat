//! 沉淀 (docs/design/agent-harness.zh.md §7.7): 记得的事 on an AI member's page and
//! 设置 › 团队约定. One board per scope: at the top the newest day's 记忆更新 (新增, 改了,
//! 停用, 可能过时), each line with 撤回; then the memories grouped by kind, each with
//! when it applies, its evidence, where it came from and "⋯" (编辑, 停用); then what was
//! 停用, folded, with 恢复. Nothing asks before it is kept: people correct afterwards.
//! Writers (owners; an AI member for its own) change it, everyone else reads.
//!
//! The same board shows 群里的约定 on a 群's page (its members write it) and 我的偏好 in
//! 设置 (only I read and write it). It shows what the server returns and nothing else: the
//! server leaves out what was learned in a group I am not in. When one person uses the
//! server (盯 4.12) 我的偏好 becomes 我的约定, the team's and mine on one board, and a
//! group's board shows only when an AI member kept something there. Without an AI 服务
//! (盯 4.6) an owner reads that nothing is summed up or tidied, with the way to set it up.

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
    /// One person uses the server ([`model::single_user`]).
    folded: bool,
    /// An owner learned that the server has no AI 服务.
    ai_off: bool,
    /// A person's board while nobody is signed in: it reads and shows nothing.
    signed_out: bool,
    /// Me and the names of members, read from the provider once per change
    /// to the members or the session: a frame reads no store.
    me: std::cell::RefCell<Option<Option<crate::data::Member>>>,
    names: std::cell::RefCell<std::collections::HashMap<String, Option<String>>>,
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
        let subscriptions = vec![cx.subscribe(
            &services.hub,
            |this: &mut Self, _, changes: &Changes, cx| {
                // Who may write follows my level; 我的约定 follows who uses the server.
                // 我的偏好 is the signed-in person's, also after a change of account.
                if changes.any(|c| matches!(c, Change::Members | Change::Session)) {
                    this.me.replace(None);
                    this.names.borrow_mut().clear();
                    let rescoped = this.rescope();
                    let folded = this.single_user();
                    if rescoped || folded != this.folded {
                        this.folded = folded;
                        this.load(cx);
                    }
                    cx.notify();
                }
            },
        )];
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
            folded: false,
            ai_off: false,
            signed_out: false,
            me: Default::default(),
            names: Default::default(),
            _subscriptions: subscriptions,
        };
        this.rescope();
        this.folded = this.single_user();
        this.load(cx);
        this
    }

    pub fn scope(&self) -> &MemoryScope {
        &self.scope
    }

    /// Points a person's board at whoever is signed in now ([`model::scope_for`]); true
    /// when what it reads changed, and then what it showed is dropped.
    fn rescope(&mut self) -> bool {
        let me = self.services.me();
        let (scope, signed_out) = match model::scope_for(&self.scope, me.as_deref()) {
            Some(s) => (s, false),
            None => (self.scope.clone(), true),
        };
        if scope == self.scope && signed_out == self.signed_out {
            return false;
        }
        self.scope = scope;
        self.signed_out = signed_out;
        self.memories = None;
        self.day = None;
        self.busy.clear();
        self.failure = None;
        self.ai_off = false;
        true
    }

    /// True when one person uses the server. Before the first sync the cache holds only
    /// me, so it waits for the connection.
    fn single_user(&self) -> bool {
        let p = &self.services.provider;
        p.connection() != crate::data::ConnectionState::Connecting
            && model::single_user(&p.members())
    }

    /// One person uses the server: 团队约定 has no place of its own, it is in 我的约定.
    pub fn folded(&self) -> bool {
        self.folded
    }

    /// 我的约定: my own board while one person uses the server.
    pub fn folded_mine(&self) -> bool {
        self.folded && matches!(self.scope, MemoryScope::Person(_))
    }

    /// The scopes the board reads: its own, and the team's in 我的约定.
    fn scopes(&self) -> Vec<MemoryScope> {
        let mut out = vec![self.scope.clone()];
        if self.folded_mine() {
            out.push(MemoryScope::Team);
        }
        out
    }

    /// The board's name: 记得的事, 团队约定, 群里的约定, 我的偏好 or 我的约定.
    pub fn title(&self) -> &'static str {
        match &self.scope {
            _ if self.folded_mine() => c::MINE_TITLE,
            MemoryScope::Team => c::TEAM_TITLE,
            MemoryScope::Member(_) => c::MEMORY_TITLE,
            MemoryScope::Group(_) => c::GROUP_TITLE,
            MemoryScope::Person(_) => c::PERSON_TITLE,
        }
    }

    /// A group's board while one person uses the server, with nothing kept: the page
    /// leaves it out, 我的约定 is the one place to write.
    pub fn hidden(&self) -> bool {
        if !(self.folded && matches!(self.scope, MemoryScope::Group(_))) {
            return false;
        }
        let empty_day = match &self.day {
            Some(Ok(d)) => d.updates.is_empty(),
            _ => true,
        };
        matches!(&self.memories, Some(Ok(ms)) if ms.is_empty()) && empty_day
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
        if self.signed_out {
            self.loading = false;
            cx.notify();
            return;
        }
        self.loading = true;
        let p = &self.services.provider;
        let reads: Vec<_> = self
            .scopes()
            .iter()
            .map(|s| (p.memories(s), p.memory_updates(s)))
            .collect();
        // Only an owner can read the AI 服务; anyone else is never told (盯 4.6).
        let ai = crate::views::settings::i_am_owner(&self.services).then(|| p.ai_service());
        cx.spawn(async move |this, cx| {
            let mut memories: Result<Vec<Memory>, AdminError> = Ok(Vec::new());
            let mut days: Result<Vec<MemoryDay>, AdminError> = Ok(Vec::new());
            for (ms, day) in reads {
                let ms = ms.await.unwrap_or(Err(AdminError::Other));
                let day = day.await.unwrap_or(Err(AdminError::Other));
                memories = memories.and_then(|mut all| {
                    all.extend(ms?);
                    Ok(all)
                });
                days = days.and_then(|mut all| {
                    all.push(day?);
                    Ok(all)
                });
            }
            let ai_off = match ai {
                Some(reply) => matches!(reply.await, Ok(Ok(s)) if s.provider.is_none()),
                None => false,
            };
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.loading = false;
                this.memories = Some(memories);
                this.day = Some(days.map(model::merge_days));
                this.ai_off = ai_off;
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn writer(&self) -> bool {
        self.writer_of(&self.scope)
    }

    fn writer_of(&self, scope: &MemoryScope) -> bool {
        let mut me = self.me.borrow_mut();
        let me = me.get_or_insert_with(|| match self.services.provider.session() {
            SessionState::SignedIn { me, .. } => Some(me),
            _ => None,
        });
        model::can_write(scope, me.as_ref())
    }

    /// Whether I may change one memory: by its own scope (我的约定 holds two).
    fn writes(&self, m: &Memory) -> bool {
        match MemoryScope::parse(&m.scope) {
            Some(s) if s != self.scope => self.writer_of(&s),
            _ => self.writer(),
        }
    }

    /// 记一条 shows: I write the board, and a group's board is not folded away.
    fn adds(&self) -> bool {
        !self.signed_out
            && self.writer()
            && !(self.folded && matches!(self.scope, MemoryScope::Group(_)))
    }

    fn name_of(&self, id: &str) -> String {
        self.names
            .borrow_mut()
            .entry(id.to_string())
            .or_insert_with(|| {
                self.services
                    .provider
                    .member(id)
                    .map(|m| crate::ui::avatar::name_beside_avatar(&m.name).to_string())
            })
            .clone()
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
            folded: self
                .folded_mine()
                .then(|| self.writer_of(&MemoryScope::Team)),
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
            _ if self.folded_mine() => c::MINE_ABOUT.into(),
            (MemoryScope::Group(_), _) => c::GROUP_ABOUT.into(),
            (MemoryScope::Person(_), _) => c::PERSON_ABOUT.into(),
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
        ElementId::Name(format!("{}-{part}", self.id_prefix()).into())
    }

    fn id_prefix(&self) -> &'static str {
        match self.scope {
            MemoryScope::Member(_) => "member-memory",
            MemoryScope::Team => "team-memory",
            MemoryScope::Group(_) => "group-memory",
            MemoryScope::Person(_) => "my-memory",
        }
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
                .map(|u| {
                    let writes = match u.after.as_ref().or(u.before.as_ref()) {
                        Some(m) => self.writes(m),
                        None => writer,
                    };
                    self.update_row(model::update_line(u, writes), cx)
                })
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
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let lists = model::lists_in(self.kind_order(), memories);
        let mut out: Vec<AnyElement> = Vec::new();
        for g in &lists.groups {
            let rows = g
                .items
                .iter()
                .map(|m| self.memory_row(m, self.writes(m), window, cx))
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
                    .map(|m| self.memory_row(m, self.writes(m), window, cx))
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
                _ if self.folded_mine() => c::MINE_FAILED,
                MemoryScope::Team => c::TEAM_FAILED,
                MemoryScope::Member(_) => c::MEMBER_FAILED,
                MemoryScope::Group(_) => c::GROUP_FAILED,
                MemoryScope::Person(_) => c::PERSON_FAILED,
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
            _ if self.folded_mine() => c::MINE_EMPTY.into(),
            MemoryScope::Team => c::TEAM_EMPTY.into(),
            MemoryScope::Member(_) => fill(c::MEMBER_EMPTY, &[("name", &self.name)]).into(),
            MemoryScope::Group(_) => c::GROUP_EMPTY.into(),
            MemoryScope::Person(_) => c::PERSON_EMPTY.into(),
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
        let id = format!("{}-sk", self.id_prefix());
        match self.host {
            Host::Settings => GroupedForm::new()
                .row(widgets::list_loading(&id, 3, true, cx))
                .into_any_element(),
            Host::Page => page::skeleton_rows(&id, SkeletonKind::Compact, 4, cx),
        }
    }

    /// The kind groups this board shows, in order.
    fn kind_order(&self) -> &'static [crate::data::memory::MemoryKind] {
        model::kind_order(&self.scope, self.folded_mine())
    }

    /// No AI 服务 on the server (盯 4.6): nothing is summed up after work and nothing is
    /// tidied at night. Only an owner learns it, and gets the way to the setting.
    fn ai_off_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.ai_off {
            return None;
        }
        let services = self.services.clone();
        Some(
            page::page_alert(
                self.id("ai-off"),
                AlertKind::Info,
                c::AI_OFF_TITLE,
                Some(c::AI_OFF_BODY.into()),
                Some(
                    push_button(
                        self.id("ai-off-setup"),
                        c::AI_OFF_SETUP,
                        PushStyle::Outline,
                        PushSize::Small,
                        cx,
                    )
                    .on_click(move |_, _, cx| {
                        crate::views::admin::open_at(
                            services.clone(),
                            crate::views::admin::AdminPage::Ai,
                            cx,
                        )
                    })
                    .into_any_element(),
                ),
                cx,
            )
            .into_any_element(),
        )
    }

    /// Everything under the header: the failure of an action, 记忆更新 and the lists; or
    /// the loading, failed or empty state.
    fn body(&self, writer: bool, window: &Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out: Vec<AnyElement> = Vec::new();
        if self.signed_out {
            return out;
        }
        out.extend(self.failure_alert(cx));
        out.extend(self.ai_off_notice(cx));
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
        if model::lists_in(self.kind_order(), &memories)
            .groups
            .is_empty()
        {
            out.push(self.empty(cx));
        }
        out.extend(self.lists_block(&memories, window, cx));
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
        let action = self.adds().then(|| self.add_button(cx));
        crate::selftest::drawn::mark(
            "memory-board",
            format!(
                "scope={} writer={writer} loaded={} folded={} ai_off={}",
                self.scope.wire(),
                self.memories.is_some(),
                self.folded,
                self.ai_off
            ),
        );
        match self.host {
            Host::Page => page::titled_card(
                self.id_prefix(),
                self.title(),
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
                let mut header = SectionTitle::new(self.title()).description(self.about(writer));
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
