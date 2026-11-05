//! Full views in the 产出 viewer and the 产出 pane (Media.dc.html, right column of each
//! section): the HTML page with its versions and events, the Markdown reader with a table
//! of contents, the image lightbox, the video player with chapters and a frame strip, the
//! audio player with a big waveform and transcript, the PDF reader, the whole code file
//! and the sortable table.

use std::{collections::HashSet, path::PathBuf, rc::Rc, sync::Arc};

use artifact_view::ArtifactEvent;
use gpui::{
    AnyElement, App, ElementId, Entity, ImageFormat, InteractiveElement as _, IntoElement,
    ObjectFit, ParentElement, SharedString, StatefulInteractiveElement as _, Styled,
    StyledImage as _, Window, div, img, prelude::FluentBuilder as _, px, relative, uniform_list,
};

use gpui_component::{Disableable as _, Selectable as _, button::Button};

use crate::ui::native::FocusRow as _;

use super::{
    inline::{code_line, fit, group_digits},
    parts::{self, mono, tool},
    screen,
    state::{MediaState, SPEEDS, Target},
    store::{self, Data, Media},
    text,
};
use crate::{
    copy::{fill, media as cm},
    markdown,
    theme::{self, Type as _, radius, space},
    ui::{
        self, NamedButton as _, Sym,
        web_embed::{WebEmbed, WebEmbedSpec, WebEmbedState, live},
    },
};

pub type CloseFn = Rc<dyn Fn(&mut Window, &mut App)>;

/// One row of the HTML version list.
#[derive(Clone, Debug)]
pub struct VersionRow {
    pub version: u32,
    pub when: String,
    pub note: String,
}

/// What a full view shows and how it is framed.
#[derive(Clone)]
pub struct FullSpec {
    pub media: Arc<Media>,
    pub target: Target,
    /// Newest first; empty shows only the current version.
    pub versions: Vec<VersionRow>,
    pub close: Option<CloseFn>,
    /// Keys the view's native page and state.
    pub id: SharedString,
    /// Start the video playing.
    pub autoplay: bool,
}

pub fn render(
    spec: &FullSpec,
    state: &Entity<MediaState>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let media = spec.media.clone();
    match &media.data {
        Data::Html => html(spec, state, window, cx),
        Data::Markdown(md) => markdown_view(spec, md, state, cx),
        Data::Images(pics) => lightbox(spec, pics, state, cx),
        Data::Video(v) => video(spec, v, state, cx),
        Data::Audio(a) => audio(spec, a, state, cx),
        Data::Pdf(p) => pdf(spec, p, state, cx),
        Data::Code(c) => code(spec, c, state, cx),
        Data::Table(t) => table(spec, t, state, cx),
        Data::File => file(spec, cx),
    }
}

fn id(spec: &FullSpec, part: &str) -> ElementId {
    ElementId::Name(format!("{}-{part}", spec.id).into())
}

/// What a full view's fixed header holds besides its icon and title.
#[derive(Default)]
struct Head {
    /// 12 muted after the title (kind, length, rows).
    meta: Option<SharedString>,
    /// The kind's own toggles (table of contents, wrap, page list, send), before the
    /// fixed actions.
    toggles: Vec<Button>,
    /// Copy, when the kind has something to copy.
    copy: Option<Rc<dyn Fn(&mut App)>>,
}

/// A 32 ghost header action (§2.2).
fn action(spec: &FullSpec, part: &str, icon: Sym, name: &'static str) -> Button {
    ui::native::icon_button_bare(id(spec, part), icon, ui::native::IconSize::Regular).named(name)
}

/// The fixed header of every full view (F52, §2.2): icon 16, title 16 semibold, meta
/// 12 muted, then the kind's toggles and the actions in one order: copy, download,
/// open, close. A border under it.
fn head(
    spec: &FullSpec,
    icon: Sym,
    title: impl Into<SharedString>,
    h: Head,
    cx: &App,
) -> gpui::Div {
    let k = theme::colors(cx);
    let n = ui::native::native_colors(cx);
    let save = spec.media.entry_path();
    let open = save.clone();
    // The kind's own tools sit in a second slot, left of the fixed set and a
    // separator; the fixed set is the same in every kind and state: copy
    // (disabled at 50 % when nothing copies), download, open, close (F52).
    let has_toggles = !h.toggles.is_empty();
    let mut tools = div()
        .flex_none()
        .flex()
        .items_center()
        .gap(space::S4)
        .children(h.toggles)
        .when(has_toggles, |d| d.child(tool_separator(cx)));
    tools = match h.copy {
        Some(copy) => tools
            .child(action(spec, "copy", Sym::Copy, cm::COPY).on_click(move |_, _, cx| copy(cx))),
        None => tools.child(
            action(spec, "copy", Sym::Copy, cm::COPY)
                .disabled(true)
                .opacity(0.5),
        ),
    };
    tools = tools
        .child(
            action(spec, "save", Sym::Download, cm::SAVE)
                .on_click(move |_, _, cx| parts::save_to_downloads(&[save.as_path()], cx)),
        )
        .child(
            action(spec, "sys", Sym::OpenExternal, cm::OPEN_WITH_SYSTEM)
                .on_click(move |_, _, cx| parts::open_with_system(&open, cx)),
        );
    if let Some(close) = spec.close.clone() {
        tools = tools.child(
            action(spec, "close", Sym::Close, cm::CLOSE)
                .on_click(move |_, window, cx| close(window, cx)),
        );
    }
    div()
        .flex_none()
        .h(ui::native::metrics::TOOLBAR_ROW)
        .pl(space::S16)
        .pr(space::S12)
        .flex()
        .items_center()
        .gap(space::S8)
        .border_b_1()
        .border_color(n.separator)
        .child(ui::icon(icon, px(16.), n.secondary))
        .child(
            // A file name: cut in the middle, the extension kept (§2.21, F3).
            ui::native::file_name(title)
                .flex_shrink(1.)
                .min_w(px(0.))
                .t_card_title()
                .text_color(k.text_1),
        )
        .when_some(h.meta, |d, m| {
            d.child(
                div()
                    .flex_shrink(4.)
                    .min_w(px(0.))
                    .t_meta()
                    .tnum()
                    .text_color(n.secondary)
                    .one_line()
                    .child(m),
            )
        })
        .child(div().flex_1())
        .child(tools)
}

/// The row height of the full table (compact, §2.7).
pub const TABLE_ROW: gpui::Pixels = ui::native::tokens::height::TABLE_ROW_COMPACT;
/// The height of a full view's footer: p 16 around a 16 line, plus its border.
pub const FOOT_HEIGHT: gpui::Pixels = px(49.);

/// The footer of a full view (F52): a border above, p 16, 12 muted user-facing text.
fn foot(cx: &App) -> gpui::Div {
    let n = ui::native::native_colors(cx);
    div()
        .flex_none()
        .p(space::S16)
        .flex()
        .items_center()
        .gap(space::S8)
        .border_t_1()
        .border_color(n.separator)
        .t_meta()
        .tnum()
        .text_color(n.secondary)
}

/// The viewer while the version is read: the fixed header, then skeleton lines where
/// the content will be (§2.14).
pub fn loading(title: &str, close: CloseFn, cx: &App) -> AnyElement {
    let bar = |w: f32| ui::native::skeleton(relative(w), px(14.));
    frame(cx)
        .child(bare_head(title, close, cx))
        .child(
            div()
                .flex_1()
                .p(space::S24)
                .flex()
                .flex_col()
                .gap(space::S12)
                .child(bar(0.9))
                .child(bar(0.7))
                .child(bar(0.4)),
        )
        .into_any_element()
}

/// The viewer when the version cannot be read: the fixed header, then an Alert in place
/// of the content (§2.15) with 重试.
pub fn failed(
    key: &store::MediaKey,
    title: &str,
    target: Target,
    close: CloseFn,
    cx: &App,
) -> AnyElement {
    let (key, again) = (key.clone(), title.to_string());
    frame(cx)
        .child(bare_head(title, close, cx))
        // The same height as while loading (E5): the Alert takes the
        // skeleton's place.
        .child(
            div().p(space::S24).child(
                // The same destructive variant as the inline row (one
                // failure, one Alert).
                ui::native::Alert::new(Sym::Warning, cm::STATE_FAILED)
                    .tone(ui::native::AlertTone::Destructive)
                    .description(cm::STATE_FAILED_RETRY)
                    .action(
                        ui::native::push_button(
                            "media-viewer-retry",
                            cm::RETRY,
                            ui::PushStyle::Outline,
                            ui::PushSize::Small,
                            cx,
                        )
                        .on_click(move |_, _, cx| {
                            if let Some(p) = target.provider.clone() {
                                store::forget(&key, cx);
                                store::request_artifact(&p, &key, &again, cx);
                            }
                        }),
                    ),
            ),
        )
        .into_any_element()
}

