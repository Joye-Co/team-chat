//! 产出 references in a message: one card per reference (title, version,
//! author). A click opens that version in the 产出 window.

use gpui::{
    AnyElement, App, ElementId, InteractiveElement as _, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, Window, div, px,
};
use gpui_component::{Sizable as _, button::Button, tooltip::Tooltip};

use super::{CARD_H, CARD_W, window};
use crate::{
    copy::{artifacts as ca, fill},
    data::{ArtifactKind, Message},
    theme::{self, Elevation, Type as _, space},
    ui,
    views::{
        files::extra::{TileKind, tile},
        timeline::extras::{ExtrasContext, MessageExtra},
    },
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
        let mut list = div().mt(space::S6).flex().flex_col().gap(space::S6);
        for (index, r) in message.artifact_refs.iter().enumerate() {
            let detail = ctx.provider.artifact(&r.artifact_id);
            if detail.is_none() {
                ctx.provider
                    .request_artifact(&message.group_id, &message.topic_id, &r.artifact_id);
            }
            let opening = window::is_opening(&r.artifact_id, cx);
            let (title, meta, kind) = match &detail {
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
                    let kind = match d.summary.kind {
                        ArtifactKind::Page => TileKind::Page,
                        _ => TileKind::Text,
                    };
                    let kind_word = match d.summary.kind {
                        ArtifactKind::Page => ca::KIND_PAGE,
                        _ => ca::KIND_FILE,
                    };
                    let mut meta = format!(
                        "{kind_word} · {}",
                        fill(ca::CARD_META, &[("name", &name), ("n", &r.version.to_string())])
                    );
                    if d.summary.version > r.version {
                        meta.push_str(" · ");
                        meta.push_str(&fill(ca::CARD_LATEST, &[("n", &d.summary.version.to_string())]));
                    }
                    (d.summary.title.clone(), meta, kind)
                }
                None => (
                    crate::copy::panel::ARTIFACTS.to_string(),
                    ca::CARD_LOADING.to_string(),
                    TileKind::Page,
                ),
            };
            let key = if message.id.is_empty() { &message.client_msg_id } else { &message.id };
            let open = {
                let provider = ctx.provider.clone();
                let id = r.artifact_id.clone();
                let version = r.version;
                move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                    window::open_artifact(&provider, &id, Some(version), cx)
                }
            };
            let title_tip: SharedString = title.clone().into();
            // §13.5 产出 card: level 1, 10 / 12 padding, a 40 pt tile, the
            // name 13 / 600, meta 12 pt `text-2` and a small 打开 button.
            list = list.child(
                ui::card(cx)
                    .id(ElementId::Name(SharedString::from(format!("artifact-{key}-{index}"))))
                    .w(CARD_W)
                    .max_w_full()
                    .h(CARD_H)
                    .flex_none()
                    .px(space::S12)
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(space::S12)
                    .child(tile(kind, px(40.), cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .id(ElementId::Name(SharedString::from(format!(
                                        "artifact-{key}-{index}-title"
                                    ))))
                                    .t_ui()
                                    .semibold()
                                    .text_color(colors.text_1)
                                    .one_line()
                                    .tooltip(move |window, cx| {
                                        Tooltip::new(title_tip.clone()).build(window, cx)
                                    })
                                    .child(title),
                            )
                            .child(
                                div()
                                    .t_callout()
                                    .tnum()
                                    .text_color(colors.text_2)
                                    .one_line()
                                    .child(if opening { ca::OPENING.to_string() } else { meta }),
                            ),
                    )
                    .child(
                        Button::new(ElementId::Name(SharedString::from(format!(
                            "artifact-{key}-{index}-open"
                        ))))
                        .small()
                        .label(ca::OPEN)
                        .loading(opening)
                        .shadow(colors.shadow(Elevation::Control))
                        .on_click(open.clone()),
                    )
                    .on_click(open),
            );
        }
        Some(list.into_any_element())
    }
}
