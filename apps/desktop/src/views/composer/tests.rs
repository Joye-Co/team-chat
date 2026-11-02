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
    cx.simulate_input("请@小");
    let names = |h: &Harness, cx: &mut VisualTestContext| -> Vec<String> {
        h.view.update(cx, |this, _| {
            this.mention
                .as_ref()
                .map(|p| p.results.iter().map(|m| m.name.clone()).collect())
                .unwrap_or_default()
        })
    };
    assert_eq!(names(&h, cx), vec!["小王", "小码"]);
    cx.simulate_keystrokes("down");
    cx.simulate_keystrokes("enter");
    assert_eq!(h.text(cx), "请@小码 ");
    assert!(names(&h, cx).is_empty(), "list closes after a pick");
    cx.simulate_input("看下");
    // Esc closes an open list without touching the text.
    cx.simulate_input(" @Alex");
    assert_eq!(names(&h, cx), vec!["Alex Chen"]);
    cx.simulate_keystrokes("escape");
    assert!(names(&h, cx).is_empty());
    cx.simulate_keystrokes("enter");
    let sent = h.last(TOPIC);
    assert_eq!(sent.body, "请@小码 看下 @Alex");
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
