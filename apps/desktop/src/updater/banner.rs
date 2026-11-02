//! The quiet update banner above the topic view, and the 关于 section of settings.
//! Both render [`Updater`] state and call its commands; nothing here touches the network.

use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window, div, img,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _,
    button::{Button, ButtonVariants as _},
    progress::Progress,
    spinner::Spinner,
};

use super::{Failure, Phase, Updater, version};
use crate::{
    copy::{fill, update as u},
    theme::{self, Type as _},
    ui::{Lucide, icon},
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

fn action_button(action: Action, id: &'static str, updater: &Entity<Updater>) -> Button {
    let updater = updater.clone();
    let (label, primary) = match action {
        Action::Update => (u::UPDATE, true),
        Action::Retry => (u::RETRY, true),
        Action::Later => (u::LATER, false),
        Action::Dismiss => (u::DISMISS, false),
    };
    let button = Button::new((id, action as usize))
        .xsmall()
        .label(label)
        .on_click(move |_, _, cx| {
            updater.update(cx, |up, cx| match action {
                Action::Update | Action::Retry => up.start_update(cx),
                Action::Later => up.postpone(cx),
                Action::Dismiss => up.dismiss(cx),
            })
        });
    if primary {
        button.primary()
    } else {
        button.ghost()
    }
}

/// Shown above the topic view while an update is available or running.
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
        // A neutral band (DESIGN.md §11, §13.14): 30 pt, radius 8, `fill-field`,
        // a 13 pt glyph and 12 pt `text-2`. Never red: a failure shows only
        // its glyph in the danger color.
        let glyph = if failed {
            Lucide::CircleAlert
        } else {
            Lucide::Download
        };
        div()
            .flex_none()
            .w_full()
            .px(theme::space::S20)
            .pb(theme::space::S8)
            .child(
                div()
                    .w_full()
                    .min_h(px(30.))
                    .pl(theme::space::S10)
                    .pr(theme::space::S4)
                    .py(theme::space::S2)
                    .rounded(theme::radius::ROW)
                    .bg(colors.fill_field)
                    .flex()
                    .items_center()
                    .gap(theme::space::S8)
                    .t_callout()
                    .tnum()
                    .text_color(colors.text_2)
                    .map(|el| {
                        if busy {
                            el.child(Spinner::new().xsmall().color(colors.text_2))
                        } else {
                            el.child(icon(
                                glyph,
                                theme::icon::SMALL,
                                if failed { colors.danger } else { colors.text_2 },
                            ))
                        }
                    })
                    .child(div().flex_1().min_w(px(0.)).one_line().child(text))
                    .when_some(progress, |el, pct| {
                        el.child(
                            Progress::new("update-progress")
                                .w(px(120.))
                                .h(px(4.))
                                .value(pct)
                                .color(colors.accent),
                        )
                    })
                    .children(
                        actions(&phase)
                            .iter()
                            .map(|a| action_button(*a, "update-banner", &updater)),
                    ),
            )
            .into_any_element()
    }
}

/// The 关于 section of settings: version, update state and 检查更新.
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
                        Button::new("about-update")
                            .small()
                            .primary()
                            .label(fill(u::UPDATE_TO, &[("version", &i.version)]))
                            .on_click(move |_, _, cx| up.update(cx, |u, cx| u.start_update(cx))),
                    );
                }
                Phase::Idle | Phase::UpToDate | Phase::Failed { info: None, .. } => {
                    let up = updater.clone();
                    buttons.push(
                        Button::new("about-check")
                            .small()
                            .label(u::CHECK)
                            .on_click(move |_, _, cx| up.update(cx, |u, cx| u.check_now(cx))),
                    );
                }
                _ => {}
            }
        }
        // One row of the 关于 group (DESIGN.md §5.5): the app icon, the
        // product and version, the update state and its button.
        div()
            .w_full()
            .min_h(px(52.))
            .px(theme::space::S12)
            .py(theme::space::S10)
            .flex()
            .items_center()
            .gap(theme::space::S12)
            .child(img(crate::ui::icons::APP_ICON_88).flex_none().size(px(40.)))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(theme::space::S2)
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
                            .t_callout()
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
                                .gap(theme::space::S6)
                                .t_callout()
                                .tnum()
                                .text_color(if failed {
                                    colors.danger_text
                                } else {
                                    colors.text_2
                                })
                                .when(busy, |el| {
                                    el.child(Spinner::new().xsmall().color(colors.text_3))
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
