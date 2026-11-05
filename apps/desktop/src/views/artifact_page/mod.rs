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

use crate::ui::native::FocusRow as _;
use crate::ui::native::tokens::space as tsp;
use crate::{
    app::{Place, Services},
    copy::{artifact_page as c, fill},
    data::{ArtifactDetail, ArtifactFolder, FileError, Millis, TopicStatus},
    theme::{self, Type as _, space},
    ui::{
        MemberAvatar, Sym,
        native::page,
        web_embed::{WebEmbed, WebEmbedSpec},
    },
    views::{
        group_page::{kind_label, kit, short_time},
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

/// The columns of the 产出 table (§2.18): 名称, 类型, 话题, 做的人, 版本,
/// 更新, ⋯. 名称 and 话题 share the free width; numbers keep their room
/// (§2.7). The loading skeleton draws the same columns.
fn table_cols() -> Vec<page::Col> {
    vec![
        page::Col {
            header: c::ARTIFACTS_COL_NAME.into(),
            width: None,
            right: false,
        },
        page::Col {
            header: c::ARTIFACTS_COL_KIND.into(),
            width: Some(px(72.)),
            right: false,
        },
        page::Col {
            header: c::ARTIFACTS_COL_TOPIC.into(),
            width: None,
            right: false,
        },
        page::Col {
            header: c::ARTIFACTS_COL_BY.into(),
            width: Some(px(136.)),
            right: false,
        },
        page::Col {
            header: c::ARTIFACTS_COL_VERSION.into(),
            width: Some(px(56.)),
            right: true,
        },
        page::Col {
            header: c::ARTIFACTS_COL_UPDATED.into(),
            width: Some(px(96.)),
            right: true,
        },
        page::Col {
            header: SharedString::default(),
            width: Some(tsp::S48),
            right: true,
        },
    ]
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
    // --- end track cards ---
    /// The width of the 产出 list's content, measured in the last frame (the icon
    /// view's column count).
    list_width: Rc<Cell<Pixels>>,
    /// The table footer's page (1-based) and rows per page (§2.7).
    page: usize,
    per_page: usize,
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
            // A table first (§2.18); the icon view is one click away.
            grid: false,
            sort: ListSort::Time,
            search: None,
            list_width: Rc::new(Cell::new(px(0.))),
            page: 1,
            per_page: page::PER_PAGE_CHOICES[0],
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
        use gpui_component::button::ButtonVariants as _;
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
        // The version pop-up as a toolbar item (R12): the same capsule,
        // height and shape as the toolbar groups beside it.
        let picker = nk::popup_button(
            "artifact-version",
            format!("v{shown} · {}", version_time(now, at)),
            nk::PushSize::Regular,
            window,
            cx,
        )
        .custom(nk::borderless_variant(cx))
        .h(nk::metrics::TOOLBAR_ITEM)
        .px(nk::metrics::FIELD_PAD)
        .rounded(nk::metrics::BUTTON_REGULAR_RADIUS)
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
            .group(crate::views::group_page::rows::toolbar_capsule(picker, cx))
            .group(nk::ToolbarGroup::new([share, open_window, side_chat]))
            .group(crate::views::group_page::rows::toolbar_separator(cx))
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
        // An older version: an Alert at the top of the content (§2.16).
        let band = old.then(|| {
            let at = detail
                .versions
                .iter()
                .find(|v| v.version == shown)
                .map(|v| v.created_at)
                .unwrap_or(detail.summary.updated_at);
            let nav = self.services.nav.clone();
            let id = detail.summary.id.clone();
            let mut parts = vec![format!("v{shown}"), version_time(now, at)];
            parts.extend(version_note(&self.services, detail, shown));
            div()
                .flex_none()
                .px(page::m::EDGE)
                .pt(page::m::GUTTER)
                .child(page::page_alert(
                    "artifact-old-version",
                    page::AlertKind::Info,
                    fill(c::ARTIFACT_OLD_VERSION_TITLE, &[("v", &shown.to_string())]),
                    Some(parts.join(" · ").into()),
                    Some(
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
                        })
                        .into_any_element(),
                    ),
                    cx,
                ))
        });
        let folder = self
            .folders
            .get(&self.services, &detail.summary.id, shown, cx);
        // The page fills the content area under the toolbar row and the
        // Alert; what it may do is in the inspector's `这一页` (no footer
        // strip that cuts the page, F37).
        let height = window.viewport_size().height - crate::ui::native::metrics::TOOLBAR_ROW;
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
            .fill()
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
            .min_h_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .children(band)
            .child(div().flex_1().min_h_0().flex().flex_col().child(body))
            .into_any_element()
    }

    /// The inspector (COMPLETENESS.md §2.5): a header band (`产出信息`, a
    /// line, close) on the toolbar row, then fixed sections in this order:
    /// 来自话题 (with 做的人 and 要求), 版本, 这一页 (a live page), 页面里的回应,
    /// 用到的决定 and 被引用. A section with no data shows one muted line and
    /// never disappears. Values truncate with a tooltip (the role first).
    fn story(
        &self,
        detail: &ArtifactDetail,
        shown: u32,
        live: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use crate::ui::native::{
            self as nk, InspectorSection, NativeType as _, native_colors, page::desc_row,
        };
        use crate::views::group_page::rows::line;
        let k = native_colors(cx);
        let p = self.services.provider.clone();
        let now = p.clock_now();
        let story = p.artifact_story(&detail.summary.id).unwrap_or_default();
        let tip = |text: String| {
            move |window: &mut Window, cx: &mut App| {
                gpui_component::tooltip::Tooltip::new(text.clone()).build(window, cx)
            }
        };

        let close = nk::borderless_button(
            "artifact-info-close",
            crate::ui::sf_symbols::Sf::Close,
            c::ARTIFACT_CLOSE_INFO,
            window,
            cx,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.aside_open = false;
            cx.notify();
        }));
        let header = div()
            .flex_none()
            .h(nk::metrics::TOOLBAR_ROW)
            .pl(nk::metrics::INSPECTOR_PAD)
            .pr(tsp::S8)
            .border_b_1()
            .border_color(k.separator)
            .flex()
            .items_center()
            .gap(tsp::S8)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .n_title3()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(k.label)
                            .child(c::ARTIFACT_INFO),
                    )
                    .child(
                        line(c::ARTIFACT_INFO_LINE)
                            .n_subheadline()
                            .text_color(k.secondary),
                    ),
            )
            .child(close);
        let mut body = div()
            .id("artifact-story")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .pb(tsp::S24)
            .flex()
            .flex_col()
            // Pull the first GroupLabel up so its ink starts at y 68 (B2):
            // the section's own 16 top sits under the header band.
            .child(div().flex_none().mt(-tsp::S12));

        // 来自话题, then 做的人 and 要求 as a DescriptionList.
        let topic = story
            .topic
            .clone()
            .or_else(|| Some((detail.group_id.clone(), detail.summary.topic_id.clone())))
            .and_then(|(g, t)| p.topic(&g, &t));
        let made_by = story
            .made_by
            .clone()
            .unwrap_or_else(|| detail.summary.created_by.clone());
        // A person: avatar 16 + the name (keeps its room) + the role, which
        // truncates first; both in the tooltip.
        let person = |id: &str| {
            let m = p.member(id);
            // X-01: no leading emoji beside the avatar.
            let name = m
                .as_ref()
                .map(|m| crate::ui::avatar::name_beside_avatar(&m.name).to_string())
                .unwrap_or_default();
            let role = m.as_ref().and_then(|m| m.role.clone()).unwrap_or_default();
            let full = if role.is_empty() {
                name.clone()
            } else {
                format!("{name} · {role}")
            };
            div()
                .id(SharedString::from(format!("artifact-person-{id}")))
                .w_full()
                .flex()
                .items_center()
                .gap(tsp::S8)
                .child(MemberAvatar::new(m.as_ref(), tsp::S16))
                // The role gives way first (§2.5): it shows only when at
                // least a few glyphs of it fit beside the whole name, never
                // as a lone "…"; the tooltip holds both.
                .when(role_fits(&name), |d| {
                    d.child(
                        line(name.clone())
                            .flex_none()
                            .max_w(gpui::relative(0.7))
                            .text_color(k.label),
                    )
                    .child(line(role).flex_1().min_w_0().text_color(k.secondary))
                })
                .when(!role_fits(&name), |d| {
                    d.child(line(name).flex_1().min_w_0().text_color(k.label))
                })
                .tooltip(tip(full))
        };
        let mut from = InspectorSection::new("artifact-ins-topic", c::ARTIFACT_FROM_TOPIC);
        if let Some(t) = topic {
            let group = p
                .groups()
                .into_iter()
                .find(|g| g.id == t.group_id)
                .map(|g| g.name)
                .unwrap_or_default();
            let done = t.status == TopicStatus::Done;
            // §2.12 one mapping (`topic_state`): asks to me first, 在做 only
            // with a run, as the sidebar and the 话题 header say.
            let state = crate::views::topic_page::topic_state(
                p.as_ref(),
                self.services.me().as_deref(),
                &t,
            );
            let meta = if done {
                fill(
                    c::ARTIFACT_TOPIC_DONE,
                    &[
                        ("group", &group),
                        ("time", &short_time(now, t.last_activity_at)),
                    ],
                )
            } else {
                fill(
                    c::ARTIFACT_TOPIC_ACTIVE,
                    &[
                        ("group", &group),
                        (
                            "state",
                            if state.short.is_empty() {
                                &state.text
                            } else {
                                &state.short
                            },
                        ),
                    ],
                )
            };
            let title = if t.title.trim().is_empty() {
                c::ARTIFACT_NO_TITLE.to_string()
            } else {
                t.title.clone()
            };
            // The state is an icon with its word in the meta line (§2.12),
            // never a free dot.
            let (sym, color) = if done {
                (Sym::Done, k.green)
            } else {
                (
                    state.badge.symbol().unwrap_or(Sym::Waiting),
                    state.badge.color(&k),
                )
            };
            let nav = self.services.nav.clone();
            let (g, id) = (t.group_id.clone(), t.id.clone());
            from = from.row(
                div()
                    .id("artifact-from-topic")
                    .focus_row(false, cx)
                    .min_h(tsp::S48)
                    .py(tsp::S6)
                    .flex()
                    .items_center()
                    .gap(tsp::S8)
                    .child(page::row_icon(sym, color))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(line(title.clone()).n_body().text_color(k.label))
                            .child(line(meta.clone()).n_subheadline().text_color(k.secondary)),
                    )
                    .child(page::row_icon(Sym::ChevronRight, k.secondary))
                    .tooltip(tip(format!("{title} · {meta}")))
                    .on_click(move |_, _, cx| {
                        let (g, id) = (g.clone(), id.clone());
                        nav.update(cx, |n, cx| n.open_topic(g, id, cx))
                    }),
            );
        }
        from = from.row(desc_row(c::ARTIFACT_MADE_BY, person(&made_by), cx));
        from = from.row(desc_row(
            c::ARTIFACT_ASKED_BY,
            match &story.asked_by {
                Some(asked) => person(asked).into_any_element(),
                None => div()
                    .text_color(k.secondary)
                    .child(c::ARTIFACT_NOBODY)
                    .into_any_element(),
            },
            cx,
        ));
        body = body.child(from);

        // 版本.
        let id = detail.summary.id.clone();
        let mut versions = InspectorSection::new("artifact-ins-versions", c::ARTIFACT_VERSIONS);
        if shown > 1 {
            let nav = self.services.nav.clone();
            let id = id.clone();
            versions = versions.action(
                nk::push_button(
                    "artifact-compare",
                    fill(c::ARTIFACT_COMPARE, &[("v", &(shown - 1).to_string())]),
                    nk::PushStyle::Outline,
                    nk::PushSize::Small,
                    cx,
                )
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    let id = id.clone();
                    nav.update(cx, |n, cx| n.open_artifact(id, Some(shown - 1), cx))
                }),
            );
        }
        for v in &detail.versions {
            let on = v.version == shown;
            let nav = self.services.nav.clone();
            let (id, ver) = (id.clone(), v.version);
            let latest = detail.summary.version;
            let note = version_note(&self.services, detail, v.version);
            let note_text = note
                .clone()
                .unwrap_or_else(|| c::ARTIFACT_NO_NOTE.to_string());
            let when = version_time(now, v.created_at);
            versions = versions.row(
                div()
                    .id(SharedString::from(format!("artifact-v{}", v.version)))
                    .focus_row(v.version == shown, cx)
                    .h(tsp::S32)
                    .mx(-tsp::S8)
                    .px(tsp::S8)
                    .rounded(tsp::S6)
                    .flex()
                    .items_center()
                    .gap(tsp::S8)
                    .n_body()
                    .when(on, |el| el.bg(k.fill).font_weight(gpui::FontWeight::MEDIUM))
                    .child(
                        div()
                            .flex_none()
                            .w(tsp::S24)
                            .n_subheadline()
                            .n_figures()
                            .text_color(if on { k.label } else { k.secondary })
                            .child(format!("v{}", v.version)),
                    )
                    .child(
                        line(note_text.clone())
                            .flex_1()
                            .text_color(if note.is_some() { k.label } else { k.secondary }),
                    )
                    .child(
                        div()
                            .flex_none()
                            .n_subheadline()
                            .n_figures()
                            .text_color(k.secondary)
                            .child(when.clone()),
                    )
                    .tooltip(tip(format!("v{} · {note_text} · {when}", v.version)))
                    .on_click(move |_, _, cx| {
                        let id = id.clone();
                        let ver = (ver != latest).then_some(ver);
                        nav.update(cx, |n, cx| n.open_artifact(id, ver, cx))
                    }),
            );
        }
        body = body.child(versions);

        // 这一页: what the live page may do and its last request.
        if live {
            let mut page_sec = InspectorSection::new("artifact-ins-page", c::ARTIFACT_PAGE_CAN);
            for row in host::page_rows(
                "artifact-page-foot",
                self.services.provider.as_ref(),
                &detail.summary.id,
                shown,
                cx,
            ) {
                page_sec = page_sec.row(row);
            }
            body = body.child(page_sec);
        }

        // 页面里的回应: the server's page log (`GET /v1/artifacts/:id/events`).
        let responses = p.artifact_responses(&detail.summary.id);
        let responses_load = p.artifact_responses_load(&detail.summary.id);
        let has_events = detail.versions.iter().any(|v| {
            !p.artifact_page_events(&detail.summary.id, v.version)
                .is_empty()
        });
        let mut resp = InspectorSection::new("artifact-ins-responses", c::ARTIFACT_RESPONSES);
        if responses.is_empty() {
            use crate::data::pages::ResponsesLoad;
            resp = resp.row(match (has_events, responses_load) {
                (false, _) => {
                    page::compact_empty(Sym::AskAside, c::ARTIFACT_RESPONSES_NONE, None, cx)
                        .into_any_element()
                }
                (true, ResponsesLoad::Loading) => {
                    page::skeleton_rows("artifact-responses-sk", page::SkeletonKind::Compact, 2, cx)
                }
                (true, ResponsesLoad::Failed) => {
                    let services = self.services.clone();
                    let id = detail.summary.id.clone();
                    page::page_alert(
                        "artifact-responses-failed",
                        page::AlertKind::Error,
                        c::ARTIFACT_RESPONSES_FAILED,
                        None,
                        Some(
                            nk::push_button(
                                "artifact-responses-retry",
                                c::ARTIFACT_RESPONSES_RETRY,
                                nk::PushStyle::Outline,
                                nk::PushSize::Small,
                                cx,
                            )
                            .on_click(move |_, _, _| {
                                services.provider.reload_artifact_responses(&id)
                            })
                            .into_any_element(),
                        ),
                        cx,
                    )
                    .into_any_element()
                }
                (true, ResponsesLoad::Ready) => {
                    page::compact_empty(Sym::AskAside, c::ARTIFACT_RESPONSES_EMPTY, None, cx)
                        .into_any_element()
                }
            });
        }
        for (i, r) in responses.into_iter().enumerate() {
            let m = p.member(&r.member_id);
            // The row names the actor from the member record, next to their
            // avatar (the log's text is `{name} {action}` as it was sent).
            let r = crate::data::pages::ArtifactResponse {
                text: response_text(m.as_ref().map(|m| m.name.as_str()), &r.text),
                ..r
            };
            let mut row = div()
                .id(SharedString::from(format!("artifact-response-{i}")))
                .h(tsp::S32)
                .flex()
                .items_center()
                .gap(tsp::S8)
                .n_body()
                .text_color(k.label)
                .child(MemberAvatar::new(m.as_ref(), tsp::S16))
                // §2.5: the action keeps its meaning; a long name is cut
                // first (at most 120), so "选了「B 站」" always shows.
                .map(
                    |d| match response_parts(m.as_ref().map(|m| m.name.as_str()), &r.text) {
                        Some((name, action)) => d
                            .child(
                                line(crate::ui::avatar::name_beside_avatar(&name).to_string())
                                    .flex_none()
                                    .max_w(tsp::S48 * 2. + tsp::S24),
                            )
                            .child(line(action).flex_1().min_w_0()),
                        None => d.child(line(r.text.clone()).flex_1()),
                    },
                )
                .tooltip(tip(r.text.clone()));
            row = match (r.topic.clone(), r.at) {
                (Some((g, t)), _) => {
                    let nav = self.services.nav.clone();
                    row.child(page::row_icon(Sym::ChevronRight, k.secondary))
                        .on_click(move |_, _, cx| {
                            let (g, t) = (g.clone(), t.clone());
                            nav.update(cx, |n, cx| n.open_topic(g, t, cx))
                        })
                }
                (None, at) => row.children(at.map(|at| {
                    div()
                        .flex_none()
                        .n_subheadline()
                        .n_figures()
                        .text_color(k.secondary)
                        .child(short_time(now, at))
                })),
            };
            resp = resp.row(row);
        }
        body = body.child(resp);

        // 用到的决定.
        let mut decisions = InspectorSection::new("artifact-ins-decisions", c::ARTIFACT_DECISIONS);
        if story.decisions.is_empty() {
            decisions = decisions.row(page::compact_empty(
                Sym::List,
                c::ARTIFACT_DECISIONS_EMPTY,
                None,
                cx,
            ));
        }
        for (i, d) in story.decisions.iter().enumerate() {
            let group = p
                .groups()
                .into_iter()
                .find(|g| g.id == d.group_id)
                .map(|g| g.name)
                .unwrap_or_default();
            let meta = fill(
                c::ARTIFACT_DECISION_META,
                &[
                    ("name", &member_name(&self.services, &d.by)),
                    ("time", &kit::month_day(d.at)),
                    ("group", &group),
                ],
            );
            let nav = self.services.nav.clone();
            let g = d.group_id.clone();
            decisions = decisions.row(
                div()
                    .id(SharedString::from(format!("artifact-decision-{i}")))
                    .focus_row(false, cx)
                    .py(tsp::S6)
                    .flex()
                    .flex_col()
                    .gap(tsp::S2)
                    .child(kit::decision_text(&d.text, k.label))
                    .child(line(meta).n_subheadline().text_color(k.secondary))
                    .tooltip(tip(d.text.clone()))
                    .on_click(move |_, _, cx| {
                        let g = g.clone();
                        nav.update(cx, |n, cx| n.open_group(g, cx))
                    }),
            );
        }
        body = body.child(decisions);

        // 被引用.
        let mut cited = InspectorSection::new("artifact-ins-cited", c::ARTIFACT_CITED_BY);
        if story.cited_by.is_empty() {
            cited = cited.row(page::compact_empty(
                Sym::List,
                c::ARTIFACT_CITED_EMPTY,
                None,
                cx,
            ));
        }
        for (i, (g, t, title)) in story.cited_by.iter().enumerate() {
            let group = p
                .groups()
                .into_iter()
                .find(|x| &x.id == g)
                .map(|x| x.name)
                .unwrap_or_default();
            let nav = self.services.nav.clone();
            let (g, t) = (g.clone(), t.clone());
            cited = cited.row(
                div()
                    .id(SharedString::from(format!("artifact-cited-{i}")))
                    .focus_row(false, cx)
                    .h(tsp::S32)
                    .flex()
                    .items_center()
                    .gap(tsp::S8)
                    .n_body()
                    .text_color(k.label)
                    .child(page::row_icon(Sym::Topic, k.secondary))
                    .child(line(title.clone()).flex_1())
                    .child(
                        line(group.clone())
                            .flex_none()
                            .max_w(px(96.))
                            .n_subheadline()
                            .text_color(k.secondary),
                    )
                    .tooltip(tip(format!("{title} · {group}")))
                    .on_click(move |_, _, cx| {
                        let (g, t) = (g.clone(), t.clone());
                        nav.update(cx, |n, cx| n.open_topic(g, t, cx))
                    }),
            );
        }
        body = body.child(cited);

        nk::inspector_column(cx)
            .child(header)
            .child(body)
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
        use gpui_component::Disableable as _;
        let n = nk::native_colors(cx);
        let p = self.services.provider.clone();
        let now = p.clock_now();
        let fresh = self.search.is_none();
        let start = fresh.then(scenes::take_list_start).flatten();
        if let Some(st) = &start {
            self.grid = st.grid;
        }
        let search = self
            .search
            .get_or_insert_with(|| {
                let state = cx.new(|cx| {
                    let s = gpui_component::input::InputState::new(window, cx)
                        .placeholder(crate::copy::common::SEARCH_PLACEHOLDER);
                    match start.as_ref().and_then(|st| st.query) {
                        Some(q) => s.default_value(q),
                        None => s,
                    }
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
        // §2.7: the table pages; the footer moves between pages and picks
        // the rows per page. A search or a smaller list keeps a valid page.
        let pages = all.len().div_ceil(self.per_page).max(1);
        self.page = self.page.clamp(1, pages);
        let matched = all.len();
        let all: Vec<_> = all
            .into_iter()
            .skip((self.page - 1) * self.per_page)
            .take(self.per_page)
            .collect();
        let this = cx.entity().downgrade();
        let pager = {
            let (go_to, set) = (this.clone(), this.clone());
            page::Pager {
                page: self.page,
                pages,
                go: Some(Rc::new(move |to: usize, _: &mut Window, cx: &mut App| {
                    let _ = go_to.update(cx, |v, cx| {
                        v.page = to;
                        cx.notify();
                    });
                })),
                per_page: self.per_page,
                set_per_page: Some(Rc::new(move |n: usize, _: &mut Window, cx: &mut App| {
                    let _ = set.update(cx, |v, cx| {
                        v.per_page = n;
                        v.page = 1;
                        cx.notify();
                    });
                })),
            }
        };
        let active = nk::emphasized(window);
        // The view control is one Segmented h32 (§2.2, §1.8): 图标 | 列表.
        // With nothing to show it is disabled at 50 % (§1.8).
        let empty = total == 0;
        let views = div().flex_none().when(empty, |d| d.opacity(0.5)).child(
            nk::Segmented::new(
                "artifacts-view",
                [
                    nk::Segment::Label(c::ARTIFACTS_VIEW_GRID.into()),
                    nk::Segment::Label(c::ARTIFACTS_VIEW_LIST.into()),
                ],
            )
            .selected(if self.grid { 0 } else { 1 })
            .disabled(empty)
            .on_change({
                let this = this.clone();
                move |ix, _, cx| {
                    let _ = this.update(cx, |v, cx| {
                        v.grid = ix == 0;
                        cx.notify();
                    });
                }
            }),
        );
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
        .disabled(empty)
        .when(empty, |b| b.opacity(0.5))
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
        let load = page::PageLoad::of(&self.services, total > 0);
        // The toolbar's last control ends on the content edge (x - 24), like the
        // table under it (§2.2): an empty slot after it takes the 8 the toolbar's
        // own inset (16) lacks.
        let toolbar_row = nk::ToolbarRow::new(c::ARTIFACTS_TITLE)
            .leading_inset(leading)
            // The subtitle slot stays in every state, so the title never moves.
            .subtitle(match load {
                page::PageLoad::Ready => fill(c::ARTIFACTS_COUNT, &[("n", &total.to_string())]),
                page::PageLoad::Loading => crate::copy::desk::DESK_SUBTITLE_SYNCING.to_string(),
                page::PageLoad::Failed => crate::copy::desk::DESK_SUBTITLE_OFFLINE.to_string(),
            })
            .group(views)
            .group(nk::toolbar_slot(sort))
            .group(
                nk::SearchField::new("artifacts-search", &search)
                    .sidebar()
                    .clearable()
                    .width(px(200.)),
            )
            .group(div().flex_none().w(px(0.)));
        // ToolbarRow draws the one 1 px line at y 52 itself (§2.2).
        let toolbar = div().flex_none().child(toolbar_row);
        let shell = |body: AnyElement| {
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(toolbar)
                .child(
                    div()
                        .id("artifacts-list")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(
                            // Frames hug their content; space below the last
                            // frame is allowed (A1).
                            div()
                                .min_h_full()
                                .px(page::m::EDGE)
                                .pt(page::m::GUTTER)
                                .pb(page::m::GUTTER)
                                .flex()
                                .flex_col()
                                .gap(page::m::GUTTER)
                                .child(body),
                        ),
                )
                .into_any_element()
        };
        match load {
            page::PageLoad::Failed => {
                // The failure takes the content's place (§2.15).
                return shell(
                    div()
                        .flex()
                        .flex_col()
                        .child(crate::views::group_page::rows::failed_region(
                            "artifacts",
                            &self.services,
                            cx,
                        ))
                        .into_any_element(),
                );
            }
            page::PageLoad::Loading => {
                return shell(page::skeleton_table("artifacts-sk", &table_cols(), 5, cx));
            }
            page::PageLoad::Ready if total == 0 => {
                return shell(
                    div()
                        .flex_none()
                        .flex()
                        .rounded(page::m::FRAME_RADIUS)
                        .border_1()
                        .border_color(n.separator)
                        .child(page::page_empty(
                            Sym::Outputs,
                            c::ARTIFACTS_EMPTY,
                            c::ARTIFACTS_EMPTY_BODY,
                            // A verb (§2.13): 产出 come out of 话题.
                            vec![
                                nk::push_button(
                                    "artifacts-empty-new",
                                    crate::copy::desk::DESK_NEW_TOPIC,
                                    nk::PushStyle::Default,
                                    nk::PushSize::Small,
                                    cx,
                                )
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(crate::app::NewTopic), cx)
                                })
                                .into_any_element(),
                            ],
                            cx,
                        ))
                        .into_any_element(),
                );
            }
            page::PageLoad::Ready => {}
        }
        let group_name = |g: &str| {
            p.groups()
                .into_iter()
                .find(|x| x.id == g)
                .map(|x| x.name)
                .unwrap_or_default()
        };
        let topic_name = |g: &str, t: &str| {
            let topic = p.topic(g, t).map(|t| t.title).unwrap_or_default();
            if topic.trim().is_empty() {
                group_name(g)
            } else {
                topic
            }
        };
        let open = |id: String, nav: gpui::Entity<crate::app::Navigation>| {
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                let id = id.clone();
                nav.update(cx, |nav, cx| nav.open_artifact(id, None, cx))
            }
        };
        let footer_text = if query.is_empty() {
            fill(c::ARTIFACTS_FOOTER, &[("n", &total.to_string())])
        } else {
            fill(
                c::ARTIFACTS_FOOTER_MATCH,
                &[("n", &matched.to_string()), ("total", &total.to_string())],
            )
        };
        let cell_tip = |id: String, text: String, muted: bool, cx: &App| {
            let tip = text.clone();
            div()
                .id(SharedString::from(id))
                .min_w_0()
                .child(page::cell_text(text, muted, cx))
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                })
                .into_any_element()
        };
        // §2.7 table footer: the count, 每页, 第 n / m 页 and the four
        // page buttons (the same footer under the icon view).
        let footer = page::table_footer("artifacts-footer", footer_text, pager, cx);
        let body: AnyElement = if self.grid && !all.is_empty() {
            // Icon view (§2.18): one group per day (今天, 昨天, 11 月 1 日),
            // equal cards in a grid, gap 16. The column count follows the
            // measured width (tiles at least 220 wide), so the tiles use the
            // whole width.
            let width = match self.list_width.get() {
                w if w > px(0.) => w,
                // Before the first measure: the window less the sidebar.
                _ => window.viewport_size().width - px(248. + 16. + 48.),
            };
            // Never more columns than tiles, so a short list still fills the width.
            let cols = (((f32::from(width) + 16.) / (TILE_MIN + 16.))
                .floor()
                .max(1.) as u16)
                .min(all.len().max(1) as u16);
            let mut days: Vec<(String, Vec<&(String, crate::data::ArtifactSummary)>)> = Vec::new();
            for item in &all {
                let day = day_label(now, item.1.updated_at);
                match days.iter_mut().find(|(d, _)| *d == day) {
                    Some((_, list)) => list.push(item),
                    None => days.push((day, vec![item])),
                }
            }
            let measured = self.list_width.clone();
            let probe = gpui::canvas(
                move |bounds, window, _| {
                    if (bounds.size.width - measured.get()).abs() > px(0.5) {
                        measured.set(bounds.size.width);
                        window.refresh();
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full();
            div()
                .relative()
                .flex()
                .flex_col()
                .gap(page::m::GUTTER)
                .child(probe)
                .children(days.into_iter().map(|(day, items)| {
                    let tiles = items.into_iter().map(|(g, a)| {
                        let opener = open(a.id.clone(), self.services.nav.clone());
                        let meta = fill(
                            c::ARTIFACTS_TILE_META,
                            &[
                                ("v", &a.version.to_string()),
                                ("topic", &topic_name(g, &a.topic_id)),
                            ],
                        );
                        let full = format!("{} · {meta}", a.title);
                        page::card(cx)
                            .id(SharedString::from(format!("artifact-{}", a.id)))
                            .focus_row_with(false, crate::ui::native::Arrows::Grid, cx)
                            .min_w_0()
                            .overflow_hidden()
                            .cursor_pointer()
                            .child(thumbnail(&self.services, a, cx))
                            .child(
                                div()
                                    .p(tsp::S12)
                                    .border_t_1()
                                    .border_color(n.separator)
                                    .flex()
                                    .flex_col()
                                    .gap(tsp::S4)
                                    .child(
                                        // Up to two lines, so titles that
                                        // share a long start still differ
                                        // (F07); the meta follows the title
                                        // at gap 4 (no void between, A1).
                                        div()
                                            .n_body()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(n.label)
                                            .line_clamp(2)
                                            .child(a.title.clone()),
                                    )
                                    .child(
                                        div()
                                            .n_callout()
                                            .text_color(n.secondary)
                                            .one_line()
                                            .child(meta),
                                    ),
                            )
                            .tooltip(move |window, cx| {
                                gpui_component::tooltip::Tooltip::new(full.clone())
                                    .build(window, cx)
                            })
                            .on_click(opener)
                            .context_menu({
                                let (services, a) = (self.services.clone(), a.clone());
                                move |menu, _, _| artifact_menu(menu, &services, &a)
                            })
                    });
                    let count = tiles.len();
                    div()
                        .flex()
                        .flex_col()
                        .gap(page::m::HEADER_GAP)
                        .child(page::section_header(day, Some(count), None, None, cx))
                        .child(
                            div()
                                .grid()
                                .grid_cols(cols)
                                .gap(page::m::GRID_GAP)
                                .children(tiles),
                        )
                }))
                // The count in a footer row under the tiles (§2.6).
                .child(page::frame(cx).child(footer.border_t_0()))
                .into_any_element()
        } else {
            // Table view (default): 名称, 类型, 话题, 做的人, 版本, 更新, ⋯. 名称 and
            // 话题 share the free width; numbers keep their room (§2.7).
            let cols = table_cols();
            let rows = all
                .iter()
                .map(|(g, a)| {
                    let opener = open(a.id.clone(), self.services.nav.clone());
                    let by = p.member(&a.created_by);
                    let full = a.title.clone();
                    let name = div()
                        .id(SharedString::from(format!("artifact-name-{}", a.id)))
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(tsp::S8)
                        .child(
                            div()
                                .flex_none()
                                .child(page::row_icon(kind_sym(a.kind), n.secondary)),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .one_line()
                                .child(a.title.clone()),
                        )
                        .tooltip(move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(full.clone()).build(window, cx)
                        })
                        .into_any_element();
                    // X-01: a leading emoji in the name is not shown beside
                    // the avatar.
                    let who = crate::ui::avatar::name_beside_avatar(&member_name(
                        &self.services,
                        &a.created_by,
                    ))
                    .to_string();
                    let who_tip = who.clone();
                    let person = div()
                        .id(SharedString::from(format!("artifact-by-{}", a.id)))
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(tsp::S8)
                        .child(
                            div()
                                .flex_none()
                                .child(crate::views::group_page::rows::avatar(
                                    by.as_ref(),
                                    tsp::S16,
                                    cx,
                                )),
                        )
                        .child(page::cell_text(who, false, cx))
                        .tooltip(move |window, cx| {
                            gpui_component::tooltip::Tooltip::new(who_tip.clone()).build(window, cx)
                        })
                        .into_any_element();
                    let menu = {
                        let (services, art) = (self.services.clone(), a.clone());
                        nk::borderless_button(
                            SharedString::from(format!("artifact-more-{}", a.id)),
                            crate::ui::sf_symbols::Sf::More,
                            c::ARTIFACTS_MORE,
                            window,
                            cx,
                        )
                        .dropdown_menu_with_anchor(gpui::Anchor::TopRight, move |menu, _, _| {
                            artifact_menu(menu.min_w(px(128.)), &services, &art)
                        })
                        .into_any_element()
                    };
                    let click: Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App)> =
                        Box::new(opener);
                    (
                        vec![
                            name,
                            page::outline_badge(kind_label(a.kind), cx),
                            cell_tip(
                                format!("artifact-topic-{}", a.id),
                                topic_name(g, &a.topic_id),
                                true,
                                cx,
                            ),
                            person,
                            div()
                                .n_figures()
                                .text_color(n.secondary)
                                .child(fill(c::ARTIFACTS_VERSION, &[("v", &a.version.to_string())]))
                                .into_any_element(),
                            div()
                                .n_figures()
                                .text_color(n.secondary)
                                .child(short_time(now, a.updated_at))
                                .into_any_element(),
                            menu,
                        ],
                        Some(click),
                    )
                })
                .collect();
            // No match: the header and footer stay; one cell says so
            // (§2.7 empty).
            let no_match = all.is_empty().then(|| {
                let search = search.clone();
                let clear = nk::push_button(
                    "artifacts-clear",
                    c::ARTIFACTS_CLEAR_SEARCH,
                    nk::PushStyle::Outline,
                    nk::PushSize::Small,
                    cx,
                )
                .on_click(move |_, window, cx| {
                    search.update(cx, |s, cx| s.set_value("", window, cx));
                });
                div()
                    .h(px(96.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(tsp::S8)
                    .child(
                        div()
                            .n_body()
                            .text_color(n.secondary)
                            .child(fill(c::ARTIFACTS_NO_MATCH_TITLE, &[("q", &query)])),
                    )
                    .child(clear)
            });
            // The frame hugs the table and ends in its footer row.
            page::frame(cx)
                .child(page::table("artifacts", &cols, rows, cx))
                .children(no_match)
                .child(footer)
                .into_any_element()
        };
        shell(body)
    }
}

/// The narrowest tile of the icon view.
const TILE_MIN: f32 = 220.;

/// The day group of the icon view: `今天`, `昨天` or `11 月 1 日`.
fn day_label(now: Millis, at: Millis) -> String {
    use chrono::{Datelike as _, Local, TimeZone as _};
    let (Some(n), Some(t)) = (
        Local.timestamp_millis_opt(now).single(),
        Local.timestamp_millis_opt(at).single(),
    ) else {
        return crate::copy::common::TIME_UNKNOWN.to_string();
    };
    if at <= 0 {
        return crate::copy::common::TIME_UNKNOWN.to_string();
    }
    if t.date_naive() == n.date_naive() {
        c::ARTIFACTS_DAY_TODAY.to_string()
    } else if t.date_naive().succ_opt() == Some(n.date_naive()) {
        c::ARTIFACTS_DAY_YESTERDAY.to_string()
    } else if t.year() == n.year() {
        fill(
            c::ARTIFACTS_DAY,
            &[("m", &t.month().to_string()), ("d", &t.day().to_string())],
        )
    } else {
        crate::views::shell::day(at)
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
        let live = matches!(
            self.folders.get(&self.services, &detail.summary.id, shown, cx),
            Some(Ok(f)) if f.is_page
        );
        let aside = self
            .aside_open
            .then(|| self.story(&detail, shown, live, window, cx));
        div()
            .size_full()
            .flex()
            .bg(k.bg)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
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
pub(crate) fn kind_sym(kind: crate::data::ArtifactKind) -> Sym {
    use crate::data::ArtifactKind as K;
    match kind {
        K::Page | K::Decision => Sym::Document,
        K::File => Sym::File,
        K::Image => Sym::Image,
    }
}

/// True when a person value has room for its role beside the name: the name's
/// width (CJK about 14, other glyphs about 8 at 14 pt) plus a gap and three
/// glyphs of the role fit the 320 pane's value column after the avatar.
pub(crate) fn role_fits(name: &str) -> bool {
    let w: f32 = name
        .chars()
        .map(|c| if c.is_ascii() { 8. } else { 14. })
        .sum();
    w + 8. + 3. * 14. <= 152.
}

/// A 页面里的回应 line with the actor's current name: the log's text starts with
/// the name the actor had when they acted.
fn response_text(name: Option<&str>, text: &str) -> String {
    match name.filter(|n| !n.is_empty()) {
        Some(n) if text.starts_with(n) => text.to_string(),
        Some(n) => match text.split_once(' ') {
            Some((_, rest)) => format!("{n} {rest}"),
            None => text.to_string(),
        },
        None => text.to_string(),
    }
}

/// A 页面里的回应 line as (actor name, action) when its text starts with
/// the actor's name.
fn response_parts(name: Option<&str>, text: &str) -> Option<(String, String)> {
    let n = name.filter(|n| !n.is_empty())?;
    let rest = text.strip_prefix(n)?.trim_start();
    (!rest.is_empty()).then(|| (n.to_string(), rest.to_string()))
}

/// A 36 pt toolbar item drawn with a concept symbol (§5.1): `label` (or `secondary` in
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

/// The picture of a tile (§9.6, R11): the page's snapshot scaled to fit
/// the tile whole (never a crop of one corner), a poster for media, else
/// the kind's symbol with no frame, as Finder draws a document icon.
fn thumbnail(services: &Services, a: &crate::data::ArtifactSummary, cx: &App) -> gpui::Div {
    use crate::ui::native as nk;
    use gpui::StyledImage as _;
    let n = nk::native_colors(cx);
    let picture: Option<gpui::ImageSource> = crate::ui::web_embed::page_preview(&a.id, a.version)
        .map(gpui::ImageSource::from)
        .or_else(|| {
            services
                .provider
                .artifact_poster(&a.id, a.version)
                .map(gpui::ImageSource::from)
        });
    // The well follows the tile's top corners (inside its 1 px line), so
    // its fill never shows past the rounded border (§1.6).
    let well = div()
        .w_full()
        .h(px(132.))
        .rounded_t(page::m::CARD_RADIUS - crate::ui::native::metrics::HAIRLINE)
        .overflow_hidden()
        .bg(n.fill)
        .flex()
        .items_center()
        .justify_center();
    match picture {
        Some(src) => well.p(tsp::S8).child(
            gpui::img(src)
                .size_full()
                .object_fit(gpui::ObjectFit::Contain),
        ),
        None => well.child(
            kind_sym(a.kind)
                .icon(crate::ui::sf_symbols::SymSize::new(
                    40.,
                    48.,
                    crate::ui::sf_symbols::Weight::Light,
                ))
                .text_color(n.secondary),
        ),
    }
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
