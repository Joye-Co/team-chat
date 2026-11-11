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
//!   with its character at its live status fitted inside
//!   (`characters::tile`). A member whose character has no art in this
//!   build draws its initial, as if it had no
//!   character: a bot in its squircle, a person in its circle. The status shows as a pip with the size and
//!   offset of a person's presence dot, only for working, needs-input and
//!   error; callers that already show a status label skip `presence`.
//!
//! The shape alone says bot or person.

use gpui::{
    App, Hsla, IntoElement, ParentElement, Pixels, RenderOnce, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::avatar::Avatar;

use crate::ui::native::tokens::avatar as avatar_size;
use crate::{
    characters::{self, CharacterStatus, status::StatusBoard},
    data::Member,
    theme::{self, Type as _, type_scale},
    ui::icons::{Sym, icon},
};

/// Ring around presence dots and pips.
const DOT_RING: Pixels = px(2.);

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
    /// v3 status dot on the avatar: blue while the member works, amber
    /// while it waits on me.
    ring: Option<Ring>,
}

/// The status of an avatar (v3), drawn as a small dot on its corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ring {
    /// `--live`: the member works.
    Live,
    /// `--you`: the member waits on me.
    You,
}

impl MemberAvatar {
    pub fn new(member: Option<&Member>, size: Pixels) -> Self {
        let size = snap(size);
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
            // Only a character this build has art for; else the initial.
            character: member.and_then(|m| {
                m.character
                    .filter(|&c| characters::has_art(c))
                    .map(|c| (c, m.clone()))
            }),
            status_override: None,
            ring: None,
        }
    }

    /// Older API: the v3 status dot. It draws nothing now (T1-09, §2.12:
    /// a colored dot with no text is not a status; the presence dot is the
    /// only dot on an avatar). Show the state as a StatusBadge in the row.
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

    /// Draws the character at a fixed status, with its default face.
    pub fn status(mut self, status: CharacterStatus) -> Self {
        self.status_override = Some(status);
        self
    }
}

impl RenderOnce for MemberAvatar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = theme::colors(cx);
        let size = self.size;
        let surface = self.surface.unwrap_or(k.content);
        let small = size <= px(24.);
        // §1.8: the presence dot is 8 with a 2 px ring of the surface.
        let (dot_size, dot_offset) = if small {
            (avatar_size::PRESENCE, px(-3.))
        } else {
            (avatar_size::PRESENCE, px(-1.))
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

        // The provider gives the downloaded file; a remote URL is not drawn (it needs the
        // token), so the fallback shows until the file is on disk.
        let local = self
            .photo
            .as_deref()
            .map(|p| p.strip_prefix("file://").unwrap_or(p))
            .filter(|p| is_local_file(p))
            .map(str::to_string);

        // An AI member with an uploaded photo: the photo in the agent squircle,
        // in place of its character.
        if self.bot
            && let Some(path) = &local
        {
            return div()
                .relative()
                .flex_none()
                .size(size)
                .when(self.dimmed, |el| el.opacity(0.5))
                .child(
                    div()
                        .size(size)
                        .rounded(theme::radius::bot(size))
                        .bg(k.avatar_bg)
                        .shadow(k.tile_edge())
                        .when(self.selected, |el| el.border_1().border_color(k.tile_ring))
                        .overflow_hidden()
                        .child(
                            Avatar::new()
                                .src(std::path::PathBuf::from(path))
                                .size(size)
                                .border_0()
                                .rounded(theme::radius::bot(size)),
                        ),
                )
                .into_any_element();
        }

        // Bots: a tinted tile with the fitted character.
        if self.bot || self.character.is_some() {
            let look: characters::Look = match (&self.character, self.status_override) {
                (_, Some(s)) => s.into(),
                (Some((_, member)), None) => StatusBoard::look(cx, member),
                (None, None) => CharacterStatus::Idle.into(),
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
            // §1.8, T1-09: an agent with no character art falls back to its
            // initial on the muted fill with a 1 px border, like a person,
            // in the agent squircle. No colored status pip or ring: the
            // row's StatusBadge carries the state (§2.12).
            let Some((c, _)) = &self.character else {
                let font = initial_size(size);
                return div()
                    .relative()
                    .flex_none()
                    .size(size)
                    .when(self.dimmed, |el| el.opacity(0.5))
                    .child(
                        div()
                            .size(size)
                            .rounded(theme::radius::bot(size))
                            .bg(k.avatar_bg)
                            .text_color(k.avatar_fg)
                            .border_1()
                            .border_color(k.border)
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(font)
                            .line_height(size)
                            .font_weight(theme::MEDIUM_WEIGHT)
                            .overflow_hidden()
                            .child(monogram(&self.name, size)),
                    )
                    .into_any_element();
            };
            let art = characters::tile::tile_image(*c, look, size, window.scale_factor())
                .into_any_element();
            let _ = (self.show_status, surface, &dot);
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
                .into_any_element();
        }

        // People: a circle.
        let text = monogram(&self.name, size);
        let font = initial_size(size);
        let line = size;
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
                // §1.8: one fallback for every person, me included: the muted
                // fill, a 1 px border line (§2.22) and the initial in foreground.
                .bg(k.avatar_bg)
                .text_color(k.avatar_fg)
                .border_1()
                .border_color(k.border)
                .flex()
                .items_center()
                .justify_center()
                .text_size(font)
                .line_height(line)
                .font_weight(theme::MEDIUM_WEIGHT)
                .overflow_hidden()
                // No name yet (bare data, first sync): a person glyph, never
                // a blank disc (B-16).
                .map(|d| {
                    if text.is_empty() {
                        d.child(icon(Sym::Person, size * 0.5, k.text_2))
                    } else {
                        d.child(text)
                    }
                })
                .into_any_element(),
        };
        div()
            .relative()
            .flex_none()
            .size(size)
            .when(self.dimmed, |el| el.opacity(0.5))
            .child(face)
            .when(self.online, |el| el.child(dot(k.online)))
            .into_any_element()
    }
}

