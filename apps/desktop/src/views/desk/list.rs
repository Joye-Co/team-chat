//! The rows of the 首页 lists (COMPLETENESS.md §2.6, §2.18): 需要你拍板,
//! 正在做 and 做完了 as list rows in one frame each, with fixed right columns
//! (StatusBadge, time, actions) so every row of a list lines up. A
//! needs-you row shows the small buttons `看看` and its one answer; a
//! button that asks first ends with `…`. Once answered a row shows the
//! record and, while the answer waits to be sent, `撤回`. Jojo's priority
//! note is an Alert above the KPI row.

use gpui::{
    AnyElement, App, Context, ElementId, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div, prelude::FluentBuilder as _,
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
        native::{
            PushSize, PushStyle, native_colors,
            page::{self, AlertKind, Columns, PageRow, Status, m},
            push_button,
        },
        sf_symbols::Sym,
    },
    views::{
        group_page::rows::{self, AVATAR, line, state_line},
        nav::open_dm_with,
        shell::clock,
    },
};

/// The columns of 需要你拍板: StatusBadge, time, and room for `看看` and
/// one answer such as `发布到生产…`.
pub const ASK_COLUMNS: Columns = Columns {
    status: Some(gpui::px(80.)),
    time: Some(m::TIME_COLUMN),
    actions: Some(gpui::px(200.)),
};
/// The columns of 正在做: StatusBadge (`在做 3/5`) and the start time.
pub const RUN_COLUMNS: Columns = Columns {
    status: Some(gpui::px(96.)),
    time: Some(m::TIME_COLUMN),
    actions: None,
};
/// The columns of 做完了: the time.
pub const DONE_COLUMNS: Columns = Columns {
    status: None,
    time: Some(m::TIME_COLUMN),
    actions: None,
};

