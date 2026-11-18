//! The 新话题 sheet (NATIVE.md §7.1, ⌘N): the 群 (a pop-up) and the first
//! message, then 取消 and 开始. Nobody types a title: the server names the
//! 话题 with AI after it starts (server/src/autotitle). An `@成员` in the
//! first message hands the 话题 to that member; ⌘Return or 开始 starts the
//! 话题 and shows it.

use std::cell::Cell;

use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, ParentElement,
    Render, SharedString, Styled, Subscription, Window, div, px,
};
use gpui_component::{
    WindowExt as _,
    input::{InputEvent, TextareaState},
    menu::{DropdownMenu as _, PopupMenuItem},
};

use super::dialogs::{
    form_field_width, sheet_form, sheet_form_row, sheet_form_row_top, sheet_form_row_wrap,
};
use crate::ui::native::tokens::space;
use crate::{
    app::Services,
    copy::{common, overlays as o},
    data::{GroupId, StartTopicRequest},
    ui::native::{
        NativeTextArea, PushSize, PushStyle, SheetLayout, SheetWidth, disabled_push,
        native_sheet_keyboard, popup_button, push_button, sheet_buttons,
    },
    views::rail::model as rail_model,
};

gpui::actions!(new_topic, [StartTopic]);

const CONTEXT: &str = "NewTopicSheet";

/// The sheet's keys (call once at start-up): ⌘Return starts the 话题 from
/// the multi-line message field.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([gpui::KeyBinding::new(
        crate::secondary!("enter"),
        StartTopic,
        Some(CONTEXT),
    )]);
}

/// The members of `group_id` that `body` mentions (`@名字`), resolved as the
/// composer resolves them: the longest name after each @ (or ＠) wins, and a
/// deactivated member is never mentioned.
pub fn mentioned(services: &Services, group_id: &str, body: &str) -> Vec<String> {
    mentioned_among(
        &services.provider.groups(),
        &services.provider.members(),
        group_id,
        body,
    )
}

fn mentioned_among(
    groups: &[crate::data::GroupSummary],
    members: &[crate::data::Member],
    group_id: &str,
    body: &str,
) -> Vec<String> {
    let ids = groups
        .iter()
        .find(|g| g.id == group_id)
        .map(|g| g.member_ids.as_slice())
        .unwrap_or_default();
    crate::views::composer::model::resolve_mentions(body, &[], members, ids)
}

/// The ids of the 话题 of `group_id` now.
pub fn topic_ids(provider: &dyn crate::data::ChatProvider, group_id: &str) -> Vec<String> {
    provider
        .topics(group_id)
        .into_iter()
        .map(|t| t.id)
        .collect()
}

/// The 话题 a `start_topic` just made in `group_id`: the newest one that was
/// not there `before` (another 话题 may have had newer activity). `None`
/// when the start was refused.
pub fn started_topic(
    provider: &dyn crate::data::ChatProvider,
    group_id: &str,
    before: &[String],
) -> Option<String> {
    provider
        .topics(group_id)
        .into_iter()
        .map(|t| t.id)
        .find(|id| !before.contains(id))
}

/// 开个话题 (1.1.1): starts a 话题 from `message` of a 群聊 at once and opens
/// it with the caret in its composer. Nothing to fill in: the first message
/// only quotes the 群聊 message (the server titles the 话题 from it, then
/// with AI), and whatever there is to say is said in the 话题 itself.
pub fn start_from(
    services: &Services,
    message: &crate::data::Message,
    window: &mut Window,
    cx: &mut App,
) {
    if open_started_from(services, message, cx) {
        window.dispatch_action(Box::new(crate::app::FocusComposer), cx);
    }
}

/// [`start_from`] without the focus: starts the 话题 and opens it; `false`
/// when the start was refused.
pub fn open_started_from(
    services: &Services,
    message: &crate::data::Message,
    cx: &mut App,
) -> bool {
    let provider = services.provider.clone();
    let group_id = message.group_id.clone();
    let before = topic_ids(provider.as_ref(), &group_id);
    provider.start_topic(StartTopicRequest {
        group_id: group_id.clone(),
        title: None,
        body: String::new(),
        mentions: Vec::new(),
        quote_of: Some(message.id.clone()),
    });
    let Some(topic) = started_topic(provider.as_ref(), &group_id, &before) else {
        return false;
    };
    services
        .nav
        .update(cx, |nav, cx| nav.open_topic(group_id, topic, cx));
    true
}

