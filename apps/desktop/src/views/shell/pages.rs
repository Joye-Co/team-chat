//! The page entities of one main window and the page column (title bar +
//! body) the shell draws for the current [`Page`].

use gpui::{
    AnyElement, AnyView, App, AppContext as _, Entity, InteractiveElement as _, IntoElement,
    ParentElement, Pixels, Styled, Window, div, prelude::FluentBuilder as _, px,
};

use super::{PageView, TitleBarSpec, title_bar};
use crate::{
    app::{Page, RightPane, Services},
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

    /// The page column (NATIVE.md §2): the 52 pt toolbar row over the
    /// body, on `content`. Content that scrolls up fades out under the
    /// toolbar (the scroll edge); there is no hairline. `leading` is where
    /// the title starts; `active` is false in an inactive window.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &self,
        page: Page,
        open: &RightPane,
        leading: Pixels,
        active: bool,
        services: &Services,
        window: &mut Window,
        cx: &App,
    ) -> AnyElement {
        let k = super::chrome::colors(cx);
        let (spec, body) = self.parts(page, cx);
        // pages track: a page with its own title bars gets none from here.
        let own_header = match page {
            Page::Group => self.group.read(cx).draws_own_header(cx),
            Page::Members => self.members.read(cx).draws_own_header(cx),
            Page::Artifacts | Page::Artifact => self.artifact.read(cx).draws_own_header(cx),
            Page::Desk | Page::Topic => false,
        };
        let toolbar = !own_header && spec != TitleBarSpec::default();
        // §2.2: the header band h52 with its border-b in the flow; the body
        // scrolls under it with no fade.
        div()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .relative()
            .flex()
            .flex_col()
            .bg(k.content)
            .when(toolbar, |el| {
                el.child(title_bar::render(
                    &spec, open, leading, active, services, window, cx,
                ))
            })
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .relative()
                    .child(div().size_full().child(body)),
            )
            .into_any_element()
    }
}

// --- v12/fx-shell: the empty state ---
/// The window of a member with no 群 yet (coordinator decision, §9.4): the
/// toolbar title 首页, then centered `secondary` Body text and two push
/// buttons, `新建群…` (Default) and `发起私聊…`. No illustration.
pub fn empty_page(leading: Pixels, active: bool, _window: &mut Window, cx: &App) -> AnyElement {
    use crate::{
        copy::{nav, rail},
        ui::native::{PushSize, PushStyle, ToolbarRow, push_button},
        views::nav::{NewGroup, StartDm},
    };
    let k = super::chrome::colors(cx);
    div()
        .flex_1()
        .min_w(px(0.))
        .h_full()
        .flex()
        .flex_col()
        .bg(k.content)
        .child(
            ToolbarRow::new(rail::RAIL_DESK)
                .leading_inset(leading)
                .inactive(!active),
        )
        // §2.13: the full Empty state (40 icon box, title 18, description,
        // primary sm and outline sm), centered in the page.
        .child(
            div()
                .id("empty-state")
                .flex_1()
                .min_h(px(0.))
                .flex()
                .child(
                    crate::ui::native::Empty::new(crate::ui::Sym::Members, rail::EMPTY_NO_GROUPS_TITLE)
                        .description(nav::NO_GROUPS_HINT)
                        .action(
                            push_button(
                                "empty-new-group",
                                nav::MENU_NEW_GROUP,
                                PushStyle::Default,
                                PushSize::Small,
                                cx,
                            )
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(NewGroup), cx)
                            }),
                        )
                        .action(
                            push_button(
                                "empty-start-dm",
                                nav::MENU_START_DM,
                                PushStyle::Outline,
                                PushSize::Small,
                                cx,
                            )
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(StartDm), cx)
                            }),
                        )
                        .wide(),
                ),
        )
        .into_any_element()
}
// --- end v12/fx-shell ---
