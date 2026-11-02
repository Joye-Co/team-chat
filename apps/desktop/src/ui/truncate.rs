//! Text that is cut with `…` keeps a way to be read (DESIGN.md §15): one
//! line, the ellipsis at the end, and the full text in a tooltip, but only
//! when the text was actually cut.

use gpui::{
    AppContext as _, ElementId, InteractiveElement as _, ParentElement as _, Pixels, SharedString,
    StatefulInteractiveElement as _, Styled as _, StyledText, TextLayout, div,
    prelude::FluentBuilder as _, px,
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

/// One line of `text`, cut with `…` at the end; hovering a cut line shows
/// the full text in a tooltip. Style the returned box (type, color, width):
/// the text inherits it.
pub fn truncating(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
) -> gpui::Stateful<gpui::Div> {
    let text: SharedString = text.into();
    let styled = StyledText::new(text.clone());
    let layout = styled.layout().clone();
    // The tooltip sits on the box, not on an `InteractiveText`: that element
    // measures its line differently inside a flex row and cuts it early.
    div()
        .id(id)
        .min_w_0()
        .one_line()
        .child(styled)
        .tooltip(move |window, cx| {
            if was_cut(&layout, &text) {
                text_tip(text.clone()).build(window, cx)
            } else {
                // A line shown whole needs no tooltip: an empty view draws
                // nothing.
                cx.new(|_| gpui::Empty).into()
            }
        })
}

/// True when the laid-out line is not the whole text. The tooltip asks only
/// after a hover, so the line has been laid out by then.
fn was_cut(layout: &TextLayout, full: &str) -> bool {
    cut_text_differs(&layout.text(), full)
}

fn cut_text_differs(shown: &str, full: &str) -> bool {
    shown != full
}

#[cfg(test)]
mod tests {
    use gpui::{Context, IntoElement, Render, TestAppContext, Window, div, px, size};

    use super::*;

    struct Host {
        width: f32,
        layout: Option<TextLayout>,
    }

    const NAME: &str = "Aleksandra Wiśniewska-Kowalczyk-Montgomery";

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let styled = StyledText::new(NAME);
            self.layout = Some(styled.layout().clone());
            div()
                .w(px(self.width))
                .child(div().min_w_0().one_line().child(styled))
        }
    }

    fn shown(cx: &mut TestAppContext, width: f32) -> bool {
        let window = cx.open_window(size(px(800.), px(100.)), |_, _| Host {
            width,
            layout: None,
        });
        cx.run_until_parked();
        window
            .update(cx, |host, _, _| {
                was_cut(host.layout.as_ref().unwrap(), NAME)
            })
            .unwrap()
    }

    #[gpui::test]
    fn a_cut_line_offers_the_tooltip(cx: &mut TestAppContext) {
        assert!(shown(cx, 60.), "a 60 pt box cuts the name");
    }

    #[gpui::test]
    fn a_whole_line_has_no_tooltip(cx: &mut TestAppContext) {
        assert!(!shown(cx, 780.), "a 780 pt box shows the whole name");
    }

    #[test]
    fn compares_the_shown_text() {
        assert!(cut_text_differs("Aleksandra…", NAME));
        assert!(!cut_text_differs(NAME, NAME));
    }
}
