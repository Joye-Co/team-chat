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

pub(crate) mod blocks;
mod chart;
pub(crate) mod confirm;
pub mod form;
pub mod gallery;
mod line;
pub mod samples;
pub mod scenes;
pub mod state;

use std::sync::Arc;

use gpui::{
    AnyElement, App, Entity, FontWeight, InteractiveElement as _, IntoElement, ParentElement,
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
    theme::{self, Type as _, card_v2 as m, space},
    ui::{
        self, PushStyle, Sym,
        native::{BadgeState, NativeType as _},
    },
};

#[allow(unused_imports)]
pub use line::{notification_text, plan_row};
// --- inc-cards21 ---
pub(crate) use chart::{
    chart as chart_el, plot_left as chart_plot_left, y_from_top as chart_y_from_top,
};
// --- end inc-cards21 ---

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

    pub(crate) fn now(&self) -> Millis {
        self.now
            .unwrap_or_else(|| crate::selftest::clock::utc_now().timestamp_millis())
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
    // --- inc-cards21: 2.1 keeps local values across revs (INTERACT §4.4) ---
    let is21 = super::v21::is_21(&card);
    let prev = env.preset.as_ref().and_then(|p| p.prev.clone());
    let kept: Vec<String> = env
        .preset
        .as_ref()
        .map(|p| p.drafts_kept.iter().map(|s| s.to_string()).collect())
        .unwrap_or_default();
    let ui = window.use_keyed_state(SharedString::from(format!("{key}-ui")), cx, |_, _| {
        let mut u = super::v21::CardUi::new(prev);
        u.drafts_kept = kept.into_iter().collect();
        u
    });
    let seen = ui.read(cx).seen.clone();
    if is21 {
        ui.update(cx, |u, _| u.observe(&card, msg.cause.as_ref()));
    }
    if form_rev < card.rev && editing.is_none() {
        match seen.filter(|_| is21) {
            Some(old) => {
                let kept = form.update(cx, |f, cx| f.rebase(&old, &card, cx));
                let f = form.read(cx);
                let holds: Vec<String> = ui
                    .read(cx)
                    .drafts_kept
                    .iter()
                    .filter(|id| f.holds_text(id))
                    .cloned()
                    .collect();
                ui.update(cx, |u, _| {
                    u.keep_drafts(kept, &card, |id| holds.iter().any(|h| h == id))
                });
            }
            None => form.update(cx, |f, _| f.reset(&card)),
        }
    }
    // --- end inc-cards21 ---
    let local_rev = form.read(cx).editing;
    let state = state::derive(
        &card,
        &answers,
        pending.as_ref(),
        local_rev,
        &viewer,
        env.now(),
    );
    // --- inc-cards21 ---
    let state = super::v21::band::adjust(&card, state, &answers, &viewer, env.now());
    let open = msg.card_state.as_ref().map(|s| s.open_ask().is_some());
    let surface_live = super::v21::band::surface_live(&card, &state, &answers, &viewer, open);
    // --- end inc-cards21 ---
    let values = form.read(cx).values(cx);
    let author = env
        .provider
        .member(&msg.author_id)
        // D1-56: the header shows the avatar; a leading emoji in the name
        // would read as a second one (in the meta too).
        .map(|m| crate::copy::isolate(crate::ui::avatar::name_beside_avatar(&m.name)))
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
        ui,
        surface_live,
    };
    record(&ctx);
    Some(match mode {
        // A card at work is the one WorkingCard in the overview too (C-04).
        CardMode::Line => working(&ctx, window, cx).unwrap_or_else(|| line::overview(&ctx, cx)),
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
    // --- inc-cards21 ---
    /// Local UI state of the 2.1 blocks.
    pub ui: Entity<super::v21::CardUi>,
    /// The decision surface of a non-blocking ask is live for the viewer.
    pub surface_live: bool,
    // --- end inc-cards21 ---
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
        self.state.live() || self.surface_live
    }

    /// True when the card is a quick choice: one decision with two short
    /// options and nothing else to fill in. A press answers.
    pub fn quick(&self) -> bool {
        quick(self.card)
    }
}

