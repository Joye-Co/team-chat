//! One timeline row (DESIGN.md §7.5, §8.1): what sits above the message (the
//! topic start block on the first row, a date divider when the day changes,
//! the new-messages divider), then the message: a 36 pt gutter (the avatar,
//! or the hover time of a continuation), the meta line (name, bot role tag,
//! time), the steps row, the body, extras, the send state and the hover
//! toolbar. Consecutive messages of one author within five minutes share one
//! meta line.

use gpui::{
    AnyElement, App, ClipboardItem, Context, ElementId, InteractiveElement as _, IntoElement,
    ParentElement, Pixels, SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _,
    button::{Button, ButtonVariants as _},
    input::Textarea,
    menu::{ContextMenuExt as _, DropdownMenu as _},
    popover::Popover,
};

use super::{TimelineView, extras, model};
use crate::ui::NamedButton as _;
use crate::{
    copy::{common, fill, timeline as t},
    data::{Member, Message, SendState, Topic},
    markdown,
    theme::{self, Type as _, space},
    ui::{self, Sym},
};

/// The row's inset inside the content box: hover fills reach 8 pt past the
/// text (§7.5).
const ROW_INSET: Pixels = px(8.);

impl TimelineView {
    pub(super) fn render_row(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(message) = self.messages.get(ix).cloned() else {
            return div().into_any_element();
        };
        let key = model::key(&message).to_string();
        // --- v11/pb-cards: a drawn row is on screen (a card's jump to its reply) ---
        crate::views::cards::v21::interact::mark_drawn(&message.id, cx);
        // --- end v11/pb-cards ---
        let prev = ix.checked_sub(1).and_then(|i| self.messages.get(i));
        let start = ix == 0 && !self.has_older;
        let new_day = model::starts_day(prev, &message) && !start;
        let divider = self.divider_key.as_deref() == Some(key.as_str()) && ix > 0;
        // A v2 row that replaces its message (a routing line, a decision
        // record, a result card) has its own header: it never joins a group,
        // and the next message starts a new one.
        let grouped = !divider
            && !new_day
            && prev.is_some_and(|prev| {
                model::continues_group(prev, &message)
                    && !(self.v2
                        && (self.v2_special(prev).is_some() || self.v2_special(&message).is_some()))
            });
        let last = ix + 1 == self.messages.len();

        let v2 = self.v2;
        let mut item = div()
            .w_full()
            .when(!v2, |el| el.px(theme::CONTENT_GUTTER - ROW_INSET))
            .flex()
            .flex_col()
            .when(last, |el| el.pb(space::S8));
        if ix == 0 {
            // The first message sits 16 pt under the header (§7.5).
            item = item.pt(if start { space::S16 } else { space::S10 });
        }
        if v2 {
            // The v2 page has no start block: the first row is the date.
            if start || new_day {
                item = item.child(super::v2::date_divider(message.created_at, start, cx));
            }
        } else {
            if start && let Some(topic) = self.topic.clone() {
                item = item.child(self.render_start(&topic, cx));
            }
            if new_day {
                item = item.child(date_divider(message.created_at, cx));
            }
        }
        if self.divider_key.as_deref() == Some(key.as_str()) {
            // The first unread row is drawn, so it is on screen (the jump
            // pill must not show), also when it is the first row and has no
            // divider line.
            self.divider_painted = true;
        }
        if divider {
            item = item.child(new_messages_divider(cx));
        }
        let gap = if grouped {
            None
        } else if start && !v2 {
            Some(space::S12)
        } else {
            Some(space::S6)
        };
        let mut message_row = self.render_message(&message, &key, grouped, window, cx);
        if self
            .arrived
            .get(&key)
            .is_some_and(|at| at.elapsed() < theme::motion::FAST)
        {
            // Opacity only, so it stays with reduce motion (§12).
            message_row = ui::motion::fade_in(
                ElementId::Name(format!("arrive-{key}").into()),
                theme::motion::FAST,
                div().child(message_row),
                cx,
            );
        }
        // The plan cards of the members at work follow the last message, in
        // the list so they scroll with the 话题 (Live.dc.html).
        let plans = if v2 && last {
            self.render_plans(cx)
        } else {
            Vec::new()
        };
        let item = item
            .children(gap.map(|h| div().flex_none().h(h)))
            .child(message_row)
            .children(plans);
        if v2 {
            // First paint: the rows of a 话题 that just opened rise in, 40 ms
            // apart from the top (CONCEPT2 §5); later rows only fade.
            let first_paint = self
                .opened_at
                .is_some_and(|at| at.elapsed() < super::v2::FIRST_PAINT);
            let item: AnyElement = if first_paint {
                let from_top = ix.saturating_sub(self.messages.len().saturating_sub(10));
                ui::motion::rise(
                    ElementId::Name(format!("v2-rise-{key}").into()),
                    from_top,
                    item.w_full(),
                    cx,
                )
            } else {
                item.into_any_element()
            };
            // The 720 pt column of the v2 page, centered, 32 pt from the
            // edges; rows reach 8 pt past the text for their hover fill.
            return div()
                .w_full()
                .px(super::v2::SIDE - ROW_INSET)
                .flex()
                .justify_center()
                .child(
                    div()
                        .w_full()
                        .max_w(super::v2::COLUMN + ROW_INSET * 2.)
                        .child(item),
                )
                .into_any_element();
        }
        item.into_any_element()
    }

