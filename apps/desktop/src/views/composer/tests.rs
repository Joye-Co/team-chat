//! Composer tests on GPUI's test platform with the real gpui-component input.
//!
//! The IME tests replay what the macOS input method does through the
//! platform input handler (`replace_and_mark_text_in_range`,
//! `replace_text_in_range`, `unmark_text`) and press keys in between. The test
//! platform does not give keys to an IME first (macOS does), so Enter during
//! composition reaches the composer: it must confirm the candidate and never
//! send.

use std::sync::Arc;

use gpui::{EntityInputHandler, TestAppContext, VisualTestContext};

use super::*;
use crate::{
    app::Services,
    data::ChatProvider,
    fake::{FakeProvider, ME},
};

const GROUP: &str = "g-dev";
const TOPIC: &str = "t-api";

struct Harness {
    provider: Arc<FakeProvider>,
    view: Entity<ComposerView>,
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    topic: Option<&str>,
) -> (Harness, &'a mut VisualTestContext) {
    cx.update(gpui_component::init);
    let provider = FakeProvider::new();
    let dyn_provider: Arc<dyn ChatProvider> = provider.clone();
    let topic = topic.map(str::to_string);
    let (view, vcx) = cx.add_window_view(move |window, cx| {
        let services = Services::new(dyn_provider, cx);
        services.nav.update(cx, |nav, cx| match topic {
            Some(t) => nav.open_topic(GROUP.into(), t, cx),
            None => nav.open_group(GROUP.into(), cx),
        });
        ComposerView::new(services, window, cx)
    });
    view.update_in(vcx, |this, window, cx| {
        let input = this.input.clone();
        input.update(cx, |state, cx| state.focus(window, cx));
    });
    vcx.run_until_parked();
    (Harness { provider, view }, vcx)
}

impl Harness {
    fn count(&self, topic: &str) -> usize {
        self.provider
            .messages(GROUP, topic, usize::MAX / 2)
            .messages
            .len()
    }

    fn last(&self, topic: &str) -> crate::data::Message {
        self.provider
            .messages(GROUP, topic, 1)
            .messages
            .pop()
            .expect("a message")
    }

    fn text(&self, cx: &mut VisualTestContext) -> String {
        self.view.update(cx, |this, cx| this.text(cx))
    }

    fn marked(&self, cx: &mut VisualTestContext) -> Option<std::ops::Range<usize>> {
        self.view.update_in(cx, |this, window, cx| {
            this.input
                .update(cx, |s, cx| s.marked_text_range(window, cx))
        })
    }

    /// The IME sets or replaces the marked (composing) text.
    fn mark(&self, cx: &mut VisualTestContext, text: &str) {
        self.view.update_in(cx, |this, window, cx| {
            this.input.update(cx, |s, cx| {
                s.replace_and_mark_text_in_range(None, text, None, window, cx)
            })
        });
        cx.run_until_parked();
    }

    /// The IME commits a candidate in place of the marked text.
    fn commit(&self, cx: &mut VisualTestContext, text: &str) {
        self.view.update_in(cx, |this, window, cx| {
            this.input.update(cx, |s, cx| {
                s.replace_text_in_range(None, text, window, cx);
                s.unmark_text(window, cx);
            })
        });
        cx.run_until_parked();
    }
}