/// The header of a version that is not read yet (loading, failed): the same
/// anatomy as the ready header (icon, title, the fixed actions), with copy,
/// download and open disabled at 50 % until there is something to act on (§2.9).
fn bare_head(title: &str, close: CloseFn, cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    let n = ui::native::native_colors(cx);
    let off = |id: &'static str, sym: Sym, name: &'static str| {
        ui::native::icon_button_bare(id, sym, ui::native::IconSize::Regular)
            .named(name)
            .disabled(true)
            .opacity(0.5)
    };
    div()
        .flex_none()
        .h(ui::native::metrics::TOOLBAR_ROW)
        .pl(space::S16)
        .pr(space::S12)
        .flex()
        .items_center()
        .gap(space::S8)
        .border_b_1()
        .border_color(n.separator)
        .child(ui::icon(Sym::Document, px(16.), n.secondary))
        .child(
            ui::native::file_name(title.to_string())
                .flex_1()
                .min_w(px(0.))
                .t_card_title()
                .text_color(k.text_1),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(space::S4)
                .child(off("media-viewer-copy", Sym::Copy, cm::COPY))
                .child(off("media-viewer-save", Sym::Download, cm::SAVE))
                .child(off(
                    "media-viewer-sys",
                    Sym::OpenExternal,
                    cm::OPEN_WITH_SYSTEM,
                ))
                .child(
                    ui::native::icon_button_bare(
                        "media-viewer-close",
                        Sym::Close,
                        ui::native::IconSize::Regular,
                    )
                    .named(cm::CLOSE)
                    .on_click(move |_, window, cx| close(window, cx)),
                ),
        )
}

/// The h16 line between a viewer's own tools and its fixed actions (§2.2).
fn tool_separator(cx: &App) -> gpui::Div {
    let n = ui::native::native_colors(cx);
    div()
        .flex_none()
        .mx(space::S4)
        .w(ui::native::metrics::HAIRLINE)
        .h(space::S16)
        .bg(n.separator)
}

fn frame(cx: &App) -> gpui::Div {
    // §2.9: a sheet sits on the card surface (23 in dark), not the window
    // background.
    let n = ui::native::native_colors(cx);
    div()
        .size_full()
        .flex()
        .flex_col()
        .bg(n.sheet)
        .overflow_hidden()
}

fn file(spec: &FullSpec, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let path = spec.media.entry_path();
    frame(cx)
        .child(head(
            spec,
            spec.media.kind.icon(),
            spec.media.file_name().to_string(),
            Head {
                meta: Some(super::size_text(spec.media.bytes).into()),
                ..Head::default()
            },
            cx,
        ))
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(space::S12)
                .t_small()
                .text_color(k.text_3)
                .child(ui::icon(Sym::Alert, px(20.), k.text_3))
                .child(cm::STATE_FAILED_DETAIL)
                .child(
                    ui::native::push_button(
                        id(spec, "sysbig"),
                        cm::OPEN_WITH_SYSTEM,
                        ui::PushStyle::Outline,
                        ui::PushSize::Regular,
                        cx,
                    )
                    .on_click(move |_, _, cx| parts::open_with_system(&path, cx)),
                ),
        )
        .into_any_element()
}

// --- HTML ---------------------------------------------------------------------------

fn html(
    spec: &FullSpec,
    state: &Entity<MediaState>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let k = theme::colors(cx);
    let (shown, events) = {
        let st = state.read(cx);
        (
            st.version.unwrap_or(spec.media.key.version),
            st.events.clone(),
        )
    };
    // Another version: load it like any 产出 version, then show its folder.
    let media = if shown == spec.media.key.version {
        Some(spec.media.clone())
    } else {
        let key = store::MediaKey::new(spec.media.key.artifact_id.clone(), shown);
        if store::get(&key, cx).is_none()
            && let Some(p) = spec.target.provider.clone()
        {
            store::request_artifact(&p, &key, &spec.media.title, cx);
        }
        store::ready(&key, cx)
    };
    let page: AnyElement = match media {
        Some(m) => {
            let record = state.clone();
            WebEmbed::new(
                id(spec, "page"),
                WebEmbedSpec::new(
                    m.key.artifact_id.clone(),
                    artifact_view::VersionSpec::new(
                        m.key.version,
                        artifact_view::Content::Folder(m.dir.clone()),
                    )
                    .with_entry(m.entry.clone()),
                )
                // Pages from teammates get no bridge capabilities (as in the 产出 window).
                .capabilities(artifact_view::Capabilities::NONE),
            )
            // The page sits on the sheet's background edge to edge, with the
            // pane padding (§2.22: no darker panel without a line).
            .fill()
            .rounded(gpui::Pixels::ZERO)
            .pad(space::S16)
            .on_event(move |e, _, cx| {
                if let ArtifactEvent::Emit { name, .. } = e {
                    let name = name.clone();
                    record.update(cx, |s, cx| {
                        s.events.push(name);
                        if s.events.len() > 50 {
                            s.events.remove(0);
                        }
                        cx.notify();
                    });
                }
            })
            .into_any_element()
        }
        None => parts::placeholder(px(320.), ui::spinner(), cx).into_any_element(),
    };
    let _ = window;
    let versions = if spec.versions.is_empty() {
        vec![VersionRow {
            version: spec.media.key.version,
            when: String::new(),
            note: String::new(),
        }]
    } else {
        spec.versions.clone()
    };
    let mut list = div().flex().flex_col().gap(space::S2).child(
        div()
            .px(space::S8)
            .pb(space::S6)
            .t_meta()
            .text_color(k.text_3)
            .child(cm::VERSIONS),
    );
    for v in &versions {
        let pick = state.clone();
        let n = v.version;
        list = list.child(
            div()
                .id(ElementId::Name(
                    format!("{}-v{}", spec.id, v.version).into(),
                ))
                .focus_row(v.version == shown, cx)
                .px(space::S8)
                .py(space::S6)
                .rounded(radius::MD)
                .when(v.version == shown, |el| el.bg(k.selected))
                .flex()
                .flex_col()
                .gap(space::S2)
                .child(
                    div()
                        .flex()
                        .gap(space::S6)
                        .child(
                            mono(
                                fill(cm::VERSION_CHIP, &[("n", &v.version.to_string())]),
                                k.text_1,
                            )
                            .font_weight(gpui::FontWeight::SEMIBOLD),
                        )
                        .child(mono(v.when.clone(), k.text_3)),
                )
                .when(!v.note.is_empty(), |el| {
                    el.child(div().t_meta().text_color(k.text_2).child(v.note.clone()))
                })
                .on_click(move |_, _, cx| {
                    pick.update(cx, |s, cx| {
                        s.version = Some(n);
                        cx.notify();
                    })
                }),
        );
    }
    let mut log = div()
        .mt(space::S12)
        .pt(space::S12)
        .px(space::S8)
        .border_t_1()
        .border_color(k.sep)
        .flex()
        .flex_col()
        .gap(space::S6)
        .child(div().t_meta().text_color(k.text_3).child(cm::PAGE_EVENTS));
    if events.is_empty() {
        log = log.child(
            div()
                .t_meta()
                .text_color(k.text_3)
                .child(cm::PAGE_EVENTS_NONE),
        );
    }
    for e in events.iter().rev().take(8) {
        log = log.child(mono(e.clone(), k.text_2));
    }
    let header = head(
        spec,
        Sym::Document,
        spec.media.title.clone(),
        Head {
            meta: Some(
                format!(
                    "{} · {}",
                    cm::KIND_HTML,
                    fill(cm::VERSION_CHIP, &[("n", &shown.to_string())])
                )
                .into(),
            ),
            ..Head::default()
        },
        cx,
    );
    frame(cx)
        .child(header)
        .child(
            div()
                .flex_1()
                .min_h(px(0.))
                .flex()
                .child(div().flex_1().min_w(px(0.)).flex().flex_col().child(page))
                .child(
                    div()
                        .w(px(150.))
                        .flex_none()
                        .border_l_1()
                        .border_color(k.sep)
                        .p(space::S8)
                        .child(list)
                        .child(log),
                ),
        )
        .into_any_element()
}

// --- Markdown -----------------------------------------------------------------------

