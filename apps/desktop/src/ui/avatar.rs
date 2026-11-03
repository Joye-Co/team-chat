//! `MemberAvatar` (DESIGN.md §7.9): the one avatar path of the app.
//!
//! - People are circles: a photo through gpui-component's `Avatar`, or the
//!   neutral `avatar-bg` disc with `avatar-fg` initials; my own disc is ink
//!   (`btn` with `btn-ink`), as in the rail footer. One character for a
//!   CJK name (the second of a two-character name that starts with 小, 老
//!   or 阿: 小王 is 王), two letters for a Latin name at 32 pt and above, one letter at
//!   24 pt and below; a leading emoji is skipped. A presence dot shows for
//!   people who are online; offline shows no dot.
//! - Bots are rounded-square tiles (radius 0.25 x size) in the bot's tint,
//!   with its jojo-friends character at its live status fitted inside
//!   (`characters::tile`). The status shows as a pip with the size and
//!   offset of a person's presence dot, only for working, needs-input and
//!   error; callers that already show a status label skip `presence`.
//!
//! The shape alone says bot or person.

use gpui::{
    App, Global, Hsla, IntoElement, ParentElement, Pixels, RenderOnce, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::avatar::Avatar;

use crate::{
    app::{Changes, Services},
    characters::{self, CharacterStatus, status::StatusBoard},
    data::{Change, Member},
    theme::{self, Type as _, type_scale},
    ui::icons::{Lucide, icon},
};

/// Ring around presence dots and pips.
const DOT_RING: Pixels = px(2.);

/// My member id, for the ink disc. Kept from the session so an avatar does
/// not ask the provider while it paints.
struct Viewer(Option<String>);

impl Global for Viewer {}

/// Keeps [`Viewer`] equal to the signed-in member. Call once at start.
pub fn track_viewer(services: &Services, cx: &mut App) {
    cx.set_global(Viewer(services.me()));
    let source = services.clone();
    cx.subscribe(&services.hub, move |_, changes: &Changes, cx| {
        if changes.any(|c| matches!(c, Change::Session)) {
            cx.set_global(Viewer(source.me()));
        }
    })
    .detach();
}

/// Whether `id` is me.
fn is_viewer(id: &str, cx: &App) -> bool {
    !id.is_empty()
        && cx
            .try_global::<Viewer>()
            .and_then(|v| v.0.as_deref())
            .is_some_and(|me| me == id)
}

#[derive(IntoElement)]
pub struct MemberAvatar {
    id: String,
    name: String,
    photo: Option<String>,
    bot: bool,
    size: Pixels,
    /// Show the presence dot (people who are online) or the status pip (bots).
    online: bool,
    show_status: bool,
    /// The surface the avatar sits on: the color of the dot ring.
    surface: Option<Hsla>,
    /// The row is selected: a bot tile gets a 1 pt `tile-ring`.
    selected: bool,
    dimmed: bool,
    /// The member, when it has a character (drawn with its live status).
    character: Option<(characters::CharacterId, Member)>,
    /// A fixed status instead of the live one (previews, the self-test).
    status_override: Option<CharacterStatus>,
    /// v3 ring around the avatar: breathing blue while the member works,
    /// static amber while it waits on me.
    ring: Option<Ring>,
}

/// The ring of an avatar (v3): 1.5 pt, 3 pt out from the tile or circle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ring {
    /// `--live`, breathing (static with reduce motion).
    Live,
    /// `--you`, static.
    You,
}

impl MemberAvatar {
    pub fn new(member: Option<&Member>, size: Pixels) -> Self {
        Self {
            id: member.map(|m| m.id.clone()).unwrap_or_default(),
            name: member.map(|m| m.name.clone()).unwrap_or_default(),
            photo: member.and_then(|m| m.avatar_url.clone()),
            bot: member.is_some_and(|m| m.is_bot()),
            size,
            online: false,
            show_status: false,
            surface: None,
            selected: false,
            dimmed: member.is_some_and(|m| m.deactivated),
            character: member.and_then(|m| m.character.map(|c| (c, m.clone()))),
            status_override: None,
            ring: None,
        }
    }

    /// Draws a v3 ring around the avatar (see [`Ring`]).
    pub fn ring(mut self, ring: Option<Ring>) -> Self {
        self.ring = ring;
        self
    }

