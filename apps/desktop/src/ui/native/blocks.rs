//! Containers and rows (COMPLETENESS.md §2.3–§2.7, K06, K10): Card,
//! SectionHeader, GroupLabel, the ListRow with its frame, the Table (on
//! gpui-component's `Table`), the DescriptionList row and the hairline.

use crate::ui::truncate::TruncText;
use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, ParentElement,
    Pixels, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};

use super::focus::FocusRow as _;
use super::tokens::{
    NativeType as _, avatar, height, metrics, native_colors, radius, shadow, space,
};
use crate::theme::MEDIUM_WEIGHT;

type Click = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

// ---------------------------------------------------------------------------
// Hairline

/// A 1 px separator in the border token (§1.9), horizontal or vertical.
pub fn separator(vertical: bool, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    let d = div().flex_none().bg(k.separator);
    if vertical {
        d.w(px(1.)).h_full()
    } else {
        d.h(px(1.)).w_full()
    }
}

// ---------------------------------------------------------------------------
// Card (§2.4)

/// A card (§2.4): flex-col, gap 24, py 24, radius 14, a 1 px border, the
/// card fill, shadow sm. Header px 24 (title 16 semibold, description 14
/// muted, an action at the top right); content px 24; footer px 24, gap 8.
/// The compact card (in a thread, dense data): gap 16, p 16, title 14.
#[derive(IntoElement)]
pub struct Card {
    title: Option<SharedString>,
    description: Option<SharedString>,
    action: Option<AnyElement>,
    lead: Option<AnyElement>,
    content: Vec<AnyElement>,
    footer: Option<AnyElement>,
    footer_border: bool,
    compact: bool,
    width: Option<Pixels>,
}

impl Card {
    pub fn new() -> Self {
        Self {
            title: None,
            description: None,
            action: None,
            lead: None,
            content: Vec::new(),
            footer: None,
            footer_border: false,
            compact: false,
            width: None,
        }
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn description(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self
    }

    /// The action slot at the top right (a badge, a segmented, "⋯").
    pub fn action(mut self, el: impl IntoElement) -> Self {
        self.action = Some(el.into_any_element());
        self
    }

    /// An element before the title (a StatusBadge, an avatar).
    pub fn lead(mut self, el: impl IntoElement) -> Self {
        self.lead = Some(el.into_any_element());
        self
    }

    pub fn footer(mut self, el: impl IntoElement) -> Self {
        self.footer = Some(el.into_any_element());
        self
    }

    /// A border-t above the footer (pt 24).
    pub fn footer_border(mut self) -> Self {
        self.footer_border = true;
        self
    }

    pub fn compact(mut self) -> Self {
        self.compact = true;
        self
    }

    pub fn width(mut self, w: Pixels) -> Self {
        self.width = Some(w);
        self
    }
}

impl Default for Card {
    fn default() -> Self {
        Self::new()
    }
}

impl ParentElement for Card {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.content.extend(elements);
    }
}

/// The card surface alone (§2.4): radius 14, the card fill, shadow sm and
/// a foreground ring at 6 %: a soft edge, not a drawn line (owner decision,
/// v18: borders only where they separate something).
pub fn card_surface(cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    let mut s = shadow::sm();
    s.push(shadow::line(k.label.opacity(0.06)));
    div().rounded(radius::CARD).bg(k.sheet).shadow(s)
}

impl RenderOnce for Card {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        let (gap, pad) = if self.compact {
            (space::S16, space::S16)
        } else {
            (space::S24, space::S24)
        };
        let has_header =
            self.title.is_some() || self.description.is_some() || self.action.is_some();
        let header = has_header.then(|| {
            div()
                .px(pad)
                .flex()
                .items_start()
                .gap(space::S8)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .flex()
                        .flex_col()
                        .gap(space::S8)
                        .when(self.title.is_some() || self.lead.is_some(), |d| {
                            d.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(space::S8)
                                    .children(self.lead)
                                    .children(self.title.map(|t| {
                                        div()
                                            .min_w(px(0.))
                                            .map(|d| {
                                                if self.compact {
                                                    d.n_headline()
                                                } else {
                                                    d.n_title3()
                                                }
                                            })
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(k.label)
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .child(t)
                                    })),
                            )
                        })
                        .children(
                            self.description
                                .map(|d| div().n_body().text_color(k.secondary).child(d)),
                        ),
                )
                .children(self.action.map(|a| div().flex_none().child(a)))
        });
        let has_content = !self.content.is_empty();
        card_surface(cx)
            .when_some(self.width, |d, w| d.w(w))
            .flex()
            .flex_col()
            .gap(gap)
            .py(pad)
            .children(header)
            .when(has_content, |d| {
                d.child(div().px(pad).flex().flex_col().children(self.content))
            })
            .children(self.footer.map(|f| {
                div()
                    .px(pad)
                    .when(self.footer_border, |d| {
                        d.pt(pad).border_t_1().border_color(k.separator)
                    })
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .child(f)
            }))
    }
}