/// Leads the first message with `mention` (once) and puts the caret at its end.
fn lead_with(body: &Entity<TextareaState>, mention: &str, window: &mut Window, cx: &mut App) {
    body.update(cx, |b, cx| {
        let text = b.value().to_string();
        let next = if text.contains(mention) {
            text
        } else {
            format!("{mention} {}", text.trim_start())
        };
        let end = next.len();
        b.set_value(next, window, cx);
        b.set_selected_range(end..end, cx);
        b.focus(window, cx);
    });
}

/// At most this many members show as buttons in the 交给 row.
const HAND_TO_MAX: usize = 5;

/// Who the 交给 row offers in `group_id` (onboarding: a new member does not know the
/// names to @): the members of the 群 but me, bots first (they take work), each once.
/// The first [`HAND_TO_MAX`] are buttons, the rest are in the row's 其他成员 menu.
pub fn hand_to(services: &Services, group_id: &str) -> Vec<(String, String)> {
    let me = services.me();
    let ids = services
        .provider
        .groups()
        .into_iter()
        .find(|g| g.id == group_id)
        .map(|g| g.member_ids)
        .unwrap_or_default();
    let mut members: Vec<_> = services
        .provider
        .members()
        .into_iter()
        .filter(|m| ids.contains(&m.id) && Some(&m.id) != me.as_ref() && !m.deactivated)
        .collect();
    members.sort_by_key(|m| !m.is_bot());
    members.into_iter().map(|m| (m.id, m.name)).collect()
}

pub struct NewTopicSheet {
    services: Services,
    groups: Vec<(GroupId, String)>,
    group: Option<GroupId>,
    body: Entity<TextareaState>,
    _subscriptions: Vec<Subscription>,
}

impl NewTopicSheet {
    fn new(
        services: Services,
        group: Option<GroupId>,
        body_placeholder: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let body = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(4, 4)
                .placeholder(body_placeholder)
        });
        let groups = rail_model::groups(services.provider.as_ref());
        let listed = |g: &GroupId| groups.iter().any(|(id, _)| id == g);
        // What was typed here and not sent (Esc, 取消, a refused start) is
        // back, with the 群 it was written for.
        let draft = super::new_topic_draft(cx).filter(|(g, _)| listed(g));
        let group = match &draft {
            Some((g, _)) => Some(g.clone()),
            None => group
                .filter(listed)
                .or_else(|| groups.first().map(|(id, _)| id.clone())),
        };
        if let Some((_, text)) = draft {
            body.update(cx, |b, cx| {
                let end = text.len();
                b.set_value(text, window, cx);
                b.set_selected_range(end..end, cx);
            });
        }
        let subscriptions = vec![cx.subscribe(&body, |this, _, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                this.keep_draft(cx);
                cx.notify();
            }
        })];
        Self {
            services,
            groups,
            group,
            body,
            _subscriptions: subscriptions,
        }
    }

    /// Keeps the unsent message for the next time the sheet opens (while
    /// the app runs); an empty one is forgotten.
    fn keep_draft(&self, cx: &mut Context<Self>) {
        let text = self.body.read(cx).value().to_string();
        let draft = self
            .group
            .clone()
            .filter(|_| !text.trim().is_empty())
            .map(|g| (g, text));
        super::set_new_topic_draft(draft, cx);
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(group_id) = self.group.clone() else {
            return;
        };
        let body = self.body.read(cx).value().trim().to_string();
        // An empty message only turns 开始 off (NATIVE.md §7.4).
        if body.is_empty() {
            return;
        }
        let mentions = mentioned(&self.services, &group_id, &body);
        let provider = self.services.provider.clone();
        let before = topic_ids(provider.as_ref(), &group_id);
        provider.start_topic(StartTopicRequest {
            group_id: group_id.clone(),
            title: None,
            body,
            mentions,
            quote_of: None,
        });
        window.close_dialog(cx);
        // A refused start keeps the message for the next ⌘N.
        if let Some(topic) = started_topic(provider.as_ref(), &group_id, &before) {
            super::set_new_topic_draft(None, cx);
            self.services
                .nav
                .update(cx, |nav, cx| nav.open_topic(group_id, topic, cx));
        }
    }
}

