//! 产出 in the main window (Artifact.dc.html): one 产出 with its page live
//! in a frame (a child web view from `crates/artifact-view`, [`WebEmbed`]),
//! the version picker, and the 来龙去脉 side column (来自话题, 做的人,
//! 版本, 页面里的回应, 用到的决定, 被引用). Without a page on disk it shows
//! a card with 打开 (the native 产出 window). Also the list of every 产出,
//! and the 产出 pane beside a 话题 (`pane`).
//!
//! Bridge capabilities follow the author (R10, `views/artifacts/host.rs`): every
//! page reads its data and may point at its parts; a version an active bot member
//! made may `emit` and `ask` the events it declares (`artifact.event`). An event
//! goes only right after the viewer's own input in that page, and emits at most
//! about five per second ([`forward_page_event`]).

pub mod gate_check;
pub mod pane;
pub mod scenes;

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    time::Instant,
};

use artifact_view::{Capabilities, Reject, gate::EventLimiter};
use gpui::{
    AnyElement, App, ClipboardItem, Context, InteractiveElement as _, IntoElement, ParentElement,
    Pixels, Render, SharedString, StatefulInteractiveElement as _, Styled, Subscription, Window,
    div, prelude::FluentBuilder as _, px,
};
use gpui_component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};

use crate::{
    app::{Place, Services},
    copy::{artifact_page as c, fill},
    data::{ArtifactDetail, ArtifactFolder, FileError, Millis, TopicStatus},
    theme::{self, Type as _, space},
    ui::{
        self, MemberAvatar, Ring, Sym,
        web_embed::{WebEmbed, WebEmbedSpec},
    },
    views::{
        group_page::{
            kind_label,
            kit::{self, META},
            short_time,
        },
        shell::{PageView, TitleBarSpec, member_name, panes::watch, stub_body},
    },
};

/// The version folders a view has asked for, by `(产出, version)`.
#[derive(Clone, Default)]
pub struct Folders(Rc<RefCell<HashMap<(String, u32), FolderAsk>>>);

/// One asked folder: `None` while it loads.
type FolderAsk = Option<Result<ArtifactFolder, FileError>>;

impl Folders {
    /// The folder of a version, asking the provider once; the view is
    /// notified when it arrives.
    pub fn get<T: 'static>(
        &self,
        services: &Services,
        id: &str,
        version: u32,
        cx: &mut Context<T>,
    ) -> Option<Result<ArtifactFolder, FileError>> {
        let key = (id.to_string(), version);
        if let Some(state) = self.0.borrow().get(&key) {
            return state.clone();
        }
        self.0.borrow_mut().insert(key.clone(), None);
        let rx = services.provider.open_artifact_version(id, Some(version));
        let map = self.0.clone();
        cx.spawn(async move |this, cx| {
            let result = rx.await.unwrap_or(Err(FileError::Other));
            map.borrow_mut().insert(key, Some(result));
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
        None
    }
}

/// What the page bridge of a version may do (R10, see `artifacts::host`).
pub fn capabilities(
    provider: &dyn crate::data::ChatProvider,
    detail: &ArtifactDetail,
    version: u32,
) -> Capabilities {
    crate::views::artifacts::host::capabilities_for(
        provider.page_bridge(&detail.summary.id, version).as_ref(),
    )
}

use crate::views::artifacts::host::{self, PageRef};

/// A waiting emit: its page and payload, and the page's request when it waits.
type PendingEvent = (PageRef, serde_json::Value, Option<serde_json::Value>);

/// The page and request id of an emit that waits.
type QueuedRequest = (PageRef, serde_json::Value);

thread_local! {
    /// Page events by `(产出, version)`, paced for every page of the app.
    static PAGE_EVENTS: RefCell<EventLimiter<(String, u32), PendingEvent>> =
        RefCell::new(EventLimiter::new());
    /// When the next send of waiting page events runs.
    static FLUSH_AT: Cell<Option<Instant>> = const { Cell::new(None) };
    /// The page request of the emit that waits per `(产出, version, name)`.
    static QUEUED: RefCell<HashMap<(String, u32, String), QueuedRequest>> =
        RefCell::new(HashMap::new());
}

/// Sends a page event (`emit` / `emitEvent`) to the bot that made the version, when
/// the viewer used the page just before (`user_action`); the bridge already checked
/// that the version declares `name`. Others are dropped. Events over the limit wait,
/// and a later one of the same name replaces the waiting one (whose page request ends
/// `rate_limited`), so a page can never fill the outbox.
#[allow(clippy::too_many_arguments)]
pub fn forward_page_event(
    page: PageRef,
    artifact_id: &str,
    version: u32,
    name: &str,
    payload: &serde_json::Value,
    user_action: bool,
    reply_id: Option<serde_json::Value>,
    cx: &mut App,
) {
    if !user_action {
        tracing::debug!(
            artifact_id,
            version,
            name,
            "page event without user input dropped"
        );
        if let Some(id) = &reply_id {
            host::finish_request(&page, version, id, Err(Reject::NotAllowed), cx);
        }
        return;
    }
    let now_go = PAGE_EVENTS.with(|l| {
        l.borrow_mut().offer(
            (artifact_id.to_string(), version),
            name,
            (page.clone(), payload.clone(), reply_id.clone()),
            Instant::now(),
        )
    });
    match now_go {
        Some((page, payload, reply)) => {
            host::send_emit(page, artifact_id, version, name, payload, reply, cx)
        }
        None => {
            let key = (artifact_id.to_string(), version, name.to_string());
            let old = QUEUED.with(|q| match &reply_id {
                Some(id) => q.borrow_mut().insert(key, (page, id.clone())),
                None => q.borrow_mut().remove(&key),
            });
            if let Some((p, id)) = old {
                host::finish_request(&p, version, &id, Err(Reject::RateLimited), cx);
            }
            schedule_page_events(cx)
        }
    }
}

/// Sends the waiting page events when they may go.
fn schedule_page_events(cx: &mut App) {
    let Some(at) = PAGE_EVENTS.with(|l| l.borrow_mut().next_due(Instant::now())) else {
        return;
    };
    if FLUSH_AT.with(|f| f.get()).is_some_and(|t| t <= at) {
        return;
    }
    FLUSH_AT.with(|f| f.set(Some(at)));
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        cx.background_executor()
            .timer(at.saturating_duration_since(Instant::now()))
            .await;
        cx.update(|cx| {
            FLUSH_AT.with(|f| {
                if f.get() == Some(at) {
                    f.set(None);
                }
            });
            let due = PAGE_EVENTS.with(|l| l.borrow_mut().take_due(Instant::now()));
            for ((id, version), name, (page, payload, reply)) in due {
                QUEUED.with(|q| q.borrow_mut().remove(&(id.clone(), version, name.clone())));
                host::send_emit(page, &id, version, &name, payload, reply, cx);
            }
            schedule_page_events(cx);
        });
    })
    .detach();
}