// ---------------------------------------------------------------------------
// Section header and group label (§2.3)

/// A section header (§2.3): title 16 semibold, description 14 muted, gap 4;
/// an optional count pill after the title and an action at the right,
/// aligned to the top of the title. 12 above its group.
#[derive(IntoElement)]
pub struct SectionTitle {
    title: SharedString,
    description: Option<SharedString>,
    count: Option<u32>,
    action: Option<AnyElement>,
}

impl SectionTitle {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            description: None,
            count: None,
            action: None,
        }
    }

    pub fn description(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self
    }

    pub fn count(mut self, n: u32) -> Self {
        self.count = Some(n);
        self
    }

    pub fn action(mut self, el: impl IntoElement) -> Self {
        self.action = Some(el.into_any_element());
        self
    }
}

impl RenderOnce for SectionTitle {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        div()
            .w_full()
            .flex()
            .items_start()
            .gap(space::S8)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(space::S4)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space::S8)
                            .child(
                                div()
                                    .n_title3()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(k.label)
                                    .child(self.title),
                            )
                            .children(self.count.map(super::badge::CountPill::new)),
                    )
                    .children(
                        self.description
                            .map(|d| div().n_body().text_color(k.secondary).child(d)),
                    ),
            )
            .children(self.action.map(|a| div().flex_none().child(a)))
    }
}

/// A group label (§2.3): h32, px 8, 12 medium muted. The sidebar, menus,
/// the palette and the inspector use it instead of a section header.
pub fn group_label(label: impl Into<SharedString>, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .flex_none()
        .h(height::ROW)
        .px(space::S8)
        .flex()
        .items_center()
        .gap(space::S8)
        .n_section()
        .text_color(k.secondary)
        .child(TruncText::new(label))
}

// ---------------------------------------------------------------------------
// List row and frame (§2.6)

/// A list row (§2.6): h56 (py 12, px 12), gap 12: a 32 media, the title 14
/// medium over a description 14 muted (or meta 12 muted), then the right
/// cluster in a fixed order: status, time (12 muted tabular), actions.
#[derive(IntoElement)]
pub struct ListRow {
    id: ElementId,
    media: Option<AnyElement>,
    title: SharedString,
    title_lead: Option<AnyElement>,
    description: Option<SharedString>,
    meta: bool,
    status: Option<AnyElement>,
    time: Option<SharedString>,
    time_width: Option<Pixels>,
    actions: Option<AnyElement>,
    selected: bool,
    on_click: Option<Click>,
}

