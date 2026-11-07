//! The update in the main window: a quiet card at the bottom of the sidebar
//! ([`update_card`]) that follows the update from 有新版本 to 正在安装, and
//! one alert (2.10) only for 重启安装, once a downloaded update is ready
//! (default 重启安装, then 稍后). It also opens 已更新到 once after an
//! update. It renders [`Updater`] state and calls its commands; nothing here
//! touches the network.

use std::collections::HashSet;

use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, ParentElement,
    Render, SharedString, Styled, Subscription, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::{Sizable as _, WindowExt as _};

use super::{Failure, Phase, Updater, version};
use crate::copy::{fill, overlays as o, update as u};
use crate::ui::native::{
    AlertLayout, NativeType as _, PushSize, PushStyle, alert_button, metrics, native_colors,
    overlays::{app_icon, native_alert},
    spinner,
};

/// The line of text for a phase, or `None` when the phase has no banner text.
pub fn banner_text(phase: &Phase) -> Option<String> {
    let product = env!("PRODUCT_NAME");
    Some(match phase {
        Phase::Available(i) => fill(u::AVAILABLE, &[("version", &i.version)]),
        Phase::Downloading { info, done, total } => {
            let total = if *total == 0 { info.size } else { *total };
            let pct = version::percent(*done, total).to_string();
            fill(
                u::DOWNLOADING,
                &[("version", &info.version), ("percent", &pct)],
            )
        }
        Phase::Ready { info, .. } => fill(o::UPDATE_READY, &[("version", &info.version)]),
        Phase::WaitingForSends(_) => u::WAITING_SENDS.to_string(),
        Phase::Installing(_) => u::INSTALLING.to_string(),
        Phase::Manual(_) => fill(u::MANUAL, &[("product", product)]),
        Phase::Failed { failure, .. } => match failure {
            Failure::Check => u::CHECK_FAILED,
            Failure::Download => u::FAILED_DOWNLOAD,
            Failure::Checksum => u::FAILED_CHECKSUM,
            Failure::Install => u::FAILED_INSTALL,
        }
        .to_string(),
        Phase::Idle | Phase::Checking | Phase::UpToDate => return None,
    })
}

/// Which sheet a phase shows, keyed by version: only a ready update asks
/// (重启安装); a sheet the user closed does not come back for it.
fn sheet_key(phase: &Phase) -> Option<String> {
    match phase {
        Phase::Ready { info, .. } => Some(format!("ready-{}", info.version)),
        _ => None,
    }
}

/// The share of a download, 0–1.
pub fn download_share(phase: &Phase) -> Option<f32> {
    match phase {
        Phase::Downloading { info, done, total } => {
            let total = if *total == 0 { info.size } else { *total };
            Some(version::percent(*done, total) as f32 / 100.)
        }
        _ => None,
    }
}

/// The update sheet (NATIVE.md §7.1): a ready update asks with 稍后 and the
/// default 更新; after 更新 the same sheet shows the download (a 4 pt bar),
/// then the install. Manual and failed updates say what to do.
pub struct UpdateSheet {
    updater: Entity<Updater>,
    /// 查看更新内容 was chosen: the notes show in the alert.
    notes_open: bool,
    _subscriptions: Vec<Subscription>,
}

