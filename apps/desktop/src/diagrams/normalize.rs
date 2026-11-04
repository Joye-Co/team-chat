//! Source checks and normalization before a diagram renders (REFS §B1, §B2 step 1, §B5).
//!
//! The keyword allowlist and the forbidden-construct list are the protocol's
//! (`chat_core::proto::diagram`, from chat-protocol), the same rules the server and the SDK use; [`check_source`]
//! only maps the protocol's result to the desktop's types.

use chat_core::proto::diagram::{DIAGRAM_FORBIDDEN, DiagramCheck, check_diagram_source};

/// The diagram types 1.1.0 draws (first keyword of the source).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiagramType {
    Flowchart,
    Sequence,
    State,
    Class,
    Er,
    Gantt,
    Timeline,
}

impl DiagramType {
    /// The type name the renderer page reports (`flowchart`, `sequence`, …).
    pub fn as_str(self) -> &'static str {
        match self {
            DiagramType::Flowchart => "flowchart",
            DiagramType::Sequence => "sequence",
            DiagramType::State => "state",
            DiagramType::Class => "class",
            DiagramType::Er => "er",
            DiagramType::Gantt => "gantt",
            DiagramType::Timeline => "timeline",
        }
    }

    /// The UI word (流程图, 时序图, …).
    pub fn word(self) -> &'static str {
        use crate::copy::diagrams as c;
        match self {
            DiagramType::Flowchart => c::TYPE_FLOWCHART,
            DiagramType::Sequence => c::TYPE_SEQUENCE,
            DiagramType::State => c::TYPE_STATE,
            DiagramType::Class => c::TYPE_CLASS,
            DiagramType::Er => c::TYPE_ER,
            DiagramType::Gantt => c::TYPE_GANTT,
            DiagramType::Timeline => c::TYPE_TIMELINE,
        }
    }
}

/// Why a source is not drawn.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Refusal {
    /// Nothing but blank lines and comments.
    Empty,
    /// More than `chat_core::proto::diagram::MAX_DIAGRAM_SOURCE_BYTES`.
    TooLong,
    /// The first keyword is not in the protocol's keyword list (it is kept for the log).
    Unsupported(String),
    /// A construct of the protocol's forbidden list (or `front matter`).
    Forbidden(&'static str),
}

/// A source ready to render: the text the renderer gets and its type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Normalized {
    pub source: String,
    pub kind: DiagramType,
}

/// BOM stripped, `\r\n` and `\r` to `\n`, trailing spaces trimmed.
fn clean(source: &str) -> String {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let source = source.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = source.lines().map(str::trim_end).collect();
    let mut out = lines.join("\n");
    while out.ends_with('\n') {
        out.pop();
    }
    out
}

/// The static checks (size, front matter, forbidden constructs, first keyword): the
/// protocol's `check_diagram_source` (REFS §B1, §B10). A leading BOM is ignored.
pub fn check_source(source: &str) -> Result<DiagramType, Refusal> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    match check_diagram_source(source) {
        DiagramCheck::Ok { kind } => Ok(match kind {
            "sequenceDiagram" => DiagramType::Sequence,
            "stateDiagram" => DiagramType::State,
            "classDiagram" => DiagramType::Class,
            "erDiagram" => DiagramType::Er,
            "gantt" => DiagramType::Gantt,
            "timeline" => DiagramType::Timeline,
            _ => DiagramType::Flowchart,
        }),
        DiagramCheck::Refused { reason } => Err(refusal(&reason)),
    }
}

/// The desktop refusal of a protocol reason (`empty`, `too_large`, `front_matter`,
/// `forbidden:<text>`, `unsupported:<keyword>`).
fn refusal(reason: &str) -> Refusal {
    if let Some(text) = reason.strip_prefix("forbidden:") {
        return DIAGRAM_FORBIDDEN
            .iter()
            .find(|f| f.trim() == text)
            .map(|f| Refusal::Forbidden(f))
            .unwrap_or(Refusal::Forbidden("front matter"));
    }
    if let Some(word) = reason.strip_prefix("unsupported:") {
        return Refusal::Unsupported(word.chars().take(32).collect());
    }
    match reason {
        "too_large" => Refusal::TooLong,
        "front_matter" => Refusal::Forbidden("front matter"),
        _ => Refusal::Empty,
    }
}

/// The protocol's reason text for a refusal (`empty`, `too_large`, `front_matter`,
/// `forbidden:<text>`, `unsupported:<keyword>`).
#[cfg(test)]
pub fn refusal_reason(r: &Refusal) -> String {
    match r {
        Refusal::Empty => "empty".into(),
        Refusal::TooLong => "too_large".into(),
        Refusal::Forbidden("front matter") => "front_matter".into(),
        Refusal::Forbidden(f) => format!("forbidden:{}", f.trim()),
        Refusal::Unsupported(k) => format!("unsupported:{k}"),
    }
}

