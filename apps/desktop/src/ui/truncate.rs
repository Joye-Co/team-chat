//! Text that is cut with `…` keeps a way to be read (DESIGN.md §15): one
//! line, the ellipsis at the end, and the full text in a tooltip, but only
//! when the text was actually cut.

use gpui::{
    AnyView, App, AppContext as _, ElementId, InteractiveElement as _, ParentElement as _, Pixels,
    SharedString, StatefulInteractiveElement as _, Styled as _, StyledText, TextLayout, Window,
    div, prelude::FluentBuilder as _, px,
};
use gpui_component::tooltip::Tooltip;

use crate::theme::{self, Type as _};

/// The widest tooltip; longer text wraps.
const TIP_MAX: Pixels = px(360.);

/// A tooltip with the full `text`, wrapping past [`TIP_MAX`].
pub fn text_tip(text: SharedString) -> Tooltip {
    Tooltip::element(move |_, _| div().max_w(TIP_MAX).child(text.clone()))
}

/// The tooltip of a list row whose two lines may both be cut: the title,
/// and under it the second line (a preview, a context) in `text-2`.
pub fn row_tip(title: SharedString, detail: SharedString) -> Tooltip {
    Tooltip::element(move |_, cx| {
        let k = theme::colors(cx);
        div()
            .max_w(TIP_MAX)
            .flex()
            .flex_col()
            .child(title.clone())
            .when(!detail.is_empty(), |el| {
                el.child(div().text_color(k.text_2).child(detail.clone()))
            })
    })
}

/// `text` as an element whose layout tells, after it is drawn, whether its one-line box
/// cut it. Put the element in a `one_line()` box and pass the layout to [`tip_when_cut`].
pub fn cut_line(text: impl Into<SharedString>) -> (StyledText, TextLayout) {
    let styled = StyledText::new(text.into());
    let layout = styled.layout().clone();
    (styled, layout)
}

/// True when the last layout of any of `lines` cut its text.
pub fn any_cut(lines: &[TextLayout]) -> bool {
    lines.iter().any(TextLayout::truncated)
}

/// A `.tooltip(..)` builder that shows `tip` only while one of `lines` is cut (§15): a
/// row whose text fits shows nothing on hover. The vendored text layout records the cut
/// when it lays the line out, so a hover reads the state of the frame on screen.
pub fn tip_when_cut(
    lines: Vec<TextLayout>,
    tip: impl Fn() -> Tooltip + 'static,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |window, cx| {
        if any_cut(&lines) {
            tip().build(window, cx)
        } else {
            // A line shown whole needs no tooltip: an empty view draws nothing.
            cx.new(|_| gpui::Empty).into()
        }
    }
}

/// One line of `text`, cut with `…` at the end; hovering a cut line shows
/// the full text in a tooltip. Style the returned box (type, color, width):
/// the text inherits it.
pub fn truncating(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
) -> gpui::Stateful<gpui::Div> {
    let text: SharedString = text.into();
    let (styled, layout) = cut_line(text.clone());
    // The tooltip sits on the box, not on an `InteractiveText`: that element
    // measures its line differently inside a flex row and cuts it early.
    div()
        .id(id)
        .min_w_0()
        .one_line()
        .child(styled)
        .tooltip(tip_when_cut(vec![layout], move || text_tip(text.clone())))
}

#[cfg(test)]
mod tests {
    use gpui::{
        Context, IntoElement, ParentElement as _, Render, Styled as _, TestAppContext, Window, div,
        prelude::FluentBuilder as _, px, size,
    };

    use super::*;

    const NAME: &str = "Aleksandra Wiśniewska-Kowalczyk-Montgomery";
    const SHORT: &str = "产品研发";

    /// A list row as the sidebar and the topic list draw it: a fixed leading slot, a
    /// title and a detail line that each cut to one line, and a trailing badge.
    struct Row {
        width: f32,
        title: &'static str,
        detail: &'static str,
        middle: bool,
        lines: Vec<TextLayout>,
    }

    impl Render for Row {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let (title, t) = cut_line(self.title);
            let (detail, d) = cut_line(self.detail);
            self.lines = vec![t, d];
            let middle = self.middle;
            div()
                .w(px(self.width))
                .flex()
                .gap(px(10.))
                .child(div().flex_none().size(px(24.)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .min_w_0()
                                .one_line()
                                .when(middle, |el| el.text_ellipsis_middle())
                                .child(title),
                        )
                        .child(div().min_w_0().one_line().child(detail)),
                )
                .child(div().flex_none().w(px(20.)))
        }
    }

    fn cut(cx: &mut TestAppContext, width: f32, title: &'static str, middle: bool) -> bool {
        let window = cx.open_window(size(px(800.), px(100.)), |_, _| Row {
            width,
            title,
            detail: SHORT,
            middle,
            lines: Vec::new(),
        });
        cx.run_until_parked();
        window.update(cx, |row, _, _| any_cut(&row.lines)).unwrap()
    }

    #[gpui::test]
    fn a_cut_name_offers_the_tooltip(cx: &mut TestAppContext) {
        assert!(cut(cx, 120., NAME, false), "a 120 pt row cuts the name");
    }

    #[gpui::test]
    fn a_name_that_fits_has_no_tooltip(cx: &mut TestAppContext) {
        assert!(
            !cut(cx, 780., NAME, false),
            "a 780 pt row shows the whole name"
        );
        assert!(
            !cut(cx, 200., SHORT, false),
            "a short name fits a narrow row"
        );
    }

    #[gpui::test]
    fn a_middle_cut_counts_as_cut(cx: &mut TestAppContext) {
        assert!(cut(cx, 120., NAME, true));
        assert!(!cut(cx, 780., NAME, true));
    }

    #[gpui::test]
    fn the_state_follows_the_width(cx: &mut TestAppContext) {
        let window = cx.open_window(size(px(800.), px(100.)), |_, _| Row {
            width: 120.,
            title: NAME,
            detail: SHORT,
            middle: false,
            lines: Vec::new(),
        });
        cx.run_until_parked();
        assert!(window.update(cx, |row, _, _| any_cut(&row.lines)).unwrap());
        window
            .update(cx, |row, _, cx| {
                row.width = 780.;
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        assert!(!window.update(cx, |row, _, _| any_cut(&row.lines)).unwrap());
    }

    #[test]
    fn a_line_not_yet_laid_out_is_not_cut() {
        let (_, layout) = cut_line(NAME);
        assert!(!any_cut(&[layout]));
    }
}
