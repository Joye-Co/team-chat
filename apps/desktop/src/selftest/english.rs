//! `--lang en` for the screenshot self-test: draws every scene in English for the English
//! homepage. The app's copy stays Simplified Chinese (docs/PRODUCT.md); this only swaps the
//! text at the moment gpui shapes it, through `gpui::set_text_translator`, one text run at a
//! time.
//!
//! The table is `english.txt`: one `中文<TAB>English` pair per line, `#` starts a comment.
//! `{}` in a source matches any text (shortest first) and fills the same `{}` in order in the
//! English; each filled part is looked up again. A run whose text has Chinese and no entry is
//! listed in `<dir>/untranslated.txt` when the self-test ends, so the table can be completed.

use std::{
    collections::{BTreeSet, HashMap},
    path::Path,
    sync::{Mutex, OnceLock},
};

const TABLE: &str = include_str!("english.txt");

struct Table {
    exact: HashMap<&'static str, &'static str>,
    /// Sources with `{}`, split at it, longest first so the most specific wins.
    patterns: Vec<(Vec<&'static str>, &'static str)>,
}

static STATE: OnceLock<Table> = OnceLock::new();
static MISSES: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
/// Whole texts already drawn, so a cut copy of one finds its full text.
static SEEN: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

fn table() -> &'static Table {
    STATE.get_or_init(|| {
        let mut exact = HashMap::new();
        let mut patterns = Vec::new();
        for line in TABLE.lines() {
            if line.trim_start().starts_with('#') {
                continue;
            }
            let Some((zh, en)) = line.split_once('\t') else {
                continue;
            };
            let en = en.trim_start_matches('\t');
            if zh.contains("{}") {
                patterns.push((zh.split("{}").collect::<Vec<_>>(), en));
            } else {
                exact.insert(zh, en);
            }
        }
        patterns.sort_by_key(|(parts, _)| std::cmp::Reverse(parts.iter().map(|p| p.len()).sum::<usize>()));
        Table { exact, patterns }
    })
}

fn has_chinese(text: &str) -> bool {
    text.chars().any(|c| ('\u{3000}'..='\u{9fff}').contains(&c) || ('\u{ff00}'..='\u{ffef}').contains(&c))
}

/// Matches `text` against a source split at `{}`, giving the filled parts.
fn capture<'t>(parts: &[&str], text: &'t str) -> Option<Vec<&'t str>> {
    let (first, rest) = parts.split_first()?;
    let mut tail = text.strip_prefix(first)?;
    let mut caught = Vec::new();
    for (i, part) in rest.iter().enumerate() {
        let last = i + 1 == rest.len();
        let at = if last {
            if !tail.ends_with(part) {
                return None;
            }
            tail.len() - part.len()
        } else if part.is_empty() {
            return None;
        } else {
            let skip = tail.chars().next()?.len_utf8();
            tail[skip..].find(part)? + skip
        };
        if at == 0 {
            return None;
        }
        // A filled part never spans a " · " separator: those parts are translated one by one.
        if tail[..at].contains(" · ") {
            return None;
        }
        caught.push(&tail[..at]);
        tail = &tail[at + part.len()..];
    }
    Some(caught)
}

fn lookup(text: &str) -> Option<String> {
    let t = table();
    if let Some(en) = t.exact.get(text) {
        return Some((*en).to_string());
    }
    for (parts, en) in &t.patterns {
        if let Some(caught) = capture(parts, text) {
            let mut out = String::new();
            let mut pieces = en.split("{}");
            out.push_str(pieces.next().unwrap_or_default());
            for (piece, fill) in pieces.zip(caught.iter()) {
                out.push_str(&translate(fill).unwrap_or_else(|| (*fill).to_string()));
                out.push_str(piece);
            }
            return Some(out);
        }
    }
    None
}

