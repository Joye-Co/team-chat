//! 引用提问 (REFS part A, desktop): ask about one part of something in the conversation.
//!
//! - [`ask_about`]: the one entry point every part affordance calls. It decides where the
//!   chip goes (REFS §A3: the 单独问 composer when it had focus last, else the composer of
//!   the part's 话题, which it opens) and who is asked (§A5).
//! - [`label`], [`value`], [`parts`]: labels, value snapshots and part addresses (§A2).
//! - [`affordance`]: hover 问这里 after 300 ms, ⌥-click, the context menu and the focus ring.
//! - [`chips`]: the composer's chip row (§A4) and the read-only chips of sent messages (§A6).
//!
//! Every affordance hides when the server does not list `refs/1` in `welcome.features`.

pub mod affordance;
pub mod chips;
pub mod label;
pub mod markdown;
pub mod parts;
pub mod scenes;
pub mod value;

#[cfg(test)]
mod tests;

use std::time::{Duration, Instant};

use chat_core::proto::{BlockRef, BlockRefInput};
use gpui::{App, AppContext as _, Context, Entity, EventEmitter, Global, Window};

use crate::{
    app::RightPane,
    copy::{fill, refs as c},
    data::{AddRef, DraftKey, DraftRef, GroupId, Member, MemberId, TopicId},
};

/// Where the referenced part lives.
#[derive(Clone, Debug, PartialEq)]
pub enum RefSource {
    Message {
        group_id: String,
        topic_id: String,
        message_id: String,
    },
    Artifact {
        group_id: String,
        topic_id: String,
        artifact_id: String,
        version: u32,
    },
}

/// One referenced part (or a whole block when `part` is `None`).
#[derive(Clone, Debug, PartialEq)]
pub struct PartTarget {
    pub source: RefSource,
    /// REFS §A2 grammar: a v2 block id, `v1:<i>`, `md:<i>`, `page` or `card`.
    pub block_id: String,
    pub part: Option<chat_core::proto::card2::PartRef>,
    /// Built with `refs::label` (REFS §A2.2), at most 120 code points.
    pub label: String,
    /// A snapshot of the part's value, at most 2048 bytes as JSON.
    pub value: Option<serde_json::Value>,
    pub card_rev: Option<u64>,
}

/// Ask in the topic (the composer chip) or in a side chat (单独问).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefIntent {
    Topic,
    SideChat,
}

impl PartTarget {
    /// What is sent (`send.blockRefs[i]`).
    pub fn input(&self) -> BlockRefInput {
        let (message_id, artifact) = match &self.source {
            RefSource::Message { message_id, .. } => (Some(message_id.clone()), None),
            RefSource::Artifact {
                artifact_id,
                version,
                ..
            } => (
                None,
                Some(chat_core::proto::ArtifactVersionRef {
                    artifact_id: artifact_id.clone(),
                    version: *version,
                }),
            ),
        };
        BlockRefInput {
            message_id,
            artifact,
            card_rev: self
                .card_rev
                .and_then(|r| u32::try_from(r).ok())
                .filter(|r| *r > 0),
            block_id: self.block_id.clone(),
            part: self.part.clone(),
            label: label::cut(&self.label, label::LABEL_MAX),
            value: self.value.clone().map(value::cap),
        }
    }

    /// The (群, 话题) of the source.
    pub fn place(&self) -> (&str, &str) {
        match &self.source {
            RefSource::Message {
                group_id, topic_id, ..
            }
            | RefSource::Artifact {
                group_id, topic_id, ..
            } => (group_id, topic_id),
        }
    }

    /// The address that tells two references apart (dedupe and the flash).
    pub fn address(&self) -> String {
        address_of(&self.input())
    }
}

/// The address of a reference: source, block and part.
pub fn address_of(r: &BlockRefInput) -> String {
    let src = match (&r.message_id, &r.artifact) {
        (Some(m), _) => m.clone(),
        (None, Some(a)) => format!("{}@{}", a.artifact_id, a.version),
        _ => String::new(),
    };
    let part = r
        .part
        .as_ref()
        .map(|p| serde_json::to_string(p).unwrap_or_default())
        .unwrap_or_default();
    format!("{src}/{}/{part}", r.block_id)
}

// ---------------------------------------------------------------------------------------
// The body prefix (REFS §A6)
// ---------------------------------------------------------------------------------------

