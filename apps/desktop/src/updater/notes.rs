//! Release notes: what each version changed, in the words of
//! `docs/releases/<version>.md` (bundled at build time, see `build.rs`) or of
//! the server's update (its `notes`, the same Markdown).
//!
//! - [`Notes::parse`] keeps the headline and the item lists, and drops the
//!   「怎么更新」 section (it is for people who install by hand).
//! - [`open_notes`]: 「{version} 更新内容」 before an update, with 更新 or 重启安装.
//! - [`open_whats_new`]: 「已更新到 {version}」 once, the first time the new
//!   version opens (see [`take_whats_new`]).
//! - [`open_changelog`]: 更新记录, every bundled version, the current one open.

use std::collections::HashSet;

use gpui::{
    App, AppContext as _, Context, InteractiveElement as _, IntoElement, ParentElement, Render,
    SharedString, StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::{WindowExt as _, dialog::Dialog};

use super::{Phase, version};
use crate::{
    copy::{fill, overlays as o, update as u},
    ui::native::{
        BadgeState, GroupedForm, NativeType as _, PushSize, PushStyle, SheetLayout, SheetWidth,
        StatusBadge, metrics, native_colors, native_sheet, push_button,
        tokens::{icon, space},
    },
};

include!(concat!(env!("OUT_DIR"), "/release_notes.rs"));

/// Sections the app never shows: install steps for people who install by hand.
const SKIPPED_SECTIONS: [&str; 1] = ["怎么更新"];

/// How many items the 已更新到 sheet shows in full; the rest are one line.
const HIGHLIGHTS: usize = 3;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Notes {
    /// The one-line summary under the version.
    pub headline: String,
    pub sections: Vec<Section>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Section {
    /// 改进, 新功能, 修复, …; empty for a list without a heading.
    pub title: String,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    /// The bold lead of a bullet (「自己设快捷键」), if it has one.
    pub title: Option<String>,
    pub body: String,
}

impl Item {
    /// The short name of the item: its title, else its body.
    pub fn name(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.body)
    }
}

impl Notes {
    pub fn parse(version: &str, text: &str) -> Notes {
        let mut notes = Notes::default();
        let mut skipping = false;
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(heading) = line.strip_prefix("## ") {
                let title = clean(heading);
                skipping = SKIPPED_SECTIONS.contains(&title.as_str());
                if !skipping {
                    notes.sections.push(Section {
                        title,
                        items: Vec::new(),
                    });
                }
                continue;
            }
            if line.starts_with('#') || skipping {
                continue;
            }
            let bullet = ["- ", "* ", "· ", "• "]
                .iter()
                .find_map(|b| line.strip_prefix(b));
            let nested = raw.starts_with([' ', '\t']);
            match bullet {
                Some(rest) if !nested => {
                    if notes.sections.is_empty() {
                        notes.sections.push(Section::default());
                    }
                    if let Some(s) = notes.sections.last_mut() {
                        s.items.push(item(rest.trim()));
                    }
                }
                _ if notes.headline.is_empty() && notes.sections.is_empty() => {
                    notes.headline = headline(version, &clean(line));
                }
                // A nested bullet or a wrapped line belongs to the item above.
                _ => {
                    if let Some(it) = notes.sections.last_mut().and_then(|s| s.items.last_mut()) {
                        let more = clean(bullet.unwrap_or(line));
                        if !more.is_empty() {
                            it.body = format!("{} {more}", it.body).trim().to_string();
                        }
                    }
                }
            }
        }
        notes.sections.retain(|s| !s.items.is_empty());
        notes
    }

    pub fn items(&self) -> impl Iterator<Item = &Item> {
        self.sections.iter().flat_map(|s| s.items.iter())
    }

    pub fn count(&self) -> usize {
        self.items().count()
    }

    pub fn is_empty(&self) -> bool {
        self.headline.is_empty() && self.count() == 0
    }

    /// The notes as plain lines (the 重启安装 alert, which has no room for
    /// groups): the headline, then 「· 标题：内容」 per item.
    pub fn plain(&self) -> String {
        let mut lines = Vec::new();
        if !self.headline.is_empty() {
            lines.push(self.headline.clone());
        }
        for it in self.items() {
            lines.push(match &it.title {
                Some(t) => format!("· {t}"),
                None => format!("· {}", it.body),
            });
        }
        lines.join("\n")
    }
}