fn translate(text: &str) -> Option<String> {
    if !has_chinese(text) {
        return None;
    }
    let core = text.trim();
    let lead = &text[..text.len() - text.trim_start().len()];
    let trail = &text[text.trim_end().len()..];
    if !core.ends_with('…') {
        SEEN.lock().unwrap().insert(core.to_string());
    }
    if let Some(en) = lookup(core) {
        return Some(format!("{lead}{en}{trail}"));
    }
    // Text of several lines: each line on its own.
    if core.contains('\n') {
        let lines: Vec<String> = text.split('\n').map(|l| translate(l).unwrap_or_else(|| l.to_string())).collect();
        return Some(lines.join("\n"));
    }
    // A leading separator: "· 今天 09:12".
    if let Some(rest) = core.strip_prefix("· ") {
        if let Some(en) = translate(rest) {
            return Some(format!("{lead}· {en}{trail}"));
        }
        return None;
    }
    if let Some(rest) = core.strip_suffix(" ·") {
        if let Some(en) = translate(rest) {
            return Some(format!("{lead}{en} ·{trail}"));
        }
        return None;
    }
    // Bidi isolates around names and titles: translate what is inside each.
    if core.contains('\u{2068}') {
        let plain: String = core.chars().filter(|c| *c != '\u{2068}' && *c != '\u{2069}').collect();
        if let Some(en) = translate(&plain) {
            return Some(format!("{lead}{en}{trail}"));
        }
        return None;
    }
    // "A · B · C": each part on its own.
    if core.contains(" · ") {
        let parts: Vec<&str> = core.split(" · ").collect();
        let done: Vec<Option<String>> = parts.iter().map(|p| if has_chinese(p) { translate(p) } else { Some((*p).to_string()) }).collect();
        if done.iter().all(Option::is_some) {
            let en: Vec<String> = done.into_iter().map(Option::unwrap).collect();
            return Some(format!("{lead}{}{trail}", en.join(" · ")));
        }
        return None;
    }
    // Cut with "…" by the app's own truncation, measured on the Chinese: translate the shortest
    // entry it is the start of and cut the English at the same share of its length.
    if let Some(prefix) = core.strip_suffix('…').filter(|p| !p.is_empty()) {
        if let Some(full) = full_for_prefix(prefix) {
            if let Some(en) = translate(&full) {
                let share = prefix.chars().count() as f32 / full.chars().count() as f32;
                let keep = ((en.chars().count() as f32) * share).round().max(1.) as usize;
                let cut: String = en.chars().take(keep).collect();
                return Some(format!("{lead}{}…{trail}", cut.trim_end()));
            }
        }
        if let Some(en) = lookup(prefix) {
            return Some(format!("{lead}{en}…{trail}"));
        }
    }
    MISSES.lock().unwrap().insert(core.to_string());
    None
}

/// The shortest text this run's text was cut from, among the entries and the texts seen whole.
fn full_for_prefix(prefix: &str) -> Option<String> {
    let t = table();
    let from_table = t.exact.keys().filter(|k| k.starts_with(prefix) && k.len() > prefix.len()).map(|k| k.to_string());
    let seen = SEEN.lock().unwrap();
    let from_seen = seen.iter().filter(|k| k.starts_with(prefix) && k.len() > prefix.len()).cloned().collect::<Vec<_>>();
    from_table.chain(from_seen).min_by_key(|k| k.len())
}

/// Whether `--lang en` is on: sample data that is not drawn by gpui (the report page) picks
/// its English copy.
pub fn on() -> bool {
    STATE.get().is_some()
}

/// Turns the translation on for the rest of the process.
pub fn install() {
    table();
    gpui::set_text_translator(translate);
}

/// Writes the runs that had Chinese and no entry, one per line, or removes the file when
/// there were none.
pub fn write_misses(dir: &Path) {
    let path = dir.join("untranslated.txt");
    let misses = MISSES.lock().unwrap();
    if misses.is_empty() {
        let _ = std::fs::remove_file(path);
        return;
    }
    let text: String = misses.iter().map(|m| format!("{}\n", m.replace('\n', "\\n"))).collect();
    if let Err(e) = std::fs::write(&path, text) {
        eprintln!("could not write {}: {e}", path.display());
    } else {
        eprintln!("{} untranslated runs in {}", misses.len(), path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_source_is_listed_once() {
        let mut seen = std::collections::HashSet::new();
        for line in TABLE.lines().filter(|l| !l.trim_start().starts_with('#')) {
            if let Some((zh, _)) = line.split_once('\t') {
                assert!(seen.insert(zh), "listed twice in english.txt: {zh}");
            }
        }
    }

    #[test]
    fn fills_patterns_and_parts() {
        assert_eq!(translate("已用 55 分钟").as_deref(), Some("55 min elapsed"));
        assert_eq!(translate("产出「重连测试报告」v2").as_deref(), Some("Artifact “Reconnect test report” v2"));
        assert_eq!(translate("Tata · 写第二版需求 · 已用 55 分钟").as_deref(), Some("Tata · Writing spec v2 · 55 min elapsed"));
        assert_eq!(translate("缺 李雪").as_deref(), Some("Lisa can't come"));
        assert_eq!(translate("Forge"), None);
    }
}