impl ListRow {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            media: None,
            title: title.into(),
            title_lead: None,
            description: None,
            meta: false,
            status: None,
            time: None,
            time_width: None,
            actions: None,
            selected: false,
            on_click: None,
        }
    }

    /// The 32 media (an avatar, or [`media_box`]).
    pub fn media(mut self, el: impl IntoElement) -> Self {
        self.media = Some(el.into_any_element());
        self
    }

    /// A mark before the title (the unread dot).
    pub fn title_lead(mut self, el: impl IntoElement) -> Self {
        self.title_lead = Some(el.into_any_element());
        self
    }

    pub fn description(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self
    }

    /// The second line as 12 muted meta instead of a 14 description.
    pub fn meta(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self.meta = true;
        self
    }

    pub fn status(mut self, el: impl IntoElement) -> Self {
        self.status = Some(el.into_any_element());
        self
    }

    pub fn time(mut self, text: impl Into<SharedString>) -> Self {
        self.time = Some(text.into());
        self
    }

    /// The fixed width of the time column, so times line up in a list.
    pub fn time_width(mut self, w: Pixels) -> Self {
        self.time_width = Some(w);
        self
    }

    /// The actions column (sm buttons, primary at the right edge, or "⋯").
    pub fn actions(mut self, el: impl IntoElement) -> Self {
        self.actions = Some(el.into_any_element());
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

/// The 32 media box of a list row (§2.6): radius 6, a 1 px border, the
/// `muted` fill, a 16 icon.
pub fn media_box(sym: crate::ui::icons::Sym, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .flex_none()
        .size(avatar::MD)
        .rounded(radius::ITEM)
        .border_1()
        .border_color(k.separator)
        .bg(k.fill)
        .flex()
        .items_center()
        .justify_center()
        .child(crate::ui::icons::icon(
            sym,
            super::tokens::icon::MD,
            k.label,
        ))
}

impl RenderOnce for ListRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = native_colors(cx);
        let clickable = self.on_click.is_some();
        div()
            .id(self.id)
            .flex_none()
            .w_full()
            .h(height::LIST_ROW)
            .px(space::S12)
            .flex()
            .items_center()
            .gap(space::S12)
            .when(self.selected, |d| d.bg(k.selection))
            .when(clickable, |d| d.focus_row(self.selected, cx))
            .children(self.media)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(space::S8)
                            .child(TruncText::new(self.title).n_headline().text_color(k.label))
                            .children(self.title_lead),
                    )
                    .children(self.description.map(|d| {
                        div()
                            .map(|e| {
                                if self.meta {
                                    e.n_footnote()
                                } else {
                                    e.n_body()
                                }
                            })
                            .text_color(k.secondary)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(d)
                    })),
            )
            .children(self.status.map(|s| div().flex_none().child(s)))
            .children(self.time.map(|t| {
                div()
                    .flex_none()
                    .when_some(self.time_width, |d, w| d.w(w))
                    .flex()
                    .justify_end()
                    .n_footnote()
                    .n_figures()
                    .text_color(k.secondary)
                    .child(t)
            }))
            .children(self.actions.map(|a| {
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .child(a)
            }))
            .when_some(self.on_click, |d, f| {
                d.on_click(move |e, w, cx| f(e, w, cx))
            })
    }
}

/// A framed list (§2.6): radius 10, a 1 px border; rows separated by a
/// 1 px line inset to the title x, none after the last row; an optional
/// footer row h40 ("看全部 23 个话题 →" or "共 12 个").
pub fn list_frame(
    rows: impl IntoIterator<Item = AnyElement>,
    inset: Pixels,
    footer: Option<AnyElement>,
    cx: &App,
) -> gpui::Div {
    let k = native_colors(cx);
    let mut frame = div()
        .w_full()
        .rounded(radius::CONTAINER)
        .border_1()
        .border_color(k.separator)
        .bg(k.content)
        .overflow_hidden()
        .flex()
        .flex_col();
    for (ix, row) in rows.into_iter().enumerate() {
        if ix > 0 {
            frame = frame.child(div().ml(inset).h(px(1.)).bg(k.separator));
        }
        frame = frame.child(row);
    }
    frame.children(footer.map(|f| {
        div()
            .h(height::FOOTER_ROW)
            .px(space::S12)
            .border_t_1()
            .border_color(k.separator)
            .flex()
            .items_center()
            .n_body()
            .text_color(k.secondary)
            .child(f)
    }))
}

/// The inset of a row separator in a list of media rows: px 12 + media 32
/// + gap 12.
pub const LIST_TITLE_INSET: Pixels = px(56.);

// ---------------------------------------------------------------------------
// Table (§2.7)

/// A table column: its header, alignment and width (`None` = flexible).
#[derive(Clone, Debug)]
pub struct Column {
    pub header: SharedString,
    pub right: bool,
    pub width: Option<Pixels>,
}

impl Column {
    pub fn flex(header: impl Into<SharedString>) -> Self {
        Self {
            header: header.into(),
            right: false,
            width: None,
        }
    }

    pub fn fixed(header: impl Into<SharedString>, width: Pixels) -> Self {
        Self {
            header: header.into(),
            right: false,
            width: Some(width),
        }
    }

