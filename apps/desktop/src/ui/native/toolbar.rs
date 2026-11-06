//! The toolbar row (NATIVE.md §2, §5.1): the window title and subtitle at
//! the leading edge, glass capsule groups of 36 pt items at the trailing
//! edge. The row is transparent over the content; no hairline under it.

use crate::ui::truncate::TruncText;
use gpui::{
    AnyElement, App, Hsla, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window,
    div, prelude::FluentBuilder as _, px,
};
use gpui_component::button::Button;

use super::tokens::{NativeType as _, emphasized, icon, metrics, native_colors};
use crate::ui::sf_symbols::{Sf, Weight, symbol};

/// Related toolbar items in one glass capsule (§5.1): 36 pt tall, items 36
/// wide, no dividers. A lone prominent item draws without a capsule.
#[derive(IntoElement)]
pub struct ToolbarGroup {
    items: Vec<Button>,
    bare: bool,
}

impl ToolbarGroup {
    pub fn new(items: impl IntoIterator<Item = Button>) -> Self {
        Self {
            items: items.into_iter().collect(),
            bare: false,
        }
    }

    /// No capsule behind the items (the prominent action has its own fill).
    pub fn bare(mut self) -> Self {
        self.bare = true;
        self
    }
}

impl RenderOnce for ToolbarGroup {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        // D4: toolbar items are 32 ghost icon buttons in a row, gap 8; no
        // glass capsule and no drop shadow.
        let _ = (self.bare, cx);
        div()
            .flex_none()
            .h(metrics::TOOLBAR_GROUP)
            .flex()
            .items_center()
            .gap(metrics::TOOLBAR_GROUP_GAP)
            .children(self.items)
    }
}

/// The 52 pt toolbar row: title (Title 3) over subtitle (Subheadline,
/// `secondary`) starting 20 pt from the leading edge, at most three groups
/// 8 pt apart ending 12 pt from the trailing edge (§2).
/// A toolbar slot around an item that is not a plain `Button` (a pull-down
/// toolbar item): h32, no capsule and no shadow (D4).
pub fn toolbar_slot(item: impl IntoElement) -> gpui::Div {
    div()
        .flex_none()
        .h(metrics::TOOLBAR_GROUP)
        .flex()
        .items_center()
        .child(item)
}

#[derive(IntoElement)]
pub struct ToolbarRow {
    title: SharedString,
    subtitle: Option<SharedString>,
    state: Option<(Sf, Hsla)>,
    groups: Vec<AnyElement>,
    leading_inset: gpui::Pixels,
    inactive: Option<bool>,
    /// A control before the title (the 产出 page's back button).
    leading: Option<AnyElement>,
}

impl ToolbarRow {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            state: None,
            groups: Vec::new(),
            leading_inset: metrics::TOOLBAR_LEADING,
            inactive: None,
            leading: None,
        }
    }

    /// A control before the title, such as a back button.
    pub fn leading(mut self, el: impl IntoElement) -> Self {
        self.leading = Some(el.into_any_element());
        self
    }

    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// A state symbol before the subtitle (needs you:
    /// `exclamationmark.circle.fill` in `amber`).
    pub fn state(mut self, sf: Sf, color: Hsla) -> Self {
        self.state = Some((sf, color));
        self
    }

    pub fn group(mut self, group: impl IntoElement) -> Self {
        self.groups.push(group.into_any_element());
        self
    }

    /// Where the title starts: 20 pt after the sidebar by default.
    pub fn leading_inset(mut self, inset: gpui::Pixels) -> Self {
        self.leading_inset = inset;
        self
    }

    /// Forces the inactive-window look (gallery); otherwise the window says.
    pub fn inactive(mut self, inactive: bool) -> Self {
        self.inactive = Some(inactive);
        self
    }
}

impl RenderOnce for ToolbarRow {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        let active = self
            .inactive
            .map(|i| !i)
            .unwrap_or_else(|| emphasized(window));
        let title_ink = if active { k.label } else { k.secondary };
        // §2.2: one header band h52 with a 1 px border-b.
        div()
            .flex_none()
            .w_full()
            .h(metrics::TOOLBAR_ROW)
            .bg(k.content)
            .border_b_1()
            .border_color(k.separator)
            .pl(self.leading_inset)
            .pr(metrics::TOOLBAR_TRAILING)
            .flex()
            .items_center()
            .gap(metrics::TOOLBAR_GROUP_GAP)
            .children(self.leading)
            .child(
                // One line on the traffic-light line: the title, then the
                // subtitle after it (owner decision, v18).
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_shrink_0()
                            .max_w(gpui::relative(0.7))
                            .child(TruncText::new(self.title).n_title3().text_color(title_ink)),
                    )
                    .when_some(self.subtitle, |d, sub| {
                        d.child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .when_some(self.state, |d, (sf, color)| {
                                    d.child(symbol(sf, icon::SM, Weight::Regular, color, window))
                                })
                                .child(TruncText::new(sub).n_subheadline().text_color(k.secondary)),
                        )
                    }),
            )
            .children(self.groups)
    }
}