impl UpdateSheet {
    fn new(updater: Entity<Updater>, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.observe(&updater, |_, _, cx| cx.notify())];
        Self {
            updater,
            notes_open: false,
            _subscriptions: subscriptions,
        }
    }

    /// Under the ready alert's body (F49): the 12 muted 查看更新内容 link,
    /// or, once chosen, the notes themselves in a scrolling well.
    fn notes(
        &self,
        version: &str,
        notes: &str,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        use gpui::{InteractiveElement as _, StatefulInteractiveElement as _};
        let parsed = super::notes::Notes::parse(version, notes);
        if parsed.is_empty() {
            return None;
        }
        let notes = parsed.plain();
        let k = native_colors(cx);
        if self.notes_open {
            return Some(
                div()
                    .id("update-ready-notes")
                    .w_full()
                    .max_h(metrics::ALERT_WIDTH / 2.)
                    .overflow_y_scroll()
                    .p(crate::ui::native::tokens::space::S8)
                    .rounded(metrics::GROUP_RADIUS)
                    .border_1()
                    .border_color(k.separator)
                    .bg(k.group)
                    .n_footnote()
                    .text_color(k.label)
                    .child(notes)
                    .into_any_element(),
            );
        }
        Some(
            div()
                .w_full()
                .flex()
                .justify_center()
                .child(
                    crate::ui::native::borderless_text("update-ready-notes-link", u::NOTES, cx)
                        .text_color(k.secondary)
                        .n_footnote()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.notes_open = true;
                            cx.notify();
                        })),
                )
                .into_any_element(),
        )
    }
}

impl Render for UpdateSheet {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        let phase = self.updater.read(cx).phase().clone();
        let Some(info) = phase.info().cloned() else {
            return div().into_any_element();
        };
        let v = info.version.as_str();
        let product = env!("PRODUCT_NAME");
        let updater = self.updater.clone();
        // 2.10: the default on top, outline 取消 / 稍后 under it, all h32.
        let button = |id: &'static str, label: &'static str, style: PushStyle, cx: &App| {
            alert_button(id, label, style, false, cx)
        };
        let hide = |cx: &App| {
            button("update-hide", o::UPDATE_HIDE, PushStyle::Outline, cx)
                .on_click(|_, window, cx| window.close_dialog(cx))
        };
        let u1 = updater.clone();
        let u2 = updater.clone();
        let alert = |title: String| AlertLayout::new(title).icon(app_icon(metrics::ALERT_ICON));
        let notes = self.notes(&info.version, &info.notes, cx);
        let layout = match &phase {
            Phase::Ready { .. } => alert(fill(
                u::READY_TITLE,
                &[("product", product), ("version", v)],
            ))
            .message(fill(
                u::READY_BODY,
                &[("current", version::CURRENT), ("version", v)],
            ))
            .when_some(notes, |l, n| l.checkbox(n))
            .buttons([
                button("update-restart", o::UPDATE_RESTART, PushStyle::Default, cx)
                    .on_click(move |_, _, cx| u2.update(cx, |up, cx| up.install_now(cx))),
                button("update-later", u::LATER, PushStyle::Outline, cx).on_click(
                    move |_, window, cx| {
                        u1.update(cx, |up, cx| up.postpone(cx));
                        window.close_dialog(cx);
                    },
                ),
            ]),
            Phase::WaitingForSends(_) | Phase::Installing(_) => {
                let waiting = matches!(phase, Phase::WaitingForSends(_));
                alert(fill(o::UPDATE_INSTALLING_TITLE, &[("version", v)]))
                    .message(if waiting {
                        u::WAITING_SENDS
                    } else {
                        u::INSTALLING
                    })
                    .checkbox(
                        div()
                            .w_full()
                            .flex()
                            .justify_center()
                            .child(spinner().with_size(metrics::SPINNER).color(k.secondary)),
                    )
                    .buttons([hide(cx)])
            }
            Phase::Manual(_) => alert(fill(o::UPDATE_MANUAL_TITLE, &[("version", v)]))
                .message(
                    [
                        u::MANUAL_STEPS.to_string(),
                        fill(u::MANUAL_STEP_1, &[("product", product)]),
                        fill(u::MANUAL_STEP_2, &[("product", product)]),
                        fill(u::MANUAL_STEP_3, &[("product", product)]),
                    ]
                    .join("\n"),
                )
                .buttons([
                    button("update-ok", u::DISMISS, PushStyle::Default, cx).on_click(
                        move |_, window, cx| {
                            u1.update(cx, |up, cx| up.dismiss(cx));
                            window.close_dialog(cx);
                        },
                    ),
                ]),
            Phase::Failed { .. } => alert(fill(o::UPDATE_FAILED_TITLE, &[("version", v)]))
                .message(banner_text(&phase).unwrap_or_default())
                .buttons([
                    button("update-retry", u::RETRY, PushStyle::Default, cx)
                        .on_click(move |_, _, cx| u2.update(cx, |up, cx| up.start_update(cx))),
                    button("update-later", u::LATER, PushStyle::Outline, cx).on_click(
                        move |_, window, cx| {
                            u1.update(cx, |up, cx| up.postpone(cx));
                            window.close_dialog(cx);
                        },
                    ),
                ]),
            _ => return div().into_any_element(),
        };
        div().w_full().child(layout).into_any_element()
    }
}