/// The quote lines the server writes in front of a body with references.
pub fn prefix(refs: &[BlockRef]) -> String {
    chat_core::proto::block_ref_prefix(refs.iter().map(|r| {
        if r.deleted.is_some() {
            c::DELETED
        } else {
            r.source.label.as_str()
        }
    }))
}

/// `body` with the exact prefix of `refs` and the empty line after it removed. `None` when
/// the body does not start with exactly that text (it is then shown unchanged).
pub fn strip_prefix<'a>(body: &'a str, refs: &[BlockRef]) -> Option<&'a str> {
    if refs.is_empty() {
        return None;
    }
    let p = prefix(refs);
    if body == p {
        return Some("");
    }
    body.strip_prefix(&p)?.strip_prefix("\n\n")
}

/// The body the server will store: the prefix, one empty line, then the body (a client-side
/// copy for fakes and for 单独问, whose references travel as quote lines).
pub fn with_prefix(body: &str, refs: &[BlockRef]) -> String {
    if refs.is_empty() {
        return body.to_string();
    }
    let p = prefix(refs);
    if body.is_empty() {
        p
    } else {
        format!("{p}\n\n{body}")
    }
}

/// A 单独问 question with its chips as quote lines in front (the side chat lives in the
/// member's DM, where the server takes no `blockRefs` to the 话题; the member reads the 话题).
pub fn side_chat_body(body: &str, refs: &[DraftRef]) -> String {
    if refs.is_empty() {
        return body.to_string();
    }
    let p = chat_core::proto::block_ref_prefix(refs.iter().map(|r| r.input.label.as_str()));
    if body.is_empty() {
        p
    } else {
        format!("{p}\n\n{body}")
    }
}

/// The labels of the leading `> ↳ ` quote lines of a body, and the rest after them.
pub fn split_quote_lines(body: &str) -> (Vec<String>, &str) {
    let mut labels = Vec::new();
    let mut rest = body;
    while let Some(line_end) = rest.find('\n').or(Some(rest.len())) {
        let line = &rest[..line_end];
        let Some(label) = line.strip_prefix("> ↳ ") else {
            break;
        };
        labels.push(label.to_string());
        rest = rest.get(line_end + 1..).unwrap_or("");
        if rest.is_empty() {
            break;
        }
    }
    if labels.is_empty() {
        return (labels, body);
    }
    (labels, rest.strip_prefix('\n').unwrap_or(rest))
}

// ---------------------------------------------------------------------------------------
// Who is asked (REFS §A5)
// ---------------------------------------------------------------------------------------

/// The member to mention when the first chip is added and the text names nobody: the
/// source's author when it is an active bot that is the 话题's responsible member (or the
/// 话题 has none); else the 话题's responsible bot; else nobody.
pub fn addressee(source_author: Option<&Member>, topic_owner: Option<&Member>) -> Option<MemberId> {
    let active_bot = |m: &&Member| m.is_bot() && !m.deactivated;
    if let Some(a) = source_author.filter(active_bot)
        && topic_owner.is_none_or(|o| o.id == a.id)
    {
        return Some(a.id.clone());
    }
    topic_owner.filter(active_bot).map(|o| o.id.clone())
}

/// The text of a draft names somebody already.
pub fn names_someone(body: &str, mentions: &[MemberId]) -> bool {
    !mentions.is_empty() || body.contains('@')
}

// ---------------------------------------------------------------------------------------
// The shared state: chips of 单独问, the flash, the hover, the pending focus
// ---------------------------------------------------------------------------------------

/// How long a part or chip flashes after it was added (reduce motion: no flash).
pub const FLASH: Duration = Duration::from_millis(600);
/// How long the pointer rests on a part before 问这里 shows.
pub const HOVER_DELAY: Duration = Duration::from_millis(300);

/// The 单独问 composer of one aside.
pub type SideKey = (GroupId, TopicId, MemberId);

#[derive(Default)]
pub struct RefsHub {
    /// The 单独问 composer had focus after the 话题 composer (REFS §A3 rule 1).
    pub side_chat_focused: bool,
    /// The chips of each 单独问 composer (kept while the app runs).
    pub side_refs: std::collections::HashMap<SideKey, Vec<DraftRef>>,
    /// The address that flashes, and since when.
    pub flash: Option<(String, Instant)>,
    /// The composer to focus once it shows this draft.
    pub focus: Option<DraftKey>,
    /// The part under the pointer and since when.
    pub hover: Option<(String, Instant)>,
    /// Scenes: the part whose 问这里 shows without a pointer.
    pub forced_hover: Option<String>,
}