/// The protocol's type name (`flowchart`, `sequenceDiagram`, …).
#[cfg(test)]
pub fn protocol_type(t: DiagramType) -> &'static str {
    match t {
        DiagramType::Flowchart => "flowchart",
        DiagramType::Sequence => "sequenceDiagram",
        DiagramType::State => "stateDiagram",
        DiagramType::Class => "classDiagram",
        DiagramType::Er => "erDiagram",
        DiagramType::Gantt => "gantt",
        DiagramType::Timeline => "timeline",
    }
}

/// True for a line that only sets colors (removed: TeamChat owns colors). `class_assigns`:
/// in flowcharts and state diagrams `class a,b name` assigns a style class; in class
/// diagrams `class` declares a class and stays.
fn is_color_line(line: &str, class_assigns: bool) -> bool {
    let t = line.trim_start();
    let starts = |w: &str| {
        t.strip_prefix(w)
            .is_some_and(|r| r.starts_with(char::is_whitespace))
    };
    starts("style")
        || starts("classDef")
        || starts("linkStyle")
        || (class_assigns && starts("class"))
}

/// True when the first keyword names a flowchart or a state diagram (the protocol's
/// keyword rule: the first line that is not blank and not a `%%` comment).
fn assigns_classes(source: &str) -> bool {
    let keyword = source
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("%%"))
        .and_then(|l| {
            l.split(|c: char| c.is_whitespace() || c == ':' || c == ';')
                .next()
        })
        .unwrap_or("");
    matches!(
        keyword,
        "flowchart" | "graph" | "stateDiagram" | "stateDiagram-v2"
    )
}

/// The cleaned source's lines that stay, with their 1-based line numbers in the source
/// the person sees.
fn kept_lines(cleaned: &str) -> Vec<(u32, &str)> {
    let class_assigns = assigns_classes(cleaned);
    cleaned
        .lines()
        .enumerate()
        .filter(|(_, l)| !is_color_line(l, class_assigns))
        .map(|(i, l)| (i as u32 + 1, l))
        .collect()
}

/// Checks and normalizes a source (REFS §B2 step 1). Color lines go first, so they never
/// make a diagram fail: not by size, and not by a forbidden word in a style or class name
/// (`classDef callback …`).
pub fn normalize(source: &str) -> Result<Normalized, Refusal> {
    let cleaned = clean(source);
    let source = kept_lines(&cleaned)
        .into_iter()
        .map(|(_, l)| l)
        .collect::<Vec<_>>()
        .join("\n");
    let kind = check_source(&source)?;
    Ok(Normalized { source, kind })
}

