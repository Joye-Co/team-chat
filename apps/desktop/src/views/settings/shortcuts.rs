//! 快捷键 (NATIVE.md §8): every menu command, one group per menu, with its
//! shortcut. A click on a shortcut records a new one: the next key press
//! with ⌘, ⌃ or ⌥ becomes the command's shortcut (Esc cancels, ⌫ removes
//! it), the menu bar shows it at once, and it is saved with the local
//! settings. It reads the menus and the key bindings themselves, so it never
//! disagrees with the menu bar.

use std::{cell::RefCell, collections::BTreeMap};

use gpui::{
    Action, App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    KeyBinding, Keystroke, ParentElement, Render, SharedString, Styled, Subscription, WeakEntity,
    Window, div,
};
use gpui_component::input::InputState;

use super::{prefs, widgets};
use crate::{
    copy::{fill, settings as s},
    ui::{
        Sym,
        native::{
            FormRow, GroupedForm, KeyCap, PushSize, PushStyle, SearchField, SectionTitle,
            native_colors, push_button, row_button,
            tokens::{NativeType as _, metrics, space},
        },
    },
};

/// One menu and its commands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutGroup {
    pub menu: SharedString,
    pub rows: Vec<ShortcutRow>,
}

/// A menu command: its title, its action's name and its shortcut as the
/// menus write it (`None` when it has none).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutRow {
    pub name: SharedString,
    pub action: String,
    pub keys: Option<String>,
}

pub struct ShortcutsPage {
    filter: Entity<InputState>,
    /// The action whose shortcut is being recorded.
    recording: Option<String>,
    /// Why the last key press was refused while recording.
    refused: Option<SharedString>,
    /// Takes every key press while recording, before menus and bindings.
    intercept: Option<Subscription>,
    _subscriptions: Vec<Subscription>,
}

impl ShortcutsPage {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder(s::SHORTCUTS_FILTER));
        let subscriptions = vec![cx.observe(&filter, |_, _, cx| cx.notify())];
        Self {
            filter,
            recording: None,
            refused: None,
            intercept: None,
            _subscriptions: subscriptions,
        }
    }

    fn start(&mut self, action: String, cx: &mut Context<Self>) {
        let this: WeakEntity<Self> = cx.entity().downgrade();
        self.intercept = Some(cx.intercept_keystrokes(move |event, _window, cx| {
            let keystroke = event.keystroke.clone();
            let _ = this.update(cx, |p, cx| p.pressed(&keystroke, cx));
            cx.stop_propagation();
        }));
        self.recording = Some(action);
        self.refused = None;
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        self.recording = None;
        self.refused = None;
        self.intercept = None;
        cx.notify();
    }

    /// A key press while recording.
    fn pressed(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        let Some(action) = self.recording.clone() else {
            return;
        };
        let m = keystroke.modifiers;
        let plain = !(m.platform || m.control || m.alt || m.shift);
        match keystroke.key.as_str() {
            "escape" if plain => return self.stop(cx),
            "backspace" | "delete" if plain => {
                save(&action, String::new(), cx);
                return self.stop(cx);
            }
            _ => {}
        }
        let function_key = keystroke.key.len() > 1
            && keystroke.key.starts_with('f')
            && keystroke.key[1..].chars().all(|c| c.is_ascii_digit());
        if !(m.platform || m.control || m.alt || function_key) {
            self.refused = Some(s::SHORTCUT_NEEDS_MODIFIER.into());
            return cx.notify();
        }
        let source = keystroke_source(keystroke);
        if let Some(other) = taken_by(&source, &action, cx) {
            self.refused = Some(fill(s::SHORTCUT_TAKEN, &[("name", &other)]).into());
            return cx.notify();
        }
        save(&action, source, cx);
        self.stop(cx);
    }

    /// The shortcut of one row: its key caps (or 未设置) on a quiet button
    /// that starts recording; while recording, the prompt (or why a key was
    /// refused) in a focused field-like pill.
    fn control(&self, row: &ShortcutRow, cx: &mut Context<Self>) -> gpui::AnyElement {
        let k = native_colors(cx);
        let id = SharedString::from(format!("shortcut-{}", row.action));
        if self.recording.as_deref() == Some(row.action.as_str()) {
            let refused = self.refused.clone();
            return div()
                .id(id)
                .h(metrics::BUTTON_SMALL)
                .px(space::S12)
                .rounded(metrics::BUTTON_SMALL_RADIUS)
                .bg(k.control)
                .border_1()
                .border_color(k.label)
                .shadow(k.focus_ring())
                .flex()
                .items_center()
                .gap(space::S8)
                .n_body()
                .text_color(if refused.is_some() {
                    k.label
                } else {
                    k.secondary
                })
                .child(refused.unwrap_or_else(|| s::SHORTCUT_RECORD.into()))
                .child(KeyCap::new("esc"))
                .on_mouse_down_out(cx.listener(|this, _, _, cx| this.stop(cx)))
                .into_any_element();
        }
        let custom = prefs::get().shortcuts.contains_key(&row.action);
        let action = row.action.clone();
        let keys: gpui::AnyElement = match &row.keys {
            Some(keys) => div()
                .flex()
                .items_center()
                .gap(space::S4)
                .children(shortcut_caps(keys).into_iter().map(KeyCap::new))
                .into_any_element(),
            None => div()
                .n_body()
                .text_color(k.tertiary)
                .child(s::SHORTCUT_NONE)
                .into_any_element(),
        };
        let edit = row_button(id, cx)
            .w_auto()
            .h(metrics::BUTTON_SMALL)
            .px(space::S8)
            .rounded(metrics::BUTTON_SMALL_RADIUS)
            .tooltip(fill(s::SHORTCUT_CHANGE, &[("name", &row.name)]))
            .child(keys)
            .on_click(cx.listener(move |this, _, _, cx| this.start(action.clone(), cx)));
        let reset = custom.then(|| {
            let action = row.action.clone();
            push_button(
                SharedString::from(format!("shortcut-reset-{}", row.action)),
                s::SHORTCUT_RESET,
                PushStyle::Link,
                PushSize::Small,
                cx,
            )
            .on_click(cx.listener(move |_, _, _, cx| {
                reset(Some(&action), cx);
                cx.notify();
            }))
        });
        div()
            .flex()
            .items_center()
            .gap(space::S8)
            .children(reset)
            .child(edit)
            .into_any_element()
    }
}

