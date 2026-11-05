//! Inline previews in the 话题 timeline (Media.dc.html, left column of each section).
//!
//! Each kind keeps a fixed box from the first frame (loading included), so a message row
//! never changes height when its preview arrives (`views/timeline/extras.rs`). Clicking a
//! preview opens the full view in the 产出 viewer ([`super::viewer`]).

use std::sync::Arc;

use gpui::{
    AnyElement, App, ElementId, InteractiveElement as _, IntoElement, ObjectFit, ParentElement,
    SharedString, StatefulInteractiveElement as _, Styled, StyledImage as _, Window, div, img,
    prelude::FluentBuilder as _, px, relative,
};

use super::{
    INLINE_W, MediaKind,
    parts::{self, card, card_header, chip, footer, mono, tool},
    screen,
    state::{MediaState, SPEEDS, Target},
    states,
    store::{self, Data, Load, Media, MediaKey},
    text::{self, Tok},
    viewer::{self, Opening},
};

use crate::ui::native::FocusRow as _;
use crate::{
    copy::{fill, media as cm},
    markdown,
    theme::{self, Type as _, radius, space},
    ui::{
        self, Sym,
        web_embed::{WebEmbed, WebEmbedSpec},
    },
};

/// What the timeline knows about a 产出 reference before it loads.
#[derive(Clone)]
pub struct InlineSpec {
    pub key: MediaKey,
    pub kind: MediaKind,
    pub title: String,
    /// Total size of the version's files.
    pub bytes: u64,
    /// Where 发到话题 goes, and the provider that loads the version.
    pub target: Target,
    /// Unique per row, so two rows that show the same 产出 keep separate state.
    pub id: SharedString,
}

/// Height of the HTML page inline (Media.dc.html: 300).
pub const HTML_H: f32 = 300.;
const MD_BODY_H: f32 = 236.;
/// The source lines of the Markdown excerpt: few enough that wrapped lines
/// still end above [`MD_BODY_H`].
const MD_EXCERPT_LINES: usize = 5;
const GRID_H: f32 = 148.;
const SINGLE_H: f32 = 300.;
// Every in-thread item has the one card width (§2.4): min(640, text column).
const VIDEO_W: f32 = 640.;
const PDF_W: f32 = 640.;
const AUDIO_W: f32 = 640.;
const CODE_LINES: usize = 8;
/// Table rows inline: compact h40 (§2.7).
const TABLE_ROW: gpui::Pixels = crate::ui::native::tokens::height::TABLE_ROW_COMPACT;
const TABLE_ROWS: usize = 4;

/// The inline preview of one 产出 version, or `None` when the kind shows as a file row.
pub fn render(spec: &InlineSpec, window: &mut Window, cx: &mut App) -> Option<AnyElement> {
    if !spec.kind.previews_inline() {
        return None;
    }
    // inc-host: a live page that showed the latest version follows it (reload in place).
    let followed;
    let spec = if spec.kind == MediaKind::Html && spec.target.provider.is_some() {
        let v = crate::views::artifacts::host::inline_version(
            &spec.id,
            &spec.key.artifact_id,
            spec.key.version,
            cx,
        );
        followed = InlineSpec {
            key: MediaKey::new(spec.key.artifact_id.clone(), v),
            ..spec.clone()
        };
        &followed
    } else {
        spec
    };
    match store::get(&spec.key, cx) {
        Some(Load::Ready(media)) => Some(ready(spec, &media, window, cx)),
        Some(Load::Failed(_)) => Some(states::failed_row(spec, cx).into_any_element()),
        Some(Load::Loading) => Some(loading(spec, cx)),
        None => {
            if spec.bytes > spec.kind.auto_load_limit() {
                return Some(states::big_row(spec, cx).into_any_element());
            }
            if let Some(provider) = spec.target.provider.clone() {
                store::request_artifact(&provider, &spec.key, &spec.title, cx);
            }
            Some(loading(spec, cx))
        }
    }
}

fn open(spec: &InlineSpec, opening: Opening, cx: &mut App) {
    viewer::open(
        spec.key.clone(),
        spec.title.clone(),
        spec.target.clone(),
        opening,
        cx,
    );
}

