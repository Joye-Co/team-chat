//! The compact looks of a v2 card: the one-line overview (strip and title),
//! the system notification banner, and the collapsed plan row of a topic.

use gpui::{
    AnyElement, App, AppContext as _, FontWeight, InteractiveElement as _, IntoElement, ObjectFit,
    ParentElement, StatefulInteractiveElement as _, Styled, StyledImage as _, Window, div, img,
    prelude::FluentBuilder as _, px,
};

use super::{CardEnv, Ctx, button, frame, meta, state, strip, submit_action};
use crate::{
    copy::{cards_v2 as c, fill},
    data::cards_v2::{CardActionKind, CardIntent, CardV2Block, CardV2Message},
    theme::{self, Type as _, card_v2 as m, space},
    ui::{self, PushStyle, Ring, Sym, native::NativeType as _},
};

/// The state overview: the strip, then the title on one line.
pub(crate) fn overview(ctx: &Ctx, cx: &App) -> AnyElement {
    let n = ui::native::native_colors(cx);
    let waits = matches!(ctx.state, state::CardState::Pending);
    let inset = ui::native::cards::card::INSET;
    frame(waits, cx)
        .flex()
        .flex_col()
        .p(inset)
        .gap(space::S8)
        .children(strip(ctx, cx))
        .child(
            div()
                .n_headline()
                .text_color(n.label)
                .one_line()
                .when(ctx.state.muted(), |el| el.opacity(m::MUTED))
                .child(super::one_line_text(&ctx.card.title)),
        )
        .into_any_element()
}

/// The title and body of a system notification for a card: `Watch：把
/// qqbot v2.14 发布到生产吗？` and the first line of its fallback.
pub fn notification_text(msg: &CardV2Message, author: &str) -> (String, String) {
    let title = fill(
        c::NOTIFY_TITLE,
        &[("name", author), ("title", &msg.card.title)],
    );
    let body = msg
        .card
        .fallback
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && *l != msg.card.title.trim())
        .unwrap_or("")
        .to_string();
    (title, body)
}

/// The notification banner (only 做完了 and 需要你拍板 notify): the app
/// icon, the title, one line, and on hover the card's two answers. An
/// irreversible answer opens the confirm sheet instead of acting.
pub(crate) fn notification(ctx: &Ctx, _window: &mut Window, cx: &App) -> AnyElement {
    let k = theme::colors(cx);
    let (title, body) = notification_text(ctx.msg, &ctx.author);
    let icon = if k.is_dark() {
        ui::icons::APP_ICON_DARK_72
    } else {
        ui::icons::APP_ICON_72
    };
    let live = ctx.live() && ctx.card.intent == CardIntent::Ask;
    let actions: Vec<_> = if live && ctx.env.hovered && !super::takes_input(ctx.card) {
        let first = submit_action(ctx.card).cloned();
        let second = ctx
            .card
            .actions()
            .iter()
            .find(|a| {
                a.kind() != CardActionKind::Link && Some(&a.id) != first.as_ref().map(|f| &f.id)
            })
            .cloned();
        [first, second].into_iter().flatten().collect()
    } else {
        Vec::new()
    };
    let a = ctx.answerer();
    div()
        .w(px(380.))
        .rounded(px(16.))
        .bg(k.bg)
        .shadow(k.pop())
        .p(space::S14)
        .flex()
        .flex_col()
        .gap(space::S10)
        .child(
            div()
                .flex()
                .items_start()
                .gap(space::S10)
                .child(
                    div()
                        .flex_none()
                        .size(px(32.))
                        .rounded(crate::ui::native::tokens::radius::CONTROL)
                        .border_1()
                        .border_color(k.line)
                        .bg(k.bg)
                        .overflow_hidden()
                        .child(img(icon).size_full().object_fit(ObjectFit::Contain)),
                )
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
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .one_line()
                                        .text_size(crate::ui::native::tokens::text::UI.0)
                                        .line_height(px(20.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(k.ink)
                                        .child(title),
                                )
                                .child(meta(div()).text_color(k.ink_3).child(c::NOTIFY_NOW)),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::native::tokens::text::UI.0)
                                .line_height(px(20.))
                                .text_color(k.ink_2)
                                .one_line()
                                .child(body),
                        ),
                ),
        )
        .when(!actions.is_empty(), |el| {
            el.child(
                div()
                    .flex()
                    .gap(space::S8)
                    .children(actions.into_iter().enumerate().map(|(i, action)| {
                        let a = a.clone();
                        let primary = i == 0;
                        let b = button(
                            ctx.id(format!("notify-{i}")),
                            action.label.clone(),
                            if primary {
                                PushStyle::Default
                            } else {
                                PushStyle::Outline
                            },
                            cx,
                        )
                        .flex_1();
                        b.on_click(move |_, window, cx| a.press(&action, window, cx))
                    })),
            )
        })
        .into_any_element()
}