/// The update in the main window: the 重启安装 sheet once a downloaded
/// update is ready; nothing while it downloads (Settings › 通用 and the app
/// menu show that).
pub struct UpdateBanner {
    updater: Option<Entity<Updater>>,
    sheet_open: bool,
    /// The sheets already shown (see [`sheet_key`]).
    shown: HashSet<String>,
    /// 已更新到 was looked at for this start.
    whats_new_checked: bool,
    _subscriptions: Vec<Subscription>,
}

impl UpdateBanner {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let updater = super::global(cx);
        let subscriptions = updater
            .iter()
            .map(|u| cx.observe(u, |_, _, cx| cx.notify()))
            .collect();
        Self {
            updater,
            sheet_open: false,
            shown: HashSet::new(),
            whats_new_checked: false,
            _subscriptions: subscriptions,
        }
    }

    fn open_sheet(
        &mut self,
        updater: Entity<Updater>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sheet_open = true;
        let sheet = cx.new(|cx| UpdateSheet::new(updater.clone(), cx));
        let this = cx.entity().downgrade();
        window.defer(cx, move |window, cx| {
            window.open_dialog(cx, move |dialog, _, cx| {
                let this = this.clone();
                let updater = updater.clone();
                native_alert(dialog, cx)
                    .on_close(move |_, _, cx| {
                        let key = sheet_key(updater.read(cx).phase());
                        let _ = this.update(cx, |b, cx| {
                            b.sheet_open = false;
                            b.shown.extend(key);
                            cx.notify();
                        });
                    })
                    .child(sheet.clone())
            });
        });
    }
}

impl Render for UpdateBanner {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(updater) = self.updater.clone() else {
            return div().into_any_element();
        };
        // 已更新到: once, the first time a new version opens.
        if !self.whats_new_checked && !window.has_active_dialog(cx) {
            self.whats_new_checked = true;
            let notes = if super::preview::whats_new_demo() {
                super::notes::bundled(version::CURRENT)
            } else {
                super::notes::take_whats_new(super::just_updated())
            };
            if let Some(notes) = notes {
                window.defer(cx, move |window, cx| {
                    super::notes::open_whats_new(notes, super::notes::Place::Main, window, cx)
                });
                return div().into_any_element();
            }
        }
        let (phase, visible) = {
            let up = updater.read(cx);
            (up.phase().clone(), up.banner_visible())
        };
        if !visible {
            return div().into_any_element();
        }
        if let Some(key) = sheet_key(&phase)
            && !self.sheet_open
            && !self.shown.contains(&key)
            && !window.has_active_dialog(cx)
        {
            self.shown.insert(key);
            self.open_sheet(updater.clone(), window, cx);
        }
        update_card(&phase, updater, window, cx).unwrap_or_else(|| div().into_any_element())
    }
}

