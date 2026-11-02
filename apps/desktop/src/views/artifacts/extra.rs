//! 产出 references in a message: one card per reference (title, version,
//! author). A click opens that version in the 产出 window.

use gpui::{
    AnyElement, App, ElementId, InteractiveElement as _, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div, px,
};
use gpui_component::spinner::Spinner;

use super::{CARD_H, CARD_W, window};
use crate::{
    copy::{artifacts as ca, fill},
    data::{ArtifactKind, Message},
    theme::{self, Type as _, space},
    ui::{self, Lucide},
    views::timeline::extras::{ExtrasContext, MessageExtra},
};

pub struct ArtifactRefsExtra;

impl MessageExtra for ArtifactRefsExtra {
    fn render(
        &self,
        message: &Message,
        ctx: &ExtrasContext,
        _window: &mut Window,
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
                        ArtifactKind::Page => (ca::CARD_KIND_PAGE, Lucide::FileCode),
                        _ => (ca::CARD_KIND_FILE, Lucide::FileText),
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
                    Lucide::FileCode,
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
                    .rounded(theme::radius::LG)
                    .border_1()
                    .border_color(k.border)
                    .bg(k.content)
                    .hover(|s| s.border_color(k.border_strong))
                    .active(|s| s.bg(k.hover))
                    .cursor_pointer()
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
                        Spinner::new().into_any_element()
                    } else {
                        ui::icon(Lucide::ExternalLink, theme::icon::HEADER, k.text_2)
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
