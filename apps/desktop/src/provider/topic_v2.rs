//! The v2 topic surfaces on the chat-core cache (provider track, 1.1.0): 改动 from the
//! topic's PR link, the review on the pinned card, 用到的决定, run usage, 来龙去脉 of a 产出,
//! previews of 产出 versions read from their cached files, and the length of cached video
//! and audio files.
//!
//! - 改动 is the newest `topic.linked` PR of the 话题. It is 合并中 once someone answered the
//!   review ask with its approve action while the PR is still open.
//! - The review is the newest open ask for me in the 话题 (a Card v2 ask about a PR, or a v1
//!   approval card while the 话题 waits on my review). 批准合并 answers that card through the
//!   normal answer calls; without such a card there is nothing to approve.
//! - Previews read the entry page of a version folder that is already on disk; a picture of
//!   the page or of a video is not made, so a version without a page gives `None`.
//! - 首页 desk notes and 排队 have no data source in 1.1, so this provider leaves them empty.

use std::{
    collections::{HashMap, HashSet},
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::Mutex,
    time::{Duration, Instant},
};

use chat_core::proto::{self, CardActionKind, CardV2, CardV2ActionStyle, TopicLink};

use super::{CoreProvider, lock};
use crate::data::{
    AdminError, AdminReply, Change, ChatProvider as _, FileAttachment, MemberId, Millis,
    TopicStatus,
    card::ActionStyle,
    cards_v2::CardV2AnswerRequest,
    pages::{ArtifactThumb, ThumbRow},
    reply_now,
    v2::{
        ArtifactPreview, ArtifactStory, AskItem, AskKind, ChangeState, Currency, Decision,
        PreviewRow, RunUsage, TopicChanges,
    },
};

/// Messages of a 话题 searched for its review card.
const REVIEW_SCAN: usize = 60;
/// Topics listed under 被这些话题引用.
const CITED_LIMIT: usize = 10;
/// How long the review card of a 话题 is kept before its messages are read again (views
/// ask for it on every frame).
const REVIEW_TTL: Duration = Duration::from_secs(2);
/// How long the list of citing topics is kept before it is read again.
const CITED_TTL: Duration = Duration::from_secs(30);
/// Rows of a thumbnail.
const THUMB_ROWS: usize = 4;
/// Rows of a preview table.
const PREVIEW_ROWS: usize = 12;
/// The largest page read for a preview.
const MAX_PAGE_BYTES: u64 = 2 * 1024 * 1024;
/// Entries kept in each cache before it is cleared.
const MAX_ENTRIES: usize = 2000;

/// Per-account caches of this track; cleared with the account.
#[derive(Default)]
pub(super) struct TopicV2Cache {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    generation: u64,
    /// (artifact, version) -> the preview read from its page.
    previews: HashMap<(String, u32), Option<ArtifactPreview>>,
    /// Versions whose files were asked for in the background.
    fetching: HashSet<(String, u32)>,
    /// File sha256 -> length in ms (`None`: not a media file we can read).
    durations: HashMap<String, Option<u64>>,
    /// Topic -> its review card and when it was looked for.
    reviews: HashMap<String, (Instant, Option<ReviewCard>)>,
    /// Artifact -> citing topics and when they were read.
    cited: HashMap<String, (Instant, Vec<(String, String)>)>,
}

impl Inner {
    fn for_generation(&mut self, generation: u64) -> &mut Self {
        if self.generation != generation {
            *self = Inner {
                generation,
                ..Inner::default()
            };
        }
        if self.previews.len() >= MAX_ENTRIES {
            self.previews.clear();
        }
        if self.durations.len() >= MAX_ENTRIES {
            self.durations.clear();
        }
        if self.cited.len() >= MAX_ENTRIES {
            self.cited.clear();
        }
        if self.reviews.len() >= MAX_ENTRIES {
            self.reviews.clear();
        }
        self
    }
}

fn small(v: u64) -> u32 {
    v.min(u32::MAX as u64) as u32
}

/// 改动 of a PR link. An open PR is 合并中 while GitHub has its merge queued (`merge` without
/// `error`); a stopped merge (`merge.error`) is open again. Only a link with no merge state
/// (servers before 1.1, repos without merge queue or auto-merge) falls back to `approved`.
pub fn changes_of(pr: &proto::PrLink, approved: bool) -> TopicChanges {
    let passed = pr
        .checks
        .iter()
        .filter(|c| matches!(c.conclusion.as_str(), "success" | "neutral" | "skipped"))
        .count();
    let state = match pr.state.as_str() {
        "merged" => ChangeState::Merged,
        "closed" => ChangeState::Closed,
        _ => match &pr.merge {
            Some(m) if m.error.is_none() => ChangeState::Merging,
            Some(_) => ChangeState::Open,
            None if approved => ChangeState::Merging,
            None => ChangeState::Open,
        },
    };
    TopicChanges {
        number: small(pr.number),
        title: pr.title.clone(),
        url: (!pr.url.is_empty()).then(|| pr.url.clone()),
        additions: small(pr.additions.unwrap_or(0)),
        deletions: small(pr.deletions.unwrap_or(0)),
        files: small(pr.changed_files.unwrap_or(0)),
        checks_passed: passed as u32,
        checks_total: pr.checks.len() as u32,
        state,
    }
}