/// The document split at its headings (level 1–3): the leading part, then one part per
/// heading. A first-level title at the very top becomes the reader's title.
fn sections(md: &str) -> (Option<String>, Vec<(Option<String>, String)>) {
    let mut title = None;
    let mut out: Vec<(Option<String>, String)> = vec![(None, String::new())];
    let mut fence = false;
    for line in md.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = !fence;
        }
        if !fence {
            let level = t.chars().take_while(|c| *c == '#').count();
            if (1..=3).contains(&level) && t[level..].starts_with(' ') {
                let name = t[level..].trim().to_string();
                if level == 1 && title.is_none() && out.len() == 1 && out[0].1.trim().is_empty() {
                    title = Some(name);
                    continue;
                }
                out.push((Some(name), String::new()));
            }
        }
        let last = out.last_mut().expect("one part");
        last.1.push_str(line);
        last.1.push('\n');
    }
    out.retain(|(h, body)| h.is_some() || !body.trim().is_empty());
    (title, out)
}

fn markdown_view(
    spec: &FullSpec,
    md: &store::Markdown,
    state: &Entity<MediaState>,
    cx: &App,
) -> AnyElement {
    let k = theme::colors(cx);
    let (title, parts_) = sections(&md.text);
    let st = state.read(cx);
    let (toc_on, selected, scroll) = (st.sidebar, st.toc, st.scroll.clone());
    let mut toc = div()
        .w(px(128.))
        .flex_none()
        .border_r_1()
        .border_color(k.sep)
        .p(space::S8)
        .flex()
        .flex_col()
        .gap(space::S2)
        .child(
            div()
                .px(space::S8)
                .py(space::S6)
                .t_meta()
                .text_color(k.text_3)
                .child(cm::MD_TOC),
        );
    let offset = 1; // the title block is child 0
    for (i, (h, _)) in parts_.iter().enumerate() {
        let Some(h) = h else { continue };
        let go = state.clone();
        let handle = scroll.clone();
        toc = toc.child(
            div()
                .id(ElementId::Name(format!("{}-toc-{i}", spec.id).into()))
                .focus_row(false, cx)
                .px(space::S8)
                .py(space::S4)
                .rounded(radius::SM)
                .t_small()
                .one_line()
                .text_color(if i == selected { k.text_1 } else { k.text_2 })
                .when(i == selected, |el| el.bg(k.selected).medium())
                .child(h.clone())
                .on_click(move |_, _, cx| {
                    handle.scroll_to_top_of_item(i + offset);
                    go.update(cx, |s, cx| {
                        s.toc = i;
                        cx.notify();
                    });
                }),
        );
    }
    let mut doc = div()
        .id(id(spec, "doc"))
        .flex_1()
        .min_w(px(0.))
        .h_full()
        .overflow_y_scroll()
        .track_scroll(&scroll)
        .px(space::S24)
        .py(space::S20)
        .child(
            div()
                .pb(space::S12)
                .flex()
                .flex_col()
                .gap(space::S4)
                .child(
                    div()
                        .t_display()
                        .text_color(k.text_1)
                        .child(title.unwrap_or_else(|| spec.media.title.clone())),
                )
                .child(div().t_meta().text_color(k.text_3).child(format!(
                    "{} · {}",
                    spec.media.file_name(),
                    fill(cm::MD_WORDS, &[("n", &group_digits(md.stats.chars as u64))])
                ))),
        );
    for (i, (_, body)) in parts_.iter().enumerate() {
        doc = doc.child(div().pb(space::S8).child(markdown::message_body(
            ElementId::Name(format!("{}-md-{i}", spec.id).into()),
            body.clone(),
            &[],
        )));
    }
    let toggle = state.clone();
    let md_text = md.text.clone();
    let header = head(
        spec,
        Sym::Document,
        spec.media.title.clone(),
        Head {
            meta: Some(
                format!(
                    "{} · {}",
                    cm::KIND_MARKDOWN,
                    fill(
                        cm::VERSION_CHIP,
                        &[("n", &spec.media.key.version.to_string())]
                    )
                )
                .into(),
            ),
            toggles: vec![
                action(spec, "toc", Sym::Outline, cm::MD_TOC)
                    .selected(toc_on)
                    .on_click(move |_, _, cx| {
                        toggle.update(cx, |s, cx| {
                            s.sidebar = !s.sidebar;
                            cx.notify();
                        })
                    }),
            ],
            copy: Some(Rc::new(move |cx: &mut App| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(md_text.clone()))
            })),
        },
        cx,
    );
    frame(cx)
        .child(header)
        .child(
            div()
                .flex_1()
                .min_h(px(0.))
                .flex()
                .when(toc_on, |el| el.child(toc))
                .child(doc),
        )
        .into_any_element()
}

// --- images -------------------------------------------------------------------------

fn lightbox(
    spec: &FullSpec,
    pics: &[store::Pic],
    state: &Entity<MediaState>,
    cx: &App,
) -> AnyElement {
    let n = pics.len();
    let ix = state.read(cx).index.min(n - 1);
    let pic = &pics[ix];
    let step = |delta: isize| {
        let st = state.clone();
        move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
            st.update(cx, |s, cx| {
                s.index = ((s.index as isize + delta).rem_euclid(n as isize)) as usize;
                cx.notify();
            })
        }
    };
    let copy_path = pic.path.clone();
    let (send_path, target) = (pic.path.clone(), spec.target.clone());
    // One sheet in the app's theme (F52): the fixed header, the picture, the strip.
    let header = head(
        spec,
        Sym::Image,
        spec.media.title.clone(),
        Head {
            meta: Some(
                format!(
                    "{} · {}",
                    fill(
                        cm::IMG_POSITION,
                        &[("i", &(ix + 1).to_string()), ("n", &n.to_string())]
                    ),
                    pic.name
                )
                .into(),
            ),
            toggles: vec![action(spec, "send", Sym::Send, cm::SEND_TO_TOPIC).on_click(
                move |_, _, cx| {
                    if let Ok(bytes) = std::fs::read(&send_path) {
                        parts::send_image(&target, bytes, cx);
                    }
                },
            )],
            copy: Some(Rc::new(move |cx: &mut App| {
                parts::copy_image_file(&copy_path, cx)
            })),
        },
        cx,
    );
    let nav = |idn: &str, icon: Sym, label: &'static str| action(spec, idn, icon, label);
    let fg = theme::colors(cx).text_1;
    let mut strip = div().flex().justify_center().gap(space::S6);
    for (i, p) in pics.iter().enumerate() {
        let pick = state.clone();
        strip = strip.child(
            div()
                .id(ElementId::Name(format!("{}-thumb-{i}", spec.id).into()))
                .focus_row_with(false, crate::ui::native::Arrows::Grid, cx)
                .w(px(36.))
                .h(crate::ui::native::tokens::height::TABLE_ROW)
                .rounded(radius::MARK)
                .overflow_hidden()
                .border_2()
                .border_color(if i == ix {
                    fg
                } else {
                    gpui::transparent_black()
                })
                .opacity(if i == ix { 1. } else { 0.7 })
                .child(img(p.path.clone()).size_full().object_fit(ObjectFit::Cover))
                .on_click(move |_, _, cx| {
                    pick.update(cx, |s, cx| {
                        s.index = i;
                        cx.notify();
                    })
                }),
        );
    }
    frame(cx)
        .child(header)
        .child(
            div()
                .flex_1()
                .min_h(px(0.))
                .flex()
                .items_center()
                .gap(space::S16)
                .px(space::S16)
                .child(nav("prev", Sym::ChevronLeft, cm::IMG_PREV).on_click(step(-1)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .py(space::S4)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            img(pic.path.clone())
                                .size_full()
                                .object_fit(ObjectFit::Contain),
                        ),
                )
                .child(nav("next", Sym::ChevronRight, cm::IMG_NEXT).on_click(step(1))),
        )
        .child(div().pt(space::S12).pb(space::S16).child(strip))
        .into_any_element()
}

// --- video --------------------------------------------------------------------------

thread_local! {
    static PLAYERS: std::cell::RefCell<HashSet<PathBuf>> = std::cell::RefCell::new(HashSet::new());
}