impl Render for NewTopicSheet {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ready = self.group.is_some() && !self.body.read(cx).value().trim().is_empty();
        let group_name: SharedString = self
            .groups
            .iter()
            .find(|(id, _)| Some(id) == self.group.as_ref())
            .map(|(_, n)| n.clone())
            .unwrap_or_default()
            .into();
        let groups = self.groups.clone();
        let current = self.group.clone();
        let this = cx.entity().downgrade();
        let group_tip = group_name.clone();
        let group_popup =
            popup_button("new-topic-group", group_name, PushSize::Regular, window, cx)
                .w(form_field_width(SheetWidth::Form))
                .tooltip(group_tip)
                .dropdown_menu(move |menu, _, _| {
                    groups.iter().fold(menu, |menu, (id, name)| {
                        let this = this.clone();
                        let id2 = id.clone();
                        menu.item(
                            PopupMenuItem::new(name.clone())
                                .checked(current.as_ref() == Some(id))
                                .on_click(move |_, _, cx| {
                                    let id = id2.clone();
                                    let _ = this.update(cx, |this, cx| {
                                        this.group = Some(id);
                                        this.keep_draft(cx);
                                        cx.notify();
                                    });
                                }),
                        )
                    })
                });
        // The §2.8 textarea: the 标题 field's edge, fill and ring over four
        // lines (review 3 K-02).
        let body_field = NativeTextArea::new("new-topic-body", &self.body).rows(4);
        let hand_row = self.hand_row(cx);
        let [cancel, start] = sheet_buttons("new-topic", common::CANCEL, o::NEW_TOPIC_CREATE, cx);
        let (sheet_title, explanation) = (o::NEW_TOPIC_TITLE, o::NEW_TOPIC_EXPLANATION);
        div()
            .key_context(CONTEXT)
            .on_action(cx.listener(|this, _: &StartTopic, window, cx| this.submit(window, cx)))
            .w_full()
            .child(
                SheetLayout::new(sheet_title)
                    .explanation(explanation)
                    .body(
                        sheet_form()
                            .child(sheet_form_row(o::NEW_TOPIC_GROUP, group_popup, cx))
                            .child(sheet_form_row_top(o::NEW_TOPIC_BODY, body_field, cx))
                            .children(hand_row),
                    )
                    .buttons([
                        cancel.on_click(|_, window, cx| window.close_dialog(cx)),
                        disabled_push(start, !ready, cx)
                            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                    ]),
            )
            .into_any_element()
    }
}

impl NewTopicSheet {
    /// 交给: one small button per member of the 群; a click leads the
    /// first message with @名字 (once). `None` when nobody can take it.
    fn hand_row(&self, cx: &mut Context<Self>) -> Option<gpui::Div> {
        let mut hand_to = self
            .group
            .as_deref()
            .map(|g| hand_to(&self.services, g))
            .unwrap_or_default();
        let rest = hand_to.split_off(hand_to.len().min(HAND_TO_MAX));
        (!hand_to.is_empty()).then(|| {
            let more =
                (!rest.is_empty()).then(|| {
                    let body = self.body.clone();
                    push_button(
                        "new-topic-hand-more",
                        o::NEW_TOPIC_HAND_TO_MORE,
                        PushStyle::Ghost,
                        PushSize::Small,
                        cx,
                    )
                    .dropdown_menu(move |menu, _, _| {
                        rest.iter().fold(menu, |menu, (_, name)| {
                            let body = body.clone();
                            let mention = format!("@{name}");
                            menu.item(PopupMenuItem::new(name.clone()).on_click(
                                move |_, window, cx| lead_with(&body, &mention, window, cx),
                            ))
                        })
                    })
                });
            let buttons = hand_to.into_iter().map(|(id, name)| {
                let body = self.body.clone();
                let mention = format!("@{name}");
                push_button(
                    SharedString::from(format!("new-topic-hand-{id}")),
                    mention.clone(),
                    PushStyle::Outline,
                    PushSize::Small,
                    cx,
                )
                .on_click(move |_, window, cx| lead_with(&body, &mention, window, cx))
            });
            sheet_form_row_wrap(
                o::NEW_TOPIC_HAND_TO,
                PushSize::Small.height(),
                div()
                    .flex()
                    .flex_wrap()
                    .gap(space::S8)
                    .children(buttons)
                    .children(more),
                cx,
            )
        })
    }
}

