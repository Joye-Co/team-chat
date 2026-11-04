//! 单独问 (SideChat.dc.html, ⌘;): a private aside with one member about the
//! open 话题. It reads the 话题 up to now; answers stay private; a suggestion
//! posts into the 话题 only when pressed.
//!
//! Layout (400 pt, `sidebar` fill, NATIVE.md §9.2–§9.3): a 52 pt header
//! (单独问, the member pop-up button, a borderless close), the privacy line,
//! the aside anchored to the bottom (a centered context line, my questions
//! as bubbles at the right, answers under the transcript's sender line,
//! numbered points with their references as symbol + text runs, the
//! suggestion as a push button), and the transcript's composer field.

pub mod scenes;

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _,
    IntoElement, ParentElement, Pixels, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _,
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
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
            NativeType as _, PushSize, PushStyle, borderless_button, native_colors, popup_button,
            push_button,
        },
        sf_symbols::{Sf, Weight},
    },
    views::shell::{PaneView, clock, panes::watch},
};

/// Width of my question bubbles.
const BUBBLE_MAX: Pixels = px(280.);
/// The number column of a point in an answer (`1.`).
const POINT_NUMBER: Pixels = px(16.);
/// A reference under a point: 20 pt.
const REF_HEIGHT: Pixels = px(20.);
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

    /// The 52 pt header (NATIVE.md §9.3): 单独问 in Headline, the member as
    /// a pop-up button, and a borderless `xmark` close at the trailing edge.
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
            PushSize::Regular,
            window,
            cx,
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
            // The 52 pt toolbar row: Title 3 like the window's toolbar title.
            .h(crate::ui::native::metrics::TOOLBAR_ROW)
            .pl(space::S16)
            .pr(space::S10)
            .flex()
            .items_center()
            .gap(space::S8)
            .child(
                div()
                    .n_title3()
                    .text_color(k.label)
                    .child(c::SIDE_CHAT_TITLE),
            )
            .child(picker)
            .child(div().flex_1())
            .child(
                borderless_button("side-chat-close", Sf::Close, c::SIDE_CHAT_CLOSE, window, cx)
                    .on_click(move |_, _, cx| {
                        services
                            .nav
                            .update(cx, |nav, cx| nav.set_right_pane(RightPane::None, cx))
                    }),
            )
    }

    /// The privacy line: `lock` and `secondary` Subheadline text, no box.
    fn note(&self, cx: &App) -> impl IntoElement {
        let k = native_colors(cx);
        div()
            .flex_none()
            .px(space::S16)
            .h(px(20.))
            .flex()
            .items_center()
            .gap(space::S6)
            .n_subheadline()
            .text_color(k.secondary)
            .child(ui::sym(Sym::Lock, ui::SymSize::INLINE_SMALL, k.secondary))
            .child(c::SIDE_CHAT_PRIVATE)
    }

    /// `Jojo 读过话题里 10:20 之前的内容`: centered `secondary` Subheadline,
    /// no rules (NATIVE.md §9.2 day separators).
    fn context_line(&self, template: &str, name: &str, at: Millis, cx: &App) -> AnyElement {
        let k = native_colors(cx);
        let text = fill(template, &[("name", name)]);
        let text = text.replace("{time}", &clock(at));
        ui::motion::rise(
            "side-chat-context",
            0,
            div()
                .flex()
                .justify_center()
                .n_subheadline()
                .n_figures()
                .text_color(k.secondary)
                .child(text),
            cx,
        )
    }

    fn question(&self, msg: &SideChatMessage, ix: usize, cx: &App) -> AnyElement {
        let n = native_colors(cx);
        ui::motion::rise(
            SharedString::from(format!("side-chat-q-{}", msg.id)),
            ix,
            div().flex().flex_col().items_end().child(
                div()
                    .max_w(BUBBLE_MAX)
                    .px(space::S12)
                    .py(px(7.))
                    .rounded(px(18.))
                    .bg(n.fill_hover)
                    .n_message()
                    .text_color(n.label)
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
        let member = self.services.provider.member(&msg.author_id);
        let id = |part: &str| SharedString::from(format!("side-chat-a-{}-{part}", msg.id));
        let n = native_colors(cx);
        let time = clock(msg.at);
        let run = match member.as_ref().and_then(|m| m.role.clone()) {
            Some(role) if !role.trim().is_empty() => fill(
                crate::copy::topic_rail::ROLE_TIME,
                &[("role", role.trim()), ("time", &time)],
            ),
            _ => time,
        };
        // The sender line of the transcript: Headline name, then 岗位 and
        // time as one `secondary` run (no tag pill).
        let head = div()
            .flex()
            .items_center()
            .gap(space::S8)
            .child(ui::MemberAvatar::new(member.as_ref(), px(20.)))
            .child(
                div()
                    .n_headline()
                    .text_color(n.label)
                    .child(member.as_ref().map(|m| m.name.clone()).unwrap_or_default()),
            )
            .child(
                div()
                    .min_w_0()
                    .one_line()
                    .n_subheadline()
                    .n_figures()
                    .text_color(n.secondary)
                    .child(run),
            );

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
                        div().n_message().text_color(n.label).child(text.clone()),
                        cx,
                    ));
                }
                Block::Point(nn, text) => {
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
        let k = native_colors(cx);
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
                    .min_h(px(24.))
                    .flex()
                    .items_center()
                    .gap(space::S6)
                    .n_callout()
                    .text_color(k.secondary)
                    .child(ui::sym(Sym::Done, ui::SymSize::INLINE_SMALL, k.green))
                    .child(c::SIDE_CHAT_POSTED)
                    .child(div().n_figures().text_color(k.tertiary).child(when)),
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
            .flex_col()
            .items_start()
            .gap(space::S4)
            .child(
                push_button(
                    SharedString::from(format!("side-chat-suggest-{}", msg.id)),
                    label,
                    PushStyle::Standard,
                    PushSize::Regular,
                    cx,
                )
                .when(long, |b| b.tooltip(full))
                .on_click(move |_, _, _| {
                    services.provider.post_side_chat_suggestion(&g, &t, &m, &id)
                }),
            )
            .child(
                div()
                    .n_subheadline()
                    .text_color(k.secondary)
                    .child(c::SIDE_CHAT_SUGGEST_HINT),
            )
            .into_any_element()
    }

    fn composer(&self, name: &str, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
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
        let send = ready.then(|| {
            Button::new("side-chat-send")
                .custom(
                    ButtonCustomVariant::new(cx)
                        .color(n.accent)
                        .foreground(n.on_accent)
                        .hover(n.accent)
                        .active(n.accent.opacity(0.82)),
                )
                .with_size(px(24.))
                .size(px(24.))
                .p_0()
                .rounded(px(12.))
                .bg(n.accent)
                .tooltip(c::SIDE_CHAT_SEND)
                .child(ui::sym(
                    Sym::ArrowUp,
                    ui::SymSize::new(13., 14., Weight::Semibold),
                    n.on_accent,
                ))
                .on_click(cx.listener(|this, _, window, cx| this.send(window, cx)))
        });
        let dark = n.is_dark();
        div()
            .flex_none()
            .px(space::S16)
            .pb(space::S16)
            .child(
                div()
                    .id("side-chat-composer")
                    .w_full()
                    .min_h(px(36.))
                    .rounded(px(18.))
                    .map(|el| {
                        if dark {
                            el.bg(n.fill)
                        } else {
                            el.bg(n.content).border(px(0.5)).border_color(n.separator)
                        }
                    })
                    .cursor_text()
                    .pl(px(10.))
                    .pr(px(6.))
                    .flex()
                    .flex_col()
                    .children(chip_row.map(|row| div().pt(space::S8).pl(px(4.)).child(row)))
                    .child(
                        div()
                            .w_full()
                            .min_h(px(35.))
                            .flex()
                            .items_end()
                            .gap(space::S6)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .py(px(7.))
                                    .text_color(n.label)
                                    .child(editor),
                            )
                            .children(send.map(|b| div().flex_none().pb(px(6.)).child(b))),
                    ),
            )
            .into_any_element()
    }
}

impl PaneView for SideChat {
    fn width(&self, _window_w: Pixels) -> Pixels {
        crate::views::topic_rail::PANE_WIDE
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
fn ref_chip(label: &str, cx: &App) -> impl IntoElement {
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
            d.child(
                div()
                    .min_w_0()
                    .one_line()
                    .code_font()
                    .text_size(px(11.))
                    .text_color(k.label)
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
                .n_body()
                .text_color(native_colors(cx).secondary)
                .text_center()
                .child(fill(c::SIDE_CHAT_EMPTY, &[("name", &name)]))
        });

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(native_colors(cx).sidebar)
            .child(self.header(member.as_ref(), window, cx))
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
