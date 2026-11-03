//! Chip labels (REFS §A2.2): `{source} · {block} · {part}`, each segment cut with `…` at its
//! limit, the whole label cut last, at most 120 code points.

use std::{cell::RefCell, collections::HashMap};

/// Most code points of each segment and of the whole label.
pub const SOURCE_MAX: usize = 24;
pub const BLOCK_MAX: usize = 16;
pub const PART_MAX: usize = 60;
pub const LABEL_MAX: usize = 120;

/// Joins the segments.
pub const SEP: &str = " · ";

/// A label and how many of its code points are the quiet lead (source and block); the rest
/// is the part, which the chip draws strong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub text: String,
    pub lead_chars: usize,
}

/// One line of plain text: control characters and line breaks become spaces, runs of spaces
/// one space, then cut to `max` code points with `…` as the last one.
pub fn cut(text: &str, max: usize) -> String {
    let mut clean = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        let c = if c.is_control() || c == '\u{2028}' || c == '\u{2029}' {
            ' '
        } else {
            c
        };
        if c == ' ' {
            if !space && !clean.is_empty() {
                clean.push(' ');
            }
            space = true;
        } else {
            clean.push(c);
            space = false;
        }
    }
    let clean = clean.trim_end();
    if clean.chars().count() <= max {
        return clean.to_string();
    }
    let mut out: String = clean.chars().take(max.saturating_sub(1)).collect();
    out = out.trim_end().to_string();
    out.push('…');
    out
}

thread_local! {
    /// The lead of labels built here, so a chip made from a [`super::PartTarget`] (which
    /// carries only the text) can draw its strong part.
    static LEADS: RefCell<HashMap<String, usize>> = RefCell::new(HashMap::new());
}

/// Builds the label. `block` is `None` when it is omitted (one block of its type and no
/// title); `part` is `None` for a whole block.
pub fn build(source: &str, block: Option<&str>, part: Option<&str>) -> Label {
    let mut segs: Vec<String> = Vec::new();
    let source = cut(source, SOURCE_MAX);
    if !source.is_empty() {
        segs.push(source);
    }
    if let Some(b) = block.map(|b| cut(b, BLOCK_MAX)).filter(|b| !b.is_empty()) {
        segs.push(b);
    }
    if let Some(p) = part.map(|p| cut(p, PART_MAX)).filter(|p| !p.is_empty()) {
        segs.push(p);
    }
    let last = segs.len().saturating_sub(1);
    let lead: usize = segs[..last]
        .iter()
        .map(|s| s.chars().count() + SEP.chars().count())
        .sum();
    let text = cut(&segs.join(SEP), LABEL_MAX);
    let lead = lead.min(text.chars().count());
    LEADS.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() > 256 {
            m.clear();
        }
        m.insert(text.clone(), lead);
    });
    Label {
        text,
        lead_chars: lead,
    }
}

/// The lead of a label: the one [`build`] made, else everything before the last ` · `.
pub fn lead_of(label: &str) -> usize {
    if let Some(n) = LEADS.with(|m| m.borrow().get(label).copied()) {
        return n;
    }
    match label.rfind(SEP) {
        Some(at) => label[..at + SEP.len()].chars().count(),
        None => 0,
    }
}

/// Splits a label at its lead.
pub fn split(label: &str, lead_chars: usize) -> (&str, &str) {
    let at = label
        .char_indices()
        .nth(lead_chars)
        .map_or(label.len(), |(i, _)| i);
    label.split_at(at)
}
