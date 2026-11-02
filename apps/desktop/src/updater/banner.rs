//! The quiet update banner above the sidebar footer, and the about card of 通用 in settings.
//! Both render [`Updater`] state and call its commands; nothing here touches the network.

use gpui::{
    App, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window, div,
    img, prelude::FluentBuilder as _, px,
};
use gpui_component::{Sizable as _, button::Button};

use super::{Failure, Phase, Updater, version};
use crate::ui::NamedButton as _;
use crate::{
    copy::{fill, update as u},
    theme::{self, Type as _, space},
    ui::{self, ButtonKind, ButtonSize, Lucide, icon},
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

/// Action buttons a phase offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Update,
    Retry,
    Later,
    Dismiss,
}

fn actions(phase: &Phase) -> &'static [Action] {
    match phase {
        Phase::Available(_) => &[Action::Update, Action::Later],
        Phase::WaitingForSends(_) => &[Action::Later],
        Phase::Manual(_) => &[Action::Dismiss],
        Phase::Failed { info: Some(_), .. } => &[Action::Retry, Action::Later],
        _ => &[],
    }
}

fn action_button(action: Action, id: &'static str, updater: &Entity<Updater>, cx: &App) -> Button {
    let updater = updater.clone();
    let button = match action {
        Action::Update => ui::button(
            (id, 0usize),
            u::UPDATE,
            ButtonKind::Primary,
            ButtonSize::Sm,
            cx,
        ),
        Action::Retry => ui::button(
            (id, 1usize),
            u::RETRY,
            ButtonKind::Primary,
            ButtonSize::Sm,
            cx,
        ),
        // The narrow card closes with an `x`; its tooltip names the action.
        Action::Later => ui::icon_button_sm((id, 2usize), Lucide::X).named(u::LATER),
        Action::Dismiss => ui::icon_button_sm((id, 3usize), Lucide::X).named(u::DISMISS),
    };
    button.on_click(move |_, _, cx| {
        updater.update(cx, |up, cx| match action {
            Action::Update | Action::Retry => up.start_update(cx),
            Action::Later => up.postpone(cx),
            Action::Dismiss => up.dismiss(cx),
        })
    })
}

/// The app-level update banner (DESIGN.md §8.6): a 48 pt card above the
/// sidebar footer, never in the topic column.
pub struct UpdateBanner {
    updater: Option<Entity<Updater>>,
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
            _subscriptions: subscriptions,
        }
    }
}

/// The banner's two lines for a phase: a short title and a detail. The
/// sidebar is narrow, so the long sentences go to the detail and its tooltip.
fn banner_lines(phase: &Phase, text: String) -> (String, Option<String>) {
    match phase {
        Phase::Available(i) => (
            u::BANNER_AVAILABLE.to_string(),
            Some(fill(u::BANNER_VERSION, &[("version", &i.version)])),
        ),
        Phase::Downloading { info, done, total } => {
            let total = if *total == 0 { info.size } else { *total };
            let pct = version::percent(*done, total).to_string();
            (
                fill(u::BANNER_DOWNLOADING, &[("version", &info.version)]),
                Some(fill(u::BANNER_PERCENT, &[("percent", &pct)])),
            )
        }
        Phase::Manual(_) => (
            u::BANNER_MANUAL.to_string(),
            Some(u::BANNER_MANUAL_DETAIL.to_string()),
        ),
        Phase::Failed { .. } => (text, None),
        _ => (text, None),
    }
}

impl Render for UpdateBanner {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(updater) = self.updater.clone() else {
            return div().into_any_element();
        };
        let (phase, visible) = {
            let up = updater.read(cx);
            (up.phase().clone(), up.banner_visible())
        };
        let Some(text) = banner_text(&phase).filter(|_| visible) else {
            return div().into_any_element();
        };
        let colors = theme::colors(cx);
        let progress = match &phase {
            Phase::Downloading { info, done, total } => {
                let total = if *total == 0 { info.size } else { *total };
                Some(version::percent(*done, total) as f32)
            }
            _ => None,
        };
        let busy = matches!(phase, Phase::Installing(_) | Phase::WaitingForSends(_));
        let failed = matches!(phase, Phase::Failed { .. });
        let manual = matches!(phase, Phase::Manual(_));
        let (glyph, glyph_color) = if failed {
            (Lucide::CircleAlert, colors.danger)
        } else {
            (Lucide::Download, colors.accent_text)
        };
        let (title, detail) = banner_lines(&phase, text.clone());
        let actions: Vec<Button> = actions(&phase)
            .iter()
            .map(|a| action_button(*a, "update-banner", &updater, cx))
            .collect();
        // The download bar follows the percent on the detail line: a 3 pt
        // inset track.
        let detail = detail.map(|d| {
            div()
                .flex()
                .items_center()
                .gap(space::S8)
                .child(crate::views::banners::detail_text(d))
                .when_some(progress, |el, pct| {
                    el.child(
                        div()
                            .flex_1()
                            .h(px(3.))
                            .rounded(px(1.5))
                            .bg(colors.field)
                            .child(
                                div()
                                    .h_full()
                                    .rounded(px(1.5))
                                    .bg(colors.accent)
                                    .w(gpui::relative((pct / 100.).clamp(0., 1.))),
                            ),
                    )
                })
                .into_any_element()
        });
        let glyph = if busy {
            crate::ui::spinner()
                .with_size(theme::icon::ROW)
                .color(colors.text_2)
                .into_any_element()
        } else {
            icon(glyph, theme::icon::ROW, glyph_color).into_any_element()
        };
        crate::views::banners::Card {
            id: "update-banner",
            glyph,
            title: title.into(),
            detail,
            actions: actions.into_iter().map(|b| b.into_any_element()).collect(),
            // The manual steps are a paragraph: the card shows the short
            // form and the tooltip the whole of it.
            tooltip: manual.then(|| text.into()),
        }
        .render(cx)
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
        assert_eq!(actions(&Phase::Installing(info())), &[] as &[Action]);
        assert_eq!(actions(&Phase::WaitingForSends(info())), &[Action::Later]);
    }
}
