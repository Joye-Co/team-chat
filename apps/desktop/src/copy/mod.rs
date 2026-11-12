//! The copy catalog: every string a user can read comes from here.
//!
//! One file per area. Each file declares its entries with [`copy!`], which makes
//! one `pub const` per entry and an `ENTRIES` table for the catalog test.
//! Placeholders look like `{name}`; fill them with [`fill`].
//!
//! Rules (docs/PRODUCT.md): Simplified Chinese; product nouns are only 成员, 群,
//! 话题 and 产出; developer words only in [`dev`]. The product name is never
//! typed here: use `{product}` and fill it with `env!("PRODUCT_NAME")`.

pub mod artifacts;
pub mod cards;
pub mod common;
pub mod composer;
pub mod connection;
pub mod dev;
pub mod files;
pub mod login;
// --- v11/media ---
pub mod media;
// --- end v11/media ---
// --- v11/inc-diagrams ---
pub mod diagrams;
// --- end v11/inc-diagrams ---
// --- inc-host track ---
pub mod page_host;
// --- end inc-host track ---
// --- inc-refs ---
pub mod refs;
// --- end inc-refs ---
// --- settings-backend track ---
pub mod settings_backend;
// --- end settings-backend track ---
pub mod nav;
pub mod notify;
pub mod panel;
pub mod process;
pub mod realtime;
pub mod search;
pub mod settings;
pub mod timeline;
pub mod update;
// v2 areas (1.1.0): one file per feature track, registered once here.
pub mod appearance;
pub mod artifact_page;
pub mod cards_v2;
// --- inc-cards21 ---
pub mod cards21;
// --- end inc-cards21 ---
pub mod admin;
pub mod desk;
pub mod group_page;
pub mod live;
pub mod members_page;
pub mod memory;
pub mod rail;
// Usable: 按顺序做 and who is at work in a 话题.
pub mod relay;
pub mod side_chat;
pub mod topic_rail;
// --- v12/kit ---
pub mod native_kit;
// --- end v12/kit ---
// --- v12/overlays ---
pub mod overlays;
// --- end v12/overlays ---
// --- r1-kit ---
pub mod kit;
// --- end r1-kit ---

/// A catalog text that differs by system: `mac` on macOS, `other` on
/// Windows and Linux. The Mac app names the Mac, the Finder, the menu bar and
/// the keychain; elsewhere the text says 电脑 and names what that system has.
/// A device that really is a Mac (a name from the server) is never rewritten.
pub const fn per_system(mac: &'static str, other: &'static str) -> &'static str {
    if cfg!(target_os = "macos") {
        mac
    } else {
        other
    }
}

/// Declares catalog entries: `copy! { KEY = "text", ... }`.
#[macro_export]
macro_rules! copy {
    ($($name:ident = $text:expr),* $(,)?) => {
        $(
            #[allow(dead_code)]
            pub const $name: &str = $text;
        )*
        /// Every entry of this section, for the catalog test.
        #[allow(dead_code)]
        pub const ENTRIES: &[(&str, &str)] = &[$((stringify!($name), $name)),*];
    };
}

/// A catalog section: its name and its entries.
pub struct Section {
    pub name: &'static str,
    pub entries: &'static [(&'static str, &'static str)],
    /// The developer view may use developer words.
    pub developer: bool,
}

