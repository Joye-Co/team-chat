//! Emoji parts shared by the composer and the timeline: the picker grid
//! (the composer's 表情 tool and the hover toolbar's add reaction) and the
//! reaction pills under a message (DESIGN.md §7.5).

use gpui::{
    App, ElementId, InteractiveElement as _, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use gpui_component::{
    Sizable as _,
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
};

use crate::{
    theme::{self, Type as _, size, space, type_scale},
    ui::{Lucide, icon},
};

/// Emoji in the picker: the reactions a team uses most.
pub const EMOJI: &[&str] = &[
    "👍", "👌", "🙏", "👏", "🎉", "✅", "❌", "👀", "🤔", "😂", "😅", "🙂", "😮", "😢", "🔥", "💯",
    "🚀", "⚠️", "📌", "💡", "☕", "🐛", "🛠️", "📎",
];

/// Cells per picker row.
pub const PICKER_COLUMNS: f32 = 8.;
const CELL: f32 = 36.;

/// The picker's width: eight 36 pt cells plus the surface padding.
pub fn picker_width() -> gpui::Pixels {
    px(PICKER_COLUMNS * CELL + 8.)
}

/// One picker cell: 36 × 36, radius 6, a 20 pt emoji; hover `hover`,
/// pressed `pressed`.
pub fn picker_cell(
    id: impl Into<ElementId>,
    emoji: &'static str,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let colors = theme::colors(cx);
    div()
        .id(id)
        .size(px(CELL))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::radius::SM)
        .cursor_pointer()
        .text_size(px(20.))
        .hover(|el| el.bg(colors.hover))
        .active(|el| el.bg(colors.pressed))
        .child(emoji)
}

/// A reaction pill (§7.5): 26 pt, radius 13, padding 0 8 0 6, gap 4,
/// `field`; a 16 pt emoji and a 12/16 400 label. Mine: `accent-soft`, a
/// 1 pt `accent` ring and `accent-text`.
pub fn reaction_pill(
    id: impl Into<ElementId>,
    emoji: SharedString,
    label: SharedString,
    mine: bool,
    cx: &App,
) -> Button {
    let k = theme::colors(cx);
    // gpui-component draws a custom variant's rest fill at 20 %, so the
    // spec fill is set on the button itself; hover and pressed stay full.
    let (fill, variant) = if mine {
        (
            k.accent_soft,
            ButtonCustomVariant::new(cx)
                .color(k.accent_soft)
                .foreground(k.accent_text)
                .hover(k.focus_soft)
                .active(k.focus_soft),
        )
    } else {
        (
            k.field,
            ButtonCustomVariant::new(cx)
                .color(k.field)
                .foreground(k.text_2)
                .hover(k.pressed)
                .active(k.border),
        )
    };
    let button = Button::new(id)
        .custom(variant)
        .with_size(gpui_component::Size::Small)
        .h(size::REACTION)
        .pl(space::S6)
        .pr(space::S8)
        .rounded(size::REACTION / 2.)
        .bg(fill)
        .child(
            div()
                .flex()
                .items_center()
                .gap(space::S4)
                .child(div().type_role(type_scale::REACTION_EMOJI).child(emoji))
                .child(div().t_meta().tnum().one_line().child(label)),
        );
    if mine {
        button.border_1().border_color(k.accent)
    } else {
        button
    }
}

/// The pill after the reactions that opens the picker: 26 pt, radius 13,
/// `field`, `face-slightly-smiling-plus` 16 in `text-2`.
pub fn reaction_add(id: impl Into<ElementId>, cx: &App) -> Button {
    let k = theme::colors(cx);
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(k.field)
                .foreground(k.text_2)
                .hover(k.pressed)
                .active(k.border),
        )
        .with_size(gpui_component::Size::Small)
        .h(size::REACTION)
        .w(px(34.))
        .rounded(size::REACTION / 2.)
        .bg(k.field)
        .child(icon(
            Lucide::FaceSlightlySmilingPlus,
            theme::icon::ROW,
            k.text_2,
        ))
}

/// The pill label (§7.5): the names for one or two members (`Joye、Tata`),
/// else the count, capped at `99+`. An unknown member (empty name) makes it
/// a count too.
pub fn reaction_label(names: &[String]) -> String {
    let named = names.iter().all(|n| !n.is_empty());
    match names {
        [one] if named => short_name(one),
        [a, b] if named => format!("{}、{}", short_name(a), short_name(b)),
        _ if names.len() > 99 => "99+".into(),
        _ => names.len().to_string(),
    }
}

/// A name for a pill or the typing line: an email shows its local part, and
/// a long name keeps about six CJK characters (twelve Latin ones), then `…`.
pub fn short_name(name: &str) -> String {
    const MAX: usize = 12;
    let name = name.split('@').next().unwrap_or(name).trim();
    let width = |c: char| if c >= '\u{2E80}' { 2 } else { 1 };
    if name.chars().map(width).sum::<usize>() <= MAX {
        return name.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in name.chars() {
        if used + width(c) > MAX - 1 {
            break;
        }
        used += width(c);
        out.push(c);
    }
    format!("{}…", out.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_name_two_then_count() {
        let n = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(reaction_label(&n(&["Joye"])), "Joye");
        assert_eq!(reaction_label(&n(&["Joye", "Tata"])), "Joye、Tata");
        assert_eq!(reaction_label(&n(&["a", "b", "c"])), "3");
        assert_eq!(reaction_label(&vec!["x".to_string(); 120]), "99+");
        assert_eq!(reaction_label(&n(&["Joye", ""])), "2");
    }

    #[test]
    fn shortens_emails_and_long_names() {
        assert_eq!(short_name("李雪"), "李雪");
        assert_eq!(
            short_name("bartholomew.fitzgerald@northwind.example"),
            "bartholomew…"
        );
        assert_eq!(short_name("Aleksandra Wiśniewska"), "Aleksandra…");
        assert_eq!(short_name("产品设计与前端体验改进"), "产品设计与…");
        assert_eq!(short_name("欧阳慕容"), "欧阳慕容");
    }
}
