//! 做完了 (today): one row per result, `Ledger 做好了 10 月数据月报`, with its
//! proof under it: a 产出 preview, or chips for a PR, tests and a recording.
//! A check count reads as a quiet detail after the line (`24 项都通过`).

use gpui::{
    AnyElement, App, Hsla, InteractiveElement as _, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement as _, Styled, div, px,
};

use crate::{
    app::Services,
    copy::{desk as c, fill},
    data::{
        ArtifactKind,
        v2::{DoneItem, Proof},
    },
    theme::{self, Type as _, space},
    ui::{self, Sym},
    views::shell::clock,
};

/// A proof chip: 26 pt, 1 pt `line`, radius 7, 12 pt `ink-2`, an optional
/// leading glyph.
pub fn chip(
    id: impl Into<SharedString>,
    glyph: Option<(Sym, Hsla)>,
    parts: Vec<AnyElement>,
    cx: &App,
) -> AnyElement {
    let k = theme::colors(cx);
    div()
        .id(id.into())
        .flex_none()
        .flex()
        .items_center()
        .gap(space::S6)
        .h(theme::card_v2::PROOF)
        .px(space::S8)
        .bg(k.sunken)
        .rounded(theme::card_v2::CHIP_RADIUS)
        .t_meta()
        .text_color(k.ink_2)
        .children(glyph.map(|(g, color)| ui::icon(g, px(13.), color)))
        .children(parts)
        .into_any_element()
}

/// `148 项通过`: the leading number in mono, the rest as text.
pub fn mono_lead(text: &str, _cx: &App) -> Vec<AnyElement> {
    let split = text
        .char_indices()
        .find(|(_, ch)| !(ch.is_ascii_digit() || matches!(ch, ',' | '.' | ':' | '%' | '+' | '-')))
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    let (num, rest) = text.split_at(split);
    let mut out = Vec::new();
    if !num.is_empty() {
        out.push(div().mono().child(num.to_string()).into_any_element());
    }
    let rest = rest.trim();
    if !rest.is_empty() {
        out.push(div().child(rest.to_string()).into_any_element());
    }
    out
}

/// `{n}` of a template in mono, the rest as text: `测试 148 项通过`.
fn templated(template: &str, key: &str, value: &str) -> Vec<AnyElement> {
    let token = format!("{{{key}}}");
    let (before, after) = template.split_once(&token).unwrap_or((template, ""));
    let mut out = Vec::new();
    if !before.trim().is_empty() {
        out.push(div().child(before.trim().to_string()).into_any_element());
    }
    out.push(div().mono().child(value.to_string()).into_any_element());
    if !after.trim().is_empty() {
        out.push(div().child(after.trim().to_string()).into_any_element());
    }
    out
}

/// `0:42`.
pub fn duration(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

fn kind_name(kind: Option<ArtifactKind>) -> &'static str {
    match kind {
        Some(ArtifactKind::File) => c::DESK_KIND_FILE,
        Some(ArtifactKind::Image) => c::DESK_KIND_IMAGE,
        Some(ArtifactKind::Decision) => c::DESK_KIND_DECISION,
        _ => c::DESK_KIND_PAGE,
    }
}

/// The miniature of a 网页 产出: a few bars in a `sunken` tile, the last
/// one strong. It marks the kind; it is not the page's data.
fn page_thumb(cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    const BARS: [f32; 5] = [9., 13., 11., 16., 20.];
    div()
        .flex_none()
        .w(px(76.))
        .h(px(46.))
        .rounded(px(6.))
        .bg(k.sunken)
        .px(px(7.))
        .py(px(6.))
        .flex()
        .flex_col()
        .justify_between()
        .child(
            div()
                .flex()
                .gap(px(4.))
                .child(div().w(px(14.)).h(px(3.)).rounded(px(1.5)).bg(k.line_2))
                .child(div().w(px(10.)).h(px(3.)).rounded(px(1.5)).bg(k.line_2)),
        )
        .child(div().flex().items_end().gap(px(3.)).h(px(20.)).children(
            BARS.iter().enumerate().map(|(i, h)| {
                div()
                    .w(px(8.))
                    .h(px(*h))
                    .rounded(px(1.5))
                    .bg(if i + 1 == BARS.len() {
                        k.ink_3
                    } else {
                        k.line_2
                    })
            }),
        ))
        .into_any_element()
}

