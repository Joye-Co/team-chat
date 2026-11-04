//! 单独问 (SideChat.dc.html, ⌘;): a private aside with one member about the
//! open 话题. It reads the 话题 up to now; answers stay private; a suggestion
//! posts into the 话题 only when pressed.
//!
//! Layout (380 pt, `rail`): a 52 pt header (单独问, the member picker chip,
//! close), the privacy note, the aside anchored to the bottom (a context
//! line, my questions as `sel` bubbles at the right, answers with a 20 pt
//! identity, numbered points with ref chips and the suggestion button), and
//! the 96 pt composer.

pub mod scenes;

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _,
    IntoElement, ParentElement, Pixels, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _,
    button::Button,
    input::{InputEvent, Textarea, TextareaState},
    menu::{DropdownMenu as _, PopupMenuItem},
};

use crate::ui::NamedButton as _;
use crate::{
    app::{RightPane, Services},
    copy::{common, fill, side_chat as c},
    data::{
        Member, MemberId, Millis,
        v2::{SideChatMessage, SideChatSource, SideChatThread},
    },
    theme::{self, Type as _, space},
    ui::{self, Lucide},
    views::shell::{PaneView, clock, panes::watch},
};

/// Width of my question bubbles.
const BUBBLE_MAX: Pixels = px(280.);
/// The member chip in the header: 26 pt, radius 7, a 18 pt tile.
const CHIP_HEIGHT: Pixels = px(26.);
const CHIP_TILE: Pixels = px(18.);
/// The number badge of a point in an answer: 18 pt, radius 5.
const POINT_BADGE: Pixels = px(18.);
/// A ref chip under a point: 24 pt, radius 6.
const REF_HEIGHT: Pixels = px(24.);
/// The suggestion button and the posted line: 32 pt.
const SUGGEST_HEIGHT: Pixels = px(32.);
/// The privacy note: 32 pt, radius 8.
const NOTE_HEIGHT: Pixels = px(32.);
/// The longest suggestion a button shows whole.
const SUGGEST_MAX_CHARS: usize = 20;

pub struct SideChat {
    services: Services,
    input: Entity<TextareaState>,
    scroll: ScrollHandle,
    /// (member, message count) last shown, to keep the newest answer in view.
    shown: Option<(MemberId, usize)>,
    _subscriptions: Vec<Subscription>,
}

