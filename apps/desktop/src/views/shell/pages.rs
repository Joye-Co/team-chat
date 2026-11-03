//! The page entities of one main window and the page column (title bar +
//! body) the shell draws for the current [`Page`].

use gpui::{
    AnyElement, AnyView, App, AppContext as _, Entity, IntoElement, ParentElement, Pixels, Styled,
    div, prelude::FluentBuilder as _, px,
};

use super::{PageView, TitleBarSpec, title_bar};
use crate::{
    app::{Page, RightPane, Services},
    theme,
    views::{
        artifact_page::ArtifactPage, composer::ComposerView, desk::DeskPage, group_page::GroupPage,
        members_page::MembersPage, timeline::TimelineView, topic_page::TopicPage,
    },
};

pub struct Pages {
    pub desk: Entity<DeskPage>,
    pub topic: Entity<TopicPage>,
    pub group: Entity<GroupPage>,
    pub members: Entity<MembersPage>,
    pub artifact: Entity<ArtifactPage>,
}

impl Pages {
    pub fn new(
        services: &Services,
        timeline: Entity<TimelineView>,
        composer: Entity<ComposerView>,
        cx: &mut App,
    ) -> Self {
        Self {
            desk: cx.new(|cx| DeskPage::new(services.clone(), cx)),
            group: cx.new(|cx| GroupPage::new(services.clone(), composer.clone(), cx)),
            topic: cx.new(|cx| TopicPage::new(services.clone(), timeline, composer, cx)),
            members: cx.new(|cx| MembersPage::new(services.clone(), cx)),
            artifact: cx.new(|cx| ArtifactPage::new(services.clone(), cx)),
        }
    }

    /// The title bar spec and the body of `page`.
    pub fn parts(&self, page: Page, cx: &App) -> (TitleBarSpec, AnyView) {
        fn of<T: PageView>(e: &Entity<T>, cx: &App) -> (TitleBarSpec, AnyView) {
            (e.read(cx).title_bar(cx), e.clone().into())
        }
        match page {
            Page::Desk => of(&self.desk, cx),
            Page::Topic => of(&self.topic, cx),
            Page::Group => of(&self.group, cx),
            Page::Members => of(&self.members, cx),
            Page::Artifacts | Page::Artifact => of(&self.artifact, cx),
        }
    }

    /// The page column: the title bar over the body, on `bg`.
    pub fn render(
        &self,
        page: Page,
        open: &RightPane,
        leading: Pixels,
        services: &Services,
        cx: &App,
    ) -> AnyElement {
        let k = theme::colors(cx);
        let (spec, body) = self.parts(page, cx);
        div()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .flex()
            .flex_col()
            .bg(k.bg)
            // A page that draws its own header (首页, desk track) returns an
            // empty spec: no title bar.
            .when(spec != TitleBarSpec::default(), |el| {
                el.child(title_bar::render(&spec, open, leading, services, cx))
            })
            .child(div().flex_1().min_h(px(0.)).child(body))
            .into_any_element()
    }
}