/// The update card at the bottom of the sidebar, above the account footer:
/// one card whose content follows the update (有新版本, 正在下载, 已经下载好了,
/// 等消息发完, 正在安装, 没能更新, 需要手动安装). Only its 16 icon carries a
/// state color; the words stay in the foreground (2.15). `None` when there is
/// nothing to say.
pub fn update_card(
    phase: &Phase,
    updater: Entity<Updater>,
    window: &mut Window,
    cx: &mut App,
) -> Option<gpui::AnyElement> {
    use crate::ui::{
        Sym,
        icons::icon as sym_icon,
        native::{
            IconSize, card_surface, icon_button, progress,
            tokens::{icon, radius, space},
        },
    };
    let k = native_colors(cx);
    let info = phase.info()?.clone();
    let v = info.version.as_str();
    let product = env!("PRODUCT_NAME");
    let headline = super::notes::Notes::parse(v, &info.notes).headline;
    let has_notes = !info.notes.trim().is_empty();
    let button = |id: &'static str, label: SharedString, style: PushStyle, cx: &App| {
        crate::ui::native::push_button(id, label, style, PushSize::Small, cx).w_full()
    };
    let notes_button = |cx: &App| {
        let info = info.clone();
        button(
            "update-card-notes",
            u::CARD_NOTES.into(),
            PushStyle::Outline,
            cx,
        )
        .on_click(move |_, window, cx| {
            super::notes::open_notes(info.clone(), super::notes::Place::Main, window, cx)
        })
    };
    let up = updater.clone();
    let mut close = None;
    let mut bar = None;
    let mut buttons: Vec<gpui_component::button::Button> = Vec::new();
    let (sym, tint, title, detail): (Sym, gpui::Hsla, String, String) = match phase {
        Phase::Available(_) => {
            buttons.push(
                button(
                    "update-card-update",
                    u::UPDATE.into(),
                    PushStyle::Default,
                    cx,
                )
                .on_click(move |_, _, cx| up.update(cx, |u, cx| u.start_update(cx))),
            );
            if has_notes {
                buttons.push(notes_button(cx));
            }
            let up = updater.clone();
            close = Some(
                icon_button(
                    "update-card-hide",
                    Sym::Close,
                    u::CARD_HIDE,
                    IconSize::Small,
                    window,
                    cx,
                )
                .on_click(move |_, _, cx| up.update(cx, |u, cx| u.postpone(cx))),
            );
            (
                Sym::Download,
                k.blue,
                fill(u::CARD_AVAILABLE, &[("version", v)]),
                if headline.is_empty() {
                    u::CARD_AVAILABLE_DETAIL.to_string()
                } else {
                    headline
                },
            )
        }
        Phase::Downloading { done, total, .. } => {
            let total = if *total == 0 { info.size } else { *total };
            let share = version::percent(*done, total);
            bar = Some(progress(share as f32 / 100., cx));
            buttons.push(
                button(
                    "update-card-cancel",
                    u::CANCEL_DOWNLOAD.into(),
                    PushStyle::Outline,
                    cx,
                )
                .on_click(move |_, _, cx| up.update(cx, |u, cx| u.cancel_download(cx))),
            );
            (
                Sym::Download,
                k.blue,
                fill(u::BANNER_DOWNLOADING, &[("version", v)]),
                fill(
                    u::DOWNLOAD_PROGRESS,
                    &[
                        ("percent", &share.to_string()),
                        ("done", &crate::views::settings::general::megabytes(*done)),
                        ("total", &crate::views::settings::general::megabytes(total)),
                    ],
                ),
            )
        }
        Phase::Ready { .. } => {
            buttons.push(
                button(
                    "update-card-restart",
                    o::UPDATE_RESTART.into(),
                    PushStyle::Default,
                    cx,
                )
                .on_click(move |_, _, cx| up.update(cx, |u, cx| u.install_now(cx))),
            );
            if has_notes {
                buttons.push(notes_button(cx));
            }
            (
                Sym::Done,
                k.green,
                fill(u::CARD_READY, &[("version", v)]),
                u::CARD_READY_DETAIL.to_string(),
            )
        }
        Phase::WaitingForSends(_) => (
            Sym::Waiting,
            k.blue,
            u::CARD_WAITING.to_string(),
            u::WAITING_SENDS.to_string(),
        ),
        Phase::Installing(_) => (
            Sym::Waiting,
            k.blue,
            fill(o::UPDATE_INSTALLING_TITLE, &[("version", v)]),
            u::INSTALLING.to_string(),
        ),
        Phase::Manual(_) => {
            buttons.push(
                button(
                    "update-card-dismiss",
                    u::DISMISS.into(),
                    PushStyle::Outline,
                    cx,
                )
                .on_click(move |_, _, cx| up.update(cx, |u, cx| u.dismiss(cx))),
            );
            (
                Sym::Warning,
                k.amber,
                fill(o::UPDATE_MANUAL_TITLE, &[("version", v)]),
                fill(u::CARD_MANUAL_DETAIL, &[("product", product)]),
            )
        }
        Phase::Failed { .. } => {
            buttons.push(
                button("update-card-retry", u::RETRY.into(), PushStyle::Default, cx)
                    .on_click(move |_, _, cx| up.update(cx, |u, cx| u.start_update(cx))),
            );
            let up = updater.clone();
            buttons.push(
                button("update-card-later", u::LATER.into(), PushStyle::Outline, cx)
                    .on_click(move |_, _, cx| up.update(cx, |u, cx| u.postpone(cx))),
            );
            (
                Sym::Alert,
                k.red,
                fill(o::UPDATE_FAILED_TITLE, &[("version", v)]),
                banner_text(phase).unwrap_or_default(),
            )
        }
        _ => return None,
    };
    let busy = matches!(phase, Phase::WaitingForSends(_) | Phase::Installing(_));
    let mark = div()
        .flex_none()
        .size(icon::LG)
        .rounded(radius::ITEM)
        .bg(k.fill)
        .flex()
        .items_center()
        .justify_center()
        .map(|d| {
            if busy {
                d.child(spinner().with_size(icon::MD).color(tint))
            } else {
                d.child(sym_icon(sym, icon::MD, tint))
            }
        });
    Some(
        div()
            .id("update-card")
            .flex_none()
            .mx(metrics::SIDEBAR_ROW_INSET)
            .pb(space::S8)
            .child(
                card_surface(cx)
                    .rounded(radius::CONTAINER)
                    .p(space::S12)
                    .flex()
                    .flex_col()
                    .gap(space::S8)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space::S8)
                            .child(mark)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(gpui::Pixels::ZERO)
                                    .n_headline()
                                    .text_color(k.label)
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(title),
                            )
                            .children(close),
                    )
                    .children(bar)
                    .child(
                        div()
                            .n_subheadline()
                            .n_figures()
                            .text_color(k.secondary)
                            .child(detail),
                    )
                    .when(!buttons.is_empty(), |d| {
                        d.child(
                            div().flex().gap(space::S8).children(
                                buttons
                                    .into_iter()
                                    .map(|b| div().flex_1().min_w(gpui::Pixels::ZERO).child(b)),
                            ),
                        )
                    }),
            )
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chat_core::proto::UpdateInfo;

    fn info() -> UpdateInfo {
        UpdateInfo {
            version: "1.2.3".into(),
            notes: String::new(),
            size: 200,
            sha256: "0".repeat(64),
            published_at: 0,
            download_url: "http://127.0.0.1/v1/updates/1.2.3/download".into(),
        }
    }

    #[test]
    fn texts() {
        assert_eq!(banner_text(&Phase::Idle), None);
        assert_eq!(
            banner_text(&Phase::Available(info())).as_deref(),
            Some("新版本 1.2.3 可以更新了")
        );
        assert_eq!(
            banner_text(&Phase::Downloading {
                info: info(),
                done: 50,
                total: 0
            })
            .as_deref(),
            Some("正在下载新版本 1.2.3… 25%")
        );
        let manual = banner_text(&Phase::Manual(info())).unwrap();
        assert!(manual.contains(env!("PRODUCT_NAME")) && !manual.contains('{'));
        assert_eq!(sheet_key(&Phase::Installing(info())), None);
        assert_eq!(
            sheet_key(&Phase::Ready {
                info: info(),
                dmg: "/tmp/x.dmg".into()
            })
            .as_deref(),
            Some("ready-1.2.3")
        );
        assert_eq!(sheet_key(&Phase::Idle), None);
    }
}
