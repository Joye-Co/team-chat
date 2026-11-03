//! Card v2: the generative cards of Cards.dc.html (DESIGN.md §7.10).
//!
//! [`render`] is the one entry point. The 首页, the 群 page, the topic
//! timeline, the pinned decision card and notification previews all call it
//! with the card's group and message id and a [`CardMode`]; the card looks
//! the same everywhere. Everything comes from the provider on every frame
//! (the card with its message context, the stored answers and my answer on
//! its way), so an edit, an answer or a refusal redraws the card in place.
//!
//! Anatomy: a status strip ([`state::CardState`]), a header (tile, name, 岗位,
//! `群 / 话题`, time), the title, the blocks (`blocks.rs`) and a footer with
//! the actions. Per-viewer controls follow `ask.from`: only the asked viewer
//! gets live controls; everyone sees the same card. What the viewer chose
//! or typed lives in a [`form::CardForm`] kept as keyed window state.
//!
//! Files: `state.rs` (state derivation), `form.rs` (choices, fields,
//! validation, the answer payload), `blocks.rs` (one renderer per block),
//! `chart.rs` (line, area, bar and spark charts), `confirm.rs` (the confirm
//! sheet with type-to-confirm), `line.rs` (one-line and notification looks),
//! `samples.rs`, `gallery.rs` and `scenes.rs` (the cards-v2 screenshot
//! scenes).

mod blocks;
mod chart;
mod confirm;
pub mod form;
pub mod gallery;
mod line;
pub mod samples;
pub mod scenes;
pub mod state;

use std::sync::Arc;

use gpui::{
    AnyElement, App, Entity, FontWeight, Hsla, InteractiveElement as _, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{Disableable as _, button::Button};

use self::{
    form::{CardForm, Preset, Problem, Values},
    state::{CardState, Viewer},
};
use crate::{
    app::{Navigation, RightPane, RootView},
    copy::{cards_v2 as c, fill},
    data::{
        CardPressError, ChatProvider, MemberId, MemberLevel, Millis, SessionState,
        cards_v2::{
            CardActionKind, CardV2, CardV2Action, CardV2ActionStyle, CardV2Answer,
            CardV2AnswerRequest, CardV2Block, CardV2Message, InputKind, RefTarget,
        },
    },
    theme::{self, Type as _, card_v2 as m, card_v2::mix, space},
    ui::{self, ButtonKind, ButtonSize, Lucide},
};

#[allow(unused_imports)]
pub use line::{notification_text, plan_row};

/// Where a card is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CardMode {
    /// 首页, 群 page and galleries: the full header with `群 / 话题`.
    Desk,
    /// In the card's own topic: the header drops the `群 / 话题` path.
    Topic,
    /// The pinned decision card above the composer of its topic.
    Pinned,
    /// The state overview: the strip and the title on one line.
    Line,
    /// The system notification banner (title, one line, the two actions).
    Notification,
}

impl CardMode {
    fn tag(self) -> &'static str {
        match self {
            CardMode::Desk => "desk",
            CardMode::Topic => "topic",
            CardMode::Pinned => "pin",
            CardMode::Line => "line",
            CardMode::Notification => "notify",
        }
    }
}

/// What a card needs from its host.
#[derive(Clone)]
pub struct CardEnv {
    pub provider: Arc<dyn ChatProvider>,
    /// Opens topics, 实时查看 and 产出. Without it the card looks for the
    /// main window's navigation when a link is pressed.
    pub nav: Option<Entity<Navigation>>,
    /// The viewer; `None` reads the signed-in member.
    pub viewer: Option<MemberId>,
    /// Values set before the first paint (screenshot scenes).
    pub preset: Option<Preset>,
    /// A fixed clock (screenshot scenes); `None` is now.
    pub now: Option<Millis>,
    /// Keeps form state apart when one card shows twice in a window.
    pub instance: Option<SharedString>,
    /// A notification preview shows its hover actions.
    pub hovered: bool,
}

impl CardEnv {
    pub fn new(provider: Arc<dyn ChatProvider>) -> Self {
        Self {
            provider,
            nav: None,
            viewer: None,
            preset: None,
            now: None,
            instance: None,
            hovered: false,
        }
    }

    /// A card on a page of the main window.
    pub fn from_services(services: &crate::app::Services) -> Self {
        Self {
            nav: Some(services.nav.clone()),
            ..Self::new(services.provider.clone())
        }
    }

    fn me(&self) -> Option<MemberId> {
        self.viewer
            .clone()
            .or_else(|| match self.provider.session() {
                SessionState::SignedIn { me, .. } => Some(me.id),
                _ => None,
            })
    }

    fn now(&self) -> Millis {
        self.now
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis())
    }
}

/// True when the provider has a v2 card for this message.
pub fn has_card(provider: &Arc<dyn ChatProvider>, group_id: &str, message_id: &str) -> bool {
    !message_id.is_empty() && provider.card_v2(group_id, message_id).is_some()
}

/// Draws the card of `message` wherever it shows: its v2 card, or else its
/// v1 card unchanged (1.0 bots). `None` when the message has no card.
pub fn render_message(
    env: &CardEnv,
    message: &crate::data::Message,
    mode: CardMode,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    if has_card(&env.provider, &message.group_id, &message.id) {
        return render(env, &message.group_id, &message.id, mode, window, cx);
    }
    let v1 = env.provider.card(&message.group_id, &message.id)?;
    let members = env.provider.members();
    let me = env.me();
    let ctx = crate::views::timeline::extras::ExtrasContext {
        provider: &env.provider,
        members: &members,
        window_messages: &[],
        me: me.as_deref(),
    };
    Some(super::render_card(&v1, message, &ctx, window, cx))
}

/// Draws the v2 card of a message, or `None` when it has none.
pub fn render(
    env: &CardEnv,
    group_id: &str,
    message_id: &str,
    mode: CardMode,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let msg = env.provider.card_v2(group_id, message_id)?;
    let answers = env.provider.card_v2_answers(group_id, message_id);
    let pending = env.provider.card_v2_pending(group_id, message_id);
    let card = msg.card.clone();

    let me = env.me();
    let me_member = me.as_ref().and_then(|id| env.provider.member(id));
    let requester = env
        .provider
        .topic(group_id, &msg.topic_id)
        .map(|t| t.created_by);
    let viewer = Viewer {
        id: me.as_deref(),
        is_owner: me_member
            .as_ref()
            .is_some_and(|m| m.level == MemberLevel::Owner),
        requester: requester.as_deref(),
    };

    let key = format!(
        "cv2-{}-{}-{}",
        mode.tag(),
        env.instance.as_deref().unwrap_or(""),
        message_id
    );
    let preset = env.preset.clone();
    let form = window.use_keyed_state(SharedString::from(key.clone()), cx, |_, _| {
        CardForm::new(&card, preset.as_ref())
    });
    // A newer rev resets the form, unless an edit is open: that edit is
    // superseded and says so.
    let (form_rev, editing) = {
        let f = form.read(cx);
        (f.rev, f.editing)
    };
    if form_rev < card.rev && editing.is_none() {
        form.update(cx, |f, _| f.reset(&card));
    }
    let local_rev = form.read(cx).editing;
    let state = state::derive(
        &card,
        &answers,
        pending.as_ref(),
        local_rev,
        &viewer,
        env.now(),
    );
    let values = form.read(cx).values(cx);
    let author = env
        .provider
        .member(&msg.author_id)
        .map(|m| crate::copy::isolate(&m.name))
        .unwrap_or_default();
    let ctx = Ctx {
        env,
        msg: &msg,
        card: &card,
        answers: &answers,
        state: &state,
        form: &form,
        values,
        me: me.clone(),
        author,
        mode,
        key,
    };
    record(&ctx);
    Some(match mode {
        CardMode::Line => line::overview(&ctx, cx),
        CardMode::Notification => line::notification(&ctx, window, cx),
        _ => full(&ctx, window, cx),
    })
}

