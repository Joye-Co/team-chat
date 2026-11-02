//! The welcome of a team member with no 群 yet (DESIGN.md §8.5, U06): one
//! centered state in the content area, instead of an empty topic list, an
//! empty timeline and an empty sidebar section side by side.

use gpui::{App, IntoElement, ParentElement, RenderOnce, Styled, Window, div, px};

use crate::{
    characters::{self, CharacterId, CharacterStatus},
    copy::nav,
    theme::{self, Type as _, space},
    ui,
    views::nav::{NewGroup, StartDm},
};

#[derive(IntoElement, Default)]
pub struct Welcome;

impl RenderOnce for Welcome {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = theme::colors(cx);
        div()
            .size_full()
            .bg(k.content)
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
                    .child(characters::image(
                        CharacterId::Jojo,
                        CharacterStatus::Idle,
                        px(64.),
                    ))
                    .child(
                        div()
                            .pt(space::S20)
                            .t_display()
                            .text_color(k.text_1)
                            .child(nav::NO_GROUPS),
                    )
                    .child(
                        div()
                            .pt(space::S6)
                            .t_body()
                            .text_color(k.text_2)
                            .child(nav::NO_GROUPS_HINT),
                    )
                    .child(
                        div()
                            .pt(space::S24)
                            .flex()
                            .gap(space::S12)
                            .child(
                                ui::button(
                                    "welcome-new-group",
                                    nav::NEW_GROUP,
                                    ui::ButtonKind::Primary,
                                    ui::ButtonSize::Lg,
                                    cx,
                                )
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(NewGroup), cx)
                                }),
                            )
                            .child(
                                ui::button(
                                    "welcome-start-dm",
                                    nav::START_DM,
                                    ui::ButtonKind::Secondary,
                                    ui::ButtonSize::Lg,
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