/// Opens the 新话题 sheet, the 群 preset to `group` (the open one), focus in
/// the first message.
pub fn open_new_topic(
    services: Services,
    group: Option<GroupId>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<NewTopicSheet> {
    let sheet =
        cx.new(|cx| NewTopicSheet::new(services, group, o::NEW_TOPIC_BODY_PLACEHOLDER, window, cx));
    present(sheet, window, cx)
}

fn present(
    sheet: Entity<NewTopicSheet>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<NewTopicSheet> {
    let first = Cell::new(true);
    let view = sheet.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        if first.replace(false) {
            let body = view.read(cx).body.clone();
            body.update(cx, |b, cx| b.focus(window, cx));
        }
        // ⌘N and the rail's 新话题 button send one action, many times a
        // day: the sheet opens at once, with no slide (review-animations 9).
        native_sheet_keyboard(dialog, SheetWidth::Form, cx)
            .px(px(0.))
            .child(view.clone())
    });
    sheet
}

#[cfg(test)]
mod mention_tests {
    use super::*;
    use crate::data::{GroupKind, GroupSummary, Member, MemberKind, MemberLevel};

    fn member(id: &str, name: &str) -> Member {
        Member {
            id: id.into(),
            kind: MemberKind::Bot,
            name: name.into(),
            role: None,
            level: MemberLevel::Member,
            avatar_url: None,
            deactivated: false,
            online: true,
            character: None,
            team_owner: false,
            deleted: false,
        }
    }

    fn group(members: &[&str]) -> GroupSummary {
        GroupSummary {
            id: "g".into(),
            kind: GroupKind::Group,
            name: "研发".into(),
            member_ids: members.iter().map(|m| m.to_string()).collect(),
            dm_peer: None,
            last_seq: 0,
            read_seq: 0,
            unread: 0,
            mentions: 0,
            last_activity_at: 0,
        }
    }

    /// A member whose name starts another's (Jo / Jojo, 小明 / 小明明) is not
    /// mentioned by the longer name, a deactivated member never is, and the
    /// full-width ＠ counts.
    #[test]
    fn the_longest_name_after_the_at_is_the_one_mentioned() {
        let mut gone = member("m-gone", "小明明明");
        gone.deactivated = true;
        let members = vec![
            member("m-jo", "Jo"),
            member("m-jojo", "Jojo"),
            member("m-ming", "小明"),
            member("m-mingming", "小明明"),
            gone,
            member("m-out", "Out"),
        ];
        let groups = vec![group(&["m-jo", "m-jojo", "m-ming", "m-mingming", "m-gone"])];
        let of = |body: &str| mentioned_among(&groups, &members, "g", body);
        assert_eq!(of("@Jojo 看一下发布"), vec!["m-jojo"]);
        assert_eq!(of("@Jo 看一下发布"), vec!["m-jo"]);
        assert_eq!(
            of("＠小明明 写周报，@小明 审"),
            vec!["m-mingming", "m-ming"]
        );
        // The deactivated 小明明明 is skipped: the longest live name wins.
        assert_eq!(of("@小明明明"), vec!["m-mingming"]);
        // Not in the 群, or an unknown 群: plain text.
        assert!(of("@Out 你好").is_empty());
        assert!(mentioned_among(&groups, &members, "g-other", "@Jo").is_empty());
    }
}

#[cfg(test)]
mod draft_tests {
    use std::sync::Arc;

    use gpui::{TestAppContext, VisualTestContext};

    use super::*;
    use crate::data::ChatProvider;
    use crate::fake::v2::{DEV, V2Samples};

    struct Host;

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    /// ⌘N: the sheet over the window, after the one before it closed (Esc).
    fn reopen(services: &Services, cx: &mut VisualTestContext) -> Entity<NewTopicSheet> {
        let services = services.clone();
        let sheet = cx.update(|window, cx| {
            window.close_all_dialogs(cx);
            open_new_topic(services, None, window, cx)
        });
        cx.run_until_parked();
        sheet
    }

