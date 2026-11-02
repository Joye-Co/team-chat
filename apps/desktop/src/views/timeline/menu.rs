//! Extension point: the context menu of a message row, and in-place edit.
//!
//! Items: 引用回复 (emits `TimelineEvent::Quote`), 编辑 (my sent messages),
//! 删除 (mine, or any for an owner; asks first), 复制 (the Markdown source),
//! 复制链接 (a link with the product URL scheme), and 重试 for a failed send.
//! To add an item, add it in [`build`]; anything that changes data calls the
//! provider.

use gpui::{
    App, AppContext as _, ClipboardItem, Context, WeakEntity, Window, prelude::FluentBuilder as _,
};
use gpui_component::{
    WindowExt as _,
    button::ButtonVariant,
    input::{InputEvent, TextareaState},
    menu::{PopupMenu, PopupMenuItem},
};

use super::{Editing, TimelineView, model};
use crate::{
    copy::{common, timeline as t},
    data::{Message, SendState},
};

/// The link of a message: `<scheme>://group/<g>/topic/<t>/message/<m>`.
pub fn message_link(message: &Message) -> String {
    format!(
        "{}://group/{}/topic/{}/message/{}",
        env!("PRODUCT_URL_SCHEME"),
        message.group_id,
        message.topic_id,
        message.id
    )
}

pub(super) fn build(
    menu: PopupMenu,
    view: WeakEntity<TimelineView>,
    message: &Message,
    can_edit: bool,
    can_delete: bool,
    _window: &mut Window,
    _cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let sent = !message.id.is_empty() && message.deleted_at.is_none();
    let failed = message.send_state == SendState::Failed;
    let m = message.clone();
    let quote = {
        let (view, m) = (view.clone(), m.clone());
        PopupMenuItem::new(t::MENU_QUOTE).on_click(move |_, _, cx| {
            let _ = view.update(cx, |this, cx| this.quote(&m, cx));
        })
    };
    let edit = {
        let (view, m) = (view.clone(), m.clone());
        PopupMenuItem::new(t::EDIT).on_click(move |_, window, cx| {
            let _ = view.update(cx, |this, cx| this.start_edit(&m, window, cx));
        })
    };
    let delete = {
        let (view, m) = (view.clone(), m.clone());
        PopupMenuItem::new(t::DELETE)
            .on_click(move |_, window, cx| confirm_delete(view.clone(), m.clone(), window, cx))
    };
    let copy_text = {
        let body = m.body.clone();
        PopupMenuItem::new(t::MENU_COPY).on_click(move |_, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(body.clone()))
        })
    };
    let copy_link = {
        let link = message_link(&m);
        PopupMenuItem::new(t::MENU_COPY_LINK).on_click(move |_, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(link.clone()))
        })
    };
    let retry = {
        let (view, m) = (view.clone(), m.clone());
        PopupMenuItem::new(t::RETRY).on_click(move |_, _, cx| {
            let _ = view.update(cx, |this, _| this.retry(&m));
        })
    };
    menu.when(sent, |menu| menu.item(quote))
        .when(failed, |menu| menu.item(retry))
        .when(m.deleted_at.is_none(), |menu| menu.item(copy_text))
        .when(sent, |menu| menu.item(copy_link))
        .when(can_edit || can_delete, |menu| menu.separator())
        .when(can_edit, |menu| menu.item(edit))
        .when(can_delete, |menu| menu.item(delete))
}

fn confirm_delete(
    view: WeakEntity<TimelineView>,
    message: Message,
    window: &mut Window,
    cx: &mut App,
) {
    window.open_alert_dialog(cx, move |dialog, _, cx| {
        let dialog = crate::ui::SheetExt::sheet(dialog, cx);
        let (view, message) = (view.clone(), message.clone());
        dialog
            .title(t::DELETE_CONFIRM)
            .description(t::DELETE_CONFIRM_DETAIL)
            .show_cancel(true)
            .ok_text(t::DELETE)
            .ok_variant(ButtonVariant::Danger)
            .cancel_text(common::CANCEL)
            .on_ok(move |_, _, cx| {
                let _ = view.update(cx, |this, _| {
                    this.services
                        .provider
                        .delete(&message.group_id, &message.id);
                });
                true
            })
    });
}

impl TimelineView {
    pub(super) fn start_edit(
        &mut self,
        message: &Message,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_edit(message) {
            return;
        }
        let body = message.body.clone();
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 12)
                .submit_on_enter(true)
                .default_value(body)
        });
        let subscription = cx.subscribe_in(&input, window, |this, _, event: &InputEvent, _, cx| {
            if let InputEvent::PressEnter { shift: false, .. } = event {
                this.save_edit(cx);
            }
        });
        input.update(cx, |state, cx| state.focus(window, cx));
        let key = model::key(message).to_string();
        if let Some(old) = self.editing.take() {
            self.remeasure_key(&old.key);
        }
        self.remeasure_key(&key);
        self.editing = Some(Editing {
            key,
            message_id: message.id.clone(),
            input,
            _subscription: subscription,
        });
        cx.notify();
    }

    pub(super) fn save_edit(&mut self, cx: &mut Context<Self>) {
        let Some(editing) = self.editing.take() else {
            return;
        };
        let body = editing.input.read(cx).value().trim().to_string();
        let old = self
            .messages
            .iter()
            .find(|m| model::key(m) == editing.key)
            .map(|m| m.body.clone());
        if let (Some((g, _)), Some(old)) = (&self.open, old)
            && !body.is_empty()
            && body != old.trim()
            && body.chars().count() <= crate::views::composer::MAX_BODY_CHARS
        {
            self.services.provider.edit(g, &editing.message_id, body);
        }
        self.remeasure_key(&editing.key);
        cx.notify();
    }

    pub(super) fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(editing) = self.editing.take() {
            self.remeasure_key(&editing.key);
            cx.notify();
        }
    }
}