fn ready(spec: &InlineSpec, media: &Arc<Media>, window: &mut Window, cx: &mut App) -> AnyElement {
    if media.preview_failed {
        return states::failed_media_row(spec, media, cx).into_any_element();
    }
    match &media.data {
        Data::Html => html(spec, media, cx),
        Data::Markdown(md) => markdown_card(spec, media, md, cx),
        Data::Images(pics) => images(spec, pics, cx),
        Data::Video(v) => video(spec, media, v, cx),
        Data::Audio(a) => audio(spec, media, a, window, cx),
        Data::Pdf(p) => pdf(spec, media, p, cx),
        Data::Code(c) => code(spec, media, c, cx),
        Data::Table(t) => table(spec, media, t, cx),
        Data::File => states::failed_media_row(spec, media, cx).into_any_element(),
    }
}

/// The fixed box of a kind while it loads.
fn loading(spec: &InlineSpec, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let (w, h) = inline_box(spec.kind);
    let _ = k;
    // §2.14: a skeleton of the final shape, no spinner text in an empty region.
    card(cx)
        .w(w)
        .max_w_full()
        .child(card_header(spec.kind.icon(), spec.title.clone(), cx).child(div().flex_1()))
        .child(
            div()
                .h(h)
                .px(space::S16)
                .pb(space::S16)
                .flex()
                .flex_col()
                .gap(space::S8)
                .child(ui::native::skeleton(gpui::relative(1.), h - px(64.)))
                .child(ui::native::skeleton(gpui::relative(0.6), px(14.)))
                .child(ui::native::skeleton(gpui::relative(0.35), px(14.))),
        )
        .into_any_element()
}

/// Width and body height of each kind's box (the header and footer are fixed too).
fn inline_box(kind: MediaKind) -> (gpui::Pixels, gpui::Pixels) {
    match kind {
        MediaKind::Html => (INLINE_W, px(HTML_H + 34.)),
        MediaKind::Markdown => (INLINE_W, px(MD_BODY_H + 34.)),
        MediaKind::Image => (INLINE_W, px(SINGLE_H)),
        MediaKind::ImageSet => (INLINE_W, px(GRID_H)),
        MediaKind::Video => (px(VIDEO_W), px(VIDEO_W * 9. / 16. + 86.)),
        MediaKind::Audio => (px(AUDIO_W), px(70.)),
        MediaKind::Pdf => (px(PDF_W), px(186.)),
        MediaKind::Code => (INLINE_W, px(CODE_LINES as f32 * 20. + 16.)),
        MediaKind::Table => (
            INLINE_W,
            TABLE_ROW * (TABLE_ROWS as f32 + 1.)
                + crate::ui::native::tokens::height::TABLE_ROW_COMPACT,
        ),
        MediaKind::File => (INLINE_W, px(64.)),
    }
}

fn version_chip(v: u32, cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    div()
        .flex_none()
        .h(px(20.))
        .flex()
        .items_center()
        .child(mono(
            fill(cm::VERSION_CHIP, &[("n", &v.to_string())]),
            k.text_2,
        ))
}

fn open_tool(spec: &InlineSpec, suffix: &str, opening: Opening) -> gpui_component::button::Button {
    let s = spec.clone();
    tool(
        ElementId::Name(format!("{}-{suffix}", spec.id).into()),
        Sym::OpenExternal,
        cm::OPEN_IN_WINDOW,
    )
    .on_click(move |_, _, cx| open(&s, opening.clone(), cx))
}

// --- HTML ---------------------------------------------------------------------------