/// The label of a quick answer: `发布到生产…` when it asks first (§5.2).
pub fn quick_label(quick: &QuickAnswer) -> String {
    if quick.confirm.is_some() {
        fill(c::DESK_ACTION_MORE, &[("label", &quick.label)])
    } else {
        quick.label.to_string()
    }
}

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
        .or_else(|| a.answered.as_ref().map(|r| (r.record.clone(), r.at, false)));
    let from = fill(
        c::DESK_ASK_FROM,
        &[("name", &snap.name(&a.item.from)), ("topic", &topic)],
    );
    let (time, second, status, actions): (String, AnyElement, AnyElement, Option<AnyElement>) =
        match &done {
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
                        PushStyle::Outline,
                        PushSize::Small,
                        cx,
                    )
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        page.update(cx, |this, cx| this.undo(&key, cx));
                    })
                    .into_any_element()
                });
                (
                    when,
                    second,
                    page::status_badge(Status::Done, c::DESK_STATUS_ANSWERED, cx),
                    undo,
                )
            }
            None => {
                let look = {
                    let services = services.clone();
                    let (g, t) = (a.item.group_id.clone(), a.item.topic_id.clone());
                    push_button(
                        SharedString::from(format!("desk-look-{key}")),
                        c::DESK_LOOK,
                        PushStyle::Outline,
                        PushSize::Small,
                        cx,
                    )
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        open_topic(&services, &g, &t, cx);
                    })
                };
                // Standard, not Default: a list holds many asks, and a view
                // has at most one Default button (§5.2).
                let approve = quick_answer(a).map(|quick| {
                    let page = cx.entity();
                    let ask = a.clone();
                    push_button(
                        SharedString::from(format!("desk-approve-{key}")),
                        quick_label(&quick),
                        PushStyle::Outline,
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
                    page::status_badge(Status::NeedsYou, c::DESK_STATUS_WAITING, cx),
                    Some(
                        div()
                            .flex()
                            .items_center()
                            .gap(gpui::px(8.))
                            // `看看` keeps the trailing edge, so it lines up
                            // in every row; the answer sits to its left.
                            .children(approve)
                            .child(look)
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
    PageRow {
        media: Some(rows::avatar(asker.as_ref(), AVATAR, cx)),
        second: Some(second),
        status: Some(status),
        time: Some(time.into()),
        actions,
        separator: !last,
        ..PageRow::new(a.item.title.clone())
    }
    .render(
        ElementId::Name(format!("desk-ask-{key}").into()),
        ASK_COLUMNS,
        cx,
    )
    .on_click(move |_, _, cx| open_topic(&nav, &g, &t, cx))
    .context_menu(move |menu, _, _| {
        let services = menu_services.clone();
        let (g, t) = (
            menu_ask.item.group_id.clone(),
            menu_ask.item.topic_id.clone(),
        );
        let menu = menu.item(
            PopupMenuItem::new(c::DESK_OPEN_TOPIC)
                .on_click(move |_, _, cx| open_topic(&services, &g, &t, cx)),
        );
        match menu_quick.clone() {
            Some(quick) => {
                let (page, ask) = (menu_page.clone(), menu_ask.clone());
                menu.item(
                    PopupMenuItem::new(quick_label(&quick)).on_click(move |_, window, cx| {
                        press(page.clone(), ask.clone(), quick.clone(), window, cx)
                    }),
                )
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
    let member = snap.member(&r.run.bot_id);
    let (n, m) = r.step();
    let mut rest = vec![snap.name(&r.run.bot_id)];
    rest.extend(r.current_text());
    rest.extend(r.step_started.and_then(elapsed));
    let status = if m > 0 {
        fill(
            c::DESK_STATUS_WORKING,
            &[("n", &n.to_string()), ("m", &m.to_string())],
        )
    } else {
        c::DESK_STATUS_STARTED.to_string()
    };
    let started = r.run.started_at.unwrap_or(r.run.created_at);
    let nav = services.nav.clone();
    let (g, t, id) = (
        r.run.group_id.clone(),
        r.run.topic_id.clone(),
        r.run.id.clone(),
    );
    let open = services.clone();
    let (mg, mt) = (g.clone(), t.clone());
    PageRow {
        media: Some(rows::avatar(member.as_ref(), AVATAR, cx)),
        second: Some(line(rest.join(" · ")).into_any_element()),
        status: Some(page::status_badge(Status::Working, status, cx)),
        time: Some(clock(started).into()),
        separator: !last,
        ..PageRow::new(r.topic_title.clone())
    }
    .render(
        ElementId::Name(format!("desk-run-row-{i}").into()),
        RUN_COLUMNS,
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
    let member = snap.member(&d.member_id);
    let mut rest = vec![snap.name(&d.member_id)];
    rest.extend(proof_line(&d.proof, snap));
    let (s1, s2) = (services.clone(), services.clone());
    let (g, t) = (d.group_id.clone(), d.topic_id.clone());
    let (mg, mt) = (g.clone(), t.clone());
    PageRow {
        media: Some(rows::avatar(member.as_ref(), AVATAR, cx)),
        second: Some(line(rest.join(" · ")).into_any_element()),
        time: Some(clock(d.at).into()),
        separator: !last,
        ..PageRow::new(d.title.clone())
    }
    .render(
        ElementId::Name(format!("desk-done-{i}").into()),
        DONE_COLUMNS,
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

/// Jojo's priority as an Alert above the KPI row (§2.18): `Jojo 的提醒`,
/// the note as its description, and `回复`, which opens the 私聊.
pub fn note_alert(snap: &Snapshot, cx: &App) -> Option<AnyElement> {
    let n = snap.note.as_ref()?;
    let id = n.member_id.clone();
    let open = n.member_id.clone();
    let title = fill(c::DESK_NOTE_TITLE, &[("name", &snap.name(&n.member_id))]);
    let reply = push_button(
        "desk-note-reply",
        c::DESK_NOTE_REPLY,
        PushStyle::Outline,
        PushSize::Small,
        cx,
    )
    .on_click(move |_, _, cx| open_dm_with(id.clone(), cx));
    Some(
        page::page_alert(
            "desk-note",
            AlertKind::Info,
            format!("{title} · {}", clock(n.at)),
            Some(n.text.clone().into()),
            Some(reply.into_any_element()),
            cx,
        )
        .context_menu(move |menu, _, _| {
            let id = open.clone();
            menu.item(
                PopupMenuItem::new(c::DESK_OPEN_DM)
                    .on_click(move |_, _, cx| open_dm_with(id.clone(), cx)),
            )
        })
        .into_any_element(),
    )
}
