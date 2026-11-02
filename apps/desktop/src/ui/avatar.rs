//! `MemberAvatar` (DESIGN.md §14): the one avatar path of the app. People are
//! circles, bots are rounded squares with a graphite mark. A photo goes
//! through gpui-component's `Avatar`; without one the monogram box is drawn
//! here with the §2.7 palettes, because `Avatar`'s initials and colors do not
//! follow our rules.

use gpui::{
    App, Hsla, IntoElement, ParentElement, Pixels, RenderOnce, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::avatar::Avatar;

use crate::{
    data::Member,
    theme::{self, Type as _},
    ui::icons::{Lucide, icon},
};

/// Ring width of the bot mark and presence dot.
const RING: Pixels = px(1.5);

#[derive(IntoElement)]
pub struct MemberAvatar {
    id: String,
    name: String,
    photo: Option<String>,
    bot: bool,
    size: Pixels,
    /// Show the presence dot (people who are online).
    online: bool,
    /// The surface the avatar sits on: the color of the 1.5 pt rings.
    surface: Option<Hsla>,
    dimmed: bool,
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
            surface: None,
            dimmed: member.is_some_and(|m| m.deactivated),
        }
    }

    /// Shows the presence dot when the member is online (people only; the
    /// bot mark takes that corner on bots).
    pub fn presence(mut self, member: Option<&Member>) -> Self {
        self.online = member.is_some_and(|m| m.online && !m.is_bot());
        self
    }

    /// The surface behind the avatar, for the rings of the mark and the dot.
    pub fn on(mut self, surface: Hsla) -> Self {
        self.surface = Some(surface);
        self
    }
}

impl RenderOnce for MemberAvatar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = theme::colors(cx);
        let size = self.size;
        let radius = if self.bot {
            theme::radius::bot(size)
        } else {
            size / 2.
        };
        let surface = self.surface.unwrap_or(k.content);
        let (bg, fg) = k.palette(theme::palette_index(&self.id));
        let text = monogram(&self.name, self.bot, size);
        let han = text.chars().any(is_han);
        let font = match (han, text.chars().count()) {
            (true, 2) => size * 0.34,
            (true, _) => size * 0.44,
            _ => size * 0.38,
        };

        let face = match self.photo {
            Some(url) => Avatar::new()
                .src(url)
                .size(size)
                .border_0()
                .rounded(radius)
                .into_any_element(),
            None => div()
                .size(size)
                .rounded(radius)
                .bg(bg)
                .text_color(fg)
                .flex()
                .items_center()
                .justify_center()
                .text_size(font)
                .line_height(size)
                .medium()
                .overflow_hidden()
                .child(text)
                .into_any_element(),
        };

        let mark = self.bot.then(|| {
            let mark = (size * 0.42).max(px(11.));
            let glyph = (size * 0.28).max(px(8.));
            let offset = -(mark * 0.22) - RING;
            div()
                .absolute()
                .right(offset)
                .bottom(offset)
                .size(mark + RING * 2.)
                .rounded_full()
                .border(RING)
                .border_color(surface)
                .bg(k.bot_mark)
                .flex()
                .items_center()
                .justify_center()
                .child(icon(Lucide::Bot, glyph, k.on_accent))
        });
        let dot = self.online.then(|| {
            let d = (size * 0.3).max(px(8.));
            div()
                .absolute()
                .right(-RING)
                .bottom(-RING)
                .size(d + RING * 2.)
                .rounded_full()
                .border(RING)
                .border_color(surface)
                .bg(k.online)
        });

        div()
            .relative()
            .flex_none()
            .size(size)
            .when(self.dimmed, |el| el.opacity(0.5))
            .child(face)
            .children(mark)
            .children(dot)
    }
}

/// A rounded-square tile with the first character of a 群 name (§5.2).
#[derive(IntoElement)]
pub struct GroupTile {
    id: String,
    name: String,
    size: Pixels,
}

impl GroupTile {
    pub fn new(id: impl Into<String>, name: impl Into<String>, size: Pixels) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            size,
        }
    }
}

impl RenderOnce for GroupTile {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = theme::colors(cx);
        let (bg, fg) = k.palette(theme::palette_index(&self.id));
        let first = self
            .name
            .chars()
            .find(|c| c.is_alphanumeric())
            .map(|c| c.to_uppercase().collect::<String>())
            // Emoji-only names show their first character.
            .or_else(|| self.name.trim().chars().next().map(String::from))
            .unwrap_or_default();
        div()
            .flex_none()
            .size(self.size)
            .rounded(theme::radius::TILE)
            .bg(bg)
            .text_color(fg)
            .flex()
            .items_center()
            .justify_center()
            .text_size(self.size * 0.55)
            .line_height(self.size)
            .semibold()
            .child(first)
    }
}

pub fn is_han(c: char) -> bool {
    matches!(c as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2EBEF)
}

/// The monogram of a name (§14).
pub fn monogram(name: &str, bot: bool, size: Pixels) -> String {
    let name = name.trim();
    let chars: Vec<char> = name.chars().collect();
    let Some(&first) = chars.first() else {
        return String::new();
    };
    if chars.iter().all(|&c| is_han(c)) {
        let two = size >= px(28.) && chars.len() >= 2;
        return match (two, bot) {
            (false, _) => first.to_string(),
            (true, true) => chars[..2].iter().collect(),
            (true, false) => chars[chars.len() - 2..].iter().collect(),
        };
    }
    let latin_word = |w: &&str| {
        w.chars()
            .all(|c| c.is_alphabetic() && !is_han(c) || c == '-' || c == '\'' || c == '.')
    };
    let words: Vec<&str> = name.split_whitespace().collect();
    if words.iter().all(latin_word) && first.is_alphabetic() {
        let initial = |w: &str| {
            w.chars()
                .next()
                .map(|c| c.to_uppercase().collect::<String>())
        };
        return match words.as_slice() {
            [one] => initial(one).unwrap_or_default(),
            [a, .., b] => format!(
                "{}{}",
                initial(a).unwrap_or_default(),
                initial(b).unwrap_or_default()
            ),
            [] => String::new(),
        };
    }
    // Mixed or emoji-first: the first letter.
    chars
        .iter()
        .find(|c| c.is_alphabetic())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| first.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monograms_follow_section_14() {
        let big = px(32.);
        let small = px(20.);
        assert_eq!(monogram("林晓", false, big), "林晓");
        assert_eq!(monogram("陈思远", false, big), "思远");
        assert_eq!(monogram("设计助手", true, big), "设计");
        assert_eq!(monogram("陈思远", false, small), "陈");
        assert_eq!(monogram("Aleksandra Wiśniewska", false, big), "AW");
        assert_eq!(monogram("Joye", false, big), "J");
        assert_eq!(monogram("🦊 Fox 狐狸", false, big), "F");
        assert_eq!(monogram("王", false, big), "王");
        assert_eq!(monogram("", false, big), "");
    }
}