#[gpui::test]
fn ime_enter_confirms_the_candidate_and_does_not_send(cx: &mut TestAppContext) {
    let (h, cx) = open(cx, Some(TOPIC));
    let before = h.count(TOPIC);

    // Pinyin while composing: n, ni, nih, nihao.
    for step in ["n", "ni", "nih", "nihao"] {
        h.mark(cx, step);
        assert_eq!(h.count(TOPIC), before, "sent while composing {step}");
    }
    assert_eq!(h.text(cx), "nihao");
    assert_eq!(h.marked(cx), Some(0..5));

    // Enter during composition: confirms the marked text, sends nothing.
    cx.simulate_keystrokes("enter");
    assert_eq!(h.count(TOPIC), before, "Enter during composition sent");
    assert_eq!(h.marked(cx), None, "Enter did not confirm the candidate");
    assert_eq!(h.text(cx), "nihao");

    // Clear, then compose and pick candidates: 你, then 好.
    h.view
        .update_in(cx, |this, window, cx| this.set_text("", window, cx));
    h.mark(cx, "ni");
    h.commit(cx, "你");
    h.mark(cx, "hao");
    assert_eq!(h.text(cx), "你hao");
    assert_eq!(h.count(TOPIC), before);
    h.commit(cx, "好");
    assert_eq!(h.text(cx), "你好");
    assert_eq!(h.marked(cx), None);
    assert_eq!(h.count(TOPIC), before, "sent before Enter");

    // Now Enter sends the committed text.
    cx.simulate_keystrokes("enter");
    assert_eq!(h.count(TOPIC), before + 1);
    assert_eq!(h.last(TOPIC).body, "你好");
    assert_eq!(h.text(cx), "");
}

#[gpui::test]
fn shift_enter_adds_a_line_and_cmd_enter_sends(cx: &mut TestAppContext) {
    let (h, cx) = open(cx, Some(TOPIC));
    let before = h.count(TOPIC);
    cx.simulate_input("ab");
    cx.simulate_keystrokes("shift-enter");
    cx.simulate_input("cd");
    assert_eq!(h.text(cx), "ab\ncd");
    assert_eq!(h.count(TOPIC), before);
    cx.simulate_keystrokes("cmd-enter");
    assert_eq!(h.count(TOPIC), before + 1);
    assert_eq!(h.last(TOPIC).body, "ab\ncd");
    // Enter on an empty composer sends nothing.
    cx.simulate_keystrokes("enter");
    assert_eq!(h.count(TOPIC), before + 1);
}

#[gpui::test]
fn mention_list_picks_with_keys_and_sends_ids(cx: &mut TestAppContext) {
    let (h, cx) = open(cx, Some(TOPIC));
    cx.simulate_input("请@F");
    let names = |h: &Harness, cx: &mut VisualTestContext| -> Vec<String> {
        h.view.update(cx, |this, _| {
            this.mention
                .as_ref()
                .map(|p| p.results.iter().map(|m| m.name.clone()).collect())
                .unwrap_or_default()
        })
    };
    assert_eq!(names(&h, cx), vec!["Forge"]);
    cx.simulate_keystrokes("enter");
    assert_eq!(h.text(cx), "请@Forge ");
    assert!(names(&h, cx).is_empty(), "list closes after a pick");
    cx.simulate_input("看下");
    // Esc closes an open list without touching the text.
    cx.simulate_input(" @Alex");
    assert_eq!(names(&h, cx), vec!["Alex Chen"]);
    cx.simulate_keystrokes("escape");
    assert!(names(&h, cx).is_empty());
    cx.simulate_keystrokes("enter");
    let sent = h.last(TOPIC);
    assert_eq!(sent.body, "请@Forge 看下 @Alex");
    assert_eq!(sent.mentions, vec!["m-coder".to_string()]);
}

#[gpui::test]
fn up_edits_my_last_message_and_esc_cancels(cx: &mut TestAppContext) {
    let (h, cx) = open(cx, Some(TOPIC));
    cx.simulate_input("第一版");
    cx.simulate_keystrokes("enter");
    let mine = h.last(TOPIC);
    assert_eq!(mine.author_id, ME);

    cx.simulate_keystrokes("up");
    assert_eq!(h.text(cx), "第一版");
    assert!(h.view.update(cx, |this, _| this.editing.is_some()));
    cx.simulate_keystrokes("escape");
    assert_eq!(h.text(cx), "");
    assert!(h.view.update(cx, |this, _| this.editing.is_none()));

    let before = h.count(TOPIC);
    cx.simulate_keystrokes("up");
    cx.simulate_input("，第二版");
    cx.simulate_keystrokes("enter");
    assert_eq!(h.count(TOPIC), before, "an edit is not a new message");
    let edited = h.last(TOPIC);
    assert_eq!(edited.id, mine.id);
    assert_eq!(edited.body, "第一版，第二版");
    assert!(edited.edited_at.is_some());
    assert_eq!(h.text(cx), "");

    // Up in a composer with text moves the cursor; it does not edit.
    cx.simulate_input("x");
    cx.simulate_keystrokes("up");
    assert!(h.view.update(cx, |this, _| this.editing.is_none()));
}