/// The folder of the in-window player page: `index.html`, the video (hard link, else a
/// copy) and the poster. Made once per version.
fn player_dir(media: &Media, v: &store::Video) -> Option<PathBuf> {
    let dir = std::env::temp_dir().join("tc-media").join(format!(
        "player-{}-v{}",
        sanitize(&media.key.artifact_id),
        media.key.version
    ));
    if PLAYERS.with(|p| p.borrow().contains(&dir)) {
        return Some(dir);
    }
    std::fs::create_dir_all(&dir).ok()?;
    let ext = media_ext(&v.path);
    let file = dir.join(format!("media.{ext}"));
    if !file.exists() && std::fs::hard_link(&v.path, &file).is_err() {
        std::fs::copy(&v.path, &file).ok()?;
    }
    if let Some(p) = &v.poster {
        let _ = std::fs::copy(p, dir.join("poster.jpg"));
    }
    let duration = v.info.map_or(0., |i| i.duration);
    let html = player_html(&ext, v.poster.is_some(), duration, &v.chapters);
    std::fs::write(dir.join("index.html"), html).ok()?;
    PLAYERS.with(|p| p.borrow_mut().insert(dir.clone()));
    Some(dir)
}

/// The extension of the player's copy of the video: letters and digits only (the file
/// name comes from the member), else `mp4`.
fn media_ext(path: &std::path::Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|e| !e.is_empty() && e.len() <= 8 && e.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or_else(|| "mp4".into())
}

/// JSON that is safe inside an inline `<script>`: `<`, `>`, `&`, U+2028 and U+2029 are
/// written as `\u` escapes, so member text (VTT cue titles) can never end the script or
/// start markup. The value the script reads is the same.
fn script_json(value: &serde_json::Value) -> String {
    let mut out = String::new();
    for c in value.to_string().chars() {
        match c {
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c => out.push(c),
        }
    }
    out
}

/// The player page. Member text (chapter titles) goes in only as escaped JSON data, last,
/// so no later placeholder can be filled inside it.
fn player_html(ext: &str, poster: bool, duration: f64, chapters: &[store::Cue]) -> String {
    let chapters = serde_json::Value::Array(
        chapters
            .iter()
            .map(|c| serde_json::json!({"t": c.t, "title": c.text}))
            .collect(),
    );
    let duration = if duration.is_finite() { duration } else { 0. };
    PLAYER_HTML
        .replace("{SRC}", &format!("media.{ext}"))
        .replace("{POSTER}", if poster { "poster.jpg" } else { "" })
        .replace("{DURATION}", &format!("{duration}"))
        .replace("{PLAY}", cm::VIDEO_PLAY)
        .replace("{CHAPTERS}", &script_json(&chapters))
}

/// The player page may only report its position (`time`); it reads no data and sends
/// nothing to a member.
fn player_spec(dir: PathBuf, version: u32) -> artifact_view::VersionSpec {
    artifact_view::VersionSpec::new(version, artifact_view::Content::Folder(dir)).with_events(vec![
        artifact_view::EventDecl::new("time", artifact_view::EventKind::Emit),
    ])
}

const PLAYER_CAPS: artifact_view::Capabilities = artifact_view::Capabilities {
    emit: true,
    ..artifact_view::Capabilities::NONE
};
// The player only reports its position: it emits, and asks, reads and points at nothing.
const _: () = assert!(
    PLAYER_CAPS.emit
        && !PLAYER_CAPS.ask
        && !PLAYER_CAPS.get_data
        && !PLAYER_CAPS.r#ref
        && !PLAYER_CAPS.live_data
);

fn sanitize(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn player_embed(spec: &FullSpec, cx: &App) -> Option<Entity<WebEmbedState>> {
    let host = format!("player-{}", spec.media.key.artifact_id);
    live(cx)
        .into_iter()
        .find(|e| e.read(cx).artifact_id() == host)
}

/// Moves the player to `t` seconds (and keeps it paused or playing as it was).
fn seek_player(spec: &FullSpec, t: f64, cx: &App) {
    if let Some(e) = player_embed(spec, cx) {
        e.read(cx)
            .evaluate_script(&format!("window.seekTo && window.seekTo({t})"));
    }
}

fn video(spec: &FullSpec, v: &store::Video, state: &Entity<MediaState>, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let s = screen(cx);
    let info = v.info;
    let duration = info.map_or(0., |i| i.duration);
    let st = state.read(cx);
    let (position, picked) = (st.position, st.frame);
    let player: AnyElement = match player_dir(&spec.media, v) {
        Some(dir) => {
            let follow = state.clone();
            WebEmbed::new(
                id(spec, "player"),
                WebEmbedSpec::new(
                    format!("player-{}", spec.media.key.artifact_id),
                    player_spec(dir, spec.media.key.version),
                )
                .capabilities(PLAYER_CAPS),
            )
            .height(px(360.))
            .rounded(crate::ui::native::tokens::radius::CONTAINER)
            .autoplay(spec.autoplay)
            .on_event(move |e, _, cx| {
                if let ArtifactEvent::Emit { name, payload, .. } = e
                    && name == "time"
                {
                    let t = payload["t"].as_f64().unwrap_or(0.);
                    follow.update(cx, |s, cx| {
                        s.position = t;
                        cx.notify();
                    });
                }
            })
            .into_any_element()
        }
        None => div().size_full().bg(s.bg).into_any_element(),
    };
    let current_chapter = v.chapters.iter().rposition(|c| c.t <= position + 0.25);
    let mut chapters = div().flex().flex_col().gap(space::S2);
    if !v.chapters.is_empty() {
        chapters = chapters.child(
            div()
                .px(space::S8)
                .pb(space::S4)
                .t_meta()
                .text_color(k.text_3)
                .child(cm::VIDEO_CHAPTERS),
        );
        for (i, c) in v.chapters.iter().enumerate() {
            let go = spec.clone();
            let st2 = state.clone();
            let t = c.t;
            chapters = chapters.child(
                div()
                    .id(ElementId::Name(format!("{}-ch-{i}", spec.id).into()))
                    .focus_row(false, cx)
                    .h(crate::ui::native::tokens::height::DEFAULT)
                    .px(space::S8)
                    .rounded(radius::MD)
                    .flex()
                    .items_center()
                    .gap(space::S12)
                    .when(current_chapter == Some(i), |el| el.bg(k.selected))
                    .child(mono(super::time_text(c.t), k.text_3).w(px(36.)))
                    .child(
                        div()
                            .t_small()
                            .text_color(k.text_1)
                            .one_line()
                            .child(c.text.clone()),
                    )
                    .on_click(move |_, _, cx| {
                        seek_player(&go, t, cx);
                        st2.update(cx, |s, cx| {
                            s.position = t;
                            cx.notify();
                        });
                    }),
            );
        }
    }
    // The picked frame, else the frame at the playing position.
    let sel = picked.or_else(|| v.frames.iter().rposition(|f| f.t <= position + 0.25));
    let mut strip = div().flex().gap(space::S8);
    for (i, f) in v.frames.iter().enumerate() {
        let go = spec.clone();
        let st2 = state.clone();
        let t = f.t;
        strip = strip.child(
            div()
                .id(ElementId::Name(format!("{}-fr-{i}", spec.id).into()))
                .focus_row_with(false, crate::ui::native::Arrows::Grid, cx)
                .flex_1()
                .h(crate::ui::native::tokens::height::TABLE_ROW)
                .rounded(radius::SM)
                .overflow_hidden()
                .border_2()
                .border_color(if sel == Some(i) {
                    k.text_1
                } else {
                    gpui::transparent_black()
                })
                .bg(s.bg)
                .child(img(f.path.clone()).size_full().object_fit(ObjectFit::Cover))
                .on_click(move |_, _, cx| {
                    seek_player(&go, t, cx);
                    st2.update(cx, |s, cx| {
                        s.frame = Some(i);
                        s.position = t;
                        cx.notify();
                    });
                }),
        );
    }
    let at = sel.and_then(|i| v.frames.get(i)).map_or(position, |f| f.t);
    let (copy_v, send_v, target) = (v.path.clone(), v.path.clone(), spec.target.clone());
    // Length, frame size and file size once, in the header (not again in a footer).
    let mut meta = vec![super::time_text(duration)];
    if let Some(i) = info {
        meta.push(format!("{} × {}", i.width, i.height));
    }
    meta.push(super::size_text(spec.media.bytes));
    let header = head(
        spec,
        Sym::Video,
        spec.media.file_name().to_string(),
        Head {
            meta: Some(meta.join(" · ").into()),
            ..Head::default()
        },
        cx,
    );
    frame(cx)
        .child(header)
        .child(
            div()
                .id(id(spec, "video-scroll"))
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .p(space::S12)
                .flex()
                .flex_col()
                .gap(space::S12)
                .child(div().w_full().h(px(360.)).flex_none().child(player))
                .when(!v.chapters.is_empty(), |el| el.child(chapters))
                .child(
                    div()
                        .h(crate::ui::native::tokens::layout::BORDER)
                        .bg(k.sep)
                        .mx(space::S4),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S8)
                        .px(space::S4)
                        .child(
                            div()
                                .t_meta()
                                .text_color(k.text_3)
                                .child(fill(cm::VIDEO_FRAME_AT, &[("t", &super::time_text(at))])),
                        )
                        .child(div().flex_1())
                        .child(
                            ui::native::push_button(
                                id(spec, "copyf"),
                                cm::VIDEO_COPY_FRAME,
                                ui::native::PushStyle::Outline,
                                ui::native::PushSize::Small,
                                cx,
                            )
                            .icon(ui::icon(Sym::Copy, px(13.), k.text_2))
                            .on_click(move |_, _, cx| {
                                if let Some(png) = store::frame_png(&copy_v, at) {
                                    parts::copy_image(ImageFormat::Png, png, cx);
                                }
                            }),
                        )
                        .child(
                            ui::native::push_button(
                                id(spec, "sendf"),
                                cm::VIDEO_SEND_FRAME,
                                ui::native::PushStyle::Outline,
                                ui::native::PushSize::Small,
                                cx,
                            )
                            .icon(ui::icon(Sym::Send, px(13.), k.text_2))
                            .on_click(move |_, _, cx| {
                                if let Some(png) = store::frame_png(&send_v, at) {
                                    parts::send_image(&target, png, cx);
                                }
                            }),
                        ),
                )
                .child(strip),
        )
        .into_any_element()
}