fn html(spec: &InlineSpec, media: &Arc<Media>, cx: &App) -> AnyElement {
    use crate::views::artifacts::host;
    let k = theme::colors(cx);
    let id = &spec.key.artifact_id;
    let version = spec.key.version;
    // While the 产出 pane shows this page, the thread keeps one reference row
    // instead of a second live copy (§2.17, F36).
    if host::shown_in_pane(id, cx) {
        return host::reference_row(
            ElementId::Name(format!("{}-ref", spec.id).into()),
            id,
            spec.title.clone(),
            version,
            cx,
        );
    }
    // inc-host: the page host's bridge (capabilities by author, live data, asks, refs).
    let page_spec = match spec.target.provider.as_deref() {
        Some(p) => {
            let (v, caps) =
                host::version_spec(p, id, version, media.dir.clone(), media.entry.clone());
            WebEmbedSpec::new(id.clone(), v)
                .capabilities(caps)
                .viewer(host::viewer(p))
                .ref_menu(Some(host::ref_menu(cx)))
        }
        // Samples without a provider: no bridge.
        None => WebEmbedSpec::new(
            id.clone(),
            artifact_view::VersionSpec::new(
                version,
                artifact_view::Content::Folder(media.dir.clone()),
            )
            .with_entry(media.entry.clone()),
        )
        .capabilities(artifact_view::Capabilities::NONE),
    };
    let mut embed = WebEmbed::new(
        ElementId::Name(format!("{}-page", spec.id).into()),
        page_spec,
    )
    .height(px(HTML_H))
    .rounded(px(0.))
    .pass_scroll(true);
    if spec.target.provider.is_some() {
        embed = embed.on_host(host::embed_handler(id.clone()));
    }
    let latest = host::latest(id, cx).unwrap_or(version);
    let notice = host::notice(id, version, latest, cx).map(|t| host::notice_chip(t, cx));
    let s = spec.clone();
    let expand = div()
        .id(ElementId::Name(format!("{}-expand", spec.id).into()))
        .focus_row(false, cx)
        .flex_none()
        .flex()
        .items_center()
        .gap(space::S4)
        .child(ui::icon(Sym::ChevronDown, px(14.), k.text_3))
        .child(cm::EXPAND)
        .on_click(move |_, _, cx| open(&s, Opening::default(), cx));
    let foot = match spec.target.provider.as_deref() {
        Some(p) => host::page_footer(
            &spec.id,
            p,
            id,
            version,
            Some(expand.into_any_element()),
            cx,
        ),
        None => footer(cx)
            .child(expand)
            .child(div().flex_1())
            .child(media.file_name().to_string()),
    };
    card(cx)
        .w(INLINE_W)
        .max_w_full()
        .child(
            card_header(Sym::Document, spec.title.clone(), cx)
                .child(version_chip(spec.key.version, cx))
                .children(notice)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S4)
                        .child(ui::icon(Sym::Shield, px(12.), k.text_3))
                        .child(div().t_meta().text_color(k.text_3).child(cm::SANDBOX)),
                )
                .child(div().flex_1())
                .child(open_tool(spec, "open", Opening::default())),
        )
        // The page sets its own height: it ends on a whole block (§2.21).
        .child(embed)
        .child(foot)
        .into_any_element()
}

// --- Markdown -----------------------------------------------------------------------

fn markdown_card(
    spec: &InlineSpec,
    media: &Arc<Media>,
    md: &store::Markdown,
    cx: &App,
) -> AnyElement {
    let k = theme::colors(cx);
    let minutes = (md.stats.chars as f64 / 450.).ceil().max(1.) as u64;
    let excerpt = text::md_excerpt(&md.text, MD_EXCERPT_LINES);
    let mut chips = div().flex().items_center().gap(space::S6);
    if md.stats.tables > 0 {
        chips = chips.child(chip(
            fill(cm::MD_TABLES, &[("n", &md.stats.tables.to_string())]),
            cx,
        ));
    }
    if md.stats.images > 0 {
        chips = chips.child(chip(
            fill(cm::MD_IMAGES, &[("n", &md.stats.images.to_string())]),
            cx,
        ));
    }
    if md.stats.todo_all > 0 {
        chips = chips.child(chip(
            fill(
                cm::MD_TODO,
                &[
                    ("done", &md.stats.todo_done.to_string()),
                    ("all", &md.stats.todo_all.to_string()),
                ],
            ),
            cx,
        ));
    }
    let s = spec.clone();
    card(cx)
        .w(INLINE_W)
        .max_w_full()
        .child(
            card_header(Sym::Document, media.file_name().to_string(), cx)
                .child(mono(
                    fill(cm::MD_WORDS, &[("n", &group_digits(md.stats.chars as u64))]),
                    k.text_3,
                ))
                .child(div().flex_1())
                .child(open_tool(spec, "open", Opening::default())),
        )
        .child(
            // The excerpt ends on whole blocks (its first lines, a fence
            // closed) and the card hugs it: nothing is cut under the footer
            // (§2.21, A4).
            div()
                .max_h(px(MD_BODY_H))
                .overflow_hidden()
                .px(space::S16)
                .pt(space::S12)
                .pb(space::S12)
                .child(markdown::message_body(
                    ElementId::Name(format!("{}-md", spec.id).into()),
                    excerpt,
                    &[],
                )),
        )
        .child(
            footer(cx)
                .id(ElementId::Name(format!("{}-more", spec.id).into()))
                .focus_row(false, cx)
                .child(ui::icon(Sym::ChevronDown, px(14.), k.text_3))
                .child(fill(cm::MD_EXPAND, &[("m", &minutes.to_string())]))
                .child(div().flex_1())
                .child(chips)
                .on_click(move |_, _, cx| open(&s, Opening::default(), cx)),
        )
        .into_any_element()
}