/// §1.8: the initial is 12 at 16-24, 14 at 32-40, 24 at 64.
fn initial_size(size: Pixels) -> Pixels {
    if size <= px(24.) {
        type_scale::META.0
    } else if size <= px(40.) {
        type_scale::UI.0
    } else {
        crate::ui::native::tokens::text::HERO.0
    }
}

/// The status dot of an avatar (was a ring around it): a small `live` or
/// `you` dot on the bottom-right corner, cut out of the avatar by a ring in
/// the surface color, the way macOS draws presence.
/// The avatar sizes of §1.8: 16, 24, 32, 40, 64. Any other size snaps to
/// the nearest one (ties go up).
pub fn snap(size: Pixels) -> Pixels {
    let mut best = avatar_size::SIZES[0];
    for s in avatar_size::SIZES {
        let (d, b) = (
            (f32::from(s) - f32::from(size)).abs(),
            (f32::from(best) - f32::from(size)).abs(),
        );
        if d < b || (d == b && s > best) {
            best = s;
        }
    }
    best
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
            .child(icon(Sym::Topic, glyph, color))
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

/// `name` as it reads next to the member's avatar: a leading emoji (and its
/// ZWJ parts, skin tones and spaces) is left out, so the row never shows a
/// second, emoji "avatar" (review 2 D1-56). A name that is only symbols
/// stays whole.
pub fn name_beside_avatar(name: &str) -> &str {
    let rest = name
        .trim_start_matches(|c: char| c.is_whitespace() || (!c.is_ascii() && !c.is_alphanumeric()));
    if rest.is_empty() { name } else { rest }
}

/// The initials of a person's name (§7.9): one character for a CJK name
/// (the second of a two-character name that starts with 小, 老 or 阿); two
/// letters for a Latin name at 32 pt and above, one at 24 pt and below; a
/// leading emoji is skipped.
/// Whether a member's photo is a file on this computer (the provider hands the
/// downloaded file's path): `/Users/…` on the Mac, `C:\Users\…` on Windows.
pub fn is_local_file(photo: &str) -> bool {
    std::path::Path::new(photo).is_absolute()
}

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
    #[test]
    fn a_photo_on_this_computer_is_an_absolute_path() {
        #[cfg(windows)]
        assert!(super::is_local_file(
            r"C:\Users\me\AppData\Roaming\app\avatars\a.png"
        ));
        #[cfg(not(windows))]
        assert!(super::is_local_file("/Users/me/Library/app/avatars/a.png"));
        assert!(!super::is_local_file("https://example.com/a.png"));
        assert!(!super::is_local_file("avatars/a.png"));
    }

    #[test]
    fn avatar_sizes_snap_to_the_scale() {
        use gpui::px;
        assert_eq!(super::snap(px(20.)), px(24.));
        assert_eq!(super::snap(px(28.)), px(32.));
        assert_eq!(super::snap(px(57.)), px(64.));
        assert_eq!(super::snap(px(21.)), px(24.));
        assert_eq!(super::snap(px(16.)), px(16.));
        assert_eq!(super::snap(px(36.)), px(40.));
    }

    use super::*;

    #[test]
    fn a_leading_emoji_stays_out_of_the_name_beside_the_avatar() {
        assert_eq!(name_beside_avatar("👩🏽\u{200d}💻 Priya"), "Priya");
        assert_eq!(name_beside_avatar("🦊 代码审查助手"), "代码审查助手");
        assert_eq!(name_beside_avatar("🚀🚀🚀"), "🚀🚀🚀");
        assert_eq!(name_beside_avatar("@ops 王"), "@ops 王");
        assert_eq!(name_beside_avatar("Joye"), "Joye");
    }

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
    fn a_character_without_art_draws_the_initial() {
        let member = Member {
            id: "m-bot".into(),
            kind: crate::data::MemberKind::Bot,
            name: "Forge".into(),
            role: None,
            level: crate::data::MemberLevel::Member,
            avatar_url: None,
            deactivated: false,
            online: true,
            character: Some(characters::CharacterId::O08),
            team_owner: false,
        };
        let avatar = MemberAvatar::new(Some(&member), px(32.));
        assert!(avatar.bot);
        assert_eq!(
            avatar.character.is_some(),
            characters::has_art(characters::CharacterId::O08)
        );
        assert_eq!(monogram(&avatar.name, px(32.)), "F");
    }

    #[test]
    fn group_tiles_keep_the_first_character() {
        assert_eq!(first_letter("小组周会"), "小");
        assert_eq!(first_letter("老友"), "老");
        assert_eq!(first_letter("design"), "D");
        assert_eq!(first_letter("🚀 launch"), "L");
    }
}