/// Every section of the catalog. Add a new area file here too.
pub const SECTIONS: &[Section] = &[
    // --- inc-cards21 ---
    Section {
        name: "cards21",
        entries: cards21::ENTRIES,
        developer: false,
    },
    // --- end inc-cards21 ---
    Section {
        name: "common",
        entries: common::ENTRIES,
        developer: false,
    },
    Section {
        name: "memory",
        entries: memory::ENTRIES,
        developer: false,
    },
    Section {
        name: "nav",
        entries: nav::ENTRIES,
        developer: false,
    },
    Section {
        name: "timeline",
        entries: timeline::ENTRIES,
        developer: false,
    },
    Section {
        name: "cards",
        entries: cards::ENTRIES,
        developer: false,
    },
    Section {
        name: "composer",
        entries: composer::ENTRIES,
        developer: false,
    },
    Section {
        name: "login",
        entries: login::ENTRIES,
        developer: false,
    },
    Section {
        name: "settings",
        entries: settings::ENTRIES,
        developer: false,
    },
    Section {
        name: "panel",
        entries: panel::ENTRIES,
        developer: false,
    },
    Section {
        name: "realtime",
        entries: realtime::ENTRIES,
        developer: false,
    },
    Section {
        name: "connection",
        entries: connection::ENTRIES,
        developer: false,
    },
    Section {
        name: "notify",
        entries: notify::ENTRIES,
        developer: false,
    },
    Section {
        name: "update",
        entries: update::ENTRIES,
        developer: false,
    },
    Section {
        name: "files",
        entries: files::ENTRIES,
        developer: false,
    },
    Section {
        name: "artifacts",
        entries: artifacts::ENTRIES,
        developer: false,
    },
    Section {
        name: "search",
        entries: search::ENTRIES,
        developer: false,
    },
    Section {
        name: "process",
        entries: process::ENTRIES,
        developer: false,
    },
    Section {
        name: "dev",
        entries: dev::ENTRIES,
        developer: true,
    },
    Section {
        name: "admin",
        entries: admin::ENTRIES,
        developer: true,
    },
    Section {
        name: "desk",
        entries: desk::ENTRIES,
        developer: false,
    },
    Section {
        name: "rail",
        entries: rail::ENTRIES,
        developer: false,
    },
    Section {
        name: "topic_rail",
        entries: topic_rail::ENTRIES,
        developer: false,
    },
    Section {
        name: "relay",
        entries: relay::ENTRIES,
        developer: false,
    },
    Section {
        name: "side_chat",
        entries: side_chat::ENTRIES,
        developer: false,
    },
    Section {
        name: "live",
        entries: live::ENTRIES,
        developer: false,
    },
    Section {
        name: "group_page",
        entries: group_page::ENTRIES,
        developer: false,
    },
    Section {
        name: "members_page",
        entries: members_page::ENTRIES,
        developer: false,
    },
    Section {
        name: "artifact_page",
        entries: artifact_page::ENTRIES,
        developer: false,
    },
    Section {
        name: "appearance",
        entries: appearance::ENTRIES,
        developer: false,
    },
    Section {
        name: "cards_v2",
        entries: cards_v2::ENTRIES,
        developer: false,
    },
    // --- v11/media ---
    Section {
        name: "media",
        entries: media::ENTRIES,
        developer: false,
    },
    // --- end v11/media ---
    // --- v11/inc-diagrams ---
    Section {
        name: "diagrams",
        entries: diagrams::ENTRIES,
        developer: false,
    },
    // --- end v11/inc-diagrams ---
    // --- inc-host track ---
    Section {
        name: "page_host",
        entries: page_host::ENTRIES,
        developer: false,
    },
    // --- end inc-host track ---
    // --- inc-refs ---
    Section {
        name: "refs",
        entries: refs::ENTRIES,
        developer: false,
    },
    // --- end inc-refs ---
    // --- settings-backend track ---
    Section {
        name: "settings_backend",
        entries: settings_backend::ENTRIES,
        developer: false,
    },
    // --- end settings-backend track ---
    // --- v12/kit ---
    Section {
        name: "native_kit",
        entries: native_kit::ENTRIES,
        developer: false,
    },
    // --- end v12/kit ---
    // --- v12/overlays ---
    Section {
        name: "overlays",
        entries: overlays::ENTRIES,
        developer: false,
    },
    // --- end v12/overlays ---
    // --- r1-kit ---
    Section {
        name: "kit",
        entries: kit::ENTRIES,
        developer: false,
    },
    // --- end r1-kit ---
];

/// Replaces `{key}` placeholders: `fill(MINUTES_AGO, &[("n", "5")])`.
pub fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (key, value) in values {
        out = out.replace(&format!("{{{key}}}"), value);
    }
    out
}

/// Whether `text` has a right-to-left letter (Hebrew, Arabic, Syriac,
/// Thaana, N'Ko and their presentation forms).
fn has_rtl(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(c as u32,
            0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF)
    })
}

/// A name for joining into a line (previews, context and typing lines):
/// an RTL name is wrapped in Unicode isolates (FSI … PDI), so it cannot turn
/// the line around (break-ui CATALOG, RTL names).
pub fn isolate(name: &str) -> String {
    if has_rtl(name) {
        format!("\u{2068}{name}\u{2069}")
    } else {
        name.to_string()
    }
}