impl SideChat {
    pub fn new(services: Services, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 6)
                .submit_on_enter(true)
        });
        let mut subs = watch(&services, cx);
        // --- inc-refs: chips of this composer, and which composer had focus last.
        let refs_hub = crate::views::refs::hub(cx);
        subs.push(cx.subscribe(
            &refs_hub,
            |_, _, event: &crate::views::refs::RefsEvent, cx| {
                if matches!(event, crate::views::refs::RefsEvent::SideChanged(_)) {
                    cx.notify();
                }
            },
        ));
        subs.push(cx.on_focus_in(&input.focus_handle(cx), window, |_, _, cx| {
            crate::views::refs::note_focus(true, cx);
        }));
        // --- end inc-refs
        subs.push(cx.subscribe_in(
            &input,
            window,
            |this: &mut Self, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { shift: false, .. } = event {
                    this.send(window, cx);
                }
                if let InputEvent::Change = event {
                    cx.notify();
                }
            },
        ));
        Self {
            services,
            input,
            scroll: ScrollHandle::new(),
            shown: None,
            _subscriptions: subs,
        }
    }

    /// The (group, topic, member) of the open aside.
    fn target(&self, cx: &App) -> Option<(String, String, String)> {
        let nav = self.services.nav.read(cx);
        let (g, t) = nav.topic.clone()?;
        match nav.visible_pane() {
            RightPane::SideChat(m) => Some((g, t, m)),
            _ => None,
        }
    }

    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let body = self.input.read(cx).value().trim().to_string();
        let Some((g, t, m)) = self.target(cx) else {
            return;
        };
        // inc-refs: the chips travel as quote lines in front of the question (a side chat
        // lives in the member's DM, where `blockRefs` to the 话题 are not accepted).
        let key = (g.clone(), t.clone(), m.clone());
        let has_refs = crate::views::refs::hub(cx)
            .read(cx)
            .side_refs
            .get(&key)
            .is_some_and(|r| !r.is_empty());
        if body.is_empty() && !has_refs {
            return;
        }
        let refs = crate::views::refs::hub(cx).update(cx, |h, cx| h.take_side(&key, cx));
        let body = crate::views::refs::side_chat_body(&body, &refs);
        self.services.provider.send_side_chat(&g, &t, &m, body);
        self.input.update(cx, |s, cx| s.set_value("", window, cx));
        self.scroll.scroll_to_bottom();
    }

    /// Bots I can ask, Jojo first.
    fn askable(&self) -> Vec<Member> {
        let mut bots: Vec<Member> = self
            .services
            .provider
            .members()
            .into_iter()
            .filter(|m| m.is_bot() && !m.deactivated)
            .collect();
        let default = self.services.provider.side_chat_default_member();
        bots.sort_by_key(|m| (Some(&m.id) != default.as_ref(), m.name.to_lowercase()));
        bots
    }

    /// The newest 话题 message at or before `at` (what the member read).
    fn read_upto(&self, g: &str, t: &str, at: Option<Millis>) -> Option<Millis> {
        let window = self.services.provider.messages(g, t, 200);
        window
            .messages
            .iter()
            .filter(|m| m.created_seq > 0 && at.is_none_or(|at| m.created_at <= at))
            .map(|m| m.created_at)
            .max()
    }

    fn header(&self, member: Option<&Member>, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let name = member.map(|m| m.name.clone()).unwrap_or_default();
        let services = self.services.clone();
        let bots = self.askable();
        let current = member.map(|m| m.id.clone());
        let picker = Button::new("side-chat-member")
            .outline()
            .h(CHIP_HEIGHT)
            .pl(space::S4)
            .pr(space::S6)
            .rounded(px(7.))
            .bg(k.bg)
            .border_color(k.line_2)
            .shadow(k.sec_hi())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S6)
                    .t_meta_medium()
                    .text_color(k.ink)
                    .child(ui::MemberAvatar::new(member, CHIP_TILE))
                    .child(name.clone())
                    .child(ui::icon(Lucide::ChevronDown, theme::icon::TAG, k.ink_3)),
            )
            .named(fill(c::SIDE_CHAT_CHANGE_MEMBER, &[("name", &name)]))
            .dropdown_menu(move |mut menu, _window, _cx| {
                for m in &bots {
                    let services = services.clone();
                    let id = m.id.clone();
                    let label = match &m.role {
                        Some(role) => format!("{} · {role}", m.name),
                        None => m.name.clone(),
                    };
                    menu = menu.item(
                        PopupMenuItem::new(label)
                            .checked(current.as_deref() == Some(m.id.as_str()))
                            .on_click(move |_, _, cx| {
                                services.nav.update(cx, |nav, cx| {
                                    nav.set_right_pane(RightPane::SideChat(id.clone()), cx)
                                })
                            }),
                    );
                }
                menu
            });
        let services = self.services.clone();
        div()
            .flex_none()
            .h(theme::TITLE_BAR)
            .pl(space::S16)
            .pr(space::S12)
            .flex()
            .items_center()
            .gap(space::S8)
            .child(
                div()
                    .t_small()
                    .semibold()
                    .text_color(k.ink)
                    .child(c::SIDE_CHAT_TITLE),
            )
            .child(picker)
            .child(div().flex_1())
            .child(
                ui::icon_button_sm("side-chat-close", Lucide::X)
                    .named(c::SIDE_CHAT_CLOSE)
                    .on_click(move |_, _, cx| {
                        services
                            .nav
                            .update(cx, |nav, cx| nav.set_right_pane(RightPane::None, cx))
                    }),
            )
    }

    fn note(&self, cx: &App) -> impl IntoElement {
        let k = theme::colors(cx);
        div()
            .flex_none()
            .mx(space::S16)
            .h(NOTE_HEIGHT)
            .px(space::S10)
            .rounded(theme::radius::MD)
            .bg(k.sunken)
            .flex()
            .items_center()
            .gap(space::S8)
            .t_meta()
            .text_color(k.ink_2)
            .child(ui::icon(Lucide::Lock, theme::icon::TAG, k.ink_3))
            .child(c::SIDE_CHAT_PRIVATE)
    }

    /// `── Jojo 读过话题里 10:20 之前的内容 ──`, the time in mono.
    fn context_line(&self, template: &str, name: &str, at: Millis, cx: &App) -> AnyElement {
        let k = theme::colors(cx);
        let text = fill(template, &[("name", name)]);
        let (before, after) = text.split_once("{time}").unwrap_or((text.as_str(), ""));
        let rule = || div().flex_1().h(px(1.)).bg(k.line);
        ui::motion::rise(
            "side-chat-context",
            0,
            div()
                .flex()
                .items_center()
                .gap(space::S10)
                .t_meta()
                .text_color(k.ink_3)
                .child(rule())
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(space::S4)
                        .child(before.trim().to_string())
                        .child(div().mono().child(clock(at)))
                        .child(after.trim().to_string()),
                )
                .child(rule()),
            cx,
        )
    }

    fn question(&self, msg: &SideChatMessage, ix: usize, cx: &App) -> AnyElement {
        let k = theme::colors(cx);
        ui::motion::rise(
            SharedString::from(format!("side-chat-q-{}", msg.id)),
            ix,
            div().flex().flex_col().items_end().child(
                div()
                    .max_w(BUBBLE_MAX)
                    .px(space::S12)
                    .py(space::S8)
                    .rounded_tl(theme::radius::LG)
                    .rounded_tr(theme::radius::LG)
                    .rounded_bl(theme::radius::LG)
                    .rounded_br(space::S4)
                    .bg(k.sel)
                    .t_body()
                    .text_color(k.ink)
                    // inc-refs: the quote lines of the chips the question carried.
                    .child({
                        let (labels, rest) = crate::views::refs::split_quote_lines(&msg.body);
                        div()
                            .flex()
                            .flex_col()
                            .gap(space::S4)
                            .when(!labels.is_empty(), |d| {
                                d.child(crate::views::refs::chips::side_lines(&labels, cx))
                            })
                            .when(!rest.is_empty(), |d| d.child(rest.to_string()))
                    }),
            ),
            cx,
        )
    }

    fn answer(
        &self,
        target: &(String, String, String),
        msg: &SideChatMessage,
        ix: usize,
        cx: &App,
    ) -> AnyElement {
        let k = theme::colors(cx);
        let member = self.services.provider.member(&msg.author_id);
        let id = |part: &str| SharedString::from(format!("side-chat-a-{}-{part}", msg.id));
        let head = div()
            .flex()
            .items_center()
            .gap(space::S8)
            .child(ui::MemberAvatar::new(member.as_ref(), px(20.)))
            .child(
                div()
                    .t_small()
                    .medium()
                    .text_color(k.ink)
                    .child(member.as_ref().map(|m| m.name.clone()).unwrap_or_default()),
            )
            .children(
                member
                    .as_ref()
                    .and_then(|m| m.role.clone())
                    .map(ui::JobTag::new),
            )
            .child(div().flex_1())
            .child(div().t_mono().text_color(k.ink_3).child(clock(msg.at)));

        let blocks = answer_blocks(&msg.body);
        let points = blocks
            .iter()
            .filter(|b| matches!(b, Block::Point(..)))
            .count();
        // One ref chip under each point when the counts agree; else a row after the text.
        let refs_per_point = points > 0 && points == msg.refs.len();
        let mut point_ix = 0;
        let mut body: Vec<AnyElement> = Vec::new();
        let mut list: Vec<AnyElement> = Vec::new();
        let flush = |list: &mut Vec<AnyElement>, body: &mut Vec<AnyElement>| {
            if !list.is_empty() {
                body.push(
                    div()
                        .flex()
                        .flex_col()
                        .gap(space::S12)
                        .children(std::mem::take(list))
                        .into_any_element(),
                );
            }
        };
        for (i, b) in blocks.iter().enumerate() {
            match b {
                Block::Para(text) => {
                    flush(&mut list, &mut body);
                    body.push(ui::motion::rise(
                        id(&format!("p{i}")),
                        ix + i + 1,
                        div().t_body().text_color(k.ink).child(text.clone()),
                        cx,
                    ));
                }
                Block::Point(n, text) => {
                    let chip = refs_per_point
                        .then(|| ref_chip(&msg.refs[point_ix], cx).into_any_element());
                    point_ix += 1;
                    list.push(ui::motion::rise(
                        id(&format!("p{i}")),
                        ix + i + 1,
                        div()
                            .flex()
                            .gap(space::S10)
                            .child(
                                div()
                                    .flex_none()
                                    .mt(px(1.))
                                    .size(POINT_BADGE)
                                    .rounded(theme::TAG_RADIUS)
                                    .bg(k.sunken)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(11.))
                                    .mono()
                                    .text_color(k.ink_2)
                                    .child(n.clone()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .items_start()
                                    .gap(space::S6)
                                    .child(div().t_small().text_color(k.ink).child(text.clone()))
                                    .children(chip),
                            ),
                        cx,
                    ));
                }
            }
        }
        flush(&mut list, &mut body);
        if !refs_per_point && !msg.refs.is_empty() {
            body.push(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(space::S6)
                    .children(msg.refs.iter().map(|r| ref_chip(r, cx)))
                    .into_any_element(),
            );
        }
        if msg.streaming && msg.body.trim().is_empty() {
            let name = member.as_ref().map(|m| m.name.clone()).unwrap_or_default();
            body.push(
                ui::motion::breathe(
                    id("writing"),
                    div()
                        .t_small()
                        .text_color(k.ink_3)
                        .child(fill(c::SIDE_CHAT_WRITING, &[("name", &name)])),
                    cx,
                )
                .into_any_element(),
            );
        }
        // `看了 …`: what the answer read, when the member said.
        if let Some(text) = sources_text(&msg.sources) {
            body.push(
                div()
                    .flex()
                    .items_start()
                    .gap(space::S6)
                    .t_meta()
                    .text_color(k.ink_3)
                    .child(div().flex_none().mt(px(2.)).child(ui::icon(
                        Lucide::Eye,
                        theme::icon::TAG,
                        k.ink_3,
                    )))
                    .child(div().flex_1().min_w_0().child(text))
                    .into_any_element(),
            );
        }
        let suggestion = msg
            .suggestion
            .as_ref()
            .map(|s| self.suggestion(target, msg, s, cx));
        ui::motion::rise(
            id("head"),
            ix,
            div()
                .flex()
                .flex_col()
                .gap(space::S10)
                .child(head)
                .children(body)
                .children(suggestion),
            cx,
        )
    }

    fn suggestion(
        &self,
        target: &(String, String, String),
        msg: &SideChatMessage,
        s: &crate::data::v2::SideChatSuggestion,
        cx: &App,
    ) -> AnyElement {
        let k = theme::colors(cx);
        if let Some(at) = s.posted_at {
            let when = if (crate::selftest::clock::utc_now().timestamp_millis() - at) < 60_000 {
                common::JUST_NOW.to_string()
            } else {
                clock(at)
            };
            return ui::motion::rise(
                SharedString::from(format!("side-chat-posted-{}", msg.id)),
                0,
                div()
                    .h(SUGGEST_HEIGHT)
                    .pl(space::S10)
                    .pr(space::S4)
                    .rounded(theme::radius::MD)
                    .bg(k.sunken)
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .t_small()
                    .text_color(k.ink)
                    .child(ui::icon(Lucide::Check, theme::icon::CHIP, k.ok))
                    .child(c::SIDE_CHAT_POSTED)
                    .child(
                        div()
                            .t_mono()
                            .text_color(k.ink_3)
                            .child(format!("· {when}")),
                    ),
                cx,
            );
        }
        let (services, (g, t, m), id) = (self.services.clone(), target.clone(), msg.id.clone());
        // A button never wraps: a long suggestion is cut, with the whole text in its tooltip.
        let long = s.text.chars().count() > SUGGEST_MAX_CHARS;
        let label = if long {
            let cut: String = s.text.chars().take(SUGGEST_MAX_CHARS - 1).collect();
            format!("{cut}…")
        } else {
            s.text.clone()
        };
        let full = s.text.clone();
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(space::S10)
            .child(
                ui::button(
                    SharedString::from(format!("side-chat-suggest-{}", msg.id)),
                    label,
                    ui::ButtonKind::Secondary,
                    ui::ButtonSize::Md,
                    cx,
                )
                .icon(ui::icon(
                    Lucide::CornerDownRight,
                    theme::icon::CHIP,
                    k.ink_2,
                ))
                .when(long, |b| b.tooltip(full))
                .on_click(move |_, _, _| {
                    services.provider.post_side_chat_suggestion(&g, &t, &m, &id)
                }),
            )
            .child(
                div()
                    .t_meta()
                    .text_color(k.ink_3)
                    .child(c::SIDE_CHAT_SUGGEST_HINT),
            )
            .into_any_element()
    }

    fn composer(&self, name: &str, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let k = theme::colors(cx);
        let placeholder = fill(c::SIDE_CHAT_PLACEHOLDER, &[("name", name)]);
        self.input.update(cx, |s, cx| {
            s.set_placeholder(placeholder.clone(), window, cx)
        });
        // --- inc-refs: the chips of this aside's composer.
        let key = self.target(cx);
        let side_refs: Vec<crate::data::DraftRef> = key
            .as_ref()
            .and_then(|k| {
                crate::views::refs::hub(cx)
                    .read(cx)
                    .side_refs
                    .get(k)
                    .cloned()
            })
            .unwrap_or_default();
        let chip_row = (!side_refs.is_empty()).then(|| {
            let chips = side_refs
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    let (open, k2) = (r.clone(), key.clone());
                    crate::views::refs::chips::composer_chip(
                        gpui::ElementId::Name(format!("side-ref-{i}").into()),
                        r,
                        false,
                        crate::views::refs::chips::flashing(r, cx),
                        move |_, cx| crate::views::refs::chips::open_source(&open, cx),
                        move |_, cx| {
                            if let Some(k) = &k2 {
                                crate::views::refs::hub(cx)
                                    .update(cx, |h, cx| h.remove_side(k, i, cx));
                            }
                        },
                        cx,
                    )
                })
                .collect();
            crate::views::refs::chips::row(chips)
        });
        // --- end inc-refs
        let ready = !self.input.read(cx).value().trim().is_empty() || chip_row.is_some();
        let editor = ui::placeholder_input(
            Textarea::new(&self.input)
                .appearance(false)
                .xsmall()
                .p_0()
                .t_body(),
            window,
            cx,
        );
        div()
            .flex_none()
            .px(space::S16)
            .pb(space::S16)
            .child(
                ui::ComposerShell::new("side-chat-composer", editor)
                    .when_some(chip_row, |shell, row| shell.above(row))
                    .tool(
                        div()
                            .ml(space::S4)
                            .flex()
                            .items_center()
                            .gap(space::S4)
                            .t_meta()
                            .text_color(k.ink_3)
                            .child(div().mono().child("⌘;"))
                            .child(c::SIDE_CHAT_CLOSE_HINT),
                    )
                    .send(
                        ui::send_button("side-chat-send", ready, c::SIDE_CHAT_SEND, cx)
                            .on_click(cx.listener(|this, _, window, cx| this.send(window, cx))),
                    ),
            )
            .into_any_element()
    }
}