/// The line of the original source that line `line` (1-based) of the normalized source
/// came from, so an error names the line the person sees.
pub fn original_line(source: &str, line: u32) -> Option<u32> {
    let cleaned = clean(source);
    kept_lines(&cleaned)
        .get(line.checked_sub(1)? as usize)
        .map(|(n, _)| *n)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shared cases of `protocol/checks/diagram-checks.json`.
    #[test]
    fn matches_the_protocol_check_cases() {
        let doc: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../protocol/checks/diagram-checks.json"
        ))
        .unwrap();
        let cases = doc["cases"].as_array().unwrap();
        assert!(cases.len() >= 20);
        for case in cases {
            let source = case["source"].as_str().unwrap();
            match check_source(source) {
                Ok(t) => {
                    assert_eq!(case["ok"], true, "{source:?}");
                    assert_eq!(case["type"].as_str(), Some(protocol_type(t)), "{source:?}");
                }
                Err(r) => {
                    assert_eq!(case["ok"], false, "{source:?}");
                    assert_eq!(
                        case["reason"].as_str(),
                        Some(refusal_reason(&r).as_str()),
                        "{source:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn detects_types() {
        let t = |s: &str| check_source(s);
        assert_eq!(t("flowchart LR\nA-->B"), Ok(DiagramType::Flowchart));
        assert_eq!(t("graph TD\nA-->B"), Ok(DiagramType::Flowchart));
        assert_eq!(
            t("%% note\n\nsequenceDiagram\nA->>B: hi"),
            Ok(DiagramType::Sequence)
        );
        assert_eq!(t("stateDiagram-v2\n[*]-->A"), Ok(DiagramType::State));
        assert_eq!(t("stateDiagram\n[*]-->A"), Ok(DiagramType::State));
        assert_eq!(t("classDiagram\nclass A"), Ok(DiagramType::Class));
        assert_eq!(t("erDiagram\nA ||--o{ B : has"), Ok(DiagramType::Er));
        assert_eq!(t("gantt\ntitle x"), Ok(DiagramType::Gantt));
        assert_eq!(t("timeline\n2026 : a"), Ok(DiagramType::Timeline));
        assert_eq!(t("pie\n\"a\" : 1"), Err(Refusal::Unsupported("pie".into())));
        assert_eq!(
            t("mindmap\n root"),
            Err(Refusal::Unsupported("mindmap".into()))
        );
        assert_eq!(t("graphTD"), Err(Refusal::Unsupported("graphTD".into())));
        assert_eq!(t("graph TD;\nA-->B"), Ok(DiagramType::Flowchart));
        assert_eq!(t("flowchart;A-->B"), Ok(DiagramType::Flowchart));
        assert_eq!(t("%% only\n\n"), Err(Refusal::Empty));
    }

    #[test]
    fn refuses_forbidden_constructs() {
        let f = |s: &str| check_source(s);
        assert_eq!(
            f("%%{init: {'theme':'dark'}}%%\ngraph TD\nA-->B"),
            Err(Refusal::Forbidden("%%{"))
        );
        assert_eq!(
            f("---\ntitle: x\n---\ngraph TD\nA-->B"),
            Err(Refusal::Forbidden("front matter"))
        );
        assert_eq!(
            f("graph TD\nA-->B\nclick A \"x\""),
            Err(Refusal::Forbidden("click "))
        );
        assert_eq!(
            f("graph TD\nA-->B\nCLICK A cb"),
            Err(Refusal::Forbidden("click "))
        );
        assert_eq!(
            f("graph TD\nA[\"<a href=x>\"]"),
            Err(Refusal::Forbidden("href"))
        );
        assert_eq!(
            f("graph TD\nA-->B\ncall foo()"),
            Err(Refusal::Forbidden("call "))
        );
        assert_eq!(
            f("graph TD\nA[JavaScript:alert]"),
            Err(Refusal::Forbidden("javascript:"))
        );
        assert_eq!(
            f("graph TD\nA[<Script>]"),
            Err(Refusal::Forbidden("<script"))
        );
        assert_eq!(
            f("graph TD\nA[<iframe>]"),
            Err(Refusal::Forbidden("<iframe"))
        );
        assert_eq!(
            f("graph TD\nA[<foreignObject>]"),
            Err(Refusal::Forbidden("<foreignobject"))
        );
        assert_eq!(
            f(&format!("graph TD\n{}", "A-->B\n".repeat(3000))),
            Err(Refusal::TooLong)
        );
    }

    #[test]
    fn removes_color_lines_and_cleans_text() {
        let n = normalize(
            "\u{feff}flowchart LR  \r\n  A-->B\r\n  style A fill:#f00\r\n  classDef hot fill:red\r\n  linkStyle 0 stroke:red\r\n  class A,B hot\r\n  B-->C   ",
        )
        .unwrap();
        assert_eq!(n.kind, DiagramType::Flowchart);
        assert_eq!(n.source, "flowchart LR\n  A-->B\n  B-->C");
        let s = normalize("stateDiagram-v2\n  [*]-->A\n  class A hot").unwrap();
        assert_eq!(s.source, "stateDiagram-v2\n  [*]-->A");
    }

    #[test]
    fn keeps_class_in_class_diagrams() {
        let n = normalize("classDiagram\n  class Topic\n  classDef x fill:red\n  Topic <|-- Group")
            .unwrap();
        assert_eq!(n.source, "classDiagram\n  class Topic\n  Topic <|-- Group");
    }

    #[test]
    fn style_lines_never_fail_a_diagram() {
        // Forbidden words only in style and class names.
        let n = normalize(
            "flowchart LR\n  A-->B\n  classDef callback fill:#f00\n  class A callback\n  style B fill:url(href)\n  linkStyle 0 stroke:red",
        )
        .unwrap();
        assert_eq!(n.source, "flowchart LR\n  A-->B");
        let s = normalize("stateDiagram-v2\n  [*]-->A\n  classDef call fill:red\n  class A call ")
            .unwrap();
        assert_eq!(s.source, "stateDiagram-v2\n  [*]-->A");
        // Style lines do not count against the size limit.
        let big = format!(
            "graph TD\n  A-->B\n{}",
            "  style A fill:#ffffff,stroke:#000000\n".repeat(600)
        );
        assert!(big.len() > chat_core::proto::diagram::MAX_DIAGRAM_SOURCE_BYTES);
        assert_eq!(normalize(&big).unwrap().source, "graph TD\n  A-->B");
        // A forbidden construct outside a style line still refuses.
        assert_eq!(
            normalize("flowchart LR\n  style A fill:red\n  click A call x()"),
            Err(Refusal::Forbidden("click "))
        );
        // In class diagrams `class` lines are kept and checked.
        assert_eq!(
            normalize("classDiagram\n  class callback"),
            Err(Refusal::Forbidden("callback"))
        );
    }

    #[test]
    fn maps_normalized_lines_to_the_source_lines() {
        let src = "flowchart LR\n  style A fill:red\n  A-->B\n  classDef x fill:red\n  B-- >C";
        assert_eq!(original_line(src, 1), Some(1));
        assert_eq!(original_line(src, 2), Some(3));
        assert_eq!(original_line(src, 3), Some(5));
        assert_eq!(original_line(src, 4), None);
        assert_eq!(original_line(src, 0), None);
    }

    #[test]
    fn style_words_inside_labels_stay() {
        let n = normalize("flowchart LR\n  A[style guide]-->B[classes]").unwrap();
        assert_eq!(n.source, "flowchart LR\n  A[style guide]-->B[classes]");
    }
}