/// The live page of a version, sized `height`, wired to the page host: data,
/// declared events and asks to the bot that made it, refs to the composer.
pub fn embed(
    services: &Services,
    key: &str,
    detail: &ArtifactDetail,
    folder: &ArtifactFolder,
    height: Pixels,
    radius: Pixels,
    cx: &App,
) -> WebEmbed {
    let id = detail.summary.id.clone();
    let (spec, caps) = host::version_spec(
        services.provider.as_ref(),
        &id,
        folder.version,
        folder.dir.clone(),
        folder.entry.clone(),
    );
    WebEmbed::new(
        SharedString::from(format!("{key}-{id}")),
        WebEmbedSpec::new(id.clone(), spec)
            .capabilities(caps)
            .viewer(host::viewer(services.provider.as_ref()))
            .ref_menu(Some(host::ref_menu(cx))),
    )
    .height(height)
    .rounded(radius)
    .on_host(host::embed_handler(id))
    .on_event(move |event, _, _| {
        if let artifact_view::ArtifactEvent::ExternalLink { url } = event {
            crate::views::artifacts::window::ask_before_link(url.clone())
        }
    })
}

pub struct ArtifactPage {
    services: Services,
    folders: Folders,
    aside_open: bool,
    copied: bool,
    // --- track cards: the Finder-like list (NATIVE.md §9.6) ---
    /// Icons (true) or list.
    grid: bool,
    sort: ListSort,
    search: Option<gpui::Entity<gpui_component::input::InputState>>,
    selected: Option<String>,
    // --- end track cards ---
    _subscriptions: Vec<Subscription>,
}

/// How the 产出 list is sorted (the sort pop-up).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ListSort {
    Time,
    Name,
    Kind,
}

impl ListSort {
    const ALL: [ListSort; 3] = [ListSort::Time, ListSort::Name, ListSort::Kind];

    fn label(self) -> &'static str {
        match self {
            ListSort::Time => c::ARTIFACTS_SORT_TIME,
            ListSort::Name => c::ARTIFACTS_SORT_NAME,
            ListSort::Kind => c::ARTIFACTS_SORT_KIND,
        }
    }
}

impl ArtifactPage {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        Self {
            _subscriptions: watch(&services, cx),
            services,
            folders: Folders::default(),
            aside_open: true,
            copied: false,
            grid: true,
            sort: ListSort::Time,
            search: None,
            selected: None,
        }
    }

    fn open(&self, cx: &App) -> Option<(String, Option<u32>)> {
        match &self.services.nav.read(cx).place {
            Some(Place::Artifact { id, version }) => Some((id.clone(), *version)),
            _ => None,
        }
    }
}

impl PageView for ArtifactPage {
    fn title_bar(&self, cx: &App) -> TitleBarSpec {
        match self
            .open(cx)
            .and_then(|(id, _)| self.services.provider.artifact(&id))
        {
            Some(a) => TitleBarSpec::page(a.summary.title),
            None => TitleBarSpec::page(c::ARTIFACTS_TITLE),
        }
    }

    fn draws_own_header(&self, _cx: &App) -> bool {
        true
    }
}

/// The note of a version: the provider's, else the 来龙去脉 one.
fn version_note(services: &Services, detail: &ArtifactDetail, v: u32) -> Option<String> {
    detail
        .versions
        .iter()
        .find(|x| x.version == v)
        .and_then(|x| x.note.clone())
        .or_else(|| {
            services
                .provider
                .artifact_story(&detail.summary.id)
                .and_then(|s| {
                    s.version_notes
                        .into_iter()
                        .find(|(n, _)| *n == v)
                        .map(|(_, t)| t)
                })
        })
}

/// `今天 09:40`, `昨天` or `10-31`.
fn version_time(now: Millis, at: Millis) -> String {
    kit::day_and_time(now, at, false)
}

