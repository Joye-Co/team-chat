//! The native composer of the 话题 and 群 pages (NATIVE.md §9.2, Messages
//! and Codex): a 28 pt `plus` circle outside the field with a menu (文件…,
//! 图片…, 截图); the field, radius 18, at least 36 pt tall and growing to
//! eight lines, on `content` with a 0.5 pt `separator` edge in light and on
//! `fill` in dark; a 24 pt `accent` send circle with `arrow.up` inside the
//! field, shown only when there is something to send. No @ button: typing
//! `@` opens the member pop-up.

use gpui::{
    AnyElement, Context, ElementId, InteractiveElement as _, IntoElement, ParentElement,
    SharedString, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    Sizable as _,
    button::Button,
    input::Textarea,
    menu::{DropdownMenu as _, PopupMenuItem},
};

use super::ComposerView;
use crate::{
    copy::{composer as cc, fill, topic_rail as c},
    data::NewFile,
    theme::{Type as _, space},
    ui::{
        self, Sym,
        native::{NativeType as _, native_colors},
        sf_symbols::Sf,
    },
};

/// The `plus` circle and the send circle (NATIVE.md §9.2).
const ADD: gpui::Pixels = px(28.);
const SEND: gpui::Pixels = px(24.);
/// The field: radius 18, at least 36 pt tall.
const FIELD_RADIUS: gpui::Pixels = px(18.);
const FIELD_MIN: gpui::Pixels = px(36.);
/// Text inset: 14 pt to the text; gpui-component's `Textarea` keeps 4 pt.
const FIELD_PAD: gpui::Pixels = px(10.);

/// Image files the 图片… item keeps.
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "heic", "webp", "tiff", "bmp"];