/// The collapsed plan row of a topic (话题里): `5 步都做完了 用时 41 分钟
/// 看计划 ⌄`, or `Forge 在做：跑一万条消息的压测 3/5 实时查看 ›` while the
/// plan runs. `None` when the message has no v2 card with a plan.
pub fn plan_row(
    env: &CardEnv,
    group_id: &str,
    message_id: &str,
    _window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let msg = env.provider.card_v2(group_id, message_id)?;
    if !msg
        .card
        .all_blocks()
        .iter()
        .any(|b| matches!(b, CardV2Block::Plan { .. }))
    {
        return None;
    }
    let k = theme::colors(cx);
    let p = state::progress(&msg.card);
    let member = env.provider.member(&msg.author_id);
    let name = member.as_ref().map(|m| m.name.clone()).unwrap_or_default();
    let working = msg.card.intent == CardIntent::Working;
    let id = format!("cv2-plan-{message_id}");
    let (text, meta_text, action, glyph) = if working {
        (
            format!(
                "{}{}",
                fill(c::PLAN_LIVE_LINE, &[("name", &name)]),
                p.current.clone().unwrap_or_default()
            ),
            p.step
                .map(|s| format!("{s}/{}", p.total))
                .unwrap_or_default(),
            c::PLAN_LIVE_VIEW,
            Sym::ChevronRight,
        )
    } else {
        let mins = msg
            .edited_at
            .map(|e| ((e - msg.created_at).max(0) / 60_000).to_string());
        (
            fill(c::PLAN_ALL_DONE, &[("n", &p.total.to_string())]),
            mins.map(|m| fill(c::PLAN_TOOK, &[("n", &m)]))
                .unwrap_or_default(),
            c::PLAN_SEE_PLAN,
            Sym::ChevronDown,
        )
    };
    let a = super::Answerer {
        provider: env.provider.clone(),
        nav: env.nav.clone(),
        group_id: msg.group_id.clone(),
        topic_id: msg.topic_id.clone(),
        message_id: msg.message_id.clone(),
        card: msg.card.clone(),
        form: cx.new(|_| super::form::CardForm::new(&msg.card, None)),
    };
    let run = msg.card.all_blocks().iter().find_map(|b| match b {
        CardV2Block::Plan { run_id, .. } => run_id.clone(),
        _ => None,
    });
    Some(
        div()
            .id(gpui::SharedString::from(id))
            .flex()
            .items_center()
            .gap(space::S10)
            .h(crate::ui::native::tokens::height::LG)
            .px(space::S12)
            .rounded(ui::native::cards::card::RADIUS)
            .bg(ui::native::native_colors(cx).fill)
            .n_body()
            .child(
                ui::MemberAvatar::new(member.as_ref(), m::TILE).ring(working.then_some(Ring::Live)),
            )
            .child(
                div()
                    .min_w(px(0.))
                    .one_line()
                    .text_color(k.ink)
                    .when(!working, |el| el.font_weight(theme::MEDIUM_WEIGHT))
                    .child(text),
            )
            .child(
                div()
                    .flex_none()
                    .n_subheadline()
                    .n_figures()
                    .text_color(k.ink_3)
                    .child(meta_text),
            )
            .child(
                div()
                    .ml_auto()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(space::S4)
                    .text_color(k.ink_2)
                    .child(action)
                    .child(ui::icon(glyph, px(14.), k.ink_3)),
            )
            .when(working, |el| {
                el.on_click(move |_, window, cx| a.open_live(run.as_deref(), window, cx))
            })
            .into_any_element(),
    )
}
