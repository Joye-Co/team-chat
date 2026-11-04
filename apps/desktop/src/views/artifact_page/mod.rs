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
        self, Lucide, MemberAvatar, Ring,
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
pub struct Folders(Rc<RefCell<HashMap<(String, u32), Option<Result<ArtifactFolder, FileError>>>>>);

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

thread_local! {
    /// Page events by `(产出, version)`, paced for every page of the app.
    static PAGE_EVENTS: RefCell<EventLimiter<(String, u32), PendingEvent>> =
        RefCell::new(EventLimiter::new());
    /// When the next send of waiting page events runs.
    static FLUSH_AT: Cell<Option<Instant>> = const { Cell::new(None) };
    /// The page request of the emit that waits per `(产出, version, name)`.
    static QUEUED: RefCell<HashMap<(String, u32, String), (PageRef, serde_json::Value)>> =
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
    _subscriptions: Vec<Subscription>,
}

impl ArtifactPage {
    pub fn new(services: Services, cx: &mut Context<Self>) -> Self {
        Self {
            _subscriptions: watch(&services, cx),
            services,
            folders: Folders::default(),
            aside_open: true,
            copied: false,
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
    fn header(&self, detail: &ArtifactDetail, shown: u32, cx: &mut Context<Self>) -> AnyElement {
        let k = theme::colors(cx);
        let p = self.services.provider.clone();
        let now = p.clock_now();
        let a = &detail.summary;
        let group = p
            .groups()
            .into_iter()
            .find(|g| g.id == detail.group_id)
            .map(|g| g.name)
            .unwrap_or_default();
        let nav = self.services.nav.clone();
        let (gid, tid) = (detail.group_id.clone(), a.topic_id.clone());
        let crumb = div()
            .id("artifact-crumb")
            .flex_none()
            .flex()
            .items_center()
            .gap(space::S6)
            .h(px(28.))
            .px(space::S6)
            .ml(-space::S6)
            .rounded(px(7.))
            .t_small()
            .text_color(k.ink_3)
            .child(group)
            .child("/")
            .on_click(move |_, _, cx| {
                let (gid, tid) = (gid.clone(), tid.clone());
                nav.update(cx, |n, cx| n.open_topic(gid, tid, cx))
            });
        let at = detail
            .versions
            .iter()
            .find(|v| v.version == shown)
            .map(|v| v.created_at)
            .unwrap_or(a.updated_at);
        let versions = detail.versions.clone();
        let services = self.services.clone();
        let id = a.id.clone();
        let picker = ui::button(
            "artifact-version",
            "",
            ui::ButtonKind::Secondary,
            ui::ButtonSize::Md,
            cx,
        )
        .pl(space::S12)
        .pr(space::S8)
        .child(
            div()
                .flex()
                .items_center()
                .gap(space::S8)
                .t_small()
                .child(
                    div()
                        .mono()
                        .type_role(META)
                        .semibold()
                        .text_color(k.ink)
                        .child(format!("v{shown}")),
                )
                .child(
                    div()
                        .font_weight(gpui::FontWeight::NORMAL)
                        .text_color(k.ink_3)
                        .child(version_time(now, at)),
                )
                .child(ui::icon(Lucide::ChevronDown, px(14.), k.ink_3)),
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
        let share = ui::button(
            "artifact-share",
            if self.copied {
                c::ARTIFACT_COPIED
            } else {
                c::ARTIFACT_SHARE
            },
            ui::ButtonKind::Secondary,
            ui::ButtonSize::Md,
            cx,
        )
        .text_size(px(13.))
        .font_weight(gpui::FontWeight::NORMAL)
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
        kit::bar()
            .child(crumb)
            .child(
                div()
                    .ml(-space::S2)
                    .min_w_0()
                    .one_line()
                    .t_page_title()
                    .text_color(k.ink)
                    .child(a.title.clone()),
            )
            .child(
                div()
                    .ml(space::S4)
                    .child(ui::JobTag::new(kind_label(a.kind))),
            )
            // inc-host: 已更新到 v{n} after a reload in place, or 有 v{n}.
            .children(
                host::notice(&a.id, shown, a.version, cx)
                    .map(|t| div().ml(space::S4).child(host::notice_chip(t, cx))),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .child(picker)
                    .child(share)
                    .child(div().mx(space::S2).child(kit::bar_divider(cx)))
                    .child(
                        kit::bar_icon(
                            "artifact-open-window",
                            Lucide::ExternalLink,
                            c::ARTIFACT_OPEN_WINDOW,
                            false,
                            cx,
                        )
                        .on_click(move |_, _, cx| {
                            crate::views::artifacts::window::open_artifact(
                                &provider,
                                &open_id,
                                Some(shown),
                                cx,
                            )
                        }),
                    )
                    .child(
                        kit::bar_icon(
                            "artifact-side-chat",
                            Lucide::MessageCircle,
                            c::ARTIFACT_SIDE_CHAT,
                            false,
                            cx,
                        )
                        .on_click(move |_, _, cx| {
                            let (g, t, m) = (sg.clone(), st.clone(), made_by.clone());
                            nav.update(cx, |n, cx| {
                                n.open_topic(g, t, cx);
                                n.toggle_side_chat(Some(m), cx);
                            })
                        }),
                    ),
            )
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
                .child(ui::icon(Lucide::Clock, px(14.), k.ink_3))
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
                    ui::button(
                        "artifact-back-latest",
                        fill(c::ARTIFACT_BACK_TO, &[("v", &latest.to_string())]),
                        ui::ButtonKind::Ghost,
                        ui::ButtonSize::Sm,
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
        let height = window.viewport_size().height
            - theme::TITLE_BAR
            - space::S24
            - px(2.)
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
                        ui::button(
                            "artifact-open",
                            c::ARTIFACT_OPEN,
                            ui::ButtonKind::Primary,
                            ui::ButtonSize::Md,
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
        ui::motion::rise(
            "artifact-frame",
            0,
            ui::card(cx)
                .flex_1()
                .min_h(px(0.))
                .overflow_hidden()
                .flex()
                .flex_col()
                .children(band)
                .child(div().flex_1().min_h(px(0.)).child(body))
                .children(footer),
            cx,
        )
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
                            ui::icon(Lucide::Check, px(14.), k.ok).into_any_element()
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
                .child(ui::icon(Lucide::ChevronRight, px(14.), k.ink_3))
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
                            .child(ui::icon(Lucide::ChevronRight, px(14.), k.ink_3))
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
                            .child(ui::icon(Lucide::CircleCheck, px(14.), k.ink_3)),
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
                    .child(ui::icon(Lucide::List, px(14.), k.ink_3))
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
        let this = cx.entity().downgrade();
        kit::aside(
            "artifact-story",
            c::ARTIFACT_STORY,
            kit::ASIDE_WIDTH,
            space::S16,
            Some(Box::new(move |_, cx| {
                let _ = this.update(cx, |this, cx| {
                    this.aside_open = false;
                    cx.notify();
                });
            })),
            cx,
        )
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
        ))
        .into_any_element()
    }

    /// Every 产出, newest first (the 产出 destination of the rail).
    fn list(&self, cx: &App) -> AnyElement {
        let k = theme::colors(cx);
        let p = &self.services.provider;
        let now = p.clock_now();
        let all = p.all_artifacts();
        let header = kit::bar()
            .child(kit::page_title(c::ARTIFACTS_TITLE, cx))
            .child(
                div()
                    .mono()
                    .type_role(META)
                    .text_color(k.ink_3)
                    .child(all.len().to_string()),
            );
        if all.is_empty() {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .px(space::S32.max(kit::leading(&self.services, cx)))
                .child(header)
                .child(stub_body(Lucide::Layers, c::ARTIFACTS_EMPTY, None, cx))
                .into_any_element();
        }
        let rows = all.into_iter().enumerate().map(|(i, (g, a))| {
            let nav = self.services.nav.clone();
            let id = a.id.clone();
            let group = p
                .groups()
                .into_iter()
                .find(|x| x.id == g)
                .map(|x| x.name)
                .unwrap_or_default();
            let row =
                div()
                    .id(SharedString::from(format!("artifact-{}", a.id)))
                    .flex()
                    .items_center()
                    .gap(space::S12)
                    .p(space::S8)
                    .mx(-space::S8)
                    .rounded(px(10.))
                    .child(kit::artifact_thumb(
                        &self.services,
                        &a,
                        px(64.),
                        px(48.),
                        cx,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .one_line()
                                    .t_ui()
                                    .medium()
                                    .text_color(k.ink)
                                    .child(a.title.clone()),
                            )
                            .child(div().one_line().type_role(META).text_color(k.ink_3).child(
                                fill(
                                    c::ARTIFACT_LIST_META,
                                    &[
                                        ("v", &a.version.to_string()),
                                        ("kind", kind_label(a.kind)),
                                        ("name", &member_name(&self.services, &a.created_by)),
                                    ],
                                ),
                            )),
                    )
                    .child(
                        div()
                            .flex_none()
                            .type_role(META)
                            .text_color(k.ink_3)
                            .child(group),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(48.))
                            .flex()
                            .justify_end()
                            .mono()
                            .type_role(META)
                            .text_color(k.ink_3)
                            .child(short_time(now, a.updated_at)),
                    )
                    .on_click(move |_, _, cx| {
                        let id = id.clone();
                        nav.update(cx, |nav, cx| nav.open_artifact(id, None, cx))
                    })
                    .context_menu({
                        let (services, a) = (self.services.clone(), a.clone());
                        move |menu, _, _| artifact_menu(menu, &services, &a)
                    });
            ui::motion::rise(SharedString::from(format!("artifact-row-{i}")), i, row, cx)
        });
        div()
            .id("artifacts-list")
            .size_full()
            .overflow_y_scroll()
            .px(space::S32)
            .pb(space::S24)
            .child(
                div().max_w(kit::MEASURE).mx_auto().child(header).child(
                    div()
                        .mt(space::S4)
                        .flex()
                        .flex_col()
                        .gap(space::S2)
                        .children(rows),
                ),
            )
            .into_any_element()
    }
}

impl Render for ArtifactPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let Some((id, version)) = self.open(cx) else {
            return self.list(cx);
        };
        let Some(detail) = self.services.provider.artifact(&id) else {
            return stub_body(Lucide::Layers, c::ARTIFACTS_EMPTY, None, cx).into_any_element();
        };
        let shown = version
            .filter(|v| detail.versions.iter().any(|x| x.version == *v))
            .unwrap_or(detail.summary.version);
        let header = self.header(&detail, shown, cx);
        let frame = self.frame(&detail, shown, window, cx);
        let aside = self.aside_open.then(|| self.story(&detail, shown, cx));
        let reopen = (!self.aside_open).then(|| {
            kit::bar_icon(
                "artifact-aside-open",
                Lucide::PanelRight,
                c::ARTIFACT_EXPAND,
                false,
                cx,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.aside_open = true;
                cx.notify();
            }))
        });
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
                    .pl(space::S24.max(kit::leading(&self.services, cx)))
                    .pr(space::S24)
                    .pb(space::S24)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(div().flex_1().min_w_0().child(header))
                            .children(reopen),
                    )
                    .child(frame),
            )
            .children(aside)
            .into_any_element()
    }
}

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