/// What views listen to.
#[derive(Clone, Debug, PartialEq)]
pub enum RefsEvent {
    /// A chip was added to this draft (its composer reloads the draft text, which may now
    /// start with `@name `).
    Added(DraftKey),
    /// The chips of a 单独问 composer changed.
    SideChanged(SideKey),
}

impl EventEmitter<RefsEvent> for RefsHub {}

struct HubGlobal(Entity<RefsHub>);
impl Global for HubGlobal {}

/// The shared state, made on first use.
pub fn hub(cx: &mut App) -> Entity<RefsHub> {
    if let Some(h) = cx.try_global::<HubGlobal>() {
        return h.0.clone();
    }
    let h = cx.new(|_| RefsHub::default());
    cx.set_global(HubGlobal(h.clone()));
    h
}

impl RefsHub {
    /// The address flashes now (and for [`FLASH`]).
    pub fn flash(&mut self, address: String, cx: &mut Context<Self>) {
        self.flash = Some((address, Instant::now()));
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FLASH).await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    /// True while `address` flashes.
    pub fn flashing(&self, address: &str) -> bool {
        self.flash
            .as_ref()
            .is_some_and(|(a, at)| a == address && at.elapsed() < FLASH)
    }

    /// The 问这里 button of `id` shows.
    pub fn hover_shown(&self, id: &str) -> bool {
        self.forced_hover.as_deref() == Some(id)
            || self
                .hover
                .as_ref()
                .is_some_and(|(h, at)| h == id && at.elapsed() >= HOVER_DELAY)
    }

    /// `id` is under the pointer (shown or not yet).
    pub fn hovered(&self, id: &str) -> bool {
        self.forced_hover.as_deref() == Some(id)
            || self.hover.as_ref().is_some_and(|(h, _)| h == id)
    }

    /// Adds a chip to a 单独问 composer.
    pub fn add_side(&mut self, key: SideKey, r: DraftRef, cx: &mut Context<Self>) -> AddRef {
        let list = self.side_refs.entry(key.clone()).or_default();
        if let Some(i) = list.iter().position(|d| d.same_address(&r.input)) {
            return AddRef::Duplicate(i);
        }
        if list.len() >= crate::data::MAX_DRAFT_REFS {
            return AddRef::Full;
        }
        list.push(r);
        cx.emit(RefsEvent::SideChanged(key));
        cx.notify();
        AddRef::Added
    }

    /// Removes the 单独问 chip at `index`.
    pub fn remove_side(&mut self, key: &SideKey, index: usize, cx: &mut Context<Self>) {
        if let Some(list) = self.side_refs.get_mut(key)
            && index < list.len()
        {
            list.remove(index);
            cx.emit(RefsEvent::SideChanged(key.clone()));
            cx.notify();
        }
    }

    /// Takes the 单独问 chips (sent).
    pub fn take_side(&mut self, key: &SideKey, cx: &mut Context<Self>) -> Vec<DraftRef> {
        let out = self.side_refs.remove(key).unwrap_or_default();
        if !out.is_empty() {
            cx.emit(RefsEvent::SideChanged(key.clone()));
            cx.notify();
        }
        out
    }
}

struct ServicesGlobal(crate::app::Services);
impl Global for ServicesGlobal {}

/// Keeps the app services for affordances, which run in element callbacks (called by
/// `Services::new`; the newest services win, as in the screenshot self-test).
pub fn install(services: &crate::app::Services, cx: &mut App) {
    cx.set_global(ServicesGlobal(services.clone()));
}

/// The app services, once [`install`] ran.
pub fn services(cx: &App) -> Option<crate::app::Services> {
    cx.try_global::<ServicesGlobal>().map(|s| s.0.clone())
}

/// Marks which composer had focus last (the 单独问 one or a 话题 one).
pub fn note_focus(side_chat: bool, cx: &mut App) {
    let h = hub(cx);
    if h.read(cx).side_chat_focused != side_chat {
        h.update(cx, |h, _| h.side_chat_focused = side_chat);
    }
}

