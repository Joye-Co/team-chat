//! Source checks and normalization before a diagram renders (REFS §B1, §B2 step 1, §B5).
//!
//! The keyword allowlist and the forbidden-construct list sit behind one function,
//! [`check_source`]. The same lists are planned for `crates/chat-protocol/src/diagram.rs`
//! (branch `v11/inc-protocol`, with `diagram-checks.json` fixtures); when that file lands,
//! the integration switches this function to it.

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

/// Supported first keywords, matched as a whole word.
pub const DIAGRAM_KEYWORDS: &[(&str, DiagramType)] = &[
    ("flowchart", DiagramType::Flowchart),
    ("graph", DiagramType::Flowchart),
    ("sequenceDiagram", DiagramType::Sequence),
    ("stateDiagram-v2", DiagramType::State),
    ("stateDiagram", DiagramType::State),
    ("classDiagram", DiagramType::Class),
    ("erDiagram", DiagramType::Er),
    ("gantt", DiagramType::Gantt),
    ("timeline", DiagramType::Timeline),
];

/// Constructs a source must not have (matched case-insensitively anywhere). A front matter
/// block (`---` as the first line) is refused separately.
pub const DIAGRAM_FORBIDDEN: &[&str] = &[
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

/// Largest source in UTF-8 bytes.
pub const MAX_SOURCE_BYTES: usize = 16 * 1024;

/// Why a source is not drawn.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Refusal {
    /// Nothing but blank lines and comments.
    Empty,
    /// More than [`MAX_SOURCE_BYTES`].
    TooLong,
    /// The first keyword is not in [`DIAGRAM_KEYWORDS`] (it is kept for the log).
    Unsupported(String),
    /// A construct of [`DIAGRAM_FORBIDDEN`] (or `front matter`).
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

/// The static checks (size, front matter, forbidden constructs, first keyword). Shared rule
/// with the server and the SDK (REFS §B1, §B10).
pub fn check_source(source: &str) -> Result<DiagramType, Refusal> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(Refusal::TooLong);
    }
    let source = clean(source);
    if source.lines().next().is_some_and(|l| l.trim() == "---") {
        return Err(Refusal::Forbidden("front matter"));
    }
    let lower = source.to_lowercase();
    if let Some(bad) = DIAGRAM_FORBIDDEN.iter().find(|b| lower.contains(*b)) {
        return Err(Refusal::Forbidden(bad));
    }
    let first = source
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("%%"))
        .ok_or(Refusal::Empty)?;
    let word = first.split_whitespace().next().unwrap_or("");
    DIAGRAM_KEYWORDS
        .iter()
        .find(|(k, _)| *k == word)
        .map(|(_, t)| *t)
        .ok_or_else(|| Refusal::Unsupported(word.chars().take(32).collect()))
}

/// True for a line that only sets colors (removed: TeamChat owns colors).
fn is_color_line(line: &str, kind: DiagramType) -> bool {
    let t = line.trim_start();
    let starts = |w: &str| {
        t.strip_prefix(w)
            .is_some_and(|r| r.starts_with(char::is_whitespace))
    };
    if starts("style") || starts("classDef") || starts("linkStyle") {
        return true;
    }
    // `class a,b name` assigns a style class in flowcharts and state diagrams; in class
    // diagrams `class` declares a class and stays.
    matches!(kind, DiagramType::Flowchart | DiagramType::State) && starts("class")
}

/// Checks and normalizes a source (REFS §B2 step 1).
pub fn normalize(source: &str) -> Result<Normalized, Refusal> {
    let kind = check_source(source)?;
    let cleaned = clean(source);
    let source = cleaned
        .lines()
        .filter(|l| !is_color_line(l, kind))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Normalized { source, kind })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_types() {
        let t = |s: &str| check_source(s);
        assert_eq!(t("flowchart LR\nA-->B"), Ok(DiagramType::Flowchart));
        assert_eq!(t("graph TD\nA-->B"), Ok(DiagramType::Flowchart));
        assert_eq!(t("%% note\n\nsequenceDiagram\nA->>B: hi"), Ok(DiagramType::Sequence));
        assert_eq!(t("stateDiagram-v2\n[*]-->A"), Ok(DiagramType::State));
        assert_eq!(t("stateDiagram\n[*]-->A"), Ok(DiagramType::State));
        assert_eq!(t("classDiagram\nclass A"), Ok(DiagramType::Class));
        assert_eq!(t("erDiagram\nA ||--o{ B : has"), Ok(DiagramType::Er));
        assert_eq!(t("gantt\ntitle x"), Ok(DiagramType::Gantt));
        assert_eq!(t("timeline\n2026 : a"), Ok(DiagramType::Timeline));
        assert_eq!(t("pie\n\"a\" : 1"), Err(Refusal::Unsupported("pie".into())));
        assert_eq!(t("mindmap\n root"), Err(Refusal::Unsupported("mindmap".into())));
        assert_eq!(t("graphTD"), Err(Refusal::Unsupported("graphTD".into())));
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
        assert_eq!(f("graph TD\nA-->B\nclick A \"x\""), Err(Refusal::Forbidden("click ")));
        assert_eq!(f("graph TD\nA-->B\nCLICK A cb"), Err(Refusal::Forbidden("click ")));
        assert_eq!(f("graph TD\nA[\"<a href=x>\"]"), Err(Refusal::Forbidden("href")));
        assert_eq!(f("graph TD\nA-->B\ncall foo()"), Err(Refusal::Forbidden("call ")));
        assert_eq!(f("graph TD\nA[JavaScript:alert]"), Err(Refusal::Forbidden("javascript:")));
        assert_eq!(f("graph TD\nA[<Script>]"), Err(Refusal::Forbidden("<script")));
        assert_eq!(f("graph TD\nA[<iframe>]"), Err(Refusal::Forbidden("<iframe")));
        assert_eq!(f("graph TD\nA[<foreignObject>]"), Err(Refusal::Forbidden("<foreignobject")));
        assert_eq!(f(&format!("graph TD\n{}", "A-->B\n".repeat(3000))), Err(Refusal::TooLong));
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
    fn style_words_inside_labels_stay() {
        let n = normalize("flowchart LR\n  A[style guide]-->B[classes]").unwrap();
        assert_eq!(n.source, "flowchart LR\n  A[style guide]-->B[classes]");
    }
}