#[gpui::test]
fn quote_chip_is_sent_and_esc_removes_it(cx: &mut TestAppContext) {
    let (h, cx) = open(cx, Some(TOPIC));
    let quoted = h.last(TOPIC);
    let key = DraftKey::topic(GROUP, TOPIC);
    // The timeline sets the quote through the provider.
    h.provider.set_quote(&key, Some(quoted.id.clone()));
    cx.run_until_parked();
    assert!(h.view.update(cx, |this, _| this.quote.is_some()));
    cx.simulate_keystrokes("escape");
    assert!(h.view.update(cx, |this, _| this.quote.is_none()));
    assert_eq!(h.provider.draft(&key).quote_of, None);

    h.provider.set_quote(&key, Some(quoted.id.clone()));
    cx.run_until_parked();
    cx.simulate_input("同意");
    cx.simulate_keystrokes("enter");
    let sent = h.last(TOPIC);
    assert_eq!(sent.body, "同意");
    assert_eq!(sent.quote_of.as_deref(), Some(quoted.id.as_str()));
    assert_eq!(h.provider.draft(&key), crate::data::Draft::default());
    assert!(h.view.update(cx, |this, _| this.quote.is_none()));
}

#[gpui::test]
fn drafts_are_kept_per_topic(cx: &mut TestAppContext) {
    let (h, cx) = open(cx, Some(TOPIC));
    cx.simulate_input("还没写完");
    let other = h
        .provider
        .topics(GROUP)
        .into_iter()
        .find(|t| t.id != TOPIC)
        .unwrap()
        .id;
    let nav = h.view.update(cx, |this, _| this.services.nav.clone());
    nav.update(cx, |nav, cx| {
        nav.open_topic(GROUP.into(), other.clone(), cx)
    });
    cx.run_until_parked();
    assert_eq!(h.text(cx), "");
    cx.simulate_input("另一个");
    nav.update(cx, |nav, cx| nav.open_topic(GROUP.into(), TOPIC.into(), cx));
    cx.run_until_parked();
    assert_eq!(h.text(cx), "还没写完");
    assert_eq!(
        h.provider.draft(&DraftKey::topic(GROUP, other)).body,
        "另一个"
    );
}

#[gpui::test]
fn new_topic_mode_uses_the_first_line_as_title(cx: &mut TestAppContext) {
    let (h, cx) = open(cx, None);
    let topics_before = h.provider.topics(GROUP).len();
    cx.simulate_input("接口改版");
    cx.simulate_keystrokes("shift-enter");
    cx.simulate_input("细节在这里");
    assert_eq!(
        h.view
            .update(cx, |this, _| model::topic_title(&this.last_text)),
        Some("接口改版".to_string())
    );
    cx.simulate_keystrokes("enter");
    let topics = h.provider.topics(GROUP);
    assert_eq!(topics.len(), topics_before + 1);
    assert_eq!(topics[0].title, "接口改版");
    // The composer now writes into the new topic.
    let key = h.view.update(cx, |this, _| this.key.clone()).unwrap();
    assert_eq!(key.topic_id.as_deref(), Some(topics[0].id.as_str()));
    assert_eq!(h.text(cx), "");
    assert_eq!(h.provider.draft(&DraftKey::new_topic(GROUP)).body, "");
}