/// `1,820`.
pub fn group_digits(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

// --- images -------------------------------------------------------------------------

fn images(spec: &InlineSpec, pics: &[store::Pic], cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let total: u64 = pics.iter().map(|p| p.bytes).sum();
    let first = &pics[0];
    let mut meta = vec![fill(cm::IMG_COUNT, &[("n", &pics.len().to_string())])];
    if first.width > 0 {
        meta.push(format!("{} × {}", first.width, first.height));
    }
    meta.push(super::size_text(total));
    let tiles = if pics.len() == 1 {
        let (w, h) = fit(
            first.width,
            first.height,
            f32::from(INLINE_W) - 100.,
            SINGLE_H,
        );
        let s = spec.clone();
        div().h(px(SINGLE_H)).flex().items_end().child(
            div()
                .id(ElementId::Name(format!("{}-img", spec.id).into()))
                .focus_row(false, cx)
                .w(px(w))
                .h(px(h))
                .rounded(radius::MD)
                .overflow_hidden()
                .bg(k.code_bg)
                .child(
                    img(first.path.clone())
                        .size_full()
                        .rounded(radius::MD)
                        .object_fit(ObjectFit::Cover),
                )
                .on_click(move |_, _, cx| open(&s, Opening::at(0), cx)),
        )
    } else {
        let mut row = div().h(px(GRID_H)).flex().gap(space::S6);
        for (i, p) in pics.iter().take(4).enumerate() {
            let s = spec.clone();
            let more = (i == 3 && pics.len() > 4).then(|| pics.len() - 4);
            row = row.child(
                div()
                    .id(ElementId::Name(format!("{}-img-{i}", spec.id).into()))
                    .focus_row_with(false, crate::ui::native::Arrows::Grid, cx)
                    .relative()
                    .w(px(111.))
                    .h_full()
                    .rounded(radius::MD)
                    .overflow_hidden()
                    .bg(k.code_bg)
                    .child(
                        img(p.path.clone())
                            .size_full()
                            .rounded(radius::MD)
                            .object_fit(ObjectFit::Cover),
                    )
                    .when_some(more, |el, n| {
                        el.child(
                            div()
                                .absolute()
                                .inset_0()
                                .bg(gpui::hsla(0., 0., 0., 0.45))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(crate::ui::native::tokens::text::TITLE.0)
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(gpui::white())
                                .child(format!("+{n}")),
                        )
                    })
                    .on_click(move |_, _, cx| open(&s, Opening::at(i), cx)),
            );
        }
        row
    };
    let paths: Vec<std::path::PathBuf> = pics.iter().map(|p| p.path.clone()).collect();
    div()
        .flex()
        .flex_col()
        .gap(space::S8)
        .child(tiles)
        .child(
            div()
                .flex()
                .items_center()
                .gap(space::S8)
                .child(mono(meta.join(" · "), k.text_3))
                .child(
                    div()
                        .id(ElementId::Name(format!("{}-save", spec.id).into()))
                        .focus_row(false, cx)
                        .t_meta()
                        .text_color(k.text_2)
                        .child(cm::IMG_SAVE_ALL)
                        .on_click(move |_, _, cx| {
                            let refs: Vec<&std::path::Path> =
                                paths.iter().map(|p| p.as_path()).collect();
                            parts::save_to_downloads(&refs, cx);
                        }),
                ),
        )
        .into_any_element()
}

/// A picture's box inside `max_w` x `max_h`, keeping its ratio.
pub fn fit(w: u32, h: u32, max_w: f32, max_h: f32) -> (f32, f32) {
    if w == 0 || h == 0 {
        return (max_w.min(max_h * 4. / 3.), max_h);
    }
    let s = (max_w / w as f32).min(max_h / h as f32);
    ((w as f32 * s).max(48.), (h as f32 * s).max(48.))
}

// --- video --------------------------------------------------------------------------

/// The round play glyph over a poster.
pub fn play_badge(size: f32) -> gpui::Div {
    div()
        .size(px(size))
        .rounded_full()
        .bg(gpui::hsla(0., 0., 1., 0.78))
        .flex()
        .items_center()
        .justify_center()
        .child(ui::icon(
            Sym::Play,
            px(size * 0.4),
            gpui::hsla(0., 0., 0.08, 1.),
        ))
}

/// Chapter ticks along the bottom edge of a poster or scrubber: one segment per chapter.
pub fn chapter_ticks(chapters: &[store::Cue], duration: f64, color: gpui::Hsla) -> gpui::Div {
    let mut row = div()
        .w_full()
        .h(px(3.))
        .flex()
        .gap(crate::ui::native::tokens::space::S4);
    if duration <= 0. || chapters.is_empty() {
        return row;
    }
    for (i, c) in chapters.iter().enumerate() {
        let end = chapters.get(i + 1).map_or(duration, |n| n.t);
        let frac = ((end - c.t) / duration).clamp(0., 1.) as f32;
        row = row.child(
            div()
                .h_full()
                .flex_basis(relative(frac))
                .flex_grow(1.)
                .bg(color),
        );
    }
    row
}

/// The poster of a video 产出: the sender's (`MediaInfo.posterFileId`, MEDIA §3: receivers
/// compute nothing), else the frame made here.
fn poster_of(spec: &InlineSpec, v: &store::Video) -> Option<std::path::PathBuf> {
    let sender = spec
        .target
        .provider
        .as_ref()
        .and_then(|p| p.artifact_poster(&spec.key.artifact_id, spec.key.version));
    pick_poster(sender, v.poster.clone())
}

fn pick_poster(
    sender: Option<std::path::PathBuf>,
    local: Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    sender.or(local)
}

fn video(spec: &InlineSpec, media: &Arc<Media>, v: &store::Video, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let s = screen(cx);
    let duration = v.info.map_or(0., |i| i.duration);
    let h = VIDEO_W * 9. / 16.;
    let play = spec.clone();
    let poster = div()
        .id(ElementId::Name(format!("{}-poster", spec.id).into()))
        .focus_row(false, cx)
        .relative()
        .w(px(VIDEO_W))
        .h(px(h))
        .rounded(crate::ui::native::tokens::radius::CONTAINER)
        .overflow_hidden()
        .bg(s.bg)
        .when_some(poster_of(spec, v), |el, p| {
            el.child(
                img(p)
                    .size_full()
                    .rounded(crate::ui::native::tokens::radius::CONTAINER)
                    .object_fit(ObjectFit::Contain),
            )
        })
        .child(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .child(play_badge(44.)),
        )
        .child(
            div()
                .absolute()
                .right(space::S12)
                .bottom(space::S12)
                .px(space::S6)
                .rounded(radius::MARK)
                .bg(gpui::hsla(0., 0., 0., 0.55))
                .child(mono(super::time_text(duration), gpui::white())),
        )
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .child(chapter_ticks(
                    &v.chapters,
                    duration,
                    gpui::hsla(0., 0., 1., 0.35),
                )),
        )
        .on_click(move |_, _, cx| open(&play, Opening::play(), cx));
    let mut strip = div().flex().gap(space::S6);
    for (i, f) in v.frames.iter().enumerate() {
        let go = spec.clone();
        let send_target = spec.target.clone();
        let (video_path, t) = (v.path.clone(), f.t);
        let group = SharedString::from(format!("{}-f{i}", spec.id));
        strip = strip.child(
            // §2.4: the frames share the in-thread width (no void after
            // the last one).
            div()
                .relative()
                .group(group.clone())
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(space::S4)
                .child(
                    div()
                        .id(ElementId::Name(format!("{}-frame-{i}", spec.id).into()))
                        .focus_row_with(false, crate::ui::native::Arrows::Grid, cx)
                        .w_full()
                        .h(crate::ui::native::tokens::space::S48 + space::S8)
                        .rounded(radius::MARK)
                        .overflow_hidden()
                        .border_1()
                        .border_color(k.border)
                        .group_hover(group.clone(), |st| st.border_color(k.text_1))
                        .bg(s.bg)
                        .child(img(f.path.clone()).size_full().object_fit(ObjectFit::Cover))
                        .on_click(move |_, _, cx| open(&go, Opening::frame(i), cx)),
                )
                .child(mono(super::time_text(f.t), k.text_3))
                .child(
                    div()
                        .id(ElementId::Name(format!("{}-send-{i}", spec.id).into()))
                        .focus_row_with(false, crate::ui::native::Arrows::Grid, cx)
                        .invisible()
                        .group_hover(group, |st| st.visible())
                        .absolute()
                        .bottom(px(64.))
                        .left(px(-20.))
                        .h(px(26.))
                        .px(space::S8)
                        .rounded(radius::SM)
                        .bg(k.text_1)
                        .flex()
                        .items_center()
                        .gap(space::S4)
                        .whitespace_nowrap()
                        .child(ui::icon(Sym::Send, px(12.), k.content))
                        .child(
                            div()
                                .t_meta()
                                .text_color(k.content)
                                .child(cm::VIDEO_SEND_FRAME),
                        )
                        .on_click(move |_, _, cx| {
                            if let Some(png) = store::frame_png(&video_path, t) {
                                parts::send_image(&send_target, png, cx);
                            }
                        }),
                ),
        );
    }
    let _ = media;
    div()
        .flex()
        .flex_col()
        .gap(space::S8)
        .child(poster)
        .child(
            div()
                .flex()
                .items_center()
                .gap(space::S8)
                .child(
                    div()
                        .t_meta()
                        .medium()
                        .text_color(k.text_2)
                        .child(cm::VIDEO_FRAMES),
                )
                .child(div().t_meta().text_color(k.text_3).child(fill(
                    cm::VIDEO_FRAMES_META,
                    &[("n", &v.frames.len().to_string())],
                ))),
        )
        .child(strip)
        .into_any_element()
}