/// A line with the interface direction (LTR) as its base: a line with RTL
/// text starts with a left-to-right mark, so its `…` stays at the right end.
pub fn ltr(line: String) -> String {
    if has_rtl(&line) {
        format!("\u{200E}{line}")
    } else {
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn rtl_names_are_isolated() {
        assert_eq!(isolate("Alex"), "Alex");
        assert_eq!(isolate("نور"), "\u{2068}نور\u{2069}");
        let line = ltr(format!("{}：好", isolate("نور")));
        assert!(line.starts_with('\u{200E}'));
        assert_eq!(ltr("中文".into()), "中文");
    }

    /// Both texts of every `per_system(mac, other)` entry, read from the
    /// catalog's source: only one of the two is compiled in.
    fn per_system_texts() -> Vec<(String, String, String)> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/copy");
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&dir).expect("the catalog folder") {
            let path = entry.expect("a catalog file").path();
            if path.file_name().is_some_and(|n| n == "mod.rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("a catalog file");
            let file = path.file_name().unwrap().to_string_lossy().to_string();
            let mut rest = source.as_str();
            while let Some(at) = rest.find("per_system(") {
                rest = &rest[at + "per_system(".len()..];
                let (mac, after) = string_literal(rest).expect("per_system takes two literals");
                let (other, after) = string_literal(after).expect("per_system takes two literals");
                out.push((file.clone(), mac, other));
                rest = after;
            }
        }
        out
    }

    /// The next Rust string literal of `source` (escapes resolved) and what
    /// follows it.
    fn string_literal(source: &str) -> Option<(String, &str)> {
        let start = source.find('"')? + 1;
        let mut text = String::new();
        let mut chars = source[start..].char_indices();
        while let Some((i, c)) = chars.next() {
            match c {
                '"' => return Some((text, &source[start + i + 1..])),
                '\\' => match chars.next()?.1 {
                    'n' => text.push('\n'),
                    other => text.push(other),
                },
                c => text.push(c),
            }
        }
        None
    }

    fn is_han(c: char) -> bool {
        matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}')
    }

    /// Where `text` breaks the spacing rule of mixed Chinese and Latin text:
    /// a space between two Chinese characters, or a Chinese character right
    /// next to a Latin letter or a digit.
    fn spacing_faults(text: &str) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        let around = |i: usize| -> String {
            chars[i.saturating_sub(3)..(i + 4).min(chars.len())]
                .iter()
                .collect()
        };
        let mut faults = Vec::new();
        for i in 0..chars.len().saturating_sub(1) {
            let (a, b) = (chars[i], chars[i + 1]);
            let latin = |c: char| c.is_ascii_alphanumeric();
            if (is_han(a) && latin(b)) || (latin(a) && is_han(b)) {
                faults.push(format!("no space in 「{}」", around(i)));
            }
            if a == ' ' && i > 0 && is_han(chars[i - 1]) && is_han(b) {
                faults.push(format!(
                    "a space between Chinese characters in 「{}」",
                    around(i)
                ));
            }
        }
        faults
    }

    /// A Windows text made by swapping one word of the Mac text must still
    /// read as a sentence: 「这台 Mac 上」 becomes 「这台电脑上」, never
    /// 「这台电脑 上」, and 「钥匙串」 becomes 「 Windows 凭据管理器」 with its spaces.
    #[test]
    fn per_system_texts_keep_the_spacing_of_chinese_and_latin() {
        let texts = per_system_texts();
        assert!(
            texts.len() >= 20,
            "found only {} per_system entries",
            texts.len()
        );
        let mut faults = Vec::new();
        for (file, mac, other) in &texts {
            for (system, text) in [("mac", mac), ("other", other)] {
                for fault in spacing_faults(text) {
                    faults.push(format!("{file} ({system}) 「{text}」: {fault}"));
                }
            }
        }
        assert!(faults.is_empty(), "{}", faults.join("\n"));
    }

    #[test]
    fn the_spacing_rule_finds_both_faults() {
        assert!(spacing_faults("值会在这台电脑上加密").is_empty());
        assert!(spacing_faults("值会在这台 Mac 上加密，按 4 次").is_empty());
        assert_eq!(spacing_faults("值会在这台电脑 上加密").len(), 1);
        assert_eq!(spacing_faults("保存在Windows 凭据管理器里").len(), 1);
        assert_eq!(spacing_faults("后4 位").len(), 1);
    }

    /// Developer words from docs/PRODUCT.md: the bullet that starts with
    /// "Developer words appear only". "model names" stands for a list of model
    /// and CLI names, so those are added here.
    fn developer_words() -> (Vec<String>, Vec<String>) {
        let product = include_str!("../../../../docs/PRODUCT.md");
        let start = product
            .find("Developer words appear only")
            .expect("docs/PRODUCT.md lists developer words");
        let rest = &product[start..];
        let rest = &rest[rest.find(':').expect("word list after a colon") + 1..];
        let end = rest.find("\n- ").unwrap_or(rest.len());
        let list = rest[..end].replace('\n', " ");
        let list = list.trim().trim_end_matches('.');
        let mut ascii = Vec::new();
        let mut cjk = Vec::new();
        for item in list.split([',', '，']) {
            let item = item
                .trim()
                .trim_start_matches("and their Chinese forms")
                .trim();
            let item = item.trim_start_matches("and ").trim();
            if item.is_empty() {
                continue;
            }
            if item == "model names" {
                for model in [
                    "model", "claude", "codex", "gpt", "gemini", "opus", "sonnet", "haiku",
                ] {
                    ascii.push(model.to_string());
                }
            } else if item.is_ascii() {
                ascii.push(item.to_lowercase());
            } else {
                cjk.push(item.to_string());
            }
        }
        (ascii, cjk)
    }

    /// Developer words found in `text`: whole ASCII words (case-insensitive,
    /// plural "s" too) and Chinese forms anywhere.
    fn found_words(text: &str, ascii: &[String], cjk: &[String]) -> Vec<String> {
        let lower = text.to_lowercase();
        let words: HashSet<&str> = lower
            .split(|c: char| !c.is_ascii_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect();
        let mut found = Vec::new();
        for word in ascii {
            if words.contains(word.as_str()) || words.contains(format!("{word}s").as_str()) {
                found.push(word.clone());
            }
        }
        for word in cjk {
            if text.contains(word.as_str()) {
                found.push(word.clone());
            }
        }
        found
    }

    #[test]
    fn product_md_word_list_is_parsed() {
        let (ascii, cjk) = developer_words();
        for word in [
            "session",
            "cli",
            "token",
            "bot",
            "dm",
            "thread",
            "artifact",
            "attachment",
        ] {
            assert!(
                ascii.contains(&word.to_string()),
                "missing {word} in {ascii:?}"
            );
        }
        for word in [
            "会话",
            "机器人",
            "智能体",
            "频道",
            "线程",
            "任务",
            "工单",
            "附件",
            "令牌",
        ] {
            assert!(cjk.contains(&word.to_string()), "missing {word} in {cjk:?}");
        }
    }

    #[test]
    fn detector_finds_words() {
        let (ascii, cjk) = developer_words();
        assert_eq!(found_words("Ask the Bot", &ascii, &cjk), vec!["bot"]);
        assert_eq!(found_words("两个 tokens", &ascii, &cjk), vec!["token"]);
        assert_eq!(found_words("新建任务", &ascii, &cjk), vec!["任务"]);
        assert!(found_words("robotics 和 botany", &ascii, &cjk).is_empty());
        assert!(found_words("等你回答", &ascii, &cjk).is_empty());
    }

    #[test]
    fn no_developer_words_outside_dev_section() {
        let (ascii, cjk) = developer_words();
        let mut bad = Vec::new();
        for section in SECTIONS.iter().filter(|s| !s.developer) {
            for (key, text) in section.entries {
                let found = found_words(text, &ascii, &cjk);
                if !found.is_empty() {
                    bad.push(format!("{}::{key} = {text:?} uses {found:?}", section.name));
                }
            }
        }
        assert!(
            bad.is_empty(),
            "developer words in the catalog:\n{}",
            bad.join("\n")
        );
    }

    #[test]
    fn entries_are_not_empty_and_keys_are_unique() {
        for section in SECTIONS {
            let mut keys = HashSet::new();
            for (key, text) in section.entries {
                assert!(!text.trim().is_empty(), "{}::{key} is empty", section.name);
                assert!(keys.insert(*key), "{}::{key} twice", section.name);
            }
        }
    }

    #[test]
    fn product_name_is_never_typed() {
        let name = env!("PRODUCT_NAME").to_lowercase();
        for section in SECTIONS {
            for (key, text) in section.entries {
                assert!(
                    !text.to_lowercase().contains(&name),
                    "{}::{key} types the product name",
                    section.name
                );
            }
        }
    }

    #[test]
    fn fill_replaces_placeholders() {
        assert_eq!(fill(common::MINUTES_AGO, &[("n", "5")]), "5 分钟前");
        assert_eq!(
            fill(common::STATUS_WAITING_ANSWER_NAMED, &[("name", "小王")]),
            "等小王回答"
        );
    }
}
