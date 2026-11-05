//! The topic menu: status, who the topic waits on (`waitingOn`), the one
//! responsible member (`ownerId`), and follow. The topic header opens it as a
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
    data::{Member, MemberId, Topic, TopicStatus},
    theme,
    ui::native::{NativeTextField, SheetWidth, disabled_push, native_sheet, sheet_buttons},
    views::presence_dot,
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

/// The body of the rename dialog: one field, Enter or 保存 renames.
struct RenameForm {
    services: Services,
    group_id: String,
    topic_id: String,
    title: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl RenameForm {
    /// An empty title only turns 保存 off (no error text, NATIVE.md §7.4).
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Ok(title) = validate_title(&self.title.read(cx).value()) {
            self.services
                .provider
                .rename_topic(&self.group_id, &self.topic_id, title);
            window.close_dialog(cx);
        }
    }
}

impl Render for RenameForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let filled = !self.title.read(cx).value().trim().is_empty();
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
            disabled_push(save, !filled, cx)
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
        let title = cx.new(|cx| InputState::new(window, cx).default_value(current));
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

/// The menu label of a status (the chip text without a name).
pub fn status_label(status: TopicStatus) -> &'static str {
    match status {
        TopicStatus::Open => common::STATUS_QUEUED,
        TopicStatus::InProgress => common::STATUS_IN_PROGRESS,
        TopicStatus::WaitingAnswer => common::STATUS_WAITING_ANSWER,
        TopicStatus::WaitingReview => common::STATUS_WAITING_REVIEW,
        TopicStatus::Blocked => common::STATUS_BLOCKED,
        TopicStatus::Done => common::STATUS_DONE,
    }
}

/// The waitingOn list to keep when the status changes: a done or open topic
/// waits on nobody.
pub fn waiting_for(status: TopicStatus, current: &[MemberId]) -> Vec<MemberId> {
    match status {
        TopicStatus::Open | TopicStatus::Done => Vec::new(),
        _ => current.to_vec(),
    }
}

/// `current` with `member` added or removed.
pub fn toggle_waiting(current: &[MemberId], member: &str) -> Vec<MemberId> {
    if current.iter().any(|m| m == member) {
        current.iter().filter(|m| *m != member).cloned().collect()
    } else {
        let mut out = current.to_vec();
        out.push(member.to_string());
        out
    }
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

fn member_item(member: &Member, me: Option<&str>) -> PopupMenuItem {
    let online = member.online;
    let mut name = member.name.clone();
    if me == Some(member.id.as_str()) {
        name.push_str(r::ME_SUFFIX);
    }
    let name = SharedString::from(name);
    PopupMenuItem::element(move |_window: &mut Window, cx: &mut App| {
        div()
            .flex()
            .items_center()
            .gap(crate::ui::native::tokens::space::S6)
            .child(presence_dot(online, cx))
            .child(name.clone())
    })
}

/// Adds the topic items to `menu`. Every action goes to the provider; the
/// new state arrives as `Change::Topics`.
pub fn build(
    mut menu: PopupMenu,
    services: &Services,
    group_id: &str,
    topic: &Topic,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let me = services.me();
    let members = group_members(services, group_id);
    let following = services.provider.topic_following(group_id, &topic.id);

    let (s, g, tp) = (services.clone(), group_id.to_string(), topic.clone());
    menu = menu
        .item(
            PopupMenuItem::new(r::RENAME)
                .on_click(move |_, window, cx| open_rename(s.clone(), &g, &tp, window, cx)),
        )
        .separator();

    for status in TopicStatus::ALL {
        let (s, g, t) = (services.clone(), group_id.to_string(), topic.id.clone());
        let waiting = waiting_for(status, &topic.waiting_on);
        menu = menu.item(
            PopupMenuItem::new(status_label(status))
                .checked(topic.status == status)
                .on_click(move |_, _, _| {
                    s.provider.set_topic_status(&g, &t, status, waiting.clone());
                }),
        );
    }
    menu = menu.separator();

    // Who the topic waits on: each click adds or removes one member.
    let (s, g, t) = (services.clone(), group_id.to_string(), topic.id.clone());
    let current = topic.waiting_on.clone();
    let status = topic.status;
    let list = members.clone();
    let me2 = me.clone();
    menu = menu.submenu(r::WAITING_ON, window, cx, move |mut sub, _, _| {
        for m in &list {
            let (s, g, t) = (s.clone(), g.clone(), t.clone());
            let next = toggle_waiting(&current, &m.id);
            // Waiting on someone means the topic is not open or done.
            let next_status = match status {
                TopicStatus::Open | TopicStatus::Done if !next.is_empty() => {
                    TopicStatus::WaitingAnswer
                }
                other => other,
            };
            sub = sub.item(
                member_item(m, me2.as_deref())
                    .checked(current.contains(&m.id))
                    .on_click(move |_, _, _| {
                        s.provider
                            .set_topic_status(&g, &t, next_status, next.clone());
                    }),
            );
        }
        let (s, g, t) = (s.clone(), g.clone(), t.clone());
        sub.separator().item(
            PopupMenuItem::new(r::WAITING_ON_CLEAR)
                .disabled(current.is_empty())
                .on_click(move |_, _, _| s.provider.set_topic_status(&g, &t, status, Vec::new())),
        )
    });

    // The one responsible member.
    let (s, g, t) = (services.clone(), group_id.to_string(), topic.id.clone());
    let owner = topic.owner_id.clone();
    let list = members;
    let me2 = me.clone();
    menu = menu.submenu(r::OWNER, window, cx, move |mut sub, _, _| {
        for m in &list {
            let (s, g, t, id) = (s.clone(), g.clone(), t.clone(), m.id.clone());
            sub = sub.item(
                member_item(m, me2.as_deref())
                    .checked(owner.as_deref() == Some(m.id.as_str()))
                    .on_click(move |_, _, _| s.provider.set_topic_owner(&g, &t, Some(id.clone()))),
            );
        }
        let (s, g, t) = (s.clone(), g.clone(), t.clone());
        sub.separator().item(
            PopupMenuItem::new(r::OWNER_NONE)
                .disabled(owner.is_none())
                .on_click(move |_, _, _| s.provider.set_topic_owner(&g, &t, None)),
        )
    });

    menu = menu.separator();
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

    #[test]
    fn waiting_toggles_and_clears_for_done() {
        let cur = vec!["a".to_string()];
        assert_eq!(toggle_waiting(&cur, "b"), vec!["a", "b"]);
        assert!(toggle_waiting(&cur, "a").is_empty());
        assert!(waiting_for(TopicStatus::Done, &cur).is_empty());
        assert_eq!(waiting_for(TopicStatus::WaitingReview, &cur), cur);
    }
}
