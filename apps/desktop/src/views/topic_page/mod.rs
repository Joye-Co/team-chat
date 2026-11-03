//! The 话题 page (Topic.dc.html): the existing timeline and composer under
//! the shell's title bar (群 /, title, state sentence, pane buttons).
//!
//! Foundation stub: wraps `TimelineView` (its own header hidden) and
//! `ComposerView`. The topic track restyles the timeline, adds the pinned
//! decision card above the composer and the collapsed plan line.

pub mod scenes;

use gpui::{
    App, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window, div, px,
};

use crate::{
    app::{RightPane, Services},
    data::TopicStatus,
    theme::{self, StateTone},
    views::{
        composer::ComposerView,
        shell::{PageView, TitleBarSpec, TitleButton, TitleButtonKind, TitleStyle, panes::watch},
        timeline::TimelineView,
    },
};

pub struct TopicPage {
    services: Services,
    timeline: Entity<TimelineView>,
    composer: Entity<ComposerView>,
    _subscriptions: Vec<Subscription>,
}

impl TopicPage {
    pub fn new(
        services: Services,
        timeline: Entity<TimelineView>,
        composer: Entity<ComposerView>,
        cx: &mut Context<Self>,
    ) -> Self {
        // The shell draws the title bar; the v1 header band would repeat it.
        timeline.update(cx, |t, cx| t.set_show_header(false, cx));
        Self {
            _subscriptions: watch(&services, cx),
            services,
            timeline,
            composer,
        }
    }
}

/// The state sentence of a 话题 and its tone: amber only when it waits on
/// me, blue while a member works on it.
pub fn state_sentence(
    services: &Services,
    group_id: &str,
    topic_id: &str,
) -> Option<(String, StateTone)> {
    let p = &services.provider;
    let topic = p.topic(group_id, topic_id)?;
    let members = p.members();
    let me = services.me();
    let text = crate::views::status_chip_text(&topic, me.as_deref(), &members)?;
    let mine = me.is_some_and(|me| topic.waiting_on.iter().any(|m| *m == me));
    let tone = match topic.status {
        _ if mine => StateTone::You,
        TopicStatus::InProgress => StateTone::Live,
        TopicStatus::Blocked => StateTone::Problem,
        TopicStatus::Done => StateTone::Done,
        _ => StateTone::Quiet,
    };
    Some((text, tone))
}

/// The pane buttons of a 话题 page: 单独问, 产出 (when it has one), 电脑
/// (while a member's computer is active), 改动 (when it has a PR), rail.
pub fn pane_buttons(services: &Services, group_id: &str, topic_id: &str) -> Vec<TitleButton> {
    let p = &services.provider;
    let mut out = Vec::new();
    if let Some(m) = p.side_chat_default_member() {
        out.push(TitleButton::new(
            TitleButtonKind::SideChat,
            RightPane::SideChat(m),
        ));
    }
    if let Some(a) = p.artifacts(group_id, topic_id).first() {
        out.push(TitleButton::new(
            TitleButtonKind::Outputs,
            RightPane::Artifact {
                id: a.id.clone(),
                version: None,
            },
        ));
    }
    if let Some(r) = p
        .runs(group_id, topic_id)
        .iter()
        .find(|r| r.status.is_active())
    {
        out.push(TitleButton::new(
            TitleButtonKind::Machine,
            RightPane::Live(r.id.clone()),
        ));
    }
    if p.topic_changes(group_id, topic_id).is_some() {
        out.push(TitleButton::new(
            TitleButtonKind::Changes,
            RightPane::Changes,
        ));
    }
    out.push(TitleButton::new(
        TitleButtonKind::Rail,
        RightPane::TopicRail,
    ));
    out
}

impl PageView for TopicPage {
    fn title_bar(&self, cx: &App) -> TitleBarSpec {
        let Some((g, t)) = self.services.nav.read(cx).topic.clone() else {
            return TitleBarSpec::default();
        };
        let p = &self.services.provider;
        let group = p.groups().into_iter().find(|x| x.id == g).map(|x| x.name);
        let title = p
            .topic(&g, &t)
            .map(|x| crate::views::sidebar::topic_display_title(&x.title).0)
            .unwrap_or_default();
        TitleBarSpec {
            breadcrumb: group,
            title,
            style: TitleStyle::Topic,
            subtitle: None,
            state: state_sentence(&self.services, &g, &t),
            buttons: pane_buttons(&self.services, &g, &t),
        }
    }
}

impl Render for TopicPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = theme::colors(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(k.bg)
            .child(div().flex_1().min_h(px(0.)).child(self.timeline.clone()))
            .child(self.composer.clone())
    }
}