impl ArtifactPage {
    /// The toolbar row (NATIVE.md §2, §5.1): the 产出's name as the title,
    /// `群 · 类型` as the subtitle (no breadcrumb, no tag), then the version
    /// pop-up and one glass group: 分享, 在新窗口打开, 单独问.
    fn header(
        &self,
        detail: &ArtifactDetail,
        shown: u32,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use crate::ui::native as nk;
        let p = self.services.provider.clone();
        let now = p.clock_now();
        let a = &detail.summary;
        let group = p
            .groups()
            .into_iter()
            .find(|g| g.id == detail.group_id)
            .map(|g| g.name)
            .unwrap_or_default();
        let at = detail
            .versions
            .iter()
            .find(|v| v.version == shown)
            .map(|v| v.created_at)
            .unwrap_or(a.updated_at);
        let versions = detail.versions.clone();
        let services = self.services.clone();
        let id = a.id.clone();
        let picker = nk::popup_button(
            "artifact-version",
            format!("v{shown} · {}", version_time(now, at)),
            nk::PushSize::Regular,
            window,
            cx,
        )
        .dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, _, _| {
            let mut menu = menu.label(c::ARTIFACT_VERSIONS);
            for v in &versions {
                let note = services
                    .provider
                    .artifact(&id)
                    .and_then(|d| version_note(&services, &d, v.version))
                    .unwrap_or_default();
                let label = format!(
                    "v{}  {}  {}",
                    v.version,
                    note,
                    version_time(services.provider.clock_now(), v.created_at)
                );
                let nav = services.nav.clone();
                let (id, ver) = (id.clone(), v.version);
                menu = menu.item(PopupMenuItem::new(label).checked(ver == shown).on_click(
                    move |_, _, cx| {
                        let id = id.clone();
                        nav.update(cx, |n, cx| n.open_artifact(id, Some(ver), cx))
                    },
                ));
            }
            menu
        });
        let share_text = fill(
            c::ARTIFACT_SHARE_TEXT,
            &[("title", &a.title), ("v", &shown.to_string())],
        );
        let active = nk::emphasized(window);
        let share = nk::toolbar_item(
            "artifact-share",
            crate::ui::sf_symbols::Sf::Copy,
            if self.copied {
                c::ARTIFACT_COPIED
            } else {
                c::ARTIFACT_SHARE
            },
            nk::ToolbarItemKind::Plain,
            active,
            window,
            cx,
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(share_text.clone()));
            this.copied = true;
            cx.notify();
        }));
        let provider = self.services.provider.clone();
        let open_id = a.id.clone();
        let nav = self.services.nav.clone();
        let (sg, st) = (detail.group_id.clone(), a.topic_id.clone());
        let made_by = a.created_by.clone();
        let open_window = nk::toolbar_item(
            "artifact-open-window",
            crate::ui::sf_symbols::Sf::Open,
            c::ARTIFACT_OPEN_WINDOW,
            nk::ToolbarItemKind::Plain,
            active,
            window,
            cx,
        )
        .on_click(move |_, _, cx| {
            crate::views::artifacts::window::open_artifact(&provider, &open_id, Some(shown), cx)
        });
        let side_chat = nk::toolbar_item(
            "artifact-side-chat",
            crate::ui::sf_symbols::Sf::SideChat,
            c::ARTIFACT_SIDE_CHAT,
            nk::ToolbarItemKind::Plain,
            active,
            window,
            cx,
        )
        .on_click(move |_, _, cx| {
            let (g, t, m) = (sg.clone(), st.clone(), made_by.clone());
            nav.update(cx, |n, cx| {
                n.open_topic(g, t, cx);
                n.toggle_side_chat(Some(m), cx);
            })
        });
        // inc-host: 已更新到 v{n} after a reload in place, or 有 v{n}, in the
        // subtitle.
        let mut subtitle = vec![group, kind_label(a.kind).to_string()];
        subtitle.extend(host::notice(&a.id, shown, a.version, cx));
        let subtitle = subtitle
            .into_iter()
            .filter(|s| !s.trim().is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        // The inspector toggle (NATIVE.md §2): `sidebar.trailing`, toggled while 来龙去脉
        // shows. The inspector has no heading or button of its own.
        let inspector = nk::toolbar_item(
            "artifact-inspector",
            crate::ui::sf_symbols::Sf::PaneRight,
            c::ARTIFACT_STORY,
            if self.aside_open {
                nk::ToolbarItemKind::Toggled
            } else {
                nk::ToolbarItemKind::Plain
            },
            active,
            window,
            cx,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.aside_open = !this.aside_open;
            cx.notify();
        }));
        let leading = content_leading(&self.services, cx);
        nk::ToolbarRow::new(a.title.clone())
            .subtitle(subtitle)
            .leading_inset(leading)
            .group(picker)
            .group(nk::ToolbarGroup::new([share, open_window, side_chat]))
            .group(nk::ToolbarGroup::new([inspector]))
            .into_any_element()
    }

    fn frame(
        &mut self,
        detail: &ArtifactDetail,
        shown: u32,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = theme::colors(cx);
        let p = self.services.provider.clone();
        let now = p.clock_now();
        let latest = detail.summary.version;
        let old = shown < latest;
        let band = old.then(|| {
            let at = detail
                .versions
                .iter()
                .find(|v| v.version == shown)
                .map(|v| v.created_at)
                .unwrap_or(detail.summary.updated_at);
            let nav = self.services.nav.clone();
            let id = detail.summary.id.clone();
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(space::S8)
                .h(px(40.))
                .pl(space::S16)
                .pr(space::S8)
                .border_b_1()
                .border_color(k.line)
                .bg(k.raised)
                .t_small()
                .text_color(k.ink_2)
                .child(ui::icon(Sym::Waiting, px(14.), k.ink_3))
                .child(c::ARTIFACT_OLD_VERSION)
                .child(div().text_color(k.ink_3).child(fill(
                    c::ARTIFACT_OLD_VERSION_LINE,
                    &[
                        ("v", &shown.to_string()),
                        ("time", &version_time(now, at)),
                        (
                            "note",
                            &version_note(&self.services, detail, shown).unwrap_or_default(),
                        ),
                    ],
                )))
                .child(div().flex_1())
                .child(
                    crate::ui::native::push_button(
                        "artifact-back-latest",
                        fill(c::ARTIFACT_BACK_TO, &[("v", &latest.to_string())]),
                        crate::ui::native::PushStyle::Outline,
                        crate::ui::native::PushSize::Small,
                        cx,
                    )
                    .on_click(move |_, _, cx| {
                        let id = id.clone();
                        nav.update(cx, |n, cx| n.open_artifact(id, None, cx))
                    }),
                )
        });
        let folder = self
            .folders
            .get(&self.services, &detail.summary.id, shown, cx);
        // inc-host: a live page has the footer of what it may do and its pending band.
        let live = matches!(&folder, Some(Ok(f)) if f.is_page);
        let footer = live.then(|| {
            host::page_footer(
                &SharedString::from("artifact-page-foot"),
                self.services.provider.as_ref(),
                &detail.summary.id,
                shown,
                None,
                cx,
            )
        });
        // The page fills the content area edge to edge under the toolbar row (no frame).
        let height = window.viewport_size().height
            - crate::ui::native::metrics::TOOLBAR_ROW
            - if old { px(40.) } else { px(0.) }
            - if live { px(35.) } else { px(0.) };
        let body = match folder {
            Some(Ok(folder)) if folder.is_page => embed(
                &self.services,
                "artifact-page",
                detail,
                &folder,
                height,
                px(0.),
                cx,
            )
            .into_any_element(),
            Some(Ok(_)) | Some(Err(_)) => {
                let provider = self.services.provider.clone();
                let id = detail.summary.id.clone();
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(space::S12)
                    .child(kit::artifact_thumb(
                        &self.services,
                        &detail.summary,
                        px(192.),
                        px(144.),
                        cx,
                    ))
                    .child(
                        div()
                            .t_small()
                            .text_color(k.ink_3)
                            .child(c::ARTIFACT_PAGE_SNAPSHOT),
                    )
                    .child(
                        crate::ui::native::push_button(
                            "artifact-open",
                            c::ARTIFACT_OPEN,
                            crate::ui::native::PushStyle::Default,
                            crate::ui::native::PushSize::Regular,
                            cx,
                        )
                        .on_click(move |_, _, cx| {
                            crate::views::artifacts::window::open_artifact(
                                &provider,
                                &id,
                                Some(shown),
                                cx,
                            )
                        }),
                    )
                    .into_any_element()
            }
            None => div().size_full().into_any_element(),
        };
        div()
            .flex_1()
            .min_h(px(0.))
            .overflow_hidden()
            .flex()
            .flex_col()
            .children(band)
            .child(div().flex_1().min_h(px(0.)).child(body))
            .children(footer)
            .into_any_element()
    }

    fn story(&self, detail: &ArtifactDetail, shown: u32, cx: &mut Context<Self>) -> AnyElement {
        let k = theme::colors(cx);
        let p = self.services.provider.clone();
        let now = p.clock_now();
        let story = p.artifact_story(&detail.summary.id).unwrap_or_default();
        let mut cards: Vec<AnyElement> = Vec::new();
        // 来自话题, 做的人, 要求.
        let topic = story
            .topic
            .clone()
            .or_else(|| Some((detail.group_id.clone(), detail.summary.topic_id.clone())))
            .and_then(|(g, t)| p.topic(&g, &t));
        let made_by = story
            .made_by
            .clone()
            .unwrap_or_else(|| detail.summary.created_by.clone());
        let person = |label: &'static str, id: &str| {
            let m = p.member(id);
            let busy = p.active_runs().iter().any(|r| r.bot_id == id);
            (
                div()
                    .type_role(META)
                    .text_color(k.ink_3)
                    .child(label)
                    .into_any_element(),
                div()
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .t_small()
                    .child(MemberAvatar::new(m.as_ref(), px(20.)).ring(busy.then_some(Ring::Live)))
                    .child(
                        div()
                            .medium()
                            .text_color(k.ink)
                            .child(m.as_ref().map(|m| m.name.clone()).unwrap_or_default()),
                    )
                    .children(m.and_then(|m| m.role).map(ui::JobTag::new))
                    .into_any_element(),
            )
        };
        let mut people = vec![person(c::ARTIFACT_MADE_BY, &made_by)];
        if let Some(asked) = &story.asked_by {
            people.push(person(c::ARTIFACT_ASKED_BY, asked));
        }
        let topic_row = topic.map(|t| {
            let group = p
                .groups()
                .into_iter()
                .find(|g| g.id == t.group_id)
                .map(|g| g.name)
                .unwrap_or_default();
            let done = t.status == TopicStatus::Done;
            let meta = if done {
                fill(
                    c::ARTIFACT_TOPIC_DONE,
                    &[
                        ("group", &group),
                        ("time", &short_time(now, t.last_activity_at)),
                    ],
                )
            } else {
                fill(c::ARTIFACT_TOPIC_ACTIVE, &[("group", &group)])
            };
            let nav = self.services.nav.clone();
            let (g, id) = (t.group_id.clone(), t.id.clone());
            div()
                .id("artifact-from-topic")
                .mt(space::S6)
                .mx(-space::S6)
                .p(space::S6)
                .rounded(theme::radius::MD)
                .flex()
                .items_center()
                .gap(space::S10)
                .child(
                    div()
                        .flex_none()
                        .w(px(16.))
                        .flex()
                        .justify_center()
                        .child(if done {
                            ui::icon(Sym::Check, px(14.), k.ok).into_any_element()
                        } else {
                            kit::live_dot("artifact-topic-live", 6., cx)
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .one_line()
                                .t_small()
                                .medium()
                                .text_color(k.ink)
                                .child(t.title.clone()),
                        )
                        .child(div().type_role(META).text_color(k.ink_3).child(meta)),
                )
                .child(ui::icon(Sym::ChevronRight, px(14.), k.ink_3))
                .on_click(move |_, _, cx| {
                    let (g, id) = (g.clone(), id.clone());
                    nav.update(cx, |n, cx| n.open_topic(g, id, cx))
                })
        });
        cards.push(
            kit::side_card(cx)
                .child(kit::card_label(c::ARTIFACT_FROM_TOPIC, None, cx).h(px(18.)))
                .children(topic_row)
                .child(
                    div()
                        .mt(space::S8)
                        .pt(space::S10)
                        .border_t_1()
                        .border_color(k.line)
                        .flex()
                        .flex_col()
                        .gap(space::S8)
                        .children(people.into_iter().map(|(label, value)| {
                            div()
                                .flex()
                                .items_center()
                                .gap(space::S8)
                                .child(div().flex_none().w(px(44.)).child(label))
                                .child(value)
                        })),
                )
                .into_any_element(),
        );
        // 版本.
        if !detail.versions.is_empty() {
            let id = detail.summary.id.clone();
            let compare = (shown > 1).then(|| {
                let nav = self.services.nav.clone();
                let id = id.clone();
                kit::text_link(
                    "artifact-compare",
                    &fill(c::ARTIFACT_COMPARE, &[("v", &(shown - 1).to_string())]),
                    cx,
                )
                .on_click(move |_, _, cx| {
                    let id = id.clone();
                    nav.update(cx, |n, cx| n.open_artifact(id, Some(shown - 1), cx))
                })
            });
            let rows =
                detail.versions.iter().map(|v| {
                    let on = v.version == shown;
                    let nav = self.services.nav.clone();
                    let (id, ver) = (id.clone(), v.version);
                    let latest = detail.summary.version;
                    div()
                        .id(SharedString::from(format!("artifact-v{}", v.version)))
                        .flex()
                        .items_center()
                        .gap(space::S10)
                        .h(px(36.))
                        .px(space::S6)
                        .rounded(theme::radius::MD)
                        .t_small()
                        .text_color(if on { k.ink } else { k.ink_2 })
                        .when(on, |el| el.bg(k.sel))
                        .child(
                            div()
                                .flex_none()
                                .w(px(24.))
                                .mono()
                                .type_role(META)
                                .when(on, |el| el.semibold().text_color(k.ink))
                                .when(!on, |el| el.text_color(k.ink_3))
                                .child(format!("v{}", v.version)),
                        )
                        .child(div().flex_1().min_w_0().one_line().child(
                            version_note(&self.services, detail, v.version).unwrap_or_default(),
                        ))
                        .child(
                            div()
                                .flex_none()
                                .type_role(META)
                                .text_color(k.ink_3)
                                .child(version_time(now, v.created_at)),
                        )
                        .on_click(move |_, _, cx| {
                            let id = id.clone();
                            let ver = (ver != latest).then_some(ver);
                            nav.update(cx, |n, cx| n.open_artifact(id, ver, cx))
                        })
                });
            cards.push(
                kit::side_card(cx)
                    .child(
                        kit::card_label(c::ARTIFACT_VERSIONS, None, cx)
                            .h(px(18.))
                            .child(div().flex_1())
                            .children(compare.map(|el| el.mr(-space::S6))),
                    )
                    .child(
                        div()
                            .mt(space::S6)
                            .mx(-space::S6)
                            .flex()
                            .flex_col()
                            .gap(space::S2)
                            .children(rows),
                    )
                    .into_any_element(),
            );
        }
        // 页面里的回应.
        let responses = p.artifact_responses(&detail.summary.id);
        if !responses.is_empty() {
            let rows = responses.into_iter().enumerate().map(|(i, r)| {
                let m = p.member(&r.member_id);
                let row = div()
                    .id(SharedString::from(format!("artifact-response-{i}")))
                    .flex()
                    .items_center()
                    .gap(space::S10)
                    .t_small()
                    .child(MemberAvatar::new(m.as_ref(), px(18.)))
                    .child(div().flex_1().min_w_0().one_line().child(r.text.clone()));
                match (r.topic.clone(), r.at) {
                    (Some((g, t)), _) => {
                        let nav = self.services.nav.clone();
                        row.mx(-space::S6)
                            .px(space::S6)
                            .py(space::S4)
                            .rounded(theme::radius::MD)
                            .text_color(k.ink_2)
                            .child(ui::icon(Sym::ChevronRight, px(14.), k.ink_3))
                            .on_click(move |_, _, cx| {
                                let (g, t) = (g.clone(), t.clone());
                                nav.update(cx, |n, cx| n.open_topic(g, t, cx))
                            })
                            .into_any_element()
                    }
                    (None, at) => row
                        .text_color(k.ink)
                        .children(at.map(|at| {
                            div()
                                .flex_none()
                                .mono()
                                .type_role(META)
                                .text_color(k.ink_3)
                                .child(short_time(now, at))
                        }))
                        .into_any_element(),
                }
            });
            cards.push(
                kit::side_card(cx)
                    .child(kit::card_label(c::ARTIFACT_RESPONSES, None, cx).h(px(18.)))
                    .child(
                        div()
                            .mt(space::S8)
                            .flex()
                            .flex_col()
                            .gap(space::S8)
                            .children(rows),
                    )
                    .into_any_element(),
            );
        }
        // 用到的决定.
        if !story.decisions.is_empty() {
            let rows = story.decisions.iter().enumerate().map(|(i, d)| {
                let group = p
                    .groups()
                    .into_iter()
                    .find(|g| g.id == d.group_id)
                    .map(|g| g.name)
                    .unwrap_or_default();
                let nav = self.services.nav.clone();
                let g = d.group_id.clone();
                div()
                    .id(SharedString::from(format!("artifact-decision-{i}")))
                    .mx(-space::S6)
                    .p(space::S6)
                    .rounded(theme::radius::MD)
                    .flex()
                    .gap(space::S10)
                    .child(
                        div()
                            .mt(px(2.))
                            .child(ui::icon(Sym::Done, px(14.), k.ink_3)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().t_small().text_color(k.ink).child(d.text.clone()))
                            .child(div().type_role(META).text_color(k.ink_3).child(fill(
                                c::ARTIFACT_DECISION_META,
                                &[
                                    ("name", &member_name(&self.services, &d.by)),
                                    ("time", &kit::month_day(d.at)),
                                    ("group", &group),
                                ],
                            ))),
                    )
                    .on_click(move |_, _, cx| {
                        let g = g.clone();
                        nav.update(cx, |n, cx| n.open_group(g, cx))
                    })
            });
            cards.push(
                kit::side_card(cx)
                    .child(kit::card_label(c::ARTIFACT_DECISIONS, None, cx).h(px(18.)))
                    .child(div().mt(space::S6).flex().flex_col().children(rows))
                    .into_any_element(),
            );
        }
        // 被引用.
        if !story.cited_by.is_empty() {
            let rows = story.cited_by.iter().enumerate().map(|(i, (g, t, title))| {
                let group = p
                    .groups()
                    .into_iter()
                    .find(|x| &x.id == g)
                    .map(|x| x.name)
                    .unwrap_or_default();
                let nav = self.services.nav.clone();
                let (g, t) = (g.clone(), t.clone());
                div()
                    .id(SharedString::from(format!("artifact-cited-{i}")))
                    .flex()
                    .items_center()
                    .gap(space::S10)
                    .h(px(32.))
                    .px(space::S6)
                    .rounded(theme::radius::MD)
                    .t_small()
                    .text_color(k.ink)
                    .child(ui::icon(Sym::List, px(14.), k.ink_3))
                    .child(div().flex_1().min_w_0().one_line().child(title.clone()))
                    .child(
                        div()
                            .flex_none()
                            .type_role(META)
                            .text_color(k.ink_3)
                            .child(group),
                    )
                    .on_click(move |_, _, cx| {
                        let (g, t) = (g.clone(), t.clone());
                        nav.update(cx, |n, cx| n.open_topic(g, t, cx))
                    })
            });
            cards.push(
                kit::side_card(cx)
                    .child(kit::card_label(c::ARTIFACT_CITED_BY, None, cx).h(px(18.)))
                    .child(
                        div()
                            .mt(space::S6)
                            .mx(-space::S6)
                            .flex()
                            .flex_col()
                            .gap(space::S2)
                            .children(rows),
                    )
                    .into_any_element(),
            );
        }
        // The inspector (NATIVE.md §2): 280 pt, edge to edge, its content below the 52 pt
        // toolbar row; no heading (the toolbar's `sidebar.trailing` toggles it).
        crate::ui::native::inspector_column(cx)
            .child(
                div()
                    .id("artifact-story")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .pt(crate::ui::native::metrics::TOOLBAR_ROW)
                    .px(space::S16)
                    .pb(space::S24)
                    .child(ui::rail_stack(
                        cards.into_iter().enumerate().map(|(i, el)| {
                            ui::motion::rise(
                                SharedString::from(format!("artifact-side-{i}")),
                                i + 1,
                                div().child(el),
                                cx,
                            )
                        }),
                        cx,
                    )),
            )
            .into_any_element()
    }

    /// Every 产出, newest first (the 产出 destination of the rail).
    /// The 产出 list, Finder-like (NATIVE.md §9.6): the toolbar holds the
    /// title and count, a segmented 图标 / 列表 switch, the sort pop-up and
    /// search. Icons: 160 pt thumbnails, radius 8, the name under them and
    /// `类型 · 时间` in `secondary`. List: columns 名称, 类型, 话题, 时间.
    fn list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        use crate::ui::native::{self as nk, NativeType as _};
        use gpui::AppContext as _;
        let n = nk::native_colors(cx);
        let p = self.services.provider.clone();
        let now = p.clock_now();
        let search = self
            .search
            .get_or_insert_with(|| {
                let state = cx.new(|cx| {
                    gpui_component::input::InputState::new(window, cx)
                        .placeholder(crate::copy::common::SEARCH_PLACEHOLDER)
                });
                cx.observe(&state, |_, _, cx| cx.notify()).detach();
                state
            })
            .clone();
        let query = search.read(cx).value().trim().to_lowercase();
        let total = p.all_artifacts().len();
        let mut all: Vec<_> = p
            .all_artifacts()
            .into_iter()
            .filter(|(_, a)| query.is_empty() || a.title.to_lowercase().contains(&query))
            .collect();
        match self.sort {
            ListSort::Time => all.sort_by(|x, y| y.1.updated_at.cmp(&x.1.updated_at)),
            ListSort::Name => all.sort_by(|x, y| x.1.title.cmp(&y.1.title)),
            ListSort::Kind => all.sort_by(|x, y| {
                kind_label(x.1.kind)
                    .cmp(kind_label(y.1.kind))
                    .then(y.1.updated_at.cmp(&x.1.updated_at))
            }),
        }
        let this = cx.entity().downgrade();
        // Finder's view control (NATIVE.md §9.6, §5.1): `square.grid.2x2` / `list.bullet`
        // in one toolbar group, the shown view on the toggled pill.
        let active = nk::emphasized(window);
        let view_item = |grid: bool, cx: &mut Context<Self>| {
            let on = self.grid == grid;
            sym_toolbar_item(
                if grid {
                    "artifacts-view-grid"
                } else {
                    "artifacts-view-list"
                },
                if grid { Sym::Grid } else { Sym::List },
                if grid {
                    c::ARTIFACTS_VIEW_GRID
                } else {
                    c::ARTIFACTS_VIEW_LIST
                },
                on,
                active,
                cx,
            )
            .on_click(cx.listener(move |v, _, _, cx| {
                v.grid = grid;
                cx.notify();
            }))
        };
        let views = nk::ToolbarGroup::new([view_item(true, cx), view_item(false, cx)]);
        // The sort pop-up as a toolbar item (`arrow.up.arrow.down`): its menu checks the
        // order; the tooltip names it.
        let sort = sym_toolbar_item(
            "artifacts-sort",
            Sym::Sort,
            self.sort.label(),
            false,
            active,
            cx,
        )
        .dropdown_menu_with_anchor(gpui::Anchor::TopRight, {
            let (this, current) = (this.clone(), self.sort);
            move |mut menu, _, _| {
                for s in ListSort::ALL {
                    let this = this.clone();
                    menu = menu.item(
                        PopupMenuItem::new(s.label())
                            .checked(s == current)
                            .on_click(move |_, _, cx| {
                                let _ = this.update(cx, |v, cx| {
                                    v.sort = s;
                                    cx.notify();
                                });
                            }),
                    );
                }
                menu
            }
        });
        let leading = content_leading(&self.services, cx);
        let toolbar = nk::ToolbarRow::new(c::ARTIFACTS_TITLE)
            .subtitle(fill(c::ARTIFACTS_COUNT, &[("n", &total.to_string())]))
            .leading_inset(px(0.))
            .group(views)
            .group(nk::toolbar_slot(sort))
            .group(
                nk::SearchField::new("artifacts-search", &search)
                    .sidebar()
                    .width(px(200.)),
            );
        if total == 0 {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .pl(leading)
                .pr(space::S12)
                .child(toolbar)
                .child(stub_body(Sym::Outputs, c::ARTIFACTS_EMPTY, None, cx))
                .into_any_element();
        }
        let group_name = |g: &str| {
            p.groups()
                .into_iter()
                .find(|x| x.id == g)
                .map(|x| x.name)
                .unwrap_or_default()
        };
        let topic_name = |g: &str, t: &str| p.topic(g, t).map(|t| t.title).unwrap_or_default();
        let open = |id: String, nav: gpui::Entity<crate::app::Navigation>| {
            move |e: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                // Finder: a click selects, a double click opens.
                let _ = e;
                let id = id.clone();
                nav.update(cx, |nav, cx| nav.open_artifact(id, None, cx))
            }
        };
        let body: AnyElement = if all.is_empty() {
            div()
                .pt(px(80.))
                .flex()
                .justify_center()
                .n_body()
                .text_color(n.secondary)
                .child(c::ARTIFACTS_NO_MATCH)
                .into_any_element()
        } else if self.grid {
            let thumb_w = px(160.);
            let cells = all.iter().map(|(_g, a)| {
                let selected = self.selected.as_deref() == Some(a.id.as_str());
                let id = a.id.clone();
                let nav = self.services.nav.clone();
                let this = this.clone();
                let opener = open(a.id.clone(), nav);
                div()
                    .id(SharedString::from(format!("artifact-{}", a.id)))
                    .w(thumb_w + px(8.))
                    .p(px(4.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(6.))
                    // The 160 pt icon well (radius 8): the page's miniature
                    // in it, like a document icon in Finder; selection
                    // fills the well.
                    .child(thumbnail(&self.services, a, selected, cx))
                    .child(
                        div()
                            .max_w(thumb_w)
                            .px(px(4.))
                            .rounded(px(4.))
                            .when(selected, |d| d.bg(n.accent).text_color(n.on_accent))
                            .n_body()
                            .text_color(if selected { n.on_accent } else { n.label })
                            .one_line()
                            .child(a.title.clone()),
                    )
                    .child(
                        div()
                            .max_w(thumb_w)
                            .n_subheadline()
                            .n_figures()
                            .text_color(n.secondary)
                            .one_line()
                            .child(fill(
                                c::ARTIFACT_KIND_TIME,
                                &[
                                    ("kind", kind_label(a.kind)),
                                    ("time", &short_time(now, a.updated_at)),
                                ],
                            )),
                    )
                    .on_click(move |e, w, cx| {
                        if e.click_count() >= 2 {
                            opener(e, w, cx);
                        } else {
                            let id = id.clone();
                            let _ = this.update(cx, |v, cx| {
                                v.selected = Some(id);
                                cx.notify();
                            });
                        }
                    })
                    .context_menu({
                        let (services, a) = (self.services.clone(), a.clone());
                        move |menu, _, _| artifact_menu(menu, &services, &a)
                    })
            });
            div()
                .flex()
                .flex_wrap()
                .gap_x(px(16.))
                .gap_y(px(12.))
                .children(cells)
                .into_any_element()
        } else {
            let col = |text: &'static str, w: Option<Pixels>| {
                div()
                    .map(|d| match w {
                        Some(w) => d.flex_none().w(w),
                        None => d.flex_1().min_w(px(0.)),
                    })
                    .n_subheadline()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(n.secondary)
                    .child(text)
            };
            let (kind_w, topic_w, time_w) = (px(80.), px(200.), px(96.));
            let header = div()
                .h(px(28.))
                .mx(nk::metrics::LIST_ROW_INSET)
                .px(px(8.))
                .flex()
                .items_center()
                .gap(px(8.))
                .border_b_1()
                .border_color(n.separator)
                .child(col(c::ARTIFACTS_COL_NAME, None))
                .child(col(c::ARTIFACTS_COL_KIND, Some(kind_w)))
                .child(col(c::ARTIFACTS_COL_TOPIC, Some(topic_w)))
                .child(col(c::ARTIFACTS_COL_TIME, Some(time_w)));
            let active = nk::emphasized(window);
            let rows = all.iter().map(|(g, a)| {
                let selected = self.selected.as_deref() == Some(a.id.as_str());
                let ink = nk::CompactRow::selection_ink(selected, true, active, cx);
                let sub = ink.map(|i| i.1).unwrap_or(n.secondary);
                let id = a.id.clone();
                let this = this.clone();
                let opener = open(a.id.clone(), self.services.nav.clone());
                let topic = topic_name(g, &a.topic_id);
                let topic = if topic.trim().is_empty() {
                    group_name(g)
                } else {
                    topic
                };
                div()
                    .context_menu({
                        let (services, a) = (self.services.clone(), a.clone());
                        move |menu, _, _| artifact_menu(menu, &services, &a)
                    })
                    .child(
                        nk::CompactRow::new(SharedString::from(format!("artifact-{}", a.id)))
                            .selected(selected)
                            .on_click(move |e, w, cx| {
                                if e.click_count() >= 2 {
                                    opener(e, w, cx);
                                } else {
                                    let id = id.clone();
                                    let _ = this.update(cx, |v, cx| {
                                        v.selected = Some(id);
                                        cx.notify();
                                    });
                                }
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        kind_sym(a.kind)
                                            .inline()
                                            .text_color(ink.map(|i| i.1).unwrap_or(n.secondary)),
                                    )
                                    .child(div().min_w(px(0.)).one_line().child(a.title.clone())),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .w(kind_w)
                                    .one_line()
                                    .text_color(sub)
                                    .child(kind_label(a.kind)),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .w(topic_w)
                                    .one_line()
                                    .text_color(sub)
                                    .child(topic),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .w(time_w)
                                    .n_figures()
                                    .text_color(sub)
                                    .child(short_time(now, a.updated_at)),
                            ),
                    )
            });
            div()
                .flex()
                .flex_col()
                .mx(-nk::metrics::LIST_ROW_INSET)
                .child(header)
                .child(div().pt(px(4.)).flex().flex_col().children(rows))
                .into_any_element()
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .pl(leading)
            .pr(space::S12)
            .child(toolbar)
            .child(
                div()
                    .id("artifacts-list")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .pr(space::S8)
                    .pt(space::S8)
                    .pb(space::S24)
                    .child(body),
            )
            .into_any_element()
    }
}

