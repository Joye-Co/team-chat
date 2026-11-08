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

use gpui_component::menu::PopupMenu;

use super::{RailView, modes::TopicItem};
use crate::ui::native::{
    SidebarRow, SidebarSection,
    sidebar::{RowMenu, SidebarMedia},
};

/// A row's click handler.
pub(super) type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// The 16 pt member avatar of a sidebar row (§9.1).
pub(super) const ROW_AVATAR: gpui::Pixels = px(16.);
/// The indent of a 话题 under its member (mode C).
pub(super) const NESTED_PAD: gpui::Pixels = px(18.);

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
            .unread(unread)
            .muted(
                self.services
                    .provider
                    .topic_muted(&topic.group_id, &topic.id),
            );
        // Owner decision (2026-11-08): one signal per row. The state is
        // the 12 mark in the leading slot (等你, 在做, 失败, 等别人), its
        // words in the tooltip; unread is the Semibold title alone.
        let k = crate::ui::native::native_colors(cx);
        let mark = (!line.short.is_empty())
            .then(|| crate::ui::native::badge::state_mark(line.badge, &k))
            .flatten()
            .map(|mark| {
                let words = SharedString::from(line.short.clone());
                div()
                    .id(SharedString::from(format!("rail-mark-{key}-{}", topic.id)))
                    .child(mark)
                    .tooltip(move |window, cx| {
                        gpui_component::tooltip::Tooltip::new(words.clone()).build(window, cx)
                    })
            });
        row = match mark {
            Some(mark) => row.leading(mark),
            None if *indent > px(0.) => row.symbol(crate::ui::sf_symbols::Sf::Group),
            None => row,
        }
        .media_kind(SidebarMedia::Avatar);
        let (services, menu_topic) = (self.services.clone(), topic.clone());
        let row = row
            .on_click(move |_, _, cx| {
                let (g, t) = (g.clone(), t.clone());
                nav.update(cx, |nav, cx| nav.open_topic(g, t, cx))
            })
            .menu(self.row_menu(move |menu, window, cx| {
                super::menus::topic(menu, &services, &menu_topic, window, cx)
            }));
        div()
            .id(SharedString::from(format!("rail-row-{key}-{}", topic.id)))
            .pl(*indent)
            .child(row)
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
        self.section_header_selected(id, label, (folded, false, false, false), fold, open, None)
    }

    /// [`Self::section_header`] with the selected look (T-12: the 群 header
    /// while its 群 page is open) and the muted mark.
    /// `look`: folded, selected, unread (the 群's 群聊 has unread messages)
    /// and muted.
    pub(super) fn section_header_selected(
        &self,
        id: impl Into<ElementId>,
        label: String,
        (folded, selected, unread, muted): (bool, bool, bool, bool),
        fold: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        open: Option<ClickHandler>,
        menu: Option<RowMenu>,
    ) -> AnyElement {
        let fold = std::rc::Rc::new(fold);
        let chevron_fold = fold.clone();
        // The chevron folds the section, whatever the header opens.
        let header = SidebarSection::new(id, label)
            .collapsed(folded)
            .selected(selected)
            .unread(unread && !selected)
            .muted(muted)
            .on_toggle(move |e, window, cx| match &open {
                Some(open) => open(e, window, cx),
                None => fold(e, window, cx),
            })
            .on_chevron(move |e, window, cx| chevron_fold(e, window, cx));
        match menu {
            Some(menu) => header.menu(menu),
            None => header,
        }
        .into_any_element()
    }

    /// A right-click menu of a sidebar item: it stays inside the sidebar
    /// panel, and the item keeps its ring while the menu is open (T-20).
    pub(super) fn row_menu(
        &self,
        build: impl Fn(PopupMenu, &mut Window, &mut gpui::Context<PopupMenu>) -> PopupMenu + 'static,
    ) -> RowMenu {
        RowMenu::new(build, self.panel_bounds.get())
    }
}
