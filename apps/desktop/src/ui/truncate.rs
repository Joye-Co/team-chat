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

/// `text` for one line: a text with a line break or a tab (a 话题 title typed
/// on two lines) has every run of white space collapsed to one space, so it
/// never draws a second line over the next row (§2.21). Other text is kept.
pub fn single_line(text: SharedString) -> SharedString {
    if text.contains(['\n', '\r', '\t', '\u{2028}', '\u{2029}']) {
        text.split_whitespace().collect::<Vec<_>>().join(" ").into()
    } else {
        text
    }
}

/// `text` as an element whose layout tells, after it is drawn, whether its one-line box
/// cut it. Put the element in a `one_line()` box and pass the layout to [`tip_when_cut`].
/// Line breaks in `text` become spaces ([`single_line`]).
pub fn cut_line(text: impl Into<SharedString>) -> (StyledText, TextLayout) {
    let styled = StyledText::new(single_line(text.into()));
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

/// Where [`TruncText`] cuts a line that does not fit (COMPLETENESS.md §2.21).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cut {
    /// `…` at the end: names, titles, labels (the default).
    #[default]
    End,
    /// `…` in the middle, so the start and the end stay: file names,
    /// emails, IDs and paths.
    Middle,
}

/// The one truncating label of the kit (T1-02, §2.21): one line, cut with
/// `…` (at the end, or in the middle for file names, emails, IDs and
/// paths), and the full text in a tooltip only when the line was cut. Style
/// it like a `div` (type, color, width); the text inherits the style.
///
/// The element id comes from the text unless [`TruncText::id`] sets one;
/// give two equal labels in one parent their own ids.
pub struct TruncText {
    base: gpui::Div,
    id: ElementId,
    text: SharedString,
    tip: Option<SharedString>,
    cut: Cut,
}

impl TruncText {
    pub fn new(text: impl Into<SharedString>) -> Self {
        let text: SharedString = text.into();
        let id = ElementId::Name(format!("trunc-{}", hash(&text)).into());
        Self {
            base: div(),
            id,
            text,
            tip: None,
            cut: Cut::End,
        }
    }

    /// The element id (two equal labels in one parent need their own).
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    /// Cut in the middle: file names, emails, IDs and paths.
    pub fn middle(mut self) -> Self {
        self.cut = Cut::Middle;
        self
    }

    pub fn cut(mut self, cut: Cut) -> Self {
        self.cut = cut;
        self
    }

    /// The tooltip text, when it is not the label itself (a role after a
    /// name, a full path).
    pub fn tip(mut self, tip: impl Into<SharedString>) -> Self {
        self.tip = Some(tip.into());
        self
    }
}

/// A file name on one line (§2.21, F3; review 2 R1-06): cut in the middle so
/// the start and the extension stay (`IMG_2025…_HDR.jpg`), the whole name in
/// a tooltip when cut. Every attachment, upload, download and media title
/// uses it; style it like a `div` (type, color, width).
pub fn file_name(name: impl Into<SharedString>) -> TruncText {
    TruncText::new(name).middle()
}

fn hash(text: &str) -> u64 {
    use std::hash::{Hash as _, Hasher as _};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

impl gpui::Styled for TruncText {
    fn style(&mut self) -> &mut gpui::StyleRefinement {
        gpui::Styled::style(&mut self.base)
    }
}

impl gpui::IntoElement for TruncText {
    type Element = gpui::Stateful<gpui::Div>;

    fn into_element(self) -> Self::Element {
        let (styled, layout) = cut_line(self.text.clone());
        let tip = self.tip.unwrap_or(self.text);
        let base = self
            .base
            .id(self.id)
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap();
        let base = match self.cut {
            Cut::End => base.text_ellipsis(),
            Cut::Middle => base.text_ellipsis_middle(),
        };
        base.child(styled)
            .tooltip(tip_when_cut(vec![layout], move || text_tip(tip.clone())))
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Context, IntoElement, Render, TestAppContext, Window, div, px, size};

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

    #[test]
    fn a_line_break_becomes_one_space() {
        assert_eq!(
            single_line("第一行\n第二行".into()).as_ref(),
            "第一行 第二行"
        );
        assert_eq!(single_line(" a \r\n\t b ".into()).as_ref(), "a b");
        // Text with no break keeps its spaces.
        assert_eq!(single_line("  Sam   Lee ".into()).as_ref(), "  Sam   Lee ");
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

    /// `TruncText` records its cut like `cut_line` (T1-02).
    struct Label {
        width: f32,
        middle: bool,
    }

    impl Render for Label {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let t = TruncText::new(NAME).w(px(self.width));
            div().child(if self.middle { t.middle() } else { t })
        }
    }

    #[gpui::test]
    fn trunc_text_draws_in_both_cuts(cx: &mut TestAppContext) {
        for middle in [false, true] {
            let window = cx.open_window(size(px(800.), px(100.)), |_, _| Label {
                width: 120.,
                middle,
            });
            cx.run_until_parked();
            window.update(cx, |_, _, _| ()).unwrap();
        }
        assert_ne!(hash("a"), hash("b"));
    }

    /// R1-06: a file name cut in the middle keeps its start and its
    /// extension, and the kept parts are plain prefix and suffix (no `_`,
    /// `-` or `.` dropped before the `…`).
    #[gpui::test]
    fn a_file_name_keeps_its_extension(cx: &mut TestAppContext) {
        let name = "IMG_20250914_183022_HDR_final-v2.jpg";
        let font = gpui::font(".SystemUIFont");
        let runs = [gpui::TextRun {
            len: name.len(),
            font: font.clone(),
            ..Default::default()
        }];
        let text_system = cx.text_system().clone();
        let mut wrapper = text_system.line_wrapper(font, px(14.));
        let (cut, _) = wrapper.truncate_line(
            name.into(),
            px(140.),
            "…",
            &runs,
            gpui::TruncateFrom::Middle,
        );
        let (front, back) = cut.split_once('…').expect("cut in the middle");
        assert!(back.ends_with(".jpg"), "{cut}");
        assert!(name.starts_with(front) && !front.is_empty(), "{cut}");
        assert!(name.ends_with(back), "{cut}");
    }

    #[test]
    fn a_line_not_yet_laid_out_is_not_cut() {
        let (_, layout) = cut_line(NAME);
        assert!(!any_cut(&[layout]));
    }
}