/// One key cap per key (1.8 Kbd): each modifier, then the key.
#[cfg(not(target_os = "macos"))]
pub fn shortcut_caps(text: &str) -> Vec<String> {
    // "Ctrl+Shift+K"; a trailing "+" is the plus key itself ("Ctrl++").
    let (text, plus) = match text.strip_suffix("++") {
        Some(rest) => (rest, true),
        None if text == "+" => ("", true),
        None => (text, false),
    };
    let mut caps: Vec<String> = text
        .split('+')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    if plus {
        caps.push("+".into());
    }
    caps
}

/// One key cap per key (1.8 Kbd): each modifier, then the key.
#[cfg(target_os = "macos")]
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

/// A key as Windows menus write it: Ctrl+Alt+Shift+Win+ then the key.
#[cfg(not(target_os = "macos"))]
pub fn shortcut_text(modifiers: &gpui::Modifiers, key: &str) -> String {
    let mut out = String::new();
    for (on, name) in [
        (modifiers.control, "Ctrl+"),
        (modifiers.alt, "Alt+"),
        (modifiers.shift, "Shift+"),
        (modifiers.platform, "Win+"),
    ] {
        if on {
            out.push_str(name);
        }
    }
    let key = match key {
        "up" => "↑".to_string(),
        "down" => "↓".to_string(),
        "left" => "←".to_string(),
        "right" => "→".to_string(),
        "escape" => "Esc".to_string(),
        "backspace" => "Backspace".to_string(),
        "delete" => "Del".to_string(),
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

/// A key as the Mac menus write it: ⌃⌥⇧⌘ then the key.
#[cfg(target_os = "macos")]
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

/// The menus of the menu bar with every command and its shortcut.
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

fn collect(items: &[gpui::MenuItem], keymap: &gpui::Keymap, rows: &mut Vec<ShortcutRow>) {
    for item in items {
        match item {
            gpui::MenuItem::Action { name, action, .. } => {
                // The last binding wins for display, as in the menu bar.
                let keys = keymap
                    .bindings_for_action(action.as_ref())
                    .last()
                    .and_then(|b| match b.keystrokes() {
                        [stroke] => Some(shortcut_text(stroke.modifiers(), stroke.key())),
                        _ => None,
                    });
                rows.push(ShortcutRow {
                    name: name.clone(),
                    action: action.name().to_string(),
                    keys,
                });
            }
            gpui::MenuItem::Submenu(menu) => collect(&menu.items, keymap, rows),
            _ => {}
        }
    }
}

/// Every menu action by name.
fn menu_actions() -> BTreeMap<String, (SharedString, Box<dyn Action>)> {
    fn walk(items: &[gpui::MenuItem], out: &mut BTreeMap<String, (SharedString, Box<dyn Action>)>) {
        for item in items {
            match item {
                gpui::MenuItem::Action { name, action, .. } => {
                    out.entry(action.name().to_string())
                        .or_insert_with(|| (name.clone(), action.boxed_clone()));
                }
                gpui::MenuItem::Submenu(menu) => walk(&menu.items, out),
                _ => {}
            }
        }
    }
    let mut out = BTreeMap::new();
    for menu in crate::views::nav::menus::app_menus() {
        walk(&menu.items, &mut out);
    }
    out
}

thread_local! {
    /// The app's own key bindings, as installed at start, before any change.
    static DEFAULTS: RefCell<Option<Vec<KeyBinding>>> = const { RefCell::new(None) };
}

/// At start, after every default binding: binds the shortcuts saved in
/// Settings.
pub fn apply_saved(cx: &mut App) {
    let saved = prefs::get().shortcuts;
    if !saved.is_empty() {
        rebind(&saved, cx);
    }
}

/// The keymap again: the defaults without the global binding of each changed
/// action, then the changed shortcuts; the menu bar follows.
fn rebind(changed: &BTreeMap<String, String>, cx: &mut App) {
    let defaults = DEFAULTS.with(|d| {
        d.borrow_mut()
            .get_or_insert_with(|| cx.key_bindings().borrow().bindings().cloned().collect())
            .clone()
    });
    let actions = menu_actions();
    cx.clear_key_bindings();
    cx.bind_keys(
        defaults
            .into_iter()
            .filter(|b| !(b.predicate().is_none() && changed.contains_key(b.action().name()))),
    );
    for (name, source) in changed {
        if source.is_empty() {
            continue;
        }
        if let Some((_, action)) = actions.get(name)
            && let Ok(binding) =
                KeyBinding::load(source, action.boxed_clone(), None, false, None, cx)
        {
            cx.bind_keys([binding]);
        }
    }
    crate::views::nav::menus::refresh(cx);
}

/// Saves `source` ("" = none) as the shortcut of `action` and binds it.
fn save(action: &str, source: String, cx: &mut App) {
    prefs::update(|p| {
        p.shortcuts.insert(action.to_string(), source);
    });
    rebind(&prefs::get().shortcuts, cx);
}

/// Puts back the default of `action` (every action when `None`).
fn reset(action: Option<&str>, cx: &mut App) {
    prefs::update(|p| match action {
        Some(a) => {
            p.shortcuts.remove(a);
        }
        None => p.shortcuts.clear(),
    });
    rebind(&prefs::get().shortcuts, cx);
}

/// The source form of a key press ("ctrl-alt-shift-cmd-k").
pub fn keystroke_source(k: &Keystroke) -> String {
    let m = k.modifiers;
    let mut out = String::new();
    for (on, name) in [
        (m.control, "ctrl-"),
        (m.alt, "alt-"),
        (m.shift, "shift-"),
        (m.platform, "cmd-"),
    ] {
        if on {
            out.push_str(name);
        }
    }
    out.push_str(&k.key);
    out
}

/// The menu command that already has `source` as its global shortcut, other
/// than `action`.
fn taken_by(source: &str, action: &str, cx: &App) -> Option<String> {
    let actions = menu_actions();
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    keymap.bindings().rev().find_map(|b| {
        let name = b.action().name();
        if b.predicate().is_some() || name == action {
            return None;
        }
        let [stroke] = b.keystrokes() else {
            return None;
        };
        let same = keystroke_source(stroke.inner()) == source;
        same.then(|| actions.get(name).map(|(title, _)| title.to_string()))
            .flatten()
    })
}

impl Render for ShortcutsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.filter.read(cx).value().trim().to_lowercase();
        let groups: Vec<ShortcutGroup> = shortcut_groups(cx)
            .into_iter()
            .filter_map(|mut g| {
                if !query.is_empty() && !g.menu.to_lowercase().contains(&query) {
                    g.rows.retain(|r| {
                        r.name.to_lowercase().contains(&query)
                            || r.keys
                                .as_deref()
                                .is_some_and(|k| k.to_lowercase().contains(&query))
                    });
                }
                (!g.rows.is_empty()).then_some(g)
            })
            .collect();
        let empty = groups.is_empty();
        let any_custom = !prefs::get().shortcuts.is_empty();
        let sections: Vec<_> = groups
            .into_iter()
            .enumerate()
            .map(|(ix, g)| {
                let mut title = SectionTitle::new(g.menu);
                if ix == 0 {
                    title = title.description(s::SHORTCUTS_FOOTER);
                    if any_custom {
                        title = title.action(
                            push_button(
                                "shortcuts-reset-all",
                                s::SHORTCUT_RESET_ALL,
                                PushStyle::Outline,
                                PushSize::Small,
                                cx,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.stop(cx);
                                reset(None, cx);
                            })),
                        );
                    }
                }
                let rows: Vec<_> = g
                    .rows
                    .iter()
                    .map(|r| FormRow::new(r.name.clone()).control(self.control(r, cx)))
                    .collect();
                widgets::section(title, GroupedForm::new().rows(rows))
            })
            .collect();
        widgets::pane()
            .child(SearchField::new("shortcuts-filter", &self.filter))
            .children(sections)
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

    /// Recording: a click arms the row, the next key press with ⌘ becomes
    /// the command's shortcut (the old one is gone), a key another command
    /// holds is refused, and 恢复默认 puts the default back.
    #[gpui::test]
    fn a_recorded_key_becomes_the_shortcut(cx: &mut gpui::TestAppContext) {
        use gpui::{AppContext as _, VisualTestContext};
        prefs::stop_saving();
        cx.update(|cx| {
            gpui_component::init(cx);
            cx.bind_keys([
                KeyBinding::new("cmd-,", crate::app::OpenSettings, None),
                KeyBinding::new("cmd-q", crate::app::Quit, None),
            ]);
        });
        let (page, cx) = cx.add_window_view(|window, cx| ShortcutsPage::new(window, cx));
        let cx: &mut VisualTestContext = cx;
        let settings = crate::app::OpenSettings.name().to_string();
        let keys = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| {
                shortcut_groups(cx)
                    .into_iter()
                    .flat_map(|g| g.rows)
                    .find(|r| r.action == crate::app::OpenSettings.name())
                    .and_then(|r| r.keys)
            })
        };
        // The test binds the platform key itself (⌘, the Windows key).
        let (comma, shift_k) = if cfg!(target_os = "macos") {
            ("⌘,", "⇧⌘K")
        } else {
            ("Win+,", "Shift+Win+K")
        };
        assert_eq!(keys(cx).as_deref(), Some(comma));
        page.update(cx, |p, cx| p.start(settings.clone(), cx));
        cx.simulate_keystrokes("cmd-q");
        page.read_with(cx, |p, _| {
            assert!(p.recording.is_some(), "⌘Q belongs to 退出: still recording");
            assert!(p.refused.is_some());
        });
        cx.simulate_keystrokes("k");
        page.read_with(cx, |p, _| {
            assert!(p.recording.is_some(), "K alone is refused")
        });
        cx.simulate_keystrokes("cmd-shift-k");
        page.read_with(cx, |p, _| assert!(p.recording.is_none(), "recorded"));
        assert_eq!(keys(cx).as_deref(), Some(shift_k));
        assert_eq!(
            prefs::get().shortcuts.get(&settings).map(String::as_str),
            Some("shift-cmd-k")
        );
        cx.update(|_, cx| reset(Some(&settings), cx));
        assert_eq!(keys(cx).as_deref(), Some(comma));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn shortcuts_read_as_windows_menus_write_them() {
        let ctrl = gpui::Modifiers {
            control: true,
            ..Default::default()
        };
        assert_eq!(shortcut_text(&ctrl, ","), "Ctrl+,");
        let all = gpui::Modifiers {
            platform: true,
            shift: true,
            alt: true,
            control: true,
            ..Default::default()
        };
        assert_eq!(shortcut_text(&all, "n"), "Ctrl+Alt+Shift+Win+N");
        assert_eq!(shortcut_text(&ctrl, "down"), "Ctrl+↓");
        assert_eq!(shortcut_text(&ctrl, "enter"), "Ctrl+Enter");
        assert_eq!(
            shortcut_caps("Ctrl+Alt+Shift+N"),
            vec!["Ctrl", "Alt", "Shift", "N"]
        );
        assert_eq!(shortcut_caps("Ctrl++"), vec!["Ctrl", "+"]);
        assert_eq!(shortcut_caps("F11"), vec!["F11"]);
        // "secondary" is Ctrl here, so the app's bindings read Ctrl.
        let k = Keystroke::parse("secondary-shift-k").unwrap();
        assert_eq!(shortcut_text(&k.modifiers, &k.key), "Ctrl+Shift+K");
        assert_eq!(keystroke_source(&k), "ctrl-shift-k");
    }

    #[cfg(target_os = "macos")]
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
        let k = Keystroke::parse("cmd-shift-k").unwrap();
        assert_eq!(keystroke_source(&k), "shift-cmd-k");
        let k = Keystroke::parse("ctrl-alt-f5").unwrap();
        assert_eq!(keystroke_source(&k), "ctrl-alt-f5");
    }
}
