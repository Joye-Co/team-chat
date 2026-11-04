//! 单独问 (SideChat.dc.html, ⌘;): a private aside with one member about the
//! open 话题. It reads the 话题 up to now; answers stay private; a suggestion
//! posts into the 话题 only when pressed.
//!
//! Layout (COMPLETENESS.md §2.5, 320 wide, `background` fill): a 52 header
//! (`单独问` + the member select h28 over the 12 muted `只有你能看到`, a 32
//! close), the aside anchored at the top (a centered context line, then the
//! same message rows as the thread for my questions and the answers, with
//! numbered points and their references, and the suggestion as an outline
//! sm button), and the composer docked at the bottom. With no messages the
//! body is an Empty state with three suggested questions.

pub mod scenes;

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _,
    IntoElement, ParentElement, Pixels, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _,
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
    theme::{Type as _, space},
    ui::{
        self, Sym,
        native::{
            Empty, NativeType as _, PushSize, PushStyle, borderless_button, metrics, native_colors,
            popup_button, push_button, tokens::avatar,
        },
        sf_symbols::Sf,
    },
    views::shell::{PaneView, clock, panes::watch},
};

/// The number column of a point in an answer (`1.`).
const POINT_NUMBER: Pixels = px(16.);
/// A reference under a point: 20 pt.
const REF_HEIGHT: Pixels = px(20.);
/// The longest suggestion a button shows whole: an outline sm button fits
/// the 244 text column of the 320 pane.
const SUGGEST_MAX_CHARS: usize = 15;
/// The longest member name the composer placeholder shows whole.
const PLACEHOLDER_NAME_MAX: usize = 16;
/// The member select in the header (§2.11: max 200, the name truncates).
const SELECT_MAX: Pixels = px(200.);
/// The suggested questions of an empty aside (§2.5).
const STARTERS: [&str; 3] = [
    c::SIDE_CHAT_ASK_PROGRESS,
    c::SIDE_CHAT_ASK_RISK,
    c::SIDE_CHAT_ASK_NEXT,
];

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
                .auto_grow(1, 8)
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

    /// Sends one of the suggested questions of the empty aside.
    fn ask(&mut self, question: &str, cx: &mut Context<Self>) {
        let Some((g, t, m)) = self.target(cx) else {
            return;
        };
        self.services
            .provider
            .send_side_chat(&g, &t, &m, question.to_string(), Vec::new());
        self.scroll.scroll_to_bottom();
    }

    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let body = self.input.read(cx).value().trim().to_string();
        let Some((g, t, m)) = self.target(cx) else {
            return;
        };
        // inc-refs: the chips travel as `blockRefs` that name the 话题's 群 (`sourceGroupId`);
        // the server writes their quote lines in front of the question.
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
        self.services
            .provider
            .send_side_chat(&g, &t, &m, body, refs);
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

    /// The h52 header (§2.2, §2.5): `单独问` and the member select h28 (max
    /// 200, the name truncates, the full name in its tooltip) over the 12
    /// muted `只有你能看到`, and the 32 ghost close at the far right.
    fn header(
        &self,
        member: Option<&Member>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let k = native_colors(cx);
        let name = member.map(|m| m.name.clone()).unwrap_or_default();
        let services = self.services.clone();
        let bots = self.askable();
        let current = member.map(|m| m.id.clone());
        let picker = popup_button(
            "side-chat-member",
            name.clone(),
            PushSize::Small,
            window,
            cx,
        )
        .min_w_0()
        .max_w(SELECT_MAX)
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
            .h(metrics::TOOLBAR_ROW)
            .pl(metrics::INSPECTOR_PAD)
            .pr(metrics::TOOLBAR_TRAILING)
            .flex()
            .items_center()
            .gap(space::S8)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(space::S6)
                            .child(
                                div()
                                    .flex_none()
                                    .n_title3()
                                    .text_color(k.label)
                                    .child(c::SIDE_CHAT_TITLE),
                            )
                            .child(picker),
                    )
                    .child(
                        div()
                            .one_line()
                            .n_footnote()
                            .text_color(k.secondary)
                            .child(c::SIDE_CHAT_PRIVATE),
                    ),
            )
            .child(
                borderless_button("side-chat-close", Sf::Close, c::SIDE_CHAT_CLOSE, window, cx)
                    .on_click(move |_, _, cx| {
                        services
                            .nav
                            .update(cx, |nav, cx| nav.set_right_pane(RightPane::None, cx))
                    }),
            )
    }

    /// The Empty state of an aside with no messages (§2.5, §2.13): the
    /// 40 icon box, what the member reads, and three suggested questions as
    /// outline sm buttons, centered in the body.
    fn empty(&self, name: &str, read_upto: Option<Millis>, cx: &mut Context<Self>) -> AnyElement {
        let line = match read_upto {
            Some(at) => fill(
                c::SIDE_CHAT_EMPTY_LINE,
                &[("name", name), ("time", &clock(at))],
            ),
            None => fill(c::SIDE_CHAT_EMPTY_LINE_NO_TIME, &[("name", name)]),
        };
        // Two sentences on two lines, so neither breaks mid-phrase.
        let line = format!("{line}\n{}", c::SIDE_CHAT_EMPTY_PRIVATE);
        let starters: Vec<_> = STARTERS
            .iter()
            .enumerate()
            .map(|(i, q)| {
                push_button(
                    SharedString::from(format!("side-chat-starter-{i}")),
                    *q,
                    PushStyle::Outline,
                    PushSize::Small,
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.ask(q, cx)))
            })
            .collect();
        Empty::new(
            Sym::AskAside,
            fill(c::SIDE_CHAT_EMPTY_TITLE, &[("name", name)]),
        )
        .description(line)
        .action(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(space::S8)
                .children(starters),
        )
        .into_any_element()
    }

    /// `Jojo 读过话题里 10:20 之前的内容`: centered `secondary` Subheadline,
    /// no rules (NATIVE.md §9.2 day separators).
    fn context_line(&self, template: &str, name: &str, at: Millis, cx: &App) -> AnyElement {
        let k = native_colors(cx);
        let text = fill(template, &[("name", name)]);
        let text = text.replace("{time}", &clock(at));
        div()
            .flex()
            .justify_center()
            .text_center()
            .n_subheadline()
            .n_figures()
            .text_color(k.secondary)
            .child(text)
            .into_any_element()
    }

    /// One message row as the thread draws it (§2.17): avatar 32, then at
    /// the text column the sender line (name 14 medium, which keeps its
    /// room; the role, which truncates first; the time 12 muted in its own
    /// slot) over the body. My questions and the answers share it: no
    /// bubbles.
    fn row(
        &self,
        id: SharedString,
        author: Option<&Member>,
        at: Millis,
        body: Vec<AnyElement>,
        cx: &App,
    ) -> AnyElement {
        let k = native_colors(cx);
        let role = author
            .and_then(|m| m.role.clone())
            .filter(|r| !r.trim().is_empty());
        let head = div()
            .h(px(20.))
            .min_w_0()
            .flex()
            .items_center()
            .gap(space::S6)
            .child(
                ui::truncating(
                    SharedString::from(format!("{id}-name")),
                    author.map(|m| m.name.clone()).unwrap_or_default(),
                )
                .flex_shrink_0()
                .max_w(gpui::relative(0.7))
                .n_headline()
                .text_color(k.label),
            )
            .children(role.map(|role| {
                ui::truncating(
                    SharedString::from(format!("{id}-role")),
                    role.trim().to_string(),
                )
                .flex_shrink(1.)
                .n_subheadline()
                .text_color(k.secondary)
            }))
            .child(
                div()
                    .flex_none()
                    .n_subheadline()
                    .n_figures()
                    .text_color(k.secondary)
                    .child(clock(at)),
            );
        div()
            .id(id)
            .flex()
            .items_start()
            .gap(space::S12)
            .child(
                div()
                    .flex_none()
                    .child(ui::MemberAvatar::new(author, avatar::MD)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(space::S4)
                    .child(head)
                    .children(body),
            )
            .into_any_element()
    }

    fn question(&self, msg: &SideChatMessage, cx: &App) -> AnyElement {
        let n = native_colors(cx);
        let me = self.services.provider.member(&msg.author_id);
        // inc-refs: the quote lines of the chips the question carried.
        let (labels, rest) = crate::views::refs::split_quote_lines(&msg.body);
        let body = div()
            .flex()
            .flex_col()
            .gap(space::S4)
            .n_message()
            .text_color(n.label)
            .when(!labels.is_empty(), |d| {
                d.child(crate::views::refs::chips::side_lines(&labels, cx))
            })
            .when(!rest.is_empty(), |d| d.child(rest.to_string()))
            .into_any_element();
        self.row(
            SharedString::from(format!("side-chat-q-{}", msg.id)),
            me.as_ref(),
            msg.at,
            vec![body],
            cx,
        )
    }

    fn answer(
        &self,
        target: &(String, String, String),
        msg: &SideChatMessage,
        cx: &App,
    ) -> AnyElement {
        let member = self.services.provider.member(&msg.author_id);
        let id = |part: &str| SharedString::from(format!("side-chat-a-{}-{part}", msg.id));
        let n = native_colors(cx);
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
                        .gap(space::S8)
                        .children(std::mem::take(list))
                        .into_any_element(),
                );
            }
        };
        for b in &blocks {
            match b {
                Block::Para(text) => {
                    flush(&mut list, &mut body);
                    body.push(
                        div()
                            .n_message()
                            .text_color(n.label)
                            .child(text.clone())
                            .into_any_element(),
                    );
                }
                Block::Point(nn, text) => {
                    let chip = refs_per_point.then(|| {
                        ref_chip(id(&format!("ref{point_ix}")), &msg.refs[point_ix], cx)
                            .into_any_element()
                    });
                    point_ix += 1;
                    list.push(
                        div()
                            .flex()
                            .gap(space::S8)
                            .child(
                                div()
                                    .flex_none()
                                    .min_w(POINT_NUMBER)
                                    .n_message()
                                    .n_figures()
                                    .text_color(n.secondary)
                                    .child(format!("{nn}.")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .items_start()
                                    .gap(space::S6)
                                    .child(
                                        div().n_message().text_color(n.label).child(text.clone()),
                                    )
                                    .children(chip),
                            )
                            .into_any_element(),
                    );
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
                    .children(
                        msg.refs
                            .iter()
                            .enumerate()
                            .map(|(i, r)| ref_chip(id(&format!("ref{i}")), r, cx)),
                    )
                    .into_any_element(),
            );
        }
        if msg.streaming && msg.body.trim().is_empty() {
            let name = member.as_ref().map(|m| m.name.clone()).unwrap_or_default();
            body.push(
                ui::motion::breathe(
                    id("writing"),
                    div()
                        .n_callout()
                        .text_color(n.secondary)
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
                    .n_subheadline()
                    .text_color(n.secondary)
                    .child(div().flex_none().child(ui::sym(
                        Sym::Watch,
                        ui::SymSize::INLINE_SMALL,
                        n.secondary,
                    )))
                    .child(div().flex_1().min_w_0().child(text))
                    .into_any_element(),
            );
        }
        let suggestion = msg
            .suggestion
            .as_ref()
            .map(|s| self.suggestion(target, msg, s, cx));
        body.extend(suggestion);
        let body = vec![
            div()
                .flex()
                .flex_col()
                .gap(space::S8)
                .children(body)
                .into_any_element(),
        ];
        self.row(id("row"), member.as_ref(), msg.at, body, cx)
    }

    fn suggestion(
        &self,
        target: &(String, String, String),
        msg: &SideChatMessage,
        s: &crate::data::v2::SideChatSuggestion,
        cx: &App,
    ) -> AnyElement {
        let k = native_colors(cx);
        if let Some(at) = s.posted_at {
            let when = if (crate::selftest::clock::utc_now().timestamp_millis() - at) < 60_000 {
                common::JUST_NOW.to_string()
            } else {
                clock(at)
            };
            return div()
                .min_h(px(24.))
                .flex()
                .items_center()
                .gap(space::S6)
                .n_callout()
                .text_color(k.secondary)
                .child(ui::sym(Sym::Done, ui::SymSize::INLINE_SMALL, k.green))
                .child(c::SIDE_CHAT_POSTED)
                .child(div().n_figures().text_color(k.secondary).child(when))
                .into_any_element();
        }
        let (services, (g, t, m), id) = (self.services.clone(), target.clone(), msg.id.clone());
        // A button never wraps: a long suggestion is cut, with the whole text in its tooltip.
        let label = if s.text.chars().count() > SUGGEST_MAX_CHARS {
            let cut: String = s.text.chars().take(SUGGEST_MAX_CHARS - 1).collect();
            format!("{cut}…")
        } else {
            s.text.clone()
        };
        let full = s.text.clone();
        div()
            .flex()
            .flex_col()
            .items_start()
            .gap(space::S4)
            .child(
                push_button(
                    SharedString::from(format!("side-chat-suggest-{}", msg.id)),
                    label,
                    PushStyle::Outline,
                    PushSize::Small,
                    cx,
                )
                .max_w_full()
                .tooltip(full)
                .on_click(move |_, _, _| {
                    services.provider.post_side_chat_suggestion(&g, &t, &m, &id)
                }),
            )
            .child(
                div()
                    .n_footnote()
                    .text_color(k.secondary)
                    .child(c::SIDE_CHAT_SUGGEST_HINT),
            )
            .into_any_element()
    }

    fn composer(&self, name: &str, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // A placeholder cannot truncate itself: a long name is cut (the
        // placeholder's own `…` ends it) so the line never clips mid-glyph
        // at the field edge.
        let short: String = name
            .chars()
            .take(PLACEHOLDER_NAME_MAX)
            .collect::<String>()
            .trim_end()
            .to_string();
        let placeholder = fill(c::SIDE_CHAT_PLACEHOLDER, &[("name", &short)]);
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
        let n = native_colors(cx);
        let editor = ui::placeholder_input(
            Textarea::new(&self.input)
                .appearance(false)
                .xsmall()
                .p_0()
                .n_message(),
            window,
            cx,
        );
        // The composer of the transcript (NATIVE.md §9.2): radius 18, at
        // least 36 pt, `content` with a hairline in light and `fill` in
        // dark, the `accent` send circle only when there is text.
        // The composer (COMPLETENESS.md §2.17): one frame, radius 10, a
        // 1 px border, shadow xs, p 8; Send is a primary sm icon button at
        // 50 % until there is text.
        let send = ui::native::disabled_push(
            ui::native::icon_button_styled(
                "side-chat-send",
                Sym::ArrowUp,
                c::SIDE_CHAT_SEND,
                ui::native::PushStyle::Default,
                ui::native::IconSize::Small,
                window,
                cx,
            )
            .when(ready, |b| {
                b.on_click(cx.listener(|this, _, window, cx| this.send(window, cx)))
            }),
            !ready,
            cx,
        );
        div()
            .flex_none()
            .px(space::S16)
            .pb(space::S16)
            .child(
                div()
                    .id("side-chat-composer")
                    .w_full()
                    .rounded(ui::native::tokens::radius::CONTAINER)
                    .bg(n.content)
                    .border_1()
                    .border_color(n.separator)
                    .shadow(ui::native::tokens::shadow::xs())
                    .cursor_text()
                    .p(space::S8)
                    .flex()
                    .flex_col()
                    .gap(space::S8)
                    .children(chip_row.map(|row| div().px(space::S4).child(row)))
                    .child(
                        div()
                            .w_full()
                            .px(space::S4)
                            .text_color(n.label)
                            .child(editor),
                    )
                    .child(div().w_full().flex().justify_end().child(send)),
            )
            .into_any_element()
    }
}

impl PaneView for SideChat {
    fn width(&self, _window_w: Pixels) -> Pixels {
        metrics::INSPECTOR_WIDTH
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

/// A reference under a point: a symbol and text run in `secondary`, the
/// leading file name in the code font (NATIVE.md rule 4: no chips).
fn ref_chip(id: SharedString, label: &str, cx: &App) -> impl IntoElement {
    let k = native_colors(cx);
    let (first, rest) = label.split_once(' ').unwrap_or((label, ""));
    let file_like = first.contains('.') || first.starts_with('#');
    let glyph = if file_like {
        Sym::File
    } else {
        Sym::PullRequest
    };
    div()
        .max_w_full()
        .min_w_0()
        .overflow_hidden()
        .h(REF_HEIGHT)
        .flex()
        .items_center()
        .gap(space::S4)
        .n_callout()
        .text_color(k.secondary)
        .child(
            div()
                .flex_none()
                .child(ui::sym(glyph, ui::SymSize::INLINE_SMALL, k.secondary)),
        )
        .when(file_like, |d| {
            // A path cuts in the middle, so the file name stays (§2.21);
            // the whole path is in the tooltip.
            d.child(
                ui::truncating(id.clone(), first.to_string())
                    .text_ellipsis_middle()
                    .code_font()
                    .text_size(crate::ui::native::tokens::text::META.0)
                    .text_color(k.label),
            )
            .when(!rest.is_empty(), |d| {
                d.child(div().flex_none().child(rest.to_string()))
            })
        })
        .when(!file_like, |d| {
            d.child(ui::truncating(id, label.to_string()))
        })
}

impl Render for SideChat {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        // Empty: the Empty state centered in the body (§2.5). Else the
        // aside reads from the top (short threads stay top-anchored, A1),
        // and the scroll content ends 24 above the composer (§2.21).
        let body = if thread.messages.is_empty() {
            let read = self.read_upto(&g, &t, None);
            div()
                .flex_1()
                .min_h_0()
                .child(self.empty(&name, read, cx))
                .into_any_element()
        } else {
            let context = self
                .read_upto(&g, &t, first_question)
                .map(|at| self.context_line(c::SIDE_CHAT_READ_UPTO, &name, at, cx));
            let rows: Vec<AnyElement> = thread
                .messages
                .iter()
                .map(|msg| {
                    if Some(&msg.author_id) == me.as_ref() {
                        self.question(msg, cx)
                    } else {
                        self.answer(&target, msg, cx)
                    }
                })
                .collect();
            div()
                .id("side-chat-thread")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .child(
                    div()
                        .pt(space::S16)
                        .px(metrics::INSPECTOR_PAD)
                        .pb(space::S24)
                        .flex()
                        .flex_col()
                        .gap(space::S16)
                        .children(context)
                        .children(rows),
                )
                .into_any_element()
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(native_colors(cx).content)
            .child(self.header(member.as_ref(), window, cx))
            .child(body)
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