// --- audio --------------------------------------------------------------------------

/// Waveform bars from `peaks`: `n` bars that share the row's width; bars before
/// `played` (0..1) are dark.
pub fn waveform(
    peaks: &[f32],
    n: usize,
    height: f32,
    played: f32,
    on: gpui::Hsla,
    off: gpui::Hsla,
) -> gpui::Div {
    let mut row = div()
        .w_full()
        .h(px(height))
        .flex()
        .items_center()
        .gap(crate::ui::native::tokens::space::S2);
    if peaks.is_empty() {
        return row;
    }
    for i in 0..n {
        let a = i * peaks.len() / n;
        let b = ((i + 1) * peaks.len() / n).max(a + 1).min(peaks.len());
        let v = peaks[a..b].iter().cloned().fold(0f32, f32::max);
        let h = (v * height).max(2.);
        let color = if (i as f32 + 0.5) / n as f32 <= played {
            on
        } else {
            off
        };
        // The bars share the row's width (no void after the last, §2.4).
        row = row.child(div().flex_1().min_w_0().h(px(h)).rounded(px(1.)).bg(color));
    }
    row
}

fn audio(
    spec: &InlineSpec,
    media: &Arc<Media>,
    a: &store::Audio,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let k = theme::colors(cx);
    let state = MediaState::for_view(
        ElementId::Name(format!("{}-audio", spec.id).into()),
        window,
        cx,
    );
    let st = state.read(cx);
    let (playing, position, speed) = (st.playing(), st.position, st.speed);
    let played = if a.duration > 0. {
        (position / a.duration) as f32
    } else {
        0.
    };
    let path = a.path.clone();
    let toggle = state.clone();
    let speed_state = state.clone();
    let footer_text = if a.transcript.is_empty() {
        cm::AUDIO_NOTE.to_string()
    } else {
        fill(
            cm::AUDIO_TRANSCRIPT_COUNT,
            &[("n", &a.transcript.len().to_string())],
        )
    };
    let _ = media;
    card(cx)
        .w(px(AUDIO_W))
        .max_w_full()
        .child(
            div()
                .h(crate::ui::native::tokens::height::LIST_ROW)
                .px(space::S12)
                .flex()
                .items_center()
                .gap(space::S10)
                .child(parts::play_button(
                    ElementId::Name(format!("{}-play", spec.id).into()),
                    playing,
                    32.,
                    cx,
                    move |_, _, cx| {
                        toggle.update(cx, |s, cx| s.toggle_audio(&path, cx));
                    },
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .overflow_hidden()
                        .child(waveform(
                            &a.peaks,
                            70,
                            28.,
                            played,
                            k.text_1,
                            k.border_strong,
                        )),
                )
                .child(mono(
                    format!(
                        "{} / {}",
                        super::clock_text(position),
                        super::clock_text(a.duration)
                    ),
                    k.text_2,
                ))
                .child(
                    div()
                        .id(ElementId::Name(format!("{}-speed", spec.id).into()))
                        .focus_row(false, cx)
                        // A borderless text button (NATIVE.md §5.1).
                        .h(px(22.))
                        .px(space::S6)
                        .rounded(radius::SM)
                        .flex()
                        .items_center()
                        .child(mono(SPEEDS[speed].1, k.text_2))
                        .on_click(move |_, _, cx| {
                            speed_state
                                .update(cx, |s, cx| s.set_speed((s.speed + 1) % SPEEDS.len(), cx));
                        }),
                ),
        )
        .child(
            footer(cx)
                .child(ui::icon(Sym::Document, px(13.), k.text_3))
                .child(footer_text)
                .child(div().flex_1())
                .child(open_tool(spec, "open", Opening::default())),
        )
        .into_any_element()
}

// --- PDF ----------------------------------------------------------------------------

fn pdf(spec: &InlineSpec, media: &Arc<Media>, p: &store::Pdf, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let mut thumbs = div()
        .h(px(140.))
        .px(space::S10)
        .pt(space::S10)
        .flex()
        .gap(space::S8)
        .bg(k.code_bg)
        .overflow_hidden();
    // §2.4: as many whole pages as the 640 card holds (px 10, gap 8), so
    // the strip has no void at the right and no page cut at the edge.
    let mut room = PDF_W - 20.;
    let mut shown = 0;
    for page in &p.images {
        let (w, _) = fit(page.width, page.height, 120., 150.);
        let need = if shown == 0 { w } else { w + 8. };
        if need > room {
            break;
        }
        room -= need;
        shown += 1;
    }
    for page in p.images.iter().take(shown.max(1)) {
        let (w, h) = fit(page.width, page.height, 120., 150.);
        thumbs = thumbs.child(
            div()
                .w(px(w))
                .h(px(h))
                .flex_none()
                .rounded(px(3.))
                .overflow_hidden()
                .border_1()
                .border_color(k.border)
                .bg(gpui::white())
                .shadow_xs()
                .child(
                    img(page.path.clone())
                        .size_full()
                        .object_fit(ObjectFit::Cover),
                ),
        );
    }
    let s = spec.clone();
    card(cx)
        .id(ElementId::Name(format!("{}-pdf", spec.id).into()))
        .focus_row(false, cx)
        .w(px(PDF_W))
        .max_w_full()
        .child(thumbs)
        .child(
            div()
                .h(crate::ui::native::tokens::height::FORM_ROW)
                .px(space::S12)
                .flex()
                .items_center()
                .gap(space::S8)
                .border_t_1()
                .border_color(k.sep)
                .child(ui::icon(Sym::Document, theme::icon::ROW, k.text_2))
                .child(
                    ui::native::file_name(media.file_name().to_string())
                        .t_small()
                        .medium()
                        .text_color(k.text_1),
                )
                .child(mono(
                    format!(
                        "{} · {}",
                        fill(cm::PDF_PAGES, &[("n", &p.pages.to_string())]),
                        super::size_text(media.bytes)
                    ),
                    k.text_3,
                ))
                .child(div().flex_1())
                .child(div().t_meta().text_color(k.text_2).child(cm::PDF_OPEN))
                .child(ui::icon(Sym::ChevronRight, px(14.), k.text_2)),
        )
        .on_click(move |_, _, cx| open(&s, Opening::default(), cx))
        .into_any_element()
}

// --- code ---------------------------------------------------------------------------

/// One highlighted line: keywords 600 `text-1`, strings and numbers `text-2`, comments
/// `text-3` italic (monochrome, so status colors keep their meaning).
pub fn code_line(line: &str, lang: text::Lang, block: &mut bool, cx: &App) -> gpui::Div {
    let k = theme::colors(cx);
    let mut row = div().flex().whitespace_nowrap();
    for (tok, s) in text::highlight(line, lang, block) {
        let el = div().child(s.replace('\t', "    "));
        row = row.child(match tok {
            Tok::Keyword => el
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(k.text_1),
            Tok::Str | Tok::Num => el.text_color(k.text_2),
            Tok::Comment => el.italic().text_color(k.text_3),
            Tok::Plain => el.text_color(k.text_1),
        });
    }
    row
}

fn code(spec: &InlineSpec, media: &Arc<Media>, c: &store::Code, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let first = c
        .lines
        .iter()
        .position(|l| !l.trim().is_empty())
        .unwrap_or(0);
    let last = (first + CODE_LINES).min(c.lines.len());
    // Long lines scroll sideways inside the card, never a hard clip (§2.17).
    let mut body = div()
        .id(ElementId::Name(format!("{}-code", spec.id).into()))
        .h(px(CODE_LINES as f32 * 20. + 16.))
        .py(space::S8)
        .overflow_x_scroll()
        .t_code()
        .tnum();
    let mut block = false;
    for l in &c.lines[..first] {
        let _ = text::highlight(l, c.lang, &mut block);
    }
    for (i, l) in c.lines[first..last].iter().enumerate() {
        body = body.child(
            div()
                .h(px(20.))
                .flex()
                .child(
                    div()
                        .w(px(40.))
                        .flex_none()
                        .pr(space::S12)
                        .flex()
                        .justify_end()
                        .text_color(k.text_3)
                        .child((first + i + 1).to_string()),
                )
                .child(code_line(l, c.lang, &mut block, cx)),
        );
    }
    let text_all = c.lines[first..last].join("\n");
    let copy_id = ElementId::Name(format!("{}-copy", spec.id).into());
    card(cx)
        .w(INLINE_W)
        .max_w_full()
        .child(
            card_header(Sym::CodeFile, String::new(), cx)
                .child(mono(media.entry.clone(), k.text_1))
                .child(div().t_meta().text_color(k.text_3).child(fill(
                    cm::CODE_RANGE,
                    &[
                        ("a", &(first + 1).to_string()),
                        ("b", &last.to_string()),
                        ("n", &c.lines.len().to_string()),
                    ],
                )))
                .child(div().flex_1())
                .child(
                    tool(copy_id, Sym::Copy, cm::COPY).on_click(move |_, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text_all.clone()));
                    }),
                )
                .child(open_tool(spec, "open", Opening::default())),
        )
        .child(body)
        .into_any_element()
}

