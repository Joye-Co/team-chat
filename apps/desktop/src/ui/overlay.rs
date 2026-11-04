//! Floating surfaces (DESIGN.md §5, §7.12, §8.6): popovers, menus and their
//! rows, the @mention popup, the hover toolbar, toasts and banners.

use std::rc::Rc;

use gpui::{
    AnyElement, App, ElementId, InteractiveElement as _, IntoElement, ParentElement, RenderOnce,
    SharedString, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{WindowExt as _, notification::Notification};

use crate::{
    theme::{self, Type as _, size, space},
    ui::icons::{Sym, icon},
    ui::native::NativeType as _,
};

/// A level-2 overlay surface: `overlay` fill, radius 12, the overlay shadow
/// (a visible 1 pt edge in dark mode). Popovers, menus, the switcher,
/// dialogs and toasts use it.
pub fn overlay_surface(cx: &App) -> gpui::Div {
    // COMPLETENESS.md §1.9 float surface: opaque `card`, radius 8, a 1 px
    // border and shadow md.
    let n = crate::ui::native::native_colors(cx);
    div()
        .bg(n.menu)
        .text_color(n.label)
        .rounded(crate::ui::native::metrics::MENU_RADIUS)
        .border_1()
        .border_color(n.separator)
        .shadow(crate::ui::native::tokens::shadow::md())
}

/// A menu surface: the float surface with p 4 (§2.11).
pub fn menu_surface(cx: &App) -> gpui::Div {
    overlay_surface(cx)
        .min_w(px(128.))
        .p(crate::ui::native::metrics::MENU_PAD)
        .flex()
        .flex_col()
}

/// The @mention popup (§2.11): a menu, w 320, max 8 rows.
pub fn mention_surface(cx: &App) -> gpui::Div {
    overlay_surface(cx)
        .w(size::MENTION_POPUP_WIDTH)
        .max_h(size::MENU_ROW * 8. + crate::ui::native::metrics::MENU_PAD * 2.)
        .p(crate::ui::native::metrics::MENU_PAD)
        .flex()
        .flex_col()
}

/// A small floating capsule (the hover toolbar): the float surface, p 2.
pub fn float_surface(cx: &App) -> gpui::Div {
    overlay_surface(cx).p(space::S2).flex().items_center()
}

/// A menu row (§7.12): 32 pt, radius 6, padding 0 8, gap 8, a 16 pt icon in
/// `text-2` and `ui` 14 text. `active` is the keyboard-active row.
pub fn menu_row(
    id: impl Into<ElementId>,
    glyph: Option<Sym>,
    label: impl Into<SharedString>,
    active: bool,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    // §2.11: h32, px 8, radius 6, 14 text, a 16 icon, gap 8; the active row
    // is the `accent` fill (D10).
    let n = crate::ui::native::native_colors(cx);
    popup_row(id, active, cx)
        .when_some(glyph, |el, g| {
            el.child(icon(g, crate::ui::native::tokens::icon::MD, n.secondary))
        })
        .child(div().flex_1().min_w(px(0.)).one_line().child(label.into()))
}

/// A menu separator (§2.11): 1 px, edge to edge, my 4.
pub fn menu_separator(cx: &App) -> gpui::Div {
    crate::ui::native::menu_separator(false, cx)
}

/// A row of a pop-up list (mention popup, pickers): the menu item of §2.11.
pub fn popup_row(id: impl Into<ElementId>, active: bool, cx: &App) -> gpui::Stateful<gpui::Div> {
    let n = crate::ui::native::native_colors(cx);
    div()
        .id(id)
        .flex_none()
        .w_full()
        .h(size::MENU_ROW)
        .px(crate::ui::native::metrics::MENU_ROW_PAD)
        .gap(space::S8)
        .rounded(crate::ui::native::metrics::MENU_ROW_RADIUS)
        .flex()
        .items_center()
        .t_ui()
        .text_color(n.label)
        .when(active, |el| el.bg(n.fill_hover))
        .when(!active, |el| el.hover(|s| s.bg(n.fill_hover)))
}

/// The return glyph at the end of the active popup row.
pub fn return_hint(cx: &App) -> impl IntoElement {
    icon(Sym::Return, theme::icon::CHIP, theme::colors(cx).text_3)
}

/// The tone of a toast or banner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Success,
    Danger,
}

/// How far above the window's bottom edge pushed toasts stop: 0, or the
/// height of a docked element they must not cover (the focused composer).
/// Kept in pixels × 100.
static TOAST_FLOOR: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The bottom margin of pushed toasts: 16 above the window edge, or 16
/// above the floor that [`set_toast_floor`] set.
pub fn toast_bottom() -> gpui::Pixels {
    let floor = TOAST_FLOOR.load(std::sync::atomic::Ordering::Relaxed) as f32 / 100.;
    space::S16 + gpui::px(floor)
}