/// The 引用提问 affordances show (the server takes references).
pub fn enabled(cx: &App) -> bool {
    services(cx).is_some_and(|s| s.provider.refs_supported())
}

fn toast(text: &str, cx: &mut App) {
    crate::views::files::notify(
        gpui_component::notification::Notification::new().message(text.to_string()),
        cx,
    );
}

/// The draft chip for a target, in the source's 话题.
fn draft_ref(target: &PartTarget, group_id: &str, topic_id: &str) -> DraftRef {
    let input = target.input();
    let lead = label::lead_of(&target.label).min(input.label.chars().count());
    DraftRef {
        input,
        group_id: group_id.to_string(),
        topic_id: topic_id.to_string(),
        lead_chars: lead,
    }
}

/// The 群 of a 话题 when the affordance did not know it.
fn group_of(topic_id: &str, cx: &App) -> Option<GroupId> {
    let services = services(cx)?;
    let p = &services.provider;
    p.groups()
        .into_iter()
        .find(|g| p.topic(&g.id, topic_id).is_some())
        .map(|g| g.id)
}

/// The author of the referenced source (message author or 产出 version maker).
fn source_author(target: &PartTarget, cx: &App) -> Option<MemberId> {
    let services = services(cx)?;
    let p = &services.provider;
    let (g, t) = target.place();
    match &target.source {
        RefSource::Message { message_id, .. } => p
            .messages(g, t, 400)
            .messages
            .into_iter()
            .find(|m| &m.id == message_id || &m.client_msg_id == message_id)
            .map(|m| m.author_id),
        RefSource::Artifact { artifact_id, .. } => p
            .artifacts(g, t)
            .into_iter()
            .find(|a| &a.id == artifact_id)
            .map(|a| a.created_by),
    }
}

/// Adds a reference to a part (REFS §A3): the chip goes to the 单独问 composer when asked
/// for there or when it had focus last, else to the composer of the part's 话题, which opens.
/// The same part again flashes its chip; a sixth part shows 最多引用 5 处.
pub fn ask_about(target: PartTarget, intent: RefIntent, window: &mut Window, cx: &mut App) {
    ask_about_outcome(target, intent, window, cx);
}

/// Where [`ask_about_outcome`] put the chip, and what happened to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Asked {
    /// [`AddRef::Unsupported`] when nothing could take the chip (no `refs/1`, no 话题).
    pub added: AddRef,
    /// The chip went to the 单独问 composer, not the 话题 one.
    pub side_chat: bool,
}

