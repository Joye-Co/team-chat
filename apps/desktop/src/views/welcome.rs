//! The welcome of a team member with no 群 yet (NATIVE.md §3, §5.2): one
//! centered state in the content area, instead of an empty topic list, an
//! empty timeline and an empty sidebar section side by side.

use gpui::{
    App, IntoElement, ParentElement, RenderOnce, Styled, Window, div, prelude::FluentBuilder as _,
    px,
};

use crate::{
    characters::{self, CharacterId, CharacterStatus},
    copy::nav,
    theme::space,
    ui::native::{NativeType as _, PushSize, PushStyle, native_colors, push_button},
    views::nav::{NewGroup, StartDm},
};

#[derive(IntoElement, Default)]
pub struct Welcome;

impl RenderOnce for Welcome {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let n = native_colors(cx);
        div()
            .size_full()
            .bg(n.content)
            .flex()
            .items_center()
            .justify_center()
            .pb(space::S44)
            .child(
                div()
                    .w(px(400.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .text_center()
                    // Jojo greets with its calm face.
                    .when(characters::has_art(CharacterId::Jojo), |el| {
                        el.child(characters::image(
                            CharacterId::Jojo,
                            CharacterStatus::Idle,
                            px(64.),
                        ))
                    })
                    .child(
                        div()
                            .pt(px(16.))
                            .n_title2()
                            .text_color(n.label)
                            .child(nav::NO_GROUPS),
                    )
                    .child(
                        div()
                            .pt(px(4.))
                            .n_body()
                            .text_color(n.secondary)
                            .child(nav::NO_GROUPS_HINT),
                    )
                    .child(
                        div()
                            .pt(px(20.))
                            .flex()
                            .gap(crate::ui::native::tokens::space::S12)
                            .child(
                                push_button(
                                    "welcome-new-group",
                                    nav::NEW_GROUP,
                                    PushStyle::Default,
                                    PushSize::Regular,
                                    cx,
                                )
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(NewGroup), cx)
                                }),
                            )
                            .child(
                                push_button(
                                    "welcome-start-dm",
                                    nav::START_DM,
                                    PushStyle::Outline,
                                    PushSize::Regular,
                                    cx,
                                )
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(StartDm), cx)
                                }),
                            ),
                    ),
            )
    }
}