/// What the parts of one card read while they render.
pub(crate) struct Ctx<'a> {
    pub env: &'a CardEnv,
    pub msg: &'a CardV2Message,
    pub card: &'a Arc<CardV2>,
    pub answers: &'a [CardV2Answer],
    pub state: &'a CardState,
    pub form: &'a Entity<CardForm>,
    /// The viewer's values now.
    pub values: Values,
    pub me: Option<MemberId>,
    /// The author's name.
    pub author: String,
    pub mode: CardMode,
    /// Element id prefix, unique per card and mode.
    pub key: String,
}

impl Ctx<'_> {
    pub fn id(&self, part: impl std::fmt::Display) -> SharedString {
        SharedString::from(format!("{}-{part}", self.key))
    }

    /// A member's name, isolated for mixed-direction lines.
    pub fn name(&self, id: &str) -> String {
        self.env
            .provider
            .member(id)
            .map(|m| crate::copy::isolate(&m.name))
            .unwrap_or_default()
    }

    pub fn answerer(&self) -> Answerer {
        Answerer {
            provider: self.env.provider.clone(),
            nav: self.env.nav.clone(),
            group_id: self.msg.group_id.clone(),
            topic_id: self.msg.topic_id.clone(),
            message_id: self.msg.message_id.clone(),
            card: self.card.clone(),
            form: self.form.clone(),
        }
    }

    /// The input blocks are live for the viewer.
    pub fn live(&self) -> bool {
        self.state.live()
    }

    /// True when the card is a quick choice: one decision with two short
    /// options and nothing else to fill in. A press answers.
    pub fn quick(&self) -> bool {
        quick(self.card)
    }
}

/// One decision with two short options and nothing else to fill in.
pub(crate) fn quick(card: &CardV2) -> bool {
    let blocks = card.all_blocks();
    let decisions: Vec<_> = blocks
        .iter()
        .filter_map(|b| match b {
            CardV2Block::Decision {
                options, multiple, ..
            } if !multiple.unwrap_or(false) => Some(options),
            _ => None,
        })
        .collect();
    let others = blocks.iter().any(|b| {
        matches!(b, CardV2Block::Input { .. })
            || matches!(
                b,
                CardV2Block::Draft {
                    editable: Some(true),
                    ..
                }
            )
            || matches!(
                b,
                CardV2Block::Decision {
                    multiple: Some(true),
                    ..
                }
            )
    });
    let submits = card
        .actions()
        .iter()
        .filter(|a| a.kind() == CardActionKind::Submit)
        .count();
    decisions.len() == 1
        && !others
        && submits <= 1
        && decisions[0].len() == 2
        && decisions[0].iter().all(|o| o.label.chars().count() <= 16)
}

/// The submit action a decision press or a form submit sends.
pub(crate) fn submit_action(card: &CardV2) -> Option<&CardV2Action> {
    let submits: Vec<&CardV2Action> = card
        .actions()
        .iter()
        .filter(|a| a.kind() == CardActionKind::Submit)
        .collect();
    submits
        .iter()
        .find(|a| a.style == Some(CardV2ActionStyle::Primary))
        .or(submits.first())
        .copied()
}

/// The card has blocks that take input (a decision, a field, an editable
/// draft).
pub(crate) fn takes_input(card: &CardV2) -> bool {
    card.blocks.iter().any(CardV2Block::takes_input)
}

fn has_draft(card: &CardV2) -> bool {
    card.all_blocks().iter().any(|b| {
        matches!(
            b,
            CardV2Block::Draft {
                editable: Some(true),
                ..
            }
        )
    })
}

/// The card has form fields (the optional note of a decision does not
/// count: the footer draws it).
fn has_fields(card: &CardV2) -> bool {
    let note = blocks::footer_note_input(card).map(|(id, _)| id);
    card.all_blocks().iter().any(|b| match b {
        CardV2Block::Input { id, .. } => note.as_deref() != Some(id.as_str()),
        _ => false,
    })
}

/// Sends answers and follows links for one card.
#[derive(Clone)]
pub(crate) struct Answerer {
    provider: Arc<dyn ChatProvider>,
    nav: Option<Entity<Navigation>>,
    group_id: String,
    topic_id: String,
    message_id: String,
    card: Arc<CardV2>,
    form: Entity<CardForm>,
}

impl Answerer {
    /// Presses `action`: follows a link, or answers (after the confirm sheet
    /// when the action or the ask asks for one).
    pub fn press(&self, action: &CardV2Action, window: &mut Window, cx: &mut App) {
        match action.kind() {
            CardActionKind::Link => {
                if let Some(t) = &action.target {
                    self.open(t, window, cx);
                }
            }
            CardActionKind::Unknown => {}
            kind => {
                let irreversible = self.card.ask.as_ref().and_then(|a| a.irreversible.clone());
                let guarded = kind == CardActionKind::Submit
                    && action.style.unwrap_or_default() != CardV2ActionStyle::Default
                    || submit_action(&self.card).is_some_and(|s| s.id == action.id);
                if kind == CardActionKind::Submit && guarded && irreversible.is_some()
                    || action.confirm.is_some()
                {
                    confirm::open(self.clone(), action.clone(), irreversible, window, cx);
                } else {
                    self.send(action, cx);
                }
            }
        }
    }

    /// Sends the answer with `action` now. A cancel sends no values.
    pub fn send(&self, action: &CardV2Action, cx: &mut App) {
        let values = self.form.read(cx).values(cx);
        let submit = action.kind() == CardActionKind::Submit;
        let request = CardV2AnswerRequest {
            rev: self.card.rev,
            action_id: action.id.clone(),
            inputs: submit
                .then(|| form::to_inputs(&self.card, &values))
                .flatten(),
            edited_values: submit
                .then(|| form::to_edits(&self.card, &values))
                .flatten(),
        };
        self.provider
            .answer_card_v2(&self.group_id, &self.topic_id, &self.message_id, request);
        self.form.update(cx, |f, cx| {
            if f.editing.is_some() {
                f.editing = None;
                cx.notify();
            }
        });
    }

