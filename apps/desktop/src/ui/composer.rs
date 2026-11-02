//! The composer shell (DESIGN.md §7.2): a real writing area like Slack's and
//! Feishu's. The view owns the editor (a gpui-component `Textarea`), the
//! mention popup and the send logic; this shell gives it the box, the states
//! and the toolbar row, so every topic, DM and new-topic composer is the same.
//!
//! Empty height: 12 top padding + 44 text (two 22 pt lines) + 40 toolbar =
//! 96 pt. Radius 12, `content` fill, 1 pt `border`; hover `border-strong`;
//! focus a 1 pt `focus-border` with a 3 pt `focus-soft` ring.

use gpui::{
    AnyElement, App, ElementId, InteractiveElement as _, IntoElement, ParentElement, Pixels,
    RenderOnce, SharedString, Styled, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::{
    Disableable as _, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
};

use crate::{
    theme::{self, Type as _, size, space},
    ui::icons::Lucide,
};

/// The horizontal inset gpui-component's `Textarea` keeps at `xsmall`.
pub const EDITOR_INSET: gpui::Pixels = gpui::px(4.);

#[derive(IntoElement)]
pub struct ComposerShell {
    id: ElementId,
    editor: AnyElement,
    focused: bool,
    disabled: bool,
    /// Rows above the text (chips, the editing line), inside the box.
    above: Vec<AnyElement>,
    /// The blocking slab attached to the top of the box (offline, quota).
    slab: Option<AnyElement>,
    /// The format bar: a 36 pt row at the top of the box (§7.2).
    format_bar: Option<AnyElement>,
    tools: Vec<AnyElement>,
    send: Option<AnyElement>,
    max_text_height: Option<Pixels>,
}

impl ComposerShell {
    /// `editor` is the text area: a gpui-component `Textarea` with
    /// `.appearance(false)` and the `body` text style.
    pub fn new(id: impl Into<ElementId>, editor: impl IntoElement) -> Self {
        Self {
            id: id.into(),
            editor: editor.into_any_element(),
            focused: false,
            disabled: false,
            above: Vec::new(),
            slab: None,
            format_bar: None,
            tools: Vec::new(),
            send: None,
            max_text_height: None,
        }
    }

    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// No permission to post: `field` fill, no border, no toolbar.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn above(mut self, el: impl IntoElement) -> Self {
        self.above.push(el.into_any_element());
        self
    }

    pub fn slab(mut self, el: impl IntoElement) -> Self {
        self.slab = Some(el.into_any_element());
        self
    }

    /// The format bar's buttons (use [`composer_tool`]); shown at the top of
    /// the box with a 0.5 pt `sep` under it.
    pub fn format_bar(mut self, buttons: Vec<AnyElement>) -> Self {
        self.format_bar = Some(
            div()
                .flex()
                .items_center()
                .gap(space::S2)
                .children(buttons)
                .into_any_element(),
        );
        self
    }

    /// A tool at the left of the toolbar row (use [`composer_tool`]).
    pub fn tool(mut self, el: impl IntoElement) -> Self {
        self.tools.push(el.into_any_element());
        self
    }

    /// The send button at the right of the toolbar row (use [`send_button`]).
    pub fn send(mut self, el: impl IntoElement) -> Self {
        self.send = Some(el.into_any_element());
        self
    }

    /// The text grows to this height, then scrolls inside: 40 % of the topic
    /// view less the toolbar.
    pub fn max_text_height(mut self, height: Pixels) -> Self {
        self.max_text_height = Some(height);
        self
    }
}

