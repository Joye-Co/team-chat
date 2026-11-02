//! 产出 references in a message: one card per reference (title, version,
//! author). A click opens that version in the 产出 window.

use gpui::{
    AnyElement, App, ElementId, InteractiveElement as _, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{Icon, IconName, spinner::Spinner};

use super::{CARD_H, CARD_W, window};
use crate::{
    copy::{artifacts as ca, fill},
    data::{ArtifactKind, Message},
    theme,
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
        let colors = theme::colors(cx);
        let mut list = div().mt(px(4.)).flex().flex_col().gap(px(4.));
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
                    let mut meta = fill(ca::CARD_META, &[("name", &name), ("n", &r.version.to_string())]);
                    if d.summary.version > r.version {
                        meta.push_str(" · ");
                        meta.push_str(&fill(ca::CARD_LATEST, &[("n", &d.summary.version.to_string())]));
                    }
                    let icon = match d.summary.kind {
                        ArtifactKind::Page => IconName::Globe,
                        _ => IconName::FileText,
                    };
                    (d.summary.title.clone(), meta, icon)
                }
                None => (
                    crate::copy::panel::ARTIFACTS.to_string(),
                    ca::CARD_LOADING.to_string(),
                    IconName::Globe,
                ),
            };
            let key = if message.id.is_empty() { &message.client_msg_id } else { &message.id };
            let provider = ctx.provider.clone();
            let (id, version) = (r.artifact_id.clone(), r.version);
            list = list.child(
                div()
                    .id(ElementId::Name(SharedString::from(format!("artifact-{key}-{index}"))))
                    .w(CARD_W)
                    .max_w_full()
                    .h(CARD_H)
                    .flex_none()
                    .px(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(colors.border)
                    .bg(colors.background)
                    .hover(|s| s.bg(colors.row_hover))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(Icon::new(icon).text_color(colors.accent).flex_none())
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(colors.muted_foreground)
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(if opening { ca::OPENING.to_string() } else { meta }),
                            ),
                    )
                    .when(opening, |el| el.child(Spinner::new()))
                    .on_click(move |_, _, cx| window::open_artifact(&provider, &id, Some(version), cx)),
            );
        }
        Some(list.into_any_element())
    }
}