    /// Shows the presence dot when a person is online, and the status pip of
    /// a bot that works, waits or failed.
    pub fn presence(mut self, member: Option<&Member>) -> Self {
        self.online = member.is_some_and(|m| m.online && !m.is_bot());
        self.show_status = member.is_some_and(|m| m.is_bot());
        self
    }

    /// The surface behind the avatar, for the ring of the dot.
    pub fn on(mut self, surface: Hsla) -> Self {
        self.surface = Some(surface);
        self
    }

    /// The avatar sits on a selected row.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Draws the character at a fixed status.
    pub fn status(mut self, status: CharacterStatus) -> Self {
        self.status_override = Some(status);
        self
    }
}

impl RenderOnce for MemberAvatar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = theme::colors(cx);
        let size = self.size;
        let surface = self.surface.unwrap_or(k.content);
        let small = size <= px(24.);
        let (dot_size, dot_offset) = if small {
            (px(8.), px(-3.))
        } else {
            (px(10.), px(-1.))
        };
        let dot = |color: Hsla| {
            div()
                .absolute()
                .right(dot_offset - DOT_RING)
                .bottom(dot_offset - DOT_RING)
                .size(dot_size + DOT_RING * 2.)
                .rounded_full()
                .border(DOT_RING)
                .border_color(surface)
                .bg(color)
        };

        // Bots: a tinted tile with the fitted character.
        if self.bot || self.character.is_some() {
            let status = match (&self.character, self.status_override) {
                (_, Some(s)) => s,
                (Some((_, member)), None) => StatusBoard::status(cx, member),
                (None, None) => CharacterStatus::Idle,
            };
            // v3: the named member tint (`--c-<name>`), by name when it is
            // a roster name, else by the roster character, else by id.
            let named = theme::MEMBER_TINTS
                .iter()
                .any(|n| n.eq_ignore_ascii_case(self.name.trim()));
            let key = if named {
                self.name.clone()
            } else {
                self.character
                    .as_ref()
                    .map(|(c, _)| c.as_str().to_string())
                    .unwrap_or_else(|| self.id.clone())
            };
            let art = match &self.character {
                Some((c, _)) => characters::tile::tile_image(*c, status, size).into_any_element(),
                None => icon(Lucide::Bot, size * 0.5, k.text_2).into_any_element(),
            };
            let pip = match status {
                CharacterStatus::Working => Some(k.accent),
                CharacterStatus::NeedsInput => Some(k.you_glyph),
                CharacterStatus::Error => Some(k.danger),
                _ => None,
            }
            .filter(|_| self.show_status);
            return div()
                .relative()
                .flex_none()
                .size(size)
                .when(self.dimmed, |el| el.opacity(0.5))
                .child(
                    div()
                        .size(size)
                        .rounded(theme::radius::bot(size))
                        .bg(k.bot_tint(&key))
                        .shadow(k.tile_edge())
                        .when(self.selected, |el| el.border_1().border_color(k.tile_ring))
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(art),
                )
                .children(pip.map(dot))
                .children(
                    self.ring.map(|r| {
                        ring_element(&self.id, r, size, Some(theme::radius::bot(size)), cx)
                    }),
                )
                .into_any_element();
        }

        // People: a circle.
        let text = monogram(&self.name, size);
        let (font, line) = if size <= px(16.) {
            // Breadcrumb size: one character that keeps a margin in the disc.
            (px(10.), size)
        } else if small {
            type_scale::META
        } else {
            let share = if text.chars().count() > 1 { 0.38 } else { 0.44 };
            ((size * share).round(), size)
        };
        // The provider gives the downloaded file; a remote URL is not drawn (it needs the
        // token), so the monogram shows until the file is on disk.
        let local = self
            .photo
            .as_deref()
            .map(|p| p.strip_prefix("file://").unwrap_or(p))
            .filter(|p| p.starts_with('/'))
            .map(str::to_string);
        let face = match local {
            Some(path) => Avatar::new()
                .src(std::path::PathBuf::from(path))
                .size(size)
                .border_0()
                .rounded_full()
                .into_any_element(),
            None => div()
                .size(size)
                .rounded_full()
                .map(|el| {
                    if is_viewer(&self.id, cx) {
                        el.bg(k.btn).text_color(k.btn_ink)
                    } else {
                        el.bg(k.avatar_bg).text_color(k.avatar_fg)
                    }
                })
                .flex()
                .items_center()
                .justify_center()
                .text_size(font)
                .line_height(line)
                .semibold()
                .overflow_hidden()
                .child(text)
                .into_any_element(),
        };
        div()
            .relative()
            .flex_none()
            .size(size)
            .when(self.dimmed, |el| el.opacity(0.5))
            .child(face)
            .when(self.online, |el| el.child(dot(k.online)))
            .children(self.ring.map(|r| ring_element(&self.id, r, size, None, cx)))
            .into_any_element()
    }
}