/// A bullet: 「**标题**：内容」 or plain text.
fn item(text: &str) -> Item {
    if let Some(rest) = text.strip_prefix("**")
        && let Some(end) = rest.find("**")
    {
        let title = clean(&rest[..end]);
        let body = clean(
            rest[end + 2..]
                .trim_start()
                .trim_start_matches(['：', ':'])
                .trim_start(),
        );
        if !title.is_empty() {
            return Item {
                title: Some(title),
                body,
            };
        }
    }
    Item {
        title: None,
        body: clean(text),
    }
}

/// 「TeamChat 1.2.3：可以自己设快捷键…」 → 「可以自己设快捷键…」.
fn headline(version: &str, line: &str) -> String {
    for sep in ["：", ": "] {
        if let Some((lead, rest)) = line.split_once(sep)
            && lead.contains(version)
        {
            return rest.trim().to_string();
        }
    }
    line.to_string()
}

/// Markdown marks out: bold, code, links (their text stays).
fn clean(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let Some(close) = after.find("](") else { break };
        let Some(end) = after[close + 2..].find(')') else {
            break;
        };
        out.push_str(&rest[..open]);
        out.push_str(&after[..close]);
        rest = &after[close + 2 + end + 1..];
    }
    out.push_str(rest);
    out.replace("**", "").replace('`', "").trim().to_string()
}

/// The bundled notes of `version`.
pub fn bundled(version: &str) -> Option<Notes> {
    RELEASES
        .iter()
        .find(|(v, _)| *v == version)
        .map(|(v, text)| Notes::parse(v, text))
        .filter(|n| !n.is_empty())
}

/// Every bundled version that is not newer than the running one, newest first.
pub fn history() -> Vec<(String, Notes)> {
    RELEASES
        .iter()
        .filter(|(v, _)| !version::is_newer(v, version::CURRENT))
        .map(|(v, text)| (v.to_string(), Notes::parse(v, text)))
        .collect()
}

/// Whether the 已更新到 sheet is due: the running version has notes, the
/// person did not turn it off, and this Mac last opened another version
/// (or none, right after an update the app installed itself). Records the
/// running version either way, so it shows once.
pub fn take_whats_new(just_updated: bool) -> Option<Notes> {
    use crate::views::settings::prefs;
    let p = prefs::get();
    let current = version::CURRENT;
    let due = match p.last_seen_version.as_deref() {
        Some(seen) => seen != current,
        None => just_updated,
    };
    if p.last_seen_version.as_deref() != Some(current) {
        prefs::update(|p| p.last_seen_version = Some(current.to_string()));
    }
    if !(due && p.show_whats_new) {
        return None;
    }
    bundled(current)
}

/// 「11 月 7 日」 for a publish time in milliseconds; `None` when unknown.
fn date(published_at: u64) -> Option<String> {
    use chrono::{Datelike as _, Local, TimeZone as _};
    if published_at == 0 {
        return None;
    }
    let t = Local.timestamp_millis_opt(published_at as i64).single()?;
    Some(fill(
        u::DATE,
        &[
            ("month", &t.month().to_string()),
            ("day", &t.day().to_string()),
        ],
    ))
}

// ---------------------------------------------------------------------------
// Drawing

/// Which window a sheet hangs from: they have toolbars of different heights.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Main,
    Settings,
}

fn open_sheet(
    place: Place,
    width: SheetWidth,
    window: &mut Window,
    cx: &mut App,
    build: impl Fn(Dialog, &mut Window, &mut App) -> Dialog + 'static,
) {
    match place {
        Place::Settings => crate::views::settings::widgets::open_sheet(
            window,
            cx,
            width,
            crate::ui::native::tokens::layout::WINDOW_MIN_HEIGHT * 0.66,
            build,
        ),
        Place::Main => window.open_dialog(cx, move |dialog, window, cx| {
            build(native_sheet(dialog, width, cx), window, cx)
        }),
    }
}

/// One item: its title (14 medium) over its text (14 muted), or the text alone.
fn item_row(item: &Item, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .w_full()
        .px(metrics::FORM_ROW_INSET)
        .py(space::S12)
        .flex()
        .flex_col()
        .gap(space::S4)
        .when_some(item.title.clone(), |d, t| {
            d.child(div().n_headline().text_color(k.label).child(t))
        })
        .when(!item.body.is_empty(), |d| {
            d.child(
                div()
                    .n_body()
                    .text_color(if item.title.is_some() {
                        k.secondary
                    } else {
                        k.label
                    })
                    .child(item.body.clone()),
            )
        })
}

