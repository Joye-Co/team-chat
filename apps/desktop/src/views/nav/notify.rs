//! How one 群, 私聊 or 话题 reminds me. These are settings of one
//! conversation, so they live where the conversation is: the 通知 section
//! of its page's right pane (群资料, the 话题 inspector), its sidebar row's
//! context menu and the 群 page's ⋯ menu. Settings → 通知 holds only the
//! global ones.

use gpui::{App, Context, IntoElement, ParentElement, Styled, Window, div};
use gpui_component::{
    Disableable as _,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
};

use crate::{
    app::Services,
    copy::{nav, settings},
    data::NotifyLevel,
    ui::native::{
        InspectorSection, NativeSwitch, NativeType as _, PushSize, native_colors, select_button,
        tokens::space,
    },
    views::settings::widgets::{answer, error_text},
};

/// The levels the 提醒 submenu offers; 不提醒 is the 静音 item beside it.
const LEVELS: [NotifyLevel; 2] = [NotifyLevel::All, NotifyLevel::Mentions];

fn level_text(level: NotifyLevel) -> &'static str {
    match level {
        NotifyLevel::All => nav::NOTIFY_ALL,
        NotifyLevel::Mentions => nav::NOTIFY_MENTIONS,
        NotifyLevel::None => nav::MUTE,
    }
}

/// The level the submenu checks: none while the 群 is muted or set to 不提醒
/// (the 静音 item says that state).
pub fn checked_level(muted: bool, level: NotifyLevel) -> Option<NotifyLevel> {
    (!muted && level != NotifyLevel::None).then_some(level)
}

/// Picks a level: it also unmutes, since a muted 群 reminds of nothing.
/// A refused save says so in an alert; the menu reads the cache again
/// next time it opens.
pub fn pick_level(services: &Services, group_id: &str, level: NotifyLevel, cx: &mut App) {
    let reply = services
        .provider
        .set_group_prefs(group_id, Some(false), Some(level));
    cx.spawn(async move |cx| {
        if let Err(e) = answer(reply).await {
            tracing::warn!(?e, "group notify level refused");
            let text = format!("{}：{}", settings::NOTIFY_SAVE_FAILED, error_text(e));
            let _ = cx.update(|cx| crate::views::files::notify_error(text, cx));
        }
    })
    .detach();
}

/// Adds 静音 / 取消静音 and the 提醒 submenu for `group_id`.
pub fn items(
    menu: PopupMenu,
    services: &Services,
    group_id: &str,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let prefs = services.provider.group_prefs(group_id);
    let muted = services.provider.group_muted(group_id);
    let checked = checked_level(muted, prefs.notify);
    let (s, id) = (services.clone(), group_id.to_string());
    let menu = menu.item(
        PopupMenuItem::new(if muted { nav::UNMUTE } else { nav::MUTE })
            .on_click(move |_, _, _| s.provider.set_group_muted(&id, !muted)),
    );
    let (s, id) = (services.clone(), group_id.to_string());
    menu.submenu(nav::NOTIFY_LEVEL, window, cx, move |sub, _, _| {
        LEVELS.into_iter().fold(sub, |sub, level| {
            let (s, id) = (s.clone(), id.clone());
            sub.item(
                PopupMenuItem::new(level_text(level))
                    .checked(checked == Some(level))
                    .on_click(move |_, _, cx| pick_level(&s, &id, level, cx)),
            )
        })
    })
}

/// The width of the 提醒 pop-up in the 通知 section.
const LEVEL_POPUP: gpui::Pixels = gpui::px(112.);

/// One label + control row of a 通知 section.
fn row(label: &'static str, control: impl IntoElement, cx: &App) -> impl IntoElement {
    let k = native_colors(cx);
    div()
        .h(space::S32)
        .flex()
        .items_center()
        .justify_between()
        .gap(space::S8)
        .child(div().n_body().text_color(k.label).child(label))
        .child(control)
}

/// The muted footnote under a muted conversation's rows.
fn footnote(text: &'static str, cx: &App) -> impl IntoElement {
    let k = native_colors(cx);
    div()
        .pt(space::S4)
        .n_subheadline()
        .text_color(k.secondary)
        .child(text)
}

/// The 通知 section of a 群 or 私聊 (群资料): 静音, and 提醒 (所有消息 /
/// 只有 @我), off while muted.
pub fn group_section(services: &Services, group_id: &str, cx: &App) -> InspectorSection {
    let prefs = services.provider.group_prefs(group_id);
    let muted = services.provider.group_muted(group_id);
    let shown = if prefs.notify == NotifyLevel::None {
        NotifyLevel::Mentions
    } else {
        prefs.notify
    };
    let (s, id) = (services.clone(), group_id.to_string());
    let mute = NativeSwitch::new("group-notify-mute", muted)
        .name(nav::MUTE)
        .on_change(move |on, _, _| s.provider.set_group_muted(&id, on));
    let (s, id) = (services.clone(), group_id.to_string());
    let level = select_button("group-notify-level", level_text(shown), PushSize::Small, cx)
        .w(LEVEL_POPUP)
        .disabled(muted)
        .dropdown_menu(move |menu, _, _| {
            LEVELS.into_iter().fold(menu, |menu, level| {
                let (s, id) = (s.clone(), id.clone());
                menu.item(
                    PopupMenuItem::new(level_text(level))
                        .checked(level == shown)
                        .on_click(move |_, _, cx| pick_level(&s, &id, level, cx)),
                )
            })
        });
    let section = InspectorSection::new("group-ins-notify", nav::NOTIFY_SECTION)
        .row(row(nav::MUTE, mute, cx))
        .row(row(nav::NOTIFY_LEVEL, level, cx));
    if muted {
        section.row(footnote(nav::GROUP_MUTED_HINT, cx))
    } else {
        section
    }
}

/// The 通知 section of a 话题 (its inspector): 关注 and 静音.
pub fn topic_section(
    services: &Services,
    group_id: &str,
    topic_id: &str,
    cx: &App,
) -> InspectorSection {
    let p = &services.provider;
    let following = p.topic_following(group_id, topic_id);
    let muted = p.topic_muted(group_id, topic_id);
    let (s, g, t) = (services.clone(), group_id.to_string(), topic_id.to_string());
    let follow = NativeSwitch::new("topic-notify-follow", following)
        .name(nav::FOLLOW_ROW)
        .on_change(move |on, _, _| s.provider.set_topic_following(&g, &t, on));
    let (s, g, t) = (services.clone(), group_id.to_string(), topic_id.to_string());
    let mute = NativeSwitch::new("topic-notify-mute", muted)
        .name(nav::MUTE)
        .on_change(move |on, _, _| s.provider.set_topic_muted(&g, &t, on));
    let section = InspectorSection::new("topic-ins-notify", nav::NOTIFY_SECTION)
        .row(row(nav::FOLLOW_ROW, follow, cx))
        .row(row(nav::MUTE, mute, cx));
    section.row(footnote(
        if muted {
            nav::TOPIC_MUTED_HINT
        } else {
            nav::FOLLOW_HINT
        },
        cx,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_muted_or_silent_group_checks_no_level() {
        assert_eq!(
            checked_level(false, NotifyLevel::All),
            Some(NotifyLevel::All)
        );
        assert_eq!(
            checked_level(false, NotifyLevel::Mentions),
            Some(NotifyLevel::Mentions)
        );
        assert_eq!(checked_level(true, NotifyLevel::Mentions), None);
        assert_eq!(checked_level(false, NotifyLevel::None), None);
    }
}