impl PaneView for SideChat {
    fn width(&self, _window_w: Pixels) -> Pixels {
        theme::SIDE_CHAT_WIDTH
    }
}

/// A part of an answer: a paragraph, or a numbered point (`1. …`).
#[derive(Debug, PartialEq)]
enum Block {
    Para(String),
    Point(String, String),
}

/// Splits an answer into paragraphs and numbered points.
fn answer_blocks(body: &str) -> Vec<Block> {
    let mut out = Vec::new();
    for line in body.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let digits: String = line.chars().take_while(|c| c.is_ascii_digit()).collect();
        let rest = &line[digits.len()..];
        let point = (!digits.is_empty())
            .then(|| {
                rest.strip_prefix(". ")
                    .or_else(|| rest.strip_prefix("、"))
                    .or_else(|| rest.strip_prefix(") "))
            })
            .flatten();
        match point {
            Some(text) => out.push(Block::Point(digits, text.trim().to_string())),
            None => out.push(Block::Para(line.to_string())),
        }
    }
    out
}

/// The source line of an answer: `看了 A`, `看了 A和B`, `看了 A、B和C`.
fn sources_text(sources: &[SideChatSource]) -> Option<String> {
    let parts: Vec<String> = sources
        .iter()
        .map(|s| match s {
            SideChatSource::Pr {
                number,
                files: Some(files),
            } => fill(
                c::SIDE_CHAT_SOURCE_PR_FILES,
                &[("n", &number.to_string()), ("files", &files.to_string())],
            ),
            SideChatSource::Pr {
                number,
                files: None,
            } => fill(c::SIDE_CHAT_SOURCE_PR, &[("n", &number.to_string())]),
            SideChatSource::Artifact { title, version } => fill(
                c::SIDE_CHAT_SOURCE_ARTIFACT,
                &[("title", title), ("version", &version.to_string())],
            ),
        })
        .collect();
    let (last, rest) = parts.split_last()?;
    let list = if rest.is_empty() {
        last.clone()
    } else {
        format!(
            "{}{}{last}",
            rest.join(c::SIDE_CHAT_SOURCES_SEP),
            c::SIDE_CHAT_SOURCES_LAST
        )
    };
    Some(fill(c::SIDE_CHAT_SOURCES, &[("sources", &list)]))
}