    /// Right-aligned (numbers, times), header included.
    pub fn right(mut self) -> Self {
        self.right = true;
        self
    }
}

/// The table of §2.7 on gpui-component's `Table`: a frame radius 10 with a
/// 1 px border, header row h40 (14 medium, or 12 medium muted when
/// `compact`), rows h48 (h40 compact), a line between rows, cells px 12
/// (first and last px 16), one line each. Empty: one row "没有结果" h96.
pub fn table(
    columns: &[Column],
    rows: Vec<Vec<AnyElement>>,
    compact: bool,
    empty: impl Into<SharedString>,
    footer: Option<AnyElement>,
    cx: &App,
) -> gpui::Div {
    use gpui_component::table::{Table, TableBody, TableCell, TableHead, TableHeader, TableRow};
    let k = native_colors(cx);
    let n = columns.len();
    let pad = |ix: usize| {
        if ix == 0 || ix + 1 == n {
            space::S16
        } else {
            space::S12
        }
    };
    let row_h = if compact {
        height::TABLE_ROW_COMPACT
    } else {
        height::TABLE_ROW
    };
    let mut head = TableRow::new()
        .h(height::TABLE_HEAD)
        .border_b_1()
        .border_color(k.separator);
    for (ix, c) in columns.iter().enumerate() {
        let mut h = TableHead::new()
            .px(pad(ix))
            .map(|h| {
                if compact {
                    h.n_section().text_color(k.secondary)
                } else {
                    h.n_headline().text_color(k.label)
                }
            })
            .child(c.header.clone());
        h = match c.width {
            Some(w) => h.w(w).flex_none(),
            None => h.flex_1().min_w(px(0.)),
        };
        if c.right {
            h = h.text_right();
        }
        head = head.child(h);
    }
    let mut body = TableBody::new();
    let count = rows.len();
    if count == 0 {
        body = body.child(
            TableRow::new().child(
                TableCell::new()
                    .col_span(n.max(1))
                    .w_full()
                    .h(px(96.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .n_body()
                    .text_color(k.secondary)
                    .child(empty.into()),
            ),
        );
    }
    for (rix, cells) in rows.into_iter().enumerate() {
        let mut row = TableRow::new().h(row_h).when(rix + 1 < count, |r| {
            r.border_b_1().border_color(k.separator)
        });
        for (ix, cell) in cells.into_iter().enumerate() {
            let c = &columns[ix.min(n.saturating_sub(1))];
            let mut td = TableCell::new()
                .px(pad(ix))
                .n_body()
                .text_color(k.label)
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(cell);
            td = match c.width {
                Some(w) => td.w(w).flex_none(),
                None => td.flex_1().min_w(px(0.)),
            };
            if c.right {
                td = td.text_right().n_figures();
            }
            row = row.child(td);
        }
        body = body.child(row);
    }
    div()
        .w_full()
        .rounded(radius::CONTAINER)
        .border_1()
        .border_color(k.separator)
        .bg(k.content)
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(
            Table::new()
                .child(TableHeader::new().child(head))
                .child(body),
        )
        .children(footer.map(|f| {
            div()
                .h(height::TABLE_ROW)
                .px(space::S16)
                .border_t_1()
                .border_color(k.separator)
                .flex()
                .items_center()
                .gap(space::S32)
                .n_body()
                .text_color(k.secondary)
                .child(f)
        }))
}

// ---------------------------------------------------------------------------
// Description list (§1.9, §2.5)

/// One DescriptionList row (§2.5): the label 14 muted in a 112 column, the
/// value 14 foreground, left-aligned, one line.
pub fn description_row(
    label: impl Into<SharedString>,
    value: impl IntoElement,
    cx: &App,
) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .w_full()
        .min_h(height::ROW)
        .flex()
        .items_center()
        .gap(space::S8)
        .child(
            TruncText::new(label)
                .flex_none()
                .w(metrics::LABEL_COLUMN)
                .n_body()
                .text_color(k.secondary),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .n_body()
                .text_color(k.label)
                .child(value),
        )
}

/// A selected list or menu row weight (D10): font-medium.
pub fn selected_weight() -> gpui::FontWeight {
    MEDIUM_WEIGHT
}