    fn type_in(sheet: &Entity<NewTopicSheet>, text: &str, cx: &mut VisualTestContext) {
        sheet.update_in(cx, |s, window, cx| {
            s.body.update(cx, |b, cx| b.set_value(text, window, cx));
            s.keep_draft(cx);
        });
    }

    /// What was typed and not sent (Esc, 取消) is in the sheet the next time
    /// it opens, with its 群; starting the 话题 forgets it.
    #[gpui::test]
    fn the_unsent_message_is_back_when_the_sheet_reopens(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let provider: Arc<dyn ChatProvider> = V2Samples::new();
        let services = cx.update(|cx| {
            let services = Services::new(provider.clone(), cx);
            super::super::install_for_test(services.clone(), None, cx);
            services
        });
        let (_, cx) = cx.add_window_view(|window, cx| {
            let host = cx.new(|_| Host);
            gpui_component::Root::new(host, window, cx)
        });
        let last_group = rail_model::groups(provider.as_ref())
            .pop()
            .map(|(id, _)| id)
            .expect("a 群");

        let first = reopen(&services, cx);
        first.update(cx, |s, _| s.group = Some(last_group.clone()));
        type_in(&first, "写了\n好几行", cx);

        let again = reopen(&services, cx);
        let (group, text) = again.update(cx, |s, cx| {
            (s.group.clone(), s.body.read(cx).value().to_string())
        });
        assert_eq!(group, Some(last_group));
        assert_eq!(text, "写了\n好几行");

        // An emptied field forgets it.
        type_in(&again, "", cx);
        assert_eq!(cx.update(|_, cx| super::super::new_topic_draft(cx)), None);

        // A started 话题 forgets it too.
        again.update(cx, |s, _| s.group = Some(DEV.into()));
        type_in(&again, "发出去的", cx);
        assert!(
            cx.update(|_, cx| super::super::new_topic_draft(cx))
                .is_some()
        );
        again.update_in(cx, |s, window, cx| s.submit(window, cx));
        assert_eq!(cx.update(|_, cx| super::super::new_topic_draft(cx)), None);
        let fresh = reopen(&services, cx);
        assert_eq!(
            fresh.update(cx, |s, cx| s.body.read(cx).value().to_string()),
            ""
        );
    }
}

#[cfg(test)]
mod start_from_tests {
    use std::sync::Arc;

    use gpui::TestAppContext;

    use super::*;
    use crate::data::{ChatProvider, SendRequest, main_topic_id};
    use crate::fake::v2::{DEV, V2Samples};

    /// 开个话题 on the `--fake` sample team: one click starts a 话题 whose first
    /// message only quotes the 群聊 message, titled from it, and opens it.
    #[gpui::test]
    fn a_click_starts_the_topic_from_the_message_and_opens_it(cx: &mut TestAppContext) {
        let provider: Arc<dyn ChatProvider> = V2Samples::new();
        let chat = main_topic_id(DEV);
        provider.send(SendRequest {
            group_id: DEV.into(),
            topic_id: chat.clone(),
            body: "下午的会我晚 10 分钟到\n第二行".into(),
            mentions: Vec::new(),
            quote_of: None,
        });
        let quoted = provider.messages(DEV, &chat, 10).messages.pop().unwrap();
        let services = cx.update(|cx| Services::new(provider.clone(), cx));
        cx.update(|cx| {
            services
                .nav
                .update(cx, |nav, cx| nav.open_group_chat(DEV.into(), cx));
            assert!(open_started_from(&services, &quoted, cx));
        });
        let (group, topic) = cx
            .update(|cx| services.nav.read(cx).topic.clone())
            .expect("a 话题 is open");
        assert_eq!(group, DEV);
        assert_ne!(topic, chat, "the new 话题, not the 群聊");
        let t = provider.topic(DEV, &topic).expect("the 话题 exists");
        assert_eq!(t.title, "下午的会我晚 10 分钟到");
        let first = &provider.messages(DEV, &topic, 10).messages[0];
        assert_eq!(first.quote_of.as_deref(), Some(quoted.id.as_str()));
        assert!(first.body.is_empty());
        // The first message shows the quote (it lives in the 群聊, not here).
        let shown = provider.quoted_message(DEV, &first.id, &quoted.id);
        assert_eq!(shown.map(|q| q.body), Some(quoted.body.clone()));
    }
}