/// A ref chip: 24 pt, `bg`, 1 pt `line`, a file glyph, a leading file name in mono.
fn ref_chip(label: &str, cx: &App) -> impl IntoElement {
    let k = theme::colors(cx);
    let (first, rest) = label.split_once(' ').unwrap_or((label, ""));
    let file_like = first.contains('.') || first.starts_with('#');
    let glyph = if file_like {
        Lucide::File
    } else {
        Lucide::GitPullRequest
    };
    div()
        .max_w_full()
        .min_w_0()
        .overflow_hidden()
        .h(REF_HEIGHT)
        .px(space::S8)
        .rounded(theme::radius::SM)
        .border_1()
        .border_color(k.line)
        .bg(k.bg)
        .flex()
        .items_center()
        .gap(space::S6)
        .t_meta()
        .text_color(k.ink_2)
        .child(
            div()
                .flex_none()
                .child(ui::icon(glyph, theme::icon::TAG, k.ink_3)),
        )
        .when(file_like, |d| {
            d.child(
                div()
                    .min_w_0()
                    .one_line()
                    .mono()
                    .text_color(k.ink)
                    .child(first.to_string()),
            )
            .when(!rest.is_empty(), |d| {
                d.child(div().flex_none().child(rest.to_string()))
            })
        })
        .when(!file_like, |d| {
            d.child(div().min_w_0().one_line().child(label.to_string()))
        })
}