#[gpui::test]
fn pending_files_become_chips_and_go_with_the_message(cx: &mut TestAppContext) {
    let (h, cx) = open(cx, Some(TOPIC));
    let key = DraftKey::topic(GROUP, TOPIC);
    let paths = ExternalPaths(
        vec![
            std::path::PathBuf::from("/tmp/报告.pdf"),
            std::path::PathBuf::from("/tmp/图.png"),
        ]
        .into(),
    );
    h.view.update(cx, |this, cx| this.on_drop_paths(&paths, cx));
    cx.run_until_parked();
    let names: Vec<String> = h.view.update(cx, |this, _| {
        this.files.iter().map(|f| f.name.clone()).collect()
    });
    assert_eq!(names, vec!["报告.pdf", "图.png"]);

    // Paste of an image from the clipboard.
    let image = gpui::Image::from_bytes(gpui::ImageFormat::Png, vec![0x89, b'P', b'N', b'G']);
    cx.write_to_clipboard(gpui::ClipboardItem::new_image(&image));
    cx.simulate_keystrokes("cmd-v");
    assert_eq!(h.view.update(cx, |this, _| this.files.len()), 3);
    assert_eq!(h.text(cx), "", "an image paste adds no text");

    let first = h.view.update(cx, |this, _| this.files[0].id.clone());
    h.view.update(cx, |this, cx| this.remove_file(&first, cx));
    cx.run_until_parked();
    assert_eq!(h.view.update(cx, |this, _| this.files.len()), 2);

    // Files alone can be sent; the draft is cleared afterwards.
    let before = h.count(TOPIC);
    cx.simulate_keystrokes("enter");
    assert_eq!(h.count(TOPIC), before + 1);
    assert!(h.provider.draft(&key).files.is_empty());
    assert!(h.view.update(cx, |this, _| this.files.is_empty()));
}

#[gpui::test]
fn emoji_tool_toggles_the_picker_and_esc_closes_it(cx: &mut TestAppContext) {
    let (h, cx) = open(cx, Some(TOPIC));
    let click = |x: f32| {
        let at = gpui::point(gpui::px(x), gpui::px(10.));
        gpui::ClickEvent::Mouse(gpui::MouseClickEvent {
            down: gpui::MouseDownEvent {
                position: at,
                ..Default::default()
            },
            up: gpui::MouseUpEvent {
                position: at,
                ..Default::default()
            },
        })
    };
    let open_now = |cx: &mut VisualTestContext| h.view.update(cx, |this, _| this.emoji_open);
    h.view
        .update(cx, |this, cx| this.toggle_emoji(&click(5.), cx));
    assert!(open_now(cx));
    // A second press on the tool: the picker's mouse-down-out closes it
    // first, and the click on mouse up must not open it again.
    h.view.update(cx, |this, cx| {
        this.emoji_open = false;
        this.emoji_closed_at = Some(gpui::point(gpui::px(5.), gpui::px(10.)));
        this.toggle_emoji(&click(5.), cx);
    });
    assert!(!open_now(cx));
    // A press elsewhere closed it before; the tool opens it again.
    h.view.update(cx, |this, cx| {
        this.emoji_closed_at = Some(gpui::point(gpui::px(300.), gpui::px(10.)));
        this.toggle_emoji(&click(5.), cx);
    });
    assert!(open_now(cx));
    cx.simulate_keystrokes("escape");
    assert!(!open_now(cx));
}

#[test]
fn placeholders_name_untitled_topics_and_keep_their_fixed_text() {
    use super::{new_topic_placeholder, reply_placeholder};
    assert_eq!(reply_placeholder(""), "回复「（无标题）」");
    assert_eq!(reply_placeholder("  \n "), "回复「（无标题）」");
    assert_eq!(reply_placeholder("周五 demo"), "回复「周五 demo」");
    let long = reply_placeholder(&"很长的话题标题".repeat(10));
    assert!(
        long.starts_with("回复「") && long.ends_with("…」"),
        "{long}"
    );
    let new = new_topic_placeholder(&"二零二六年第四季度跨部门产品规划讨论群".repeat(2));
    assert!(new.ends_with("…」发起新话题，第一行是标题"), "{new}");
    assert_eq!(
        new_topic_placeholder("产品讨论"),
        "在「产品讨论」发起新话题，第一行是标题"
    );
}