impl Render for ArtifactPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let Some((id, version)) = self.open(cx) else {
            return self.list(window, cx);
        };
        let Some(detail) = self.services.provider.artifact(&id) else {
            return stub_body(Sym::Outputs, c::ARTIFACTS_EMPTY, None, cx).into_any_element();
        };
        let shown = version
            .filter(|v| detail.versions.iter().any(|x| x.version == *v))
            .unwrap_or(detail.summary.version);
        crate::selftest::drawn::mark(
            "artifact-page",
            format!("{} v{shown}", detail.summary.title),
        );
        let header = self.header(&detail, shown, window, cx);
        let frame = self.frame(&detail, shown, window, cx);
        let aside = self.aside_open.then(|| self.story(&detail, shown, cx));
        div()
            .size_full()
            .flex()
            .bg(k.bg)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(header)
                    .child(frame),
            )
            .children(aside)
            .into_any_element()
    }
}

// --- v12/fx-cards: the 产出 list's toolbar items and thumbnails ---

/// Where content starts after the sidebar: the toolbar title, list rows and the page
/// (NATIVE.md §2: 20 pt), or after the traffic lights when the sidebar is hidden.
fn content_leading(services: &Services, cx: &App) -> Pixels {
    crate::ui::native::metrics::TOOLBAR_LEADING.max(kit::leading(services, cx))
}

