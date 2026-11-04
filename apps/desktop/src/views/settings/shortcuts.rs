//! 快捷键 (NATIVE.md §8): a read-only table of the app's menu shortcuts,
//! one group per menu. It reads the menus and the key bindings themselves,
//! so it never disagrees with the menu bar.

use gpui::{
    App, Context, IntoElement, ParentElement, Render, SharedString, Window, div, px,
};

use super::widgets;
use crate::{
    copy::settings as s,
    ui::native::{FormRow, GroupedForm},
};

/// One menu and its items that have a shortcut.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutGroup {
    pub menu: SharedString,
    pub rows: Vec<(SharedString, String)>,
}

pub struct ShortcutsPage;

impl ShortcutsPage {
    pub fn new() -> Self {
        Self
    }
}

/// A key as the Mac menus write it: ⌃⌥⇧⌘ then the key.
pub fn shortcut_text(modifiers: &gpui::Modifiers, key: &str) -> String {
    let mut out = String::new();
    if modifiers.control {
        out.push('⌃');
    }
    if modifiers.alt {
        out.push('⌥');
    }
    if modifiers.shift {
        out.push('⇧');
    }
    if modifiers.platform {
        out.push('⌘');
    }
    let key = match key {
        "up" => "↑".to_string(),
        "down" => "↓".to_string(),
        "left" => "←".to_string(),
        "right" => "→".to_string(),
        "enter" => "↩".to_string(),
        "escape" => "⎋".to_string(),
        "backspace" => "⌫".to_string(),
        "delete" => "⌦".to_string(),
        "tab" => "⇥".to_string(),
        "space" => "␣".to_string(),
        k if k.chars().count() == 1 => k.to_uppercase(),
        k => {
            let mut c = k.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect())
                .unwrap_or_default()
        }
    };
    out.push_str(&key);
    out
}

/// The menus of the menu bar with the shortcut of each item that has one.
pub fn shortcut_groups(cx: &App) -> Vec<ShortcutGroup> {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let mut out = Vec::new();
    for menu in crate::views::nav::menus::app_menus() {
        let mut rows = Vec::new();
        collect(&menu.items, &keymap, &mut rows);
        if !rows.is_empty() {
            out.push(ShortcutGroup {
                menu: menu.name.clone(),
                rows,
            });
        }
    }
    out
}

fn collect(
    items: &[gpui::MenuItem],
    keymap: &gpui::Keymap,
    rows: &mut Vec<(SharedString, String)>,
) {
    for item in items {
        match item {
            gpui::MenuItem::Action { name, action, .. } => {
                // The last binding wins for display, as in the menu bar.
                if let Some(binding) = keymap.bindings_for_action(action.as_ref()).last()
                    && let [stroke] = binding.keystrokes()
                {
                    rows.push((
                        name.clone(),
                        shortcut_text(stroke.modifiers(), stroke.key()),
                    ));
                }
            }
            gpui::MenuItem::Submenu(menu) => collect(&menu.items, keymap, rows),
            _ => {}
        }
    }
}

impl Render for ShortcutsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let groups = shortcut_groups(cx);
        let n = groups.len();
        widgets::pane()
            .children(groups.into_iter().enumerate().map(|(ix, g)| {
                let form = GroupedForm::new().header(g.menu).rows(
                    g.rows
                        .into_iter()
                        .map(|(name, keys)| FormRow::new(name).value(keys, cx).min_height(px(30.))),
                );
                if ix + 1 == n {
                    form.footer(s::SHORTCUTS_FOOTER)
                } else {
                    form
                }
            }))
            .children((n == 0).then(|| div().child(widgets::quiet(s::SHORTCUTS_FOOTER, cx))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_read_as_mac_menus_write_them() {
        let cmd = gpui::Modifiers {
            platform: true,
            ..Default::default()
        };
        assert_eq!(shortcut_text(&cmd, ","), "⌘,");
        let all = gpui::Modifiers {
            platform: true,
            shift: true,
            alt: true,
            control: true,
            ..Default::default()
        };
        assert_eq!(shortcut_text(&all, "n"), "⌃⌥⇧⌘N");
        assert_eq!(shortcut_text(&cmd, "down"), "⌘↓");
    }
}