/// Lifts pushed toasts so they end above a docked element `height` tall
/// at the window's bottom (§2.16, review 2 R1-15): the composer calls it
/// with its height while it has the focus, and with 0 when it loses it, so
/// a toast never covers Send. Applies at once to the toasts on screen.
pub fn set_toast_floor(height: gpui::Pixels, cx: &mut App) {
    let value = (f32::from(height).max(0.) * 100.).round() as u32;
    let old = TOAST_FLOOR.swap(value, std::sync::atomic::Ordering::Relaxed);
    if old != value {
        gpui_component::Theme::global_mut(cx)
            .notification
            .margins
            .bottom = toast_bottom();
        cx.refresh_windows();
    }
}

/// The toast (COMPLETENESS.md §2.16, T1-06): float surface, radius 10, a
/// 1 px border, shadow md, p 16, w 360; a 16 icon (the only part in a state
/// color), title 14 medium, description 14 muted, an outline sm action and
/// a 16 close. Draw it in place, or push it bottom-right with
/// [`Toast::push`].
#[derive(IntoElement)]
pub struct Toast {
    glyph: Sym,
    tone: Tone,
    title: SharedString,
    description: Option<SharedString>,
    action: Option<ToastAction>,
    /// In-place drawing only: an element action (the gallery).
    action_el: Option<AnyElement>,
    on_close: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    id: Option<SharedString>,
}

/// The action of a pushed [`Toast`]: an outline sm button.
#[derive(Clone)]
pub struct ToastAction {
    pub label: SharedString,
    pub run: Rc<dyn Fn(&mut Window, &mut App)>,
}

impl Toast {
    pub fn new(glyph: Sym, title: impl Into<SharedString>) -> Self {
        Self {
            glyph,
            tone: Tone::Neutral,
            title: title.into(),
            description: None,
            action: None,
            action_el: None,
            on_close: None,
            id: None,
        }
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    /// A 14 muted line under the title.
    pub fn description(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self
    }

    /// The action: an outline sm button that runs `run` and closes the toast.
    pub fn action_button(
        mut self,
        label: impl Into<SharedString>,
        run: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.action = Some(ToastAction {
            label: label.into(),
            run: Rc::new(run),
        });
        self
    }

    /// Any element as the action, for a toast drawn in place.
    pub fn action(mut self, el: impl IntoElement) -> Self {
        self.action_el = Some(el.into_any_element());
        self
    }

    /// What the close button does in a toast drawn in place.
    pub fn on_close(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(f));
        self
    }

    /// A toast with the same id replaces the one on screen.
    pub fn id(mut self, id: impl Into<SharedString>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// The toast as a gpui-component notification: the theme places it
    /// bottom-right, 16 from the window edges, 360 wide.
    pub fn notification(self) -> Notification {
        let Toast {
            glyph,
            tone,
            title,
            description,
            action,
            id,
            ..
        } = self;
        let mut note = Notification::new()
            .close_button(false)
            .autohide(action.is_none())
            .p(space::S16)
            .content(move |this, _window, cx| {
                let close = cx.listener(|this, _, window, cx| this.dismiss(window, cx));
                let run = action.clone().map(|a| {
                    let r = a.run.clone();
                    (
                        a.label,
                        cx.listener(move |this: &mut Notification, _, window, cx| {
                            r(window, cx);
                            this.dismiss(window, cx);
                        }),
                    )
                });
                let _ = this;
                toast_body(
                    glyph,
                    tone,
                    title.clone(),
                    description.clone(),
                    run.map(|(label, f)| {
                        crate::ui::native::push_button(
                            "toast-action",
                            label,
                            crate::ui::native::PushStyle::Outline,
                            crate::ui::native::PushSize::Small,
                            cx,
                        )
                        .on_click(f)
                        .into_any_element()
                    }),
                    close_button(cx).on_click(close).into_any_element(),
                    cx,
                )
                .into_any_element()
            });
        if let Some(id) = id {
            note = note.id1::<Toast>(id);
        }
        note
    }

    /// Pushes the toast on the active window (bottom-right, §2.16).
    pub fn push(self, cx: &mut App) {
        let note = self.notification();
        let target = cx
            .active_window()
            .or_else(|| cx.windows().into_iter().next());
        if let Some(window) = target {
            let _ = window.update(cx, |_, window, cx| window.push_notification(note, cx));
        }
    }
}

/// The 16 close of a toast: a 28 ghost icon button.
fn close_button(cx: &App) -> gpui_component::button::Button {
    crate::ui::native::icon_button_sym(
        "toast-close",
        Sym::Close,
        crate::copy::common::CLOSE,
        crate::ui::native::IconSize::Small,
        cx,
    )
}

/// The inside of a toast: icon, title over description, action, close.
fn toast_body(
    glyph: Sym,
    tone: Tone,
    title: SharedString,
    description: Option<SharedString>,
    action: Option<AnyElement>,
    close: AnyElement,
    cx: &App,
) -> gpui::Div {
    let n = crate::ui::native::native_colors(cx);
    let glyph_color = match tone {
        Tone::Neutral => n.label,
        Tone::Success => n.green,
        Tone::Danger => n.red,
    };
    div()
        .w_full()
        .flex()
        .items_start()
        .gap(space::S12)
        .child(
            // The icon centers on the title line (20 high).
            div()
                .flex_none()
                .h(px(20.))
                .flex()
                .items_center()
                .child(icon(
                    glyph,
                    crate::ui::native::tokens::icon::MD,
                    glyph_color,
                )),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(space::S2)
                .child(div().n_headline().text_color(n.label).child(title))
                .when_some(description, |d, t| {
                    d.child(div().n_body().text_color(n.secondary).child(t))
                }),
        )
        .children(action.map(|a| div().flex_none().self_center().child(a)))
        .child(div().flex_none().mt(px(-4.)).mr(px(-6.)).child(close))
}

impl RenderOnce for Toast {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        // A toast drawn in place (gallery, kit scene): the same body on the
        // toast surface.
        let t = self;
        let action = t.action_el.or_else(|| {
            t.action.map(|a| {
                let run = a.run.clone();
                crate::ui::native::push_button(
                    "toast-action",
                    a.label,
                    crate::ui::native::PushStyle::Outline,
                    crate::ui::native::PushSize::Small,
                    cx,
                )
                .on_click(move |_, w, cx| run(w, cx))
                .into_any_element()
            })
        });
        let on_close = t.on_close.clone();
        let close = close_button(cx)
            .when_some(on_close, |b, f| b.on_click(move |_, w, cx| f(w, cx)))
            .into_any_element();
        crate::ui::native::feedback::toast_surface(cx).child(toast_body(
            t.glyph,
            t.tone,
            t.title,
            t.description,
            action,
            close,
            cx,
        ))
    }
}

