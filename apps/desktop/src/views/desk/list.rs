//! The rows of the 首页 list (NATIVE.md §9.4): 需要你拍板, 正在做 and 做完了
//! as Mail-like 56 pt rows. A needs-you row ends with the small push
//! buttons `看看` and its default answer; once answered it shows the
//! record and, while the answer waits to be sent, `撤回`.

use gpui::{
    AnyElement, App, Context, ElementId, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, Window,
    div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    WindowExt as _,
    button::ButtonVariant,
    menu::{ContextMenuExt as _, PopupMenuItem},
};

use super::{
    DeskPage,
    ask::{QuickAnswer, quick_answer},
    done::proof_line,
    model::{AskCard, RunCard, Snapshot},
    working::elapsed,
};
use crate::{
    app::{RightPane, Services},
    copy::{common, desk as c, fill},
    data::v2::DoneItem,
    ui::{
        MemberAvatar,
        native::{PushSize, PushStyle, native_colors, push_button},
        sf_symbols::{Sym, SymSize},
    },
    views::{
        group_page::rows::{self, AVATAR, ROW, RowInk, line, mail_row, state_line},
        shell::clock,
    },
};

fn open_topic(services: &Services, g: &str, t: &str, cx: &mut App) {
    let (g, t) = (g.to_string(), t.to_string());
    services.nav.update(cx, |nav, cx| nav.open_topic(g, t, cx));
}

/// Answers `ask` with `quick`, asking first in an alert when the card says
/// so (§7.2).
fn press(
    page: gpui::Entity<DeskPage>,
    ask: AskCard,
    quick: QuickAnswer,
    window: &mut Window,
    cx: &mut App,
) {
    let record = quick.record();
    let Some((question, body)) = quick.confirm.clone() else {
        page.update(cx, |this, cx| this.answer(ask, quick.answer, record, cx));
        return;
    };
    if window.has_active_dialog(cx) {
        return;
    }
    window.open_alert_dialog(cx, move |dialog, _, cx| {
        let dialog = crate::ui::SheetExt::sheet(dialog, cx);
        let (page, ask, answer, record) = (
            page.clone(),
            ask.clone(),
            quick.answer.clone(),
            record.clone(),
        );
        dialog
            .title(question.clone())
            .when_some(body.clone(), |d, b| d.description(b))
            .show_cancel(true)
            .ok_text(quick.label.clone())
            .ok_variant(ButtonVariant::Primary)
            .cancel_text(common::CANCEL)
            .on_ok(move |_, _, cx| {
                page.update(cx, |this, cx| {
                    this.answer(ask.clone(), answer.clone(), record.clone(), cx)
                });
                true
            })
    });
}

/// Whether an ask has an answer (mine, made here or known to the provider).
pub fn is_answered(a: &AskCard, page: &DeskPage) -> bool {
    page.local_answer(&a.key()).is_some() || a.answered.is_some()
}

pub fn ask_row(
    a: &AskCard,
    last: bool,
    page: &DeskPage,
    snap: &Snapshot,
    cx: &mut Context<DeskPage>,
) -> AnyElement {
    let k = native_colors(cx);
    let key = a.key();
    let services = page.services().clone();
    let asker = snap.member(&a.item.from);
    let (_, topic) = snap.places.get(&key).cloned().unwrap_or_default();
    // The record of my answer, when there is one: (record, at, undoable).
    let done = page
        .local_answer(&key)
        .map(|l| (l.record.clone(), l.at, l.undoable))
        .or_else(|| {
            a.answered
                .as_ref()
                .map(|r| (r.record.clone(), r.at, false))
        });
    let ink = RowInk::new(None, cx);
    let (second, accessory): (AnyElement, Option<AnyElement>) = match &done {
        Some((record, at, undoable)) => {
            let when = if crate::ui::motion::just_answered(*at) {
                c::DESK_JUST_NOW.to_string()
            } else {
                clock(*at)
            };
            let second = state_line(
                Some((Sym::Check, k.secondary, record.clone())),
                Some(when),
                k.secondary,
            );
            let undo = undoable.then(|| {
                let page = cx.entity();
                let key = key.clone();
                push_button(
                    SharedString::from(format!("desk-undo-{key}")),
                    c::DESK_UNDO,
                    PushStyle::Standard,
                    PushSize::Small,
                    cx,
                )
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    page.update(cx, |this, cx| this.undo(&key, cx));
                })
                .into_any_element()
            });
            // The slot stays when 撤回 goes, so the times line up.
            let undo = Some(
                div()
                    .min_w(rows::ACCESSORY_MIN)
                    .flex()
                    .justify_end()
                    .children(undo)
                    .into_any_element(),
            );
            (second, undo)
        }
        None => {
            let from = fill(
                c::DESK_ASK_FROM,
                &[("name", &snap.name(&a.item.from)), ("topic", &topic)],
            );
            let look = {
                let services = services.clone();
                let (g, t) = (a.item.group_id.clone(), a.item.topic_id.clone());
                push_button(
                    SharedString::from(format!("desk-look-{key}")),
                    c::DESK_LOOK,
                    PushStyle::Standard,
                    PushSize::Small,
                    cx,
                )
                .on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    open_topic(&services, &g, &t, cx);
                })
            };
            let approve = quick_answer(a).map(|quick| {
                let page = cx.entity();
                let ask = a.clone();
                push_button(
                    SharedString::from(format!("desk-approve-{key}")),
                    quick.label.clone(),
                    PushStyle::Default,
                    PushSize::Small,
                    cx,
                )
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    press(page.clone(), ask.clone(), quick.clone(), window, cx);
                })
            });
            (
                line(from).into_any_element(),
                Some(
                    div()
                        .min_w(rows::ACCESSORY_MIN)
                        .flex()
                        .justify_end()
                        .items_center()
                        .gap(px(8.))
                        .child(look)
                        .children(approve)
                        .into_any_element(),
                ),
            )
        }
    };
    let (g, t) = (a.item.group_id.clone(), a.item.topic_id.clone());
    let nav = services.clone();
    let menu_services = services.clone();
    let menu_quick = done.is_none().then(|| quick_answer(a)).flatten();
    let menu_page = cx.entity();
    let menu_ask = a.clone();
    mail_row(
        ElementId::Name(format!("desk-ask-{key}").into()),
        ROW,
        MemberAvatar::new(asker.as_ref(), AVATAR).into_any_element(),
        a.item.title.clone(),
        Some(clock(a.item.at)),
        second,
        accessory,
        !last,
        ink,
        cx,
    )
    .on_click(move |_, _, cx| open_topic(&nav, &g, &t, cx))
    .context_menu(move |menu, _, _| {
        let services = menu_services.clone();
        let (g, t) = (menu_ask.item.group_id.clone(), menu_ask.item.topic_id.clone());
        let menu = menu.item(
            PopupMenuItem::new(c::DESK_OPEN_TOPIC)
                .on_click(move |_, _, cx| open_topic(&services, &g, &t, cx)),
        );
        match menu_quick.clone() {
            Some(quick) => {
                let (page, ask) = (menu_page.clone(), menu_ask.clone());
                menu.item(PopupMenuItem::new(quick.label.clone()).on_click(
                    move |_, window, cx| press(page.clone(), ask.clone(), quick.clone(), window, cx),
                ))
            }
            None => menu,
        }
    })
    .into_any_element()
}

