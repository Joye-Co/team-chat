//! Static checks of a Mermaid diagram source (docs/protocol.md, "Diagrams"), the same rules
//! as `checkDiagramSource` in packages/protocol. Both read protocol/checks/diagram-checks.json
//! in their tests. No rendering here.

/// Max UTF-8 bytes of a diagram source.
pub const MAX_DIAGRAM_SOURCE_BYTES: usize = 16_384;

/// First keywords of the diagram types 1.1.0 draws, and the type each one names.
pub const DIAGRAM_KEYWORDS: [(&str, &str); 9] = [
    ("flowchart", "flowchart"),
    ("graph", "flowchart"),
    ("sequenceDiagram", "sequenceDiagram"),
    ("stateDiagram", "stateDiagram"),
    ("stateDiagram-v2", "stateDiagram"),
    ("classDiagram", "classDiagram"),
    ("erDiagram", "erDiagram"),
    ("gantt", "gantt"),
    ("timeline", "timeline"),
];

/// Text a source must not have (compared without case): directives, links, script and HTML.
pub const DIAGRAM_FORBIDDEN: [&str; 9] = [
    "%%{",
    "click ",
    "href",
    "callback",
    "call ",
    "javascript:",
    "<script",
    "<iframe",
    "<foreignobject",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagramCheck {
    Ok {
        kind: &'static str,
    },
    /// `empty`, `too_large`, `front_matter`, `forbidden:<text>` or `unsupported:<keyword>`.
    Refused {
        reason: String,
    },
}

/// Checks a diagram source: size, no front matter, none of [`DIAGRAM_FORBIDDEN`], and a first
/// non-comment line that starts with a keyword of [`DIAGRAM_KEYWORDS`].
pub fn check_diagram_source(source: &str) -> DiagramCheck {
    let refuse = |r: String| DiagramCheck::Refused { reason: r };
    if source.len() > MAX_DIAGRAM_SOURCE_BYTES {
        return refuse("too_large".into());
    }
    let lines: Vec<&str> = source
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    let Some(first) = lines.iter().find(|l| !l.trim().is_empty()) else {
        return refuse("empty".into());
    };
    if first.trim() == "---" {
        return refuse("front_matter".into());
    }
    let lower = source.to_lowercase();
    if let Some(f) = DIAGRAM_FORBIDDEN.iter().find(|f| lower.contains(*f)) {
        return refuse(format!("forbidden:{}", f.trim()));
    }
    let Some(head) = lines
        .iter()
        .find(|l| !l.trim().is_empty() && !l.trim().starts_with("%%"))
    else {
        return refuse("empty".into());
    };
    let keyword = head
        .trim()
        .split(|c: char| c.is_whitespace() || c == ':' || c == ';')
        .next()
        .unwrap_or("");
    match DIAGRAM_KEYWORDS.iter().find(|(k, _)| *k == keyword) {
        Some((_, kind)) => DiagramCheck::Ok { kind },
        None => refuse(format!("unsupported:{keyword}")),
    }
}