    fn render_message(
        &mut self,
        message: &Message,
        key: &str,
        grouped: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.v2
            && self.editing.as_ref().is_none_or(|e| e.key != key)
            && let Some(row) = self.v2_special_row(message, key, cx)
        {
            // v12/behavior: the special rows get the message menu too (§7.3).
            let menu_message = message.clone();
            let view = cx.entity().downgrade();
            let (can_edit, can_delete) = (self.can_edit(message), self.can_delete(message));
            return div()
                .id(ElementId::Name(format!("special-{key}").into()))
                .child(row)
                .context_menu(move |menu, window, cx| {
                    super::menu::build(
                        menu,
                        view.clone(),
                        &menu_message,
                        can_edit,
                        can_delete,
                        window,
                        cx,
                    )
                })
                .into_any_element();
        }
        let v2 = self.v2;
        let colors = theme::colors(cx);
        let author = self
            .members
            .iter()
            .find(|m| m.id == message.author_id)
            .cloned();
        let deleted = message.deleted_at.is_some();
        let hovered = self.hovered.as_deref() == Some(key);
        let editing = self
            .editing
            .as_ref()
            .filter(|e| e.key == key)
            .map(|e| e.input.clone());

        let ctx = extras::ExtrasContext {
            provider: &self.services.provider,
            members: &self.members,
            window_messages: &self.messages,
            me: self.me.as_deref(),
        };
        let above = extras::render(extras::Placement::AboveBody, message, &ctx, window, cx);
        let v2_files = if v2 { self.v2_files(message, cx) } else { None };
        let below = if v2_files.is_some() {
            let mut out = vec![];
            out.extend(v2_files);
            out.extend(extras::render_without_files(
                extras::Placement::BelowBody,
                message,
                &ctx,
                window,
                cx,
            ));
            out
        } else {
            extras::render(extras::Placement::BelowBody, message, &ctx, window, cx)
        };
        let body_hidden = extras::hides_body(message, &ctx);
        if !message.id.is_empty()
            && self
                .services
                .provider
                .card(&message.group_id, &message.id)
                .is_some()
        {
            self.card_seen = true;
        }
        let steps = (!v2 && !deleted && author.as_ref().is_some_and(Member::is_bot))
            .then(|| self.render_steps(message, cx))
            .flatten();

        // inc-refs: the body without the quote lines of its references (drawn as chips).
        let shown_body = crate::views::refs::chips::shown_body(message, &self.services.provider);
        let edited = message.edited_at.is_some() && !deleted;
        // The mark follows the last line (§7.5, U12): inside the body text
        // when it can, on its own line only after a code block or a table.
        let mut mark_inline = false;
        let body: Option<AnyElement> = if deleted {
            Some(
                div()
                    .t_body()
                    .text_color(colors.text_3)
                    .child(t::DELETED)
                    .into_any_element(),
            )
        } else if let Some(input) = editing {
            Some(self.render_editor(&input, window, cx))
        } else if body_hidden {
            None
        } else if shown_body.is_empty() && message.streaming {
            Some(
                div()
                    .t_body()
                    .text_color(colors.text_3)
                    .child(t::STREAMING)
                    .into_any_element(),
            )
        } else if shown_body.is_empty() {
            None
        } else if model::is_jumbo(shown_body) {
            // The edited mark sits right of the emoji, on its baseline.
            mark_inline = true;
            Some(
                div()
                    .flex()
                    .items_baseline()
                    .child(
                        div()
                            .type_role(theme::type_scale::EMOJI)
                            .child(shown_body.trim().to_string()),
                    )
                    .when(edited, |el| {
                        el.child(
                            div()
                                .ml(space::S4)
                                .t_meta()
                                .text_color(colors.text_3)
                                .child(t::EDITED),
                        )
                    })
                    .into_any_element(),
            )
        } else {
            // inc-refs: tables and code fences of a sent message can be referenced.
            let refs_body = (!message.id.is_empty() && !message.streaming).then(|| {
                crate::views::refs::markdown::RefsBody {
                    group_id: message.group_id.clone(),
                    topic_id: message.topic_id.clone(),
                    message_id: message.id.clone(),
                    author: author.as_ref().map(|a| a.name.clone()).unwrap_or_default(),
                    block_offset: usize::from(shown_body.len() != message.body.len()),
                }
            });
            let (view, inline) = markdown::message_body_refs(
                ElementId::Name(format!("body-{key}").into()),
                shown_body.to_string(),
                &markdown::mention_targets(&self.members, self.me.as_deref()),
                edited,
                Some(&markdown::DiagramContext {
                    message_id: if message.id.is_empty() {
                        message.client_msg_id.clone()
                    } else {
                        message.id.clone()
                    },
                    topic_id: message.topic_id.clone(),
                    streaming: message.streaming,
                }),
                refs_body.as_ref(),
            );
            mark_inline = inline;
            Some(view.w_full().text_color(colors.text_1).into_any_element())
        };
        // Sending (§7.5, §12): after 400 ms the body dims to 60 % with a
        // 14 pt spinner after it, for at least 500 ms. A normal ack is
        // faster, so my sent message does not change at all.
        let body = body.map(|b| {
            let pending = message.send_state == SendState::Pending;
            let spinner_color = colors.text_3;
            ui::motion::busy_gate(
                ElementId::Name(format!("sending-{key}").into()),
                pending,
                move |shown| {
                    div()
                        .w_full()
                        .flex()
                        .items_end()
                        .gap(space::S6)
                        .child(
                            div()
                                .flex_shrink(1.)
                                .min_w_0()
                                .when(shown, |el| el.opacity(0.6))
                                .child(b),
                        )
                        .when(shown, |el| {
                            el.child(
                                div().flex_none().pb(px(3.)).child(
                                    ui::spinner()
                                        .with_size(theme::icon::CHIP)
                                        .color(spinner_color),
                                ),
                            )
                        })
                        .into_any_element()
                },
            )
            .into_any_element()
        });

        let marks = (edited && !mark_inline).then(|| {
            div()
                .t_meta()
                .text_color(colors.text_3)
                .child(t::EDITED)
                .into_any_element()
        });

        let failed = (message.send_state == SendState::Failed).then(|| {
            div()
                .h(px(22.))
                .flex()
                .items_center()
                .gap(space::S8)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(space::S4)
                        .t_meta()
                        .text_color(colors.danger_text)
                        .child(ui::icon(
                            Sym::Alert,
                            theme::icon::CHIP,
                            colors.danger_text,
                        ))
                        .child(t::FAILED),
                )
                .child(
                    ui::text_button(ElementId::Name(format!("retry-{key}").into()), t::RETRY, cx)
                        .on_click(cx.listener({
                            let message = message.clone();
                            move |this, _, _, _| this.retry(&message)
                        })),
                )
                .into_any_element()
        });