/// The sections of a version: a 12 medium label over a group of items.
fn sections(notes: &Notes, cx: &App) -> gpui::Div {
    let k = native_colors(cx);
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(space::S16)
        .children(notes.sections.iter().map(|s| {
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(space::S8)
                .when(!s.title.is_empty(), |d| {
                    d.child(
                        div()
                            .px(space::S4)
                            .n_section()
                            .text_color(k.secondary)
                            .child(s.title.clone()),
                    )
                })
                .child(GroupedForm::new().rows(s.items.iter().map(|it| item_row(it, cx))))
        }))
}

/// A scrolling body: header and buttons stay, the list scrolls (§2.9).
fn scroll(id: &'static str, el: impl IntoElement) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .w_full()
        .max_h(metrics::SHEET_WIDE * 0.7)
        .overflow_y_scroll()
        .child(el)
}

/// 「{version} 更新内容」 of an update that is not installed yet. Its buttons
/// follow the update: 更新到 … while it waits, 重启安装 once downloaded, else 好.
pub fn open_notes(
    info: chat_core::proto::UpdateInfo,
    place: Place,
    window: &mut Window,
    cx: &mut App,
) {
    let notes = Notes::parse(&info.version, &info.notes);
    open_sheet(place, SheetWidth::List, window, cx, move |dialog, _, cx| {
        let k = native_colors(cx);
        let v = info.version.clone();
        let explanation = match date(info.published_at) {
            Some(d) => fill(
                u::NOTES_PUBLISHED,
                &[("date", &d), ("current", version::CURRENT)],
            ),
            None => fill(u::NOTES_CURRENT, &[("current", version::CURRENT)]),
        };
        let updater = super::global(cx);
        let phase = updater.as_ref().map(|up| up.read(cx).phase().clone());
        let later = |cx: &App| {
            push_button(
                "notes-later",
                u::LATER,
                PushStyle::Outline,
                PushSize::Regular,
                cx,
            )
            .on_click(|_, window, cx| window.close_dialog(cx))
        };
        let (status, buttons) = match (phase, updater) {
            (Some(Phase::Available(i) | Phase::Failed { info: Some(i), .. }), Some(up))
                if i.version == v =>
            {
                (
                    true,
                    vec![
                        later(cx),
                        push_button(
                            "notes-update",
                            fill(u::UPDATE_TO, &[("version", &v)]),
                            PushStyle::Default,
                            PushSize::Regular,
                            cx,
                        )
                        .on_click(move |_, window, cx| {
                            up.update(cx, |up, cx| up.start_update(cx));
                            window.close_dialog(cx);
                        }),
                    ],
                )
            }
            (Some(Phase::Ready { info: i, .. }), Some(up)) if i.version == v => (
                true,
                vec![
                    later(cx),
                    push_button(
                        "notes-restart",
                        o::UPDATE_RESTART,
                        PushStyle::Default,
                        PushSize::Regular,
                        cx,
                    )
                    .on_click(move |_, _, cx| up.update(cx, |up, cx| up.install_now(cx))),
                ],
            ),
            _ => (
                false,
                vec![
                    push_button(
                        "notes-ok",
                        u::NOTES_OK,
                        PushStyle::Default,
                        PushSize::Regular,
                        cx,
                    )
                    .on_click(|_, window, cx| window.close_dialog(cx)),
                ],
            ),
        };
        let body = if notes.is_empty() {
            div().n_body().text_color(k.secondary).child(u::NOTES_EMPTY)
        } else {
            div()
                .w_full()
                .flex()
                .flex_col()
                .gap(space::S16)
                .when(!notes.headline.is_empty(), |d| {
                    d.child(
                        div()
                            .n_message()
                            .text_color(k.label)
                            .child(notes.headline.clone()),
                    )
                })
                .child(sections(&notes, cx))
        };
        dialog.on_ok(|_, _, _| true).child(
            SheetLayout::new(fill(u::NOTES_TITLE, &[("version", &v)]))
                .explanation(explanation)
                .body(scroll("update-notes", body))
                .when(status, |l| l.status(u::NOTES_FOOTER))
                .buttons(buttons),
        )
    });
}

