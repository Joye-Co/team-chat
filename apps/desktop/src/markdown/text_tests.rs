//! Tests for the text rules patched into the vendored gpui
//! (`third_party/gpui-pre`): CJK line breaking and the truncation helper.
//! GPUI's test text system gives every character the same advance, so the
//! widths below count characters.

use gpui::{Pixels, SharedString, TestAppContext, TextRun, Window, font, px};

const SIZE: Pixels = px(16.);

fn runs(text: &str) -> Vec<TextRun> {
    vec![TextRun {
        len: text.len(),
        font: font("Dummy"),
        ..Default::default()
    }]
}

/// The advance of one character.
fn em(window: &Window) -> Pixels {
    window
        .text_system()
        .shape_text("m".into(), SIZE, &runs("m"), None, None)
        .expect("shaped")[0]
        .unwrapped_layout
        .width
}

/// The text of each visual line when `text` wraps at `chars` characters.
fn wrap(window: &Window, text: &str, chars: f32) -> Vec<String> {
    let width = em(window) * chars + px(0.5);
    let lines = window
        .text_system()
        .shape_text(
            SharedString::from(text.to_string()),
            SIZE,
            &runs(text),
            Some(width),
            None,
        )
        .expect("shaped");
    let line = &lines[0];
    let mut starts = vec![0];
    for boundary in line.wrap_boundaries() {
        let run = &line.unwrapped_layout.runs[boundary.run_ix];
        starts.push(run.glyphs[boundary.glyph_ix].index);
    }
    starts.push(text.len());
    starts
        .windows(2)
        .map(|w| text[w[0]..w[1]].to_string())
        .collect()
}

/// `text` cut to `chars` characters with an ellipsis.
fn truncate(window: &Window, cx: &gpui::App, text: &str, chars: f32) -> String {
    let width = em(window) * chars + px(0.5);
    let mut wrapper = cx.text_system().line_wrapper(font("Dummy"), SIZE);
    let (cut, _) = wrapper.truncate_line(
        SharedString::from(text.to_string()),
        width,
        "…",
        &runs(text),
        gpui::TruncateFrom::End,
    );
    cut.to_string()
}

#[gpui::test]
fn no_line_starts_with_closing_or_ends_with_opening_punctuation(cx: &mut TestAppContext) {
    let vcx = cx.add_empty_window();
    vcx.update(|window, _| {
        // `，` would start line 2: the character before it goes down too.
        assert_eq!(wrap(window, "你好世界，再见", 4.), ["你好世", "界，再见"]);
        assert_eq!(wrap(window, "你好世界。再见", 4.), ["你好世", "界。再见"]);
        assert_eq!(wrap(window, "你好世界」再见", 4.), ["你好世", "界」再见"]);
        // `「` would end line 1: it goes down with the next character.
        assert_eq!(wrap(window, "你好世「界」", 4.), ["你好世", "「界」"]);
        assert_eq!(wrap(window, "你好世（界）", 4.), ["你好世", "（界）"]);
        // Plain Han text still breaks between any two characters.
        assert_eq!(wrap(window, "你好世界再见", 4.), ["你好世界", "再见"]);
    });
}

#[gpui::test]
fn truncation_trims_punctuation_and_short_word_cuts(cx: &mut TestAppContext) {
    let vcx = cx.add_empty_window();
    vcx.update(|window, cx| {
        // Fits: unchanged.
        assert_eq!(
            truncate(window, cx, "周五 demo 安排", 20.),
            "周五 demo 安排"
        );
        // A cut that leaves `5` of `5xx` drops the fragment.
        assert_eq!(
            truncate(window, cx, "合并重复的 5xx 告警", 8.),
            "合并重复的…"
        );
        // A Latin word cut to four or more characters stays.
        let long = "Composer: paste images from the clipboard";
        assert_eq!(truncate(window, cx, long, 22.), "Composer: paste image…");
        assert_eq!(truncate(window, cx, long, 20.), "Composer: paste…");
        // Punctuation and an opening bracket before the ellipsis go.
        assert_eq!(
            truncate(window, cx, "按你上次喜欢的口吻写的。确认", 13.),
            "按你上次喜欢的口吻写的…"
        );
        assert_eq!(
            truncate(window, cx, "Joye Huang（审核）", 12.),
            "Joye Huang…"
        );
        // A cut version number goes as a whole.
        assert_eq!(
            truncate(window, cx, "生产环境 v0.9.2 发布", 10.),
            "生产环境…"
        );
    });
}