    /// A quick decision: choose `option` and send the submit action.
    pub fn pick(&self, block: &str, option: &str, cx: &mut App) {
        self.form
            .update(cx, |f, cx| f.choose(block, option, false, cx));
        let action = submit_action(&self.card).cloned().unwrap_or(CardV2Action {
            id: "submit".into(),
            label: String::new(),
            style: None,
            kind: None,
            target: None,
            confirm: None,
        });
        self.send(&action, cx);
    }

    pub fn withdraw(&self, cx: &mut App) {
        self.provider
            .withdraw_card_v2_answer(&self.group_id, &self.topic_id, &self.message_id);
        let _ = cx;
    }

    /// Posts a message to the topic that mentions `member` (让 X 补上,
    /// 重新问).
    pub fn mention(&self, member: &str, name: &str, text: &str) {
        self.provider.send(crate::data::SendRequest {
            group_id: self.group_id.clone(),
            topic_id: self.topic_id.clone(),
            body: format!("@{name} {text}"),
            mentions: vec![member.to_string()],
            quote_of: Some(self.message_id.clone()),
        });
    }

    fn nav(&self, window: &mut Window, cx: &App) -> Option<Entity<Navigation>> {
        self.nav.clone().or_else(|| {
            window
                .root::<RootView>()
                .flatten()
                .map(|r| r.read(cx).services().nav.clone())
        })
    }

    pub fn open_topic(&self, group: &str, topic: &str, window: &mut Window, cx: &mut App) {
        if let Some(nav) = self.nav(window, cx) {
            let (g, t) = (group.to_string(), topic.to_string());
            nav.update(cx, |nav, cx| nav.open_topic(g, t, cx));
        }
    }

    /// 实时查看 of the run in the card's topic.
    pub fn open_live(&self, run_id: Option<&str>, window: &mut Window, cx: &mut App) {
        let Some(nav) = self.nav(window, cx) else {
            return;
        };
        let run = run_id.map(str::to_string).or_else(|| {
            self.provider
                .runs(&self.group_id, &self.topic_id)
                .into_iter()
                .max_by_key(|r| r.updated_at)
                .map(|r| r.id)
        });
        let (g, t) = (self.group_id.clone(), self.topic_id.clone());
        nav.update(cx, |nav, cx| {
            nav.open_topic(g, t, cx);
            if let Some(run) = run {
                nav.set_right_pane(RightPane::Live(run), cx);
            }
        });
    }

    pub fn open(&self, target: &RefTarget, window: &mut Window, cx: &mut App) {
        match target {
            RefTarget::Pr { url, .. } | RefTarget::Url { url } => cx.open_url(url),
            RefTarget::Topic { group_id, topic_id } => {
                self.open_topic(group_id, topic_id, window, cx)
            }
            RefTarget::Artifact {
                artifact_id,
                version,
            } => {
                if let Some(nav) = self.nav(window, cx) {
                    let (id, v) = (artifact_id.clone(), *version);
                    nav.update(cx, |nav, cx| nav.open_artifact(id, v, cx));
                }
            }
            RefTarget::Member { .. } | RefTarget::Unknown => {}
        }
    }
}

// ---------------------------------------------------------------------------
// The full card

/// Tone of a strip: its mark color, label ink, fill and edge.
struct Tone {
    mark: Hsla,
    ink: Hsla,
    fill: Hsla,
    edge: Hsla,
}

fn tone(state: &CardState, k: &theme::Colors) -> Option<Tone> {
    let mixed = |c: Hsla, fill: f32, edge: f32, ink: Hsla| Tone {
        mark: c,
        ink,
        fill: mix(c, k.bg, fill),
        edge: mix(c, k.bg, edge),
    };
    Some(match state {
        CardState::Pending => mixed(k.you, m::YOU_FILL, m::YOU_EDGE, k.you_ink),
        CardState::Answered { .. } | CardState::Done { .. } => {
            mixed(k.ok, m::OK_FILL, m::OK_EDGE, k.ok_ink)
        }
        CardState::Executing { progress, .. } if progress.waiting => Tone {
            mark: k.ink_2,
            ink: k.ink_2,
            fill: k.sunken,
            edge: k.line,
        },
        CardState::Executing { .. } => mixed(k.live, m::LIVE_FILL, m::LIVE_EDGE, k.live_ink),
        CardState::Failed { .. } | CardState::SendFailed { .. } => {
            mixed(k.bad, m::OK_FILL, m::OK_EDGE, k.bad_ink)
        }
        CardState::WaitingOn | CardState::Sending { .. } => Tone {
            mark: k.ink_2,
            ink: k.ink_2,
            fill: k.sunken,
            edge: k.line,
        },
        CardState::Expired | CardState::Superseded => {
            mixed(k.ink_3, m::QUIET_FILL, m::QUIET_EDGE, k.ink_2)
        }
        CardState::Info => return None,
    })
}

/// The outer box: 1 pt edge, radius 12, `bg`, `lift`; amber edge and glow
/// while it waits on the viewer.
pub(crate) fn frame(waits: bool, cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    let base = div()
        .w_full()
        .rounded(theme::radius::LG)
        .bg(k.bg)
        .border_1()
        .overflow_hidden();
    if waits {
        base.border_color(mix(k.you, k.line, m::YOU_BORDER))
            .shadow(vec![
                gpui::BoxShadow {
                    color: m::alpha(k.you, 0.14),
                    offset: gpui::point(px(0.), px(1.)),
                    blur_radius: px(2.),
                    spread_radius: px(0.),
                    inset: false,
                },
                gpui::BoxShadow {
                    color: m::alpha(k.you, 0.22),
                    offset: gpui::point(px(0.), px(8.)),
                    blur_radius: px(20.),
                    spread_radius: px(-10.),
                    inset: false,
                },
            ])
    } else {
        base.border_color(k.line).shadow(k.lift())
    }
}

/// 12/18 text, the artboard's meta line.
pub(crate) fn meta(el: gpui::Div) -> gpui::Div {
    el.text_size(px(12.)).line_height(px(18.))
}

fn strip_icon(state: &CardState, t: &Tone, ctx: &Ctx, cx: &App) -> AnyElement {
    let glyph = |name: Lucide| ui::icon(name, px(16.), t.mark).into_any_element();
    match state {
        CardState::Pending => glyph(Lucide::CircleDot),
        CardState::WaitingOn => glyph(Lucide::Hourglass),
        CardState::Executing { progress, .. } if progress.waiting => glyph(Lucide::Hourglass),
        CardState::Sending { .. } => ui::motion::spinner().color(t.mark).into_any_element(),
        CardState::Answered { .. } | CardState::Done { .. } => ui::motion::check_draw(
            ctx.id("strip-check"),
            ui::icon(Lucide::CircleCheck, px(16.), t.mark),
            px(16.),
            cx,
        ),
        CardState::Executing { .. } => div()
            .flex_none()
            .size(px(16.))
            .flex()
            .items_center()
            .justify_center()
            .child(ui::motion::breathe(
                ctx.id("strip-dot"),
                div().size(px(8.)).rounded_full().bg(t.mark),
                cx,
            ))
            .into_any_element(),
        CardState::Failed { .. } | CardState::SendFailed { .. } => glyph(Lucide::CircleX),
        CardState::Expired => glyph(Lucide::Clock),
        CardState::Superseded => glyph(Lucide::Ban),
        CardState::Info => div().into_any_element(),
    }
}