/// The symbol of a 产出's kind (NATIVE.md §6).
fn kind_sym(kind: crate::data::ArtifactKind) -> Sym {
    use crate::data::ArtifactKind as K;
    match kind {
        K::Page | K::Decision => Sym::Document,
        K::File => Sym::File,
        K::Image => Sym::Image,
    }
}

/// A toolbar item drawn with a concept symbol (D4: a 32 ghost square): `label` (or `secondary` in
/// an inactive window), on a 28 pt `selection` pill with the symbol in `accent` when
/// toggled. Like `native::toolbar_item`, for symbols outside its comparison set.
fn sym_toolbar_item(
    id: &'static str,
    sym: Sym,
    name: &'static str,
    toggled: bool,
    active: bool,
    cx: &App,
) -> gpui_component::button::Button {
    crate::ui::native::toolbar_item_sym(id, sym, name, toggled, active, cx)
}

/// A 160 × 120 pt thumbnail, radius 8 (§9.6): the rendered page once it was on screen,
/// a poster for media, else the kind symbol on a quiet well. Never a miniature table.
/// Selection draws a `selection` ring outside it, like Finder's icon view.
fn thumbnail(
    services: &Services,
    a: &crate::data::ArtifactSummary,
    selected: bool,
    cx: &App,
) -> gpui::Div {
    use crate::ui::native as nk;
    use gpui::StyledImage as _;
    let n = nk::native_colors(cx);
    let picture: Option<gpui::ImageSource> = crate::ui::web_embed::page_preview(&a.id)
        .map(gpui::ImageSource::from)
        .or_else(|| {
            services
                .provider
                .artifact_poster(&a.id, a.version)
                .map(gpui::ImageSource::from)
        });
    let well = div()
        .w(px(160.))
        .h(px(120.))
        .rounded(px(8.))
        .overflow_hidden()
        .bg(n.fill)
        .border_1()
        .border_color(n.separator)
        .flex()
        .items_center()
        .justify_center();
    let well = match picture {
        Some(src) => well.child(
            gpui::img(src)
                .size_full()
                .object_fit(gpui::ObjectFit::Cover),
        ),
        None => well.child(
            kind_sym(a.kind)
                .icon(crate::ui::sf_symbols::SymSize::new(
                    32.,
                    40.,
                    crate::ui::sf_symbols::Weight::Light,
                ))
                .text_color(n.tertiary),
        ),
    };
    div()
        .p(px(3.))
        .rounded(px(11.))
        .when(selected, |d| d.bg(n.selection))
        .child(well)
}
// --- end v12/fx-cards ---