/// What a run used: its time, its tokens and its cost when the CLI reported them. The
/// server reports the cost in US dollars (`usage.costUsd`); it stays in dollars.
pub fn usage_of(run: &proto::BotRun) -> Option<RunUsage> {
    let duration_ms = run
        .duration_ms
        .or_else(|| match (run.started_at, run.ended_at) {
            (Some(a), Some(b)) => Some(b.saturating_sub(a)),
            _ => None,
        });
    if duration_ms.is_none() && run.usage.is_none() {
        return None;
    }
    Some(RunUsage {
        duration_ms: duration_ms.unwrap_or(0),
        cost_cents: run
            .usage
            .as_ref()
            .and_then(|u| u.cost_usd())
            .filter(|c| c.is_finite() && *c >= 0.)
            .map(|c| (c * 100.).round() as u64),
        currency: Currency::Usd,
        input_tokens: run.usage.as_ref().map(|u| u.input_tokens),
        output_tokens: run.usage.as_ref().map(|u| u.output_tokens),
    })
}

/// The approve action of a v2 review card: its primary submit action, else its first
/// submit action that is not dangerous.
fn approve_action_v2(card: &CardV2) -> Option<String> {
    let submit = |a: &&proto::CardV2Action| a.kind() == CardActionKind::Submit;
    let actions = card.actions();
    // 1.1: the author marks it (`role: "approve"`). A card that marks any action with a role
    // gets no guess.
    if let Some(a) = actions.iter().find(|a| a.is_approve()) {
        return Some(a.id.clone());
    }
    if actions.iter().any(|a| a.role.is_some()) {
        return None;
    }
    actions
        .iter()
        .filter(submit)
        .find(|a| a.style == Some(CardV2ActionStyle::Primary))
        .or_else(|| {
            actions
                .iter()
                .filter(submit)
                .find(|a| a.style != Some(CardV2ActionStyle::Danger))
        })
        .map(|a| a.id.clone())
}

/// The approve action of a v1 approval card: its primary button, else its first button
/// that is not dangerous.
fn approve_action_v1(card: &proto::Card) -> Option<String> {
    let actions = card.actions.as_deref().unwrap_or(&[]);
    actions
        .iter()
        .find(|a| a.style == Some(ActionStyle::Primary))
        .or_else(|| {
            actions
                .iter()
                .find(|a| a.style != Some(ActionStyle::Danger))
        })
        .map(|a| a.id.clone())
}

// ----------------------------------------------------------------------
// 产出 page previews
// ----------------------------------------------------------------------

/// Text without tags, entities decoded, white space folded.
fn plain(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    let out = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The inner HTML of each `<tag …>…</tag>` in `html`, in order (no nesting of the same tag).
fn elements<'a>(html: &'a str, lower: &str, tag: &str) -> Vec<&'a str> {
    let open = format!("<{tag}");
    let close = format!("</{tag}");
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(i) = lower[at..].find(&open) {
        let start = at + i;
        // `<th` must not match `<thead`.
        let next = lower[start + open.len()..].chars().next();
        if !matches!(next, Some('>' | ' ' | '\t' | '\n' | '\r' | '/')) {
            at = start + open.len();
            continue;
        }
        let Some(gt) = lower[start..].find('>') else {
            break;
        };
        let body = start + gt + 1;
        let end = lower[body..].find(&close).map_or(lower.len(), |e| body + e);
        if let Some(inner) = html.get(body..end) {
            out.push(inner);
        }
        at = end.max(body);
    }
    out
}

/// A pass or fail mark in a table cell.
fn mark(cell: &str) -> Option<bool> {
    let c = cell.to_lowercase();
    if ["✓", "✔", "通过", "pass", "ok", "成功"]
        .iter()
        .any(|m| c.contains(m))
    {
        Some(true)
    } else if ["✗", "✘", "✕", "没通过", "失败", "fail"]
        .iter()
        .any(|m| c.contains(m))
    {
        Some(false)
    } else {
        None
    }
}

fn cut(text: String, max: usize) -> String {
    if text.chars().count() <= max {
        text
    } else {
        let mut s: String = text.chars().take(max - 1).collect();
        s.push('…');
        s
    }
}