/// 「已更新到 {version}」, laid out like the What's New sheet of Apple's own
/// apps: the app icon and title centered, the first items as plain rows (a
/// title over its text, no box), the rest as one line, one wide 好, and
/// 查看全部更新记录 as a link under it.
pub fn open_whats_new(notes: Notes, place: Place, window: &mut Window, cx: &mut App) {
    open_sheet(place, SheetWidth::Form, window, cx, move |dialog, _, cx| {
        let k = native_colors(cx);
        let shown: Vec<&Item> = notes.items().take(HIGHLIGHTS).collect();
        let rest: Vec<&str> = notes.items().skip(HIGHLIGHTS).map(Item::name).collect();
        let list = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(space::S20)
            .children(shown.into_iter().map(|it| {
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(space::S4)
                    .when_some(it.title.clone(), |d, t| {
                        d.child(div().n_headline().text_color(k.label).child(t))
                    })
                    .when(!it.body.is_empty(), |d| {
                        d.child(
                            div()
                                .n_body()
                                .text_color(if it.title.is_some() {
                                    k.secondary
                                } else {
                                    k.label
                                })
                                .child(it.body.clone()),
                        )
                    })
            }))
            .when(!rest.is_empty(), |d| {
                d.child(
                    div()
                        .n_footnote()
                        .text_color(k.tertiary)
                        .child(fill(u::WHATS_NEW_MORE, &[("titles", &rest.join("、"))])),
                )
            });
        dialog.on_ok(|_, _, _| true).child(
            div()
                .w_full()
                .px(space::S32)
                .pt(space::S32)
                .pb(space::S24)
                .flex()
                .flex_col()
                .items_center()
                .child(crate::ui::native::overlays::app_icon(icon::APP))
                .child(
                    div()
                        .pt(space::S12)
                        .n_title2()
                        .text_color(k.label)
                        .child(fill(u::WHATS_NEW_TITLE, &[("version", version::CURRENT)])),
                )
                .when(!notes.headline.is_empty(), |d| {
                    d.child(
                        div()
                            .pt(space::S6)
                            .w_full()
                            .text_center()
                            .n_body()
                            .text_color(k.secondary)
                            .child(notes.headline.clone()),
                    )
                })
                .child(div().pt(space::S24).w_full().child(scroll("whats-new", list)))
                .child(
                    div().pt(space::S32).w_full().child(
                        push_button(
                            "whats-new-ok",
                            u::WHATS_NEW_OK,
                            PushStyle::Default,
                            PushSize::Large,
                            cx,
                        )
                        .w_full()
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
                )
                .child(
                    div().pt(space::S8).child(
                        push_button(
                            "whats-new-all",
                            u::WHATS_NEW_ALL,
                            PushStyle::Link,
                            PushSize::Small,
                            cx,
                        )
                        .text_color(k.secondary)
                        .on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            window.defer(cx, move |window, cx| open_changelog(place, window, cx));
                        }),
                    ),
                ),
        )
    });
}

/// 更新记录: every bundled version up to the running one; a version opens
/// and closes with a click, the running one starts open.
pub struct Changelog {
    versions: Vec<(String, Notes)>,
    open: HashSet<String>,
}

impl Changelog {
    fn new() -> Self {
        Self {
            versions: history(),
            open: HashSet::from([version::CURRENT.to_string()]),
        }
    }

    fn toggle(&mut self, v: &str, cx: &mut Context<Self>) {
        if !self.open.remove(v) {
            self.open.insert(v.to_string());
        }
        cx.notify();
    }
}

impl Render for Changelog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = native_colors(cx);
        let rows = self.versions.iter().enumerate().map(|(ix, (v, notes))| {
            let open = self.open.contains(v);
            let current = v == version::CURRENT;
            let id = SharedString::from(format!("changelog-{v}"));
            let label = fill(
                if open {
                    u::CHANGELOG_COLLAPSE
                } else {
                    u::CHANGELOG_EXPAND
                },
                &[("version", v)],
            );
            let key = v.clone();
            // A row that grows with its headline (a button keeps one height).
            let hover = k.row_hover;
            let header = div()
                .id(id)
                .w_full()
                .px(space::S8)
                .py(space::S8)
                .rounded(crate::ui::native::tokens::radius::CONTROL)
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(label.clone()).build(window, cx)
                })
                .on_click(cx.listener(move |this, _, _, cx| this.toggle(&key, cx)))
                .child(
                    div()
                        .w_full()
                        .flex()
                        .items_start()
                        .gap(space::S12)
                        .child(
                            div()
                                .flex_none()
                                .w(space::S48 * 2.)
                                .flex()
                                .flex_col()
                                .items_start()
                                .gap(space::S8)
                                .child(
                                    div()
                                        .n_headline()
                                        .n_figures()
                                        .text_color(k.label)
                                        .child(v.clone()),
                                )
                                .when(current, |d| {
                                    d.child(StatusBadge::new(
                                        BadgeState::Done,
                                        u::CHANGELOG_CURRENT,
                                    ))
                                }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(gpui::Pixels::ZERO)
                                .n_body()
                                .text_color(if open { k.label } else { k.secondary })
                                .whitespace_normal()
                                .child(notes.headline.clone()),
                        )
                        .child(crate::ui::sf_symbols::symbol(
                            if open {
                                crate::ui::sf_symbols::Sf::ChevronDown
                            } else {
                                crate::ui::sf_symbols::Sf::ChevronRight
                            },
                            icon::MD,
                            crate::ui::sf_symbols::Weight::Regular,
                            k.secondary,
                            window,
                        )),
                );
            div()
                .w_full()
                .flex()
                .flex_col()
                .when(ix > 0, |d| {
                    d.pt(space::S8).border_t_1().border_color(k.separator)
                })
                .pb(space::S8)
                .child(header)
                .when(open && notes.count() > 0, |d| {
                    d.child(
                        div()
                            .pl(space::S8 + space::S48 * 2. + space::S12)
                            .pr(space::S8)
                            .pt(space::S8)
                            .child(sections(notes, cx)),
                    )
                })
        });
        div().w_full().flex().flex_col().children(rows)
    }
}