// --- v12/behavior ---
/// The context menu of a 产出 row: 打开, 在新窗口打开 and 分享 (it copies
/// the line the 分享 button copies), as on the 产出's own page (§7.3).
fn artifact_menu(
    menu: PopupMenu,
    services: &Services,
    a: &crate::data::ArtifactSummary,
) -> PopupMenu {
    let (nav, id) = (services.nav.clone(), a.id.clone());
    let (provider, window_id) = (services.provider.clone(), a.id.clone());
    let share_text = fill(
        c::ARTIFACT_SHARE_TEXT,
        &[("title", &a.title), ("v", &a.version.to_string())],
    );
    menu.item(
        PopupMenuItem::new(c::ARTIFACT_OPEN).on_click(move |_, _, cx| {
            let id = id.clone();
            nav.update(cx, |nav, cx| nav.open_artifact(id, None, cx))
        }),
    )
    .item(
        PopupMenuItem::new(c::ARTIFACT_OPEN_WINDOW).on_click(move |_, _, cx| {
            crate::views::artifacts::window::open_artifact(&provider, &window_id, None, cx)
        }),
    )
    .separator()
    .item(
        PopupMenuItem::new(c::ARTIFACT_SHARE).on_click(move |_, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(share_text.clone()))
        }),
    )
}
// --- end v12/behavior ---