/// One decision with two or three short options and nothing else to fill
/// in: they show as push buttons and a press answers.
pub(crate) fn quick(card: &CardV2) -> bool {
    let blocks = card.all_blocks();
    let decisions: Vec<_> = blocks
        .iter()
        .filter_map(|b| match b {
            CardV2Block::Decision {
                options,
                multiple,
                allow_other,
                ..
            } if !multiple.unwrap_or(false) && allow_other.is_none() => Some(options),
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
        && (2..=3).contains(&decisions[0].len())
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
        // --- inc-cards21: a 2.1 answer is `card.event action.press` with its inputs ---
        if super::v21::is_21(&self.card) {
            let inputs = matches!(
                action.kind(),
                CardActionKind::Submit | CardActionKind::Reject
            )
            .then(|| super::v21::form21::to_inputs(&self.card, &values, Some(action)))
            .flatten();
            self.provider.send_card_event(
                &self.group_id,
                &self.topic_id,
                &self.message_id,
                crate::data::cards_v2::CardEventRequest {
                    rev: self.card.rev,
                    action_id: Some(action.id.clone()),
                    event: "action.press".into(),
                    inputs,
                    ..Default::default()
                },
            );
            self.form.update(cx, |f, cx| {
                if f.editing.is_some() {
                    f.editing = None;
                    cx.notify();
                }
            });
            return;
        }
        // --- end inc-cards21 ---
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
            role: None,
            id: "submit".into(),
            label: String::new(),
            style: None,
            kind: None,
            target: None,
            confirm: None,
            inputs: None,
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

    /// 停止… of the run `run_id` in the card's topic: the same alert and
    /// stop as the thread's working card (`process::stop_run`).
    pub fn stop_run(&self, run_id: &str, window: &mut Window, cx: &mut App) {
        let Some(services) = window
            .root::<RootView>()
            .flatten()
            .map(|r| r.read(cx).services().clone())
        else {
            return;
        };
        if let Some(run) = self
            .provider
            .runs(&self.group_id, &self.topic_id)
            .into_iter()
            .find(|r| r.id == run_id)
        {
            crate::views::process::stop_run(&services, &run, cx);
        }
    }

    /// v11/pb-cards: scrolls the card's 话题 to message `id` and highlights it.
    pub fn open_message(&self, id: &str, window: &mut Window, cx: &mut App) {
        let Some(nav) = self.nav(window, cx) else {
            return;
        };
        let seq = self
            .provider
            .messages(&self.group_id, &self.topic_id, 400)
            .messages
            .iter()
            .find(|m| m.id == id)
            .map_or(0, |m| m.created_seq);
        let (g, t, id) = (self.group_id.clone(), self.topic_id.clone(), id.to_string());
        nav.update(cx, |nav, cx| nav.open_message(g, t, id, seq, cx));
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

/// The StatusBadge state of a card state (§2.12: one mapping everywhere; the
/// color sits on the 12 icon only, the words stay foreground).
fn badge_state(state: &CardState) -> Option<BadgeState> {
    Some(match state {
        CardState::Pending => BadgeState::NeedsYou,
        CardState::Answered { .. } | CardState::Done { .. } => BadgeState::Done,
        CardState::Executing { progress, .. } if progress.waiting => BadgeState::Queued,
        CardState::Executing { .. } | CardState::Sending { .. } => BadgeState::Working,
        CardState::Failed { .. } | CardState::SendFailed { .. } => BadgeState::Failed,
        CardState::WaitingOn => BadgeState::Queued,
        CardState::Expired | CardState::Superseded => BadgeState::Deactivated,
        CardState::Info => return None,
    })
}

/// The card container (NATIVE.md §9.2): radius 12, `fill`, no border, no
/// shadow. A card that waits on the viewer says so in its header (the
/// `amber` symbol and word), not with a glow.
pub(crate) fn frame(_waits: bool, cx: &App) -> gpui::Div {
    ui::native::cards::card_frame(cx)
}

/// 12/18 text, the artboard's meta line.
pub(crate) fn meta(el: gpui::Div) -> gpui::Div {
    el.text_size(crate::ui::native::tokens::text::META.0)
        .line_height(px(18.))
}

/// The label and the detail of the strip.
pub(crate) fn strip_words(ctx: &Ctx, cx: &App) -> (String, String) {
    // A name in a state line is cut first, so times and the state word stay.
    let author_short = short_name(&ctx.author);
    let author = author_short.as_str();
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
        let name = short_name(&ctx.name(&a.actor_id));
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
                // The §2.12 word of the ask (T, `topic_page::ask_words`): 等你审 for a
                // review, 等你回答 for any other ask.
                let word = ctx
                    .env
                    .provider
                    .asks_for_me()
                    .into_iter()
                    .find(|a| a.message_id.as_deref() == Some(ctx.msg.message_id.as_str()))
                    .map_or(c::CARD_NEEDS_YOU, |a| {
                        crate::views::topic_page::ask_words(a.kind)
                    });
                (word.into(), fill(detail, &[("name", author)]))
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
            let who = short_name(&ctx.name(&answer.actor_id));
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
        // The working card (§2.4): the badge says `在做 3/5`, the detail
        // `正在做：<current step>`, then who started it and when.
        CardState::Executing { answer, progress } => {
            let label = if progress.total > 0 {
                fill(
                    c::STRIP_WORKING_COUNT,
                    &[
                        ("n", &progress.done.to_string()),
                        ("total", &progress.total.to_string()),
                    ],
                )
            } else {
                c::STRIP_WORKING.to_string()
            };
            let mut parts = Vec::new();
            match &progress.current {
                Some(cur) => parts.push(fill(c::STRIP_DOING, &[("step", cur)])),
                None if answer.is_none() => {
                    parts.push(fill(c::STRIP_WORKING_SELF, &[("name", author)]))
                }
                None => {}
            }
            if let Some(a) = answer {
                parts.push(answered_at(a));
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
    names
        .iter()
        .map(|n| short_name(n))
        .collect::<Vec<_>>()
        .join(c::FORM_LIST_SEPARATOR)
}

/// The longest name a state line keeps whole (characters).
const SHORT_NAME: usize = 12;

/// `name` cut at [`SHORT_NAME`] characters with `…`: in a state line the name
/// gives way first, so `拍板` and the times stay (§2.21 truncation order).
pub(crate) fn short_name(name: &str) -> String {
    if name.chars().count() <= SHORT_NAME {
        return name.to_string();
    }
    let mut s: String = name.chars().take(SHORT_NAME - 1).collect();
    s.push('…');
    s
}

/// The state's action at the header's trailing edge, if any: a small
/// standard push button (NATIVE.md §5.2).
fn strip_action(ctx: &Ctx, cx: &App) -> Option<AnyElement> {
    let quiet = |id: SharedString, label: &str| {
        ui::native::push_button(
            id,
            label.to_string(),
            ui::native::PushStyle::Outline,
            ui::native::PushSize::Small,
            cx,
        )
        .flex_none()
    };
    let a = ctx.answerer();
    match ctx.state {
        CardState::SendFailed { action_id, .. } => {
            let action = ctx
                .card
                .actions()
                .iter()
                .find(|x| &x.id == action_id)
                .cloned();
            // A 2.1 answer of a block (an immediate commit): the footer's 重试 sends
            // the same answer again (CARD21 §2); one 重试 per card (§2.15).
            let action = action.filter(|_| !super::v21::is_21(ctx.card))?;
            crate::selftest::drawn::mark("card-band-retry", ctx.msg.message_id.clone());
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
                    .on_click(move |_, _, cx| form.update(cx, |f, cx| f.toggle_expanded(cx)))
                    .into_any_element(),
            )
        }
        _ => None,
    }
}

/// The state of a card as header parts: the StatusBadge state and words,
/// the detail, a right note, an action and the progress of work.
pub(crate) struct StripParts {
    /// The badge (`None`: a rev a member's event caused, drawn with the
    /// refresh icon).
    pub state: Option<BadgeState>,
    pub label: String,
    pub detail: String,
    pub right: Option<String>,
    pub action: Option<AnyElement>,
    pub bar: Option<f32>,
    /// `3/5` under the bar of work in progress.
    pub steps: Option<(usize, usize)>,
}

impl StripParts {
    /// The StatusBadge of the state (§2.12), its label cut at half a card.
    pub(crate) fn badge(&self, cx: &App) -> AnyElement {
        let n = ui::native::native_colors(cx);
        match self.state {
            Some(state) => ui::native::StatusBadge::new(state, self.label.clone())
                .max_w(m::MAX_WIDTH / 2.)
                .into_any_element(),
            None => ui::native::Badge::new(self.label.clone())
                .icon(Sym::Refresh, n.secondary)
                .max_w(m::MAX_WIDTH / 2.)
                .into_any_element(),
        }
    }
}

/// The state parts of the card's header, or `None` for a card without a
/// state (info).
pub(crate) fn strip_parts(ctx: &Ctx, cx: &App) -> Option<StripParts> {
    // --- inc-cards21: my live event waits, 2.1 words, the deadline ---
    // inc-integrate: a page event of an embed in this card waits too (the page host).
    // v11/pb2-cards: the state says the stage (INTERACT §4.4): working with the spinner
    // while the author has it, queued once no answer came, 失败 when the step was not sent.
    use super::v21::band::PendTone;
    let waiting = super::v21::band::pending(ctx)
        .map(|p| (p.label, p.detail, p.tone))
        .or_else(|| {
            super::v21::band::page_pending(ctx, cx).map(|(l, tone)| (l, String::new(), tone))
        });
    // v11/r3-cards: a rev a member's event caused (Interact.dc.html 02 step 3).
    let updated = waiting
        .is_none()
        .then(|| super::v21::band::updated(ctx))
        .flatten();
    let state = match waiting.as_ref().map(|w| w.2) {
        _ if updated.is_some() => None,
        Some(PendTone::Live) => Some(BadgeState::Working),
        Some(PendTone::Quiet) => Some(BadgeState::Queued),
        Some(PendTone::Bad) => Some(BadgeState::Failed),
        None => Some(badge_state(ctx.state)?),
    };
    let (label, detail) = match (&waiting, &updated) {
        (Some((l, d, _)), _) => (l.clone(), d.clone()),
        (None, Some((l, d))) => (l.clone(), d.clone()),
        (None, None) => super::v21::band::words(ctx, strip_words(ctx, cx)),
    };
    let right = (waiting.is_none() && updated.is_none())
        .then(|| super::v21::band::right(ctx))
        .flatten();
    let action = if waiting.is_some() || updated.is_some() {
        None
    } else {
        strip_action(ctx, cx)
    };
    // --- end inc-cards21 ---
    let (bar, steps) = match ctx.state {
        CardState::Executing { progress, .. } if !progress.waiting => (
            progress.fraction(),
            (progress.total > 0).then_some((progress.done, progress.total)),
        ),
        _ => (None, None),
    };
    Some(StripParts {
        state,
        label,
        detail,
        right,
        action,
        bar,
        steps,
    })
}

/// The Progress of work (D12) with `3/5` in 12 muted tabular figures.
fn work_progress(f: f32, steps: Option<(usize, usize)>, cx: &App) -> AnyElement {
    let n = ui::native::native_colors(cx);
    div()
        .flex()
        .items_center()
        .gap(space::S8)
        .child(
            div()
                .flex_1()
                .child(ui::native::progress(f.clamp(0.02, 1.), cx)),
        )
        .when_some(steps, |el, (done, total)| {
            el.child(
                div()
                    .flex_none()
                    .n_subheadline()
                    .n_figures()
                    .text_color(n.secondary)
                    .child(format!("{done}/{total}")),
            )
        })
        .into_any_element()
}

/// The muted detail after the badge: one line, cut at the end, the full
/// text in a tooltip (§2.21).
fn detail_text(detail: String, cx: &App) -> AnyElement {
    let n = ui::native::native_colors(cx);
    ui::truncate::TruncText::new(detail)
        .flex_1()
        .n_subheadline()
        .text_color(n.secondary)
        .into_any_element()
}

/// The state line on its own (the one-line overview): badge, detail, right
/// note and action, then the Progress of work.
pub(crate) fn strip(ctx: &Ctx, cx: &App) -> Option<AnyElement> {
    let n = ui::native::native_colors(cx);
    let p = strip_parts(ctx, cx)?;
    Some(
        div()
            .flex()
            .flex_col()
            .gap(space::S8)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .child(p.badge(cx))
                    .child(detail_text(p.detail.clone(), cx))
                    .children(p.action)
                    .when_some(p.right, |el, r| {
                        el.child(
                            div()
                                .flex_none()
                                .one_line()
                                .n_subheadline()
                                .n_figures()
                                .text_color(n.secondary)
                                .child(r),
                        )
                    }),
            )
            .when_some(p.bar, |el, f| el.child(work_progress(f, p.steps, cx)))
            .into_any_element(),
    )
}

/// The header (NATIVE.md §9.2): the member name in Headline and the state
/// as a symbol and its word, then the time. In the card's own topic the
/// transcript's sender block already says who and when, so the header is
/// the state alone. On the 首页 a second `secondary` line says where:
/// `群 · 话题` (opens the topic). Work in progress adds a 4 pt bar.
fn header(ctx: &Ctx, cx: &App) -> Option<AnyElement> {
    let n = ui::native::native_colors(cx);
    let p = &ctx.env.provider;
    let parts = strip_parts(ctx, cx);
    let in_topic = ctx.mode == CardMode::Topic;
    if in_topic && parts.is_none() && !super::v21::interact::conflicted(ctx) {
        return None;
    }
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
        let text = [group, topic]
            .into_iter()
            .filter(|t| !t.trim().is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        let a = ctx.answerer();
        let (g, t) = (ctx.msg.group_id.clone(), ctx.msg.topic_id.clone());
        div()
            .id(ctx.id("path"))
            .pl(ui::native::cards::card::AVATAR + space::S8)
            .min_w(px(0.))
            .flex()
            .on_click(move |_, window, cx| a.open_topic(&g, &t, window, cx))
            .child(
                // `群 · 话题` cuts in the middle: the 群 and the end of the
                // 话题 (a file name's extension) both stay (§2.21).
                ui::truncate::TruncText::new(text)
                    .middle()
                    .id(ctx.id("path-text"))
                    .n_subheadline()
                    .text_color(n.secondary),
            )
    });
    // Header slots (§2.4): StatusBadge, the muted detail, the action, then
    // one time column at the far right. On the 首页 the time is when the
    // message was sent; a deadline or a failure time joins the detail there.
    let time = (!in_topic).then(|| crate::views::shell::clock(ctx.msg.created_at));
    let (badge, detail, right, action, bar, steps) = match parts {
        Some(p) => {
            let badge = p.badge(cx);
            let (detail, right) = match (p.right, &time) {
                (Some(r), Some(_)) if p.detail.trim().is_empty() => (r, None),
                (Some(r), Some(_)) => (format!("{} · {r}", p.detail), None),
                (r, _) => (p.detail, r),
            };
            (Some(badge), detail, right, p.action, p.bar, p.steps)
        }
        None => (None, String::new(), None, None, None, None),
    };
    let row = div()
        .flex()
        .items_center()
        .gap(space::S8)
        .min_h(ui::native::cards::card::AVATAR)
        .when(!in_topic, |el| {
            el.child(ui::MemberAvatar::new(
                member.as_ref(),
                ui::native::cards::card::AVATAR,
            ))
            .child(
                ui::truncate::TruncText::new(ctx.author.clone())
                    .id(ctx.id("author"))
                    .flex_shrink_0()
                    .max_w(m::MAX_WIDTH / 4.)
                    .n_headline()
                    .text_color(n.label),
            )
        })
        .children(badge)
        .child(detail_text(detail, cx))
        .children(action)
        .children(right.or(time).map(|t| {
            div()
                .flex_none()
                .one_line()
                .n_subheadline()
                .n_figures()
                .text_color(n.secondary)
                .child(t)
        }));
    // The card changed while I answered: a muted line in the header stays
    // while the Toast (once, with 看新版) says what happened.
    let conflict = super::v21::interact::conflicted(ctx).then(|| {
        div()
            .pt(space::S4)
            .flex()
            .items_center()
            .gap(space::S4)
            .n_subheadline()
            .text_color(n.secondary)
            .child(Sym::Alert.inline_small().text_color(n.secondary))
            .child(crate::copy::cards21::CONFLICT_TOAST)
    });
    Some(
        div()
            .flex()
            .flex_col()
            .gap(space::S2)
            .child(row)
            .children(path)
            .children(conflict)
            .when_some(bar, |el, f| {
                el.child(div().pt(space::S8).child(work_progress(f, steps, cx)))
            })
            .into_any_element(),
    )
}

fn title(ctx: &Ctx, has_header: bool, cx: &App) -> AnyElement {
    let n = ui::native::native_colors(cx);
    div()
        .when(has_header, |el| el.mt(ui::native::cards::card::STACK))
        // A compact card's title: 14 semibold (§2.4).
        .n_headline()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(n.label)
        // A title is one line of text: line breaks and runs of spaces collapse
        // (break-ui), and it wraps at most to 2 lines.
        .line_clamp(2)
        .child(one_line_text(&ctx.card.title))
        // --- inc-cards21: the 2.1 summary line under the title ---
        .when_some(
            ctx.card
                .summary
                .clone()
                .filter(|_| super::v21::is_21(ctx.card))
                .filter(|_| !super::v21::band::summary_in_band(ctx)),
            |el, s| {
                el.child(
                    div()
                        .mt(space::S4)
                        .n_message()
                        .font_weight(FontWeight::NORMAL)
                        .text_color(n.secondary)
                        .child(s),
                )
            },
        )
        .into_any_element()
}

/// A card that is not fetched yet (§2.14): the card frame with skeletons in
/// the final shapes: the 24 author circle and the header line, the title line,
/// then two body lines.
pub(crate) fn loading_card(cx: &App) -> AnyElement {
    let line = |w: f32| ui::native::skeleton(gpui::relative(w), px(14.));
    frame(false, cx)
        .max_w(m::MAX_WIDTH)
        .p(ui::native::cards::card::INSET)
        .flex()
        .flex_col()
        .gap(space::S12)
        .child(
            div()
                .flex()
                .items_center()
                .gap(space::S8)
                .child(
                    ui::native::skeleton(
                        ui::native::cards::card::AVATAR,
                        ui::native::cards::card::AVATAR,
                    )
                    .rounded_full(),
                )
                .child(line(0.4)),
        )
        .child(line(0.7))
        .child(line(1.))
        .child(line(0.55))
        .into_any_element()
}

/// A card that cannot be read (§2.15): an Alert in its place, with 重试.
pub(crate) fn failed_card(id: SharedString, cx: &App) -> AnyElement {
    let n = ui::native::native_colors(cx);
    div()
        .max_w(m::MAX_WIDTH)
        .child(
            ui::native::Alert::new(Sym::Failed, c::CARD_DECODE_FAILED)
                .description(c::CARD_DECODE_FAILED_DETAIL)
                .icon_color(n.red)
                .action(button(id, c::STRIP_RETRY, PushStyle::Outline, cx)),
        )
        .into_any_element()
}

/// `text` with every run of whitespace (line breaks included) as one space.
pub(crate) fn one_line_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The whole card: header (member, state), title, blocks, footer, in one
/// `fill` container with 14 pt insets.
/// A card at work (§2.4 working card, R5-01): the thread's `WorkingCard`
/// (one component): avatar 24 + `<name> 正在做：<step>` + `在做 3/5`, the one
/// Progress, the card's title and blocks, then the time used and 查看 and
/// 停止… (when the plan follows a run I asked for). `None`: not at work.
fn working(ctx: &Ctx, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    use crate::copy::timeline as tl;
    use crate::views::timeline::plan::{USED, WorkingCard, span_text};
    let CardState::Executing { answer, progress } = ctx.state else {
        return None;
    };
    if progress.waiting || !matches!(ctx.mode, CardMode::Topic | CardMode::Desk | CardMode::Line) {
        return None;
    }
    let n = ui::native::native_colors(cx);
    let p = &ctx.env.provider;
    let step = progress
        .current
        .clone()
        .unwrap_or_else(|| one_line_text(&ctx.card.title));
    let title = fill(
        tl::PLAN_DOING_NAMED,
        &[("name", &ctx.author), ("step", &step)],
    );
    let badge = if progress.total > 0 {
        fill(
            c::STRIP_WORKING_COUNT,
            &[
                ("n", &progress.done.to_string()),
                ("total", &progress.total.to_string()),
            ],
        )
    } else {
        c::STRIP_WORKING.to_string()
    };
    let bar = (progress.total > 0).then_some((progress.done, progress.done, progress.total));
    // The overview shows the title only; the full card adds its blocks.
    let body = if ctx.mode == CardMode::Line {
        Vec::new()
    } else if super::v21::is_21(ctx.card) {
        super::v21::body(ctx, window, cx)
    } else {
        blocks::body(ctx, window, cx)
    };
    let content = div()
        .flex()
        .flex_col()
        .min_w(px(0.))
        .child(self::title(ctx, false, cx))
        .children(body);
    // Footer: the time used and the start, then 查看 and 停止….
    let run_id = ctx.card.all_blocks().iter().find_map(|b| match b {
        CardV2Block::Plan { run_id, .. } => run_id.clone(),
        _ => None,
    });
    let run = run_id.as_ref().and_then(|id| {
        p.runs(&ctx.msg.group_id, &ctx.msg.topic_id)
            .into_iter()
            .find(|r| &r.id == id)
    });
    let started = run
        .as_ref()
        .and_then(|r| r.started_at)
        .or(answer.as_ref().map(|a| a.at))
        .unwrap_or(ctx.msg.created_at);
    let used = fill(
        tl::PLAN_FOOTER,
        &[
            ("used", &span_text(USED, ctx.env.now() - started)),
            ("time", &crate::views::shell::clock(started)),
        ],
    );
    let a = ctx.answerer();
    let view = {
        let (a, id) = (a.clone(), run_id.clone());
        ui::native::push_button(
            ctx.id("working-view"),
            tl::PLAN_VIEW,
            PushStyle::Ghost,
            ui::native::PushSize::Small,
            cx,
        )
        .on_click(move |_, window, cx| a.open_live(id.as_deref(), window, cx))
    };
    let me = ctx.me.as_deref();
    let stop = run
        .filter(|r| r.status.is_active() && crate::views::process::asked_by(r, me))
        .map(|r| {
            let stopping = p.run_stopping(&r.id);
            let id = r.id.clone();
            let a = a.clone();
            ui::native::push_button(
                ctx.id("working-stop"),
                tl::PLAN_STOP,
                PushStyle::Outline,
                ui::native::PushSize::Small,
                cx,
            )
            .loading(stopping)
            .disabled(stopping)
            .on_click(move |_, window, cx| a.stop_run(&id, window, cx))
        });
    let footer = div()
        .flex()
        .items_center()
        .gap(space::S8)
        .child(
            ui::truncate::TruncText::new(used)
                .flex_1()
                .n_footnote()
                .n_figures()
                .text_color(n.secondary),
        )
        .child(view)
        .children(stop);
    Some(
        WorkingCard {
            id: ctx.id("working"),
            member: p.member(&ctx.msg.author_id),
            title,
            badge: (BadgeState::Working, badge),
            progress: bar,
            trailing: None,
            body: Some(content.into_any_element()),
            footer: Some(footer.into_any_element()),
        }
        .render(cx),
    )
}

fn full(ctx: &Ctx, window: &mut Window, cx: &mut App) -> AnyElement {
    super::v21::interact::conflict_toast(ctx, cx);
    if let Some(card) = working(ctx, window, cx) {
        return card;
    }
    let header = header(ctx, cx);
    let has_header = header.is_some();
    // --- inc-integrate: 就这张卡提问 on the header (the whole card, `blockId` `card`) ---
    let header = match (ctx.mode, ctx.msg.message_id.is_empty(), header) {
        (CardMode::Topic | CardMode::Desk, false, Some(header)) => {
            let target = crate::views::refs::parts::whole(&ctx.card.title).target(
                crate::views::refs::RefSource::Message {
                    group_id: ctx.msg.group_id.clone(),
                    topic_id: ctx.msg.topic_id.clone(),
                    message_id: ctx.msg.message_id.clone(),
                },
                &ctx.card.title,
                "card",
                None,
                Some(ctx.card.rev as u64),
            );
            Some(crate::views::refs::affordance::whole_card(
                div().child(header),
                Some(target),
                cx,
            ))
        }
        (_, _, header) => header,
    };
    // --- end inc-integrate ---
    let title = title(ctx, has_header, cx);
    // --- inc-cards21 ---
    let body = if super::v21::is_21(ctx.card) {
        super::v21::body(ctx, window, cx)
    } else {
        blocks::body(ctx, window, cx)
    };
    // --- end inc-cards21 ---
    let footer = footer(ctx, window, cx);
    let muted = ctx.state.muted();
    let inset = ui::native::cards::card::INSET;
    let card = frame(false, cx)
        .flex()
        .flex_col()
        // One in-thread width (§2.4): min(640, the text column), in every mode.
        .max_w(m::MAX_WIDTH)
        .child(
            div()
                .pt(inset)
                .px(inset)
                .pb(if footer.is_some() { space::S4 } else { inset })
                .flex()
                .flex_col()
                .min_w(px(0.))
                .children(header)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .min_w(px(0.))
                        .when(muted, |el| el.opacity(m::MUTED))
                        .child(title)
                        .children(body),
                ),
        )
        .children(footer);
    // In a 话题 the timeline row handles arrival; elsewhere the card fades
    // in once per session, not on every remount.
    if ctx.mode == CardMode::Topic {
        return card.into_any_element();
    }
    ui::motion::once(ctx.id("enter"), theme::motion::FAST, card, |el, t| {
        el.opacity(theme::motion::out_fn()(t))
    })
}

// ---------------------------------------------------------------------------
// Footer and buttons

/// A card action (NATIVE.md §5.2): a regular push button. The one primary
/// is the default button (`accent`); every other answer, link or
/// destructive action is a standard button whose title says what it does.
pub(crate) fn button(
    id: SharedString,
    label: impl Into<SharedString>,
    kind: PushStyle,
    cx: &App,
) -> Button {
    use ui::native::{PushSize, PushStyle};
    let style = match kind {
        PushStyle::Default => PushStyle::Default,
        PushStyle::Destructive => PushStyle::Destructive,
        _ => PushStyle::Outline,
    };
    // In-card controls are sm h28 (§1.8 card footers, F51).
    ui::native::push_button(id, label, style, PushSize::Small, cx).flex_none()
}

/// A disabled primary: the primary at 50% (§1.8, D11), so it never looks
/// like the outline Cancel.
pub(crate) fn disabled_button(
    id: SharedString,
    label: impl Into<SharedString>,
    cx: &App,
) -> Button {
    ui::native::disabled_push(button(id, label, PushStyle::Default, cx), true, cx)
}

fn style_kind(action: &CardV2Action, first_default_used: &mut bool) -> PushStyle {
    match action.style.unwrap_or_default() {
        CardV2ActionStyle::Primary => PushStyle::Default,
        CardV2ActionStyle::Danger => PushStyle::Destructive,
        _ if action.kind() == CardActionKind::Link || *first_default_used => PushStyle::Ghost,
        _ => {
            *first_default_used = true;
            PushStyle::Outline
        }
    }
}

/// The footer note at the right: `meta` `ink-3`, with a lock glyph for
/// irreversible asks.
fn note(text: String, lock: bool, cx: &App) -> AnyElement {
    let n = ui::native::native_colors(cx);
    div()
        .n_subheadline()
        .min_w(px(0.))
        .flex_shrink(1.)
        .flex()
        .items_center()
        .gap(space::S4)
        .text_color(n.secondary)
        .when(lock, |el| {
            el.child(ui::icon(Sym::Lock, px(14.), n.secondary))
        })
        // A note with an address cuts in the middle, so both ends stay.
        .child(ui::truncate::TruncText::new(text).middle())
        .into_any_element()
}

fn footer_bar(children: Vec<AnyElement>, cx: &App) -> AnyElement {
    footer_stack(children, None, cx)
}

/// The footer with its description (§2.4): the description always sits above
/// the actions, on its own line.
pub(crate) fn footer_stack(
    children: Vec<AnyElement>,
    note: Option<AnyElement>,
    cx: &App,
) -> AnyElement {
    match note {
        None => card_footer(cx).children(children).into_any_element(),
        Some(note) => {
            let inset = ui::native::cards::card::INSET;
            div()
                .flex_none()
                .flex()
                .flex_col()
                .gap(space::S8)
                .px(inset)
                .pt(ui::native::cards::card::STACK)
                .pb(inset)
                .child(note)
                .when(!children.is_empty(), |el| {
                    el.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space::S8)
                            .children(children),
                    )
                })
                .into_any_element()
        }
    }
}