impl ComposerView {
    /// 图片…: the system open panel, keeping image files.
    fn pick_images(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(c::ATTACH_MORE.into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await {
                let files = paths
                    .into_iter()
                    .filter(|p| {
                        p.extension()
                            .and_then(|e| e.to_str())
                            .is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
                    })
                    .map(NewFile::Path)
                    .collect();
                let _ = this.update(cx, |this, cx| this.attach(files, cx));
            }
        })
        .detach();
    }

    /// 截图: the system's interactive screenshot (`screencapture -i`); the
    /// picture becomes a chip when the person took one.
    fn take_screenshot(&mut self, cx: &mut Context<Self>) {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        let path = std::env::temp_dir().join(format!("composer-shot-{stamp}.png"));
        let shot = path.clone();
        let task = cx.background_executor().spawn(async move {
            std::process::Command::new("/usr/sbin/screencapture")
                .arg("-i")
                .arg("-x")
                .arg(&shot)
                .status()
                .is_ok_and(|s| s.success())
                && shot.exists()
        });
        cx.spawn(async move |this, cx| {
            if task.await {
                let _ = this.update(cx, |this, cx| this.attach(vec![NewFile::Path(path)], cx));
            }
        })
        .detach();
    }

    /// The attach button and its menu: a 28 ghost icon button inside the
    /// composer frame (COMPLETENESS.md §2.17, F28).
    fn native_add(&self, enabled: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let view = cx.entity().downgrade();
        let b = ui::native::icon_button(
            "composer-add",
            Sf::Plus,
            c::ATTACH_MORE,
            ui::native::IconSize::Small,
            window,
            cx,
        );
        ui::native::disabled_push(b, !enabled, cx)
            .dropdown_menu(move |menu, _, _| {
                let (a, b, s) = (view.clone(), view.clone(), view.clone());
                menu.item(
                    PopupMenuItem::new(c::ATTACH_FILE).on_click(move |_, _, cx| {
                        let _ = a.update(cx, |this, cx| this.pick_files(cx));
                    }),
                )
                .item(
                    PopupMenuItem::new(c::ATTACH_IMAGE).on_click(move |_, _, cx| {
                        let _ = b.update(cx, |this, cx| this.pick_images(cx));
                    }),
                )
                .item(
                    PopupMenuItem::new(c::ATTACH_SCREENSHOT).on_click(move |_, _, cx| {
                        let _ = s.update(cx, |this, cx| this.take_screenshot(cx));
                    }),
                )
            })
            .into_any_element()
    }

    /// The mention button: a 28 ghost icon button that types `@`.
    fn native_mention(&self, enabled: bool, window: &mut Window, cx: &mut Context<Self>) -> Button {
        let b = ui::native::icon_button(
            "composer-mention",
            Sym::Mention,
            cc::MENTION,
            ui::native::IconSize::Small,
            window,
            cx,
        )
        .on_click(cx.listener(|this, _, window, cx| this.insert_text("@", window, cx)));
        ui::native::disabled_push(b, !enabled, cx)
    }

    /// Send (§2.17): a primary sm icon button with `arrow.up` (`checkmark`
    /// while editing), at 50 % until there is something to send.
    fn native_send(
        &self,
        editing: bool,
        ready: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Button {
        let b = ui::native::icon_button_styled(
            "send",
            if editing { Sym::Check } else { Sym::ArrowUp },
            if editing {
                crate::copy::common::SAVE
            } else {
                c::SEND
            },
            ui::native::PushStyle::Default,
            ui::native::IconSize::Small,
            window,
            cx,
        )
        .when(ready, |b| {
            b.on_click(cx.listener(|this, _, window, cx| this.submit(window, cx)))
        });
        ui::native::disabled_push(b, !ready, cx)
    }

    /// The native composer. `group` is the 群 page's (starts a 话题).
    pub(super) fn render_native(
        &mut self,
        group: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = native_colors(cx);
        let enabled = self.key.is_some();
        let editing = self.editing.is_some();
        let new_topic = !editing && group;

        let mut above: Vec<AnyElement> = Vec::new();
        if editing {
            above.push(
                div()
                    .flex()
                    .items_center()
                    .gap(space::S8)
                    .n_callout()
                    .child(div().text_color(k.label).child(cc::EDITING))
                    .child(
                        ui::native::borderless_text("cancel-edit", crate::copy::common::CANCEL, cx)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.cancel_edit(window, cx)),
                            ),
                    )
                    .into_any_element(),
            );
        }
        if let Some(title) = new_topic
            .then(|| super::model::topic_title(&self.last_text))
            .flatten()
        {
            above.push(
                div()
                    .n_callout()
                    .text_color(k.secondary)
                    .one_line()
                    .child(fill(cc::NEW_TOPIC_TITLE, &[("title", &title)]))
                    .into_any_element(),
            );
        }
        let mut chips: Vec<AnyElement> = Vec::new();
        if !editing {
            if let Some(q) = self.quote.clone() {
                chips.push(
                    self.render_chip(
                        "quote-remove".into(),
                        Sym::Quote,
                        q.text,
                        |this, cx| this.remove_quote(cx),
                        cx,
                    )
                    .into_any_element(),
                );
            }
            // §2.17: at most 2 rows of reference chips, the rest fold into "+N".
            let ref_labels: Vec<String> = self.refs.iter().map(|r| r.input.label.clone()).collect();
            chips.extend(crate::views::refs::chips::capped(
                self.render_refs(cx),
                &ref_labels,
            ));
            for file in self.files.clone() {
                let id = file.id.clone();
                chips.push(
                    self.render_chip(
                        ElementId::Name(SharedString::from(format!("file-remove-{}", file.id))),
                        Sym::Attach,
                        file.name,
                        move |this, cx| this.remove_file(&id, cx),
                        cx,
                    )
                    .into_any_element(),
                );
            }
        }
        let has_chips = !chips.is_empty();
        if has_chips {
            above.push(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(space::S6)
                    .children(chips)
                    .into_any_element(),
            );
        }
        if self.too_long {
            above.push(
                div()
                    .n_callout()
                    .text_color(k.red)
                    .child(cc::TOO_LONG)
                    .into_any_element(),
            );
        }
        for line in &self.rejected {
            above.push(
                div()
                    .n_callout()
                    .text_color(k.red)
                    .child(line.clone())
                    .into_any_element(),
            );
        }

        let has_text = !self.last_text.trim().is_empty() || has_chips;
        let send = self.native_send(editing, has_text && enabled, window, cx);
        let mention = (!editing).then(|| self.native_mention(enabled, window, cx));
        let editor = ui::placeholder_input(
            Textarea::new(&self.input)
                .appearance(false)
                .xsmall()
                .p_0()
                .n_message()
                .disabled(!enabled),
            window,
            cx,
        );
        // The composer hint (12 muted, §2.17): offline, messages wait for
        // the connection (§2.15); else who reads it next. It sits in the
        // tool row, so the frame stays 16 above the window bottom.
        let offline = !matches!(
            self.services.provider.connection(),
            crate::data::ConnectionState::Online
        );
        let hint = if editing {
            None
        } else if offline {
            Some(cc::QUEUED_WHEN_ONLINE.to_string())
        } else if group {
            self.services
                .provider
                .side_chat_default_member()
                .map(|id| crate::views::shell::member_name(&self.services, &id))
                .filter(|n| !n.is_empty())
                .map(|name| {
                    fill(
                        crate::copy::group_page::GROUP_PAGE_COMPOSER_HINT,
                        &[("name", &name)],
                    )
                })
        } else {
            self.working
                .as_ref()
                .map(|name| fill(c::COMPOSER_NEXT_STEP, &[("name", name)]))
        };
        let add = self.native_add(enabled && !editing, window, cx);
        // §2.17: one frame, radius 10, a 1 px border, shadow xs, p 8: the
        // text, then a row with the ghost tools at the left and Send at the
        // right.
        let field = div()
            .id("composer-box")
            .w_full()
            .rounded(ui::native::tokens::radius::CONTAINER)
            .bg(k.content)
            .border_1()
            .border_color(k.separator)
            .shadow(ui::native::tokens::shadow::xs())
            .cursor_text()
            .p(space::S8)
            .flex()
            .flex_col()
            .gap(space::S8)
            .when(!above.is_empty(), |el| {
                el.child(
                    div()
                        .px(space::S4)
                        .flex()
                        .flex_col()
                        .gap(space::S6)
                        .children(above),
                )
            })
            .child(
                div()
                    .w_full()
                    .px(space::S4)
                    .text_color(k.label)
                    .capture_action(cx.listener(Self::on_enter))
                    .capture_action(cx.listener(Self::on_tab))
                    .capture_action(cx.listener(Self::on_up))
                    .capture_action(cx.listener(Self::on_down))
                    .capture_action(cx.listener(Self::on_escape))
                    .capture_action(cx.listener(Self::on_paste))
                    .capture_key_down(cx.listener(Self::on_format_key))
                    .capture_key_down(cx.listener(Self::on_refs_key))
                    .child(editor),
            )
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(space::S4)
                    .child(add)
                    .children(mention)
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .pl(space::S4)
                            .n_footnote()
                            .text_color(k.secondary)
                            .one_line()
                            .children(hint),
                    )
                    .child(send),
            );

        let anchor = self.anchor.clone();
        div()
            .id("composer")
            .flex_none()
            .w_full()
            .on_drop(
                cx.listener(|this, paths: &gpui::ExternalPaths, _, cx| {
                    this.on_drop_paths(paths, cx)
                }),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .child(field)
                    .child(
                        gpui::canvas(move |bounds, _, _| anchor.set(bounds), |_, _, _, _| {})
                            .absolute()
                            .size_full(),
                    )
                    .children(self.render_mentions(window, cx)),
            )
            .into_any_element()
    }
}