pub fn run_row(
    i: usize,
    r: &RunCard,
    last: bool,
    snap: &Snapshot,
    services: &Services,
    cx: &App,
) -> AnyElement {
    let k = native_colors(cx);
    let member = snap.member(&r.run.bot_id);
    let (n, m) = r.step();
    let mut rest = vec![snap.name(&r.run.bot_id)];
    rest.extend(r.current_text());
    rest.extend(r.step_started.and_then(elapsed));
    let second = state_line(None, Some(rest.join(" · ")), k.secondary);
    let progress = (m > 0).then(|| {
        div()
            .min_w(rows::ACCESSORY_MIN)
            .flex()
            .justify_end()
            .items_center()
            .gap(px(6.))
            .child(rows::sym(Sym::Live, SymSize::INLINE, k.blue))
            .child(
                div()
                    .min_w(px(28.))
                    .flex()
                    .justify_end()
                    .text_size(px(11.))
                    .text_color(k.secondary)
                    .child(format!("{n}/{m}")),
            )
            .into_any_element()
    });
    let nav = services.nav.clone();
    let (g, t, id) = (
        r.run.group_id.clone(),
        r.run.topic_id.clone(),
        r.run.id.clone(),
    );
    let open = services.clone();
    let (mg, mt) = (g.clone(), t.clone());
    mail_row(
        ElementId::Name(format!("desk-run-row-{i}").into()),
        ROW,
        MemberAvatar::new(member.as_ref(), AVATAR).into_any_element(),
        r.topic_title.clone(),
        None,
        second,
        progress,
        !last,
        RowInk::new(None, cx),
        cx,
    )
    .on_click(move |_, _, cx| {
        let (g, t, id) = (g.clone(), t.clone(), id.clone());
        nav.update(cx, |nav, cx| {
            nav.open_topic(g, t, cx);
            nav.set_right_pane(RightPane::Live(id), cx);
        })
    })
    .context_menu(move |menu, _, _| {
        let (s, g, t) = (open.clone(), mg.clone(), mt.clone());
        menu.item(
            PopupMenuItem::new(c::DESK_OPEN_TOPIC)
                .on_click(move |_, _, cx| open_topic(&s, &g, &t, cx)),
        )
    })
    .into_any_element()
}

pub fn done_row(
    i: usize,
    d: &DoneItem,
    last: bool,
    snap: &Snapshot,
    services: &Services,
    cx: &App,
) -> AnyElement {
    let k = native_colors(cx);
    let member = snap.member(&d.member_id);
    let mut rest = vec![snap.name(&d.member_id)];
    rest.extend(proof_line(&d.proof, snap));
    let second = state_line(None, Some(rest.join(" · ")), k.secondary);
    let (s1, s2) = (services.clone(), services.clone());
    let (g, t) = (d.group_id.clone(), d.topic_id.clone());
    let (mg, mt) = (g.clone(), t.clone());
    mail_row(
        ElementId::Name(format!("desk-done-{i}").into()),
        ROW,
        MemberAvatar::new(member.as_ref(), AVATAR).into_any_element(),
        d.title.clone(),
        Some(clock(d.at)),
        second,
        // An empty slot: the times line up with the needs-you rows.
        Some(div().min_w(rows::ACCESSORY_MIN).into_any_element()),
        !last,
        RowInk::new(None, cx),
        cx,
    )
    .on_click(move |_, _, cx| open_topic(&s1, &g, &t, cx))
    .context_menu(move |menu, _, _| {
        let (s, g, t) = (s2.clone(), mg.clone(), mt.clone());
        menu.item(
            PopupMenuItem::new(c::DESK_OPEN_TOPIC)
                .on_click(move |_, _, cx| open_topic(&s, &g, &t, cx)),
        )
    })
    .into_any_element()
}