/// [`ask_about`], and says what happened, so a caller that confirms the add (a 产出 page)
/// confirms it only when the chip was really added.
pub fn ask_about_outcome(
    target: PartTarget,
    intent: RefIntent,
    window: &mut Window,
    cx: &mut App,
) -> Asked {
    let _ = window;
    let nothing = Asked {
        added: AddRef::Unsupported,
        side_chat: false,
    };
    let Some(services) = services(cx) else {
        return nothing;
    };
    let provider = services.provider.clone();
    if !provider.refs_supported() {
        return nothing;
    }
    let (g, t) = target.place();
    let topic_id = t.to_string();
    if topic_id.is_empty() {
        return nothing;
    }
    let group_id = if g.is_empty() {
        match group_of(&topic_id, cx) {
            Some(g) => g,
            None => return nothing,
        }
    } else {
        g.to_string()
    };
    let mut target = target;
    match &mut target.source {
        RefSource::Message { group_id: gid, .. } | RefSource::Artifact { group_id: gid, .. } => {
            *gid = group_id.clone();
        }
    }
    tracing::info!(
        target: "refs",
        block = %target.block_id,
        part = ?target.part.as_ref().map(|p| p.kind.as_str()),
        intent = ?intent,
        "ask about a part"
    );
    let chip = draft_ref(&target, &group_id, &topic_id);
    let address = target.address();
    let h = hub(cx);

    // Rule 1: the 单独问 composer (asked for, or open on this 话题 and focused last).
    let nav = services.nav.read(cx);
    let pane_member = match nav.visible_pane() {
        RightPane::SideChat(m)
            if nav.topic.as_ref() == Some(&(group_id.clone(), topic_id.clone())) =>
        {
            Some(m)
        }
        _ => None,
    };
    let to_side =
        intent == RefIntent::SideChat || (pane_member.is_some() && h.read(cx).side_chat_focused);
    if to_side {
        let member = pane_member.or_else(|| provider.side_chat_default_member());
        let Some(member) = member else { return nothing };
        let (gg, tt, mm) = (group_id.clone(), topic_id.clone(), member.clone());
        services.nav.update(cx, |nav, cx| {
            if nav.topic.as_ref() != Some(&(gg.clone(), tt.clone())) {
                nav.open_topic(gg, tt, cx);
            }
            nav.set_right_pane(RightPane::SideChat(mm), cx);
        });
        let key = (group_id, topic_id, member);
        let added = h.update(cx, |h, cx| {
            h.side_chat_focused = true;
            let r = h.add_side(key, chip, cx);
            h.flash(address, cx);
            r
        });
        if added == AddRef::Full {
            toast(c::TOO_MANY, cx);
        }
        return Asked {
            added,
            side_chat: true,
        };
    }

    // Rule 2: the composer of the part's 话题.
    let key = DraftKey::topic(group_id.clone(), topic_id.clone());
    let before = provider.draft(&key);
    let added = provider.add_draft_ref(&key, chip);
    match added {
        AddRef::Added => {
            if before.refs.is_empty() && !names_someone(&before.body, &before.mentions) {
                let owner = provider
                    .topic(&group_id, &topic_id)
                    .and_then(|t| t.owner_id)
                    .and_then(|o| provider.member(&o));
                let author = source_author(&target, cx).and_then(|a| provider.member(&a));
                if let Some(id) = addressee(author.as_ref(), owner.as_ref())
                    && let Some(m) = provider.member(&id)
                {
                    let body = format!("@{} {}", m.name, before.body);
                    let mut mentions = before.mentions.clone();
                    mentions.push(id);
                    provider.save_draft_text(&key, body, mentions);
                }
            }
            let (gg, tt) = (group_id.clone(), topic_id.clone());
            services.nav.update(cx, |nav, cx| {
                if nav.topic.as_ref() != Some(&(gg.clone(), tt.clone())) {
                    nav.open_topic(gg, tt, cx);
                }
            });
            h.update(cx, |h, cx| {
                h.side_chat_focused = false;
                h.focus = Some(key.clone());
                h.flash(address, cx);
                cx.emit(RefsEvent::Added(key));
            });
        }
        AddRef::Duplicate(_) => {
            h.update(cx, |h, cx| h.flash(address, cx));
        }
        AddRef::Full => toast(c::TOO_MANY, cx),
        AddRef::Unsupported => {}
    }
    Asked {
        added,
        side_chat: false,
    }
}

/// ⌘; while the 话题 composer has chips: 单独问 opens with them (改成单独问，引用一起带过去).
/// Nothing happens when 单独问 is open already or the 话题 composer has no chips.
pub fn carry_to_side_chat(services: &crate::app::Services, member: Option<&str>, cx: &mut App) {
    let nav = services.nav.read(cx);
    if matches!(nav.visible_pane(), RightPane::SideChat(_)) {
        return;
    }
    let (Some((g, t)), Some(m)) = (nav.topic.clone(), member) else {
        return;
    };
    let key = DraftKey::topic(g.clone(), t.clone());
    let refs = services.provider.draft(&key).refs;
    if refs.is_empty() {
        return;
    }
    let side = (g, t, m.to_string());
    let h = hub(cx);
    h.update(cx, |h, cx| {
        h.side_chat_focused = true;
        for r in refs.iter().cloned() {
            h.add_side(side.clone(), r, cx);
        }
    });
    for i in (0..refs.len()).rev() {
        services.provider.remove_draft_ref(&key, i);
    }
}

/// A reference to nothing (an affordance whose part went away); [`ask_about`] ignores it.
pub fn empty_target() -> PartTarget {
    PartTarget {
        source: RefSource::Message {
            group_id: String::new(),
            topic_id: String::new(),
            message_id: String::new(),
        },
        block_id: String::new(),
        part: None,
        label: String::new(),
        value: None,
        card_rev: None,
    }
}

/// The text 引用了 {n} 处.
pub fn cited(n: usize) -> String {
    fill(c::CITED, &[("n", &n.to_string())])
}
