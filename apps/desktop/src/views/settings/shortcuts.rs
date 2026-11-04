//! 快捷键 (NATIVE.md §8): a read-only table of the app's menu shortcuts,
//! one group per menu. It reads the menus and the key bindings themselves,
//! so it never disagrees with the menu bar.

use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, SharedString,
    Styled, Subscription, Window, div,
};
use gpui_component::input::InputState;

use super::widgets;
use crate::{
    copy::settings as s,
    ui::{
        Sym,
        native::{FormRow, GroupedForm, KeyCap, SearchField, SectionTitle, tokens::space},
    },
};

/// One menu and its items that have a shortcut.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutGroup {
    pub menu: SharedString,
    pub rows: Vec<(SharedString, String)>,
}

pub struct ShortcutsPage {
    filter: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl ShortcutsPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder(s::SHORTCUTS_FILTER));
        let subscriptions = vec![cx.observe(&filter, |_, _, cx| cx.notify())];
        Self {
            filter,
            _subscriptions: subscriptions,
        }
    }
}

/// One key cap per key (1.8 Kbd): each modifier, then the key.
pub fn shortcut_caps(text: &str) -> Vec<String> {
    let mut caps = Vec::new();
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if !matches!(c, '⌃' | '⌥' | '⇧' | '⌘') || rest.chars().count() == 1 {
            break;
        }
        caps.push(c.to_string());
        rest = &rest[c.len_utf8()..];
    }
    if !rest.is_empty() {
        caps.push(rest.to_string());
    }
    caps
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
        let query = self.filter.read(cx).value().trim().to_lowercase();
        let groups: Vec<ShortcutGroup> = shortcut_groups(cx)
            .into_iter()
            .filter_map(|mut g| {
                if !query.is_empty() && !g.menu.to_lowercase().contains(&query) {
                    g.rows.retain(|(name, keys)| {
                        name.to_lowercase().contains(&query) || keys.to_lowercase().contains(&query)
                    });
                }
                (!g.rows.is_empty()).then_some(g)
            })
            .collect();
        let empty = groups.is_empty();
        widgets::pane()
            .child(SearchField::new("shortcuts-filter", &self.filter))
            .children(groups.into_iter().enumerate().map(|(ix, g)| {
                let title = SectionTitle::new(g.menu);
                widgets::section(
                    if ix == 0 {
                        title.description(s::SHORTCUTS_FOOTER)
                    } else {
                        title
                    },
                    GroupedForm::new().rows(g.rows.into_iter().map(|(name, keys)| {
                        FormRow::new(name).control(
                            div()
                                .flex()
                                .items_center()
                                .gap(space::S4)
                                .children(shortcut_caps(&keys).into_iter().map(KeyCap::new)),
                        )
                    })),
                )
            }))
            .children(empty.then(|| {
                GroupedForm::new().row(widgets::list_empty(
                    Sym::Search,
                    s::SHORTCUTS_NO_MATCH,
                    None,
                    cx,
                ))
            }))
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
        assert_eq!(shortcut_caps("⌃⌥⇧⌘N"), vec!["⌃", "⌥", "⇧", "⌘", "N"]);
        assert_eq!(shortcut_caps("⌘Tab"), vec!["⌘", "Tab"]);
        assert_eq!(shortcut_caps("⌘"), vec!["⌘"]);
    }
}
