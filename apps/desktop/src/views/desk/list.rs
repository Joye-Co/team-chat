//! The rows of the 首页 list (NATIVE.md §9.4): 需要你拍板, 正在做 and 做完了
//! as Mail-like 56 pt rows with one trailing slot on the title line (the
//! time, or the live dot and `3/5` of a run). A needs-you row shows the
//! Standard Small buttons `看看` and its answer under the time; there is no
//! Default button in the list (one per view, §5.2: the topic's decision
//! bar has it). Once answered a row shows the record and, while the answer
//! waits to be sent, `撤回`. The first row is Jojo's priority note.

use gpui::{
    AnyElement, App, Context, ElementId, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, Window,
    div, prelude::FluentBuilder as _,
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
        native::{PushSize, PushStyle, native_colors, push_button},
        sf_symbols::Sym,
    },
    views::{
        group_page::rows::{self, AVATAR, ROW, RowInk, line, mail_row, state_line},
        nav::open_dm_with,
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
    let from = fill(
        c::DESK_ASK_FROM,
        &[("name", &snap.name(&a.item.from)), ("topic", &topic)],
    );
    let (time, second, trailing): (String, AnyElement, Option<AnyElement>) = match &done {
        Some((record, at, undoable)) => {
            let when = if crate::ui::motion::just_answered(*at) {
                c::DESK_JUST_NOW.to_string()
            } else {
                clock(*at)
            };
            let second = state_line(
                Some((Sym::Check, k.secondary, record.clone())),
                Some(from.clone()),
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
            (when, second, undo)
        }
        None => {
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
            // Standard, not Default: a list holds many asks, and a view has
            // at most one Default button (§5.2).
            let approve = quick_answer(a).map(|quick| {
                let page = cx.entity();
                let ask = a.clone();
                push_button(
                    SharedString::from(format!("desk-approve-{key}")),
                    quick.label.clone(),
                    PushStyle::Standard,
                    PushSize::Small,
                    cx,
                )
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    press(page.clone(), ask.clone(), quick.clone(), window, cx);
                })
            });
            (
                clock(a.item.at),
                line(from).into_any_element(),
                Some(
                    div()
                        .flex()
                        .items_center()
                        .gap(rows::BUTTON_GAP)
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
        rows::avatar(asker.as_ref(), AVATAR, cx),
        a.item.title.clone(),
        Some(rows::time_slot(time, ink.secondary)),
        second,
        trailing,
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
    // The run's progress takes the title line's one trailing slot.
    let progress = (m > 0).then(|| rows::progress_slot(n, m, k.blue, k.secondary));
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
        rows::avatar(member.as_ref(), AVATAR, cx),
        r.topic_title.clone(),
        progress,
        second,
        None,
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
        rows::avatar(member.as_ref(), AVATAR, cx),
        d.title.clone(),
        Some(rows::time_slot(clock(d.at), k.secondary)),
        second,
        None,
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

/// Jojo's priority as the first row of the list (§9.4): avatar 32, the
/// member's name, the time, and the note as the one `secondary` line.
/// Opening it opens the 私聊 with that member.
pub fn note_row(snap: &Snapshot, cx: &App) -> Option<AnyElement> {
    let n = snap.note.as_ref()?;
    let k = native_colors(cx);
    let member = snap.member(&n.member_id);
    let id = n.member_id.clone();
    let menu_id = n.member_id.clone();
    Some(
        mail_row(
            "desk-note",
            ROW,
            rows::avatar(member.as_ref(), AVATAR, cx),
            snap.name(&n.member_id),
            Some(rows::time_slot(clock(n.at), k.secondary)),
            line(n.text.clone()).into_any_element(),
            None,
            false,
            RowInk::new(None, cx),
            cx,
        )
        .on_click(move |_, _, cx| open_dm_with(id.clone(), cx))
        .context_menu(move |menu, _, _| {
            let id = menu_id.clone();
            menu.item(
                PopupMenuItem::new(c::DESK_OPEN_DM)
                    .on_click(move |_, _, cx| open_dm_with(id.clone(), cx)),
            )
        })
        .into_any_element(),
    )
}