/// The label and the detail of the strip.
fn strip_words(ctx: &Ctx, cx: &App) -> (String, String) {
    let author = ctx.author.as_str();
    let at = |a: &CardV2Answer| crate::views::shell::clock(a.at);
    let label_of = |a: &CardV2Answer| {
        ctx.card
            .actions()
            .iter()
            .find(|x| x.id == a.action_id)
            .map(|x| x.label.clone())
            .unwrap_or_default()
    };
    // `Joye 发布到生产 10:40`, or `Joye 拍板 10:40` once the action is gone.
    let answered_at = |a: &CardV2Answer| {
        let label = label_of(a);
        let name = ctx.name(&a.actor_id);
        if label.is_empty() {
            fill(c::STRIP_DECIDED_AT, &[("name", &name), ("time", &at(a))])
        } else {
            fill(
                c::STRIP_ANSWER_AT,
                &[("name", &name), ("label", &label), ("time", &at(a))],
            )
        }
    };
    match ctx.state {
        CardState::Pending => {
            if has_fields(ctx.card) {
                let ready = form::problems(ctx.card, &ctx.values).is_empty();
                if ready {
                    (
                        c::STRIP_FILL_READY.into(),
                        c::STRIP_FILL_READY_DETAIL.into(),
                    )
                } else {
                    (
                        c::STRIP_FILL.into(),
                        fill(c::STRIP_WAITS, &[("name", author)]),
                    )
                }
            } else if has_draft(ctx.card) && ctx.form.read(cx).editing.is_some() {
                (
                    c::STRIP_EDITING.into(),
                    fill(c::STRIP_WAITS, &[("name", author)]),
                )
            } else {
                let detail = if ctx.quick() {
                    c::STRIP_WAITS_QUICK
                } else if has_draft(ctx.card) {
                    c::STRIP_WAITS_CONFIRM
                } else if takes_input(ctx.card) {
                    c::STRIP_WAITS_CHOOSE
                } else {
                    c::STRIP_WAITS_APPROVE
                };
                (c::CARD_NEEDS_YOU.into(), fill(detail, &[("name", author)]))
            }
        }
        CardState::WaitingOn => {
            let names = asked_names(ctx);
            (
                fill(c::CARD_WAITING_ON, &[("name", &names)]),
                c::STRIP_WAITING_ON_DETAIL.into(),
            )
        }
        CardState::Sending { .. } => (
            c::STRIP_SENDING.into(),
            fill(c::STRIP_SENDING_DETAIL, &[("name", author)]),
        ),
        CardState::SendFailed { error, .. } => (
            c::STRIP_SEND_FAILED.into(),
            match error {
                CardPressError::Gone => c::STRIP_SEND_GONE,
                CardPressError::Forbidden => c::STRIP_SEND_FORBIDDEN,
                CardPressError::Other => c::STRIP_SEND_OTHER,
            }
            .into(),
        ),
        CardState::Answered { answer } => {
            let who = ctx.name(&answer.actor_id);
            let kind = ctx
                .card
                .actions()
                .iter()
                .find(|x| x.id == answer.action_id)
                .map(CardV2Action::kind);
            if kind == Some(CardActionKind::Cancel) {
                (
                    c::STRIP_ANSWERED.into(),
                    fill(
                        c::STRIP_PICKED,
                        &[
                            ("name", &who),
                            ("label", &label_of(answer)),
                            ("time", &at(answer)),
                        ],
                    ),
                )
            } else if has_draft(ctx.card) {
                let t = if answer.edited_values.as_ref().is_some_and(|e| !e.is_empty()) {
                    c::STRIP_CONFIRMED_EDITED
                } else {
                    c::STRIP_CONFIRMED_DETAIL
                };
                (
                    c::STRIP_CONFIRMED.into(),
                    fill(t, &[("name", &who), ("time", &at(answer))]),
                )
            } else if has_fields(ctx.card) {
                (
                    c::STRIP_SUBMITTED.into(),
                    fill(
                        c::STRIP_SUBMITTED_DETAIL,
                        &[("name", &who), ("time", &at(answer))],
                    ),
                )
            } else {
                (
                    c::STRIP_ANSWERED.into(),
                    fill(c::STRIP_ANSWERED_DETAIL, &[("name", author)]),
                )
            }
        }
        CardState::Executing { progress, .. } if progress.waiting => (
            fill(c::CARD_WAITING_ON, &[("name", &asked_names(ctx))]),
            fill(
                c::STRIP_STOPPED,
                &[
                    ("n", &progress.step.unwrap_or(0).to_string()),
                    ("done", &progress.done.to_string()),
                    ("total", &progress.total.to_string()),
                ],
            ),
        ),
        CardState::Executing { answer, progress } => {
            let mut parts = Vec::new();
            let label = match answer {
                Some(a) => {
                    parts.push(answered_at(a));
                    c::STRIP_WORKING.to_string()
                }
                None => fill(c::STRIP_WORKING_SELF, &[("name", author)]),
            };
            if let Some(step) = progress.step {
                parts.push(fill(
                    c::STRIP_STEP,
                    &[
                        ("n", &step.to_string()),
                        ("total", &progress.total.to_string()),
                    ],
                ));
                if answer.is_none()
                    && let Some(cur) = &progress.current
                {
                    parts.push(cur.clone());
                }
            }
            (label, parts.join(" · "))
        }
        CardState::Done { answer } => (
            c::CARD_DONE.into(),
            match (answer.as_ref().map(answered_at), subtitle(ctx.card)) {
                (Some(a), s) if !s.is_empty() => format!("{a} · {s}"),
                (Some(a), _) => a,
                (None, s) => s,
            },
        ),
        CardState::Failed { .. } => (c::STRIP_FAILED.into(), failures(ctx.card)),
        CardState::Expired => (
            c::CARD_EXPIRED.into(),
            ctx.card
                .ask
                .as_ref()
                .and_then(|a| a.expires_at)
                .map(|t| {
                    fill(
                        c::STRIP_EXPIRED_DETAIL,
                        &[("time", &crate::views::shell::clock(t as Millis))],
                    )
                })
                .unwrap_or_default(),
        ),
        CardState::Superseded => (
            c::STRIP_SUPERSEDED.into(),
            fill(
                if has_draft(ctx.card) {
                    c::STRIP_SUPERSEDED_DRAFT
                } else {
                    c::STRIP_SUPERSEDED_DETAIL
                },
                &[("name", author)],
            ),
        ),
        CardState::Info => (String::new(), String::new()),
    }
}