/// A preview of an HTML page: its title, its first paragraph, and its first table (a label,
/// a pass or fail mark and a value per row). `None` when the page has no title or heading.
pub fn preview_of_html(html: &str) -> Option<ArtifactPreview> {
    // ASCII lowercasing keeps every byte offset, so offsets in `lower` work in `html`.
    let lower = html.to_ascii_lowercase();
    let body_at = lower.find("<body").unwrap_or(0);
    let (head_html, html_body) = html.split_at(body_at);
    let (_, lower_body) = lower.split_at(body_at);
    let h1 = elements(html_body, lower_body, "h1")
        .into_iter()
        .map(plain)
        .find(|t| !t.is_empty());
    let title = h1.or_else(|| super::files::html_title(head_html))?;
    let summary = elements(html_body, lower_body, "p")
        .into_iter()
        .map(plain)
        .find(|t| !t.is_empty())
        .map(|t| cut(t, 160));
    let mut preview = ArtifactPreview {
        title: cut(title, 80),
        summary,
        ..ArtifactPreview::default()
    };
    if let Some(table) = elements(html_body, lower_body, "table").into_iter().next() {
        let table_lower = table.to_ascii_lowercase();
        for row in elements(table, &table_lower, "tr") {
            let row_lower = row.to_ascii_lowercase();
            let heads: Vec<String> = elements(row, &row_lower, "th")
                .into_iter()
                .map(plain)
                .collect();
            let cells: Vec<String> = elements(row, &row_lower, "td")
                .into_iter()
                .map(plain)
                .collect();
            if cells.is_empty() {
                if preview.columns.is_empty() && !heads.is_empty() {
                    preview.columns = heads.into_iter().map(|h| cut(h, 20)).collect();
                }
                continue;
            }
            let mut cells = heads.into_iter().chain(cells).collect::<Vec<_>>();
            let label = cut(cells.remove(0), 40);
            let ok = cells.iter().find_map(|c| mark(c));
            let value = cells
                .iter()
                .rev()
                .find(|c| mark(c).is_none())
                .cloned()
                .unwrap_or_default();
            preview.rows.push(PreviewRow {
                label,
                ok,
                value: cut(value, 24),
            });
            if preview.rows.len() >= PREVIEW_ROWS {
                break;
            }
        }
        // The heading right before the table names it.
        let before = &html_body[..html_body.find(table).unwrap_or(0)];
        let before_lower = before.to_ascii_lowercase();
        preview.table_title = ["h2", "h3"]
            .iter()
            .filter_map(|t| elements(before, &before_lower, t).into_iter().last())
            .map(plain)
            .find(|t| !t.is_empty());
    }
    Some(preview)
}

/// The author's preview card of a version (`ArtifactVersion.preview`, GAPS SF-11): its title,
/// summary and figures. `fallback_title` (the 产出 title) fills a preview that has none.
/// Rows and tables still come from the page (`merge_scraped`).
pub fn preview_of_version(p: &proto::ArtifactPreview, fallback_title: &str) -> ArtifactPreview {
    ArtifactPreview {
        title: cut(
            p.title
                .clone()
                .unwrap_or_else(|| fallback_title.to_string()),
            80,
        ),
        summary: p.summary.clone().map(|t| cut(t, 160)),
        metrics: p
            .metrics
            .iter()
            .flatten()
            .map(|m| crate::data::v2::PreviewMetric {
                label: m.label.clone(),
                value: m.value.clone(),
                unit: None,
                note: m.delta.clone(),
            })
            .collect(),
        ..ArtifactPreview::default()
    }
}

/// The author's preview with what only the page has (its table), when the page was read.
fn merge_scraped(mut author: ArtifactPreview, scraped: Option<ArtifactPreview>) -> ArtifactPreview {
    if let Some(s) = scraped {
        author.table_title = s.table_title;
        author.columns = s.columns;
        author.rows = s.rows;
        author.chart_title = s.chart_title;
        author.chart_note = s.chart_note;
        author.bars = s.bars;
        author.axis = s.axis;
        author.eyebrow = author.eyebrow.or(s.eyebrow);
    }
    author
}

/// A list thumbnail from a preview: the title and its first rows.
pub fn thumb_of(preview: &ArtifactPreview) -> ArtifactThumb {
    ArtifactThumb {
        title: preview.title.clone(),
        rows: preview
            .rows
            .iter()
            .take(THUMB_ROWS)
            .map(|r| ThumbRow {
                label: r.label.clone(),
                value: (!r.value.is_empty()).then(|| r.value.clone()),
                ok: r.ok == Some(true),
            })
            .collect(),
    }
}