        let meta = (!grouped && v2).then(|| {
            div()
                .flex()
                .flex_col()
                .child(self.v2_meta(author.as_ref(), message.created_at, cx))
                .child(div().h(space::S4))
                .into_any_element()
        });
        let meta = meta.or_else(|| {
            (!grouped).then(|| {
                let name = author.as_ref().map(|m| m.name.clone()).unwrap_or_default();
                div()
                    .h(px(22.))
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .child(
                        ui::truncating("sender-name", name)
                            .flex_shrink(1.)
                            .max_w(px(160.))
                            .t_body()
                            .semibold()
                            .text_color(colors.text_1),
                    )
                    .when_some(
                        author
                            .as_ref()
                            .filter(|a| a.is_bot())
                            .and_then(|a| a.role.clone())
                            .filter(|r| !r.trim().is_empty()),
                        |el, role| {
                            el.child(
                                div()
                                    .flex_shrink(1.)
                                    .min_w(px(40.))
                                    .child(ui::RoleTag::new(role)),
                            )
                        },
                    )
                    .child(
                        div()
                            .flex_none()
                            .t_meta()
                            .tnum()
                            .text_color(colors.text_3)
                            .child(model::clock(message.created_at)),
                    )
                    .into_any_element()
            })
        });

        let gutter_w = if v2 {
            super::v2::GUTTER
        } else {
            theme::AVATAR_SIZE
        };
        let gutter = if grouped {
            div()
                .w(gutter_w)
                .flex_none()
                .flex()
                .justify_center()
                .when(hovered, |el| {
                    el.child(
                        div()
                            .t_meta()
                            .line_height(px(22.))
                            .tnum()
                            .text_color(colors.text_3)
                            .child(model::clock(message.created_at)),
                    )
                })
                .into_any_element()
        } else {
            div()
                .w(gutter_w)
                .flex_none()
                .child(if v2 {
                    // v2: tiles and circles only; presence lives in the rail.
                    ui::MemberAvatar::new(author.as_ref(), gutter_w)
                } else {
                    // People show presence; bot tiles carry no pip in the
                    // timeline (§7.5: no status dot on every bot avatar).
                    ui::MemberAvatar::new(author.as_ref(), theme::AVATAR_SIZE)
                        .presence(author.as_ref().filter(|a| !a.is_bot()))
                })
                .into_any_element()
        };