pub fn open_changelog(place: Place, window: &mut Window, cx: &mut App) {
    let list = cx.new(|_| Changelog::new());
    open_sheet(place, SheetWidth::List, window, cx, move |dialog, _, cx| {
        let n = list.read(cx).versions.len();
        dialog.on_ok(|_, _, _| true).child(
            SheetLayout::new(u::CHANGELOG)
                .explanation(format!(
                    "{} · {}",
                    fill(u::CHANGELOG_COUNT, &[("n", &n.to_string())]),
                    u::CHANGELOG_DESCRIPTION
                ))
                .body(scroll("changelog", list.clone()))
                .buttons([push_button(
                    "changelog-ok",
                    u::NOTES_OK,
                    PushStyle::Default,
                    PushSize::Regular,
                    cx,
                )
                .on_click(|_, window, cx| window.close_dialog(cx))]),
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "{product} 1.2.3：可以自己设快捷键，引用消息会自动 @ 对方。

## 怎么更新

- 已经装了 1.2.2：App 里会提示有新版本。

## 改进

- **自己设快捷键**：「设置 → 快捷键」里点某个快捷键，直接按下新的组合键。
- **引用时 @ 对方**：引用一条消息回复时，会自动 @ 发送者。
- 修好了 `App` 启动时一连串加载失败的问题，见 [说明](https://example.com)。
";

    #[test]
    fn parses_headline_and_items() {
        let text = SAMPLE.replace("{product}", env!("PRODUCT_NAME"));
        let n = Notes::parse("1.2.3", &text);
        assert_eq!(n.headline, "可以自己设快捷键，引用消息会自动 @ 对方。");
        assert_eq!(n.sections.len(), 1, "怎么更新 is dropped");
        assert_eq!(n.sections[0].title, "改进");
        assert_eq!(n.count(), 3);
        let first = &n.sections[0].items[0];
        assert_eq!(first.title.as_deref(), Some("自己设快捷键"));
        assert!(first.body.starts_with("「设置"));
        let last = &n.sections[0].items[2];
        assert_eq!(last.title, None);
        assert_eq!(
            last.body,
            "修好了 App 启动时一连串加载失败的问题，见 说明。"
        );
    }

    #[test]
    fn plain_bullets_without_headings() {
        let n = Notes::parse("9.9.9", "· 消息列表滚动更顺\n· 产出页可以按话题筛选");
        assert_eq!(n.headline, "");
        assert_eq!(n.count(), 2);
        assert_eq!(n.plain(), "· 消息列表滚动更顺\n· 产出页可以按话题筛选");
    }

    #[test]
    fn every_bundled_version_parses() {
        assert!(!RELEASES.is_empty());
        for (v, text) in RELEASES {
            let n = Notes::parse(v, text);
            assert!(!n.headline.is_empty(), "{v} has a headline");
            assert!(n.count() > 0, "{v} has items");
            assert!(
                !n.headline.contains(env!("PRODUCT_NAME")),
                "{v}: {}",
                n.headline
            );
        }
        let versions: Vec<_> = RELEASES
            .iter()
            .map(|(v, _)| chat_core::proto::parse_release_version(v).unwrap())
            .collect();
        assert!(versions.windows(2).all(|w| w[0] > w[1]), "newest first");
    }
}