/// The 产出 preview card under a result (340 pt).
fn artifact_card(
    services: &Services,
    group_id: &str,
    artifact_id: &str,
    title: &str,
    version: u32,
    kind: Option<ArtifactKind>,
    cx: &App,
) -> AnyElement {
    let k = theme::colors(cx);
    let _ = group_id;
    let nav = services.nav.clone();
    let id = artifact_id.to_string();
    div()
        .id(SharedString::from(format!("desk-art-{artifact_id}")))
        .flex()
        .items_center()
        .gap(space::S12)
        .p(space::S8)
        .w(px(340.))
        .max_w_full()
        .border_1()
        .border_color(k.line)
        .rounded(px(9.))
        .bg(k.bg)
        .shadow(k.lift())
        .cursor_pointer()
        .child(page_thumb(cx))
        .child(
            div()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(
                    div()
                        .one_line()
                        .t_small()
                        .medium()
                        .text_color(k.ink)
                        .child(title.to_string()),
                )
                .child(
                    div()
                        .t_meta()
                        .line_height(px(18.))
                        .mono()
                        .text_color(k.ink_3)
                        .child(fill(
                            c::DESK_ARTIFACT_META,
                            &[("v", &version.to_string()), ("kind", kind_name(kind))],
                        )),
                ),
        )
        .on_click(move |_, _, cx| {
            let id = id.clone();
            nav.update(cx, |nav, cx| nav.open_artifact(id, Some(version), cx));
        })
        .into_any_element()
}

/// One row of 做完了. `ix` is its place in the first-paint stagger.
pub fn render(
    d: &DoneItem,
    ix: usize,
    snap: &super::Snapshot,
    services: &Services,
    cx: &App,
) -> AnyElement {
    let k = theme::colors(cx);
    let kinds = &snap.artifact_kinds;
    let member = snap.member(&d.member_id);
    let name = snap.name(&d.member_id);
    let key = format!("{}-{}", d.topic_id, d.at);

    let mut detail = None;
    let mut chips = Vec::new();
    let mut previews = Vec::new();
    for (i, proof) in d.proof.iter().enumerate() {
        let id = format!("desk-done-chip-{key}-{i}");
        match proof {
            Proof::Checks { passed, total } => {
                detail = Some(if passed == total {
                    fill(c::DESK_CHECKS_ALL, &[("n", &total.to_string())])
                } else {
                    fill(
                        c::DESK_CHECKS_SOME,
                        &[
                            ("passed", &passed.to_string()),
                            ("total", &total.to_string()),
                        ],
                    )
                });
            }
            Proof::Pr {
                number,
                additions,
                deletions,
            } => chips.push(chip(
                id,
                Some((Sym::PullRequest, k.ink_3)),
                vec![
                    div().mono().child(format!("#{number}")).into_any_element(),
                    div()
                        .mono()
                        .text_color(k.ok_ink)
                        .child(format!("+{additions}"))
                        .into_any_element(),
                    div()
                        .mono()
                        .text_color(k.bad_ink)
                        .child(format!("−{deletions}"))
                        .into_any_element(),
                ],
                cx,
            )),
            Proof::Tests { passed, failed } => {
                let (glyph, parts) = if *failed > 0 {
                    (
                        (Sym::Close, k.bad),
                        templated(c::DESK_TESTS_FAILED, "n", &failed.to_string()),
                    )
                } else {
                    (
                        (Sym::Check, k.ok),
                        templated(c::DESK_TESTS_PASSED, "n", &passed.to_string()),
                    )
                };
                chips.push(chip(id, Some(glyph), parts, cx));
            }
            Proof::Recording { duration_ms } => chips.push(chip(
                id,
                Some((Sym::Play, k.ink_3)),
                templated(c::DESK_RECORDING, "t", &duration(*duration_ms)),
                cx,
            )),
            Proof::Artifact {
                artifact_id,
                title,
                version,
            } => previews.push(artifact_card(
                services,
                &d.group_id,
                artifact_id,
                title,
                *version,
                kinds.get(artifact_id).copied(),
                cx,
            )),
        }
    }

    let nav = services.nav.clone();
    let (g, t) = (d.group_id.clone(), d.topic_id.clone());
    let has_chips = !chips.is_empty();
    let row = div()
        .id(SharedString::from(format!("desk-done-{key}")))
        .flex()
        .gap(space::S12)
        .p(space::S12)
        .mx(px(-12.))
        .rounded(px(10.))
        .cursor_pointer()
        .hover(|s| s.bg(k.hover))
        .child(
            div()
                .flex_none()
                .mt(px(1.))
                .child(ui::MemberAvatar::new(member.as_ref(), px(20.))),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(space::S8)
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(space::S8)
                        .child(
                            div()
                                .min_w(px(0.))
                                .text_size(px(14.))
                                .line_height(px(22.))
                                .medium()
                                .text_color(k.ink)
                                .child(format!("{name} {}", d.title)),
                        )
                        .children(
                            detail
                                .map(|t| div().flex_none().t_small().text_color(k.ink_3).child(t)),
                        )
                        .child(
                            div()
                                .ml_auto()
                                .flex_none()
                                .t_meta()
                                .mono()
                                .text_color(k.ink_3)
                                .child(clock(d.at)),
                        ),
                )
                .children(previews)
                .children(
                    has_chips.then(|| div().flex().flex_wrap().gap(space::S6).children(chips)),
                ),
        )
        .on_click(move |_, _, cx| {
            let (g, t) = (g.clone(), t.clone());
            nav.update(cx, |nav, cx| nav.open_topic(g, t, cx));
        });
    ui::motion::rise(
        SharedString::from(format!("desk-done-rise-{key}")),
        ix,
        row,
        cx,
    )
}