/// The player page inside the window: the system video decoder, custom controls in the
/// design's dark bar (play, time, scrubber with chapter marks, speed, mute, picture in
/// picture), and `window.seekTo(t)` for the frame strip. It reports the position as a
/// `time` event while it plays.
const PLAYER_HTML: &str = r##"<!doctype html><html><head><meta charset="utf-8"><style>
:root{color-scheme:dark}
html,body{margin:0;height:100%;background:#16161A;color:#C9C9D1;font:12px -apple-system,"PingFang SC",sans-serif;-webkit-font-smoothing:antialiased;overflow:hidden}
@media (prefers-color-scheme: dark){html,body{background:#0B0B0D}}
.wrap{position:absolute;inset:0;display:flex;flex-direction:column}
.stage{flex:1;position:relative;min-height:0}
video{position:absolute;inset:0;width:100%;height:100%;object-fit:contain;background:transparent}
.big{position:absolute;left:50%;top:50%;width:52px;height:52px;margin:-26px 0 0 -26px;border-radius:50%;border:0;background:rgba(255,255,255,.8);cursor:pointer;display:flex;align-items:center;justify-content:center}
.big svg{width:20px;height:20px;margin-left:3px}
.bar{height:44px;display:flex;align-items:center;gap:12px;padding:0 14px;font:12px -apple-system,"PingFang SC",sans-serif;font-variant-numeric:tabular-nums}
.bar button{background:none;border:0;color:#C9C9D1;cursor:pointer;padding:4px;display:flex;align-items:center;border-radius:6px}
.bar button:hover{background:rgba(255,255,255,.08)}
.bar svg{width:16px;height:16px;fill:none;stroke:currentColor;stroke-width:1.75;stroke-linecap:round;stroke-linejoin:round}
.time{color:#C9C9D1;white-space:nowrap}.time i{font-style:normal;color:#7A7A84}
.track{flex:1;position:relative;height:16px;cursor:pointer}
.segs{position:absolute;left:0;right:0;top:6px;height:4px;display:flex;gap:3px}
.seg{height:4px;border-radius:2px;background:rgba(255,255,255,.22);position:relative;overflow:hidden}
.seg b{position:absolute;left:0;top:0;bottom:0;background:#C9C9D1}
.thumb{position:absolute;top:3px;width:10px;height:10px;margin-left:-5px;border-radius:50%;background:#fff;box-shadow:0 1px 2px rgba(0,0,0,.3)}
.speed{padding:2px 7px!important;font:12px -apple-system,sans-serif;font-variant-numeric:tabular-nums}
</style></head><body><div class="wrap">
<div class="stage"><video id="v" src="{SRC}" poster="{POSTER}" playsinline preload="metadata"></video>
<button class="big" id="big" aria-label="{PLAY}"><svg viewBox="0 0 24 24"><path d="M7 4l13 8-13 8z" fill="#141416"/></svg></button></div>
<div class="bar">
<button id="pp" aria-label="{PLAY}"><svg viewBox="0 0 24 24" id="ppi"><path d="M7 4l13 8-13 8z" fill="currentColor"/></svg></button>
<span class="time" id="tm">0:00 <i>/ 0:00</i></span>
<div class="track" id="tr"><div class="segs" id="segs"></div><div class="thumb" id="th"></div></div>
<button class="speed" id="sp">1×</button>
<button id="mu"><svg viewBox="0 0 24 24"><path d="M11 5L6 9H2v6h4l5 4z"/><path d="M15.5 8.5a5 5 0 0 1 0 7"/><path d="M19 5a10 10 0 0 1 0 14"/></svg></button>
<button id="pip"><svg viewBox="0 0 24 24"><rect x="3" y="5" width="18" height="14" rx="2"/><rect x="12" y="11" width="7" height="6" rx="1"/></svg></button>
</div></div>
<script>
(function(){
var v=document.getElementById('v'),D={DURATION},C={CHAPTERS};
var segs=document.getElementById('segs'),th=document.getElementById('th'),tm=document.getElementById('tm');
function dur(){return (v.duration&&isFinite(v.duration))?v.duration:D}
function fmt(s){s=Math.max(0,Math.round(s));var m=Math.floor(s/60),x=s%60;return m+':'+(x<10?'0':'')+x}
var parts=C.length?C.map(function(c,i){return {t:c.t,end:i+1<C.length?C[i+1].t:null}}):[{t:0,end:null}];
parts.forEach(function(p){var d=document.createElement('div');d.className='seg';d.appendChild(document.createElement('b'));segs.appendChild(d);p.el=d});
function layout(){var T=dur()||1;parts.forEach(function(p){var e=p.end==null?T:p.end;p.el.style.flex=Math.max(0.001,(e-p.t)/T)+' 1 0'})}
function draw(){var T=dur()||1,t=v.currentTime;parts.forEach(function(p){var e=p.end==null?T:p.end;var f=Math.min(1,Math.max(0,(t-p.t)/(e-p.t)));p.el.firstChild.style.width=(f*100)+'%'});th.style.left=(t/T*100)+'%';tm.innerHTML=fmt(t)+' <i>/ '+fmt(T)+'</i>'}
var last=-1;function report(){if(Math.abs(v.currentTime-last)<0.2)return;last=v.currentTime;try{window.artifact.emitEvent('time',{t:v.currentTime})}catch(e){}}
function icon(){document.getElementById('ppi').innerHTML=v.paused?'<path d="M7 4l13 8-13 8z" fill="currentColor"/>':'<rect x="6" y="5" width="4" height="14" fill="currentColor"/><rect x="14" y="5" width="4" height="14" fill="currentColor"/>';document.getElementById('big').style.display=v.paused&&v.currentTime<0.01?'flex':'none'}
function toggle(){if(v.paused)v.play();else v.pause()}
document.getElementById('big').onclick=toggle;document.getElementById('pp').onclick=toggle;v.onclick=toggle;
var speeds=[1,1.25,1.5,2,0.75],si=0;document.getElementById('sp').onclick=function(){si=(si+1)%speeds.length;v.playbackRate=speeds[si];this.textContent=speeds[si]+'×'};
document.getElementById('mu').onclick=function(){v.muted=!v.muted;this.style.opacity=v.muted?.45:1};
document.getElementById('pip').onclick=function(){try{v.webkitSetPresentationMode(v.webkitPresentationMode==='picture-in-picture'?'inline':'picture-in-picture')}catch(e){}};
document.getElementById('tr').onclick=function(e){var r=this.getBoundingClientRect();v.currentTime=(e.clientX-r.left)/r.width*dur()};
window.seekTo=function(t){v.currentTime=t;draw();report()};
v.addEventListener('loadedmetadata',function(){layout();draw()});
v.addEventListener('timeupdate',function(){draw();report()});
v.addEventListener('play',icon);v.addEventListener('pause',icon);v.addEventListener('seeked',function(){draw();report();icon()});
document.addEventListener('keydown',function(e){if(e.key===' '){e.preventDefault();toggle()}if(e.key==='ArrowRight')v.currentTime+=5;if(e.key==='ArrowLeft')v.currentTime-=5});
layout();draw();icon();
if(v.autoplay){v.muted=true;v.play()}
})();
</script></body></html>"##;

// --- audio --------------------------------------------------------------------------

/// The height of the full audio view's player block (wave, speakers,
/// controls), so the sheet hugs it.
pub fn audio_player_height(a: &store::Audio) -> gpui::Pixels {
    let speakers = a.transcript.iter().any(|c| c.speaker.is_some()) && a.duration > 0.;
    // pt 16, wave 64, gap 8, controls 4 + 36, pb 12, the 1 px rule.
    let base = px(16. + 64. + 8. + 40. + 12. + 1.);
    // The speaker segments (6) and the legend (16), each after a gap of 8.
    if speakers {
        base + px(8. + 6. + 8. + 16.)
    } else {
        base
    }
}

fn audio(spec: &FullSpec, a: &store::Audio, state: &Entity<MediaState>, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let st = state.read(cx);
    let (playing, position, speed) = (st.playing(), st.position, st.speed);
    let played = if a.duration > 0. {
        (position / a.duration) as f32
    } else {
        0.
    };
    let bars = 160usize;
    // Clickable waveform: each bar seeks to its place.
    let mut wave = div()
        .h(px(64.))
        .flex()
        .items_center()
        .gap(crate::ui::native::tokens::space::S2)
        .relative();
    for i in 0..bars {
        let n = a.peaks.len().max(1);
        let lo = i * n / bars;
        let hi = ((i + 1) * n / bars).max(lo + 1).min(n);
        let v = a
            .peaks
            .get(lo..hi)
            .map_or(0., |s| s.iter().cloned().fold(0f32, f32::max));
        let frac = (i as f32 + 0.5) / bars as f32;
        let seek = state.clone();
        let path = a.path.clone();
        let t = a.duration * i as f64 / bars as f64;
        wave = wave.child(
            div()
                .id(ElementId::Name(format!("{}-bar-{i}", spec.id).into()))
                .flex_1()
                .h_full()
                .flex()
                .items_center()
                .child(div().w_full().h(px((v * 64.).max(2.))).rounded(px(1.)).bg(
                    if frac <= played {
                        k.text_1
                    } else {
                        k.border_strong
                    },
                ))
                .on_click(move |_, _, cx| seek.update(cx, |s, cx| s.seek_audio(&path, t, cx))),
        );
    }
    // Speaker segments from the transcript, in three greys.
    let mut speakers: Vec<String> = Vec::new();
    for c in &a.transcript {
        if let Some(s) = &c.speaker
            && !speakers.contains(s)
        {
            speakers.push(s.clone());
        }
    }
    let tones = [k.text_1, k.text_3, k.border_strong];
    let mut segs = div()
        .h(px(6.))
        .w_full()
        .flex()
        .gap(crate::ui::native::tokens::space::S2);
    let mut legend = div().flex().gap(space::S12);
    if !speakers.is_empty() && a.duration > 0. {
        for (i, c) in a.transcript.iter().enumerate() {
            let end = a.transcript.get(i + 1).map_or(a.duration, |n| n.t);
            let tone = c
                .speaker
                .as_ref()
                .and_then(|s| speakers.iter().position(|x| x == s))
                .map_or(k.border, |ix| tones[ix % tones.len()]);
            segs = segs.child(
                div()
                    .h_full()
                    .flex_basis(relative(((end - c.t) / a.duration).clamp(0., 1.) as f32))
                    .flex_grow(1.)
                    .rounded(px(1.))
                    .bg(tone),
            );
        }
        for (i, s) in speakers.iter().enumerate().take(3) {
            legend = legend.child(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S4)
                    .child(
                        div()
                            .size(px(8.))
                            .rounded(px(2.))
                            .bg(tones[i % tones.len()]),
                    )
                    .child(div().t_meta().text_color(k.text_2).child(s.clone())),
            );
        }
    }
    let path = a.path.clone();
    let (toggle, back, fwd) = (state.clone(), state.clone(), state.clone());
    let (pb, pf) = (path.clone(), path.clone());
    // The speed choice is a segmented control (NATIVE.md §5.5).
    let speeds = {
        let set = state.clone();
        crate::ui::native::Segmented::new(
            ElementId::Name(format!("{}-sp", spec.id).into()),
            SPEEDS
                .iter()
                .map(|(_, l)| crate::ui::native::Segment::Label((*l).into())),
        )
        .selected(speed)
        .on_change(move |i, _, cx| set.update(cx, |s, cx| s.set_speed(i, cx)))
    };
    let current = a.transcript.iter().rposition(|c| c.t <= position + 0.25);
    let mut lines = div()
        .flex()
        .flex_col()
        .gap(crate::ui::native::tokens::space::S2);
    for (i, c) in a.transcript.iter().enumerate().take(400) {
        let seek = state.clone();
        let p = a.path.clone();
        let t = c.t;
        lines = lines.child(
            div()
                .id(ElementId::Name(format!("{}-cue-{i}", spec.id).into()))
                .focus_row(false, cx)
                .min_h(px(32.))
                .px(space::S8)
                .py(space::S6)
                .rounded(radius::MD)
                .flex()
                .gap(space::S12)
                .when(current == Some(i), |el| el.bg(k.selected))
                .child(mono(super::clock_text(c.t), k.text_3).w(px(40.)).pt(px(2.)))
                .child(
                    div()
                        .flex_1()
                        .t_small()
                        .text_color(k.text_1)
                        .flex()
                        .gap(space::S6)
                        .when_some(c.speaker.clone(), |el, s| {
                            el.child(div().flex_none().semibold().child(s))
                        })
                        .child(div().flex_1().min_w(px(0.)).child(c.text.clone())),
                )
                .on_click(move |_, _, cx| seek.update(cx, |s, cx| s.seek_audio(&p, t, cx))),
        );
    }
    let round = |idn: &str, icon: Sym, label: &'static str| tool(id(spec, idn), icon, label);
    let header = head(
        spec,
        Sym::Audio,
        spec.media.file_name().to_string(),
        Head {
            meta: Some(
                format!(
                    "{} · {}",
                    super::time_text(a.duration),
                    super::size_text(spec.media.bytes)
                )
                .into(),
            ),
            ..Head::default()
        },
        cx,
    );
    frame(cx)
        .child(header)
        .child(
            div()
                .px(space::S16)
                .pt(space::S16)
                .pb(space::S12)
                .flex()
                .flex_col()
                .gap(space::S8)
                .border_b_1()
                .border_color(k.sep)
                .child(wave)
                // The time shows once, in the controls (A5); the speakers row
                // shows only when the transcript names speakers.
                .when(!speakers.is_empty(), |el| el.child(segs).child(legend))
                .child(
                    div()
                        .pt(space::S4)
                        .flex()
                        .items_center()
                        .gap(space::S12)
                        .child(round("back", Sym::Retry, cm::AUDIO_BACK).on_click(
                            move |_, _, cx| {
                                back.update(cx, |s, cx| {
                                    let t = s.position - 15.;
                                    s.seek_audio(&pb, t, cx)
                                })
                            },
                        ))
                        .child(parts::play_button(
                            id(spec, "play"),
                            playing,
                            36.,
                            cx,
                            move |_, _, cx| toggle.update(cx, |s, cx| s.toggle_audio(&path, cx)),
                        ))
                        .child(round("fwd", Sym::Refresh, cm::AUDIO_FORWARD).on_click(
                            move |_, _, cx| {
                                fwd.update(cx, |s, cx| {
                                    let t = s.position + 15.;
                                    s.seek_audio(&pf, t, cx)
                                })
                            },
                        ))
                        .child(
                            mono(
                                format!(
                                    "{} / {}",
                                    super::clock_text(position),
                                    super::clock_text(a.duration)
                                ),
                                k.text_1,
                            )
                            .text_size(crate::ui::native::tokens::text::UI.0),
                        )
                        .child(div().flex_1())
                        .child(speeds),
                ),
        )
        .child(
            div()
                .id(id(spec, "transcript"))
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .p(space::S8)
                .when(!a.transcript.is_empty(), |el| {
                    el.child(
                        div()
                            .px(space::S8)
                            .py(space::S6)
                            .t_small()
                            .medium()
                            .text_color(k.text_2)
                            .child(cm::AUDIO_TRANSCRIPT),
                    )
                    .child(lines)
                })
                .when(a.transcript.is_empty(), |el| {
                    el.child(
                        div()
                            .p(space::S8)
                            .t_meta()
                            .text_color(k.text_3)
                            .child(cm::AUDIO_NOTE),
                    )
                }),
        )
        .into_any_element()
}

// --- PDF ----------------------------------------------------------------------------

fn pdf(spec: &FullSpec, p: &store::Pdf, state: &Entity<MediaState>, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let st = state.read(cx);
    let (ix, zoom, sidebar, pages) = (
        st.index.min(p.images.len().saturating_sub(1)),
        st.zoom,
        st.sidebar,
        st.pages.clone(),
    );
    let go = |delta: isize| {
        let s = state.clone();
        let n = p.images.len();
        let handle = pages.clone();
        move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
            s.update(cx, |s, cx| {
                let i = (s.index as isize + delta).clamp(0, n.saturating_sub(1) as isize) as usize;
                s.index = i;
                handle.scroll_to_top_of_item(i);
                cx.notify();
            })
        }
    };
    let zoom_by = |f: f32| {
        let s = state.clone();
        move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
            s.update(cx, |s, cx| {
                s.zoom = (s.zoom * f).clamp(0.5, 2.5);
                cx.notify();
            })
        }
    };
    let side = state.clone();
    let toolbar = div()
        .h(crate::ui::native::tokens::height::FOOTER_ROW)
        .px(space::S12)
        .flex()
        .items_center()
        .gap(space::S8)
        .border_b_1()
        .border_color(k.sep)
        .child(
            tool(id(spec, "side"), Sym::SidebarToggle, cm::PDF_SIDEBAR)
                .selected(sidebar)
                .on_click(move |_, _, cx| {
                    side.update(cx, |s, cx| {
                        s.sidebar = !s.sidebar;
                        cx.notify();
                    })
                }),
        )
        .child(
            div()
                .w(px(1.))
                .h(crate::ui::native::tokens::icon::MD)
                .bg(k.sep),
        )
        .child(tool(id(spec, "prev"), Sym::ChevronLeft, cm::PDF_PREV).on_click(go(-1)))
        .child(
            mono(
                fill(
                    cm::PDF_PAGE_OF,
                    &[("i", &(ix + 1).to_string()), ("n", &p.pages.to_string())],
                ),
                k.text_1,
            )
            .text_size(crate::ui::native::tokens::text::UI.0),
        )
        .child(tool(id(spec, "next"), Sym::ChevronRight, cm::PDF_NEXT).on_click(go(1)))
        .child(div().flex_1())
        .child(tool(id(spec, "zout"), Sym::Minus, cm::PDF_ZOOM_OUT).on_click(zoom_by(1. / 1.25)))
        .child(
            mono(format!("{}%", (zoom * 100.).round()), k.text_1)
                .text_size(crate::ui::native::tokens::text::MONO.0)
                .w(px(44.))
                .flex()
                .justify_center(),
        )
        .child(tool(id(spec, "zin"), Sym::Plus, cm::PDF_ZOOM_IN).on_click(zoom_by(1.25)));
    let mut thumbs = div()
        .id(id(spec, "thumbs"))
        .w(px(104.))
        .flex_none()
        .h_full()
        .overflow_y_scroll()
        .border_r_1()
        .border_color(k.sep)
        .p(space::S12)
        .flex()
        .flex_col()
        .items_center()
        .gap(space::S12);
    for (i, page) in p.images.iter().enumerate() {
        let (w, h) = fit(page.width, page.height, 66., 96.);
        let pick = state.clone();
        let handle = pages.clone();
        thumbs = thumbs.child(
            div()
                .id(ElementId::Name(format!("{}-pt-{i}", spec.id).into()))
                .focus_row_with(false, crate::ui::native::Arrows::Grid, cx)
                .flex()
                .flex_col()
                .items_center()
                .gap(space::S4)
                .child(
                    div()
                        .w(px(w + 4.))
                        .h(px(h + 4.))
                        .rounded(crate::ui::native::tokens::radius::CHECKBOX)
                        .border_2()
                        .border_color(if i == ix {
                            k.text_1
                        } else {
                            gpui::transparent_black()
                        })
                        .child(
                            div()
                                .size_full()
                                .border_1()
                                .border_color(k.border)
                                .bg(gpui::white())
                                .child(
                                    img(page.path.clone())
                                        .size_full()
                                        .object_fit(ObjectFit::Contain),
                                ),
                        ),
                )
                .child(mono(
                    (i + 1).to_string(),
                    if i == ix { k.text_1 } else { k.text_3 },
                ))
                .on_click(move |_, _, cx| {
                    handle.scroll_to_top_of_item(i);
                    pick.update(cx, |s, cx| {
                        s.index = i;
                        cx.notify();
                    })
                }),
        );
    }
    let page_w = 520. * zoom;
    let mut doc = div()
        .id(id(spec, "pages"))
        .flex_1()
        .min_w(px(0.))
        .h_full()
        .overflow_scroll()
        .track_scroll(&pages)
        .bg(k.code_bg)
        .p(space::S16)
        .flex()
        .flex_col()
        .items_center()
        .gap(space::S16);
    for page in &p.images {
        let ratio = page.width.max(1) as f32 / page.height.max(1) as f32;
        // D2-33: at 100 % and below the page fits the width inside a 16
        // gutter on the muted backdrop; zoomed in, it keeps its own width
        // and the pane scrolls.
        let sized = if zoom <= 1. {
            div().flex_none().w_full().max_w(px(page_w))
        } else {
            div().flex_none().w(px(page_w))
        };
        doc = doc.child(
            sized
                .aspect_ratio(ratio)
                .bg(gpui::white())
                .border_1()
                .border_color(k.border)
                .shadow_sm()
                .child(
                    img(page.path.clone())
                        .size_full()
                        .object_fit(ObjectFit::Contain),
                ),
        );
    }
    let header = head(
        spec,
        Sym::Document,
        spec.media.file_name().to_string(),
        Head {
            meta: Some(
                format!(
                    "{} · {}",
                    fill(cm::PDF_PAGES, &[("n", &p.pages.to_string())]),
                    super::size_text(spec.media.bytes)
                )
                .into(),
            ),
            ..Head::default()
        },
        cx,
    );
    frame(cx)
        .child(header)
        .child(toolbar)
        .child(
            div()
                .flex_1()
                .min_h(px(0.))
                .flex()
                .when(sidebar, |el| el.child(thumbs))
                .child(doc),
        )
        .into_any_element()
}

// --- code ---------------------------------------------------------------------------

fn code(spec: &FullSpec, c: &store::Code, state: &Entity<MediaState>, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let lines = Arc::new(c.lines.clone());
    let lang = c.lang;
    // Block comments carry across lines: the state at each line start, computed once.
    let starts = Arc::new(block_starts(&c.lines, lang));
    let width = (c.lines.len().max(1).ilog10() + 1) as f32 * 8. + 24.;
    let wrap = state.read(cx).wrap;
    let list_lines = lines.clone();
    let list = uniform_list(id(spec, "lines"), lines.len(), move |range, _window, cx| {
        let k = theme::colors(cx);
        range
            .map(|i| {
                let mut block = starts[i];
                div()
                    .h(px(20.))
                    .flex()
                    .child(
                        div()
                            .w(px(width))
                            .flex_none()
                            .pr(space::S12)
                            .flex()
                            .justify_end()
                            .text_color(k.text_3)
                            .child((i + 1).to_string()),
                    )
                    .child(code_line(&list_lines[i], lang, &mut block, cx))
            })
            .collect()
    })
    .track_scroll(&state.read(cx).list)
    .flex_1()
    .t_code()
    .tnum();
    let all = lines.join("\n");
    let toggle = state.clone();
    let header = head(
        spec,
        Sym::CodeFile,
        spec.media.file_name().to_string(),
        Head {
            meta: Some(
                format!(
                    "{} · {}",
                    fill(
                        cm::VERSION_CHIP,
                        &[("n", &spec.media.key.version.to_string())]
                    ),
                    fill(
                        cm::CODE_NOTE,
                        &[("lang", c.language), ("n", &lines.len().to_string())]
                    )
                )
                .into(),
            ),
            toggles: vec![
                action(spec, "wrap", Sym::List, cm::CODE_WRAP)
                    .selected(wrap)
                    .on_click(move |_, _, cx| {
                        toggle.update(cx, |s, cx| {
                            s.wrap = !s.wrap;
                            cx.notify();
                        })
                    }),
            ],
            copy: Some(Rc::new(move |cx: &mut App| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(all.clone()))
            })),
        },
        cx,
    );
    let body: AnyElement = if wrap && lines.len() <= 4000 {
        let mut col = div()
            .id(id(spec, "wrapped"))
            .flex_1()
            .overflow_y_scroll()
            .t_code();
        for (i, l) in lines.iter().enumerate() {
            col = col.child(
                div()
                    .flex()
                    .child(
                        div()
                            .w(px(width))
                            .flex_none()
                            .pr(space::S12)
                            .flex()
                            .justify_end()
                            .text_color(k.text_3)
                            .child((i + 1).to_string()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_color(k.text_1)
                            .child(l.clone()),
                    ),
            );
        }
        col.into_any_element()
    } else {
        list.into_any_element()
    };
    frame(cx)
        .child(header)
        // The same space above the first line's ink and under the last
        // line's (A4): 8 + the line's own leading at the top, 12 under it.
        .child(
            div()
                .flex_1()
                .min_h_0()
                .pt(space::S8)
                .pb(space::S12)
                .flex()
                .flex_col()
                .child(body),
        )
        .into_any_element()
}

fn block_starts(lines: &[String], lang: text::Lang) -> Vec<bool> {
    let mut out = Vec::with_capacity(lines.len());
    let mut block = false;
    for l in lines {
        out.push(block);
        let _ = text::highlight(l, lang, &mut block);
    }
    out
}

// --- table --------------------------------------------------------------------------

fn table(spec: &FullSpec, t: &text::Table, state: &Entity<MediaState>, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let cols = t.header.len().max(1);
    let sort = state.read(cx).sort;
    let mut order: Vec<usize> = (0..t.rows.len()).collect();
    if let Some((c, asc)) = sort {
        let numeric = t.numeric.get(c).copied().unwrap_or(false);
        order.sort_by(|&a, &b| {
            let (x, y) = (
                t.rows[a].get(c).map(String::as_str).unwrap_or(""),
                t.rows[b].get(c).map(String::as_str).unwrap_or(""),
            );
            let o = if numeric {
                text::number(x)
                    .unwrap_or(f64::MIN)
                    .partial_cmp(&text::number(y).unwrap_or(f64::MIN))
                    .unwrap_or(std::cmp::Ordering::Equal)
            } else {
                x.cmp(y)
            };
            if asc { o } else { o.reverse() }
        });
    }
    // §2.7: header h40 on the muted fill, rows h40 (compact), cells px 12 (16 at the
    // frame edges), numbers right-aligned and tabular.
    let n = ui::native::native_colors(cx);
    let edge = |i: usize, cols: usize, d: gpui::Stateful<gpui::Div>| {
        d.when(i == 0, |d| d.pl(space::S16))
            .when(i + 1 == cols, |d| d.pr(space::S16))
    };
    let mut table_head = div()
        .flex_none()
        .h(TABLE_ROW)
        .flex()
        .items_center()
        .border_b_1()
        .border_color(n.separator)
        .bg(n.fill);
    for (i, h) in t.header.iter().enumerate() {
        let numeric = t.numeric.get(i).copied().unwrap_or(false);
        let set = state.clone();
        let mark = match sort {
            Some((c, asc)) if c == i => Some(if asc {
                Sym::ChevronUp
            } else {
                Sym::ChevronDown
            }),
            _ => None,
        };
        table_head = table_head.child(edge(
            i,
            cols,
            div()
                .id(ElementId::Name(format!("{}-h-{i}", spec.id).into()))
                .focus_row_with(false, crate::ui::native::Arrows::Grid, cx)
                .flex_1()
                .min_w(px(0.))
                .h_full()
                .px(space::S12)
                .flex()
                .items_center()
                .gap(space::S2)
                .when(numeric, |el| el.justify_end())
                .t_meta_medium()
                .text_color(k.text_2)
                .child(div().one_line().child(h.clone()))
                .when_some(mark, |el, m| el.child(ui::icon(m, px(12.), k.text_2)))
                .on_click(move |_, _, cx| {
                    set.update(cx, |s, cx| {
                        s.sort = match s.sort {
                            Some((c, true)) if c == i => Some((i, false)),
                            Some((c, false)) if c == i => None,
                            _ => Some((i, true)),
                        };
                        cx.notify();
                    })
                }),
        ));
    }
    let rows = Arc::new(t.rows.clone());
    let numeric = Arc::new(t.numeric.clone());
    let order = Arc::new(order);
    let list = uniform_list(id(spec, "rows"), order.len(), move |range, _window, cx| {
        let k = theme::colors(cx);
        range
            .map(|i| {
                let r = &rows[order[i]];
                let n = ui::native::native_colors(cx);
                let mut row = div()
                    .w_full()
                    .h(TABLE_ROW)
                    .flex()
                    .items_center()
                    .when(i + 1 < order.len(), |r| {
                        r.border_b_1().border_color(n.separator)
                    });
                for c in 0..cols {
                    let v = r.get(c).cloned().unwrap_or_default();
                    let num = numeric.get(c).copied().unwrap_or(false);
                    let cell = div()
                        .flex_1()
                        .min_w(px(0.))
                        .px(space::S12)
                        .when(c == 0, |d| d.pl(space::S16))
                        .when(c + 1 == cols, |d| d.pr(space::S16))
                        .flex()
                        .one_line();
                    row = row.child(if num {
                        cell.justify_end().child(
                            mono(v, k.text_1).text_size(crate::ui::native::tokens::text::MONO.0),
                        )
                    } else {
                        cell.t_small().text_color(k.text_1).child(v)
                    });
                }
                row
            })
            .collect()
    })
    .track_scroll(&state.read(cx).list)
    .flex_1();
    let sorted = sort
        .and_then(|(c, _)| t.header.get(c))
        .map(|h| fill(cm::TABLE_SORTED, &[("col", h)]));
    let header = head(
        spec,
        Sym::Table,
        spec.media.file_name().to_string(),
        Head {
            meta: Some(
                fill(
                    cm::TABLE_META,
                    &[
                        ("rows", &group_digits(t.total_rows as u64)),
                        ("cols", &t.header.len().to_string()),
                    ],
                )
                .into(),
            ),
            ..Head::default()
        },
        cx,
    );
    frame(cx)
        .child(header)
        .child(table_head)
        .child(div().flex_1().min_h(px(0.)).flex().flex_col().child(list))
        .child(
            foot(cx)
                .child(fill(
                    cm::TABLE_FOOTER,
                    &[("n", &group_digits(t.total_rows as u64))],
                ))
                .child(div().flex_1())
                .when_some(sorted, |el, s| el.child(s)),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{media_ext, player_html, sections, store::Cue};

    #[test]
    fn chapter_text_stays_data_in_the_player_page() {
        let cue = |t: f64, text: &str| Cue {
            t,
            end: t + 1.,
            speaker: None,
            text: text.into(),
        };
        let evil = "</script><script>alert(1)</script>&\u{2028}{PLAY}";
        let html = player_html("mp4", false, 12., &[cue(0., "开场"), cue(5., evil)]);
        // The page has its one script; the cue text cannot close it or open another.
        assert_eq!(html.matches("<script>").count(), 1);
        assert_eq!(html.matches("</script>").count(), 1);
        assert!(!html.contains("alert(1)</"));
        // The script reads the same text back.
        let start = html.find("C=").unwrap() + 2;
        let end = start + html[start..].find(";\n").unwrap();
        let back: serde_json::Value = serde_json::from_str(&html[start..end]).unwrap();
        assert_eq!(back[1]["title"], evil);
        assert_eq!(back[0]["title"], "开场");
    }

    #[test]
    fn the_player_only_reports_its_position() {
        let ext = |p: &str| media_ext(std::path::Path::new(p));
        assert_eq!(ext("clip.MOV"), "mov");
        assert_eq!(ext("clip.mp4\"><b"), "mp4");
        assert_eq!(ext("clip"), "mp4");
    }

    #[test]
    fn splits_markdown_at_headings() {
        let (title, parts) =
            sections("# 复盘\n\n## 原因\n旧 seq。\n\n```\n## 不是\n```\n## 后续\n- [ ] 测试\n");
        assert_eq!(title.as_deref(), Some("复盘"));
        let names: Vec<_> = parts
            .iter()
            .map(|(h, _)| h.clone().unwrap_or_default())
            .collect();
        assert_eq!(names, ["原因", "后续"]);
        assert!(parts[0].1.contains("## 不是"));
    }
}