impl Render for SideChat {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        let Some(target) = self.target(cx) else {
            return div().into_any_element();
        };
        let (g, t, m) = target.clone();
        let member = self.services.provider.member(&m);
        let name = member.as_ref().map(|m| m.name.clone()).unwrap_or_default();
        let me = self.services.me();
        let thread: SideChatThread = self.services.provider.side_chat(&g, &t, &m);
        crate::selftest::drawn::mark(
            "side-chat",
            format!("{name} messages={}", thread.messages.len()),
        );

        // Keep the newest message in view when the aside or its length changes.
        let shown = (m.clone(), thread.messages.len());
        if self.shown.as_ref() != Some(&shown) {
            self.shown = Some(shown);
            self.scroll.scroll_to_bottom();
        }

        let first_question = thread
            .messages
            .iter()
            .find(|msg| Some(&msg.author_id) == me.as_ref())
            .map(|msg| msg.at);
        let context = if thread.messages.is_empty() {
            self.read_upto(&g, &t, None)
                .map(|at| self.context_line(c::SIDE_CHAT_WILL_READ, &name, at, cx))
        } else {
            self.read_upto(&g, &t, first_question)
                .map(|at| self.context_line(c::SIDE_CHAT_READ_UPTO, &name, at, cx))
        };
        let rows: Vec<AnyElement> = thread
            .messages
            .iter()
            .enumerate()
            .map(|(i, msg)| {
                if Some(&msg.author_id) == me.as_ref() {
                    self.question(msg, i + 1, cx)
                } else {
                    self.answer(&target, msg, i + 1, cx)
                }
            })
            .collect();
        let empty = thread.messages.is_empty().then(|| {
            div()
                .t_small()
                .text_color(k.ink_3)
                .text_center()
                .child(fill(c::SIDE_CHAT_EMPTY, &[("name", &name)]))
        });

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(k.rail)
            .child(self.header(member.as_ref(), cx))
            .child(self.note(cx))
            .child(
                div()
                    .id("side-chat-thread")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(
                        div()
                            .min_h_full()
                            .pt(space::S16)
                            .px(space::S16)
                            .pb(space::S12)
                            .flex()
                            .flex_col()
                            .justify_end()
                            .gap(space::S16)
                            .children(context)
                            .children(empty)
                            .children(rows),
                    ),
            )
            .child(self.composer(&name, window, cx))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_source_line_joins_what_the_answer_read() {
        assert_eq!(sources_text(&[]), None);
        let pr = SideChatSource::Pr {
            number: 61,
            files: Some(12),
        };
        let art = SideChatSource::Artifact {
            title: "重连测试报告".into(),
            version: 2,
        };
        assert_eq!(
            sources_text(&[pr.clone(), art.clone()]).as_deref(),
            Some("看了 PR #61 的 12 个文件和「重连测试报告」v2")
        );
        assert_eq!(
            sources_text(&[
                SideChatSource::Pr {
                    number: 7,
                    files: None
                },
                pr,
                art
            ])
            .as_deref(),
            Some("看了 PR #7、PR #61 的 12 个文件和「重连测试报告」v2")
        );
    }

    #[test]
    fn answers_split_into_paragraphs_and_points() {
        let blocks =
            answer_blocks("风险不大：\n1. 去重靠 seq。\n2、退避 30 秒\n\n测试覆盖了 6 个场景。");
        assert_eq!(
            blocks,
            vec![
                Block::Para("风险不大：".into()),
                Block::Point("1".into(), "去重靠 seq。".into()),
                Block::Point("2".into(), "退避 30 秒".into()),
                Block::Para("测试覆盖了 6 个场景。".into()),
            ]
        );
    }
}