/// An app-level banner (update ready, offline; §8.6): a 48 pt card above the
/// sidebar footer, radius 8, `raised`, 1 pt `border`, a 16 pt glyph, 13/20
/// text and an optional `sm` action. Never in the topic column.
#[derive(IntoElement)]
pub struct Banner {
    glyph: Sym,
    glyph_color: Option<gpui::Hsla>,
    text: SharedString,
    detail: Option<SharedString>,
    action: Option<AnyElement>,
}

impl Banner {
    pub fn new(glyph: Sym, text: impl Into<SharedString>) -> Self {
        Self {
            glyph,
            glyph_color: None,
            text: text.into(),
            detail: None,
            action: None,
        }
    }

    pub fn glyph_color(mut self, color: gpui::Hsla) -> Self {
        self.glyph_color = Some(color);
        self
    }

    /// A second line in 12/16 `text-3`.
    pub fn detail(mut self, text: impl Into<SharedString>) -> Self {
        self.detail = Some(text.into());
        self
    }

    pub fn action(mut self, el: impl IntoElement) -> Self {
        self.action = Some(el.into_any_element());
        self
    }
}

impl RenderOnce for Banner {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        // COMPLETENESS.md §2.15, §2.16: an in-page banner is the Alert.
        let mut alert = crate::ui::native::feedback::Alert::new(self.glyph, self.text);
        if let Some(c) = self.glyph_color {
            alert = alert.icon_color(c);
        }
        if let Some(d) = self.detail {
            alert = alert.description(d);
        }
        if let Some(a) = self.action {
            alert = alert.action(a);
        }
        alert
    }
}

/// Tests that move the toast floor (one process-wide value) take this lock,
/// so parallel tests never see each other's floor.
#[cfg(test)]
pub(crate) static TOAST_FLOOR_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod toast_floor_tests {
    use gpui::{TestAppContext, px};

    /// R1-15: a focused composer lifts pushed toasts over itself, and they
    /// come back down when it lets go.
    #[gpui::test]
    fn the_composer_lifts_toasts(cx: &mut TestAppContext) {
        let _lock = super::TOAST_FLOOR_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        cx.update(|cx| {
            gpui_component::init(cx);
            super::set_toast_floor(px(120.), cx);
            let m = gpui_component::ActiveTheme::theme(cx)
                .notification
                .margins
                .bottom;
            assert_eq!(m, px(136.));
            super::set_toast_floor(px(0.), cx);
            let m = gpui_component::ActiveTheme::theme(cx)
                .notification
                .margins
                .bottom;
            assert_eq!(m, px(16.));
        });
    }
}