#[test]
fn the_group_page_placeholder_names_the_group_and_keeps_its_fixed_text() {
    use super::group_page_placeholder;
    assert_eq!(
        group_page_placeholder("开发"),
        "在开发里开个话题，或 @ 成员交代…"
    );
    let long = group_page_placeholder(&"二零二六年第四季度跨部门产品规划讨论群".repeat(2));
    assert!(long.ends_with("…里开个话题，或 @ 成员交代…"), "{long}");
}

// --- inc-refs: reference chips (REFS §A4, §A5) ---

fn chip(n: usize) -> crate::data::DraftRef {
    crate::data::DraftRef {
        input: chat_core::proto::BlockRefInput {
            message_id: Some("m-src".into()),
            block_id: format!("v1:{n}"),
            label: format!("新增群从哪来 · 第 {n} 处"),
            ..Default::default()
        },
        group_id: GROUP.into(),
        topic_id: TOPIC.into(),
        lead_chars: 7,
    }
}

#[gpui::test]
fn chips_cap_at_five_backspace_and_esc_remove_and_send_carries_them(cx: &mut TestAppContext) {
    use crate::data::AddRef;
    let (h, cx) = open(cx, Some(TOPIC));
    let key = DraftKey::topic(GROUP, TOPIC);
    for n in 0..5 {
        assert_eq!(h.provider.add_draft_ref(&key, chip(n)), AddRef::Added);
    }
    assert_eq!(h.provider.add_draft_ref(&key, chip(5)), AddRef::Full);
    assert_eq!(
        h.provider.add_draft_ref(&key, chip(2)),
        AddRef::Duplicate(2)
    );
    cx.run_until_parked();
    assert_eq!(h.view.update(cx, |this, _| this.refs.len()), 5);

    // Backspace at the start of an empty field removes the last chip.
    cx.simulate_keystrokes("backspace");
    assert_eq!(h.provider.draft(&key).refs.len(), 4);
    // With text, Backspace edits the text.
    cx.simulate_input("ab");
    cx.simulate_keystrokes("backspace");
    assert_eq!(h.text(cx), "a");
    assert_eq!(h.provider.draft(&key).refs.len(), 4);
    cx.simulate_keystrokes("backspace");
    // Esc: the quote first, then the newest chip.
    let quoted = h.last(TOPIC);
    h.provider.set_quote(&key, Some(quoted.id.clone()));
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    assert_eq!(h.provider.draft(&key).quote_of, None);
    assert_eq!(h.provider.draft(&key).refs.len(), 4);
    cx.simulate_keystrokes("escape");
    let left: Vec<String> = h
        .provider
        .draft(&key)
        .refs
        .iter()
        .map(|r| r.input.block_id.clone())
        .collect();
    assert_eq!(left, ["v1:0", "v1:1", "v1:2"]);

    // Chips with an empty text send: the body is the quote lines only.
    let before = h.count(TOPIC);
    cx.simulate_keystrokes("enter");
    assert_eq!(h.count(TOPIC), before + 1);
    let sent = h.last(TOPIC);
    let refs = h.provider.message_block_refs(&sent);
    assert_eq!(refs.len(), 3);
    assert_eq!(
        sent.body,
        "> ↳ 新增群从哪来 · 第 0 处\n> ↳ 新增群从哪来 · 第 1 处\n> ↳ 新增群从哪来 · 第 2 处"
    );
    assert_eq!(
        crate::views::refs::strip_prefix(&sent.body, &refs),
        Some("")
    );
    assert!(h.provider.draft(&key).refs.is_empty());
    assert!(h.view.update(cx, |this, _| this.refs.is_empty()));
}

