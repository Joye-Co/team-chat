//! The quiet update banner above the topic view, and the 关于 section of settings.
//! Both render [`Updater`] state and call its commands; nothing here touches the network.

use gpui::{
    Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window, div,
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
    theme,
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

fn action_button(
    action: Action,
    id: &'static str,
    updater: &Entity<Updater>,
) -> Button {
    let updater = updater.clone();
    let (label, primary) = match action {
        Action::Update => (u::UPDATE, true),
        Action::Retry => (u::RETRY, true),
        Action::Later => (u::LATER, false),
        Action::Dismiss => (u::DISMISS, false),
    };
    let button = Button::new((id, action as usize))
        .small()
        .label(label)
        .on_click(move |_, _, cx| {
            updater.update(cx, |up, cx| match action {
                Action::Update | Action::Retry => up.start_update(cx),
                Action::Later => up.postpone(cx),
                Action::Dismiss => up.dismiss(cx),
            })
        });
    if primary { button.primary() } else { button.ghost() }
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
        div()
            .flex_none()
            .w_full()
            .px(px(16.))
            .py(px(6.))
            .flex()
            .items_center()
            .gap(px(8.))
            .bg(colors.panel)
            .border_b_1()
            .border_color(colors.border)
            .text_sm()
            .when(busy, |el| el.child(Spinner::new().small()))
            .child(div().flex_1().min_w(px(0.)).child(text))
            .when_some(progress, |el, pct| {
                el.child(
                    Progress::new("update-progress")
                        .w(px(120.))
                        .value(pct)
                        .color(colors.accent),
                )
            })
            .children(
                actions(&phase)
                    .iter()
                    .map(|a| action_button(*a, "update-banner", &updater)),
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
        let root = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(div().pt(px(16.)).pb(px(6.)).text_base().child(u::ABOUT))
            .child(div().child(version_line));
        let Some(updater) = self.updater.clone() else {
            return root.into_any_element();
        };
        let phase = updater.read(cx).phase().clone();
        let status = match &phase {
            Phase::Idle => None,
            Phase::Checking => Some(u::CHECKING.to_string()),
            Phase::UpToDate => Some(u::UP_TO_DATE.to_string()),
            other => banner_text(other),
        };
        let mut buttons: Vec<Button> = Vec::new();
        match &phase {
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
                        .outline()
                        .label(u::CHECK)
                        .on_click(move |_, _, cx| up.update(cx, |u, cx| u.check_now(cx))),
                );
            }
            _ => {}
        }
        root.when_some(status, |el, s| {
            el.child(div().text_color(colors.muted_foreground).child(s))
        })
        .when(!buttons.is_empty(), |el| {
            el.child(div().flex().gap(px(8.)).children(buttons))
        })
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
