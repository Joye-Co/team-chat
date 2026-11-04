//! 产出 references in a message: one card per reference (title, version,
//! author). A click opens that version in the 产出 window.

use gpui::{
    AnyElement, App, ElementId, InteractiveElement as _, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div, px,
};

use super::{CARD_H, CARD_W, window};
use crate::{
    copy::{artifacts as ca, fill},
    data::{ArtifactKind, Message},
    theme::{self, Type as _, space},
    ui::{self, Sym},
    views::timeline::extras::{ExtrasContext, MessageExtra},
};

pub struct ArtifactRefsExtra;

impl MessageExtra for ArtifactRefsExtra {
    fn render(
        &self,
        message: &Message,
        ctx: &ExtrasContext,
        window_: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        if message.artifact_refs.is_empty() {
            return None;
        }
        let k = theme::colors(cx);
        let mut list = div().mt(space::S8).flex().flex_col().gap(space::S8);
        for (index, r) in message.artifact_refs.iter().enumerate() {
            let detail = ctx.provider.artifact(&r.artifact_id);
            if detail.is_none() {
                ctx.provider
                    .request_artifact(&message.group_id, &message.topic_id, &r.artifact_id);
            }
            // --- v11/media: a preview of what the version holds, in place of the file row ---
            if let Some(el) = media_preview(message, r, detail.as_ref(), ctx, index, window_, cx) {
                list = list.child(el);
                continue;
            }
            // --- end v11/media ---
            let opening = window::is_opening(&r.artifact_id, cx);
            let (title, meta, icon) = match &detail {
                Some(d) => {
                    let author = d
                        .versions
                        .iter()
                        .find(|v| v.version == r.version)
                        .map(|v| v.created_by.as_str())
                        .unwrap_or(d.summary.created_by.as_str());
                    let name = ctx
                        .member(author)
                        .map(|m| m.name.clone())
                        .unwrap_or_else(|| crate::copy::composer::UNKNOWN_MEMBER.to_string());
                    let (kind, icon) = match d.summary.kind {
                        // The kind symbol (NATIVE.md §6): a page is `doc.text`, an
                        // image `photo`, any other file `doc`.
                        ArtifactKind::Page => (ca::CARD_KIND_PAGE, Sym::Document),
                        ArtifactKind::Image => (ca::CARD_KIND_FILE, Sym::Image),
                        _ => (ca::CARD_KIND_FILE, Sym::File),
                    };
                    let mut meta = format!(
                        "{kind} · {} · {name}",
                        fill(ca::VERSION_SHORT, &[("n", &r.version.to_string())])
                    );
                    if d.summary.version > r.version {
                        meta.push_str(" · ");
                        meta.push_str(&fill(
                            ca::LATEST_SHORT,
                            &[("n", &d.summary.version.to_string())],
                        ));
                    }
                    (d.summary.title.clone(), meta, icon)
                }
                None => (
                    crate::copy::panel::ARTIFACTS.to_string(),
                    ca::CARD_LOADING.to_string(),
                    Sym::CodeFile,
                ),
            };
            let key = if message.id.is_empty() {
                &message.client_msg_id
            } else {
                &message.id
            };
            let provider = ctx.provider.clone();
            let (id, version) = (r.artifact_id.clone(), r.version);
            // §7.11: the 产出 reference is a file row: 40 pt `accent-soft`
            // tile, name 14/500, meta 12/16 `text-2`, open glyph at the right.
            list = list.child(
                div()
                    .id(ElementId::Name(SharedString::from(format!(
                        "artifact-{key}-{index}"
                    ))))
                    .w_full()
                    .max_w(CARD_W)
                    .h(CARD_H)
                    .flex_none()
                    .pl(space::S12)
                    .pr(space::S16)
                    .rounded(crate::ui::native::cards::card::RADIUS)
                    .bg(crate::ui::native::native_colors(cx).fill)
                    .flex()
                    .items_center()
                    .gap(space::S12)
                    .child(
                        div()
                            .flex_none()
                            .size(px(40.))
                            .rounded(theme::radius::MD)
                            .bg(k.accent_soft)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(ui::icon(icon, theme::icon::EMPTY, k.accent_text)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(space::S2)
                            .child(
                                div()
                                    .t_ui()
                                    .medium()
                                    .text_color(k.text_1)
                                    .one_line()
                                    .child(title),
                            )
                            .child(div().t_meta().tnum().text_color(k.text_2).one_line().child(
                                if opening {
                                    ca::OPENING.to_string()
                                } else {
                                    meta
                                },
                            )),
                    )
                    .child(if opening {
                        crate::ui::spinner().into_any_element()
                    } else {
                        ui::icon(Sym::OpenExternal, theme::icon::HEADER, k.text_2)
                            .into_any_element()
                    })
                    .on_click(move |_, _, cx| {
                        window::open_artifact(&provider, &id, Some(version), cx)
                    }),
            );
        }
        Some(list.into_any_element())
    }
}

// --- v11/media ---
/// The inline media preview of one reference, when its version holds something the app
/// can show (`views/media`). `None` keeps the file row.
fn media_preview(
    message: &Message,
    r: &crate::data::ArtifactRef,
    detail: Option<&crate::data::ArtifactDetail>,
    ctx: &ExtrasContext,
    index: usize,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    use crate::views::media::{self, MediaFile, inline, state::Target, store::MediaKey};
    let d = detail?;
    let v = d.versions.iter().find(|v| v.version == r.version)?;
    if v.entry.is_empty() {
        return None;
    }
    let files: Vec<MediaFile> = v
        .files
        .iter()
        .map(|f| MediaFile {
            path: f.path.clone(),
            size: f.size,
            mime: f.mime.clone(),
        })
        .collect();
    // inc-host (R9): the sender's metadata decides the kind when it came with the version;
    // receivers compute nothing.
    let sent = ctx.provider.artifact_media(&r.artifact_id, r.version);
    let kind = sent
        .as_ref()
        .and_then(|m| media::poster::local_kind(m.kind))
        .unwrap_or_else(|| media::classify(&v.entry, &files));
    let key = if message.id.is_empty() {
        &message.client_msg_id
    } else {
        &message.id
    };
    let spec = inline::InlineSpec {
        key: MediaKey::new(r.artifact_id.clone(), r.version),
        kind,
        title: d.summary.title.clone(),
        bytes: files.iter().map(|f| f.size).sum(),
        target: Target {
            provider: Some(ctx.provider.clone()),
            group_id: message.group_id.clone(),
            topic_id: message.topic_id.clone(),
        },
        id: SharedString::from(format!("media-{key}-{index}")),
    };
    // inc-host (R9): the sender's preview state. 正在生成预览 until `artifact.previewReady`;
    // 预览失败 · 用系统应用打开 when the sender could not make one.
    match sent.as_ref().map(|m| m.preview) {
        Some(chat_core::proto::MediaPreview::Pending) if kind != media::MediaKind::Html => {
            let lead = if kind == media::MediaKind::Video {
                media::states::Lead::Blank
            } else {
                media::states::Lead::Icon(kind.icon())
            };
            return Some(
                media::states::row(
                    SharedString::from(format!("{}-pending", spec.id)),
                    lead,
                    spec.title.clone(),
                    &media::states::Status::Generating {
                        duration: sent
                            .as_ref()
                            .and_then(|m| m.duration.as_ref())
                            .and_then(|d| d.as_f64()),
                    },
                    None,
                    cx,
                )
                .into_any_element(),
            );
        }
        Some(chat_core::proto::MediaPreview::Failed) => {
            return Some(media::states::failed_row(&spec, cx).into_any_element());
        }
        _ => {}
    }
    inline::render(&spec, window, cx)
}
// --- end v11/media ---
