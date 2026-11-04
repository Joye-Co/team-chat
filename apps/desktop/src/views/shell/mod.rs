//! The v2 shell of the main window (1.1.0): left rail | page | right pane.
//!
//! The shell owns the layout and the wiring; each page and pane is an entity
//! in its feature's own folder, behind the small traits below, so the
//! feature tracks fill their stub without touching the shell:
//!
//! | Part | Entity | Folder |
//! | --- | --- | --- |
//! | rail | `RailView` | `views/rail` |
//! | 首页 | `DeskPage` | `views/desk` |
//! | 话题 | `TopicPage` (timeline + composer) | `views/topic_page` |
//! | 群 | `GroupPage` | `views/group_page` |
//! | 成员 | `MembersPage` | `views/members_page` |
//! | 产出 | `ArtifactPage` (list and one 产出) | `views/artifact_page` |
//! | topic rail, 改动 | `TopicRail`, `ChangesPane` | `views/topic_rail` |
//! | 单独问 | `SideChat` | `views/side_chat` |
//! | 实时查看 | `LivePane` | `views/live` |
//! | 产出 pane | `ArtifactPane` | `views/artifact_page` |
//!
//! Navigation state (`Place`, `Page`, `RightPane`, `RailMode`) lives in
//! `app::Navigation`; the title bar of every page is drawn here from the
//! page's [`TitleBarSpec`].

// --- v12/shell ---
pub mod chrome;
// --- end v12/shell ---
pub mod layout;
pub mod pages;
pub mod panes;
pub mod title_bar;

use gpui::{App, Div, ParentElement, Pixels, Render, Styled, div};

pub use title_bar::{TitleBarSpec, TitleButton, TitleButtonKind, TitleStyle};

use crate::{
    theme::{self, Type as _, space},
    ui::{self, Sym},
};

/// A page of the main window: it renders its body and says what its title
/// bar shows. The shell draws the title bar above the body.
pub trait PageView: Render {
    fn title_bar(&self, cx: &App) -> TitleBarSpec;
    /// pages track: true when the page draws its own 52 pt title bars (the
    /// 群, 成员 and 产出 pages have a side column with its own header that
    /// starts at the top of the window). The shell then draws none.
    fn draws_own_header(&self, _cx: &App) -> bool {
        false
    }
}

/// A pane at the right of a page. The shell gives each pane kind one
/// width (`panes::pane_width`: the inspector 280 pt, the wide panes
/// 400 pt) and starts the inspector panes below the 52 pt toolbar row; a
/// pane with a header draws it in a 52 pt row at its top.
pub trait PaneView: Render {
    /// The width the pane would like; the shell uses `panes::pane_width`.
    fn width(&self, window_w: Pixels) -> Pixels;
}

/// The body of a page or pane that a feature track has not built yet: an
/// icon, the page's name and one quiet line, centered. Feature tracks replace
/// it; it never ships in a finished screen.
pub fn stub_body(icon: Sym, title: &str, line: Option<String>, cx: &App) -> Div {
    let k = theme::colors(cx);
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(space::S8)
        .child(ui::icon(icon, theme::icon::EMPTY, k.ink_3))
        .child(
            div()
                .t_ui()
                .medium()
                .text_color(k.ink_2)
                .child(title.to_string()),
        )
        .children(line.map(|l| div().t_small().text_color(k.ink_3).child(l)))
}

/// A section label of a stub list (12/500 `ink-3`), with an optional count.
pub fn section_label(label: &str, count: Option<usize>, cx: &App) -> Div {
    let k = theme::colors(cx);
    div()
        .flex()
        .items_center()
        .gap(space::S6)
        .h(gpui::px(28.))
        .t_meta_medium()
        .text_color(k.ink_3)
        .child(label.to_string())
        .children(count.map(|n| div().t_mono().child(n.to_string())))
}

/// A wall-clock time as the v2 screens print it in mono: `09:42` today,
/// `昨天 09:42`, `明天 08:00`, or `11-01` further away.
pub fn clock(at: crate::data::Millis) -> String {
    use chrono::{Local, TimeZone as _};
    let Some(then) = Local.timestamp_millis_opt(at).single() else {
        return String::new();
    };
    let today = crate::selftest::clock::local_now().date_naive();
    if then.date_naive() == today {
        then.format("%H:%M").to_string()
    } else if then.date_naive().succ_opt() == Some(today) {
        format!(
            "{} {}",
            crate::copy::common::YESTERDAY,
            then.format("%H:%M")
        )
    } else if today.succ_opt() == Some(then.date_naive()) {
        format!("{} {}", crate::copy::common::TOMORROW, then.format("%H:%M"))
    } else {
        then.format("%m-%d").to_string()
    }
}

/// A member's name, or an empty string.
pub fn member_name(services: &crate::app::Services, id: &str) -> String {
    services
        .provider
        .member(id)
        .map(|m| m.name)
        .unwrap_or_default()
}

/// A member's identity in a card header (CONCEPT2 §6): tile or circle 20,
/// name 13/500 and the 岗位 tag, with an optional ring.
pub fn identity(
    services: &crate::app::Services,
    member_id: &str,
    ring: Option<ui::Ring>,
    cx: &App,
) -> Div {
    let k = theme::colors(cx);
    let member = services.provider.member(member_id);
    div()
        .flex()
        .items_center()
        .gap(space::S8)
        .child(ui::MemberAvatar::new(member.as_ref(), gpui::px(20.)).ring(ring))
        .child(
            div()
                .t_small()
                .medium()
                .text_color(k.ink)
                .child(member.as_ref().map(|m| m.name.clone()).unwrap_or_default()),
        )
        .children(member.and_then(|m| m.role).map(ui::JobTag::new))
}

/// `群 / 话题` in `ink-3`, for card headers.
pub fn topic_path(services: &crate::app::Services, group_id: &str, topic_id: &str) -> String {
    let p = &services.provider;
    let group = p
        .groups()
        .into_iter()
        .find(|g| g.id == group_id)
        .map(|g| g.name)
        .unwrap_or_default();
    let topic = p
        .topic(group_id, topic_id)
        .map(|t| t.title)
        .unwrap_or_default();
    format!("{group} / {topic}")
}