#[gpui::test]
fn asking_about_a_part_adds_the_chip_and_mentions_the_author_bot(cx: &mut TestAppContext) {
    use crate::fake::v2::{self as d, V2Samples, refs_samples};
    cx.update(gpui_component::init);
    let provider = V2Samples::with_refs();
    let dyn_provider: Arc<dyn ChatProvider> = provider.clone();
    let (view, cx) = cx.add_window_view(move |window, cx| {
        let services = Services::new(dyn_provider, cx);
        services.nav.update(cx, |nav, cx| {
            nav.open_topic(d::GROWTH.into(), d::T_REPORT.into(), cx)
        });
        ComposerView::new(services, window, cx)
    });
    cx.run_until_parked();
    let key = DraftKey::topic(d::GROWTH, d::T_REPORT);
    let ask = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            crate::views::refs::ask_about(
                refs_samples::bili_cell(),
                crate::views::refs::RefIntent::Topic,
                window,
                cx,
            )
        });
        cx.run_until_parked();
    };
    ask(cx);
    let draft = provider.draft(&key);
    assert_eq!(draft.refs.len(), 1);
    assert_eq!(draft.refs[0].input.label, "新增群从哪来 · B 站 · 新增群 9");
    assert_eq!(draft.body, "@Ledger ");
    assert_eq!(draft.mentions, vec![d::LEDGER.to_string()]);
    assert_eq!(view.update(cx, |this, cx| this.text(cx)), "@Ledger ");
    assert_eq!(view.update(cx, |this, _| this.refs.len()), 1);
    // The same part again flashes its chip; no second chip, no second mention.
    ask(cx);
    assert_eq!(provider.draft(&key).refs.len(), 1);
    assert_eq!(provider.draft(&key).body, "@Ledger ");
    // Send: Ledger is mentioned and the reference goes with the message.
    cx.simulate_input("为什么比 9 月少了？");
    cx.simulate_keystrokes("enter");
    let window = provider.messages(d::GROWTH, d::T_REPORT, 50);
    let sent = window.messages.last().unwrap();
    let refs = provider.message_block_refs(sent);
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].source_author_id.as_deref(), Some(d::LEDGER));
    assert_eq!(
        crate::views::refs::strip_prefix(&sent.body, &refs),
        Some("@Ledger 为什么比 9 月少了？")
    );
}

#[gpui::test]
fn cmd_semicolon_takes_the_chips_into_the_side_chat(cx: &mut TestAppContext) {
    use crate::fake::v2::{self as d, V2Samples, refs_samples};
    cx.update(gpui_component::init);
    let provider = V2Samples::with_refs();
    let dyn_provider: Arc<dyn ChatProvider> = provider.clone();
    let services = std::rc::Rc::new(std::cell::RefCell::new(None));
    let keep = services.clone();
    let (_view, cx) = cx.add_window_view(move |window, cx| {
        let s = Services::new(dyn_provider, cx);
        s.nav.update(cx, |nav, cx| {
            nav.open_topic(d::GROWTH.into(), d::T_REPORT.into(), cx)
        });
        *keep.borrow_mut() = Some(s.clone());
        ComposerView::new(s, window, cx)
    });
    let services = services.borrow().clone().unwrap();
    let key = DraftKey::topic(d::GROWTH, d::T_REPORT);
    let chip = crate::data::DraftRef {
        input: refs_samples::bili_cell().input(),
        group_id: d::GROWTH.into(),
        topic_id: d::T_REPORT.into(),
        lead_chars: 0,
    };
    provider.add_draft_ref(&key, chip);
    cx.update(|_, cx| crate::views::refs::carry_to_side_chat(&services, Some(d::LEDGER), cx));
    cx.run_until_parked();
    assert!(provider.draft(&key).refs.is_empty());
    let side = (
        d::GROWTH.to_string(),
        d::T_REPORT.to_string(),
        d::LEDGER.to_string(),
    );
    let moved = cx.update(|_, cx| {
        crate::views::refs::hub(cx)
            .read(cx)
            .side_refs
            .get(&side)
            .map_or(0, Vec::len)
    });
    assert_eq!(moved, 1);
}

