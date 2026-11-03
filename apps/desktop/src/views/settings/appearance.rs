//! 设置 → 外观 (1.1.0): 主题 (跟随系统, 浅色, 深色) and 侧栏样式 (rail modes
//! A, B, C). Both are kept on this Mac in `layout.json`.
//!
//! Foundation stub: the choices work and are saved; the appearance track
//! gives the page its artboard look (rail previews per option).

use gpui::{
    AnyWindowHandle, App, Context, IntoElement, ParentElement, Pixels, Render, SharedString, Size,
    StatefulInteractiveElement as _, Styled, Window, div, prelude::FluentBuilder as _, px,
};

use super::Page;
use crate::{
    app::{RailMode, Services},
    copy::appearance as c,
    theme::{self, ThemeChoice, Type as _, space},
    ui::{self, Lucide, RowSurface},
    views::shell::{layout, section_label},
};

pub struct AppearancePage {
    services: Services,
    /// Save choices to `layout.json` (the app, never a self-test scene).
    persist: bool,
}

impl AppearancePage {
    pub fn new(services: Services, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self {
            services,
            persist: true,
        }
    }

    fn option(
        &self,
        id: String,
        title: &'static str,
        help: Option<&'static str>,
        on: bool,
        cx: &App,
    ) -> gpui::Stateful<gpui::Div> {
        let k = theme::colors(cx);
        ui::list_row(SharedString::from(id), RowSurface::Content, false, cx)
            .min_h(theme::size::OPTION_ROW)
            .px(space::S12)
            .py(space::S8)
            .gap(space::S10)
            .child(div().flex_none().size(px(16.)).when(on, |el| {
                el.child(ui::icon(Lucide::Check, theme::icon::ROW, k.ink))
            }))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .t_ui()
                            .text_color(k.ink)
                            .when(on, |el| el.medium())
                            .child(title),
                    )
                    .children(help.map(|h| div().t_meta().text_color(k.ink_3).child(h))),
            )
    }
}

impl Render for AppearancePage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current_theme = theme::theme_choice();
        let current_rail = self.services.nav.read(cx).rail_mode;
        let themes = ThemeChoice::ALL.into_iter().map(|choice| {
            let title = match choice {
                ThemeChoice::System => c::APPEARANCE_THEME_SYSTEM,
                ThemeChoice::Light => c::APPEARANCE_THEME_LIGHT,
                ThemeChoice::Dark => c::APPEARANCE_THEME_DARK,
            };
            let persist = self.persist;
            self.option(
                format!("appearance-theme-{}", choice.slug()),
                title,
                None,
                choice == current_theme,
                cx,
            )
            .on_click(move |_, window, cx| {
                theme::set_theme_choice(choice);
                if persist {
                    layout::save_theme(choice);
                }
                theme::apply_theme_choice(window, cx);
            })
        });
        let rails = RailMode::ALL.into_iter().map(|mode| {
            let (title, help) = match mode {
                RailMode::Work => (c::APPEARANCE_RAIL_WORK, c::APPEARANCE_RAIL_WORK_HELP),
                RailMode::Attention => (
                    c::APPEARANCE_RAIL_ATTENTION,
                    c::APPEARANCE_RAIL_ATTENTION_HELP,
                ),
                RailMode::Member => (c::APPEARANCE_RAIL_MEMBER, c::APPEARANCE_RAIL_MEMBER_HELP),
            };
            let nav = self.services.nav.clone();
            let persist = self.persist;
            self.option(
                format!("appearance-rail-{}", mode.slug()),
                title,
                Some(help),
                mode == current_rail,
                cx,
            )
            .on_click(move |_, _, cx| {
                nav.update(cx, |nav, cx| nav.set_rail_mode(mode, cx));
                if persist {
                    layout::save_rail_mode(mode);
                }
            })
        });
        div()
            .flex()
            .flex_col()
            .gap(space::S8)
            .child(section_label(c::APPEARANCE_THEME, None, cx))
            .children(themes)
            .child(div().h(space::S16))
            .child(section_label(c::APPEARANCE_RAIL, None, cx))
            .children(rails)
    }
}

/// The `settings-appearance` scene: the settings window on 外观.
pub fn open_scene(
    services: Services,
    _content: Size<Pixels>,
    cx: &mut App,
) -> Result<AnyWindowHandle, String> {
    super::open_window(services, Page::Appearance, false, cx).map(|(h, _)| h)
}