        // The toolbar stays while its emoji picker is open.
        let picking = self
            .reacting
            .as_ref()
            .is_some_and(|(k, from_pill)| k == key && !from_pill);
        let toolbar = ((hovered || picking) && !deleted && editing_is_none(self, key))
            .then(|| self.render_hover_toolbar(message, key, cx));
        let reactions = (!deleted && !message.reactions.is_empty())
            .then(|| self.render_reactions(message, key, cx));

        let menu_message = message.clone();
        let view = cx.entity().downgrade();
        let can_edit = self.can_edit(message);
        let can_delete = self.can_delete(message);
        let hover_key: SharedString = key.to_string().into();
        div()
            .id(ElementId::Name(key.to_string().into()))
            .relative()
            .w_full()
            .px(ROW_INSET)
            .pt(if grouped {
                space::S2
            } else if v2 {
                space::S4
            } else {
                space::S6
            })
            .pb(if v2 { space::S4 } else { space::S6 })
            .rounded(theme::radius::MD)
            .flex()
            .gap(space::S12)
            // The search highlight, fading out (§7.5).
            .when(self.highlight_alpha(key) > 0., |el| {
                el.bg(colors.me_bg.opacity(self.highlight_alpha(key)))
            })
            .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                let key = hover_key.to_string();
                if *hovering {
                    if this.hovered.as_deref() != Some(key.as_str()) {
                        this.hovered = Some(key);
                        cx.notify();
                    }
                } else if this.hovered.as_deref() == Some(key.as_str()) {
                    this.hovered = None;
                    cx.notify();
                }
            }))
            .child(gutter)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .when(!v2, |el| el.max_w(theme::MEASURE))
                    .flex()
                    .flex_col()
                    .children(meta)
                    .children(steps)
                    .children(above)
                    .children(body)
                    .children(marks)
                    .children(below)
                    .children(reactions)
                    .children(failed),
            )
            .children(toolbar)
            .context_menu(move |menu, window, cx| {
                super::menu::build(
                    menu,
                    view.clone(),
                    &menu_message,
                    can_edit,
                    can_delete,
                    window,
                    cx,
                )
            })
            .into_any_element()
    }

    /// The hover toolbar (§7.5): a level-1 capsule 18 pt above the row, at
    /// the right edge of the content box: add reaction (the emoji picker),
    /// reply (quotes into the composer), copy as quote and more (the row
    /// menu).
    fn render_hover_toolbar(
        &self,
        message: &Message,
        key: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let sent = !message.id.is_empty();
        let menu_message = message.clone();
        let view = cx.entity().downgrade();
        let can_edit = self.can_edit(message);
        let can_delete = self.can_delete(message);
        let tool = |id: &str, glyph: Sym| {
            ui::icon_button(ElementId::Name(format!("{id}-{key}").into()), glyph)
                .rounded(theme::radius::SM)
        };
        ui::capsule(cx)
            .id(ElementId::Name(format!("hover-bar-{key}").into()))
            .absolute()
            .top(px(-18.))
            .right(ROW_INSET)
            .gap(space::S2)
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .when(sent, |el| {
                el.child(self.reaction_picker(
                    message,
                    key,
                    false,
                    tool("react", Sym::Emoji).named(t::ADD_REACTION),
                    cx,
                ))
                .child(
                    tool("reply", Sym::Reply)
                        .named(t::REPLY)
                        .on_click(cx.listener({
                            let m = message.clone();
                            move |this, _, _, cx| this.quote(&m, cx)
                        })),
                )
            })
            .child({
                let quoted = quote_markdown(&message.body);
                tool("copy-quote", Sym::Quote)
                    .named(t::COPY_QUOTE)
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(quoted.clone()))
                    })
            })
            .child(tool("more", Sym::More).named(t::MORE).dropdown_menu(
                move |menu, window, cx| {
                    super::menu::build(
                        menu,
                        view.clone(),
                        &menu_message,
                        can_edit,
                        can_delete,
                        window,
                        cx,
                    )
                },
            ))
            .into_any_element()
    }

    /// The reactions under the body and attachments (§7.5): 26 pt pills 6 pt
    /// apart, then the add pill while the message is hovered. A click toggles my reaction; the tooltip
    /// names everyone who reacted.
    fn render_reactions(&self, message: &Message, key: &str, cx: &mut Context<Self>) -> AnyElement {
        let me = self.me.clone().unwrap_or_default();
        let name = |id: &str| {
            self.members
                .iter()
                .find(|m| m.id == id)
                .map(|m| m.name.clone())
                .unwrap_or_default()
        };
        let pills = message
            .reactions
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let names: Vec<String> = r.member_ids.iter().map(|id| name(id)).collect();
                let mine = r.member_ids.contains(&me);
                let label = ui::emoji::reaction_label(&names);
                let known: Vec<&str> = names
                    .iter()
                    .filter(|n| !n.is_empty())
                    .map(String::as_str)
                    .collect();
                let tip = fill(
                    t::REACTED_BY,
                    &[("names", &known.join("、")), ("emoji", &r.emoji)],
                );
                let emoji = r.emoji.clone();
                let m = message.clone();
                ui::emoji::reaction_pill(
                    ElementId::Name(format!("reaction-{key}-{i}").into()),
                    r.emoji.clone().into(),
                    label.into(),
                    mine,
                    cx,
                )
                .tooltip(tip)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.react(&m, &emoji, !mine, cx);
                }))
                .into_any_element()
            })
            .collect::<Vec<_>>();
        // Like Slack, the add pill shows on the hovered message (or while its
        // picker is open); it keeps its place so the line never reflows.
        let show_add = self.hovered.as_deref() == Some(key)
            || self.reacting.as_ref() == Some(&(key.to_string(), true));
        let add = self.reaction_picker(
            message,
            key,
            true,
            ui::emoji::reaction_add(ElementId::Name(format!("reaction-add-{key}").into()), cx)
                .named(t::ADD_REACTION),
            cx,
        );
        // The add pill rides with the last reaction in one unbreakable
        // group, so the hidden pill never wraps onto a blank line of its own.
        let mut pills = pills;
        let last = pills.pop();
        div()
            .mt(space::S6)
            .flex()
            .flex_wrap()
            .gap(space::S6)
            .children(pills)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .gap(space::S6)
                    .children(last)
                    .child(div().when(!show_add, |el| el.invisible()).child(add)),
            )
            .into_any_element()
    }

    /// The emoji picker (the composer's grid) in a popover on `trigger`:
    /// the hover toolbar's add reaction, or the add pill (`from_pill`).
    /// Picking an emoji adds my reaction, or removes it when it is there.
    fn reaction_picker(
        &self,
        message: &Message,
        key: &str,
        from_pill: bool,
        trigger: Button,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = self.reacting.as_ref() == Some(&(key.to_string(), from_pill));
        let view = cx.entity().downgrade();
        let me = self.me.clone().unwrap_or_default();
        let m = message.clone();
        let close = cx.entity().downgrade();
        let open_key = key.to_string();
        Popover::new(ElementId::Name(
            format!("react-picker-{key}-{from_pill}").into(),
        ))
        .anchor(if from_pill {
            gpui::Anchor::BottomLeft
        } else {
            gpui::Anchor::TopRight
        })
        .appearance(false)
        .trigger(gpui_component::Selectable::selected(trigger, open))
        .open(open)
        .on_open_change(move |open, _, cx| {
            let key = open_key.clone();
            let open = *open;
            close
                .update(cx, |this, cx| {
                    this.reacting = open.then_some((key, from_pill));
                    cx.notify();
                })
                .ok();
        })
        .content(move |_, _, cx| {
            let view = view.clone();
            let m = m.clone();
            let me = me.clone();
            let dismiss = view.clone();
            ui::menu_surface(cx)
                .w(ui::emoji::picker_width())
                .flex_row()
                .flex_wrap()
                .on_mouse_down_out(move |_, _, cx| {
                    dismiss
                        .update(cx, |this, cx| {
                            this.reacting = None;
                            cx.notify();
                        })
                        .ok();
                })
                .children(ui::emoji::EMOJI.iter().enumerate().map(|(i, e)| {
                    let view = view.clone();
                    let m = m.clone();
                    let on = !m
                        .reactions
                        .iter()
                        .any(|r| r.emoji == *e && r.member_ids.contains(&me));
                    ui::emoji::picker_cell(("react-emoji", i), e, cx).on_click(move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.reacting = None;
                            this.react(&m, e, on, cx);
                        })
                        .ok();
                    })
                }))
        })
        .into_any_element()
    }

    /// Adds (`on`) or removes my reaction.
    pub(super) fn react(
        &mut self,
        message: &Message,
        emoji: &str,
        on: bool,
        cx: &mut Context<Self>,
    ) {
        if message.id.is_empty() {
            return;
        }
        self.services
            .provider
            .react(&message.group_id, &message.topic_id, &message.id, emoji, on);
        cx.notify();
    }

    /// The topic start block (§8.1): the responsible member at 48 pt, the
    /// title in `display` (two lines at most), `Joye 发起 · Taro 负责 · 今天
    /// 14:05`, and a 1 pt rule under it.
    fn render_start(&self, topic: &Topic, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme::colors(cx);
        let name = |id: &str| {
            self.members
                .iter()
                .find(|m| m.id == id)
                .map(|m| m.name.clone())
                .unwrap_or_default()
        };
        let starter = name(&topic.created_by);
        let people = match &topic.owner_id {
            Some(owner) => fill(
                t::STARTED_LINE,
                &[("starter", &starter), ("owner", &name(owner))],
            ),
            None => fill(t::STARTED_LINE_NO_OWNER, &[("starter", &starter)]),
        };
        let when = match model::local_day(topic.created_at) {
            Some(day) => format!(
                "{} {}",
                model::day_label(day, crate::selftest::clock::local_now().date_naive()),
                model::clock(topic.created_at)
            ),
            None => String::new(),
        };
        let lead_id = topic.owner_id.as_deref().unwrap_or(&topic.created_by);
        let lead = self.members.iter().find(|m| m.id == lead_id);
        let (title, untitled) = crate::views::sidebar::topic_display_title(&topic.title);
        // How far the top of row 0 is scrolled out of view: each part of
        // the block fades as it reaches the top edge, so no half-cut title
        // or meta line shows under the header (§4.4).
        let cut = self.start_cut;
        let title_h = self.start_title_h.clone();
        let fade = StartFade::at(cut, title_h.get());
        div()
            .mx(ROW_INSET)
            .pt(space::S8)
            .pb(space::S16)
            .border_b_1()
            .border_color(colors.border)
            .flex()
            .flex_col()
            .child(
                div()
                    .mb(space::S12)
                    .opacity(fade.avatar)
                    .child(ui::MemberAvatar::new(lead, theme::size::AVATAR_START)),
            )
            .child(
                div()
                    .relative()
                    .t_display()
                    .text_color(if untitled {
                        colors.text_2
                    } else {
                        colors.text_1
                    })
                    .line_clamp(2)
                    .opacity(fade.title)
                    .child(title)
                    .child(
                        gpui::canvas(
                            move |bounds, _, _| title_h.set(bounds.size.height),
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full(),
                    ),
            )
            .child(
                div()
                    .mt(space::S8)
                    .opacity(fade.meta)
                    .min_w(px(0.))
                    .flex()
                    .t_small()
                    .text_color(colors.text_2)
                    .child(div().min_w(px(0.)).one_line().child(people))
                    .when(!when.is_empty(), |el| {
                        el.child(div().flex_none().tnum().child(format!(" · {when}")))
                    }),
            )
            .into_any_element()
    }

    fn render_editor(
        &self,
        input: &gpui::Entity<gpui_component::input::TextareaState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = theme::colors(cx);
        div()
            .flex()
            .flex_col()
            .gap(space::S8)
            .py(space::S2)
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.cancel_edit(cx);
                }
            }))
            .child(
                div()
                    .rounded(theme::radius::FIELD)
                    .border_1()
                    .border_color(colors.focus_border)
                    .shadow(colors.focus_halo())
                    .bg(colors.content)
                    .px(space::S12 - crate::ui::composer::EDITOR_INSET)
                    .py(space::S8)
                    .t_body()
                    .child(crate::ui::placeholder_input(
                        Textarea::new(input)
                            .appearance(false)
                            .xsmall()
                            .p_0()
                            .t_body(),
                        window,
                        cx,
                    )),
            )
            .child(
                div()
                    .flex()
                    .gap(space::S8)
                    .child(
                        Button::new("edit-save")
                            .primary()
                            .label(common::SAVE)
                            .h(theme::size::BUTTON_SM)
                            .on_click(cx.listener(|this, _, _, cx| this.save_edit(cx))),
                    )
                    .child(
                        ui::secondary_button("edit-cancel", common::CANCEL, cx)
                            .h(theme::size::BUTTON_SM)
                            .on_click(cx.listener(|this, _, _, cx| this.cancel_edit(cx))),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn can_edit(&self, message: &Message) -> bool {
        !message.id.is_empty()
            && message.deleted_at.is_none()
            && message.send_state == SendState::Sent
            && !message.streaming
            && Some(message.author_id.as_str()) == self.me.as_deref()
    }

    pub(super) fn can_delete(&self, message: &Message) -> bool {
        !message.id.is_empty()
            && message.deleted_at.is_none()
            && (Some(message.author_id.as_str()) == self.me.as_deref() || self.i_am_owner)
    }

    pub(super) fn retry(&mut self, message: &Message) {
        self.services
            .provider
            .retry_send(&message.group_id, &message.client_msg_id);
    }

    /// Opens the hover toolbar's emoji picker on row `ix` (screenshot scenes).
    pub fn preview_reacting(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.reacting = self
            .messages
            .get(ix)
            .map(|m| (model::key(m).to_string(), false));
        cx.notify();
    }

    /// Shows the hover toolbar on row `ix` (screenshot scenes).
    pub fn preview_hover(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.hovered = self.messages.get(ix).map(|m| model::key(m).to_string());
        cx.notify();
    }
}

fn editing_is_none(view: &TimelineView, key: &str) -> bool {
    view.editing.as_ref().is_none_or(|e| e.key != key)
}

/// A message as a Markdown quote, for 复制为引用.
fn quote_markdown(body: &str) -> String {
    body.lines()
        .map(|l| {
            if l.is_empty() {
                ">".to_string()
            } else {
                format!("> {l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The date divider (§7.5): `12/16 500 text-3` centered between two 0.5 pt
/// `sep` lines across the content box; 16 pt above, 4 below.
fn date_divider(at: crate::data::Millis, cx: &App) -> impl IntoElement {
    let colors = theme::colors(cx);
    let label = model::local_day(at)
        .map(|day| model::day_label(day, crate::selftest::clock::local_now().date_naive()))
        .unwrap_or_default();
    div()
        .mx(ROW_INSET)
        .mt(space::S16)
        .mb(space::S4)
        .h(px(20.))
        .flex()
        .items_center()
        .gap(space::S12)
        .t_meta_medium()
        .text_color(colors.text_3)
        .child(div().flex_1().h(theme::HAIRLINE).bg(colors.sep))
        .child(label)
        .child(div().flex_1().h(theme::HAIRLINE).bg(colors.sep))
}

/// The new-messages divider (§7.5): a `danger` hairline with `新消息`
/// 12/500 `danger-text` at the right.
fn new_messages_divider(cx: &App) -> impl IntoElement {
    let colors = theme::colors(cx);
    div()
        .mx(ROW_INSET)
        .mt(space::S16)
        .mb(space::S4)
        .h(px(20.))
        .flex()
        .items_center()
        .gap(space::S8)
        .t_meta_medium()
        .text_color(colors.danger_text)
        .child(div().flex_1().h(theme::BORDER).bg(colors.danger))
        .child(t::NEW_MESSAGES)
}

#[cfg(test)]
mod tests {
    use super::quote_markdown;

    #[test]
    fn quotes_every_line() {
        assert_eq!(quote_markdown("a\n\nb"), "> a\n>\n> b");
    }
}

/// Opacity of the start block's parts while row 0 scrolls out at the top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct StartFade {
    pub avatar: f32,
    pub title: f32,
    pub meta: f32,
}

impl StartFade {
    /// Row 0's top 16 pt, then the block's 8 pt padding.
    const AVATAR_TOP: f32 = 24.;
    /// A part starts to fade this far before it reaches the top edge, and
    /// is gone when it gets there: the top fade already covers this band,
    /// and a part at the edge would show as a ghost line under the header.
    const LEAD: f32 = 20.;
    /// Cut of a part at which it is gone.
    const GONE: f32 = 0.;

    /// `cut`: how much of row 0 is above the viewport; `title_h`: the
    /// title's height (30 per line).
    pub(super) fn at(cut: Pixels, title_h: Pixels) -> Self {
        let cut = f32::from(cut);
        let part = |top: f32| ((top + Self::GONE - cut) / (Self::LEAD + Self::GONE)).clamp(0., 1.);
        let avatar_top = Self::AVATAR_TOP;
        let title_top = avatar_top + f32::from(theme::size::AVATAR_START) + f32::from(space::S12);
        let meta_top = title_top + f32::from(title_h) + f32::from(space::S8);
        Self {
            avatar: part(avatar_top),
            title: part(title_top),
            meta: part(meta_top),
        }
    }
}

#[cfg(test)]
mod start_fade_tests {
    use super::*;

    #[test]
    fn whole_block_shows_at_the_top() {
        let f = StartFade::at(px(0.), px(30.));
        assert_eq!((f.avatar, f.title, f.meta), (1., 1., 1.));
    }

    #[test]
    fn a_half_cut_title_is_hidden_and_the_meta_line_stays() {
        // Title top is at 84; half of a one-line title (15 pt) is cut.
        let f = StartFade::at(px(99.), px(30.));
        assert_eq!(f.avatar, 0.);
        assert_eq!(f.title, 0.);
        assert_eq!(f.meta, 1.);
    }

    #[test]
    fn a_half_cut_meta_line_is_hidden() {
        // Meta top is at 84 + 60 + 8 = 152 under a two-line title.
        let f = StartFade::at(px(162.), px(60.));
        assert_eq!(f.meta, 0.);
        let f = StartFade::at(px(130.), px(60.));
        assert!(f.meta > 0.9);
    }
}