/// R1-15, T-04: a composer on screen lifts pushed toasts 16 above its top
/// (the window bottom minus its top), with or without the focus; a frame
/// with no composer puts them back to 16.
#[gpui::test]
fn a_composer_on_screen_lifts_the_toasts(cx: &mut TestAppContext) {
    let _lock = crate::ui::overlay::TOAST_FLOOR_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (h, cx) = open(cx, Some(TOPIC));
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    let bottom = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            gpui_component::ActiveTheme::theme(cx)
                .notification
                .margins
                .bottom
        })
    };
    h.view.update_in(cx, |_, window, cx| window.blur(cx));
    cx.run_until_parked();
    let top = h.view.update(cx, |this, _| this.anchor.get().origin.y);
    let height = cx.update(|window, _| window.viewport_size().height);
    assert!(top > px(0.), "the composer was drawn");
    assert!(
        height - top > px(48.),
        "the floor is the box's top, not its bottom"
    );
    assert_eq!(bottom(cx), height - top + px(16.));
    // Two frames with no composer painted: the toasts go back down (read
    // before the window draws the composer again).
    let after = cx.update(|_, cx| {
        crate::views::composer::floor_frame(cx);
        crate::views::composer::floor_frame(cx);
        gpui_component::ActiveTheme::theme(cx)
            .notification
            .margins
            .bottom
    });
    assert_eq!(after, px(16.));
}

#[gpui::test]
fn avatar_click_mentions_after_a_space_and_sends_the_id(cx: &mut TestAppContext) {
    let (h, cx) = open(cx, Some(TOPIC));
    cx.simulate_input("看下");
    h.view.update_in(cx, |this, window, cx| {
        this.mention_member("m-coder", window, cx)
    });
    assert_eq!(h.text(cx), "看下 @Forge ");
    // Twice is two mentions in the text but one id.
    h.view.update_in(cx, |this, window, cx| {
        this.mention_member("m-coder", window, cx)
    });
    cx.simulate_input("好");
    cx.simulate_keystrokes("enter");
    let sent = h.last(TOPIC);
    assert_eq!(sent.body, "看下 @Forge @Forge 好");
    assert_eq!(sent.mentions, vec!["m-coder".to_string()]);
}

#[gpui::test]
fn reply_quotes_a_message_of_the_open_topic_only(cx: &mut TestAppContext) {
    let (h, cx) = open(cx, Some(TOPIC));
    let target = h
        .provider
        .messages(GROUP, TOPIC, usize::MAX / 2)
        .messages
        .last()
        .cloned()
        .expect("the fake topic has messages");
    let author = h
        .provider
        .members()
        .into_iter()
        .find(|m| m.id == target.author_id)
        .expect("the sender is a member");
    let quote = |cx: &mut VisualTestContext, topic: &str| {
        let (id, author_id) = (target.id.clone(), target.author_id.clone());
        h.view.update_in(cx, |this, window, cx| {
            this.quote_message(GROUP, topic, id, Some(&author_id), window, cx)
        });
    };
    quote(cx, "t-other");
    assert!(h.view.update(cx, |this, _| this.quote.is_none()));
    assert_eq!(h.text(cx), "");
    quote(cx, TOPIC);
    assert!(h.view.update(cx, |this, _| this.quote.is_some()));
    // The quote @s its sender, once however often it is quoted.
    let at = format!("@{} ", author.name);
    assert_eq!(h.text(cx), at);
    quote(cx, TOPIC);
    assert_eq!(h.text(cx), at);
    cx.simulate_input("同意");
    cx.simulate_keystrokes("enter");
    let sent = h.last(TOPIC);
    assert_eq!(sent.quote_of.as_deref(), Some(target.id.as_str()));
    assert_eq!(sent.body, format!("{at}同意"));
    assert_eq!(sent.mentions, vec![author.id.clone()]);

    // My own message: the quote alone.
    h.view.update_in(cx, |this, window, cx| {
        this.quote_message(GROUP, TOPIC, sent.id.clone(), None, window, cx)
    });
    assert!(h.view.update(cx, |this, _| this.quote.is_some()));
    assert_eq!(h.text(cx), "");
}