/// The first line of the fallback when it says more than the title (the
/// bot's one-line subtitle for 做完了).
fn subtitle(card: &CardV2) -> String {
    let first = card.fallback.lines().next().unwrap_or("").trim();
    if first.is_empty() || first == card.title.trim() {
        String::new()
    } else {
        first.to_string()
    }
}

fn failures(card: &CardV2) -> String {
    card.all_blocks()
        .iter()
        .filter_map(|b| match b {
            CardV2Block::Evidence {
                label,
                state: Some(crate::data::cards_v2::EvidenceState::Fail),
                ..
            } => Some(label.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The names `ask.from` resolves to (`Joye`, `所有者`, the requester).
fn asked_names(ctx: &Ctx) -> String {
    let Some(ask) = &ctx.card.ask else {
        return String::new();
    };
    let p = &ctx.env.provider;
    let mut names: Vec<String> = Vec::new();
    for f in &ask.from {
        match f.as_str() {
            crate::data::cards_v2::ASK_FROM_OWNERS => names.extend(
                p.members()
                    .into_iter()
                    .filter(|m| m.level == MemberLevel::Owner && !m.deactivated)
                    .map(|m| m.name),
            ),
            crate::data::cards_v2::ASK_FROM_REQUESTER => {
                if let Some(t) = p.topic(&ctx.msg.group_id, &ctx.msg.topic_id) {
                    names.push(ctx.name(&t.created_by));
                }
            }
            id => names.push(ctx.name(id)),
        }
    }
    names.dedup();
    names.truncate(3);
    names.join(c::FORM_LIST_SEPARATOR)
}

/// The strip's right action, if any.
fn strip_action(ctx: &Ctx, cx: &App) -> Option<AnyElement> {
    let k = theme::colors(cx);
    let quiet = |id: SharedString, label: &str| {
        div()
            .id(id)
            .ml_auto()
            .flex_none()
            .h(px(24.))
            .px(space::S8)
            .rounded(theme::radius::SM)
            .flex()
            .items_center()
            .text_size(px(12.))
            .font_weight(theme::MEDIUM_WEIGHT)
            .text_color(k.ink_2)
            .cursor_pointer()
            .hover(|s| s.bg(k.hover))
            .child(label.to_string())
    };
    let a = ctx.answerer();
    match ctx.state {
        CardState::SendFailed { action_id, .. } => {
            let action = ctx
                .card
                .actions()
                .iter()
                .find(|x| &x.id == action_id)?
                .clone();
            Some(
                quiet(ctx.id("retry"), c::STRIP_RETRY)
                    .on_click(move |_, window, cx| a.press(&action, window, cx))
                    .into_any_element(),
            )
        }
        CardState::Failed { .. } => {
            let action = submit_action(ctx.card)?.clone();
            Some(
                quiet(ctx.id("retry"), c::STRIP_RETRY)
                    .text_color(k.bad_ink)
                    .on_click(move |_, window, cx| a.press(&action, window, cx))
                    .into_any_element(),
            )
        }
        CardState::Expired => {
            let (author, name) = (ctx.msg.author_id.clone(), ctx.author.clone());
            Some(
                quiet(ctx.id("ask-again"), c::STRIP_ASK_AGAIN)
                    .on_click(move |_, _, _| a.mention(&author, &name, c::ASK_AGAIN_MESSAGE))
                    .into_any_element(),
            )
        }
        CardState::Superseded => {
            let card = ctx.card.clone();
            let form = ctx.form.clone();
            Some(
                quiet(ctx.id("see-new"), c::STRIP_SEE_NEW)
                    .on_click(move |_, _, cx| {
                        form.update(cx, |f, cx| {
                            f.reset(&card);
                            cx.notify();
                        })
                    })
                    .into_any_element(),
            )
        }
        CardState::Answered { answer }
            if answer.edited_values.as_ref().is_some_and(|e| !e.is_empty()) =>
        {
            let form = ctx.form.clone();
            Some(
                quiet(ctx.id("see-edits"), c::STRIP_SEE_EDITS)
                    .text_color(k.ok_ink)
                    .on_click(move |_, _, cx| form.update(cx, |f, cx| f.toggle_expanded(cx)))
                    .into_any_element(),
            )
        }
        _ => None,
    }
}

/// The status strip: 34 pt, the state's tint, a glyph, the label in its
/// ink, the detail in `ink-2`, an optional action at the right, and for work
/// in progress a 2 pt live bar along the bottom edge.
pub(crate) fn strip(ctx: &Ctx, cx: &App) -> Option<AnyElement> {
    let k = theme::colors(cx);
    let t = tone(ctx.state, &k)?;
    let (label, detail) = strip_words(ctx, cx);
    let action = strip_action(ctx, cx);
    let bar = match ctx.state {
        CardState::Executing { progress, .. } if !progress.waiting => progress.fraction(),
        _ => None,
    };
    Some(
        meta(div())
            .relative()
            .flex_none()
            .h(m::STRIP)
            .pl(space::S16)
            .pr(if action.is_some() {
                space::S8
            } else {
                space::S16
            })
            .bg(t.fill)
            .border_b_1()
            .border_color(t.edge)
            .flex()
            .items_center()
            .gap(space::S8)
            .child(strip_icon(ctx.state, &t, ctx, cx))
            .child(
                div()
                    .flex_shrink(0.)
                    .max_w(gpui::relative(0.7))
                    .one_line()
                    .text_size(px(13.))
                    .line_height(px(20.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.ink)
                    .child(label),
            )
            .when(!detail.is_empty(), |el| {
                el.child(
                    div()
                        .min_w(px(0.))
                        .flex_shrink(1.)
                        .one_line()
                        .text_color(k.ink_2)
                        .child(detail),
                )
            })
            .children(action)
            .when_some(bar, |el, f| {
                el.child(
                    div()
                        .absolute()
                        .left_0()
                        .bottom(px(-1.))
                        .h(m::LIVE_BAR)
                        .w(gpui::relative(f.clamp(0.02, 1.)))
                        .bg(k.live),
                )
            })
            .into_any_element(),
    )
}

/// The header: tile 20, name 13/500, 岗位 tag, `群 / 话题` (a link) and the
/// mono time at the right.
fn header(ctx: &Ctx, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let p = &ctx.env.provider;
    let member = p.member(&ctx.msg.author_id);
    let path = (ctx.mode == CardMode::Desk).then(|| {
        let group = p
            .groups()
            .into_iter()
            .find(|g| g.id == ctx.msg.group_id)
            .map(|g| g.name)
            .unwrap_or_default();
        let topic = p
            .topic(&ctx.msg.group_id, &ctx.msg.topic_id)
            .map(|t| t.title)
            .map(|t| {
                if t.trim().is_empty() {
                    crate::copy::nav::UNTITLED.to_string()
                } else {
                    t
                }
            })
            .unwrap_or_default();
        // Both parts open the topic.
        let part = |key: &str, text: String| {
            let a = ctx.answerer();
            let (g, t) = (ctx.msg.group_id.clone(), ctx.msg.topic_id.clone());
            div()
                .id(ctx.id(key))
                .min_w(px(0.))
                .one_line()
                .cursor_pointer()
                .hover(|s| s.text_color(k.ink))
                .on_click(move |_, window, cx| a.open_topic(&g, &t, window, cx))
                .child(text)
        };
        // The path takes the room up to the time, so the group can be
        // held to 45 % of it: the group gives way first (it shrinks ten
        // times faster, down to one character and an ellipsis), then the
        // topic shrinks with its own ellipsis. Both stay readable.
        // A name of one or two characters never shrinks.
        let short = group.chars().count() <= 2;
        let both = !group.trim().is_empty() && !topic.trim().is_empty();
        div()
            .ml(px(2.))
            .min_w(px(0.))
            .flex_1()
            .flex()
            .overflow_hidden()
            .text_color(k.ink_3)
            .when(!group.trim().is_empty(), |el| {
                el.child(
                    part("path-group", group)
                        .when(short, |el| el.flex_none())
                        .when(!short, |el| {
                            el.min_w(m::PATH_GROUP_MIN)
                                .max_w(gpui::relative(0.45))
                                .flex_shrink(10.)
                        }),
                )
            })
            .when(both, |el| el.child(div().flex_none().px(space::S4).child("/")))
            .when(!topic.trim().is_empty(), |el| {
                el.child(
                    part("path-topic", topic)
                        .flex_shrink(1.)
                        .text_color(k.ink_2),
                )
            })
    });
    meta(div())
        .flex()
        .items_center()
        .gap(space::S8)
        .text_color(k.ink_3)
        .child(ui::MemberAvatar::new(member.as_ref(), m::TILE))
        .child(
            div()
                .flex_shrink(0.)
                .max_w(px(180.))
                .one_line()
                .text_size(px(13.))
                .font_weight(theme::MEDIUM_WEIGHT)
                .text_color(k.ink)
                .child(ctx.author.clone()),
        )
        .children(
            member
                .and_then(|m| m.role)
                .filter(|r| !r.trim().is_empty())
                .map(|r| div().flex_none().child(ui::JobTag::new(r))),
        )
        .children(path)
        .child(
            div()
                .ml_auto()
                .flex_none()
                .mono()
                .child(crate::views::shell::clock(
                    ctx.msg.edited_at.unwrap_or(ctx.msg.created_at),
                )),
        )
        .into_any_element()
}

fn title(ctx: &Ctx, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    div()
        .mt(space::S10)
        .text_size(m::TITLE.0)
        .line_height(m::TITLE.1)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(k.ink)
        .child(ctx.card.title.clone())
        .into_any_element()
}

/// The whole card: strip, header, title, blocks, footer.
fn full(ctx: &Ctx, window: &mut Window, cx: &mut App) -> AnyElement {
    let waits = matches!(ctx.state, CardState::Pending);
    let strip = strip(ctx, cx);
    let header = header(ctx, cx);
    let title = title(ctx, cx);
    let body = blocks::body(ctx, window, cx);
    let footer = footer(ctx, window, cx);
    let muted = ctx.state.muted();
    let card = frame(waits, cx)
        .flex()
        .flex_col()
        .when(ctx.mode == CardMode::Desk, |el| el.max_w(m::MAX_WIDTH))
        .children(strip)
        .child(
            div()
                .pt(space::S14)
                .px(space::S16)
                .pb(space::S16)
                .flex()
                .flex_col()
                .min_w(px(0.))
                .when(muted, |el| el.opacity(m::MUTED))
                .child(header)
                .child(title)
                .children(body),
        )
        .children(footer);
    ui::motion::fade_in(ctx.id("enter"), theme::motion::FAST, card, cx)
}

// ---------------------------------------------------------------------------
// Footer and buttons

/// A footer button: 32 pt, radius 8, 13/500.
pub(crate) fn button(
    id: SharedString,
    label: impl Into<SharedString>,
    kind: ButtonKind,
    cx: &App,
) -> Button {
    ui::button(id, label, kind, ButtonSize::Md, cx)
        .h(m::BUTTON)
        .text_size(px(13.))
        .line_height(px(20.))
}

/// A disabled primary: `sunken` fill, `ink-3` text, no shadow.
pub(crate) fn disabled_button(
    id: SharedString,
    label: impl Into<SharedString>,
    cx: &App,
) -> Button {
    let k = theme::colors(cx);
    button(id, label, ButtonKind::Primary, cx)
        .disabled(true)
        .bg(k.sunken)
        .text_color(k.ink_3)
        .shadow(vec![])
}

fn style_kind(action: &CardV2Action, first_default_used: &mut bool) -> ButtonKind {
    match action.style.unwrap_or_default() {
        CardV2ActionStyle::Primary => ButtonKind::Primary,
        CardV2ActionStyle::Danger => ButtonKind::Danger,
        _ if action.kind() == CardActionKind::Link || *first_default_used => ButtonKind::Ghost,
        _ => {
            *first_default_used = true;
            ButtonKind::Secondary
        }
    }
}

/// The footer note at the right: `meta` `ink-3`, with a lock glyph for
/// irreversible asks.
fn note(text: String, lock: bool, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    meta(div())
        .ml_auto()
        .min_w(px(0.))
        .flex_shrink(1.)
        .flex()
        .items_center()
        .gap(space::S6)
        .text_color(k.ink_3)
        .when(lock, |el| {
            el.child(ui::icon(Lucide::Lock, px(13.), k.ink_3))
        })
        .child(div().min_w(px(0.)).one_line().child(text))
        .into_any_element()
}

fn footer_bar(children: Vec<AnyElement>, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(space::S8)
        .px(space::S16)
        .py(space::S10)
        .border_t_1()
        .border_color(k.line)
        .bg(k.raised)
        .children(children)
        .into_any_element()
}

/// The footer for the card's state, or `None`.
fn footer(ctx: &Ctx, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    let actions: Vec<CardV2Action> = ctx
        .card
        .actions()
        .iter()
        .filter(|a| a.kind() != CardActionKind::Unknown)
        .cloned()
        .collect();
    let links: Vec<&CardV2Action> = actions
        .iter()
        .filter(|a| a.kind() == CardActionKind::Link)
        .collect();
    let answers: Vec<&CardV2Action> = actions
        .iter()
        .filter(|a| a.kind() != CardActionKind::Link)
        .collect();
    let a = ctx.answerer();
    let mut items: Vec<AnyElement> = Vec::new();

    let link_buttons = |items: &mut Vec<AnyElement>, cx: &App| {
        for (i, l) in links.iter().enumerate() {
            let (a, action) = (a.clone(), (*l).clone());
            items.push(
                button(
                    ctx.id(format!("link-{i}")),
                    l.label.clone(),
                    ButtonKind::Ghost,
                    cx,
                )
                .on_click(move |_, window, cx| a.press(&action, window, cx))
                .into_any_element(),
            );
        }
    };

    match ctx.state {
        CardState::Pending | CardState::SendFailed { .. } | CardState::WaitingOn => {
            let live = ctx.live();
            if ctx.quick() && links.is_empty() {
                return None;
            }
            if has_fields(ctx.card) {
                // 提交 with the note of what is missing.
                let problems = form::problems(ctx.card, &ctx.values);
                let submit = submit_action(ctx.card)?.clone();
                let ready = problems.is_empty() && live;
                items.push(if ready {
                    let a = a.clone();
                    button(
                        ctx.id("submit"),
                        submit.label.clone(),
                        ButtonKind::Primary,
                        cx,
                    )
                    .on_click(move |_, window, cx| a.press(&submit, window, cx))
                    .into_any_element()
                } else {
                    disabled_button(ctx.id("submit"), submit.label.clone(), cx).into_any_element()
                });
                for x in answers
                    .iter()
                    .filter(|x| x.kind() == CardActionKind::Cancel)
                {
                    let (a, action) = (a.clone(), (*x).clone());
                    items.push(
                        button(
                            ctx.id(format!("act-{}", x.id)),
                            x.label.clone(),
                            ButtonKind::Ghost,
                            cx,
                        )
                        .disabled(!live)
                        .on_click(move |_, window, cx| a.press(&action, window, cx))
                        .into_any_element(),
                    );
                }
                link_buttons(&mut items, cx);
                let missing = form::missing_labels(&problems);
                let invalid = problems.iter().find_map(|p| match p {
                    Problem::Invalid(l) => Some(l.clone()),
                    _ => None,
                });
                let text = if !live {
                    c::STRIP_WAITING_ON_DETAIL.to_string()
                } else if !missing.is_empty() {
                    fill(
                        c::FORM_MISSING,
                        &[("labels", &missing.join(c::FORM_LIST_SEPARATOR))],
                    )
                } else if let Some(l) = invalid {
                    fill(c::FORM_INVALID, &[("label", &l)])
                } else {
                    fill(c::FORM_READY, &[("name", &ctx.author)])
                };
                items.push(note(text, false, cx));
                return Some(footer_bar(items, cx));
            }
            if has_draft(ctx.card) {
                let submit = submit_action(ctx.card)?.clone();
                let editing = ctx.form.read(cx).editing.is_some();
                if editing {
                    let a2 = a.clone();
                    items.push(
                        button(
                            ctx.id("draft-done"),
                            fill(c::DRAFT_DONE, &[("label", &submit.label)]),
                            ButtonKind::Primary,
                            cx,
                        )
                        .disabled(!live)
                        .on_click(move |_, window, cx| a2.press(&submit, window, cx))
                        .into_any_element(),
                    );
                    let form = ctx.form.clone();
                    items.push(
                        button(
                            ctx.id("draft-cancel"),
                            crate::copy::common::CANCEL,
                            ButtonKind::Ghost,
                            cx,
                        )
                        .on_click(move |_, _, cx| form.update(cx, |f, cx| f.set_editing(false, cx)))
                        .into_any_element(),
                    );
                    items.push(note(c::DRAFT_REPLACES.into(), false, cx));
                    return Some(footer_bar(items, cx));
                }
                let a2 = a.clone();
                let s2 = submit.clone();
                items.push(
                    button(
                        ctx.id("draft-ok"),
                        submit.label.clone(),
                        ButtonKind::Primary,
                        cx,
                    )
                    .disabled(!live)
                    .on_click(move |_, window, cx| a2.press(&s2, window, cx))
                    .into_any_element(),
                );
                let form = ctx.form.clone();
                items.push(
                    button(
                        ctx.id("draft-edit"),
                        c::DRAFT_EDIT,
                        ButtonKind::Secondary,
                        cx,
                    )
                    .disabled(!live)
                    .on_click(move |_, _, cx| form.update(cx, |f, cx| f.set_editing(true, cx)))
                    .into_any_element(),
                );
                for x in answers.iter().filter(|x| x.id != submit.id) {
                    let (a, action) = (a.clone(), (*x).clone());
                    items.push(
                        button(
                            ctx.id(format!("act-{}", x.id)),
                            x.label.clone(),
                            ButtonKind::Ghost,
                            cx,
                        )
                        .disabled(!live)
                        .on_click(move |_, window, cx| a.press(&action, window, cx))
                        .into_any_element(),
                    );
                }
                link_buttons(&mut items, cx);
                items.extend(lock_note(ctx, live, cx));
                return Some(footer_bar(items, cx));
            }
            if takes_input(ctx.card) {
                // A decision with three or more options: an optional note
                // field and 提交, enabled once something is chosen.
                let submit = submit_action(ctx.card)?.clone();
                if let Some((id, placeholder)) = blocks::footer_note_input(ctx.card) {
                    items.push(blocks::note_field(
                        ctx,
                        &id,
                        placeholder.as_deref(),
                        live,
                        window,
                        cx,
                    ));
                }
                let ready = live && form::problems(ctx.card, &ctx.values).is_empty();
                items.push(if ready {
                    let a = a.clone();
                    button(
                        ctx.id("submit"),
                        submit.label.clone(),
                        ButtonKind::Primary,
                        cx,
                    )
                    .on_click(move |_, window, cx| a.press(&submit, window, cx))
                    .into_any_element()
                } else {
                    disabled_button(ctx.id("submit"), submit.label.clone(), cx).into_any_element()
                });
                link_buttons(&mut items, cx);
                return Some(footer_bar(items, cx));
            }
            // An approval: the card's buttons, one primary.
            let mut used = false;
            for x in &answers {
                let kind = style_kind(x, &mut used);
                let (a, action) = (a.clone(), (*x).clone());
                items.push(
                    button(ctx.id(format!("act-{}", x.id)), x.label.clone(), kind, cx)
                        .disabled(!live)
                        .on_click(move |_, window, cx| a.press(&action, window, cx))
                        .into_any_element(),
                );
            }
            link_buttons(&mut items, cx);
            items.extend(lock_note(ctx, live, cx));
            if items.is_empty() {
                return None;
            }
            Some(footer_bar(items, cx))
        }
        CardState::Sending { action_id } => {
            // The pressed control spins; the rest wait.
            if ctx.quick() || takes_input(ctx.card) {
                return None;
            }
            let mut used = false;
            for x in &answers {
                let kind = style_kind(x, &mut used);
                items.push(
                    button(ctx.id(format!("act-{}", x.id)), x.label.clone(), kind, cx)
                        .disabled(true)
                        .loading(&x.id == action_id)
                        .into_any_element(),
                );
            }
            Some(footer_bar(items, cx))
        }
        CardState::Executing { progress, .. } if progress.waiting && links.is_empty() => None,
        CardState::Executing { .. } => {
            // 看 X 的电脑 when the plan follows a run.
            let plan = ctx.card.all_blocks().iter().any(|b| {
                matches!(
                    b,
                    CardV2Block::Plan {
                        run_id: Some(_),
                        ..
                    }
                )
            });
            if !plan && links.is_empty() {
                return None;
            }
            if plan {
                let run = ctx.card.all_blocks().iter().find_map(|b| match b {
                    CardV2Block::Plan { run_id, .. } => run_id.clone(),
                    _ => None,
                });
                let k = theme::colors(cx);
                let a2 = a.clone();
                items.push(
                    div()
                        .id(ctx.id("watch"))
                        .flex()
                        .items_center()
                        .gap(space::S6)
                        .h(m::BUTTON)
                        .px(space::S6)
                        .rounded(theme::radius::MD)
                        .cursor_pointer()
                        .hover(|s| s.bg(k.hover))
                        .text_size(px(13.))
                        .text_color(k.ink_2)
                        .child(ui::icon(Lucide::Monitor, px(14.), k.ink_2))
                        .child(fill(c::PLAN_WATCH_COMPUTER, &[("name", &ctx.author)]))
                        .on_click(move |_, window, cx| a2.open_live(run.as_deref(), window, cx))
                        .into_any_element(),
                );
            }
            link_buttons(&mut items, cx);
            if plan {
                items.push(note(c::PLAN_IN_PLACE.into(), false, cx));
            }
            Some(footer_bar(items, cx))
        }
        CardState::Done { .. } | CardState::Failed { .. } | CardState::Info => {
            // A result keeps its reverse action at hand (回滚到 v2.13), and
            // its links.
            let mut used = false;
            let note_text = ctx.card.actions().iter().find_map(|x| {
                x.confirm
                    .as_ref()
                    .and_then(|c| c.body.clone())
                    .filter(|_| x.kind() == CardActionKind::Submit)
            });
            if ctx.card.ask.is_none() {
                for x in &answers {
                    let kind = style_kind(x, &mut used);
                    let (a, action) = (a.clone(), (*x).clone());
                    let reverse = x.kind() == CardActionKind::Submit
                        && x.style.unwrap_or_default() == CardV2ActionStyle::Default;
                    let mut b = button(ctx.id(format!("act-{}", x.id)), x.label.clone(), kind, cx)
                        .on_click(move |_, window, cx| a.press(&action, window, cx));
                    if reverse {
                        b = b.icon(ui::icon(Lucide::RotateCcw, px(14.), theme::colors(cx).ink));
                    }
                    items.push(b.into_any_element());
                }
            }
            link_buttons(&mut items, cx);
            if items.is_empty() {
                return None;
            }
            if let Some(t) = note_text {
                items.push(note(t, false, cx));
            }
            Some(footer_bar(items, cx))
        }
        CardState::Answered { .. } => {
            if links.is_empty() {
                return None;
            }
            link_buttons(&mut items, cx);
            Some(footer_bar(items, cx))
        }
        CardState::Expired | CardState::Superseded => None,
    }
}

/// `🔒 要输入名字确认` or `🔒 做了不能撤回` for an irreversible ask;
/// `只有被问的人能选` for other viewers.
fn lock_note(ctx: &Ctx, live: bool, cx: &App) -> Option<AnyElement> {
    if !live {
        return Some(note(c::STRIP_WAITING_ON_DETAIL.into(), false, cx));
    }
    let irr = ctx.card.ask.as_ref()?.irreversible.as_ref()?;
    Some(note(
        if irr.type_to_confirm.is_some() {
            c::LOCK_TYPE_NAME
        } else if has_draft(ctx.card) {
            c::LOCK_SEND
        } else {
            c::LOCK_IRREVERSIBLE
        }
        .into(),
        true,
        cx,
    ))
}

/// The record of an answer, by option letter or action label.
pub(crate) fn choice_text(
    card: &CardV2,
    answer_action: &str,
    inputs: Option<&crate::data::cards_v2::AnswerInputs>,
) -> String {
    let mut parts = Vec::new();
    for block in card.all_blocks() {
        if let CardV2Block::Decision { id, options, .. } = block
            && let Some(v) = inputs.and_then(|i| i.get(id))
        {
            let ids: Vec<String> = match v {
                serde_json::Value::String(s) => vec![s.clone()],
                serde_json::Value::Array(a) => a
                    .iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect(),
                _ => vec![],
            };
            for id in ids {
                if let Some((i, o)) = options.iter().enumerate().find(|(_, o)| o.id == id) {
                    parts.push(format!("{}：{}", letter(i), o.label));
                }
            }
        }
    }
    if parts.is_empty() {
        card.actions()
            .iter()
            .find(|a| a.id == answer_action)
            .map(|a| format!("「{}」", a.label))
            .unwrap_or_default()
    } else {
        parts.join(c::FORM_LIST_SEPARATOR)
    }
}

/// `A`, `B`, … for option `i`.
pub(crate) fn letter(i: usize) -> char {
    (b'A' + (i.min(25) as u8)) as char
}

/// The label of a select option or chip, by id.
pub(crate) fn option_label(card: &CardV2, block: &str, id: &str) -> Option<String> {
    card.all_blocks().iter().find_map(|b| match b {
        CardV2Block::Input {
            id: bid,
            options: Some(opts),
            kind: InputKind::Select | InputKind::Chips,
            ..
        } if bid == block => opts.iter().find(|o| o.id == id).map(|o| o.label.clone()),
        _ => None,
    })
}

// ---------------------------------------------------------------------------
// Self-test record

/// What the last renders showed, for tests and `--selftest-cards`-style
/// checks: message id → (state name, blocks drawn).
#[derive(Clone, Debug, Default)]
pub(crate) struct Drawn {
    pub cards: std::collections::HashMap<String, &'static str>,
}

thread_local! {
    pub(crate) static DRAWN: std::cell::RefCell<Option<Drawn>> = const { std::cell::RefCell::new(None) };
}

fn record(ctx: &Ctx) {
    DRAWN.with(|d| {
        if let Some(d) = d.borrow_mut().as_mut() {
            d.cards
                .insert(ctx.msg.message_id.clone(), state_name(ctx.state));
        }
    });
}

pub(crate) fn state_name(s: &CardState) -> &'static str {
    match s {
        CardState::Pending => "pending",
        CardState::WaitingOn => "waiting",
        CardState::Sending { .. } => "sent",
        CardState::SendFailed { .. } => "send-failed",
        CardState::Answered { .. } => "answered",
        CardState::Executing { .. } => "executing",
        CardState::Done { .. } => "done",
        CardState::Failed { .. } => "failed",
        CardState::Expired => "expired",
        CardState::Superseded => "superseded",
        CardState::Info => "info",
    }
}
