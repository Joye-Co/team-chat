//! Message search. Cmd+F searches the current 群, Cmd+Shift+F (and the
//! sidebar field) all of mine. The dialog sends the query to the server
//! through [`ChatProvider::search`] as the user types, shows each hit with
//! its 群, 话题, author, time and a snippet with the match highlighted, and
//! Enter opens the topic at that message (`Navigation::open_message`).

pub mod preview;
mod results;

use std::cell::Cell;

use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, KeyBinding, ParentElement, Render, Styled,
    Window, div, px,
};
use gpui_component::{
    Sizable as _, WindowExt as _,
    list::{List, ListState},
    tab::{Tab, TabBar},
};

use crate::{
    app::{Place, Services},
    copy::search as s,
    data::GroupId,
    theme,
};

pub use results::SearchDelegate;

gpui::actions!(search, [SearchInGroup, SearchEverywhere]);

/// Key bindings and the two global actions. Call once after `nav::install`.
pub fn install(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-f", SearchInGroup, None),
        KeyBinding::new("cmd-shift-f", SearchEverywhere, None),
    ]);
    cx.on_action(|_: &SearchInGroup, cx| open_deferred(false, cx));
    cx.on_action(|_: &SearchEverywhere, cx| open_deferred(true, cx));
}

fn open_deferred(everywhere: bool, cx: &mut App) {
    super::nav::in_main_window(cx, move |window, cx| {
        if !window.has_active_dialog(cx) {
            open(super::nav::services_of(cx), everywhere, window, cx);
        }
    });
}

/// The group Cmd+F searches: the open topic's group or the selected group.
pub fn current_group(services: &Services, cx: &App) -> Option<GroupId> {
    let nav = services.nav.read(cx);
    match (&nav.topic, &nav.place) {
        (Some((g, _)), _) => Some(g.clone()),
        (None, Some(Place::Group(g))) => Some(g.clone()),
        _ => None,
    }
}

/// The dialog body: the scope tabs and the searchable result list.
pub struct SearchDialog {
    list: Entity<ListState<SearchDelegate>>,
}

impl SearchDialog {
    pub fn new(
        services: Services,
        everywhere: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let group = current_group(&services, cx);
        let delegate = SearchDelegate::new(services, group, everywhere, cx);
        let list = cx.new(|cx| ListState::new(delegate, window, cx).searchable(true));
        Self { list }
    }

    pub fn list(&self) -> &Entity<ListState<SearchDelegate>> {
        &self.list
    }

    fn set_everywhere(&mut self, everywhere: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.list.update(cx, |state, cx| {
            if state.delegate().everywhere() == everywhere {
                return;
            }
            state.delegate_mut().set_everywhere(everywhere);
            let query = state.delegate().query().to_string();
            state.set_query(&query, window, cx);
        });
    }
}

impl Render for SearchDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::colors(cx);
        let delegate = self.list.read(cx).delegate();
        let has_group = delegate.group().is_some();
        let everywhere = delegate.everywhere();
        let footer = delegate.footer_text();
        let view = cx.entity().downgrade();
        let tabs = TabBar::new("search-scope")
            .segmented()
            .small()
            .selected_index(if everywhere { 1 } else { 0 })
            .on_click(move |ix: &usize, window, cx| {
                let everywhere = *ix == 1;
                let _ = view.update(cx, |this, cx| this.set_everywhere(everywhere, window, cx));
            })
            .child(Tab::new().label(s::SCOPE_GROUP).disabled(!has_group))
            .child(Tab::new().label(s::SCOPE_ALL));
        div()
            .flex()
            .flex_col()
            .w_full()
            .child(div().px(px(12.)).pt(px(12.)).flex().child(tabs))
            .child(
                List::new(&self.list)
                    .search_placeholder(s::PLACEHOLDER)
                    .h(px(440.)),
            )
            .child(
                div()
                    .px(px(12.))
                    .py(px(8.))
                    .flex()
                    .border_t_1()
                    .border_color(colors.border)
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(div().flex_1().child(footer))
                    .child(s::OPEN_HINT),
            )
    }
}

/// Opens the search dialog in `window`.
pub fn open(services: Services, everywhere: bool, window: &mut Window, cx: &mut App) {
    open_with_query(services, everywhere, "", window, cx);
}

/// Opens the search dialog with `query` typed in (empty for none).
pub fn open_with_query(
    services: Services,
    everywhere: bool,
    query: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let dialog = cx.new(|cx| SearchDialog::new(services, everywhere, window, cx));
    if !query.is_empty() {
        let list = dialog.read(cx).list().clone();
        list.update(cx, |state, cx| state.set_query(query, window, cx));
    }
    let first = Cell::new(true);
    window.open_dialog(cx, move |d, window, cx| {
        if first.replace(false) {
            let list = dialog.read(cx).list().clone();
            list.update(cx, |state, cx| state.focus(window, cx));
        }
        crate::ui::sheet::switcher(d)
            .close_button(false)
            .p_0()
            .child(dialog.clone())
    });
}