#[gpui::test]
fn backspace_and_cmd_a_take_the_mention_and_the_quote_without_the_mouse(cx: &mut TestAppContext) {
    let (h, cx) = open(cx, Some(TOPIC));
    let key = DraftKey::topic(GROUP, TOPIC);
    let target = h.last(TOPIC);
    let author = h
        .provider
        .members()
        .into_iter()
        .find(|m| m.id == target.author_id)
        .expect("the sender is a member");
    let quote = |cx: &mut VisualTestContext| {
        let (id, author_id) = (target.id.clone(), target.author_id.clone());
        h.view.update_in(cx, |this, window, cx| {
            this.quote_message(GROUP, TOPIC, id, Some(&author_id), window, cx)
        });
        cx.run_until_parked();
    };
    let quoted = |cx: &mut VisualTestContext| h.view.update(cx, |this, _| this.quote.is_some());
    let at = format!("@{} ", author.name);

    // One Backspace takes the whole @, the next one the quote.
    quote(cx);
    assert_eq!(h.text(cx), at);
    cx.simulate_keystrokes("backspace");
    assert_eq!(h.text(cx), "");
    assert_eq!(h.provider.draft(&key).body, "");
    assert!(quoted(cx));
    // A held key (clearing the text) stops at the empty field.
    cx.simulate_event(gpui::KeyDownEvent {
        keystroke: gpui::Keystroke::parse("backspace").unwrap(),
        is_held: true,
        prefer_character_input: false,
    });
    assert!(quoted(cx));
    cx.simulate_keystrokes("backspace");
    assert!(!quoted(cx));
    assert_eq!(h.provider.draft(&key).quote_of, None);

    // The first ⌘A selects the text, the second the quote too; Backspace
    // then removes both.
    quote(cx);
    cx.simulate_input("同意");
    cx.simulate_keystrokes("cmd-a");
    assert!(!h.view.update(cx, |this, _| this.chips_selected));
    cx.simulate_keystrokes("cmd-a");
    assert!(h.view.update(cx, |this, _| this.chips_selected));
    cx.simulate_keystrokes("backspace");
    assert_eq!(h.text(cx), "");
    assert!(!quoted(cx));
    assert_eq!(h.provider.draft(&key).quote_of, None);

    // A single ⌘A selects the text only: Backspace keeps the quote.
    quote(cx);
    cx.simulate_input("嗯");
    cx.simulate_keystrokes("cmd-a backspace");
    assert_eq!(h.text(cx), "");
    assert!(quoted(cx));
    h.view.update(cx, |this, cx| this.remove_quote(cx));

    // ⌘A on an empty field: nothing more to select, the quote at once.
    h.provider.set_quote(&key, Some(target.id.clone()));
    cx.run_until_parked();
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_keystrokes("delete");
    assert!(!quoted(cx));

    // Moving the caret drops the chip selection: Backspace edits the text.
    quote(cx);
    cx.simulate_input("好");
    cx.simulate_keystrokes("cmd-a cmd-a");
    cx.simulate_keystrokes("right");
    assert!(!h.view.update(cx, |this, _| this.chips_selected));
    cx.simulate_keystrokes("backspace");
    assert_eq!(h.text(cx), at);
    assert!(quoted(cx));
}

#[gpui::test]
fn reedit_puts_the_recalled_text_back_after_the_draft(cx: &mut TestAppContext) {
    let (h, cx) = open(cx, Some(TOPIC));
    h.view.update_in(cx, |this, window, cx| {
        this.restore_recalled(GROUP, "t-other", "别的话题", &[], window, cx)
    });
    assert_eq!(h.text(cx), "", "another 话题's text stays out");
    h.view.update_in(cx, |this, window, cx| {
        this.restore_recalled(
            GROUP,
            TOPIC,
            "@Forge 原来的话",
            &["m-coder".into()],
            window,
            cx,
        )
    });
    assert_eq!(h.text(cx), "@Forge 原来的话");
    cx.simulate_input("，改一下");
    cx.simulate_keystrokes("enter");
    let sent = h.last(TOPIC);
    assert_eq!(sent.body, "@Forge 原来的话，改一下");
    assert_eq!(sent.mentions, vec!["m-coder".to_string()]);

    cx.simulate_input("先写的");
    h.view.update_in(cx, |this, window, cx| {
        this.restore_recalled(GROUP, TOPIC, "撤回的", &[], window, cx)
    });
    assert_eq!(h.text(cx), "先写的\n撤回的");
}