fn read_page(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_PAGE_BYTES {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

// ----------------------------------------------------------------------
// Media lengths
// ----------------------------------------------------------------------

fn be_u32(b: &[u8]) -> u64 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as u64
}

fn be_u64(b: &[u8]) -> u64 {
    u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

/// Finds a box of `kind` among the boxes in `[start, end)`; returns its payload range.
fn find_box<R: Read + Seek>(r: &mut R, start: u64, end: u64, kind: &[u8; 4]) -> Option<(u64, u64)> {
    let mut at = start;
    while at + 8 <= end {
        r.seek(SeekFrom::Start(at)).ok()?;
        let mut head = [0u8; 16];
        r.read_exact(&mut head[..8]).ok()?;
        let mut size = be_u32(&head[..4]);
        let mut header = 8;
        if size == 1 {
            r.read_exact(&mut head[8..16]).ok()?;
            size = be_u64(&head[8..16]);
            header = 16;
        } else if size == 0 {
            size = end - at;
        }
        if size < header {
            return None;
        }
        if &head[4..8] == kind {
            return Some((at + header, (at + size).min(end)));
        }
        at = at.checked_add(size)?;
    }
    None
}

/// The length of an MP4, MOV or M4A file from its `moov/mvhd` box.
fn mp4_duration<R: Read + Seek>(r: &mut R, len: u64) -> Option<u64> {
    let (moov, moov_end) = find_box(r, 0, len, b"moov")?;
    let (mvhd, _) = find_box(r, moov, moov_end, b"mvhd")?;
    r.seek(SeekFrom::Start(mvhd)).ok()?;
    let mut b = [0u8; 32];
    r.read_exact(&mut b[..4]).ok()?;
    let (timescale, duration) = if b[0] == 1 {
        r.read_exact(&mut b[..28]).ok()?;
        (be_u32(&b[16..20]), be_u64(&b[20..28]))
    } else {
        r.read_exact(&mut b[..16]).ok()?;
        (be_u32(&b[8..12]), be_u32(&b[12..16]))
    };
    (timescale > 0 && duration != u64::MAX && duration != u32::MAX as u64)
        .then(|| duration.saturating_mul(1000) / timescale)
}

/// The length of a WAV file from its `fmt ` byte rate and `data` size.
fn wav_duration<R: Read + Seek>(r: &mut R, len: u64) -> Option<u64> {
    let mut head = [0u8; 12];
    r.seek(SeekFrom::Start(0)).ok()?;
    r.read_exact(&mut head).ok()?;
    if &head[..4] != b"RIFF" || &head[8..12] != b"WAVE" {
        return None;
    }
    let mut at = 12u64;
    let mut byte_rate = None;
    while at + 8 <= len {
        r.seek(SeekFrom::Start(at)).ok()?;
        let mut c = [0u8; 8];
        r.read_exact(&mut c).ok()?;
        let size = u32::from_le_bytes([c[4], c[5], c[6], c[7]]) as u64;
        match &c[..4] {
            b"fmt " => {
                let mut f = [0u8; 12];
                r.read_exact(&mut f).ok()?;
                byte_rate = Some(u32::from_le_bytes([f[8], f[9], f[10], f[11]]) as u64);
            }
            b"data" => {
                let rate = byte_rate.filter(|r| *r > 0)?;
                return Some(size.min(len - at - 8).saturating_mul(1000) / rate);
            }
            _ => {}
        }
        at += 8 + size + (size & 1);
    }
    None
}

/// The length in ms of a cached video or audio file, when its container says.
pub fn media_length(path: &Path) -> Option<u64> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let mut magic = [0u8; 12];
    f.read_exact(&mut magic).ok()?;
    if &magic[..4] == b"RIFF" {
        wav_duration(&mut f, len)
    } else if &magic[4..8] == b"ftyp" || &magic[4..8] == b"moov" || &magic[4..8] == b"mdat" {
        mp4_duration(&mut f, len)
    } else {
        None
    }
}

// ----------------------------------------------------------------------
// The provider
// ----------------------------------------------------------------------

/// The review card of a 话题: the card message and its approve action.
#[derive(Clone)]
struct ReviewCard {
    message_id: String,
    v2: Option<CardV2>,
    approve: String,
}

impl CoreProvider {
    fn topic_v2_generation(&self) -> u64 {
        self.state().generation
    }

    fn pr_link(&self, topic_id: &str) -> Option<proto::PrLink> {
        let a = self.account()?;
        a.client
            .topic_links(&self.resolve_topic(topic_id))
            .unwrap_or_default()
            .into_iter()
            .find_map(|l| match l {
                TopicLink::Pr(p) => Some(p),
                TopicLink::Unknown => None,
            })
    }

    pub(super) fn topic_changes_inner(
        &self,
        group_id: &str,
        topic_id: &str,
    ) -> Option<TopicChanges> {
        let pr = self.pr_link(topic_id)?;
        let approved = pr.state != "merged"
            && pr.state != "closed"
            && self.changes_approval_inner(group_id, topic_id).is_some();
        Some(changes_of(&pr, approved))
    }

    /// The newest card in a 话题 that asks for a review of its PR: a v2 ask about a PR (with
    /// a diff or a PR ref), else a v1 approval card while the 话题 waits on a review.
    fn review_card(&self, group_id: &str, topic_id: &str) -> Option<ReviewCard> {
        let real = self.resolve_topic(topic_id);
        let generation = self.topic_v2_generation();
        if let Some((at, card)) = lock(&self.shared.topic_v2.inner)
            .for_generation(generation)
            .reviews
            .get(&real)
            && at.elapsed() < REVIEW_TTL
        {
            return card.clone();
        }
        let card = self.find_review_card(group_id, topic_id, &real);
        lock(&self.shared.topic_v2.inner)
            .for_generation(generation)
            .reviews
            .insert(real, (Instant::now(), card.clone()));
        card
    }

    fn find_review_card(&self, group_id: &str, topic_id: &str, real: &str) -> Option<ReviewCard> {
        let a = self.account()?;
        let waits_on_review = self
            .topic(group_id, topic_id)
            .is_some_and(|t| t.status == TopicStatus::WaitingReview);
        let messages = a
            .client
            .messages(real, None, REVIEW_SCAN)
            .unwrap_or_default();
        messages.iter().rev().find_map(|m| {
            if m.is_deleted() {
                return None;
            }
            if let Some(c) = &m.card_v2 {
                let review = c.ask.is_some() && super::desk::kind_of_v2(c) == AskKind::Review;
                return review
                    .then(|| approve_action_v2(c))
                    .flatten()
                    .map(|approve| ReviewCard {
                        message_id: m.id.clone(),
                        v2: Some(c.clone()),
                        approve,
                    });
            }
            let c = m.card.as_ref()?;
            let approval = super::desk::kind_of_v1(c) == AskKind::Approval;
            (approval && waits_on_review)
                .then(|| approve_action_v1(c))
                .flatten()
                .map(|approve| ReviewCard {
                    message_id: m.id.clone(),
                    v2: None,
                    approve,
                })
        })
    }

    /// The pinned card: the newest open ask for me. A v1 approval card on a 话题 that waits
    /// on my review of its PR is the review.
    pub(super) fn pinned_ask_inner(&self, group_id: &str, topic_id: &str) -> Option<AskItem> {
        let mut ask = self
            .asks_for_me()
            .into_iter()
            .filter(|a| a.group_id == group_id && a.topic_id == topic_id)
            .max_by_key(|a| a.at)?;
        if ask.kind == AskKind::Approval
            && ask.message_id.is_some()
            && self.pr_link(topic_id).is_some()
            && self
                .review_card(group_id, topic_id)
                .is_some_and(|r| Some(&r.message_id) == ask.message_id.as_ref())
        {
            ask.kind = AskKind::Review;
        }
        Some(ask)
    }

    /// 批准合并: answers the review card that waits on me with its approve action.
    pub(super) fn approve_changes_inner(&self, group_id: &str, topic_id: &str) -> AdminReply<()> {
        let Some(ask) = self
            .pinned_ask_inner(group_id, topic_id)
            .filter(|a| a.kind == AskKind::Review)
        else {
            return reply_now(Err(AdminError::Unsupported));
        };
        let Some(review) = self
            .review_card(group_id, topic_id)
            .filter(|r| ask.message_id.as_ref() == Some(&r.message_id))
        else {
            return reply_now(Err(AdminError::Unsupported));
        };
        match &review.v2 {
            Some(card) => self.answer_card_v2(
                group_id,
                topic_id,
                &review.message_id,
                CardV2AnswerRequest {
                    rev: card.rev,
                    action_id: review.approve,
                    inputs: None,
                    edited_values: None,
                },
            ),
            None => self.press_card_action(group_id, topic_id, &review.message_id, &review.approve),
        }
        self.emit(vec![
            Change::Topics {
                group_id: group_id.to_string(),
            },
            Change::Todo,
        ]);
        reply_now(Ok(()))
    }

    /// Who approved the review card and when: a stored answer or press with its approve
    /// action, or my answer on its way. `None` without a review card.
    pub(super) fn changes_approval_inner(
        &self,
        group_id: &str,
        topic_id: &str,
    ) -> Option<(MemberId, Millis)> {
        let review = self.review_card(group_id, topic_id)?;
        match &review.v2 {
            Some(card) => {
                let stored = self
                    .card_v2_answers(group_id, &review.message_id)
                    .into_iter()
                    .filter(|x| x.rev == card.rev && x.action_id == review.approve)
                    .map(|x| (x.actor_id, x.at))
                    .next_back();
                stored.or_else(|| {
                    let mine = self.card_v2_pending(group_id, &review.message_id)?;
                    let me = self.account()?.me.id.clone();
                    (mine.action_id == review.approve
                        && !matches!(
                            mine.phase,
                            crate::data::cards_v2::CardV2SendPhase::Failed(_)
                        ))
                    .then_some((me, mine.at))
                })
            }
            None => self
                .card_presses(group_id, &review.message_id)
                .into_iter()
                .filter(|p| p.action_id == review.approve)
                .map(|p| (p.actor_id, p.at))
                .next_back(),
        }
    }

    /// My answer to the review card is still queued (in the outbox, not stored).
    fn review_answer_queued(&self, group_id: &str, review: &ReviewCard) -> bool {
        review.v2.is_some()
            && self
                .card_v2_pending(group_id, &review.message_id)
                .is_some_and(|p| p.phase == crate::data::cards_v2::CardV2SendPhase::Sending)
    }

    /// My approval is still queued: 撤回 can take it back.
    pub(super) fn changes_approval_queued_inner(&self, group_id: &str, topic_id: &str) -> bool {
        self.review_card(group_id, topic_id)
            .is_some_and(|review| self.review_answer_queued(group_id, &review))
    }

    /// 撤回 while my approval is still queued.
    pub(super) fn withdraw_changes_approval_inner(
        &self,
        group_id: &str,
        topic_id: &str,
    ) -> AdminReply<()> {
        let Some(review) = self.review_card(group_id, topic_id) else {
            return reply_now(Err(AdminError::Unsupported));
        };
        if !self.review_answer_queued(group_id, &review) {
            return reply_now(Err(AdminError::Unsupported));
        }
        self.withdraw_card_v2_answer(group_id, topic_id, &review.message_id);
        self.emit(vec![
            Change::Topics {
                group_id: group_id.to_string(),
            },
            Change::Todo,
        ]);
        reply_now(Ok(()))
    }

    pub(super) fn run_usage_inner(&self, run_id: &str) -> Option<RunUsage> {
        let a = self.account()?;
        let run = a.client.store().run(run_id).ok().flatten()?;
        usage_of(&run)
    }

    /// 用到的决定: the 群's decisions made in this 话题 (answered asks marked `record`).
    pub(super) fn topic_decisions_inner(&self, group_id: &str, topic_id: &str) -> Vec<Decision> {
        let real = self.resolve_topic(topic_id);
        self.group_decisions_inner(group_id)
            .into_iter()
            .filter(|d| d.topic_id.as_deref() == Some(real.as_str()))
            .collect()
    }

    fn citations(&self, artifact_id: &str) -> Vec<(String, String)> {
        let Some(a) = self.account() else {
            return Vec::new();
        };
        let generation = self.topic_v2_generation();
        if let Some((at, list)) = lock(&self.shared.topic_v2.inner)
            .for_generation(generation)
            .cited
            .get(artifact_id)
            && at.elapsed() < CITED_TTL
        {
            return list.clone();
        }
        let list = a
            .client
            .artifact_citations(artifact_id, CITED_LIMIT + 1)
            .unwrap_or_default();
        lock(&self.shared.topic_v2.inner)
            .for_generation(generation)
            .cited
            .insert(artifact_id.to_string(), (Instant::now(), list.clone()));
        list
    }

    /// 来龙去脉: the 话题 it came from, who made it and who asked (the requester of the run
    /// that made it, else who started the 话题), the decisions of that 话题, the notes of its
    /// versions and the other 话题 that point at it.
    pub(super) fn artifact_story_inner(&self, artifact_id: &str) -> Option<ArtifactStory> {
        let a = self.account()?;
        let row = a.client.artifact(artifact_id).ok().flatten()?;
        let art = &row.artifact;
        let oldest = row.versions.iter().min_by_key(|v| v.version);
        let made_by = oldest
            .map(|v| v.created_by.clone())
            .unwrap_or_else(|| art.created_by.clone());
        let run = row
            .versions
            .iter()
            .filter_map(|v| v.run_id.as_deref())
            .find_map(|id| a.client.store().run(id).ok().flatten());
        // An anonymous run (a private card's answer started it) names nobody.
        let anonymous = run.as_ref().is_some_and(|r| {
            r.requested_by.is_some() && super::process::requester(r.requested_by.clone()).is_none()
        });
        let asked_by = run
            .and_then(|r| super::process::requester(r.requested_by))
            .or_else(|| {
                if anonymous {
                    return None;
                }
                self.topic(&art.group_id, &art.topic_id)
                    .map(|t| t.created_by)
            })
            .filter(|who| *who != made_by);
        let mut version_notes: Vec<(u32, String)> = row
            .versions
            .iter()
            .filter_map(|v| v.note.clone().map(|n| (v.version, n)))
            .filter(|(_, n)| !n.trim().is_empty())
            .collect();
        version_notes.sort_by_key(|(v, _)| std::cmp::Reverse(*v));
        let cited_by = self
            .citations(artifact_id)
            .into_iter()
            .filter(|(_, t)| *t != art.topic_id)
            .filter_map(|(g, t)| {
                let title = self.topic(&g, &t)?.title;
                Some((g, t, title))
            })
            .take(CITED_LIMIT)
            .collect();
        Some(ArtifactStory {
            topic: Some((art.group_id.clone(), art.topic_id.clone())),
            made_by: Some(made_by),
            asked_by,
            decisions: self.topic_decisions_inner(&art.group_id, &art.topic_id),
            version_notes,
            cited_by,
        })
    }

    /// The preview of a version's page, read once from its folder on disk. The newest version
    /// of a topic's 产出 is fetched in the background the first time it is asked for.
    pub(super) fn artifact_preview_inner(
        &self,
        artifact_id: &str,
        version: u32,
        fetch: bool,
    ) -> Option<ArtifactPreview> {
        let a = self.account()?;
        let generation = self.topic_v2_generation();
        let key = (artifact_id.to_string(), version);
        if let Some(p) = lock(&self.shared.topic_v2.inner)
            .for_generation(generation)
            .previews
            .get(&key)
        {
            return p.clone();
        }
        let row = a.client.artifact(artifact_id).ok().flatten()?;
        let v = row.versions.iter().find(|v| v.version == version)?;
        // The author's preview comes first (any kind); the page only adds its table.
        let author = v
            .preview
            .as_ref()
            .map(|p| preview_of_version(p, &row.artifact.title));
        if row.artifact.kind != proto::ArtifactKind::Html {
            return author;
        }
        let Some(dir) = a.client.cached_artifact_version(artifact_id, version) else {
            if fetch {
                self.fetch_version(artifact_id, version);
            }
            return author;
        };
        let scraped = read_page(&dir.join(&v.entry)).and_then(|html| preview_of_html(&html));
        let preview = match author {
            Some(p) => Some(merge_scraped(p, scraped)),
            None => scraped,
        };
        lock(&self.shared.topic_v2.inner)
            .for_generation(generation)
            .previews
            .insert(key, preview.clone());
        preview
    }

    fn fetch_version(&self, artifact_id: &str, version: u32) {
        let Some(a) = self.account() else { return };
        let generation = self.topic_v2_generation();
        if !lock(&self.shared.topic_v2.inner)
            .for_generation(generation)
            .fetching
            .insert((artifact_id.to_string(), version))
        {
            return;
        }
        let handle =
            a.client
                .open_artifact_version(artifact_id, Some(version), chat_core::no_progress());
        let this = self.clone();
        let change = a
            .client
            .artifact(artifact_id)
            .ok()
            .flatten()
            .map(|r| Change::Artifacts {
                group_id: r.artifact.group_id,
                topic_id: r.artifact.topic_id,
            });
        chat_core::background().spawn(async move {
            match handle.await {
                Ok(Ok(_)) => {
                    if let Some(c) = change {
                        this.emit(vec![c]);
                    }
                }
                Ok(Err(e)) => tracing::info!(error = %e, "产出 preview: version not fetched"),
                Err(e) => tracing::info!(error = %e, "产出 preview: fetch task failed"),
            }
        });
    }

    /// A list thumbnail of a 产出's current version, only from files on disk.
    pub(super) fn artifact_thumb_inner(&self, artifact_id: &str) -> Option<ArtifactThumb> {
        let a = self.account()?;
        let row = a.client.artifact(artifact_id).ok().flatten()?;
        self.artifact_preview_inner(artifact_id, row.artifact.current_version, false)
            .as_ref()
            .map(thumb_of)
    }

    /// The length of a video or audio file that is on disk.
    pub(super) fn media_duration_inner(&self, file: &FileAttachment) -> Option<u64> {
        let media = file.mime.starts_with("video/") || file.mime.starts_with("audio/");
        if !media || file.sha256.is_empty() {
            return None;
        }
        let generation = self.topic_v2_generation();
        if let Some(d) = lock(&self.shared.topic_v2.inner)
            .for_generation(generation)
            .durations
            .get(&file.sha256)
        {
            return *d;
        }
        // Not on disk yet: ask again later.
        let path = self.cached_file_inner(file)?;
        let length = media_length(&path);
        lock(&self.shared.topic_v2.inner)
            .for_generation(generation)
            .durations
            .insert(file.sha256.clone(), length);
        length
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pr_link_gives_the_changes_and_their_state() {
        let pr = proto::PrLink {
            merge: None,
            repo: "a/b".into(),
            number: 61,
            url: "https://github.com/a/b/pull/61".into(),
            title: "按 seq 去重".into(),
            state: "open".into(),
            checks: vec![
                proto::PrCheck {
                    name: "ci".into(),
                    conclusion: "success".into(),
                },
                proto::PrCheck {
                    name: "lint".into(),
                    conclusion: "pending".into(),
                },
            ],
            additions: Some(214),
            deletions: Some(63),
            changed_files: Some(12),
        };
        let ch = changes_of(&pr, false);
        assert_eq!(
            (ch.number, ch.additions, ch.deletions, ch.files),
            (61, 214, 63, 12)
        );
        assert_eq!((ch.checks_passed, ch.checks_total), (1, 2));
        assert_eq!(ch.state, ChangeState::Open);
        assert_eq!(changes_of(&pr, true).state, ChangeState::Merging);
        let merged = proto::PrLink {
            state: "merged".into(),
            ..pr
        };
        assert_eq!(changes_of(&merged, true).state, ChangeState::Merged);
    }

    #[test]
    fn merging_follows_the_pr_merge_state_not_the_approve_answer() {
        let pr = proto::PrLink {
            repo: "a/b".into(),
            number: 61,
            url: "https://github.com/a/b/pull/61".into(),
            state: "open".into(),
            merge: Some(proto::PrMerge {
                queued_at: 1,
                error: None,
            }),
            ..Default::default()
        };
        assert_eq!(changes_of(&pr, false).state, ChangeState::Merging);
        // GitHub dequeued it (merge conflict): the topic leaves 合并中, approved or not.
        let failed = proto::PrLink {
            merge: Some(proto::PrMerge {
                queued_at: 1,
                error: Some("merge conflict".into()),
            }),
            ..pr.clone()
        };
        assert_eq!(changes_of(&failed, true).state, ChangeState::Open);
    }

    #[test]
    fn the_author_preview_comes_before_the_scraped_page() {
        let p: proto::ArtifactPreview = serde_json::from_value(serde_json::json!({
            "summary": "这周报名 38 人，B 站最多。",
            "metrics": [{ "label": "报名", "value": "38 人", "delta": "+12" }]
        }))
        .unwrap();
        let author = preview_of_version(&p, "报名看板");
        assert_eq!(author.title, "报名看板");
        assert_eq!(
            author.summary.as_deref(),
            Some("这周报名 38 人，B 站最多。")
        );
        assert_eq!(author.metrics.len(), 1);
        assert_eq!(
            (
                author.metrics[0].value.as_str(),
                author.metrics[0].note.as_deref()
            ),
            ("38 人", Some("+12"))
        );
        let scraped = preview_of_html(
            "<h1>别的标题</h1><p>别的</p><table><tr><td>B 站</td><td>9</td></tr></table>",
        );
        let merged = merge_scraped(author, scraped);
        assert_eq!(merged.title, "报名看板");
        assert_eq!(merged.rows.len(), 1);
    }

    #[test]
    fn the_approve_action_is_the_one_with_role_approve() {
        let card: CardV2 = serde_json::from_value(serde_json::json!({
            "v": 2, "rev": 1, "intent": "ask", "title": "合并？", "fallback": "x", "blocks": [],
            "actions": [
                { "id": "changes", "label": "要改", "style": "primary" },
                { "id": "merge", "label": "合并", "role": "approve" }
            ]
        }))
        .unwrap();
        assert_eq!(approve_action_v2(&card).as_deref(), Some("merge"));
        let unknown: CardV2 = serde_json::from_value(serde_json::json!({
            "v": 2, "rev": 1, "intent": "ask", "title": "合并？", "fallback": "x", "blocks": [],
            "actions": [{ "id": "ok", "label": "好", "style": "primary", "role": "later" }]
        }))
        .unwrap();
        assert_eq!(approve_action_v2(&unknown), None);
    }

    #[test]
    fn run_usage_takes_the_reported_time_and_tokens() {
        let run: proto::BotRun = serde_json::from_value(serde_json::json!({
            "id": "r1", "botId": "b", "groupId": "g", "topicId": "t", "status": "done",
            "createdAt": 1, "startedAt": 1000, "endedAt": 61000, "updatedAt": 2,
            "usage": { "inputTokens": 1200, "outputTokens": 300, "costUsd": 0.42 },
            "durationMs": 59000
        }))
        .expect("a run");
        let u = usage_of(&run).expect("usage");
        assert_eq!(u.duration_ms, 59_000);
        assert_eq!((u.input_tokens, u.output_tokens), (Some(1200), Some(300)));
        assert_eq!((u.cost_cents, u.currency), (Some(42), Currency::Usd));
        let bare = proto::BotRun {
            usage: None,
            duration_ms: None,
            started_at: None,
            ..run
        };
        assert_eq!(usage_of(&bare), None);
    }

    #[test]
    fn a_page_preview_has_its_title_summary_and_first_table() {
        let html = r#"<!doctype html><html><head><title>标题</title></head><body>
            <h1>重连测试报告</h1><p>每个场景跑 20 次。</p><p>别的</p>
            <h2>逐项结果</h2>
            <table><thead><tr><th>场景</th><th>结果</th><th>重复</th></tr></thead>
            <tbody><tr><td>断网 5 秒</td><td>✓ 通过</td><td>0</td></tr>
            <tr><td>弱网 &amp; 丢包</td><td>失败</td><td>3</td></tr></tbody></table></body></html>"#;
        let p = preview_of_html(html).expect("a preview");
        assert_eq!(p.title, "重连测试报告");
        assert_eq!(p.summary.as_deref(), Some("每个场景跑 20 次。"));
        assert_eq!(p.table_title.as_deref(), Some("逐项结果"));
        assert_eq!(p.columns, ["场景", "结果", "重复"]);
        assert_eq!(p.rows.len(), 2);
        assert_eq!(
            (
                p.rows[1].label.as_str(),
                p.rows[1].ok,
                p.rows[1].value.as_str()
            ),
            ("弱网 & 丢包", Some(false), "3")
        );
        let t = thumb_of(&p);
        assert_eq!(t.title, "重连测试报告");
        assert!(t.rows[0].ok && !t.rows[1].ok);
        // The <title> is used without a heading; nothing without either.
        assert_eq!(
            preview_of_html("<title>只有标题</title><body>x</body>").map(|p| p.title),
            Some("只有标题".into())
        );
        assert_eq!(preview_of_html("<body><div>x</div></body>"), None);
    }

    #[test]
    fn media_lengths_come_from_mp4_and_wav_headers() {
        let dir = std::env::temp_dir().join(format!("topic-v2-media-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        // ftyp, then moov with mvhd v0: timescale 1000, duration 18000.
        let mut mp4 = Vec::new();
        mp4.extend_from_slice(&16u32.to_be_bytes());
        mp4.extend_from_slice(b"ftypisom\0\0\0\0");
        let mut mvhd = Vec::new();
        mvhd.extend_from_slice(&[0, 0, 0, 0]);
        mvhd.extend_from_slice(&0u32.to_be_bytes());
        mvhd.extend_from_slice(&0u32.to_be_bytes());
        mvhd.extend_from_slice(&1000u32.to_be_bytes());
        mvhd.extend_from_slice(&18_000u32.to_be_bytes());
        let mvhd_box = [&((mvhd.len() + 8) as u32).to_be_bytes()[..], b"mvhd", &mvhd].concat();
        let moov = [
            &((mvhd_box.len() + 8) as u32).to_be_bytes()[..],
            b"moov",
            &mvhd_box,
        ]
        .concat();
        mp4.extend_from_slice(&moov);
        let p = dir.join("a.mp4");
        std::fs::write(&p, &mp4).expect("write");
        assert_eq!(media_length(&p), Some(18_000));
        // 8000 bytes a second, 4000 bytes of data: half a second.
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36u32 + 4000).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8000u32.to_le_bytes());
        wav.extend_from_slice(&8000u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&4000u32.to_le_bytes());
        wav.extend(std::iter::repeat_n(0u8, 4000));
        let p = dir.join("a.wav");
        std::fs::write(&p, &wav).expect("write");
        assert_eq!(media_length(&p), Some(500));
        let p = dir.join("a.txt");
        std::fs::write(&p, b"not a media file").expect("write");
        assert_eq!(media_length(&p), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