/// The ring element: absolute, around a box of `size` with `radius` (`None`
/// for a circle). Live rings breathe unless reduce motion is on.
fn ring_element(
    id: &str,
    ring: Ring,
    size: Pixels,
    radius: Option<Pixels>,
    cx: &App,
) -> gpui::AnyElement {
    let k = theme::colors(cx);
    let gap = theme::RING_GAP;
    let color = match ring {
        Ring::Live => k.live,
        Ring::You => k.you,
    };
    let el = div()
        .absolute()
        .left(-gap)
        .top(-gap)
        .size(size + gap * 2.)
        .border(theme::RING_WIDTH)
        .border_color(color)
        .map(|el| match radius {
            Some(r) => el.rounded(r + gap),
            None => el.rounded_full(),
        });
    match ring {
        Ring::You => el.into_any_element(),
        Ring::Live => crate::ui::motion::breathe(format!("ring-{id}"), el, cx),
    }
}

/// The glyph slot of a 群 in lists (§7.9): a monochrome `hash` in a 24 pt
/// slot, never a letter tile. `text-3` at rest, `text-1` when unread,
/// `sel-glyph` when selected.
#[derive(IntoElement)]
pub struct GroupGlyph {
    unread: bool,
    selected: bool,
    size: Pixels,
}

impl GroupGlyph {
    pub fn new() -> Self {
        Self {
            unread: false,
            selected: false,
            size: theme::size::AVATAR_ROW,
        }
    }

    pub fn unread(mut self, unread: bool) -> Self {
        self.unread = unread;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// The slot size (24 in rows; the glyph is 16, 18 from 32 pt slots).
    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }
}

impl Default for GroupGlyph {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderOnce for GroupGlyph {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = theme::colors(cx);
        let color = if self.selected {
            k.sel_glyph
        } else if self.unread {
            k.text_1
        } else {
            k.text_3
        };
        let glyph = if self.size >= px(32.) {
            theme::icon::HEADER
        } else {
            theme::icon::ROW
        };
        div()
            .flex_none()
            .size(self.size)
            .flex()
            .items_center()
            .justify_center()
            .child(icon(Lucide::Hash, glyph, color))
    }
}

/// A neutral rounded tile with the first character of a 群 name, for dialogs
/// that need a group image (§7.9). Lists use [`GroupGlyph`].
#[derive(IntoElement)]
pub struct GroupTile {
    name: String,
    size: Pixels,
}

impl GroupTile {
    pub fn new(_id: impl Into<String>, name: impl Into<String>, size: Pixels) -> Self {
        Self {
            name: name.into(),
            size,
        }
    }
}

impl RenderOnce for GroupTile {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = theme::colors(cx);
        let first = first_letter(&self.name);
        div()
            .flex_none()
            .size(self.size)
            .rounded(theme::radius::bot(self.size))
            .bg(k.tile)
            .text_color(k.text_2)
            .flex()
            .items_center()
            .justify_center()
            .text_size((self.size * 0.5).round())
            .line_height(self.size)
            .semibold()
            .child(first)
    }
}

/// Hebrew, Arabic and the other right-to-left blocks.
fn is_rtl(c: char) -> bool {
    matches!(c as u32, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF)
}

/// `I`, `II`, `III`, `IV`, `V`, `Jr`, `Sr` (with or without the dot or a comma).
fn is_name_suffix(word: &str) -> bool {
    let w = word.trim_end_matches(['.', ',']).to_ascii_lowercase();
    matches!(w.as_str(), "i" | "ii" | "iii" | "iv" | "v" | "jr" | "sr")
}

