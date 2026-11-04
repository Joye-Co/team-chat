//! The update sheet of the main window (NATIVE.md §7.1: 更新 default,
//! 稍后), its quiet status line in the sidebar while it is hidden, and the
//! about card of 通用 in settings.
//! Both render [`Updater`] state and call its commands; nothing here touches the network.

use std::collections::HashSet;

use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, ParentElement,
    Render, StatefulInteractiveElement as _, Styled, Subscription, Window, div, img,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{Sizable as _, WindowExt as _, button::Button};

use super::{Failure, Phase, Updater, version};
use crate::ui::{
    native::{
        NativeType as _, PushSize, PushStyle, SheetLayout, SheetWidth, determinate_bar, metrics,
        native_colors, native_sheet, push_button, spinner,
    },
    sf_symbols::SymSize,
};
use crate::{
    copy::{fill, overlays as o, update as u},
    theme::{self, Type as _, space},
    ui::{self, Sym},
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

/// Which sheet a phase shows, keyed by phase kind and version: a sheet the
/// user closed does not come back for the same key.
fn sheet_key(phase: &Phase) -> Option<String> {
    let (kind, info) = match phase {
        Phase::Available(i) => ("available", Some(i)),
        Phase::Downloading { info, .. } | Phase::WaitingForSends(info) | Phase::Installing(info) => {
            ("progress", Some(info))
        }
        Phase::Manual(i) => ("manual", Some(i)),
        Phase::Failed { info: Some(i), .. } => ("failed", Some(i)),
        _ => ("", None),
    };
    info.map(|i| format!("{kind}-{}", i.version))
}

/// The share of a download, 0–1.
fn download_share(phase: &Phase) -> Option<f32> {
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
    _subscriptions: Vec<Subscription>,
}

impl UpdateSheet {
    fn new(updater: Entity<Updater>, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.observe(&updater, |_, _, cx| cx.notify())];
        Self {
            updater,
            _subscriptions: subscriptions,
        }
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
        let updater = self.updater.clone();
        let button = |id: &'static str, label: &'static str, style: PushStyle, cx: &App| {
            push_button(id, label, style, PushSize::Large, cx)
        };
        let hide = |cx: &App| {
            button("update-hide", o::UPDATE_HIDE, PushStyle::Standard, cx)
                .on_click(|_, window, cx| window.close_dialog(cx))
        };
        let u1 = updater.clone();
        let u2 = updater.clone();
        let layout = match &phase {
            Phase::Available(_) => SheetLayout::new(fill(o::UPDATE_TITLE, &[("version", v)]))
                .explanation(o::UPDATE_EXPLANATION)
                .buttons([
                    button("update-later", u::LATER, PushStyle::Standard, cx).on_click(
                        move |_, window, cx| {
                            u1.update(cx, |up, cx| up.postpone(cx));
                            window.close_dialog(cx);
                        },
                    ),
                    button("update-now", u::UPDATE, PushStyle::Default, cx)
                        .on_click(move |_, _, cx| u2.update(cx, |up, cx| up.start_update(cx))),
                ]),
            Phase::Downloading { .. } => {
                let share = download_share(&phase).unwrap_or(0.);
                SheetLayout::new(fill(o::UPDATE_DOWNLOADING_TITLE, &[("version", v)]))
                    .body(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(div().flex_1().child(determinate_bar(share, None, cx)))
                            .child(
                                div()
                                    .flex_none()
                                    .n_subheadline()
                                    .n_figures()
                                    .text_color(k.secondary)
                                    .child(fill(
                                        u::BANNER_PERCENT,
                                        &[("percent", &((share * 100.).round() as u32).to_string())],
                                    )),
                            ),
                    )
                    .buttons([hide(cx)])
            }
            Phase::WaitingForSends(_) | Phase::Installing(_) => {
                let waiting = matches!(phase, Phase::WaitingForSends(_));
                SheetLayout::new(fill(o::UPDATE_INSTALLING_TITLE, &[("version", v)]))
                    .explanation(if waiting { u::WAITING_SENDS } else { u::INSTALLING })
                    .body(div().child(spinner().with_size(metrics::SPINNER).color(k.secondary)))
                    .buttons([hide(cx)])
            }
            Phase::Manual(_) => SheetLayout::new(fill(o::UPDATE_MANUAL_TITLE, &[("version", v)]))
                .explanation(fill(u::MANUAL, &[("product", env!("PRODUCT_NAME"))]))
                .buttons([button("update-ok", u::DISMISS, PushStyle::Default, cx).on_click(
                    move |_, window, cx| {
                        u1.update(cx, |up, cx| up.dismiss(cx));
                        window.close_dialog(cx);
                    },
                )]),
            Phase::Failed { .. } => SheetLayout::new(fill(o::UPDATE_FAILED_TITLE, &[("version", v)]))
                .explanation(banner_text(&phase).unwrap_or_default())
                .buttons([
                    button("update-later", u::LATER, PushStyle::Standard, cx).on_click(
                        move |_, window, cx| {
                            u1.update(cx, |up, cx| up.postpone(cx));
                            window.close_dialog(cx);
                        },
                    ),
                    button("update-retry", u::RETRY, PushStyle::Default, cx)
                        .on_click(move |_, _, cx| u2.update(cx, |up, cx| up.start_update(cx))),
                ]),
            _ => return div().into_any_element(),
        };
        div().w_full().child(layout).into_any_element()
    }
}