/// The footer under the blocks: no band, no rule, no fill; 14 pt insets
/// like the rest of the card, buttons 8 pt apart.
pub(crate) fn card_footer(_cx: &App) -> gpui::Div {
    let inset = ui::native::cards::card::INSET;
    // One footer row (§2.4): the buttons keep their width and a note cuts at
    // its end instead of wrapping under them.
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(space::S8)
        .px(inset)
        .pt(ui::native::cards::card::STACK)
        .pb(inset)
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
    let mut desc: Option<AnyElement> = None;
    // --- inc-cards21: a non-blocking ask inside a report, and 2.1 footers ---
    if let Some(f) = super::v21::leaves::footer(ctx, window, cx) {
        return f;
    }
    // --- end inc-cards21 ---

    let link_buttons = |items: &mut Vec<AnyElement>, cx: &App| {
        for (i, l) in links.iter().enumerate() {
            let (a, action) = (a.clone(), (*l).clone());
            items.push(
                button(
                    ctx.id(format!("link-{i}")),
                    l.label.clone(),
                    PushStyle::Ghost,
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
                        shown_label(ctx.card, &submit),
                        PushStyle::Default,
                        cx,
                    )
                    .on_click(move |_, window, cx| a.press(&submit, window, cx))
                    .into_any_element()
                } else {
                    disabled_button(ctx.id("submit"), shown_label(ctx.card, &submit), cx)
                        .into_any_element()
                });
                for x in answers
                    .iter()
                    .filter(|x| x.kind() == CardActionKind::Cancel)
                {
                    let (a, action) = (a.clone(), (*x).clone());
                    items.push(
                        button(
                            ctx.id(format!("act-{}", x.id)),
                            shown_label(ctx.card, x),
                            PushStyle::Ghost,
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
                desc = Some(note(text, false, cx));
                return Some(footer_stack(items, desc.take(), cx));
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
                            PushStyle::Default,
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
                            PushStyle::Ghost,
                            cx,
                        )
                        .on_click(move |_, _, cx| form.update(cx, |f, cx| f.set_editing(false, cx)))
                        .into_any_element(),
                    );
                    desc = Some(note(c::DRAFT_REPLACES.into(), false, cx));
                    return Some(footer_stack(items, desc.take(), cx));
                }
                let a2 = a.clone();
                let s2 = submit.clone();
                items.push(
                    button(
                        ctx.id("draft-ok"),
                        shown_label(ctx.card, &submit),
                        PushStyle::Default,
                        cx,
                    )
                    .disabled(!live)
                    .on_click(move |_, window, cx| a2.press(&s2, window, cx))
                    .into_any_element(),
                );
                let form = ctx.form.clone();
                items.push(
                    button(ctx.id("draft-edit"), c::DRAFT_EDIT, PushStyle::Outline, cx)
                        .disabled(!live)
                        .on_click(move |_, _, cx| form.update(cx, |f, cx| f.set_editing(true, cx)))
                        .into_any_element(),
                );
                for x in answers.iter().filter(|x| x.id != submit.id) {
                    let (a, action) = (a.clone(), (*x).clone());
                    items.push(
                        button(
                            ctx.id(format!("act-{}", x.id)),
                            shown_label(ctx.card, x),
                            PushStyle::Ghost,
                            cx,
                        )
                        .disabled(!live)
                        .on_click(move |_, window, cx| a.press(&action, window, cx))
                        .into_any_element(),
                    );
                }
                link_buttons(&mut items, cx);
                items.extend(lock_note(ctx, live, cx));
                return Some(footer_stack(items, desc.take(), cx));
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
                        shown_label(ctx.card, &submit),
                        PushStyle::Default,
                        cx,
                    )
                    .on_click(move |_, window, cx| a.press(&submit, window, cx))
                    .into_any_element()
                } else {
                    disabled_button(ctx.id("submit"), shown_label(ctx.card, &submit), cx)
                        .into_any_element()
                });
                link_buttons(&mut items, cx);
                // --- inc-cards21: the ask's promise at the right ---
                if super::v21::is_21(ctx.card)
                    && let Some(p) = ctx.card.ask.as_ref().and_then(|a| a.promise.clone())
                {
                    desc = Some(note(p, false, cx));
                }
                // --- end inc-cards21 ---
                return Some(footer_stack(items, desc.take(), cx));
            }
            // An approval: the card's buttons, one primary.
            let mut used = false;
            for x in &answers {
                let kind = style_kind(x, &mut used);
                let (a, action) = (a.clone(), (*x).clone());
                items.push(
                    button(
                        ctx.id(format!("act-{}", x.id)),
                        shown_label(ctx.card, x),
                        kind,
                        cx,
                    )
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
            Some(footer_stack(items, desc.take(), cx))
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
                    button(
                        ctx.id(format!("act-{}", x.id)),
                        shown_label(ctx.card, x),
                        kind,
                        cx,
                    )
                    .disabled(true)
                    .loading(&x.id == action_id)
                    .into_any_element(),
                );
            }
            Some(footer_stack(items, desc.take(), cx))
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
                let a2 = a.clone();
                items.push(
                    button(
                        ctx.id("watch"),
                        fill(c::PLAN_WATCH_COMPUTER, &[("name", &ctx.author)]),
                        PushStyle::Outline,
                        cx,
                    )
                    .icon(Sym::Computer)
                    .on_click(move |_, window, cx| a2.open_live(run.as_deref(), window, cx))
                    .into_any_element(),
                );
            }
            link_buttons(&mut items, cx);
            if plan {
                desc = Some(note(c::PLAN_IN_PLACE.into(), false, cx));
            }
            Some(footer_stack(items, desc.take(), cx))
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
                    let mut b = button(
                        ctx.id(format!("act-{}", x.id)),
                        shown_label(ctx.card, x),
                        kind,
                        cx,
                    )
                    .on_click(move |_, window, cx| a.press(&action, window, cx));
                    if reverse {
                        b = b.icon(ui::icon(Sym::Retry, px(14.), theme::colors(cx).ink));
                    }
                    items.push(b.into_any_element());
                }
            }
            link_buttons(&mut items, cx);
            if items.is_empty() {
                return None;
            }
            if let Some(t) = note_text {
                desc = Some(note(t, false, cx));
            }
            Some(footer_stack(items, desc.take(), cx))
        }
        CardState::Answered { .. } => {
            if links.is_empty() {
                return None;
            }
            link_buttons(&mut items, cx);
            Some(footer_stack(items, desc.take(), cx))
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
                // By label, like an action (`「8 个分格，逐位输入」`): options show no
                // letters (NATIVE.md rule 4).
                if let Some(o) = options.iter().find(|o| o.id == id) {
                    parts.push(format!("「{}」", o.label));
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

// --- v13/ux-pages ---
/// The title of an answer button: it ends with `…` when pressing it opens
/// the confirm sheet first (NATIVE.md §5.2, R26), as `press`
/// decides.
fn shown_label(card: &CardV2, action: &CardV2Action) -> String {
    let kind = action.kind();
    let irreversible = card
        .ask
        .as_ref()
        .and_then(|a| a.irreversible.as_ref())
        .is_some();
    let guarded = kind == CardActionKind::Submit
        && action.style.unwrap_or_default() != CardV2ActionStyle::Default
        || submit_action(card).is_some_and(|s| s.id == action.id);
    let asks_first = matches!(kind, CardActionKind::Submit | CardActionKind::Reject)
        && (kind == CardActionKind::Submit && guarded && irreversible || action.confirm.is_some());
    if asks_first {
        crate::copy::fill(
            crate::copy::desk::DESK_ACTION_MORE,
            &[("label", &action.label)],
        )
    } else {
        action.label.clone()
    }
}
// --- end v13/ux-pages ---