// --- table --------------------------------------------------------------------------

fn table(spec: &InlineSpec, media: &Arc<Media>, t: &text::Table, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let cols = t.header.len().max(1);
    let cell = |text: String, numeric: bool, header: bool, c: usize| {
        // §2.7: cells px 12, 16 at the frame edges; number columns keep their width.
        let el = div()
            .flex_1()
            .min_w(px(0.))
            .px(space::S12)
            .when(c == 0, |d| d.pl(space::S16))
            .when(c + 1 == cols, |d| d.pr(space::S16))
            .flex()
            .one_line();
        let el = if numeric { el.justify_end() } else { el };
        if header {
            el.t_meta_medium().text_color(k.text_2).child(text)
        } else if numeric {
            el.child(mono(text, k.text_1).text_size(crate::ui::native::tokens::text::MONO.0))
        } else {
            el.t_small().text_color(k.text_1).child(text)
        }
    };
    let n = ui::native::native_colors(cx);
    let mut head = div()
        .h(TABLE_ROW)
        .flex()
        .items_center()
        .border_y_1()
        .border_color(n.separator)
        .bg(n.fill);
    for (i, h) in t.header.iter().enumerate() {
        head = head.child(cell(
            h.clone(),
            t.numeric.get(i).copied().unwrap_or(false),
            true,
            i,
        ));
    }
    let mut body = div().flex().flex_col();
    for r in t.rows.iter().take(TABLE_ROWS) {
        let mut row = div()
            .h(TABLE_ROW)
            .flex()
            .items_center()
            .border_b_1()
            .border_color(n.separator);
        for c in 0..cols {
            row = row.child(cell(
                r.get(c).cloned().unwrap_or_default(),
                t.numeric.get(c).copied().unwrap_or(false),
                false,
                c,
            ));
        }
        body = body.child(row);
    }
    let s = spec.clone();
    card(cx)
        .w(INLINE_W)
        .max_w_full()
        .child(
            card_header(Sym::Table, media.file_name().to_string(), cx)
                .child(mono(
                    fill(
                        cm::TABLE_META,
                        &[
                            ("rows", &group_digits(t.total_rows as u64)),
                            ("cols", &cols.to_string()),
                        ],
                    ),
                    k.text_3,
                ))
                .child(div().flex_1())
                .child(open_tool(spec, "open", Opening::default())),
        )
        .child(head)
        .child(body)
        .child(
            div()
                .id(ElementId::Name(format!("{}-all", spec.id).into()))
                .focus_row(false, cx)
                .h(crate::ui::native::tokens::height::TABLE_ROW_COMPACT)
                .px(space::S16)
                .flex()
                .items_center()
                .gap(space::S4)
                .t_meta()
                .text_color(k.text_2)
                .child(fill(
                    cm::TABLE_SHOW_ALL,
                    &[("n", &group_digits(t.total_rows as u64))],
                ))
                .child(ui::icon(Sym::ChevronRight, px(13.), k.text_2))
                .on_click(move |_, _, cx| open(&s, Opening::default(), cx)),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::pick_poster;

    #[test]
    fn the_sender_s_poster_wins() {
        let (a, b) = (Some("sent.jpg".into()), Some("made.jpg".into()));
        assert_eq!(pick_poster(a.clone(), b.clone()), a);
        assert_eq!(pick_poster(None, b.clone()), b);
        assert_eq!(pick_poster(None, None), None);
    }
}
