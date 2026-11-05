//! The topic menu: rename and follow. The topic header opens it as a
//! dropdown, and topic rows as a context menu.

use std::cell::Cell;

use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement, Render,
    SharedString, Styled, Subscription, Window, div, px,
};
use gpui_component::{
    WindowExt as _,
    input::{InputEvent, InputState},
    menu::{PopupMenu, PopupMenuItem},
};

use crate::{
    app::Services,
    copy::{common, overlays as o, realtime as r},
    data::{Member, MemberId, Topic},
    theme,
    ui::native::{NativeTextField, SheetWidth, disabled_push, native_sheet, sheet_buttons},
};

/// Longest topic title (docs/protocol.md).
const MAX_TITLE_CHARS: usize = 80;

/// Checks a new topic title: trimmed, not empty, cut to 80 characters.
pub fn validate_title(title: &str) -> Result<String, &'static str> {
    let t = title.trim();
    if t.is_empty() {
        return Err(r::TOPIC_TITLE_REQUIRED);
    }
    Ok(t.chars().take(MAX_TITLE_CHARS).collect())
}

/// 保存 is on for a valid title that differs from the current one (T-19:
/// the same rule as 重命名群, `nav::rename::can_save`).
pub fn can_save_title(current: &str, typed: &str) -> bool {
    validate_title(typed).is_ok_and(|t| t != current.trim())
}

/// The body of the rename dialog: one field, Enter or 保存 renames.
struct RenameForm {
    services: Services,
    group_id: String,
    topic_id: String,
    /// The title when the dialog opened.
    current: String,
    title: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl RenameForm {
    /// An empty title only turns 保存 off (no error text, NATIVE.md §7.4).
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.title.read(cx).value();
        if !can_save_title(&self.current, &typed) {
            return;
        }
        if let Ok(title) = validate_title(&typed) {
            self.services
                .provider
                .rename_topic(&self.group_id, &self.topic_id, title);
            window.close_dialog(cx);
        }
    }
}

impl Render for RenameForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let can_save = can_save_title(&self.current, &self.title.read(cx).value());
        let [cancel, save] = sheet_buttons("rename-topic", common::CANCEL, common::SAVE, cx);
        crate::views::nav::rename::rename_sheet(
            r::RENAME_TITLE,
            o::RENAME_TOPIC_EXPLANATION,
            o::TOPIC_TITLE,
            NativeTextField::new("rename-topic-title", &self.title).width(
                crate::views::nav::dialogs::form_field_width(SheetWidth::Form),
            ),
            None,
            cancel.on_click(|_, window, cx| window.close_dialog(cx)),
            disabled_push(save, !can_save, cx)
                .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
            cx,
        )
    }
}

/// Opens the dialog that renames a topic, with its title selected.
pub fn open_rename(
    services: Services,
    group_id: &str,
    topic: &Topic,
    window: &mut Window,
    cx: &mut App,
) {
    let current = topic.title.clone();
    let (group_id, topic_id) = (group_id.to_string(), topic.id.clone());
    let form = cx.new(|cx| {
        let title = cx.new(|cx| InputState::new(window, cx).default_value(current.clone()));
        let subscriptions = vec![cx.subscribe_in(
            &title,
            window,
            |this: &mut RenameForm, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            },
        )];
        RenameForm {
            services,
            group_id,
            topic_id,
            current,
            title,
            _subscriptions: subscriptions,
        }
    });
    let first = Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            let title = form.read(cx).title.clone();
            title.read(cx).focus_handle(cx).focus(window, cx);
            title.update(cx, |state, cx| state.select_all(window, cx));
        }
        native_sheet(dialog, SheetWidth::Form, cx)
            .px(px(0.))
            .child(form.clone())
    });
}

/// The active members of a group, people first, then by name.
pub fn group_members(services: &Services, group_id: &str) -> Vec<Member> {
    let p = &services.provider;
    let ids: Vec<MemberId> = p
        .groups()
        .into_iter()
        .find(|g| g.id == group_id)
        .map(|g| g.member_ids)
        .unwrap_or_default();
    let mut members: Vec<Member> = p
        .members()
        .into_iter()
        .filter(|m| !m.deactivated && ids.contains(&m.id))
        .collect();
    members.sort_by(|a, b| (a.is_bot(), &a.name).cmp(&(b.is_bot(), &b.name)));
    members
}

/// Adds the topic items to `menu`: 重命名 and 关注. Status, who the topic
/// waits on and its owner follow the work itself, so nobody sets them by hand.
/// Every action goes to the provider; the new state arrives as `Change::Topics`.
pub fn build(
    mut menu: PopupMenu,
    services: &Services,
    group_id: &str,
    topic: &Topic,
    _window: &mut Window,
    _cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let following = services.provider.topic_following(group_id, &topic.id);

    let (s, g, tp) = (services.clone(), group_id.to_string(), topic.clone());
    menu = menu
        .item(
            PopupMenuItem::new(r::RENAME)
                .on_click(move |_, window, cx| open_rename(s.clone(), &g, &tp, window, cx)),
        )
        .separator();

    let (s, g, t) = (services.clone(), group_id.to_string(), topic.id.clone());
    menu.item(
        PopupMenuItem::new(if following { r::UNFOLLOW } else { r::FOLLOW })
            .on_click(move |_, _, _| s.provider.set_topic_following(&g, &t, !following)),
    )
}

/// "负责人：name" for the topic header, when an owner is set.
pub fn owner_text(topic: &Topic, members: &[Member]) -> Option<String> {
    let id = topic.owner_id.as_ref()?;
    let name = members.iter().find(|m| &m.id == id)?.name.clone();
    Some(crate::copy::fill(r::OWNER_LABEL, &[("name", &name)]))
}

/// A small muted label (owner, following) for the topic header.
pub fn header_label(text: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    let colors = theme::colors(cx);
    div()
        .flex_none()
        .text_xs()
        .text_color(colors.muted_foreground)
        .whitespace_nowrap()
        .child(text.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_needs_a_changed_title() {
        assert!(!can_save_title("V1 性能压测", "V1 性能压测"));
        assert!(!can_save_title("V1 性能压测", " V1 性能压测 "));
        assert!(!can_save_title("V1 性能压测", "  "));
        assert!(can_save_title("V1 性能压测", "V1 性能压测：一万条"));
    }

    #[test]
    fn titles_are_trimmed_and_cut() {
        assert_eq!(
            validate_title("  修复登录 500 "),
            Ok("修复登录 500".to_string())
        );
        assert_eq!(validate_title(" \n "), Err(r::TOPIC_TITLE_REQUIRED));
        assert_eq!(
            validate_title(&"长".repeat(100)).unwrap().chars().count(),
            80
        );
    }
}