impl RenderOnce for ComposerShell {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = theme::colors(cx);
        let has_slab = self.slab.is_some();
        let edge = if self.disabled {
            gpui::transparent_black()
        } else if self.focused {
            k.focus_border
        } else {
            k.border
        };
        div()
            .flex_none()
            .w_full()
            .flex()
            .flex_col()
            .children(self.slab.map(|slab| {
                div()
                    .w_full()
                    .rounded_t(theme::radius::LG)
                    .bg(k.raised)
                    .border_1()
                    .border_b_0()
                    .border_color(k.border)
                    .px(space::S14)
                    .py(space::S8)
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .t_meta()
                    .text_color(k.text_2)
                    .child(slab)
            }))
            .child(
                div()
                    .id(self.id)
                    .w_full()
                    .min_h(size::COMPOSER_MIN)
                    .map(|el| {
                        if has_slab {
                            el.rounded_b(theme::radius::LG)
                        } else {
                            el.rounded(theme::radius::LG)
                        }
                    })
                    .bg(if self.disabled { k.field } else { k.content })
                    .border_1()
                    .border_color(edge)
                    .when(self.focused && !self.disabled, |el| {
                        el.shadow(k.focus_halo())
                    })
                    .when(!self.focused && !self.disabled, |el| {
                        el.hover(|s| s.border_color(k.border_strong))
                    })
                    .cursor_text()
                    .flex()
                    .flex_col()
                    .children(self.format_bar.map(|bar| {
                        div()
                            .flex_none()
                            .h(size::FORMAT_BAR)
                            .px(space::S8)
                            .flex()
                            .items_center()
                            .border_b(theme::HAIRLINE)
                            .border_color(k.sep)
                            .child(bar)
                    }))
                    .when(!self.above.is_empty(), |el| {
                        el.child(
                            div()
                                .pt(space::S10)
                                .px(space::S14)
                                .flex()
                                .flex_col()
                                .gap(space::S6)
                                .children(self.above),
                        )
                    })
                    .child(
                        div()
                            .w_full()
                            .pt(space::S12)
                            // 14 pt to the text: the editor keeps a 4 pt inset of its own.
                            .px(space::S14 - EDITOR_INSET)
                            .min_h(size::COMPOSER_TEXT_MIN + space::S12)
                            .when_some(self.max_text_height, |el, h| el.max_h(h + space::S12))
                            .t_body()
                            .text_color(k.text_1)
                            .child(self.editor),
                    )
                    .when(!self.disabled, |el| {
                        el.child(
                            div()
                                .flex_none()
                                .h(size::COMPOSER_TOOLBAR)
                                .px(space::S8)
                                .pb(space::S8)
                                .flex()
                                .items_end()
                                .gap(space::S2)
                                .children(self.tools)
                                .child(div().flex_1())
                                .children(self.send),
                        )
                    }),
            )
    }
}

/// A composer toolbar button: 32 x 32, radius 8, an 18 pt glyph in
/// `text-2` (`plus` 添加附件, `at-sign` 提及, `smile` 表情, `type` 格式).
pub fn composer_tool(
    id: impl Into<ElementId>,
    name: Lucide,
    tooltip: impl Into<SharedString>,
) -> Button {
    Button::new(id)
        .ghost()
        .icon(Icon::new(name).size(theme::icon::HEADER))
        .with_size(size::ICON_BUTTON)
        .rounded(theme::radius::MD)
        .tooltip(tooltip.into())
}

/// The send button: 32 x 32, radius 8, `arrow-up` 18. Empty: `field` fill,
/// `placeholder` arrow, disabled. With text: `accent` fill, white arrow.
pub fn send_button(
    id: impl Into<ElementId>,
    ready: bool,
    tooltip: impl Into<SharedString>,
    cx: &App,
) -> Button {
    let k = theme::colors(cx);
    let b = Button::new(id)
        .icon(Icon::new(Lucide::ArrowUp).size(theme::icon::HEADER))
        .with_size(size::ICON_BUTTON)
        .rounded(theme::radius::MD)
        .tooltip(tooltip.into());
    if ready {
        b.primary()
    } else {
        b.ghost()
            .disabled(true)
            .bg(k.field)
            .text_color(k.placeholder)
    }
}
