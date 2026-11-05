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
/// `昨天 09:42`, `明天 08:00`, `11-01` further away this year and
/// `2025-11-01` in another year. A missing time (0 or less, or out of
/// range) is `未知`, never `01-01` or 1970 (§2.15, F2).
///
/// This is the one zero-time-safe date formatter of the app: desk, group,
/// machines and cards call it instead of formatting a `Millis` themselves.
pub fn clock(at: crate::data::Millis) -> String {
    clock_at(at, crate::selftest::clock::local_now())
}

/// [`clock`] against a given "now" (tests).
pub fn clock_at(at: crate::data::Millis, now: chrono::DateTime<chrono::Local>) -> String {
    use chrono::{Datelike as _, Local, TimeZone as _};
    let Some(then) = known(at).and_then(|at| Local.timestamp_millis_opt(at).single()) else {
        return crate::copy::common::TIME_UNKNOWN.to_string();
    };
    let today = now.date_naive();
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
    } else if then.year() == now.year() {
        then.format("%m-%d").to_string()
    } else {
        then.format("%Y-%m-%d").to_string()
    }
}

/// The times of one column in one format (A1-24, F2): `HH:MM` when all are
/// today, `MM-DD` when all are this year, else `YYYY-MM-DD`. A missing time
/// reads `未知`.
pub fn column_clocks(ats: &[crate::data::Millis]) -> Vec<String> {
    column_clocks_at(ats, crate::selftest::clock::local_now())
}

/// [`column_clocks`] against a given "now" (tests).
pub fn column_clocks_at(
    ats: &[crate::data::Millis],
    now: chrono::DateTime<chrono::Local>,
) -> Vec<String> {
    use chrono::{Datelike as _, Local, TimeZone as _};
    let local = |at| known(at).and_then(|at| Local.timestamp_millis_opt(at).single());
    let days: Vec<_> = ats.iter().map(|&at| local(at)).collect();
    let today = now.date_naive();
    let known_days = || days.iter().flatten();
    let format = if known_days().all(|d| d.date_naive() == today) {
        "%H:%M"
    } else if known_days().all(|d| d.year() == now.year()) {
        "%m-%d"
    } else {
        "%Y-%m-%d"
    };
    days.iter()
        .map(|d| match d {
            Some(d) => d.format(format).to_string(),
            None => crate::copy::common::TIME_UNKNOWN.to_string(),
        })
        .collect()
}

/// A day as [`clock`] prints it without the time: `11-01`, `2025-11-01`
/// in another year, `未知` for a missing time.
pub fn day(at: crate::data::Millis) -> String {
    use chrono::{Datelike as _, Local, TimeZone as _};
    let Some(then) = known(at).and_then(|at| Local.timestamp_millis_opt(at).single()) else {
        return crate::copy::common::TIME_UNKNOWN.to_string();
    };
    if then.year() == crate::selftest::clock::local_now().year() {
        then.format("%m-%d").to_string()
    } else {
        then.format("%Y-%m-%d").to_string()
    }
}

/// A time range on one line (§2.15): `10:21–14:21` today, `11-01
/// 10:21–14:21` on another day, both ends in full across days. A missing
/// end shows the start only.
pub fn clock_range(start: crate::data::Millis, end: crate::data::Millis) -> String {
    use chrono::{Local, TimeZone as _};
    let local = |at| known(at).and_then(|at| Local.timestamp_millis_opt(at).single());
    let (Some(a), Some(b)) = (local(start), local(end)) else {
        return clock(start);
    };
    if a.date_naive() != b.date_naive() {
        return format!("{}–{}", clock(start), clock(end));
    }
    let times = format!("{}–{}", a.format("%H:%M"), b.format("%H:%M"));
    if a.date_naive() == crate::selftest::clock::local_now().date_naive() {
        times
    } else {
        format!("{} {times}", day(start))
    }
}

/// `Some(at)` when the time is set: 0 and negative times are "missing"
/// (an unset field on the wire), not 1970.
pub fn known(at: crate::data::Millis) -> Option<crate::data::Millis> {
    (at > 0).then_some(at)
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone as _};

    #[test]
    fn a_column_of_times_has_one_format() {
        let now = Local.with_ymd_and_hms(2026, 11, 3, 12, 0, 0).unwrap();
        let ms = |y, m, d| {
            Local
                .with_ymd_and_hms(y, m, d, 10, 21, 0)
                .unwrap()
                .timestamp_millis()
        };
        let today = ms(2026, 11, 3);
        assert_eq!(column_clocks_at(&[today, today], now), ["10:21", "10:21"]);
        assert_eq!(
            column_clocks_at(&[today, ms(2026, 10, 22)], now),
            ["11-03", "10-22"]
        );
        assert_eq!(
            column_clocks_at(&[today, ms(2026, 10, 22), ms(2023, 11, 4)], now),
            ["2026-11-03", "2026-10-22", "2023-11-04"]
        );
    }

    #[test]
    fn clock_never_prints_epoch_and_adds_the_year() {
        let now = Local.with_ymd_and_hms(2026, 11, 3, 12, 0, 0).unwrap();
        let ms = |y, m, d, h| {
            Local
                .with_ymd_and_hms(y, m, d, h, 5, 0)
                .unwrap()
                .timestamp_millis()
        };
        assert_eq!(clock_at(0, now), "未知");
        assert_eq!(clock_at(-5, now), "未知");
        assert_eq!(clock_at(ms(2026, 11, 3, 9), now), "09:05");
        assert_eq!(clock_at(ms(2026, 11, 2, 9), now), "昨天 09:05");
        assert_eq!(clock_at(ms(2026, 11, 4, 9), now), "明天 09:05");
        assert_eq!(clock_at(ms(2026, 10, 30, 9), now), "10-30");
        assert_eq!(clock_at(ms(2025, 10, 30, 9), now), "2025-10-30");
    }
}