pub fn is_han(c: char) -> bool {
    matches!(c as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2EBEF)
}

/// Prefixes of a familiar two-character Chinese name (小王, 老李, 阿强):
/// the monogram is the second character.
const FAMILIAR_PREFIXES: [char; 3] = ['小', '老', '阿'];

/// The first letter of a 群 name, upper case.
fn first_letter(name: &str) -> String {
    let trimmed = name.trim_start_matches(|c: char| !c.is_alphanumeric());
    match trimmed.chars().next() {
        Some(c) => c.to_uppercase().collect(),
        None => name
            .trim()
            .chars()
            .next()
            .map(String::from)
            .unwrap_or_default(),
    }
}

/// The initials of a person's name (§7.9): one character for a CJK name
/// (the second of a two-character name that starts with 小, 老 or 阿); two
/// letters for a Latin name at 32 pt and above, one at 24 pt and below; a
/// leading emoji is skipped.
pub fn monogram(name: &str, size: Pixels) -> String {
    // Skip leading symbols (emoji, ZWJ, variation selectors) and spaces.
    let trimmed: &str = name.trim_start_matches(|c: char| !c.is_alphanumeric());
    let trimmed = trimmed.trim();
    let Some(first) = trimmed.chars().next() else {
        return name
            .trim()
            .chars()
            .next()
            .map(String::from)
            .unwrap_or_default();
    };
    if is_han(first) {
        let chars: Vec<char> = trimmed.chars().collect();
        if chars.len() == 2 && FAMILIAR_PREFIXES.contains(&first) && is_han(chars[1]) {
            return chars[1].to_string();
        }
        return first.to_string();
    }
    let upper = |c: char| c.to_uppercase().collect::<String>();
    // Right-to-left scripts (Hebrew, Arabic) join letters: one initial.
    if is_rtl(first) {
        return first.to_string();
    }
    let mut words: Vec<&str> = trimmed.split_whitespace().collect();
    // A generational suffix (III, Jr.) is not the family name.
    while words.len() > 1 && words.last().is_some_and(|w| is_name_suffix(w)) {
        words.pop();
    }
    let two = size >= px(32.) && words.len() > 1;
    let last = words
        .last()
        .and_then(|w| w.chars().find(|c| c.is_alphabetic() && !is_han(*c)));
    match (two, last) {
        (true, Some(l)) => format!("{}{}", upper(first), upper(l)),
        _ => upper(first),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_follow_section_7_9() {
        let big = px(36.);
        let small = px(24.);
        assert_eq!(monogram("王一鸣", big), "王");
        assert_eq!(monogram("王一鸣", small), "王");
        assert_eq!(monogram("Alex Chen", big), "AC");
        assert_eq!(monogram("Alex Chen", small), "A");
        assert_eq!(monogram("Aleksandra Wiśniewska", big), "AW");
        assert_eq!(monogram("Joye", big), "J");
        assert_eq!(monogram("🧑‍💻 Priya", big), "P");
        assert_eq!(monogram("", big), "");
        assert_eq!(monogram("Christopher Alexander Montgomery III", big), "CM");
        assert_eq!(monogram("Martin Luther King Jr.", big), "MK");
        assert_eq!(monogram("John Smith, Sr", big), "JS");
        assert_eq!(monogram("محمد عبد الله", big), "م");
        assert_eq!(monogram("דוד כהן", big), "ד");
    }

    #[test]
    fn familiar_chinese_names_use_the_second_character() {
        let size = px(24.);
        assert_eq!(monogram("小王", size), "王");
        assert_eq!(monogram("老李", size), "李");
        assert_eq!(monogram("阿强", size), "强");
        assert_eq!(monogram("李雪", size), "李");
        assert_eq!(monogram("陈晨", size), "陈");
        // Three characters: a full name, not a familiar form.
        assert_eq!(monogram("小林一", size), "小");
        // One character, or a non-Han second character.
        assert_eq!(monogram("小", size), "小");
        assert_eq!(monogram("小A", size), "小");
    }

    #[test]
    fn group_tiles_keep_the_first_character() {
        assert_eq!(first_letter("小组周会"), "小");
        assert_eq!(first_letter("老友"), "老");
        assert_eq!(first_letter("design"), "D");
        assert_eq!(first_letter("🚀 launch"), "L");
    }
}