/// The update in the main window: a sheet when an update is ready (and
/// when it needs the user later), and, while the sheet is hidden during a
/// download or install, a quiet status line in the sidebar (no banner).
pub struct UpdateBanner {
    updater: Option<Entity<Updater>>,
    sheet_open: bool,
    /// The sheets already shown (see [`sheet_key`]).
    shown: HashSet<String>,
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
            _subscriptions: subscriptions,
        }
    }

    fn open_sheet(&mut self, updater: Entity<Updater>, window: &mut Window, cx: &mut Context<Self>) {
        self.sheet_open = true;
        let sheet = cx.new(|cx| UpdateSheet::new(updater.clone(), cx));
        let this = cx.entity().downgrade();
        window.defer(cx, move |window, cx| {
            window.open_dialog(cx, move |dialog, _, cx| {
                let this = this.clone();
                let updater = updater.clone();
                native_sheet(dialog, SheetWidth::Form, cx)
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
            self.open_sheet(updater, window, cx);
            return div().into_any_element();
        }
        if self.sheet_open {
            return div().into_any_element();
        }
        // The sheet is hidden: a quiet line while the update runs.
        let k = native_colors(cx);
        let Some(info) = phase.info() else {
            return div().into_any_element();
        };
        let share = download_share(&phase);
        let text = match &phase {
            Phase::Downloading { .. } => fill(
                o::UPDATE_STATUS,
                &[
                    ("version", &info.version),
                    ("percent", &((share.unwrap_or(0.) * 100.).round() as u32).to_string()),
                ],
            ),
            Phase::WaitingForSends(_) | Phase::Installing(_) => {
                fill(o::UPDATE_INSTALLING_TITLE, &[("version", &info.version)])
            }
            _ => return div().into_any_element(),
        };
        div()
            .id("update-status")
            .flex_none()
            .px(px(20.))
            .pb(px(10.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .n_subheadline()
                    .n_figures()
                    .text_color(k.secondary)
                    .child(Sym::Download.icon(SymSize::INLINE_SMALL).text_color(k.secondary))
                    .child(div().min_w(px(0.)).one_line().child(text)),
            )
            .when_some(share, |d, s| d.child(determinate_bar(s, None, cx)))
            .on_click(cx.listener(move |this, _, window, cx| {
                // A click on the line shows the sheet again.
                if let Some(u) = this.updater.clone() {
                    this.open_sheet(u, window, cx);
                }
            }))
            .into_any_element()
    }
}

/// The 关于 row of settings: version, update state and 检查更新.
pub struct AboutUpdate {
    updater: Option<Entity<Updater>>,
    _subscriptions: Vec<Subscription>,
}

impl AboutUpdate {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let updater = super::global(cx);
        let subscriptions = updater
            .iter()
            .map(|u| cx.observe(u, |_, _, cx| cx.notify()))
            .collect();
        Self {
            updater,
            _subscriptions: subscriptions,
        }
    }
}

impl Render for AboutUpdate {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let version_line = fill(u::VERSION, &[("version", version::CURRENT)]);
        let phase = self.updater.as_ref().map(|u| u.read(cx).phase().clone());
        let status = match &phase {
            None | Some(Phase::Idle) => None,
            Some(Phase::Checking) => Some(u::CHECKING.to_string()),
            Some(Phase::UpToDate) => Some(u::UP_TO_DATE.to_string()),
            Some(other) => banner_text(other),
        };
        let failed = matches!(phase, Some(Phase::Failed { .. }));
        let busy = matches!(
            phase,
            Some(
                Phase::Checking
                    | Phase::Downloading { .. }
                    | Phase::Installing(_)
                    | Phase::WaitingForSends(_)
            )
        );
        let mut buttons: Vec<Button> = Vec::new();
        if let (Some(updater), Some(phase)) = (self.updater.clone(), &phase) {
            match phase {
                Phase::Available(i) | Phase::Failed { info: Some(i), .. } => {
                    let up = updater.clone();
                    buttons.push(
                        ui::primary_button(
                            "about-update",
                            fill(u::UPDATE_TO, &[("version", &i.version)]),
                            cx,
                        )
                        .on_click(move |_, _, cx| up.update(cx, |u, cx| u.start_update(cx))),
                    );
                }
                Phase::Idle | Phase::UpToDate | Phase::Failed { info: None, .. } => {
                    let up = updater.clone();
                    buttons.push(
                        ui::secondary_button("about-check", u::CHECK, cx)
                            .on_click(move |_, _, cx| up.update(cx, |u, cx| u.check_now(cx))),
                    );
                }
                _ => {}
            }
        }
        let dark = theme::is_dark(cx);
        // The first row of the 关于 table: the app icon, the product and
        // version, the update state and its button.
        div()
            .w_full()
            .min_h(px(72.))
            .pl(space::S16)
            .pr(space::S12)
            .py(space::S12)
            .flex()
            .items_center()
            .gap(space::S12)
            .child(
                img(if dark {
                    crate::ui::icons::APP_ICON_DARK_72
                } else {
                    crate::ui::icons::APP_ICON_72
                })
                .flex_none()
                .size(px(44.)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .t_ui()
                            .semibold()
                            .text_color(colors.text_1)
                            .one_line()
                            .child(env!("PRODUCT_NAME")),
                    )
                    .child(
                        div()
                            .t_small()
                            .tnum()
                            .text_color(colors.text_2)
                            .one_line()
                            .child(version_line),
                    )
                    .when_some(status, |el, s| {
                        el.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(space::S6)
                                .t_meta()
                                .tnum()
                                .text_color(if failed {
                                    colors.danger_text
                                } else {
                                    colors.text_3
                                })
                                .when(busy, |el| {
                                    el.child(
                                        crate::ui::spinner()
                                            .with_size(theme::icon::CHIP)
                                            .color(colors.text_3),
                                    )
                                })
                                .child(div().min_w(px(0.)).child(s)),
                        )
                    }),
            )
            .children(buttons.into_iter().map(|b| div().flex_none().child(b)))
            .into_any_element()
    }
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
        assert_eq!(
            sheet_key(&Phase::Installing(info())).as_deref(),
            Some("progress-1.2.3")
        );
        assert_eq!(sheet_key(&Phase::Idle), None);
    }
}
