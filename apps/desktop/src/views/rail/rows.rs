//! The rows of the sidebar (NATIVE.md §9.1), built from the kit's
//! `SidebarRow` and `SidebarSection`: 32 pt 话题 rows with an 18 pt leading
//! slot (the amber needs-you symbol, the working member's 16 pt avatar, or
//! nothing) and a trailing count or time; section headers with the
//! disclosure chevron on hover. No hover fill, the arrow cursor, a context
//! menu on every object.

use gpui::{
    AnyElement, App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement as _, Styled, Window, div, px,
};

use gpui_component::menu::ContextMenuExt as _;

use super::{RailView, modes::TopicItem};
use crate::ui::native::{BadgeState, SidebarRow, SidebarSection, sidebar::{RightColumn, SidebarMedia}};

/// A row's click handler.
pub(super) type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// The 16 pt member avatar of a sidebar row (§9.1).
pub(super) const ROW_AVATAR: gpui::Pixels = px(16.);
/// The indent of a 话题 under its member (mode C).
pub(super) const NESTED_PAD: gpui::Pixels = px(18.);
/// The room the disclosure chevron of a section header takes at its
/// trailing edge (10 pt inset, 8 pt padding, the 11 pt chevron and air).
const CHEVRON_HIT: gpui::Pixels = px(34.);

/// How a 话题 row shows its leading slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SlotStyle {
    /// The doing member's avatar (modes A and B).
    Tile,
    /// Under its member (mode C): only the needs-you symbol.
    Dot,
}

impl RailView {
    /// A sidebar row with the window's inactive look when the shell asks
    /// for it.
    pub(super) fn row(
        &self,
        id: impl Into<ElementId>,
        title: impl Into<SharedString>,
    ) -> SidebarRow {
        let row = SidebarRow::new(id, title);
        if self.show_inactive {
            row.inactive(true)
        } else {
            row
        }
    }

    /// A 话题 row (§2.1, §2.12): the title (Semibold while unread) and one
    /// right-column item. No state media: inside a group every 话题 row has
    /// the same empty media slot, and the state goes to the right column as
    /// a StatusBadge (`等你`, `失败`, `3/5`) or the unread count pill.
    pub(super) fn topic_row(&self, item: &TopicItem, cx: &App) -> AnyElement {
        let TopicItem {
            key,
            topic,
            line,
            selected,
            indent,
            ..
        } = item;
        let nav = self.services.nav.clone();
        let (g, t) = (topic.group_id.clone(), topic.id.clone());
        let title = crate::views::sidebar::topic_display_title(&topic.title).0;
        let unread = topic.unread > 0 && !*selected;
        let mut row = self
            .row(
                SharedString::from(format!("rail-{key}-{}", topic.id)),
                title,
            )
            .selected(*selected)
            .unread(unread);
        let badge = || RightColumn::Status(line.badge, line.short.clone().into());
        let urgent = matches!(line.badge, BadgeState::NeedsYou | BadgeState::Failed);
        row = row.media_kind(SidebarMedia::None).right(if urgent && !line.short.is_empty() {
            badge()
        } else if unread {
            RightColumn::Count(topic.unread as u32)
        } else if !line.short.is_empty() {
            badge()
        } else {
            RightColumn::None
        });
        let _ = cx;
        let row = row.on_click(move |_, _, cx| {
            let (g, t) = (g.clone(), t.clone());
            nav.update(cx, |nav, cx| nav.open_topic(g, t, cx))
        });
        div()
            .id(SharedString::from(format!("rail-row-{key}-{}", topic.id)))
            .pl(*indent)
            .child(row)
            .context_menu({
                let (services, topic) = (self.services.clone(), topic.clone());
                move |menu, window, cx| super::menus::topic(menu, &services, &topic, window, cx)
            })
            .into_any_element()
    }

    /// A section header (`开发`, `私聊`, `需要你`): Section style with the
    /// disclosure chevron on hover. A click on the chevron folds the
    /// section; a click on the rest runs `open` when given (a 群 opens its
    /// page), else it folds too.
    pub(super) fn section_header(
        &self,
        id: impl Into<ElementId>,
        label: String,
        folded: bool,
        fold: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        open: Option<ClickHandler>,
    ) -> AnyElement {
        let id: ElementId = id.into();
        let fold = std::rc::Rc::new(fold);
        let chevron_fold = fold.clone();
        let header = SidebarSection::new(id.clone(), label)
            .collapsed(folded)
            .on_toggle(move |e, window, cx| match &open {
                Some(open) => open(e, window, cx),
                None => fold(e, window, cx),
            });
        div()
            .relative()
            .child(header)
            // The chevron folds the section, whatever the header opens.
            .child(
                div()
                    .id(ElementId::Name(format!("{id}-fold").into()))
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right_0()
                    .w(CHEVRON_HIT)
                    .on_click(move |e, window, cx| {
                        cx.stop_propagation();
                        chevron_fold(e, window, cx)
                    }),
            )
            .into_any_element()
    }
}
